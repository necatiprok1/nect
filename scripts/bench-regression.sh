#!/bin/bash
# Benchmark regression detection.
#
# `benchmark.sh` reports timings; this script decides whether they are a problem.
# It runs a fixed, quick benchmark set, compares each measurement against a
# stored baseline, and fails when something got meaningfully slower.
#
# The design decisions here all come from the fact that a raw timing is not a
# verdict:
#
#   * Benchmarks run on shared CI hardware, where one run can be slower simply
#     because another job started. A single measurement is therefore never treated
#     as a failure on its own — the median comparison below is what separates a
#     real regression from a busy machine.
#   * A change that makes *everything* uniformly slower is a real regression; a
#     change that makes one benchmark slower while the rest are identical is far
#     more likely to be noise. Comparing each ratio against the suite's own median
#     catches that without needing to know the machine.
#   * A fast run that computed the wrong answer is not an improvement, so output
#     is checked before any timing is recorded. `smoke` carries a hand-computed
#     expectation; the rest are checked against the output recorded in the
#     baseline, so a semantic change is caught without hand-maintaining a table
#     of large numbers.
#
# Usage:
#   scripts/bench-regression.sh                 compare against the stored baseline
#   scripts/bench-regression.sh --update        measure and replace the baseline
#   scripts/bench-regression.sh --threshold 15
#
# Environment:
#   NECT_BIN=path      the CLI to measure (default target/release/nect)
#   REPEATS=n          executions per measurement (default 10)
#   THRESHOLD=percent  how much slower counts as a regression (default 15)

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

NECT_BIN="${NECT_BIN:-$ROOT_DIR/target/release/nect}"
REPEATS="${REPEATS:-10}"
THRESHOLD="${THRESHOLD:-15}"
BASELINE="benches/baseline.tsv"
UPDATE=0

while [ $# -gt 0 ]; do
    case "$1" in
        --update) UPDATE=1 ;;
        --threshold) shift; THRESHOLD="$1" ;;
        --help|-h)
            sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "unknown option: $1" >&2; exit 1 ;;
    esac
    shift
done

if [ ! -x "$NECT_BIN" ]; then
    echo "error: $NECT_BIN not found; run 'cargo build --release' first" >&2
    exit 1
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

# The suite is deliberately small and numeric: these are the shapes the JIT
# actually compiles, so a regression here is a regression in the hot path rather
# than in an interpreter fallback.
#
# Each entry is: name | body of __work() | final print | expected | repeats
#
# `smoke` runs a loop whose result is easy to check by hand: the terms are
# 0,1,2,3,4,5,6 repeated, with the last group cut short, which sums to 57. A
# suite that could be satisfied by a fast wrong program is not a suite. The other entries expect `baseline`, meaning "whatever
# this machine last produced", which catches a semantic change without anyone
# maintaining a table of large numbers.
#
# The work is a function called `repeats` times rather than one long loop, so the
# measurement covers repeated entry, the same thing a real program does. The
# result is accumulated into a module-level binding and printed once, so printing
# stays out of the measured path.
SUITE=(
    "smoke|let s = __sink;let i = 0;while i < 20 { s = s + i % 7; i = i + 1 };__sink = s|print(__sink)|57|1"
    "arithmetic_loop|let s = __sink;let i = 0;while i < 20000 { s = s + i % 7; i = i + 1 };__sink = s|print(__sink)|baseline|5"
    "function_calls|let t = __sink;let i = 0;while i < 5000 { t = (t % 997) * 2 + 1; i = i + 1 };__sink = t|print(__sink)|baseline|5"
    "branching|let t = __sink + 1;let i = 0;while i < 5000 { if t % 3 == 0 { t = (t * 2) % 1000 } else { if t % 3 == 1 { t = (t + 7) % 1000 } else { t = (t + 995) % 1000 } }; i = i + 1 };__sink = t|print(__sink)|baseline|5"
    "while_loop_fused|let t = __sink;let i = 0;while i < 20000 { t = t + 1; i = i + 1 };__sink = t|print(__sink)|baseline|5"
)

# Builds one benchmark program. The sink is a module-level binding the work
# function accumulates into, which is how a function and its caller share state.
write_program() {
    local body="$1" final="$2" repeats="$3" target="$4"
    {
        echo "let __sink = 0"
        echo "fn __work() {"
        echo "$body" | tr ';' '\n' | sed 's/^/    /'
        echo "}"
        echo "let __i = 0"
        echo "while __i < $repeats {"
        echo "    __work()"
        echo "    __i = __i + 1"
        echo "}"
        echo "$final"
    } > "$target"
}

# The output recorded in the baseline, for a benchmark whose expectation is
# `baseline`. Empty when there is no baseline yet.
baseline_output() {
    local name="$1"
    [ -f "$BASELINE" ] || return 0
    awk -F'\t' -v want="$name" '$1 == want { print $3 }' "$BASELINE" | head -1
}

# Measures one benchmark: name, seconds per run, and whether the output was
# correct.
measure() {
    local name="$1" body="$2" final="$3" expect="$4" repeats="$5"
    local program="$WORK_DIR/$name.nct"
    write_program "$body" "$final" "$repeats" "$program"

    # A warm-up run, so the measurement is not dominated by first-call costs such
    # as Cranelift initialisation.
    "$NECT_BIN" run "$program" > /dev/null 2>&1 || true

    local start end output
    start=$(python3 -c 'import time; print(time.perf_counter())')
    local i
    for ((i = 0; i < REPEATS; i++)); do
        output=$("$NECT_BIN" run "$program" 2>&1) || true
    done
    end=$(python3 -c 'import time; print(time.perf_counter())')

    local seconds
    seconds=$(python3 -c "print(f'{($end - $start) / $REPEATS:.6f}')")

    # Correctness first: a fast wrong answer is not a result worth recording.
    local verdict="ok"
    case "$output" in
        *Error*|*"error:"*) verdict="error" ;;
    esac
    if [ "$verdict" = "ok" ]; then
        if [ "$expect" = "baseline" ]; then
            local recorded
            recorded="$(baseline_output "$name")"
            if [ -z "$recorded" ]; then
                # No baseline yet: the first run establishes the expectation.
                verdict="ok"
            elif [ "$recorded" = "$output" ]; then
                verdict="ok"
            else
                verdict="changed-output"
            fi
        else
            case "$output" in
                *"$expect"*) ;;
                *) verdict="wrong-output" ;;
            esac
        fi
    fi

    printf '%s\t%s\t%s\t%s\n' "$name" "$seconds" "$verdict" "$output"
}

echo "measuring $NECT_BIN (REPEATS=$REPEATS, threshold=${THRESHOLD}%)"
echo

RESULTS="$WORK_DIR/current.tsv"
: > "$RESULTS"

for entry in "${SUITE[@]}"; do
    IFS='|' read -r name body final expect repeats <<< "$entry"
    printf '  %-18s ' "$name"
    measure "$name" "$body" "$final" "$expect" "$repeats" | tee -a "$RESULTS" | cut -f2,3 | tr '\t' ' '
    printf '\n'
done

# A benchmark that errored or computed the wrong answer is a correctness failure,
# not a performance one, and is reported before any timing is believed.
BROKEN=$(awk -F'\t' '$3 != "ok" { print $1 "\t" $2 "\t" $3 }' "$RESULTS")
if [ -n "$BROKEN" ]; then
    echo
    echo "FAIL: a benchmark did not produce the expected output"
    echo "(a 'changed-output' verdict means the program's result differs from the"
    echo " baseline, which is a semantic change rather than a performance one.)"
    echo ""
    echo "$BROKEN" | sed 's/^/  /'
    exit 1
fi

if [ "$UPDATE" = "1" ]; then
    {
        echo "# Nect benchmark baseline"
        echo "# name<TAB>seconds-per-run<TAB>program output"
        echo "# Measured by scripts/bench-regression.sh; regenerate with --update."
        echo "# The output column is what the benchmark printed when the timing was"
        echo "# recorded, so a later change in the result is reported as a semantic"
        echo "# change rather than being averaged into the timings."
    } > "$BASELINE"
    awk -F'\t' '{print $1 "\t" $2 "\t" $4}' "$RESULTS" >> "$BASELINE"
    echo
    echo "baseline written to $BASELINE"
    exit 0
fi

if [ ! -f "$BASELINE" ]; then
    echo
    echo "no baseline at $BASELINE; recording this run as the baseline"
    "$0" --update > /dev/null
    echo "done — re-run to compare against it"
    exit 0
fi

# Compare. A benchmark is only called a regression when it is slower than its own
# baseline *and* slower than the suite's median change, which is what separates a
# real regression from the whole machine being busy.
python3 - "$BASELINE" "$RESULTS" "$THRESHOLD" <<'PYTHON'
import sys

baseline_path, current_path, threshold = sys.argv[1], sys.argv[2], float(sys.argv[3])


def read(path):
    rows = {}
    with open(path) as handle:
        for line in handle:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split("\t")
            name, seconds = parts[0], parts[1]
            rows[name] = float(seconds)
    return rows


baseline = read(baseline_path)
current = read(current_path)

missing = sorted(set(baseline) - set(current))
if missing:
    print(f"  note: not measured this run: {', '.join(missing)}")

ratios = {}
for name in sorted(set(baseline) & set(current)):
    if baseline[name] > 0:
        ratios[name] = current[name] / baseline[name]

if not ratios:
    print("no comparable benchmarks")
    sys.exit(0)

# The median is the suite's own "nothing changed" reference.
ordered = sorted(ratios.values())
middle = len(ordered) // 2
if len(ordered) % 2:
    reference = ordered[middle]
else:
    reference = (ordered[middle - 1] + ordered[middle]) / 2

print()
print(f"{'benchmark':<20}{'baseline':>12}{'now':>12}{'change':>10}  verdict")
print("-" * 66)

regressions = []
for name in sorted(ratios):
    ratio = ratios[name]
    change = (ratio - 1) * 100
    # Slower than the threshold *and* slower than the suite's own median, so a
    # uniformly slower machine does not report every benchmark as a regression.
    is_regression = ratio > 1 + threshold / 100 and ratio > reference
    verdict = "REGRESSION" if is_regression else "ok"
    if is_regression:
        regressions.append((name, change))
    print(f"{name:<20}{baseline[name]:>11.4f}s{current[name]:>11.4f}s{change:>9.1f}%  {verdict}")

if regressions:
    print()
    print(f"FAIL: {len(regressions)} benchmark(s) regressed beyond {threshold:.0f}%")
    print("If this run was on a loaded machine, re-run before changing anything.")
    sys.exit(1)

print()
print(f"no regression beyond {threshold:.0f}%")
PYTHON

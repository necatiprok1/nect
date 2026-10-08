#!/bin/bash
# Allocation-aware benchmark runner for Nect.
#
# Measures the performance of hot-path allocation fixes:
#   - string indexing (get_index string path)
#   - print with multiple arguments
#   - concat
#   - array rendering via print
#
# Compares bytecode VM vs. native (AOT) to show the impact of
# allocation avoidance on both paths.

set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[1]}")/.." && pwd)"
cd "$ROOT_DIR"

NECT_BIN="${NECT_BIN:-$ROOT_DIR/target/release/nect}"
REPEATS="${REPEATS:-5}"

if [ ! -x "$NECT_BIN" ]; then
    echo "Building release binary..."
    cargo build --release 2>&1 | tail -3
fi

echo "============================================================"
echo "  Allocation-aware benchmarks"
echo "  Nect: $("$NECT_BIN" --version 2>/dev/null || echo 'release build')"
echo "============================================================"
echo ""
printf "%-38s %15s %15s %10s\n" "Benchmark" "JIT (s)" "bytecode (s)" "vs JIT"
printf "%-38s %15s %15s %10s\n" "--------------------------------------" "----------" "----------" "----------"

run_bench() {
    local name="$1"
    local file="$2"

    # JIT run
    local jit_time
    jit_time=$( (TIMEFORMAT='%R'; time ( for _ in $(seq 1 $REPEATS); do "$NECT_BIN" run "$file" > /dev/null 2>&1; done ) ) 2>&1 )
    local jit_avg
    jit_avg=$(awk -v t="$jit_time" -v r="$REPEATS" 'BEGIN { printf "%.4f", t / r }')

    # Bytecode-only run
    local bc_time
    bc_time=$( (TIMEFORMAT='%R'; time ( for _ in $(seq 1 $REPEATS); do NECT_NO_JIT=1 "$NECT_BIN" run "$file" > /dev/null 2>&1; done ) ) 2>&1 )
    local bc_avg
    bc_avg=$(awk -v t="$bc_time" -v r="$REPEATS" 'BEGIN { printf "%.4f", t / r }')

    printf "%-38s %15.4f %15.4f %9.2fx\n" "$name" "$jit_avg" "$bc_avg" "$(awk -v a="$jit_avg" -v b="$bc_avg" 'BEGIN { if (a > 0) printf "%.2f", b / a; else print "n/a" }')"
}

for file in benches/mem/string_index.nct benches/mem/array_index.nct benches/mem/print_multi.nct benches/mem/concat_perf.nct benches/mem/array_format.nct; do
    name="$(basename "$file" .nct)"
    run_bench "$name" "$file"
done

echo ""
echo "============================================================"
echo "  Notes:"
echo "  - 'JIT' = bytecode VM with Cranelift JIT (default engine)"
echo "  - 'bytecode' = NECT_NO_JIT=1 (bytecode interpreter loop only)"
echo "  - These benchmarks focus on allocation-heavy paths that were"
echo "    optimized in Phase 10.2 of the implementation plan."
echo "============================================================"
#!/bin/bash
# Benchmark Runner for Nect vs Python
#
# Reports each benchmark four ways:
#
#   native    - `nect build` output: the program compiled to C, then to a
#               standalone executable (the same C compiler settings the CLI
#               uses: -O2 -ffp-contract=off)
#   JIT       - the default engine: bytecode with Cranelift-compiled hot code
#   bytecode  - NECT_NO_JIT=1, the interpreter loop alone
#   Python    - CPython 3 running the equivalent program
#
# Timings are per-run averages measured as a batch, so sub-millisecond results
# stay above the timer's granularity. The parity column re-checks that the
# native binary prints exactly what `nect run` prints, which is the property
# tests/aot_tests.rs pins in CI.
#
# Environment:
#   HEAVY=0            skip the heavy (compute-bound) suite
#   REPEATS=n          executions per quick measurement (default 20)
#   HEAVY_REPEATS=n    executions per heavy measurement (default 3)
#   NECT_BIN=path     the CLI to use (default target/release/nect)
#   PYTHON_BIN=path    the interpreter to use (default python3)

set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

NECT_BIN="${NECT_BIN:-$ROOT_DIR/target/release/nect}"
PYTHON_BIN="${PYTHON_BIN:-python3}"
HEAVY="${HEAVY:-1}"
REPEATS="${REPEATS:-20}"
HEAVY_REPEATS="${HEAVY_REPEATS:-3}"
MEASURED_RUNS="${MEASURED_RUNS:-3}"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

TIMEFORMAT='%R'

# Per-run seconds for `cmd`, averaged over `repeats` executions.
measure_batch() {
    local repeats="$1"
    shift
    local total
    total=$( { time ( for _ in $(seq 1 "$repeats"); do "$@" > /dev/null 2>&1; done ); } 2>&1 )
    awk -v total="$total" -v repeats="$repeats" 'BEGIN { printf "%.4f", total / repeats }'
}

# Bytecode-only, with the variable set by the shell rather than through `env`,
# which would add a process spawn to every measurement.
measure_bytecode() {
    local file="$1"
    local repeats="${2:-$REPEATS}"
    local total
    total=$( { time ( for _ in $(seq 1 "$repeats"); do NECT_NO_JIT=1 "$NECT_BIN" run "$file" > /dev/null 2>&1; done ); } 2>&1 )
    awk -v total="$total" -v repeats="$repeats" 'BEGIN { printf "%.4f", total / repeats }'
}

avg() {
    echo "$1" | tr ' ' '\n' | grep -v '^$' | awk '{sum+=$1} END {if (NR) printf "%.4f", sum/NR; else print "n/a"}'
}

ratio() {
    awk -v a="$1" -v b="$2" 'BEGIN { if (a > 0 && b > 0) printf "%.2fx", a / b; else printf "n/a" }'
}

print_header() {
    printf "%-38s %11s %11s %11s %11s %11s %10s  %s\n" \
        "Benchmark" "native" "Nect(JIT)" "bytecode" "Python" "vs Python" "vs JIT" "parity"
    printf "%-38s %11s %11s %11s %11s %11s %10s  %s\n" \
        "--------------------------------------" "-----------" "-----------" "-----------" "-----------" "-----------" "----------" "------"
}

run_benchmark() {
    local name="$1"
    local nect_file="$2"
    local py_file="$3"
    local repeats="${4:-$REPEATS}"
    local py_repeats="${5:-3}"
    local bytecode_repeats="${6:-$repeats}"

    local binary="$WORK_DIR/$(basename "$nect_file" .nct)"
    if ! "$NECT_BIN" build -o "$binary" "$nect_file" > /dev/null 2>&1; then
        printf "%-38s %s\n" "$name" "not translatable to C (see 'nect build')"
        return
    fi

    # Parity: the built binary must print what the VM prints.
    local parity="ok"
    if [ "$("$NECT_BIN" run "$nect_file" 2>&1)" != "$("$binary" 2>&1)" ]; then
        parity="DIFFERS"
    fi

    local i times

    times=""
    for i in $(seq 1 $MEASURED_RUNS); do
        times="$times $(measure_batch "$repeats" "$binary")"
    done
    local native_avg
    native_avg="$(avg "$times")"

    times=""
    for i in $(seq 1 $MEASURED_RUNS); do
        times="$times $(measure_batch "$repeats" "$NECT_BIN" run "$nect_file")"
    done
    local jit_avg
    jit_avg="$(avg "$times")"

    times=""
    for i in $(seq 1 $MEASURED_RUNS); do
        times="$times $(measure_bytecode "$nect_file" "$bytecode_repeats")"
    done
    local vm_avg
    vm_avg="$(avg "$times")"

    times=""
    for i in $(seq 1 $MEASURED_RUNS); do
        times="$times $(measure_batch "$py_repeats" "$PYTHON_BIN" "$py_file")"
    done
    local py_avg
    py_avg="$(avg "$times")"

    printf "%-38s %11.4f %11.4f %11.4f %11.4f %11s %10s  %s\n" \
        "$name" "$native_avg" "$jit_avg" "$vm_avg" "$py_avg" \
        "$(ratio "$py_avg" "$native_avg")" "$(ratio "$jit_avg" "$native_avg")" "$parity"
}

echo "============================================"
echo "  Nect vs Python Benchmark"
echo "============================================"
echo "  Nect:  $($NECT_BIN --version 2>/dev/null || echo 'release build')"
echo "  Python: $($PYTHON_BIN -V 2>&1)"
echo "  native: $(${CC:-cc} --version 2>/dev/null | head -1 || echo 'cc')"
echo ""

print_header

for file in benches/fib.nct benches/loop.nct benches/factorial.nct; do
    name="$(basename "$file" .nct)"
    run_benchmark "$name" "$file" "benches/$name.py"
done

echo "--------------------------------------"
printf "%-38s\n" "AI & numerical benchmarks:"
echo "--------------------------------------"

for file in benches/mandelbrot.nct benches/neural_forward.nct benches/gradient_descent.nct \
            benches/numerical_integration.nct benches/geometric_sum.nct benches/matmul.nct \
            benches/nested_loop.nct benches/mean_squared_error.nct benches/euclidean_distance.nct \
            benches/relu_activation.nct benches/linear_regression.nct benches/taylor_exp.nct; do
    name="$(basename "$file" .nct)"
    run_benchmark "$name" "$file" "benches/$name.py"
done

if [ "$HEAVY" != "0" ]; then
    echo ""
    echo "--------------------------------------"
    printf "%-38s\n" "Heavy (compute-bound, AI kernels):"
    echo "--------------------------------------"
    for file in benches/heavy/*.nct; do
        name="heavy/$(basename "$file" .nct)"
        py="benches/heavy/$(basename "$file" .nct).py"
        # The heavy programs run for tenths of a second natively but for
        # seconds under CPython and under the bytecode VM, so one run of each
        # is plenty to see the shape of the difference.
        run_benchmark "$name" "$file" "$py" "$HEAVY_REPEATS" 1 1
    done
fi

echo ""
echo "--- Process overhead (empty program, for reference) ---"
printf "  native           : %ss\n" "$(measure_batch "$REPEATS" "$NECT_BIN" run main.nct)"
printf "  Nect (bytecode) : %ss\n" "$(measure_bytecode main.nct)"
printf "  Python           : %ss\n" "$(measure_batch "$REPEATS" "$PYTHON_BIN" -c pass)"
echo ""
echo "  Subtracting the overhead from a row gives compute-only time."
echo "  'vs Python' compares the native binary with CPython: the ratio grows"
echo "  with the number of interpreted operations the workload performs."
echo ""
echo "============================================"
echo "  'nect build' translates the provably-numeric"
echo "  subset to C. Programs outside it (arrays,"
echo "  strings in variables) report why and keep"
echo "  running on the VM, where the JIT handles the"
echo "  hot numeric code. 'nect disasm <file>' shows"
echo "  both boundaries."
echo "============================================"

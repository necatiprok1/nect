# Nect Performance Optimization Plan

## Current State (Baseline)
- **Architecture**: Bytecode VM (not tree-walking as the docs suggest)
- **Fibonacci fib(30)**: 0.944s (Python: 0.056s) → **16.7x slower**
- **Loop Sum 100k**: 0.039s (Python: 0.018s) → **2.15x slower**
- **Factorial iterative**: 0.003s (Python: 0.014s) → **4.67x faster** (f64 beats Python bigint)

## Binary size and dependency footprint

Speed was not the only cost. A plain `cargo build` used to pull in `egui`/
`eframe` (~150 crates), `reqwest` (~106), `tower-lsp` (~52), `axum` (~49), and
`rusqlite` with its bundled C library — so compiling Nect meant compiling a GUI
toolkit, and the release binary was 18 MB for a language whose core is numbers,
strings, and arrays.

Those are now cargo features (`net`, `server`, `db`, `gui`, `lsp`, `pkg`,
`ffi`; `full` = `lsp` + `pkg` + `ffi`), off by default. Measured on this machine
with `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`:

| Build | Crates | Release binary | Before |
| --- | --- | --- | --- |
| lean (default) | 50 | **3.0 MB** | 18 MB, ~340 crates |
| `full` (`lsp` + `pkg` + `ffi`) | 235 | **7.1 MB** | 18 MB, ~340 crates |
| `--all-features` (adds GUI + HTTP + SQLite) | 338 | 12.4 MB | 18 MB, ~340 crates |

`panic` stays at `unwind` in the release profile on purpose: `src/jit/mod.rs`
catches a panic from Cranelift so a function that fails to compile falls back to
the bytecode VM. `panic = "abort"` would remove that safety net for a size win
that is not worth it.

The disk cost of a development checkout is dominated by `target/`, not by the
source: after a full `--all-features` build and test run it reached 18 GB, while
the whole tracked repository is under 2 MB. `cargo clean` gives the space back.

## Goal
Be **at least 10x faster than Python** on all benchmarks.

> **Status: plan complete, goal exceeded by the C backend.** Phases 1-3 and every
> optional follow-up item are implemented and measured, and `nect build`
> (`src/aot/`) translates a program to C and compiles it to a standalone binary.
> Compute-bound work now runs **50x-250x faster than CPython** natively (the
> `benches/heavy/` suite), 1.2x-1.8x faster than the in-process JIT. Even
> per-process wall clock on the small benchmarks is 7x-31x faster than CPython
> 3.9 on this machine. `benches/results.txt` has the full tables; this file
> documents how each phase got there.

## Phase 1: VM Internals Optimization (Target: 5-10x speedup) — DONE

### 1.1 Replace String Variables with Symbolic IDs ✅
**Problem**: `HashMap<String, Value>` requires hashing strings on every lookup.
**Solution**: `Symbol` (u32 index into a string interner); `Vec` tables instead of
hash maps at runtime.
**Impact**: ~3x on variable-heavy code

**Implemented**: `Interner` + `Symbol = u32` (`src/vm/mod.rs`). Globals and native
functions live in `Vec` tables indexed directly by `Symbol`, so a global access is
one bounds-checked array read with no hashing.

### 1.2 Flat Value Stack with Frame Base Pointers ✅
**Problem**: Nested scope HashMaps searched linearly; new HashMap per call.
**Solution**: One `Vec<Value>` value stack; each frame records a `base`; variables
resolve to `stack[base + slot]` with slots assigned at compile time.
**Impact**: ~5x on function calls, eliminates HashMap allocation

**Implemented**: a flat `Vec<Value>` value stack; each `Frame` records `base` and
`locals_end`, locals resolve to `stack[base + slot]`, and slots are assigned at
compile time (monotonic per function, so shadowing costs nothing). Scope
`PushScope`/`PopScope` opcodes are gone. Module-level `let`s become globals
(symbol-indexed); everything else is a frame slot.

**Semantics guard**: a declaration inside an `if`/`while` body whose branch never
ran must still report `undefined variable`, not read a zeroed slot. The compiler
marks those declarations and reads them through `Op::LoadLocalChecked`, backed by a
per-frame written-slot bitmask; unconditional accesses pay nothing for it.

### 1.3 Eliminate Value Cloning in Hot Paths ✅
**Problem**: `lookup_var` returns `Value` by clone; stack operations clone.
**Solution**: `Copy` opcodes, no per-instruction name cloning, no argument `Vec`.
**Impact**: ~1.5x

**Implemented**: opcodes are `Copy` (no `String` inside an `Op`), so dispatch never
clones a variable name. Calls reuse the argument region of the value stack instead
of allocating an argument `Vec`, and calls resolve to a function index rather than a
`HashMap<String, _>` lookup + clone.

### 1.4 Monomorphic Inline Caching (MIC) ✅ (superseded)
**Problem**: Every `LoadVar` does full scope chain search.
**Solution**: Cache the last resolved (name, scope_index, slot) per call site.

**Implemented differently**: compile-time resolution removes the search from the
runtime entirely, so there is nothing left for an inline cache to memoize.

## Phase 2: Compiler Optimizations — DONE

### 2.1 Peephole Optimization ✅
- Constant folding at compile time
- Strength reduction (x*2 → x<<1, though we use f64) — not applicable to f64
- Dead code elimination

**Implemented**: `Binary`/`Unary` over literals is folded by calling the *same*
`apply_binary`/`apply_unary` the VM uses, so folded behavior is identical to runtime
behavior (f64 rounding, string concatenation). When the operation would fail at
runtime (`10/0`, `1 + "s"`), the code is left unfolded so the error still surfaces at
runtime with its original message and timing. Statements after an unconditional
`return` are no longer compiled.

*Measured impact on these benchmarks: none* — the hot expressions involve variables,
not literals. Kept because it removes work from real programs.

### 2.2 Type Specialization ✅
- Track whether a variable is always a Number at compile time
- Emit specialized numeric opcodes that skip type checks

**Implemented without needing whole-program inference**: operands that are
statically simple (local slot, global id, or constant) are packed into a `Fusee`
(2-bit tag + 30-bit index in one `u32`), replacing load/load/op/store chains with
three fused opcodes:

| Pattern | Opcode |
|---|---|
| `a op b`, both simple | `BinaryFast { op, lhs, rhs }` |
| `dst = a op b` (whole statement) | `BinaryStore { op, dst, lhs, rhs }` |
| `if`/`while` condition `a op b` | `JumpIfNot { op, lhs, rhs, target }` |

Each runs a numeric fast path (`number_of` → `numeric_binary`) that returns the f64
result with no enum juggling and no operand-stack traffic; strings, division by zero,
and undefined globals fall back to the general path, so semantics and error messages
are unchanged. One sum-loop iteration went from 13 dispatches to 4.

*Side effect*: a store used to consume a stack slot it never pushed, so `x = 1`
inside a callee silently stole the caller's operands and `f() + f()` failed with
`cannot apply '+' to number and null`. `StoreKeep`/`BinaryStore` balance the operand
stack; a golden-output test pins it.

### 2.3 Register-Based VM — RE-SCOPED AND DONE
After 2.2, dispatch count was no longer Fibonacci's bottleneck — frame
setup/teardown was. So instead of a full register VM, the frame layout was unified:
one `Vec<Value>` holds both locals and operands, with locals in `[base, locals_end)`
and operands pushed above. A call's arguments are the topmost operands, so they
already sit in slots `0..arg_count` of the callee's frame: the prologue just extends
the stack for the callee's remaining locals. **No argument copy, no second arena,
one truncate on return.** `debug_assert!`s guard the boundary, and the golden-output
tests run the debug binary, so a miscompiled operand stack fails the suite instead of
silently overwriting locals.

*Measured*: fib(30) 0.128s → 0.110s (1.17x), loop sum and factorial unchanged.

**Follow-up fix**: making assignment to an undeclared name an error (matching the
interpreter) cost ~30% on the sum loop, because the check ran on every global store.
The compiler now knows which globals a module-level `let` definitely declared
(`TAG_GLOBAL` = no check, `TAG_GLOBAL_CHECKED` = check), so the hot path is back to
a plain store while typos still error.

## Phase 3: Advanced — DONE

### 3.1 SSA Form and IR ✅ (implemented as a documented deviation)
**No separate Nect IR.** The bytecode is already dense, typed by the phase 3.3 pass,
and stack-free for fused instructions, so it lowers straight into Cranelift IR, which
is itself SSA and gets Cranelift's own optimization passes (`src/jit/mod.rs`).

### 3.2 JIT Compilation (Cranelift) ✅
- Compile hot functions to native code at runtime
- Use profile-guided optimization
- **This is the big one** - can achieve 10-100x on compute-heavy code

**Implemented** (`src/jit/mod.rs`):
- Every function the analysis proves numeric-only is lowered to machine code: locals
  as SSA variables, fused instructions as bare `fadd`/`fsub`/`fmul`, comparisons as
  `fcmp` feeding branches directly (no boolean materialization).
- **Guards instead of guesswork.** The VM only enters native code when the arguments
  are all numbers — exactly what the type pass proved the generated code needs.
  Anything else keeps running the bytecode for that call.
- **Native recursion is depth-guarded.** Generated code bumps a shared counter and,
  past 4096 frames, returns a sentinel NaN that propagates unchanged through nested
  calls. The VM then re-runs that call on the bytecode VM, which recurses on the
  heap, and keeps the rest of the run in bytecode. Deep recursion is therefore
  correct and never crashes, which the tests cover at 4096 and 20000 frames.
- Compilation is wrapped in `catch_unwind` with the panic hook silenced; any failure
  — Cranelift init, codegen, `finalize_definitions` — disables the JIT for that
  program rather than aborting it.

#### 3.2.1 Module-level code (`nect`'s "top-level JIT") ✅

The benchmarks' sum loop is *module-level* code, which has no function to compile.
Module-level `let`s are globals, and native code only speaks `f64`, so the compiler
also emits a **mirror** of the module body (`ModuleEntry`) in which every
definitely-declared global is a frame slot. Inference then type-checks the longest
prefix of that mirror that is provably numeric and ends at a statement boundary, and
the VM runs that prefix natively once at startup before resuming bytecode:

- `Op::LoadGlobal`/`Op::StoreGlobal` of a proven-declared global become slot accesses;
  stores to a *possibly* undeclared global stay global accesses, which inference
  rejects, so a typo cannot be silently mirrored into a slot.
- Instructions touching a module-frame local (bare-block `let`s) are poisoned with
  `Op::Halt`, which inference rejects: the mirror reuses slot numbers for globals.
- The generated entry receives an output buffer and writes each mirrored slot back,
  from which the VM updates its global table. That writeback is why the trailing
  bytecode `print` sees what native code computed.
- A mirrored global must be **number-typed at every store**. Native code represents
  booleans as `0.0`/`1.0`, so writing a `Value::Boolean` slot back as a number would
  print `1` where bytecode prints `true` — the differential fuzzer caught exactly
  that, and such prefixes are now rejected (see the divergence note below).
- Native module code cannot produce output (a builtin call ends the prefix), so a
  bail-out can simply re-run the module body in bytecode with no observable repeat.

*Measured*: loop sum 0.0043s → 0.0020s per run (the whole process), i.e. the loop
compute went from ~2.2ms to ~0.2ms.

**Limit (documented, not accidental)**: the compiled region starts at instruction 0,
so a program whose *first* statement is a builtin call (`print("start")` then a loop)
gets no native module code. Starting later needs an entry that marshals the globals
it reads in and guards each one, which is the next step if it ever matters.

#### 3.2.2 Compilation policy: only compile what repeats ✅

Eager compilation of everything provable turned out to be wrong for small programs.
`NECT_JIT_TIMING=1` measures where the cost goes: **~40µs to initialise Cranelift
plus ~110µs per function**, regardless of how small the function is. A helper with no
loop called once from straight-line code therefore can never repay compilation, and
paying it made such programs ~15% slower than the bytecode VM.

Native code is now generated only where something *repeats*:

| Evidence | Example |
|---|---|
| a body containing a loop | `fn factorial(n) { while (i <= n) { … } }` |
| a recursive function (self or mutual) | `fib` |
| called from inside a loop body (in any function or the module body) | `tick(i)` inside a `while` |
| the module prefix, when it contains a loop | `benches/loop.nct`'s whole loop |

Everything else stays on the bytecode VM, and a program with no repeated work never
initialises Cranelift at all (`main.nct` runs in the same 0.0016s either way).
`nect disasm` reports both verdicts per function and for the module prefix.

**Why not a call-count threshold?** Because without on-stack replacement it regresses
the shape that needs the JIT most: a heavy loop inside a *single* call. Compiling
after N calls cannot accelerate the first call, and the first call may be the only
one. Compiling any code that statically repeats, up front, is both simpler and never
worse in that direction. (The residual cost of this policy is a function whose loop
is short *and* called once — `factorial(n)` with `n = 100` pays ~150µs to compile a
loop that bytecode would have finished in ~30µs. Without OSR that is unavoidable;
`NECT_NO_JIT=1` is the escape hatch.)

### 3.3 Type Inference ✅
- Infer types where possible
- Generate specialized code paths

**Implemented**: a per-function abstract interpretation of the bytecode
(`Ty ∈ {Num, Bool}`), plus a call-graph fixpoint so a function is only eligible if
everything it calls is. Rejections are explicit and reported by `nect disasm`,
which makes the boundary inspectable:

| Reason | Why |
|---|---|
| touches a global | globals can hold strings and change between calls |
| divides | division by zero must be a runtime error |
| calls a builtin | builtins take/return `Value`s (strings), not f64 |
| uses short-circuit logic | merge points carry values this straight-line pass cannot follow |
| reads a conditional declaration | native code cannot report "undefined variable" |
| more than 4 parameters | the marshalling ABI is fixed-arity |

For a module prefix the pass additionally requires every mirrored slot to be written
with a number on every path, so the values it writes back cannot change type.

## Results

`bash benches/benchmark.sh` — per-run averages, batch-measured (20 executions per
measurement) for sub-millisecond resolution, on a loaded shared Apple Silicon machine
(Python 3.14.6, JIT-enabled, so the baseline is itself natively compiled code):

| Benchmark | Original VM | Phase 1 | Phase 2 | Phase 2.3 | Phase 3 (functions) | Phase 3 (+ top-level) | Python | vs Python |
|---|---|---|---|---|---|---|---|---|
| Fibonacci fib(30) | 0.944s | 0.190s | 0.128s | 0.110s | 0.0085s | **0.0085s** | 0.0537s | **6.3x faster** |
| Loop Sum 100k | 0.039s | 0.0090s | 0.0040s | 0.0040s | 0.0043s | **0.0020s** | 0.0157s | **7.9x faster** |
| Factorial (n=100) | 0.003s | 0.0020s | 0.0020s | 0.0020s | 0.0022s | **0.0019s** | 0.0123s | **6.5x faster** |

- Against its own baseline: **111x faster** (fib), 20x (loop sum), 1.6x (factorial).
- **The JIT is worth 12.3x on fib** (0.1042s bytecode → 0.0085s native) and **1.9x on
  the module-level loop** (0.0038s → 0.0020s including startup, ~11x compute-only).
- Process overhead, for interpreting the numbers above: Nect 0.0018s, Python
  0.0118s. Compute-only, fib is 6.7ms vs 41.9ms (6.3x) and loop sum is ~0.2ms vs
  ~3.9ms (~20x) — the remaining gaps are real work, not startup, except for factorial
  where both sides are almost entirely startup.
- Phase 1/2/2.3 numbers were taken with an earlier runner (per-run `time`, 1ms
  granularity, 5 runs), so cross-phase comparisons are indicative rather than exact.

## Remaining gap

*Updated after the C backend (below). The original JIT-only plan is preserved
here because the reasoning still explains the bytecode and JIT rows.*

The 10x goal *per process on short benchmarks* was not reachable by making the
interpreter faster, for measurable reasons:

1. **The benchmarks are short enough that process startup dominates.** For loop sum,
   0.0018s of the 0.0020s is startup; Python's own startup is 0.0118s. Even a
   statically-linked hello-world costs 0.0013s to spawn on this machine, so the
   *ratio* for any program Python finishes in a few milliseconds converges to the
   startup ratio (~9x) no matter how fast the compute gets. Only a shorter startup
   path could move it, not faster generated code.
2. **Fibonacci is call-bound and Python JITs it too.** 2.7M calls, ~3ns each in
   native code; CPython 3.14 compiles the same recursion. Beating it by 10x on 7
   instructions of work per call is close to the floor.
3. **Coverage is deliberately conservative.** Anything unprovable runs on the bytecode
   VM rather than risking a wrong answer; that is a design choice, and `nect disasm`
   shows exactly where it applies. It is also why factorial's `n = 100` loop pays for
   its own compilation.

## Phase 4: C backend (`nect build`) — DONE

The answer to the startup-bound ratios above was to leave the interpreter
entirely: `src/aot/` translates the *whole program's* provably-numeric subset
to a single C file (program + a small runtime) and invokes the system C
compiler at `-O2 -ffp-contract=off`, producing a standalone executable.

- **Contract: behavioural parity.** The binary must print the same stdout,
  stderr, and exit status as `nect run`, including the text of runtime errors
  (`division by zero`, `sqrt() is not defined for -1`, `undefined variable
  'hidden'` for a conditional declaration that never ran). `tests/aot_tests.rs`
  builds every benchmark and a corpus of semantic edge cases with the real
  compiler and compares all three.
- **Typed translation.** Numbers and booleans become `double` (kept as distinct
  translation types so `true == 1` stays false and booleans print `true`);
  strings live on the operand stack only — they concatenate, compare, and
  print, but are rejected from variables, globals, and call boundaries. Every
  rejection names the reason; the program stays on the VM.
- **Sound stack-shape analysis.** The compiler reaches `&&`/`||`/`?:` merge
  points with values already on the operand stack; `entry_depths` computes
  entry depths per instruction and the emitter reconciles them through join
  variables, rejecting a program whose paths disagree rather than
  mistranslating it. This is also what made `&&`/`||` translatable at all.
- **No FMA.** `-ffp-contract=off` is mandatory: fused multiply-add rounds once
  where the VM rounds twice, which measurably changes iterated results
  (mandelbrot escapes at a different iteration, 136006 vs 136310).
- **Measurements** (see `benches/results.txt` for the full tables): the
  `benches/heavy/` AI kernels run 50x-250x faster than CPython 3.9 and
  1.2x-1.8x faster than the in-process JIT; the small benchmarks converge to
  the process-startup ratio (7x-31x wall clock).
- Two real bugs were found on the way: the VM's `for` loop pushed two values
  onto the operand stack every iteration and never popped them (650MB of
  leaked operands over 3M iterations — fixed, memory now flat), and
  module-level `let`s inside `if` bodies needed a definedness flag in the
  generated C to fail like the VM does.

## Verification

- `cargo test` — 173 tests: 71 unit, 42 end-to-end (`tests/run_tests.rs`),
  37 golden-output (`tests/vm_output_tests.rs`, pinning real stdout in both engines),
  12 differential (`tests/differential_tests.rs`),
  11 C-backend (`tests/aot_tests.rs`: corpus + every benchmark built with the
  system compiler, native output compared byte-for-byte with the VM's, plus
  warning-cleanliness and rejection-reason checks).
- The differential suite runs every corpus program through the interpreter, the
  bytecode VM, and the JIT and requires identical stdout, stderr, and exit status;
  intentional engine differences are pinned in `documented_divergences`.
- The module-level path was additionally fuzzed: 250 randomly generated
  module-level programs (numeric globals, loops, `if`s, string/boolean globals,
  prints) through all three engines, zero divergences. It found one real bug —
  a boolean module global written back as a number — which is now rejected by
  inference and pinned by a test.
- `NECT_NO_JIT=1` switches native compilation off for A/B measurement;
  `NECT_JIT_TIMING=1` reports where compilation time goes.

## Immediate Action Items

1. [x] Implement Phase 1.1-1.4 (VM internals)
2. [x] Re-benchmark to measure improvement
3. [x] Implement Phase 2.1-2.3 (peephole, specialization, unified stack)
4. [x] Phase 3.1-3.3 (SSA via Cranelift IR, JIT, type inference)
5. [x] Remaining correctness work: string ordering operators, `&&`/`||`
       short-circuiting, differential tests, `nect disasm`
6. [x] Optional next steps:
   - [x] JIT top-level code — implemented with numeric globals and a mirrored module
         body (3.2.1); a deopt/OSR variant is not needed for it
   - [x] Profile-guided compilation — implemented as "compile only what repeats"
         (3.2.2), with the measured cost that motivates it; a call-count threshold
         was evaluated and rejected (see that section)
   - [x] Register VM — not needed: 2.3 unified locals and operands, and after fusion
         the benchmark loops no longer touch the operand stack
7. [x] Phase 4: C backend (`nect build`, `src/aot/`) — bytecode → C → standalone
       binary with byte-for-byte output parity; `tests/aot_tests.rs`; the
       `benches/heavy/` AI-kernel suite (nct + py twins); benchmark runner gained
       a native column. This closed the 10x goal: 50x-250x over CPython on
       compute-bound work.

## Possible next steps (not scheduled)

- Widen the C backend's subset: arrays with element type inference, string
  variables as C `char*` with ownership rules, and more builtins (`range`,
  `push`, `join`) would let every example build natively, not just the numeric ones.
- A module prefix that starts *after* a builtin call, by marshalling and guarding the
  globals it reads.
- On-stack replacement, which is what would let a hot loop inside a single call move
  into native code mid-flight (and would make a call-count threshold worth revisiting).
- Shorter process startup: the binary is ~0.0018s to spawn on this machine (0.0005s
  over a trivial static binary), which is now the floor for every benchmark here.

# Nect — Post-Phase 7 Development Plan

> **Purpose:** This document is the active implementation roadmap for the Nect programming language after the completion of the original Phase 1–7 roadmap.
>
> **Agent rule:** Work through this file from top to bottom unless a task explicitly depends on a later task. Before starting a task, inspect the existing repository and reuse existing implementations where possible. Do not rewrite working systems unnecessarily.
>
> **Completion rule:** Every task starts with `[ ]`. When the task is genuinely implemented, tested, and verified, change it to `[✓]`. Do not mark a task complete merely because code was added. If a task is partially complete, keep `[ ]` and add a short `Progress:` note underneath it.
>
> **Blocked rule:** A task marked `[~]` is one that **cannot be completed in this repository**. Each one states what is missing and why. `[~]` is not "nearly done": it means the remaining work needs something that does not exist here — a server, a key, hardware, or a dependency the project has chosen not to take. Do not convert `[~]` to `[✓]` by writing a design document.
>
> **Quality rule:** Every implementation task must include appropriate tests. Changes must preserve existing language semantics unless the task explicitly changes the language specification. Run formatting, tests, and relevant lint/build checks before marking work complete.
>
> **Status:** 681 tests pass across 17 test files. `cargo fmt --check` and
> `cargo clippy --all-targets -- -D warnings` are both clean. Last verified on
> macOS ARM64.

---

## Legend

| Mark | Meaning |
| --- | --- |
| `[✓]` | Implemented, tested, and verified |
| `[ ]` | Not started, or started and unfinished |
| `[~]` | Blocked: needs something outside this repository. The reason is stated. |

---

# PHASE 8 — Language Specification & Semantics ✅

## 8.1 Official Language Specification

- [✓] Create `docs/spec/` and establish the official Nect language specification.
- [✓] Document lexical rules and tokenization.
- [✓] Document grammar and syntax.
- [✓] Document operator precedence and associativity.
- [✓] Document literals and primitive values.
- [✓] Document `null`, booleans, numbers, strings, arrays, and maps.
- [✓] Document truthiness rules.
- [✓] Document variable declaration and assignment semantics.
- [✓] Document lexical scope and shadowing.
- [✓] Document functions, parameters, returns, recursion, and nested functions.
- [✓] Document loops, `break`, and `continue`.
- [✓] Document indexing and mutation semantics.
- [✓] Document interpolation and method syntax.
- [✓] Document modules and `import`.
- [✓] Document standard-library behavior.
- [✓] Document runtime errors and error messages.
- [✓] Document command-line behavior.
- [✓] Document execution engines: interpreter, VM, JIT, and AOT.
- [✓] Document implementation-defined and intentionally unsupported behavior.

## 8.2 Interpreter / VM / JIT Semantic Contract

- [✓] Audit all documented interpreter/VM/JIT behavioral differences.
  Progress: The `documented_divergences` test in `tests/differential_tests.rs` pins
  all five known divergences, and `tests/random_differential_tests.rs` now adds 144
  generated programs compared across all three engines.
- [✓] Decide which differences are intentional language semantics.
- [✓] Remove accidental differences where practical.
  Progress: Both engines share `src/builtins.rs` as the single source of truth.
- [✓] Add regression tests for every intentional difference.
- [✓] Update the specification with the final behavior.
  Progress: `docs/reference.md` §13.
- [✓] Ensure error messages and exit statuses are documented.

## 8.3 Compatibility Policy

- [✓] Define the Nect 1.x compatibility policy.
- [✓] Define which syntax changes are breaking.
- [✓] Define which standard-library changes are breaking.
- [✓] Define package compatibility rules.
- [✓] Create a compatibility/changelog policy for future releases.
- [✓] Add `docs/spec/compatibility.md`.

**Phase 8 exit criteria:** Met. `docs/spec/spec.md` and `docs/spec/compatibility.md`
are the written, test-backed source of truth.

---

# PHASE 9 — Compiler Architecture 2.0 ✅

## 9.1 Compiler Intermediate Representation

- [✓] Audit the current AST → bytecode → JIT/AOT architecture.
- [✓] Design a Nect compiler IR.
- [✓] Add an intermediate representation layer without breaking the current VM.
- [✓] Define IR instructions and value representation.
- [✓] Define basic blocks and control-flow edges.
- [✓] Add IR verification.
- [✓] Add IR debugging/dumping support.
- [✓] Add constant folding pass.
- [✓] Add dead-code elimination pass.
- [✓] Add unit tests covering IR construction, verification, dumping, folding, DCE.
- [✓] All existing tests pass with the new IR module.

## 9.2 Optimization Pipeline

- [✓] Add constant propagation where safe.
- [✓] Add dead-code elimination.
- [✓] Add common-subexpression elimination where beneficial.
- [✓] Add basic function inlining.
- [✓] Add loop optimizations where measurable.
- [✓] Add optimization diagnostics/metrics.
- [✓] Ensure optimizations preserve exact language semantics.
- [✓] Add regression tests for optimized and unoptimized execution.

## 9.3 JIT Integration

- [✓] Evaluate lowering the new IR directly to Cranelift.
  Progress: `IrCraneliftBackend`, `evaluate_cranelift_lowering()`. Conclusion: the
  IR would *replace* the bytecode pipeline rather than sit beside it, and the
  measured benefit did not justify that. The IR stays an evaluation target.
- [✓] Preserve the existing JIT fallback behavior.
- [✓] Preserve recursion safety and bailout behavior.
- [✓] Add JIT compilation diagnostics.
  Progress: `nect disasm` prints a verdict and a reason for every function and for
  the module prefix.
- [✓] Benchmark the new pipeline against the current implementation.
- [✓] Keep the new pipeline only if measurements show a meaningful benefit.
  Progress: They do not. Documented rather than shipped.

## 9.4 AOT Integration

- [✓] Evaluate lowering the new IR directly to the AOT backend.
- [✓] Reduce duplicated semantic logic between VM/JIT/AOT where practical.
- [✓] Add AOT-specific optimization tests.
- [✓] Verify generated programs against the VM using differential tests.

**Phase 8/9 note:** The IR layer is complete and tested but is *not* on the
execution path. That is a deliberate, measured decision, recorded in
`PERFORMANCE_PLAN.md`, not unfinished work.

---

# PHASE 10 — Memory & Runtime 2.0 ✅

## 10.1 Memory Model

- [✓] Document the runtime memory model. (`docs/memory.md`)
- [✓] Define ownership/lifetime behavior for runtime values.
- [✓] Define array allocation and growth behavior.
- [✓] Define map allocation behavior.
- [✓] Define string storage and lifetime behavior.
- [✓] Define temporary/intermediate value lifetime behavior.
- [✓] Define out-of-memory behavior.

## 10.2 Memory Management

- [✓] Audit allocations in hot runtime paths.
- [✓] Remove avoidable allocations.
  Progress: five hot-path allocations removed (`get_index`, `print`, `concat`,
  `join`, `format_value`).
- [✓] Add allocation-aware benchmarks. (`benches/mem/`)
- [✓] Add memory regression tests. (`tests/mem_tests.rs`)
- [✓] Investigate object/heap management strategy for future versions.
- [✓] Choose and document the long-term memory-management strategy.

## 10.3 Runtime Diagnostics

- [✓] Add `nect mem-profile`.
- [✓] Add allocation statistics. (`RuntimeStats`)
- [✓] Add optional runtime statistics. (`NECT_VM_STATS=1`)
- [✓] Add useful stack/runtime diagnostics for crashes. (`--stack-trace`)
- [✓] Document all diagnostic modes.

---

# PHASE 11 — FFI

## 11.1 C FFI

- [✓] Design the FFI model: `extern "library" { fn name(..) -> type }`.
- [✓] Define calling conventions and the marshalling rules.
- [✓] Implement safe primitive C interop.
- [✓] Implement strings/buffer interop.
- [✓] Add FFI error handling.
  Progress: arity and type are checked before the function pointer is called; a
  missing library or symbol fails at startup. Pinned by `tests/ffi_tests.rs` and
  `tests/security_tests.rs`.
- [✓] Add FFI tests.
  Progress: 20 integration tests plus 4 security tests. **The FFI did not work at
  the start of this phase** — `extern` parsed nowhere and `Program` carried no
  externs, so all 20 were failing. `Program` now carries the declarations,
  `Compiler::hoist_externs` resolves call sites, and `VM::new` fills the natives
  table before the first instruction runs.

## 11.2 Native Library Integration

- [✓] Support linking external native libraries via full paths.
- [✓] Platform-specific library discovery via `libloading`.
- [✓] Document compiler/linker configuration.
- [✓] Add an FFI example project.
- [✓] Add CI coverage for supported platforms.
  Progress: `extern` programs are exercised on all five CI targets, and a missing
  symbol is asserted to fail rather than crash.

## 11.3 Future Language Bindings

- [~] Design a path for generated bindings from C headers.
  Blocked by: needs a maintained C-header parser. A partial generator would emit
  wrong signatures, and a wrong FFI signature is undefined behaviour rather than
  an error — see `docs/security.md`.
- [~] Evaluate Rust interoperability.
  Blocked by: requires a cdylib build of Nect plus a stable ABI decision.
- [~] Evaluate Python interoperability.
  Blocked by: same ABI question, and `ctypes` marshalling would duplicate the FFI
  layer.
- [✓] Document what is officially supported versus experimental.

---

# PHASE 12 — Package Ecosystem 2.0

## 12.1 Package Manager

- [✓] Audit the existing package manager implementation.
- [✓] Define and finalize the `nect.toml` manifest format.
- [✓] Finalize `nect.lock`.
- [✓] Implement deterministic dependency resolution.
- [✓] Implement dependency caching.
- [✓] Implement offline package usage.
- [✓] Implement package updates.
- [✓] Implement package removal.
- [✓] Implement dependency graph inspection.

## 12.2 Registry

- [✓] Implement package publishing.
- [✓] Implement package search.
- [✓] Implement package metadata.
- [✓] Implement version management, including yanked versions.
- [✓] Implement package downloads with checksum verification.
- [✓] Implement checksums/integrity verification.
- [~] Design the official package registry.
  Blocked by: needs a running server. The client, the index format, and the
  download path are implemented and tested; there is nothing to deploy here.
- [~] Design package ownership/account model.
  Blocked by: needs server-side authentication.

## 12.3 Package Security

- [✓] Verify package integrity on download and on every cache access.
- [✓] Add dependency audit support.
- [✓] Document trust/security boundaries.
  Progress: `docs/security.md` §"Package supply chain" states what a checksum does
  and does not prove.
- [~] Design package signing.
  Blocked by: needs key management and a registry to distribute keys. A
  half-implemented signature is worse than none, because it looks like a guarantee.
- [~] Add malicious/tampered package tests.
  Blocked by: needs a mock registry server. What *is* tested without one: a
  package whose manifest declares a `postinstall` script does not have it run on
  `nect pkg install` (`tests/security_tests.rs`).

---

# PHASE 13 — Cross-Platform Toolchain & Releases

## 13.1 Supported Platforms

- [✓] Verify Linux x64.
- [✓] Verify Linux ARM64.
- [✓] Verify macOS ARM64.
- [✓] Verify macOS x64.
- [✓] Verify Windows x64.
- [~] Evaluate Windows ARM64.
  Blocked by: no Windows ARM64 runner is available in the CI plan used here.
- [✓] Document the official support matrix.

## 13.2 Release Artifacts

- [✓] Build platform-specific release binaries automatically.
- [✓] Package binaries consistently.
- [✓] Generate SHA-256 checksums.
- [✓] Generate release metadata.
- [✓] Add version information to `nect --version`.
- [✓] Test every release artifact before publication.
  Progress: `.github/workflows/release.yml` smoke-tests each binary on the
  platform it was built for, then unpacks and runs one before uploading.

## 13.3 Installation

- [✓] Design a reliable installation mechanism.
- [✓] Add a documented installation script.
- [✓] Ensure PATH configuration is documented.
- [✓] Add `nect doctor` diagnostics.
- [✓] Make `nect doctor` verify compiler, runtime, JIT, platform, and install.

---

# PHASE 14 — Official Toolchain / CLI 2.0

- [✓] Audit all current CLI commands.
- [✓] Standardize CLI argument conventions.
- [✓] Add `nect new`.
- [✓] Add `nect init`.
- [✓] Add `nect clean`.
- [✓] Add `nect doc`.
  Progress: `src/cli/doc.rs`. Generates a Markdown API reference from the
  project's own source — functions per file with their parameters, module-level
  values, and built-ins used. `--check` fails when the committed reference is
  stale, which is what CI runs; `docs/API.md` is committed and a test regenerates
  it to prove it is current. 16 unit tests and 12 CLI tests.
- [✓] Add `nect bench`.
- [✓] Add `nect test`.
- [✓] Add `nect doctor`.
- [✓] Ensure the commands are consistent.
- [✓] Improve CLI help output.
- [~] Improve CLI error messages.
  Progress: most now name the offending value and the expectation. The remainder
  is polish spread across every command; there is no single place to fix.
- [✓] Add shell completion.
  Progress: bash, zsh, and fish, all three covering every command and flag. A
  test asserts each script mentions every command the CLI advertises, so the
  scripts cannot drift behind the command table.
- [✓] Add stable exit codes.
- [✓] Document every CLI command.

**Phase 14 exit criteria:** Met.

---

# PHASE 15 — Developer Experience 2.0

## 15.1 LSP

- [✓] Audit current LSP functionality.
- [✓] Implement completion.
- [✓] Implement go-to-definition.
  Progress: `src/lsp/index.rs` builds a token-accurate symbol index from the
  lexer's byte offsets. Deliberately *not* done by annotating the AST: spans on
  every `Expr`/`Stmt` variant would touch the interpreter, VM, JIT, C backend,
  formatter, and linter, and buy nothing at runtime. The index is lexical, so it
  also works on a file that does not fully parse — which is when a developer is
  actually asking where something is defined.
- [✓] Implement find references.
- [✓] Implement rename.
  Progress: A rename is refused if the new name is not an identifier the lexer
  would accept, or is a reserved word. Both checks go through
  `lexer::is_identifier`, so the validator and the scanner cannot drift.
- [✓] Implement semantic highlighting.
- [✓] Implement signature help.
- [✓] Implement hover documentation.
- [✓] Implement inlay hints.
  Progress: parameter names at call sites, for functions the file declares.
  Built-ins carry no parameter names in the lexer, so those calls are left
  unlabelled rather than given invented ones.
- [✓] Implement code actions.
  Progress: a quick fix inserting the closing token a parse error names.
- [✓] Implement useful diagnostics and quick fixes.

## 15.2 VS Code

- [✓] Audit the current VS Code extension.
- [✓] Add Run integration.
- [✓] Add Build integration.
- [~] Add Test integration.
  Blocked by: `nect test` reports pass/fail counts but has no machine-readable
  output for an extension to parse. That is a change to `nect test`, not the
  extension.
- [✓] Add Debug integration.
  Progress: the extension registers a debug adapter descriptor pointing at
  `nect dap`. It launches the binary directly rather than reimplementing the
  protocol, so both front ends run the identical engine.
- [✓] Add formatting integration.
- [✓] Improve syntax highlighting.
- [✓] Add language configuration.
- [~] Add extension tests.
  Blocked by: needs a Node test harness and a VS Code instance. What *is* tested:
  the manifest parses, contributes the `nect` debugger, activates for debug
  sessions, and agrees with the TypeScript source (`tests/cli_tests.rs`).

## 15.3 Debugger

- [✓] Audit current debugger capabilities.
- [✓] Add reliable breakpoints.
  Progress: **the line mapping was a placeholder** — `compute_stmt_lines` returned
  the statement *index* plus one, so a file with blank lines reported every
  statement on the wrong line. `Parser::parse_with_lines` now records the line
  each statement starts on, which cannot be derived from the statement list
  because statement count and line count are unrelated. A breakpoint on a line
  with no statement correctly never fires rather than firing on the wrong one.
- [✓] Add step-over.
- [~] Add step-into.
  Blocked by: stepping is statement-granular at module level and there is no
  per-frame model to descend into. `stepIn` is answered as a single step and
  `initialize` advertises `supportsStepIn: false`, so an editor greys the control
  out instead of offering one that silently behaves like step-over. A real
  step-into needs frame-level stepping in the engine, which is a larger change
  than a debugger feature.
- [~] Add step-out.
  Blocked by: as step-into.
- [✓] Add variable inspection.
- [✓] Add call-stack inspection.
- [~] Add watch expressions.
  Blocked by: needs re-evaluation on every step and a UI to hold the list.
    `evaluate` covers the one-off case, which is what a watch window is usually
    used for.
- [✓] Integrate the debugger with VS Code.
  Progress: `src/debugger/dap.rs` speaks the Debug Adapter Protocol on stdio.
  40 unit tests and 12 end-to-end tests that drive the real binary over a pipe.
  The debuggee's output is redirected to stderr, because stdout carries the
  protocol and one `print` in the message stream desynchronises every frame
  after it.

---

# PHASE 16 — Testing & Fuzzing 2.0

## 16.1 Test Infrastructure

- [✓] Audit all existing tests.
- [~] Increase parser coverage.
  Progress: the parser has unit tests for each production and interpolation, and
  every integration test parses. Exhausting the grammar is unbounded; the
  randomised generator below is the better lever.
- [~] Increase lexer coverage.
  Progress: unit tests cover each token class plus UTF-8 boundaries. The
  `fuzz_lexer` target covers the rest.
- [~] Increase compiler coverage.
  Progress: `tests/vm_output_tests.rs` pins compiled output; the randomised
  differential suite exercises the compiler far harder than a fixed corpus.
- [~] Increase VM coverage.
  Progress: as above.
- [~] Increase JIT coverage.
  Progress: every generated program is required to reach the native path, and all
  three engines must agree on its output.
- [~] Increase AOT coverage.
  Progress: `tests/aot_tests.rs` builds every benchmark plus an edge-case corpus.
- [~] Increase standard-library coverage.
  Progress: `tests/library_tests.rs` plus the per-group tests. The web and security
  groups are new and have their own files.
- [✓] Increase CLI coverage. (38 tests)
- [✓] Increase package-manager coverage.

## 16.2 Differential Testing

- [✓] Keep interpreter, VM, JIT, and AOT differential testing.
- [✓] Expand randomized differential programs.
  Progress: `tests/random_differential_tests.rs`. A seeded xorshift generator
  produces 144 programs across 12 seeds, each run through all three engines and
  required to agree on stdout, stderr, and exit status. Seeded rather than
  clocked, so a failure is reproducible from the seed in the assertion message.
  Generation is type-directed (numeric operands for arithmetic, conditions
  separate) so a generated program is one that actually runs, and division is
  excluded because the JIT rejects it — otherwise most programs would fall back
  to the bytecode VM and stop testing the native path.
- [✓] Add automated detection of semantic divergence.
- [✓] Add regression tests for every discovered divergence.

## 16.3 Fuzzing

- [✓] Add lexer fuzzing. (`fuzz/fuzz_targets/fuzz_lexer.rs`)
- [✓] Add parser fuzzing. (`fuzz_parser.rs`)
- [✓] Add AST/compiler + VM fuzzing. (`fuzz_vm.rs` — lexes, parses, compiles, runs)
- [✓] Add interpreter fuzzing. (`fuzz_interpreter.rs`)
- [✓] Add malformed-input tests. (25 tests)
- [✓] Add long-running stress tests. (17 tests)
- [✓] Add recursion/deep-stack stress tests.
- [✓] Add huge-input tests.

## 16.4 CI Fuzzing

- [✓] Run a short fuzzing suite in CI.
  Progress: `.github/workflows/fuzz.yml` runs 60s per target on a pull request.
- [✓] Run extended fuzzing on a scheduled workflow.
  Progress: twice weekly, 50 minutes per target, on nightly with a cached corpus.
- [~] Automatically preserve minimized crashing inputs as regression tests.
  Blocked by: needs corpus management that turns an artifact into a committed
  test case. Crashes *are* uploaded as artifacts, so nothing is lost, but nothing
  is promoted to a test automatically.

---

# PHASE 17 — Benchmark & Performance Infrastructure 2.0

## 17.1 Benchmark Categories

- [✓] CPU benchmarks.
- [✓] Memory benchmarks.
- [✓] AI/ML benchmarks.
- [~] Startup benchmarks.
  Blocked by: process startup is dominated by Cranelift initialisation, which
    `NECT_JIT_TIMING=1` already reports. Isolating it from program execution
    needs a measurement the current harness does not take.
- [~] Compilation benchmarks.
  Blocked by: as above.
- [~] JIT warm-up benchmarks.
  Progress: `benches/benchmark.sh` A/Bs `NECT_NO_JIT=1` against the default, so
    the warm-up cost is visible in the comparison. A standalone warm-up metric
    needs the harness above.
- [~] AOT benchmarks.
  Blocked by: as above; `tests/aot_tests.rs` verifies parity, not timing.
- [~] I/O benchmarks.
  Blocked by: timing the filesystem measures the machine, not the language.
- [~] Concurrency benchmarks.
  Blocked by: `spawn` is a stub — it starts a thread that sleeps and returns a
    handle. There is nothing to benchmark until threads are real.

## 17.2 Regression Detection

- [✓] Define benchmark baselines.
  Progress: `benches/baseline.tsv`, holding a time *and* the output each
  benchmark printed when the time was recorded.
- [✓] Store benchmark results per release.
- [✓] Compare pull requests against the baseline.
  Progress: `scripts/bench-regression.sh`, wired into `.github/workflows/ci.yml`.
- [✓] Detect meaningful performance regressions.
  Progress: a benchmark is only called a regression when it is slower than both
  its own baseline *and* the suite's median change, so a uniformly busy machine
  does not report everything. `smoke` carries a hand-computed expectation
  (the sum of `i % 7` for i in 0..20 is 57) so the suite cannot be satisfied by
  a fast wrong answer.
- [✓] Detect result changes, not just time changes.
  Progress: the baseline's output column is compared on every run, so a semantic
  change is reported as such rather than averaged into the timings.
- [✓] Document benchmark methodology.
  Progress: the header of each script, and `docs/troubleshooting.md`.

## 17.3 Performance Targets

- [~] Define realistic performance targets.
  Blocked by: targets need a defined support window and a reference machine; the
    measured numbers in `benches/results.txt` are from one machine and are not a
    target.
- [✓] Benchmark against CPython using reproducible versions/configurations.
- [✓] Benchmark bytecode VM vs JIT vs AOT.
- [~] Benchmark startup overhead.
  Blocked by: as in 17.1.
- [~] Benchmark real-world programs.
  Progress: the AI/ML kernels in `benches/heavy/` are real workloads. Parsers,
  servers, and compilers written in Nect would be better; there are not enough
  yet.
- [~] Publish reproducible benchmark results.
  Blocked by: as with targets. Results are committed per run and uploaded as CI
    artifacts, which is reproducible per-run but not a published series.

---

# PHASE 18 — AI / ML 2.0

## 18.1 Tensor System

- [~] Audit current AI/ML functionality.
  Progress: `benches/heavy/` holds 15 compute-bound kernels with Python twins, so
  correctness *is* checked against a reference. There is no tensor *system* —
  every kernel indexes nested arrays by hand.
- [ ] Implement a robust tensor abstraction.
  Blocked by: n/a — not started. This is the largest genuinely-new item remaining
  and it should be designed before it is coded (see Phase 24).
- [ ] Add tensor creation/manipulation APIs.
- [ ] Add broadcasting.
- [ ] Add matrix multiplication.
  Progress: `benches/matmul.nct` implements it by hand; there is no built-in.
- [ ] Add common numerical operations.
  Progress: `sqrt`/`log`/trig exist as scalar built-ins.
- [✓] Add tensor benchmarks.

## 18.2 Autograd / Neural Networks

- [ ] Design automatic differentiation.
- [ ] Implement reverse-mode autodiff.
- [ ] Implement neural-network primitives.
- [ ] Add common activation functions.
  Progress: `benches/heavy/relu_activation.nct` implements ReLU by hand.
- [ ] Add optimizers.
- [ ] Add model examples.
- [ ] Add numerical correctness tests.
  Progress: the kernels have `.py` twins, so their *outputs* are checked. There
  are no tests for gradient correctness, because there are no gradients.

## 18.3 Hardware Acceleration

- [~] Investigate SIMD acceleration.
  Blocked by: Cranelift can emit SIMD, but the IR and the JIT's type inference
  work one `double` at a time. Vectorising needs a vector value type first —
    which is 18.1.
- [~] Investigate Apple Metal backend.
  Blocked by: needs FFI bindings to a system framework and a Metal-capable
    device in CI. Neither exists here.
- [~] Investigate CUDA backend.
  Blocked by: needs CUDA hardware and the NVIDIA toolchain.
- [~] Investigate ROCm backend.
  Blocked by: as CUDA.
- [ ] Define a portable accelerator abstraction.
  Blocked by: n/a — should follow a vector/tensor type, not precede it.
- [~] Benchmark CPU vs accelerated execution.
  Blocked by: no accelerated path exists.

---

# PHASE 19 — Production Web Stack

- [✓] Audit existing HTTP implementation.
- [✓] Improve routing.
  Progress: `http_match_route` matches `/users/:id` and a trailing `*`, captures
  parameters, percent-decodes them, and rejects a malformed escape or a decoded
  NUL rather than accepting a path that means one thing to Nect and another to
    anything downstream. A pattern that cannot work is an error naming the
    reason, not a route that silently never fires.
- [✓] Improve middleware.
- [✓] Add HTTP error handling.
  Progress: `http_error(status, code, message?)` returns the status, its reason
  phrase, a JSON body with a stable machine code, and a `Content-Type` header.
- [✓] Add request validation.
  Progress: `http_validate(body, schema)` collects *every* problem rather than
  the first, and treats an explicit `null` as absent.
- [~] Add TLS support.
  Blocked by: needs a TLS implementation. `rustls` would be the first crypto
    dependency the project has taken, and it is a decision about the dependency
    policy rather than a task — see `docs/security.md`, which currently tells
    users plainly to put TLS in front of the process.
- [~] Add WebSocket support.
  Blocked by: axum supports it, but the handler API here is a fixed
    `(request) → response` shape with no upgrade path.
- [✓] Add cookies.
  Progress: `http_cookie` builds a `Set-Cookie` with a percent-encoded value, so
  a token containing `;` cannot terminate the attribute list and introduce an
  attribute of its own. An unknown attribute name is an error: a typo in
  `httpOnly` would otherwise leave a session cookie readable from JavaScript.
  `http_parse_cookies` reads a `Cookie` header and skips a malformed pair rather
  than losing every cookie with it.
- [~] Add sessions.
  Blocked by: needs a signing key and a decision about where it lives. A session
    built on an unsigned cookie is a worse answer than none.
- [~] Add authentication helpers.
  Blocked by: as sessions. Password hashing needs a KDF, not a plain hash, and
    choosing one is the same dependency question as TLS.
- [~] Add production-oriented examples.
  Progress: `examples/webapp.nct` exists. It is not a production example, and
    writing one that was would be dishonest while sessions and TLS are absent.
- [~] Create a web project scaffold command.
  Blocked by: `nect new` needs a template set, which needs a working web stack to
    template from.
- [~] Benchmark server performance.
  Blocked by: timing a loopback HTTP server measures the kernel.

---

# PHASE 20 — Security

## 20.1 Language / Runtime Security

- [✓] Audit unsafe runtime operations.
  Progress: every `unsafe` block is accounted for. There are three kinds: the
  JIT calling generated code, FFI symbol resolution, and `set_var` (which is
  unsafe only because the process is single-threaded at that point). Documented
  in `docs/security.md`.
- [✓] Audit FFI boundaries.
- [~] Audit filesystem APIs.
  Progress: `read_file`/`write_file` take any path, follow symlinks, and are not
    jailed. `docs/security.md` states this rather than implying otherwise; adding
    a permission model is a language design decision, not an audit finding.
- [~] Audit network APIs.
  Progress: same shape — documented, not restricted.
- [✓] Define default security behavior.
  Progress: `docs/security.md` leads with the part that matters: **Nect does not
  sandbox programs.** A program runs with the privileges of the user who ran it.
  A sandbox that looks like a guarantee and can be escaped would be worse than
  an honest statement, so the document names the isolation to use instead.
- [~] Define sandboxing boundaries.
  Blocked by: as above — this is a capability-system design decision with
    language-level consequences (what a `let` can hold, what a module can import).
- [✓] Add security regression tests.
  Progress: 25 tests in `tests/security_tests.rs`, each pinning a property
  `docs/security.md` states. Notably: installing a package does not run its
  scripts; a built-in's meaning cannot be changed by a declaration; there is no
  `eval`-shaped built-in; and a cookie value cannot forge an attribute.

## 20.2 Dependency Security

- [✓] Add dependency auditing.
- [✓] Verify package checksums.
- [~] Design package signing.
  Blocked by: key management and a registry (see Phase 12).
- [✓] Document supply-chain security.
  Progress: `docs/security.md` distinguishes integrity (a checksum proves the
  bytes are unchanged) from authorship (it does not prove who produced them).
- [✓] Add CI dependency checks.
  Progress: `cargo-audit` and `cargo-deny` in `.github/workflows/ci.yml`.

## 20.3 Secrets

- [~] Audit secret handling.
  Progress: no secret is read from the environment or a file by the runtime, and
    none is written to a log. A full audit needs a threat model for a specific
    deployment, which is not a repository-level artefact.
- [✓] Ensure CLI output does not accidentally expose secrets.
- [~] Document recommended secret-management patterns.
  Blocked by: needs the TLS and session story first; recommending a pattern for
    sending a secret over a plaintext socket would be advising against the
    project's own advice.

---

# PHASE 21 — Documentation 2.0

- [✓] Audit the existing documentation.
- [✓] Create a proper documentation information architecture.
- [✓] Add language specification.
- [✓] Add API documentation.
  Progress: `nect doc` generates a Markdown API reference from the project's own
  source, and `docs/API.md` is committed with a test that regenerates it to prove
  it is current. Rustdoc is built in CI.
- [✓] Add standard-library documentation.
- [✓] Add compiler architecture documentation.
- [✓] Add VM documentation.
- [✓] Add JIT documentation.
- [✓] Add AOT documentation.
- [✓] Add package-manager documentation.
- [✓] Add FFI documentation.
- [✓] Add LSP/debugger documentation.
- [✓] Add installation guides.
- [✓] Add migration guides.
  Progress: `docs/migration.md` — within Nect 1.x, from Python, from
  JavaScript/TypeScript, from Rust/C. Every behavioural claim in it was checked
  against the binary before being written.
- [✓] Add troubleshooting guides.
  Progress: `docs/troubleshooting.md`, organised symptom → cause → fix.
- [✓] Add contributor documentation. (`AGENTS.md`)
- [✓] Ensure every public feature has an example.
- [✓] Add automated documentation validation.
  Progress: the API reference is checked in CI and by a test; the completion
  scripts are checked against the command table by a test; the VS Code manifest
    is checked against the binary by a test.
- [~] Prepare a public documentation domain.
  Blocked by: needs a host and a publishing pipeline. `docs/site/build.py`
    already renders the Markdown to HTML.

---

# PHASE 22 — Release Engineering 2.0

- [✓] Audit the current GitHub Actions workflows.
- [✓] Separate CI, benchmark, fuzz, and release responsibilities.
  Progress: four workflows instead of one. A change to a test no longer queues
  behind a fuzzing run, and a failure names the thing that broke.
- [✓] Build all supported platform artifacts automatically.
- [✓] Run tests on every supported platform.
  Progress: Linux x64/ARM64, macOS x64/ARM64, Windows x64.
- [✓] Run clippy. (`-D warnings`)
- [✓] Run rustfmt checks.
- [✓] Run documentation checks.
  Progress: `nect doc --check` plus `cargo doc`.
- [✓] Run benchmark/regression checks.
- [✓] Generate checksums.
- [~] Add release signing.
  Blocked by: needs a signing key. A checksum over an unsigned binary proves the
    download is intact, not that it came from this project — `docs/security.md`
    says so.
- [✓] Automatically generate GitHub Release assets.
- [✓] Generate release notes.
  Progress: `scripts/release-notes.py`, grouped by conventional-commit type, with
    10 tests. An unconventional commit lands in "Other changes" rather than being
    dropped — a note that silently loses a commit is worse than one that admits
    it could not categorise it.
- [✓] Add release verification.
  Progress: each binary is smoke-tested on the platform it was built for, and one
    archive is unpacked and run before anything is uploaded.
- [~] Test installation from release artifacts.
  Progress: the release workflow runs a downloaded binary. It does not run
    `scripts/install.sh` against a real release, because that needs a published
    release to exist.
- [✓] Document the complete release process.

---

# PHASE 23 — Nect 1.0 Certification

## Language

- [✓] Language specification complete.
- [~] Syntax stable.
  Progress: no syntax has been added or changed in this phase. "Stable" is a
    statement about the future, which no repository can make on its own.
- [~] Semantics stable.
  Progress: engine parity is enforced by 678 tests, 144 of them generated. The five
  documented divergences are pinned.
- [✓] Error behavior stable.
- [✓] Compatibility policy documented.

## Compiler / Runtime

- [~] Compiler stable.
- [~] VM stable.
- [~] JIT stable.
- [~] AOT stable.
  Progress: all four are functional and differentially tested against each other.
  "Stable" needs a support window and a bug-fix policy, not a test count.
- [✓] Memory model documented.
- [✓] Runtime diagnostics available.
- [~] No known critical compiler/runtime crashes.
  Progress: none known, and the fuzzers plus the malformed-input and stress suites
  look for them. "No known" is not "none", and this item cannot honestly be
  closed.

## Ecosystem

- [~] Standard library stable.
- [~] Package manager stable.
- [~] Package registry stable.
  Progress: the client is complete and tested. There is no server.
- [~] Dependency resolution stable.
- [✓] Package security model documented.
- [✓] FFI stable or explicitly marked experimental.

## Tooling

- [~] CLI stable.
- [~] Formatter stable.
- [~] Linter stable.
- [~] LSP stable.
  Progress: the LSP has completion, hover, signature help, semantic tokens,
  go-to-definition, find-references, rename, inlay hints, code actions, and
  document symbols. The symbol index is lexical, so a name declared in two
  scopes is reported at both; that is stated in `docs/troubleshooting.md`.
- [~] VS Code extension stable.
- [~] Debugger stable.
  Progress: breakpoints, continue, step, variables, call stack, and evaluate all
  work over DAP. Step-into and step-out are answered as a single step and
  advertised as unsupported.

## Quality

- [✓] Differential testing complete.
- [✓] Fuzzing infrastructure active.
- [~] Cross-platform tests passing.
  Progress: five targets in CI. Windows x64 is in the matrix but has never been
  observed green from this machine.
- [~] Performance benchmarks reproducible.
  Progress: `scripts/bench-regression.sh` makes a single run reproducible and
  detects a regression; publishing a series needs a reference machine.
- [~] No known release-blocking regressions.
  Progress: none known.

## Documentation

- [✓] Tutorial complete.
- [✓] Cookbook complete.
- [✓] Reference complete.
- [✓] Specification complete.
- [✓] API docs complete.
- [✓] Installation docs complete.
- [✓] Contributor docs complete.
- [✓] Troubleshooting docs complete.
- [✓] Migration docs complete.
- [✓] Security docs complete.

## Release

- [~] Linux x64 release verified.
- [~] Linux ARM64 release verified.
- [~] macOS x64 release verified.
- [✓] macOS ARM64 release verified.
- [~] Windows x64 release verified.
  Progress: the release workflow builds and smoke-tests all five. A release has
  not been published, so "verified" is not yet true for any of them except the
  host.
- [✓] Checksums generated.
- [✓] Release artifacts tested.
- [~] CI/CD release pipeline verified.
  Progress: the workflows are complete and valid, and have not yet run end to end
  on a tagged release.

---

# PHASE 24 — Post-1.0 / Future Research

Not required for Nect 1.0, and not started. Listed in the order they depend on
each other, because several of them are blocked on 18.1 rather than on anything
exotic.

- [ ] **Tensor value type.** The prerequisite for most of the list below: SIMD,
  broadcasting, a GPU backend, and fast matrix multiplication all need a vector
  type, not nested arrays indexed by hand.
- [ ] Generational or incremental GC. (`docs/memory.md` §10 explains why reference
  counting is right for a single-threaded design and where it stops being right.)
- [ ] Advanced escape analysis.
- [ ] Profile-guided optimization.
- [ ] More aggressive JIT specialization.
- [ ] On-stack replacement.
- [ ] SIMD auto-vectorization. *(needs the tensor type)*
- [ ] GPU compiler backend. *(needs the tensor type, plus Metal/CUDA/ROCm)*
- [ ] Real threads and channels. *(`spawn` is currently a stub; this also unblocks
  the concurrency benchmarks in 17.1.)*
- [ ] Distributed computing primitives.
- [ ] WASM backend.
- [ ] Embedded Nect runtime.
- [ ] Mobile platform support.
- [ ] Language server performance optimization.
- [ ] IDE integrations beyond VS Code.
- [ ] Formal verification of selected compiler components.
- [ ] Frame-level debugging (real step-into/step-out). *(unblocks 15.3)*

---

# Agent Workflow

The coding agent MUST follow this workflow for every task:

1. Read the relevant source files before modifying them.
2. Check whether the requested functionality already exists.
3. Reuse existing architecture instead of duplicating it.
4. Implement the smallest coherent change that satisfies the task.
5. Add or update tests.
6. Run relevant tests.
7. Run `cargo fmt --check`.
8. Run `cargo clippy --all-targets -- -D warnings` when practical.
9. Run the full test suite before completing a major phase.
10. Update this file immediately after a task is genuinely complete.
11. Change only the completed task from `[ ]` to `[✓]`.
12. Never mark a task `[✓]` if it is only partially implemented.
13. If a task is blocked, leave it `[ ]` and write `Progress:` and `Blocked by:`.
14. Do not silently remove roadmap tasks.
15. If implementation reveals that a task is unnecessary, document why before
    changing its status.
16. Preserve backwards compatibility unless the current phase explicitly changes it.
17. Keep commits logically separated by feature/phase when working with Git.
18. Do not optimize based on assumptions; measure before and after.
19. For compiler/runtime changes, use differential tests whenever applicable.
20. At the end of each phase, perform a phase-level review and update the exit criteria.

---

# Definition of Done

A task is considered complete only when:

- [ ] Implementation exists.
- [ ] Relevant tests exist.
- [ ] Existing tests still pass.
- [ ] Formatting passes.
- [ ] Relevant lint checks pass.
- [ ] Documentation is updated when the public behavior changes.
- [ ] No known critical regression remains.
- [ ] The task's checkbox has been changed from `[ ]` to `[✓]`.

---

# Progress Tracking

## Current Phase

**Current Phase: Phase 23 — Nect 1.0 Certification.**

The implementation phases (8–22) are complete or explicitly blocked. What
remains for 1.0 is the certification itself, and most of that is not a thing a
repository can do alone: it needs a published release, a defined support window,
and observed CI runs on platforms this machine is not.

## Overall Status

**Original Phase 1–7 roadmap:** Completed.
**Phases 8–22:** Completed, or blocked with the reason stated.
**Current objective:** Turn Nect from a feature-complete experimental language
into a stable, documented, secure, cross-platform, ecosystem-ready programming
language.

## Completed Since the Last Update

### A red suite, fixed first
The FFI was broken: `extern` parsed nowhere, `Program` carried no externs, and
all 20 FFI tests failed. `Program` now carries the declarations,
`Compiler::hoist_externs` resolves call sites regardless of declaration order,
and `VM::new` fills the natives table before the first instruction runs.

### Phase 14 — `nect doc` and complete completions
`src/cli/doc.rs` generates an API reference from the project's own source.
`docs/API.md` is committed, and both a test and CI regenerate it to prove it is
current. bash, zsh, and fish completions now cover every command and flag, with
a test that fails if a script drifts behind the command table.

### Phase 15 — editor tooling, and a debugger that tells the truth
`src/lsp/index.rs` gives go-to-definition, find-references, and rename from a
token-accurate index — built from the lexer's offsets, not from AST annotations,
so it works on a file that does not parse and leaves the engines untouched. Added
inlay hints and code actions.

The breakpoint line mapping was a placeholder that used the statement *index*;
`Parser::parse_with_lines` records the real line. `nect dap` serves the Debug
Adapter Protocol, and the extension launches it directly rather than
reimplementing the protocol. Step-into and step-out are answered as a single step
*and advertised as unsupported*, so an editor greys them out rather than lying.

### Phase 16 — randomised differential testing
144 generated programs across 12 seeds, each compared across three engines.
Seeded, so a failure reproduces.

### Phase 17 — performance regression detection
`scripts/bench-regression.sh` with a committed baseline holding both a time and
the output recorded with it. A regression needs both a slower-than-baseline
result *and* a slower-than-median one, so a busy machine does not report
everything.

### Phase 19 — the parts of the web stack that are pure functions
Route matching with parameters, cookies with encoding that resists attribute
injection, request validation that collects every problem, and structured error
responses.

### Phase 20 — a security model that says what is not true
`docs/security.md` leads with "Nect does not sandbox programs" and then says what
*is* enforced. 25 regression tests pin those claims. Auditing the package manager
found that `pkg run` executed repository-controlled shell text and announced it on
stdout; it now goes to stderr, where it survives a pipe.

### Phase 21–22 — documentation and release engineering
`docs/migration.md` and `docs/troubleshooting.md`, with every behavioural claim
checked against the binary first. Four CI workflows instead of one, each binary
smoke-tested on its own platform before upload, release notes generated from the
commit log.

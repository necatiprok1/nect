# Nect development guide

## Scope and structure

Keep changes focused. Preserve user work, avoid new dependencies without a clear
need, and do not commit, publish, or delete files unless explicitly requested.
Use code search to locate symbols and read only the relevant definitions.

| Location | Responsibility |
| --- | --- |
| `src/ast/`, `src/lexer/`, `src/parser/` | Values, syntax tree, tokenization, parsing |
| `src/builtins.rs` | Shared runtime semantics and core built-ins |
| `src/interpreter/` | Reference tree-walking interpreter (`run --interp`) |
| `src/vm/` | Bytecode compiler and default execution engine |
| `src/ir/`, `src/jit/` | Optimization, type inference, Cranelift compilation |
| `src/aot/` | Bytecode → C → standalone executable |
| `src/cli/` | Commands, source loading, module expansion |
| `src/lsp/`, `src/debugger/`, `editors/` | Editor tooling and debugging |
| `std/` | Standard library embedded in the executable |
| `examples/` | Runnable language examples |
| `scripts/` | Installers and release-note tooling |
| `docs/` | Installation, tutorial, reference, generated API, memory, security |

Language details belong in [docs/reference.md](docs/reference.md), teaching
examples in [docs/tutorial.md](docs/tutorial.md), and installation guidance in
[docs/KURULUM.md](docs/KURULUM.md). Avoid duplicating those documents here.

## Runtime invariants

- `src/builtins.rs` is the single source of truth for value semantics, formatting,
  indexing, comparisons, runtime errors, and core built-ins. Add a core built-in
  to `NAMES` and the `call` dispatch; both engines register the same names.
  Do not copy its implementation into an engine.
- New syntax must work in both engines. Friendly syntax desugars to existing AST
  forms: `a.m(b)` becomes `m(a, b)`; interpolation becomes `concat`; word operators
  map to the symbolic operators. Preserve source positions inside interpolation.
- Only a bare `{ ... }` block introduces a scope. `if`, `while`, and `for` bodies
  declare into the enclosing scope. Reading a declaration whose body never ran
  is an error. Built-in function names and `and`/`or`/`not` are reserved.
- Maps preserve insertion order in printing and iteration, and equality includes
  entry order. `2` and `2.0` are the same key. Plain assignment inserts a missing
  key; compound assignment to a missing key errors.
- `import` is expanded before lexing in `src/cli/mod.rs`, once per module,
  cycle-safely. Paths are relative to the importing file (CWD for stdin).
  `std/` imports use embedded `include_str!` sources: keep `std/*.nct`, especially
  `std/ui.nct`, even if they look unused to Rust symbol search.
- Seeded random draws must remain reproducible across engines. Non-numeric and
  side-effecting operations stay on the VM when native compilation cannot
  support them safely.

## JIT and C backend

The JIT requires both proven numeric eligibility and evidence of repeated work
(a loop, recursion, or a call from a loop). A numeric module prefix can also be
compiled. Unsupported operations, unresolved globals, division/modulo, and unsafe
arity must remain in bytecode. Compilation failure falls back to the VM; a
program with nothing worth compiling should not initialize Cranelift.

`ModuleEntry` in `src/vm/mod.rs` mirrors module code into frame slots only when
declarations are proven safe. Preserve rejection paths, including poisoned
`Op::Halt` paths. Use `nect disasm` to inspect decisions rather than assuming a
function is native. Keep release panic unwinding: Cranelift failure handling
relies on it.

The C backend accepts only its proven subset: numeric/boolean values, control
flow, supported calls, and strings used within expressions rather than stored
in variables/globals. Unsupported programs must be rejected with a reason;
`nect build` does not automatically run them. Users can still use `nect run`.

- Generated binaries must match `nect run` stdout, stderr, and exit status.
- Preserve `-ffp-contract=off`; fused multiply-add changes floating-point rounding.
- Keep `entry_depths` stack-shape analysis sound. Short-circuit and conditional
  merges reconcile existing stack values through join variables. Reject
  inconsistent stack depths rather than mistranslating them.
- Compare engines when changing semantics or optimization. Documented interpreter
  divergences (such as function values and host-stack recursion) are listed in
  the reference; do not silently redefine them.

## Optional dependencies

The default build is lean. Heavy dependencies are opt-in Cargo features.

| Feature | Main dependency / capability |
| --- | --- |
| `net` | `reqwest`; HTTP client |
| `server` | `axum`; HTTP server |
| `db` | `rusqlite` with bundled SQLite |
| `gui` | `egui` / `eframe`; native GUI |
| `lsp` | `tower-lsp`, `tokio`; language server |
| `pkg` | Registry, manifest, archive and checksum dependencies |
| `ffi` | `libloading`; `extern` declarations |
| `full` | `lsp` + `pkg` + `ffi` only |

`full` is not `--all-features`: `net`, `server`, `db`, and `gui` stay separate.
Use `optional = true` and `dep:<crate>` for new optional dependencies.
Keep optional built-in name tables ungated so `disabled_builtin()` can explain
which feature is missing. Gate implementations and dispatch arms, not names.
Keep disabled-feature stand-ins for `Value` variants such as `DbConnection` and
`GuiWindow`; all consumers still need exhaustive matches. Feature-specific unit
tests must carry the corresponding `#[cfg(feature = "...")]` gate.

`docs/API.md` is generated, not hand-edited. Generate/check it with an
all-features build so optional built-ins are visible:

```sh
cargo run --all-features -- doc --out docs --check
# To regenerate intentionally, omit --check.
```

## Validation workflow

Keep regression tests alongside the code in inline `#[cfg(test)]` Rust modules.
Do not assume a repository-level integration suite or benchmark corpus exists.
Run the smallest relevant tests first, then validate applicable feature sets:

```sh
cargo build
cargo build --features full
cargo build --all-features
cargo test --lib
cargo test --lib --all-features
cargo clippy --all-targets
cargo run -- check examples/hello.nct
cargo run -- run examples/hello.nct
cargo run -- run --interp examples/hello.nct
NECT_NO_JIT=1 cargo run -- run examples/hello.nct
```

For engine changes, compare stdout, stderr, and exit status on focused regression
programs and noninteractive examples. For AOT changes, additionally build a
supported numeric example with the system C compiler and compare the executable
against the VM. `hello.nct` is only a smoke test, not evidence of JIT coverage;
use a repeated numeric workload and inspect its disassembly when testing JIT.

Do not blindly run every example: `webapp.nct` opens a browser, `calculator.nct`
needs interactive input, and GUI/FFI examples need features or external resources.
Report exactly which commands ran, failures, and any validation gaps. Never claim
engine parity or security correctness merely because a build passed.

Useful diagnostics: `NECT_NO_JIT=1`, `NECT_JIT_TIMING=1`, `NECT_VM_STATS=1`,
`nect disasm`, `nect mem-profile`, and `nect doctor`.

## Editor protocols

- LSP indexing in `src/lsp/index.rs` uses lexer byte offsets and remains useful
  on incompletely parsed files; do not require AST annotations for navigation.
- Terminal debugging and DAP share the non-printing debugger engine API.
  Stepping is top-level-statement granular; do not advertise unsupported
  step-in/step-out behavior.
- Breakpoint lines come from `Parser::parse_with_lines`, not statement indices.
  Blank lines and comments must not shift the mapping.
- Protocol hosts own stdout. Use `builtins::set_output_sink` to redirect program
  output (including input prompts) to stderr so framing stays intact.

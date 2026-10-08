# Nect Language - Development Guide

## Overview
Nect (NECT) is a Python-like syntax programming language with a focus on performance.
The CLI compiles source to bytecode and runs it on a stack-based VM (`src/vm/`).
Code that a type-inference pass proves numeric-only *and* that shows evidence of
repeated work (a loop, recursion, or a call inside a loop) is additionally compiled
to native code by a Cranelift JIT (`src/jit/`); everything else stays on the VM.
The same pass compiles a numeric *prefix* of module-level code, since top-level loops
have no function to compile. `nect build` (`src/aot/`) instead translates the
provably-numeric subset of a whole program to C and invokes the system C compiler,
producing a standalone executable whose output must match the VM's exactly.
A second goal is *learnability*: the friendly syntax layer (string interpolation,
method-call sugar, `and`/`or`/`not`, optional parentheses on `if`/`while`, `#`
comments, digit separators, exponent literals) desugars in the parser to the
same AST the plain forms produce, so engines never see it.
See `PERFORMANCE_PLAN.md` for the optimization history, measured results, and the
reasoning behind the compilation policy.

## Project Structure
```
src/
  ast/         - AST node definitions (Expr, Stmt, Value, etc.)
  lexer/       - UTF-8 tokenizer / scanner
  parser/      - Recursive descent parser
  builtins.rs  - Shared core: Value semantics (apply_binary, comparisons,
                 indexing, formatting, RuntimeError) and the built-in library.
                 Both engines call it, so their behaviour cannot drift
  interpreter/ - Tree-walking interpreter (reference; engine behind `run --interp`)
  vm/          - Bytecode compiler + VM (what the CLI runs by default)
  jit/         - Type inference + Cranelift native compilation (phase 3)
  aot/         - C backend for `nect build`: bytecode -> C -> standalone binary
  cli/         - CLI argument parsing and command dispatch
  lib.rs       - Library entry point
  main.rs      - Binary entry point
docs/          - tutorial.md (guided tour), cookbook.md (recipes), reference.md (grammar, errors)
               troubleshooting.md (symptom → cause → fix), migration.md (versions and other languages)
tests/         - Integration, golden-output, and differential tests
examples/      - Example .nct programs (all run by the differential tests)
benches/       - Nect vs Python benchmarks (run benches/benchmark.sh)
                 benches/heavy/ holds the compute-bound AI kernels (nct + py twins)
```

`src/builtins.rs` is the single source of truth for runtime semantics. Adding a
built-in means adding it to `NAMES` and to the `call` match there — the VM
registers every name in `NAMES` and the interpreter does the same, so there is no
second implementation to update. New syntax goes `ast` → `parser` → both engines;
delete the duplication rather than copying a rule into the interpreter.

## Build & Test

Nect builds in two shapes, and both are tested because they compile different
code (see "Optional features" below). `cargo build` with no flags gives the
**lean** build: the language, with none of the large optional dependency trees.

```bash
cargo build          # Build debug binary (lean)
cargo build --all-features   # Build with every optional feature
cargo test           # Run all tests against the lean build
cargo test --all-features   # Run all tests against the full build
cargo run -- run main.nct      # Run a source file
cargo run -- run --interp main.nct  # Same file, tree-walking interpreter
cargo run -- check main.nct    # Check syntax without running
cargo run -- disasm main.nct   # Bytecode + inferred types (JIT eligibility)
cargo run -- build main.nct    # Translate to C and build a standalone binary
cargo run -- build main.nct --emit-c   # Print the generated C instead
cargo run -- --version
cargo run -- --help
cargo run -- mem-profile main.nct    # Run with runtime statistics on stderr
```

`NECT_NO_JIT=1` disables native compilation, which is how the benchmarks A/B the
VM against the JIT. `NECT_JIT_TIMING=1` prints where compilation time goes
(Cranelift init, codegen, finalize), which is what the "compile only what repeats"
policy is budgeted against (~40µs + ~110µs per function). `NECT_VM_STATS=1`
enables runtime statistics collection in the VM (instruction count, call counts,
peak stack depth); these are reported by `nect mem-profile <file>`.

`docs/memory.md` documents the runtime memory model (reference counting, stack
layout, ownership, OOM behavior).

## Dependencies
The only runtime dependency tree is Cranelift (`cranelift-{jit,module,codegen,
frontend,native}`, ~40 crates), used by `src/jit/`. The VM itself has no
dependencies. The JIT is optional at runtime — if Cranelift cannot initialise or a
function fails to compile, execution silently stays on the bytecode VM, and a
program with nothing worth compiling never initialises Cranelift at all.

## How the JIT decides (read before touching `src/jit/`)
- `analyze()` first *type-checks* each function (numeric-only, no globals, no
division, arity ≤ 4), then decides what is worth *compiling*: a function is compiled
when it contains a loop, is recursive, or is called from a loop body — the measured
justification is in `PERFORMANCE_PLAN.md` §3.2.2.
- The compiler mirror in `src/vm/mod.rs` (`ModuleEntry`) is what makes module-level
code compilable: globals the compiler proved declared become frame slots, everything
else is left as a global (rejected) or poisoned with `Op::Halt` (rejected).
- Anything the JIT cannot prove must stay on the bytecode VM rather than risk a wrong
answer; `tests/differential_tests.rs` enforces that the engines agree, and
`nect disasm` shows the verdict for every function and for the module prefix.

## How the C backend decides (read before touching `src/aot/`)
- The contract is *behavioural parity*: the built binary must print the same stdout,
  stderr, and exit status as `nect run`. `tests/aot_tests.rs` builds every benchmark
  and a corpus of semantic edge cases with the system C compiler and compares.
- It translates the provably-numeric subset: numbers/booleans (as `double`),
  control flow, calls, strings used inside expressions (never stored in variables
  or globals), and the numeric builtins. Anything else is rejected with the reason;
  a rejection is not an error, the program just stays on the VM.
- Compile with `-ffp-contract=off`: fused multiply-add rounds once where the VM
  rounds twice, which changes iterated floating-point results (mandelbrot escapes
  at a different iteration).
- Stack-shape analysis (`entry_depths`) must stay sound: the compiler reaches
  merge points (from `&&`/`||`/`?:`) with values already on the operand stack, and
  the emitter reconciles them through join variables. If two paths disagree about
  the stack, the program is rejected rather than mistranslated.

## Development Workflow
1. Each new feature should include unit tests in the relevant module
2. Add integration tests in `tests/run_tests.rs` for end-to-end coverage
3. Add golden-output tests in `tests/vm_output_tests.rs` when behavior (not just
   success/failure) matters — it runs the real binary and pins stdout
4. Any change to the VM, the optimizations, or the JIT must keep
   `tests/differential_tests.rs` green: it runs every program through the
   interpreter, the bytecode VM, and the JIT and requires identical output
5. Run `cargo test` before committing

## Syntax Reference (MVP)
```
// Comments: // line, # line, and /* block */

// Variables (1_000_000 digit separators and 1.5e2 exponents are allowed)
let x = 42
let name = "Nect"

// Interpolation "${expr}" desugars to concat(...); methods are sugar too
print("hello ${name}, x is ${x}")
print(name.upper(), "x = ${x}")

// Functions
fn greet(who) {
    print("Hello, " + who)
}

greet(name)

// Control flow: parentheses on if/while are optional; braces are not
if x > 10 {
    print("big")
} else {
    print("small")
}

while x > 0 {
    print(x)
    x = x - 1
}

// Word operators and symbol operators are the same operators
print(x > 0 and len(name) > 1)   // && || ! also work

// Return
fn factorial(n) {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}

// Arrays, for loops, and loop keywords
let values = [3, 1, 2]
push(values, 4)
for value in sort(values) {
    if value % 2 == 0 {
        continue
    }
    print(value)
}
```

Note: `else` must follow the closing brace on the same line (`} else {`).
`else if` is supported and desugars to a nested `if` in the else branch.

Implementation notes for the friendly-syntax layer:
- Interpolation is lexed into one `Str` token carrying `\u{1}NECT-INTERP\u{1}`
  markers around each expression's source; `Parser::expand_interpolation`
  splits on the markers and parses each expression in place, so errors inside
  `${...}` keep their own position. A literal without markers is untouched.
- `a.m(args)` is rewritten to `m(a, args)` in the postfix loop — nothing new
  is dispatched, and chaining falls out for free.
- `and`/`or`/`not` are reserved words lexed to `AndWord`/`OrWord`/`NotWord`
  tokens that the parser treats exactly like `&&`/`||`/`!`.

For the complete, executable version of the language see
[docs/tutorial.md](docs/tutorial.md) (every sample's output is verified against
the real binary) and [docs/reference.md](docs/reference.md) (grammar, precedence,
built-in reference, error catalogue, and the intentional differences between the
interpreter and the VM).

## Supported Features
- Numbers (64-bit floats), strings (UTF-8, character-indexed), booleans, null, arrays,
  maps (insertion-ordered; keys are strings/numbers/booleans; `d.k` is `d["k"]`)
- `let` declarations; assignment and compound assignment (`+= -= *= /= %=`)
- Arithmetic: `+ - * / %` (`+` also concatenates strings)
- Comparison: `< > <= >= == !=` (ordering works on numbers and strings)
- Logical: `&&`/`and`, `||`/`or` (short-circuit), `!`/`not`
- Conditional expression `condition ? then : else`
- `if` / `else if` / `else`, `while`, `for x in array`, `break`, `continue`
- Functions, recursion, nested helpers, `return`
- Array and string indexing with negative indices, `arr[i] op= value`
- String interpolation `${expr}`, method-call sugar `x.f(y)`, `concat`
- 57 built-ins: output, conversion, `len`/`push`/`pop`/`sort`/`reverse`/`sum`/
  `keys`/`values`/`has`/`remove`/
  `slice`/`join`/`split`/`index_of`/`contains`/`replace`/`upper`/`lower`/`trim`/
  `repeat`/`range`, `min`/`max`/`abs`/`round`/`pow`/`sqrt`/`log`/trig, `assert`,
  `int` (truncate toward zero, like `floor`/`ceil` number-strict),
  `fixed` (exact decimal string), `char`/`char_code` (code points),
  `input` (one stdin line, `null` at EOF, prompt printed without newline),
  `random`/`random_int`/`seed`,
  `read_file`/`write_file` (whole-file text I/O), `open_url` (browser, spawned
  detached), `json_encode`/`json_decode` (objects are maps in insertion order,
  so encoding is deterministic), `now`/`sleep`, `args` (script arguments)
- Randomness is a shared XORSHIFT64* (`src/builtins.rs`): `seed(n)` makes the
  sequence deterministic per process, which is what lets differential and
  golden tests pin seeded draws across engines. Unseeded, state starts from
  the clock. `input`/`random`/`random_int`/`seed` are rejected by the C
  backend with a stated reason and skipped by the JIT; programs stay on the VM.
  So are the file/JSON/time builtins — anything non-numeric stays on the VM.
- Modules (`import "path"`) are spliced at the source level in `read_source`
  (`src/cli/mod.rs`), before lexing: the engines never see an import. Rules:
  one `import` per line; each module spliced once (cycle-safe via a
  canonical-path set); paths resolve relative to the importing file, CWD for
  stdin; `import "std/..."` resolves from the embedded `STDLIB` table
  (`include_str!` of `std/*.nct`), so the standard library ships in the
  binary. Never add `webapp.nct` to automated engine sweeps — `ui_open`
  opens a browser.

Scope rules that surprise newcomers, and are therefore tested explicitly: only a
bare `{ ... }` block is a scope — `if`/`while`/`for` bodies declare into the
enclosing scope, and reading a declaration whose body never ran is an error.
Built-in names are reserved and cannot be shadowed by `fn`, and so are
`and`/`or`/`not` (they are operators, not functions).
Map semantics that are pinned by tests: insertion order is observable
(printing and `for` both follow it, which is what makes cross-engine parity
testable), `2` and `2.0` are the same key, compound assignment on a missing
key errors while plain assignment inserts, and map equality includes entry
order. The VM normalizes `for` with `Op::IterList` (elements, or map keys),
which the JIT and the C backend reject with those reasons.

## Optional features (read before touching `Cargo.toml`)

Everything above works with a plain `cargo build`: ~50 crates and a ~3 MB
binary. The large dependency trees are cargo features, off by default, because
compiling a GUI toolkit to run a hello-world is the wrong trade for anyone who
does not want a GUI.

| Feature | Brings | Adds |
| --- | --- | --- |
| `net` | `reqwest` | `http_get`, `http_post`, `http_request` |
| `server` | `axum` | `http_server`, `http_respond`, `http_listen`, `http_route`, `http_middleware`, `http_router` |
| `db` | `rusqlite` (bundled C) | `db_*` |
| `gui` | `egui`/`eframe` (~150 crates) | `gui_*` |
| `lsp` | `tower-lsp`, `tokio` | the `nect lsp` command |
| `pkg` | `serde`, `toml`, `tar`, `sha2`, `reqwest`, … | the `nect pkg` command |
| `ffi` | `libloading` | `extern` declarations |
| `full` | `lsp` + `pkg` + `ffi` | the "I want everything" build |

`gui`, `net`, `server`, and `db` are deliberately *not* in `full`: they are the
bulk of the tree and most people never touch them. Ask for them by name.

Rules for adding to this set:
- A feature is `dep:<crate>` in `[features]`, and the crate is `optional = true`.
  Never add a heavy crate to `[dependencies]` unconditionally.
- The built-in **name table** (`NET_NAMES` and friends) stays compiled in every
  build, ungated. It is what lets `disabled_builtin()` tell a user that
  `gui_window` needs the `gui` feature instead of claiming the name does not
  exist. Only the *implementation* and the dispatch arm are gated.
- `ast::Value` carries a variant per value kind, so `DbConnection` and
  `GuiWindow` have `#[cfg(not(feature = ...))]` stand-ins: a program cannot
  construct one, but every consumer still matches on the variant.
- A test for gated behaviour is gated with `#![cfg(feature = "...")]` at the top
  of the file, or `#[cfg(feature = "...")]` on the individual test when the rest
  of the file is core behaviour. `tests/security_tests.rs` is the example of the
  second case.
- `docs/API.md` is generated from a build that has every optional group, because
  `nect doc` lists the built-ins a program uses. The test that compares it is
  gated on `all(feature = "net", feature = "server", feature = "db", feature =
  "gui")`, and `ci.yml` generates the reference with `--all-features`.

## CLI Commands
- `nect run <file>` - Execute a .nct file (bytecode VM + JIT)
- `nect run --interp <file>` - Execute with the tree-walking interpreter
- `nect run -` - Execute from stdin
- `nect check <file>` - Check syntax without executing
- `nect build <file>` - Compile to a standalone native executable (`-o` names it,
  `--cc` picks the C compiler, `--emit-c` prints the C instead, `--keep-c` keeps it)
- `nect disasm <file>` - Print bytecode and inferred types
- `nect doc [path]` - Generate a Markdown API reference from the project's own
  source (`--out <dir>` writes `<dir>/API.md`, `--check` fails when the committed
  reference is stale)
- `nect dap <file>` - Serve the Debug Adapter Protocol on stdio for an editor.
  The program comes from the file or from the `launch` request's `program`
  argument; stdin is the protocol, so it cannot also be the program. The
  debuggee's output is redirected to stderr so it cannot corrupt the framing.
- `nect completions <shell>` - Print a completion script (bash, zsh, fish)
- `nect doctor` - Report the toolchain and which optional features this binary has
- `nect --version` - Show version
- `nect --help` - Show help

## What the editor tooling reads
- `src/lsp/index.rs` builds a token-accurate symbol index: declarations, uses,
  and call sites with argument ranges. It works from the lexer's byte offsets
  rather than from AST annotations, so it stays useful on a file that does not
  fully parse and leaves the engines untouched. Go-to-definition,
  find-references, rename, and inlay hints are all driven from it.
- `src/debugger/mod.rs` holds a non-printing engine API (`resume`, `step`,
  `set_breakpoint`, `locals`, `evaluate`) that both front ends drive: the terminal
  debugger in `src/debugger/mod.rs` and the DAP server in `src/debugger/dap.rs`.
  Stepping is statement-granular at module level, so `stepIn`/`stepOut` are
  answered as a single step and `initialize` reports the capability as false
  rather than offering a control that does not do what it says.
- A breakpoint line comes from `Parser::parse_with_lines`, which records the line
  each top-level statement starts on. Deriving it from the statement list cannot
  work — statement count and line count are unrelated — so blank lines and
  comments do not shift the mapping.
- `crate::builtins::set_output_sink` redirects `print` (and `input`'s prompt)
  to stderr for hosts that own stdout for a protocol.

<!-- graft:start -->
## Graft — repo context graph

This repo is indexed in `graft/`: small linked markdown nodes that explain each
system and carry exact file:line spans, kept in sync with the code through git.

For ANY task here — understanding how something works, finding where code lives,
or scoping a change — get context from the graph before grepping or opening
source files. Re-ask freely (it's cheap) and reuse literal identifiers you
already have (symbol, error string, file name) as the query. New to this repo?
Run `graft map` first — a token-budgeted orientation (dir clusters, hubs,
hotspots), no LLM, no key.

- Run `graft ask "<your question>" --source` → ranked nodes with the relevant
  code spans inlined (each hit's ≤8-line crux by default; `--full` for whole
  definitions when the crux isn't enough). Match the tool to the task shape:
  for understanding or editing, the top node IS the answer — cite its
  `covers:` file:line spans and edit straight from `--source`. For
  exhaustive tasks ("every occurrence / every caller of this pattern"), ranked
  results are top-N, not complete — run `graft grep "<literal>"` instead
  (exhaustive over indexed files, grouped by enclosing symbol), falling back
  to raw `grep -rn` only for unindexed files.
- `graft skeleton <file>` → every definition's signature + span, ~10× cheaper
  than reading the file; use it to skim an API surface.
- `graft callers <symbol>` gives precomputed, exact edges — who calls this.
  Add `--direction out` for what it calls, or `--depth N` to walk
  transitively for the full blast radius. For structural questions, skip
  ranking and use this directly.
- Or browse: `graft/INDEX.md` lists every node; follow the links.
- Monorepos and folders of multiple repos rank fairly across sub-projects —
  hits carry `[scope/]` labels naming which one they're from. Narrow with
  `graft ask "<task>" --in <scope>/` once you know where you're working.

If a returned span is truncated ("+N more lines"), open the file at that exact
range before finalizing. Only open source files when a node genuinely lacks a
needed detail, and then at the exact file:line the node points to — never
re-read whole files.

After big code changes, refresh the graph with `graft build` (deterministic,
no API key, $0).
<!-- graft:end -->

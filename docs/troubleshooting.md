# Troubleshooting

Symptoms first, then the cause, then what to do about it. Every error message
here is the real one the tool prints; `docs/reference.md` §9 has the full
catalogue.

## Build and install

### `cargo build` fails with a linker error about a system library

Nect links SQLite, so it needs a C toolchain and SQLite's development headers.

| Platform | What is needed |
| --- | --- |
| Debian/Ubuntu | `build-essential libsqlite3-dev` |
| Fedora | `gcc sqlite-devel` |
| macOS | Xcode command line tools: `xcode-select --install` |
| Windows | Visual Studio Build Tools with the C++ workload |

`rusqlite` is built with its `bundled` feature, so SQLite itself is compiled from
source and no system SQLite is required — but a C compiler still is.

### `nect: command not found` after `cargo install`

The binary lands in Cargo's bin directory, which is often not on `PATH`. Find it
and add it:

```bash
cargo install --path .          # prints the install directory
export PATH="$HOME/.cargo/bin:$PATH"   # the usual location
```

`nect doctor` reports where it found the binary, the C compiler, and whether
native compilation works, so it is the quickest way to confirm an install is
sound.

### The release build is very slow

The first build compiles roughly forty Cranelift crates. Later builds reuse the
cache. `--release` additionally optimises them, so expect a few minutes the
first time and seconds afterwards.

## Running programs

### `error: undefined function 'foo'` but `fn foo` is defined above

The VM resolves calls at compile time and a call has to be to a name it can see.
The usual cause is a typo, or calling a function defined *inside another
function's body* from module level. Nested helpers work when called from within
the same function; hoist them to module level to share them.

Compare `nect run file.nct` with `nect run --interp file.nct`. The interpreter
reports `undefined variable 'foo'` where the VM reports `undefined function
'foo'`; the two wordings are a documented difference (`docs/reference.md` §13), not
a second bug.

### `error: undefined variable 'x'` inside an `if` or `while` body

Only a bare `{ ... }` block introduces a scope. An `if`/`while`/`for` body
declares into the *enclosing* scope, so a declaration inside a conditional may
never run:

```
// Prints 0 or 1, not an error: the branch always runs here.
let x = 0
if true {
    x = 1
}
print(x)

// This is the error: `y` exists only if the branch is taken.
if false {
    let y = 1
}
print(y)   // error: undefined variable 'y'
```

That is deliberate — the language has no block-local `if` scope — and it is
pinned by tests. Declare before the conditional if the value must exist
unconditionally.

### `error: cannot use 'x' as a value (it is a function)`

Built-in names are reserved. `print` and the rest cannot be shadowed by a
declaration or a parameter, and neither can `and`, `or`, and `not` — those are
operators, not functions. Pick another name.

### A number prints differently than expected

Numbers are 64-bit floats. A whole value prints without a decimal point and
anything else prints as the shortest decimal that reads back identically, which
is not always the shortest decimal with the same digits:

```
print(0.1 + 0.2)   // 0.30000000000000004
print(1 / 3)       // 0.3333333333333333
```

`fixed(n, places)` renders an exact decimal string when a specific number of
places is wanted. Floating-point results are identical across all three engines
and in `nect build` output — the C backend is compiled with `-ffp-contract=off`
precisely so a fused multiply-add cannot round once where the VM rounds twice.

### `error: modulo by zero`

`%` is a runtime operation, so a zero divisor is only detected when the
expression is evaluated. Guard it if the divisor is computed.

### `nect build` says the program is outside the translatable subset

That is not an error in the program. `nect build` translates a deliberately
narrow, provably-numeric subset to C; anything else stays on the VM. The message
names the construct:

```
error: the module body uses an array literal which the C backend cannot translate
```

The program still runs with `nect run`, which is always the answer. `nect build`
succeeds for numeric code with control flow, calls, and strings used inside
expressions.

### `nect build` output differs from `nect run`

It should not: the contract is that the built binary prints the same stdout,
stderr, and exit status. `tests/aot_tests.rs` builds every benchmark and a corpus
of edge cases and compares. If you find a difference, that is a bug worth
reporting with the program that triggers it.

## The debugger

### A breakpoint on a line does nothing

The debugger stops at the first *statement starting on or after* the line, and a
line with no statement on it never fires — blank lines, comment lines, and the
middle of a multi-line statement are all such lines. Put the breakpoint on the
`let`, `print`, or `fn` line.

### `nect dap` shows no output in the editor's debug console

The debuggee's output is deliberately sent to stderr, because stdout carries the
protocol — a single `print` landing in the message stream would desynchronise
every frame after it. Look in the editor's *Nect* debug console, which collects
the adapter's stderr.

### Stepping does not enter a function

Stepping is statement-granular at module level; there is no per-frame model to
descend into. `stepIn` and `stepOut` are answered as a single step, and
`initialize` advertises `supportsStepIn: false` and `supportsStepOut: false` so
editors can grey the controls out. See `docs/reference.md` §12.

## Formatting and linting

### `nect fmt` produces a large diff

The formatter is opinionated and deterministic: running it twice changes
nothing. A large first diff usually means the file was hand-formatted to
different rules. `nect fmt --check` exits `1` instead of rewriting, which is what
a pre-commit hook wants.

### `nect lint` reports `unused variable` for a variable that is used

Check the spelling. Lint findings carry a rule id (`unused_variable`,
`unused_parameters`, `shadowing`, `dead_code`, `invalid_break`,
`invalid_continue`, `invalid_return`) so a specific rule can be turned off:

```bash
nect lint --no-unused file.nct
```

A parameter whose name starts with `_` is treated as deliberately unused.

## Editor support

### The language server does not start

Check the setting `nect.lsp.path` points at the `nect` binary, and that
`nect doctor` passes. The server is started as `nect lsp` over stdio; the
*Nect* output channel in the editor shows its stderr.

### Go-to-definition or find-references finds nothing

The language server builds a symbol index from the token stream, so it works even
on a file that does not fully parse. It is lexical rather than scope-aware: a name
declared in two scopes is reported at both, because resolving the shadowed one
needs scope analysis the index deliberately does not attempt. A built-in has no
declaration in the file, so go-to-definition declines rather than guessing.

### Inlay hints only appear on some calls

Parameter names are shown for calls to functions the file declares. Built-ins
carry no parameter names in the lexer, so those calls are left unlabelled rather
than given invented ones.

## Packages

### `nect pkg install` cannot reach the registry

Check network access, then try `--offline`, which installs from `~/.nect/cache`
if the packages are already there. A checksum mismatch is reported rather than
retried: it means a download was corrupted or tampered with, and installing it
anyway would defeat the point of verifying it.

### A dependency version will not resolve

Versions are semver and pinned by `nect.lock`. Delete the lock entry (or the
lock file) to re-resolve, and check the package is not yanked — yanked versions
are excluded from resolution by design.

## Performance

### A program is slower than expected

1. Is native compilation active? `nect disasm file.nct` prints the verdict for
   every function and for the module prefix, with the reason for each rejection.
2. Compare the engines: `NECT_NO_JIT=1 nect run file.nct` runs the bytecode VM
   alone. If the two are the same speed, native compilation is not being used.
3. Measure rather than guess: `benches/benchmark.sh` compares the VM, the JIT,
   the native build, and CPython with `.py` twins of every benchmark.

A function is compiled natively when it is numeric-only *and* shows evidence of
repeated work — a loop, recursion, or a call from a loop body. The reasoning and
the measurements are in `PERFORMANCE_PLAN.md` §3.2.2.

### A benchmark regression is reported

`scripts/bench-regression.sh` compares against `benches/baseline.tsv`. CI
hardware is shared, so re-run before believing it: the script only reports a
regression when a benchmark is slower than both its baseline and the suite's
median change, precisely to filter out a uniformly busy machine. To accept a real
change as the new baseline, run `scripts/bench-regression.sh --update`.

## Reporting a bug

Include the program (or the smallest program that still shows it), what you
expected, what happened, and the output of `nect --version` and `nect doctor`.
For anything involving the engines, `nect disasm` output is what identifies
whether it is a VM, JIT, or C-backend problem.

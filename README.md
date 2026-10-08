# Nect

A small dynamically typed scripting language with three execution engines: a
bytecode VM, a Cranelift JIT for hot numeric code, and a C backend that
translates a program to a standalone native binary. A tree-walking interpreter
serves as the reference engine used to cross-check the others.

```nct
fn sieve(limit) {
    let is_prime = []
    for i in range(limit + 1) {
        push(is_prime, true)
    }
    is_prime[0] = false
    is_prime[1] = false

    for candidate in range(2, limit + 1) {
        if (!is_prime[candidate]) {
            continue
        }
        if (candidate * candidate > limit) {
            break
        }
        let multiple = candidate * candidate
        while (multiple <= limit) {
            is_prime[multiple] = false
            multiple += candidate
        }
    }

    let found = []
    for i in range(limit + 1) {
        if (is_prime[i]) {
            push(found, i)
        }
    }
    return found
}

print(sieve(30))
```

## Install

**macOS and Linux**

```sh
curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | sh
```

**Windows** (PowerShell)

```powershell
irm https://github.com/necatiprok1/nect/releases/latest/download/install.ps1 | iex
```

**No Rust, Cargo, Visual Studio, or C/C++ compiler is needed to install Nect or
run `.nct` programs.** The scripts download a prebuilt executable, verify it
against `SHA256SUMS`, and set up your user `PATH` without administrator access.
Open a new terminal afterward.

The default **lean** build includes the VM, JIT, and embedded standard library.
Measured locally on macOS ARM64: **3.00 MiB installed / 1.34 MiB download**;
other platforms and versions may differ. Add `--full` for the language server,
package manager, and FFI. In PowerShell, download the script and run
`.\install.ps1 -Full`.

Prefer a single file? Download `nect.exe` for Windows, or extract `nect` from
your platform's archive on the [Releases page](https://github.com/necatiprok1/nect/releases/latest).
No separate runtime is bundled or required beyond supported system libraries.
`nect build` is the exception: its C backend still needs a system C compiler.

These commands become available once a release with binary assets has been
published. A source-only release is not enough; maintainers can first run the
**Release** workflow with `dry-run` enabled.

**[docs/KURULUM.md](docs/KURULUM.md)** is the full guide in Turkish:
[Windows / macOS / Linux installation](docs/KURULUM.md), manual installation,
building from source, the optional features and what each one costs, and how to
verify or uninstall.

From source (for Nect contributors or custom feature builds, not required for users):

```sh
git clone https://github.com/necatiprok1/nect.git
cd nect
cargo install --path .                # lean
cargo install --path . --features full
```

## Quick start

After installation, save a file called `hello.nct` containing `print("Hello, Nect!")`, then:

```sh
nect --version
nect run hello.nct
nect check hello.nct       # parse only
nect disasm hello.nct      # bytecode + JIT decisions
nect run --interp hello.nct
nect run -                # read source from stdin
```

The `examples/` directory is in this repository, not required by the installed executable.

## Optional features

The default build is the language. Everything that pulls in a large third-party
dependency tree is a cargo feature, so nobody downloads a GUI toolkit in order to
run a hello-world:

| Feature | Adds | Cost |
| --- | --- | --- |
| `net` | `http_get`, `http_post`, `http_request` | ~106 crates |
| `server` | `http_server`, `http_route`, `http_listen`, … | ~49 crates |
| `db` | `db_open`, `db_query`, … (SQLite) | ~9 crates + a C compile |
| `gui` | `gui_window`, `gui_show`, … | ~150 crates |
| `lsp` | the `nect lsp` language server | ~52 crates |
| `pkg` | the `nect pkg` package manager | ~60 crates |
| `ffi` | `extern` declarations | a few crates |
| `full` | `lsp` + `pkg` + `ffi` | the "I want everything" build |

```sh
cargo install --path . --features full
cargo install --path . --features gui,db
```

`gui`, `net`, `server`, and `db` are deliberately *not* in `full`: together they
are the bulk of the tree, and most people never touch them. Ask for them by
name.

A lean binary is not a broken binary — it just says so:

```console
$ nect run gui_demo.nct
error: 'gui_window' needs the `gui` feature (rebuild with --features gui)
```

`nect doctor` lists what a given binary has.

## Compiling to a native binary

```bash
./target/release/nect build benches/heavy/dot_product.nct   # -> ./dot_product
./dot_product                                                # a standalone executable
./target/release/nect build prog.nct --emit-c               # inspect the C
./target/release/nect build prog.nct --keep-c -o prog       # keep the C too
```

`nect build` translates the provably-numeric subset of the language to C
(numbers, booleans, control flow, calls, strings used inside expressions) and
invokes the system C compiler. The result needs neither Nect nor a runtime
installed, and its output is checked to be identical to the VM's — including
error messages and exit codes. Compute-bound programs typically land 50x–250x
faster than CPython and 1.2x–1.8x faster than the in-process JIT (see
`benches/results.txt`). Programs that use something the subset does not cover
report exactly why and keep running on the VM, where the JIT handles the hot
numeric code.

## Learn the language

* **[docs/tutorial.md](docs/tutorial.md)** — the language taught from scratch,
  with every sample's exact output.
* **[docs/cookbook.md](docs/cookbook.md)** — task-oriented recipes for text,
  arrays, loops, math, and formatting; every recipe runnable as-is.
* **[docs/reference.md](docs/reference.md)** — grammar, precedence, built-in
  reference, error catalogue, command line, and the engines.
* **[examples/](examples)** — thirteen runnable programs, from `hello.nct` to
  `interpolation.nct`, `maps.nct`, `statistics.nct`, a complete
  `calculator.nct` with an ANSI terminal UI and its own expression parser
  (written entirely in Nect), and `webapp.nct`, a browser painting app built
  with the embedded `std/ui.nct` library.

## Libraries

Programs grow with `import`, which splices a module's source into yours before
parsing:

```nct
import "helpers.nct"      // a file next to this one
import "std/ui.nct"       // from the standard library embedded in the binary
```

Each module is spliced once (shared imports and cycles are safe), paths
resolve relative to the importing file, and `std/` ships inside the executable.
The standard library today:

* **`std/ui.nct`** — build graphical apps that run in the browser: compose a
  page from `ui_page`/`ui_heading`/`ui_button`/`ui_input` widgets and
  `ui_open` it. See `examples/webapp.nct`.

Beyond modules, the runtime covers application needs directly: `read_file` /
`write_file`, `json_encode` / `json_decode`, `open_url`, `now` / `sleep`,
`args()` for command-line arguments, and the `input` / `random` / `seed`
family for interactive programs.

## Language at a glance

```nct
let name = "Nect"          // let declares; assignment needs an existing name
let values = [3, 1, 2]      // arrays are shared references, growable
values[0] += 10             // compound assignment, negative indices allowed

print("${name} has ${values.len()} values")   // interpolation + method sugar

let user = {name: "Ada", score: 91}          // maps: insertion-ordered key/values
print("${user.name}: ${user.score}")         // Ada: 91
user.score += 5                              // dot write; has() asks membership

if values.len() > 2 {       // parentheses on if/while are optional
    print("three or more")
} else if values.len() == 2 {
    print("two")
} else {
    print("fewer")
}

for value in sort(values) { // for walks arrays; range(n) builds one
    print(value, value % 2 == 0 ? "even" : "odd")
}

fn mean(items) {            // functions return null unless they return a value
    assert(len(items) > 0, "mean() needs values")
    return sum(items) / len(items)
}

while true {                // break/continue work in both loop kinds
    if mean([1, 2, 3]) == 2 {
        break
    }
}

print(1 < 2 and 2 < 3)      // and/or/not or &&/||/! — same operators
print(1_000_000, 1.5e2)     # digit separators, exponents, # comments too
```

## Development

```bash
cargo test                  # 228 unit, golden-output, differential, AOT, library tests
cargo clippy --all-targets
bash benches/benchmark.sh   # bytecode vs JIT vs native vs CPython
```

The tests that matter most for correctness are in
`tests/differential_tests.rs`: every program runs on the interpreter, the
bytecode VM, and the JIT-enabled VM, and all three must produce identical stdout,
stderr, and exit status.

See [PERFORMANCE_PLAN.md](PERFORMANCE_PLAN.md) for the optimization work and its
measurements, and [AGENTS.md](AGENTS.md) for project conventions.

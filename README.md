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

## Quick start

```bash
cargo build --release

./target/release/nect run examples/hello.nct
./target/release/nect run examples/primes.nct
./target/release/nect check  examples/arrays.nct   # parse only
./target/release/nect disasm examples/arrays.nct   # bytecode + JIT decisions
./target/release/nect run --interp examples/primes.nct  # reference engine
./target/release/nect run -                             # read stdin
```

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

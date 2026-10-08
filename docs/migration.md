# Migration guide

How to move code between Nect versions, and between Nect and the languages it
resembles.

The compatibility promise is in [`spec/compatibility.md`](spec/compatibility.md).
In short: within a major version, existing correct programs keep working. Syntax
that is removed, semantics that change, and built-ins that change behaviour are
all breaking changes and only happen at a major version boundary. Engine
differences that are intentional are pinned by tests, so they cannot drift.

## Within Nect 1.x

Minor and patch releases require no changes. The things most likely to matter:

### `nect fmt` output

The formatter is deterministic, so a new version may reformat code an earlier
one left alone. Nothing breaks; the diff is the whole change. `nect fmt --check`
tells you whether a file would change without rewriting it.

### Lint rules

New rules can be added in a minor release. A rule that fires on existing code is
a warning, not a build failure — `nect lint` exits `1` but nothing in the
toolchain treats that as fatal except a pre-commit hook you configured
yourself. Each finding carries a rule id, so you can disable one specifically
rather than switching linting off:

```bash
nect lint --no-unused --no-shadowing file.nct
```

### New built-ins

Built-in names are reserved: a program that declares `let max = 1` works today
and stops working the day `max` becomes a built-in. This is the one way a minor
release can break a working program, and it is deliberate — a built-in that
could be shadowed would behave differently in two files that look identical.
The compatibility policy classifies this as breaking and says so.

If you are confident a name is safe today, it is still worth checking the
built-in list in [`reference.md`](reference.md) §8 when upgrading.

### Diagnostic wording

Error messages are part of the public contract, so their *wording* is stable.
Positions and the set of detected errors can improve, so a program that relied on
a particular error being reported is relying on something unspecified.

## Tooling

### Editor

The VS Code extension follows the language server's capabilities. After
updating Nect, restart the language server (`Nect: Restart Language Server`) so
the new capabilities are picked up — a running server keeps the ones it advertised
at `initialize` time.

### Debugging

Stepping is statement-granular at module level, and has been since the debugger
was introduced. `stepIn` and `stepOut` are answered as a single step, and the
`initialize` response says so, so an editor can grey them out. If your editor
shows them as enabled, it is ignoring the capability flags.

### CI

`nect build` needs a C compiler; the release workflow installs one per platform.
`cargo test` runs the differential suite, so an engine regression fails the build
rather than reaching a user.

## From Python

Nect's syntax is Python-shaped, which makes this mostly a matter of learning what
is deliberately different.

### Blocks are braces, not indentation

```
# Python
def add(a, b):
    return a + b

# Nect
fn add(a, b) {
    return a + b
}
```

### Semicolons are optional and newlines are statement separators

A statement ends at a newline or a `;`. A newline inside `(`, `[`, or `{` is
folded away, so a multi-line call or literal needs no continuation marker.

### `and` / `or` / `not` work as well as `&&` / `||` / `!`

```
if x > 0 and len(name) > 1 {   // also: if x > 0 && len(name) > 1 {
```

### Comparison operators are the C family, not `==`/`!=` spelled differently

`< > <= >= == !=` behave as expected. There is no chained comparison
(`0 < x < 10` is `(0 < x) < 10`, not a range test), and no `is` — identity
comparisons use `==`.

### Indexing is zero-based and counts characters, not bytes

`text[0]` is the first character, including for multi-byte text: `"🎯"` has
length 1. Negative indices count from the end, so `arr[-1]` is the last element.

### There is no `None`, and no separate boolean-to-number conversion

`null` is the absent value. There is no implicit conversion between booleans and
numbers, so `true + 1` is an error; write a conditional instead.

### Types are dynamic, but arithmetic is not

Any number is a 64-bit float. There is no integer type, so `1 / 2` is `0.5` —
which is the opposite of Python 3, where it is also `0.5`, but the opposite of
Python 2. Division by zero is an error rather than an exception.

### String formatting is interpolation, not `format` or `%`

```
print("hello ${name}, x is ${x}")
```

`name.upper()` is sugar for `upper(name)`, so any built-in can be written as a
method.

### Errors are values, not exceptions

There is no `try`. A runtime error stops the program with a message on stderr and
exit status 1. `assert(condition, "message")` is the closest thing to a raised
error.

### Maps are insertion-ordered

`{ "b": 1, "a": 2 }` prints in that order, and `for k in map` yields the keys in
that order. Map equality includes entry order, so two maps with the same pairs in
different orders are different maps — which is what makes cross-engine output
comparison meaningful.

## From JavaScript and TypeScript

### Types are annotations-free

There is no `type` or `interface`, no generics, and no compile-time checking.
`nect disasm` prints inferred types for the JIT's benefit; that output is a
report, not a contract you write against.

### `let` is a declaration, not a block-scoped binding you can revisit

`let x = 1` declares `x`. Re-running it in a loop redeclares rather than
assigns. Use `x = 1` to assign.

### Only a bare block is a scope

```
// `y` is scoped to the block, and reading it afterwards is an error.
{
    let y = 1
    print(y)
}
```

An `if`/`while`/`for` body is *not* a scope, so declarations inside one land in
the enclosing scope. This is the scope rule that surprises newcomers most, and
it is tested explicitly.

### Arrays are 1-based in length, 0-based in index

`len(arr)` gives the count; `arr[0]` is the first element. There is no `undefined`
— reading past the end is an error, not `undefined`.

### `null`, not `null`/`undefined`

One absent value, spelled `null`. `has(map, key)` and `map["k"]` are how you test
for presence.

### `push`/`pop` mutate in place and return the array

`push(arr, 1)` appends and returns `arr`, so it can be chained or ignored. There
is no separate `arr.push(1)` method form for the mutating operations, though
`push(arr, 1)` reads fine as method sugar.

### Callbacks are functions, and there is no `Array.map`

```
fn double(x) {
    return x * 2
}
let doubled = [1, 2, 3]
let out = []
for value in doubled {
    push(out, double(value))
}
```

This is closer to the language's design than a `map` builtin would be: the
standard library stays small, and a loop is always available.

## From Rust and C

### `fn` and `let` will feel familiar; the rest is not

`fn`/`let`/`return`/`if`/`while`/`for` read as you would expect, and functions
are first-class in the interpreter. Differences worth knowing:

- No types, no generics, no traits, no lifetimes, no modules beyond source-level
  `import` splicing.
- Errors are messages, not `Result`.
- `for x in xs` iterates arrays and map keys; there is no iterator protocol.

### `extern` for native code

Calling C is declared rather than linked:

```
extern "/usr/lib/libm.so" {
  fn sin(number) -> number;
}
```

Types are `number`, `string`, `bool`, and `void`, with at most four arguments.
Anything non-numeric is rejected by the JIT and the C backend, so a program using
`extern` runs on the VM. Full rules are in [`reference.md`](reference.md) §10.

## From other implementations of Nect

There are no other implementations to migrate from. The three engines
(`run --interp`, the VM, and `nect build`) are required to agree, and
`tests/differential_tests.rs` enforces it for every program in the suite plus
generated ones. If you have observed a difference between engines, it is either
one of the five documented in [`reference.md`](reference.md) §13 — which exist
because the VM resolves calls at compile time and the interpreter does not, and
because the VM recurses on the heap while the interpreter recurses on the host
stack — or it is a bug.

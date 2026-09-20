# The Nect language tutorial

Nect is a small, dynamically typed scripting language: numbers, strings,
booleans, `null`, and arrays; `if`/`while`/`for`; and functions. It runs on a
bytecode VM that compiles provably-numeric functions to machine code, with a
tree-walking interpreter kept as a reference implementation.

This tutorial teaches the language from the ground up. Every code block is a
complete program, and each sample shows exactly what it prints. If you want the
short version of the syntax, jump to [docs/reference.md](reference.md); for
task-oriented recipes (text shaping, arrays, math, formatting) see
[docs/cookbook.md](cookbook.md).

- [1. Running a program](#1-running-a-program)
- [2. Hello, world](#2-hello-world)
- [3. Values and types](#3-values-and-types)
- [4. Variables, assignment, and scope](#4-variables-assignment-and-scope)
- [5. Operators](#5-operators)
- [6. Conditionals](#6-conditionals)
- [7. Loops](#7-loops)
- [8. Strings](#8-strings)
- [9. Arrays](#9-arrays)
- [10. Maps](#10-maps)
- [11. Functions](#11-functions)
- [12. Errors and assertions](#12-errors-and-assertions)
- [13. Built-in function reference](#13-built-in-function-reference)
- [14. Tools: check, disasm, and the two engines](#14-tools-check-disasm-and-the-two-engines)
- [15. A complete program, step by step](#15-a-complete-program-step-by-step)
- [16. Limitations and idioms](#16-limitations-and-idioms)
- [17. Modules, files, and apps](#17-modules-files-and-apps)

---

## 1. Running a program

Build the compiler once and put it on your path as `nect`:

```bash
cargo build --release
./target/release/nect run examples/hello.nct
```

The command line has four commands and two environment switches:

```bash
nect run <file>          # run a .nct file (bytecode VM + JIT)
nect run -               # run source read from stdin
nect run --interp <file> # run on the reference interpreter instead
nect check <file>        # parse only: report syntax errors
nect disasm <file>       # print bytecode and native-compilation decisions
nect --version
nect --help

NECT_NO_JIT=1 nect run <file>   # bytecode only, no native compilation
```

Source files are plain text and conventionally end in `.nct`. Statements are
separated by newlines or by `;`, and comments are `// line` or `/* block */`.
Inside parentheses and brackets you may break an expression across lines:

```nct
let values = [
    3,
    1,
    2,
]

print(
    "sorted:",
    sort(values),
)
```
Output:
```text
sorted: [1, 2, 3]
```

---

## 2. Hello, world

```nct
print("Hello, Nect!")
```
Output:
```text
Hello, Nect!
```

`print` is a built-in that writes its arguments to standard output, joined by a
space, and ends the line. With no arguments it prints an empty line.

```nct
print("one", 2, true)
print()
print("after the blank line")
```
Output:
```text
one 2 true

after the blank line
```

A program is parsed completely before anything runs, so a syntax error stops
the whole file — even in code that would never execute:

```nct
print("this never prints")
print(2 ** 3)
```
Error:
```text
2 | print(2 ** 3)
  |          ^
error: expected an expression, found '*'
```

---

## 3. Values and types

Nect has six kinds of value. `type(x)` returns the name of the one it got:

```nct
print(type(42), type(4.5), type("text"), type(true), type(null), type([1, 2]))
```
Output:
```text
number number string boolean null array
```

**Numbers** are 64-bit floating point values. A number prints without a decimal
point when it is whole, and `1/3` shows the usual floating-point precision:

```nct
print(7)
print(7.5)
print(10 / 5)
print(1 / 3)
print(0.1 + 0.2)
```
Output:
```text
7
7.5
2
0.3333333333333333
0.30000000000000004
```

There is no separate integer type, and therefore no exact integers beyond 2^53:

```nct
print(9007199254740993)
```
Output:
```text
9007199254740992
```

Number literals are digits with an optional fraction (`1`, `1.5`), underscore
digit separators for readability (`1_000_000`, `1.5_5`), and an optional
exponent (`1.5e2`, `2e-3`, `2E+4`). Separators are stripped before parsing, so
a literal must still have a digit on each side of every `_`.

**Strings** are immutable sequences of characters, written with double quotes.
Escapes are `\n`, `\t`, `\r`, `\\`, and `\"`; any other escaped character stands
for itself (`"\q"` is `"q"`).

**Booleans** are `true` and `false`. **`null`** is the absence of a value, and it
is what a function returns when it does not `return` anything.

**Arrays** are ordered, growable collections of any values — described in
[chapter 9](#9-arrays).

**Truthiness.** Only `false`, `null`, and `0` are falsy. An empty string and an
empty array are *truthy*, which is different from Python and JavaScript:

```nct
print(!0, !1, !"", ![], !null, !false)
```
Output:
```text
true false false false true true
```

---

## 4. Variables, assignment, and scope

`let` declares a name and gives it a value. There is no uninitialised variable
and no type declaration:

```nct
let name = "Nect"
let version = 1
print(name + " " + str(version))
```
Output:
```text
Nect 1
```

Assigning changes an existing name. Assigning to a name that was never declared
is an error rather than an implicit declaration, so typos are caught:

```nct
let x = 1
x = x + 1
print(x)

let y = 1
z = y + 1
print(z)
```
Output:
```text
2
```
Error:
```text
error: undefined variable 'z'
```

`let` can also shadow an existing name; each declaration gets its own storage.

### Compound assignment

`+=`, `-=`, `*=`, `/=`, and `%=` read the current value, apply the operator, and
write the result back:

```nct
let counter = 10
counter += 5
counter -= 3
counter *= 2
counter /= 4
counter %= 4
print(counter)
```
Output:
```text
2
```

`+=` also concatenates strings, which is how you build text in a loop:

```nct
let report = "items:"
for n in range(1, 4) {
    report += " " + str(n)
}
print(report)
```
Output:
```text
items: 1 2 3
```

Assignment is an *expression*: it evaluates to the value that was stored, so it
can be used anywhere a value is expected.

```nct
let x = 0
let y = x = 5
print(x, y)
```
Output:
```text
5 5
```

### Scope

Rules, in the order they matter:

1. A `let` at the top level of a file declares a **global** — visible to every
   function, and writable from inside one.
2. A `let` inside a function declares a **local** of that call.
3. A **bare block** `{ ... }` opens a new scope. Declarations inside it, and
   re-declarations that shadow outer names, are discarded at the `}`.
4. An `if`, `while`, or `for` *body* is **not** a scope: a `let` inside it
   declares in the enclosing scope, and stays visible after the body. Because
   the body may never run, reading such a name before it has been executed is an
   error.

```nct
let name = "outer"

{
    let name = "inner"
    print(name)
}

if (true) {
    let from_if = "leaks out of an if body"
}
print(name)
print(from_if)
```
Output:
```text
inner
outer
leaks out of an if body
```

Use a bare block when you want to scope a name inside a conditional:

```nct
if (true) {
    {
        let temporary = "gone at the closing brace"
        print(len(temporary))
    }
}

if (false) {
    let never_ran = 1
}
print(never_ran)
```
Output:
```text
25
```
Error:
```text
error: undefined variable 'never_ran'
```

---

## 5. Operators

| Category | Operators | Notes |
|---|---|---|
| Arithmetic | `+` `-` `*` `/` `%` | `+` also concatenates two strings |
| Comparison | `==` `!=` `<` `>` `<=` `>=` | ordering needs two numbers or two strings |
| Logical | `and` `or` `not` (or `&&` `\|\|` `!`) | short-circuit; the result is always a boolean |
| Unary | `-` `!` | numeric negation; logical not |
| Conditional | `? :` | `condition ? then : else` |
| Assignment | `=` `+=` `-=` `*=` `/=` `%=` | `=` for plain assignment |

The word forms `and`, `or`, and `not` are the same operators as `&&`, `||`,
and `!` — use whichever you find more readable:

```nct
print(true and false, true && false)
print(true or false, true || false)
print(not true, !true)
let age = 20
if age >= 18 and age < 65 {
    print("working age")
}
```
Output:
```text
false false
true true
false false
working age
```

Precedence runs from `? :` (loosest) through `or`/`||`, `and`/`&&`, equality,
comparison, `+ -`, `* / %`, unary (`not`/`!`, `-`), and finally
call/index/postfix. Parentheses group as you would expect.

```nct
print(2 + 3 * 4)
print((2 + 3) * 4)
print(-2 * -3)
print(10 / 4)
print(17 % 5)
print(-17 % 5)
print(5.5 % 2)
```
Output:
```text
14
20
6
2.5
2
-2
1.5
```

`%` follows the sign of the left operand, as in C and Rust, not Python.
Dividing or taking a remainder by zero is a runtime error:

```nct
print(10 % 0)
```
Error:
```text
error: modulo by zero
```

### Comparisons

`==` and `!=` compare any two values of any types (arrays compare structurally).
The ordering operators need comparable operands:

```nct
print("b" > "a")
print("abc" == "abc")
print([1, 2] == [1, 2])
print(2 >= 2)
print(1 < "a")
```
Output:
```text
true
true
true
true
```
Error:
```text
error: comparison requires numbers or strings, got number and string
```

### Logical operators

`&&` and `||` short-circuit — the right operand is only evaluated when the left
one does not decide the answer — and they always produce `true` or `false`, not
one of their operands:

```nct
fn noisy(x) {
    print("evaluating " + str(x))
    return x
}

print(false && noisy(1))
print(true || noisy(2))
print(1 && 2)
print(0 || "yes")
```
Output:
```text
false
true
true
true
```

The conditional expression is the way to choose between two values, and only the
taken branch is evaluated:

```nct
fn noisy(x) {
    print("evaluating " + str(x))
    return x
}

let n = 7
print(n % 2 == 0 ? "even" : "odd")
print(n > 5 ? "big" : n > 3 ? "medium" : "small")
print(true ? noisy(1) : noisy(2))
```
Output:
```text
odd
big
evaluating 1
1
```

---

## 6. Conditionals

The condition may be written with or without parentheses — both spellings are
the same statement, so pick whichever reads best for you. Braces around the
body are required. `else if` chains are supported and the first true branch
wins:

```nct
fn grade(score) {
    if score >= 90 {
        return "A"
    } else if score >= 80 {
        return "B"
    } else if score >= 70 {
        return "C"
    }
    return "F"
}

print(grade(95), grade(87), grade(72), grade(40))
```
Output:
```text
A B C F
```

The parenthesised form works too, and is equivalent:

```nct
if (score >= 90) {
    print("outstanding")
}
```

The condition is any value; its truthiness decides:

```nct
if (0) {
    print("not printed")
} else if ("non-empty strings are truthy") {
    print("second branch wins")
}
```
Output:
```text
second branch wins
```

Because a body is not a scope, each branch shares the enclosing scope, and
shadowing needs a bare block:

```nct
let limit = 10

if (limit > 5) {
    {
        let limit = 100
        print("inner limit: " + str(limit))
    }
    print("outer limit: " + str(limit))
}
```
Output:
```text
inner limit: 100
outer limit: 10
```

---

## 7. Loops

### `while`

`while condition { ... }` tests the condition before each iteration. The
condition may also be wrapped in parentheses, as in `while (condition)`:

```nct
let result = 1
let i = 1
while i <= 5 {
    result *= i
    i += 1
}
print("5! = " + str(result))
```
Output:
```text
5! = 120
```

### `for`

`for name in array { ... }` visits each element of an array in order. The loop
variable is a copy, so assigning to it does not change the array, and it stays
in scope after the loop (like any `let` in a body):

```nct
let items = ["a", "b", "c"]
for item in items {
    print(item)
}

let numbers = [1, 2, 3]
for number in numbers {
    number *= 10
    print(number)
}
print(numbers)
```
Output:
```text
a
b
c
10
20
30
[1, 2, 3]
```

`for` needs an array. For counting loops, build one with `range()`:

```nct
for n in range(3) {
    print(n)
}
print(range(2, 6))
print(range(10, 0, -3))
print(sum(range(1, 101)))
```
Output:
```text
0
1
2
[2, 3, 4, 5]
[10, 7, 4, 1]
5050
```

### `break` and `continue`

`continue` skips to the next iteration, `break` leaves the loop. In a `for` loop,
`continue` still advances the index. Both apply to the innermost loop:

```nct
for n in range(1, 11) {
    if (n % 2 == 0) {
        continue
    }
    if (n > 7) {
        break
    }
    print(n)
}
```
Output:
```text
1
3
5
7
```

```nct
let i = 0
while (i < 3) {
    i += 1
    for j in [1, 2, 3] {
        if (j == 2) {
            continue
        }
        if (i == 3) {
            break
        }
        print(i * 10 + j)
    }
}
```
Output:
```text
11
13
21
23
```

`break` and `continue` outside a loop are errors, and they cannot escape the
function they appear in:

```nct
fn f() {
    break
}
f()
```
Error:
```text
error: 'break' outside of a loop
```

`while (true)` plus `break` is the idiom for "loop until something happens":

```nct
let guess = 0
while (true) {
    guess += 7
    if (guess % 5 == 0) {
        break
    }
}
print(guess)
```
Output:
```text
35
```

---

## 8. Strings

Strings are immutable: you can read them and build new ones, but `s[0] = "x"` is
an error.

### Indexing and slicing

`s[i]` is a one-character string. Negative indices count from the end.
`slice(s, start)` and `slice(s, start, end)` take a half-open range, where either
index may be negative and an out-of-range endpoint clamps instead of failing:

```nct
let s = "Hello, Nect!"
print(s[0])
print(s[-1])
print(len(s))
print(slice(s, 0, 5))
print(slice(s, -4))
print(slice(s, 1, -1))
```
Output:
```text
H
!
12
Hello
ect!
ello, Nect
```

Source is UTF-8, and strings count **characters**, not bytes: `len`, indexing,
`slicing`, and `reverse` all work on characters, so non-ASCII text behaves the way
you would expect.

```nct
let word = "héllo"
print(len(word))
print(word[1])
print(reverse(word))
print(upper("straße"))
print(len("日本語"), "日本語"[1])
print(slice("aébc", 1, 3))
print(len("🎯"))
```
Output:
```text
5
é
olléh
STRASSE
3 本
éb
1
```

Identifiers, on the other hand, are ASCII letters, digits, and underscores.

Indexing outside the string is an error:

```nct
print("abc"[3])
```
Error:
```text
error: string index 3 out of bounds (length 3)
```

### Searching, splitting, joining

```nct
let message = "Hello, Nect!"

print(index_of(message, "Nect"))
print(index_of(message, "zz"))
print(contains(message, "lo,"))
print(replace(message, "Nect", "World"))
print(upper(message))
print(lower(message))
print(trim("   padded   "))
print(split("north,east,west", ","))
print(join(["a", "b", "c"], "-"))
print(repeat("ab", 3))
```
Output:
```text
7
-1
true
Hello, World!
HELLO, NECT!
hello, nect!
padded
["north", "east", "west"]
a-b-c
ababab
```

### Building strings

**Interpolation** splices any expression into a string with `${...}`. It is
the easiest way to build a message, and it replaces most manual `str`
conversions:

```nct
let user = "ada"
let score = 97.5
print("user: ${user}, score: ${score}")
print("next year: ${score + 2.5}")
print(" verdict: ${score > 90 ? "excellent" : "good"}")
print("escaped: \${not an expression}")
```
Output:
```text
user: ada, score: 97.5
next year: 100
 verdict: excellent
escaped: ${not an expression}
```

An interpolation may hold any expression — calls, indexing, even another
string with its own interpolation. A `${` without a closing `}` is an error,
and `\$` escapes a literal dollar sign.

**Method syntax** calls a builtin with the value as the first argument:
`s.upper()` is exactly `upper(s)`, and arguments follow: `s.split(",")` is
`split(s, ",")`. Chaining reads left to right:

```nct
let raw = "  Mixed Case  "
print(raw.trim().lower())
print(raw.trim().upper())
print("a,b,c".split(",").len())
print([3, 1, 2].sort().join(" < "))
```
Output:
```text
mixed case
MIXED CASE
3
1 < 2 < 3
```

Every builtin is reachable this way — methods are sugar, not a separate
dispatch, so there is nothing new to memorise.

**`concat`** turns values into one string without a template:

```nct
print(concat("n = ", 42, ", ok = ", true))
print(concat(1.5, " / ", [1, 2]))
```
Output:
```text
n = 42, ok = true
1.5 / [1, 2]
```

Because `+` only concatenates *two strings*, convert numbers with `str` (or
use `print`, which formats its arguments for you):

```nct
let total = 0
for n in range(1, 5) {
    total += n
}
print("total: " + str(total))
print("total:", total)
```
Output:
```text
total: 10
total: 10
```

---

## 9. Arrays

Array literals use square brackets and hold any values, including other arrays.
Arrays print with commas and spaces, and strings *inside* an array are quoted so
you can tell `["1"]` from `[1]`:

```nct
let mixed = [1, "two", true, null, [3, 4]]
print(mixed)
print(mixed[4][1])
print(type(mixed), len(mixed))
```
Output:
```text
[1, "two", true, null, [3, 4]]
4
array 5
```

### Indexing and assigning

Reads and writes accept negative indices; the index must be in range, and a
fractional index is truncated toward zero:

```nct
let scores = [10, 20, 30]
print(scores[0], scores[-1], scores[-3])
print(scores[1.9])

scores[0] = 99
scores[1] += 5
scores[-1] *= 2
print(scores)
```
Output:
```text
10 30 10
20
[99, 25, 60]
```

An index assignment is an expression, like any other assignment. Its index is
evaluated exactly once, so a call used as an index runs a single time:

```nct
let calls = 0
fn bump() {
    calls += 1
    return 1
}

let a = [1, 2, 3]
a[bump()] += 10
print(a, calls)
```
Output:
```text
[1, 12, 3] 1
```

Out-of-range and non-numeric indices are errors:

```nct
print([1, 2][-3])
```
Error:
```text
error: array index -3 out of bounds (length 2)
```

```nct
print([1, 2]["x"])
```
Error:
```text
error: array index must be a number, got string
```

### Growing and shrinking

`push` appends one or more values and returns the array, so appending happens in
place. `pop` removes and returns the last element:

```nct
let stack = []
push(stack, "a")
push(stack, "b", "c")
print(stack)
print(pop(stack))
print(stack, len(stack))
```
Output:
```text
["a", "b", "c"]
c
["a", "b"] 2
```

### Whole-array helpers

`sort` sorts in place and returns the array; `reverse` reverses an array in place
(or returns a reversed string); `sum`, `min`, `max`, `contains`, `index_of`,
`join`, and `slice` read the array without modifying it:

```nct
let data = [3, 1, 4, 1, 5, 9]
print(sort(data))
print(reverse(data))
print(sum(data))
print(min(data), max(data))
print(index_of(data, 9))
print(contains(data, 4))
print(join(data, "+"))
print(slice(data, 1, 4))
print(data)
```
Output:
```text
[1, 1, 3, 4, 5, 9]
[9, 5, 4, 3, 1, 1]
23
1 9
0
true
9+5+4+3+1+1
[5, 4, 3]
[9, 5, 4, 3, 1, 1]
```

Note that `sort` and `reverse` acted on `data` itself, so `index_of(data, 9)`
searched the reversed array and found the 9 at position 0. Read a helper's row
in the table above for which ones copy and which ones mutate.

`sort` needs values it can order: all numbers or all strings.

```nct
print(sort([1, "a"]))
```
Error:
```text
error: sort() requires an array of only numbers or only strings
```

### Arrays are shared, not copied

Assigning an array to another name copies the *reference*. Both names point at
the same elements, so a mutation through one is visible through the other. Use
`slice(a, 0)` when you want an independent copy:

```nct
let original = [1, 2]
let alias = original
let copy = slice(original, 0)

push(alias, 3)
push(copy, 9)

print(original)
print(alias)
print(copy)
```
Output:
```text
[1, 2, 3]
[1, 2, 3]
[1, 2, 9]
```

This also means an array passed to a function can be mutated by it, while
numbers, strings, and booleans are passed by value:

```nct
fn fill(items, value) {
    push(items, value)
}

let box = []
fill(box, 1)
fill(box, 2)
print(box)
```
Output:
```text
[1, 2]
```

### Building arrays

Start from `[]` or `range()` and grow with `push`:

```nct
let squares = []
for n in range(1, 6) {
    push(squares, n * n)
}
print(squares)
print(len(squares))
```
Output:
```text
[1, 4, 9, 16, 25]
5
```

---

## 10. Maps

A **map** pairs keys with values: `{key: value, ...}`. Maps keep their
**insertion order**, so printing one and iterating one are deterministic. A
bare identifier key is shorthand for the string of the same name —
`{name: "Ada"}` is exactly `{"name": "Ada"}`.

```nct
let ages = {ada: 36, linus: 25}
print(ages)
print(len(ages))
```
Output:
```text
{"ada": 36, "linus": 25}
2
```

### Reading and writing

Read with dot syntax or with brackets — `d.key` is exactly `d["key"]`:

```nct
let ages = {ada: 36, linus: 25}
print(ages.ada)
print(ages["linus"])
```
Output:
```text
36
25
```

Plain assignment inserts or overwrites; compound assignment reads first, so
its key must already exist:

```nct
let ages = {ada: 36}
ages.ada = 37          // overwrite
ages["new"] = 0        // insert
ages.ada += 1          // read + write: the key must exist
print(ages)
```
Output:
```text
{"ada": 38, "new": 0}
```

Keys may be strings, numbers, or booleans — `2` and `2.0` are the same key.
`null`, arrays, and maps cannot be keys. An unhashable key is an error:

```nct
let d = {a: 1}
d[[1]] = 2
```
Error:
```text
error: map keys must be strings, numbers, or booleans, got array
```

### Missing keys

Reading a missing key is an error, not `null` — a typo stays visible:

```nct
let ages = {ada: 36}
print(ages.ADa)
```
Error:
```text
error: map key ADa not found
```

Ask with `has` when absence is normal, and `remove` returns the removed value
(or `null` when the key was not there):

```nct
let d = {debug: true}
print(has(d, "debug"), has(d, "level"))
print(remove(d, "debug"), remove(d, "level"), d)
```
Output:
```text
true false
true null {}
```

### Iterating a map

A `for` over a map visits its **keys** in insertion order; use each key to
fetch its value:

```nct
let stock = {pen: 3, book: 7}
for item in stock {
    print("${item}: ${stock[item]}")
}
```
Output:
```text
pen: 3
book: 7
```

`keys` and `values` give the same order as plain arrays:

```nct
let stock = {pen: 3, book: 7}
print(keys(stock))
print(values(stock))
```
Output:
```text
["pen", "book"]
[3, 7]
```

### Maps compose

Maps and arrays nest freely, and chains read left to right:

```nct
let team = {
    lead: {name: "ada", score: 91},
    members: ["linus", "grace"],
}
print("${team.lead.name}: ${team.lead.score}")
print(team.members.len())
print([{id: 1}, {id: 2}][1].id)
```
Output:
```text
ada: 91
2
2
```

Maps compare structurally, **and entries must appear in the same order** —
two maps with equal entries in a different order are not equal (use `has`/
explicit comparisons when order-independent equality matters). An empty map
is truthy, like an empty array:

```nct
print({a: 1, b: 2} == {a: 1, b: 2}, {a: 1, b: 2} == {b: 2, a: 1})
print(!{}, {a: 1} ? "truthy" : "falsy")
```
Output:
```text
true false
false truthy
```

---

## 11. Functions

A function is declared with `fn` and called with parentheses. It returns `null`
unless a `return` statement gives a value:

```nct
fn add(a, b) {
    return a + b
}

fn note(message) {
    print("[log] " + message)
}

print(add(2, 3))
print(note("message"))
print(add(1))
```
Output:
```text
5
[log] message
null
```
Error:
```text
error: function 'add' expects 2 argument(s), got 1
```

Built-in names are reserved: a `fn` cannot take one over. Name your helper
something else — `fn log(x)` would still call the built-in logarithm:

```nct
fn log(x) {
    return "my log"
}

print(log("message"))
```
Error:
```text
error: log() requires a number, got string
```

Parameters are local copies, so assigning to one never affects the caller.
Arrays are the exception from [chapter 9](#arrays-are-shared-not-copied): the
reference is copied, the elements are shared.

```nct
fn increment(count) {
    count += 1
    return count
}

let n = 1
print(increment(n))
print(n)
```
Output:
```text
2
1
```

### Recursion

Functions may call themselves. The bytecode VM keeps call frames on the heap, so
recursion depth is bounded by memory rather than by the host stack (the
reference interpreter does recurse on the host stack — see
[chapter 13](#13-tools-check-disasm-and-the-two-engines)):

```nct
fn factorial(n) {
    if (n <= 1) {
        return 1
    }
    return n * factorial(n - 1)
}

fn down(n) {
    if (n <= 0) {
        return 0
    }
    return 1 + down(n - 1)
}

print(factorial(5))
print(factorial(20))
print(down(100))
```
Output:
```text
120
2432902008176640000
100
```

### Globals, helpers, and early returns

A function can read and write top-level `let`s, declare helper functions, and
return from inside a loop:

```nct
let visited = 0

fn first_negative(items) {
    for item in items {
        visited += 1
        if (item < 0) {
            return item
        }
    }
    return 0
}

fn describe(n) {
    fn kind(x) {
        return x % 2 == 0 ? "even" : "odd"
    }
    return str(n) + " is " + kind(n)
}

print(first_negative([3, -2, 7]))
print(visited)
print(first_negative([1, 2]))
print(describe(4), describe(5))
```
Output:
```text
-2
2
0
4 is even 5 is odd
```

Functions are not first-class values in the bytecode VM: a call is resolved
while the program is compiled, so you call a function by name and cannot store
it in a variable or pass it to another function. Referring to a declared
function as a value is an error in the default engine, and the reference
interpreter instead hands you an opaque function value (`<function double>`) —
one of the intentional differences listed in [the reference](reference.md).

---

## 12. Errors and assertions

Two things can go wrong: the program can fail to parse, or it can fail while
running. Parse errors come with the source line and a caret; runtime errors stop
the program immediately and exit with status 1.

```nct
let x = 5
let y = x / 0
print(y)
```
Error:
```text
error: division by zero
```

`assert(condition, message)` checks a condition and stops the program with your
message when it is falsy. It is the cheapest way to document an assumption or to
test a program without a test framework:

```nct
fn mean(values) {
    assert(len(values) > 0, "mean() needs at least one value")
    return sum(values) / len(values)
}

print(mean([2, 4, 6]))
print(mean([]))
```
Output:
```text
4
```
Error:
```text
error: assertion failed: mean() needs at least one value
```

Common runtime errors, all of which you can see in
`tests/vm_output_tests.rs`:

| Program | Error |
|---|---|
| `print(1 + "s")` | `cannot apply '+' to number and string` |
| `print(1 / 0)` | `division by zero` |
| `print(7 % 0)` | `modulo by zero` |
| `print(nope)` | `undefined variable 'nope'` |
| `print(-"s")` | `cannot negate a string` |
| `print([1, 2][5])` | `array index 5 out of bounds (length 2)` |
| `print(pop([]))` | `pop() on an empty array` |
| `assert(false)` | `assertion failed` |
| `return 1` (at top level) | `'return' outside of a function` |
| `break` (outside a loop) | `'break' outside of a loop` |
| `for x in 5 { }` | `for loop requires an array` |
| `print(len)` | `cannot use 'len' as a value (it is a function)` |

---

## 13. Built-in function reference

Built-ins are ordinary names you can call but not read as values. Everything
below is available in both engines.

### Output

| Call | Result |
|---|---|
| `print(...)` | Writes the arguments, separated by spaces, and a newline. Returns `null`. |
| `println(...)` | Same as `print`. |

### Conversion and inspection

| Call | Result |
|---|---|
| `str(x)` | `x` as a string; `str()` is `""`. |
| `num(x)` | A number from a number, numeric string (surrounding spaces are trimmed), boolean, or `null` (which is `0`). |
| `bool(x)` | The truthiness of `x` as a boolean. |
| `type(x)` | `"number"`, `"string"`, `"boolean"`, `"null"`, `"array"`, or `"function"`. |
| `assert(condition, message?)` | Errors unless the condition is truthy. |

```nct
print(str(12), num("  12  "), num(true), num(null))
print(bool(0), bool(""), bool([]))
print(type(1), type("1"), type([1]))
```
Output:
```text
12 12 1 0
false true true
number string array
```

### Strings

| Call | Result |
|---|---|
| `len(s)` | Number of characters. |
| `upper(s)` / `lower(s)` | Case-converted copy. |
| `trim(s)` | Copy without leading and trailing whitespace. |
| `slice(s, start, end?)` | Substring; negative indices count from the end, `end` is exclusive. |
| `index_of(s, needle)` | Character index of the first occurrence, or `-1`. |
| `contains(s, needle)` | Whether the substring occurs. |
| `replace(s, from, to)` | Copy with every occurrence of `from` replaced. |
| `split(s, separator)` | Array of the pieces between separators; the separator must not be empty. |
| `join(array, separator?)` | The elements rendered as text and joined. |
| `repeat(s, count)` | `s` repeated `count` times. |
| `reverse(s)` | Reversed copy. |
| `concat(value, ...)` | Every value rendered as text and joined; no separator. |

### Arrays

| Call | Result |
|---|---|
| `len(a)` | Number of elements. |
| `push(a, value, ...)` | Appends the values to `a`, in place, and returns `a`. |
| `pop(a)` | Removes and returns the last element; an error on an empty array. |
| `slice(a, start, end?)` | A *copy* of the requested range. |
| `index_of(a, value)` | Index of the first equal element, or `-1`. |
| `contains(a, value)` | Whether an equal element exists. |
| `join(a, separator?)` | The elements rendered as text and joined. |
| `sort(a)` | Sorts numbers or strings in place and returns `a`. |
| `reverse(a)` | Reverses `a` in place and returns `a`. |
| `sum(a)` | The total of the numbers in `a`. |

### Numbers

| Call | Result |
|---|---|
| `abs(n)` | Absolute value. |
| `floor(n)` / `ceil(n)` / `round(n)` | Rounding. |
| `int(n)` | Truncation toward zero: `int(-7.9)` is `-7`, not `floor`'s `-8`. |
| `fixed(n, digits)` | `n` as a string with exactly `digits` decimals (`fixed(1/3, 4)` is `"0.3333"`). |
| `sqrt(n)` | Square root; an error for negative input. |
| `pow(base, exponent)` | Exponentiation. |
| `log(n)` | Natural logarithm; an error for `n <= 0`. |
| `sin(n)` / `cos(n)` / `tan(n)` | Trigonometric functions, in radians. |
| `min(...)` / `max(...)` | Smallest or largest of several numbers, or of one array. |
| `range(stop)` / `range(start, stop)` / `range(start, stop, step)` | A new array counting from `start` (default `0`, inclusive) up to but not including `stop`. |

```nct
print(abs(-3), floor(2.7), ceil(2.1), round(2.5), sqrt(16))
print(pow(2, 10), log(1))
print(min(3, 1, 2), max(3, 1, 2))
print(min([9, 4, 6]), max([9, 4, 6]))
print(range(3), range(1, 4), range(4, 0, -1))
```
Output:
```text
3 2 3 3 4
1024 0
1 3
4 9
[0, 1, 2] [1, 2, 3] [4, 3, 2, 1]
```

`range` and `repeat` refuse to build more than ten million elements or
characters, which turns a mistyped argument into an error instead of a hang:

```nct
print(range(0, 99999999))
```
Error:
```text
error: range() would build 99999999 elements (limit 10000000)
```

### Characters and code points

| Call | Result |
|---|---|
| `char(code)` | The one-character string for a code point: `char(78)` is `"N"`. |
| `char_code(s)` | The code point of the first character: `char_code("N")` is `78`. |

They round-trip, and they work on any character, not just ASCII:

```nct
print(char(78), char_code("N"))
let dart = char(127919)          // the 🎯 emoji, built from its code point
print(char_code(dart), char(char_code(dart)) == dart)
```
Output:
```text
N 78
127919 true
```

### Input and randomness

| Call | Result |
|---|---|
| `input(prompt?)` | Reads one line from stdin, without the newline; `null` at end of input. The prompt, if given, is written first without a newline. |
| `random()` | A pseudo-random number in `[0, 1)`. |
| `random_int(low, high)` | A whole number in `low..=high`, both inclusive. |
| `seed(n)` | Resets the random sequence deterministically. |

A tiny guessing game shows them together:

```nct
seed(6)                          // same seed -> same sequence, every run
let secret = random_int(1, 10)
let guess = input("guess 1..10: ")
print(num(guess) == secret ? "bildin!" : "bilemedin: " + str(secret))
```

With the answer `2` piped in, the output is:

```text
guess 1..10: bildin!
```

Seeding makes programs reproducible: the same seed replays the exact same
sequence in every engine, so games and simulations stay testable. Without a
`seed` call the generator starts from the clock, so runs differ.

---

## 14. Tools: check, disasm, and the two engines

Nect ships two implementations of the same language:

* the **bytecode VM**, the default, which compiles the AST to compact
  instructions and additionally compiles provably-numeric functions to machine
  code with Cranelift;
* the **reference interpreter** (`--interp`), the original tree-walking
  evaluator, used to cross-check the VM.

`tests/differential_tests.rs` runs every program it can through all three
configurations — interpreter, bytecode VM, and VM with native compilation — and
compares stdout, stderr, and exit status character for character. Use `--interp`
when you want a second opinion on a surprising result, and `NECT_NO_JIT=1` to
see whether native compilation is involved:

```bash
nect run --interp program.nct
NECT_NO_JIT=1 nect run program.nct
```

`nect check` parses a file without running it, and `nect disasm` shows the
compiled program together with the decisions that drive native compilation. For
this program:

```nct
fn add(a, b) {
    return a + b
}

let total = 0
let i = 0
while (i < 3) {
    total += add(i, 2)
    i += 1
}
print(total)
```
Output:
```text
9
```

`nect disasm` reports:

```bash
nect disasm total.nct
```
Output:
```text
=== module: 0 slot(s), 18 instruction(s) ===
     0  load.const   0 (0)
     1  store.global #39   ; total
     2  load.const   0 (0)
     3  store.global #40   ; i
     4  branch       unless #40(i) < const1 (3) -> 14
     5  load.global  #39   ; total
     6  load.global  #40   ; i
     7  load.const   2 (2)
     8  call         fn0 add 2
     9  binary       Add
    10  store.keep   #39(total)
    11  pop
    12  store        #40(i) = #40(i) + const3 (1)
    13  jump         4
    14  load.global  #39   ; total
    15  call         print (builtin) 1
    16  pop
    17  halt

=== fn add (index 0, 2 param(s), 2 slot(s), 4 instruction(s)) ===
     0  fast         slot0 + slot1
     1  return
     2  load.const   0 (null)
     3  return

=== inferred types / native compilation ===
--- module: numeric prefix with a loop, native up to instruction 14 (rest in bytecode)
--- fn add: numeric, jit-eligible
    slot 0: number
    slot 1: number
```

Native compilation only applies to code that is provably numeric and that
actually repeats — a function called from a loop or by itself. Everything else
stays in bytecode, and the two must produce identical output.

---

## 15. A complete program, step by step

Here is `examples/primes.nct`, which uses most of the language:

```nct
fn sieve(limit) {
    // `true` means "still believed prime".
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
        // Cross out the multiples. Only go up to the square root.
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
print(sum(sieve(100)))
```
Output:
```text
[2, 3, 5, 7, 11, 13, 17, 19, 23, 29]
1060
```

Things worth noticing:

* `range(limit + 1)` builds the indices once; `for` walks arrays, not ranges.
* `push(is_prime, true)` grows the array in place, so `is_prime` is shared while
  the loop fills it.
* A `break` inside a `for` stops crossing out as soon as the remaining candidates
  cannot have multiples left to mark.
* `sum(sieve(100))` calls a function inside a call argument, so the result is a
  plain number by the time `sum` sees it.

More programs in the same style:

| File | Shows |
|---|---|
| `examples/hello.nct` | the smallest program |
| `examples/variables.nct` | types, assignment, scoping, truthiness |
| `examples/control_flow.nct` | `if`/`else if`, `while`, `break`, `continue`, `? :` |
| `examples/strings.nct` | indexing, slicing, searching, splitting, joining |
| `examples/arrays.nct` | literals, mutation, helpers, nesting, `range` |
| `examples/functions.nct` | recursion, helpers, by-value vs shared arguments |
| `examples/fizzbuzz.nct` | modulo and `else if` chains |
| `examples/primes.nct` | loops, `break`/`continue`, arrays as lookup tables |
| `examples/statistics.nct` | arrays, math builtins, working on a copy |

---

## 16. Limitations and idioms

Worth knowing before you write something larger:

* **No maps, no records, no user-defined types.** Arrays and parallel arrays are
  the tool you have.
* **No first-class functions.** Call by name; there are no closures, and a
  function name cannot be stored or passed.
* **No `switch`, no `do..while`, no `++`/`--`, no `**`.**
  Use `if`/`else if`, `while (true)` with `break`, `+= 1`, and `pow`.
* **Bodies are not scopes.** Reach for a bare block `{ ... }` when you want a
  name to disappear.
* **`+` does not coerce.** Convert with `str` and `num`; a mismatch is an error
  rather than a silent `"1" + 2` → `"12"`.
* **Truthiness is narrow.** `0`, `false`, and `null` are falsy; `""` and `[]` are
  not, so test `len(a) == 0` where you mean "empty".
* **Numbers are doubles.** Exact integers stop at 2^53, and `0.1 + 0.2` is not
  `0.3`.
* **Strings are immutable.** Build new ones with `+`, `slice`, `replace`, and
  `join`.
* **Deep recursion in the interpreter.** The VM stores frames on the heap and
  handles very deep recursion; `--interp` recurses on the host stack and
  typically overflows after a few thousand calls (fewer in a debug build). If a
  deeply recursive program behaves differently under `--interp`, that is why.
* **Nested `fn` timing.** In the VM, a nested function is compiled with its
  enclosing function and can be called as soon as the enclosing definition has
  been compiled; the interpreter only defines it when the enclosing function
  actually runs. Prefer top-level helper functions.

Idioms that fit the language:

```nct
// Accumulate with += instead of rebuilding a sum.
let total = 0
for value in [1, 2, 3, 4] {
    total += value
}

// Search with an early return; the loop exits as soon as the answer is known.
fn find_first(items, predicate_value) {
    for item in items {
        if (item == predicate_value) {
            return item
        }
    }
    return null
}

// Copy before sorting when the caller's order matters.
let signed_in = [12, 7, 3, 19]
let ordered = slice(signed_in, 0)
sort(ordered)

print(total, find_first(signed_in, 19), ordered)
```
Output:
```text
10 19 [3, 7, 12, 19]
```

## 17. Modules, files, and apps

Real programs grow past one file, and applications need to touch the world:
read data, save results, show an interface. Nect covers this with one language
keyword-level idea (`import`) and a handful of built-ins.

### Importing modules

An `import` must be alone on its line and names a `.nct` file. The module's
source is spliced into the program before parsing, so its functions and
variables become part of yours:

```nct
import "mathx.nct"      // a file next to this one
import "std/ui.nct"     // from the standard library embedded in the binary

print(double(21))
```

Rules, in one breath: each module is spliced **once**, so two files can import
the same helper and import cycles cannot hang; paths resolve relative to the
importing file; a missing file stops the program before it runs; and modules
belong at the top of the file, where a reader expects them.

### Reading and writing files

`read_file(path)` returns the whole file as one string; `write_file(path,
text)` creates or truncates it:

```nct
write_file("/tmp/nect-notes.txt", "first\nsecond\n")
let text = read_file("/tmp/nect-notes.txt")
print(len(split(text, "\n")))
```
Output:
```text
3
```

A failed `read_file` is an error, not a silent empty string — missing data
should be loud.

### JSON

`json_encode` turns any value (numbers, strings, booleans, null, arrays, maps)
into JSON text; `json_decode` parses it back. Maps keep insertion order, so
encoding is deterministic — the same value always prints the same bytes:

```nct
let user = {name: "ada", scores: [9, 8.5, 10]}
let back = json_decode(json_encode(user))
print(back.name, back.scores[2], back == user)
```
Output:
```text
ada 10 true
```

### Script arguments and time

`nect run app.nct a b` hands `["a", "b"]` to the program through `args()`.
`now()` is the current time in seconds; `sleep(seconds)` pauses:

```nct
print(args())
print(now() > 1_000_000_000)
```
Output:
```text
[]
true
```

### Building an interface

The embedded `std/ui.nct` library composes HTML pages: `ui_page` is the shell
(a dark, card-shaped layout), widgets like `ui_heading`, `ui_button`,
`ui_input`, `ui_row`, and `ui_region` fill the body, and `ui_open` writes the
page to a file and opens it in the system browser:

```nct
import "std/ui.nct"

let body = (
    ui_heading("Counter", "tap the button")
    + ui_button("again", "bump()")
    + ui_region("out", "0")
)
ui_open("Counter", body)      // writes one .html file, opens the browser
```

The page is a single self-contained HTML file — the app's interactivity is the
HTML/JS you compose, and Nect is the language that builds it. Run
`./target/release/nect run examples/webapp.nct` for a complete painting app.

Continue with [the reference](reference.md) for the grammar, the full error
catalogue, and the engineering notes behind the two engines.

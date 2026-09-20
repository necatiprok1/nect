# The Nect Cookbook

Task-oriented recipes for common needs. Every recipe is a complete, runnable
program — the comments show the exact output of a run against the real
`nect` binary. For the guided tour see [`tutorial.md`](tutorial.md); for the
full grammar and builtin tables see [`reference.md`](reference.md).

Run any recipe with:

```bash
nect run recipe.nct
```

---

## 1. Building and shaping text

**Interpolation for reports, `concat` for values.** `${...}` splices any
expression into a string; `concat(...)` turns values into one string without
a template.

```nct
// nect run text.nct
let user = "ada"
let score = 97.5
print("user: ${user}, score: ${score}")     // user: ada, score: 97.5
print("rounded: " + str(round(score)))       // rounded: 98
print(concat(user, " scored ", score))       // ada scored 97.5
```

**Trimming and case.** Methods read left to right: `"  pad  ".trim()`
means `trim("  pad  ")`, and chains compose.

```nct
// nect run case.nct
let raw = "  Mixed Case  "
print(raw.trim().lower())        // mixed case
print(raw.trim().upper())        // MIXED CASE
print("a,b,c".split(","))        // ["a", "b", "c"]
print("a-b-c".replace("-", "+")) // a+b+c
```

**Padding a number for a table.** `repeat` builds the padding; `slice` caps it.

```nct
// nect run pad.nct
fn pad(text, width) {
    let fill = repeat(" ", width - len(text))
    return text + fill
}
print("[" + pad("id", 6) + "]")
print("[" + pad("name", 6) + "]")
// [id    ]
// [name  ]
```

---

## 2. Working with arrays

**Build, transform, summarize.**

```nct
// nect run arrays.nct
let readings = [12.5, 9.0, 15.25, 7.75]

print(sort(readings))            // [7.75, 9, 12.5, 15.25]
print(reverse(readings))         // [15.25, 12.5, 9, 7.75]
print(sum(readings))             // 44.5
print(min(readings), max(readings)) // 7.75 15.25
print(slice(readings, 1, 3))     // [12.5, 9]
```

**Accumulate a new list with a loop.**

```nct
// nect run map.nct
let numbers = range(1, 6)        // the numbers 1 through 5
let squares = []
for n in numbers {
    push(squares, n * n)
}
print(squares)                   // [1, 4, 9, 16, 25]
```

**Filter with `contains` and conditions.**

```nct
// nect run filter.nct
let values = [3, 8, 1, 12, 5]
let big = []
for v in values {
    if v > 4 {
        push(big, v)
    }
}
print(big)                        // [8, 12, 5]
print(contains(values, 12))       // true
print(index_of(values, 1))        // 2
```

**Table of counts (update in place).** Compound assignment on an index
evaluates the index once.

```nct
// nect run counts.nct
let words = ["red", "blue", "red", "green", "red"]
let names = []
let counts = []
for w in words {
    let at = index_of(names, w)
    if at >= 0 {
        counts[at] += 1
    } else {
        push(names, w)
        push(counts, 1)
    }
}
print(names)    // ["red", "blue", "green"]
print(counts)   // [3, 1, 1]
```

---

## 3. Working with maps

Maps pair keys with values and keep their insertion order. Read and write with
dots or brackets — `user.name` is exactly `user["name"]`.

```nct
// nect run map_basics.nct
let user = {name: "ada", score: 91, active: true}
print(user)                          // {"name": "ada", "score": 91, "active": true}
print(user.name, user["score"])      // ada 91

user.score += 9
user.level = 3
print(user)
// {"name": "ada", "score": 100, "active": true, "level": 3}
```

**Count things.** Grouping is the classic map job:

```nct
// nect run word_counts.nct
let words = ["red", "blue", "red", "green", "red", "blue"]
let counts = {}
for w in words {
    if has(counts, w) {
        counts[w] += 1
    } else {
        counts[w] = 1
    }
}
print(counts)                        // {"red": 3, "blue": 2, "green": 1}
for word in counts {
    print("${word}: ${counts[word]}")
}
// red: 3
// blue: 2
// green: 1
```

**Iterate in order** — a `for` over a map visits its keys, and `keys`/`values`
give the same order as arrays:

```nct
// nect run inventory.nct
let stock = {pen: 3, book: 7, lamp: 1}
print(keys(stock))                   // ["pen", "book", "lamp"]
print(values(stock))                 // [3, 7, 1]
let total = 0
for item in stock {
    total += stock[item]
}
print(total)                         // 11
```

**Records: maps in arrays.** This is the shape most real data takes:

```nct
// nect run records.nct
let people = [
    {name: "ada", score: 91},
    {name: "linus", score: 78},
    {name: "grace", score: 95},
]
let best = people[0]
for p in people {
    if p.score > best.score {
        best = p
    }
}
print("top: ${best.name} (${best.score})")   // top: grace (95)
```

**Ask before you read.** A missing key is an error (typos stay visible);
`has` answers membership and `remove` deletes:

```nct
// nect run settings.nct
let settings = {theme: "dark"}
print(has(settings, "theme"), has(settings, "lang"))   // true false
print(remove(settings, "theme"))                       // dark
print(remove(settings, "theme"))                       // null
print(settings)                                        // {}
```

---

## 4. Loops and control flow

**Parentheses are optional** on `if` and `while`; use whichever style you
prefer. Both are the same language.

```nct
// nect run control.nct
let total = 0
let i = 1
while i <= 10 {
    total += i
    i += 1
}
print(total)                      // 55

for n in range(1, 6) {
    if n % 2 == 0 {
        print("${n} is even")
    } else {
        print("${n} is odd")
    }
}
// 1 is odd
// 2 is even
// 3 is odd
// 4 is even
// 5 is odd
```

**Skip and stop early.**

```nct
// nect run skip.nct
for n in range(1, 10) {
    if n % 3 == 0 {
        continue                  // skip multiples of three
    }
    if n > 7 {
        break                     // stop the loop entirely
    }
    print(n)
}
// 1
// 2
// 4
// 5
// 7
```

**Pick with the conditional expression.** `cond ? a : b` is an expression, so
it nests inside interpolation and calls.

```nct
// nect run pick.nct
let points = 72
print(points >= 70 ? "pass" : "fail")            // pass
print("grade ${points >= 90 ? "A" : points >= 80 ? "B" : "C"}") // grade C
```

---

## 5. Functions

**Small helpers, early returns, recursion.** Every function returns its last
value implicitly; `return` exits early.

```nct
// nect run funcs.nct
fn clamp(value, low, high) {
    if value < low {
        return low
    }
    if value > high {
        return high
    }
    return value
}

print(clamp(5, 0, 10))     // 5
print(clamp(-3, 0, 10))    // 0
print(clamp(42, 0, 10))    // 10

fn factorial(n) {
    return n <= 1 ? 1 : n * factorial(n - 1)
}
print(factorial(10))       // 3628800
```

**A function that returns several results** packs them into an array.

```nct
// nect run stats.nct
fn summarize(values) {
    let total = sum(values)
    return [total, total / len(values)]
}

let both = summarize([2, 4, 6])
print("total ${both[0]}, mean ${both[1]}")   // total 12, mean 4
```

---

## 6. Math recipes

**Discrete math with `%` and `floor`.**

```nct
// nect run math.nct
// Digits of a number, least significant first.
fn digits(n) {
    let out = []
    while n > 0 {
        push(out, floor(n % 10))
        n = floor(n / 10)
    }
    return out
}
print(digits(90210))              // [0, 1, 2, 0, 9]

// Evenly spaced samples.
for t in range(0, 5) {
    let x = t / 4                 // t / 4: 0 .. 1
    print("x=${x} sin=${round(sin(x) * 1000) / 1000}")
}
// x=0 sin=0
// x=0.25 sin=0.247
// x=0.5 sin=0.479
// x=0.75 sin=0.682
// x=1 sin=0.841
```

**Clamp a value into a range, the one-liner way.**

```nct
// nect run clamps.nct
fn clamp(v, lo, hi) {
    return max(lo, min(hi, v))
}
for v in [-5, 0.5, 4, 9] {
    print(clamp(v, 0, 5))         // one value per line: 0, 0.5, 4, 5
}
```

---

## 7. Formatting output

**Aligned columns.**

```nct
// nect run table.nct
let rows = [
    ["apples", 3, 1.2],
    ["bananas", 12, 0.35],
]
print("fruit       qty  price")
for row in rows {
    print("${row[0]}${repeat(" ", 8 - len(row[0]))} ${row[1]}   ${row[2]}")
}
// fruit       qty  price
// apples   3   1.2
// bananas  12   0.35
```

**Debugging values with `type` and `assert`.**

```nct
// nect run debug.nct
let maybe = null
print(type(maybe))               // null
print(type([1]), type("s"), type(1), type(true))  // array string number boolean

let total = 0
for i in range(1, 5) {
    total += i
}
assert(total == 10, "sum of 1..4 should be 10")
print("checked: ${total}")       // checked: 10
```

---

## 8. Guard rails

**Validate input early and fail loudly.** The program stops with a nonzero
exit status and a clear message on stderr.

```nct
// nect run guards.nct
fn withdraw(balance, amount) {
    assert(amount > 0, "amount must be positive")
    assert(amount <= balance, "insufficient funds: ${balance} < ${amount}")
    return balance - amount
}

print(withdraw(100, 30))         // 70
```

Running `withdraw(100, 300)` prints:

```text
error: assertion failed: insufficient funds: 100 < 300
```

and the process exits with status 1. Nothing is printed on stdout.

---

## 9. Numerical work at native speed

Compute-heavy functions that only touch numbers compile to native machine
code automatically (see `nect disasm` for the decision). Write the obvious
loop; the compiler fuses the hot stores.

```nct
// nect run numeric.nct
fn dot(a, b) {
    let total = 0.0
    for i in range(0, len(a)) {
        total += a[i] * b[i]
    }
    return total
}

let v1 = []
let v2 = []
for i in range(0, 1000) {
    push(v1, i * 0.5)
    push(v2, i * 0.25)
}
print(dot(v1, v2))               // 41604187.5
```

Check what compiles natively:

```bash
nect disasm numeric.nct | grep 'fn dot'
# fn dot: numeric, jit-eligible
```

---

## 10. Interactivity and randomness

**Reading a line of input.** `input(prompt?)` writes the prompt without a
newline and reads one line, without the trailing newline. End of input yields
`null`, so interactive loops can end cleanly:

```nct
// nect run echo.nct
while true {
    let line = input("> ")
    if line == null or line == "quit" {
        break
    }
    print("you said: ${line}")
}
```

**Formatting numbers for people.** `fixed(n, digits)` gives exact decimals;
`int` truncates toward zero:

```nct
// nect run fmt2.nct
print(fixed(1 / 3, 4))            // 0.3333
print(fixed(7, 2))                // 7.00
print(int(7.9), int(-7.9))        // 7 -7
```

**Reproducible randomness.** Seed the generator and the sequence replays
exactly — in every engine, on every machine. Same seed, same dice:

```nct
// nect run dice.nct
seed(6)
let rolls = []
for i in range(5) {
    push(rolls, random_int(1, 6))
}
print(rolls)                      // [2, 1, 2, 3, 5]
```

Leave out `seed` and the generator starts from the clock, so each run differs.
Dice, shuffles, and simulations become testable by fixing the seed first.

---

## 11. Files and data exchange

**Writing and reading a file.** `write_file` creates or truncates;
`read_file` returns the whole file as one string:

```nct
// nect run notes.nct
write_file("/tmp/nect-notes.txt", "buy milk\ncall ada\n")
let text = read_file("/tmp/nect-notes.txt")
for line in split(text, "\n") {
    print("* " + line)
}
// * buy milk
// * call ada
// (and an empty line from the trailing newline)
```

**JSON in, JSON out.** Maps and arrays convert directly; decode gives back
the same structure, in the same order:

```nct
// nect run json.nct
let stock = {pen: 3, book: 7, ink: null}
let text = json_encode(stock)
print(text)                       // {"pen":3,"book":7,"ink":null}
let back = json_decode(text)
print(back.book, back == stock)   // 7 true
```

**Script arguments.** `nect run report.nct 2026 q3` → `args()` is
`["2026", "q3"]`:

```nct
// nect run report.nct 2026 q3
let a = args()
if len(a) < 2 {
    print("kullanım: report.nct <yıl> <çeyrek>")
} else {
    print("rapor: " + a[0] + "/" + a[1])
}
// rapor: 2026/q3
```

---

## 12. Interface apps in the browser

The embedded `std/ui.nct` library builds single-file HTML apps and opens them
in the system browser — no server, no install. `ui_page` is the shell;
`ui_heading`, `ui_button`, `ui_input`, `ui_row`, and `ui_region` fill it;
`ui_open` writes the file and opens it:

```nct
// nect run counter.nct
import "std/ui.nct"

let body = (
    ui_heading("Sayaç", "düğmeye bas")
    + ui_button("+1", "bump()")
    + ui_region("out", "0")
)
ui_open("Sayaç", body)   // writes /tmp/nect-ui-<ms>.html and opens it
print("tarayıcıda açıldı")
```

The interactivity is the JavaScript you compose; Nect builds and ships the
page. A full painting app lives in `examples/webapp.nct` — canvas, five ink
colors, clear button — in about sixty lines of Nect:

```bash
nect run examples/webapp.nct
```

---

## See also

- [`tutorial.md`](tutorial.md) — the language, chapter by chapter.
- [`reference.md`](reference.md) — grammar, operator precedence, builtin
  tables, error catalogue, and the CLI.
- [`../examples/`](../examples/) — small complete programs, all verified to
  print identical output on the interpreter, the bytecode VM, and the JIT.

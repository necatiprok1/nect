# Nect language reference

The precise version of the language described in
[the tutorial](tutorial.md): lexical structure, grammar, semantics, the
built-in library, the error catalogue, the command line, and the differences
between the two engines.

- [1. Lexical structure](#1-lexical-structure)
- [2. Grammar](#2-grammar)
- [3. Precedence and associativity](#3-precedence-and-associativity)
- [4. Values](#4-values)
- [5. Operators](#5-operators)
- [6. Statements](#6-statements)
- [7. Scope](#7-scope)
- [8. Built-in functions](#8-built-in-functions)
- [9. Error catalogue](#9-error-catalogue)
- [10. Command line](#10-command-line)
- [11. Engines and native compilation](#11-engines-and-native-compilation)
- [12. Intentional differences between the engines](#12-intentional-differences-between-the-engines)
- [13. Project layout](#13-project-layout)

---

## 1. Lexical structure

Source is UTF-8. Statements are separated by newlines or by `;`, and both are
interchangeable — a line may hold several statements. Inside `(...)` and
`[...]` newlines are ignored, so an expression may be laid out over several
lines; inside a `{ ... }` block they separate statements as usual.

Comments are `// to end of line` and `/* delimited, may span lines */`.
An unterminated `/*` is not an error; it comments out the rest of the file.

**Modules**: a line consisting solely of `import "path.nct"` splices another
file's source into the program before parsing — definitions become part of the
importing program. `import "std/..."` resolves from the standard library
embedded in the binary; other paths resolve relative to the importing file.
Each module is spliced at most once, so shared imports and import cycles are
safe, and a missing file is an error before the program runs.

**Identifiers** are ASCII: `[A-Za-z_][A-Za-z0-9_]*`.

**Number literals** are digits with an optional fraction and exponent:
`1`, `1.5`, `1.`, `1_000_000`, `1.5e2`, `2e-3`. Underscores may group digits in
any position. There is no sign (use unary `-`). Values are 64-bit floats.

**String literals** are delimited by double quotes and may span lines. The
recognised escapes are:

| Escape | Meaning |
|---|---|
| `\n` | line feed |
| `\t` | tab |
| `\r` | carriage return |
| `\\` | backslash |
| `\"` | double quote |
| `\$` | literal `$` (escapes interpolation) |
| anything else | the character itself (`\q` is `q`) |

An interpolation `${expression}` splices an expression's value into the
string; the expression may not span lines, braces nest, and an unclosed `${`
is an error. See [section 5](#5-operators-and-sugar).

Strings count characters, not bytes: `len`, indexing, `slice`, `index_of`, and
`reverse` all operate on characters.

**Keywords** (reserved, may not be used as identifiers):

```text
and  break  continue  else  false  fn  for  if  in  let  not  null  or
return  true  while
```

`and`, `or`, and `not` are alternative spellings of `&&`, `||`, and `!`
(section 3). They are reserved words and cannot be used as variable names.

**Punctuation and operators**:

```text
( ) { } [ ] , ; :
= += -= *= /= %=
== != < <= > >=
+ - * / %
&& || !
? .
```

---

## 2. Grammar

Informal EBNF. `stmt*` separated by newlines or `;`.

```text
program        = statement*
statement      = "let" name "=" expression
               | "fn" name "(" params? ")" block
               | "if" condition block ("else" ( "if" ... | block ))?
               | "while" condition block
condition      = expression
               | "for" name "in" expression block    ; arrays iterate elements, maps iterate keys
               | "return" expression?
               | "break"
               | "continue"
               | block
               | expression

block          = "{" statement* "}"
params         = name ("," name)* ","?

expression     = assignment
assignment     = lvalue assign_op assignment | conditional
lvalue         = name index_suffix*
index_suffix   = "[" expression "]"
assign_op      = "=" | "+=" | "-=" | "*=" | "/=" | "%="
conditional    = logical_or ("?" expression ":" assignment)?
logical_or     = logical_and ( ("or" | "||") logical_and )*
logical_and    = not_expr ( ("and" | "&&") not_expr )*
not_expr       = ("not" | "!") not_expr | comparison
equality       = comparison (("==" | "!=") comparison)*
comparison     = term (("<" | "<=" | ">" | ">=") term)*
term           = factor (("+" | "-") factor)*
factor         = unary (("*" | "/" | "%") unary)*
unary          = ("!" | "-") unary | postfix
postfix        = primary ( "(" arguments? ")" | "[" expression "]" | "." property )*
property       = name                        ; without "(": d.k means d["k"]
               | name "(" arguments? ")"     ; with "(": method call m(d, ...)
arguments      = expression ("," expression)* ","?
arguments      = expression ("," expression)* ","?
primary        = number | string | "true" | "false" | "null"
               | "[" elements? "]"
               | "{" map_entries? "}"     ; expression position only
               | name
               | "(" expression ")"
elements       = expression ("," expression)* ","?
map_entries    = map_entry ("," map_entry)* ","?
map_entry      = (name | expression) ":" expression   ; bare name means the string of that name
```

A `for` iterates an expression that evaluates to an array. Bodies are not scopes
(see [section 7](#7-scope)).

---

## 3. Precedence and associativity

Tightest first. All binary operators are left-associative; `? :` and `=` are
right-associative.

| Level | Operators |
|---|---|
| 1 | `f(...)`, `a[...]`, `a.m(...)` |
| 2 | unary `-`, `!` / `not` |
| 3 | `*`, `/`, `%` |
| 4 | `+`, `-` |
| 5 | `<`, `<=`, `>`, `>=` |
| 6 | `==`, `!=` |
| 7 | `&&` / `and` |
| 8 | `\|\|` / `or` |
| 9 | `? :` |
| 10 | `=`, `+=`, `-=`, `*=`, `/=`, `%=` |

Comparisons do not chain: `1 < 2 < 3` compares `true < 3` and fails with
`comparison requires numbers or strings, got boolean and number`.

---

## 4. Values

| Type | Literals | Notes |
|---|---|---|
| `number` | `1`, `1.5`, `1.` | 64-bit float; whole values print without `.0` |
| `string` | `"text"` | immutable, character-indexed |
| `boolean` | `true`, `false` | |
| `null` | `null` | result of a function without `return`, and of `print` |
| `array` | `[1, "two", [3]]` | shared reference, growable, structural equality |
| `map` | `{a: 1, "b": [2]}` | insertion-ordered key/value pairs; keys are strings, numbers, or booleans |
| `function` | `fn name(...) { ... }` | callable by name only (the bytecode VM has no function values) |

**Truthiness.** `false`, `null`, and `0` are falsy. Everything else is truthy,
including `""`, `[]`, and `{}`.

**Equality.** `==` and `!=` work on any two values. Numbers compare by value,
strings by content, arrays structurally (element-wise, recursively), maps
structurally *including entry order*, and `null` with `null`. Comparing
functions is only reachable in the reference interpreter, which compares their
names and bodies.

**Ordering.** `<`, `<=`, `>`, `>=` accept two numbers or two strings, and report
an error otherwise. With NaN every ordering comparison is `false`.

**Formatting** (`print`, `str`, `join`):

| Value | Rendered as |
|---|---|
| `2.0` | `2` |
| `2.5` | `2.5` |
| `0.1 + 0.2` | `0.30000000000000004` |
| NaN / infinity | `nan`, `inf`, `-inf` |
| `"text"` | `text` — quoted as `"text"` *inside* an array |
| `null` | `null` |
| `[1, "a", [2]]` | `[1, "a", [2]]` |
| a function (interpreter only) | `<function name>` |

Whole numbers are printed as integers while `|n| < 1e15`; beyond that the float
representation is used.

---

## 5. Operators

| Operator | Operands | Result | Errors |
|---|---|---|---|
| `+` | two numbers, or two strings | number, or concatenation | `cannot apply '+' to A and B` |
| `-`, `*`, `/`, `%` | two numbers | number | `cannot apply '-' to A and B`; `division by zero`; `modulo by zero` |
| `==`, `!=` | any two values | boolean | none |
| `<`, `<=`, `>`, `>=` | two numbers or two strings | boolean | `comparison requires numbers or strings, got A and B` |
| `&&`/`and`, `\|\|`/`or` | any two values | boolean | none (short-circuit, always boolean) |
| `!`/`not`, `-` | any value / a number | boolean / number | `cannot negate a A` |

Notes:

* `%` keeps the sign of the left operand: `-17 % 5` is `-2`.
* `/` is float division: `10 / 5` is `2`, `1 / 3` is `0.3333333333333333`.
* `&&`/`and` and `||`/`or` do **not** return their operands; `0 || "yes"` is `true`.
* `!`/`not` is defined for every type because it tests truthiness.

---

## 6. Statements

| Statement | Meaning |
|---|---|
| `let name = expression` | Declares `name` in the current scope. `let` is required before any assignment. |
| `name = expression` | Writes to an existing name; an undeclared name is a runtime error. |
| `name op= expression` | Compound assign: `+= -= *= /= %=`. |
| `target[index] = expression` | Writes an array element. |
| `target[index] op= expression` | Reads the element, applies the operator, writes it back. The index is evaluated once. |
| `if condition { ... } else if condition { ... } else { ... }` | The condition may optionally be wrapped in parentheses; braces are required. |
| `while condition { ... }` | Tests before each iteration; parentheses around the condition are optional. |
| `for name in array { ... }` | Visits each element; the loop variable is a copy. |
| `break` | Leaves the innermost enclosing loop. |
| `continue` | Skips to the next iteration of the innermost enclosing loop. |
| `return expression?` | Returns from the enclosing function; without a value it returns `null`. |
| `fn name(params) { ... }` | Declares a function. |
| `{ ... }` | A bare block: a scope, with no other effect. |
| `expression` | Evaluates and discards the value (assignments and calls are the usual uses). |

Assignments are expressions whose value is the stored value, so `let y = x = 5`
sets both to `5`, and `print(a[0] = 9)` prints `9`.

`break`, `continue`, and `return` cannot escape a function boundary: they are
rejected while compiling when no loop (for `break`/`continue`) or function (for
`return`) is open.

---

## 7. Scope

1. Top-level `let`s are globals. Functions read and write them.
2. Function parameters and `let`s inside a function body are locals of that call.
3. A bare `{ ... }` block introduces a scope. Shadowing is allowed and the inner
   declaration disappears at the closing brace.
4. `if`, `while`, and `for` bodies are **not** scopes: a `let` inside them
   declares in the enclosing scope and remains visible afterwards, but only if
   the body actually ran. Reading a name that was never declared-and-run is a
   runtime error (`undefined variable 'x'`), which is what makes an accidental
   typo in a branch visible.

```nct
let total = 0
if (0) {
    let never = 1
}
print(total)
// print(never)   // error: undefined variable 'never'
```

---

## 8. Built-in functions

Built-in names are reserved: a `fn` with the same name is ignored in favour of
the built-in, and using a built-in as a value is an error
(`cannot use 'len' as a value (it is a function)`).

| Call | Arguments | Returns |
|---|---|---|
| `print(...)` / `println(...)` | any | `null`; writes a line |
| `str(x)` | any (optional) | string |
| `num(x)` | number, numeric string, boolean, `null` | number |
| `bool(x)` | any | boolean (truthiness) |
| `type(x)` | any | type name |
| `assert(condition, message?)` | any, string | `null`; errors when falsy |
| `len(x)` | array, string, or map | number of elements/characters/entries |
| `push(a, v, ...)` | array, values | the array, after appending in place |
| `pop(a)` | non-empty array | the removed last element |
| `slice(x, start, end?)` | array or string, numbers | a copy of the range; negative indices count from the end |
| `join(a, separator?)` | array, string | string |
| `index_of(x, needle)` | array or string, value | index of the first match, or `-1` |
| `contains(x, needle)` | array or string, value | boolean |
| `split(s, separator)` | string, non-empty string | array of pieces |
| `upper(s)` / `lower(s)` / `trim(s)` | string | string |
| `replace(s, from, to)` | strings, non-empty `from` | string |
| `repeat(s, count)` | string, whole number ≥ 0 | string |
| `reverse(x)` | array (in place) or string | array or string |
| `sort(a)` | array of only numbers or only strings | the array, sorted in place |
| `sum(a)` | array of numbers | number |
| `abs`, `sqrt`, `floor`, `ceil`, `round`, `sin`, `cos`, `tan`, `log` | number | number |
| `pow(base, exponent)` | numbers | number |
| `min(...)` / `max(...)` | numbers, or one array | number |
| `int(x)` | number | the value truncated toward zero |
| `fixed(x, digits)` | number, whole number 0–100 | string: `x` with exactly `digits` decimals |
| `char(code)` | whole number 0–1114111 | the one-character string for that code point |
| `char_code(s)` | non-empty string | the code point of the first character |
| `input(prompt?)` | string (optional) | one line from stdin without the newline; `null` at end of input |
| `random()` | — | a pseudo-random number in `[0, 1)` |
| `random_int(low, high)` | whole numbers, `low <= high` | a whole number in `low..=high` |
| `seed(n)` | number | `null`; resets the random sequence deterministically |
| `range(stop)` / `range(start, stop)` / `range(start, stop, step)` | numbers | array of numbers, `stop` exclusive |
| `concat(value, ...)` | any values | one string: every value rendered as text and joined |
| `keys(d)` | map | the keys in insertion order, as an array |
| `values(d)` | map | the values in key order, as an array |
| `has(d, key)` | map, value | whether the key exists |
| `remove(d, key)` | map, key | the removed value, or `null` when absent |
| `read_file(path)` | string | the whole file as a string |
| `write_file(path, content)` | strings | `null`; creates or truncates the file |
| `open_url(target)` | string (path or URL) | `null`; opens the system browser, without waiting |
| `json_encode(value)` | any (no functions, NaN, inf) | the value as JSON text |
| `json_decode(text)` | string | the JSON value: objects are maps, arrays are arrays |
| `now()` | — | seconds since the Unix epoch, as a number |
| `sleep(seconds)` | number 0–3600 | `null`; pauses the current run |
| `args()` | — | the script arguments after the file name, as an array of strings |

Notes:

* `min`/`max` accept either several numbers or a single array.
* `range` counts with a non-zero `step`; `range(stop)` starts at `0`.
* `range` and `repeat` refuse to build more than 10,000,000 elements or
  characters, reporting the limit instead of exhausting memory.
* `sort` uses numeric order for numbers and lexicographic order for strings;
  mixed arrays are rejected.
* `keys`/`values` report the map's insertion order; `for` over a map visits
  the same keys in the same order.
* Map keys are strings, numbers, or booleans. `2` and `2.0` denote the same
  key; `null`, arrays, maps, and functions are rejected as keys.
* `slice` clamps out-of-range endpoints (`slice("hello", 9)` is `""`) but reports
  an error when `start > end`.
* `index_of` on a string returns a *character* index.
* `random`/`random_int`/`seed` share one deterministic generator: seeding with
  the same value replays the exact same sequence, in every engine — seeded
  runs are reproducible and testable. Unseeded, the state starts from the
  clock, so runs differ.
* `input()` reads a line from stdin. A prompt is written first without a
  newline; end of input yields `null` rather than an error.
* The C backend (`nect build`) rejects `input`, `random`, `random_int`,
  `seed`, `read_file`, `write_file`, `open_url`, `json_encode`, `json_decode`,
  `now`, `sleep`, and `args` with a stated reason — interactive state, the
  filesystem, and JSON have no numeric C translation. The JIT skips functions
  that call them. Programs keep running on the VM.
* `json_encode` emits objects in the map's insertion order, so the same value
  always encodes to the same text. `json_decode` accepts any whitespace
  between tokens and reports the position of the first problem.
* `write_file` and `open_url` act on the outside world: in `nect run -` the
  paths are relative to the working directory.

---

## 9. Error catalogue

### Syntax errors

Reported while parsing, with the offending line, a caret, and the message. The
program does not start.

```text
3 | let x = (1 + 2
  |               ^
error: expected ')' after expression — found 'end of input'
```

Common messages: `expected an expression, found '...'`,
`expected ')' after arguments — found '...'`,
`expected '{' to start block — found '...'`,
`expected '=' after variable name — found '...'`,
`expected ':' in a conditional expression — found '...'`,
`unexpected character '@'`, `unterminated string`.

### Runtime errors

Reported as `error: MESSAGE`, and the program exits with status 1.

| Message | Cause |
|---|---|
| `undefined variable 'x'` | reading or assigning a name that was never declared |
| `undefined function 'x'` | calling a name that is neither a function nor a built-in |
| `cannot use 'x' as a value (it is a function)` | reading a built-in as a value |
| `function 'f' expects N argument(s), got M` | wrong call arity |
| `cannot apply '+' to A and B` | `+` on a non-matching pair |
| `cannot apply '-' to A and B` (and `*`, `/`, `%`) | arithmetic on non-numbers |
| `division by zero`, `modulo by zero` | divisor of `0` |
| `comparison requires numbers or strings, got A and B` | ordering mixed or non-ordered values |
| `cannot negate a A` | unary `-` on a non-number |
| `array index N out of bounds (length L)` | index outside the array, negatives included |
| `string index N out of bounds (length L)` | index outside the string |
| `array index must be a number, got A` | non-numeric index |
| `cannot index into A` | indexing a value that is not an array, string, or map |
| `strings are immutable: cannot assign to a string index` | `s[0] = "x"` |
| `for loop requires an array or map` | `for` over a non-iterable |
| `map key K not found` | reading or compound-assigning an absent key |
| `map keys must be strings, numbers, or booleans, got A` | a map literal, write, or insert with an unhashable key |
| `keys() requires a map, got A` (also `values`, `has`, `remove`) | map built-in on a non-map |
| `'break' outside of a loop` / `'continue' outside of a loop` | keyword outside any loop |
| `'return' outside of a function` | `return` at the top level |
| `assertion failed` / `assertion failed: MESSAGE` | failed `assert` |
| `cannot convert 'x' to number` | `num` on a non-numeric string |
| `sqrt() is not defined for N`, `log() is not defined for N` | domain error |
| `pop() on an empty array` | |
| `sort() requires an array of only numbers or only strings` | mixed array |
| `range() requires a non-zero step`, `range() would build N elements (limit 10000000)` | |
| `repeat() would build N characters (limit 10000000)` | |
| `split() requires a non-empty separator` | |
| `cannot convert an array to number` | `num` on an array |
| `cannot convert a map to number` | `num` on a map |

---

## 10. Command line

```text
nect run [--interp] <file>   Run a program (file, or `-` for stdin)
nect check <file>            Parse only; report syntax errors
nect disasm <file>           Bytecode plus native-compilation decisions
nect build <file>            Compile to a standalone native executable
      -o <path>               Where to write it (default: the file's stem)
      --cc <compiler>         C compiler to use (default: $CC, then cc)
      --emit-c                Print the generated C instead of building
      --keep-c                Keep the generated C next to the executable
nect --version | -V
nect --help | -h | help

NECT_NO_JIT=1                Disable native compilation (bytecode VM only)
CC                            C compiler used by `nect build`
```

`run` exits `0` on success and `1` on any error. Errors go to stderr; program
output goes to stdout. `build` exits `0` when the executable was written and `1`
when the program is outside the translatable subset or the C compiler failed —
its reason says which.

---

## 11. Engines and native compilation

There are two implementations of the same language:

* **Bytecode VM** (default). Compiles the AST to a compact instruction set with
  interned identifiers, flat local slots, fused three-address opcodes, and a
  numeric fast path. It also hosts the JIT.
* **Reference interpreter** (`--interp`). The original tree-walking evaluator,
  kept for cross-checking. It recurses on the host stack.

**Native compilation.** When Cranelift initialises, the VM compiles to machine
code the parts of a program that are (a) provably numeric — no strings, arrays,
built-ins, globals the compiler cannot prove are numbers, division, or modulo —
and (b) actually repeated: a function called from a loop or by itself, or a
module-level numeric prefix that contains a loop. Everything else stays in
bytecode, and the two must produce identical output. `nect disasm` reports the
decision per function with a reason:

```text
=== inferred types / native compilation ===
--- module: bytecode only (calls a builtin)
--- fn numeric: numeric, jit-eligible
    slot 0: number
--- fn textual: bytecode only (loads a non-numeric constant)
--- fn divides: bytecode only (divides)
```

Typical reasons: `calls a builtin`, `loads a non-numeric constant`, `divides`,
`touches a global`, `uses arrays`, `uses maps`, `iterates a map`,
`uses short-circuit logic`, `reads a conditional declaration`,
`no reachable return`.

Division and modulo stay in bytecode because a zero divisor has to raise a
runtime error, which native code cannot do without unwinding; keeping those
operations in the VM keeps error behaviour identical.

`tests/differential_tests.rs` runs a corpus, every example, and every benchmark
through the interpreter, the bytecode VM, and the JIT-driven VM, comparing
stdout, stderr, and exit status exactly. See `PERFORMANCE_PLAN.md` for the
measurements behind the design.

### Ahead-of-time compilation to C (`nect build`)

`nect build` takes the same bytecode and emits a single C file — the program
plus a small runtime — and compiles it with the system C compiler
(`-O2 -ffp-contract=off`) into a standalone executable. The contract is
*behavioural parity*: the binary must print the same stdout, stderr, and exit
status as `nect run`, including the text of runtime errors. `tests/aot_tests.rs`
builds every benchmark and a corpus of edge cases and compares.

What translates: numbers and booleans (as `double`), arithmetic and
comparisons, `if`/`while`/`for`/`break`/`continue`, `&&`/`||`/`?:`, function
calls and recursion, `print`/`str`/`abs`/`sqrt`/`floor`/`ceil`/`round`/`min`/
`max`/`pow`/`sin`/`cos`/`tan`/`log`, and strings **used inside expressions** —
a string may be concatenated, compared, and printed, but not stored in a
variable or passed across a call. Modulo, arrays, maps, and every other
built-in are outside the subset.

```text
$ nect build benches/heavy/dot_product.nct
built dot_product
$ ./dot_product
dot product total: 6333892543.821902

$ nect build examples/arrays.nct
error: the module body uses an array literal which the C backend cannot translate
```

A rejection is not an error in the program: it only means the C backend cannot
type it. The program keeps running on the VM, whose JIT covers a wider subset
(including arrays, division, and every built-in). Strings print identically
because the emitted runtime reproduces the VM's number formatting exactly —
whole values without a decimal point, otherwise the shortest decimal that
round-trips, `nan`/`inf` spelled out.

---

## 12. Intentional differences between the engines

These are accepted and pinned by `documented_divergences` in
`tests/differential_tests.rs`, so a change to them fails a test:

1. **Functions as values.** The interpreter treats functions as first-class
   values (`print(f)` prints `<function f>`); the VM resolves calls statically
   and reports `undefined variable 'f'`.
2. **Calling an unknown name.** The VM reports `undefined function 'nope'` while
   compiling; the interpreter reports `undefined variable 'nope'` at runtime.
3. **Nested `fn` timing.** In the VM a nested function becomes callable as soon
   as its enclosing function has been compiled; the interpreter defines it only
   when the enclosing function runs.
4. **Calling a non-function.** `a[0](1)` or `x()` on a non-function reports
   `calling non-variable expressions is not yet supported` in the VM (a compile
   error) and `can only call functions` in the interpreter (a runtime error).
5. **Recursion depth.** The VM keeps frames on the heap and handles very deep
   recursion; the interpreter recurses on the host stack and overflows after a
   few thousand calls (fewer in a debug build), which aborts the process.

---

## 13. Project layout

```text
src/ast.rs (src/ast/mod.rs)   Syntax tree and values
src/lexer/mod.rs              UTF-8 lexer
src/parser/mod.rs             Recursive-descent parser
src/builtins.rs               Shared value semantics + built-in library
src/interpreter/mod.rs        Reference tree-walking interpreter
src/vm/mod.rs                 Bytecode compiler + VM
src/jit/mod.rs                Cranelift native compilation and type inference
src/aot/mod.rs                C backend for `nect build` (bytecode -> C)
src/cli/mod.rs                Command line, source maps, disassembler
docs/tutorial.md              This language, taught
docs/reference.md             This file
examples/*.nct                Runnable examples used by the test suite
benches/*.nct                 Benchmarks with Python counterparts (benches/heavy/:
                              compute-bound AI kernels)
tests/                        Unit, golden-output, differential, and AOT tests
```

```bash
cargo test                 # unit + golden + differential + AOT + benchmark tests
cargo clippy --all-targets # lints
cargo build --release      # ./target/release/nect
```

`AGENTS.md` covers the project conventions and the performance work;
`PERFORMANCE_PLAN.md` records the optimizations and their measurements.

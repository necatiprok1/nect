# Nect Language Specification

> **Status:** Official specification, version 1.0
>
> This document is the authoritative source of truth for the Nect programming
> language. It is derived from and verified against the reference implementation
> in `src/builtins.rs` (shared runtime semantics), `src/parser/mod.rs`,
> `src/lexer/mod.rs`, and `src/ast/mod.rs`.
>
> Observable behavior — stdout, stderr, and exit status — is validated by
> `tests/differential_tests.rs` (interpreter, bytecode VM, and JIT engines must
> agree) and `tests/vm_output_tests.rs` (golden-output pinning).

- [1. Lexical Structure](#1-lexical-structure)
- [2. Grammar](#2-grammar)
- [3. Precedence and Associativity](#3-precedence-and-associativity)
- [4. Values](#4-values)
- [5. Operators](#5-operators)
- [6. Statements](#6-statements)
- [7. Scope](#7-scope)
- [8. Standard Library](#8-standard-library)
- [9. Errors](#9-errors)
- [10. Command Line](#10-command-line)
- [11. Engines and Compilation](#11-engines-and-compilation)
- [12. Intentional Engine Divergences](#12-intentional-engine-divergences)
- [13. Implementation Notes](#13-implementation-notes)

---

## 1. Lexical Structure

Source files are encoded in UTF-8. The lexer (`src/lexer/mod.rs`) tokenizes
source into a flat token stream; a newline-folding pass in the parser
(`fold_expressions_over_lines`) then drops newlines inside `(...)`, `[...]`, and
map-literal braces, so expressions may be laid out across multiple lines.

### 1.1 Comments

```text
// line comment — to end of line
# line comment — same effect (Python/shell familiarity)
/* block comment — may span lines */
```

An unterminated `/*` comments out the rest of the file. Comments produce no
tokens.

### 1.2 Newlines and statement separators

Statements are separated by newlines or by `;`. Both are interchangeable: a
single line may hold multiple statements separated by `;`. Newlines inside
parentheses `(...)`, brackets `[...]`, or map-literal braces `{...}` are
suppressed by the folding pass. Inside a block `{ ... }`, newlines always serve
as statement separators.

### 1.3 Identifiers

Identifiers match `[A-Za-z_][A-Za-z0-9_]*`. They are ASCII-only. Identifier
matching is case-sensitive.

### 1.4 Reserved keywords

The following tokens are keywords and may not be used as identifiers:

```text
and    break    continue  else     false    fn      for      if
in     let      not       null     or       return  true     while
```

`and`, `or`, and `not` are reserved word spellings of the logical operators
`&&`, `||`, and `!` (see section 5.4). They are lexed as distinct tokens
(`AndWord`, `OrWord`, `NotWord`) that the parser treats identically to their
symbol forms.

### 1.5 Numbers

Number literals match the grammar:

```text
number      = digits ( "." digits? )? ( exponent )?
exponent    = ("e" | "E") ("+" | "-")? digits
digits      = digit_group ( "_" digit_group )*
digit_group = digit+
```

Examples: `1`, `1.5`, `1.`, `1_000_000`, `1.5e2`, `2e-3`, `2E+4`.

- Underscores (`_`) may appear between digit groups for readability; they are
  stripped before parsing. An underscore must be between two digits
  (`1_` is a lex error; `_1` is an identifier).
- There is no sign in literals — use unary `-` (section 5.2).
- All numbers are 64-bit IEEE 754 double-precision floats.
- An identifier like `e5` or `else` is not confused with exponent notation:
  the `e`/`E` exponent is only consumed when followed by an optional sign and
  at least one digit.

### 1.6 Strings

String literals are delimited by double quotes (`"..."`). They may span multiple
lines. The following escape sequences are recognized:

| Escape  | Meaning           |
|---------|-------------------|
| `\n`    | line feed (U+000A)|
| `\t`    | tab (U+0009)      |
| `\r`    | carriage return   |
| `\\`    | backslash         |
| `\"`    | double quote      |
| `\$`    | literal `$`       |
| `\x`    | literal `x`       |

Any other backslash followed by a character yields that character itself
(`"\q"` is `"q"`). A `$` is only special when immediately followed by `{`; a
bare `$` outside a string is a lex error.

Strings count **characters**, not bytes. `len`, indexing, `slice`, `index_of`,
and `reverse` all operate on Unicode code points. Invalid UTF-8 bytes inside a
string literal are replaced with U+FFFD rather than failing the lex.

### 1.7 String interpolation

An interpolation `${expression}` inside a string literal splices the expression's
value (rendered as text) into the string. The lexer records the expression's
source text between sentinel markers (`\u{1}NECT-INTERP\u{1}...`); the parser
(`expand_interpolation`) splits the literal on these markers, lexes and parses
each expression in place, and produces a `concat(...)` call. A string without
`${...}` remains a plain string literal.

Rules:
- The interpolated expression **may not span multiple lines**.
- Braces nest: `${a[b(c)}]}` is parsed correctly. The depth counter tracks
  `{` and `}` so the first matching `}` closes the interpolation.
- An unclosed `${` is a lex error: `"unterminated '${' in a string"`.
- An empty interpolation `${}` is a parse error: `"empty interpolation: put an expression inside '${}'"`.
- `\$` produces a literal `$` and prevents interpolation.

### 1.8 Punctuation and operators

```text
()  {}  []  ,   ;   :
=  +=  -=  *=  /=  %=
==  !=  <  <=  >  >=
+  -  *  /  %
&&  ||  !
?  .
```

Single-character tokens: `(`, `)`, `{`, `}`, `[`, `]`, `,`, `;`, `:`, `?`, `.`.

Two-character operators are greedy: `++`, `==`, `!=`, `<=`, `>=`, `+=`, `-=`,
`*=`, `/=`, `%=`. Single `&` or `|` without a following matching character is a
lex error.

---

## 2. Grammar

Informal EBNF. The top level is a sequence of statements separated by newlines
or semicolons (after newline folding).

```text
program        = statement*

statement      = "let" name "=" expression
               | "fn" name "(" params? ")" block
               | "if" condition block ("else" ("if" ... | block))?
               | "while" condition block
               | "for" name "in" expression block
               | "return" expression?
               | "break"
               | "continue"
               | block
               | expression

block          = "{" statement* "}"
params         = name ("," name)* ","?

condition      = expression
               | "for" name "in" expression block

expression     = assignment
assignment     = lvalue assign_op assignment | conditional
lvalue         = name index_suffix*
index_suffix   = "[" expression "]"
assign_op      = "=" | "+=" | "-=" | "*=" | "/=" | "%="
conditional    = logical_or ("?" expression ":" assignment)?
logical_or     = logical_and (("or" | "||") logical_and)*
logical_and    = not_expr (("and" | "&&") not_expr)*
not_expr       = ("not" | "!") not_expr | comparison
equality       = comparison (("==" | "!=") comparison)*
comparison     = term (("<" | "<=" | ">" | ">=") term)*
term           = factor (("+" | "-") factor)*
factor         = unary (("*" | "/" | "%") unary)*
unary          = ("!" | "-" | "not") unary | postfix
postfix        = primary (call_suffix | index_suffix | method_suffix)*
call_suffix    = "(" arguments? ")"
method_suffix  = "." name (call_suffix)?
arguments      = expression ("," expression)* ","?
primary        = number | string | "true" | "false" | "null"
               | "[" elements? "]"
               | "{" map_entries? "}"   ; expression-position only
               | name
               | "(" expression ")"
elements       = expression ("," expression)* ","?
map_entries    = map_entry ("," map_entry)* ","?
map_entry      = (name | expression) ":" expression
```

A `{` in statement position starts a bare block; a `{` in expression position
(starting an expression or following `=`, `,`, `:`, `?`, `(`, `[`, a comparison
or assignment operator) starts a map literal. The newline-folding pass
(`expression_continues`) determines whether a `{` increases the bracket depth, so
`{ }` at the start of a statement remains a bare block.

### 2.1 `if` / `else if` / `else`

```nct
if condition {
    body
} else if condition2 {
    body2
} else {
    body3
}
```

`else if` desugars to a nested `if` in the else branch. The `else` keyword must
immediately follow the closing brace on the same line: `} else {`.

### 2.2 `while` and `for`

`while condition { body }` — tests before each iteration. The condition may
optionally be wrapped in parentheses.

`for name in expression { body }` — iterates an array, visiting each element in
order. Iterating a map visits its keys in insertion order.

---

## 3. Precedence and Associativity

Tightest-binding operators first. All binary operators are left-associative
except `?` and `=`, which are right-associative.

| Level | Operators                              |
|-------|----------------------------------------|
| 1     | `f(...)`, `a[...]`, `a.m(...)`         |
| 2     | unary `-`, `!` / `not`               |
| 3     | `*`, `/`, `%`                          |
| 4     | `+`, `-`                               |
| 5     | `<`, `<=`, `>`, `>=`                   |
| 6     | `==`, `!=`                             |
| 7     | `&&` / `and`                           |
| 8     | `\|\|` / `or`                          |
| 9     | `? :`                                  |
| 10    | `=`, `+=`, `-=`, `*=`, `/=`, `%=`      |

Comparisons do not chain: `1 < 2 < 3` evaluates `(1 < 2) < 3` = `true < 3`,
which raises a type error because `true` is not a number or string.

---

## 4. Values

Nect has eight value types. A value's type can be inspected with `type(x)`,
which returns one of: `"number"`, `"string"`, `"boolean"`, `"null"`, `"array"`,
`"map"`, `"function"`, and other engine-specific types (see 4.7).

### 4.1 Numbers

- **Representation:** 64-bit IEEE 754 double.
- **No integer type.** All arithmetic is floating-point.
- **Printing:** Whole numbers with `|n| < 1e15` print as integers (`2`, not `2.0`).
  Larger magnitudes and non-whole values print with the shortest representation
  that round-trips (Rust's `Display` for `f64`). NaN prints as `nan`, positive
  infinity as `inf`, negative as `-inf`.

### 4.2 Strings

- **Representation:** immutable, UTF-8, character-indexed.
- A bare string prints as its contents (no quotes). Inside an array or map,
  strings are quoted with escaped `\\` and `\"`.

### 4.3 Booleans

- `true` and `false`. Print as `true` / `false`.

### 4.4 Null

- `null` represents the absence of a value. The return value of a function with
  no `return` statement, and of `print`/`println`, is `null`.

### 4.5 Arrays

- Ordered, growable, heterogeneous. `let a = [1, "two", [3, 4]]`.
- **Shared reference semantics:** assignment and parameter passing copy the
  reference, not the contents. Mutations through one alias are visible through
  all aliases.
- Structural equality: two arrays are equal if they have the same length and
  equal elements at each position (recursive).
- Print format: `[elem1, elem2, ...]` with strings quoted inside.
- `len(a)` returns the element count.

### 4.6 Maps

- Key-value stores with insertion-ordered entries.
- **Keys:** strings, numbers, or booleans. `2` and `2.0` are the same key
  (numbers are normalized). `null`, arrays, maps, and functions are rejected
  as keys.
- **Insertion order is observable:** printing a map and `for` iteration both
  follow insertion order. Overwriting an existing key preserves its position;
  deleting and re-inserting moves it to the end.
- **Plain assignment** `d[k] = v` inserts or overwrites. **Compound
  assignment** `d[k] += v` reads first — the key must exist, otherwise error.
- `d.key` is shorthand for `d["key"]` for both reads and writes.
- Structural equality **includes entry order**: `{a: 1, b: 2} != {b: 2, a: 1}`.
- Print format: string keys are always quoted (`"key"`), number and boolean
  keys are printed bare.
- `len(d)` returns the entry count.

### 4.7 Truthiness

Only three values are falsy:

| Value        | Truthy? |
|--------------|---------|
| `false`      | No      |
| `null`       | No      |
| `0` (and `-0`)| No      |
| `""`         | **Yes** |
| `[]`         | **Yes** |
| `{}`         | **Yes** |
| everything else | Yes |

### 4.8 Functions

- Declared with `fn name(params) { body }`.
- In the bytecode VM, functions are **not first-class values**: calls are
  resolved at compile time by name. Reading a function name as a value is an
  error (`cannot use 'name' as a value (it is a function)`).
- Functions support recursion and may call themselves.
- A function returns `null` if execution falls off the end or hits a bare
  `return`.
- The reference interpreter treats functions as first-class values (prints as
  `<function name>`); this is an intentional divergence (section 12).

### 4.9 Equality and Ordering

- **Equality** (`==`, `!=`): works on any two values. Numbers compare by value,
  strings by content, arrays structurally (element-wise, recursively), maps
  structurally including entry order, booleans by value, `null` with `null`.
  Comparing functions is only reachable in the interpreter.
- **Ordering** (`<`, `<=`, `>`, `>=`): accept two numbers or two strings,
  erroring otherwise. With NaN, every ordering comparison is `false`.
- Logical operators (`&&`, `||`, `!`) always return a boolean: `0 || "yes"`
  is `true`, not `"yes"`.

---

## 5. Operators

### 5.1 Arithmetic

| Operator | Operands        | Result     |
|----------|-----------------|------------|
| `+`      | two numbers     | sum        |
| `+`      | two strings     | concatenation |
| `-`      | two numbers     | difference |
| `*`      | two numbers     | product    |
| `/`      | two numbers     | quotient   |
| `%`      | two numbers     | remainder  |

- `-` on non-numbers: `cannot apply '-' to {type} and {type}`.
- `/` by zero: `division by zero`.
- `%` by zero: `modulo by zero`. The sign of the result follows the left operand
  (`-17 % 5` is `-2`), as in C and Rust.

### 5.2 Unary

| Operator | Operand | Result  | Error on non-number |
|----------|---------|---------|---------------------|
| `-`      | number  | negation| `cannot negate a {type}` |
| `!`      | any     | boolean | none (tests truthiness) |

### 5.3 Comparison

| Operator | Operands                    | Errors                                   |
|----------|-----------------------------|------------------------------------------|
| `<` `>` `<=` `>=` | two numbers or two strings | `comparison requires numbers or strings, got {a} and {b}` |
| `==` `!=`         | any two values             | none                                     |

NaN: every ordering comparison returns `false` for NaN operands.

### 5.4 Logical (short-circuit)

| Operator | Behavior                                           |
|----------|----------------------------------------------------|
| `&&` / `and` | True iff both operands are truthy. Right operand evaluated only if left is truthy. Result is always `true` or `false`. |
| `\|\|` / `or`  | True iff at least one operand is truthy. Right operand evaluated only if left is falsy. Result is always `true` or `false`. |
| `!` / `not`   | Logical negation of truthiness.                    |

These operators always return a boolean, never one of their operands.

### 5.5 Conditional

`condition ? then_expr : else_expr` — only the taken branch is evaluated.
Right-associative: `a ? b : c ? d : e` parses as `a ? b : (c ? d : e)`.

### 5.6 Assignment

| Form              | Effect                                            |
|-------------------|---------------------------------------------------|
| `name = expr`     | Store value into an existing variable.            |
| `name op= expr`   | Read, apply `op`, store back. `+=` etc.           |
| `target[idx] = v` | Write an array element or map key.               |
| `target[idx] op=` | Read element, apply `op`, write back.            |
| `d.key = v`       | Shorthand for `d["key"] = v`.                    |

Assignment is an **expression**: its value is the stored value. This permits
`let y = x = 5` and `print(a[0] = 9)`.

Compound assignment on a plain name desugars to a binary operation:
`x += 2` becomes `x = x + 2`. On an indexed target, the operator is preserved
on the `SetIndex` AST node so the target and index are evaluated exactly once.

Assignment to an undeclared name is a runtime error: `undefined variable 'name'`.
Assignment to a string index is an error: `strings are immutable: cannot assign to a string index`.

---

## 6. Statements

| Statement                              | Meaning                                                        |
|----------------------------------------|----------------------------------------------------------------|
| `let name = expression`                | Declare a new variable in the current scope.                   |
| `name = expression`                    | Assign to an existing variable.                                |
| `name op= expression`                  | Compound assignment.                                           |
| `target[index] = expression`            | Element assignment.                                            |
| `target[index] op= expression`         | Element compound assignment (index evaluated once).            |
| `if cond { ... } else if cond2 { ... } else { ... }` | Conditional. Parentheses around condition optional.  |
| `while cond { ... }`                   | Loop; parentheses around condition optional.                   |
| `for name in expr { ... }`             | Iterate an array (elements) or map (keys).                     |
| `return expr?`                         | Return from function; bare `return` yields `null`.            |
| `break`                                | Exit the innermost enclosing loop.                             |
| `continue`                             | Skip to the next iteration of the innermost loop.              |
| `fn name(params) { ... }`              | Declare a function.                                            |
| `{ ... }`                              | Bare block: a new scope.                                       |
| `expression`                           | Evaluate and discard (for calls and assignments).              |

`break` and `continue` are rejected at compile time when no loop is open:
`'break' outside of a loop` / `'continue' outside of a loop`.

`return` outside a function: `'return' outside of a function`.

---

## 7. Scope

There are two kinds of scope:

1. **Module (global) scope** — top-level `let` declarations. Functions read and
   write globals declared before the call.
2. **Function-local scope** — parameters and `let`s inside a function body.

### 7.1 Bare blocks

A bare `{ ... }` block introduces a new scope. Declarations inside it (including
shadows of outer names) disappear at the closing brace.

### 7.2 Control-flow bodies are NOT scopes

`if`, `while`, and `for` bodies do **not** introduce a new scope. A `let`
inside them declares in the enclosing scope. The declaration remains visible
after the body, **but only if the body actually ran**. Reading a name whose
declaration was in a branch that never executed is a runtime error:
`undefined variable 'name'`.

```nct
if (0) {
    let never = 1
}
print(never)   // error: undefined variable 'never'
```

### 7.3 Shadowing

`let` always creates a fresh binding, even if a name with the same identifier
exists in an outer scope. The inner binding is visible until its scope ends.

---

## 8. Standard Library

Built-in names are reserved — a `fn` with the same name is ignored (the built-in
takes precedence), and reading a built-in as a value is an error.

Built-in names are defined in `src/builtins.rs` in `NAMES`. The complete list,
grouped by category:

### 8.1 Output

| Call         | Arguments | Returns | Description |
|--------------|-----------|---------|-------------|
| `print(...)` | any       | `null`  | Writes args joined by spaces, then a newline. |
| `println(...)`** | any   | `null`  | Alias of `print`. |

### 8.2 Conversion and inspection

| Call           | Arguments | Returns | Description |
|----------------|-----------|---------|-------------|
| `str(x)`       | any       | string  | Formats a value as text. `str()` is `""`. |
| `num(x)`       | number, numeric string, boolean, null | number | Numeric string is trimmed before parsing. `num(true)`=1, `num(false)`=0, `num(null)`=0. |
| `bool(x)`      | any       | boolean | Truthiness of `x`. |
| `type(x)`      | any       | string  | Type name. |
| `assert(c, msg?)` | any, string | `null` | Errors with `assertion failed: msg` when `c` is falsy. |
| `concat(v, ...)` | any values | string | Renders each value as text and joins them. |

- `num` on a non-numeric string: `cannot convert 'x' to number`.
- `num` on an array: `cannot convert an array to number`.
- `num` on a map: `cannot convert a map to number`.
- `assert()` with wrong arity: `assert() requires a condition and an optional message`.

### 8.3 Strings and arrays

| Call           | Arguments | Returns | Description |
|----------------|-----------|---------|-------------|
| `len(x)`       | array, string, or map | number | Element/character/entry count. |
| `push(a, v, ...)` | array, values | array | Appends values in place, returns the array. |
| `pop(a)`       | non-empty array | value | Removes and returns the last element. |
| `join(a, sep?)` | array, string | string | Elements rendered as text, joined. |
| `slice(x, start, end?)` | array or string, numbers | copy | Negative indices count from end; clamped, not errors. |
| `index_of(x, needle)` | array or string, value | number | Character/element index of first match, or `-1`. |
| `contains(x, needle)` | array or string, value | boolean | Whether needle is present. |
| `split(s, sep)` | string, non-empty string | array | Pieces between separators. |
| `upper(s)` / `lower(s)` | string | string | Case conversion. |
| `trim(s)`      | string | string | Remove leading/trailing whitespace. |
| `replace(s, from, to)` | strings, non-empty `from` | string | Replace all occurrences. |
| `repeat(s, count)` | string, whole number ≥ 0 | string | Repeat string. |
| `reverse(x)`   | array (in place) or string | array or string | Mutates array, copies string. |
| `sort(a)`      | array of all numbers or all strings | array | In-place sort. |
| `sum(a)`       | array of numbers | number | Sum of elements. |
| `range(...)`   | numbers | array | `range(stop)` or `range(start, stop)` or `range(start, stop, step)`. |

Limits and errors:
- `slice` errors if `start > end`: `slice() start index {start} is past its end index {end}`.
- `index_of` on a string returns a **character** index, not a byte index.
- `sort` on a mixed array: `sort() requires an array of only numbers or only strings`.
- `pop` on empty array: `pop() on an empty array`.
- `range` with step 0: `range() requires a non-zero step`.
- `range` / `repeat` refuse more than 10,000,000 elements/characters.
- `split` with empty separator: `split() requires a non-empty separator`.
- `replace` with empty pattern: `replace() requires a non-empty pattern`.

Number indices into arrays and strings are truncated toward zero
(`floor` for positive, toward zero for negative). Out-of-range indices raise:
`array index {n} out of bounds (length {len})` /
`string index {n} out of bounds (length {len})`.

### 8.4 Maps

| Call           | Arguments | Returns | Description |
|----------------|-----------|---------|-------------|
| `keys(d)`      | map | array | Keys in insertion order. |
| `values(d)`    | map | array | Values in insertion order. |
| `has(d, key)`  | map, value | boolean | Whether key exists. |
| `remove(d, key)` | map, key | value | Removed value, or `null`. |

`keys()`/`values()`/`has()`/`remove()` on a non-map:
`{name}() requires a map, got {type}`.

Map key lookup on a missing key: `map key {key} not found`.

### 8.5 Math

| Call        | Arguments | Returns | Description |
|-------------|-----------|---------|-------------|
| `abs(n)`    | number | number | Absolute value. |
| `sqrt(n)`   | number | number | Square root; error if `n < 0`. |
| `floor(n)` / `ceil(n)` / `round(n)` | number | number | Rounding. |
| `sin(n)` / `cos(n)` / `tan(n)` | number | number | Trigonometric (radians). |
| `log(n)`    | number | number | Natural log; error if `n <= 0`. |
| `pow(b, e)` | numbers | number | Exponentiation. |
| `min(...)` / `max(...)` | numbers, or one array | number | Minimum/maximum. |
| `int(n)`    | number | number | Truncation toward zero. |
| `fixed(n, d)` | number, 0–100 | string | `n` with exactly `d` decimals. |

- `sqrt(n)` where `n < 0`: `sqrt() is not defined for {n}`.
- `log(n)` where `n <= 0`: `log() is not defined for {n}`.
- `min()`/`max()` with no args: `{name}() requires arguments`.
- `fixed` with digits out of range: `fixed() requires a digit count between 0 and 100`.

### 8.6 Characters and code points

| Call           | Arguments | Returns | Description |
|----------------|-----------|---------|-------------|
| `char(code)`   | whole number 0–1,114,111 | string | One-character string for a code point. |
| `char_code(s)` | non-empty string | number | Code point of first character. |

- `char` out of range or non-integer: `char() requires a whole code point in 0..=1114111, got {n}`.
- `char_code` on empty string: `char_code() requires a non-empty string`.

### 8.7 Randomness

Shared XORSHIFT64* state, defined in `src/builtins.rs`:

| Call            | Arguments | Returns | Description |
|-----------------|-----------|---------|-------------|
| `random()`      | —         | number  | Uniform in `[0, 1)`. |
| `random_int(low, high)` | whole numbers, `low <= high` | number | Whole number in `low..=high`. |
| `seed(n)`       | number    | `null`  | Reseed the generator deterministically. |

- Same seed → same sequence across all engines (enables reproducible testing).
- Unseeded: state initializes from the wall clock, so runs differ.
- `random_int` with fractional bounds: `random_int() requires whole-number bounds`.
- `random_int` with `low > high`: `random_int() requires low <= high, got {low} > {high}`.

### 8.8 Input

| Call           | Arguments | Returns | Description |
|----------------|-----------|---------|-------------|
| `input(prompt?)` | string (optional) | string or null | One stdin line without `\n`; `null` at EOF. |

The prompt (if given) is written without a trailing newline. EOF yields `null`.

### 8.9 Files, time, and JSON

| Call             | Arguments | Returns | Description |
|------------------|-----------|---------|-------------|
| `read_file(path)` | string | string | Entire file contents. |
| `write_file(path, content)` | strings | `null` | Creates/truncates file. |
| `json_encode(value)` | any (no functions/NaN/inf) | string | JSON text. |
| `json_decode(text)` | string | value | JSON → Nect value (objects become maps). |
| `now()`          | —          | number | Seconds since Unix epoch. |
| `sleep(seconds)` | 0–3600    | `null` | Pause execution. |
| `args()`         | —          | array of strings | Script arguments after the file name. |

- `json_encode` on NaN/infinity: `json_encode() cannot represent NaN or infinity`.
- `json_encode` on a function: `json_encode() cannot represent the function '{name}'`.
- `json_decode` reports the position of the first problem: `json_decode(): {reason} at position {pos}`.
- `sleep` outside 0–3600: `sleep() requires a duration between 0 and 3600 seconds`.

### 8.10 Method-call sugar

`x.method(args)` desugars to `method(x, args)`. Chaining is left-to-right:
`s.trim().upper()` is `upper(trim(s))`. Every builtin is reachable this way.

### 8.11 Compilation-rejected builtins

The C backend (`nect build`) rejects: `input`, `random`, `random_int`, `seed`,
`read_file`, `write_file`, `open_url`, `json_encode`, `json_decode`, `now`,
`sleep`, `args`. The JIT skips functions that call them. Programs keep running
on the VM. A rejection carries a stated reason but is not a program error.

---

## 9. Errors

There are two error classes: **syntax errors** (parse time) and **runtime
errors** (execution time).

### 9.1 Syntax errors

Reported before the program starts. Format:

```text
{line} | {source line}
  | {caret}
error: {message}
```

Common messages:
- `expected an expression, found '{token}'`
- `expected ')' after arguments — found '{token}'`
- `expected '{' to start block — found '{token}'`
- `expected '=' after variable name — found '{token}'`
- `expected ':' in a conditional expression — found '{token}'`
- `unexpected character '{c}'`
- `unterminated string`
- `unterminated '${' in a string` (lex)
- `an interpolated expression cannot span lines` (lex)
- `empty interpolation: put an expression inside '${}'` (parse)
- `an underscore in a number must be between digits` (lex)
- `expected a digit after the decimal point` (lex)

Exit code: `1`. Error on stderr; no stdout.

### 9.2 Runtime errors

Format: `error: {message}` on stderr, exit code `1`.

A complete catalogue of runtime error messages:

| Message | Trigger |
|---------|---------|
| `undefined variable '{name}'` | Reading or assigning an undeclared name. |
| `undefined function '{name}'` | Calling an unknown name (VM reports at compile time; interpreter at runtime). |
| `cannot use '{name}' as a value (it is a function)` | Reading a built-in as a value. |
| `function '{name}' expects {n} argument(s), got {m}` | Wrong call arity. |
| `cannot apply '{op}' to {a} and {b}` | Arithmetic/type mismatch. |
| `division by zero` | Divisor of `0` in `/`. |
| `modulo by zero` | Divisor of `0` in `%`. |
| `comparison requires numbers or strings, got {a} and {b}` | Ordering mixed types. |
| `cannot negate a {type}` | Unary `-` on non-number. |
| `array index {i} out of bounds (length {l})` | Array index out of range. |
| `string index {i} out of bounds (length {l})` | String index out of range. |
| `array index must be a number, got {type}` | Non-numeric array index. |
| `strings are immutable: cannot assign to a string index` | `s[i] = v`. |
| `for loop requires an array or map` | `for` over a non-iterable. |
| `map key {k} not found` | Reading or compound-assigning a missing map key. |
| `map keys must be strings, numbers, or booleans, got {type}` | Unhashable map key. |
| `'break' outside of a loop` | `break` with no enclosing loop. |
| `'continue' outside of a loop` | `continue` with no enclosing loop. |
| `'return' outside of a function` | `return` at top level. |
| `assertion failed` | `assert(false)`. |
| `assertion failed: {message}` | `assert(false, message)`. |
| `cannot convert '{s}' to number` | `num` on a non-numeric string. |
| `cannot convert an array to number` | `num` on an array. |
| `cannot convert a map to number` | `num` on a map. |
| `sqrt() is not defined for {n}` | `sqrt` of a negative number. |
| `log() is not defined for {n}` | `log` of ≤ 0. |
| `pop() on an empty array` | `pop([])`. |
| `sort() requires an array of only numbers or only strings` | Mixed array. |
| `range() requires a non-zero step` | Step is 0. |
| `range() would build {n} elements (limit 10000000)` | Too many elements. |
| `repeat() would build {n} characters (limit 10000000)` | Too many characters. |
| `split() requires a non-empty separator` | Empty split separator. |
| `replace() requires a non-empty pattern` | Empty replace pattern. |
| `slice() start index {s} is past its end index {e}` | start > end. |

---

## 10. Command Line

```text
nect run [--interp] <file>   Run a program (file, or "-" for stdin)
nect check <file>            Parse only; report syntax errors
nect disasm <file>           Bytecode plus native-compilation decisions
nect build <file>            Compile to a standalone native executable
  -o <path>               Where to write it (default: the file's stem)
  --cc <compiler>         C compiler to use (default: $CC, then cc)
  --emit-c                Print the generated C instead of building
  --keep-c                Keep the generated .c file
nect --version | -V
nect --help | -h | help
```

Environment variables:
- `NECT_NO_JIT=1` — disable native compilation (bytecode VM only).
- `CC` — preferred C compiler for `nect build`.

### 10.1 Exit codes

- `run`: `0` on success, `1` on any error.
- `check`: `0` if no syntax errors, `1` otherwise.
- `disasm`: `0` on success, `1` on parse failure.
- `build`: `0` when the executable was written, `1` when the program is outside
  the translatable subset or the C compiler failed. The reason is printed to
  stderr.

### 10.2 Output

Program output goes to stdout. Errors (parse and runtime) go to stderr.

---

## 11. Engines and Compilation

### 11.1 Bytecode VM

The default engine. Compiles the AST to a compact instruction set with interned
identifiers, flat local slots, fused three-address opcodes, and a numeric fast
path. Hosted in `src/vm/`.

The VM also performs constant folding at compile time (e.g., `1 + 2 * 3` becomes
`7`), fused comparison-and-branch opcodes, and module-level native compilation of
numeric prefixes (see 11.3).

### 11.2 Reference interpreter

The original tree-walking evaluator, kept for cross-checking. Enabled with
`nect run --interp`. It recurses on the host stack, so very deep recursion can
overflow (section 12).

### 11.3 Just-In-Time native compilation (Cranelift)

When Cranelift initializes, the type-inference pass in `src/jit/` compiles to
machine code the parts of a program that are:

(a) **Provably numeric** — no strings, arrays, maps, built-in calls, unresolvable
globals, division, or modulo; and

(b) **Actually repeated** — a function called from a loop or by itself
(recursion), or a module-level numeric prefix containing a loop.

Everything else stays in bytecode, and the two must produce identical output.
`nect disasm` reports the decision per function with a reason.

Typical JIT rejection reasons:
- `calls a builtin or extern`
- `loads a non-numeric constant`
- `divides`
- `touches a global`
- `uses arrays`
- `uses maps`
- `iterates a map`
- `uses short-circuit logic`
- `reads a conditional declaration`
- `no reachable return`
- `arity exceeds 4`
- `recursion depth exceeded` (falls back to bytecode)

Division and modulo stay in bytecode because a zero divisor must raise a runtime
error, which native code cannot do without unwinding.

### 11.4 Ahead-of-Time compilation to C (`nect build`)

`nect build` takes the bytecode and emits a single C file (program + small
runtime) compiled with the system C compiler (`-O2 -ffp-contract=off`).
Compiled with `-ffp-contract=off` to prevent fused multiply-add from changing
floating-point results in iterated computation (mandelbrot escapes differ
without it).

**Translates:**
- Numbers and booleans (as `double`).
- Arithmetic, comparisons, `&&`/`||`/`?:`.
- `if`/`while`/`for`/`break`/`continue`.
- Function calls and recursion.
- `print`, `str`, `abs`, `sqrt`, `floor`, `ceil`, `round`, `min`, `max`,
  `pow`, `sin`, `cos`, `tan`, `log`.
- Strings used inside expressions (concatenation, comparison, printing) — but
  **not** stored in variables or passed as arguments.

**Rejected:** arrays, maps, modulo, every other built-in, `input`, file I/O,
JSON, time, HTTP, and all non-numeric operations. A rejection is not a program
error — it means the C backend cannot type the code. The program runs on the VM.

### 11.5 Behavioral parity

The contract is **exact**: the bytecode VM, the JIT, and the C binary must
produce identical stdout, stderr, and exit status. This is enforced by
`tests/differential_tests.rs` (interpreter + VM + JIT) and
`tests/aot_tests.rs` (C binary vs VM).

---

## 12. Intentional Engine Divergences

These differences are accepted and pinned by `documented_divergences` in
`tests/differential_tests.rs`:

1. **Functions as values.** The interpreter treats functions as first-class
   values (`print(f)` prints `<function f>`); the VM resolves calls statically
   and reports `undefined variable 'f'`.

2. **Calling an unknown name.** The VM reports `undefined function 'nope'`
   while compiling; the interpreter reports `undefined variable 'nope'` at
   runtime.

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

## 13. Implementation Notes

### 13.1 Single source of truth

`src/builtins.rs` defines all runtime semantics (operator behavior, comparisons,
indexing, formatting, errors) and the built-in library (`NAMES` + `call`).
Both engines call into it, so their behavior cannot drift. The interpreter and
the VM implement value *semantics* identically; they differ only in how
expressions are evaluated (tree-walk vs bytecode) and where errors surface
(compile-time vs runtime), as documented in section 12.

### 13.2 Number formatting

Whole-number floats with `|n| < 1e15` are printed without a decimal point. Beyond
that threshold, or for non-whole values, the shortest round-tripping
representation is used. The C backend's runtime reproduces this formatting
exactly to maintain output parity.

### 13.3 Map ordering

Maps are `Vec<(Value, Value)>` (insertion-ordered) with sequential lookup.
`2` and `2.0` are the same key. `insert` preserves the position of an
overwritten key; `remove` shifts subsequent entries. Map equality is structural
and order-sensitive.

### 13.4 Randomness

Shared XORSHIFT64* implementation (`src/builtins.rs`). The state must be non-zero.
Unseeded, it initializes from `SystemTime` nanoseconds. `seed(n)` maps the
value to a non-zero u64 via `to_bits() | 1`. The exact sequence is an
implementation detail — only the reproducibility property (same seed → same
sequence across engines) is guaranteed.

### 13.5 Standard library modules

`import "std/ui.nct"` resolves from `STDLIB` embedded in the binary via
`include_str!`. The only bundled module is `std/ui.nct` (an HTML page builder
for web-style interfaces). All `import` lines are spliced before lexing; the
engines never see an import statement.

### 13.6 String indexing

String indices are character-based (code points), not byte-based. Indexing a
string yields a one-character string. This applies uniformly to `len`, `[]`,
`slice`, `index_of`, `reverse`, `upper`, and `lower`.

### 13.7 Compound assignment evaluation

For `a[b()] += 1`, the index expression `b()` is evaluated exactly once — the
operator is carried on the `SetIndex` AST node, not desugared to a separate read
and write. For plain names `x += 1`, the desugaring `x = x + 1` re-evaluates
nothing (the name is already bound).

### 13.8 Conditional evaluation

`condition ? then_expr : else_expr` evaluates only the taken branch. The `else`
is right-associative: `a ? b : c ? d : e` parses as `a ? b : (c ? d : e)`.

### 13.9 What is not supported

- No `switch` / pattern matching — use `if`/`else if`.
- No `do..while` — use `while (true)` with `break`.
- No `++`/`--` — use `+= 1` / `-= 1`.
- No `**` — use `pow(base, exp)`.
- No first-class functions in the VM — call by name only.
- No closures — functions are not values that capture their environment.
- No user-defined types or records — use maps and arrays.
- No module-level return values from `import` (modules are spliced, not
  namespaced).

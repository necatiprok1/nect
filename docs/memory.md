# Nect Runtime Memory Model

This document describes how Nect values are represented in memory, how they
are allocated and deallocated, how sharing and mutation work, and what happens
when memory runs out. It applies to both the bytecode VM and the reference
interpreter — the two engines share `src/builtins.rs` as the single source of
truth for value semantics.

---

## 1. Value representation

Nect's runtime `Value` enum (`src/ast/mod.rs:87-111`) uses **reference counting**
for composite types and **by-value storage** for primitives:

```rust
pub enum Value {
    Number(f64),                              // Copy, 8 bytes
    String(String),                           // Heap-allocated UTF-8
    Boolean(bool),                            // Copy, 1 byte
    Null,                                     // Zero-sized
    Function(Function),                       // Owned AST
    Array(Rc<RefCell<Vec<Value>>>),           // Shared, mutable
    Map(Rc<RefCell<crate::builtins::Map>>),   // Shared, mutable
    // ... (Future, Channel, Mutex, etc. — all Rc<RefCell<...>>)
}
```

**Primitives** (`Number`, `Boolean`, `Null`) are stored inline — no heap
allocation. They are `Copy` in Rust, so passing them around is a bitwise copy.

**Strings** own a heap-allocated `String` buffer. Each `Value::String` clones
the buffer contents when copied (no interning or deduplication in the general
case, though the VM deduplicates constants at compile time).

**Composite values** (arrays, maps, futures, channels, etc.) are
`Rc<RefCell<T>>` — **reference-counted, shared, and interior-mutable**. This
means:

- Multiple variables, array elements, and map entries can refer to the **same**
  underlying object — mutation through one reference is visible through all
  others (call-byshare semantics).
- `RefCell` provides **runtime** borrow checking: a panic occurs if a value is
  borrowed mutably while already borrowed immutably (or vice versa).
- The last `Rc` clone holding an object triggers its deallocation.

The `Map` type (`src/builtins.rs:20-23`) stores entries as a `Vec<(Value,
Value)>` — **not** a `HashMap`:

```rust
pub struct Map {
    pub entries: Vec<(Value, Value)>,  // insertion-ordered
}
```

This makes **insertion order observable** (printing, `for` over a map,
`keys()`/`values()`) at the cost of O(n) lookup. This is a deliberate semantic
choice, not a performance bug.

---

## 2. Stack vs. heap

### VM stack (`src/vm/mod.rs:1398, 1483-1485`)

The bytecode VM uses a **single contiguous `Vec<Value>` stack** for the entire
program — there is no per-frame heap allocation:

```
stack: [ frame_0_locals | frame_0_operands | frame_1_locals | ... ]
```

- Each `Frame` holds `base` and `locals_end` pointers into the shared stack.
- Arguments are passed **by position** (caller pushes, callee's `base` points at
  the first argument — **zero-copy** for the call itself).
- `stack.truncate(base)` on function return releases the entire frame in O(1).
- The stack grows (`resize`) only when a function needs more locals than the
  current stack can hold — amortized O(1) across calls.

### Interpreter scopes (`src/interpreter/mod.rs:41-42`)

The tree-walking interpreter uses `Vec<HashMap<String, Value>>` for scopes.
Each `let` or block pushes a new `HashMap`; exiting the block drops it. Lookup
is O(depth) per access.

### Heap objects

All `String` buffers, `Vec<Value>` arrays, and `Vec<(Value, Value)>` maps live
on the heap. They are owned by exactly the `Rc<RefCell<T>>` that wraps them.

---

## 3. Ownership and aliases

### `Rc::clone` is cheap

Cloning an `Rc<RefCell<T>>` pointer bumps the reference count — it does **not**
copy the underlying data. This is used extensively:

- `Op::LoadConst`, `Op::LoadLocal`, `Op::LoadGlobal` clone `Value` from the
  constant table / stack / globals. For `Number`/`Boolean`/`Null` this is a
  bitwise copy. For `String` it copies the string buffer. For `Array`/`Map`
  it increments the refcount (no allocation).
- Function calls pass `Rc<Vec<Op>>` and `Rc<Vec<Value>>` (the compiled
  instructions and constants) — another refcount bump.
- Passing an array or map to a built-in increments the refcount, **not** copies
  the data.

### Aliasing is observable

Because arrays and maps are shared references, mutations are visible across all
aliases:

```nct
let a = [1, 2, 3]
let b = a          // b and a refer to the same array
push(b, 4)
print(a)           // [1, 2, 3, 4] — a sees b's mutation
```

This is why `Map::get` and `Vec` indexing return `.clone()` — they hand the
caller a new owned `Value` that shares the underlying `Rc` of any composite
element but is otherwise independent.

### Deep vs. shallow copy

- `Value::clone()` is **shallow** for composites (refcount bump).
- Some built-ins (e.g. `keys()`, `values()`, `slice()`, `concat()`) produce
  **new** `Rc<RefCell<...>>` wrappers, so the result does not alias the source.

---

## 4. Temporary values

### VM engine

Temporaries are **operands on the stack**. After `locals_end`, the stack holds
the operand stack for the current frame. Each opcode pops its operands and
pushes its result; `Op::Pop` explicitly discards statement results. The net
allocation cost per opcode is zero for primitive arithmetic.

### Interpreter

Temporaries live as **local variables** in the Rust `eval()` function on the
host stack. They are dropped when `eval()` returns, i.e., at the end of each
sub-expression. No explicit allocation tracking is needed.

### Built-in calls

Arguments are passed as `&[Value]` slices (no allocation on either engine).
Built-ins return `Value` by value (move semantics). `apply_binary` and
`apply_unary` take `Value` by value and return `Value` by value.

---

## 5. Constant deduplication

The VM compiler (`src/vm/mod.rs:675-683`) deduplicates constants by structural
equality:

```rust
fn constant_index(&mut self, value: &Value) -> u32 {
    match self.constants.iter().position(|c| *c == *value) {
        Some(i) => i as u32,           // Reuses existing constant slot
        None => { self.constants.push(value.clone()); ... }
    }
}
```

- `Value::PartialEq` (`src/ast/mod.rs:113-132`) uses `Rc::ptr_equal` for
  composite types — two arrays are equal constants only if they are the
  *same allocation*.
- Constants are stored in an `Rc<Vec<Value>>` shared across all frames.

---

## 6. Out-of-memory behavior

**There is no explicit OOM handling.** The runtime relies on Rust's standard
`Vec`/`String`/`Rc` allocation behavior:

- `Vec::push`, `Vec::with_capacity`, `String` operations, and `Rc::new` can all
  panic on allocation failure (Rust's default behavior when the
  `alloc` crate runs out of memory).
- This typically causes `std::alloc::handle_alloc_error` to abort the process.
- `range()` and `repeat()` enforce a **10,000,000 element limit**
  (`src/builtins.rs`) to prevent runaway allocations, reporting the limit as
  a runtime error instead of exhausting memory.

**The JIT and AOT backends** do not add OOM handling of their own — if the IR
or Cranelift pipeline allocates (e.g., for type tables), Rust's default
panicking allocator applies.

---

## 7. Stack depth and recursion

### VM

The VM stores frames on the heap (in `Vec`s), so frame depth is bounded only by
available memory. Very deep recursion does not overflow the host stack.

### Interpreter

The interpreter recurses on the **host** stack via `eval()`. Deep recursion
(typically a few thousand calls, fewer in debug builds) causes a stack overflow
and an **abort** of the process. This is a documented divergence pinned by
`tests/differential_tests.rs`.

### Native code (JIT)

The JIT compiles provably-numeric functions, including recursive ones. The
native stack is used, but the recursion depth limit is governed by the
`IR_MAX_NATIVE_DEPTH` constant (defined alongside the IR module), which guards
against infinite native recursion. If a function exceeds this limit, it is kept
in bytecode instead.

---

## 8. Summary

| Property | VM | Interpreter |
|---|---|---|
| Value storage | `Vec<Value>` (shared stack) | `HashMap<String, Value>` per scope |
| Composite values | `Rc<RefCell<T>>` (shared) | `Rc<RefCell<T>>` (shared) |
| Primitive values | Copy (inline) | Copy (inline) |
| Borrow checking | `RefCell` runtime | `RefCell` runtime |
| Temporary lifetime | End of opcode (stack pop) | End of `eval()` call (Rust drop) |
| Call overhead | Stack slice (zero-copy args) | `Vec<Value>` clone per call |
| Stack overflow risk | None (heap frames) | Yes (host stack) |
| OOM handling | Panic/abort (Rust default) | Panic/abort (Rust default) |

---

## 10. Heap management strategy

The current strategy is **reference counting** via `Rc<RefCell<T>>` for composite
values. This is the right choice for Nect's current design:

- **Single-threaded**: The language has no shared-memory concurrency visible to
  the user level (threads exist internally for async/GUI/DB operations but are
  not part of the language's memory model). `Rc` (not `Arc`) is sufficient and
  faster.
- **Deterministic cleanup**: `Rc` drops the underlying object as soon as the
  last reference goes away. No pause times from a garbage collector.
- **Interior mutability**: `RefCell` allows mutation through a shared
  reference, matching Nect's call-by-share semantics for arrays and maps.

### Why not a tracing garbage collector?

A tracing GC (Boehm, mimalloc-trace, etc.) would handle reference cycles, but:
1. Nect does not have a way to create reference cycles through its public API --
   the only cycle-forming types (`Future`, `Channel`) are created by built-ins
   that manage their own lifetimes internally.
2. A tracing GC would add a dependency and a runtime pause, complicating the
   standalone C backend.
3. `Rc<RefCell<T>>` is already well-tuned for Nect's access patterns (shared
   reads, occasional writes).

### Why not copy-on-write (fork-based)?

Copy-on-write for arrays/maps would require a `Cow` wrapper or a custom
copy-on-write type. The current `Rc<RefCell<...>>` approach already provides O(1)
cloning (refcount bump) and deferred writes, which is effectively free COW for
the common case.

### Future considerations

- If Nect gains **shared-memory threading** (e.g., `threads` module with
  `spawn`), `Rc` must become `Arc` for the shared values.
- If reference cycles become possible through a new language feature, a cycle
  collector or weak-reference-based approach would be needed.
- The `range()` and `repeat()` built-ins already enforce a 10,000,000 element
  cap to prevent runaway allocations; this could become configurable.

---

## 9. Key file references

| Concept | File | Lines |
|---|---|---|
| `Value` enum | `src/ast/mod.rs` | 87-111 |
| `Value::PartialEq` | `src/ast/mod.rs` | 113-132 |
| `Map` structure | `src/builtins.rs` | 20-23 |
| VM stack & frames | `src/vm/mod.rs` | 1398, 1483-1485 |
| VM call/return (zero-copy args) | `src/vm/mod.rs` | 1663-1687 |
| VM constant deduplication | `src/vm/mod.rs` | 675-683 |
| Interpreter scopes | `src/interpreter/mod.rs` | 41-42 |
| `range()` / `repeat()` limits | `src/builtins.rs` | (enforced in respective builtins) |
| `apply_binary` / numeric fast path | `src/builtins.rs:329-361`, `src/vm/mod.rs:1744-1816` |

---

## 11. Diagnostic modes

Nect provides three diagnostic mechanisms for understanding memory and runtime
behavior:

### `nect mem-profile <file>`

Runs the program with statistics collection enabled. After the program
completes (successfully or with error), prints runtime statistics to stderr:

```text
=== Runtime Statistics ===
Runtime Statistics
  instructions executed: 6750
  total calls: 900
    native calls: 900
    function calls: 0
  peak stack depth: 3
  peak frame count: 0
  stack growths: 0
```

Environment variable: `NECT_VM_STATS=1` (enables the same collection for `run`).

### Stack traces on runtime errors

When `NECT_STACK_TRACE=1` is set or `nect run --stack-trace <file>` is used,
runtime errors print a call stack trace to stderr showing the chain of Nect
function calls:

```text
runtime error: cannot index into number

Stack trace:
  frame 0: helper
  frame 1: middle
  frame 2: <module>
```

The trace is only available for the bytecode VM (not the interpreter, which
recurses on the host stack).

### `nect disasm <file>`

Prints the bytecode and the JIT eligibility analysis for each function. This
shows which functions would be compiled natively, which stay in bytecode, and
why — useful for understanding allocation behavior of numeric vs. non-numeric
code.

### `NECT_JIT_TIMING=1`

When set, prints where JIT compilation time is spent (Cranelift init, codegen,
finalize). This helps distinguish JIT overhead from runtime allocation costs.

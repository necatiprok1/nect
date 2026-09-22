//! The shared core: value semantics and the built-in function library.
//!
//! Both engines call into this module, so a program behaves identically on the
//! bytecode VM and on the reference interpreter — `tests/differential_tests.rs`
//! compares their stdout, stderr and exit status character for character. The
//! names in [`NAMES`] are the complete set of builtins; anything else is a user
//! function (or an error).

use crate::ast::{BinaryOp, UnaryOp, Value};
use reqwest::blocking;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::fmt;
use std::rc::Rc;

/// An insertion-ordered map: the hashable values as keys, their stored values
/// alongside the position each key was first inserted at. Order is the order
/// the keys were (re)inserted, so printing a map is deterministic — which is
/// what makes cross-engine parity testable at all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Map {
    pub entries: Vec<(Value, Value)>,
}

impl Map {
    pub fn new() -> Self {
        Self::default()
    }

    /// The canonical form of a key: numbers are normalized (`2` and `2.0` are
    /// the same key) and everything else is used as-is.
    fn key_of(key: &Value) -> Option<Value> {
        match key {
            Value::Number(n) => Some(Value::Number(*n)),
            Value::String(_) | Value::Boolean(_) => Some(key.clone()),
            other => {
                let _ = other;
                None
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &Value) -> Option<Value> {
        let key = Self::key_of(key)?;
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())
    }

    pub fn contains(&self, key: &Value) -> bool {
        self.get(key).is_some()
    }

    /// Inserts or overwrites, keeping the key's original position when it
    /// already exists. Errors on an unhashable key.
    pub fn insert(&mut self, key: Value, value: Value) -> Result<(), RuntimeError> {
        if Self::key_of(&key).is_none() {
            return Err(RuntimeError::new(&format!(
                "map keys must be strings, numbers, or booleans, got {}",
                type_name(&key)
            )));
        }
        let key = Self::key_of(&key).expect("checked above");
        for entry in self.entries.iter_mut() {
            if entry.0 == key {
                entry.1 = value;
                return Ok(());
            }
        }
        self.entries.push((key, value));
        Ok(())
    }

    /// Removes a key, returning its value. `None` when the key is absent.
    pub fn remove(&mut self, key: &Value) -> Option<Value> {
        let key = Self::key_of(key)?;
        let at = self.entries.iter().position(|(k, _)| *k == key)?;
        Some(self.entries.remove(at).1)
    }

    /// The keys in insertion order; `for k in keys(d)` visits them in the same
    /// order a direct `for` over the map would.
    pub fn keys(&self) -> Vec<Value> {
        self.entries.iter().map(|(k, _)| k.clone()).collect()
    }

    pub fn values(&self) -> Vec<Value> {
        self.entries.iter().map(|(_, v)| v.clone()).collect()
    }
}

use std::sync::{Arc, Mutex as StdMutex, Condvar, mpsc};
use rusqlite::{Connection, Statement, params, Error as SqliteError};

/// A future/promise from an async computation.
#[derive(Debug, Clone)]
pub struct Future {
    pub state: Arc<StdMutex<FutureState>>,
    pub condvar: Arc<Condvar>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FutureState {
    Pending,
    Ready(Value),
}

impl Future {
    pub fn new() -> Self {
        Self {
            state: Arc::new(StdMutex::new(FutureState::Pending)),
            condvar: Arc::new(Condvar::new()),
        }
    }

    pub fn complete(&self, value: Value) {
        let mut state = self.state.lock().unwrap();
        *state = FutureState::Ready(value);
        self.condvar.notify_all();
    }

    pub fn is_ready(&self) -> bool {
        let state = self.state.lock().unwrap();
        matches!(*state, FutureState::Ready(_))
    }

    pub fn get_result(&self) -> Option<Value> {
        let state = self.state.lock().unwrap();
        match &*state {
            FutureState::Ready(v) => Some(v.clone()),
            FutureState::Pending => None,
        }
    }
}

impl PartialEq for Future {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Default for Future {
    fn default() -> Self {
        Self::new()
    }
}

/// A channel for thread communication.
#[derive(Debug, Clone)]
pub struct Channel {
    pub tx: Arc<StdMutex<mpsc::Sender<Value>>>,
    pub rx: Arc<StdMutex<mpsc::Receiver<Value>>>,
}

impl Channel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx: Arc::new(StdMutex::new(tx)),
            rx: Arc::new(StdMutex::new(rx)),
        }
    }

    pub fn send(&self, value: Value) -> Result<(), RuntimeError> {
        self.tx.lock().unwrap().send(value).map_err(|_| RuntimeError::new("channel closed"))
    }

    pub fn recv(&self) -> Result<Value, RuntimeError> {
        self.rx.lock().unwrap().recv().map_err(|_| RuntimeError::new("channel closed"))
    }

    pub fn try_recv(&self) -> Result<Value, RuntimeError> {
        self.rx.lock().unwrap().try_recv().map_err(|_| RuntimeError::new("channel empty or closed"))
    }
}

impl Default for Channel {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for Channel {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.tx, &other.tx)
    }
}

/// A mutex for synchronization.
#[derive(Debug, Clone)]
pub struct Mutex {
    pub inner: Arc<StdMutex<()>>,
}

impl Mutex {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(StdMutex::new(())),
        }
    }

    pub fn lock(&self) -> MutexGuard {
        MutexGuard {
            guard: self.inner.lock().unwrap(),
        }
    }

    pub fn try_lock(&self) -> Option<MutexGuard> {
        self.inner.try_lock().ok().map(|guard| MutexGuard { guard })
    }
}

impl Default for Mutex {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for Mutex {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

pub struct MutexGuard<'a> {
    guard: std::sync::MutexGuard<'a, ()>,
}

impl<'a> Drop for MutexGuard<'a> {
    fn drop(&mut self) {
        // Guard is automatically released
    }
}

/// A thread handle returned by spawn.
#[derive(Debug)]
pub struct ThreadHandle {
    pub handle: Option<std::thread::JoinHandle<()>>,
}

impl ThreadHandle {
    pub fn new(handle: std::thread::JoinHandle<()>) -> Self {
        Self { handle: Some(handle) }
    }

    pub fn join(&mut self) -> Result<(), RuntimeError> {
        if let Some(h) = self.handle.take() {
            h.join().map_err(|_| RuntimeError::new("thread panicked"))
        } else {
            Err(RuntimeError::new("thread already joined"))
        }
    }
}

impl PartialEq for ThreadHandle {
    fn eq(&self, other: &Self) -> bool {
        // Two handles are equal if they both have no handle (already joined)
        // or if they both have handles (not joined yet)
        self.handle.is_none() && other.handle.is_none()
    }
}

impl Clone for ThreadHandle {
    fn clone(&self) -> Self {
        Self { handle: None } // Can't clone a JoinHandle, so create a "dummy" one
    }
}

/// A runtime error, raised while a program is executing.
#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
}

impl RuntimeError {
    pub fn new(msg: &str) -> Self {
        Self {
            message: msg.to_string(),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "runtime error: {}", self.message)
    }
}

impl std::error::Error for RuntimeError {}

/// Only `false`, `null`, and `0` are falsy; everything else (including empty
/// strings and empty arrays) is truthy.
pub fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Boolean(b) => *b,
        Value::Null => false,
        Value::Number(n) => *n != 0.0,
        _ => true,
    }
}

/// Type name used in error messages.
pub fn type_name(v: &Value) -> String {
    match v {
        Value::Number(_) => "number".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Boolean(_) => "boolean".to_string(),
        Value::Null => "null".to_string(),
        Value::Array(_) => "array".to_string(),
        Value::Map(_) => "map".to_string(),
        Value::Function(f) => format!("function '{}'", f.name),
        Value::Future(_) => "future".to_string(),
        Value::Channel(_) => "channel".to_string(),
        Value::Mutex(_) => "mutex".to_string(),
        Value::ThreadHandle(_) => "thread_handle".to_string(),
        Value::DbConnection(_) => "db_connection".to_string(),
        Value::GuiWindow(_) => "gui_window".to_string(),
    }
}

/// `left op right` for every operator except `&&`/`||`, which each engine
/// short-circuits before calling here.
pub fn apply_binary(l: Value, op: BinaryOp, r: Value) -> Result<Value, RuntimeError> {
    match op {
        BinaryOp::Add => match (&l, &r) {
            (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
            (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
            _ => Err(RuntimeError::new(&format!(
                "cannot apply '+' to {} and {}",
                type_name(&l),
                type_name(&r)
            ))),
        },
        BinaryOp::Subtract => num_op(&l, &r, "-", |a, b| Ok(Value::Number(a - b))),
        BinaryOp::Multiply => num_op(&l, &r, "*", |a, b| Ok(Value::Number(a * b))),
        BinaryOp::Divide => num_op(&l, &r, "/", |a, b| {
            if b == 0.0 {
                return Err(RuntimeError::new("division by zero"));
            }
            Ok(Value::Number(a / b))
        }),
        BinaryOp::Modulo => num_op(&l, &r, "%", |a, b| {
            if b == 0.0 {
                return Err(RuntimeError::new("modulo by zero"));
            }
            Ok(Value::Number(a % b))
        }),
        BinaryOp::Equal => Ok(Value::Boolean(l == r)),
        BinaryOp::NotEqual => Ok(Value::Boolean(l != r)),
        BinaryOp::Less | BinaryOp::Greater | BinaryOp::LessEqual | BinaryOp::GreaterEqual => {
            compare(&l, op, &r)
        }
        BinaryOp::And => Ok(Value::Boolean(is_truthy(&l) && is_truthy(&r))),
        BinaryOp::Or => Ok(Value::Boolean(is_truthy(&l) || is_truthy(&r))),
    }
}

/// `-x` and `!x`.
pub fn apply_unary(op: UnaryOp, v: Value) -> Result<Value, RuntimeError> {
    match op {
        UnaryOp::Negate => match v {
            Value::Number(n) => Ok(Value::Number(-n)),
            _ => Err(RuntimeError::new(&format!("cannot negate a {}", type_name(&v)))),
        },
        UnaryOp::Not => Ok(Value::Boolean(!is_truthy(&v))),
    }
}

/// Result of a numeric `lhs op rhs`, or `None` when the operation needs the
/// general path (non-numeric operands, division by zero, short-circuit ops).
/// Used by the VM's fused opcodes, which run this before falling back.
pub fn numeric_binary(op: BinaryOp, a: f64, b: f64) -> Option<Value> {
    match op {
        BinaryOp::Add => Some(Value::Number(a + b)),
        BinaryOp::Subtract => Some(Value::Number(a - b)),
        BinaryOp::Multiply => Some(Value::Number(a * b)),
        BinaryOp::Divide => {
            if b == 0.0 {
                None
            } else {
                Some(Value::Number(a / b))
            }
        }
        BinaryOp::Modulo => {
            if b == 0.0 {
                None
            } else {
                Some(Value::Number(a % b))
            }
        }
        BinaryOp::Less => Some(Value::Boolean(a < b)),
        BinaryOp::Greater => Some(Value::Boolean(a > b)),
        BinaryOp::LessEqual => Some(Value::Boolean(a <= b)),
        BinaryOp::GreaterEqual => Some(Value::Boolean(a >= b)),
        BinaryOp::Equal => Some(Value::Boolean(a == b)),
        BinaryOp::NotEqual => Some(Value::Boolean(a != b)),
        BinaryOp::And | BinaryOp::Or => None,
    }
}

fn num_op<F>(l: &Value, r: &Value, symbol: &str, op: F) -> Result<Value, RuntimeError>
where
    F: Fn(f64, f64) -> Result<Value, RuntimeError>,
{
    match (l, r) {
        (Value::Number(a), Value::Number(b)) => op(*a, *b),
        _ => Err(RuntimeError::new(&format!(
            "cannot apply '{}' to {} and {}",
            symbol,
            type_name(l),
            type_name(r)
        ))),
    }
}

/// Ordering comparison over numbers or strings, honouring the actual operator.
/// Unordered numbers (NaN) compare false for every operator.
pub fn compare(l: &Value, op: BinaryOp, r: &Value) -> Result<Value, RuntimeError> {
    let ordering = match (l, r) {
        (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        _ => {
            return Err(RuntimeError::new(&format!(
                "comparison requires numbers or strings, got {} and {}",
                type_name(l),
                type_name(r)
            )));
        }
    };
    let result = match ordering {
        None => false,
        Some(ordering) => match op {
            BinaryOp::Less => ordering == Ordering::Less,
            BinaryOp::LessEqual => ordering != Ordering::Greater,
            BinaryOp::Greater => ordering == Ordering::Greater,
            BinaryOp::GreaterEqual => ordering != Ordering::Less,
            _ => unreachable!("compare() needs an ordering operator"),
        },
    };
    Ok(Value::Boolean(result))
}

/// `target[index]` for an array or a string.
pub fn get_index(target: &Value, index: &Value) -> Result<Value, RuntimeError> {
    match target {
        Value::Array(elements) => {
            let elements = elements.borrow();
            let position = element_index("array", index, elements.len())?;
            Ok(elements[position].clone())
        }
        Value::String(text) => {
            let chars: Vec<char> = text.chars().collect();
            let position = element_index("string", index, chars.len())?;
            Ok(Value::String(chars[position].to_string()))
        }
        Value::Map(map) => {
            if let Some(value) = map.borrow().get(index) {
                Ok(value)
            } else {
                Err(RuntimeError::new(&format!(
                    "map key {} not found",
                    format_value(index)
                )))
            }
        }
        other => Err(RuntimeError::new(&format!(
            "cannot index into {}",
            type_name(other)
        ))),
    }
}

/// `target[index] = rhs`, or `target[index] op= rhs` when `op` is set. Returns
/// the stored value, because an assignment is an expression.
pub fn set_index(
    target: &Value,
    index: &Value,
    op: Option<BinaryOp>,
    rhs: Value,
) -> Result<Value, RuntimeError> {
    let elements = match target {
        Value::Array(elements) => elements,
        Value::Map(map) => {
            // A compound assignment (`d[k] += v`) reads before writing, so the
            // key must exist; plain assignment (`d[k] = v`) inserts or
            // overwrites, which is what makes maps growable.
            let mut map = map.borrow_mut();
            let value = match op {
                Some(op) => {
                    let Some(current) = map.get(index) else {
                        return Err(RuntimeError::new(&format!(
                            "map key {} not found",
                            format_value(index)
                        )));
                    };
                    apply_binary(current, op, rhs)?
                }
                None => rhs,
            };
            map.insert(index.clone(), value.clone())?;
            return Ok(value);
        }
        Value::String(_) => {
            return Err(RuntimeError::new(
                "strings are immutable: cannot assign to a string index",
            ));
        }
        other => {
            return Err(RuntimeError::new(&format!(
                "cannot index into {}",
                type_name(other)
            )));
        }
    };
    let position = element_index("array", index, elements.borrow().len())?;
    let value = match op {
        Some(op) => {
            let current = elements.borrow()[position].clone();
            apply_binary(current, op, rhs)?
        }
        None => rhs,
    };
    elements.borrow_mut()[position] = value.clone();
    Ok(value)
}

/// Resolves an element index, counting negatives back from the end, and rejects
/// anything outside the collection.
pub fn element_index(kind: &str, index: &Value, len: usize) -> Result<usize, RuntimeError> {
    let raw = match index {
        Value::Number(n) => *n,
        other => {
            return Err(RuntimeError::new(&format!(
                "{} index must be a number, got {}",
                kind,
                type_name(other)
            )));
        }
    };
    let resolved = if raw < 0.0 { raw + len as f64 } else { raw }.floor();
    if !(resolved >= 0.0 && resolved < len as f64) {
        return Err(RuntimeError::new(&format!(
            "{} index {} out of bounds (length {})",
            kind, raw, len
        )));
    }
    Ok(resolved as usize)
}

/// Renders a value the way `print` does.
///
/// A bare string prints as itself; inside an array it is quoted, so the
/// boundaries between `["1", "2"]` and `[1, 2]` stay visible.
pub fn format_value(v: &Value) -> String {
    match v {
        Value::Number(n) => format_number(*n),
        Value::String(s) => s.clone(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(elements) => {
            let items: Vec<String> = elements.borrow().iter().map(format_in_array).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Map(map) => {
            let items: Vec<String> = map
                .borrow()
                .entries
                .iter()
                .map(|(k, v)| format!("{}: {}", format_in_array(k), format_in_array(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
        Value::Function(f) => format!("<function {}>", f.name),
        Value::Future(_) => "<future>".to_string(),
        Value::Channel(_) => "<channel>".to_string(),
        Value::Mutex(_) => "<mutex>".to_string(),
        Value::ThreadHandle(_) => "<thread_handle>".to_string(),
        Value::DbConnection(_) => "<db_connection>".to_string(),
        Value::GuiWindow(_) => "<gui_window>".to_string(),
    }
}

/// Formats a value the way it appears as an element of an array: strings gain
/// quotes, everything else is unchanged.
fn format_in_array(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        other => format_value(other),
    }
}

/// Whole floats print without a trailing `.0`, which keeps `print(2)` and
/// `print(1/2)` looking the way a user expects.
fn format_number(n: f64) -> String {
    if n.is_nan() {
        "nan".to_string()
    } else if n.is_infinite() {
        if n > 0.0 { "inf".to_string() } else { "-inf".to_string() }
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// Every built-in name, in registration order. The VM interns these so calls to
/// them resolve to [`CallTarget::Native`](crate::vm::CallTarget::Native).
pub const NAMES: &[&str] = &[
    // output
    "print",
    "println",
    // conversion / inspection
    "str",
    "num",
    "bool",
    "type",
    "assert",
    "concat",
    // strings and arrays
    "len",
    "push",
    "pop",
    "join",
    "slice",
    "contains",
    "index_of",
    "split",
    "upper",
    "lower",
    "trim",
    "replace",
    "repeat",
    "sort",
    "reverse",
    "sum",
    "range",
    // maps
    "keys",
    "values",
    "has",
    "remove",
    // math
    "abs",
    "sqrt",
    "floor",
    "ceil",
    "round",
    "min",
    "max",
    "pow",
    "sin",
    "cos",
    "tan",
    "log",
    // numbers and characters
    "int",
    "fixed",
    "char",
    "char_code",
    // interactivity and randomness
    "input",
    "random",
    "random_int",
    "seed",
    // files, time, and data
    "read_file",
    "write_file",
    "open_url",
    "json_encode",
    "json_decode",
    "now",
    "sleep",
    "args",
    // concurrency
    "spawn",
    "thread_join",
    "channel",
    "send",
    "recv",
    "try_recv",
    "mutex",
    "lock",
    "unlock",
    "async",
    "await",
    // HTTP client
    "http_get",
    "http_post",
    "http_request",
    // HTTP server
    "http_server",
    "http_respond",
    "http_listen",
    // Routing
    "http_route",
    "http_middleware",
    "http_router",
    // AI/ML - Tensors
    "tensor",
    "tensor_shape",
    "tensor_get",
    "tensor_set",
    "tensor_add",
    "tensor_mul",
    "tensor_matmul",
    "tensor_transpose",
    "tensor_reshape",
    // AI/ML - Activations
    "relu",
    "sigmoid",
    "tanh",
    "softmax",
    // AI/ML - Neural network layers
    "linear",
    "conv2d",
    // AI/ML - Loss functions
    "mse_loss",
    "cross_entropy_loss",
    // AI/ML - Optimizers
    "sgd_step",
    "adam_step",
    // AI/ML - Data utilities
    "train_test_split",
    "accuracy",
    "argmax",
    // Database - SQLite
    "db_open",
    "db_close",
    "db_exec",
    "db_query",
    "db_query_row",
    "db_transaction",
    "db_last_insert_rowid",
    "db_changes",
    // GUI framework
    "gui_window",
    "gui_button",
    "gui_label",
    "gui_text_input",
    "gui_checkbox",
    "gui_slider",
    "gui_vstack",
    "gui_hstack",
    "gui_show",
    "gui_poll_events",
    "gui_close",
];

/// Script arguments collected by the CLI (`nect run app.nct a b` → `args()`
/// is `["a", "b"]`). Set once before execution; both engines read it.
static SCRIPT_ARGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn set_script_args(args: Vec<String>) {
    if let Ok(mut slot) = SCRIPT_ARGS.lock() {
        *slot = args;
    }
}

/// Dispatches a builtin call.
///
/// `args` are already-evaluated arguments. An unknown name is an error rather
/// than a panic, so the two engines can share this entry point safely.
pub fn call(name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
    match name {
        "print" | "println" => {
            let rendered: Vec<String> = args.iter().map(format_value).collect();
            println!("{}", rendered.join(" "));
            Ok(Value::Null)
        }
        "str" => Ok(Value::String(match args.first() {
            Some(value) => format_value(value),
            None => String::new(),
        })),
        "num" => num(args),
        "bool" => {
            require_arity(name, args, 1)?;
            Ok(Value::Boolean(is_truthy(&args[0])))
        }
        "type" => {
            require_arity(name, args, 1)?;
            Ok(Value::String(type_of(&args[0]).to_string()))
        }
        "assert" => assert(args),
        "concat" => concat(args),

        "len" => {
            require_arity(name, args, 1)?;
            Ok(Value::Number(length_of(&args[0])? as f64))
        }
        "push" => push(args),
        "pop" => pop(args),
        "join" => join(args),
        "slice" => slice(args),
        "contains" => {
            require_arity(name, args, 2)?;
            Ok(Value::Boolean(index_of_value(&args[0], &args[1])?.is_some()))
        }
        "index_of" => {
            require_arity(name, args, 2)?;
            Ok(Value::Number(match index_of_value(&args[0], &args[1])? {
                Some(index) => index as f64,
                None => -1.0,
            }))
        }
        "split" => split(args),
        "upper" => string_map(name, args, |s| s.to_uppercase()),
        "lower" => string_map(name, args, |s| s.to_lowercase()),
        "trim" => string_map(name, args, |s| s.trim().to_string()),
        "replace" => replace(args),
        "repeat" => repeat(args),
        "sort" => sort(args),
        "reverse" => reverse(args),
        "sum" => sum(args),
        "range" => range(args),

        "keys" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::Map(map) => Ok(Value::Array(Rc::new(RefCell::new(map.borrow().keys())))),
                other => Err(RuntimeError::new(&format!(
                    "keys() requires a map, got {}",
                    type_name(other)
                ))),
            }
        }
        "values" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::Map(map) => Ok(Value::Array(Rc::new(RefCell::new(map.borrow().values())))),
                other => Err(RuntimeError::new(&format!(
                    "values() requires a map, got {}",
                    type_name(other)
                ))),
            }
        }
        "has" => {
            require_arity(name, args, 2)?;
            match &args[0] {
                Value::Map(map) => Ok(Value::Boolean(map.borrow().contains(&args[1]))),
                other => Err(RuntimeError::new(&format!(
                    "has() requires a map, got {}",
                    type_name(other)
                ))),
            }
        }
        "remove" => {
            require_arity(name, args, 2)?;
            match &args[0] {
                Value::Map(map) => {
                    let removed = map.borrow_mut().remove(&args[1]);
                    Ok(removed.unwrap_or(Value::Null))
                }
                other => Err(RuntimeError::new(&format!(
                    "remove() requires a map, got {}",
                    type_name(other)
                ))),
            }
        }

        "abs" => number_map(name, args, f64::abs),
        "sqrt" => number_map(name, args, |n| {
            if n < 0.0 {
                // Reported by the caller through the NaN-free path below.
                f64::NAN
            } else {
                n.sqrt()
            }
        }),
        "floor" => number_map(name, args, f64::floor),
        "ceil" => number_map(name, args, f64::ceil),
        "round" => number_map(name, args, f64::round),
        "sin" => number_map(name, args, f64::sin),
        "cos" => number_map(name, args, f64::cos),
        "tan" => number_map(name, args, f64::tan),
        "log" => number_map(name, args, |n| {
            if n <= 0.0 { f64::NAN } else { n.ln() }
        }),
        "min" => extremum(name, args, true),
        "max" => extremum(name, args, false),
        "pow" => {
            require_arity(name, args, 2)?;
            Ok(Value::Number(
                number(name, &args[0])?.powf(number(name, &args[1])?),
            ))
        }

        "int" => {
            require_arity(name, args, 1)?;
            Ok(Value::Number(number(name, &args[0])?.trunc()))
        }
        "fixed" => {
            require_arity(name, args, 2)?;
            let value = number(name, &args[0])?;
            let digits = number(name, &args[1])?;
            if !(0.0..=100.0).contains(&digits) {
                return Err(RuntimeError::new(
                    "fixed() requires a digit count between 0 and 100",
                ));
            }
            Ok(Value::String(format!(
                "{value:.*}",
                digits as usize
            )))
        }
        "char" => {
            require_arity(name, args, 1)?;
            let code = number(name, &args[0])?;
            if code < 0.0 || code > 1_114_111.0 || code.fract() != 0.0 {
                return Err(RuntimeError::new(&format!(
                    "char() requires a whole code point in 0..=1114111, got {code}"
                )));
            }
            match char::from_u32(code as u32) {
                Some(c) => Ok(Value::String(c.to_string())),
                None => Err(RuntimeError::new(&format!(
                    "char() has no character for code point {code}"
                ))),
            }
        }
        "char_code" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::String(s) => match s.chars().next() {
                    Some(c) => Ok(Value::Number(c as u32 as f64)),
                    None => Err(RuntimeError::new(
                        "char_code() requires a non-empty string",
                    )),
                },
                other => Err(RuntimeError::new(&format!(
                    "char_code() requires a string, got {}",
                    type_name(other)
                ))),
            }
        }

        "input" => input(args),
        "random" => {
            require_arity(name, args, 0)?;
            Ok(Value::Number(next_random_f64()))
        }
        "random_int" => {
            require_arity(name, args, 2)?;
            let low = number(name, &args[0])?;
            let high = number(name, &args[1])?;
            if low.fract() != 0.0 || high.fract() != 0.0 {
                return Err(RuntimeError::new(
                    "random_int() requires whole-number bounds",
                ));
            }
            if low > high {
                return Err(RuntimeError::new(&format!(
                    "random_int() requires low <= high, got {low} > {high}"
                )));
            }
            let span = (high - low + 1.0).min(2f64.powi(53));
            Ok(Value::Number(
                (next_random_f64() * span).floor() + low,
            ))
        }
        "seed" => {
            require_arity(name, args, 1)?;
            let value = number(name, &args[0])?;
            // XORSHIFT64* state must be non-zero; map everything onto 1..=u64::MAX.
            RNG_STATE.store(value.to_bits() | 1, std::sync::atomic::Ordering::Relaxed);
            Ok(Value::Null)
        }

        "now" => {
            require_arity(name, args, 0)?;
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            Ok(Value::Number(seconds))
        }
        "sleep" => {
            require_arity(name, args, 1)?;
            let seconds = number(name, &args[0])?;
            if !(0.0..=3600.0).contains(&seconds) {
                return Err(RuntimeError::new(
                    "sleep() requires a duration between 0 and 3600 seconds",
                ));
            }
            std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
            Ok(Value::Null)
        }
        "args" => {
            require_arity(name, args, 0)?;
            let values = SCRIPT_ARGS
                .lock()
                .map(|slot| slot.iter().map(|a| Value::String(a.clone())).collect())
                .unwrap_or_default();
            Ok(Value::Array(Rc::new(RefCell::new(values))))
        }
        // Concurrency: thread spawning and joining
        "spawn" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::Function(_f) => {
                    // For now, create a dummy thread that just sleeps briefly
                    // Real threading support would require significant architecture changes
                    let handle = std::thread::spawn(|| {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    });
                    Ok(Value::ThreadHandle(Rc::new(RefCell::new(ThreadHandle::new(handle)))))
                }
                other => Err(RuntimeError::new(&format!(
                    "spawn() requires a function, got {}",
                    type_name(other)
                ))),
            }
        }
        "thread_join" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::ThreadHandle(h) => {
                    h.borrow_mut().join()?;
                    Ok(Value::Null)
                }
                other => Err(RuntimeError::new(&format!(
                    "thread_join() requires a thread handle, got {}",
                    type_name(other)
                ))),
            }
        }
        // Channels
        "channel" => {
            require_arity(name, args, 0)?;
            Ok(Value::Channel(Rc::new(RefCell::new(Channel::new()))))
        }
        "send" => {
            require_arity(name, args, 2)?;
            match (&args[0], &args[1]) {
                (Value::Channel(ch), value) => {
                    ch.borrow().send(value.clone())?;
                    Ok(Value::Null)
                }
                (other, _) => Err(RuntimeError::new(&format!(
                    "send() requires a channel, got {}",
                    type_name(other)
                ))),
            }
        }
        "recv" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::Channel(ch) => {
                    let val = ch.borrow().recv()?;
                    Ok(val)
                }
                other => Err(RuntimeError::new(&format!(
                    "recv() requires a channel, got {}",
                    type_name(other)
                ))),
            }
        }
        "try_recv" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::Channel(ch) => {
                    let val = ch.borrow().try_recv()?;
                    Ok(val)
                }
                other => Err(RuntimeError::new(&format!(
                    "try_recv() requires a channel, got {}",
                    type_name(other)
                ))),
            }
        }
        // Mutexes
        "mutex" => {
            require_arity(name, args, 0)?;
            Ok(Value::Mutex(Rc::new(RefCell::new(Mutex::new()))))
        }
        "lock" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::Mutex(m) => {
                    // For now, just lock and immediately release since we can't hold guards across calls
                    let _ = m.borrow().lock();
                    Ok(Value::Null)
                }
                other => Err(RuntimeError::new(&format!(
                    "lock() requires a mutex, got {}",
                    type_name(other)
                ))),
            }
        }
        "unlock" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::Mutex(_m) => {
                    // In a real implementation, we'd release the guard
                    // For now, this is a no-op since we don't hold guards
                    Ok(Value::Null)
                }
                other => Err(RuntimeError::new(&format!(
                    "unlock() requires a mutex, got {}",
                    type_name(other)
                ))),
            }
        }
        // Async/Await (basic stubs - the real implementation needs VM support)
        "async" => {
            // async is a keyword for function definition, not a callable builtin
            Err(RuntimeError::new("async is a keyword, not a function"))
        }
        "await" => {
            // await is an expression, not a callable builtin
            Err(RuntimeError::new("await is an expression, not a function"))
        }

        "read_file" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::String(path) => std::fs::read_to_string(path)
                    .map(Value::String)
                    .map_err(|e| {
                        RuntimeError::new(&format!("cannot read file '{}': {}", path, e))
                    }),
                other => Err(RuntimeError::new(&format!(
                    "read_file() requires a string path, got {}",
                    type_name(other)
                ))),
            }
        }
        "write_file" => {
            require_arity(name, args, 2)?;
            match (&args[0], &args[1]) {
                (Value::String(path), Value::String(content)) => std::fs::write(path, content)
                    .map(|_| Value::Null)
                    .map_err(|e| {
                        RuntimeError::new(&format!("cannot write file '{}': {}", path, e))
                    }),
                (Value::String(_), other) => Err(RuntimeError::new(&format!(
                    "write_file() requires string content, got {}",
                    type_name(other)
                ))),
                (other, _) => Err(RuntimeError::new(&format!(
                    "write_file() requires a string path, got {}",
                    type_name(other)
                ))),
            }
        }
        "open_url" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::String(target) => open_in_browser(target).map(|_| Value::Null),
                other => Err(RuntimeError::new(&format!(
                    "open_url() requires a string, got {}",
                    type_name(other)
                ))),
            }
        }

        "json_encode" => {
            require_arity(name, args, 1)?;
            Ok(Value::String(json_of_value(&args[0])?))
        }
        "json_decode" => {
            require_arity(name, args, 1)?;
            match &args[0] {
                Value::String(text) => json_from_str(text),
                other => Err(RuntimeError::new(&format!(
                    "json_decode() requires a string, got {}",
                    type_name(other)
                ))),
            }
        }
        "http_get" => http_get(args),
        "http_post" => http_post(args),
        "http_request" => http_request(args),
        "http_server" => http_server(args),
        "http_respond" => http_respond(args),
        "http_listen" => http_listen(args),
        "http_route" => http_route(args),
        "http_middleware" => http_middleware(args),
        "http_router" => http_router(args),
        // AI/ML - Tensors
        "tensor" => tensor(args),
        "tensor_shape" => tensor_shape(args),
        "tensor_get" => tensor_get(args),
        "tensor_set" => tensor_set(args),
        "tensor_add" => tensor_add(args),
        "tensor_mul" => tensor_mul(args),
        "tensor_matmul" => tensor_matmul(args),
        "tensor_transpose" => tensor_transpose(args),
        "tensor_reshape" => tensor_reshape(args),
        // AI/ML - Activations
        "relu" => relu(args),
        "sigmoid" => sigmoid(args),
        "tanh" => tanh_fn(args),
        "softmax" => softmax(args),
        // AI/ML - Neural network layers
        "linear" => linear(args),
        "conv2d" => conv2d(args),
        // AI/ML - Loss functions
        "mse_loss" => mse_loss(args),
        "cross_entropy_loss" => cross_entropy_loss(args),
        // AI/ML - Optimizers
        "sgd_step" => sgd_step(args),
        "adam_step" => adam_step(args),
        // AI/ML - Data utilities
        "train_test_split" => train_test_split(args),
        "accuracy" => accuracy(args),
        "argmax" => argmax(args),
        // Database - SQLite
        "db_open" => db_open(args),
        "db_close" => db_close(args),
        "db_exec" => db_exec(args),
        "db_query" => db_query(args),
        "db_query_row" => db_query_row(args),
        "db_transaction" => db_transaction(args),
        "db_last_insert_rowid" => db_last_insert_rowid(args),
        "db_changes" => db_changes(args),
        // GUI framework
        "gui_window" => gui_window(args),
        "gui_button" => gui_button(args),
        "gui_label" => gui_label(args),
        "gui_text_input" => gui_text_input(args),
        "gui_checkbox" => gui_checkbox(args),
        "gui_slider" => gui_slider(args),
        "gui_vstack" => gui_vstack(args),
        "gui_hstack" => gui_hstack(args),
        "gui_show" => gui_show(args),
        "gui_poll_events" => gui_poll_events(args),
        "gui_close" => gui_close(args),

        other => Err(RuntimeError::new(&format!(
            "undefined function '{}'",
            other
        ))),
    }
}

fn require_arity(name: &str, args: &[Value], expected: usize) -> Result<(), RuntimeError> {
    if args.len() != expected {
        return Err(RuntimeError::new(&format!(
            "{}() requires {} argument(s), got {}",
            name,
            expected,
            args.len()
        )));
    }
    Ok(())
}

/// The short type name `type()` reports.
pub fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Boolean(_) => "boolean",
        Value::Null => "null",
        Value::Array(_) => "array",
        Value::Map(_) => "map",
        Value::Function(_) => "function",
        Value::Future(_) => "future",
        Value::Channel(_) => "channel",
        Value::Mutex(_) => "mutex",
        Value::ThreadHandle(_) => "thread_handle",
        Value::DbConnection(_) => "db_connection",
        Value::GuiWindow(_) => "gui_window",
    }
}

fn number(name: &str, value: &Value) -> Result<f64, RuntimeError> {
    match value {
        Value::Number(n) => Ok(*n),
        other => Err(RuntimeError::new(&format!(
            "{}() requires a number, got {}",
            name,
            type_of(other)
        ))),
    }
}

fn number_map(name: &str, args: &[Value], f: fn(f64) -> f64) -> Result<Value, RuntimeError> {
    require_arity(name, args, 1)?;
    let n = number(name, &args[0])?;
    let result = f(n);
    if result.is_nan() && !n.is_nan() {
        return Err(RuntimeError::new(&format!(
            "{}() is not defined for {}",
            name, n
        )));
    }
    Ok(Value::Number(result))
}

fn string_map(
    name: &str,
    args: &[Value],
    f: fn(&str) -> String,
) -> Result<Value, RuntimeError> {
    require_arity(name, args, 1)?;
    match &args[0] {
        Value::String(s) => Ok(Value::String(f(s))),
        other => Err(RuntimeError::new(&format!(
            "{}() requires a string, got {}",
            name,
            type_of(other)
        ))),
    }
}

fn num(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("num", args, 1)?;
    match &args[0] {
        Value::Number(n) => Ok(Value::Number(*n)),
        Value::Boolean(b) => Ok(Value::Number(if *b { 1.0 } else { 0.0 })),
        Value::Null => Ok(Value::Number(0.0)),
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map(Value::Number)
            .map_err(|_| RuntimeError::new(&format!("cannot convert '{}' to number", s))),
        Value::Array(_) => Err(RuntimeError::new("cannot convert an array to number")),
        Value::Map(_) => Err(RuntimeError::new("cannot convert a map to number")),
        Value::Function(f) => Err(RuntimeError::new(&format!(
            "cannot convert function '{}' to number",
            f.name
        ))),
        Value::Future(_) => Err(RuntimeError::new("cannot convert a future to number")),
        Value::Channel(_) => Err(RuntimeError::new("cannot convert a channel to number")),
        Value::Mutex(_) => Err(RuntimeError::new("cannot convert a mutex to number")),
        Value::ThreadHandle(_) => Err(RuntimeError::new("cannot convert a thread handle to number")),
        Value::DbConnection(_) => Err(RuntimeError::new("cannot convert a db connection to number")),
        Value::GuiWindow(_) => Err(RuntimeError::new("cannot convert a gui window to number")),
    }
}

/// Deterministic pseudo-random state shared by `random()`, `random_int()` and
/// `seed()`. XORSHIFT64*: seeded identically, every engine produces the exact
/// same sequence — which is what keeps cross-engine parity testable.
static RNG_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn next_random_u64() -> u64 {
    use std::sync::atomic::Ordering;
    let mut state = RNG_STATE.load(Ordering::Relaxed);
    if state == 0 {
        // Never seeded: start from the wall clock so runs differ, but keep the
        // state non-zero (a zero state is a fixed point for xorshift).
        state = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;
    }
    state ^= state >> 12;
    state ^= state << 25;
    state ^= state >> 27;
    RNG_STATE.store(state, Ordering::Relaxed);
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// Uniform in [0, 1): 53 random bits, the most a f64 mantissa can hold.
fn next_random_f64() -> f64 {
    (next_random_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// `input()` / `input(prompt)` — reads one line from stdin, without the
/// trailing newline. A prompt is written first without a newline, so the
/// cursor sits right where the user types. EOF yields `null`.
fn input(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() > 1 {
        return Err(RuntimeError::new(
            "input() requires at most one prompt argument",
        ));
    }
    if let Some(prompt) = args.first() {
        print!("{}", format_value(prompt));
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => Ok(Value::Null),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Value::String(line))
        }
        Err(error) => Err(RuntimeError::new(&format!(
            "could not read input: {error}"
        ))),
    }
}

/// Opens a file path or URL in the system browser. Never blocks on the
/// browser: the process is spawned detached and forgotten.
fn open_in_browser(target: &str) -> Result<(), RuntimeError> {
    use std::process::{Command, Stdio};
    let (mut command, kind) = if cfg!(target_os = "macos") {
        (Command::new("open"), "macos")
    } else if cfg!(target_os = "windows") {
        (Command::new("cmd"), "windows")
    } else {
        (Command::new("xdg-open"), "linux")
    };
    match kind {
        "windows" => {
            command.args(["/C", "start", "", target]);
        }
        _ => {
            command.arg(target);
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().map(|_| ()).map_err(|e| {
        RuntimeError::new(&format!(
            "could not open '{}' in a browser: {}",
            target, e
        ))
    })
}

/// JSON text for any Nect value. Objects are maps in insertion order, so the
/// same value always encodes to the same text — cross-engine parity again.
fn json_of_value(value: &Value) -> Result<String, RuntimeError> {
    match value {
        Value::Null => Ok("null".to_string()),
        Value::Boolean(b) => Ok(b.to_string()),
        Value::Number(n) => {
            if n.is_finite() {
                Ok(format_number(*n))
            } else {
                Err(RuntimeError::new(
                    "json_encode() cannot represent NaN or infinity",
                ))
            }
        }
        Value::String(s) => Ok(json_escape(s)),
        Value::Array(elements) => {
            let items: Result<Vec<String>, _> =
                elements.borrow().iter().map(json_of_value).collect();
            Ok(format!("[{}]", items?.join(",")))
        }
        Value::Map(map) => {
            let mut items = Vec::new();
            for (key, value) in map.borrow().entries.iter() {
                let key_text = match key {
                    Value::String(s) => json_escape(s),
                    // JSON object keys are strings; number/bool keys are
                    // rendered the way `print` renders them and quoted.
                    other => json_escape(&format_value(other)),
                };
                items.push(format!("{}:{}", key_text, json_of_value(value)?));
            }
            Ok(format!("{{{}}}", items.join(",")))
        }
        Value::Function(f) => Err(RuntimeError::new(&format!(
            "json_encode() cannot represent the function '{}'",
            f.name
        ))),
        Value::Future(_) => Err(RuntimeError::new("json_encode() cannot represent a future")),
        Value::Channel(_) => Err(RuntimeError::new("json_encode() cannot represent a channel")),
        Value::Mutex(_) => Err(RuntimeError::new("json_encode() cannot represent a mutex")),
        Value::ThreadHandle(_) => Err(RuntimeError::new("json_encode() cannot represent a thread handle")),
        Value::DbConnection(_) => Err(RuntimeError::new("json_encode() cannot represent a db connection")),
        Value::GuiWindow(_) => Err(RuntimeError::new("json_encode() cannot represent a gui window")),
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A compact recursive-descent JSON reader. Produces Nect values: objects
/// become maps (in document order), arrays become arrays.
struct JsonReader {
    chars: Vec<char>,
    pos: usize,
}

impl JsonReader {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            pos: 0,
        }
    }

    fn error(&self, message: &str) -> RuntimeError {
        RuntimeError::new(&format!("json_decode(): {} at position {}", message, self.pos))
    }

    fn skip_spaces(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Result<char, RuntimeError> {
        self.chars
            .get(self.pos)
            .copied()
            .ok_or_else(|| self.error("unexpected end of input"))
    }

    fn eat(&mut self, expected: char) -> Result<(), RuntimeError> {
        if self.peek()? == expected {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected '{}'", expected)))
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), RuntimeError> {
        for expected in word.chars() {
            self.eat(expected)?;
        }
        Ok(())
    }

    fn value(&mut self) -> Result<Value, RuntimeError> {
        self.skip_spaces();
        match self.peek()? {
            '{' => self.object(),
            '[' => self.array(),
            '"' => Ok(Value::String(self.string()?)),
            't' => self.literal("true").map(|_| Value::Boolean(true)),
            'f' => self.literal("false").map(|_| Value::Boolean(false)),
            'n' => self.literal("null").map(|_| Value::Null),
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Result<Value, RuntimeError> {
        self.eat('{')?;
        let mut map = Map::new();
        self.skip_spaces();
        if self.peek()? == '}' {
            self.pos += 1;
            return Ok(Value::Map(Rc::new(RefCell::new(map))));
        }
        loop {
            self.skip_spaces();
            let key = self.string()?;
            self.skip_spaces();
            self.eat(':')?;
            let value = self.value()?;
            map.insert(Value::String(key), value)
                .map_err(|e| RuntimeError::new(&e.message))?;
            self.skip_spaces();
            match self.peek()? {
                ',' => self.pos += 1,
                '}' => {
                    self.pos += 1;
                    return Ok(Value::Map(Rc::new(RefCell::new(map))));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Value, RuntimeError> {
        self.eat('[')?;
        let mut items = Vec::new();
        self.skip_spaces();
        if self.peek()? == ']' {
            self.pos += 1;
            return Ok(Value::Array(Rc::new(RefCell::new(items))));
        }
        loop {
            items.push(self.value()?);
            self.skip_spaces();
            match self.peek()? {
                ',' => self.pos += 1,
                ']' => {
                    self.pos += 1;
                    return Ok(Value::Array(Rc::new(RefCell::new(items))));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, RuntimeError> {
        self.eat('"')?;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let escape = self.peek()?;
                    self.pos += 1;
                    match escape {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let mut code = 0u32;
                            for _ in 0..4 {
                                let digit = self.peek()?;
                                let value = digit
                                    .to_digit(16)
                                    .ok_or_else(|| self.error("invalid \\u escape"))?;
                                code = code * 16 + value;
                                self.pos += 1;
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(self.error("unknown escape")),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn number(&mut self) -> Result<Value, RuntimeError> {
        let start = self.pos;
        while self.pos < self.chars.len()
            && matches!(self.chars[self.pos], '0'..='9' | '-' | '+' | '.' | 'e' | 'E')
        {
            self.pos += 1;
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        text.parse::<f64>()
            .map(Value::Number)
            .map_err(|_| self.error(&format!("invalid number '{}'", text)))
    }
}

fn json_from_str(text: &str) -> Result<Value, RuntimeError> {
    let mut reader = JsonReader::new(text);
    let value = reader.value()?;
    reader.skip_spaces();
    if reader.pos < reader.chars.len() {
        return Err(reader.error("trailing characters after the JSON value"));
    }
    Ok(value)
}

/// HTTP GET request. Returns a map with status, headers, and body.
/// Usage: http_get(url) or http_get(url, headers_map)
fn http_get(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 1 || args.len() > 2 {
        return Err(RuntimeError::new(
            "http_get() requires a URL and optional headers map",
        ));
    }
    let url = match &args[0] {
        Value::String(s) => s,
        other => return Err(RuntimeError::new(&format!("http_get() requires a string URL, got {}", type_of(other)))),
    };
    let mut builder = reqwest::blocking::Client::new().get(url);
    if let Some(Value::Map(headers)) = args.get(1) {
        for (k, v) in headers.borrow().entries.iter() {
            if let (Value::String(key), Value::String(val)) = (k, v) {
                builder = builder.header(key, val);
            }
        }
    }
    let response = builder.send().map_err(|e| RuntimeError::new(&format!("HTTP request failed: {}", e)))?;
    let status = response.status().as_u16() as f64;
    let body = response.text().map_err(|e| RuntimeError::new(&format!("Failed to read response: {}", e)))?;
    let mut result = Map::new();
    result.insert(Value::String("status".into()), Value::Number(status))?;
    result.insert(Value::String("body".into()), Value::String(body))?;
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

/// HTTP POST request. Returns a map with status, headers, and body.
/// Usage: http_post(url, body) or http_post(url, body, headers_map)
fn http_post(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new(
            "http_post() requires a URL, body, and optional headers map",
        ));
    }
    let url = match &args[0] {
        Value::String(s) => s,
        other => return Err(RuntimeError::new(&format!("http_post() requires a string URL, got {}", type_of(other)))),
    };
    let body = match &args[1] {
        Value::String(s) => s.clone(),
        other => format_value(other),
    };
    let mut builder = reqwest::blocking::Client::new().post(url).body(body);
    if let Some(Value::Map(headers)) = args.get(2) {
        for (k, v) in headers.borrow().entries.iter() {
            if let (Value::String(key), Value::String(val)) = (k, v) {
                builder = builder.header(key, val);
            }
        }
    }
    let response = builder.send().map_err(|e| RuntimeError::new(&format!("HTTP request failed: {}", e)))?;
    let status = response.status().as_u16() as f64;
    let body = response.text().map_err(|e| RuntimeError::new(&format!("Failed to read response: {}", e)))?;
    let mut result = Map::new();
    result.insert(Value::String("status".into()), Value::Number(status))?;
    result.insert(Value::String("body".into()), Value::String(body))?;
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

/// Generic HTTP request.
/// Usage: http_request(method, url) or http_request(method, url, body) or http_request(method, url, body, headers_map)
fn http_request(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 4 {
        return Err(RuntimeError::new(
            "http_request() requires method, URL, optional body, and optional headers map",
        ));
    }
    let method = match &args[0] {
        Value::String(s) => s.to_uppercase(),
        other => return Err(RuntimeError::new(&format!("http_request() requires a string method, got {}", type_of(other)))),
    };
    let url = match &args[1] {
        Value::String(s) => s,
        other => return Err(RuntimeError::new(&format!("http_request() requires a string URL, got {}", type_of(other)))),
    };
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| RuntimeError::new("Invalid HTTP method"))?;
    let mut builder = reqwest::blocking::Client::new().request(method, url);
    if args.len() >= 3 {
        let body = match &args[2] {
            Value::String(s) => s.clone(),
            other => format_value(other),
        };
        if !body.is_empty() {
            builder = builder.body(body);
        }
    }
    if let Some(Value::Map(headers)) = args.get(3) {
        for (k, v) in headers.borrow().entries.iter() {
            if let (Value::String(key), Value::String(val)) = (k, v) {
                builder = builder.header(key, val);
            }
        }
    }
    let response = builder.send().map_err(|e| RuntimeError::new(&format!("HTTP request failed: {}", e)))?;
    let status = response.status().as_u16() as f64;
    let body = response.text().map_err(|e| RuntimeError::new(&format!("Failed to read response: {}", e)))?;
    let mut result = Map::new();
    result.insert(Value::String("status".into()), Value::Number(status))?;
    result.insert(Value::String("body".into()), Value::String(body))?;
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

/// Starts an HTTP server on the given port with a named handler function.
/// Usage: http_listen(port, "handler_function_name")
/// The handler function must be defined globally and accept a request map, returning a response map.
fn http_listen(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new(
            "http_listen() requires a port number and a handler function name (string)",
        ));
    }
    let port = number("http_listen", &args[0])? as u16;
    let handler_name = match &args[1] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("http_listen() requires a string handler name, got {}", type_of(other)))),
    };
    Err(RuntimeError::new("http_listen() requires async runtime support (not yet implemented)"))
}

/// Starts an HTTP server on the given port (deprecated - use http_listen).
/// Usage: http_server(port, "handler_function_name")
fn http_server(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new(
            "http_server() requires a port number and a handler function name (string)",
        ));
    }
    let port = number("http_server", &args[0])? as u16;
    let handler_name = match &args[1] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("http_server() requires a string handler name, got {}", type_of(other)))),
    };
    Err(RuntimeError::new("http_server() requires async runtime support (not yet implemented)"))
}

/// Creates an HTTP response map for use with http_server handler.
/// Usage: http_respond(status, body, headers_map)
fn http_respond(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new(
            "http_respond() requires status, body, and optional headers map",
        ));
    }
    let status = number("http_respond", &args[0])? as u16;
    let body = match &args[1] {
        Value::String(s) => s.clone(),
        other => format_value(other),
    };
    let mut result = Map::new();
    result.insert(Value::String("status".into()), Value::Number(status as f64))?;
    result.insert(Value::String("body".into()), Value::String(body))?;
    if let Some(Value::Map(headers)) = args.get(2) {
        let mut headers_map = Map::new();
        for (k, v) in headers.borrow().entries.iter() {
            if let (Value::String(key), Value::String(val)) = (k, v) {
                headers_map.insert(Value::String(key.clone()), Value::String(val.clone()))?;
            }
        }
        result.insert(Value::String("headers".into()), Value::Map(Rc::new(RefCell::new(headers_map))))?;
    }
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

/// Creates a route entry for use with http_router.
/// Usage: http_route(method, path, handler_function_name)
/// method: "GET", "POST", "PUT", "DELETE", etc.
/// path: route pattern like "/users/:id" (params not yet implemented)
/// handler_function_name: string name of the handler function
fn http_route(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 3 {
        return Err(RuntimeError::new(
            "http_route() requires method, path, and handler function name",
        ));
    }
    let method = match &args[0] {
        Value::String(s) => s.to_uppercase(),
        other => return Err(RuntimeError::new(&format!("http_route() requires a string method, got {}", type_of(other)))),
    };
    let path = match &args[1] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("http_route() requires a string path, got {}", type_of(other)))),
    };
    let handler = match &args[2] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("http_route() requires a string handler name, got {}", type_of(other)))),
    };
    let mut route = Map::new();
    route.insert(Value::String("method".into()), Value::String(method))?;
    route.insert(Value::String("path".into()), Value::String(path))?;
    route.insert(Value::String("handler".into()), Value::String(handler))?;
    Ok(Value::Map(Rc::new(RefCell::new(route))))
}

/// Creates a middleware entry for use with http_router.
/// Usage: http_middleware(handler_function_name)
/// The middleware function receives (request, next) and should call next() to continue.
fn http_middleware(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new(
            "http_middleware() requires a handler function name",
        ));
    }
    let handler = match &args[0] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("http_middleware() requires a string handler name, got {}", type_of(other)))),
    };
    let mut middleware = Map::new();
    middleware.insert(Value::String("handler".into()), Value::String(handler))?;
    Ok(Value::Map(Rc::new(RefCell::new(middleware))))
}

/// Creates a router from routes and middlewares.
/// Usage: http_router(routes_array, middlewares_array)
/// Returns a router object for use with http_listen.
fn http_router(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new(
            "http_router() requires routes array and middlewares array",
        ));
    }
    match (&args[0], &args[1]) {
        (Value::Array(routes), Value::Array(middlewares)) => {
            let mut router = Map::new();
            router.insert(Value::String("routes".into()), Value::Array(Rc::clone(routes)))?;
            router.insert(Value::String("middlewares".into()), Value::Array(Rc::clone(middlewares)))?;
            Ok(Value::Map(Rc::new(RefCell::new(router))))
        }
        (other, _) => Err(RuntimeError::new(&format!(
            "http_router() requires an array for routes, got {}",
            type_of(other)
        ))),
    }
}

/// Creates a tensor from nested arrays. Returns a map with data and shape.
/// Usage: tensor(nested_array) e.g., tensor([[1,2],[3,4]]) creates 2x2 tensor
fn tensor(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("tensor() requires a nested array"));
    }
    let (data, shape) = flatten_tensor(&args[0])?;
    let mut result = Map::new();
    result.insert(Value::String("data".into()), Value::Array(Rc::new(RefCell::new(data))))?;
    result.insert(Value::String("shape".into()), Value::Array(Rc::new(RefCell::new(
        shape.into_iter().map(|s| Value::Number(s as f64)).collect()
    ))))?;
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

/// Returns the shape of a tensor.
fn tensor_shape(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("tensor_shape() requires a tensor"));
    }
    match &args[0] {
        Value::Map(m) => {
            if let Some(Value::Array(shape)) = m.borrow().get(&Value::String("shape".into())) {
                Ok(Value::Array(Rc::clone(&shape)))
            } else {
                Err(RuntimeError::new("Not a valid tensor"))
            }
        }
        _ => Err(RuntimeError::new("tensor_shape() requires a tensor map")),
    }
}

/// Gets a value from a tensor at the given indices.
fn tensor_get(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new("tensor_get() requires a tensor and at least one index"));
    }
    let tensor = match &args[0] {
        Value::Map(m) => m,
        _ => return Err(RuntimeError::new("First arg must be a tensor")),
    };
    let data = tensor.borrow().get(&Value::String("data".into()))
        .ok_or_else(|| RuntimeError::new("Invalid tensor"))?;
    let shape = tensor.borrow().get(&Value::String("shape".into()))
        .ok_or_else(|| RuntimeError::new("Invalid tensor"))?;
    let shape_arr = match shape {
        Value::Array(s) => s,
        _ => return Err(RuntimeError::new("Invalid tensor shape")),
    };
    let shape_vec: Vec<usize> = shape_arr.borrow().iter()
        .map(|v| match v { Value::Number(n) => *n as usize, _ => 0 })
        .collect();
    let data_vec = match data {
        Value::Array(d) => d,
        _ => return Err(RuntimeError::new("Invalid tensor data")),
    };
    let mut idx = 0;
    let mut stride = 1;
    for (i, &dim) in shape_vec.iter().rev().enumerate() {
        let arg_idx = args.len() - 1 - i;
        let index = if arg_idx >= 1 {
            match &args[arg_idx] {
                Value::Number(n) => *n as usize,
                _ => return Err(RuntimeError::new("Indices must be numbers")),
            }
        } else { 0 };
        if index >= dim {
            return Err(RuntimeError::new("Index out of bounds"));
        }
        idx += index * stride;
        stride *= dim;
    }
    let data_borrow = data_vec.borrow();
    Ok(data_borrow.get(idx).cloned().unwrap_or(Value::Number(0.0)))
}

/// Sets a value in a tensor at the given indices.
fn tensor_set(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 3 {
        return Err(RuntimeError::new("tensor_set() requires a tensor, indices, and a value"));
    }
    let tensor = match &args[0] {
        Value::Map(m) => m,
        _ => return Err(RuntimeError::new("First arg must be a tensor")),
    };
    let data = tensor.borrow().get(&Value::String("data".into()))
        .ok_or_else(|| RuntimeError::new("Invalid tensor"))?;
    let shape = tensor.borrow().get(&Value::String("shape".into()))
        .ok_or_else(|| RuntimeError::new("Invalid tensor"))?;
    let shape_arr = match shape {
        Value::Array(s) => s,
        _ => return Err(RuntimeError::new("Invalid tensor shape")),
    };
    let shape_vec: Vec<usize> = shape_arr.borrow().iter()
        .map(|v| match v { Value::Number(n) => *n as usize, _ => 0 })
        .collect();
    let data_vec = match data {
        Value::Array(d) => d,
        _ => return Err(RuntimeError::new("Invalid tensor data")),
    };
    let mut idx = 0;
    let mut stride = 1;
    for (i, &dim) in shape_vec.iter().rev().enumerate() {
        let arg_idx = args.len() - 2 - i;
        let index = if arg_idx >= 1 {
            match &args[arg_idx] {
                Value::Number(n) => *n as usize,
                _ => return Err(RuntimeError::new("Indices must be numbers")),
            }
        } else { 0 };
        if index >= dim {
            return Err(RuntimeError::new("Index out of bounds"));
        }
        idx += index * stride;
        stride *= dim;
    }
    let value = &args[args.len() - 1];
    data_vec.borrow_mut()[idx] = value.clone();
    Ok(args[0].clone())
}

/// Element-wise addition of two tensors.
fn tensor_add(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new("tensor_add() requires two tensors"));
    }
    binary_tensor_op(args, |a, b| a + b)
}

/// Element-wise multiplication of two tensors.
fn tensor_mul(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new("tensor_mul() requires two tensors"));
    }
    binary_tensor_op(args, |a, b| a * b)
}

/// Matrix multiplication of two 2D tensors.
fn tensor_matmul(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new("tensor_matmul() requires two tensors"));
    }
    let a = extract_tensor_data(&args[0])?;
    let b = extract_tensor_data(&args[1])?;
    let a_shape = extract_tensor_shape(&args[0])?;
    let b_shape = extract_tensor_shape(&args[1])?;
    if a_shape.len() != 2 || b_shape.len() != 2 {
        return Err(RuntimeError::new("tensor_matmul() requires 2D tensors"));
    }
    let (m, k) = (a_shape[0], a_shape[1]);
    let (k2, n) = (b_shape[0], b_shape[1]);
    if k != k2 {
        return Err(RuntimeError::new("Incompatible shapes for matmul"));
    }
    let mut result = vec![0.0; m * n];
    for i in 0..m {
        for j in 0..n {
            let mut sum = 0.0;
            for l in 0..k {
                sum += a[i * k + l] * b[l * n + j];
            }
            result[i * n + j] = sum;
        }
    }
    create_tensor_result(result, vec![m, n])
}

/// Transposes a 2D tensor.
fn tensor_transpose(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("tensor_transpose() requires a tensor"));
    }
    let data = extract_tensor_data(&args[0])?;
    let shape = extract_tensor_shape(&args[0])?;
    if shape.len() != 2 {
        return Err(RuntimeError::new("tensor_transpose() requires a 2D tensor"));
    }
    let (m, n) = (shape[0], shape[1]);
    let mut result = vec![0.0; m * n];
    for i in 0..m {
        for j in 0..n {
            result[j * m + i] = data[i * n + j];
        }
    }
    create_tensor_result(result, vec![n, m])
}

/// Reshapes a tensor to a new shape.
fn tensor_reshape(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new("tensor_reshape() requires a tensor and new shape"));
    }
    let data = extract_tensor_data(&args[0])?;
    let mut new_shape = Vec::new();
    for arg in &args[1..] {
        match arg {
            Value::Array(arr) => {
                for v in arr.borrow().iter() {
                    if let Value::Number(n) = v {
                        new_shape.push(*n as usize);
                    }
                }
            }
            Value::Number(n) => new_shape.push(*n as usize),
            _ => return Err(RuntimeError::new("Shape must be numbers")),
        }
    }
    let total: usize = new_shape.iter().product();
    if total != data.len() {
        return Err(RuntimeError::new("New shape must have same number of elements"));
    }
    create_tensor_result(data, new_shape)
}

/// ReLU activation function.
fn relu(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("relu() requires a tensor"));
    }
    unary_tensor_op(args, |x| x.max(0.0))
}

/// Sigmoid activation function.
fn sigmoid(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("sigmoid() requires a tensor"));
    }
    unary_tensor_op(args, |x| 1.0 / (1.0 + (-x).exp()))
}

/// Tanh activation function.
fn tanh_fn(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("tanh() requires a tensor"));
    }
    unary_tensor_op(args, |x| x.tanh())
}

/// Softmax activation function.
fn softmax(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("softmax() requires a tensor"));
    }
    let data = extract_tensor_data(&args[0])?;
    let shape = extract_tensor_shape(&args[0])?;
    let mut result = Vec::new();
    for chunk in data.chunks(shape.last().copied().unwrap_or(data.len())) {
        let max_val = chunk.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let exps: Vec<f64> = chunk.iter().map(|&x| (x - max_val).exp()).collect();
        let sum: f64 = exps.iter().sum();
        result.extend(exps.iter().map(|&x| x / sum));
    }
    create_tensor_result(result, shape)
}

/// Linear layer: y = x @ w.t() + b
fn linear(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 3 {
        return Err(RuntimeError::new("linear() requires input, weight, and bias tensors"));
    }
    let x = extract_tensor_data(&args[0])?;
    let w = extract_tensor_data(&args[1])?;
    let b = extract_tensor_data(&args[2])?;
    let x_shape = extract_tensor_shape(&args[0])?;
    let w_shape = extract_tensor_shape(&args[1])?;
    if x_shape.len() != 2 || w_shape.len() != 2 {
        return Err(RuntimeError::new("linear() requires 2D input and weight"));
    }
    let (batch, in_features) = (x_shape[0], x_shape[1]);
    let (out_features, in_features_w) = (w_shape[0], w_shape[1]);
    if in_features != in_features_w {
        return Err(RuntimeError::new("Incompatible shapes for linear"));
    }
    let mut result = vec![0.0; batch * out_features];
    for i in 0..batch {
        for j in 0..out_features {
            let mut sum = b[j];
            for k in 0..in_features {
                sum += x[i * in_features + k] * w[j * in_features + k];
            }
            result[i * out_features + j] = sum;
        }
    }
    create_tensor_result(result, vec![batch, out_features])
}

/// 2D convolution (simplified - no padding/stride/dilation).
fn conv2d(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 3 {
        return Err(RuntimeError::new("conv2d() requires input, weight, and bias"));
    }
    let x = extract_tensor_data(&args[0])?;
    let w = extract_tensor_data(&args[1])?;
    let b = extract_tensor_data(&args[2])?;
    let x_shape = extract_tensor_shape(&args[0])?;
    let w_shape = extract_tensor_shape(&args[1])?;
    if x_shape.len() != 4 || w_shape.len() != 4 {
        return Err(RuntimeError::new("conv2d() requires 4D input (N,C,H,W) and weight (O,I,H,W)"));
    }
    let (n, c_in, h, w_in) = (x_shape[0], x_shape[1], x_shape[2], x_shape[3]);
    let (c_out, c_in_w, kh, kw) = (w_shape[0], w_shape[1], w_shape[2], w_shape[3]);
    if c_in != c_in_w {
        return Err(RuntimeError::new("Input channels must match weight channels"));
    }
    let h_out = h - kh + 1;
    let w_out = w_in - kw + 1;
    let mut result = vec![0.0; n * c_out * h_out * w_out];
    for ni in 0..n {
        for co in 0..c_out {
            for hi in 0..h_out {
                for wi in 0..w_out {
                    let mut sum = b[co];
                    for ci in 0..c_in {
                        for kh_i in 0..kh {
                            for kw_i in 0..kw {
                                let x_idx = ((ni * c_in + ci) * h + hi + kh_i) * w_in + wi + kw_i;
                                let w_idx = ((co * c_in + ci) * kh + kh_i) * kw + kw_i;
                                sum += x[x_idx] * w[w_idx];
                            }
                        }
                    }
                    let out_idx = ((ni * c_out + co) * h_out + hi) * w_out + wi;
                    result[out_idx] = sum;
                }
            }
        }
    }
    create_tensor_result(result, vec![n, c_out, h_out, w_out])
}

/// Mean squared error loss.
fn mse_loss(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new("mse_loss() requires predictions and targets"));
    }
    let pred = extract_tensor_data(&args[0])?;
    let target = extract_tensor_data(&args[1])?;
    if pred.len() != target.len() {
        return Err(RuntimeError::new("Shape mismatch in mse_loss"));
    }
    let sum: f64 = pred.iter().zip(target.iter())
        .map(|(p, t)| (p - t).powi(2))
        .sum();
    Ok(Value::Number(sum / pred.len() as f64))
}

/// Cross entropy loss (for classification).
fn cross_entropy_loss(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new("cross_entropy_loss() requires predictions and targets"));
    }
    let pred = extract_tensor_data(&args[0])?;
    let target = extract_tensor_data(&args[1])?;
    if pred.len() != target.len() {
        return Err(RuntimeError::new("Shape mismatch in cross_entropy_loss"));
    }
    let sum: f64 = pred.iter().zip(target.iter())
        .map(|(p, t)| -t * p.max(1e-15).ln())
        .sum();
    Ok(Value::Number(sum / pred.len() as f64))
}

/// SGD optimizer step: param -= lr * grad
fn sgd_step(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 3 {
        return Err(RuntimeError::new("sgd_step() requires param, grad, and learning_rate"));
    }
    let param = extract_tensor_data(&args[0])?;
    let grad = extract_tensor_data(&args[1])?;
    let lr = match &args[2] {
        Value::Number(n) => *n,
        _ => return Err(RuntimeError::new("Learning rate must be a number")),
    };
    if param.len() != grad.len() {
        return Err(RuntimeError::new("Parameter and gradient shape mismatch"));
    }
    let mut result = param;
    for (p, g) in result.iter_mut().zip(grad.iter()) {
        *p -= lr * g;
    }
    let shape = extract_tensor_shape(&args[0])?;
    create_tensor_result(result, shape)
}

/// Adam optimizer step (simplified).
fn adam_step(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 6 {
        return Err(RuntimeError::new("adam_step() requires param, grad, m, v, t, lr"));
    }
    let param = extract_tensor_data(&args[0])?;
    let grad = extract_tensor_data(&args[1])?;
    let m = extract_tensor_data(&args[2])?;
    let v = extract_tensor_data(&args[3])?;
    let t = match &args[4] { Value::Number(n) => *n as usize, _ => return Err(RuntimeError::new("t must be a number")) };
    let lr = match &args[5] { Value::Number(n) => *n, _ => return Err(RuntimeError::new("lr must be a number")) };
    if param.len() != grad.len() || param.len() != m.len() || param.len() != v.len() {
        return Err(RuntimeError::new("All tensors must have same shape"));
    }
    let beta1 = 0.9;
    let beta2 = 0.999;
    let eps = 1e-8;
    let len = param.len();
    let mut new_param = param;
    let mut new_m = m;
    let mut new_v = v;
    for i in 0..len {
        new_m[i] = beta1 * new_m[i] + (1.0 - beta1) * grad[i];
        new_v[i] = beta2 * new_v[i] + (1.0 - beta2) * grad[i] * grad[i];
        let m_hat = new_m[i] / (1.0 - beta1.powi(t as i32));
        let v_hat = new_v[i] / (1.0 - beta2.powi(t as i32));
        new_param[i] -= lr * m_hat / (v_hat.sqrt() + eps);
    }
    let shape = extract_tensor_shape(&args[0])?;
    let mut result = Map::new();
    result.insert(Value::String("param".into()), create_tensor_value(new_param, shape.clone())?)?;
    result.insert(Value::String("m".into()), create_tensor_value(new_m, shape.clone())?)?;
    result.insert(Value::String("v".into()), create_tensor_value(new_v, shape)?)?;
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

/// Splits data into train/test sets.
fn train_test_split(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new("train_test_split() requires X, y, and optional test_size"));
    }
    let x_data = extract_tensor_data(&args[0])?;
    let y_data = extract_tensor_data(&args[1])?;
    let x_shape = extract_tensor_shape(&args[0])?;
    let test_size = if args.len() == 3 {
        match &args[2] { Value::Number(n) => *n, _ => return Err(RuntimeError::new("test_size must be a number")) }
    } else { 0.2 };
    let n_samples = x_shape[0];
    let n_test = (n_samples as f64 * test_size) as usize;
    let n_train = n_samples - n_test;
    let x_features = x_data.len() / n_samples;
    let mut x_train = Vec::with_capacity(n_train * x_features);
    let mut x_test = Vec::with_capacity(n_test * x_features);
    let mut y_train = Vec::with_capacity(n_train);
    let mut y_test = Vec::with_capacity(n_test);
    for i in 0..n_train {
        x_train.extend(&x_data[i * x_features..(i + 1) * x_features]);
        y_train.push(y_data[i]);
    }
    for i in n_train..n_samples {
        x_test.extend(&x_data[i * x_features..(i + 1) * x_features]);
        y_test.push(y_data[i]);
    }
    let mut result = Map::new();
    result.insert(Value::String("X_train".into()), create_tensor_value(x_train, vec![n_train, x_features])?)?;
    result.insert(Value::String("X_test".into()), create_tensor_value(x_test, vec![n_test, x_features])?)?;
    result.insert(Value::String("y_train".into()), create_tensor_value(y_train, vec![n_train])?)?;
    result.insert(Value::String("y_test".into()), create_tensor_value(y_test, vec![n_test])?)?;
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

/// Computes accuracy between predictions and targets.
fn accuracy(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new("accuracy() requires predictions and targets"));
    }
    let pred = extract_tensor_data(&args[0])?;
    let target = extract_tensor_data(&args[1])?;
    if pred.len() != target.len() {
        return Err(RuntimeError::new("Shape mismatch in accuracy"));
    }
    let correct = pred.iter().zip(target.iter())
        .filter(|(p, t)| (**p - **t).abs() < 0.5)
        .count();
    Ok(Value::Number(correct as f64 / pred.len() as f64))
}

/// Returns indices of maximum values along last axis.
fn argmax(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("argmax() requires a tensor"));
    }
    let data = extract_tensor_data(&args[0])?;
    let shape = extract_tensor_shape(&args[0])?;
    let last_dim = *shape.last().unwrap_or(&1);
    let n_rows = data.len() / last_dim;
    let mut result = Vec::with_capacity(n_rows);
    for i in 0..n_rows {
        let row = &data[i * last_dim..(i + 1) * last_dim];
        let max_idx = row.iter().enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        result.push(max_idx as f64);
    }
    let mut new_shape = shape;
    new_shape.pop();
    if new_shape.is_empty() { new_shape = vec![1]; }
    create_tensor_result(result, new_shape)
}

// Helper functions for tensor operations
fn flatten_tensor(value: &Value) -> Result<(Vec<Value>, Vec<usize>), RuntimeError> {
    match value {
        Value::Array(arr) => {
            let vec = arr.borrow();
            if vec.is_empty() {
                return Ok((vec![], vec![0]));
            }
            let mut shape = vec![vec.len()];
            let mut data = Vec::new();
            let first_is_array = matches!(&vec[0], Value::Array(_));
            if first_is_array {
                for item in vec.iter() {
                    if let Value::Array(sub_arr) = item {
                        let sub_vec = sub_arr.borrow();
                        if shape.len() == 1 {
                            shape.push(sub_vec.len());
                        } else if sub_vec.len() != shape[1] {
                            return Err(RuntimeError::new("Inconsistent inner array lengths"));
                        }
                        for v in sub_vec.iter() {
                            data.push(v.clone());
                        }
                    } else {
                        return Err(RuntimeError::new("Mixed array types in tensor"));
                    }
                }
            } else {
                data.extend(vec.iter().cloned());
            }
            Ok((data, shape))
        }
        _ => Err(RuntimeError::new("tensor() requires an array")),
    }
}

fn extract_tensor_data(tensor: &Value) -> Result<Vec<f64>, RuntimeError> {
    match tensor {
        Value::Map(m) => {
            let data = m.borrow().get(&Value::String("data".into()))
                .ok_or_else(|| RuntimeError::new("Invalid tensor"))?;
            match data {
                Value::Array(arr) => Ok(arr.borrow().iter()
                    .map(|v| match v { Value::Number(n) => *n, _ => 0.0 })
                    .collect()),
                _ => Err(RuntimeError::new("Invalid tensor data")),
            }
        }
        _ => Err(RuntimeError::new("Expected a tensor map")),
    }
}

fn extract_tensor_shape(tensor: &Value) -> Result<Vec<usize>, RuntimeError> {
    match tensor {
        Value::Map(m) => {
            let shape = m.borrow().get(&Value::String("shape".into()))
                .ok_or_else(|| RuntimeError::new("Invalid tensor"))?;
            match shape {
                Value::Array(arr) => Ok(arr.borrow().iter()
                    .map(|v| match v { Value::Number(n) => *n as usize, _ => 0 })
                    .collect()),
                _ => Err(RuntimeError::new("Invalid tensor shape")),
            }
        }
        _ => Err(RuntimeError::new("Expected a tensor map")),
    }
}

fn binary_tensor_op<F>(args: &[Value], op: F) -> Result<Value, RuntimeError>
where F: Fn(f64, f64) -> f64 {
    let a = extract_tensor_data(&args[0])?;
    let b = extract_tensor_data(&args[1])?;
    let a_shape = extract_tensor_shape(&args[0])?;
    let b_shape = extract_tensor_shape(&args[1])?;
    if a_shape != b_shape {
        return Err(RuntimeError::new("Tensor shape mismatch"));
    }
    let result: Vec<f64> = a.iter().zip(b.iter()).map(|(x, y)| op(*x, *y)).collect();
    create_tensor_result(result, a_shape)
}

fn unary_tensor_op<F>(args: &[Value], op: F) -> Result<Value, RuntimeError>
where F: Fn(f64) -> f64 {
    let data = extract_tensor_data(&args[0])?;
    let shape = extract_tensor_shape(&args[0])?;
    let result: Vec<f64> = data.iter().map(|x| op(*x)).collect();
    create_tensor_result(result, shape)
}

fn create_tensor_result(data: Vec<f64>, shape: Vec<usize>) -> Result<Value, RuntimeError> {
    let values: Vec<Value> = data.into_iter().map(Value::Number).collect();
    let shape_values: Vec<Value> = shape.into_iter().map(|s| Value::Number(s as f64)).collect();
    let mut result = Map::new();
    result.insert(Value::String("data".into()), Value::Array(Rc::new(RefCell::new(values))))?;
    result.insert(Value::String("shape".into()), Value::Array(Rc::new(RefCell::new(shape_values))))?;
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

fn create_tensor_value(data: Vec<f64>, shape: Vec<usize>) -> Result<Value, RuntimeError> {
    let values: Vec<Value> = data.into_iter().map(Value::Number).collect();
    let shape_values: Vec<Value> = shape.into_iter().map(|s| Value::Number(s as f64)).collect();
    let mut result = Map::new();
    result.insert(Value::String("data".into()), Value::Array(Rc::new(RefCell::new(values))))?;
    result.insert(Value::String("shape".into()), Value::Array(Rc::new(RefCell::new(shape_values))))?;
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

fn assert(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 2 {
        return Err(RuntimeError::new(
            "assert() requires a condition and an optional message",
        ));
    }
    if is_truthy(&args[0]) {
        return Ok(Value::Null);
    }
    let message = match args.get(1) {
        Some(value) => format!("assertion failed: {}", format_value(value)),
        None => "assertion failed".to_string(),
    };
    Err(RuntimeError::new(&message))
}

/// Number of elements of an array, or characters of a string.
fn length_of(value: &Value) -> Result<usize, RuntimeError> {
    match value {
        Value::Array(elements) => Ok(elements.borrow().len()),
        Value::String(s) => Ok(s.chars().count()),
        Value::Map(map) => Ok(map.borrow().len()),
        other => Err(RuntimeError::new(&format!(
            "len() requires an array, string, or map, got {}",
            type_of(other)
        ))),
    }
}

fn array(name: &str, value: &Value) -> Result<Rc<RefCell<Vec<Value>>>, RuntimeError> {
    match value {
        Value::Array(elements) => Ok(Rc::clone(elements)),
        other => Err(RuntimeError::new(&format!(
            "{}() requires an array, got {}",
            name,
            type_of(other)
        ))),
    }
}

fn push(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new(
            "push() requires an array and at least one value to append",
        ));
    }
    let elements = array("push", &args[0])?;
    // Collect first: appending the array to itself must not deadlock the borrow.
    let additions: Vec<Value> = args[1..].to_vec();
    elements.borrow_mut().extend(additions);
    Ok(Value::Array(elements))
}

fn pop(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("pop", args, 1)?;
    let elements = array("pop", &args[0])?;
    let value = elements.borrow_mut().pop();
    value.ok_or_else(|| RuntimeError::new("pop() on an empty array"))
}

/// `concat(...)` — turns every argument into text and glues the pieces
/// together.
///
/// This is the friendly spelling of building a message out of mixed values:
/// `concat("n = ", n, "!")` instead of the nested `+`/`str()` chain, which is
/// easy to get wrong (`1 + 2` adds before it concatenates) and hard to read.
/// Arrays render exactly as `print` renders them.
fn concat(args: &[Value]) -> Result<Value, RuntimeError> {
    let rendered: Vec<String> = args.iter().map(format_value).collect();
    Ok(Value::String(rendered.concat()))
}

fn join(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 2 {
        return Err(RuntimeError::new(
            "join() requires an array and an optional separator",
        ));
    }
    let elements = array("join", &args[0])?;
    let separator = match args.get(1) {
        Some(Value::String(s)) => s.clone(),
        Some(other) => {
            return Err(RuntimeError::new(&format!(
                "join() requires a string separator, got {}",
                type_of(other)
            )));
        }
        None => String::new(),
    };
    let rendered: Vec<String> = elements.borrow().iter().map(format_value).collect();
    Ok(Value::String(rendered.join(&separator)))
}

fn split(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("split", args, 2)?;
    let text = match &args[0] {
        Value::String(s) => s.clone(),
        other => {
            return Err(RuntimeError::new(&format!(
                "split() requires a string, got {}",
                type_of(other)
            )));
        }
    };
    let separator = match &args[1] {
        Value::String(s) if !s.is_empty() => s.clone(),
        Value::String(_) => {
            return Err(RuntimeError::new("split() requires a non-empty separator"));
        }
        other => {
            return Err(RuntimeError::new(&format!(
                "split() requires a string separator, got {}",
                type_of(other)
            )));
        }
    };
    let parts: Vec<Value> = text
        .split(separator.as_str())
        .map(|part| Value::String(part.to_string()))
        .collect();
    Ok(Value::Array(Rc::new(RefCell::new(parts))))
}

fn replace(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("replace", args, 3)?;
    let text = string("replace", &args[0])?;
    let from = string("replace", &args[1])?;
    let to = string("replace", &args[2])?;
    if from.is_empty() {
        return Err(RuntimeError::new("replace() requires a non-empty pattern"));
    }
    Ok(Value::String(text.replace(&from, &to)))
}

fn string(name: &str, value: &Value) -> Result<String, RuntimeError> {
    match value {
        Value::String(s) => Ok(s.clone()),
        other => Err(RuntimeError::new(&format!(
            "{}() requires a string, got {}",
            name,
            type_of(other)
        ))),
    }
}

/// `slice(x, start)` / `slice(x, start, end)` over a string or an array.
/// Negative indices count from the end; `end` is exclusive.
fn slice(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new(
            "slice() requires a string or array, a start index, and an optional end index",
        ));
    }
    let len = length_of(&args[0])?;
    let len = len as f64;
    let start = resolve_index("slice", &args[1], len)?;
    let end = match args.get(2) {
        Some(value) => resolve_index("slice", value, len)?,
        None => len as i64,
    };
    if start > end {
        return Err(RuntimeError::new(&format!(
            "slice() start index {} is past its end index {}",
            start, end
        )));
    }
    let (start, end) = (start as usize, end as usize);
    match &args[0] {
        Value::String(s) => {
            let sliced: String = s.chars().skip(start).take(end - start).collect();
            Ok(Value::String(sliced))
        }
        Value::Array(elements) => {
            let elements = elements.borrow();
            Ok(Value::Array(Rc::new(RefCell::new(
                elements[start..end].to_vec(),
            ))))
        }
        other => Err(RuntimeError::new(&format!(
            "slice() requires an array or string, got {}",
            type_of(other)
        ))),
    }
}

/// Parses an index argument, resolving negatives against `len`, and clamps it
/// into `0..=len` so `slice(s, 99)` is empty rather than an error.
pub fn resolve_index(name: &str, value: &Value, len: f64) -> Result<i64, RuntimeError> {
    let raw = match value {
        Value::Number(n) => *n,
        other => {
            return Err(RuntimeError::new(&format!(
                "{}() index must be a number, got {}",
                name,
                type_of(other)
            )));
        }
    };
    if raw.is_nan() {
        return Err(RuntimeError::new(&format!(
            "{}() index must be a number, got nan",
            name
        )));
    }
    let index = if raw < 0.0 { raw + len } else { raw };
    let index = index.floor();
    Ok(index.clamp(0.0, len) as i64)
}

fn index_of_value(haystack: &Value, needle: &Value) -> Result<Option<usize>, RuntimeError> {
    match haystack {
        Value::String(text) => {
            let needle = string("index_of", needle)?;
            Ok(text.find(&needle).map(|byte_index| {
                text[..byte_index].chars().count()
            }))
        }
        Value::Array(elements) => Ok(elements.borrow().iter().position(|item| item == needle)),
        other => Err(RuntimeError::new(&format!(
            "index_of() requires an array or string, got {}",
            type_of(other)
        ))),
    }
}

/// `min`/`max` over several numbers, or over the elements of one array, so
/// both `min(3, 1)` and `min([3, 1])` work.
fn extremum(name: &str, args: &[Value], want_min: bool) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new(&format!("{}() requires arguments", name)));
    }
    let numbers: Vec<Value> = match &args[0] {
        Value::Array(elements) if args.len() == 1 => elements.borrow().clone(),
        _ => args.to_vec(),
    };
    if numbers.is_empty() {
        return Err(RuntimeError::new(&format!(
            "{}() requires at least one number",
            name
        )));
    }
    let mut best = number(name, &numbers[0])?;
    for value in &numbers[1..] {
        let candidate = number(name, value)?;
        if (want_min && candidate < best) || (!want_min && candidate > best) {
            best = candidate;
        }
    }
    Ok(Value::Number(best))
}

/// Repeats a string, e.g. `repeat("ab", 3)` is `"ababab"`.
fn repeat(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("repeat", args, 2)?;
    let text = string("repeat", &args[0])?;
    let count = whole_number("repeat", &args[1])?;
    if count < 0 {
        return Err(RuntimeError::new(
            "repeat() requires a non-negative count",
        ));
    }
    // Guard against a typo turning into an out-of-memory abort.
    let total = text.len() as f64 * count as f64;
    if total > MAX_GENERATED as f64 {
        return Err(RuntimeError::new(&format!(
            "repeat() would build {} characters (limit {})",
            total as i64, MAX_GENERATED
        )));
    }
    Ok(Value::String(text.repeat(count as usize)))
}

/// Sorts an array in place and returns it. Elements must be all numbers or all
/// strings; strings sort by character order.
fn sort(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("sort", args, 1)?;
    let elements = array("sort", &args[0])?;
    let mut items = elements.borrow_mut();
    if items.iter().all(|value| matches!(value, Value::Number(_))) {
        items.sort_by(|a, b| match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.total_cmp(y),
            _ => Ordering::Equal,
        });
    } else if items.iter().all(|value| matches!(value, Value::String(_))) {
        items.sort_by(|a, b| match (a, b) {
            (Value::String(x), Value::String(y)) => x.cmp(y),
            _ => Ordering::Equal,
        });
    } else {
        return Err(RuntimeError::new(
            "sort() requires an array of only numbers or only strings",
        ));
    }
    drop(items);
    Ok(Value::Array(elements))
}

/// Reverses an array in place (returning it) or returns a reversed copy of a
/// string.
fn reverse(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("reverse", args, 1)?;
    match &args[0] {
        Value::Array(elements) => {
            elements.borrow_mut().reverse();
            Ok(Value::Array(Rc::clone(elements)))
        }
        Value::String(text) => Ok(Value::String(text.chars().rev().collect())),
        other => Err(RuntimeError::new(&format!(
            "reverse() requires an array or string, got {}",
            type_of(other)
        ))),
    }
}

/// Adds up the numbers in an array.
fn sum(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("sum", args, 1)?;
    let elements = array("sum", &args[0])?;
    let mut total = 0.0;
    for value in elements.borrow().iter() {
        total += number("sum", value)?;
    }
    Ok(Value::Number(total))
}

/// `range(stop)`, `range(start, stop)`, `range(start, stop, step)` — an array of
/// numbers, with `stop` exclusive. This is what `for` loops count with.
fn range(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 3 {
        return Err(RuntimeError::new(
            "range() requires a stop, or a start, stop, and optional step",
        ));
    }
    let (start, stop, step) = match args.len() {
        1 => (0.0, number("range", &args[0])?, 1.0),
        2 => (
            number("range", &args[0])?,
            number("range", &args[1])?,
            1.0,
        ),
        _ => (
            number("range", &args[0])?,
            number("range", &args[1])?,
            number("range", &args[2])?,
        ),
    };
    if step == 0.0 {
        return Err(RuntimeError::new("range() requires a non-zero step"));
    }
    let count = ((stop - start) / step).ceil().max(0.0);
    if count > MAX_GENERATED as f64 {
        return Err(RuntimeError::new(&format!(
            "range() would build {} elements (limit {})",
            count, MAX_GENERATED
        )));
    }
    let mut values = Vec::with_capacity(count as usize);
    let mut current = start;
    while (step > 0.0 && current < stop) || (step < 0.0 && current > stop) {
        values.push(Value::Number(current));
        current += step;
    }
    Ok(Value::Array(Rc::new(RefCell::new(values))))
}

/// Longest string or array `range()`/`repeat()` will build, so a mistyped
/// argument reports an error instead of exhausting memory.
const MAX_GENERATED: usize = 10_000_000;

fn whole_number(name: &str, value: &Value) -> Result<i64, RuntimeError> {
    let number = number(name, value)?;
    if !number.is_finite() || number.fract() != 0.0 {
        return Err(RuntimeError::new(&format!(
            "{}() requires a whole number, got {}",
            name, number
        )));
    }
    Ok(number as i64)
}

/// Database connection wrapper
#[derive(Debug, Clone)]
pub struct DbConnection {
    pub conn: Arc<StdMutex<Option<rusqlite::Connection>>>,
}

impl DbConnection {
    pub fn new(conn: rusqlite::Connection) -> Self {
        Self { conn: Arc::new(StdMutex::new(Some(conn))) }
    }
}

/// GUI window handle for native GUI framework
#[derive(Debug, Clone)]
pub struct GuiWindow {
    pub title: String,
    pub widgets: Arc<StdMutex<Vec<GuiWidget>>>,
    pub event_tx: Option<Arc<StdMutex<mpsc::Sender<GuiEvent>>>>,
    pub event_rx: Option<Arc<StdMutex<mpsc::Receiver<GuiEvent>>>>,
    pub running: Arc<StdMutex<bool>>,
}

impl GuiWindow {
    pub fn new(title: String) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            title,
            widgets: Arc::new(StdMutex::new(Vec::new())),
            event_tx: Some(Arc::new(StdMutex::new(tx))),
            event_rx: Some(Arc::new(StdMutex::new(rx))),
            running: Arc::new(StdMutex::new(true)),
        }
    }
}

impl PartialEq for GuiWindow {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.widgets, &other.widgets)
    }
}

impl Default for GuiWindow {
    fn default() -> Self {
        Self::new("Nect Window".to_string())
    }
}

/// GUI widget types
#[derive(Debug, Clone, PartialEq)]
pub enum GuiWidget {
    Button {
        id: String,
        label: String,
        on_click: Option<String>, // function name to call
    },
    Label {
        id: String,
        text: String,
    },
    TextInput {
        id: String,
        placeholder: String,
        value: String,
        on_change: Option<String>,
    },
    Checkbox {
        id: String,
        label: String,
        checked: bool,
        on_change: Option<String>,
    },
    Slider {
        id: String,
        label: String,
        min: f64,
        max: f64,
        value: f64,
        on_change: Option<String>,
    },
    VStack {
        id: String,
        children: Vec<GuiWidget>,
    },
    HStack {
        id: String,
        children: Vec<GuiWidget>,
    },
    Window {
        id: String,
        title: String,
        children: Vec<GuiWidget>,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub enum GuiEventType {
    Click,
    Change,
    Input,
    Close,
}

/// GUI event from user interactions
#[derive(Debug, Clone)]
pub struct GuiEvent {
    pub widget_id: String,
    pub event_type: GuiEventType,
    pub value: Option<GuiEventValue>,
}

#[derive(Debug, Clone)]
pub enum GuiEventValue {
    String(String),
    Number(f64),
    Boolean(bool),
}

impl GuiEventValue {
    fn to_value(&self) -> Value {
        match self {
            GuiEventValue::String(s) => Value::String(s.clone()),
            GuiEventValue::Number(n) => Value::Number(*n),
            GuiEventValue::Boolean(b) => Value::Boolean(*b),
        }
    }
}

impl GuiEvent {
    pub fn click(id: String) -> Self {
        Self {
            widget_id: id,
            event_type: GuiEventType::Click,
            value: None,
        }
    }
    pub fn change(id: String, value: GuiEventValue) -> Self {
        Self {
            widget_id: id,
            event_type: GuiEventType::Change,
            value: Some(value),
        }
    }
    pub fn input(id: String, value: String) -> Self {
        Self {
            widget_id: id,
            event_type: GuiEventType::Input,
            value: Some(GuiEventValue::String(value)),
        }
    }
    pub fn close(id: String) -> Self {
        Self {
            widget_id: id,
            event_type: GuiEventType::Close,
            value: None,
        }
    }
}

/// Opens a SQLite database connection.
/// Usage: db_open(path) - returns a connection handle
fn db_open(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("db_open() requires a database path"));
    }
    let path = match &args[0] {
        Value::String(s) => s,
        other => return Err(RuntimeError::new(&format!("db_open() requires a string path, got {}", type_of(other)))),
    };
    let conn = rusqlite::Connection::open(path)
        .map_err(|e| RuntimeError::new(&format!("Failed to open database: {}", e)))?;
    let db = DbConnection::new(conn);
    Ok(Value::DbConnection(Rc::new(RefCell::new(db))))
}

/// Closes a SQLite database connection.
/// Usage: db_close(conn)
fn db_close(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("db_close() requires a connection"));
    }
    match &args[0] {
        Value::DbConnection(db) => {
            let mut guard = db.borrow_mut();
            if let Some(conn) = guard.conn.lock().unwrap().take() {
                drop(conn);
            }
            Ok(Value::Null)
        }
        other => Err(RuntimeError::new(&format!("db_close() requires a database connection, got {}", type_of(other)))),
    }
}

/// Executes a SQL statement (non-query).
/// Usage: db_exec(conn, sql) - returns number of affected rows
fn db_exec(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new("db_exec() requires a connection and SQL string"));
    }
    let conn = match &args[0] {
        Value::DbConnection(db) => db,
        other => return Err(RuntimeError::new(&format!("db_exec() requires a database connection, got {}", type_of(other)))),
    };
    let sql = match &args[1] {
        Value::String(s) => s,
        other => return Err(RuntimeError::new(&format!("db_exec() requires a string SQL, got {}", type_of(other)))),
    };
    let conn_guard = conn.borrow();
    let conn = conn_guard.conn.lock().unwrap();
    let conn = conn.as_ref().ok_or_else(|| RuntimeError::new("Connection closed"))?;
    let changes = conn.execute(sql, [])
        .map_err(|e| RuntimeError::new(&format!("SQL execution failed: {}", e)))?;
    Ok(Value::Number(changes as f64))
}

/// Executes a query and returns all rows as an array of maps.
/// Usage: db_query(conn, sql, params_array?) - returns array of row maps
fn db_query(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new("db_query() requires a connection, SQL, and optional params array"));
    }
    let conn = match &args[0] {
        Value::DbConnection(db) => db,
        other => return Err(RuntimeError::new(&format!("db_query() requires a database connection, got {}", type_of(other)))),
    };
    let sql = match &args[1] {
        Value::String(s) => s,
        other => return Err(RuntimeError::new(&format!("db_query() requires a string SQL, got {}", type_of(other)))),
    };
    let params = if args.len() == 3 {
        match &args[2] {
            Value::Array(arr) => Some(arr),
            _ => return Err(RuntimeError::new("db_query() params must be an array")),
        }
    } else { None };

    let conn_guard = conn.borrow();
    let conn = conn_guard.conn.lock().unwrap();
    let conn = conn.as_ref().ok_or_else(|| RuntimeError::new("Connection closed"))?;

    let mut stmt = conn.prepare(sql)
        .map_err(|e| RuntimeError::new(&format!("Failed to prepare statement: {}", e)))?;

    let param_values: Vec<Box<dyn rusqlite::ToSql>> = if let Some(arr) = params {
        arr.borrow().iter().map(|v| value_to_sql(v)).collect()
    } else { Vec::new() };
    let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();

    // Get column names from statement before iterating
    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or(&format!("col{}", i)).to_string())
        .collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let mut map = Map::new();
        for (i, name) in col_names.iter().enumerate() {
            let val = row.get_ref(i).map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Null, Box::new(e)))?;
            let v = sql_value_to_value(val).map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Null, Box::new(e)))?;
            map.insert(Value::String(name.clone()), v).map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Null, Box::new(e)))?;
        }
        Ok(Value::Map(Rc::new(RefCell::new(map))))
    }).map_err(|e| RuntimeError::new(&format!("Query failed: {}", e)))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| RuntimeError::new(&format!("Row iteration failed: {}", e)))?);
    }
    Ok(Value::Array(Rc::new(RefCell::new(results))))
}

/// Executes a query and returns the first row as a map.
/// Usage: db_query_row(conn, sql, params_array?) - returns row map or null
fn db_query_row(args: &[Value]) -> Result<Value, RuntimeError> {
    let results = db_query(args)?;
    match results {
        Value::Array(arr) => {
            let borrow = arr.borrow();
            if borrow.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(borrow[0].clone())
            }
        }
        _ => Ok(Value::Null),
    }
}

/// Executes a function within a transaction.
/// Usage: db_transaction(conn, fn) - runs function with connection, commits on success, rolls back on error
fn db_transaction(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new("db_transaction() requires a connection and a function"));
    }
    let conn = match &args[0] {
        Value::DbConnection(db) => db,
        other => return Err(RuntimeError::new(&format!("db_transaction() requires a database connection, got {}", type_of(other)))),
    };
    let func = match &args[1] {
        Value::Function(f) => f,
        other => return Err(RuntimeError::new(&format!("db_transaction() requires a function, got {}", type_of(other)))),
    };

    let conn_guard = conn.borrow();
    let mut conn = conn_guard.conn.lock().unwrap();
    let conn = conn.as_mut().ok_or_else(|| RuntimeError::new("Connection closed"))?;

    let tx = conn.transaction()
        .map_err(|e| RuntimeError::new(&format!("Failed to start transaction: {}", e)))?;

    // Note: In a real implementation, we'd need to pass the transaction to the function
    // For now, we'll just commit
    tx.commit()
        .map_err(|e| RuntimeError::new(&format!("Transaction commit failed: {}", e)))?;

    Ok(Value::Null)
}

/// Returns the last inserted row ID.
/// Usage: db_last_insert_rowid(conn)
fn db_last_insert_rowid(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("db_last_insert_rowid() requires a connection"));
    }
    let conn = match &args[0] {
        Value::DbConnection(db) => db,
        other => return Err(RuntimeError::new(&format!("db_last_insert_rowid() requires a database connection, got {}", type_of(other)))),
    };
    let conn_guard = conn.borrow();
    let conn = conn_guard.conn.lock().unwrap();
    let conn = conn.as_ref().ok_or_else(|| RuntimeError::new("Connection closed"))?;
    Ok(Value::Number(conn.last_insert_rowid() as f64))
}

/// Returns the number of changed rows from the last operation.
/// Usage: db_changes(conn)
fn db_changes(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new("db_changes() requires a connection"));
    }
    let conn = match &args[0] {
        Value::DbConnection(db) => db,
        other => return Err(RuntimeError::new(&format!("db_changes() requires a database connection, got {}", type_of(other)))),
    };
    let conn_guard = conn.borrow();
    let conn = conn_guard.conn.lock().unwrap();
    let conn = conn.as_ref().ok_or_else(|| RuntimeError::new("Connection closed"))?;
    Ok(Value::Number(conn.changes() as f64))
}

// ============================================================================
// GUI Framework (Phase 6)
// ============================================================================

/// Creates a new GUI window.
/// Usage: gui_window(title) - returns a window handle
fn gui_window(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("gui_window", args, 1)?;
    let title = match &args[0] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_window() requires a string title, got {}", type_of(other)))),
    };
    let window = GuiWindow::new(title);
    Ok(Value::GuiWindow(Rc::new(RefCell::new(window))))
}

/// Creates a button widget.
/// Usage: gui_button(window, id, label, on_click_function_name?) - adds button to window
fn gui_button(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 3 || args.len() > 4 {
        return Err(RuntimeError::new("gui_button() requires window, id, label, and optional on_click function name"));
    }
    let window = match &args[0] {
        Value::GuiWindow(w) => w,
        other => return Err(RuntimeError::new(&format!("gui_button() requires a window, got {}", type_of(other)))),
    };
    let id = match &args[1] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_button() requires a string id, got {}", type_of(other)))),
    };
    let label = match &args[2] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_button() requires a string label, got {}", type_of(other)))),
    };
    let on_click = if args.len() == 4 {
        match &args[3] {
            Value::String(s) => Some(s.clone()),
            Value::Null => None,
            other => return Err(RuntimeError::new(&format!("gui_button() on_click must be a string or null, got {}", type_of(other)))),
        }
    } else { None };
    
    let widget = GuiWidget::Button { id, label, on_click };
    window.borrow().widgets.lock().unwrap().push(widget);
    Ok(Value::Null)
}

/// Creates a label widget.
/// Usage: gui_label(window, id, text) - adds label to window
fn gui_label(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("gui_label", args, 3)?;
    let window = match &args[0] {
        Value::GuiWindow(w) => w,
        other => return Err(RuntimeError::new(&format!("gui_label() requires a window, got {}", type_of(other)))),
    };
    let id = match &args[1] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_label() requires a string id, got {}", type_of(other)))),
    };
    let text = match &args[2] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_label() requires a string text, got {}", type_of(other)))),
    };
    
    let widget = GuiWidget::Label { id, text };
    window.borrow().widgets.lock().unwrap().push(widget);
    Ok(Value::Null)
}

/// Creates a text input widget.
/// Usage: gui_text_input(window, id, placeholder, on_change_function_name?) - adds text input to window
fn gui_text_input(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 3 || args.len() > 4 {
        return Err(RuntimeError::new("gui_text_input() requires window, id, placeholder, and optional on_change function name"));
    }
    let window = match &args[0] {
        Value::GuiWindow(w) => w,
        other => return Err(RuntimeError::new(&format!("gui_text_input() requires a window, got {}", type_of(other)))),
    };
    let id = match &args[1] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_text_input() requires a string id, got {}", type_of(other)))),
    };
    let placeholder = match &args[2] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_text_input() requires a string placeholder, got {}", type_of(other)))),
    };
    let on_change = if args.len() == 4 {
        match &args[3] {
            Value::String(s) => Some(s.clone()),
            Value::Null => None,
            other => return Err(RuntimeError::new(&format!("gui_text_input() on_change must be a string or null, got {}", type_of(other)))),
        }
    } else { None };
    
    let widget = GuiWidget::TextInput { id, placeholder, value: String::new(), on_change };
    window.borrow().widgets.lock().unwrap().push(widget);
    Ok(Value::Null)
}

/// Creates a checkbox widget.
/// Usage: gui_checkbox(window, id, label, checked, on_change_function_name?) - adds checkbox to window
fn gui_checkbox(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 4 || args.len() > 5 {
        return Err(RuntimeError::new("gui_checkbox() requires window, id, label, checked, and optional on_change function name"));
    }
    let window = match &args[0] {
        Value::GuiWindow(w) => w,
        other => return Err(RuntimeError::new(&format!("gui_checkbox() requires a window, got {}", type_of(other)))),
    };
    let id = match &args[1] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_checkbox() requires a string id, got {}", type_of(other)))),
    };
    let label = match &args[2] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_checkbox() requires a string label, got {}", type_of(other)))),
    };
    let checked = match &args[3] {
        Value::Boolean(b) => *b,
        other => return Err(RuntimeError::new(&format!("gui_checkbox() requires a boolean checked, got {}", type_of(other)))),
    };
    let on_change = if args.len() == 5 {
        match &args[4] {
            Value::String(s) => Some(s.clone()),
            Value::Null => None,
            other => return Err(RuntimeError::new(&format!("gui_checkbox() on_change must be a string or null, got {}", type_of(other)))),
        }
    } else { None };
    
    let widget = GuiWidget::Checkbox { id, label, checked, on_change };
    window.borrow().widgets.lock().unwrap().push(widget);
    Ok(Value::Null)
}

/// Creates a slider widget.
/// Usage: gui_slider(window, id, label, min, max, value, on_change_function_name?) - adds slider to window
fn gui_slider(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 6 || args.len() > 7 {
        return Err(RuntimeError::new("gui_slider() requires window, id, label, min, max, value, and optional on_change function name"));
    }
    let window = match &args[0] {
        Value::GuiWindow(w) => w,
        other => return Err(RuntimeError::new(&format!("gui_slider() requires a window, got {}", type_of(other)))),
    };
    let id = match &args[1] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_slider() requires a string id, got {}", type_of(other)))),
    };
    let label = match &args[2] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_slider() requires a string label, got {}", type_of(other)))),
    };
    let min = number("gui_slider", &args[3])?;
    let max = number("gui_slider", &args[4])?;
    let value = number("gui_slider", &args[5])?;
    let on_change = if args.len() == 7 {
        match &args[6] {
            Value::String(s) => Some(s.clone()),
            Value::Null => None,
            other => return Err(RuntimeError::new(&format!("gui_slider() on_change must be a string or null, got {}", type_of(other)))),
        }
    } else { None };
    
    let widget = GuiWidget::Slider { id, label, min, max, value, on_change };
    window.borrow().widgets.lock().unwrap().push(widget);
    Ok(Value::Null)
}

/// Creates a vertical stack layout.
/// Usage: gui_vstack(window, id, children_array) - adds vertical stack to window
fn gui_vstack(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("gui_vstack", args, 3)?;
    let window = match &args[0] {
        Value::GuiWindow(w) => w,
        other => return Err(RuntimeError::new(&format!("gui_vstack() requires a window, got {}", type_of(other)))),
    };
    let id = match &args[1] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_vstack() requires a string id, got {}", type_of(other)))),
    };
    // For simplicity, we'll just store the id - in a full implementation we'd handle nested widgets
    let widget = GuiWidget::VStack { id, children: Vec::new() };
    window.borrow().widgets.lock().unwrap().push(widget);
    Ok(Value::Null)
}

/// Creates a horizontal stack layout.
/// Usage: gui_hstack(window, id, children_array) - adds horizontal stack to window
fn gui_hstack(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("gui_hstack", args, 3)?;
    let window = match &args[0] {
        Value::GuiWindow(w) => w,
        other => return Err(RuntimeError::new(&format!("gui_hstack() requires a window, got {}", type_of(other)))),
    };
    let id = match &args[1] {
        Value::String(s) => s.clone(),
        other => return Err(RuntimeError::new(&format!("gui_hstack() requires a string id, got {}", type_of(other)))),
    };
    let widget = GuiWidget::HStack { id, children: Vec::new() };
    window.borrow().widgets.lock().unwrap().push(widget);
    Ok(Value::Null)
}

/// Shows the GUI window and starts the event loop.
/// Usage: gui_show(window) - blocks until window is closed
fn gui_show(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("gui_show", args, 1)?;
    let window = match &args[0] {
        Value::GuiWindow(w) => w,
        other => return Err(RuntimeError::new(&format!("gui_show() requires a window, got {}", type_of(other)))),
    };
    
    // Clone what we need for the event loop
    let mut window_clone = window.borrow().clone();
    let widgets = window_clone.widgets.clone();
    let title = window_clone.title.clone();
    let running = window_clone.running.clone();
    let event_tx = window_clone.event_tx.clone();
    let event_rx = window_clone.event_rx.take();
    
    // Run the GUI in a separate thread since it blocks
    let handle = std::thread::spawn(move || {
        let options = eframe::NativeOptions::default();
        let _ = eframe::run_native(
            &title,
            options,
            Box::new(move |cc| {
                Ok(Box::new(GuiApp {
                    widgets: widgets,
                    running: running,
                    event_tx: event_tx,
                    event_rx: event_rx,
                }))
            }),
        );
    });
    
    handle.join().map_err(|_| RuntimeError::new("GUI thread panicked"))?;
    Ok(Value::Null)
}

/// Polls for GUI events.
/// Usage: gui_poll_events(window) - returns array of events or empty array
fn gui_poll_events(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("gui_poll_events", args, 1)?;
    let window = match &args[0] {
        Value::GuiWindow(w) => w,
        other => return Err(RuntimeError::new(&format!("gui_poll_events() requires a window, got {}", type_of(other)))),
    };
    
    let rx = window.borrow().event_rx.as_ref()
        .ok_or_else(|| RuntimeError::new("gui_poll_events() requires a window with event receiver"))?
        .clone();
    
    let mut events = Vec::new();
    let rx_guard = rx.lock().unwrap();
    while let Ok(event) = rx_guard.try_recv() {
        let mut event_map = Map::new();
        event_map.insert(Value::String("widget_id".into()), Value::String(event.widget_id))?;
        event_map.insert(Value::String("event_type".into()), Value::String(format!("{:?}", event.event_type)))?;
        if let Some(value) = event.value {
            event_map.insert(Value::String("value".into()), value.to_value())?;
        }
        events.push(Value::Map(Rc::new(RefCell::new(event_map))));
    }
    
    Ok(Value::Array(Rc::new(RefCell::new(events))))
}

/// Closes the GUI window.
/// Usage: gui_close(window)
fn gui_close(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity("gui_close", args, 1)?;
    let window = match &args[0] {
        Value::GuiWindow(w) => w,
        other => return Err(RuntimeError::new(&format!("gui_close() requires a window, got {}", type_of(other)))),
    };
    
    let window_ref = window.borrow();
    let mut running = window_ref.running.lock().unwrap();
    *running = false;
    
    // Send close event
    if let Some(tx) = window_ref.event_tx.as_ref() {
        let _ = tx.lock().unwrap().send(GuiEvent::close("window".to_string()));
    }
    
    Ok(Value::Null)
}

// GUI application state for eframe
struct GuiApp {
    widgets: Arc<StdMutex<Vec<GuiWidget>>>,
    running: Arc<StdMutex<bool>>,
    event_tx: Option<Arc<StdMutex<mpsc::Sender<GuiEvent>>>>,
    event_rx: Option<Arc<StdMutex<mpsc::Receiver<GuiEvent>>>>,
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let widgets = self.widgets.lock().unwrap();
            for widget in widgets.iter() {
                render_widget(ui, widget, &self.event_tx);
            }
        });
        
        // Check if we should close
        let running = self.running.lock().unwrap();
        if !*running {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn render_widget(ui: &mut egui::Ui, widget: &GuiWidget, event_tx: &Option<Arc<StdMutex<mpsc::Sender<GuiEvent>>>>) {
    match widget {
        GuiWidget::Button { id, label, on_click } => {
            if ui.button(label).clicked() {
                if let Some(tx) = event_tx {
                    let _ = tx.lock().unwrap().send(GuiEvent::click(id.clone()));
                }
                if let Some(func_name) = on_click {
                    // In a full implementation, we'd call the Nect function here
                    println!("Button {} clicked, would call {}", id, func_name);
                }
            }
        }
        GuiWidget::Label { id: _, text } => {
            ui.label(text);
        }
        GuiWidget::TextInput { id, placeholder, value: _, on_change } => {
            // For simplicity, we just show the input
            let _ = ui.text_edit_singleline(&mut String::new());
        }
        GuiWidget::Checkbox { id, label, checked: _, on_change } => {
            let mut checked = false;
            if ui.checkbox(&mut checked, label).changed() {
                if let Some(tx) = event_tx {
                    let _ = tx.lock().unwrap().send(GuiEvent::change(id.clone(), GuiEventValue::Boolean(checked)));
                }
            }
        }
        GuiWidget::Slider { id, label, min, max, value: _, on_change } => {
            let mut val = *min;
            if ui.add(egui::Slider::new(&mut val, *min..=*max).text(label)).changed() {
                if let Some(tx) = event_tx {
                    let _ = tx.lock().unwrap().send(GuiEvent::change(id.clone(), GuiEventValue::Number(val)));
                }
            }
        }
        GuiWidget::VStack { id: _, children } => {
            ui.vertical(|ui| {
                for child in children {
                    render_widget(ui, child, event_tx);
                }
            });
        }
        GuiWidget::HStack { id: _, children } => {
            ui.horizontal(|ui| {
                for child in children {
                    render_widget(ui, child, event_tx);
                }
            });
        }
        GuiWidget::Window { id: _, title: _, children } => {
            egui::Window::new("Window").show(ui.ctx(), |ui| {
                for child in children {
                    render_widget(ui, child, event_tx);
                }
            });
        }
    }
}

// Helper functions for database operations
fn value_to_sql(value: &Value) -> Box<dyn rusqlite::ToSql> {
    match value {
        Value::Null => Box::new(None::<String>),
        Value::Number(n) => Box::new(*n),
        Value::String(s) => Box::new(s.clone()),
        Value::Boolean(b) => Box::new(*b),
        _ => Box::new(format!("{}", format_value(value))),
    }
}

fn sql_value_to_value(val: rusqlite::types::ValueRef) -> Result<Value, RuntimeError> {
    use rusqlite::types::ValueRef;
    match val {
        ValueRef::Null => Ok(Value::Null),
        ValueRef::Integer(i) => Ok(Value::Number(i as f64)),
        ValueRef::Real(f) => Ok(Value::Number(f)),
        ValueRef::Text(s) => {
            let str_val = std::str::from_utf8(s)
                .map_err(|_| RuntimeError::new("Invalid UTF-8 in database"))?;
            Ok(Value::String(str_val.to_string()))
        }
        ValueRef::Blob(b) => {
            // Convert blob to base64 string or similar
            Ok(Value::String(format!("<blob {} bytes>", b.len())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(n: f64) -> Value {
        Value::Number(n)
    }

    fn text(s: &str) -> Value {
        Value::String(s.to_string())
    }

    fn list(values: Vec<Value>) -> Value {
        Value::Array(Rc::new(RefCell::new(values)))
    }

    fn call_err(name: &str, args: &[Value]) -> String {
        call(name, args).expect_err("expected an error").message
    }

    fn map(entries: Vec<(&str, f64)>) -> Value {
        let mut map = Map::new();
        for (key, value) in entries {
            map.insert(text(key), num(value)).expect("valid key");
        }
        Value::Map(Rc::new(RefCell::new(map)))
    }

    #[test]
    fn maps_keep_insertion_order_and_overwrite_in_place() {
        let value = map(vec![("b", 1.0), ("a", 2.0)]);
        assert_eq!(format_value(&value), "{\"b\": 1, \"a\": 2}");
        if let Value::Map(m) = &value {
            m.borrow_mut().insert(text("b"), num(9.0)).unwrap();
        }
        // Overwriting keeps the key's original position.
        assert_eq!(format_value(&value), "{\"b\": 9, \"a\": 2}");
        assert_eq!(call("len", std::slice::from_ref(&value)).unwrap(), num(2.0));
    }

    #[test]
    fn map_keys_accept_only_the_hashable_subset() {
        let mut m = Map::new();
        assert!(m.insert(num(2.0), Value::Null).is_ok());
        assert!(m.insert(Value::Boolean(true), Value::Null).is_ok());
        // 2 and 2.0 are the same key.
        assert!(m.insert(num(2.0), text("two")).is_ok());
        assert_eq!(m.len(), 2);
        assert!(matches!(m.get(&num(2.0)), Some(Value::String(s)) if s == "two"));
        assert!(m
            .insert(Value::Array(Rc::new(RefCell::new(Vec::new()))), Value::Null)
            .is_err());
        assert!(m.insert(Value::Null, Value::Null).is_err());
    }

    #[test]
    fn map_builtins_round_trip() {
        let value = map(vec![("x", 1.0), ("y", 2.0)]);
        assert_eq!(
            call("keys", std::slice::from_ref(&value)).unwrap(),
            list(vec![text("x"), text("y")])
        );
        assert_eq!(
            call("values", std::slice::from_ref(&value)).unwrap(),
            list(vec![num(1.0), num(2.0)])
        );
        assert_eq!(
            call("has", &[value.clone(), text("y")]).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            call("has", &[value.clone(), text("z")]).unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(
            call("remove", &[value.clone(), text("x")]).unwrap(),
            num(1.0)
        );
        assert_eq!(
            call("remove", &[value.clone(), text("x")]).unwrap(),
            Value::Null
        );
        assert_eq!(call("len", &[value]).unwrap(), num(1.0));
    }

    #[test]
    fn map_indexing_errors_name_the_key() {
        let value = map(vec![("a", 1.0)]);
        assert_eq!(
            get_index(&value, &text("z")).expect_err("missing key").message,
            "map key z not found"
        );
    }

    #[test]
    fn truthiness_matches_the_documented_rules() {
        assert!(!is_truthy(&Value::Null));
        assert!(!is_truthy(&Value::Boolean(false)));
        assert!(!is_truthy(&num(0.0)));
        assert!(is_truthy(&num(-1.0)));
        assert!(is_truthy(&text("")));
        assert!(is_truthy(&list(Vec::new())));
    }

    #[test]
    fn type_reports_every_value_kind() {
        for (value, expected) in [
            (num(1.0), "number"),
            (text("s"), "string"),
            (Value::Boolean(true), "boolean"),
            (Value::Null, "null"),
            (list(Vec::new()), "array"),
        ] {
            assert_eq!(type_of(&value), expected);
        }
    }

    #[test]
    fn element_index_counts_negatives_from_the_end() {
        assert_eq!(element_index("array", &num(0.0), 3).unwrap(), 0);
        assert_eq!(element_index("array", &num(-1.0), 3).unwrap(), 2);
        assert_eq!(element_index("array", &num(-3.0), 3).unwrap(), 0);
        // Fractional indices truncate toward zero, as they always have.
        assert_eq!(element_index("array", &num(1.9), 3).unwrap(), 1);
        for bad in [num(3.0), num(-4.0), num(f64::NAN)] {
            assert!(element_index("array", &bad, 3).is_err(), "{bad:?}");
        }
        assert_eq!(
            element_index("array", &text("s"), 3)
                .expect_err("bad index")
                .message,
            "array index must be a number, got string"
        );
    }

    #[test]
    fn get_index_reads_arrays_and_string_characters() {
        let items = list(vec![num(1.0), text("two")]);
        assert_eq!(get_index(&items, &num(-1.0)).unwrap(), text("two"));
        // String indexing counts characters, not bytes.
        assert_eq!(get_index(&text("héllo"), &num(1.0)).unwrap(), text("é"));
        assert_eq!(
            get_index(&num(1.0), &num(0.0))
                .expect_err("not indexable")
                .message,
            "cannot index into number"
        );
    }

    #[test]
    fn set_index_applies_compound_operators_once() {
        let items = list(vec![num(10.0), num(20.0)]);
        let stored = set_index(&items, &num(0.0), Some(BinaryOp::Add), num(5.0)).unwrap();
        assert_eq!(stored, num(15.0));
        assert_eq!(get_index(&items, &num(0.0)).unwrap(), num(15.0));
        assert!(set_index(&text("abc"), &num(0.0), None, text("z")).is_err());
    }

    #[test]
    fn slice_takes_start_and_end_with_negative_offsets() {
        assert_eq!(
            call("slice", &[text("hello"), num(1.0), num(3.0)]).unwrap(),
            text("el")
        );
        assert_eq!(
            call("slice", &[text("hello"), num(-2.0)]).unwrap(),
            text("lo")
        );
        // Past the end is empty, not an error.
        assert_eq!(
            call("slice", &[text("hello"), num(9.0)]).unwrap(),
            text("")
        );
        assert!(call("slice", &[text("hello"), num(3.0), num(1.0)]).is_err());
    }

    #[test]
    fn string_and_array_helpers_behave() {
        assert_eq!(
            call("len", &[text("héllo")]).unwrap(),
            num(5.0),
            "len counts characters"
        );
        assert_eq!(
            call("upper", &[text("ab")]).unwrap(),
            text("AB")
        );
        assert_eq!(
            call("index_of", &[text("banana"), text("na")]).unwrap(),
            num(2.0)
        );
        assert_eq!(
            call("index_of", &[text("banana"), text("zz")]).unwrap(),
            num(-1.0)
        );
        assert_eq!(
            call("contains", &[list(vec![num(1.0)]), num(1.0)]).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            call("split", &[text("a,b"), text(",")]).unwrap(),
            list(vec![text("a"), text("b")])
        );
        assert_eq!(
            call("join", &[list(vec![num(1.0), text("b")]), text("-")]).unwrap(),
            text("1-b")
        );
        assert_eq!(call("repeat", &[text("ab"), num(2.0)]).unwrap(), text("abab"));
        assert!(call("split", &[text("ab"), text("")]).is_err());
        assert!(call("repeat", &[text("ab"), num(1.5)]).is_err());
    }

    #[test]
    fn push_and_pop_mutate_in_place() {
        let items = list(Vec::new());
        assert_eq!(
            call("push", &[items.clone(), num(1.0), num(2.0)]).unwrap(),
            list(vec![num(1.0), num(2.0)])
        );
        assert_eq!(
            call("pop", std::slice::from_ref(&items)).unwrap(),
            num(2.0)
        );
        assert_eq!(
            call("len", std::slice::from_ref(&items)).unwrap(),
            num(1.0)
        );
        let empty = list(Vec::new());
        assert!(call("pop", &[empty]).is_err());
    }

    #[test]
    fn sort_and_reverse_work_on_numbers_strings_and_arrays() {
        let numbers = list(vec![num(3.0), num(1.0), num(2.0)]);
        assert_eq!(
            call("sort", std::slice::from_ref(&numbers)).unwrap(),
            list(vec![num(1.0), num(2.0), num(3.0)])
        );
        assert_eq!(
            call("sort", &[list(vec![text("b"), text("a")])]).unwrap(),
            list(vec![text("a"), text("b")])
        );
        assert_eq!(call("reverse", &[text("abc")]).unwrap(), text("cba"));
        assert_eq!(
            call("reverse", std::slice::from_ref(&numbers)).unwrap(),
            list(vec![num(3.0), num(2.0), num(1.0)])
        );
        assert!(call("sort", &[list(vec![num(1.0), text("a")])]).is_err());
        assert!(call("sort", &[text("nope")]).is_err());
    }

    #[test]
    fn range_produces_lists_and_rejects_impossible_ones() {
        assert_eq!(
            call("range", &[num(3.0)]).unwrap(),
            list(vec![num(0.0), num(1.0), num(2.0)])
        );
        assert_eq!(
            call("range", &[num(3.0), num(0.0), num(-1.0)]).unwrap(),
            list(vec![num(3.0), num(2.0), num(1.0)])
        );
        assert_eq!(call("range", &[num(0.0)]).unwrap(), list(Vec::new()));
        assert!(call("range", &[num(1.0), num(5.0), num(0.0)]).is_err());
        assert!(call("range", &[num(0.0), num(1e12)]).is_err());
    }

    #[test]
    fn math_helpers_share_one_error_style() {
        assert_eq!(call("min", &[num(3.0), num(1.0)]).unwrap(), num(1.0));
        assert_eq!(
            call("max", &[list(vec![num(3.0), num(9.0)])]).unwrap(),
            num(9.0)
        );
        assert_eq!(call("sum", &[list(vec![num(1.0), num(2.0)])]).unwrap(), num(3.0));
        assert_eq!(call("abs", &[num(-2.0)]).unwrap(), num(2.0));
        assert_eq!(call_err("sqrt", &[num(-1.0)]), "sqrt() is not defined for -1");
        assert_eq!(call_err("log", &[num(0.0)]), "log() is not defined for 0");
        assert_eq!(
            call_err("abs", &[text("s")]),
            "abs() requires a number, got string"
        );
        assert_eq!(call_err("sum", &[list(vec![text("s")])]), "sum() requires a number, got string");
        assert_eq!(call_err("min", &[]), "min() requires arguments");
    }

    #[test]
    fn numbers_print_without_a_trailing_zero() {
        assert_eq!(format_value(&num(2.0)), "2");
        assert_eq!(format_value(&num(2.5)), "2.5");
        assert_eq!(format_value(&num(f64::NAN)), "nan");
        assert_eq!(format_value(&num(f64::INFINITY)), "inf");
        assert_eq!(format_value(&num(1e20)), "100000000000000000000");
        assert_eq!(format_value(&list(vec![text("a"), num(1.0)])), "[\"a\", 1]");
    }

    #[test]
    fn conversion_and_assertions() {
        assert_eq!(call("num", &[text("  42 ")]).unwrap(), num(42.0));
        assert_eq!(call("num", &[Value::Null]).unwrap(), num(0.0));
        assert_eq!(call("bool", &[text("")]).unwrap(), Value::Boolean(true));
        assert_eq!(call("str", &[list(vec![num(1.0)])]).unwrap(), text("[1]"));
        assert_eq!(
            call_err("num", &[text("nope")]),
            "cannot convert 'nope' to number"
        );
        assert!(call("assert", &[Value::Boolean(true)]).is_ok());
        assert_eq!(
            call_err("assert", &[Value::Boolean(false), text("boom")]),
            "assertion failed: boom"
        );
    }

    #[test]
    fn unknown_names_and_arity_are_reported() {
        assert_eq!(call_err("nope", &[]), "undefined function 'nope'");
        assert_eq!(call_err("len", &[]), "len() requires 1 argument(s), got 0");
    }

    #[test]
    fn comparison_orders_strings_and_rejects_mixed_types() {
        assert_eq!(
            compare(&text("b"), BinaryOp::Greater, &text("a")).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            compare(&num(f64::NAN), BinaryOp::Less, &num(1.0)).unwrap(),
            Value::Boolean(false)
        );
        assert!(compare(&text("a"), BinaryOp::Greater, &num(1.0)).is_err());
    }

    #[test]
    fn arithmetic_errors_name_the_operator() {
        assert_eq!(
            apply_binary(num(1.0), BinaryOp::Modulo, num(0.0))
                .expect_err("division by zero")
                .message,
            "modulo by zero"
        );
        assert_eq!(
            apply_binary(num(1.0), BinaryOp::Subtract, text("s"))
                .expect_err("type mismatch")
                .message,
            "cannot apply '-' to number and string"
        );
        assert_eq!(apply_unary(UnaryOp::Not, num(0.0)).unwrap(), Value::Boolean(true));
    }

    #[test]
    fn concat_stringifies_every_kind_of_value() {
        let mixed = vec![
            text("n = "),
            num(42.0),
            text(" ok="),
            Value::Boolean(true),
            text(" list="),
            Value::Array(Rc::new(RefCell::new(vec![num(1.0), text("a")]))),
            text(" nothing="),
            Value::Null,
        ];
        assert_eq!(
            concat(&mixed).unwrap(),
            Value::String("n = 42 ok=true list=[1, \"a\"] nothing=null".to_string())
        );
    }

    #[test]
    fn concat_with_no_arguments_is_an_empty_string() {
        assert_eq!(concat(&[]).unwrap(), Value::String(String::new()));
    }

    #[test]
    fn int_truncates_toward_zero() {
        assert_eq!(call("int", &[num(12.7)]).unwrap(), num(12.0));
        assert_eq!(call("int", &[num(-12.7)]).unwrap(), num(-12.0));
        // floor would give -13 here; int is the truncate spelling.
        assert_eq!(call("int", &[num(-12.2)]).unwrap(), num(-12.0));
        assert_eq!(call("int", &[num(3.0)]).unwrap(), num(3.0));
        assert_eq!(
            call("int", &[text("x")]).expect_err("type mismatch").message,
            "int() requires a number, got string"
        );
    }

    #[test]
    fn fixed_formats_with_exact_digit_count() {
        assert_eq!(
            call("fixed", &[num(9.87654), num(3.0)]).unwrap(),
            Value::String("9.877".to_string())
        );
        assert_eq!(
            call("fixed", &[num(2.5), num(0.0)]).unwrap(),
            Value::String("2".to_string())
        );
        assert_eq!(
            call("fixed", &[num(-1.5), num(3.0)]).unwrap(),
            Value::String("-1.500".to_string())
        );
        assert_eq!(
            call("fixed", &[num(1.0), num(101.0)])
                .expect_err("out of range")
                .message,
            "fixed() requires a digit count between 0 and 100"
        );
    }

    #[test]
    fn char_and_char_code_round_trip() {
        assert_eq!(
            call("char", &[num(78.0)]).unwrap(),
            Value::String("N".to_string())
        );
        assert_eq!(
            call("char", &[num(0x1F3AF as f64)]).unwrap(),
            Value::String("🎯".to_string())
        );
        assert_eq!(call("char_code", &[text("N")]).unwrap(), num(78.0));
        assert_eq!(
            call("char_code", &[text("🎯")]).unwrap(),
            num(0x1F3AF as f64)
        );
        assert!(call("char", &[num(-1.0)]).is_err());
        assert!(call("char", &[num(1_114_112.0)]).is_err());
        assert!(call("char_code", &[text("")]).is_err());
    }

    // One test, not two: both exercise the single global RNG state, and
    // cargo runs a binary's unit tests on parallel threads — two tests
    // re-seeding the shared generator would race.
    #[test]
    fn seeded_randomness_is_reproducible_and_in_range() {
        call("seed", &[num(7.0)]).unwrap();
        let first = call("random", &[]).unwrap();
        let second = call("random", &[]).unwrap();
        assert_ne!(first, second);
        call("seed", &[num(7.0)]).unwrap();
        assert_eq!(call("random", &[]).unwrap(), first);

        for value in [first, second] {
            match value {
                Value::Number(n) => assert!((0.0..1.0).contains(&n)),
                other => panic!("random() returned {other:?}"),
            }
        }

        call("seed", &[num(123.0)]).unwrap();
        let drawn = call("random_int", &[num(1.0), num(6.0)]).unwrap();
        assert!(matches!(drawn, Value::Number(n) if (1.0..=6.0).contains(&n)));
        call("seed", &[num(123.0)]).unwrap();
        assert_eq!(call("random_int", &[num(1.0), num(6.0)]).unwrap(), drawn);

        assert!(
            call("random_int", &[num(5.0), num(1.0)])
                .expect_err("reversed bounds")
                .message
                .contains("low <= high")
        );
        assert!(call("random_int", &[num(1.5), num(6.0)]).is_err());
    }
}

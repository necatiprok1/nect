//! The shared core: value semantics and the built-in function library.
//!
//! Both engines call into this module, so a program behaves identically on the
//! bytecode VM and on the reference interpreter — `tests/differential_tests.rs`
//! compares their stdout, stderr and exit status character for character. The
//! names in [`NAMES`] are the complete set of builtins; anything else is a user
//! function (or an error).

use crate::ast::{BinaryOp, UnaryOp, Value};
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

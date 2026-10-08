//! Memory regression tests for Phase 10.2 optimization passes.
//!
//! These tests verify that allocation-heavy runtime operations produce correct
//! results and don't introduce regressions. They focus on the hot paths that
//! were optimized to avoid unnecessary heap allocations:
//!
//! - `get_index` for strings (was: `Vec<char>` allocation per index)
//! - `print` with multiple arguments (was: `Vec<String>` + `join`)
//! - `concat` (was: `Vec<String>` + `concat`)
//! - `format_value` for arrays and maps (was: `Vec<String>` + `join`)
//! - `join` builtin (was: `Vec<String>` + `join`)

use nect::ast::Value;
use nect::builtins::{element_index, format_value, get_index};

#[test]
fn string_index_returns_single_character() {
    let s = Value::String("hello".to_string());
    let idx = Value::Number(0.0);
    let result = get_index(&s, &idx).unwrap();
    assert_eq!(result, Value::String("h".to_string()));
}

#[test]
fn string_index_negative_counts_from_end() {
    let s = Value::String("hello".to_string());
    let idx = Value::Number(-1.0);
    let result = get_index(&s, &idx).unwrap();
    assert_eq!(result, Value::String("o".to_string()));
}

#[test]
fn string_index_out_of_bounds_errors() {
    let s = Value::String("hi".to_string());
    let idx = Value::Number(5.0);
    let result = get_index(&s, &idx);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("out of bounds"));
    assert!(err.contains("length 2"));
}

#[test]
fn string_index_with_multibyte_chars() {
    // Nect strings are character-indexed, not byte-indexed.
    let s = Value::String("héllo".to_string()); // 5 characters, 6 bytes
    let r = Value::Number(1.0);
    let result = get_index(&s, &r).unwrap();
    assert_eq!(result, Value::String("é".to_string()));

    let o = Value::Number(4.0);
    let result = get_index(&s, &o).unwrap();
    assert_eq!(result, Value::String("o".to_string()));
}

#[test]
fn string_index_on_empty_string_errors() {
    let s = Value::String(String::new());
    let idx = Value::Number(0.0);
    let result = get_index(&s, &idx);
    assert!(result.is_err());
}

#[test]
fn array_index_works_correctly() {
    let arr = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        Value::Number(10.0),
        Value::Number(20.0),
        Value::Number(30.0),
    ])));
    let idx = Value::Number(1.0);
    let result = get_index(&arr, &idx).unwrap();
    assert_eq!(result, Value::Number(20.0));
}

#[test]
fn array_index_negative_works() {
    let arr = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        Value::Number(10.0),
        Value::Number(20.0),
        Value::Number(30.0),
    ])));
    let idx = Value::Number(-1.0);
    let result = get_index(&arr, &idx).unwrap();
    assert_eq!(result, Value::Number(30.0));
}

#[test]
fn format_value_number_whole_prints_without_decimal() {
    assert_eq!(format_value(&Value::Number(42.0)), "42");
}

#[test]
fn format_value_number_fractional() {
    assert_eq!(
        format_value(&Value::Number(std::f64::consts::PI)),
        format!("{}", std::f64::consts::PI)
    );
}

#[test]
fn format_value_null() {
    assert_eq!(format_value(&Value::Null), "null");
}

#[test]
fn format_value_boolean() {
    assert_eq!(format_value(&Value::Boolean(true)), "true");
    assert_eq!(format_value(&Value::Boolean(false)), "false");
}

#[test]
fn format_value_string_is_bare() {
    assert_eq!(format_value(&Value::String("hello".to_string())), "hello");
}

#[test]
fn format_value_array_renders_correctly() {
    let arr = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        Value::Number(1.0),
        Value::Number(2.0),
        Value::Number(3.0),
    ])));
    assert_eq!(format_value(&arr), "[1, 2, 3]");
}

#[test]
fn format_value_array_with_strings_quotes_them() {
    let arr = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        Value::Number(1.0),
        Value::String("two".to_string()),
    ])));
    assert_eq!(format_value(&arr), r#"[1, "two"]"#);
}

#[test]
fn format_value_empty_array() {
    let arr = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![])));
    assert_eq!(format_value(&arr), "[]");
}

#[test]
fn format_value_empty_map() {
    let map = Value::Map(std::rc::Rc::new(std::cell::RefCell::new(
        nect::builtins::Map::new(),
    )));
    assert_eq!(format_value(&map), "{}");
}

#[test]
fn format_value_map_renders_correctly() {
    let mut map = nect::builtins::Map::new();
    map.insert(Value::String("a".to_string()), Value::Number(1.0))
        .unwrap();
    map.insert(
        Value::String("b".to_string()),
        Value::String("hello".to_string()),
    )
    .unwrap();
    let m = Value::Map(std::rc::Rc::new(std::cell::RefCell::new(map)));
    assert_eq!(format_value(&m), r#"{"a": 1, "b": "hello"}"#);
}

#[test]
fn format_value_map_with_numeric_keys() {
    let mut map = nect::builtins::Map::new();
    map.insert(Value::Number(1.0), Value::Number(10.0)).unwrap();
    let m = Value::Map(std::rc::Rc::new(std::cell::RefCell::new(map)));
    assert_eq!(format_value(&m), "{1: 10}");
}

#[test]
fn format_value_nested_structures() {
    let inner = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        Value::Number(1.0),
        Value::Number(2.0),
    ])));
    let arr = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        inner,
        Value::String("x".to_string()),
    ])));
    assert_eq!(format_value(&arr), r#"[[1, 2], "x"]"#);
}

#[test]
fn element_index_validates_out_of_bounds() {
    assert!(element_index("array", &Value::Number(5.0), 3).is_err());
    assert!(element_index("array", &Value::Number(-4.0), 3).is_err());
}

#[test]
fn element_index_rejects_non_numeric_index() {
    assert!(element_index("array", &Value::String("x".to_string()), 3).is_err());
    assert!(element_index("array", &Value::Boolean(true), 3).is_err());
}

#[test]
fn element_index_handles_negative_indices() {
    assert_eq!(element_index("array", &Value::Number(-1.0), 3).unwrap(), 2);
    assert_eq!(element_index("array", &Value::Number(-3.0), 3).unwrap(), 0);
}

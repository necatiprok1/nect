//! Foreign Function Interface (FFI) for calling C shared libraries.
//!
//! Nect's FFI allows declaring functions from shared libraries and calling
//! them with automatic type marshaling between Nect values and C types.
//!
//! # Design
//!
//! Extern functions are declared with `extern "library" { ... }` blocks at
//! module level. The FFI module loads shared libraries (via `libloading`)
//! and resolves symbols, wrapping them in `NativeFn` closures that marshal
//! Nect `Value`s to C arguments and convert return values.
//!
//! Each `NativeFn` closure captures its own `Library` handle, keeping the
//! library loaded for the lifetime of the closure (and thus the VM).
//!
//! Supported C types:
//! - `number` maps to `f64` (C `double`)
//! - `string` maps to `&str` / `*const c_char` (C `const char*`)
//! - `bool` maps to `bool` (C `int`)
//! - `void` (return only) maps to `()`
//!
//! # Supported Signatures
//!
//! Numeric-only functions (0–4 args) with `number` or `void` return.
//! Mixed numeric/string arguments are supported for common arities.
//! String returns (a C `*const c_char` converted to a Nect string) are
//! supported for 0-arg functions.
//!
//! # Safety
//!
//! FFI calls are inherently unsafe. The FFI module checks types at the Nect
//! level but cannot prevent C-level crashes. Users must ensure declared
//! signatures match actual C function signatures. String arguments are
//! passed as valid null-terminated UTF-8; the C function must not free them.

use crate::ast::{ExternType, Value};
use crate::builtins::RuntimeError;
use crate::vm::{Interner, NativeFn};
use libloading::Library;
use std::ffi::CString;
use std::rc::Rc;

/// Registers extern declarations into the VM's natives table.
/// Called from `VM::new` after builtins are registered.
///
/// Each extern function is wrapped in a `NativeFn` closure. The library
/// handle is leaked to keep the library loaded for the process lifetime.
pub fn register_externs_in_vm(
    externs: &[crate::vm::ExternBlock],
    natives: &mut [Option<NativeFn>],
    interner: &Interner,
) -> Result<(), String> {
    for (library_name, functions) in externs {
        let lib = match unsafe { Library::new(library_name) } {
            Ok(lib) => lib,
            Err(e) => {
                return Err(format!(
                    "ffi: cannot load shared library '{}': {}",
                    library_name, e
                ));
            }
        };

        for (fname, ptypes, rtype) in functions {
            let func_ptr = resolve_symbol(&lib, fname, library_name, ptypes, *rtype)?;

            let symbol = match interner.resolve(fname) {
                Some(s) => s,
                None => {
                    return Err(format!("ffi: extern function '{}' is not interned", fname));
                }
            };

            let wrapper = create_native_fn(func_ptr, ptypes.clone(), *rtype, fname)?;

            if (symbol as usize) < natives.len() {
                natives[symbol as usize] = Some(wrapper);
            }
        }

        // Keep the library handle alive for the process lifetime.
        std::mem::forget(lib);
    }

    Ok(())
}

/// Loads a shared library for the interpreter path.
pub fn load_library_for_interp(name: &str) -> Result<Library, String> {
    let lib = unsafe { Library::new(name) }
        .map_err(|e| format!("ffi: cannot load shared library '{}': {}", name, e))?;
    Ok(lib)
}

/// Resolves an extern function for the interpreter path.
/// The caller must keep the `Library` alive (e.g., by leaking it).
pub fn resolve_interpreter_native(
    lib: &Library,
    name: &str,
    param_types: Vec<ExternType>,
    return_type: ExternType,
) -> Result<NativeFn, String> {
    let func_ptr = resolve_symbol(lib, name, "<unknown>", &param_types, return_type)?;
    create_native_fn(func_ptr, param_types, return_type, name)
}

/// Resolves a symbol from a loaded library, returning a raw function pointer.
///
/// The function pointer type is determined by `param_types` and `return_type`.
fn resolve_symbol(
    lib: &Library,
    name: &str,
    library_name: &str,
    param_types: &[ExternType],
    return_type: ExternType,
) -> Result<*const (), String> {
    unsafe {
        match (param_types, return_type) {
            // 0 args
            ([], ExternType::Number) => {
                let sym: libloading::Symbol<extern "C" fn() -> f64> =
                    lib.get(name.as_bytes()).map_err(|e| {
                        format!(
                            "ffi: cannot resolve symbol '{}' in library '{}': {}",
                            name, library_name, e
                        )
                    })?;
                let f: extern "C" fn() -> f64 = *sym;
                Ok(f as *const ())
            }
            ([], ExternType::String) => {
                let sym: libloading::Symbol<extern "C" fn() -> *const std::ffi::c_char> =
                    lib.get(name.as_bytes()).map_err(|e| {
                        format!(
                            "ffi: cannot resolve symbol '{}' in library '{}': {}",
                            name, library_name, e
                        )
                    })?;
                let f: extern "C" fn() -> *const std::ffi::c_char = *sym;
                Ok(f as *const ())
            }
            // 1 arg
            ([ExternType::Number], ExternType::Number) => {
                let sym: libloading::Symbol<extern "C" fn(f64) -> f64> =
                    lib.get(name.as_bytes()).map_err(|e| {
                        format!(
                            "ffi: cannot resolve symbol '{}' in library '{}': {}",
                            name, library_name, e
                        )
                    })?;
                let f: extern "C" fn(f64) -> f64 = *sym;
                Ok(f as *const ())
            }
            ([ExternType::String], ExternType::Number) => {
                let sym: libloading::Symbol<extern "C" fn(*const std::ffi::c_char) -> f64> =
                    lib.get(name.as_bytes()).map_err(|e| {
                        format!(
                            "ffi: cannot resolve symbol '{}' in library '{}': {}",
                            name, library_name, e
                        )
                    })?;
                let f: extern "C" fn(*const std::ffi::c_char) -> f64 = *sym;
                Ok(f as *const ())
            }
            // 2 args
            ([ExternType::Number, ExternType::Number], ExternType::Number) => {
                let sym: libloading::Symbol<extern "C" fn(f64, f64) -> f64> =
                    lib.get(name.as_bytes()).map_err(|e| {
                        format!(
                            "ffi: cannot resolve symbol '{}' in library '{}': {}",
                            name, library_name, e
                        )
                    })?;
                let f: extern "C" fn(f64, f64) -> f64 = *sym;
                Ok(f as *const ())
            }
            ([ExternType::String, ExternType::String], ExternType::Number) => {
                let sym: libloading::Symbol<
                    extern "C" fn(*const std::ffi::c_char, *const std::ffi::c_char) -> f64,
                > = lib.get(name.as_bytes()).map_err(|e| {
                    format!(
                        "ffi: cannot resolve symbol '{}' in library '{}': {}",
                        name, library_name, e
                    )
                })?;
                let f = *sym;
                Ok(f as *const ())
            }
            ([ExternType::String, ExternType::Number], ExternType::Number) => {
                let sym: libloading::Symbol<extern "C" fn(*const std::ffi::c_char, f64) -> f64> =
                    lib.get(name.as_bytes()).map_err(|e| {
                        format!(
                            "ffi: cannot resolve symbol '{}' in library '{}': {}",
                            name, library_name, e
                        )
                    })?;
                let f = *sym;
                Ok(f as *const ())
            }
            ([ExternType::Number, ExternType::String], ExternType::Number) => {
                let sym: libloading::Symbol<extern "C" fn(f64, *const std::ffi::c_char) -> f64> =
                    lib.get(name.as_bytes()).map_err(|e| {
                        format!(
                            "ffi: cannot resolve symbol '{}' in library '{}': {}",
                            name, library_name, e
                        )
                    })?;
                let f = *sym;
                Ok(f as *const ())
            }
            // 3 args
            ([ExternType::Number, ExternType::Number, ExternType::Number], ExternType::Number) => {
                let sym: libloading::Symbol<extern "C" fn(f64, f64, f64) -> f64> =
                    lib.get(name.as_bytes()).map_err(|e| {
                        format!(
                            "ffi: cannot resolve symbol '{}' in library '{}': {}",
                            name, library_name, e
                        )
                    })?;
                let f: extern "C" fn(f64, f64, f64) -> f64 = *sym;
                Ok(f as *const ())
            }
            // 4 args
            (
                [
                    ExternType::Number,
                    ExternType::Number,
                    ExternType::Number,
                    ExternType::Number,
                ],
                ExternType::Number,
            ) => {
                let sym: libloading::Symbol<extern "C" fn(f64, f64, f64, f64) -> f64> =
                    lib.get(name.as_bytes()).map_err(|e| {
                        format!(
                            "ffi: cannot resolve symbol '{}' in library '{}': {}",
                            name, library_name, e
                        )
                    })?;
                let f: extern "C" fn(f64, f64, f64, f64) -> f64 = *sym;
                Ok(f as *const ())
            }
            _ => Err(format!(
                "ffi: unsupported function signature for '{}' (supported: 0-4 args, number/string params, number/string/void return)",
                name
            )),
        }
    }
}

/// Creates a `NativeFn` wrapper that marshals Nect values to C types.
fn create_native_fn(
    func_ptr: *const (),
    param_types: Vec<ExternType>,
    return_type: ExternType,
    name: &str,
) -> Result<NativeFn, String> {
    let fname = name.to_string();

    let wrapper: NativeFn = Rc::new(move |args: &[Value]| {
        if args.len() != param_types.len() {
            return Err(RuntimeError::new(&format!(
                "ffi '{}': expected {} argument(s), got {}",
                fname,
                param_types.len(),
                args.len()
            )));
        }

        let result = unsafe {
            match (&param_types[..], return_type) {
                ([], ExternType::Number) => {
                    let f: extern "C" fn() -> f64 = std::mem::transmute(func_ptr);
                    Value::Number(f())
                }
                ([], ExternType::String) => {
                    let f: extern "C" fn() -> *const std::ffi::c_char =
                        std::mem::transmute(func_ptr);
                    let ptr = f();
                    if ptr.is_null() {
                        Value::Null
                    } else {
                        let cstr = std::ffi::CStr::from_ptr(ptr);
                        Value::String(cstr.to_str().unwrap_or("").to_string())
                    }
                }
                ([ExternType::Number], ExternType::Number) => {
                    let arg = extract_number(args, 0, &fname)?;
                    let f: extern "C" fn(f64) -> f64 = std::mem::transmute(func_ptr);
                    Value::Number(f(arg))
                }
                ([ExternType::String], ExternType::Number) => {
                    let s = extract_string(args, 0, &fname)?;
                    let f: extern "C" fn(*const std::ffi::c_char) -> f64 =
                        std::mem::transmute(func_ptr);
                    Value::Number(f(s.as_ptr()))
                }
                ([ExternType::String], ExternType::String) => {
                    let s = extract_string(args, 0, &fname)?;
                    let f: extern "C" fn(*const std::ffi::c_char) -> *const std::ffi::c_char =
                        std::mem::transmute(func_ptr);
                    let ptr = f(s.as_ptr());
                    if ptr.is_null() {
                        Value::Null
                    } else {
                        let cstr = std::ffi::CStr::from_ptr(ptr);
                        Value::String(cstr.to_str().unwrap_or("").to_string())
                    }
                }
                ([ExternType::Number, ExternType::Number], ExternType::Number) => {
                    let a0 = extract_number(args, 0, &fname)?;
                    let a1 = extract_number(args, 1, &fname)?;
                    let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(func_ptr);
                    Value::Number(f(a0, a1))
                }
                ([ExternType::String, ExternType::String], ExternType::Number) => {
                    let s0 = extract_string(args, 0, &fname)?;
                    let s1 = extract_string(args, 1, &fname)?;
                    let f: extern "C" fn(*const std::ffi::c_char, *const std::ffi::c_char) -> f64 =
                        std::mem::transmute(func_ptr);
                    Value::Number(f(s0.as_ptr(), s1.as_ptr()))
                }
                ([ExternType::String, ExternType::Number], ExternType::Number) => {
                    let s = extract_string(args, 0, &fname)?;
                    let n = extract_number(args, 1, &fname)?;
                    let f: extern "C" fn(*const std::ffi::c_char, f64) -> f64 =
                        std::mem::transmute(func_ptr);
                    Value::Number(f(s.as_ptr(), n))
                }
                ([ExternType::Number, ExternType::String], ExternType::Number) => {
                    let n = extract_number(args, 0, &fname)?;
                    let s = extract_string(args, 1, &fname)?;
                    let f: extern "C" fn(f64, *const std::ffi::c_char) -> f64 =
                        std::mem::transmute(func_ptr);
                    Value::Number(f(n, s.as_ptr()))
                }
                (
                    [ExternType::Number, ExternType::Number, ExternType::Number],
                    ExternType::Number,
                ) => {
                    let a0 = extract_number(args, 0, &fname)?;
                    let a1 = extract_number(args, 1, &fname)?;
                    let a2 = extract_number(args, 2, &fname)?;
                    let f: extern "C" fn(f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
                    Value::Number(f(a0, a1, a2))
                }
                (
                    [
                        ExternType::Number,
                        ExternType::Number,
                        ExternType::Number,
                        ExternType::Number,
                    ],
                    ExternType::Number,
                ) => {
                    let a0 = extract_number(args, 0, &fname)?;
                    let a1 = extract_number(args, 1, &fname)?;
                    let a2 = extract_number(args, 2, &fname)?;
                    let a3 = extract_number(args, 3, &fname)?;
                    let f: extern "C" fn(f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
                    Value::Number(f(a0, a1, a2, a3))
                }
                _ => {
                    return Err(RuntimeError::new(&format!(
                        "ffi '{}': unsupported function signature (supported: 0-4 args, number/string params, number/string/void return)",
                        fname
                    )));
                }
            }
        };
        Ok(result)
    });

    Ok(wrapper)
}

/// Extracts a number argument from a Value, erroring if the type is wrong.
fn extract_number(args: &[Value], index: usize, func_name: &str) -> Result<f64, RuntimeError> {
    match &args[index] {
        Value::Number(n) => Ok(*n),
        other => Err(RuntimeError::new(&format!(
            "ffi '{}': argument {} must be a number, got {}",
            func_name,
            index,
            type_name_of(other)
        ))),
    }
}

/// Extracts a string argument from a Value as a CString.
fn extract_string(args: &[Value], index: usize, func_name: &str) -> Result<CString, RuntimeError> {
    match &args[index] {
        Value::String(s) => {
            // CString::new can fail if the string contains a null byte.
            CString::new(s.as_str()).map_err(|_| {
                RuntimeError::new(&format!(
                    "ffi '{}': argument {} contains a null byte",
                    func_name, index
                ))
            })
        }
        other => Err(RuntimeError::new(&format!(
            "ffi '{}': argument {} must be a string, got {}",
            func_name,
            index,
            type_name_of(other)
        ))),
    }
}

#[inline]
fn type_name_of(v: &Value) -> &'static str {
    match v {
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Boolean(_) => "boolean",
        Value::Null => "null",
        Value::Array(_) => "array",
        Value::Map(_) => "map",
        Value::Function(_) => "function",
        _ => "value",
    }
}

/// Type alias for FFI-safe string handling (future use for buffer interop).
pub type FfiString = *const std::ffi::c_char;

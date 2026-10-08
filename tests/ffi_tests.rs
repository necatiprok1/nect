#![cfg(feature = "ffi")]
//! Integration tests for the FFI (foreign function interface).
//!
//! These tests compile a small C shared library at test time and verify
//! that Nect programs can call C functions through `extern` declarations.
//! The tests run the actual nect binary to ensure full end-to-end behavior.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Compiles the test C library if it doesn't exist.
/// Returns the full path to the compiled library.
fn ensure_test_lib() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lib_name = if cfg!(target_os = "macos") {
        "libffi_test.dylib"
    } else if cfg!(target_os = "linux") {
        "libffi_test.so"
    } else {
        "ffi_test.dll"
    };
    let lib_path = manifest_dir.join("tests").join(lib_name);

    if !lib_path.exists() {
        // The flags are the same on every platform; only the file extension
        // differs, and that is decided by `lib_name` above.
        let src = manifest_dir.join("tests").join("ffi_test_lib.c");
        let status = Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&lib_path)
            .arg(&src)
            .status()
            .expect("failed to compile test C library");
        assert!(status.success(), "failed to compile test C library");
    }
    lib_path
}

/// Returns the library path as a string for use in `extern` declarations.
fn lib_path_str() -> String {
    ensure_test_lib().to_str().unwrap().to_string()
}

/// Runs `nect run -` with the given source on stdin.
/// The `__LIB__` placeholder in the source is replaced with the library path.
fn run_with_ffi(source: &str) -> (String, String, bool) {
    let lib_path = lib_path_str();
    let source = source.replace("__LIB__", &lib_path);

    let mut command = Command::new(env!("CARGO_BIN_EXE_nect"));
    command.args(["run", "-"]);

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");

    {
        let stdin = child.stdin.as_mut().expect("failed to open stdin");
        stdin
            .write_all(source.as_bytes())
            .expect("failed to write stdin");
    }

    let output = child.wait_with_output().expect("failed to wait");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Runs `nect run --interp -` with the given source on stdin.
fn run_interp_with_ffi(source: &str) -> (String, String, bool) {
    let lib_path = lib_path_str();
    let source = source.replace("__LIB__", &lib_path);

    let mut command = Command::new(env!("CARGO_BIN_EXE_nect"));
    command.args(["run", "--interp", "-"]);

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");

    {
        let stdin = child.stdin.as_mut().expect("failed to open stdin");
        stdin
            .write_all(source.as_bytes())
            .expect("failed to write stdin");
    }

    let output = child.wait_with_output().expect("failed to wait");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

#[test]
fn ffi_calls_c_add_two_args() {
    let source = r#"
extern "__LIB__" {
    fn ffi_add(number, number) -> number;
}
print(ffi_add(3.0, 4.0))
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi_add failed: {}", stderr);
    assert_eq!(stdout.trim(), "7");
}

#[test]
fn ffi_calls_c_multiply_two_args() {
    let source = r#"
extern "__LIB__" {
    fn ffi_multiply(number, number) -> number;
}
print(ffi_multiply(5.0, 6.0))
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi_multiply failed: {}", stderr);
    assert_eq!(stdout.trim(), "30");
}

#[test]
fn ffi_calls_c_sqrt_one_arg() {
    let source = r#"
extern "__LIB__" {
    fn ffi_sqrt_val(number) -> number;
}
print(ffi_sqrt_val(16.0))
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi_sqrt failed: {}", stderr);
    assert_eq!(stdout.trim(), "4");
}

#[test]
fn ffi_calls_c_pow_two_args() {
    let source = r#"
extern "__LIB__" {
    fn ffi_pow_val(number, number) -> number;
}
print(ffi_pow_val(2.0, 10.0))
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi_pow failed: {}", stderr);
    assert_eq!(stdout.trim(), "1024");
}

#[test]
fn ffi_calls_zero_arg_function() {
    let source = r#"
extern "__LIB__" {
    fn ffi_pi() -> number;
}
print(ffi_pi())
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi_pi failed: {}", stderr);
    assert_eq!(stdout.trim(), "3.141592653589793");
}

#[test]
fn ffi_calls_four_arg_function() {
    let source = r#"
extern "__LIB__" {
    fn ffi_max_val(number, number, number, number) -> number;
}
print(ffi_max_val(1.0, 5.0, 3.0, 2.0))
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi_max_val failed: {}", stderr);
    assert_eq!(stdout.trim(), "5");
}

#[test]
fn ffi_wrong_argument_count_errors() {
    let source = r#"
extern "__LIB__" {
    fn ffi_add(number, number) -> number;
}
print(ffi_add(1.0))
"#;
    let (_stdout, stderr, success) = run_with_ffi(source);
    assert!(!success, "should fail on wrong argument count");
    assert!(
        stderr.contains("expected 2 argument(s), got 1"),
        "unexpected stderr: {}",
        stderr
    );
}

#[test]
fn ffi_wrong_argument_type_errors() {
    let source = r#"
extern "__LIB__" {
    fn ffi_add(number, number) -> number;
}
print(ffi_add(1.0, "hello"))
"#;
    let (_stdout, stderr, success) = run_with_ffi(source);
    assert!(!success, "should fail on wrong argument type");
    assert!(
        stderr.contains("must be a number"),
        "unexpected stderr: {}",
        stderr
    );
}

#[test]
fn ffi_string_argument() {
    let source = r#"
extern "__LIB__" {
    fn ffi_string_length(string) -> number;
}
print(ffi_string_length("hello"))
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi_string_length failed: {}", stderr);
    assert_eq!(stdout.trim(), "5");
}

#[test]
fn ffi_string_return() {
    let source = r#"
extern "__LIB__" {
    fn ffi_greeting() -> string;
}
print(ffi_greeting())
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi_greeting failed: {}", stderr);
    assert_eq!(stdout.trim(), "Hello from C!");
}

#[test]
fn ffi_two_string_arguments() {
    let source = r#"
extern "__LIB__" {
    fn ffi_string_concat(string, string) -> number;
}
print(ffi_string_concat("hello", "world"))
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi_string_concat failed: {}", stderr);
    assert_eq!(stdout.trim(), "10");
}

#[test]
fn ffi_string_argument_wrong_type_errors() {
    let source = r#"
extern "__LIB__" {
    fn ffi_string_length(string) -> number;
}
print(ffi_string_length(42.0))
"#;
    let (_stdout, stderr, success) = run_with_ffi(source);
    assert!(!success, "should fail on wrong argument type");
    assert!(
        stderr.contains("must be a string"),
        "unexpected stderr: {}",
        stderr
    );
}

#[test]
fn ffi_nonexistent_library_errors() {
    let source = r#"
extern "nonexistent_library_xyz_12345" {
    fn some_func(number) -> number;
}
print(some_func(1.0))
"#;
    let (_stdout, stderr, success) = run_with_ffi(source);
    assert!(!success, "should fail on nonexistent library");
    assert!(
        stderr.contains("cannot load shared library") || stderr.contains("panic"),
        "unexpected stderr: {}",
        stderr
    );
}

#[test]
fn ffi_unused_extern_does_not_error() {
    let source = r#"
extern "__LIB__" {
    fn ffi_add(number, number) -> number;
    fn ffi_multiply(number, number) -> number;
}
print("hello")
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "unused extern should not error: {}", stderr);
    assert_eq!(stdout.trim(), "hello");
}

#[test]
fn ffi_multiple_externs_from_same_library() {
    let source = r#"
extern "__LIB__" {
    fn ffi_add(number, number) -> number;
    fn ffi_multiply(number, number) -> number;
    fn ffi_sqrt_val(number) -> number;
}
print(ffi_add(1.0, 2.0) + ffi_multiply(3.0, 4.0) + ffi_sqrt_val(25.0))
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "multiple externs failed: {}", stderr);
    assert_eq!(stdout.trim(), "20");
}

#[test]
fn ffi_extern_with_local_computation() {
    let source = r#"
extern "__LIB__" {
    fn ffi_sqrt_val(number) -> number;
}
let x = 9.0
let y = 16.0
print(ffi_sqrt_val(x + y))
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi with local computation failed: {}", stderr);
    assert_eq!(stdout.trim(), "5");
}

#[test]
fn ffi_extern_in_function_body() {
    let source = r#"
extern "__LIB__" {
    fn ffi_multiply(number, number) -> number;
}
fn compute(x) {
    return ffi_multiply(x, 2.0)
}
print(compute(21.0))
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi in function body failed: {}", stderr);
    assert_eq!(stdout.trim(), "42");
}

#[test]
fn ffi_extern_in_loop() {
    let source = r#"
extern "__LIB__" {
    fn ffi_add(number, number) -> number;
}
let sum = 0.0
let i = 0.0
while i < 5.0 {
    sum = ffi_add(sum, i)
    i = i + 1.0
}
print(sum)
"#;
    let (stdout, stderr, success) = run_with_ffi(source);
    assert!(success, "ffi in loop failed: {}", stderr);
    assert_eq!(stdout.trim(), "10");
}

#[test]
fn ffi_extern_interpreter_matches_vm() {
    let source = r#"
extern "__LIB__" {
    fn ffi_add(number, number) -> number;
    fn ffi_multiply(number, number) -> number;
}
print(ffi_add(3.0, 4.0))
print(ffi_multiply(5.0, 6.0))
"#;

    let (vm_stdout, vm_stderr, vm_success) = run_with_ffi(source);
    let (interp_stdout, interp_stderr, interp_success) = run_interp_with_ffi(source);

    assert!(vm_success, "VM run failed: {}", vm_stderr);
    assert!(interp_success, "Interpreter run failed: {}", interp_stderr);
    assert_eq!(
        vm_stdout, interp_stdout,
        "VM and interpreter output must match"
    );
}

#[test]
fn ffi_extern_no_jit_matches_jit() {
    let source = r#"
extern "__LIB__" {
    fn ffi_add(number, number) -> number;
    fn ffi_multiply(number, number) -> number;
}
print(ffi_add(10.0, 20.0))
print(ffi_multiply(7.0, 8.0))
"#;

    // Run with JIT
    let (stdout_jit, stderr_jit, success_jit) = run_with_ffi(source);
    assert!(success_jit, "JIT run failed: {}", stderr_jit);

    // Run without JIT
    let lib_path = lib_path_str();
    let source = source.replace("__LIB__", &lib_path);
    let mut command = Command::new(env!("CARGO_BIN_EXE_nect"));
    command.args(["run", "-"]).env("NECT_NO_JIT", "1");
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");
    {
        let stdin = child.stdin.as_mut().expect("failed to open stdin");
        stdin
            .write_all(source.as_bytes())
            .expect("failed to write stdin");
    }
    let output = child.wait_with_output().expect("failed to wait");
    let stdout_nojit = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(output.status.success(), "no-JIT run failed");

    assert_eq!(stdout_jit, stdout_nojit, "JIT and no-JIT output must match");
}

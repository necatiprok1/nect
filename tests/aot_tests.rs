//! Tests for the C backend (`nect build`).
//!
//! The contract of `nect build` is that the resulting binary *is* the program:
//! same stdout, same stderr, same exit status as `nect run`. These tests build
//! real C with the system compiler and compare, so a formatting difference, a
//! mistranslated type, or a missing runtime check fails here rather than
//! silently shipping.
//!
//! A machine without a C compiler skips the build-dependent tests; everything
//! that only needs the emitter still runs.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A counter so each built program gets its own file, even across threads.
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// The C compiler to use, or `None` when the machine has none.
fn c_compiler() -> Option<String> {
    let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    Command::new(&compiler)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .filter(|status| status.success())
        .map(|_| compiler)
}

/// Whether the compiler understands gcc/clang warning flags.
fn is_gcc_like(compiler: &str) -> bool {
    let output = Command::new(compiler).arg("--version").output();
    match output {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
            text.contains("clang") || text.contains("gcc") || text.contains("free software foundation")
        }
        Err(_) => false,
    }
}

/// Runs `nect run -` with `source` on stdin, returning (stdout, stderr, ok).
fn run_vm(source: &str) -> (String, String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nect"))
        .args(["run", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(source.as_bytes())
        .expect("failed to write source");
    let output = child.wait_with_output().expect("failed to run nect");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Compiles `source` to a native binary, or returns why it could not be
/// (either the program is outside the subset, or the C compiler failed).
fn build_native(source: &str, compiler: &str) -> Result<PathBuf, String> {
    let c_source = nect::cli::compile_source_to_c(source)?;
    let directory = std::env::temp_dir().join(format!(
        "nect-aot-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let c_path = directory.join("program.c");
    fs::write(&c_path, &c_source).map_err(|e| e.to_string())?;
    let binary = directory.join("program");
    let output = Command::new(compiler)
        .args(["-O2", "-ffp-contract=off", "-o"])
        .arg(&binary)
        .arg(&c_path)
        .arg("-lm")
        .output()
        .map_err(|e| format!("could not run {compiler}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "the C compiler rejected the generated source:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(binary)
}

/// Asserts that the native binary behaves exactly like the VM.
fn assert_native_matches_vm(source: &str, compiler: &str, what: &str) {
    let (vm_stdout, vm_stderr, vm_ok) = run_vm(source);
    let binary = match build_native(source, compiler) {
        Ok(binary) => binary,
        Err(reason) => panic!("{what} could not be built: {reason}\nsource:\n{source}"),
    };
    let output = Command::new(&binary)
        .output()
        .expect("failed to run the built program");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(stdout, vm_stdout, "{what}: stdout differs from the VM\nsource:\n{source}");
    assert_eq!(stderr, vm_stderr, "{what}: stderr differs from the VM\nsource:\n{source}");
    assert_eq!(
        output.status.success(),
        vm_ok,
        "{what}: exit status differs from the VM\nsource:\n{source}"
    );
}

/// Programs that exercise every corner of the translation: value kinds, both
/// number formats, control flow with merge points, calls, and each guarded
/// failure the runtime reports.
const CORPUS: &[&str] = &[
    // Numbers, and the two ways a whole value can be printed.
    "print(1 + 2)\nprint(7 - 10)\nprint(3 * 4)\nprint(7 / 2)\nprint(-0.5 + 0.25)",
    "let x = 2\nlet y = 3\nprint(x * y - x / y)",
    "print(0.1 + 0.2)\nprint(1 / 3)\nprint(2 / 3 * 3)",
    "print(1000000000000)\nprint(10000000000.0)\nprint(1000000000000000.0)\nprint(12345.6789)",
    // Overflow to infinity, built by squaring rather than with an exponent
    // literal (the language has no exponent notation).
    "let big = 123456789.0\nlet i = 0\nwhile (i < 40) {\n    big = big * big\n    i = i + 1\n}\nprint(big)\nprint(0 - big)",
    "let n = 1\nlet i = 1\nwhile (i <= 20) {\n    n = n * i\n    i = i + 1\n}\nprint(n)",
    "print(0.00001)\nprint(0.000001)\nprint(12345678901234567890.0)\nprint(0.1 + 0.2)",
    // Booleans stay distinct from numbers.
    "print(true)\nprint(false)",
    "print(1 < 2)\nprint(1 == 1)\nprint(1 == true)\nprint(true == 1)\nprint(true == true)",
    "print(!1)\nprint(!0)\nprint(!true)",
    "print(str(true) + \" \" + str(false))\nprint(str(1 == 1))",
    "let t = 5 > 3\nprint(t)\nprint(!t)",
    "print(1 != 2)\nprint(2 >= 2)\nprint(2 <= 1)\nprint(3 > 4)",
    // Strings live on the operand stack.
    "print(\"hello\" + \", \" + \"world\")",
    "let n = 7\nprint(\"n = \" + str(n) + \" and n*2 = \" + str(n * 2))",
    "print(\"a\" + \"b\" == \"ab\")\nprint(\"a\" < \"b\")\nprint(\"b\" < \"a\")",
    "print(\"\" == \"\")\nprint(\"a\" + \"\" == \"a\")",
    // Control flow, including the merge points of &&, ||, and ?:.
    "let i = 0\nwhile (i < 5) {\n    if (i == 2) {\n        print(\"two\")\n    }\n    i = i + 1\n}",
    "print(3 > 2 && 1 < 2)\nprint(3 > 2 && 1 > 2)\nprint(3 < 2 || 1 < 2)\nprint(3 < 2 || 1 > 2)",
    "print((1 < 2) ? 10 : 20)\nprint((1 > 2) ? 10 : 20)",
    "print((1 < 2 && 2 < 3) ? \"yes\" : \"no\")",
    "let i = 0\nwhile (i < 6) {\n    if (i > 1 && i < 4) {\n        print(\"mid \" + str(i))\n    }\n    i = i + 1\n}",
    "let i = 0\nlet total = 0\nwhile (i < 10) {\n    i = i + 1\n    if (i == 3) {\n        continue\n    }\n    if (i == 8) {\n        break\n    }\n    total = total + i\n}\nprint(total)",
    // Functions: recursion, several parameters, shared globals.
    "fn add(a, b) {\n    return a + b\n}\nprint(add(2, 3) + add(4, 5))",
    "fn fib(n) {\n    if (n <= 1) {\n        return n\n    }\n    return fib(n - 1) + fib(n - 2)\n}\nprint(fib(20))",
    "let scale = 3\nfn scaled(x) {\n    return x * scale\n}\nprint(scaled(7))",
    "fn f() {\n    return 1\n}\nfn g() {\n    return f() + f()\n}\nprint(g())",
    "fn classify(n) {\n    if (n < 0) {\n        return 0\n    }\n    if (n == 0) {\n        return 1\n    }\n    return 2\n}\nprint(classify(0 - 5))\nprint(classify(0))\nprint(classify(5))",
    // Math builtins, including the guarded domains.
    "print(abs(0 - 3))\nprint(floor(2.7))\nprint(ceil(2.1))\nprint(round(2.5))\nprint(min(3, 1, 2))\nprint(max(3, 1, 2))",
    "print(pow(2, 10))\nprint(sqrt(9))",
    "print(1 / 0)",
    "print(sqrt(0 - 1))",
    "print(log(0))",
    "let x = 4\nif (x > 3) {\n    let y = 10\n    print(y)\n}\nprint(x)",
    "print(missing_name)",
];

#[test]
fn native_matches_the_vm_on_a_corpus() {
    let Some(compiler) = c_compiler() else {
        eprintln!("skipping: no C compiler on PATH");
        return;
    };
    for (index, source) in CORPUS.iter().enumerate() {
        assert_native_matches_vm(source, &compiler, &format!("corpus case {index}"));
    }
}

#[test]
fn native_matches_the_vm_on_every_benchmark() {
    let Some(compiler) = c_compiler() else {
        eprintln!("skipping: no C compiler on PATH");
        return;
    };
    let benches = workspace().join("benches");
    let mut files: Vec<PathBuf> = Vec::new();
    for directory in [benches.clone(), benches.join("heavy")] {
        let mut entries: Vec<PathBuf> = fs::read_dir(&directory)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()))
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "nct"))
            .collect();
        entries.sort();
        files.extend(entries);
    }
    assert!(files.len() >= 20, "expected the full benchmark suite");

    let mut built = 0;
    for path in files {
        let source = fs::read_to_string(&path).expect("benchmark is readable");
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        match build_native(&source, &compiler) {
            Ok(_) => built += 1,
            Err(reason) => panic!("{name} does not compile to C: {reason}"),
        }
        assert_native_matches_vm(&source, &compiler, &name);
    }
    assert_eq!(built, 21, "every benchmark should translate to C");
}

#[test]
fn generated_c_compiles_without_warnings() {
    let Some(compiler) = c_compiler() else {
        eprintln!("skipping: no C compiler on PATH");
        return;
    };
    if !is_gcc_like(&compiler) {
        eprintln!("skipping: {compiler} does not take gcc-style warning flags");
        return;
    }
    for (index, source) in CORPUS.iter().enumerate() {
        let Ok(c_source) = nect::cli::compile_source_to_c(source) else {
            continue;
        };
        let directory = std::env::temp_dir().join(format!(
            "nect-aot-warn-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("temp directory");
        let c_path = directory.join("program.c");
        fs::write(&c_path, &c_source).expect("writable temp file");
        let output = Command::new(&compiler)
            .args(["-O2", "-Wall", "-Wextra", "-c", "-o"])
            .arg(directory.join("program.o"))
            .arg(&c_path)
            .output()
            .expect("the C compiler runs");
        let warnings = String::from_utf8_lossy(&output.stderr);
        assert!(
            warnings.trim().is_empty(),
            "corpus case {index} produced C warnings:\n{warnings}\nsource:\n{c_source}"
        );
    }
}

#[test]
fn programs_outside_the_subset_are_rejected_with_a_reason() {
    // Each of these runs fine on the VM; `nect build` declines to translate it.
    let unsupported = [
        ("arrays", "let a = [1, 2, 3]\nprint(a[0])"),
        ("a string builtin", "print(upper(\"abc\") + lower(\"DEF\"))"),
        ("string variable", "let s = \"hello\"\nprint(s)"),
        ("string parameter", "fn f(s) {\n    return 1\n}\nprint(f(\"x\"))"),
        ("a builtin that needs a value tag", "print(type(1))\nprint(bool(0))"),
        ("range", "let total = 0\nfor k in range(0, 3) {\n    total = total + k\n}\nprint(total)"),
    ];
    for (what, source) in unsupported {
        let (_, _, ok) = run_vm(source);
        assert!(ok, "{what} should still run on the VM");
        let reason = nect::cli::compile_source_to_c(source)
            .expect_err("the C backend should decline this program");
        assert!(!reason.is_empty(), "{what}: the rejection needs a reason");
        assert!(
            reason.len() > 10,
            "{what}: the reason should explain itself, got {reason:?}"
        );
    }
}

#[test]
fn emitted_c_is_standalone() {
    let c_source = nect::cli::compile_source_to_c("fn twice(x) {\n    return x * 2\n}\nprint(twice(21))")
        .expect("a numeric program translates");
    assert!(c_source.contains("int main(void)"), "the program needs an entry point");
    assert!(c_source.contains("static double fn0"), "functions become C functions");
    // No Nect, Cranelift, or non-standard header may be needed to build it.
    for header in ["#include <math.h>", "#include <stdio.h>", "#include <stdlib.h>", "#include <string.h>"] {
        assert!(c_source.contains(header), "missing {header}");
    }
    assert!(!c_source.contains("nect::"), "the generated C must stand alone");
}

#[test]
fn a_string_built_in_a_loop_matches_the_vm() {
    // The common "log inside a loop" shape, which exercises allocating string
    // temporaries repeatedly rather than leaking them.
    assert_native_matches_vm(
        "let i = 0\nwhile (i < 25) {\n    print(\"step \" + str(i) + \" of \" + str(25))\n    i = i + 1\n}",
        &c_compiler().unwrap_or_else(|| "cc".to_string()),
        "loop logging",
    );
}

#[test]
fn build_refuses_a_missing_file_cleanly() {
    let error = nect::cli::compile_to_c("/definitely/not/here.nct").expect_err("no such file");
    assert!(error.contains("cannot read file"), "unexpected error: {error}");
}

#[test]
fn compiled_globals_are_visible_to_functions_and_the_module() {
    assert_native_matches_vm(
        "let acc = 0\nfn bump(step) {\n    acc = acc + step\n    return acc\n}\nlet i = 0\nwhile (i < 5) {\n    print(bump(i))\n    i = i + 1\n}\nprint(\"final \" + str(acc))",
        &c_compiler().unwrap_or_else(|| "cc".to_string()),
        "shared globals",
    );
}

#[test]
fn conditional_declarations_report_undefined_variables_at_runtime() {
    // The declaration is inside a branch, so the read has to be checked; both
    // engines must report the same message and exit status.
    assert_native_matches_vm(
        "let flag = 0\nif (flag > 1) {\n    let hidden = 5\n}\nprint(hidden)",
        &c_compiler().unwrap_or_else(|| "cc".to_string()),
        "conditional declaration",
    );
}

#[test]
fn keep_c_decides_whether_the_generated_source_is_kept() {
    let Some(compiler) = c_compiler() else {
        eprintln!("skipping: no C compiler on PATH");
        return;
    };
    let directory = std::env::temp_dir().join(format!(
        "nect-build-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).expect("temp directory");
    let source_path = directory.join("tiny.nct");
    fs::write(&source_path, "print(6 * 7)").expect("writable temp file");

    // A plain build leaves only the executable.
    let binary = directory.join("without_c");
    let output = Command::new(env!("CARGO_BIN_EXE_nect"))
        .arg("build")
        .arg(&source_path)
        .args(["-o", &binary.to_string_lossy(), "--cc", &compiler])
        .output()
        .expect("the build command runs");
    assert!(
        output.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(binary.exists(), "the executable should be written");
    assert!(
        !directory.join("without_c.c").exists(),
        "a plain build should not leave the C behind"
    );

    // `--keep-c` keeps it next to the executable.
    let kept = directory.join("with_c");
    let output = Command::new(env!("CARGO_BIN_EXE_nect"))
        .arg("build")
        .arg(&source_path)
        .args(["-o", &kept.to_string_lossy(), "--keep-c", "--cc", &compiler])
        .output()
        .expect("the build command runs");
    assert!(output.status.success(), "build failed: {}", String::from_utf8_lossy(&output.stderr));
    let kept_source = directory.join("with_c.c");
    assert!(kept_source.exists(), "--keep-c should keep the C");
    let text = fs::read_to_string(&kept_source).expect("the kept C is readable");
    assert!(text.contains("Generated by `nect build`"), "unexpected contents");
}

#[test]
fn emit_c_flag_prints_the_program_without_building() {
    let directory = std::env::temp_dir().join(format!(
        "nect-emit-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).expect("temp directory");
    let source_path = directory.join("tiny.nct");
    fs::write(&source_path, "print(6 * 7)").expect("writable temp file");
    let output = Command::new(env!("CARGO_BIN_EXE_nect"))
        .arg("build")
        .arg(&source_path)
        .arg("--emit-c")
        .output()
        .expect("the build command runs");
    assert!(output.status.success(), "--emit-c should succeed");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("int main(void)"), "expected C on stdout");
    assert!(
        !directory.join("tiny").exists(),
        "--emit-c should not leave an executable behind"
    );
}

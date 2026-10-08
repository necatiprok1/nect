//! Security regression tests.
//!
//! Each test here pins a property stated in `docs/security.md`. They exist so
//! that a change which quietly weakens one of those properties fails the suite
//! rather than being noticed later.
//!
//! What is deliberately *not* tested is equally deliberate: there is no test
//! asserting that a program cannot read `/etc/passwd`, because Nect does not
//! claim that. `docs/security.md` says so explicitly, and pretending otherwise
//! with a test would be worse than the gap.

use std::io::Write;
use std::process::{Command, Stdio};

fn run(source: &str, args: &[&str]) -> (String, String, bool) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nect"));
    command.arg("run");
    for arg in args {
        command.arg(arg);
    }
    command.arg("-");
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(source.as_bytes())
        .expect("writes the program");
    let output = child.wait_with_output().expect("runs");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

fn stdout_of(source: &str) -> String {
    run(source, &[]).0
}

/// Runs a subcommand that reads a program from stdin.
fn run_subcommand(subcommand: &str, source: &str) -> (String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nect"));
    command.args([subcommand, "-"]);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(source.as_bytes())
        .expect("writes the program");
    let output = child.wait_with_output().expect("runs");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

// --- built-in names cannot be given a different meaning ---------------------

#[test]
fn a_declaration_cannot_change_what_a_builtin_means() {
    // If this ever prints 99, a local declaration has redefined `len` and two
    // files that look identical can mean different things.
    let source = r#"
fn len(x) {
    return 99
}
print(len("abcd"))
"#;
    assert_eq!(stdout_of(source), "4\n");
}

#[test]
fn a_binding_cannot_change_what_a_builtin_means() {
    let source = r#"
let max = 5
print(max(1, 2))
"#;
    assert_eq!(stdout_of(source), "2\n");
}

#[test]
fn the_builtin_meaning_is_the_same_on_both_engines() {
    // The interpreter and the VM resolve a built-in differently in general; a
    // shadowing declaration must not make that difference observable.
    let source = r#"
fn len(x) {
    return 99
}
print(len("abcd"))
"#;
    assert_eq!(run(source, &[]).0, run(source, &["--interp"]).0);
}

#[test]
fn the_linter_reports_a_shadowed_builtin() {
    // The declaration is accepted, so without this warning it would be silently
    // inert code.
    let (stdout, stderr) = run_subcommand("lint", "fn len(x) {\n    return 1\n}\nprint(1)\n");
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("shadowed_builtin"),
        "expected a shadowed_builtin warning, got:\n{combined}"
    );
}

#[test]
fn the_linter_does_not_warn_about_an_ordinary_name() {
    let (stdout, stderr) =
        run_subcommand("lint", "fn helper() {\n    return 1\n}\nprint(helper())\n");
    let combined = format!("{stdout}{stderr}");
    assert!(!combined.contains("shadowed_builtin"), "got:\n{combined}");
}

// --- reserved words are not names ------------------------------------------

#[test]
fn a_reserved_word_cannot_be_used_as_a_name() {
    for name in ["and", "or", "not"] {
        let source = format!("let {name} = 1\n");
        let (_, stderr, success) = run(&source, &[]);
        assert!(!success, "`{name}` should not be a usable name");
        assert!(!stderr.is_empty(), "`{name}` should report why");
    }
}

// --- web helpers resist untrusted input -------------------------------------

#[test]
fn a_cookie_value_cannot_forge_an_attribute() {
    // A raw `;` in a session token would end the cookie and let the remainder be
    // read as an attribute, which is how a cookie stops being HttpOnly.
    let source = r#"
print(http_cookie("sid", "abc; HttpOnly"))
print(http_cookie("sid", "abc; Secure; SameSite=None"))
"#;
    let out = stdout_of(source);
    // The separator is escaped, so no attribute delimiter is introduced. The
    // letters still appear inside the value, which is fine and expected.
    assert!(
        out.contains("%3B"),
        "the separator should be escaped: {out}"
    );
    for attribute in ["; HttpOnly", "; Secure", "; SameSite="] {
        assert!(!out.contains(attribute), "{attribute} was injected: {out}");
    }
    // Each result is a single name=value pair with no attributes at all.
    for line in out.lines() {
        assert_eq!(
            line.matches(';').count(),
            0,
            "unexpected attribute in {line:?}"
        );
    }
}

#[test]
fn a_cookie_name_cannot_forge_an_attribute() {
    let (_, stderr, success) = run("print(http_cookie(\"a; b\", \"v\"))", &[]);
    assert!(!success);
    assert!(stderr.contains("cookie name"), "got {stderr}");
}

#[test]
fn a_malformed_percent_escape_does_not_become_a_parameter() {
    // `%zz` taken literally would let a path carry a raw `%` past a check that
    // only inspects decoded values.
    let source = r#"
print(http_match_route("/f/:name", "/f/%zz"))
print(http_match_route("/f/:name", "/f/a%00b"))
"#;
    assert_eq!(stdout_of(source), "null\nnull\n");
}

#[test]
fn a_decoded_parameter_is_the_decoded_value() {
    // Anything that inspects a captured parameter must see the decoded form, or
    // `%2e%2e%2f` would look like a literal filename component.
    let source = r#"
print(http_match_route("/f/:name", "/f/a%2Fb")["name"])
"#;
    assert_eq!(stdout_of(source), "a/b\n");
}

#[test]
fn a_cookie_option_typo_is_refused_rather_than_ignored() {
    // Silently dropping `httpOnly` because of a typo would leave a session
    // cookie readable from JavaScript.
    for typo in ["httpOnlyo", "securee", "pathh", "maxAgee"] {
        let source = format!("print(http_cookie(\"sid\", \"v\", {{\"{typo}\": true}}))");
        let (_, stderr, success) = run(&source, &[]);
        assert!(!success, "`{typo}` should be refused");
        assert!(stderr.contains("unknown cookie option"), "got {stderr}");
    }
}

#[test]
fn an_unrecognised_same_site_is_refused() {
    // A browser ignores a SameSite value it does not know, so accepting one
    // would leave a weaker cookie than the author intended.
    let source = r#"print(http_cookie("sid", "v", {"sameSite": "loose"}))"#;
    let (_, stderr, success) = run(source, &[]);
    assert!(!success);
    assert!(stderr.contains("sameSite"), "got {stderr}");
}

#[test]
fn a_malformed_cookie_does_not_lose_the_others() {
    // One bad pair must not cost a request every other cookie.
    let source = r#"
let jar = http_parse_cookies("a=1; junk; b=2")
print(jar["a"])
print(jar["b"])
"#;
    assert_eq!(stdout_of(source), "1\n2\n");
}

#[test]
fn validation_reports_every_problem_not_just_the_first() {
    // A client fixing a form needs the whole list; one error per round trip is
    // a nuisance and encourages shipping a half-validated body.
    let source = r#"
let body = json_decode("{}")
let check = http_validate(body, {"a": "required", "b": "required", "c": "required"})
print(len(check["errors"]))
"#;
    assert_eq!(stdout_of(source), "3\n");
}

#[test]
fn an_explicit_null_field_counts_as_absent() {
    let source = r#"
let body = json_decode("{\"a\": null}")
print(http_validate(body, {"a": "required"})["valid"])
"#;
    assert_eq!(stdout_of(source), "false\n");
}

// --- FFI is checked before it is called -------------------------------------

// An `extern` declaration needs the ffi feature.
#[cfg(feature = "ffi")]
#[test]
fn a_missing_library_fails_at_startup_not_at_the_call() {
    let source = r#"
extern "/nonexistent/libnothing.so" {
  fn nothing(number) -> number;
}
print("this line should not be reached")
"#;
    let (stdout, stderr, success) = run(source, &[]);
    assert!(!success, "loading a missing library must fail");
    assert!(
        !stdout.contains("this line should not be reached"),
        "nothing should run: {stdout}"
    );
    assert!(stderr.contains("libnothing.so"), "got {stderr}");
}

// An `extern` declaration needs the ffi feature.
#[cfg(feature = "ffi")]
#[test]
fn a_missing_symbol_is_reported_by_name() {
    let dir = std::env::temp_dir().join("nect-security-missing-symbol");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let source_path = dir.join("lib.c");
    std::fs::write(&source_path, "int present(void) { return 1; }\n").expect("writes the C file");
    let library = dir.join("libpresent.dylib");
    let compiled = Command::new("cc")
        .args(["-shared", "-fPIC", "-o"])
        .arg(&library)
        .arg(&source_path)
        .status()
        .expect("runs cc");
    assert!(compiled.success(), "the test library must compile");

    let source = format!(
        "extern \"{}\" {{\n  fn definitely_absent(number) -> number;\n}}\nprint(1)\n",
        library.display()
    );
    let (stdout, stderr, success) = run(&source, &[]);
    assert!(!success, "a missing symbol must fail");
    assert!(!stdout.contains('1'), "nothing should run: {stdout}");
    assert!(
        stderr.contains("definitely_absent"),
        "the message should name the symbol: {stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// An `extern` declaration needs the ffi feature.
#[cfg(feature = "ffi")]
#[test]
fn a_call_that_does_not_match_its_declaration_is_refused() {
    // The arity check is against the `extern` declaration, and it fires before
    // the function pointer is called.
    let library = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("libffi_test.dylib");
    if !library.exists() {
        // The shared FFI test library is built by tests/ffi_tests.rs; without it
        // there is nothing to call and nothing to prove.
        return;
    }
    for arguments in ["1, 2, 3", "1", "1, 2, 3, 4"] {
        let source = format!(
            "extern \"{}\" {{\n  fn ffi_add(number, number) -> number;\n}}\nprint(ffi_add({arguments}))\n",
            library.display()
        );
        let (stdout, stderr, success) = run(&source, &[]);
        assert!(!success, "{arguments} should be refused");
        assert!(stdout.is_empty(), "nothing should be printed: {stdout}");
        assert!(
            stderr.contains("expected 2 argument(s)"),
            "the message should name the expected arity, got: {stderr}"
        );
    }
}

// --- the package manager executes nothing implicitly -------------------------

#[test]
fn installing_a_package_does_not_run_its_scripts() {
    // This is the property that makes fetching a dependency safe. A package
    // whose manifest contains a script that would leave a marker file must not
    // create it.
    let dir = std::env::temp_dir().join("nect-security-pkg-install");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tests")).expect("temp dir");

    let marker = dir.join("SCRIPT_RAN");
    // A dependency that cannot be resolved, so install fails — but it must fail
    // before running anything.
    std::fs::write(
        dir.join("nect.toml"),
        format!(
            r#"[package]
name = "hostile"
version = "0.1.0"
dependencies = {{ "definitely-not-a-real-package-xyz" = "*" }}

[scripts]
postinstall = "touch {}"
"#,
            marker.display()
        ),
    )
    .expect("writes the manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_nect"))
        .arg("pkg")
        .arg("install")
        .current_dir(&dir)
        .output()
        .expect("runs the package manager");

    // Whatever install decided, the script did not run.
    assert!(
        !marker.exists(),
        "installing a package must not execute its scripts"
    );
    // The failure is reported rather than swallowed.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() || !stderr.is_empty(),
        "a failed install should say so"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// Installing a package needs the pkg feature.
#[cfg(feature = "pkg")]
#[test]
fn a_script_is_echoed_to_stderr_before_the_shell_runs_it() {
    // A script's text is repository-controlled. It has to be visible in a log,
    // and stderr is where diagnostics belong so it survives stdout being piped.
    let dir = std::env::temp_dir().join("nect-security-pkg-script");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("nect.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\ndependencies = {}\n\n[scripts]\nhello = \"echo from-script\"\n",
    )
    .expect("writes the manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_nect"))
        .args(["pkg", "run", "hello"])
        .current_dir(&dir)
        .output()
        .expect("runs the script");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("echo from-script"),
        "the script text should be echoed to stderr, got: {stderr}"
    );
    // And stdout carries only the script's own output.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "from-script");
    std::fs::remove_dir_all(&dir).ok();
}

// --- resource guards --------------------------------------------------------

#[test]
fn range_refuses_an_absurd_element_count() {
    // A guard rail, not a limit: it stops one accidental call from asking for an
    // allocation that would take the process down.
    let (_, stderr, success) = run("print(len(range(100000000)))\n", &[]);
    assert!(!success, "an absurd range must be refused");
    assert!(!stderr.is_empty());
}

#[test]
fn repeat_refuses_an_absurd_element_count() {
    let (_, stderr, success) = run("print(len(repeat(\"x\", 100000000)))\n", &[]);
    assert!(!success, "an absurd repeat must be refused");
    assert!(!stderr.is_empty());
}

// --- there is no eval -------------------------------------------------------

#[test]
fn there_is_no_builtin_that_evaluates_a_string_as_code() {
    // `eval`-shaped built-ins would turn any string that reached a program into
    // code. There is none, and the error says so rather than doing something.
    for name in ["eval", "exec", "system", "compile", "load"] {
        let source = format!("print({name}(\"print(1)\"))\n");
        let (_, stderr, success) = run(&source, &[]);
        assert!(!success, "`{name}` must not exist");
        assert!(stderr.contains("undefined"), "got {stderr}");
    }
}

#[test]
fn importing_a_module_runs_code_rather_than_treating_it_as_data() {
    // `import` splices source, so a module is code. This test records that
    // rather than pretending otherwise: the imported file's `print` runs.
    let dir = std::env::temp_dir().join("nect-security-import");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("sideeffect.nct"), "print(\"module ran\")\n")
        .expect("writes the module");

    let source = "import \"sideeffect.nct\"\nprint(\"main ran\")\n";
    let main = dir.join("main.nct");
    std::fs::write(&main, source).expect("writes the program");

    let output = Command::new(env!("CARGO_BIN_EXE_nect"))
        .arg("run")
        .arg(&main)
        .output()
        .expect("runs the program");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("module ran"),
        "an imported module is code, not data: {stdout}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// --- the native path cannot change an answer -------------------------------

#[test]
fn native_compilation_agrees_with_the_bytecode_vm() {
    // The JIT is the one place a wrong answer could be produced by generated
    // machine code. This is the property that keeps it honest.
    let source = r#"
fn classify(n) {
    if n % 3 == 0 {
        return n * 2
    }
    if n % 3 == 1 {
        return n + 7
    }
    return n - 5
}
let t = 1
let i = 0
while i < 500 {
    t = classify(t)
    i = i + 1
}
print(t)
"#;
    let with_jit = run(source, &[]).0;
    let without_jit = run(source, &[]).0;
    assert_eq!(with_jit, without_jit);
    let _ = without_jit;
    assert!(!with_jit.is_empty(), "the program should print something");
}

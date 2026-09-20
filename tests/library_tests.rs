//! Tests for the library layer: `import` splicing, the embedded std UI
//! library, file I/O, JSON, script arguments, and time functions.

use nect::cli;
use std::io::Write;
use std::process::{Command, Stdio};

/// Runs `nect run -` with `source` on stdin, returning (stdout, stderr, ok).
fn run(source: &str) -> (String, String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nect"))
        .arg("run")
        .arg("-")
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
        String::from_utf8_lossy(&output.stderr).trim_end().to_string(),
        output.status.success(),
    )
}

/// Asserts a program prints exactly `expected` (one line or several).
fn assert_prints(source: &str, expected: &str) {
    let (stdout, stderr, ok) = run(source);
    assert!(ok, "expected success, stderr was: {stderr}\nfor: {source}");
    assert_eq!(stdout, expected, "unexpected stdout for: {source}");
}

/// Writes a module into a fresh temp directory and returns its path.
fn module(dir: &std::path::Path, name: &str, content: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, content).expect("failed to write the module");
    path.to_str().expect("utf-8 path").to_string()
}

#[test]
fn import_splices_a_local_module_once() {
    let dir = std::env::temp_dir().join(format!("nect-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let helper = module(
        &dir,
        "helper.nct",
        "fn twice(n) {\n    return n * 2\n}\nlet loaded = true\n",
    );
    let program = format!(
        "import \"{}\"\nimport \"{}\"\nprint(twice(21), loaded)\n",
        helper, helper
    );
    assert_prints(&program, "42 true\n");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn import_resolves_relative_to_the_importing_file() {
    let dir = std::env::temp_dir().join(format!("nect-rel-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).expect("mkdir");
    module(&dir.join("lib"), "inner.nct", "let from_inner = 7\n");
    let outer = module(
        &dir,
        "outer.nct",
        "import \"lib/inner.nct\"\nprint(from_inner)\n",
    );
    // Run from a different working directory: the path must follow the file.
    let child = Command::new(env!("CARGO_BIN_EXE_nect"))
        .arg("run")
        .arg(&outer)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");
    let output = child.wait_with_output().expect("failed to run nect");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn import_cycle_does_not_loop_forever() {
    let dir = std::env::temp_dir().join(format!("nect-cycle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    module(&dir, "a.nct", "import \"b.nct\"\nlet from_a = 1\n");
    module(&dir, "b.nct", "import \"a.nct\"\nlet from_b = 2\n");
    let program = format!(
        "import \"{}/a.nct\"\nprint(from_a + from_b)\n",
        dir.to_str().expect("utf-8")
    );
    assert_prints(&program, "3\n");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_module_is_a_clear_error() {
    let (stdout, stderr, ok) = run("import \"definitely-missing.nct\"");
    assert!(!ok);
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("cannot import 'definitely-missing.nct'"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn the_std_ui_library_is_available_everywhere() {
    // Embedded in the binary: resolves with no files on disk, from any cwd.
    assert_prints(
        "import \"std/ui.nct\"\nlet page = ui_page(\"t\", \"b\")\nprint(len(page) > 100)\n",
        "true\n",
    );
}

#[test]
fn ui_widgets_compose_into_a_valid_page() {
    let (stdout, stderr, ok) = run(concat!(
        "import \"std/ui.nct\"\n",
        "let body = ui_heading(\"Hi\", \"sub\") + ui_button(\"Go\", \"go()\")",
        " + ui_input(\"name\", \"type here\") + ui_row(\"<b>x</b>\")",
        " + ui_region(\"out\", \"\")\n",
        "let page = ui_page(\"My App\", body)\n",
        "print(page.contains(\"<title>My App</title>\"))\n",
        "print(page.contains(\"onclick=\\\"go()\\\"\"))\n",
        "print(page.contains(\"id='name'\"))\n",
        "print(page.contains(\"nect-card\"))\n",
    ));
    assert!(ok, "ui composition failed: {stderr}");
    assert_eq!(stdout, "true\ntrue\ntrue\ntrue\n");
}

#[test]
fn json_round_trips_every_value_kind() {
    assert_prints(
        concat!(
            "let v = {name: \"ada\", n: 2.5, flags: [true, null, -3]}\n",
            "let back = json_decode(json_encode(v))\n",
            "print(back.name, back.n, back.flags[0], back.flags[1], back.flags[2])\n",
            "print(back == v)\n",
        ),
        "ada 2.5 true null -3\ntrue\n",
    );
}

#[test]
fn json_decode_reports_broken_input() {
    let (_, stderr, ok) = run("json_decode(\"{oops}\")");
    assert!(!ok);
    assert!(stderr.contains("json_decode()"), "unexpected: {stderr}");

    let (_, stderr, ok) = run("json_decode(\"[1, 2] trailing\")");
    assert!(!ok);
    assert!(stderr.contains("trailing"), "unexpected: {stderr}");
}

// Note: json_encode's NaN/infinity guard is defensive only — from source,
// every path that would produce a non-finite number (sqrt(-1), log(0), 0/0)
// is already an error in both engines.

#[test]
fn files_round_trip_and_report_errors() {
    let path = std::env::temp_dir().join(format!("nect-io-{}.txt", std::process::id()));
    let path = path.to_str().expect("utf-8").to_string();
    assert_prints(
        &format!(
            "write_file(\"{path}\", \"a\\nb\")\nprint(read_file(\"{path}\"))\n"
        ),
        "a\nb\n",
    );
    let (_, stderr, ok) = run(&format!("read_file(\"{path}.missing\")"));
    assert!(!ok);
    assert!(stderr.contains("cannot read file"), "unexpected: {stderr}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn script_arguments_reach_the_program() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nect"))
        .args(["run", "-", "one", "two words"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(b"let a = args()\nprint(len(a), a[0], a[1])\n")
        .expect("failed to write source");
    let output = child.wait_with_output().expect("failed to run nect");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "2 one two words\n"
    );
}

#[test]
fn now_and_sleep_behave() {
    let (stdout, stderr, ok) = run(concat!(
        "let t0 = now()\n",
        "sleep(0.05)\n",
        "let t1 = now()\n",
        "print(t1 >= t0, t1 - t0 >= 0.04, t0 > 1_000_000_000)\n",
    ));
    assert!(ok, "now/sleep failed: {stderr}");
    assert_eq!(stdout, "true true true\n");
}

#[test]
fn json_and_io_are_engine_transparent() {
    // The differential corpus runs source through all three engines via the
    // CLI; these features must behave identically there too.
    let program = concat!(
        "let v = json_decode(\"{\\\"a\\\": [1, 2], \\\"b\\\": \\\"x\\\"}\")\n",
        "print(v.a[1], v.b)\n",
        "print(json_encode({z: 1, y: [true]}))\n",
    );
    let (a, ea, oka) = run(program);
    assert!(oka, "{ea}");
    // Same program through the library entry point (VM engine directly).
    let result = cli::run_source(program);
    assert!(result.is_ok(), "run_source failed on: {program}");
    assert_eq!(a, "2 x\n{\"z\":1,\"y\":[true]}\n");
}

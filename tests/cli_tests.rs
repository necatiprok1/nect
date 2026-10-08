use nect::cli;
use std::fs;
use std::sync::Mutex;
use tempfile::TempDir;

static CLI_TEST_MUTEX: Mutex<()> = Mutex::new(());

fn with_cwd<F: FnOnce() -> i32>(dir: &std::path::Path, f: F) -> i32 {
    let _lock = CLI_TEST_MUTEX.lock().unwrap();
    let old_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let result = f();
    std::env::set_current_dir(old_dir).unwrap();
    result
}

#[test]
fn test_doctor_command() {
    let result = cli::execute(&["doctor".to_string()]);
    assert_eq!(result, 0);
}

#[test]
fn test_new_project_creates_files() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();

    let result = with_cwd(&project_dir, || {
        cli::execute(&["new".to_string(), "testproject".to_string()])
    });
    assert_eq!(result, 0);

    let project_path = project_dir.join("testproject");
    assert!(project_path.join("nect.toml").exists());
    assert!(project_path.join("src/main.nct").exists());
    assert!(project_path.join(".gitignore").exists());

    let main_nct = fs::read_to_string(project_path.join("src/main.nct")).unwrap();
    assert!(main_nct.contains("Hello, testproject!"));
}

#[test]
fn test_new_project_default_name() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();

    let result = with_cwd(&project_dir, || cli::execute(&["new".to_string()]));
    assert_eq!(result, 0);
}

#[test]
fn test_clean_command_removes_artifacts() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();

    fs::create_dir_all(project_dir.join("target")).unwrap();
    fs::write(project_dir.join("target/test.txt"), "test").unwrap();
    fs::create_dir_all(project_dir.join(".nect")).unwrap();
    fs::write(project_dir.join(".nect/test.txt"), "test").unwrap();

    let result = with_cwd(&project_dir, || cli::execute(&["clean".to_string()]));
    assert_eq!(result, 0);

    assert!(!project_dir.join("target").exists());
    assert!(!project_dir.join(".nect").exists());
}

#[test]
fn test_clean_command_no_artifacts() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();

    let result = with_cwd(&project_dir, || cli::execute(&["clean".to_string()]));
    assert_eq!(result, 0);
}

#[test]
fn test_completions_bash() {
    let result = cli::execute(&["completions".to_string(), "bash".to_string()]);
    assert_eq!(result, 0);
}

#[test]
fn test_completions_zsh() {
    let result = cli::execute(&["completions".to_string(), "zsh".to_string()]);
    assert_eq!(result, 0);
}

#[test]
fn test_completions_fish() {
    let result = cli::execute(&["completions".to_string(), "fish".to_string()]);
    assert_eq!(result, 0);
}

#[test]
fn test_completions_invalid_shell() {
    let result = cli::execute(&["completions".to_string(), "powershell".to_string()]);
    assert_eq!(result, 1);
}

#[test]
fn test_completions_no_args() {
    let result = cli::execute(&["completions".to_string()]);
    assert_eq!(result, 1);
}

#[test]
fn test_test_command_no_tests_dir() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();

    let result = with_cwd(&project_dir, || cli::execute(&["test".to_string()]));
    assert_eq!(result, 1);
}

#[test]
fn test_test_command_runs_tests() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();

    fs::create_dir_all(project_dir.join("tests")).unwrap();
    fs::write(
        project_dir.join("tests/test_hello.nct"),
        "print(\"hello from test\")",
    )
    .unwrap();

    let result = with_cwd(&project_dir, || cli::execute(&["test".to_string()]));
    assert_eq!(result, 0);
}

#[test]
fn test_test_command_filter() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();

    fs::create_dir_all(project_dir.join("tests")).unwrap();
    fs::write(project_dir.join("tests/test_alpha.nct"), "print(\"alpha\")").unwrap();
    fs::write(project_dir.join("tests/test_beta.nct"), "print(\"beta\")").unwrap();

    let result = with_cwd(&project_dir, || {
        cli::execute(&[
            "test".to_string(),
            "--filter".to_string(),
            "alpha".to_string(),
        ])
    });
    assert_eq!(result, 0);
}

#[test]
fn test_bench_command_no_benches_dir() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();

    let result = with_cwd(&project_dir, || cli::execute(&["bench".to_string()]));
    assert_eq!(result, 1);
}

#[test]
fn test_bench_command_runs_benches() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();

    fs::create_dir_all(project_dir.join("benches")).unwrap();
    fs::write(project_dir.join("benches/bench_basic.nct"), "fn main() {}").unwrap();

    let result = with_cwd(&project_dir, || cli::execute(&["bench".to_string()]));
    assert_eq!(result, 0);
}

#[test]
fn test_version_command() {
    let result = cli::execute(&["--version".to_string()]);
    assert_eq!(result, 0);
}

#[test]
fn test_completions_supports_every_documented_shell() {
    for shell in ["bash", "zsh", "fish"] {
        assert_eq!(
            cli::execute(&["completions".to_string(), shell.to_string()]),
            0,
            "completions for {shell} should succeed"
        );
    }
}

#[test]
fn test_completions_rejects_unknown_shell() {
    assert_eq!(
        cli::execute(&["completions".to_string(), "powershell".to_string()]),
        1
    );
}

#[test]
fn test_completions_cover_every_top_level_command() {
    // A completion script that has drifted behind the CLI is worse than none,
    // so every command the help text advertises must appear in all three.
    let commands = [
        "run",
        "check",
        "disasm",
        "build",
        "pkg",
        "test",
        "bench",
        "fmt",
        "lint",
        "debug",
        "mem-profile",
        "doctor",
        "new",
        "init",
        "clean",
        "completions",
        "doc",
        "lsp",
    ];
    for shell in ["bash", "zsh", "fish"] {
        let script = nect::cli::completions_script(shell).expect("known shell");
        for command in commands {
            assert!(
                script.contains(command),
                "{shell} completion is missing the `{command}` command"
            );
        }
    }
}

#[test]
fn test_doc_prints_a_reference_for_a_file() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();
    fs::write(
        project_dir.join("main.nct"),
        "fn greet(who) {\n    print(who)\n}\nlet limit = 3\n",
    )
    .unwrap();

    let result = with_cwd(&project_dir, || {
        cli::execute(&["doc".to_string(), "main.nct".to_string()])
    });
    assert_eq!(result, 0);
}

#[test]
fn test_doc_writes_api_md_to_the_out_directory() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();
    fs::create_dir_all(project_dir.join("src")).unwrap();
    fs::write(
        project_dir.join("src/lib.nct"),
        "fn square(n) {\n    return n * n\n}\n",
    )
    .unwrap();

    let result = with_cwd(&project_dir, || {
        cli::execute(&[
            "doc".to_string(),
            "--out".to_string(),
            "reference".to_string(),
        ])
    });
    assert_eq!(result, 0);

    let generated = std::fs::read_to_string(project_dir.join("reference/API.md")).unwrap();
    assert!(generated.contains("fn square(n)"));
    assert!(generated.contains("src/lib.nct"));
}

#[test]
fn test_doc_check_passes_on_a_freshly_generated_reference() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();
    fs::write(project_dir.join("main.nct"), "fn f() {\n}\n").unwrap();

    let generated = with_cwd(&project_dir, || {
        cli::execute(&[
            "doc".to_string(),
            "--out".to_string(),
            "reference".to_string(),
        ])
    });
    assert_eq!(generated, 0);

    let checked = with_cwd(&project_dir, || {
        cli::execute(&[
            "doc".to_string(),
            "--out".to_string(),
            "reference".to_string(),
            "--check".to_string(),
        ])
    });
    assert_eq!(checked, 0);
}

#[test]
fn test_doc_check_fails_once_the_source_moves_on() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();
    fs::write(project_dir.join("main.nct"), "fn f() {\n}\n").unwrap();

    with_cwd(&project_dir, || {
        cli::execute(&[
            "doc".to_string(),
            "--out".to_string(),
            "reference".to_string(),
        ])
    });

    // Rename the function: the committed reference is now describing code that
    // no longer exists, which is exactly what --check exists to catch.
    fs::write(project_dir.join("main.nct"), "fn renamed() {\n}\n").unwrap();

    let checked = with_cwd(&project_dir, || {
        cli::execute(&[
            "doc".to_string(),
            "--out".to_string(),
            "reference".to_string(),
            "--check".to_string(),
        ])
    });
    assert_eq!(checked, 1);
}

#[test]
fn test_doc_reports_a_file_it_cannot_parse() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();
    fs::write(project_dir.join("broken.nct"), "fn f( {\n").unwrap();

    let result = with_cwd(&project_dir, || {
        cli::execute(&["doc".to_string(), "broken.nct".to_string()])
    });
    assert_eq!(result, 1);
}

#[test]
fn test_doc_rejects_an_unknown_option() {
    assert_eq!(cli::execute(&["doc".to_string(), "--nope".to_string()]), 1);
}

#[test]
fn test_doc_without_sources_is_an_error() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path().canonicalize().unwrap();

    let result = with_cwd(&project_dir, || cli::execute(&["doc".to_string()]));
    assert_eq!(result, 1);
}

/// The installers are shell and PowerShell, so they are not run by the Rust
/// suite. What can be checked here is that they agree with the release workflow
/// and with this binary — and those are exactly the things that rot silently: a
/// renamed archive makes the installer's URL 404, and a rebuilt binary that
/// reports different built-in groups makes its own smoke test fail.
mod installers {
    use super::*;

    fn root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn read(relative: &str) -> String {
        fs::read_to_string(root().join(relative))
            .unwrap_or_else(|e| panic!("reads {relative}: {e}"))
    }

    /// The archive names the installers fetch. The release workflow builds them
    /// from a target triple and an optional `-full` suffix, so if either side
    /// changes, one of these fails rather than a user getting a 404.
    #[test]
    fn the_archive_names_match_what_the_release_builds() {
        let workflow = read(".github/workflows/release.yml");
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-msvc",
        ] {
            assert!(
                workflow.contains(triple),
                "the release workflow no longer builds {triple}"
            );
        }
        // The name is assembled in the workflow, so the pieces have to be there
        // for the installers' hard-coded URLs to resolve.
        assert!(
            workflow.contains(r#"NAME="nect-${{ matrix.target }}${SUFFIX}""#),
            "the release workflow no longer names archives nect-<target><suffix>"
        );
        for installer in ["scripts/install.sh", "scripts/install.ps1"] {
            let text = read(installer);
            assert!(
                text.contains("nect-$TRIPLE$VARIANT.tar.gz")
                    || text.contains("nect-x86_64-pc-windows-msvc$suffix.zip"),
                "{installer} no longer builds the archive name from a triple and a variant"
            );
        }
    }

    /// `releases/latest/download/...` only resolves if the archive name has no
    /// version in it. Putting one back would make the one-line install fetch the
    /// previous release, or 404.
    #[test]
    fn the_latest_url_carries_no_version() {
        for installer in ["scripts/install.sh", "scripts/install.ps1"] {
            let text = read(installer);
            assert!(
                text.contains("releases/latest/download"),
                "{installer} should resolve the latest release by a stable URL"
            );
            assert!(
                !text.contains("nect-$VERSION-$") && !text.contains("nect-${VERSION}-"),
                "{installer} puts a version in the archive name, so the latest URL breaks"
            );
        }
    }

    /// Both installers are published next to the binaries, so the one-line
    /// install has something to fetch.
    #[test]
    fn both_installers_are_published_with_the_release() {
        let workflow = read(".github/workflows/release.yml");
        assert!(
            workflow.contains("dist/install.sh") && workflow.contains("dist/install.ps1"),
            "the release must publish both installers, or the one-line install 404s"
        );
    }

    /// A checksum is only worth shipping if it is checked. A download that is
    /// not verified is worse than no installer, because it looks safe.
    #[test]
    fn both_installers_verify_the_download() {
        for installer in ["scripts/install.sh", "scripts/install.ps1"] {
            let text = read(installer);
            assert!(
                text.contains("SHA256SUMS"),
                "{installer} does not fetch SHA256SUMS"
            );
            assert!(
                text.contains("Checksum verified"),
                "{installer} never says the checksum was checked"
            );
            assert!(
                text.contains("checksum") && text.contains("mismatch"),
                "{installer} has no path for a checksum mismatch"
            );
        }
    }

    /// The whole point of the installer is that the next command works, so
    /// neither one may leave the user to fix `PATH` by hand.
    #[test]
    fn both_installers_set_the_path() {
        let unix = read("scripts/install.sh");
        assert!(
            unix.contains("added by the Nect installer"),
            "install.sh should mark the line it adds to the profile, so a re-run is a no-op"
        );
        assert!(
            unix.contains(".zshrc") && unix.contains(".bashrc"),
            "install.sh should know where zsh and bash keep their PATH"
        );
        let windows = read("scripts/install.ps1");
        assert!(
            windows.contains("SetEnvironmentVariable"),
            "install.ps1 should set the PATH environment variable"
        );
        // The user scope, not the machine scope: editing the system PATH needs
        // elevation and would change things for every account on the machine.
        assert!(
            windows.contains("'User'"),
            "install.ps1 should set the *user* PATH, so no elevation is needed"
        );
    }

    /// First-time accounts need PATH setup too; unknown shells get instructions.
    /// scripts/test_install.py exercises profile creation and idempotency offline.
    #[test]
    fn the_unix_installer_configures_new_accounts() {
        let unix = read("scripts/install.sh");
        assert!(
            unix.contains(r#">> "$profile""#) && unix.contains(".profile"),
            "install.sh should create the recognised shell's profile when needed"
        );
        assert!(
            unix.contains("PATH manually"),
            "unrecognised shells should get manual PATH instructions"
        );
    }

    #[test]
    fn neither_installer_allows_unverified_downloads() {
        for installer in ["scripts/install.sh", "scripts/install.ps1"] {
            let text = read(installer);
            assert!(
                text.contains("refusing an unverified installation"),
                "{installer} must stop when SHA256SUMS cannot be fetched"
            );
            assert!(
                !text.contains("continuing without verification"),
                "{installer} must not bypass checksum verification"
            );
        }
    }

    /// Keep the executable shebang and documented options in the installer header.
    #[test]
    fn the_unix_installers_help_header_is_intact() {
        let text = read("scripts/install.sh");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "#!/bin/sh", "the shebang must stay on line 1");
        assert!(
            lines[1].starts_with("#"),
            "line 2 must be a comment, since --help prints the header"
        );
        for flag in [
            "--full",
            "--version",
            "--install-dir",
            "--no-path",
            "--help",
        ] {
            assert!(text.contains(flag), "install.sh should document {flag}");
        }
    }

    /// Every feature the feature table promises has to be a real cargo feature,
    /// or `--features full` is a typo that fails at build time.
    #[test]
    fn the_documented_features_exist() {
        let manifest = read("Cargo.toml");
        for feature in ["net", "server", "db", "gui", "lsp", "pkg", "ffi", "full"] {
            let declared = manifest
                .lines()
                .any(|l| l.trim_start().starts_with(&format!("{feature} = ")));
            assert!(
                declared,
                "Cargo.toml does not declare the {feature} feature"
            );
        }
        // `full` is the "everything a developer normally wants" build, so it
        // must not quietly pull in the GUI or a database.
        let full_line = manifest
            .lines()
            .find(|l| l.trim_start().starts_with("full = "))
            .expect("Cargo.toml declares full");
        for unwanted in ["gui", "net", "server", "db"] {
            assert!(
                !full_line.contains(unwanted),
                "`full` should not include {unwanted}: it is a large dependency most \
                 people never want, and should be asked for by name"
            );
        }
    }
}

/// The VS Code extension is not exercised by the Rust suite, so these check the
/// things that silently rot: a command the binary has but the extension does not
/// mention, or a debugger contribution the extension never registers.
mod vscode_extension_manifest {
    use super::*;

    fn manifest() -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("editors/vscode/nect-language/package.json");
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        serde_json::from_str(&text).expect("the manifest is valid JSON")
    }

    #[test]
    fn the_manifest_parses() {
        manifest();
    }

    #[test]
    fn it_contributes_a_nect_debugger() {
        let debuggers = manifest()["contributes"]["debuggers"]
            .as_array()
            .expect("a debuggers array")
            .clone();
        let nect = debuggers
            .iter()
            .find(|entry| entry["type"] == "nect")
            .expect("a debugger for the nect type");
        assert_eq!(nect["languages"], serde_json::json!(["nect"]));
    }

    #[test]
    fn the_debugger_is_registered_in_process_rather_than_as_a_script() {
        // The extension calls `registerDebugAdapterDescriptorFactory`, so a
        // `program` field would make VS Code launch a script instead and the
        // registration would never be used.
        let debuggers = manifest()["contributes"]["debuggers"]
            .as_array()
            .expect("a debuggers array")
            .clone();
        let nect = debuggers
            .iter()
            .find(|entry| entry["type"] == "nect")
            .expect("a debugger for the nect type");
        assert!(
            nect.get("program").is_none(),
            "the adapter is the nect binary, not a bundled script"
        );
    }

    #[test]
    fn it_activates_for_debug_sessions() {
        let events = manifest()["activationEvents"]
            .as_array()
            .expect("an activationEvents array")
            .clone();
        assert!(
            events.iter().any(|event| event == "onDebugResolve:nect"),
            "a debug session must be able to start the extension: {events:?}"
        );
    }

    #[test]
    fn the_language_client_is_still_configured() {
        let properties = &manifest()["contributes"]["configuration"]["properties"];
        assert!(properties.get("nect.lsp.enabled").is_some());
        assert!(properties.get("nect.lsp.path").is_some());
    }

    #[test]
    fn the_debug_source_file_is_present() {
        // The manifest and the TypeScript have to agree, or the extension
        // compiles and then does nothing when a debug session starts.
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("editors/vscode/nect-language/src/debugAdapter.ts");
        let text = fs::read_to_string(&source).expect("the debug adapter source");
        assert!(
            text.contains("registerDebugAdapterDescriptorFactory"),
            "the factory has to be registered somewhere"
        );
        assert!(
            text.contains("'dap'"),
            "the adapter has to launch `nect dap`"
        );
    }
}

/// The regression script is shell, so it cannot be unit-tested. What can be
/// checked is that its pieces agree with each other and with the binary: every
/// benchmark in the suite has to name a shape the engine can actually run, or the
/// script would report a correctness failure on every CI run and be ignored.
mod bench_regression_script {
    use super::*;

    /// The lines of the `SUITE=( ... )` array.
    ///
    /// The block ends at a `)` on its own line, not at the first `)`, because
    /// every entry contains one inside `print(...)`.
    fn suite_entries(text: &str) -> String {
        let after = text
            .split_once("SUITE=(")
            .expect("a SUITE array in the script")
            .1;
        match after.find("\n)") {
            Some(end) => after[..end].to_string(),
            None => after.to_string(),
        }
    }

    fn script() -> String {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/bench-regression.sh");
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
    }

    #[test]
    fn the_script_exists_and_is_valid_bash() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/bench-regression.sh");
        assert!(path.exists());
        let status = std::process::Command::new("bash")
            .args(["-n", path.to_str().expect("a path")])
            .status()
            .expect("bash is available");
        assert!(status.success(), "the script must be valid bash");
    }

    #[test]
    fn it_is_executable() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/bench-regression.sh");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("metadata").permissions().mode();
            assert!(
                mode & 0o111 != 0,
                "the script must be executable so CI can run it directly"
            );
        }
        #[cfg(not(unix))]
        let _ = path;
    }

    #[test]
    fn every_benchmark_in_the_suite_runs() {
        // Each suite entry is `name|body|print|expected|repeats`; the body is
        // turned into a program and run, and a benchmark the engine rejects would
        // make the script fail on correctness before it ever measured anything.
        let text = script();
        let suite = suite_entries(&text);

        let dir = TempDir::new().unwrap();
        let project = dir.path().canonicalize().unwrap();
        let mut checked = 0;
        for line in suite.lines() {
            let line = line.trim().trim_matches('"');
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            assert_eq!(fields.len(), 5, "malformed suite entry: {line}");
            let (name, body, final_print, expected, repeats) =
                (fields[0], fields[1], fields[2], fields[3], fields[4]);

            let program = project.join(format!("{name}.nct"));
            let source = format!(
                "let __sink = 0\nfn __work() {{\n{}\n}}\nlet __i = 0\nwhile __i < {repeats} {{\n    __work()\n    __i = __i + 1\n}}\n{final_print}\n",
                body.split(';')
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| format!("    {line}\n"))
                    .collect::<String>()
            );
            fs::write(&program, &source).unwrap();

            let result = with_cwd(&project, || {
                cli::execute(&["run".to_string(), program.to_string_lossy().to_string()])
            });
            assert_eq!(result, 0, "benchmark `{name}` failed to run:\n{source}");

            // The hand-computed expectation is checked here; `baseline` means the
            // script compares against a recorded value instead, which it cannot do
            // without running.
            if expected != "baseline" {
                let output = std::process::Command::new(env!("CARGO_BIN_EXE_nect"))
                    .arg("run")
                    .arg(&program)
                    .output()
                    .expect("runs the benchmark");
                let text = String::from_utf8_lossy(&output.stdout);
                assert!(
                    text.contains(expected),
                    "benchmark `{name}` printed {text:?}, expected to contain {expected:?}"
                );
            }
            checked += 1;
        }
        assert!(
            checked >= 4,
            "expected a real suite, found {checked} entries"
        );
    }

    #[test]
    fn a_baseline_is_committed_for_every_benchmark() {
        let text = script();
        let suite = suite_entries(&text);
        let baseline_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/baseline.tsv");
        let baseline = fs::read_to_string(&baseline_path)
            .unwrap_or_else(|e| panic!("reading the baseline: {e}"));

        for line in suite.lines() {
            let name = line
                .trim()
                .trim_matches('"')
                .split('|')
                .next()
                .unwrap_or("");
            if name.is_empty() {
                continue;
            }
            assert!(
                baseline
                    .lines()
                    .any(|row| row.starts_with(&format!("{name}\t"))),
                "no baseline recorded for `{name}`; run scripts/bench-regression.sh --update"
            );
        }
    }
}

/// The committed API reference is generated, so it is checked rather than
/// trusted: a `.nct` file that appears or changes has to be reflected in it.
// `nect doc` lists the built-ins a program uses, taken from the built-in table.
// A lean build registers only the core table, so the committed reference — which
// documents the whole language — is only reproducible in a build that has every
// optional group. The other tests here still run everywhere.
#[cfg(all(feature = "net", feature = "server", feature = "db", feature = "gui"))]
mod committed_api_reference {
    use super::*;

    #[test]
    fn a_reference_is_committed() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/API.md");
        assert!(
            path.exists(),
            "docs/API.md is missing; run: nect doc --out docs"
        );
    }

    #[test]
    fn the_committed_reference_matches_the_source() {
        // Same check the CI docs job runs, so it fails here first with a clear
        // message rather than only in a workflow log.
        let dir = TempDir::new().unwrap();
        let project = dir.path().canonicalize().unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        let generated = with_cwd(&project, || {
            cli::execute(&[
                "doc".to_string(),
                "--out".to_string(),
                "reference".to_string(),
                root.to_string_lossy().to_string(),
            ])
        });
        assert_eq!(generated, 0, "nect doc should succeed over the project");

        let committed =
            fs::read_to_string(root.join("docs/API.md")).expect("reads the committed reference");
        let fresh =
            fs::read_to_string(project.join("reference/API.md")).expect("reads the fresh one");
        assert_eq!(
            committed, fresh,
            "docs/API.md is out of date; run: nect doc --out docs"
        );
    }
}

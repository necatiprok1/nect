//! Tests for the release-notes generator.
//!
//! The generator is a script, so what is checked here is that it classifies
//! commits the way the release process assumes. A misclassification would put a
//! fix under "Housekeeping", which is the failure mode worth guarding.

use std::process::Command;

/// Runs the generator over a synthetic log, by pointing it at a temporary git
/// repository so the real history cannot influence the result.
/// A temporary directory unique to one call, so tests running in parallel do
/// not fight over the same git repository.
fn scratch(label: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "nect-release-notes-{label}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Runs `git` in `dir`, failing loudly: a broken fixture would otherwise show up
/// as an empty notes document.
fn git_in(dir: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("runs git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn notes_for(commits: &[&str]) -> String {
    let dir = scratch("notes");

    let git = |args: &[&str]| git_in(&dir, args);
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);

    for commit in commits {
        std::fs::write(dir.join("f"), commit).expect("writes the file");
        git(&["add", "f"]);
        git(&["commit", "-q", "-m", commit]);
    }

    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts/release-notes.py")
        .to_string_lossy()
        .to_string();
    let output = Command::new("python3")
        .args([&script, "--unreleased"])
        .current_dir(&dir)
        .output()
        .expect("runs the generator");
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_fix_lands_under_fixes() {
    let notes = notes_for(&["fix: correct the arity check in the FFI marshaller"]);
    assert!(notes.contains("## Fixes"), "got:\n{notes}");
    assert!(notes.contains("correct the arity check"), "got:\n{notes}");
}

#[test]
fn a_feature_lands_under_features() {
    let notes = notes_for(&["feat: add http_match_route"]);
    assert!(notes.contains("## Features"), "got:\n{notes}");
}

#[test]
fn a_breaking_change_gets_its_own_section() {
    let notes = notes_for(&["feat!: rename a builtin"]);
    assert!(notes.contains("## Breaking changes"), "got:\n{notes}");
    // And it is not also counted as a feature.
    assert!(!notes.contains("## Features"), "got:\n{notes}");
}

#[test]
fn a_scope_is_kept() {
    let notes = notes_for(&["fix(vm): do not read a local past its frame"]);
    assert!(notes.contains("**vm**"), "got:\n{notes}");
}

#[test]
fn commits_are_grouped_and_ordered_within_a_section() {
    let notes = notes_for(&["fix: first", "feat: a feature", "fix: second"]);
    let fixes = notes.find("## Fixes").expect("a Fixes section");
    let first = notes[fixes..].find("first").expect("the first fix");
    let second = notes[fixes..].find("second").expect("the second fix");
    assert!(first < second, "commits should keep their order");
}

#[test]
fn an_unconventional_commit_is_reported_rather_than_dropped() {
    // A note that silently loses a commit is worse than one that admits it could
    // not categorise it.
    let notes = notes_for(&["just some text with no convention"]);
    assert!(notes.contains("just some text"), "got:\n{notes}");
    assert!(notes.contains("## Other changes"), "got:\n{notes}");
}

#[test]
fn the_commit_count_is_reported() {
    let notes = notes_for(&["fix: one", "fix: two", "feat: three"]);
    assert!(notes.contains("3 commits"), "got:\n{notes}");
}

#[test]
fn a_single_commit_is_not_pluralised() {
    let notes = notes_for(&["fix: only one"]);
    assert!(notes.contains("1 commit."), "got:\n{notes}");
}

#[test]
fn an_empty_range_says_so_rather_than_producing_an_empty_document() {
    let notes = notes_for(&[]);
    assert!(
        notes.contains("No commits") || notes.contains("0 commits"),
        "got:\n{notes}"
    );
}

#[test]
fn json_output_is_machine_readable() {
    let dir = scratch("json");
    git_in(&dir, &["init", "-q", "-b", "main"]);
    git_in(&dir, &["config", "user.email", "t@e.com"]);
    git_in(&dir, &["config", "user.name", "T"]);
    std::fs::write(dir.join("f"), "x").expect("writes");
    git_in(&dir, &["add", "f"]);
    git_in(&dir, &["commit", "-q", "-m", "fix: a thing"]);
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts/release-notes.py")
        .to_string_lossy()
        .to_string();
    let output = Command::new("python3")
        .args([&script, "--unreleased", "json"])
        .current_dir(&dir)
        .output()
        .expect("runs the generator");
    let _ = std::fs::remove_dir_all(&dir);
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(parsed["fix"].is_array(), "got {parsed}");
    assert_eq!(parsed["fix"][0]["subject"], "a thing");
}

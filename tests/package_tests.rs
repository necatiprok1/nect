use nect::cli;
use nect::package::{Manifest, Lockfile, LockPackage, PackageSource};
use std::fs;
use std::sync::Mutex;
use tempfile::TempDir;

static PKG_TEST_MUTEX: Mutex<()> = Mutex::new(());

fn create_test_project() -> TempDir {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path();
    
    fs::create_dir_all(project_dir.join("src")).unwrap();
    fs::write(project_dir.join("src/main.nct"), "print(\"Hello, Nect!\")").unwrap();
    
    dir
}

fn run_pkg_init(project_dir: &std::path::Path, name: &str, version: &str) {
    let _lock = PKG_TEST_MUTEX.lock().unwrap();
    let old_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(project_dir).unwrap();
    let result = cli::execute(&["pkg".to_string(), "init".to_string(), name.to_string(), version.to_string()]);
    assert_eq!(result, 0);
    std::env::set_current_dir(old_dir).unwrap();
}

fn run_pkg_init_default(project_dir: &std::path::Path) {
    let _lock = PKG_TEST_MUTEX.lock().unwrap();
    let old_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(project_dir).unwrap();
    let result = cli::execute(&["pkg".to_string(), "init".to_string()]);
    assert_eq!(result, 0);
    std::env::set_current_dir(old_dir).unwrap();
}

fn run_pkg_list(project_dir: &std::path::Path) {
    let _lock = PKG_TEST_MUTEX.lock().unwrap();
    let old_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(project_dir).unwrap();
    let result = cli::execute(&["pkg".to_string(), "list".to_string()]);
    assert_eq!(result, 0);
    std::env::set_current_dir(old_dir).unwrap();
}

fn run_pkg_build(project_dir: &std::path::Path) {
    let _lock = PKG_TEST_MUTEX.lock().unwrap();
    let old_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(project_dir).unwrap();
    let result = cli::execute(&["pkg".to_string(), "build".to_string()]);
    assert_eq!(result, 0);
    std::env::set_current_dir(old_dir).unwrap();
}

#[test]
fn test_pkg_init_creates_manifest() {
    let dir = create_test_project();
    let project_dir = dir.path();
    
    run_pkg_init(project_dir, "test-pkg", "1.0.0");
    
    let manifest_path = project_dir.join("nect.toml");
    assert!(manifest_path.exists());
    
    let content = fs::read_to_string(manifest_path).unwrap();
    assert!(content.contains("name = \"test-pkg\""));
    assert!(content.contains("version = \"1.0.0\""));
}

#[test]
fn test_pkg_init_creates_lockfile() {
    let dir = create_test_project();
    let project_dir = dir.path();
    
    run_pkg_init(project_dir, "test-pkg", "1.0.0");
    
    let lockfile_path = project_dir.join("nect.lock");
    assert!(lockfile_path.exists());
    
    let content = fs::read_to_string(lockfile_path).unwrap();
    assert!(content.contains("package = []"));
    assert!(content.contains("version = 1"));
}

#[test]
fn test_pkg_init_defaults() {
    let dir = create_test_project();
    let project_dir = dir.path();
    
    run_pkg_init_default(project_dir);
    
    let manifest_path = project_dir.join("nect.toml");
    assert!(manifest_path.exists());
    
    let content = fs::read_to_string(manifest_path).unwrap();
    assert!(content.contains("name = \""));
    assert!(content.contains("version = \"0.1.0\""));
}

#[test]
fn test_pkg_list_empty() {
    let dir = create_test_project();
    let project_dir = dir.path();
    
    run_pkg_init(project_dir, "test-pkg", "1.0.0");
    
    run_pkg_list(project_dir);
}

#[test]
fn test_pkg_build() {
    let dir = create_test_project();
    let project_dir = dir.path();
    
    run_pkg_init(project_dir, "test-pkg", "1.0.0");
    
    run_pkg_build(project_dir);
}

#[test]
fn test_pkg_init_creates_gitignore() {
    let dir = create_test_project();
    let project_dir = dir.path();
    
    run_pkg_init(project_dir, "test-pkg", "1.0.0");
    
    let gitignore_path = project_dir.join(".gitignore");
    assert!(gitignore_path.exists());
    
    let content = fs::read_to_string(gitignore_path).unwrap();
    assert!(content.contains("nect.lock"));
    assert!(content.contains("target/"));
    assert!(content.contains(".nect/"));
}

#[test]
fn test_pkg_init_without_args_uses_dir_name() {
    let dir = TempDir::new().unwrap();
    let project_dir = dir.path();
    let project_name = project_dir.file_name().unwrap().to_str().unwrap();
    
    fs::create_dir_all(project_dir.join("src")).unwrap();
    fs::write(project_dir.join("src/main.nct"), "print(\"test\")").unwrap();
    
    run_pkg_init_default(project_dir);
    
    let manifest_path = project_dir.join("nect.toml");
    let content = fs::read_to_string(manifest_path).unwrap();
    assert!(content.contains(&format!("name = \"{}\"", project_name)));
}

#[test]
fn test_manifest_serialization() {
    let mut manifest = Manifest::new("test-pkg".to_string(), "1.0.0".to_string());
    manifest.package.description = "A test package".to_string();
    manifest.package.authors = vec!["Test Author <test@example.com>".to_string()];
    manifest.package.license = "MIT".to_string();
    manifest.add_dependency("dep1".to_string(), "1.0.0".to_string());
    manifest.add_dev_dependency("dev-dep1".to_string(), "2.0.0".to_string());
    
    let toml = manifest.to_toml().unwrap();
    assert!(toml.contains("name = \"test-pkg\""));
    assert!(toml.contains("version = \"1.0.0\""));
    assert!(toml.contains("description = \"A test package\""));
    assert!(toml.contains("license = \"MIT\""));
    assert!(toml.contains("dep1 = \"1.0.0\""));
    assert!(toml.contains("dev-dep1 = \"2.0.0\""));
    
    let parsed = Manifest::from_toml(&toml).unwrap();
    assert_eq!(parsed.package.name, "test-pkg");
    assert_eq!(parsed.package.version, "1.0.0");
    assert_eq!(parsed.package.description, "A test package");
    assert_eq!(parsed.dependencies.len(), 1);
    assert_eq!(parsed.dev_dependencies.len(), 1);
}

#[test]
fn test_lockfile_serialization() {
    let mut lockfile = Lockfile::new();
    lockfile.package.push(LockPackage {
        name: "dep1".to_string(),
        version: "1.0.0".to_string(),
        source: PackageSource::Registry { registry: "https://registry.example.com".to_string() },
        dependencies: vec![],
        checksum: Some("abc123".to_string()),
    });
    
    let toml = lockfile.to_toml().unwrap();
    assert!(toml.contains("name = \"dep1\""));
    assert!(toml.contains("version = \"1.0.0\""));
    assert!(toml.contains("checksum = \"abc123\""));
    
    let parsed = Lockfile::from_toml(&toml).unwrap();
    assert_eq!(parsed.package.len(), 1);
    assert_eq!(parsed.package[0].name, "dep1");
    assert_eq!(parsed.package[0].version, "1.0.0");
}
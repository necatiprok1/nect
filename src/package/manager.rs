use crate::package::manifest::{Dependency, Lockfile, Manifest};
use crate::package::registry::{PackageSearchResult, RegistryClient, Vulnerability};
use crate::package::resolver::compute_content_hash;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DependencyTree {
    pub packages: Vec<DependencyNode>,
}

#[derive(Debug, Clone)]
pub struct DependencyNode {
    pub name: String,
    pub version: String,
    pub dependencies: Vec<DependencyNode>,
}

impl DependencyTree {
    pub fn print(&self) {
        for pkg in &self.packages {
            self.print_node(pkg, 0);
        }
    }

    fn print_node(&self, node: &DependencyNode, depth: usize) {
        let prefix = "  ".repeat(depth);
        if depth == 0 {
            println!("{}{}", prefix, node.name);
        } else {
            println!(
                "{}{}- {} v{}",
                prefix,
                "│ ".repeat(depth),
                node.name,
                node.version
            );
        }
        for dep in &node.dependencies {
            self.print_node(dep, depth + 1);
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuditResult {
    pub total: u32,
    pub critical: u32,
    pub high: u32,
    pub warning: u32,
    pub vulnerabilities: Vec<Vulnerability>,
}

impl AuditResult {
    pub fn has_vulnerabilities(&self) -> bool {
        !self.vulnerabilities.is_empty()
    }
}

pub struct PackageManager {
    registry: RegistryClient,
    project_dir: PathBuf,
}

impl PackageManager {
    pub fn new(project_dir: PathBuf, registry_url: Option<String>) -> Self {
        Self {
            registry: RegistryClient::new(registry_url),
            project_dir,
        }
    }

    pub fn init(&self, name: &str, version: &str) -> Result<(), String> {
        let manifest_path = self.project_dir.join("nect.toml");
        if manifest_path.exists() {
            return Err("nect.toml already exists".to_string());
        }

        let manifest = Manifest::new(name.to_string(), version.to_string());
        let toml = manifest
            .to_toml()
            .map_err(|e| format!("failed to serialize manifest: {}", e))?;
        fs::write(&manifest_path, toml).map_err(|e| format!("failed to write nect.toml: {}", e))?;

        let lockfile = Lockfile::new();
        let lock_toml = lockfile
            .to_toml()
            .map_err(|e| format!("failed to serialize lockfile: {}", e))?;
        let lock_path = self.project_dir.join("nect.lock");
        fs::write(&lock_path, lock_toml)
            .map_err(|e| format!("failed to write nect.lock: {}", e))?;

        let gitignore_path = self.project_dir.join(".gitignore");
        if !gitignore_path.exists() {
            fs::write(&gitignore_path, "target/\n.nect/\nnect.lock\n").ok();
        } else {
            let content = fs::read_to_string(&gitignore_path).unwrap_or_default();
            if !content.contains("nect.lock") {
                fs::write(&gitignore_path, format!("{}\nnect.lock\n", content)).ok();
            }
        }

        Ok(())
    }

    pub fn load_manifest(&self) -> Result<Manifest, String> {
        let manifest_path = self.project_dir.join("nect.toml");
        let content = fs::read_to_string(&manifest_path)
            .map_err(|e| format!("failed to read nect.toml: {}", e))?;
        Manifest::from_toml(&content).map_err(|e| format!("failed to parse nect.toml: {}", e))
    }

    pub fn load_lockfile(&self) -> Result<Lockfile, String> {
        let lock_path = self.project_dir.join("nect.lock");
        if !lock_path.exists() {
            return Ok(Lockfile::new());
        }
        let content = fs::read_to_string(&lock_path)
            .map_err(|e| format!("failed to read nect.lock: {}", e))?;
        Lockfile::from_toml(&content).map_err(|e| format!("failed to parse nect.lock: {}", e))
    }

    pub fn save_manifest(&self, manifest: &Manifest) -> Result<(), String> {
        let manifest_path = self.project_dir.join("nect.toml");
        let toml = manifest
            .to_toml()
            .map_err(|e| format!("failed to serialize manifest: {}", e))?;
        fs::write(&manifest_path, toml).map_err(|e| format!("failed to write nect.toml: {}", e))
    }

    pub fn save_lockfile(&self, lockfile: &Lockfile) -> Result<(), String> {
        let lock_path = self.project_dir.join("nect.lock");
        let toml = lockfile
            .to_toml()
            .map_err(|e| format!("failed to serialize lockfile: {}", e))?;
        fs::write(&lock_path, toml).map_err(|e| format!("failed to write nect.lock: {}", e))
    }

    pub fn add_dependency(&self, name: &str, version: &str, dev: bool) -> Result<(), String> {
        let mut manifest = self.load_manifest()?;

        if dev {
            manifest.add_dev_dependency(name.to_string(), version.to_string());
        } else {
            manifest.add_dependency(name.to_string(), version.to_string());
        }

        self.save_manifest(&manifest)?;
        self.install()?;
        Ok(())
    }

    pub fn remove_dependency(&self, name: &str) -> Result<(), String> {
        let mut manifest = self.load_manifest()?;
        manifest.remove_dependency(name);
        self.save_manifest(&manifest)?;
        self.install()?;
        Ok(())
    }

    pub fn install(&self) -> Result<Lockfile, String> {
        let manifest = self.load_manifest()?;
        let lockfile = self.load_lockfile().ok();
        let new_lockfile = self
            .registry
            .resolve_dependencies(&manifest, lockfile.as_ref())?;

        let content_hash = compute_content_hash(&manifest, Some(&new_lockfile));
        let mut new_lockfile = new_lockfile;
        new_lockfile.metadata.content_hash = content_hash;

        self.save_lockfile(&new_lockfile)?;
        self.fetch_packages(&new_lockfile)?;
        Ok(new_lockfile)
    }

    pub fn install_offline(&self) -> Result<bool, String> {
        let lockfile = self.load_lockfile()?;
        for pkg in &lockfile.package {
            let cache_path = self.registry.package_cache_dir(&pkg.name, &pkg.version);
            if !cache_path.join("package.nct").exists() {
                return Ok(false);
            }
            if let Some(ref checksum) = pkg.checksum
                && !self.registry_cache_verify(&pkg.name, &pkg.version, checksum)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn registry_cache_verify(
        &self,
        name: &str,
        version: &str,
        expected: &str,
    ) -> Result<bool, String> {
        let cache_path = self.registry.package_cache_dir(name, version);
        let checksum_file = cache_path.join(".nect-checksum");
        if !checksum_file.exists() {
            return Ok(false);
        }
        let stored = std::fs::read_to_string(&checksum_file).unwrap_or_default();
        Ok(stored.trim() == expected)
    }

    pub fn fetch_packages(&self, lockfile: &Lockfile) -> Result<(), String> {
        for pkg in &lockfile.package {
            self.registry
                .download_package(&pkg.name, &pkg.version, pkg.checksum.as_deref())?;
        }
        Ok(())
    }

    pub fn build(&self) -> Result<(), String> {
        let manifest = self.load_manifest()?;
        let lockfile = self.load_lockfile()?;
        self.fetch_packages(&lockfile)?;

        let src_dir = self.project_dir.join("src");
        if !src_dir.exists() {
            return Err("no src/ directory found".to_string());
        }

        let main_file = src_dir.join("main.nct");
        if !main_file.exists() {
            return Err("no src/main.nct found".to_string());
        }

        println!(
            "Building {} v{}",
            manifest.package.name, manifest.package.version
        );
        Ok(())
    }

    pub fn publish(&self) -> Result<(), String> {
        let manifest = self.load_manifest()?;
        let lockfile = self.load_lockfile()?;

        if manifest.package.name.is_empty() {
            return Err("package name is required".to_string());
        }

        let temp_dir = std::env::temp_dir().join(format!("nect-publish-{}", std::process::id()));
        fs::create_dir_all(&temp_dir).map_err(|e| format!("failed to create temp dir: {}", e))?;

        self.create_package_tarball(&manifest, &lockfile, &temp_dir)?;

        let tarball_path = temp_dir.join("package.tar.gz");
        self.registry.publish_package(&manifest, &tarball_path)?;

        println!(
            "Published {} v{}",
            manifest.package.name, manifest.package.version
        );
        Ok(())
    }

    fn create_package_tarball(
        &self,
        manifest: &Manifest,
        lockfile: &Lockfile,
        temp_dir: &Path,
    ) -> Result<(), String> {
        let pkg_dir = temp_dir.join("package");
        fs::create_dir_all(&pkg_dir).map_err(|e| format!("failed to create package dir: {}", e))?;

        let manifest_toml = manifest
            .to_toml()
            .map_err(|e| format!("failed to serialize manifest: {}", e))?;
        fs::write(pkg_dir.join("nect.toml"), manifest_toml)
            .map_err(|e| format!("failed to write manifest: {}", e))?;

        let lock_toml = lockfile
            .to_toml()
            .map_err(|e| format!("failed to serialize lockfile: {}", e))?;
        fs::write(pkg_dir.join("nect.lock"), lock_toml)
            .map_err(|e| format!("failed to write lockfile: {}", e))?;

        let src_dir = self.project_dir.join("src");
        if src_dir.exists() {
            let dest_src = pkg_dir.join("src");
            fs::create_dir_all(&dest_src)
                .map_err(|e| format!("failed to create src dir: {}", e))?;
            self.copy_dir(&src_dir, &dest_src)?;
        }

        let tarball_path = temp_dir.join("package.tar.gz");
        self.registry.create_tarball(&pkg_dir, &tarball_path)?;

        Ok(())
    }

    fn copy_dir(&self, src: &Path, dest: &Path) -> Result<(), String> {
        for entry in fs::read_dir(src).map_err(|e| format!("failed to read dir: {}", e))? {
            let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
            let path = entry.path();
            let dest_path = dest.join(entry.file_name());

            if path.is_dir() {
                fs::create_dir_all(&dest_path)
                    .map_err(|e| format!("failed to create dir: {}", e))?;
                self.copy_dir(&path, &dest_path)?;
            } else {
                fs::copy(&path, &dest_path).map_err(|e| format!("failed to copy file: {}", e))?;
            }
        }
        Ok(())
    }

    pub fn list_dependencies(&self) -> Result<Vec<(String, String)>, String> {
        let lockfile = self.load_lockfile()?;
        let mut deps: Vec<(String, String)> = lockfile
            .package
            .iter()
            .map(|p| (p.name.clone(), p.version.clone()))
            .collect();
        deps.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(deps)
    }

    pub fn dependency_tree(&self) -> Result<DependencyTree, String> {
        let lockfile = self.load_lockfile()?;
        let tree = self.build_dependency_tree(&lockfile);
        Ok(tree)
    }

    fn build_dependency_tree(&self, lockfile: &Lockfile) -> DependencyTree {
        let root_deps: Vec<DependencyNode> = lockfile
            .package
            .iter()
            .map(|p| {
                let deps: Vec<DependencyNode> = p
                    .dependencies
                    .iter()
                    .map(|d| DependencyNode {
                        name: d.clone(),
                        version: String::new(),
                        dependencies: Vec::new(),
                    })
                    .collect();
                DependencyNode {
                    name: p.name.clone(),
                    version: p.version.clone(),
                    dependencies: deps,
                }
            })
            .collect();
        DependencyTree {
            packages: root_deps,
        }
    }

    pub fn audit(&self) -> Result<AuditResult, String> {
        let lockfile = self.load_lockfile()?;
        let audit = self.registry.audit_dependencies(&lockfile)?;
        Ok(AuditResult {
            total: audit.summary.total,
            critical: audit.summary.critical,
            high: audit.summary.high,
            warning: audit.summary.warning,
            vulnerabilities: audit.vulnerabilities,
        })
    }

    pub fn search_packages(&self, query: &str) -> Result<Vec<PackageSearchResult>, String> {
        self.registry.search_packages(query)
    }

    pub fn outdated(&self) -> Result<Vec<(String, String, String)>, String> {
        let lockfile = self.load_lockfile()?;
        let index = self.registry.fetch_package_index()?;
        let mut outdated = Vec::new();

        for pkg in &lockfile.package {
            if let Some(versions) = index.get(&pkg.name) {
                let latest = versions
                    .iter()
                    .filter(|v| !v.yanked)
                    .filter_map(|v| semver::Version::parse(&v.version).ok().map(|ver| (ver, v)))
                    .max_by_key(|(ver, _)| ver.clone())
                    .map(|(_, v)| v);

                if let Some(latest) = latest
                    && latest.version != pkg.version
                {
                    outdated.push((
                        pkg.name.clone(),
                        pkg.version.clone(),
                        latest.version.clone(),
                    ));
                }
            }
        }
        Ok(outdated)
    }

    pub fn update(&self) -> Result<(), String> {
        // Update all dependencies to their latest compatible versions
        let mut manifest = self.load_manifest()?;
        let _lockfile = self.load_lockfile()?;
        let index = self.registry.fetch_package_index()?;

        let mut updated = false;

        // Update regular dependencies
        for (name, dep) in &mut manifest.dependencies {
            if let Some(versions) = index.get(name) {
                let latest = versions
                    .iter()
                    .filter(|v| !v.yanked)
                    .filter_map(|v| semver::Version::parse(&v.version).ok().map(|ver| (ver, v)))
                    .max_by_key(|(ver, _)| ver.clone())
                    .map(|(_, v)| v);

                if let Some(latest) = latest {
                    let current_version = match dep {
                        Dependency::Simple(v) => v.clone(),
                        Dependency::Detailed(d) => d.version.clone(),
                    };

                    if latest.version != current_version {
                        // Check if version requirement allows update
                        if let Dependency::Simple(_) = dep {
                            *dep = Dependency::Simple(latest.version.clone());
                            updated = true;
                            println!(
                                "Updated {} v{} -> v{}",
                                name, current_version, latest.version
                            );
                        }
                    }
                }
            }
        }

        // Update dev dependencies
        for (name, dep) in &mut manifest.dev_dependencies {
            if let Some(versions) = index.get(name) {
                let latest = versions
                    .iter()
                    .filter(|v| !v.yanked)
                    .filter_map(|v| semver::Version::parse(&v.version).ok().map(|ver| (ver, v)))
                    .max_by_key(|(ver, _)| ver.clone())
                    .map(|(_, v)| v);

                if let Some(latest) = latest {
                    let current_version = match dep {
                        Dependency::Simple(v) => v.clone(),
                        Dependency::Detailed(d) => d.version.clone(),
                    };

                    if latest.version != current_version
                        && let Dependency::Simple(_) = dep
                    {
                        *dep = Dependency::Simple(latest.version.clone());
                        updated = true;
                        println!(
                            "Updated {} v{} -> v{} (dev)",
                            name, current_version, latest.version
                        );
                    }
                }
            }
        }

        if updated {
            self.save_manifest(&manifest)?;
            self.install()?;
            println!("Dependencies updated successfully");
        } else {
            println!("All dependencies are up to date");
        }

        Ok(())
    }

    /// Runs a named script from `nect.toml` through the system shell.
    ///
    /// The script's text comes from the manifest, so it is repository-controlled:
    /// running one executes whatever that repository says. Two things follow.
    ///
    /// * Nothing here happens implicitly. `install` does not run scripts, so
    ///   fetching a dependency cannot execute code — only an explicit
    ///   `nect pkg run <name>` does.
    /// * The exact text is written to stderr first, so the command being run is
    ///   visible in a log and not lost when stdout is piped somewhere. It is not
    ///   written to stdout because that is where a build's real output belongs.
    ///
    /// See `docs/security.md` for the trust boundary this sits inside.
    pub fn run_script(&self, script_name: &str) -> Result<(), String> {
        let manifest = self.load_manifest()?;

        let script = manifest
            .scripts
            .get(script_name)
            .ok_or_else(|| format!("script '{}' not found", script_name))?;

        eprintln!("Running script '{}': {}", script_name, script);

        // Execute the script using the system shell
        let status = if cfg!(target_os = "windows") {
            std::process::Command::new("cmd")
                .args(["/C", script])
                .status()
        } else {
            std::process::Command::new("sh")
                .arg("-c")
                .arg(script)
                .status()
        };

        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!(
                "script '{}' exited with status: {}",
                script_name, s
            )),
            Err(e) => Err(format!("failed to run script '{}': {}", script_name, e)),
        }
    }
}

use crate::package::manifest::{Dependency, Lockfile, Manifest};
use crate::package::registry::PackageIndexEntry;
use semver::{Version, VersionReq};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub dependencies: Vec<String>,
    pub checksum: Option<String>,
}

pub fn resolve_dependencies(
    manifest: &Manifest,
    index: &HashMap<String, Vec<PackageIndexEntry>>,
    lockfile: Option<&Lockfile>,
) -> Result<Vec<ResolvedPackage>, String> {
    let mut resolver = DependencyResolver::new(index, lockfile);
    resolver.resolve(manifest)
}

struct DependencyResolver<'a> {
    index: &'a HashMap<String, Vec<PackageIndexEntry>>,
    lockfile: Option<&'a Lockfile>,
    resolved: HashMap<String, ResolvedPackage>,
    visited: HashSet<String>,
    in_progress: HashSet<String>,
}

impl<'a> DependencyResolver<'a> {
    fn new(index: &'a HashMap<String, Vec<PackageIndexEntry>>, lockfile: Option<&'a Lockfile>) -> Self {
        Self {
            index,
            lockfile,
            resolved: HashMap::new(),
            visited: HashSet::new(),
            in_progress: HashSet::new(),
        }
    }

    fn resolve(&mut self, manifest: &Manifest) -> Result<Vec<ResolvedPackage>, String> {
        let mut all_deps = HashMap::new();
        
        for (name, dep) in &manifest.dependencies {
            all_deps.insert(name.clone(), dep.clone());
        }
        for (name, dep) in &manifest.dev_dependencies {
            all_deps.insert(name.clone(), dep.clone());
        }

        for (name, dep) in all_deps {
            self.resolve_package(&name, &dep)?;
        }

        let mut result: Vec<ResolvedPackage> = self.resolved.values().cloned().collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
    }

    fn resolve_package(&mut self, name: &str, dep: &Dependency) -> Result<String, String> {
        if self.resolved.contains_key(name) {
            return Ok(self.resolved[name].version.clone());
        }

        if self.in_progress.contains(name) {
            return Err(format!("circular dependency detected: {}", name));
        }

        self.in_progress.insert(name.to_string());

        let version_req = self.parse_dependency(dep)?;
        let locked_version = self.get_locked_version(name);

        let (version, dependencies, checksum) = {
            let entry = self.select_version(name, &version_req, locked_version.as_deref())?;
            (entry.version.clone(), entry.dependencies.keys().cloned().collect::<Vec<_>>(), entry.checksum.clone())
        };
        
        let resolved = ResolvedPackage {
            name: name.to_string(),
            version: version.clone(),
            dependencies,
            checksum: Some(checksum),
        };

        self.resolved.insert(name.to_string(), resolved);
        self.in_progress.remove(name);
        self.visited.insert(name.to_string());

        for dep_name in &self.resolved[name].dependencies.clone() {
            let dep_req = Dependency::Simple(dep_name.clone());
            self.resolve_package(dep_name, &dep_req)?;
        }

        Ok(version)
    }

    fn parse_dependency(&self, dep: &Dependency) -> Result<VersionReq, String> {
        match dep {
            Dependency::Simple(v) => VersionReq::parse(v)
                .map_err(|e| format!("invalid version requirement '{}': {}", v, e)),
            Dependency::Detailed(d) => VersionReq::parse(&d.version)
                .map_err(|e| format!("invalid version requirement '{}': {}", d.version, e)),
        }
    }

    fn get_locked_version(&self, name: &str) -> Option<String> {
        self.lockfile.and_then(|lf| {
            lf.package.iter().find(|p| p.name == name).map(|p| p.version.clone())
        })
    }

    fn select_version(
        &self,
        name: &str,
        req: &VersionReq,
        locked: Option<&str>,
    ) -> Result<&PackageIndexEntry, String> {
        let versions = self.index.get(name)
            .ok_or_else(|| format!("package '{}' not found in registry", name))?;

        let mut candidates: Vec<&PackageIndexEntry> = versions
            .iter()
            .filter(|v| {
                !v.yanked && Version::parse(&v.version).map(|ver| req.matches(&ver)).unwrap_or(false)
            })
            .collect();

        if candidates.is_empty() {
            return Err(format!("no matching version found for '{}' matching '{}'", name, req));
        }

        candidates.sort_by(|a, b| {
            Version::parse(&b.version).unwrap().cmp(&Version::parse(&a.version).unwrap())
        });

        if let Some(locked_ver) = locked {
            if let Some(v) = candidates.iter().find(|v| v.version == locked_ver) {
                return Ok(*v);
            }
        }

        Ok(candidates[0])
    }
}

pub fn compute_content_hash(manifest: &Manifest, lockfile: Option<&Lockfile>) -> String {
    use sha2::{Digest, Sha256};
    
    let mut hasher = Sha256::new();
    hasher.update(manifest.package.name.as_bytes());
    hasher.update(manifest.package.version.as_bytes());
    
    for (name, dep) in &manifest.dependencies {
        hasher.update(name.as_bytes());
        match dep {
            Dependency::Simple(v) => hasher.update(v.as_bytes()),
            Dependency::Detailed(d) => hasher.update(d.version.as_bytes()),
        }
    }
    
    for (name, dep) in &manifest.dev_dependencies {
        hasher.update(name.as_bytes());
        match dep {
            Dependency::Simple(v) => hasher.update(v.as_bytes()),
            Dependency::Detailed(d) => hasher.update(d.version.as_bytes()),
        }
    }

    if let Some(lf) = lockfile {
        for pkg in &lf.package {
            hasher.update(pkg.name.as_bytes());
            hasher.update(pkg.version.as_bytes());
        }
    }

    format!("{:x}", hasher.finalize())
}
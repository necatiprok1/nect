use crate::package::manifest::{LockPackage, Lockfile, Manifest, PackageSource};
use crate::package::resolver::resolve_dependencies;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_REGISTRY: &str = "https://registry.nect-lang.org";
const NECT_DIR: &str = ".nect";

#[derive(Debug, Clone)]
pub struct RegistryClient {
    registry_url: String,
    cache_dir: PathBuf,
}

impl RegistryClient {
    pub fn new(registry_url: Option<String>) -> Self {
        let registry_url = registry_url.unwrap_or_else(|| DEFAULT_REGISTRY.to_string());
        let cache_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(NECT_DIR)
            .join("cache");
        Self { registry_url, cache_dir }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn package_cache_dir(&self, name: &str, version: &str) -> PathBuf {
        self.cache_dir.join("packages").join(name).join(version)
    }

    pub fn fetch_package_index(&self) -> Result<HashMap<String, Vec<PackageIndexEntry>>, String> {
        let index_url = format!("{}/index.json", self.registry_url.trim_end_matches('/'));
        let response = reqwest::blocking::get(&index_url)
            .map_err(|e| format!("failed to fetch package index: {}", e))?;
        let index: HashMap<String, Vec<PackageIndexEntry>> = response
            .json()
            .map_err(|e| format!("failed to parse package index: {}", e))?;
        Ok(index)
    }

    pub fn publish_package(&self, manifest: &Manifest, tarball_path: &Path) -> Result<(), String> {
        let publish_url = format!("{}/api/v1/packages", self.registry_url.trim_end_matches('/'));
        
        let mut form = reqwest::blocking::multipart::Form::new()
            .text("name", manifest.package.name.clone())
            .text("version", manifest.package.version.clone())
            .text("description", manifest.package.description.clone())
            .text("license", manifest.package.license.clone())
            .text("repository", manifest.package.repository.clone())
            .text("homepage", manifest.package.homepage.clone());
        
        for keyword in &manifest.package.keywords {
            form = form.text("keywords", keyword.clone());
        }
        
        for category in &manifest.package.categories {
            form = form.text("categories", category.clone());
        }

        let file = fs::read(tarball_path)
            .map_err(|e| format!("failed to read tarball: {}", e))?;
        form = form.part(
            "package",
            reqwest::blocking::multipart::Part::bytes(file)
                .file_name("package.tar.gz")
                .mime_str("application/gzip")
                .map_err(|e| format!("invalid mime type: {}", e))?,
        );

        let client = reqwest::blocking::Client::new();
        let response = client
            .post(&publish_url)
            .multipart(form)
            .send()
            .map_err(|e| format!("failed to publish package: {}", e))?;

        if !response.status().is_success() {
            let error = response.text().unwrap_or_default();
            return Err(format!("publish failed: {}", error));
        }

        Ok(())
    }

    pub fn download_package(&self, name: &str, version: &str) -> Result<PathBuf, String> {
        let cache_path = self.package_cache_dir(name, version);
        
        if cache_path.join("package.nct").exists() {
            return Ok(cache_path);
        }

        let download_url = format!(
            "{}/api/v1/packages/{}/{}/download",
            self.registry_url.trim_end_matches('/'),
            name,
            version
        );

        let response = reqwest::blocking::get(&download_url)
            .map_err(|e| format!("failed to download package: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("download failed: {}", response.status()));
        }

        fs::create_dir_all(&cache_path)
            .map_err(|e| format!("failed to create cache dir: {}", e))?;

        let tarball_path = cache_path.join("package.tar.gz");
        let bytes = response
            .bytes()
            .map_err(|e| format!("failed to read response: {}", e))?;
        fs::write(&tarball_path, bytes)
            .map_err(|e| format!("failed to write tarball: {}", e))?;

        self.extract_tarball(&tarball_path, &cache_path)?;
        fs::remove_file(&tarball_path).ok();

        Ok(cache_path)
    }

    fn extract_tarball(&self, tarball_path: &Path, dest: &Path) -> Result<(), String> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        let file = fs::File::open(tarball_path)
            .map_err(|e| format!("failed to open tarball: {}", e))?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);
        archive
            .unpack(dest)
            .map_err(|e| format!("failed to extract tarball: {}", e))?;
        Ok(())
    }

    pub fn create_tarball(&self, package_dir: &Path, output_path: &Path) -> Result<(), String> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use tar::Builder;

        let file = fs::File::create(output_path)
            .map_err(|e| format!("failed to create tarball: {}", e))?;
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);

        self.add_dir_to_tarball(&mut builder, package_dir, "")?;

        builder
            .into_inner()
            .map_err(|e| format!("failed to finish tarball: {}", e))?
            .finish()
            .map_err(|e| format!("failed to finish compression: {}", e))?;

        Ok(())
    }

    fn add_dir_to_tarball(
        &self,
        builder: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>,
        dir: &Path,
        prefix: &str,
    ) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| format!("failed to read dir: {}", e))? {
            let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            
            if name_str.starts_with('.') || name_str == "target" || name_str == "nect.lock" {
                continue;
            }

            let tar_path = if prefix.is_empty() {
                name_str.to_string()
            } else {
                format!("{}/{}", prefix, name_str)
            };

            if path.is_dir() {
                self.add_dir_to_tarball(builder, &path, &tar_path)?;
            } else {
                let mut file = fs::File::open(&path)
                    .map_err(|e| format!("failed to open file: {}", e))?;
                let mut header = tar::Header::new_gnu();
                header.set_path(&tar_path)
                    .map_err(|e| format!("failed to set tar path: {}", e))?;
                header.set_size(file.metadata().map_err(|e| format!("failed to get metadata: {}", e))?.len());
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append(&header, &mut file)
                    .map_err(|e| format!("failed to append to tarball: {}", e))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageIndexEntry {
    pub version: String,
    pub yanked: bool,
    pub dependencies: HashMap<String, String>,
    pub checksum: String,
}

impl RegistryClient {
    pub fn resolve_dependencies(
        &self,
        manifest: &Manifest,
        lockfile: Option<&Lockfile>,
    ) -> Result<Lockfile, String> {
        let index = self.fetch_package_index()?;
        let resolved = resolve_dependencies(manifest, &index, lockfile)?;
        
        let mut lockfile = Lockfile::new();
        lockfile.package = resolved
            .into_iter()
            .map(|p| LockPackage {
                name: p.name,
                version: p.version,
                source: PackageSource::Registry {
                    registry: self.registry_url.clone(),
                },
                dependencies: p.dependencies,
                checksum: p.checksum,
            })
            .collect();
        
        Ok(lockfile)
    }
}

impl Default for RegistryClient {
    fn default() -> Self {
        Self::new(None)
    }
}
use crate::package::manifest::{LockPackage, Lockfile, Manifest, PackageSource};
use crate::package::resolver::resolve_dependencies;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
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
        Self {
            registry_url,
            cache_dir,
        }
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

    pub fn search_packages(&self, query: &str) -> Result<Vec<PackageSearchResult>, String> {
        let encoded = percent_encode(query);
        let search_url = format!(
            "{}/api/v1/search?q={}",
            self.registry_url.trim_end_matches('/'),
            encoded
        );
        let response = reqwest::blocking::get(&search_url)
            .map_err(|e| format!("failed to search packages: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("search failed: {}", response.status()));
        }

        let result: PackageSearchResponse = response
            .json()
            .map_err(|e| format!("failed to parse search results: {}", e))?;
        Ok(result.results)
    }

    pub fn publish_package(&self, manifest: &Manifest, tarball_path: &Path) -> Result<(), String> {
        let publish_url = format!(
            "{}/api/v1/packages",
            self.registry_url.trim_end_matches('/')
        );

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

        let file = fs::read(tarball_path).map_err(|e| format!("failed to read tarball: {}", e))?;
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

    pub fn download_package(
        &self,
        name: &str,
        version: &str,
        expected_checksum: Option<&str>,
    ) -> Result<PathBuf, String> {
        let cache_path = self.package_cache_dir(name, version);

        if cache_path.join("package.nct").exists() {
            if let Some(expected) = expected_checksum {
                if !self.verify_package_checksum(&cache_path, expected)? {
                    fs::remove_dir_all(&cache_path).ok();
                } else {
                    return Ok(cache_path);
                }
            } else {
                return Ok(cache_path);
            }
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
        fs::write(&tarball_path, bytes).map_err(|e| format!("failed to write tarball: {}", e))?;

        if let Some(expected) = expected_checksum
            && !self.verify_tarball_checksum(&tarball_path, expected)?
        {
            let _ = fs::remove_file(&tarball_path);
            return Err(format!(
                "checksum verification failed for {} v{}",
                name, version
            ));
        }

        self.extract_tarball(&tarball_path, &cache_path)?;

        if let Some(expected) = expected_checksum {
            let checksum_file = cache_path.join(".nect-checksum");
            fs::write(&checksum_file, expected)
                .map_err(|e| format!("failed to write checksum file: {}", e))?;
        }

        fs::remove_file(&tarball_path).ok();

        Ok(cache_path)
    }

    fn verify_package_checksum(&self, cache_path: &Path, expected: &str) -> Result<bool, String> {
        let checksum_file = cache_path.join(".nect-checksum");
        if !checksum_file.exists() {
            return Ok(false);
        }
        let stored = fs::read_to_string(&checksum_file)
            .map_err(|e| format!("failed to read checksum file: {}", e))?;
        Ok(stored.trim() == expected)
    }

    fn verify_tarball_checksum(&self, tarball_path: &Path, expected: &str) -> Result<bool, String> {
        use sha2::{Digest, Sha256};
        let mut file =
            fs::File::open(tarball_path).map_err(|e| format!("failed to open tarball: {}", e))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| format!("failed to read tarball: {}", e))?;
        let mut hasher = Sha256::new();
        hasher.update(&buf);
        let calculated = format!("{:x}", hasher.finalize());
        Ok(calculated == expected)
    }

    fn extract_tarball(&self, tarball_path: &Path, dest: &Path) -> Result<(), String> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        let file =
            fs::File::open(tarball_path).map_err(|e| format!("failed to open tarball: {}", e))?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);
        archive
            .unpack(dest)
            .map_err(|e| format!("failed to extract tarball: {}", e))?;
        Ok(())
    }

    pub fn create_tarball(&self, package_dir: &Path, output_path: &Path) -> Result<(), String> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
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
                let mut file =
                    fs::File::open(&path).map_err(|e| format!("failed to open file: {}", e))?;
                let mut header = tar::Header::new_gnu();
                header
                    .set_path(&tar_path)
                    .map_err(|e| format!("failed to set tar path: {}", e))?;
                header.set_size(
                    file.metadata()
                        .map_err(|e| format!("failed to get metadata: {}", e))?
                        .len(),
                );
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSearchResult {
    pub name: String,
    pub version: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub downloads: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSearchResponse {
    pub results: Vec<PackageSearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAdvisory {
    pub id: String,
    pub package_name: String,
    pub version_range: String,
    pub severity: String,
    pub title: String,
    pub description: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditResponse {
    pub vulnerabilities: Vec<Vulnerability>,
    pub summary: AuditSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vulnerability {
    pub package_name: String,
    pub installed_version: String,
    pub version_range: String,
    pub severity: String,
    pub title: String,
    pub advisory_id: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSummary {
    pub total: u32,
    pub high: u32,
    pub critical: u32,
    pub warning: u32,
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

    pub fn audit_dependencies(&self, lockfile: &Lockfile) -> Result<AuditResponse, String> {
        let audit_url = format!("{}/api/v1/audit", self.registry_url.trim_end_matches('/'));

        #[derive(serde::Serialize)]
        struct AuditRequest {
            packages: Vec<AuditPackage>,
        }
        #[derive(serde::Serialize)]
        struct AuditPackage {
            name: String,
            version: String,
        }

        let packages: Vec<AuditPackage> = lockfile
            .package
            .iter()
            .map(|p| AuditPackage {
                name: p.name.clone(),
                version: p.version.clone(),
            })
            .collect();

        let request = AuditRequest { packages };
        let response = reqwest::blocking::Client::new()
            .post(&audit_url)
            .json(&request)
            .send()
            .map_err(|e| format!("failed to check for vulnerabilities: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("audit request failed: {}", response.status()));
        }

        let audit: AuditResponse = response
            .json()
            .map_err(|e| format!("failed to parse audit response: {}", e))?;
        Ok(audit)
    }
}

impl Default for RegistryClient {
    fn default() -> Self {
        Self::new(None)
    }
}

fn percent_encode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push('%');
                result.push_str(&format!("{:02X}", byte));
            }
        }
    }
    result
}

pub mod manifest;
pub mod manager;
pub mod registry;
pub mod resolver;

pub use manifest::{Dependency, Lockfile, LockPackage, Manifest, Package, PackageSource};
pub use manager::PackageManager;
pub use registry::RegistryClient;
use std::path::PathBuf;

pub fn create_package_manager(project_dir: PathBuf, registry_url: Option<String>) -> PackageManager {
    PackageManager::new(project_dir, registry_url)
}
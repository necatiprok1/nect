pub mod manager;
pub mod manifest;
pub mod registry;
pub mod resolver;

pub use manager::PackageManager;
pub use manifest::{Dependency, LockPackage, Lockfile, Manifest, Package, PackageSource};
pub use registry::RegistryClient;
use std::path::PathBuf;

pub fn create_package_manager(
    project_dir: PathBuf,
    registry_url: Option<String>,
) -> PackageManager {
    PackageManager::new(project_dir, registry_url)
}

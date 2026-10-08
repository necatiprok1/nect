use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub package: Package,
    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,
    #[serde(default)]
    pub dev_dependencies: HashMap<String, Dependency>,
    #[serde(default)]
    pub scripts: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub repository: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Dependency {
    Simple(String),
    Detailed(DependencyDetail),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyDetail {
    pub version: String,
    #[serde(default)]
    pub git: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub registry: Option<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub features: Vec<String>,
}

impl Manifest {
    pub fn new(name: String, version: String) -> Self {
        Self {
            package: Package {
                name,
                version,
                description: String::new(),
                authors: Vec::new(),
                license: String::new(),
                repository: String::new(),
                homepage: String::new(),
                keywords: Vec::new(),
                categories: Vec::new(),
            },
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            scripts: HashMap::new(),
        }
    }

    pub fn from_toml(toml: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml)
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    pub fn add_dependency(&mut self, name: String, version: String) {
        self.dependencies.insert(name, Dependency::Simple(version));
    }

    pub fn add_dev_dependency(&mut self, name: String, version: String) {
        self.dev_dependencies
            .insert(name, Dependency::Simple(version));
    }

    pub fn remove_dependency(&mut self, name: &str) {
        self.dependencies.remove(name);
        self.dev_dependencies.remove(name);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Lockfile {
    pub package: Vec<LockPackage>,
    pub metadata: LockMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockPackage {
    pub name: String,
    pub version: String,
    pub source: PackageSource,
    pub dependencies: Vec<String>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum PackageSource {
    Registry { registry: String },
    Git { url: String, rev: String },
    Path { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockMetadata {
    pub content_hash: String,
    pub version: u32,
}

impl Lockfile {
    pub fn new() -> Self {
        Self {
            package: Vec::new(),
            metadata: LockMetadata {
                content_hash: String::new(),
                version: 1,
            },
        }
    }

    pub fn from_toml(toml: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml)
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self::new()
    }
}

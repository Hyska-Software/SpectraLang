use crate::discovery;
use crate::project::ProjectSourceEntry;
use crate::release_channel::{cli_compatibility_level, ReleaseMetadata};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use toml_edit::{value, DocumentMut, Item, Table};

const MANIFEST_NAMES: &[&str] = &["spectra.toml", "Spectra.toml"];
const LOCKFILE_NAME: &str = "spectra.lock";

#[derive(Clone, Debug)]
pub enum PackageCommand {
    Lock,
    Build,
    Check,
    Run,
    Test(PackageTestOptions),
    Bench,
    Doc,
    Add {
        name: String,
        version: Option<String>,
        path: Option<PathBuf>,
        registry: Option<PathBuf>,
        git: Option<String>,
        tag: Option<String>,
        rev: Option<String>,
        branch: Option<String>,
        catalog: Option<PathBuf>,
        allow_floating_git: bool,
    },
    Update,
    Fetch {
        offline: bool,
    },
    Search {
        query: String,
        catalog: Option<PathBuf>,
    },
    Info {
        name: String,
        catalog: Option<PathBuf>,
    },
    Versions {
        name: String,
        catalog: Option<PathBuf>,
    },
    Tree,
    Register {
        git: String,
        tag: Option<String>,
        rev: Option<String>,
        branch: Option<String>,
        catalog: PathBuf,
    },
    PublishMetadata {
        out: PathBuf,
        git: Option<String>,
        tag: Option<String>,
        rev: Option<String>,
        branch: Option<String>,
    },
    Catalog(CatalogCommand),
    Publish {
        registry: PathBuf,
    },
}

#[derive(Clone, Debug)]
pub enum CatalogCommand {
    Add { name: String, source: String },
    List,
    Sync { offline: bool, locked: bool },
    Remove { name: String },
}

#[derive(Clone, Debug, Default)]
pub struct PackageTestOptions {
    pub filter: Option<String>,
    pub list: bool,
    pub json: bool,
}

#[derive(Clone, Debug)]
pub struct PackageInvocation {
    pub root: PathBuf,
    pub command: PackageCommand,
    pub offline: bool,
    pub locked: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ResolveOptions {
    pub offline: bool,
}

#[derive(Clone, Debug)]
pub struct ResolvedWorkspace {
    pub root: PathBuf,
    pub root_name: String,
    pub packages: Vec<ResolvedPackage>,
}

impl ResolvedWorkspace {
    #[allow(dead_code)]
    pub fn source_entries(&self) -> Vec<PathBuf> {
        let mut entries = Vec::new();
        let mut seen = BTreeSet::new();

        for package in &self.packages {
            for src in &package.src_dirs {
                if seen.insert(src.clone()) {
                    entries.push(src.clone());
                }
            }
        }

        entries
    }

    pub fn source_entries_with_origins(&self) -> Vec<ProjectSourceEntry> {
        let mut entries = Vec::new();
        let mut seen = BTreeSet::new();

        for package in &self.packages {
            for src in &package.src_dirs {
                if seen.insert(src.clone()) {
                    entries.push(ProjectSourceEntry {
                        path: src.clone(),
                        package_name: Some(package.name.clone()),
                        package_root: Some(package.root.clone()),
                    });
                }
            }
        }

        entries
    }

    pub fn root_package_name(&self) -> Option<String> {
        Some(self.root_name.clone())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub release: ReleaseMetadata,
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub src_dirs: Vec<PathBuf>,
    pub entry: Option<PathBuf>,
    pub dependencies: BTreeMap<String, ResolvedDependency>,
    pub manifest_hash: String,
    pub source: PackageSource,
    pub checksum: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedDependency {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub source: PackageSource,
}

#[derive(Clone, Debug, Serialize)]
pub enum PackageSource {
    Path {
        path: PathBuf,
    },
    Registry {
        path: PathBuf,
    },
    Git {
        url: String,
        requested: String,
        resolved: String,
        floating: bool,
    },
}

#[derive(Debug)]
pub enum PackageError {
    Io {
        path: PathBuf,
        error: io::Error,
    },
    Parse {
        path: PathBuf,
        error: toml::de::Error,
    },
    EditParse {
        path: PathBuf,
        message: String,
    },
    Serialize(toml::ser::Error),
    MissingManifest(PathBuf),
    MissingPackage(String),
    DuplicatePackage {
        name: String,
        first: PathBuf,
        second: PathBuf,
    },
    IncompatiblePackage {
        name: String,
        required: String,
        found: String,
        source: String,
    },
    ConflictingCatalogPackage {
        name: String,
        version: String,
        first: String,
        second: String,
    },
    DependencyCycle(Vec<String>),
    HostNotAllowed {
        host: String,
        locator: String,
    },
    UnsafePath {
        path: PathBuf,
        reason: String,
    },
    AtomicWrite {
        path: PathBuf,
        message: String,
    },
    CacheCorrupt {
        package: String,
        path: PathBuf,
        reason: String,
    },
    CatalogSync {
        name: String,
        source: String,
        path: PathBuf,
        reason: String,
    },
    LockfileMissing(PathBuf),
    LockfileTampered {
        path: PathBuf,
        package: String,
        field: String,
    },
    InvalidManifest {
        path: PathBuf,
        message: String,
    },
    // APPEND-ONLY (RemoteRegistry): typed failures for the HTTP(S) registry protocol.
    RemoteHttp {
        status: u16,
        url: String,
    },
    RemoteTransport {
        url: String,
        message: String,
    },
    Registry(String),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageError::Io { path, error } => {
                write!(f, "failed to access '{}': {}", path.display(), error)
            }
            PackageError::Parse { path, error } => {
                write!(f, "failed to parse '{}': {}", path.display(), error)
            }
            PackageError::EditParse { path, message } => {
                write!(f, "failed to edit '{}': {}", path.display(), message)
            }
            PackageError::Serialize(error) => write!(f, "failed to serialize lockfile: {}", error),
            PackageError::MissingManifest(path) => {
                write!(f, "no spectra.toml manifest found at '{}'", path.display())
            }
            PackageError::MissingPackage(name) => write!(f, "package '{}' was not found", name),
            PackageError::DuplicatePackage {
                name,
                first,
                second,
            } => write!(
                f,
                "package '{}' appears more than once in the workspace: '{}' and '{}'",
                name,
                first.display(),
                second.display()
            ),
            PackageError::IncompatiblePackage {
                name,
                required,
                found,
                source,
            } => write!(
                f,
                "package '{}' is incompatible with CLI compatibility '{}': found '{}' at {}",
                name, required, found, source
            ),
            PackageError::ConflictingCatalogPackage {
                name,
                version,
                first,
                second,
            } => write!(
                f,
                "catalog entries for '{}' version '{}' conflict: '{}' and '{}'",
                name, version, first, second
            ),
            PackageError::DependencyCycle(chain) => {
                write!(
                    f,
                    "cyclic package dependency detected: {}",
                    chain.join(" -> ")
                )
            }
            PackageError::HostNotAllowed { host, locator } => write!(
                f,
                "package host '{}' is not allowed for locator '{}'",
                host, locator
            ),
            PackageError::UnsafePath { path, reason } => {
                write!(f, "unsafe package path '{}': {}", path.display(), reason)
            }
            PackageError::AtomicWrite { path, message } => write!(
                f,
                "atomic package write for '{}' failed: {}",
                path.display(),
                message
            ),
            PackageError::CacheCorrupt {
                package,
                path,
                reason,
            } => write!(
                f,
                "package cache for '{}' at '{}' is corrupt: {}",
                package,
                path.display(),
                reason
            ),
            PackageError::CatalogSync {
                name,
                source,
                path,
                reason,
            } => write!(
                f,
                "catalog '{}' from '{}' at '{}' could not be synchronized: {}",
                name,
                source,
                path.display(),
                reason
            ),
            PackageError::LockfileMissing(path) => write!(
                f,
                "locked package operation requires lockfile '{}', but it does not exist",
                path.display()
            ),
            PackageError::LockfileTampered {
                path,
                package,
                field,
            } => write!(
                f,
                "lockfile '{}' differs for package '{}' in field '{}'",
                path.display(),
                package,
                field
            ),
            PackageError::InvalidManifest { path, message } => {
                write!(f, "invalid manifest '{}': {}", path.display(), message)
            }
            PackageError::Registry(message) => write!(f, "registry error: {}", message),
            // APPEND-ONLY (RemoteRegistry): typed remote-registry failures.
            PackageError::RemoteHttp { status, url } => write!(
                f,
                "remote registry request to '{}' failed with HTTP status {}",
                url, status
            ),
            PackageError::RemoteTransport { url, message } => write!(
                f,
                "remote registry request to '{}' failed: {}",
                url, message
            ),
        }
    }
}

impl std::error::Error for PackageError {}

#[derive(Debug, Deserialize)]
struct Manifest {
    project: ProjectSection,
    #[serde(default)]
    release: ReleaseMetadata,
    #[serde(default)]
    workspace: WorkspaceSection,
    #[serde(default)]
    package: PackageSection,
    #[serde(default)]
    dependencies: BTreeMap<String, DependencySpec>,
    // APPEND-ONLY (RemoteRegistry): optional [registry] section with a remote base URL.
    #[serde(default)]
    registry: Option<RegistryConfig>,
}

#[derive(Debug, Deserialize)]
struct ProjectSection {
    name: String,
    #[serde(default = "default_version")]
    version: String,
    entry: Option<String>,
    #[serde(default)]
    src_dirs: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct WorkspaceSection {
    #[serde(default)]
    members: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PackageSection {
    #[serde(default)]
    catalogs: BTreeMap<String, String>,
}

// APPEND-ONLY (RemoteRegistry): `[registry]` manifest section.
#[derive(Debug, Default, Deserialize)]
struct RegistryConfig {
    #[serde(default)]
    remote: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum DependencySpec {
    Version(String),
    Detailed {
        version: Option<String>,
        path: Option<String>,
        registry: Option<String>,
        git: Option<String>,
        tag: Option<String>,
        rev: Option<String>,
        branch: Option<String>,
        #[serde(
            rename = "allow-floating-git",
            alias = "allow_floating_git",
            default
        )]
        allow_floating_git: Option<bool>,
        checksum: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Lockfile {
    version: u32,
    root: String,
    packages: Vec<LockPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LockPackage {
    name: String,
    version: String,
    channel: String,
    compatibility: String,
    deprecated_since: Option<String>,
    migration: Option<String>,
    source: String,
    source_kind: String,
    checksum: String,
    #[serde(default)]
    git_url: Option<String>,
    #[serde(default)]
    git_warning: Option<String>,
    #[serde(default)]
    git_ref: Option<String>,
    #[serde(default)]
    resolved_rev: Option<String>,
    manifest_hash: String,
    dependencies: Vec<LockDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LockDependency {
    name: String,
    version: String,
    source: String,
    source_kind: String,
}

#[derive(Serialize, Deserialize)]
struct RegistryMetadata {
    name: String,
    version: String,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    compatibility: String,
    #[serde(default)]
    deprecated_since: Option<String>,
    #[serde(default)]
    migration: Option<String>,
    checksum: String,
    #[serde(default)]
    source_path: String,
    // APPEND-ONLY (RemoteRegistry): payload file list for HTTP installs.
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CatalogIndex {
    #[serde(default = "catalog_schema")]
    schema: String,
    #[serde(default)]
    packages: Vec<CatalogPackage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CatalogPackage {
    name: String,
    version: String,
    git: String,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    rev: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    resolved_rev: Option<String>,
    #[serde(default)]
    checksum: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    compatibility: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    modules: Vec<String>,
    #[serde(default)]
    owner: String,
    #[serde(skip)]
    origin: String,
}

struct InstalledGitPackage {
    name: String,
    version: String,
    path: PathBuf,
    resolved: String,
    checksum: String,
}

// APPEND-ONLY (GitPinned): Debug needed by package_remote.rs expect_err tests.
#[derive(Debug)]
pub(crate) struct InstalledRegistryPackage {
    canonical_name: String,
    version: String,
    path: PathBuf,
}

include!("package_catalog.rs");
include!("package_dependencies.rs");
include!("package_install.rs");
include!("package_git.rs");
include!("package_catalog_resolution.rs");
include!("package_cache.rs");
include!("package_public_api.rs");
include!("package_remote.rs");
include!("package_tests.rs");

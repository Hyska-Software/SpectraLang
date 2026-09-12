// Project configuration — loads and parses `spectra.toml`.

#![allow(dead_code)]

use crate::release_channel::ReleaseMetadata;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

/// Contents of a `spectra.toml` file.
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectConfig {
    #[serde(rename = "project")]
    pub project: ProjectSection,
    #[serde(default)]
    pub release: ReleaseMetadata,
    #[serde(default)]
    pub dependencies: toml::value::Table,
}

impl ProjectConfig {
    /// Convenience accessor — package name.
    pub fn name(&self) -> &str {
        &self.project.name
    }

    /// Resolve src directories relative to the given project root.
    pub fn src_dirs(&self, root: &Path) -> Vec<PathBuf> {
        if self.project.src_dirs.is_empty() {
            vec![root.join("src")]
        } else {
            self.project.src_dirs.iter().map(|d| root.join(d)).collect()
        }
    }

    /// Resolve the entry-point path relative to the given project root.
    /// Returns `None` when no explicit entry is configured (use src_dirs instead).
    pub fn entry(&self, root: &Path) -> Option<PathBuf> {
        self.project.entry.as_ref().map(|e| root.join(e))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectSection {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    pub entry: Option<String>,
    #[serde(default)]
    pub src_dirs: Vec<String>,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

/// Error type for project configuration loading.
#[derive(Debug)]
pub enum ConfigError {
    Io(io::Error),
    Parse(toml::de::Error),
    Release { path: PathBuf, message: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "failed to read spectra.toml: {}", e),
            ConfigError::Parse(e) => write!(f, "invalid spectra.toml: {}", e),
            ConfigError::Release { path, message } => {
                write!(
                    f,
                    "invalid release metadata in '{}': {}",
                    path.display(),
                    message
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Try to load a `spectra.toml` from `dir`.
/// Returns `None` when no file is found, `Err` on parse/IO errors.
pub fn try_load_config(dir: &Path) -> Result<Option<ProjectConfig>, ConfigError> {
    let candidate = dir.join("spectra.toml");
    if candidate.exists() {
        let text = fs::read_to_string(&candidate).map_err(ConfigError::Io)?;
        let config: ProjectConfig = toml::from_str(&text).map_err(ConfigError::Parse)?;
        config
            .release
            .validate()
            .map_err(|message| ConfigError::Release {
                path: candidate.clone(),
                message,
            })?;
        return Ok(Some(config));
    }
    Ok(None)
}

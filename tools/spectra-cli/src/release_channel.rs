use serde::{Deserialize, Serialize};
use std::fmt;

pub const DEFAULT_COMPATIBILITY_LEVEL: &str = "spectralang-0.1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ReleaseChannel {
    #[default]
    Nightly,
    Beta,
    Stable,
}

impl ReleaseChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            ReleaseChannel::Nightly => "nightly",
            ReleaseChannel::Beta => "beta",
            ReleaseChannel::Stable => "stable",
        }
    }
}

impl fmt::Display for ReleaseChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReleaseMetadata {
    #[serde(default)]
    pub channel: ReleaseChannel,
    #[serde(default = "default_compatibility_level")]
    pub compatibility: String,
    #[serde(default)]
    pub deprecated_since: Option<String>,
    #[serde(default)]
    pub migration: Option<String>,
}

impl Default for ReleaseMetadata {
    fn default() -> Self {
        Self {
            channel: ReleaseChannel::default(),
            compatibility: default_compatibility_level(),
            deprecated_since: None,
            migration: None,
        }
    }
}

impl ReleaseMetadata {
    pub fn validate(&self) -> Result<(), String> {
        if !is_valid_compatibility_level(&self.compatibility) {
            return Err(format!(
                "release.compatibility '{}' is invalid; use a non-empty ASCII identifier such as '{}'",
                self.compatibility, DEFAULT_COMPATIBILITY_LEVEL
            ));
        }

        if self.deprecated_since.as_deref().is_some_and(str::is_empty) {
            return Err("release.deprecated_since cannot be empty".to_string());
        }

        if self.deprecated_since.is_some()
            && self.migration.as_deref().unwrap_or("").trim().is_empty()
        {
            return Err(
                "release.migration is required when release.deprecated_since is set".to_string(),
            );
        }

        Ok(())
    }

    pub fn deprecation_warning(&self, package_name: &str) -> Option<String> {
        let deprecated_since = self.deprecated_since.as_deref()?;
        let migration = self
            .migration
            .as_deref()
            .unwrap_or("consult the release channel policy for migration guidance");
        Some(format!(
            "warning[release-deprecated]: package '{}' is deprecated since {}; migration: {}",
            package_name, deprecated_since, migration
        ))
    }
}

pub fn default_compatibility_level() -> String {
    DEFAULT_COMPATIBILITY_LEVEL.to_string()
}

pub fn cli_channel() -> ReleaseChannel {
    match option_env!("SPECTRA_RELEASE_CHANNEL") {
        Some("stable") => ReleaseChannel::Stable,
        Some("beta") => ReleaseChannel::Beta,
        _ => ReleaseChannel::Nightly,
    }
}

pub fn cli_compatibility_level() -> &'static str {
    option_env!("SPECTRA_COMPATIBILITY_LEVEL").unwrap_or(DEFAULT_COMPATIBILITY_LEVEL)
}

fn is_valid_compatibility_level(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_deprecation_requires_migration() {
        let metadata = ReleaseMetadata {
            deprecated_since: Some("0.2.0".to_string()),
            migration: None,
            ..ReleaseMetadata::default()
        };
        assert!(metadata.validate().is_err());

        let metadata = ReleaseMetadata {
            deprecated_since: Some("0.2.0".to_string()),
            migration: Some("Use std.tensor.v2".to_string()),
            ..ReleaseMetadata::default()
        };
        assert!(metadata.validate().is_ok());
        assert!(metadata
            .deprecation_warning("demo")
            .expect("warning")
            .contains("Use std.tensor.v2"));
    }
}

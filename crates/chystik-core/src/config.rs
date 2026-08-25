//! Versioned, fail-closed user policy shared by every frontend.
//!
//! The configuration stores only a user's never-touch paths and an explicit
//! acknowledgement of the current safety policy. It contains no scan output,
//! file contents, credentials, or telemetry. A malformed configuration is an
//! error rather than an empty policy: silently dropping exclusions would make
//! a cleanup tool less safe exactly when its state became unreadable.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const CONFIG_SCHEMA_VERSION: u32 = 1;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// User-controlled policy that applies to both GUI and CLI runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserConfig {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub exclusions: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_version: Option<String>,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            exclusions: Vec::new(),
            acknowledged_version: None,
        }
    }
}

impl UserConfig {
    /// Whether this exact safety-policy version has been acknowledged.
    pub fn acknowledges_current_version(&self) -> bool {
        self.acknowledged_version.as_deref() == Some(APP_VERSION)
    }
}

/// Persistent storage location for [`UserConfig`].
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::at(
            crate::platform::current()
                .app_paths()
                .config_dir
                .join("config.json"),
        )
    }
}

impl ConfigStore {
    /// Build a store at an explicit path. Public for deterministic tests and
    /// embeddings; normal frontends should use [`Self::default`].
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read policy, failing closed for malformed or future schemas.
    pub fn load(&self) -> Result<UserConfig, ConfigError> {
        match fs::read_to_string(&self.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.load_legacy_gui_policy()
            }
            Err(error) => Err(ConfigError::Io {
                path: self.path.clone(),
                source: error,
            }),
            Ok(text) => {
                let mut config: UserConfig =
                    serde_json::from_str(&text).map_err(|source| ConfigError::Parse {
                        path: self.path.clone(),
                        source,
                    })?;
                if config.schema_version > CONFIG_SCHEMA_VERSION {
                    return Err(ConfigError::UnsupportedSchema {
                        found: config.schema_version,
                    });
                }
                config.schema_version = CONFIG_SCHEMA_VERSION;
                config.exclusions = normalize_exclusions(config.exclusions);
                Ok(config)
            }
        }
    }

    /// Store policy as one complete JSON document. The final rename happens
    /// only after the temporary file has reached the filesystem, so a crash
    /// cannot truncate a previous valid policy into an empty exclusion list.
    pub fn save(&self, config: &UserConfig) -> Result<(), ConfigError> {
        let mut next = config.clone();
        next.schema_version = CONFIG_SCHEMA_VERSION;
        next.exclusions = normalize_exclusions(next.exclusions);
        let bytes = serde_json::to_vec_pretty(&next).map_err(ConfigError::Serialize)?;
        let parent = self.path.parent().ok_or_else(|| ConfigError::Io {
            path: self.path.clone(),
            source: std::io::Error::other("configuration path has no parent directory"),
        })?;
        fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
            path: parent.to_path_buf(),
            source,
        })?;

        let nonce = format!(
            ".config-{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let temporary = parent.join(nonce);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| ConfigError::Io {
                path: temporary.clone(),
                source,
            })?;
        file.write_all(&bytes).map_err(|source| ConfigError::Io {
            path: temporary.clone(),
            source,
        })?;
        file.write_all(b"\n").map_err(|source| ConfigError::Io {
            path: temporary.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| ConfigError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::rename(&temporary, &self.path).map_err(|source| ConfigError::Io {
            path: self.path.clone(),
            source,
        })
    }

    /// Record an explicit acknowledgement after an interactive confirmation.
    pub fn acknowledge_current_version(&self) -> Result<(), ConfigError> {
        let mut config = self.load()?;
        config.acknowledged_version = Some(APP_VERSION.to_owned());
        self.save(&config)
    }

    /// Clear only persisted policy. This writes an empty configuration rather
    /// than deleting a file, so reset cannot leave an accidental partial state.
    pub fn reset(&self) -> Result<(), ConfigError> {
        self.save(&UserConfig::default())
    }

    /// Read the GUI's pre-CLI split records when there is no unified file.
    /// This is a read-only compatibility bridge: the first GUI/CLI write
    /// persists the resulting policy as `config.json`, which then takes
    /// precedence over the legacy files forever.
    fn load_legacy_gui_policy(&self) -> Result<UserConfig, ConfigError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let exclusions: Option<LegacyExclusions> =
            read_optional_json(&parent.join("exclusions.json"))?;
        let consent: Option<LegacyConsent> = read_optional_json(&parent.join("consent.json"))?;
        Ok(UserConfig {
            exclusions: normalize_exclusions(
                exclusions.map(|record| record.paths).unwrap_or_default(),
            ),
            acknowledged_version: consent.map(|record| record.acknowledged_version),
            ..UserConfig::default()
        })
    }
}

#[derive(Deserialize)]
struct LegacyExclusions {
    #[serde(default)]
    paths: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct LegacyConsent {
    acknowledged_version: String,
}

fn read_optional_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, ConfigError> {
    match fs::read_to_string(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigError::Io {
            path: path.to_path_buf(),
            source,
        }),
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|source| ConfigError::Parse {
                path: path.to_path_buf(),
                source,
            }),
    }
}

/// Normalise the smallest list of absolute never-touch roots with identical
/// semantics: duplicates and children covered by an earlier parent vanish.
/// Existing roots are canonicalized so they compare with scanner paths; this
/// is required on Windows where canonical paths carry the `\\\\?\\` prefix.
pub fn normalize_exclusions(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let cwd = std::env::current_dir().ok();
    let mut paths: Vec<PathBuf> = paths
        .into_iter()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| {
            if path.is_absolute() {
                path
            } else if let Some(cwd) = &cwd {
                cwd.join(path)
            } else {
                path
            }
        })
        // Keep a missing exclusion intact: the user may configure a directory
        // before it exists. Once it exists, the next load canonicalizes it.
        .map(|path| std::fs::canonicalize(&path).unwrap_or(path))
        .collect();
    paths.sort();
    paths.dedup();
    paths.sort_by_key(|path| path.as_os_str().len());
    let mut kept = Vec::new();
    for path in paths {
        if !kept.iter().any(|root: &PathBuf| path.starts_with(root)) {
            kept.push(path);
        }
    }
    kept.sort();
    kept
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read or write configuration at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("configuration at {} is not valid JSON: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("configuration schema {found} is newer than this Chystik version supports")]
    UnsupportedSchema { found: u32 },
    #[error("cannot serialize configuration: {0}")]
    Serialize(serde_json::Error),
}

const fn current_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_unified_config_imports_legacy_gui_policy_without_dropping_exclusions() {
        let fixture = tempfile::tempdir().unwrap();
        let store = ConfigStore::at(fixture.path().join("config.json"));
        let excluded = fixture.path().join("never-touch");
        std::fs::write(
            fixture.path().join("exclusions.json"),
            serde_json::json!({ "paths": [excluded] }).to_string(),
        )
        .unwrap();
        std::fs::write(
            fixture.path().join("consent.json"),
            serde_json::json!({ "acknowledged_version": APP_VERSION }).to_string(),
        )
        .unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(loaded.exclusions, vec![fixture.path().join("never-touch")]);
        assert!(loaded.acknowledges_current_version());
    }

    #[test]
    fn existing_exclusion_uses_the_scanner_canonical_path_but_missing_one_is_retained() {
        let fixture = tempfile::tempdir().unwrap();
        let existing = fixture.path().join("never-touch");
        let missing = fixture.path().join("created-later");
        std::fs::create_dir_all(&existing).unwrap();

        let normalized = normalize_exclusions(vec![existing.clone(), missing.clone()]);

        assert!(normalized.contains(&std::fs::canonicalize(&existing).unwrap()));
        assert!(normalized.contains(&missing));
    }
}

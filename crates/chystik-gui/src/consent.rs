//! First-run risk acknowledgement.
//!
//! Chystik deletes things. Before it will scan anything, the user has to see
//! what that means and say so explicitly. The acknowledgement is recorded
//! per application version, so a release that changes the safety model can
//! ask again by bumping the version it stores.
//!
//! Deliberately NOT stored in the app's own scan territory: it lives in
//! the platform-owned app config directory, and the deletion guard refuses `.config`
//! anyway, so Chystik can never propose deleting its own consent record.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub(crate) struct Consent {
    /// Version of Chystik the user acknowledged.
    pub acknowledged_version: String,
}

/// The core platform facade selects the config location for this host.
fn consent_path() -> PathBuf {
    chystik_core::platform::current()
        .app_paths()
        .config_dir
        .join("consent.json")
}

/// True when this exact version has already been acknowledged.
///
/// Any failure to read — missing file, malformed JSON, unreadable directory —
/// is treated as "not acknowledged". Failing towards *showing* the warning is
/// the only safe direction for a tool that deletes.
pub(crate) fn is_acknowledged() -> bool {
    let path = consent_path();
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    serde_json::from_str::<Consent>(&text)
        .map(|c| c.acknowledged_version == APP_VERSION)
        .unwrap_or(false)
}

/// Record the acknowledgement. A write failure is reported but not fatal:
/// the user simply sees the dialog again next launch, which is harmless.
pub(crate) fn acknowledge() {
    let path = consent_path();
    let record = Consent {
        acknowledged_version: APP_VERSION.to_string(),
    };
    let write = path
        .parent()
        .ok_or_else(|| std::io::Error::other("consent path has no parent"))
        .and_then(std::fs::create_dir_all)
        .and_then(|()| {
            let json = serde_json::to_string_pretty(&record)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            std::fs::write(&path, json)
        });
    if let Err(e) = write {
        eprintln!(
            "[chystik] could not record consent at {}: {e}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialising and reading back must round-trip, or the dialog reappears
    /// on every launch.
    #[test]
    fn consent_round_trips_through_json() {
        let record = Consent {
            acknowledged_version: APP_VERSION.to_string(),
        };
        let json = serde_json::to_string(&record).unwrap();
        let parsed: Consent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, record);
    }

    #[test]
    fn a_different_version_is_not_acknowledged() {
        let older = Consent {
            acknowledged_version: "0.0.1-old".to_string(),
        };
        assert_ne!(older.acknowledged_version, APP_VERSION);
    }

    /// Garbage on disk must read as "not acknowledged" — never as consent.
    #[test]
    fn malformed_records_do_not_count_as_consent() {
        for bad in [
            "",
            "{}",
            "not json at all",
            r#"{"acknowledged_version": 7}"#,
        ] {
            let parsed = serde_json::from_str::<Consent>(bad);
            assert!(
                parsed.is_err() || parsed.unwrap().acknowledged_version != APP_VERSION,
                "{bad:?} must not be read as consent"
            );
        }
    }

    #[test]
    fn consent_path_uses_the_core_platform_config_directory() {
        assert_eq!(
            consent_path(),
            chystik_core::platform::current()
                .app_paths()
                .config_dir
                .join("consent.json")
        );
    }
}

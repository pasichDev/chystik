//! Cloud sync clients (Nextcloud, Dropbox, Insync, Syncthing): transfer
//! queues, conflict copies, metadata DBs. Synced user files are NEVER
//! targets — only client-internal state.
//!
//! Owner: child agent `rules-cloud` on branch `v02/rules-cloud`.

use std::path::Path;

use crate::model::{Category, Severity};

use super::{home_root, Match};

/// Evaluate the cloud-sync rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    home_rule(dir)
}

/// Client-owned regenerable state. NEVER targeted: the synced folders
/// themselves, metadata DBs inside them (.sync_*.db etc.), Syncthing's mixed
/// config+index root, Nextcloud account state, Dropbox instance DBs.
const HOME_TARGETS: &[(&str, Severity, &str)] = &[
    (
        ".cache/rclone",
        Severity::Safe,
        "rclone transfer cache — rebuilt by the next sync",
    ),
    (
        ".cache/borg",
        Severity::Safe,
        "borg client cache (chunk indexes) — recomputed from the repository",
    ),
    (
        ".cache/restic",
        Severity::Safe,
        "restic local cache — rebuilt from the repository on the next run",
    ),
    (
        ".cache/duplicati",
        Severity::Safe,
        "Duplicati transient cache — recreated during the next backup",
    ),
    (
        ".cache/gdfuse",
        Severity::Safe,
        "google-drive-ocamlfuse metadata cache — refreshed from the Drive API",
    ),
    (
        ".local/share/nextcloud/logs",
        Severity::Safe,
        "Nextcloud desktop client logs — recreated during normal use",
    ),
];

fn home_rule(dir: &Path) -> Option<Match> {
    let home = home_root()?;
    let rel = dir
        .strip_prefix(&home)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    let (_, sev, note) = *HOME_TARGETS.iter().find(|(s, ..)| rel == *s)?;
    Some(Match {
        category: Category::CloudSync,
        severity: sev,
        note: note.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn mk(root: &std::path::Path, rel: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(root.join(rel)).unwrap();
        root.join(rel)
    }

    use crate::rules::TEST_ENV_LOCK as ENV_LOCK;

    #[test]
    fn sync_client_caches_match_but_state_roots_do_not() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());
        for rel in [
            ".cache/rclone",
            ".cache/borg",
            ".cache/restic",
            ".cache/duplicati",
            ".cache/gdfuse",
        ] {
            let m = classify(&mk(fake.path(), rel)).expect(rel);
            assert_eq!(m.category, Category::CloudSync, "{rel}");
            assert_eq!(m.severity, Severity::Safe, "{rel}");
        }
        assert!(classify(&mk(fake.path(), ".local/share/nextcloud/logs")).is_some());
        // Mixed config+index roots and the client program dirs stay untouched.
        assert!(classify(&mk(fake.path(), ".local/share/syncthing")).is_none());
        assert!(classify(&mk(fake.path(), ".dropbox-dist")).is_none());
        assert!(classify(&mk(fake.path(), ".local/share/nextcloud")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn correctly_named_tree_outside_home_does_not_match() {
        let root = tempdir().unwrap();
        assert!(classify(&mk(root.path(), "proj/.cache/borg")).is_none());
    }
}

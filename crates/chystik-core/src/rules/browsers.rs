//! Browser engines beyond the v0.1 chrome/mozilla cache roots:
//! Chromium-family profiles, WebKitGTK, Brave/Vivaldi/Opera caches.

use std::path::Path;

use crate::model::{Category, Severity};

use super::{home_root, Match};

/// Dedicated `.cache` roots of Chromium-family browsers plus WebKitGTK and
/// GNOME Web. Everything beneath them is disposable cache; profile data
/// (bookmarks/history/logins) lives under `.config`, which the deletion
/// guard refuses, so those locations are intentionally not claimed.
const HOME_TARGETS: &[&str] = &[
    ".cache/chromium",
    ".cache/BraveSoftware",
    ".cache/vivaldi",
    ".cache/microsoft-edge",
    ".cache/opera",
    ".cache/thorium",
    ".cache/webkitgtk",
    ".cache/epiphany",
];

/// True when `rel` equals a target root or lives inside one (component
/// boundaries respected, so `.cache/chromium-notes` never matches).
fn under_target(rel: &str) -> bool {
    HOME_TARGETS
        .iter()
        .any(|t| rel == *t || rel.starts_with(&format!("{t}/")))
}

fn home_rule(dir: &Path) -> Option<Match> {
    let home = home_root()?;
    let rel = dir
        .strip_prefix(&home)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    if !under_target(&rel) {
        return None;
    }
    Some(Match {
        category: Category::BrowserSystem,
        severity: Severity::Safe,
        note: "browser cache — regenerated during browsing; bookmarks, history and passwords live elsewhere and stay untouched".into(),
    })
}

/// Evaluate the extended browser rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    home_rule(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn mk(root: &std::path::Path, rel: &str) -> std::path::PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    use crate::rules::TEST_ENV_LOCK as ENV_LOCK;

    #[test]
    fn chromium_family_and_webkit_caches_match_as_safe() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());
        for rel in [
            ".cache/chromium",
            ".cache/chromium/Default/Cache",
            ".cache/BraveSoftware",
            ".cache/vivaldi",
            ".cache/microsoft-edge",
            ".cache/opera",
            ".cache/thorium",
            ".cache/webkitgtk",
            ".cache/epiphany",
        ] {
            let m = classify(&mk(fake.path(), rel)).expect(rel);
            assert_eq!(m.category, Category::BrowserSystem, "{rel}");
            assert_eq!(m.severity, Severity::Safe, "{rel}");
        }
        assert!(classify(&mk(fake.path(), "chromium")).is_none());
        assert!(classify(&mk(fake.path(), ".cache/chromium-notes")).is_none());
        assert!(classify(&mk(fake.path(), ".local/share/chromium")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn correctly_named_tree_outside_home_does_not_match() {
        let root = tempdir().unwrap();
        assert!(classify(&mk(root.path(), "webapp/.cache/chromium")).is_none());
    }
}

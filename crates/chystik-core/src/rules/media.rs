//! Media production apps (OBS, Kdenlive, GIMP, Audacity...): render/proxy
//! caches, autosave scratch. Project source files are never targets.
//!
//! Owner: child agent `rules-media` on branch `v02/rules-media`.

use std::path::Path;

use crate::model::{Category, Severity};

use super::{home_root, Match};

/// Evaluate the media-apps rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    home_rule(dir)
}

/// Verified `.cache` roots of media-production apps. Render previews and
/// proxies stored INSIDE user projects are never touched; OBS and GIMP keep
/// everything under `.config` (guard-refused), so they are documented skips.
const HOME_TARGETS: &[&str] = &[".cache/kdenlive", ".cache/blender", ".cache/inkscape"];

/// True when `rel` equals a target root or lives inside one.
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
        category: Category::MediaApps,
        severity: Severity::Safe,
        note:
            "media app cache — regenerated on the next app run; your projects are stored elsewhere"
                .into(),
    })
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
    fn media_app_caches_match_as_safe_media() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());
        for rel in [
            ".cache/kdenlive",
            ".cache/kdenlive/proxy-cache",
            ".cache/blender",
            ".cache/inkscape",
        ] {
            let m = classify(&mk(fake.path(), rel)).expect(rel);
            assert_eq!(m.category, Category::MediaApps, "{rel}");
            assert_eq!(m.severity, Severity::Safe, "{rel}");
        }
        assert!(classify(&mk(fake.path(), "kdenlive")).is_none());
        assert!(classify(&mk(fake.path(), ".config/kdenlive")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn correctly_named_tree_outside_home_does_not_match() {
        let root = tempdir().unwrap();
        assert!(classify(&mk(root.path(), "proj/.cache/blender")).is_none());
    }
}

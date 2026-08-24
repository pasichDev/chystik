//! Safety guard — TO IMPLEMENT (core-engine agent).
//!
//! Hard refusals for dangerous targets. This module is the last line of
//! defence before any deletion; every path passes through `check()`.

use crate::model::ChystikError;
use std::path::Path;

/// Refuse these absolute paths outright.
pub const PROTECTED_PREFIXES: &[&str] = &[
    "/", "/boot", "/etc", "/usr", "/var", "/opt", "/proc", "/sys", "/dev",
];

/// Refuse these directory names anywhere in the tree.
pub const PROTECTED_NAMES: &[&str] = &[".git", ".ssh", ".gnupg", ".config"];

/// Audited exceptions to the blanket `.config` ban, relative to `$HOME`.
///
/// `.config` is protected because it holds settings a user cannot
/// regenerate — but applications also park large pure caches there, and
/// rules registered against those paths produced findings the guard then
/// refused: shown, selectable-looking, permanently undeletable. Deny by
/// default, allow only what is enumerated here; every entry must be
/// content the owning application rebuilds on its own.
pub const CONFIG_CACHE_ALLOWLIST: &[&str] = &[
    ".config/Code/CachedData",
    ".config/Code/CachedExtensionVSIXs",
    ".config/Code/workspaceStorage",
    ".config/google-chrome/Default/Service Worker/CacheStorage",
    ".config/google-chrome/extensions_crx_cache",
    ".config/google-chrome/optimization_guide_model_store",
];

/// True when `candidate` is one of the allowlisted `.config` caches (or
/// lives inside one).
fn is_allowlisted_config_cache(candidate: &Path) -> bool {
    let Some(home) = crate::rules::home_root() else {
        return false;
    };
    let Ok(rel) = candidate.strip_prefix(&home) else {
        return false;
    };
    let rel = rel.to_string_lossy().replace('\\', "/");
    CONFIG_CACHE_ALLOWLIST
        .iter()
        .any(|allowed| rel == *allowed || rel.starts_with(&format!("{allowed}/")))
}

/// Validate a deletion candidate:
/// - must exist and be inside the scan root
/// - must not be a protected prefix or contain protected name components
/// - must not be a symlink (reject, do not follow)
/// - scan root itself is never deletable
pub fn check(candidate: &Path, scan_root: &Path) -> Result<(), ChystikError> {
    let refuse = || Err(ChystikError::ProtectedPath(candidate.to_path_buf()));

    // Must exist; symlink_metadata does NOT follow symlinks.
    let Ok(meta) = std::fs::symlink_metadata(candidate) else {
        return refuse();
    };
    if meta.file_type().is_symlink() {
        return refuse();
    }
    if candidate == scan_root || !candidate.starts_with(scan_root) {
        return refuse();
    }
    // System locations: candidate equals a protected prefix or lives directly
    // under one (relevant when someone scans / or /var).
    let s = candidate.to_string_lossy();
    for p in PROTECTED_PREFIXES {
        if *p == "/" {
            if s == "/" {
                return refuse();
            }
        } else if s.as_ref() == *p || s.starts_with(&format!("{p}/")) {
            return refuse();
        }
    }
    // Protected dot-dirs anywhere along the path. `.config` alone has
    // audited cache exceptions; `.git`/`.ssh`/`.gnupg` never do.
    for component in candidate
        .components()
        .filter_map(|c| c.as_os_str().to_str())
    {
        if !PROTECTED_NAMES.contains(&component) {
            continue;
        }
        if component == ".config" && is_allowlisted_config_cache(candidate) {
            continue;
        }
        return refuse();
    }
    Ok(())
}

/// Return true if the walker should descend into `dir` during scanning
/// (used to avoid wasting time in system trees).
pub fn is_scannable(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir)
        .map(|m| m.is_dir())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn accepts_regular_child_of_scan_root() {
        let root = tempdir().unwrap();
        let proj = root.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        assert!(check(&proj, root.path()).is_ok());
    }

    #[test]
    fn rejects_scan_root_itself() {
        let root = tempdir().unwrap();
        assert!(check(root.path(), root.path())
            .unwrap_err()
            .to_string()
            .contains("protected"));
    }

    #[test]
    fn rejects_outside_scan_root() {
        let outer = tempdir().unwrap();
        let root = outer.path().join("scanroot");
        std::fs::create_dir_all(&root).unwrap();
        assert!(check(outer.path(), &root).is_err());
    }

    #[test]
    fn rejects_protected_name_anywhere_in_components() {
        let root = tempdir().unwrap();
        let p = root.path().join("proj/.git/hooks");
        std::fs::create_dir_all(&p).unwrap();
        assert!(check(&p, root.path()).is_err());
    }

    #[test]
    fn rejects_symlink_without_following() {
        let root = tempdir().unwrap();
        let real = root.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(check(&link, root.path()).is_err());
    }

    #[test]
    fn allowlisted_config_caches_are_deletable_but_config_itself_is_not() {
        let _env = crate::rules::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let home = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", home.path());

        let cached = home.path().join(".config/Code/CachedData");
        std::fs::create_dir_all(&cached).unwrap();
        assert!(
            check(&cached, home.path()).is_ok(),
            "an audited .config cache must be deletable, not shown-and-refused"
        );

        // Everything else under .config stays refused.
        let settings = home.path().join(".config/Code/User");
        std::fs::create_dir_all(&settings).unwrap();
        assert!(check(&settings, home.path()).is_err());
        let other = home.path().join(".config/some-app");
        std::fs::create_dir_all(&other).unwrap();
        assert!(check(&other, home.path()).is_err());

        // The allowlist never rescues the other protected names.
        let git = home.path().join(".config/Code/CachedData/.git");
        std::fs::create_dir_all(&git).unwrap();
        assert!(check(&git, home.path()).is_err());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn rejects_opt_installed_applications() {
        assert!(check(Path::new("/opt/android-studio"), Path::new("/")).is_err());
    }

    #[test]
    fn rejects_missing_and_system_prefixes() {
        let root = tempdir().unwrap();
        assert!(check(&root.path().join("nope"), root.path()).is_err());
        let var = PathBuf::from("/var/tmp");
        assert!(check(&var, Path::new("/")).is_err()); // under /var prefix
    }

    #[test]
    fn is_scannable_matches_dirs_only() {
        let root = tempdir().unwrap();
        let d = root.path().join("d");
        std::fs::create_dir_all(&d).unwrap();
        let f = root.path().join("f.txt");
        std::fs::write(&f, "x").unwrap();
        assert!(is_scannable(&d));
        assert!(!is_scannable(&f));
        assert!(!is_scannable(&d.join("missing")));
    }
}

//! Safety guard — TO IMPLEMENT (core-engine agent).
//!
//! Hard refusals for dangerous targets. This module is the last line of
//! defence before any deletion; every path passes through `check()`.

use crate::model::ChystikError;
use std::path::Path;

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

/// Privacy traces under `.config` a privacy clean may target.
///
/// Separate from the cache allowlist on purpose: these are NOT caches, and
/// nothing here regenerates. They are listed because clearing browsing
/// history is the explicit point of the privacy view, and a guard that
/// refuses it silently would leave the feature broken rather than safe.
/// Each entry is a single file whose only content is a record of activity.
pub const PRIVACY_ALLOWLIST: &[&str] = &[
    ".config/google-chrome/Default/History",
    ".config/google-chrome/Default/Cookies",
    ".config/chromium/Default/History",
    ".config/chromium/Default/Cookies",
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
        .chain(PRIVACY_ALLOWLIST.iter())
        .any(|allowed| rel == *allowed || rel.starts_with(&format!("{allowed}/")))
}

/// Validate a deletion candidate.
///
/// Refuses anything that is not, physically, a non-symlinked path strictly
/// inside `scan_root`:
/// - must exist, and must not itself be a symlink
/// - no component BELOW the scan root may be a symlink either
/// - must resolve to a location still inside the resolved scan root
/// - must not be a protected prefix or contain a protected name, checked on
///   the resolved path as well as the given one
/// - the scan root itself is never deletable
///
/// The ancestor rules matter as much as the last-component one. Checking
/// only `symlink_metadata(candidate)` describes the final component and
/// nothing else: with `cache-link -> important-data`, the path
/// `<root>/cache-link/sub` lstats a real directory, passes every lexical
/// test, and deletes `important-data/sub`.
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
    // Every step from the scan root down to the candidate must be a real
    // directory, not a link into somewhere else.
    if has_symlinked_ancestor(candidate, scan_root) {
        return refuse();
    }
    // Where the path actually leads, after the kernel resolves it. A
    // candidate whose resolved form leaves the resolved root is refused
    // however innocent it looked lexically.
    let resolved = std::fs::canonicalize(candidate).ok();
    if let (Some(resolved), Ok(resolved_root)) = (&resolved, std::fs::canonicalize(scan_root)) {
        if resolved == &resolved_root || !resolved.starts_with(&resolved_root) {
            return refuse();
        }
    }

    for path in [Some(candidate.to_path_buf()), resolved]
        .into_iter()
        .flatten()
    {
        if is_protected_location(&path, candidate) {
            return refuse();
        }
    }
    Ok(())
}

/// True if any component strictly between `scan_root` and `candidate` is a
/// symlink. Components at or above the scan root are the user's own choice
/// of target and are covered by the resolved-containment check instead.
fn has_symlinked_ancestor(candidate: &Path, scan_root: &Path) -> bool {
    let Ok(relative) = candidate.strip_prefix(scan_root) else {
        return true; // not inside the root at all
    };
    let mut walked = scan_root.to_path_buf();
    let mut components: Vec<_> = relative.components().collect();
    components.pop(); // the candidate itself is lstatted by the caller
    for component in components {
        walked.push(component);
        match std::fs::symlink_metadata(&walked) {
            Ok(meta) if meta.file_type().is_symlink() => return true,
            Ok(_) => {}
            Err(_) => return true, // cannot vouch for it, so refuse
        }
    }
    false
}

/// Protected-prefix and protected-name rules, applied to one path.
/// `original` is only used to resolve the `.config` allowlist, which is
/// expressed relative to `$HOME`.
fn is_protected_location(path: &Path, original: &Path) -> bool {
    if crate::platform::current().is_protected_system_path(path) {
        return true;
    }
    // Protected dot-dirs anywhere along the path. `.config` alone has
    // audited cache exceptions; `.git`/`.ssh`/`.gnupg` never do.
    for component in path.components().filter_map(|c| c.as_os_str().to_str()) {
        if !PROTECTED_NAMES.contains(&component) {
            continue;
        }
        if component == ".config"
            && (is_allowlisted_config_cache(path) || is_allowlisted_config_cache(original))
        {
            continue;
        }
        return true;
    }
    false
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

    /// A symlinked PARENT inside the scan root.
    ///
    /// `symlink_metadata(candidate)` describes only the last component, so
    /// `<root>/cache-link/sub` lstatted a real directory and passed every
    /// lexical test while physically pointing at `important-data/sub`.
    #[cfg(unix)]
    #[test]
    fn rejects_a_symlinked_parent_inside_the_scan_root() {
        let root = tempdir().unwrap();
        let important = root.path().join("important-data");
        std::fs::create_dir_all(important.join("sub")).unwrap();
        let link = root.path().join("cache-link");
        std::os::unix::fs::symlink(&important, &link).unwrap();

        let through_link = link.join("sub");
        assert!(
            std::fs::symlink_metadata(&through_link).is_ok(),
            "fixture must lstat cleanly, or the test proves nothing"
        );
        assert!(
            check(&through_link, root.path()).is_err(),
            "a symlinked ancestor must be refused"
        );
        // The real location is still deletable; only the route through the
        // link is refused.
        assert!(check(&important.join("sub"), root.path()).is_ok());
    }

    /// Deeper nesting, and a link that leaves the scan root entirely.
    #[cfg(unix)]
    #[test]
    fn rejects_a_link_that_escapes_the_scan_root() {
        let outside = tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("secrets")).unwrap();
        let root = tempdir().unwrap();
        let nested = root.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        let escape = nested.join("out");
        std::os::unix::fs::symlink(outside.path(), &escape).unwrap();

        assert!(check(&escape.join("secrets"), root.path()).is_err());
        assert!(check(&escape, root.path()).is_err());
    }

    /// A protected name reached THROUGH a link must still be refused: the
    /// lexical path says nothing about where it lands.
    #[cfg(unix)]
    #[test]
    fn protected_names_are_checked_on_the_resolved_path_too() {
        let root = tempdir().unwrap();
        let real = root.path().join("project/.git");
        std::fs::create_dir_all(&real).unwrap();
        let link = root.path().join("innocent");
        std::os::unix::fs::symlink(root.path().join("project"), &link).unwrap();
        assert!(check(&link.join(".git"), root.path()).is_err());
    }

    #[cfg(unix)]
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

    /// Privacy traces under `.config` must be deletable, or the privacy
    /// view would list items the guard silently refuses.
    #[test]
    fn allowlisted_privacy_traces_are_deletable() {
        let _env = crate::rules::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let home = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", home.path());

        for rel in PRIVACY_ALLOWLIST {
            let path = home.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "activity record").unwrap();
            assert!(
                check(&path, home.path()).is_ok(),
                "{rel} is listed as clearable but the guard refuses it"
            );
        }
        // The exception is exactly these files, not their directory.
        let profile = home.path().join(".config/google-chrome/Default");
        assert!(check(&profile, home.path()).is_err());
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

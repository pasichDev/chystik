//! Stale installer & update-copy rule set (v0.2).
//!
//! The scanner reports directories only, so rules target directory-shaped
//! leftovers: per-product JetBrains update caches and downloaded IDE
//! distribution trees (e.g. `idea-IU-*`, `android-studio-*`) that carry an
//! install manifest next to a runnable `bin` directory.

use std::path::Path;

use crate::model::{Category, Severity};

use super::{home_root, parent_has_file, Match};

/// Evaluate the installers rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    jetbrains_cache_rule(dir)
        .or_else(|| jetbrains_updates_rule(dir))
        .or_else(|| ide_distribution_rule(dir))
}

/// Normalized `dir` relative to `$HOME`; falls back to the full normalized
/// path when the prefix does not match. The fallback mirrors `core.rs`:
/// under `CHYSTIK_TEST_HOME` another test thread may swap the override
/// between fixture creation and classification, and suffix matching then
/// recovers the fixture path without broadening production matching.
fn home_rel(dir: &Path) -> Option<String> {
    let home = home_root()?;
    if let Ok(rel) = dir.strip_prefix(&home) {
        return Some(rel.to_string_lossy().replace('\\', "/"));
    }
    if std::env::var_os("CHYSTIK_TEST_HOME").is_some() {
        return Some(dir.to_string_lossy().replace('\\', "/"));
    }
    None
}

/// True when `rel` equals `prefix` or starts with `prefix/`. The extra
/// component-bounded clauses cover the `home_rel` fallback branch: under
/// `CHYSTIK_TEST_HOME` another test thread may swap the override between
/// fixture creation and classification, leaving the full absolute path in
/// `rel`; matching then recovers the fixture suffix without broadening
/// production matching.
fn under_prefix(rel: &str, prefix: &str) -> bool {
    if rel == prefix || rel.starts_with(&format!("{prefix}/")) {
        return true;
    }
    std::env::var_os("CHYSTIK_TEST_HOME").is_some()
        && (rel.ends_with(&format!("/{prefix}")) || rel.contains(&format!("/{prefix}/")))
}

const JETBRAINS_CACHE: &str = ".cache/JetBrains";

/// Where a user unpacks a downloaded IDE tarball. A distribution tree found
/// anywhere else is an installation, not a leftover.
const UNPACK_LOCATIONS: &[&str] = &[
    "Downloads",
    "Desktop",
    "Downloads/idea",
    "opt",
    ".local/opt",
    ".local/share/JetBrains/Toolbox/apps",
];

/// Per-product caches under `~/.cache/JetBrains/<Product><Version>` hold
/// indexes and update staging; they are rebuilt on the next IDE launch.
fn jetbrains_cache_rule(dir: &Path) -> Option<Match> {
    if !under_prefix(&home_rel(dir)?, JETBRAINS_CACHE) {
        return None;
    }
    Some(Match {
        category: Category::Installers,
        severity: Severity::Safe,
        note: "JetBrains product cache — recreated automatically on the next IDE start".into(),
    })
}

/// Update staging inside a JetBrains install tree (`.../JetBrains/<Product>/updates`)
/// — leftover downloaded updates; the installed product keeps working.
fn jetbrains_updates_rule(dir: &Path) -> Option<Match> {
    if dir.file_name()?.to_str()? != "updates" {
        return None;
    }
    let product = dir.parent()?;
    // `Path::ends_with` compares whole components, so only installs that
    // really live directly under `.local/share/JetBrains` count.
    let jb_root = product.parent()?;
    if !jb_root.ends_with(".local/share/JetBrains") {
        return None;
    }
    Some(Match {
        category: Category::Installers,
        severity: Severity::Moderate,
        note: "downloaded JetBrains update copy — safe after updating; reinstall from jetbrains.com if needed".into(),
    })
}

/// Downloaded IDE distribution trees such as `idea-IU-241.x`,
/// `android-studio-2024.x`, or `pycharm-community-*`: recognized by an
/// install manifest sitting next to a runnable `bin` directory.
fn ide_distribution_rule(dir: &Path) -> Option<Match> {
    let name = dir.file_name()?.to_str()?;
    let looks_like_ide_dist = ["idea-", "android-studio", "pycharm-"]
        .iter()
        .any(|p| name.starts_with(p));
    if !looks_like_ide_dist || !dir.join("bin").is_dir() {
        return None;
    }
    // Only an unpacked *download* is a leftover. Without this scope the rule
    // also matched system installs — `/opt/android-studio` carries the same
    // `build.txt` + `bin/` markers, and `guard::check` permits `/opt`, so a
    // 3 GiB installed IDE was offered for deletion as "re-downloadable".
    let rel = home_rel(dir)?;
    if !UNPACK_LOCATIONS.iter().any(|p| under_prefix(&rel, p)) {
        return None;
    }
    if !parent_has_file(dir, &["Install-Linux-tar.txt", "build.txt", "install.txt"]) {
        return None;
    }
    Some(Match {
        category: Category::Installers,
        severity: Severity::Moderate,
        note: "downloaded IDE distribution — delete and re-download from the vendor site if ever needed again".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // Shared crate-wide lock (rules::TEST_ENV_LOCK) serializing every
    // rule-module test that mutates CHYSTIK_TEST_HOME. The neutral-home
    // restore and retry helper below stay as defense-in-depth.
    use crate::rules::TEST_ENV_LOCK as ENV_LOCK;

    fn mk(root: &Path, rel: &str) -> std::path::PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Neutral override left behind after each test. Deliberately NOT
    /// removed and deliberately NOT a real directory: sibling rule modules
    /// run their own `CHYSTIK_TEST_HOME` tests in parallel with separate
    /// locks, so a removal landing between another module's `set_var` and
    /// `classify` turns its `$HOME` lookups into misses, and any value that
    /// prefixes tempdir fixtures would make `strip_prefix` succeed and skip
    /// suffix recovery. A nonexistent root keeps the variable present (no
    /// absence races) while matching stays fixture-suffix based.
    const NEUTRAL_HOME: &str = "/nonexistent-chystik-test-home";

    fn with_test_home<T>(fake: &Path, f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("CHYSTIK_TEST_HOME", fake);
        let out = f();
        std::env::set_var("CHYSTIK_TEST_HOME", NEUTRAL_HOME);
        out
    }

    /// Positive lookup that tolerates sibling rule-module tests: every
    /// module keeps its own `ENV_LOCK`, yet all of them mutate the single
    /// process-global `CHYSTIK_TEST_HOME`, so another thread can swap or
    /// drop the override between fixture creation and `classify`. Each
    /// attempt re-asserts the override first; the suffix-based recovery in
    /// `under_prefix`/`home_rel` makes any foreign value work, and only a
    /// fully absent variable can cause a spurious miss, which the retries
    /// ride out.
    fn expect_match(fake: &Path, dir: &Path) -> Match {
        for _ in 0..200 {
            std::env::set_var("CHYSTIK_TEST_HOME", fake);
            if let Some(m) = classify(dir) {
                return m;
            }
        }
        panic!(
            "expected match for {}, override kept disappearing",
            dir.display()
        );
    }

    #[test]
    fn jetbrains_product_caches_match_under_dot_cache() {
        let root = tempdir().unwrap();
        let (hit, nested_hit, outside) = with_test_home(root.path(), || {
            let hit = expect_match(
                root.path(),
                &mk(root.path(), ".cache/JetBrains/IntelliJIdea2025.2"),
            );
            let nested_hit = expect_match(
                root.path(),
                &mk(root.path(), ".cache/JetBrains/Toolbox/apps"),
            );
            let outside = classify(&mk(root.path(), "projects/JetBrains"));
            (hit, nested_hit, outside)
        });
        assert_eq!(hit.category, Category::Installers);
        assert_eq!(hit.severity, Severity::Safe);
        assert_eq!(nested_hit.category, Category::Installers);
        assert!(
            outside.is_none(),
            "same name outside ~/.cache must not match"
        );
    }

    #[test]
    fn jetbrains_updates_require_local_share_jetbrains_parent() {
        let root = tempdir().unwrap();
        let (hit, wrong_parent) = with_test_home(root.path(), || {
            let product = mk(root.path(), ".local/share/JetBrains/IntelliJIdea2025.2");
            mk(&product, "bin");
            let hit = expect_match(root.path(), &mk(&product, "updates"));
            let wrong = mk(root.path(), "Downloads/myapp");
            mk(&wrong, "bin");
            let wrong_parent = classify(&mk(&wrong, "updates"));
            (hit, wrong_parent)
        });
        assert_eq!(hit.category, Category::Installers);
        assert_eq!(hit.severity, Severity::Moderate);
        assert!(wrong_parent.is_none(), "updates elsewhere must not match");
    }

    #[test]
    fn ide_distributions_need_bin_dir_and_manifest() {
        let root = tempdir().unwrap();
        let (full, no_manifest) = with_test_home(root.path(), || {
            let dist = mk(root.path(), "Downloads/idea-IU-251.23774.435");
            mk(&dist, "bin");
            std::fs::write(dist.join("Install-Linux-tar.txt"), "how to install\n").unwrap();
            let full = expect_match(root.path(), &dist);

            let bare = mk(root.path(), "Downloads/android-studio-2025.1.1");
            mk(&bare, "bin");
            let no_manifest = classify(&bare);
            (full, no_manifest)
        });
        assert_eq!(full.category, Category::Installers);
        assert_eq!(full.severity, Severity::Moderate);
        assert!(
            no_manifest.is_none(),
            "missing install manifest must yield None"
        );
    }

    #[test]
    fn system_installs_are_not_offered_for_deletion() {
        // Regression: `/opt/android-studio` carries `bin/` + `build.txt`
        // exactly like an unpacked download, and `guard::check` permits
        // `/opt`, so an unscoped rule offered a 3 GiB installed IDE as
        // "re-downloadable".
        let root = tempfile::tempdir().unwrap();
        with_test_home(root.path(), || {
            let installed = mk(root.path(), "usr-local-share/android-studio");
            mk(&installed, "bin");
            std::fs::write(installed.join("build.txt"), "AI-251").unwrap();
            assert!(
                classify(&installed).is_none(),
                "a distribution tree outside a download location is an install"
            );

            let downloaded = mk(root.path(), "Downloads/android-studio-2025.1.1");
            mk(&downloaded, "bin");
            std::fs::write(downloaded.join("build.txt"), "AI-251").unwrap();
            assert!(
                classify(&downloaded).is_some(),
                "an unpacked download is still a leftover"
            );
        });
    }

    #[test]
    fn non_ide_names_or_missing_bin_do_not_match() {
        let root = tempdir().unwrap();
        let (random_name, no_bin, marker_only) = with_test_home(root.path(), || {
            let random = mk(root.path(), "Downloads/some-app-1.0");
            mk(&random, "bin");
            std::fs::write(random.join("build.txt"), "x").unwrap();
            let random_name = classify(&random);

            let no_bin = mk(root.path(), "Downloads/idea-sources");
            std::fs::write(no_bin.join("build.txt"), "x").unwrap();
            let no_bin = classify(&no_bin);

            let marker_only = mk(root.path(), "Downloads/pycharm-notes");
            let marker_only = classify(&marker_only);
            (random_name, no_bin, marker_only)
        });
        assert!(random_name.is_none(), "unrelated names must not match");
        assert!(no_bin.is_none(), "no bin/ directory must not match");
        assert!(marker_only.is_none(), "name-only hits must not match");
    }
}

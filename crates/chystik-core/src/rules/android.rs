//! Android development tooling rule set (v0.2).
//!
//! Covers emulator virtual devices, the shared Android build cache, SDK
//! system images, and stale Android Studio update copies. Every rule is
//! marker-gated: a well-known directory name only counts inside its
//! expected location, mirroring `core.rs`.

use std::path::Path;

use crate::model::{Category, Severity};

use super::{home_root, parent_has, Match};

/// Evaluate the android rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    avd_rule(dir)
        .or_else(|| build_cache_rule(dir))
        .or_else(|| system_images_rule(dir))
        .or_else(|| studio_update_copy_rule(dir))
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

/// True when `rel` equals `suffix` or ends with `/suffix` (component
/// boundary respected, so `foo.android/avd` never matches `.android/avd`).
fn is_rel(rel: &str, suffix: &str) -> bool {
    rel == suffix || rel.ends_with(&format!("/{suffix}"))
}

fn avd_rule(dir: &Path) -> Option<Match> {
    let name = dir.file_name()?.to_str()?;
    if name == "avd" && is_rel(&home_rel(dir)?, ".android/avd") {
        return Some(Match {
            category: Category::AndroidDev,
            severity: Severity::Moderate,
            note: "emulator virtual devices — recreate with `avdmanager create avd` or Android Studio's Device Manager".into(),
        });
    }
    let parent = dir.parent()?;
    if name.ends_with(".avd") && is_rel(&home_rel(parent)?, ".android/avd") {
        return Some(Match {
            category: Category::AndroidDev,
            severity: Severity::Moderate,
            note: "single emulator device incl. snapshots — delete and recreate via `avdmanager`"
                .into(),
        });
    }
    None
}

fn build_cache_rule(dir: &Path) -> Option<Match> {
    if !is_rel(&home_rel(dir)?, ".android/build-cache") {
        return None;
    }
    Some(Match {
        category: Category::AndroidDev,
        severity: Severity::Safe,
        note: "shared Android build cache — rebuilt and re-downloaded automatically by Gradle"
            .into(),
    })
}

fn system_images_rule(dir: &Path) -> Option<Match> {
    if !is_rel(&home_rel(dir)?, "Android/Sdk/system-images") {
        return None;
    }
    let sdk = dir.parent()?;
    if !parent_has(sdk, &["platform-tools", "cmdline-tools"]) {
        return None;
    }
    Some(Match {
        category: Category::AndroidDev,
        severity: Severity::Moderate,
        note: "emulator system images — huge but re-downloadable via SDK Manager (`sdkmanager 'system-images;...'`)".into(),
    })
}

fn studio_update_copy_rule(dir: &Path) -> Option<Match> {
    if dir.file_name()?.to_str()? != "updates" {
        return None;
    }
    let product = dir.parent()?;
    let google = product.parent()?;
    // `Path::ends_with` compares whole components, so only installs that
    // really live under a `.local/share/Google` tree count.
    if !google.ends_with(".local/share/Google") || !parent_has(product, &["bin"]) {
        return None;
    }
    Some(Match {
        category: Category::AndroidDev,
        severity: Severity::Moderate,
        note: "stale Android Studio update copy — the running install stays in place; fresh builds at developer.android.com/studio".into(),
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
    /// `home_rel` makes any foreign value work, and only a fully absent
    /// variable can cause a spurious miss, which the retries ride out.
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
    fn avd_collection_and_single_devices_match() {
        let root = tempdir().unwrap();
        let (container, device, plain_child) = with_test_home(root.path(), || {
            let avd = mk(root.path(), ".android/avd");
            let container = expect_match(root.path(), &avd);
            let device = expect_match(root.path(), &mk(&avd, "Pixel_8.avd"));
            let plain_child = classify(&mk(&avd, "misc"));
            (container, device, plain_child)
        });
        assert_eq!(container.category, Category::AndroidDev);
        assert_eq!(container.severity, Severity::Moderate);

        assert_eq!(device.category, Category::AndroidDev);
        assert_eq!(device.severity, Severity::Moderate);

        assert!(plain_child.is_none(), "non-.avd child must not match");
    }

    #[test]
    fn avd_names_outside_android_home_do_not_match() {
        let root = tempdir().unwrap();
        let (stray_avd, typo_dir) = with_test_home(root.path(), || {
            let stray_avd = classify(&mk(root.path(), "projects/avd"));
            let typo_dir = classify(&mk(root.path(), ".android/avds"));
            (stray_avd, typo_dir)
        });
        assert!(stray_avd.is_none());
        assert!(typo_dir.is_none());
    }

    #[test]
    fn build_cache_matches_only_in_android_home() {
        let root = tempdir().unwrap();
        let (hit, miss) = with_test_home(root.path(), || {
            let hit = expect_match(root.path(), &mk(root.path(), ".android/build-cache"));
            let miss = classify(&mk(root.path(), ".android/buildcache"));
            (hit, miss)
        });
        assert_eq!(hit.category, Category::AndroidDev);
        assert_eq!(hit.severity, Severity::Safe);
        assert!(miss.is_none());
    }

    #[test]
    fn system_images_need_sdk_markers_as_siblings() {
        let root = tempdir().unwrap();
        let (hit, miss) = with_test_home(root.path(), || {
            let sdk = mk(root.path(), "Android/Sdk");
            mk(&sdk, "platform-tools");
            let hit = expect_match(root.path(), &mk(&sdk, "system-images"));

            let bare_sdk = mk(root.path(), "Other/Sdk");
            let miss = classify(&mk(&bare_sdk, "system-images"));
            (hit, miss)
        });
        assert_eq!(hit.category, Category::AndroidDev);
        assert_eq!(hit.severity, Severity::Moderate);
        assert!(miss.is_none(), "missing SDK markers must yield None");
    }

    #[test]
    fn studio_update_copies_require_install_shaped_product_dir() {
        let root = tempdir().unwrap();
        let (hit, no_bin, wrong_tree) = with_test_home(root.path(), || {
            let product = mk(root.path(), ".local/share/Google/AndroidStudio2025.3.1");
            mk(&product, "bin");
            let hit = expect_match(root.path(), &mk(&product, "updates"));

            let bare = mk(root.path(), ".local/share/Google/AndroidStudioOld");
            let no_bin = classify(&mk(&bare, "updates"));

            let app = mk(root.path(), "projects/myapp");
            mk(&app, "bin");
            let wrong_tree = classify(&mk(&app, "updates"));
            (hit, no_bin, wrong_tree)
        });
        assert_eq!(hit.category, Category::AndroidDev);
        assert_eq!(hit.severity, Severity::Moderate);
        assert!(
            no_bin.is_none(),
            "updates without bin sibling must not match"
        );
        assert!(
            wrong_tree.is_none(),
            "updates outside .local/share/Google must not match"
        );
    }
}

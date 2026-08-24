//! Programming-language toolchain & package-cache rule set (v0.2).
//!
//! Covers language package-manager caches, interpreter/toolchain downloads
//! and shared application virtualenvs that live directly under `$HOME` and
//! are not claimed by the core rule set. Every rule is a well-known absolute
//! path below `home_root()`, so no project markers are required. Rust
//! nightly-toolchain pruning via rustup is intentionally deferred — it needs
//! version comparison between installed toolchains (future work).

use std::path::Path;

use crate::model::{Category, Severity};

use super::{match_home_rule, HomeRule, Match};

/// Evaluate the languages rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    match_home_rule(dir, HOME_RULES)
}

const SAFE: Severity = Severity::Safe;
const MOD: Severity = Severity::Moderate;

/// One table, no hand-maintained second copy. `super::match_home_rule`
/// derives the test-override suffix fallback from this same list, so a new
/// entry cannot be half-registered the way the old "KEEP IN SYNC" comment
/// warned about.
pub(crate) const HOME_RULES: &[HomeRule] = &[
    // --- Python: uv ---
    HomeRule {
        rel: ".cache/uv",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "uv package cache — repopulated by the next `uv sync` or install",
    },
    HomeRule {
        rel: ".local/share/uv/python",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "uv-managed Python interpreters — re-downloaded per requested version",
    },
    HomeRule {
        rel: ".local/share/uv/tools",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "uv-installed CLI tools — restore with `uv tool install <name>`",
    },
    // --- Python: pipx ---
    HomeRule {
        rel: ".cache/pipx",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "pipx download cache — refetched on the next install or upgrade",
    },
    HomeRule {
        rel: ".local/pipx/venvs",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "pipx application virtualenvs — recreate with `pipx install <app>`",
    },
    // --- Conda ---
    HomeRule {
        rel: ".conda/pkgs",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "conda package cache — re-downloaded from the channel on demand",
    },
    HomeRule {
        rel: "miniconda3/pkgs",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "conda package cache — re-downloaded from the channel on demand",
    },
    HomeRule {
        rel: "anaconda3/pkgs",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "conda package cache — re-downloaded from the channel on demand",
    },
    HomeRule {
        rel: "miniforge3/pkgs",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "conda package cache — re-downloaded from the channel on demand",
    },
    // --- Haskell ---
    HomeRule {
        rel: ".cabal/packages",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "cabal download cache — packages re-fetched by the next build",
    },
    HomeRule {
        rel: ".ghcup/archive",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "GHC toolchain archives — re-downloaded per toolchain version",
    },
    HomeRule {
        rel: ".ghcup/tmp",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "GHC toolchain archives — re-downloaded per toolchain version",
    },
    HomeRule {
        rel: ".stack/indices",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "Stack package index — re-downloaded by the next build",
    },
    HomeRule {
        rel: ".stack/programs",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "Stack-managed GHC installs — re-downloaded per resolver",
    },
    // --- OCaml / Perl ---
    HomeRule {
        rel: ".opam/download-cache",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "opam source archive cache — re-fetched from the OCaml repositories",
    },
    HomeRule {
        rel: ".cpanm",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "cpanminus build cache — modules re-fetch from CPAN on install",
    },
    // --- PHP ---
    HomeRule {
        rel: ".cache/composer",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "Composer package cache — repopulated by `composer install`",
    },
    HomeRule {
        rel: ".composer/cache",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "Composer package cache — repopulated by `composer install`",
    },
    // --- .NET ---
    HomeRule {
        rel: ".nuget/packages",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "NuGet global packages — restored by `dotnet restore`",
    },
    // --- JavaScript runtimes ---
    HomeRule {
        rel: ".cache/deno",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "Deno remote-module cache — re-fetched automatically on next run",
    },
    HomeRule {
        rel: ".deno/gen",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "Deno generated code cache — rebuilt on the next run",
    },
    HomeRule {
        rel: ".bun/install/cache",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "Bun install cache — repopulated by the next `bun install`",
    },
    // --- Browser-automation binaries ---
    HomeRule {
        rel: ".cache/ms-playwright",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "Playwright browser binaries — re-download with `npx playwright install`",
    },
    // --- Flutter / Dart ---
    // The largest reclaimable miss on a Flutter developer's machine: the
    // downloaded engine, dart-sdk and web-sdk artifacts.
    HomeRule {
        rel: "flutter/bin/cache",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "Flutter engine artifacts — restored by `flutter precache` or any flutter command",
    },
    HomeRule {
        rel: ".dartServer/.analysis-driver",
        category: Category::IdeToolchains,
        severity: SAFE,
        note: "Dart analysis index — rebuilt silently by the analysis server",
    },
    HomeRule {
        rel: ".flutter-devtools",
        category: Category::IdeToolchains,
        severity: SAFE,
        note: "DevTools scratch state — recreated on the next session",
    },
    // --- Zig / LaTeX / Ruby ---
    HomeRule {
        rel: ".cache/zls",
        category: Category::IdeToolchains,
        severity: SAFE,
        note: "Zig language-server cache — rebuilt on demand",
    },
    // --- Neovim / Emacs toolchains ---
    HomeRule {
        rel: ".local/share/nvim/mason",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "Mason-installed language servers — reinstall from Mason",
    },
    HomeRule {
        rel: ".cache/nvim",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "Neovim cache — regenerated on the next start",
    },
    HomeRule {
        rel: ".emacs.d/.cache",
        category: Category::PackageCaches,
        severity: SAFE,
        note: "Emacs cache — regenerated on the next start",
    },
    // --- Misc downloaded CLI runtimes ---
    HomeRule {
        rel: ".skiko",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "downloaded Skiko/Compose native libraries — refetched by Gradle",
    },
    HomeRule {
        rel: ".fly/bin",
        category: Category::IdeToolchains,
        severity: MOD,
        note: "flyctl versioned binaries — reinstalled by `fly version upgrade`",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    fn mk(root: &std::path::Path, rel: &str) -> std::path::PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // Shared crate-wide lock (rules::TEST_ENV_LOCK) serializing every
    // rule-module test that mutates CHYSTIK_TEST_HOME. The retry helpers
    // below stay as defense-in-depth for anything bypassing the lock.
    use crate::rules::TEST_ENV_LOCK as ENV_LOCK;

    /// True iff the override currently points exactly at `fake`.
    fn env_points_at(fake: &std::path::Path) -> bool {
        std::env::var_os("CHYSTIK_TEST_HOME").is_some_and(|v| std::path::Path::new(&v) == fake)
    }

    /// Sibling rule-set modules keep their own locks yet mutate the same
    /// process-global CHYSTIK_TEST_HOME, so a concurrent test may flip or
    /// clear the variable between our fixture setup and a classify call.
    /// Only trust a result when the override pointed at our fixture both
    /// immediately before and immediately after the call; otherwise retry.
    fn stable_classify(dir: &std::path::Path, fake: &std::path::Path) -> Option<Match> {
        for _ in 0..250 {
            if env_points_at(fake) {
                let m = classify(dir);
                if env_points_at(fake) {
                    return m;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("CHYSTIK_TEST_HOME stayed contended by concurrent tests");
    }

    /// Same stability discipline for the no-override (real HOME) case.
    fn stable_classify_without_override(dir: &std::path::Path) -> Option<Match> {
        for _ in 0..250 {
            let absent_before = std::env::var_os("CHYSTIK_TEST_HOME").is_none();
            if absent_before {
                let m = classify(dir);
                if std::env::var_os("CHYSTIK_TEST_HOME").is_none() {
                    return m;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("CHYSTIK_TEST_HOME stayed contended by concurrent tests");
    }

    /// Runs `f` with CHYSTIK_TEST_HOME pointed at a fresh tempdir so
    /// fixtures never touch the real user HOME.
    fn with_fake_home(f: impl FnOnce(&std::path::Path)) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());
        f(fake.path());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn uv_caches_and_tool_dirs_match_with_expected_severity() {
        with_fake_home(|home| {
            let cache = stable_classify(&mk(home, ".cache/uv"), home).expect("uv cache matches");
            assert_eq!(cache.category, Category::PackageCaches);
            assert_eq!(cache.severity, Severity::Safe);

            let python =
                stable_classify(&mk(home, ".local/share/uv/python"), home).expect("uv pythons");
            assert_eq!(python.category, Category::IdeToolchains);
            assert_eq!(python.severity, Severity::Moderate);

            let tools =
                stable_classify(&mk(home, ".local/share/uv/tools"), home).expect("uv tools");
            assert_eq!(tools.severity, Severity::Moderate);

            // Non-matching siblings stay unclassified.
            assert!(stable_classify(&mk(home, ".cache/uvx"), home).is_none());
            assert!(stable_classify(&mk(home, ".local/share/uv/python311"), home).is_none());
            // Content deeper inside a matched root is not classified again.
            assert!(stable_classify(&mk(home, ".cache/uv/wheels-v6"), home).is_none());
        });
    }

    #[test]
    fn pipx_cache_is_safe_but_app_venvs_are_moderate() {
        with_fake_home(|home| {
            let cache = stable_classify(&mk(home, ".cache/pipx"), home).expect("pipx cache");
            assert_eq!(cache.category, Category::PackageCaches);
            assert_eq!(cache.severity, Severity::Safe);

            let venvs = stable_classify(&mk(home, ".local/pipx/venvs"), home).expect("pipx venvs");
            assert_eq!(venvs.severity, Severity::Moderate);

            assert!(stable_classify(&mk(home, ".local/pipx/venv"), home).is_none());
        });
    }

    #[test]
    fn conda_package_caches_match_across_install_layouts() {
        with_fake_home(|home| {
            for rel in [
                ".conda/pkgs",
                "miniconda3/pkgs",
                "anaconda3/pkgs",
                "miniforge3/pkgs",
            ] {
                let m = stable_classify(&mk(home, rel), home).expect("conda pkg cache");
                assert_eq!(m.category, Category::PackageCaches);
                assert_eq!(m.severity, Severity::Safe);
            }
            assert!(stable_classify(&mk(home, ".conda/envs"), home).is_none());
            assert!(stable_classify(&mk(home, "miniconda3/pkgs64"), home).is_none());
        });
    }

    #[test]
    fn ghcup_archives_are_moderate_while_cabal_cache_stays_safe() {
        with_fake_home(|home| {
            let cabal =
                stable_classify(&mk(home, ".cabal/packages"), home).expect("cabal packages");
            assert_eq!(cabal.severity, Severity::Safe);

            let arch = stable_classify(&mk(home, ".ghcup/archive"), home).expect("ghcup archive");
            assert_eq!(arch.severity, Severity::Moderate);
            assert_eq!(
                stable_classify(&mk(home, ".ghcup/tmp"), home)
                    .unwrap()
                    .severity,
                Severity::Moderate
            );

            assert!(stable_classify(&mk(home, ".cabal/store"), home).is_none());
            // Installed toolchain binaries are deliberately not claimed.
            assert!(stable_classify(&mk(home, ".ghcup/bin"), home).is_none());
        });
    }

    #[test]
    fn opam_and_cpanm_caches_match() {
        with_fake_home(|home| {
            let opam = stable_classify(&mk(home, ".opam/download-cache"), home)
                .expect("opam download-cache");
            assert_eq!(opam.category, Category::PackageCaches);
            assert_eq!(opam.severity, Severity::Safe);

            let cpanm = stable_classify(&mk(home, ".cpanm"), home).expect("cpanm");
            assert_eq!(cpanm.category, Category::PackageCaches);

            assert!(stable_classify(&mk(home, ".opam/root"), home).is_none());
            assert!(stable_classify(&mk(home, ".cpan"), home).is_none());
        });
    }

    #[test]
    fn composer_cache_matches_both_default_locations() {
        with_fake_home(|home| {
            let xdg =
                stable_classify(&mk(home, ".cache/composer"), home).expect("XDG composer cache");
            assert_eq!(xdg.category, Category::PackageCaches);
            assert!(stable_classify(&mk(home, ".composer/cache"), home).is_some());

            assert!(stable_classify(&mk(home, ".composer/local"), home).is_none());
        });
    }

    #[test]
    fn nuget_global_packages_match() {
        with_fake_home(|home| {
            let m = stable_classify(&mk(home, ".nuget/packages"), home).expect("nuget packages");
            assert_eq!(m.category, Category::PackageCaches);
            assert_eq!(m.severity, Severity::Safe);

            assert!(stable_classify(&mk(home, ".nuget/plugins"), home).is_none());
        });
    }

    #[test]
    fn deno_and_bun_caches_match() {
        with_fake_home(|home| {
            let deno = stable_classify(&mk(home, ".cache/deno"), home).expect("deno cache");
            assert_eq!(deno.category, Category::PackageCaches);
            assert_eq!(deno.severity, Severity::Safe);

            let bun = stable_classify(&mk(home, ".bun/install/cache"), home).expect("bun cache");
            assert_eq!(bun.category, Category::PackageCaches);

            assert!(stable_classify(&mk(home, ".cache/denoland"), home).is_none());
            assert!(stable_classify(&mk(home, ".bun/install/global"), home).is_none());
        });
    }

    #[test]
    fn playwright_browser_binaries_are_moderate_toolchains() {
        with_fake_home(|home| {
            let m = stable_classify(&mk(home, ".cache/ms-playwright"), home).expect("playwright");
            assert_eq!(m.category, Category::IdeToolchains);
            assert_eq!(m.severity, Severity::Moderate);

            assert!(stable_classify(&mk(home, ".cache/playwright-browsers"), home).is_none());
        });
    }

    #[test]
    fn paths_outside_home_never_match_even_with_known_suffix() {
        // Without the override a tempdir is not below the real HOME, so even
        // a directory named like a known cache stays unclassified. Runs under
        // the module lock and tolerates sibling tests re-setting the variable
        // concurrently via the stability-checked wrapper.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var("CHYSTIK_TEST_HOME");

        let root = tempdir().unwrap();
        let pw = mk(root.path(), ".cache/ms-playwright");
        let uv = mk(root.path(), "somewhere/.cache/uv");
        assert!(stable_classify_without_override(&pw).is_none());
        assert!(stable_classify_without_override(&uv).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }
}

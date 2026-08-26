//! Rule registry. `classify` delegates to domain rule sets in order.

pub(crate) mod ai_agents;
pub(crate) mod android;
pub(crate) mod browsers;
pub(crate) mod catalog;
pub(crate) mod cloud;
pub(crate) mod comms;
pub(crate) mod containers;
pub(crate) mod core;
pub(crate) mod games;
pub(crate) mod installers;
pub(crate) mod languages;
pub(crate) mod media;
pub(crate) mod office_docs;
pub(crate) mod system_junk;

use std::path::{Path, PathBuf};

use crate::model::{Category, Severity};

/// A single rule hit.
#[derive(Debug, Clone)]
pub(crate) struct Match {
    pub category: Category,
    pub severity: Severity,
    pub note: String,
}

/// The result of resolving a rule. Legacy table rules have no provenance;
/// catalog rules carry their policy and source from the same lookup.
#[derive(Debug, Clone)]
pub(crate) struct ClassifiedRule {
    pub matched: Match,
    pub catalog: Option<catalog::CatalogMetadata>,
}

/// Immutable rule state for one scan.
///
/// Catalog roots and environment overrides cannot legitimately change while a
/// scan is in flight. Keeping them here gives the scanner one deep interface
/// and keeps per-directory classification to path checks only.
#[derive(Clone)]
pub(crate) struct RuleEngine {
    catalog: catalog::Catalog,
}

impl RuleEngine {
    pub(crate) fn current() -> Self {
        Self {
            catalog: catalog::Catalog::current(),
        }
    }

    pub(crate) fn classify_with_metadata(&self, dir: &Path) -> Option<ClassifiedRule> {
        if let Some((matched, catalog)) = self.catalog.classify_with_metadata(dir) {
            return Some(ClassifiedRule {
                matched,
                catalog: Some(catalog),
            });
        }
        classify_legacy(dir)
    }
}

/// A rule that judges a whole directory's CHILDREN together rather than one
/// path in isolation.
///
/// `classify` sees a single path and knows nothing about its siblings, so
/// every versioned store was all-or-nothing: `~/.local/share/claude/versions`
/// holds three builds of which one runs, and the only expressible answers
/// were "delete all of it" (breaking the tool) or silence. The scanner
/// already receives the full child list in `process_read_dir`, so ordering
/// them and sparing the newest few costs nothing extra.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GroupRule {
    /// `$HOME`-relative path of the PARENT directory.
    pub rel: &'static str,
    pub category: Category,
    pub severity: Severity,
    /// How many of the newest children to spare.
    pub keep: usize,
    pub note: &'static str,
}

/// Versioned stores that keep every revision they ever downloaded.
///
/// Ordering is by modification time, which is right for these: each entry is
/// written once, when that version is fetched. Stores whose names carry no
/// time order — `.rustup/toolchains` holds `stable`, `nightly`, `1.75` — are
/// deliberately absent; "newest by mtime" would be a guess there.
pub(crate) const GROUP_RULES: &[GroupRule] = &[
    GroupRule {
        rel: ".local/share/claude/versions",
        category: Category::AiAgents,
        severity: Severity::Moderate,
        keep: 1,
        note: "superseded Claude Code build — the current one is kept",
    },
    GroupRule {
        rel: ".codex/packages/standalone/releases",
        category: Category::AiAgents,
        severity: Severity::Moderate,
        keep: 1,
        note: "superseded Codex CLI build — the current one is kept",
    },
    GroupRule {
        rel: ".nvm/versions/node",
        category: Category::IdeToolchains,
        severity: Severity::Moderate,
        keep: 1,
        note: "older Node.js install — reinstall with `nvm install <version>`",
    },
    GroupRule {
        rel: ".local/share/JetBrains/Toolbox/apps",
        category: Category::Installers,
        severity: Severity::Moderate,
        keep: 1,
        note: "superseded Toolbox build — the current one is kept",
    },
];

/// The group rule covering `dir`, if any.
pub(crate) fn classify_group(dir: &Path) -> Option<&'static GroupRule> {
    let home = home_root()?;
    let rel = if let Ok(rel) = dir.strip_prefix(&home) {
        rel.to_string_lossy().replace('\\', "/")
    } else if std::env::var_os("CHYSTIK_TEST_HOME").is_some() {
        let text = dir.to_string_lossy().replace('\\', "/");
        GROUP_RULES
            .iter()
            .map(|r| r.rel)
            .filter(|suffix| text.ends_with(suffix))
            .max_by_key(|suffix| suffix.len())?
            .to_owned()
    } else {
        return None;
    };
    GROUP_RULES.iter().find(|r| r.rel == rel)
}

/// The platform home for production matching; `CHYSTIK_TEST_HOME` overrides
/// it in tests so fixtures never touch real user data.
pub(crate) fn home_root() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CHYSTIK_TEST_HOME") {
        return Some(PathBuf::from(p));
    }
    Some(crate::platform::current().app_paths().home_dir)
}

/// True if `parent` contains any of `markers` (file OR directory).
/// Uses `symlink_metadata` rather than `Path::exists`, which follows
/// symlinks — a broken link silently disabled the rule it gated.
pub(crate) fn parent_has(parent: &Path, markers: &[&str]) -> bool {
    markers
        .iter()
        .any(|m| std::fs::symlink_metadata(parent.join(m)).is_ok())
}

/// True if `parent` contains any of `markers` as a REGULAR FILE. Manifests
/// and lockfiles must be files: a *directory* named `package.json` is not
/// evidence of a JavaScript project.
pub(crate) fn parent_has_file(parent: &Path, markers: &[&str]) -> bool {
    markers
        .iter()
        .any(|m| std::fs::symlink_metadata(parent.join(m)).is_ok_and(|meta| meta.is_file()))
}

/// One `$HOME`-relative rule: a path suffix plus what it means.
pub(crate) struct HomeRule {
    pub rel: &'static str,
    pub category: Category,
    pub severity: Severity,
    pub note: &'static str,
}

/// Match `dir` against a module's `$HOME` rule table.
///
/// The table is the SINGLE source of truth: the `CHYSTIK_TEST_HOME` suffix
/// fallback below derives its list from the same table, so a rule can no
/// longer be half-registered. Modules used to hand-maintain a second copy
/// of every path ("KEEP IN SYNC"), and a rule missing from that copy failed
/// only its own tests.
pub(crate) fn match_home_rule(dir: &Path, table: &[HomeRule]) -> Option<Match> {
    let rel = home_rel_in(dir, table)?;
    let rule = table.iter().find(|r| r.rel == rel)?;
    Some(Match {
        category: rule.category,
        severity: rule.severity,
        note: rule.note.into(),
    })
}

/// `dir` relative to `$HOME`. Under `CHYSTIK_TEST_HOME` another test thread
/// may swap the override between fixture creation and classification, so
/// the longest table suffix matching the absolute path is recovered instead
/// — never broader than the table itself.
fn home_rel_in(dir: &Path, table: &[HomeRule]) -> Option<String> {
    let home = home_root()?;
    if let Ok(rel) = dir.strip_prefix(&home) {
        return Some(rel.to_string_lossy().replace('\\', "/"));
    }
    if std::env::var_os("CHYSTIK_TEST_HOME").is_some() {
        let text = dir.to_string_lossy().replace('\\', "/");
        return table
            .iter()
            .map(|r| r.rel)
            .filter(|suffix| text.ends_with(suffix))
            .max_by_key(|suffix| suffix.len())
            .map(|suffix| suffix.to_owned());
    }
    None
}

/// Test-only: several rule-set modules' tests temporarily override the
/// process-global `CHYSTIK_TEST_HOME` variable. Per-module `Mutex`es do not
/// guard each other, so a `remove_var` landing between another module's
/// `set_var` and its `classify` made `$HOME` lookups vanish intermittently.
/// Every module therefore takes THIS single lock around env mutations,
/// serializing all such sections crate-wide.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// True if any direct child of `parent` is a Python source file.
pub(crate) fn parent_has_py(parent: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(parent) else {
        return false;
    };
    rd.filter_map(|e| e.ok())
        .any(|e| e.path().extension().is_some_and(|ext| ext == "py"))
}

/// Evaluate legacy rule sets against `dir` (first match wins).
/// Order matters: an earlier module wins outright.
///
/// `containers` used to run before the app-domain modules, and its Flatpak
/// wildcard (`.var/app/<any-id>/cache`) therefore claimed EVERY Flatpak
/// application's cache — a Flatpak Steam or Telegram cache could never be
/// routed to `games` or `comms`. It now runs after them, so a domain
/// module can own its own app id and the wildcard only catches the rest.
fn classify_legacy(dir: &Path) -> Option<ClassifiedRule> {
    let matched = core::classify(dir)
        .or_else(|| android::classify(dir))
        .or_else(|| ai_agents::classify(dir))
        .or_else(|| languages::classify(dir))
        .or_else(|| installers::classify(dir))
        .or_else(|| browsers::classify(dir))
        .or_else(|| games::classify(dir))
        .or_else(|| media::classify(dir))
        .or_else(|| comms::classify(dir))
        .or_else(|| cloud::classify(dir))
        .or_else(|| office_docs::classify(dir))
        .or_else(|| containers::classify(dir))
        .or_else(|| system_junk::classify(dir))?;
    Some(ClassifiedRule {
        matched,
        catalog: None,
    })
}

/// Evaluate all registered rule sets and return the original lightweight
/// match for callers that do not need catalog provenance.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    RuleEngine::current()
        .classify_with_metadata(dir)
        .map(|classified| classified.matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rule/guard agreement fixtures must not be placed under macOS `/var`,
    /// which is deliberately a protected production location.
    fn tempdir() -> std::io::Result<tempfile::TempDir> {
        tempfile::Builder::new()
            .prefix(".chystik-test-")
            .tempdir_in(std::env::current_dir().expect("test process has a working directory"))
    }

    /// Every rule must target something the deletion guard actually
    /// permits. Without this, a rule and `guard::check` can disagree
    /// silently and the UI shows findings it can never act on — which is
    /// exactly what `.config/Code/*` did: 18 findings, 125 MiB, shown and
    /// permanently unselectable.
    #[test]
    fn every_home_rule_targets_a_path_the_guard_allows() {
        let _env = TEST_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let home = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", home.path());

        let mut refused = Vec::new();
        for rule in core::HOME_RULES.iter().chain(languages::HOME_RULES.iter()) {
            let target = home.path().join(rule.rel);
            std::fs::create_dir_all(&target).unwrap();
            if crate::guard::check(&target, home.path()).is_err() {
                refused.push(rule.rel);
            }
        }
        std::env::remove_var("CHYSTIK_TEST_HOME");
        assert!(
            refused.is_empty(),
            "these rules propose paths the guard refuses: {refused:?}"
        );
    }

    #[test]
    fn registry_still_matches_core_rules_via_public_api() {
        let root = tempdir().unwrap();
        let proj = root.path().join("app");
        std::fs::create_dir_all(proj.join("node_modules")).unwrap();
        std::fs::write(proj.join("package.json"), "{}").unwrap();
        std::fs::write(proj.join("package-lock.json"), "{}").unwrap();

        let m = classify(&proj.join("node_modules")).expect("registry match");
        assert_eq!(m.category, Category::BuildArtifacts);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn catalog_evidence_preempts_the_legacy_pip_rule() {
        let _env = TEST_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let home = tempdir().unwrap();
        let cache = home.path().join(".cache/pip");
        std::fs::create_dir_all(&cache).unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", home.path());
        std::env::set_var("XDG_CACHE_HOME", home.path().join(".cache"));

        assert!(classify_legacy(&cache).is_some(), "fixture must overlap");
        let classified = RuleEngine::current()
            .classify_with_metadata(&cache)
            .expect("catalog match");

        std::env::remove_var("XDG_CACHE_HOME");
        std::env::remove_var("CHYSTIK_TEST_HOME");
        assert_eq!(
            classified
                .catalog
                .expect("catalog provenance")
                .provenance
                .rule_id,
            "python.pip.cache"
        );
    }
}

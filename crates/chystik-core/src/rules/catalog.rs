//! Evidence-backed, declarative cross-platform cleanup catalog.
//!
//! Contributors edit TOML under `rules/catalog/`. `build.rs` validates every
//! file and embeds the reviewed sources into the released binary; this module
//! then resolves only exact platform-owned targets once per scan. The guard is
//! still the final cleanup authority.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::model::{Category, FindingPolicy, RuleProvenance, Severity};
use crate::platform::{Platform, PlatformKind, RuleRoots};

use super::catalog_schema::{self, RawLocator, RawRule};
use super::Match;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/catalog_sources.rs"));
}

#[derive(Debug, Clone)]
pub(crate) struct CatalogMetadata {
    pub provenance: RuleProvenance,
    pub advice: Option<String>,
}

#[derive(Clone)]
struct RuleContext {
    platform: Platform,
    kind: PlatformKind,
    roots: RuleRoots,
    environment: BTreeMap<String, PathBuf>,
}

impl RuleContext {
    fn current() -> Self {
        let platform = crate::platform::current();
        let environment = catalog_schema::ENVIRONMENT_OVERRIDES
            .iter()
            .filter_map(|name| {
                std::env::var_os(name)
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|path| ((*name).to_owned(), path))
            })
            .collect();
        Self {
            platform,
            kind: platform.kind(),
            roots: platform.rule_roots(),
            environment,
        }
    }

    fn environment_target(&self, name: &str) -> Option<&Path> {
        let path = self.environment.get(name)?.as_path();
        self.is_owned_user_path(path).then_some(path)
    }

    fn is_owned_user_path(&self, path: &Path) -> bool {
        [
            Some(&self.roots.home_dir),
            Some(&self.roots.cache_dir),
            self.roots.local_app_data_dir.as_ref(),
            self.roots.library_caches_dir.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|root| is_under(path, root, self.kind))
    }
}

/// Resolved catalog state for one scan. Platform roots, accepted environment
/// overrides, and exact targets remain immutable during the scan.
#[derive(Clone)]
pub(crate) struct Catalog {
    context: RuleContext,
    fixed_targets: Vec<FixedTarget>,
    marker_targets: Vec<MarkerTarget>,
}

#[derive(Clone)]
struct FixedTarget {
    path: PathBuf,
    rule: RuleSpec,
}

#[derive(Clone)]
struct MarkerTarget {
    root: PathBuf,
    matcher: MarkerMatcher,
    required_files: Vec<String>,
    required_dirs: Vec<String>,
    rule: RuleSpec,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkerMatcher {
    Descendant,
    DirectChild,
}

impl Catalog {
    pub(crate) fn current() -> Self {
        Self::from_context(RuleContext::current())
    }

    pub(crate) fn classify_with_metadata(&self, dir: &Path) -> Option<(Match, CatalogMetadata)> {
        if self.context.platform.is_link_or_reparse_point(dir) {
            return None;
        }
        let rule = self
            .fixed_targets
            .iter()
            .find(|target| path_eq(dir, &target.path, self.context.kind))
            .map(|target| &target.rule)
            .or_else(|| {
                self.marker_targets
                    .iter()
                    .find(|target| target.matches(dir, self.context.kind))
                    .map(|target| &target.rule)
            })?;
        Some((rule.to_match(), rule.metadata()))
    }

    fn from_context(context: RuleContext) -> Self {
        let mut fixed_targets = Vec::new();
        let mut marker_targets = Vec::new();
        for raw in catalog_rules() {
            let rule = RuleSpec::from_raw(raw);
            for locator in &raw.locator {
                if !platform_matches(&locator.platform, context.kind) {
                    continue;
                }
                let Some(root) = resolve_root(locator, &context) else {
                    continue;
                };
                let target = join_relative(&root, locator.path.as_deref());
                match locator.matcher.as_str() {
                    "exact" => fixed_targets.push(FixedTarget {
                        path: target,
                        rule: rule.clone(),
                    }),
                    "descendant-with-markers" => marker_targets.push(MarkerTarget {
                        root: target,
                        matcher: MarkerMatcher::Descendant,
                        required_files: locator.required_files.clone(),
                        required_dirs: locator.required_dirs.clone(),
                        rule: rule.clone(),
                    }),
                    "direct-child-with-markers" => marker_targets.push(MarkerTarget {
                        root: target,
                        matcher: MarkerMatcher::DirectChild,
                        required_files: locator.required_files.clone(),
                        required_dirs: locator.required_dirs.clone(),
                        rule: rule.clone(),
                    }),
                    _ => unreachable!("validated catalog matcher"),
                }
            }
        }
        Self {
            context,
            fixed_targets,
            marker_targets,
        }
    }
}

impl MarkerTarget {
    fn matches(&self, dir: &Path, kind: PlatformKind) -> bool {
        let location_matches = match self.matcher {
            MarkerMatcher::Descendant => is_under(dir, &self.root, kind),
            MarkerMatcher::DirectChild => dir
                .parent()
                .is_some_and(|parent| path_eq(parent, &self.root, kind)),
        };
        location_matches
            && self
                .required_files
                .iter()
                .all(|name| dir.join(name).is_file())
            && self
                .required_dirs
                .iter()
                .all(|name| dir.join(name).is_dir())
    }
}

#[derive(Debug, Clone)]
struct RuleSpec {
    id: String,
    category: Category,
    severity: Severity,
    policy: FindingPolicy,
    note: String,
    source_url: String,
    recovery_cost: String,
    reviewed_at: String,
    preconditions: Vec<String>,
    advice: Option<String>,
}

impl RuleSpec {
    fn from_raw(raw: &RawRule) -> Self {
        Self {
            id: raw.id.clone(),
            category: category(&raw.category),
            severity: recovery(&raw.recovery),
            policy: cleanup_policy(&raw.cleanup_policy),
            note: raw.note.clone(),
            source_url: raw.source_url.clone(),
            recovery_cost: raw.recovery_note.clone(),
            reviewed_at: raw.reviewed_at.clone(),
            preconditions: raw.preconditions.clone(),
            advice: raw.advice.clone(),
        }
    }

    fn to_match(&self) -> Match {
        Match {
            category: self.category,
            severity: self.severity,
            note: self.note.clone(),
        }
    }

    fn metadata(&self) -> CatalogMetadata {
        CatalogMetadata {
            provenance: RuleProvenance {
                rule_id: self.id.clone(),
                source_url: self.source_url.clone(),
                policy: self.policy,
                recovery_cost: self.recovery_cost.clone(),
                reviewed_at: self.reviewed_at.clone(),
                preconditions: self.preconditions.clone(),
            },
            advice: self.advice.clone(),
        }
    }
}

fn catalog_rules() -> &'static [RawRule] {
    static RULES: OnceLock<Vec<RawRule>> = OnceLock::new();
    RULES
        .get_or_init(|| {
            let mut rules = Vec::new();
            for (name, source) in generated::CATALOG_SOURCES {
                rules.extend(
                    catalog_schema::parse_catalog(name, source)
                        .expect("catalog was validated at build time"),
                );
            }
            catalog_schema::validate_catalog(&rules).expect("catalog remains valid at runtime");
            rules
        })
        .as_slice()
}

fn platform_matches(platform: &str, kind: PlatformKind) -> bool {
    matches!(
        (platform, kind),
        ("all", _)
            | ("linux", PlatformKind::Linux)
            | ("macos", PlatformKind::MacOS)
            | ("windows", PlatformKind::Windows)
    )
}

fn resolve_root(locator: &RawLocator, context: &RuleContext) -> Option<PathBuf> {
    match locator.root.as_str() {
        "home" => Some(context.roots.home_dir.clone()),
        "cache" => Some(context.roots.cache_dir.clone()),
        "local-app-data" => context.roots.local_app_data_dir.clone(),
        "roaming-app-data" => context.roots.roaming_app_data_dir.clone(),
        "library-caches" => context.roots.library_caches_dir.clone(),
        "developer" => context.roots.developer_dir.clone(),
        "volume-root" => context.roots.volume_root.clone(),
        "environment" => locator
            .environment
            .as_deref()
            .and_then(|name| context.environment_target(name))
            .map(Path::to_path_buf),
        _ => unreachable!("validated catalog root"),
    }
}

fn join_relative(root: &Path, relative: Option<&str>) -> PathBuf {
    let mut target = root.to_path_buf();
    if let Some(relative) = relative {
        for component in relative.split('/') {
            target.push(component);
        }
    }
    target
}

fn category(value: &str) -> Category {
    match value {
        "build-artifacts" => Category::BuildArtifacts,
        "package-caches" => Category::PackageCaches,
        "ide-toolchains" => Category::IdeToolchains,
        "ai-models" => Category::AiModels,
        "browser-system" => Category::BrowserSystem,
        "android-dev" => Category::AndroidDev,
        "ai-agents" => Category::AiAgents,
        "containers" => Category::Containers,
        "installers" => Category::Installers,
        "game-launchers" => Category::GameLaunchers,
        "media-apps" => Category::MediaApps,
        "messengers" => Category::Messengers,
        "cloud-sync" => Category::CloudSync,
        "office-docs" => Category::OfficeDocs,
        "system-junk" => Category::SystemJunk,
        _ => unreachable!("validated catalog category"),
    }
}

fn recovery(value: &str) -> Severity {
    match value {
        "automatic" => Severity::Safe,
        "rebuild-redownload" => Severity::Moderate,
        "manual-irreplaceable" => Severity::Risky,
        _ => unreachable!("validated catalog recovery"),
    }
}

fn cleanup_policy(value: &str) -> FindingPolicy {
    match value {
        "auto-cleanable" => FindingPolicy::DirectSafe,
        "review-required" => FindingPolicy::DirectReview,
        "tool-managed" => FindingPolicy::VendorCommandOnly,
        "advisory-only" => FindingPolicy::AdvisoryOnly,
        _ => unreachable!("validated catalog cleanup policy"),
    }
}

fn is_under(path: &Path, root: &Path, kind: PlatformKind) -> bool {
    if kind != PlatformKind::Windows {
        return path.starts_with(root);
    }
    let normalized = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
    let path = normalized(path);
    let root = normalized(root);
    path == root || path.starts_with(&(root + "\\"))
}

fn path_eq(left: &Path, right: &Path, kind: PlatformKind) -> bool {
    if kind == PlatformKind::Windows {
        left.to_string_lossy()
            .replace('/', "\\")
            .eq_ignore_ascii_case(&right.to_string_lossy().replace('/', "\\"))
    } else {
        left == right
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use tempfile::tempdir;

    const REVIEWED_AT: &str = "2026-08-30";

    fn context(kind: PlatformKind, root: &Path) -> RuleContext {
        RuleContext {
            platform: crate::platform::current(),
            kind,
            roots: RuleRoots {
                home_dir: root.join("home"),
                cache_dir: root.join("cache"),
                local_app_data_dir: Some(root.join("local")),
                roaming_app_data_dir: Some(root.join("roaming")),
                library_caches_dir: Some(root.join("Library/Caches")),
                developer_dir: Some(root.join("Library/Developer")),
                volume_root: Some(root.join("volume")),
            },
            environment: BTreeMap::new(),
        }
    }

    fn details_in(dir: &Path, context: &RuleContext) -> CatalogMetadata {
        Catalog::from_context(context.clone())
            .classify_with_metadata(dir)
            .expect("catalog match")
            .1
    }

    fn is_unmatched(dir: &Path, context: &RuleContext) -> bool {
        Catalog::from_context(context.clone())
            .classify_with_metadata(dir)
            .is_none()
    }

    #[test]
    fn declarative_catalog_retains_the_migrated_rule_contract() {
        let expected = [
            (
                "python.pip.cache",
                Category::PackageCaches,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "ios.cocoapods.cache",
                Category::PackageCaches,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "cpp.ccache",
                Category::PackageCaches,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "cpp.sccache",
                Category::PackageCaches,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "cpp.vcpkg.binary-archives",
                Category::PackageCaches,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "nvidia.optix.cache",
                Category::PackageCaches,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "nvidia.optix.cache.custom-location",
                Category::PackageCaches,
                Severity::Safe,
                FindingPolicy::DirectReview,
            ),
            (
                "nvidia.extracted-installer",
                Category::Installers,
                Severity::Moderate,
                FindingPolicy::DirectReview,
            ),
            (
                "amd.extracted-installer",
                Category::Installers,
                Severity::Moderate,
                FindingPolicy::DirectReview,
            ),
            (
                "xcode.derived-data",
                Category::IdeToolchains,
                Severity::Moderate,
                FindingPolicy::DirectReview,
            ),
            (
                "xcode.ios-device-support",
                Category::IdeToolchains,
                Severity::Moderate,
                FindingPolicy::DirectReview,
            ),
            (
                "cpp.conan-cache",
                Category::PackageCaches,
                Severity::Moderate,
                FindingPolicy::VendorCommandOnly,
            ),
            (
                "windows.directx-shader-cache",
                Category::SystemJunk,
                Severity::Safe,
                FindingPolicy::AdvisoryOnly,
            ),
            (
                "amd.shader-cache",
                Category::SystemJunk,
                Severity::Safe,
                FindingPolicy::VendorCommandOnly,
            ),
            (
                "xcode.unavailable-simulators",
                Category::IdeToolchains,
                Severity::Moderate,
                FindingPolicy::VendorCommandOnly,
            ),
            (
                "jetbrains.ide-system-dir",
                Category::IdeToolchains,
                Severity::Moderate,
                FindingPolicy::DirectReview,
            ),
            (
                "jetbrains.toolbox-cache",
                Category::IdeToolchains,
                Severity::Moderate,
                FindingPolicy::DirectReview,
            ),
            (
                "google.android-studio-system-dir",
                Category::IdeToolchains,
                Severity::Moderate,
                FindingPolicy::DirectReview,
            ),
            (
                "dart.pub-cache",
                Category::PackageCaches,
                Severity::Moderate,
                FindingPolicy::DirectReview,
            ),
            (
                "windows.crash-dumps",
                Category::SystemJunk,
                Severity::Safe,
                FindingPolicy::DirectReview,
            ),
            (
                "windows.temp",
                Category::SystemJunk,
                Severity::Safe,
                FindingPolicy::AdvisoryOnly,
            ),
            (
                "microsoft.vscode-cache",
                Category::IdeToolchains,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "google.chrome-cache",
                Category::BrowserSystem,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "perplexity.comet-cache",
                Category::BrowserSystem,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "discord.cache",
                Category::Messengers,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "notion.cache",
                Category::OfficeDocs,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "antigravity.cache",
                Category::IdeToolchains,
                Severity::Safe,
                FindingPolicy::DirectSafe,
            ),
            (
                "epic.webcache",
                Category::GameLaunchers,
                Severity::Moderate,
                FindingPolicy::DirectReview,
            ),
            (
                "valve.steam-htmlcache",
                Category::GameLaunchers,
                Severity::Moderate,
                FindingPolicy::DirectReview,
            ),
        ];
        let mut actual: Vec<_> = catalog_rules()
            .iter()
            .map(|raw| {
                let rule = RuleSpec::from_raw(raw);
                (
                    rule.id,
                    rule.category.as_str(),
                    rule.severity.as_str(),
                    rule.policy.as_str(),
                )
            })
            .collect();
        let mut expected: Vec<_> = expected
            .into_iter()
            .map(|(id, category, severity, policy)| {
                (
                    id.to_owned(),
                    category.as_str(),
                    severity.as_str(),
                    policy.as_str(),
                )
            })
            .collect();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
    }

    #[test]
    fn contributor_category_identifiers_match_the_validator_schema() {
        assert_eq!(category("game-launchers"), Category::GameLaunchers);
        assert_eq!(category("media-apps"), Category::MediaApps);
    }

    #[test]
    fn direct_safe_defaults_are_exact_and_keep_siblings_out() {
        let root = tempdir().unwrap();
        let context = context(PlatformKind::Linux, root.path());
        for (target, rule_id, policy) in [
            (
                context.roots.cache_dir.join("pip"),
                "python.pip.cache",
                FindingPolicy::DirectSafe,
            ),
            (
                context.roots.home_dir.join(".ccache"),
                "cpp.ccache",
                FindingPolicy::DirectSafe,
            ),
            (
                context.roots.cache_dir.join("ccache"),
                "cpp.ccache",
                FindingPolicy::DirectSafe,
            ),
            (
                context.roots.cache_dir.join("sccache"),
                "cpp.sccache",
                FindingPolicy::DirectSafe,
            ),
            (
                context.roots.cache_dir.join("vcpkg/archives"),
                "cpp.vcpkg.binary-archives",
                FindingPolicy::DirectSafe,
            ),
            (
                context.roots.home_dir.join(".conan2/p"),
                "cpp.conan-cache",
                FindingPolicy::VendorCommandOnly,
            ),
        ] {
            std::fs::create_dir_all(&target).unwrap();
            let metadata = details_in(&target, &context);
            assert_eq!(metadata.provenance.rule_id, rule_id);
            assert_eq!(metadata.provenance.policy, policy);
            assert_eq!(metadata.provenance.reviewed_at, REVIEWED_AT);
            assert!(!metadata.provenance.preconditions.is_empty());
        }
        assert!(is_unmatched(&context.roots.cache_dir, &context));
        assert!(is_unmatched(
            &context.roots.cache_dir.join("pip-extra"),
            &context
        ));
    }

    #[test]
    fn environment_override_must_remain_inside_an_owned_root() {
        let root = tempdir().unwrap();
        let mut context = context(PlatformKind::Linux, root.path());
        for (variable, suffix, rule_id, policy) in [
            (
                "PIP_CACHE_DIR",
                "custom/pip",
                "python.pip.cache",
                FindingPolicy::DirectSafe,
            ),
            (
                "CCACHE_DIR",
                "custom/ccache",
                "cpp.ccache",
                FindingPolicy::DirectSafe,
            ),
            (
                "SCCACHE_DIR",
                "custom/sccache",
                "cpp.sccache",
                FindingPolicy::DirectSafe,
            ),
            (
                "VCPKG_DEFAULT_BINARY_CACHE",
                "custom/vcpkg",
                "cpp.vcpkg.binary-archives",
                FindingPolicy::DirectSafe,
            ),
            (
                "OPTIX_CACHE_PATH",
                "custom/optix",
                "nvidia.optix.cache.custom-location",
                FindingPolicy::DirectReview,
            ),
        ] {
            let allowed = context.roots.home_dir.join(suffix);
            std::fs::create_dir_all(&allowed).unwrap();
            context.environment.insert(variable.into(), allowed.clone());
            let metadata = details_in(&allowed, &context);
            assert_eq!(metadata.provenance.rule_id, rule_id);
            assert_eq!(metadata.provenance.policy, policy);
        }
        let outside = root.path().join("external/pip");
        std::fs::create_dir_all(&outside).unwrap();
        context
            .environment
            .insert("PIP_CACHE_DIR".into(), outside.clone());
        assert!(is_unmatched(&outside, &context));
    }

    #[test]
    fn windows_roots_markers_and_advisories_keep_siblings_out() {
        let root = tempdir().unwrap();
        let context = context(PlatformKind::Windows, root.path());
        let local = context.roots.local_app_data_dir.as_ref().unwrap();
        for (target, rule_id, policy) in [
            (
                local.join("pip/Cache"),
                "python.pip.cache",
                FindingPolicy::DirectSafe,
            ),
            (
                local.join("ccache"),
                "cpp.ccache",
                FindingPolicy::DirectSafe,
            ),
            (
                local.join("Mozilla/sccache"),
                "cpp.sccache",
                FindingPolicy::DirectSafe,
            ),
            (
                local.join("vcpkg/archives"),
                "cpp.vcpkg.binary-archives",
                FindingPolicy::DirectSafe,
            ),
            (
                local.join("NVIDIA/OptixCache"),
                "nvidia.optix.cache",
                FindingPolicy::DirectSafe,
            ),
            (
                local.join("D3DSCache"),
                "windows.directx-shader-cache",
                FindingPolicy::AdvisoryOnly,
            ),
            (
                local.join("AMD/DxCache"),
                "amd.shader-cache",
                FindingPolicy::VendorCommandOnly,
            ),
        ] {
            std::fs::create_dir_all(&target).unwrap();
            let metadata = details_in(&target, &context);
            assert_eq!(metadata.provenance.rule_id, rule_id);
            assert_eq!(metadata.provenance.policy, policy);
        }
        assert!(is_unmatched(&local.join("vcpkg"), &context));

        let nvidia = context
            .roots
            .volume_root
            .as_ref()
            .unwrap()
            .join("NVIDIA/DisplayDriver/1/en-US");
        std::fs::create_dir_all(nvidia.join("Display.Driver")).unwrap();
        std::fs::write(nvidia.join("setup.exe"), "fixture").unwrap();
        assert_eq!(
            details_in(&nvidia, &context).provenance.policy,
            FindingPolicy::DirectReview
        );
        assert!(is_unmatched(nvidia.parent().unwrap(), &context));

        let amd = context.roots.volume_root.as_ref().unwrap().join("AMD/24.9");
        std::fs::create_dir_all(amd.join("Packages")).unwrap();
        std::fs::write(amd.join("Setup.exe"), "fixture").unwrap();
        assert_eq!(
            details_in(&amd, &context).provenance.rule_id,
            "amd.extracted-installer"
        );
        assert!(is_unmatched(
            &context.roots.volume_root.as_ref().unwrap().join("AMD"),
            &context
        ));
    }

    #[test]
    fn macos_xcode_and_simulator_rules_keep_sensitive_siblings_out() {
        let root = tempdir().unwrap();
        let context = context(PlatformKind::MacOS, root.path());
        let developer = context.roots.developer_dir.as_ref().unwrap();
        let caches = context.roots.library_caches_dir.as_ref().unwrap();
        let derived = developer.join("Xcode/DerivedData");
        let archives = developer.join("Xcode/Archives");
        std::fs::create_dir_all(&derived).unwrap();
        std::fs::create_dir_all(&archives).unwrap();
        std::fs::create_dir_all(caches.join("CocoaPods")).unwrap();
        assert_eq!(
            details_in(&derived, &context).provenance.policy,
            FindingPolicy::DirectReview
        );
        assert_eq!(
            details_in(&caches.join("CocoaPods"), &context)
                .provenance
                .policy,
            FindingPolicy::DirectSafe
        );
        assert!(is_unmatched(&archives, &context));
        let sim = developer.join("CoreSimulator/Devices");
        std::fs::create_dir_all(&sim).unwrap();
        assert_eq!(
            details_in(&sim, &context).provenance.policy,
            FindingPolicy::VendorCommandOnly
        );
    }

    #[test]
    fn catalog_metadata_is_complete_unique_and_reviewable() {
        let mut ids = BTreeSet::new();
        for rule in catalog_rules() {
            assert!(
                ids.insert(&rule.id),
                "duplicate catalog rule id: {}",
                rule.id
            );
            assert!(rule.source_url.starts_with("https://"));
            assert_eq!(rule.reviewed_at, REVIEWED_AT);
            assert!(!rule.note.is_empty());
            assert!(!rule.preconditions.is_empty());
            assert!(!rule.locator.is_empty());
        }
        assert_eq!(ids.len(), 29);
    }
}

//! Evidence-backed cross-platform cleanup catalog.
//!
//! This is deliberately separate from the established `$HOME` tables. The
//! catalog's small interface accepts one candidate path and returns its match
//! plus provenance; resolving XDG, Library, redirected Windows profile and
//! environment roots stays here rather than leaking into scanner/frontends.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::model::{Category, FindingPolicy, RuleProvenance, Severity};
use crate::platform::{Platform, PlatformKind, RuleRoots};

use super::Match;

#[derive(Debug, Clone)]
pub(crate) struct CatalogMetadata {
    pub provenance: RuleProvenance,
    pub advice: Option<String>,
}

#[derive(Debug, Clone)]
struct CatalogHit {
    rule: &'static RuleSpec,
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
        let environment = ENVIRONMENT_OVERRIDES
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
        let roots = [
            Some(&self.roots.home_dir),
            Some(&self.roots.cache_dir),
            self.roots.local_app_data_dir.as_ref(),
            self.roots.library_caches_dir.as_ref(),
        ];
        roots
            .into_iter()
            .flatten()
            .any(|root| is_under(path, root, self.kind))
    }
}

/// Resolved catalog state for one scan. Platform roots, allowed environment
/// overrides, and exact targets are immutable while a scan runs, so callers
/// construct this once instead of repeating path work for every directory.
#[derive(Clone)]
pub(crate) struct Catalog {
    context: RuleContext,
    fixed_targets: Vec<(PathBuf, &'static RuleSpec)>,
}

impl Catalog {
    pub(crate) fn current() -> Self {
        let context = RuleContext::current();
        let fixed_targets = fixed_targets(&context);
        Self {
            context,
            fixed_targets,
        }
    }

    pub(crate) fn classify_with_metadata(&self, dir: &Path) -> Option<(Match, CatalogMetadata)> {
        let hit = details_with_targets(dir, &self.context, &self.fixed_targets)?;
        Some((hit.rule.to_match(), hit.rule.metadata()))
    }

    #[cfg(test)]
    fn from_context(context: RuleContext) -> Self {
        let fixed_targets = fixed_targets(&context);
        Self {
            context,
            fixed_targets,
        }
    }
}

/// Only variables whose values are exact tool-cache roots are accepted. The
/// catalog deliberately does not inspect generic `$HOME`, `$TMPDIR`, package
/// roots, or command output.
const ENVIRONMENT_OVERRIDES: &[&str] = &[
    "PIP_CACHE_DIR",
    "CCACHE_DIR",
    "SCCACHE_DIR",
    "VCPKG_DEFAULT_BINARY_CACHE",
    "OPTIX_CACHE_PATH",
];

#[derive(Debug, Clone, Copy)]
struct RuleSpec {
    id: &'static str,
    category: Category,
    severity: Severity,
    policy: FindingPolicy,
    note: &'static str,
    source_url: &'static str,
    recovery_cost: &'static str,
    reviewed_at: &'static str,
    preconditions: &'static [&'static str],
    advice: Option<&'static str>,
}

impl RuleSpec {
    fn to_match(self) -> Match {
        Match {
            category: self.category,
            severity: self.severity,
            note: self.note.into(),
        }
    }

    fn metadata(self) -> CatalogMetadata {
        CatalogMetadata {
            provenance: RuleProvenance {
                rule_id: self.id.into(),
                source_url: self.source_url.into(),
                policy: self.policy,
                recovery_cost: self.recovery_cost.into(),
                reviewed_at: self.reviewed_at.into(),
                preconditions: self
                    .preconditions
                    .iter()
                    .map(|item| (*item).into())
                    .collect(),
            },
            advice: self.advice.map(str::to_owned),
        }
    }
}

const REVIEWED_AT: &str = "2026-08-26";
const OWNED_CACHE: &[&str] = &[
    "the path is the exact documented cache root",
    "the path remains inside a Chystik-owned user root",
];
const CUSTOM_OWNED_CACHE: &[&str] = &[
    "the environment override names an exact tool-cache root",
    "the resolved path remains inside a Chystik-owned user root",
];
const INSTALLER_LAYOUT: &[&str] = &[
    "the path is under the exact vendor staging root",
    "the documented installer layout markers are present",
];
const XCODE_DEVELOPER_DATA: &[&str] = &[
    "the path is the exact Xcode developer-data child",
    "Xcode should be closed before manual cleanup",
];
const VENDOR_COMMAND: &[&str] = &[
    "Chystik must not move this path to Trash",
    "use the owning operating-system or vendor command instead",
];

const PIP: RuleSpec = RuleSpec {
    id: "python.pip.cache",
    category: Category::PackageCaches,
    severity: Severity::Safe,
    policy: FindingPolicy::DirectSafe,
    note: "pip download and wheel cache — refetched by the next install",
    source_url: "https://pip.pypa.io/en/stable/topics/caching/",
    recovery_cost: "the next package install re-downloads cached artifacts",
    reviewed_at: REVIEWED_AT,
    preconditions: OWNED_CACHE,
    advice: None,
};

const COCOAPODS: RuleSpec = RuleSpec {
    id: "ios.cocoapods.cache",
    category: Category::PackageCaches,
    severity: Severity::Safe,
    policy: FindingPolicy::DirectSafe,
    note: "CocoaPods download cache — restored by the next pod install",
    source_url: "https://guides.cocoapods.org/using/faq.html",
    recovery_cost: "pods are downloaded again on the next install",
    reviewed_at: REVIEWED_AT,
    preconditions: OWNED_CACHE,
    advice: None,
};

const CCACHE: RuleSpec = RuleSpec {
    id: "cpp.ccache",
    category: Category::PackageCaches,
    severity: Severity::Safe,
    policy: FindingPolicy::DirectSafe,
    note: "ccache compiler outputs — rebuilt while C/C++ projects compile",
    source_url: "https://ccache.dev/manual/latest.html",
    recovery_cost: "the next C/C++ build recompiles cache misses",
    reviewed_at: REVIEWED_AT,
    preconditions: OWNED_CACHE,
    advice: None,
};

const SCCACHE: RuleSpec = RuleSpec {
    id: "cpp.sccache",
    category: Category::PackageCaches,
    severity: Severity::Safe,
    policy: FindingPolicy::DirectSafe,
    note: "sccache compiler outputs — rebuilt while projects compile",
    source_url: "https://android.googlesource.com/toolchain/sccache/+/HEAD/docs/Local.md",
    recovery_cost: "the next compiler run repopulates local cache entries",
    reviewed_at: REVIEWED_AT,
    preconditions: OWNED_CACHE,
    advice: None,
};

const VCPKG: RuleSpec = RuleSpec {
    id: "cpp.vcpkg.binary-archives",
    category: Category::PackageCaches,
    severity: Severity::Safe,
    policy: FindingPolicy::DirectSafe,
    note: "vcpkg binary archives — rebuilt or downloaded by the next install",
    source_url: "https://learn.microsoft.com/en-us/vcpkg/users/binarycaching",
    recovery_cost: "the next vcpkg install rebuilds or downloads archives",
    reviewed_at: REVIEWED_AT,
    preconditions: OWNED_CACHE,
    advice: None,
};

const OPTIX: RuleSpec = RuleSpec {
    id: "nvidia.optix.cache",
    category: Category::PackageCaches,
    severity: Severity::Safe,
    policy: FindingPolicy::DirectSafe,
    note: "NVIDIA OptiX compilation cache — recreated by the next OptiX run",
    source_url: "https://raytracing-docs.nvidia.com/optix9/api/OptiX_API_Reference.pdf",
    recovery_cost: "the next OptiX workload recompiles kernels",
    reviewed_at: REVIEWED_AT,
    preconditions: OWNED_CACHE,
    advice: None,
};

const OPTIX_CUSTOM: RuleSpec = RuleSpec {
    id: "nvidia.optix.cache.custom-location",
    policy: FindingPolicy::DirectReview,
    preconditions: CUSTOM_OWNED_CACHE,
    ..OPTIX
};

const NVIDIA_INSTALLER: RuleSpec = RuleSpec {
    id: "nvidia.extracted-installer",
    category: Category::Installers,
    severity: Severity::Moderate,
    policy: FindingPolicy::DirectReview,
    note: "NVIDIA extracted driver installer — close setup before moving it to Trash",
    source_url: "https://nvidia.custhelp.com/app/answers/detail/a_id/2985/",
    recovery_cost: "re-download the NVIDIA driver package if setup is needed again",
    reviewed_at: REVIEWED_AT,
    preconditions: INSTALLER_LAYOUT,
    advice: None,
};

const AMD_INSTALLER: RuleSpec = RuleSpec {
    id: "amd.extracted-installer",
    category: Category::Installers,
    severity: Severity::Moderate,
    policy: FindingPolicy::DirectReview,
    note: "AMD extracted installer — confirm no install or repair is in progress",
    source_url:
        "https://rocm.docs.amd.com/projects/install-on-windows/en/latest/how-to/install.html",
    recovery_cost: "re-download the AMD installer if it is needed for repair",
    reviewed_at: REVIEWED_AT,
    preconditions: INSTALLER_LAYOUT,
    advice: None,
};

const XCODE_DERIVED: RuleSpec = RuleSpec {
    id: "xcode.derived-data",
    category: Category::IdeToolchains,
    severity: Severity::Moderate,
    policy: FindingPolicy::DirectReview,
    note: "Xcode DerivedData — close Xcode; the selected project's build data is rebuilt",
    source_url:
        "https://developer.apple.com/documentation/Xcode-Release-Notes/xcode-26-release-notes",
    recovery_cost: "the selected Xcode projects rebuild indexes and products",
    reviewed_at: REVIEWED_AT,
    preconditions: XCODE_DEVELOPER_DATA,
    advice: None,
};

const XCODE_DEVICE_SUPPORT: RuleSpec = RuleSpec {
    id: "xcode.ios-device-support",
    category: Category::IdeToolchains,
    severity: Severity::Moderate,
    policy: FindingPolicy::DirectReview,
    note: "Xcode iOS DeviceSupport — device symbols download again when needed",
    source_url: "https://developer.apple.com/forums/thread/683496",
    recovery_cost: "connect a device again to download its support files",
    reviewed_at: REVIEWED_AT,
    preconditions: XCODE_DEVELOPER_DATA,
    advice: None,
};

const CONAN: RuleSpec = RuleSpec {
    id: "cpp.conan-cache",
    category: Category::PackageCaches,
    severity: Severity::Moderate,
    policy: FindingPolicy::VendorCommandOnly,
    note: "Conan package storage — clean it through Conan so package references stay valid",
    source_url: "https://docs.conan.io/2/reference/commands/cache.html",
    recovery_cost: "Conan downloads or rebuilds selected packages",
    reviewed_at: REVIEWED_AT,
    preconditions: VENDOR_COMMAND,
    advice: Some("conan cache clean <reference> --download"),
};

const DIRECTX_SHADER: RuleSpec = RuleSpec {
    id: "windows.directx-shader-cache",
    category: Category::SystemJunk,
    severity: Severity::Safe,
    policy: FindingPolicy::AdvisoryOnly,
    note: "Windows DirectX shader cache — clear it through Temporary files, not raw deletion",
    source_url: "https://support.microsoft.com/en-us/windows/free-up-drive-space-in-windows-85529ccb-c365-4c84-8d63-4d518db795dc",
    recovery_cost: "games and graphics applications rebuild shaders on first run",
    reviewed_at: REVIEWED_AT,
    preconditions: VENDOR_COMMAND,
    advice: Some("Open Settings → System → Storage → Temporary files → DirectX Shader Cache"),
};

const AMD_SHADER: RuleSpec = RuleSpec {
    id: "amd.shader-cache",
    category: Category::SystemJunk,
    severity: Severity::Safe,
    policy: FindingPolicy::VendorCommandOnly,
    note: "AMD shader cache — reset it through AMD Software: Adrenalin",
    source_url: "https://www.amd.com/en/resources/support-articles/faqs/dh-012.html",
    recovery_cost: "games rebuild shaders on their next start",
    reviewed_at: REVIEWED_AT,
    preconditions: VENDOR_COMMAND,
    advice: Some("AMD Software: Adrenalin → Gaming → Graphics → Reset Shader Cache"),
};

const CORE_SIMULATOR: RuleSpec = RuleSpec {
    id: "xcode.unavailable-simulators",
    category: Category::IdeToolchains,
    severity: Severity::Moderate,
    policy: FindingPolicy::VendorCommandOnly,
    note: "Unavailable iOS simulators — let simctl remove only devices without a runtime",
    source_url: "https://developer.apple.com/forums/thread/835883",
    recovery_cost: "unavailable simulator devices are removed; available runtimes stay intact",
    reviewed_at: REVIEWED_AT,
    preconditions: VENDOR_COMMAND,
    advice: Some("xcrun simctl delete unavailable"),
};

#[cfg(test)]
const CATALOG_RULES: &[RuleSpec] = &[
    PIP,
    COCOAPODS,
    CCACHE,
    SCCACHE,
    VCPKG,
    OPTIX,
    OPTIX_CUSTOM,
    NVIDIA_INSTALLER,
    AMD_INSTALLER,
    XCODE_DERIVED,
    XCODE_DEVICE_SUPPORT,
    CONAN,
    DIRECTX_SHADER,
    AMD_SHADER,
    CORE_SIMULATOR,
];

fn details_with_targets(
    dir: &Path,
    context: &RuleContext,
    targets: &[(PathBuf, &'static RuleSpec)],
) -> Option<CatalogHit> {
    if context.platform.is_link_or_reparse_point(dir) {
        return None;
    }

    for (target, rule) in targets {
        if path_eq(dir, target, context.kind) {
            return Some(CatalogHit { rule });
        }
    }

    for (environment, rule) in [
        ("PIP_CACHE_DIR", &PIP),
        ("CCACHE_DIR", &CCACHE),
        ("SCCACHE_DIR", &SCCACHE),
        ("VCPKG_DEFAULT_BINARY_CACHE", &VCPKG),
    ] {
        if context
            .environment_target(environment)
            .is_some_and(|target| path_eq(dir, target, context.kind))
        {
            return Some(CatalogHit { rule });
        }
    }
    if context
        .environment_target("OPTIX_CACHE_PATH")
        .is_some_and(|target| path_eq(dir, target, context.kind))
    {
        return Some(CatalogHit {
            rule: &OPTIX_CUSTOM,
        });
    }

    if context.kind == PlatformKind::Windows {
        let volume = context.roots.volume_root.as_deref()?;
        let nvidia_root = volume.join("NVIDIA");
        if is_under(dir, &nvidia_root, context.kind)
            && dir.join("setup.exe").is_file()
            && dir.join("Display.Driver").is_dir()
        {
            return Some(CatalogHit {
                rule: &NVIDIA_INSTALLER,
            });
        }
        let amd_root = volume.join("AMD");
        if dir
            .parent()
            .is_some_and(|parent| path_eq(parent, &amd_root, context.kind))
            && dir.join("Setup.exe").is_file()
            && dir.join("Packages").is_dir()
        {
            return Some(CatalogHit {
                rule: &AMD_INSTALLER,
            });
        }
    }

    None
}

fn fixed_targets(context: &RuleContext) -> Vec<(PathBuf, &'static RuleSpec)> {
    let roots = &context.roots;
    let mut targets = Vec::new();
    match context.kind {
        PlatformKind::Linux => {
            targets.extend([
                (roots.cache_dir.join("pip"), &PIP),
                (roots.home_dir.join(".ccache"), &CCACHE),
                (roots.cache_dir.join("ccache"), &CCACHE),
                (roots.cache_dir.join("sccache"), &SCCACHE),
                (roots.cache_dir.join("vcpkg/archives"), &VCPKG),
                (roots.home_dir.join(".conan2/p"), &CONAN),
            ]);
        }
        PlatformKind::MacOS => {
            if let (Some(caches), Some(developer)) = (
                roots.library_caches_dir.as_ref(),
                roots.developer_dir.as_ref(),
            ) {
                targets.extend([
                    (caches.join("pip"), &PIP),
                    (caches.join("ccache"), &CCACHE),
                    (caches.join("Mozilla.sccache"), &SCCACHE),
                    (caches.join("vcpkg/archives"), &VCPKG),
                    (caches.join("CocoaPods"), &COCOAPODS),
                    (roots.home_dir.join(".conan2/p"), &CONAN),
                    (developer.join("Xcode/DerivedData"), &XCODE_DERIVED),
                    (
                        developer.join("Xcode/iOS DeviceSupport"),
                        &XCODE_DEVICE_SUPPORT,
                    ),
                    (developer.join("CoreSimulator/Devices"), &CORE_SIMULATOR),
                ]);
            }
        }
        PlatformKind::Windows => {
            if let Some(local) = roots.local_app_data_dir.as_ref() {
                targets.extend([
                    (local.join("pip/Cache"), &PIP),
                    (local.join("ccache"), &CCACHE),
                    (local.join("Mozilla/sccache"), &SCCACHE),
                    (local.join("vcpkg/archives"), &VCPKG),
                    (local.join("NVIDIA/OptixCache"), &OPTIX),
                    (local.join("D3DSCache"), &DIRECTX_SHADER),
                    (local.join("AMD/DxCache"), &AMD_SHADER),
                    (local.join("AMD/DxcCache"), &AMD_SHADER),
                ]);
            }
        }
        PlatformKind::Unsupported => {}
    }
    targets
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
    use std::collections::BTreeSet;
    use tempfile::tempdir;

    fn context(kind: PlatformKind, root: &Path) -> RuleContext {
        RuleContext {
            platform: crate::platform::current(),
            kind,
            roots: RuleRoots {
                home_dir: root.join("home"),
                cache_dir: root.join("cache"),
                local_app_data_dir: Some(root.join("local")),
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
    fn windows_roots_are_redirectable_and_architecture_neutral() {
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
            (
                local.join("AMD/DxcCache"),
                "amd.shader-cache",
                FindingPolicy::VendorCommandOnly,
            ),
        ] {
            std::fs::create_dir_all(&target).unwrap();
            let metadata = details_in(&target, &context);
            assert_eq!(metadata.provenance.rule_id, rule_id);
            assert_eq!(metadata.provenance.policy, policy);
        }
        assert!(is_unmatched(local.join("vcpkg").as_path(), &context));
    }

    #[test]
    fn windows_driver_installers_need_exact_roots_and_markers() {
        let root = tempdir().unwrap();
        let context = context(PlatformKind::Windows, root.path());
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
            context
                .roots
                .volume_root
                .as_ref()
                .unwrap()
                .join("AMD")
                .as_path(),
            &context
        ));
    }

    #[test]
    fn macos_xcode_and_simulator_rules_keep_sensitive_siblings_out() {
        let root = tempdir().unwrap();
        let context = context(PlatformKind::MacOS, root.path());
        let developer = context.roots.developer_dir.as_ref().unwrap();
        let caches = context.roots.library_caches_dir.as_ref().unwrap();
        for (target, rule_id, policy) in [
            (
                caches.join("pip"),
                "python.pip.cache",
                FindingPolicy::DirectSafe,
            ),
            (
                caches.join("ccache"),
                "cpp.ccache",
                FindingPolicy::DirectSafe,
            ),
            (
                caches.join("Mozilla.sccache"),
                "cpp.sccache",
                FindingPolicy::DirectSafe,
            ),
            (
                caches.join("vcpkg/archives"),
                "cpp.vcpkg.binary-archives",
                FindingPolicy::DirectSafe,
            ),
            (
                caches.join("CocoaPods"),
                "ios.cocoapods.cache",
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
        }
        let derived = developer.join("Xcode/DerivedData");
        let archives = developer.join("Xcode/Archives");
        std::fs::create_dir_all(&derived).unwrap();
        std::fs::create_dir_all(&archives).unwrap();

        assert_eq!(
            details_in(&derived, &context).provenance.policy,
            FindingPolicy::DirectReview
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
    fn vendor_command_rules_never_claim_direct_ownership() {
        let root = tempdir().unwrap();
        let context = context(PlatformKind::Windows, root.path());
        let directx = context
            .roots
            .local_app_data_dir
            .as_ref()
            .unwrap()
            .join("D3DSCache");
        std::fs::create_dir_all(&directx).unwrap();

        let metadata = details_in(&directx, &context);
        assert_eq!(metadata.provenance.policy, FindingPolicy::AdvisoryOnly);
        assert!(metadata.advice.is_some());

        let amd = context
            .roots
            .local_app_data_dir
            .as_ref()
            .unwrap()
            .join("AMD/DxcCache");
        std::fs::create_dir_all(&amd).unwrap();
        let metadata = details_in(&amd, &context);
        assert_eq!(metadata.provenance.policy, FindingPolicy::VendorCommandOnly);
        assert!(metadata
            .advice
            .as_deref()
            .is_some_and(|advice| advice.contains("Adrenalin")));
    }

    #[test]
    fn catalog_metadata_is_complete_unique_and_reviewable() {
        let mut ids = BTreeSet::new();
        for rule in CATALOG_RULES {
            assert!(
                ids.insert(rule.id),
                "duplicate catalog rule id: {}",
                rule.id
            );
            assert!(
                rule.source_url.starts_with("https://"),
                "{} needs a secure upstream source",
                rule.id
            );
            assert!(
                rule.preconditions
                    .iter()
                    .all(|condition| !condition.is_empty()),
                "{} has an empty precondition",
                rule.id
            );
            assert!(
                !rule.preconditions.is_empty(),
                "{} must declare its classification preconditions",
                rule.id
            );
            assert!(
                matches!(rule.reviewed_at.as_bytes(), [a, b, c, d, b'-', e, f, b'-', g, h]
                    if [a, b, c, d, e, f, g, h]
                        .iter()
                        .all(|byte| byte.is_ascii_digit())),
                "{} has an invalid reviewed_at date: {}",
                rule.id,
                rule.reviewed_at
            );
        }
    }
}

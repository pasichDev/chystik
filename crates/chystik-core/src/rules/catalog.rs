//! Evidence-backed cross-platform cleanup catalog.
//!
//! This is deliberately separate from the established `$HOME` tables. The
//! catalog's small interface accepts one candidate path and returns its match
//! plus provenance; resolving XDG, Library, redirected Windows profile and
//! environment roots stays here rather than leaking into scanner/frontends.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::model::{Category, FindingPolicy, RuleProvenance, Severity};
use crate::platform::{PlatformKind, RuleRoots};

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

#[derive(Debug, Clone)]
struct RuleContext {
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
            },
            advice: self.advice.map(str::to_owned),
        }
    }
}

const PIP: RuleSpec = RuleSpec {
    id: "python.pip.cache",
    category: Category::PackageCaches,
    severity: Severity::Safe,
    policy: FindingPolicy::DirectSafe,
    note: "pip download and wheel cache — refetched by the next install",
    source_url: "https://pip.pypa.io/en/stable/topics/caching/",
    recovery_cost: "the next package install re-downloads cached artifacts",
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
    advice: None,
};

const OPTIX_CUSTOM: RuleSpec = RuleSpec {
    policy: FindingPolicy::DirectReview,
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
    advice: Some("xcrun simctl delete unavailable"),
};

/// Classify one directory and return its evidence from the same resolved
/// catalog hit. Resolving once avoids a second environment/filesystem lookup
/// between the match and its policy/provenance.
pub(crate) fn classify_with_metadata(dir: &Path) -> Option<(Match, CatalogMetadata)> {
    let hit = details(dir, &RuleContext::current())?;
    Some((hit.rule.to_match(), hit.rule.metadata()))
}

fn details(dir: &Path, context: &RuleContext) -> Option<CatalogHit> {
    if crate::platform::current().is_link_or_reparse_point(dir) {
        return None;
    }

    for (target, rule) in fixed_targets(context) {
        if path_eq(dir, &target, context.kind) {
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
    use tempfile::tempdir;

    fn context(kind: PlatformKind, root: &Path) -> RuleContext {
        RuleContext {
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
        details(dir, context)
            .expect("catalog match")
            .rule
            .metadata()
    }

    #[test]
    fn direct_safe_defaults_are_exact_and_keep_siblings_out() {
        let root = tempdir().unwrap();
        let context = context(PlatformKind::Linux, root.path());
        let pip = context.roots.cache_dir.join("pip");
        std::fs::create_dir_all(&pip).unwrap();

        let metadata = details_in(&pip, &context);
        assert_eq!(metadata.provenance.rule_id, "python.pip.cache");
        assert_eq!(metadata.provenance.policy, FindingPolicy::DirectSafe);
        assert!(details(&context.roots.cache_dir, &context).is_none());
        assert!(details(&context.roots.cache_dir.join("pip-extra"), &context).is_none());
    }

    #[test]
    fn environment_override_must_remain_inside_an_owned_root() {
        let root = tempdir().unwrap();
        let mut context = context(PlatformKind::Linux, root.path());
        let allowed = context.roots.home_dir.join("custom/pip");
        let outside = root.path().join("external/pip");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        context
            .environment
            .insert("PIP_CACHE_DIR".into(), allowed.clone());
        assert_eq!(
            details_in(&allowed, &context).provenance.policy,
            FindingPolicy::DirectSafe
        );

        context
            .environment
            .insert("PIP_CACHE_DIR".into(), outside.clone());
        assert!(details(&outside, &context).is_none());
    }

    #[test]
    fn windows_roots_are_redirectable_and_architecture_neutral() {
        let root = tempdir().unwrap();
        let context = context(PlatformKind::Windows, root.path());
        let archives = context
            .roots
            .local_app_data_dir
            .as_ref()
            .unwrap()
            .join("vcpkg/archives");
        std::fs::create_dir_all(&archives).unwrap();

        let metadata = details_in(&archives, &context);
        assert_eq!(metadata.provenance.rule_id, "cpp.vcpkg.binary-archives");
        assert!(details(
            context
                .roots
                .local_app_data_dir
                .as_ref()
                .unwrap()
                .join("vcpkg")
                .as_path(),
            &context
        )
        .is_none());
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
        assert!(details(nvidia.parent().unwrap(), &context).is_none());

        let amd = context.roots.volume_root.as_ref().unwrap().join("AMD/24.9");
        std::fs::create_dir_all(amd.join("Packages")).unwrap();
        std::fs::write(amd.join("Setup.exe"), "fixture").unwrap();
        assert_eq!(
            details_in(&amd, &context).provenance.rule_id,
            "amd.extracted-installer"
        );
        assert!(details(
            context
                .roots
                .volume_root
                .as_ref()
                .unwrap()
                .join("AMD")
                .as_path(),
            &context
        )
        .is_none());
    }

    #[test]
    fn macos_xcode_and_simulator_rules_keep_sensitive_siblings_out() {
        let root = tempdir().unwrap();
        let context = context(PlatformKind::MacOS, root.path());
        let developer = context.roots.developer_dir.as_ref().unwrap();
        let derived = developer.join("Xcode/DerivedData");
        let archives = developer.join("Xcode/Archives");
        std::fs::create_dir_all(&derived).unwrap();
        std::fs::create_dir_all(&archives).unwrap();

        assert_eq!(
            details_in(&derived, &context).provenance.policy,
            FindingPolicy::DirectReview
        );
        assert!(details(&archives, &context).is_none());
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
}

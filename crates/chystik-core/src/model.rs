//! Data model — the API contract between `chystik-core` and `chystik-gui`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Severity of a finding: how safe it is to remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Regenerates automatically (build caches, browser cache).
    Safe,
    /// Regenerates after reinstall/re-download (node_modules, target).
    Moderate,
    /// User data or slow to restore (AI models, SDKs, DB volumes).
    Risky,
}

impl Severity {
    /// Stable lowercase identifier used in machine-readable output and CLI
    /// arguments. Unlike [`Self::label`], this is part of the public contract.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Severity::Safe => "safe",
            Severity::Moderate => "moderate",
            Severity::Risky => "risky",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Severity::Safe => "Safe",
            Severity::Moderate => "Moderate",
            Severity::Risky => "Risky",
        }
    }

    /// Rough human estimate of the cost to regenerate, if known.
    pub fn regeneration_cost(&self) -> Option<&'static str> {
        match self {
            Severity::Safe => Some("regenerates automatically"),
            Severity::Moderate => Some("needs reinstall / re-download (minutes)"),
            Severity::Risky => None,
        }
    }
}

/// What kind of space consumer a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Project build outputs: node_modules, target, .next, dist, build...
    BuildArtifacts,
    /// Package manager caches: gradle, go modules, npm, maven, pip...
    PackageCaches,
    /// IDE indexes and toolchain caches: VS Code workspaceStorage etc.
    IdeToolchains,
    /// Local AI models: ollama models etc.
    AiModels,
    /// Browser caches, thumbnails, trash.
    BrowserSystem,
    /// Android dev toolchain copies: old Studio installs/updates, SDK
    /// system images, emulator snapshots, side-by-side NDK versions.
    AndroidDev,
    /// Data of AI coding agents & CLI assistants (.claude, .codex,
    /// .cursor, session logs, prompt histories, agent caches).
    AiAgents,
    /// Container engines at user level: docker config/buildx caches,
    /// podman storage cache. Root-only /var/lib/docker is never touched.
    Containers,
    /// Leftover downloaded installers and archives that were already
    /// unpacked/installed: old .deb/.tar.gz/AppImage/.iso files.
    Installers,
    /// Game launchers & stores (Steam, Lutris, Heroic...): shader caches,
    /// compatdata prefixes, old Proton runtimes. Saves are never targeted.
    GameLaunchers,
    /// Media apps (video/audio editors, players, OBS): render caches,
    /// preview proxies, recordings scratch space.
    MediaApps,
    /// Messengers & social apps (Telegram, Discord, Slack...): media
    /// caches, received-file caches. Chat history itself is out of scope.
    Messengers,
    /// Cloud sync clients (Nextcloud, Dropbox, Syncthing...): transfer
    /// queues, conflict copies, metadata databases — never the synced files.
    CloudSync,
    /// Office & note apps (LibreOffice, Obsidian, OnlyOffice): document
    /// recovery data, temp locks, thumbnail caches.
    OfficeDocs,
    /// Generic system junk: crash dumps, journal leftovers, orphaned
    /// thumbnails of removed files, ~/.cache one-off strays.
    SystemJunk,
}

impl Category {
    /// Stable lowercase identifier used in machine-readable output and CLI
    /// arguments. Keep this aligned with the `serde(rename_all)` contract.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Category::BuildArtifacts => "build_artifacts",
            Category::PackageCaches => "package_caches",
            Category::IdeToolchains => "ide_toolchains",
            Category::AiModels => "ai_models",
            Category::BrowserSystem => "browser_system",
            Category::AndroidDev => "android_dev",
            Category::AiAgents => "ai_agents",
            Category::Containers => "containers",
            Category::Installers => "installers",
            Category::GameLaunchers => "game_launchers",
            Category::MediaApps => "media_apps",
            Category::Messengers => "messengers",
            Category::CloudSync => "cloud_sync",
            Category::OfficeDocs => "office_docs",
            Category::SystemJunk => "system_junk",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Category::BuildArtifacts => "Build artifacts",
            Category::PackageCaches => "Package caches",
            Category::IdeToolchains => "IDE & toolchains",
            Category::AiModels => "AI models",
            Category::BrowserSystem => "Browser & system",
            Category::AndroidDev => "Android dev",
            Category::AiAgents => "AI agents",
            Category::Containers => "Containers",
            Category::Installers => "Installers",
            Category::GameLaunchers => "Games",
            Category::MediaApps => "Media",
            Category::Messengers => "Messengers",
            Category::CloudSync => "Cloud sync",
            Category::OfficeDocs => "Office",
            Category::SystemJunk => "System junk",
        }
    }

    pub fn all() -> [Category; 15] {
        [
            Category::BuildArtifacts,
            Category::PackageCaches,
            Category::IdeToolchains,
            Category::AiModels,
            Category::BrowserSystem,
            Category::AndroidDev,
            Category::AiAgents,
            Category::Containers,
            Category::Installers,
            Category::GameLaunchers,
            Category::MediaApps,
            Category::Messengers,
            Category::CloudSync,
            Category::OfficeDocs,
            Category::SystemJunk,
        ]
    }
}

/// How Chystik may handle a finding after it is shown to the user.
///
/// Severity answers how expensive it is to get the bytes back; this policy
/// answers whether Chystik owns the cleanup action. Keeping the two concepts
/// separate prevents a `Safe` label from accidentally authorizing a
/// vendor-managed or system-owned location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingPolicy {
    /// A narrow, regenerable target that may participate in `clean --safe`.
    DirectSafe,
    /// A narrow target the user may review and select, but never bulk-select.
    DirectReview,
    /// A system or package-manager location Chystik only describes.
    AdvisoryOnly,
    /// A location that must be cleaned through its owning tool's command/UI.
    VendorCommandOnly,
    /// Deliberately never shown as a cleanup candidate.
    NeverClean,
}

impl FindingPolicy {
    /// Stable machine-readable policy identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectSafe => "direct_safe",
            Self::DirectReview => "direct_review",
            Self::AdvisoryOnly => "advisory_only",
            Self::VendorCommandOnly => "vendor_command_only",
            Self::NeverClean => "never_clean",
        }
    }

    /// Whether the finding may ever reach the native Trash flow.
    pub const fn is_actionable(self) -> bool {
        matches!(self, Self::DirectSafe | Self::DirectReview)
    }
}

/// Evidence attached to a catalog-backed finding.
///
/// Existing rules predate the catalog and intentionally omit this field. The
/// optional shape preserves their public JSON contract while ensuring every
/// newly-added cross-platform target tells users which source justified it and
/// what recovery costs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleProvenance {
    /// Stable catalog identifier; never derive meaning from a filesystem path.
    pub rule_id: String,
    /// Primary vendor or upstream documentation that defines the target.
    pub source_url: String,
    /// The authority Chystik has over this exact target.
    pub policy: FindingPolicy,
    /// Concrete cost after removal, shown instead of a vague "cache" label.
    pub recovery_cost: String,
    /// Date this exact rule was last checked against its upstream evidence.
    pub reviewed_at: String,
    /// Conditions that must hold before Chystik can classify this exact target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preconditions: Vec<String>,
}

/// One reclaimable item found on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// Absolute path of the directory (or file).
    pub path: PathBuf,
    pub category: Category,
    pub severity: Severity,
    /// On-disk size in bytes (st_blocks * 512 summed recursively).
    pub size_bytes: u64,
    /// Last modification time seen inside the tree.
    pub last_used: Option<DateTime<Utc>>,
    /// Mount point (from `/proc/self/mounts`) the finding lives on.
    pub mount: Option<String>,
    /// Short explanation shown in UI (why this is reclaimable + how to restore).
    pub note: String,
    /// Set when Chystik cannot reclaim this itself — the space sits behind a
    /// package manager or needs root — and the string is the command that
    /// does reclaim it.
    ///
    /// The guard refuses `/var` and `/usr` outright, so these locations used
    /// to be invisible: several gigabytes of superseded snap revisions and
    /// package archives that the tool knew about and never mentioned.
    /// Advisory findings are reported, never selected and never deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advice: Option<String>,
    /// Present for catalog-backed targets. Omitted for legacy rules so older
    /// automation keeps receiving the same machine document it already knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<RuleProvenance>,
}

impl Finding {
    /// Policy derived from explicit provenance or the legacy rule contract.
    pub fn policy(&self) -> FindingPolicy {
        self.provenance
            .as_ref()
            .map(|provenance| provenance.policy)
            .unwrap_or_else(|| {
                if self.advice.is_some() {
                    FindingPolicy::AdvisoryOnly
                } else if self.severity == Severity::Safe {
                    FindingPolicy::DirectSafe
                } else {
                    FindingPolicy::DirectReview
                }
            })
    }

    /// True when this finding is Chystik's to delete. Advisory and vendor
    /// command findings stay visible but never enter any selection or removal
    /// path.
    pub fn is_actionable(&self) -> bool {
        self.policy().is_actionable()
    }
}

/// Progress events emitted by the scanner while walking the filesystem.
#[derive(Debug, Clone)]
pub enum ScanProgress {
    Started { root: PathBuf },
    DirectoriesScanned { count: u64 },
    FindingFound(Box<Finding>),
    Finished { findings: Vec<Finding> },
    Cancelled,
}

/// Errors produced by core operations.
#[derive(Debug, thiserror::Error)]
pub enum ChystikError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("path is protected by safety guard: {0}")]
    ProtectedPath(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("scan was cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(
        severity: Severity,
        advice: Option<&str>,
        provenance: Option<RuleProvenance>,
    ) -> Finding {
        Finding {
            path: PathBuf::from("/tmp/finding"),
            category: Category::PackageCaches,
            severity,
            size_bytes: 1,
            last_used: None,
            mount: None,
            note: "fixture".into(),
            advice: advice.map(str::to_owned),
            provenance,
        }
    }

    #[test]
    fn policy_serializes_as_stable_snake_case() {
        assert_eq!(
            serde_json::to_string(&FindingPolicy::VendorCommandOnly).unwrap(),
            "\"vendor_command_only\""
        );
        assert_eq!(FindingPolicy::DirectReview.as_str(), "direct_review");
    }

    #[test]
    fn legacy_findings_keep_their_existing_selection_contract() {
        let safe = finding(Severity::Safe, None, None);
        let moderate = finding(Severity::Moderate, None, None);
        let advised = finding(Severity::Safe, Some("tool clean"), None);

        assert_eq!(safe.policy(), FindingPolicy::DirectSafe);
        assert_eq!(moderate.policy(), FindingPolicy::DirectReview);
        assert_eq!(advised.policy(), FindingPolicy::AdvisoryOnly);
        assert!(safe.is_actionable());
        assert!(moderate.is_actionable());
        assert!(!advised.is_actionable());
        assert!(serde_json::to_value(&safe)
            .unwrap()
            .get("provenance")
            .is_none());
    }

    #[test]
    fn vendor_command_policy_cannot_become_actionable() {
        let vendor = finding(
            Severity::Safe,
            Some("vendor reset"),
            Some(RuleProvenance {
                rule_id: "driver.amd.shader-cache".into(),
                source_url: "https://example.test/vendor".into(),
                policy: FindingPolicy::VendorCommandOnly,
                recovery_cost: "the driver rebuilds shaders".into(),
                reviewed_at: "2026-08-26".into(),
                preconditions: vec!["use the vendor command".into()],
            }),
        );

        assert_eq!(vendor.policy(), FindingPolicy::VendorCommandOnly);
        assert!(!vendor.is_actionable());
    }
}

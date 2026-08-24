//! Data model — the API contract between `chystik-core` and `chystik-gui`.
//! Changes are coordinated by the orchestrator; the v0.2 extension
//! (four new categories, `Finding::mount`) is approved and consumed
//! across the workspace.

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
    #[error("path is protected by safety guard: {0}")]
    ProtectedPath(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("scan was cancelled")]
    Cancelled,
}

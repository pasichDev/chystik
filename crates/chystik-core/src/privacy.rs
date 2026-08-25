//! Traces of what you did, rather than space you can reclaim.
//!
//! The cleaner measures bytes. This does not: `~/.bash_history` is 144 KB
//! and holds every command you have typed, including the ones a token
//! landed in by accident. Sorting these by size would put it last.
//!
//! So every entry answers two questions instead of one — what it reveals,
//! and what you lose by clearing it — and nothing is ever pre-selected.
//! Choosing to erase your own shell history is not a decision a "select
//! all safe" button gets to make.
//!
//! Items are files as often as directories, so this probes a fixed table
//! rather than going through the scanner, which classifies directories only.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::Severity;
use crate::platform::{PlatformKind, PrivacyRoots};

/// What kind of trace this is. Groups the list into something readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceKind {
    /// Commands you typed.
    ShellHistory,
    /// Sites visited, downloads, form entries.
    Browsing,
    /// Files opened, and where from.
    RecentActivity,
    /// Previews of files you looked at, including deleted ones.
    Thumbnails,
    /// Things you deleted but did not erase.
    Deleted,
    /// Editor and tool state that records what you worked on.
    ToolHistory,
}

impl TraceKind {
    pub fn label(&self) -> &'static str {
        match self {
            TraceKind::ShellHistory => "Shell history",
            TraceKind::Browsing => "Browsing",
            TraceKind::RecentActivity => "Recent activity",
            TraceKind::Thumbnails => "Thumbnails",
            TraceKind::Deleted => "Deleted files",
            TraceKind::ToolHistory => "Tool history",
        }
    }
}

/// One trace found on this machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrivacyItem {
    pub path: PathBuf,
    pub kind: TraceKind,
    /// How much is lost by clearing it, in the same vocabulary the cleaner
    /// uses: Safe regenerates, Moderate costs convenience, Risky cannot be
    /// undone in any useful sense.
    pub severity: Severity,
    pub size_bytes: u64,
    pub last_used: Option<DateTime<Utc>>,
    /// What someone reading this file would learn about you.
    pub reveals: &'static str,
    /// What stops working, or is gone for good, once it is cleared.
    pub cost: &'static str,
}

/// A `$HOME`-relative trace and what it means.
struct Trace {
    rel: &'static str,
    kind: TraceKind,
    severity: Severity,
    reveals: &'static str,
    cost: &'static str,
}

/// Which adapter-owned base contains an explicit trace. Keeping this next to
/// the catalogue makes a Windows profile relocation a data concern, not a
/// GUI deletion exception.
#[derive(Debug, Clone, Copy)]
enum TraceRoot {
    Home,
    Roaming,
    Local,
}

struct TraceGroup {
    root: TraceRoot,
    traces: &'static [Trace],
}

const LINUX_TRACES: &[Trace] = &[
    Trace {
        rel: ".bash_history",
        kind: TraceKind::ShellHistory,
        severity: Severity::Moderate,
        reveals: "every command you have run, including secrets pasted into one by accident",
        cost: "history search and recall start from nothing",
    },
    Trace {
        rel: ".zsh_history",
        kind: TraceKind::ShellHistory,
        severity: Severity::Moderate,
        reveals: "every command you have run, including secrets pasted into one by accident",
        cost: "history search and recall start from nothing",
    },
    Trace {
        rel: ".local/share/fish/fish_history",
        kind: TraceKind::ShellHistory,
        severity: Severity::Moderate,
        reveals: "every command you have run in fish",
        cost: "history search and recall start from nothing",
    },
    Trace {
        rel: ".python_history",
        kind: TraceKind::ShellHistory,
        severity: Severity::Safe,
        reveals: "everything typed into an interactive Python session",
        cost: "REPL history is gone; nothing else changes",
    },
    Trace {
        rel: ".lesshst",
        kind: TraceKind::ShellHistory,
        severity: Severity::Safe,
        reveals: "search terms you used inside less, and the files you paged through",
        cost: "nothing; less rebuilds it as you go",
    },
    Trace {
        rel: ".config/google-chrome/Default/History",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "every page visited, with timestamps, plus downloads and search terms",
        cost: "address-bar suggestions and history search are gone permanently",
    },
    Trace {
        rel: ".config/google-chrome/Default/Cookies",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "which sites you are signed in to, and the trackers that follow you",
        cost: "you are signed out of everything",
    },
    Trace {
        rel: ".config/chromium/Default/History",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "every page visited, with timestamps, plus downloads and search terms",
        cost: "address-bar suggestions and history search are gone permanently",
    },
    Trace {
        rel: ".local/share/recently-used.xbel",
        kind: TraceKind::RecentActivity,
        severity: Severity::Safe,
        reveals: "which files you opened and when, across every GTK application",
        cost: "the Recent Files list empties",
    },
    Trace {
        rel: ".local/share/RecentDocuments",
        kind: TraceKind::RecentActivity,
        severity: Severity::Safe,
        reveals: "which documents you opened, across every KDE application",
        cost: "the Recent Documents list empties",
    },
    Trace {
        rel: ".cache/thumbnails",
        kind: TraceKind::Thumbnails,
        severity: Severity::Safe,
        reveals: "previews of files you have browsed, including ones you later deleted",
        cost: "folders redraw their previews once",
    },
    Trace {
        rel: ".local/share/Trash/files",
        kind: TraceKind::Deleted,
        severity: Severity::Moderate,
        reveals: "everything you deleted and did not erase, still fully readable",
        cost: "deletion becomes permanent",
    },
    Trace {
        rel: ".viminfo",
        kind: TraceKind::ToolHistory,
        severity: Severity::Safe,
        reveals: "files edited in vim, search patterns, and clipboard contents",
        cost: "vim forgets your marks and registers",
    },
    Trace {
        rel: ".local/share/nvim/shada",
        kind: TraceKind::ToolHistory,
        severity: Severity::Safe,
        reveals: "files edited in Neovim, search patterns, and clipboard contents",
        cost: "Neovim forgets your marks and registers",
    },
    Trace {
        rel: ".local/share/keyrings",
        kind: TraceKind::ToolHistory,
        severity: Severity::Risky,
        reveals: "stored passwords and tokens, encrypted but present",
        cost: "every saved credential is gone and cannot be recovered",
    },
];

/// macOS keeps the same shell histories in `$HOME`, but applications use
/// `~/Library` rather than the XDG layout used by Linux. Keep this list
/// explicit: privacy cleanup must never guess at an application directory.
const MACOS_TRACES: &[Trace] = &[
    Trace {
        rel: ".bash_history",
        kind: TraceKind::ShellHistory,
        severity: Severity::Moderate,
        reveals: "every command you have run, including secrets pasted into one by accident",
        cost: "history search and recall start from nothing",
    },
    Trace {
        rel: ".zsh_history",
        kind: TraceKind::ShellHistory,
        severity: Severity::Moderate,
        reveals: "every command you have run, including secrets pasted into one by accident",
        cost: "history search and recall start from nothing",
    },
    Trace {
        rel: ".local/share/fish/fish_history",
        kind: TraceKind::ShellHistory,
        severity: Severity::Moderate,
        reveals: "every command you have run in fish",
        cost: "history search and recall start from nothing",
    },
    Trace {
        rel: ".python_history",
        kind: TraceKind::ShellHistory,
        severity: Severity::Safe,
        reveals: "everything typed into an interactive Python session",
        cost: "REPL history is gone; nothing else changes",
    },
    Trace {
        rel: ".lesshst",
        kind: TraceKind::ShellHistory,
        severity: Severity::Safe,
        reveals: "search terms you used inside less, and the files you paged through",
        cost: "nothing; less rebuilds it as you go",
    },
    Trace {
        rel: "Library/Safari/History.db",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "every Safari page visited, with timestamps, downloads and search terms",
        cost: "Safari address-bar suggestions and history search are gone permanently",
    },
    Trace {
        rel: "Library/Application Support/Google/Chrome/Default/History",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "every Chrome page visited, with timestamps, downloads and search terms",
        cost: "Chrome address-bar suggestions and history search are gone permanently",
    },
    Trace {
        rel: "Library/Application Support/Google/Chrome/Default/Cookies",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "which Chrome sites you are signed in to, and the trackers that follow you",
        cost: "Chrome signs you out of the sites whose cookies are cleared",
    },
    Trace {
        rel: "Library/Application Support/Chromium/Default/History",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "every Chromium page visited, with timestamps, downloads and search terms",
        cost: "Chromium address-bar suggestions and history search are gone permanently",
    },
    Trace {
        rel: "Library/Application Support/Chromium/Default/Cookies",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "which Chromium sites you are signed in to, and the trackers that follow you",
        cost: "Chromium signs you out of the sites whose cookies are cleared",
    },
    Trace {
        rel: ".viminfo",
        kind: TraceKind::ToolHistory,
        severity: Severity::Safe,
        reveals: "files edited in vim, search patterns, and clipboard contents",
        cost: "vim forgets your marks and registers",
    },
    Trace {
        rel: "Library/Application Support/nvim/shada/main.shada",
        kind: TraceKind::ToolHistory,
        severity: Severity::Safe,
        reveals: "files edited in Neovim, search patterns, and clipboard contents",
        cost: "Neovim forgets its marks and registers",
    },
];

/// Windows application data is split by the OS between a roaming profile and
/// a machine-local profile. Every path here is a single documented trace;
/// profile globs are deliberately avoided so an unknown browser layout is
/// never deleted by inference.
const WINDOWS_ROAMING_TRACES: &[Trace] = &[
    Trace {
        rel: "Microsoft/Windows/PowerShell/PSReadLine/ConsoleHost_history.txt",
        kind: TraceKind::ShellHistory,
        severity: Severity::Moderate,
        reveals: "every command entered in Windows PowerShell, including pasted secrets",
        cost: "PowerShell history search and recall start from nothing",
    },
    Trace {
        rel: "Microsoft/Windows/Recent",
        kind: TraceKind::RecentActivity,
        severity: Severity::Moderate,
        reveals: "recent files, folders and application jump-list activity",
        cost: "Windows Recent Items and jump-list suggestions are rebuilt from nothing",
    },
];

const WINDOWS_LOCAL_TRACES: &[Trace] = &[
    Trace {
        rel: "Google/Chrome/User Data/Default/History",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "every Chrome page visited, with timestamps, downloads and search terms",
        cost: "Chrome address-bar suggestions and history search are gone permanently",
    },
    Trace {
        rel: "Google/Chrome/User Data/Default/Network/Cookies",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "which Chrome sites you are signed in to, and the trackers that follow you",
        cost: "Chrome signs you out of the sites whose cookies are cleared",
    },
    Trace {
        rel: "Chromium/User Data/Default/History",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "every Chromium page visited, with timestamps, downloads and search terms",
        cost: "Chromium address-bar suggestions and history search are gone permanently",
    },
    Trace {
        rel: "Chromium/User Data/Default/Network/Cookies",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "which Chromium sites you are signed in to, and the trackers that follow you",
        cost: "Chromium signs you out of the sites whose cookies are cleared",
    },
    Trace {
        rel: "Microsoft/Edge/User Data/Default/History",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "every Edge page visited, with timestamps, downloads and search terms",
        cost: "Edge address-bar suggestions and history search are gone permanently",
    },
    Trace {
        rel: "Microsoft/Edge/User Data/Default/Network/Cookies",
        kind: TraceKind::Browsing,
        severity: Severity::Risky,
        reveals: "which Edge sites you are signed in to, and the trackers that follow you",
        cost: "Edge signs you out of the sites whose cookies are cleared",
    },
];

const LINUX_TRACE_GROUPS: &[TraceGroup] = &[TraceGroup {
    root: TraceRoot::Home,
    traces: LINUX_TRACES,
}];

const MACOS_TRACE_GROUPS: &[TraceGroup] = &[TraceGroup {
    root: TraceRoot::Home,
    traces: MACOS_TRACES,
}];

const WINDOWS_TRACE_GROUPS: &[TraceGroup] = &[
    TraceGroup {
        root: TraceRoot::Roaming,
        traces: WINDOWS_ROAMING_TRACES,
    },
    TraceGroup {
        root: TraceRoot::Local,
        traces: WINDOWS_LOCAL_TRACES,
    },
];

fn trace_groups() -> &'static [TraceGroup] {
    match crate::platform::current().kind() {
        PlatformKind::Linux => LINUX_TRACE_GROUPS,
        PlatformKind::MacOS => MACOS_TRACE_GROUPS,
        PlatformKind::Windows => WINDOWS_TRACE_GROUPS,
        PlatformKind::Unsupported => &[],
    }
}

#[cfg(test)]
fn all_traces() -> impl Iterator<Item = &'static Trace> {
    trace_groups().iter().flat_map(|group| group.traces.iter())
}

fn root_for(group: TraceRoot, roots: &PrivacyRoots) -> Option<&Path> {
    match group {
        TraceRoot::Home => Some(&roots.home_dir),
        TraceRoot::Roaming => roots.roaming_dir.as_deref(),
        TraceRoot::Local => roots.local_dir.as_deref(),
    }
}

/// Every trace that exists on this machine, largest first.
///
/// Absent paths are simply not reported: a machine with no Chrome never
/// sees a Chrome row.
pub fn probe() -> Vec<PrivacyItem> {
    probe_from_roots(&crate::platform::current().privacy_roots())
}

fn probe_from_roots(roots: &PrivacyRoots) -> Vec<PrivacyItem> {
    let mut items: Vec<PrivacyItem> = trace_groups()
        .iter()
        .flat_map(|group| {
            root_for(group.root, roots)
                .into_iter()
                .flat_map(move |root| {
                    group
                        .traces
                        .iter()
                        .filter_map(move |trace| probe_one(root, trace))
                })
        })
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.size_bytes));
    items
}

/// The exact approved root for a listed trace. It is intentionally derived
/// from the same table as [`probe`], so the GUI cannot widen Windows cleanup
/// to `$HOME` or guess a parent directory when a profile is relocated.
pub fn cleanup_root_for(path: &Path) -> Option<PathBuf> {
    cleanup_root_for_in(path, &crate::platform::current().privacy_roots())
}

fn cleanup_root_for_in(path: &Path, roots: &PrivacyRoots) -> Option<PathBuf> {
    trace_groups().iter().find_map(|group| {
        let root = root_for(group.root, roots)?;
        group
            .traces
            .iter()
            .any(|trace| root.join(trace.rel) == path)
            .then(|| root.to_path_buf())
    })
}

fn probe_one(home: &Path, trace: &Trace) -> Option<PrivacyItem> {
    let path = home.join(trace.rel);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    // Never report a link: clearing it would act on wherever it points.
    if meta.file_type().is_symlink() || crate::platform::current().is_link_or_reparse_point(&path) {
        return None;
    }
    let host = crate::platform::current();
    let (size_bytes, last_used) = if meta.is_dir() {
        tree_size(&path)
    } else {
        (
            host.allocated_bytes(&meta),
            meta.modified().ok().map(DateTime::<Utc>::from),
        )
    };
    Some(PrivacyItem {
        path,
        kind: trace.kind,
        severity: trace.severity,
        size_bytes,
        last_used,
        reveals: trace.reveals,
        cost: trace.cost,
    })
}

fn tree_size(dir: &Path) -> (u64, Option<DateTime<Utc>>) {
    let host = crate::platform::current();
    let mut stack = vec![dir.to_path_buf()];
    let (mut bytes, mut newest) = (0u64, None::<DateTime<Utc>>);
    while let Some(path) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.file_type().is_dir() && !host.is_link_or_reparse_point(&entry.path()) {
                stack.push(entry.path());
            } else if meta.file_type().is_file() {
                bytes += host.allocated_bytes(&meta);
                if let Ok(modified) = meta.modified() {
                    let stamp: DateTime<Utc> = modified.into();
                    if newest.is_none_or(|current| stamp > current) {
                        newest = Some(stamp);
                    }
                }
            }
        }
    }
    (bytes, newest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_trace_explains_itself() {
        for trace in all_traces() {
            assert!(
                trace.reveals.len() > 25,
                "{}: `reveals` is the whole product; it cannot be a label",
                trace.rel
            );
            assert!(trace.cost.len() > 10, "{}: no cost stated", trace.rel);
            assert!(
                !trace.rel.starts_with('/'),
                "{}: must be relative to its platform privacy root",
                trace.rel
            );
        }
    }

    /// Nothing here may be pre-selectable by a bulk action. Erasing your own
    /// history is a per-item decision, so no trace is `Safe` unless clearing
    /// it genuinely costs nothing.
    #[test]
    fn browsing_and_credentials_are_never_marked_safe() {
        for trace in all_traces() {
            if matches!(trace.kind, TraceKind::Browsing) || trace.rel.contains("keyrings") {
                assert_ne!(
                    trace.severity,
                    Severity::Safe,
                    "{} must not be bulk-selectable",
                    trace.rel
                );
            }
        }
    }

    #[test]
    fn probing_this_machine_is_self_consistent() {
        let items = probe();
        for item in &items {
            assert!(item.path.is_absolute());
            assert!(
                std::fs::symlink_metadata(&item.path).is_ok(),
                "{} was reported but does not exist",
                item.path.display()
            );
        }
        // Sorted largest first, so the list opens on what matters.
        for pair in items.windows(2) {
            assert!(pair[0].size_bytes >= pair[1].size_bytes);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_trace_is_not_reported() {
        let home = tempfile::tempdir().unwrap();
        let real = home.path().join("elsewhere");
        std::fs::write(&real, "secrets").unwrap();
        std::os::unix::fs::symlink(&real, home.path().join(".bash_history")).unwrap();

        let trace = all_traces()
            .find(|trace| trace.rel == ".bash_history")
            .expect("every supported Unix host has bash history in the table");
        assert_eq!(trace.rel, ".bash_history");
        assert!(
            probe_one(home.path(), trace).is_none(),
            "clearing a link would act on its target"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_probe_uses_library_paths() {
        let _env = crate::rules::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let home = tempfile::tempdir().unwrap();
        let history = home.path().join("Library/Safari/History.db");
        std::fs::create_dir_all(history.parent().unwrap()).unwrap();
        std::fs::write(&history, "history").unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", home.path());

        let items = probe();

        std::env::set_var("CHYSTIK_TEST_HOME", "/nonexistent-chystik-test-home");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, history);
        assert_eq!(items[0].kind, TraceKind::Browsing);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_probe_uses_roaming_and_local_profile_roots() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let roaming = fixture.path().join("redirected-roaming");
        let local = fixture.path().join("redirected-local");
        let powershell =
            roaming.join("Microsoft/Windows/PowerShell/PSReadLine/ConsoleHost_history.txt");
        let edge = local.join("Microsoft/Edge/User Data/Default/History");
        std::fs::create_dir_all(powershell.parent().unwrap()).unwrap();
        std::fs::create_dir_all(edge.parent().unwrap()).unwrap();
        std::fs::write(&powershell, "Get-Content $env:SECRET").unwrap();
        std::fs::write(&edge, "browsing history").unwrap();
        let roots = PrivacyRoots {
            home_dir: home,
            roaming_dir: Some(roaming.clone()),
            local_dir: Some(local.clone()),
        };

        let items = probe_from_roots(&roots);

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|item| item.path == powershell));
        assert!(items.iter().any(|item| item.path == edge));
        assert_eq!(cleanup_root_for_in(&powershell, &roots), Some(roaming));
        assert_eq!(cleanup_root_for_in(&edge, &roots), Some(local));
        assert!(
            crate::guard::check(
                &powershell,
                &cleanup_root_for_in(&powershell, &roots).unwrap()
            )
            .is_ok(),
            "the exact Windows privacy root must reach the cleanup guard"
        );
        assert!(
            crate::guard::check(&powershell, &roots.home_dir).is_err(),
            "a redirected roaming profile must not be silently widened to $HOME"
        );
    }
}

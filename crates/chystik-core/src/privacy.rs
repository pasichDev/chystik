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
use crate::rules::home_root;

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

const TRACES: &[Trace] = &[
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

/// Every trace that exists on this machine, largest first.
///
/// Absent paths are simply not reported: a machine with no Chrome never
/// sees a Chrome row.
pub fn probe() -> Vec<PrivacyItem> {
    let Some(home) = home_root() else {
        return Vec::new();
    };
    let mut items: Vec<PrivacyItem> = TRACES
        .iter()
        .filter_map(|trace| probe_one(&home, trace))
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.size_bytes));
    items
}

fn probe_one(home: &Path, trace: &Trace) -> Option<PrivacyItem> {
    let path = home.join(trace.rel);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    // Never report a link: clearing it would act on wherever it points.
    if meta.file_type().is_symlink() {
        return None;
    }
    let (size_bytes, last_used) = if meta.is_dir() {
        tree_size(&path)
    } else {
        use std::os::unix::fs::MetadataExt;
        (
            meta.blocks() * 512,
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
    use std::os::unix::fs::MetadataExt;

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
            if meta.file_type().is_dir() {
                stack.push(entry.path());
            } else if meta.file_type().is_file() {
                bytes += meta.blocks() * 512;
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
        for trace in TRACES {
            assert!(
                trace.reveals.len() > 25,
                "{}: `reveals` is the whole product; it cannot be a label",
                trace.rel
            );
            assert!(trace.cost.len() > 10, "{}: no cost stated", trace.rel);
            assert!(
                !trace.rel.starts_with('/'),
                "{}: must be $HOME-relative",
                trace.rel
            );
        }
    }

    /// Nothing here may be pre-selectable by a bulk action. Erasing your own
    /// history is a per-item decision, so no trace is `Safe` unless clearing
    /// it genuinely costs nothing.
    #[test]
    fn browsing_and_credentials_are_never_marked_safe() {
        for trace in TRACES {
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

    #[test]
    fn a_symlinked_trace_is_not_reported() {
        let home = tempfile::tempdir().unwrap();
        let real = home.path().join("elsewhere");
        std::fs::write(&real, "secrets").unwrap();
        std::os::unix::fs::symlink(&real, home.path().join(".bash_history")).unwrap();

        let trace = &TRACES[0];
        assert_eq!(trace.rel, ".bash_history");
        assert!(
            probe_one(home.path(), trace).is_none(),
            "clearing a link would act on its target"
        );
    }
}

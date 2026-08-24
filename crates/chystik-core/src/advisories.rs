//! System locations Chystik reports but never touches.
//!
//! The deletion guard refuses `/usr` and `/var` outright, and the scanner
//! does not even walk them — correctly, because reclaiming that space needs
//! root or a package-manager command. The result was that several gigabytes
//! of superseded snap revisions, package archives and coredumps were known
//! to the tool and never mentioned.
//!
//! An advisory finding closes that gap: it reports the size and names the
//! command that reclaims it. It carries `advice: Some(..)`, which makes it
//! unselectable and undeletable everywhere downstream — the honest middle
//! between deleting something dangerous and staying silent about it.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::model::{Category, Finding, Severity};

/// One advisable location. `probe` skips any that is absent or empty, so a
/// machine without snap or apt simply never sees those rows.
struct Advisory {
    path: &'static str,
    category: Category,
    severity: Severity,
    note: &'static str,
    /// The command that actually reclaims this. Shown verbatim.
    command: &'static str,
    /// Ignore anything smaller; these are only worth a row when substantial.
    min_bytes: u64,
}

const MIB: u64 = 1024 * 1024;

const ADVISORIES: &[Advisory] = &[
    Advisory {
        path: "/var/lib/snapd/snaps",
        category: Category::Installers,
        severity: Severity::Moderate,
        note: "snap keeps superseded revisions of every installed package",
        command: "sudo snap set system refresh.retain=2",
        min_bytes: 512 * MIB,
    },
    Advisory {
        path: "/var/lib/flatpak/repo",
        category: Category::Installers,
        severity: Severity::Moderate,
        note: "Flatpak object store, including runtimes nothing uses any more",
        command: "flatpak uninstall --unused && flatpak repair",
        min_bytes: 512 * MIB,
    },
    Advisory {
        path: "/var/cache/apt/archives",
        category: Category::PackageCaches,
        severity: Severity::Safe,
        note: "downloaded .deb archives kept after installation",
        command: "sudo apt clean",
        min_bytes: 128 * MIB,
    },
    Advisory {
        path: "/var/lib/apt/lists",
        category: Category::PackageCaches,
        severity: Severity::Safe,
        note: "package index lists — rebuilt by the next update",
        command: "sudo apt clean && sudo apt update",
        min_bytes: 128 * MIB,
    },
    Advisory {
        path: "/var/cache/dnf",
        category: Category::PackageCaches,
        severity: Severity::Safe,
        note: "dnf metadata and downloaded packages",
        command: "sudo dnf clean all",
        min_bytes: 128 * MIB,
    },
    Advisory {
        path: "/var/cache/pacman/pkg",
        category: Category::PackageCaches,
        severity: Severity::Safe,
        note: "downloaded pacman packages kept after installation",
        command: "sudo paccache -r",
        min_bytes: 128 * MIB,
    },
    Advisory {
        path: "/var/lib/systemd/coredump",
        category: Category::SystemJunk,
        severity: Severity::Safe,
        note: "coredumps from crashed processes — only useful for debugging",
        command: "sudo rm -rf /var/lib/systemd/coredump/*",
        min_bytes: 64 * MIB,
    },
    Advisory {
        path: "/var/log/journal",
        category: Category::SystemJunk,
        severity: Severity::Safe,
        note: "systemd journal history",
        command: "sudo journalctl --vacuum-size=50M",
        min_bytes: 128 * MIB,
    },
];

/// Report every advisable location that exists and is large enough.
///
/// Sizes come from a direct walk of the location rather than the scanner,
/// because these paths are deliberately excluded from the scan. Unreadable
/// subdirectories are skipped silently: this runs unprivileged and most of
/// `/var` is root-owned, so a partial total is expected and still useful.
pub fn probe() -> Vec<Finding> {
    ADVISORIES.iter().filter_map(probe_one).collect()
}

fn probe_one(advisory: &Advisory) -> Option<Finding> {
    let path = Path::new(advisory.path);
    if !path.is_dir() {
        return None;
    }
    let (size_bytes, last_used) = tree_size(path);
    if size_bytes < advisory.min_bytes {
        return None;
    }
    Some(Finding {
        path: PathBuf::from(advisory.path),
        category: advisory.category,
        severity: advisory.severity,
        size_bytes,
        last_used,
        mount: None,
        note: advisory.note.to_owned(),
        advice: Some(advisory.command.to_owned()),
    })
}

/// Allocated bytes and newest mtime under `dir`, ignoring what we cannot read.
fn tree_size(dir: &Path) -> (u64, Option<DateTime<Utc>>) {
    use std::os::unix::fs::MetadataExt;

    let mut stack = vec![dir.to_path_buf()];
    let (mut bytes, mut newest) = (0u64, None::<DateTime<Utc>>);
    while let Some(path) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            let file_type = metadata.file_type();
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                bytes += metadata.blocks() * 512;
                if let Ok(modified) = metadata.modified() {
                    let timestamp: DateTime<Utc> = modified.into();
                    if newest.is_none_or(|current| timestamp > current) {
                        newest = Some(timestamp);
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

    /// The whole point of an advisory: it must never look deletable.
    #[test]
    fn every_advisory_carries_a_command_and_is_not_actionable() {
        for finding in probe() {
            assert!(
                finding.advice.is_some(),
                "{:?} has no command",
                finding.path
            );
            assert!(
                !finding.is_actionable(),
                "{:?} looks deletable",
                finding.path
            );
        }
    }

    /// Advisories name system paths the guard refuses. If one ever named a
    /// path the guard ALLOWED, it would be deletable and should have been an
    /// ordinary rule instead.
    #[test]
    fn advised_paths_are_all_guard_protected() {
        for advisory in ADVISORIES {
            let path = Path::new(advisory.path);
            assert!(
                crate::guard::check(path, Path::new("/")).is_err(),
                "{} is not guard-protected — it belongs in a rule, not here",
                advisory.path
            );
        }
    }

    #[test]
    fn commands_are_present_and_specific() {
        for advisory in ADVISORIES {
            assert!(!advisory.command.trim().is_empty(), "{}", advisory.path);
            assert!(
                advisory.note.len() > 20,
                "{} note is too thin",
                advisory.path
            );
            assert!(
                advisory.min_bytes > 0,
                "{} has no size floor",
                advisory.path
            );
        }
    }

    #[test]
    fn missing_locations_are_simply_absent() {
        let nowhere = Advisory {
            path: "/var/lib/definitely-not-installed-here",
            category: Category::SystemJunk,
            severity: Severity::Safe,
            note: "a location that does not exist on this machine",
            command: "true",
            min_bytes: 0,
        };
        assert!(probe_one(&nowhere).is_none());
    }
}

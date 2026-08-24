//! Parallel filesystem scanner — TO IMPLEMENT (core-engine agent).
//!
//! Requirements:
//! - jwalk-based parallel walk with per-entry metadata (size via st_blocks,
//!   mtime)
//! - prune well-known non-interesting subtrees early (proc/sys/dev/node_modules
//!   contents are NOT descended into once matched as findings)
//! - emit progress through an mpsc channel
//! - support cancellation via AtomicBool

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use jwalk::WalkDir;

use crate::guard;
use crate::model::{ChystikError, Finding, ScanProgress};
use crate::rules;
use std::path::{Path, PathBuf};

/// Scan configuration.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Follow symlinked directories? Always false for safety.
    pub follow_symlinks: bool,
    /// Skip entries matching these names at any depth.
    pub skip_names: Vec<String>,
    /// Also skip mount points of pseudo/network filesystems
    /// (`chystik_core::disks::unscannable_mount_points`). Nothing there is
    /// cleanable, and an unreachable network mount stalls the walk.
    pub skip_unscannable_mounts: bool,
    /// Drop findings smaller than this. Without a floor, ~3 of every 4
    /// findings on a real machine are under a megabyte — 77 Flutter SDK
    /// `.dart_tool` stubs totalling 0.4 MiB, and so on — which buries the
    /// handful of multi-gigabyte items that actually matter. Matched
    /// directories are still PRUNED, so the one-finding-per-subtree
    /// invariant holds either way.
    pub min_finding_bytes: u64,
    /// Paths the user has marked as never-touch. Pruned during the walk, so
    /// an excluded tree is never classified, never reported and therefore
    /// never selectable.
    pub exclude: Vec<PathBuf>,
    /// Append advisory findings for system locations the guard refuses.
    /// These are reported with the command that reclaims them and are never
    /// deletable by Chystik itself.
    pub include_advisories: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            follow_symlinks: false,
            skip_names: vec![
                "/proc".into(),
                "/sys".into(),
                "/dev".into(),
                "/run".into(),
                // Package-manager territory: guard refuses every deletion
                // here, so classifying it produced pure noise — 692 of 977
                // findings on a `/` scan, none of them actionable.
                "/usr".into(),
                "/var".into(),
                "/opt".into(),
            ],
            skip_unscannable_mounts: true,
            min_finding_bytes: 1024 * 1024,
            exclude: Vec::new(),
            include_advisories: true,
        }
    }
}

/// Run a scan of `root`, sending progress events to `tx`.
/// Returns the full findings list when finished.
pub fn scan(
    root: &Path,
    options: &ScanOptions,
    tx: Sender<ScanProgress>,
    cancel: &Arc<AtomicBool>,
) -> Result<Vec<Finding>, ChystikError> {
    scan_many(
        std::slice::from_ref(&root.to_path_buf()),
        options,
        tx,
        cancel,
    )
}

/// Scan several roots sequentially, aggregating their findings. Each root
/// emits its own `Started` event, but exactly ONE terminal event closes the
/// whole run: `Finished` carrying every root's findings, or `Cancelled`.
/// (A `Finished` per root would tell a UI the scan is over while later
/// roots are still being walked.)
pub fn scan_many(
    roots: &[PathBuf],
    options: &ScanOptions,
    tx: Sender<ScanProgress>,
    cancel: &Arc<AtomicBool>,
) -> Result<Vec<Finding>, ChystikError> {
    let dirs = Arc::new(AtomicU64::new(0));
    let mut all = Vec::new();
    for root in roots {
        match scan_root(root, options, &tx, cancel, &dirs) {
            Ok(found) => all.extend(found),
            Err(e) => {
                if matches!(e, ChystikError::Cancelled) {
                    let _ = tx.send(ScanProgress::Cancelled);
                }
                return Err(e);
            }
        }
    }
    if options.include_advisories {
        // Appended once for the whole run, not per root: these are absolute
        // system locations, unrelated to what the user chose to scan.
        all.extend(crate::advisories::probe());
    }
    let _ = tx.send(ScanProgress::Finished {
        findings: all.clone(),
    });
    Ok(all)
}

/// Walk one root. Emits `Started`, `DirectoriesScanned` and `FindingFound`;
/// terminal events are the caller's job (see [`scan_many`]).
fn scan_root(
    root: &Path,
    options: &ScanOptions,
    tx: &Sender<ScanProgress>,
    cancel: &Arc<AtomicBool>,
    dirs: &Arc<AtomicU64>,
) -> Result<Vec<Finding>, ChystikError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ChystikError::Cancelled);
    }
    let _ = tx.send(ScanProgress::Started {
        root: root.to_path_buf(),
    });

    let found: Arc<Mutex<Vec<Finding>>> = Arc::new(Mutex::new(Vec::new()));
    let walker_tx = tx.clone();
    let walker_dirs = dirs.clone();
    let walker_found = found.clone();
    let walker_cancel = cancel.clone();
    let mut skip = options.skip_names.clone();
    if options.skip_unscannable_mounts {
        skip.extend(
            crate::disks::unscannable_mount_points()
                .into_iter()
                .map(|m| m.to_string_lossy().into_owned()),
        );
    }
    skip.extend(
        options
            .exclude
            .iter()
            .map(|p| p.to_string_lossy().into_owned()),
    );
    // Never prune the scan root or one of its ancestors — that would
    // silently prune the entire walk. Scanning `/var` explicitly stays
    // possible even though `/var` is skipped during a `/` scan.
    skip.retain(|s| !root.starts_with(s));
    let min_bytes = options.min_finding_bytes;
    let mounts = crate::disks::mount_table();

    let walker = WalkDir::new(root)
        .follow_links(options.follow_symlinks)
        .skip_hidden(false)
        .process_read_dir(move |_depth, parent, _state, children| {
            // Checked here, not only in the consuming loop below: the
            // parallel walk keeps producing work even while the consumer
            // is blocked, so this is what makes Cancel take effect at once.
            if walker_cancel.load(Ordering::Relaxed) {
                for entry in children.iter_mut().flatten() {
                    entry.read_children_path = None;
                }
                return;
            }

            // A group-ruled directory belongs to that rule entirely: its
            // children are ordered here, the newest few are spared, and the
            // rest are reported individually. Nothing below is descended
            // into, and the per-path rules never see these children.
            //
            // Files count as well as directories — `~/.local/share/claude/
            // versions` holds one 300 MB executable per build, and a
            // directory-only pass found nothing there at all.
            if let Some(rule) = rules::classify_group(parent) {
                let mut candidates: Vec<(PathBuf, std::time::SystemTime, u64)> = children
                    .iter()
                    .flatten()
                    .filter(|e| !e.file_type().is_symlink())
                    .filter_map(|e| {
                        let path = e.path();
                        let meta = std::fs::symlink_metadata(&path).ok()?;
                        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                        Some((path, mtime, meta.len()))
                    })
                    .collect();
                // Newest first: each entry is written once, when its version
                // is fetched, so mtime is a faithful order here.
                candidates.sort_by_key(|(_, mtime, _)| std::cmp::Reverse(*mtime));

                for entry in children.iter_mut().flatten() {
                    entry.read_children_path = None;
                }

                for (path, _, _) in candidates.into_iter().skip(rule.keep) {
                    let n = walker_dirs.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(1000) {
                        let _ = walker_tx.send(ScanProgress::DirectoriesScanned { count: n });
                    }
                    let (size_bytes, last_used) = entry_stats(&path, &walker_cancel);
                    if size_bytes < min_bytes {
                        continue;
                    }
                    let finding = Finding {
                        path: path.clone(),
                        category: rule.category,
                        severity: rule.severity,
                        size_bytes,
                        last_used,
                        mount: crate::disks::mount_of_in(&path, &mounts),
                        note: rule.note.to_owned(),
                        advice: None,
                    };
                    let _ = walker_tx.send(ScanProgress::FindingFound(Box::new(finding.clone())));
                    walker_found.lock().unwrap().push(finding);
                }
                return;
            }

            for entry in children.iter_mut().flatten() {
                if entry.depth == 0 || entry.file_type().is_symlink() || !entry.file_type().is_dir()
                {
                    continue;
                }
                let path = entry.path();
                if skip.iter().any(|s| path.starts_with(s)) {
                    entry.read_children_path = None;
                    continue;
                }
                if !guard::is_scannable(&path) {
                    continue;
                }
                let n = walker_dirs.fetch_add(1, Ordering::Relaxed) + 1;
                if n.is_multiple_of(1000) {
                    let _ = walker_tx.send(ScanProgress::DirectoriesScanned { count: n });
                }
                if let Some(m) = rules::classify(&path) {
                    let (size_bytes, last_used) = dir_stats(&path, &walker_cancel);
                    // Prune regardless of the size floor: a matched subtree
                    // is claimed whether or not it is worth reporting.
                    entry.read_children_path = None;
                    if size_bytes < min_bytes {
                        continue;
                    }
                    let finding = Finding {
                        path: path.clone(),
                        category: m.category,
                        severity: m.severity,
                        size_bytes,
                        last_used,
                        mount: crate::disks::mount_of_in(&path, &mounts),
                        note: m.note,
                        advice: None,
                    };
                    let _ = walker_tx.send(ScanProgress::FindingFound(Box::new(finding.clone())));
                    walker_found.lock().unwrap().push(finding);
                }
            }
        });

    for result in walker.into_iter() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ChystikError::Cancelled);
        }
        if result.is_err() {
            continue;
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(ChystikError::Cancelled);
    }

    let findings = found
        .lock()
        .map_err(|_| ChystikError::Io(std::io::Error::other("scanner lock poisoned")))?
        .clone();
    Ok(findings)
}

/// Size and mtime of one entry, whether it is a file or a whole subtree.
fn entry_stats(path: &Path, cancel: &AtomicBool) -> (u64, Option<DateTime<Utc>>) {
    use std::os::unix::fs::MetadataExt;

    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return (0, None);
    };
    if meta.is_dir() {
        return dir_stats(path, cancel);
    }
    let last_used = meta.modified().ok().map(DateTime::<Utc>::from);
    (meta.blocks() * 512, last_used)
}

/// Sum of allocated blocks and max mtime over regular files in a subtree.
/// Used only for matched (pruned) directories, which jwalk will not enter again.
fn dir_stats(dir: &Path, cancel: &AtomicBool) -> (u64, Option<DateTime<Utc>>) {
    use std::os::unix::fs::MetadataExt;

    let mut stack = vec![dir.to_path_buf()];
    let (mut bytes, mut newest) = (0u64, None::<DateTime<Utc>>);
    while let Some(path) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break; // a cancelled scan discards its findings anyway
        }
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
                    if newest.map(|current| timestamp > current).unwrap_or(true) {
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    /// Fixtures are kilobytes, so the production size floor would drop
    /// every one of them; these tests are about walking, not filtering.
    fn test_options() -> ScanOptions {
        ScanOptions {
            min_finding_bytes: 0,
            // Advisories probe real system paths; a walk test must not
            // depend on what this machine happens to have installed.
            include_advisories: false,
            ..ScanOptions::default()
        }
    }

    fn seed_project(root: &std::path::Path) -> std::path::PathBuf {
        let proj = root.join("webapp");
        let nm = proj.join("node_modules/left-pad");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(proj.join("package.json"), "{}").unwrap();
        std::fs::write(proj.join("package-lock.json"), "{}").unwrap();
        std::fs::write(nm.join("index.js"), "let pad='x'.repeat(1024);").unwrap();
        proj
    }

    #[test]
    fn scan_finds_node_modules_once_and_prunes_nested() {
        let root = tempfile::tempdir().unwrap();
        seed_project(root.path());
        let (tx, _rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let findings = scan(root.path(), &test_options(), tx, &cancel).expect("scan ok");

        let nm: Vec<_> = findings
            .iter()
            .filter(|f| f.path.ends_with("node_modules"))
            .collect();
        assert_eq!(nm.len(), 1, "exactly one node_modules finding");
        assert!(
            findings.iter().all(|f| !f
                .path
                .starts_with(root.path().join("webapp/node_modules/left-pad"))),
            "children of pruned finding are separate findings"
        );
        assert!(!findings.is_empty());
        assert!(nm[0].size_bytes > 0);
    }

    #[test]
    fn scan_emits_progress_events() {
        let root = tempfile::tempdir().unwrap();
        seed_project(root.path());
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        scan(root.path(), &test_options(), tx, &cancel).unwrap();

        let events: Vec<_> = rx.try_iter().collect();
        assert!(events.len() >= 3);
        assert!(matches!(events.first(), Some(ScanProgress::Started { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, ScanProgress::FindingFound(_))));
        assert!(matches!(events.last(), Some(ScanProgress::Finished { .. })));
    }

    #[test]
    fn scan_many_aggregates_roots() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        seed_project(a.path());
        seed_project(b.path());
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let roots = vec![a.path().to_path_buf(), b.path().to_path_buf()];
        let findings = scan_many(&roots, &test_options(), tx, &cancel).expect("scan ok");
        assert!(findings.iter().any(|f| f.path.starts_with(a.path())));
        assert!(findings.iter().any(|f| f.path.starts_with(b.path())));

        // Exactly one terminal event, and it carries BOTH roots' findings:
        // a per-root `Finished` would make the GUI join the still-running
        // scanner thread and drop the earlier roots' results.
        let events: Vec<_> = rx.try_iter().collect();
        let terminal: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, ScanProgress::Finished { .. } | ScanProgress::Cancelled))
            .collect();
        assert_eq!(terminal.len(), 1, "one terminal event for the whole run");
        let Some(ScanProgress::Finished { findings: reported }) = events.last() else {
            panic!("run must end with Finished, got {:?}", events.last());
        };
        assert_eq!(reported.len(), findings.len());
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, ScanProgress::Started { .. }))
                .count(),
            2,
            "one Started per root"
        );
    }

    #[test]
    fn scan_respects_cancellation() {
        let root = tempfile::tempdir().unwrap();
        seed_project(root.path());
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(true));
        cancel.store(true, Ordering::Relaxed);
        let err =
            scan(root.path(), &test_options(), tx, &cancel).expect_err("cancelled scan errors");
        assert!(matches!(err, crate::model::ChystikError::Cancelled));
        let events: Vec<_> = rx.try_iter().collect();
        assert!(
            matches!(events.last(), Some(ScanProgress::Cancelled)),
            "a cancelled run still emits its one terminal event"
        );
    }

    #[test]
    fn cancel_midway_stops_and_reports_once() {
        let root = tempfile::tempdir().unwrap();
        for i in 0..40 {
            seed_project(&root.path().join(format!("p{i}")));
        }
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let roots = vec![root.path().to_path_buf(), root.path().to_path_buf()];
        // Flip the flag as soon as the walk reports its first event.
        let waiter = std::thread::spawn(move || {
            let first = rx.recv().expect("Started");
            flag.store(true, Ordering::SeqCst);
            let mut events = vec![first];
            events.extend(rx);
            events
        });
        let err =
            scan_many(&roots, &test_options(), tx, &cancel).expect_err("cancelled scan errors");
        assert!(matches!(err, ChystikError::Cancelled));
        let events = waiter.join().unwrap();
        assert!(matches!(events.last(), Some(ScanProgress::Cancelled)));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, ScanProgress::Finished { .. } | ScanProgress::Cancelled))
                .count(),
            1
        );
    }

    /// A versioned store must lose its old entries and keep the newest.
    #[test]
    fn group_rules_spare_the_newest_and_report_the_rest() {
        use std::time::{Duration, SystemTime};

        let _env = crate::rules::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", home.path());

        // Entries here are FILES on a real machine, not directories: one
        // executable per build. A directory-only pass found nothing.
        let versions = home.path().join(".local/share/claude/versions");
        std::fs::create_dir_all(&versions).unwrap();
        let now = SystemTime::now();
        for (name, age_days) in [("2.1.237", 30u64), ("2.1.241", 5), ("2.1.242", 0)] {
            let file = versions.join(name);
            std::fs::write(&file, vec![0u8; 4096]).unwrap();
            let when = now - Duration::from_secs(age_days * 86_400);
            filetime_set(&file, when);
        }

        let (tx, _rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let findings = scan(home.path(), &test_options(), tx, &cancel).expect("scan ok");
        std::env::remove_var("CHYSTIK_TEST_HOME");

        let names: Vec<String> = findings
            .iter()
            .filter(|f| f.path.starts_with(&versions))
            .filter_map(|f| f.path.file_name()?.to_str().map(str::to_owned))
            .collect();
        assert!(
            !names.contains(&"2.1.242".to_string()),
            "the newest build must be spared, got {names:?}"
        );
        assert_eq!(names.len(), 2, "both older builds reported, got {names:?}");

        // The parent itself is never offered: deleting it would take the
        // running version with it.
        assert!(
            !findings.iter().any(|f| f.path == versions),
            "the store directory itself must never be a finding"
        );
    }

    /// Set an entry's mtime without pulling in a crate for it.
    fn filetime_set(path: &std::path::Path, when: std::time::SystemTime) {
        let secs = when
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let times = [
            libc::timespec {
                tv_sec: secs,
                tv_nsec: 0,
            },
            libc::timespec {
                tv_sec: secs,
                tv_nsec: 0,
            },
        ];
        let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: c_path is NUL-terminated and `times` is a valid 2-element array.
        unsafe {
            libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0);
        }
    }

    #[test]
    fn excluded_paths_are_never_reported() {
        let root = tempfile::tempdir().unwrap();
        let project = seed_project(root.path());
        let options = ScanOptions {
            exclude: vec![project.clone()],
            ..test_options()
        };
        let (tx, _rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let findings = scan(root.path(), &options, tx, &cancel).expect("scan ok");
        assert!(
            findings.iter().all(|f| !f.path.starts_with(&project)),
            "an excluded tree must not appear at all: {findings:?}"
        );

        // Without the exclusion the same tree is found, so the test is
        // proving the exclusion and not an empty fixture.
        let (tx2, _rx2) = mpsc::channel();
        let baseline = scan(root.path(), &test_options(), tx2, &cancel).expect("scan ok");
        assert!(baseline.iter().any(|f| f.path.starts_with(&project)));
    }

    #[test]
    fn default_options_prune_unscannable_mounts_and_system_dirs() {
        let d = ScanOptions::default();
        assert!(d.skip_unscannable_mounts);
        assert!(d.min_finding_bytes > 0, "a size floor is on by default");
        assert!(d.include_advisories, "system advice is on by default");
        assert!(
            d.exclude.is_empty(),
            "nothing is excluded until the user says so"
        );
        for system in ["/usr", "/var", "/opt", "/proc", "/sys", "/dev", "/run"] {
            assert!(
                d.skip_names.iter().any(|s| s == system),
                "{system} must be skipped"
            );
        }
    }

    #[test]
    fn size_floor_drops_small_findings_but_still_prunes() {
        let root = tempfile::tempdir().unwrap();
        seed_project(root.path());
        let (tx, _rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let options = ScanOptions {
            min_finding_bytes: u64::MAX,
            ..test_options()
        };
        let findings = scan(root.path(), &options, tx, &cancel).expect("scan ok");
        assert!(
            findings.is_empty(),
            "everything is below an impossible floor"
        );
    }

    #[test]
    fn an_explicitly_scanned_system_root_is_not_pruned_away() {
        // `/var` is skipped during a `/` scan, but scanning it directly
        // must still walk it — otherwise the skip list silently prunes the
        // entire run.
        let root = tempfile::tempdir().unwrap();
        seed_project(root.path());
        let (tx, _rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let options = ScanOptions {
            skip_names: vec![root.path().to_string_lossy().into_owned()],
            ..test_options()
        };
        let findings = scan(root.path(), &options, tx, &cancel).expect("scan ok");
        assert!(
            !findings.is_empty(),
            "the scan root exempts itself from the skip list"
        );
    }
}

//! Parallel filesystem scanner.
//!
//! - jwalk-based parallel walk with per-entry metadata (size via st_blocks,
//!   mtime)
//! - prune well-known non-interesting subtrees early (proc/sys/dev/node_modules
//!   contents are NOT descended into once matched as findings)
//! - emit progress to either a compatibility mpsc channel or a streaming sink
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
    /// (`chystik_core::platform::Platform::unscannable_roots`). Nothing there is
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

/// Terminal counters for a streaming scan. A CLI JSONL consumer can receive
/// every finding while retaining only these two numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanSummary {
    pub findings: u64,
    pub total_bytes: u64,
}

/// Progress sent by [`scan_many_stream`]. Unlike [`ScanProgress`], its
/// terminal event never embeds a complete findings vector.
#[derive(Debug, Clone)]
pub enum ScanStreamEvent {
    Started { root: PathBuf },
    DirectoriesScanned { count: u64 },
    FindingFound(Box<Finding>),
    Finished(ScanSummary),
    Cancelled,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            follow_symlinks: false,
            // Platform policy owns system roots. This makes an explicit scan
            // of `C:\\`/`/` safe without teaching the rule engine OS paths.
            skip_names: crate::platform::current()
                .default_skip_roots()
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
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
    let all = Arc::new(Mutex::new(Vec::new()));
    let collected = all.clone();
    let compatibility_tx = tx.clone();
    let result = scan_many_stream(roots, options, cancel, move |event| match event {
        ScanStreamEvent::Started { root } => {
            let _ = compatibility_tx.send(ScanProgress::Started { root });
        }
        ScanStreamEvent::DirectoriesScanned { count } => {
            let _ = compatibility_tx.send(ScanProgress::DirectoriesScanned { count });
        }
        ScanStreamEvent::FindingFound(finding) => {
            collected.lock().unwrap().push((*finding).clone());
            let _ = compatibility_tx.send(ScanProgress::FindingFound(finding));
        }
        ScanStreamEvent::Finished(_) | ScanStreamEvent::Cancelled => {}
    });
    match result {
        Ok(_) => {
            let findings = all
                .lock()
                .map_err(|_| ChystikError::Io(std::io::Error::other("scanner lock poisoned")))?
                .clone();
            let _ = tx.send(ScanProgress::Finished {
                findings: findings.clone(),
            });
            Ok(findings)
        }
        Err(error) => {
            if matches!(error, ChystikError::Cancelled) {
                let _ = tx.send(ScanProgress::Cancelled);
            }
            Err(error)
        }
    }
}

/// Stream several roots without retaining their findings. The callback may be
/// called by scanner worker threads, so callers that write to a single stream
/// must synchronize that writer themselves. Exactly one terminal event is
/// emitted for the whole run.
pub fn scan_many_stream<F>(
    roots: &[PathBuf],
    options: &ScanOptions,
    cancel: &Arc<AtomicBool>,
    on_event: F,
) -> Result<ScanSummary, ChystikError>
where
    F: Fn(ScanStreamEvent) + Send + Sync + 'static,
{
    let callback: Arc<dyn Fn(ScanStreamEvent) + Send + Sync> = Arc::new(on_event);
    let dirs = Arc::new(AtomicU64::new(0));
    let findings = Arc::new(AtomicU64::new(0));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let counted_callback = callback.clone();
    let counted_findings = findings.clone();
    let counted_bytes = total_bytes.clone();
    let emit: Arc<dyn Fn(ScanStreamEvent) + Send + Sync> = Arc::new(move |event| {
        if let ScanStreamEvent::FindingFound(finding) = &event {
            counted_findings.fetch_add(1, Ordering::Relaxed);
            counted_bytes.fetch_add(finding.size_bytes, Ordering::Relaxed);
        }
        counted_callback(event);
    });
    let rule_engine = rules::RuleEngine::current();

    for root in roots {
        if let Err(error) = scan_root(root, options, cancel, &dirs, &emit, &rule_engine) {
            if matches!(error, ChystikError::Cancelled) {
                callback(ScanStreamEvent::Cancelled);
            }
            return Err(error);
        }
    }
    if options.include_advisories {
        // Appended once for the whole run, not per root: these are absolute
        // system locations, unrelated to what the user chose to scan.
        for finding in crate::advisories::probe() {
            emit(ScanStreamEvent::FindingFound(Box::new(finding)));
        }
    }
    let summary = ScanSummary {
        findings: findings.load(Ordering::Relaxed),
        total_bytes: total_bytes.load(Ordering::Relaxed),
    };
    callback(ScanStreamEvent::Finished(summary));
    Ok(summary)
}

/// Walk one root. Terminal events are the caller's job (see
/// [`scan_many_stream`]).
fn scan_root(
    root: &Path,
    options: &ScanOptions,
    cancel: &Arc<AtomicBool>,
    dirs: &Arc<AtomicU64>,
    emit: &Arc<dyn Fn(ScanStreamEvent) + Send + Sync>,
    rule_engine: &rules::RuleEngine,
) -> Result<(), ChystikError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ChystikError::Cancelled);
    }
    emit(ScanStreamEvent::Started {
        root: root.to_path_buf(),
    });

    let host = crate::platform::current();
    let walker_emit = emit.clone();
    let walker_dirs = dirs.clone();
    let walker_cancel = cancel.clone();
    let walker_rules = rule_engine.clone();
    let mut skip = options.skip_names.clone();
    if options.skip_unscannable_mounts {
        skip.extend(
            host.unscannable_roots()
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
    let mounts = host.storage_volumes();

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
                    .filter(|e| !host.is_link_or_reparse_point(&e.path()))
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
                        walker_emit(ScanStreamEvent::DirectoriesScanned { count: n });
                    }
                    let (size_bytes, last_used) = entry_stats(&path, &walker_cancel, host);
                    if size_bytes < min_bytes {
                        continue;
                    }
                    let finding = Finding {
                        path: path.clone(),
                        category: rule.category,
                        severity: rule.severity,
                        size_bytes,
                        last_used,
                        mount: crate::platform::mount_of(&path, &mounts),
                        note: rule.note.to_owned(),
                        advice: None,
                        provenance: None,
                    };
                    walker_emit(ScanStreamEvent::FindingFound(Box::new(finding)));
                }
                return;
            }

            for entry in children.iter_mut().flatten() {
                if entry.depth == 0
                    || entry.file_type().is_symlink()
                    || host.is_link_or_reparse_point(&entry.path())
                    || !entry.file_type().is_dir()
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
                    walker_emit(ScanStreamEvent::DirectoriesScanned { count: n });
                }
                if let Some(classified) = walker_rules.classify_with_metadata(&path) {
                    let m = classified.matched;
                    let catalog = classified.catalog;
                    let (size_bytes, last_used) = dir_stats(&path, &walker_cancel, host);
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
                        mount: crate::platform::mount_of(&path, &mounts),
                        note: m.note,
                        advice: catalog
                            .as_ref()
                            .and_then(|metadata| metadata.advice.clone()),
                        provenance: catalog.map(|metadata| metadata.provenance),
                    };
                    walker_emit(ScanStreamEvent::FindingFound(Box::new(finding)));
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

    Ok(())
}

/// Size and mtime of one entry, whether it is a file or a whole subtree.
fn entry_stats(
    path: &Path,
    cancel: &AtomicBool,
    host: crate::platform::Platform,
) -> (u64, Option<DateTime<Utc>>) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return (0, None);
    };
    if meta.is_dir() {
        return dir_stats(path, cancel, host);
    }
    let last_used = meta.modified().ok().map(DateTime::<Utc>::from);
    (host.allocated_bytes(&meta), last_used)
}

/// Sum of allocated blocks and max mtime over regular files in a subtree.
/// Used only for matched (pruned) directories, which jwalk will not enter again.
fn dir_stats(
    dir: &Path,
    cancel: &AtomicBool,
    host: crate::platform::Platform,
) -> (u64, Option<DateTime<Utc>>) {
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
            if file_type.is_dir() && !host.is_link_or_reparse_point(&entry.path()) {
                stack.push(entry.path());
            } else if file_type.is_file() {
                bytes += host.allocated_bytes(&metadata);
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
    fn streaming_scan_emits_each_finding_without_returning_a_findings_buffer() {
        let root = tempfile::tempdir().unwrap();
        seed_project(root.path());
        let cancel = Arc::new(AtomicBool::new(false));
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let seen = emitted.clone();

        let summary = scan_many_stream(
            &[root.path().to_path_buf()],
            &test_options(),
            &cancel,
            move |event| {
                if let ScanStreamEvent::FindingFound(finding) = event {
                    seen.lock().unwrap().push(finding.path);
                }
            },
        )
        .expect("stream scan ok");

        let paths = emitted.lock().unwrap();
        assert_eq!(summary.findings, paths.len() as u64);
        assert!(paths.iter().any(|path| path.ends_with("node_modules")));
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
    #[cfg(unix)]
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

    #[cfg(target_os = "linux")]
    #[test]
    fn catalog_findings_carry_provenance_and_policy_into_scan_output() {
        let _env = crate::rules::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join("cache");
        let pip = cache.join("pip");
        std::fs::create_dir_all(&pip).unwrap();
        std::fs::write(pip.join("wheel.whl"), vec![0u8; 4096]).unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", home.path());
        std::env::set_var("XDG_CACHE_HOME", &cache);

        let (tx, _rx) = mpsc::channel();
        let findings = scan(
            home.path(),
            &test_options(),
            tx,
            &Arc::new(AtomicBool::new(false)),
        )
        .expect("scan catalog fixture");

        std::env::remove_var("XDG_CACHE_HOME");
        std::env::remove_var("CHYSTIK_TEST_HOME");
        let finding = findings
            .iter()
            .find(|finding| finding.path == pip)
            .expect("pip cache finding");
        let provenance = finding.provenance.as_ref().expect("catalog provenance");
        assert_eq!(provenance.rule_id, "python.pip.cache");
        assert_eq!(provenance.policy, crate::model::FindingPolicy::DirectSafe);
        assert!(finding.advice.is_none());
    }

    /// Set an entry's mtime without pulling in a crate for it.
    #[cfg(unix)]
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
        for system in crate::platform::current().default_skip_roots() {
            assert!(
                d.skip_names
                    .iter()
                    .any(|name| name == &system.to_string_lossy()),
                "{} must be skipped",
                system.display()
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

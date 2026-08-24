//! End-to-end deletion flow, run on every CI job.
//!
//! Until the flow moved behind `cleaner::Remover` the only test that touched
//! it needed a real desktop trash and was therefore `#[ignore]`d — the one
//! code path that destroys things was never exercised automatically. These
//! run the real scanner and the real guard against a temporary tree, and
//! substitute the trash so nothing leaves the sandbox.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};

use chystik_core::cleaner::{clean, CleanupItem, FileIdentity, Remover, SkipReason};
use chystik_core::model::{ChystikError, Finding};
use chystik_core::scanner::{self, ScanOptions};

/// Records every path handed to it and actually removes it, so the flow can
/// be observed without involving the desktop trash.
#[derive(Default)]
struct FakeTrash {
    seen: Mutex<Vec<PathBuf>>,
}

impl Remover for FakeTrash {
    fn remove(&self, path: &Path) -> Result<(), ChystikError> {
        self.seen.lock().unwrap().push(path.to_path_buf());
        std::fs::remove_dir_all(path)
            .or_else(|_| std::fs::remove_file(path))
            .map_err(ChystikError::Io)
    }
}

impl FakeTrash {
    fn seen(&self) -> Vec<PathBuf> {
        self.seen.lock().unwrap().clone()
    }

    fn saw(&self, path: &Path) -> bool {
        self.seen().iter().any(|p| p == path)
    }
}

fn scan_options() -> ScanOptions {
    ScanOptions {
        // Fixtures are kilobytes, and the advisories probe real system
        // paths that have nothing to do with this tree.
        min_finding_bytes: 0,
        include_advisories: false,
        ..ScanOptions::default()
    }
}

fn scan(root: &Path) -> Vec<Finding> {
    let (tx, _rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    scanner::scan(root, &scan_options(), tx, &cancel).expect("scan succeeds")
}

/// A JavaScript project the rules recognise: `node_modules` beside a lockfile.
fn seed_project(root: &Path, name: &str) -> PathBuf {
    let project = root.join(name);
    let modules = project.join("node_modules/left-pad");
    std::fs::create_dir_all(&modules).unwrap();
    std::fs::write(project.join("package.json"), "{}").unwrap();
    std::fs::write(project.join("package-lock.json"), "{}").unwrap();
    std::fs::write(modules.join("index.js"), "x".repeat(2048)).unwrap();
    project
}

fn items(findings: &[Finding], root: &Path) -> Vec<CleanupItem> {
    findings
        .iter()
        .map(|f| CleanupItem {
            path: f.path.clone(),
            size_bytes: f.size_bytes,
            scan_root: Some(root.to_path_buf()),
        })
        .collect()
}

#[test]
fn scan_then_clean_removes_exactly_what_was_found() {
    let root = tempfile::tempdir().unwrap();
    let project = seed_project(root.path(), "webapp");
    let findings = scan(root.path());
    assert!(!findings.is_empty(), "fixture produced no findings");

    let trash = FakeTrash::default();
    let outcome = clean(&items(&findings, root.path()), &trash);

    assert_eq!(outcome.removed_count(), findings.len());
    assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    assert!(outcome.freed_bytes > 0);
    assert!(!project.join("node_modules").exists());
    // The project itself is not a finding and must survive.
    assert!(project.join("package.json").exists());
}

/// A finding reached through a symlinked ancestor must never be removed.
///
/// `guard::check` inspected only the final component, so a path like
/// `<root>/link/node_modules` lstatted a real directory and passed every
/// lexical test while pointing into somewhere the user cares about.
#[test]
fn a_symlinked_ancestor_is_refused_and_the_real_tree_survives() {
    let root = tempfile::tempdir().unwrap();
    let real = seed_project(root.path(), "important");
    let link = root.path().join("innocent-looking");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let through_link = link.join("node_modules");
    assert!(
        std::fs::symlink_metadata(&through_link).is_ok(),
        "fixture must lstat cleanly or it proves nothing"
    );

    let trash = FakeTrash::default();
    let outcome = clean(
        &[CleanupItem {
            path: through_link.clone(),
            size_bytes: 1,
            scan_root: Some(root.path().to_path_buf()),
        }],
        &trash,
    );

    assert!(!trash.saw(&through_link), "the remover was handed a link");
    assert_eq!(outcome.skipped[0].reason, SkipReason::Refused);
    assert!(
        real.join("node_modules/left-pad/index.js").exists(),
        "the real tree was destroyed through the link"
    );
}

/// Swapping the target between validation and removal must be caught.
#[test]
fn a_directory_swapped_for_a_link_after_validation_is_not_removed() {
    let root = tempfile::tempdir().unwrap();
    let project = seed_project(root.path(), "webapp");
    let target = project.join("node_modules");
    let elsewhere = seed_project(root.path(), "elsewhere");

    // What the guard saw at validation time.
    let validated = FileIdentity::of(&target).expect("target exists");

    // The race: the validated directory becomes a link somewhere else.
    std::fs::remove_dir_all(&target).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &target).unwrap();

    assert!(
        !validated.still_matches(&target),
        "the substitution went unnoticed"
    );

    let trash = FakeTrash::default();
    let outcome = clean(
        &[CleanupItem {
            path: target.clone(),
            size_bytes: 1,
            scan_root: Some(root.path().to_path_buf()),
        }],
        &trash,
    );
    assert!(!trash.saw(&target));
    assert_eq!(outcome.skipped[0].reason, SkipReason::Refused);
    assert!(
        elsewhere.join("node_modules").exists(),
        "the link target was followed and removed"
    );
}

/// Advisory findings name system space Chystik does not own.
#[test]
fn advisory_findings_are_never_handed_to_the_remover() {
    let root = tempfile::tempdir().unwrap();
    let trash = FakeTrash::default();
    let advisories = chystik_core::advisories::probe();

    let outcome = clean(&items(&advisories, root.path()), &trash);
    assert!(trash.seen().is_empty(), "an advisory reached the remover");
    assert_eq!(outcome.removed_count(), 0);
    assert_eq!(outcome.skipped_count(), advisories.len());
}

/// One bad item must not stop the rest of a batch.
#[test]
fn a_refused_item_does_not_block_the_others() {
    let root = tempfile::tempdir().unwrap();
    let good = seed_project(root.path(), "good");
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(outside.path().join("elsewhere")).unwrap();

    let trash = FakeTrash::default();
    let outcome = clean(
        &[
            CleanupItem {
                path: outside.path().join("elsewhere"),
                size_bytes: 1,
                scan_root: Some(root.path().to_path_buf()),
            },
            CleanupItem {
                path: good.join("node_modules"),
                size_bytes: 2,
                scan_root: Some(root.path().to_path_buf()),
            },
        ],
        &trash,
    );

    assert_eq!(outcome.removed_count(), 1);
    assert_eq!(outcome.skipped_count(), 1);
    assert!(!good.join("node_modules").exists());
}

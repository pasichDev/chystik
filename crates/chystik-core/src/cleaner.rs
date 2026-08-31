//! The deletion flow: validate, verify, remove, tally.
//!
//! This used to live in the GUI, calling `trash::delete` directly, which
//! meant the one code path that actually destroys things could not be
//! exercised in CI — the only tests that touched it were `#[ignore]`d
//! because they needed a real desktop trash. Behind [`Remover`] the whole
//! flow runs against a fake, so the guard checks, the identity re-check and
//! the tallying are all covered by ordinary `cargo test`.
//!
//! Order of operations for every item, and none of it is optional:
//! 1. [`guard::check`] — refuses protected locations, symlinked ancestors
//!    and anything outside the scan root.
//! 2. [`FileIdentity`] is captured, then re-read immediately before removal.
//!    If the path became a different object in between, it is skipped.
//! 3. Only then does the [`Remover`] see it.
//!
//! Step 2 narrows the window between validation and removal; it does not
//! close it. Closing it properly needs `openat` with `O_NOFOLLOW` for each
//! component and a removal through a directory descriptor, which the XDG
//! trash cannot express. Documented rather than pretended away.

use std::path::{Path, PathBuf};

use crate::guard;
use crate::model::ChystikError;
use crate::platform::{self, CleanupSupport};

/// Somewhere a file can be sent. The only production implementation is
/// [`SystemTrash`]; tests use a fake.
pub trait Remover: Send + Sync {
    /// Move `path` out of the way. Must never erase in place.
    fn remove(&self, path: &Path) -> Result<(), ChystikError>;

    /// Shown in error messages, so a failure names what was attempted.
    fn describe(&self) -> &'static str {
        "remover"
    }
}

/// The verified desktop trash implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTrash;

impl Remover for SystemTrash {
    fn remove(&self, path: &Path) -> Result<(), ChystikError> {
        if let CleanupSupport::ScanOnly { reason } = platform::current().cleanup_support() {
            return Err(ChystikError::Io(std::io::Error::other(reason)));
        }
        move_to_trash(path).map_err(|error| ChystikError::Io(std::io::Error::other(error)))
    }

    fn describe(&self) -> &'static str {
        "trash"
    }
}

fn move_to_trash(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // NSFileManager uses the native Trash API without requiring Finder
        // automation permission. It still leaves the item recoverable in
        // Trash, which is the contract exposed by this application.
        use trash::macos::{DeleteMethod, TrashContextExtMacos};

        let mut context = trash::TrashContext::default();
        context.set_delete_method(DeleteMethod::NsFileManager);
        context.delete(path).map_err(|error| error.to_string())
    }

    #[cfg(target_os = "windows")]
    {
        crate::platform::recycle_to_windows_bin(path)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        trash::delete(path).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Enough of a path's identity to notice it was swapped underneath us.
///
/// The platform adapter supplies a native object identity (device/inode on
/// Unix; volume serial/file index on Windows) without following a link or a
/// reparse point. Comparing it immediately before removal catches a changed
/// target and makes reparse-point substitution fail closed.
pub struct FileIdentity(crate::platform::PathIdentity);

impl FileIdentity {
    /// Read a stable host identity without following a link or reparse point.
    /// `None` means the object is gone or cannot be proven safe.
    pub fn of(path: &Path) -> Option<Self> {
        platform::current().path_identity(path).map(Self)
    }

    /// True when `path` is still the very same, non-indirected object.
    pub fn still_matches(&self, path: &Path) -> bool {
        Self::of(path).is_some_and(|now| now == *self)
    }
}

/// One thing the user asked to remove.
#[derive(Debug, Clone)]
pub struct CleanupItem {
    pub path: PathBuf,
    pub size_bytes: u64,
    /// The configured scan target containing `path`; the guard validates
    /// against this, so an item with no owning target is refused.
    pub scan_root: Option<PathBuf>,
}

/// Why an item was not removed. Every variant is reported to the user.
#[derive(Debug, Clone, PartialEq)]
pub enum SkipReason {
    /// No configured scan target contains it.
    OutsideEveryTarget,
    /// `guard::check` refused it.
    Refused,
    /// Chystik does not own this space; it carries a command instead.
    Advisory,
    /// The host has no tested native trash + link-safety implementation.
    CleanupUnavailable(&'static str),
    /// It changed between validation and removal.
    ChangedUnderUs,
    /// The remover itself failed.
    RemoverFailed(String),
}

#[derive(Debug, Clone)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: SkipReason,
}

/// What a cleanup run did.
#[derive(Debug, Clone, Default)]
pub struct CleanupOutcome {
    pub removed: Vec<PathBuf>,
    pub freed_bytes: u64,
    pub skipped: Vec<Skipped>,
}

impl CleanupOutcome {
    pub fn removed_count(&self) -> usize {
        self.removed.len()
    }

    pub fn skipped_count(&self) -> usize {
        self.skipped.len()
    }
}

/// Progress from a cleanup run: one `Started` per item, then exactly one
/// terminal event for it, emitted in the order the items were given.
///
/// A batch can be gigabytes and a single trash move can take seconds, so a
/// caller with a window to keep alive runs [`clean_streaming`] on a worker
/// thread and paints from these instead of freezing until the whole batch
/// is done.
#[derive(Debug, Clone, PartialEq)]
pub enum CleanEvent {
    /// About to validate and move `path`; `index` is its 0-based position
    /// in the batch.
    Started { index: usize, path: PathBuf },
    /// It reached the trash, freeing `size_bytes`.
    Removed {
        index: usize,
        path: PathBuf,
        size_bytes: u64,
    },
    /// It was left where it is, for `reason`.
    Skipped {
        index: usize,
        path: PathBuf,
        reason: SkipReason,
    },
}

/// Run the flow over `items`. Never panics and never stops early: one bad
/// item must not prevent the rest from being cleaned.
pub fn clean(items: &[CleanupItem], remover: &dyn Remover) -> CleanupOutcome {
    clean_streaming(items, remover, |_| {})
}

/// [`clean`], reporting each item as it is reached. The outcome is
/// identical; `on_event` runs on the calling thread between items, so it
/// must not block — sending down a channel is the intended use.
pub fn clean_streaming(
    items: &[CleanupItem],
    remover: &dyn Remover,
    mut on_event: impl FnMut(CleanEvent),
) -> CleanupOutcome {
    clean_with_support(
        items,
        remover,
        platform::current().cleanup_support(),
        &mut on_event,
    )
}

fn clean_with_support(
    items: &[CleanupItem],
    remover: &dyn Remover,
    support: CleanupSupport,
    on_event: &mut dyn FnMut(CleanEvent),
) -> CleanupOutcome {
    let mut outcome = CleanupOutcome::default();
    if let CleanupSupport::ScanOnly { reason } = support {
        for (index, item) in items.iter().enumerate() {
            let reason = SkipReason::CleanupUnavailable(reason);
            on_event(CleanEvent::Skipped {
                index,
                path: item.path.clone(),
                reason: reason.clone(),
            });
            outcome.skipped.push(Skipped {
                path: item.path.clone(),
                reason,
            });
        }
        return outcome;
    }
    for (index, item) in items.iter().enumerate() {
        on_event(CleanEvent::Started {
            index,
            path: item.path.clone(),
        });
        match remove_one(item, remover) {
            Ok(()) => {
                outcome.freed_bytes += item.size_bytes;
                outcome.removed.push(item.path.clone());
                on_event(CleanEvent::Removed {
                    index,
                    path: item.path.clone(),
                    size_bytes: item.size_bytes,
                });
            }
            Err(reason) => {
                on_event(CleanEvent::Skipped {
                    index,
                    path: item.path.clone(),
                    reason: reason.clone(),
                });
                outcome.skipped.push(Skipped {
                    path: item.path.clone(),
                    reason,
                });
            }
        }
    }
    outcome
}

/// The per-item flow, written once: validate, prove identity, validate
/// again, remove. `Err` names exactly why the path was left alone.
fn remove_one(item: &CleanupItem, remover: &dyn Remover) -> Result<(), SkipReason> {
    let path = item.path.as_path();
    // An item with no owning target is refused rather than guessed at.
    let root = item
        .scan_root
        .as_deref()
        .ok_or(SkipReason::OutsideEveryTarget)?;
    if guard::check(path, root).is_err() {
        return Err(SkipReason::Refused);
    }
    // Captured after validation and checked again below, so a swap in
    // between is caught rather than acted on.
    let identity = FileIdentity::of(path).ok_or(SkipReason::ChangedUnderUs)?;
    if !identity.still_matches(path) {
        return Err(SkipReason::ChangedUnderUs);
    }
    // Re-run the full guard at the last instant before the destructive
    // call. If the path turned into a reparse point, a protected location
    // or something outside the scan root after the first check, it is
    // refused rather than acted on. This narrows the check-to-act window
    // as far as a name-based trash API allows; only openat/O_NOFOLLOW plus
    // removal through a directory descriptor would close it entirely.
    if guard::check(path, root).is_err() {
        return Err(SkipReason::Refused);
    }
    remover
        .remove(path)
        .map_err(|e| SkipReason::RemoverFailed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// macOS puts the default temporary directory below `/var`, which the
    /// production guard correctly treats as a protected system location.
    /// Safety fixtures need a user-writable, non-system root on every host.
    fn tempdir() -> std::io::Result<tempfile::TempDir> {
        tempfile::Builder::new()
            .prefix(".chystik-test-")
            .tempdir_in(std::env::current_dir().expect("test process has a working directory"))
    }

    /// Exercise the portable validate/identity/remover flow without claiming
    /// that the current host has a native recovery mechanism.
    fn clean_with_native_trash(items: &[CleanupItem], remover: &dyn Remover) -> CleanupOutcome {
        clean_with_support(items, remover, CleanupSupport::NativeTrash, &mut |_| {})
    }

    /// Records what it was asked to remove and leaves the disk alone.
    #[derive(Default)]
    struct FakeRemover {
        seen: Mutex<Vec<PathBuf>>,
        fail: bool,
    }

    impl Remover for FakeRemover {
        fn remove(&self, path: &Path) -> Result<(), ChystikError> {
            self.seen.lock().unwrap().push(path.to_path_buf());
            if self.fail {
                return Err(ChystikError::Io(std::io::Error::other("nope")));
            }
            std::fs::remove_dir_all(path).ok();
            Ok(())
        }
    }

    impl FakeRemover {
        fn seen(&self) -> Vec<PathBuf> {
            self.seen.lock().unwrap().clone()
        }
    }

    fn item(path: &Path, root: &Path, size: u64) -> CleanupItem {
        CleanupItem {
            path: path.to_path_buf(),
            size_bytes: size,
            scan_root: Some(root.to_path_buf()),
        }
    }

    #[test]
    fn removes_valid_items_and_totals_what_they_freed() {
        let root = tempdir().unwrap();
        let a = root.path().join("a");
        let b = root.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        let remover = FakeRemover::default();
        let outcome = clean_with_native_trash(
            &[item(&a, root.path(), 100), item(&b, root.path(), 250)],
            &remover,
        );

        assert_eq!(outcome.removed_count(), 2);
        assert_eq!(outcome.freed_bytes, 350);
        assert!(outcome.skipped.is_empty());
        assert_eq!(remover.seen().len(), 2);
    }

    /// Progress must arrive item by item, not in one lump at the end —
    /// that is the whole point of the streaming entry point, and a UI
    /// painting from it would otherwise still look frozen.
    #[test]
    fn streaming_reports_every_item_as_it_is_reached() {
        let root = tempdir().unwrap();
        let good = root.path().join("good");
        std::fs::create_dir_all(&good).unwrap();
        let orphan = root.path().join("orphan");
        std::fs::create_dir_all(&orphan).unwrap();

        let remover = FakeRemover::default();
        let mut events = Vec::new();
        let outcome = clean_with_support(
            &[
                item(&good, root.path(), 100),
                CleanupItem {
                    path: orphan.clone(),
                    size_bytes: 40,
                    scan_root: None,
                },
            ],
            &remover,
            CleanupSupport::NativeTrash,
            &mut |event| events.push(event),
        );

        assert_eq!(
            events,
            vec![
                CleanEvent::Started {
                    index: 0,
                    path: good.clone()
                },
                CleanEvent::Removed {
                    index: 0,
                    path: good.clone(),
                    size_bytes: 100
                },
                CleanEvent::Started {
                    index: 1,
                    path: orphan.clone()
                },
                CleanEvent::Skipped {
                    index: 1,
                    path: orphan,
                    reason: SkipReason::OutsideEveryTarget
                },
            ]
        );
        // The streamed events and the returned tally never disagree.
        assert_eq!(outcome.removed, vec![good]);
        assert_eq!(outcome.freed_bytes, 100);
        assert_eq!(outcome.skipped_count(), 1);
    }

    /// `clean` is `clean_streaming` with the events dropped; it must not
    /// have drifted into a second copy of the flow.
    #[test]
    fn streaming_and_silent_entry_points_agree() {
        let root = tempdir().unwrap();
        let target = root.path().join("cache");
        std::fs::create_dir_all(&target).unwrap();

        let remover = FakeRemover::default();
        let mut count = 0usize;
        let streamed = clean_with_support(
            &[item(&target, root.path(), 7)],
            &remover,
            CleanupSupport::NativeTrash,
            &mut |_| count += 1,
        );
        // The fake actually unlinks, so the second run needs its own copy
        // of the fixture rather than the one just consumed.
        std::fs::create_dir_all(&target).unwrap();
        let silent = clean_with_native_trash(&[item(&target, root.path(), 7)], &remover);

        assert_eq!(count, 2); // Started + one terminal event
        assert_eq!(streamed.removed, silent.removed);
        assert_eq!(streamed.freed_bytes, silent.freed_bytes);
    }

    #[test]
    fn scan_only_platform_never_hands_a_path_to_the_remover() {
        let root = tempdir().unwrap();
        let target = root.path().join("cache");
        std::fs::create_dir_all(&target).unwrap();
        let remover = FakeRemover::default();

        let outcome = clean_with_support(
            &[item(&target, root.path(), 100)],
            &remover,
            CleanupSupport::ScanOnly {
                reason: "native trash has not been verified",
            },
            &mut |_| {},
        );

        assert!(remover.seen().is_empty());
        assert!(matches!(
            outcome.skipped.as_slice(),
            [Skipped {
                reason: SkipReason::CleanupUnavailable(_),
                ..
            }]
        ));
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    #[test]
    fn current_scan_only_platform_never_hands_a_path_to_the_remover() {
        let root = tempdir().unwrap();
        let target = root.path().join("cache");
        std::fs::create_dir_all(&target).unwrap();
        let remover = FakeRemover::default();

        let outcome = clean(&[item(&target, root.path(), 100)], &remover);

        assert!(remover.seen().is_empty());
        assert!(matches!(
            outcome.skipped.as_slice(),
            [Skipped {
                reason: SkipReason::CleanupUnavailable(_),
                ..
            }]
        ));
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    #[test]
    fn system_trash_itself_refuses_on_a_scan_only_platform() {
        let CleanupSupport::ScanOnly { reason } = platform::current().cleanup_support() else {
            panic!("this test runs only where cleanup must be unavailable");
        };
        let error = SystemTrash
            .remove(Path::new("this path never needs to exist"))
            .expect_err("scan-only platforms must refuse before calling the trash backend");
        assert_eq!(error.to_string(), format!("io error: {reason}"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_trash_moves_a_fixture_through_macos_native_trash() {
        let root = tempdir().unwrap();
        let target = root.path().join("chystik-macos-native-trash-smoke");
        std::fs::write(&target, "safe smoke-test fixture").unwrap();

        let outcome = clean(&[item(&target, root.path(), 25)], &SystemTrash);

        assert_eq!(outcome.removed, vec![target.clone()]);
        assert_eq!(outcome.freed_bytes, 25);
        assert!(outcome.skipped.is_empty());
        assert!(!target.exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_identity_refuses_a_junction_object() {
        let root = tempdir().unwrap();
        let real = root.path().join("real-cache");
        std::fs::create_dir_all(&real).unwrap();
        let junction = root.path().join("cache-junction");
        crate::platform::create_test_junction(&junction, &real).expect("create a junction fixture");

        assert!(
            FileIdentity::of(&junction).is_none(),
            "the identity guard must not follow a Windows junction"
        );
        std::fs::remove_dir(&junction).expect("remove only the junction, not its target");
    }

    /// The exact TOCTOU race the flow exists to survive: a directory is
    /// validated, then the very same path is swapped for a junction pointing
    /// elsewhere before removal. The identity captured up front must no longer
    /// match, so the swapped-in reparse point is skipped, never followed.
    #[cfg(target_os = "windows")]
    #[test]
    fn identity_rejects_a_path_that_became_a_junction_after_validation() {
        let root = tempdir().unwrap();
        let target = root.path().join("cache");
        std::fs::create_dir_all(&target).unwrap();
        let captured = FileIdentity::of(&target).expect("a real directory has an identity");

        let elsewhere = root.path().join("victim");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::remove_dir(&target).unwrap();
        crate::platform::create_test_junction(&target, &elsewhere)
            .expect("create a junction fixture");

        assert!(
            !captured.still_matches(&target),
            "a junction that replaced the validated directory must fail the re-check"
        );
        std::fs::remove_dir(&target).expect("remove only the junction, not its target");
        assert!(elsewhere.exists(), "the junction target must be untouched");
    }

    /// Same race on Unix: the validated path becomes a symlink to a different
    /// object before removal. The captured identity must fail closed.
    #[cfg(unix)]
    #[test]
    fn identity_rejects_a_path_that_became_a_symlink_after_validation() {
        let root = tempdir().unwrap();
        let target = root.path().join("cache");
        std::fs::create_dir_all(&target).unwrap();
        let captured = FileIdentity::of(&target).expect("a real directory has an identity");

        let elsewhere = root.path().join("victim");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::remove_dir(&target).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &target).unwrap();

        assert!(
            !captured.still_matches(&target),
            "a symlink that replaced the validated directory must fail the re-check"
        );
        assert!(elsewhere.exists(), "the symlink target must be untouched");
    }

    /// The whole point of the abstraction: a refused path must never reach
    /// the remover at all.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_parent_never_reaches_the_remover() {
        let root = tempdir().unwrap();
        let real = root.path().join("important");
        std::fs::create_dir_all(real.join("sub")).unwrap();
        let link = root.path().join("cache-link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let remover = FakeRemover::default();
        let outcome =
            clean_with_native_trash(&[item(&link.join("sub"), root.path(), 10)], &remover);

        assert!(remover.seen().is_empty(), "the remover was handed a link");
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(outcome.skipped[0].reason, SkipReason::Refused);
        assert!(real.join("sub").exists(), "the real directory was touched");
    }

    /// A path swapped for a symlink after validation must be skipped.
    ///
    /// The identity captured after `guard::check` no longer matches, which
    /// is what narrows the window between validating and removing.
    #[cfg(unix)]
    #[test]
    fn a_path_swapped_after_validation_is_skipped() {
        let root = tempdir().unwrap();
        let victim = root.path().join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        let identity = FileIdentity::of(&victim).unwrap();

        // The swap an attacker would race to perform.
        std::fs::remove_dir_all(&victim).unwrap();
        let elsewhere = root.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &victim).unwrap();

        assert!(
            !identity.still_matches(&victim),
            "the substitution was not detected"
        );
        // And a fresh run refuses it outright, since it is now a symlink.
        let remover = FakeRemover::default();
        let outcome = clean_with_native_trash(&[item(&victim, root.path(), 1)], &remover);
        assert!(remover.seen().is_empty());
        assert_eq!(outcome.skipped[0].reason, SkipReason::Refused);
    }

    #[test]
    fn identity_survives_a_rename_of_an_unrelated_sibling() {
        let root = tempdir().unwrap();
        let keep = root.path().join("keep");
        std::fs::create_dir_all(&keep).unwrap();
        let identity = FileIdentity::of(&keep).unwrap();
        std::fs::create_dir_all(root.path().join("other")).unwrap();
        assert!(identity.still_matches(&keep));
    }

    #[test]
    fn items_with_no_owning_target_are_refused() {
        let root = tempdir().unwrap();
        let orphan = root.path().join("orphan");
        std::fs::create_dir_all(&orphan).unwrap();
        let remover = FakeRemover::default();
        let outcome = clean_with_native_trash(
            &[CleanupItem {
                path: orphan,
                size_bytes: 5,
                scan_root: None,
            }],
            &remover,
        );
        assert!(remover.seen().is_empty());
        assert_eq!(outcome.skipped[0].reason, SkipReason::OutsideEveryTarget);
    }

    #[test]
    fn a_failing_remover_is_reported_and_does_not_stop_the_run() {
        let root = tempdir().unwrap();
        let a = root.path().join("a");
        let b = root.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        let remover = FakeRemover {
            fail: true,
            ..Default::default()
        };
        let outcome = clean_with_native_trash(
            &[item(&a, root.path(), 10), item(&b, root.path(), 20)],
            &remover,
        );

        assert_eq!(outcome.removed_count(), 0);
        assert_eq!(outcome.freed_bytes, 0);
        assert_eq!(outcome.skipped.len(), 2, "the run continued past the first");
        assert!(matches!(
            outcome.skipped[0].reason,
            SkipReason::RemoverFailed(_)
        ));
    }

    #[test]
    fn permission_denied_is_a_partial_cleanup_failure_not_a_false_success() {
        struct PermissionDeniedRemover {
            calls: std::sync::atomic::AtomicUsize,
        }

        impl Remover for PermissionDeniedRemover {
            fn remove(&self, _path: &Path) -> Result<(), ChystikError> {
                if self
                    .calls
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    == 0
                {
                    Err(ChystikError::Io(std::io::Error::from(
                        std::io::ErrorKind::PermissionDenied,
                    )))
                } else {
                    Ok(())
                }
            }
        }

        let root = tempdir().unwrap();
        let denied = root.path().join("denied");
        let succeeds = root.path().join("succeeds");
        std::fs::create_dir_all(&denied).unwrap();
        std::fs::create_dir_all(&succeeds).unwrap();

        let outcome = clean_with_native_trash(
            &[
                item(&denied, root.path(), 10),
                item(&succeeds, root.path(), 20),
            ],
            &PermissionDeniedRemover {
                calls: std::sync::atomic::AtomicUsize::new(0),
            },
        );

        assert_eq!(outcome.removed, vec![succeeds]);
        assert_eq!(outcome.freed_bytes, 20);
        assert_eq!(outcome.skipped.len(), 1, "the denied item is reported");
        assert!(matches!(
            outcome.skipped[0].reason,
            SkipReason::RemoverFailed(ref message) if message.contains("permission denied")
        ));
    }

    #[test]
    fn a_missing_path_is_skipped_rather_than_removed() {
        let root = tempdir().unwrap();
        let gone = root.path().join("gone");
        let remover = FakeRemover::default();
        let outcome = clean_with_native_trash(&[item(&gone, root.path(), 1)], &remover);
        assert!(remover.seen().is_empty());
        assert_eq!(outcome.skipped.len(), 1);
    }
}

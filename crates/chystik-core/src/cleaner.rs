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
        move_to_trash(path).map_err(|e| ChystikError::Io(std::io::Error::other(e.to_string())))
    }

    fn describe(&self) -> &'static str {
        "trash"
    }
}

fn move_to_trash(path: &Path) -> Result<(), trash::Error> {
    #[cfg(target_os = "macos")]
    {
        // NSFileManager uses the native Trash API without requiring Finder
        // automation permission. It still leaves the item recoverable in
        // Trash, which is the contract exposed by this application.
        use trash::macos::{DeleteMethod, TrashContextExtMacos};

        let mut context = trash::TrashContext::default();
        context.set_delete_method(DeleteMethod::NsFileManager);
        context.delete(path)
    }

    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path)
    }
}

/// Enough of a path's identity to notice it was swapped underneath us.
///
/// Device and inode together identify a filesystem object. A symlink put in
/// place of a validated directory has a different inode, so comparing this
/// immediately before removal catches the substitution.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    device: u64,
    inode: u64,
    is_symlink: bool,
}

/// Non-Unix fallback identity. It is never used to enable cleanup: platforms
/// without a verified native-trash adapter are stopped before this point.
#[cfg(not(unix))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    len: u64,
    modified: Option<std::time::SystemTime>,
    is_dir: bool,
    is_symlink: bool,
}

#[cfg(unix)]
impl FileIdentity {
    /// Read without following symlinks. `None` when the path is gone.
    pub fn of(path: &Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path).ok()?;
        Some(Self {
            device: meta.dev(),
            inode: meta.ino(),
            is_symlink: meta.file_type().is_symlink(),
        })
    }

    /// True when `path` is still the very same object, and still not a link.
    pub fn still_matches(&self, path: &Path) -> bool {
        !self.is_symlink && Self::of(path).is_some_and(|now| now == *self)
    }
}

#[cfg(not(unix))]
impl FileIdentity {
    /// Read without following symlinks. `None` when the path is gone.
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        Some(Self {
            len: meta.len(),
            modified: meta.modified().ok(),
            is_dir: meta.is_dir(),
            is_symlink: meta.file_type().is_symlink(),
        })
    }

    /// This fallback is defense in depth only; cleanup is scan-only here.
    pub fn still_matches(&self, path: &Path) -> bool {
        !self.is_symlink && Self::of(path).is_some_and(|now| now == *self)
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

/// Run the flow over `items`. Never panics and never stops early: one bad
/// item must not prevent the rest from being cleaned.
pub fn clean(items: &[CleanupItem], remover: &dyn Remover) -> CleanupOutcome {
    clean_with_support(items, remover, platform::current().cleanup_support())
}

fn clean_with_support(
    items: &[CleanupItem],
    remover: &dyn Remover,
    support: CleanupSupport,
) -> CleanupOutcome {
    let mut outcome = CleanupOutcome::default();
    if let CleanupSupport::ScanOnly { reason } = support {
        outcome.skipped.extend(items.iter().map(|item| Skipped {
            path: item.path.clone(),
            reason: SkipReason::CleanupUnavailable(reason),
        }));
        return outcome;
    }
    for item in items {
        let path = item.path.clone();

        let Some(root) = item.scan_root.as_deref() else {
            outcome.skipped.push(Skipped {
                path,
                reason: SkipReason::OutsideEveryTarget,
            });
            continue;
        };
        if guard::check(&path, root).is_err() {
            outcome.skipped.push(Skipped {
                path,
                reason: SkipReason::Refused,
            });
            continue;
        }
        // Captured after validation and checked again below, so a swap in
        // between is caught rather than acted on.
        let Some(identity) = FileIdentity::of(&path) else {
            outcome.skipped.push(Skipped {
                path,
                reason: SkipReason::ChangedUnderUs,
            });
            continue;
        };
        if !identity.still_matches(&path) {
            outcome.skipped.push(Skipped {
                path,
                reason: SkipReason::ChangedUnderUs,
            });
            continue;
        }
        match remover.remove(&path) {
            Ok(()) => {
                outcome.freed_bytes += item.size_bytes;
                outcome.removed.push(path);
            }
            Err(e) => outcome.skipped.push(Skipped {
                path,
                reason: SkipReason::RemoverFailed(e.to_string()),
            }),
        }
    }
    outcome
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
        clean_with_support(items, remover, CleanupSupport::NativeTrash)
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

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
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

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
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
    fn a_missing_path_is_skipped_rather_than_removed() {
        let root = tempdir().unwrap();
        let gone = root.path().join("gone");
        let remover = FakeRemover::default();
        let outcome = clean_with_native_trash(&[item(&gone, root.path(), 1)], &remover);
        assert!(remover.seen().is_empty());
        assert_eq!(outcome.skipped.len(), 1);
    }
}

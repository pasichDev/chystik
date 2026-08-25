//! Opt-in integration evidence for the real desktop recovery mechanism.
//!
//! This is ignored by default because it deliberately moves a fixture into the
//! host Trash. CI invokes it against a disposable fixture; the Windows
//! assertion queries the native Recycle Bin item/byte counters rather than
//! merely checking that the source path disappeared.

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
mod native_trash {
    use chystik_core::cleaner::{clean, CleanupItem, SystemTrash};

    #[cfg(target_os = "windows")]
    fn recycle_bin_totals() -> (i64, i64) {
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};

        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: a null root asks Shell for every local Recycle Bin, and
        // `info` is an initialized writable output structure.
        unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }
            .expect("query the native Windows Recycle Bin");
        (info.i64NumItems, info.i64Size)
    }

    #[test]
    #[ignore = "moves a fixture through the host native Trash"]
    fn system_trash_moves_a_fixture_without_direct_deletion() {
        // GitHub's Windows workspace lives on an ephemeral D: volume whose
        // Recycle Bin is disabled. The user profile stays on the system
        // volume, which is the supported native Recycle Bin path and is where
        // Windows users normally keep Chystik's default scan roots.
        #[cfg(target_os = "windows")]
        let fixture_parent = std::env::var_os("USERPROFILE")
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .expect("Windows must provide an absolute USERPROFILE");
        #[cfg(not(target_os = "windows"))]
        let fixture_parent = std::env::current_dir().expect("test must have a working directory");
        let root = tempfile::Builder::new()
            .prefix("chystik-native-trash-")
            .tempdir_in(fixture_parent)
            .expect("create a disposable trash fixture root");
        let target = root.path().join("recoverable-fixture.txt");
        std::fs::write(&target, "safe native Trash smoke-test fixture")
            .expect("write the disposable fixture");
        let target_bytes = std::fs::metadata(&target)
            .expect("stat the disposable fixture")
            .len();
        #[cfg(target_os = "windows")]
        let before_recycle = recycle_bin_totals();

        let outcome = clean(
            &[CleanupItem {
                path: target.clone(),
                size_bytes: target_bytes,
                scan_root: Some(root.path().to_path_buf()),
            }],
            &SystemTrash,
        );

        assert_eq!(
            outcome.removed,
            vec![target.clone()],
            "the native recycle operation was skipped: {:?}",
            outcome.skipped
        );
        assert_eq!(outcome.freed_bytes, target_bytes);
        assert!(outcome.skipped.is_empty());
        assert!(
            !target.exists(),
            "the fixture must leave the source directory through native Trash"
        );

        #[cfg(target_os = "windows")]
        {
            // The hosted Windows image does not enumerate every Shell-folder
            // property through `trash::list`, even though it supports the
            // documented Recycle Bin API. Query the native aggregate instead:
            // a new item and its bytes prove Shell recycled this fixture and
            // did not silently perform a permanent delete.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                let after_recycle = recycle_bin_totals();
                if after_recycle.0 >= before_recycle.0 + 1
                    && after_recycle.1 >= before_recycle.1 + target_bytes as i64
                {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the fixture must increase Windows Recycle Bin items and bytes: before={before_recycle:?}, after={after_recycle:?}"
                );
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

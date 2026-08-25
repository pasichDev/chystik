//! Opt-in integration evidence for the real desktop recovery mechanism.
//!
//! This is ignored by default because it deliberately moves a fixture into the
//! host Trash. CI invokes it with an isolated home where the host supports
//! that; the Windows assertion additionally proves the fixture is visible in
//! the Recycle Bin rather than merely absent from its source path.

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
mod native_trash {
    use chystik_core::cleaner::{clean, CleanupItem, SystemTrash};

    #[test]
    #[ignore = "moves a fixture through the host native Trash"]
    fn system_trash_moves_a_fixture_without_direct_deletion() {
        let root = tempfile::Builder::new()
            .prefix("chystik-native-trash-")
            .tempdir_in(std::env::current_dir().expect("test must have a working directory"))
            .expect("create a disposable trash fixture root");
        let target = root.path().join("recoverable-fixture.txt");
        std::fs::write(&target, "safe native Trash smoke-test fixture")
            .expect("write the disposable fixture");

        let outcome = clean(
            &[CleanupItem {
                path: target.clone(),
                size_bytes: 41,
                scan_root: Some(root.path().to_path_buf()),
            }],
            &SystemTrash,
        );

        assert_eq!(outcome.removed, vec![target.clone()]);
        assert_eq!(outcome.freed_bytes, 41);
        assert!(outcome.skipped.is_empty());
        assert!(
            !target.exists(),
            "the fixture must leave the source directory through native Trash"
        );

        #[cfg(target_os = "windows")]
        {
            let recycle_bin_item = trash::os_limited::list()
                .expect("enumerate the native Windows Recycle Bin")
                .into_iter()
                .find(|item| item.original_path() == target)
                .expect("the fixture must be recoverable from the Windows Recycle Bin");
            trash::os_limited::restore_all([recycle_bin_item])
                .expect("restore the recoverable fixture through the Windows Recycle Bin");
            assert!(
                target.is_file(),
                "the restored fixture must return to its source path"
            );
        }
    }
}

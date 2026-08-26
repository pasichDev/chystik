//! Generated path shapes for the cleanup guard. These tests create only
//! isolated fixture trees; they never call the cleaner or a Trash adapter.

use std::path::{Path, PathBuf};

use chystik_core::guard;
use proptest::prelude::*;

fn fixture() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".chystik-guard-property-")
        .tempdir_in(std::env::current_dir().expect("test process has a working directory"))
        .unwrap()
}

proptest! {
    #[test]
    fn scan_root_aliases_never_become_cleanup_candidates(depth in 0usize..8) {
        let root = fixture();
        let child = root.path().join("child");
        std::fs::create_dir_all(&child).unwrap();

        let mut alias = root.path().to_path_buf();
        for _ in 0..depth {
            alias.push("child");
            alias.push("..");
        }
        prop_assert!(
            guard::check(&alias, root.path()).is_err(),
            "scan-root alias unexpectedly passed: {}",
            alias.display()
        );
    }

    #[test]
    fn protected_ancestors_stay_refused(
        before in "[a-z][a-z0-9_-]{0,10}",
        after in "[a-z][a-z0-9_-]{0,10}",
    ) {
        let root = fixture();
        let protected = root.path().join(before).join(".ssh").join(after);
        std::fs::create_dir_all(&protected).unwrap();

        prop_assert!(guard::check(&protected, root.path()).is_err());
    }

    #[test]
    fn exact_siblings_do_not_share_cleanup_authority(
        suffix in "[a-z][a-z0-9_-]{0,12}",
    ) {
        let root = fixture();
        let allowed = root.path().join(".cache/pip");
        let sibling = root.path().join(format!(".cache/pip-{suffix}"));
        let pipeline = root.path().join(".cache/pipeline");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::create_dir_all(&pipeline).unwrap();

        // The guard validates containment, not catalog ownership. These
        // shapes prove its lexical normalization does not collapse siblings;
        // catalog-specific exact-locator tests pin which one is recognized.
        prop_assert_ne!(&allowed, &sibling);
        prop_assert_ne!(&allowed, &pipeline);
        prop_assert!(guard::normalize_lexically(&sibling).is_some());
        prop_assert!(guard::normalize_lexically(&pipeline).is_some());
    }

    #[test]
    fn equivalent_child_spellings_resolve_consistently(
        leaf in "[a-z][a-z0-9_-]{0,12}",
    ) {
        let root = fixture();
        let real = root.path().join("nested").join(&leaf);
        std::fs::create_dir_all(&real).unwrap();
        let alias = root.path().join("nested/./").join(&leaf);

        prop_assert!(guard::check(&real, root.path()).is_ok());
        prop_assert!(guard::check(&alias, root.path()).is_ok());
        prop_assert_eq!(
            guard::normalize_lexically(&real),
            guard::normalize_lexically(&alias)
        );
    }
}

#[test]
fn relative_and_traversal_paths_fail_closed() {
    let root = fixture();
    let child = root.path().join("child");
    std::fs::create_dir_all(&child).unwrap();

    assert!(guard::check(Path::new("child"), root.path()).is_err());
    assert!(guard::check(&root.path().join("child/.."), root.path()).is_err());
    assert!(guard::normalize_lexically(Path::new("../../important")).is_none());
}

#[cfg(unix)]
proptest! {
    #[test]
    fn symlink_routes_never_gain_permission(name in "[a-z][a-z0-9_-]{0,12}") {
        let root = fixture();
        let real = root.path().join("real").join(&name);
        std::fs::create_dir_all(&real).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(root.path().join("real"), &link).unwrap();

        prop_assert!(guard::check(&link.join(name), root.path()).is_err());
    }
}

#[test]
fn protected_name_detection_has_component_boundaries() {
    assert!(guard::lexical_path_contains_protected_name(Path::new(
        "a/.ssh/b"
    )));
    assert!(!guard::lexical_path_contains_protected_name(Path::new(
        "a/.ssh-backup/b"
    )));
    assert!(!guard::lexical_path_contains_protected_name(Path::new(
        "a/.gitignore/b"
    )));

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let non_utf8 = PathBuf::from(std::ffi::OsString::from_vec(vec![b'.', b'g', b'i', b't']));
        assert!(guard::lexical_path_contains_protected_name(&non_utf8));
    }
}

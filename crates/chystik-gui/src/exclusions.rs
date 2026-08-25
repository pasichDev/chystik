//! Paths the user has marked never-touch.
//!
//! For a tool that deletes, "leave this alone" is not a convenience — it is
//! what makes the bulk actions usable at all. Without it a careful user
//! never presses *Select all safe*, and the whole category workflow is dead
//! weight.
//!
//! Enforced twice, deliberately. The scanner prunes excluded trees so they
//! are never classified or shown, and `filter` runs again on the deletion
//! path in case a finding predates the exclusion.
//!
//! Stored beside the consent record in the platform-owned app config directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub(crate) struct Exclusions {
    pub paths: Vec<PathBuf>,
}

fn exclusions_path() -> PathBuf {
    chystik_core::platform::current()
        .app_paths()
        .config_dir
        .join("exclusions.json")
}

/// Load the list. Any read or parse failure yields an empty list: an
/// unreadable file must not silently un-exclude anything, so the caller is
/// told through the returned `bool`.
pub(crate) fn load() -> (Vec<PathBuf>, bool) {
    let path = exclusions_path();
    match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), true),
        Err(e) => {
            eprintln!("[chystik] cannot read {}: {e}", path.display());
            (Vec::new(), false)
        }
        Ok(text) => match serde_json::from_str::<Exclusions>(&text) {
            Ok(list) => (normalise(list.paths), true),
            Err(e) => {
                eprintln!("[chystik] {} is malformed: {e}", path.display());
                (Vec::new(), false)
            }
        },
    }
}

pub(crate) fn save(paths: &[PathBuf]) {
    let path = exclusions_path();
    let record = Exclusions {
        paths: paths.to_vec(),
    };
    let write = path
        .parent()
        .ok_or_else(|| std::io::Error::other("exclusions path has no parent"))
        .and_then(std::fs::create_dir_all)
        .and_then(|()| {
            let json = serde_json::to_string_pretty(&record)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            std::fs::write(&path, json)
        });
    if let Err(e) = write {
        eprintln!("[chystik] could not save exclusions: {e}");
    }
}

/// Drop duplicates and any path already covered by a shorter one, so the
/// list stays the smallest set that means the same thing.
pub(crate) fn normalise(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths.dedup();
    paths.sort_by_key(|p| p.as_os_str().len());
    let mut kept: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !kept.iter().any(|k| path.starts_with(k)) {
            kept.push(path);
        }
    }
    kept.sort();
    kept
}

/// True if `path` is excluded — itself or anything under an excluded root.
pub(crate) fn is_excluded(path: &Path, exclusions: &[PathBuf]) -> bool {
    exclusions.iter().any(|e| path.starts_with(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_exclusions_collapse_into_their_parent() {
        let list = normalise(vec![
            PathBuf::from("/home/u/repo/app/node_modules"),
            PathBuf::from("/home/u/repo"),
            PathBuf::from("/home/u/repo"),
            PathBuf::from("/home/u/other"),
        ]);
        assert_eq!(
            list,
            vec![
                PathBuf::from("/home/u/other"),
                PathBuf::from("/home/u/repo")
            ]
        );
    }

    #[test]
    fn exclusion_covers_the_whole_subtree() {
        let list = vec![PathBuf::from("/home/u/repo")];
        assert!(is_excluded(Path::new("/home/u/repo"), &list));
        assert!(is_excluded(
            Path::new("/home/u/repo/app/node_modules"),
            &list
        ));
        assert!(!is_excluded(Path::new("/home/u/repository"), &list));
        assert!(!is_excluded(Path::new("/home/u/other"), &list));
    }

    #[test]
    fn an_empty_list_excludes_nothing() {
        assert!(!is_excluded(Path::new("/anything"), &[]));
    }

    #[test]
    fn exclusions_round_trip_through_json() {
        let record = Exclusions {
            paths: vec![PathBuf::from("/home/u/repo")],
        };
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<Exclusions>(&json).unwrap(), record);
    }

    #[test]
    fn exclusions_path_uses_the_core_platform_config_directory() {
        assert_eq!(
            exclusions_path(),
            chystik_core::platform::current()
                .app_paths()
                .config_dir
                .join("exclusions.json")
        );
    }
}

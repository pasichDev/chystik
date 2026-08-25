//! GUI adapter for the shared never-touch policy.
//!
//! Core owns persistence and normalization so the CLI cannot accidentally
//! scan a path that the desktop frontend previously marked as excluded.

use std::path::{Path, PathBuf};

pub(crate) fn load() -> (Vec<PathBuf>, bool) {
    let store = chystik_core::config::ConfigStore::default();
    match store.load() {
        Ok(config) => (config.exclusions, true),
        Err(error) => {
            eprintln!("[chystik] cannot read {}: {error}", store.path().display());
            (Vec::new(), false)
        }
    }
}

pub(crate) fn save(paths: &[PathBuf]) {
    let store = chystik_core::config::ConfigStore::default();
    let write = store.load().and_then(|mut config| {
        config.exclusions = normalise(paths.to_vec());
        store.save(&config)
    });
    if let Err(error) = write {
        eprintln!(
            "[chystik] could not save exclusions at {}: {error}",
            store.path().display()
        );
    }
}

pub(crate) fn normalise(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    chystik_core::config::normalize_exclusions(paths)
}

pub(crate) fn is_excluded(path: &Path, exclusions: &[PathBuf]) -> bool {
    exclusions.iter().any(|root| path.starts_with(root))
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
}

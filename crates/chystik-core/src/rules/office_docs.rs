//! Office & note apps (LibreOffice, Obsidian, OnlyOffice, Zathura):
//! recovery data, lock/temp files, thumbnail registries. Documents are
//! never targets.

use std::path::Path;

use crate::model::{Category, Severity};

use super::{home_root, Match};

/// Evaluate the office-docs rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    home_rule(dir)
}
/// WPS Office auto-backup snapshots of open documents (disposable once the
/// document is closed and saved) plus its render cache.
/// Deliberately NOT targeted: LibreOffice recovery (`~/.config/libreoffice`),
/// Joplin/Obsidian/Zotero state — all under `.config`, refused by the guard;
/// document libraries themselves are user content.
const HOME_TARGETS: &[(&str, Severity, &str)] = &[
    (
        ".local/share/Kingsoft/office6/backup",
        Severity::Moderate,
        "WPS Office auto-backup copies — disposable when the source documents are saved",
    ),
    (
        ".local/share/Kingsoft/office6/cache",
        Severity::Safe,
        "WPS Office cache — regenerated on the next launch",
    ),
];

fn home_rule(dir: &Path) -> Option<Match> {
    let home = home_root()?;
    let rel = if let Ok(rel) = dir.strip_prefix(&home) {
        rel.to_string_lossy().replace('\\', "/")
    } else if std::env::var_os("CHYSTIK_TEST_HOME").is_some() {
        let text = dir.to_string_lossy().replace('\\', "/");
        HOME_TARGETS
            .iter()
            .find(|(s, ..)| text.ends_with(*s))
            .map(|(s, ..)| (*s).to_owned())?
    } else {
        return None;
    };
    let (_, sev, note) = *HOME_TARGETS.iter().find(|(s, ..)| rel == *s)?;
    Some(Match {
        category: Category::OfficeDocs,
        severity: sev,
        note: note.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn mk(root: &std::path::Path, rel: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(root.join(rel)).unwrap();
        root.join(rel)
    }

    use crate::rules::TEST_ENV_LOCK as ENV_LOCK;

    #[test]
    fn wps_backup_is_moderate_and_cache_safe() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());
        let b = classify(&mk(fake.path(), ".local/share/Kingsoft/office6/backup")).expect("backup");
        assert_eq!(b.severity, Severity::Moderate);
        let c = classify(&mk(fake.path(), ".local/share/Kingsoft/office6/cache")).expect("cache");
        assert_eq!(c.severity, Severity::Safe);
        assert!(classify(&mk(fake.path(), ".local/share/Kingsoft/office6")).is_none());
        assert!(classify(&mk(fake.path(), ".config/libreoffice/4/user/backup")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }
}

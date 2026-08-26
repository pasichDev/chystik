//! Game launchers & stores (Steam, Lutris, Heroic, Epic): shader caches,
//! Proton/Wine leftovers. User saves and installed games are never targets.

use std::path::Path;

use crate::model::{Category, Severity};

use super::{home_root, Match};

/// Evaluate the games rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    home_rule(dir)
}

/// Regenerable Steam state (native install under `.local/share/Steam` and
/// the flatpak copy), Lutris/Heroic/legendary caches, and the shared Wine
/// installer cache.
///
/// Deliberately NOT targeted: `steamapps/common` and `steamapps/workshop`
/// (installed games and workshop content), `userdata` (cloud-saved games),
/// `compatdata` (Wine prefixes that frequently hold game saves), Heroic and
/// itch state (lives under `.config`, which the deletion guard refuses).
const HOME_TARGETS: &[(&str, Severity, &str)] = &[
    (
        ".local/share/Steam/steamapps/shadercache",
        Severity::Safe,
        "Steam shader pre-caches — rebuilt automatically when games launch",
    ),
    (
        ".local/share/Steam/htmlcache",
        Severity::Safe,
        "Steam web-browser cache — regenerated while the client runs",
    ),
    (
        ".local/share/Steam/logs",
        Severity::Safe,
        "Steam client logs — recreated during normal use",
    ),
    (
        ".local/share/Steam/dumps",
        Severity::Safe,
        "Steam crash dumps — only useful for debugging past crashes",
    ),
    (
        ".local/share/Steam/depotcache",
        Severity::Moderate,
        "downloaded depot fragments kept for repair — Steam re-downloads anything it needs",
    ),
    (
        ".var/app/com.valvesoftware.Steam/data/Steam/steamapps/shadercache",
        Severity::Safe,
        "flatpak Steam shader pre-caches — rebuilt when games launch",
    ),
    (
        ".var/app/com.valvesoftware.Steam/data/Steam/htmlcache",
        Severity::Safe,
        "flatpak Steam web-browser cache — regenerated while the client runs",
    ),
    // Snap Steam keeps an entirely separate tree under ~/snap. Its
    // `package/` directory alone holds hundreds of megabytes of client
    // update payloads that Steam re-downloads on demand.
    (
        "snap/steam/common/.local/share/Steam/package",
        Severity::Safe,
        "snap Steam client update payloads — re-downloaded when needed",
    ),
    (
        "snap/steam/common/.local/share/Steam/steamapps/shadercache",
        Severity::Safe,
        "snap Steam shader pre-caches — rebuilt automatically when games launch",
    ),
    (
        "snap/steam/common/.local/share/Steam/htmlcache",
        Severity::Safe,
        "snap Steam web-browser cache — regenerated while the client runs",
    ),
    (
        "snap/steam/common/.local/share/Steam/logs",
        Severity::Safe,
        "snap Steam client logs — recreated during normal use",
    ),
    (
        "snap/steam/common/.local/share/Steam/dumps",
        Severity::Safe,
        "snap Steam crash dumps — only useful for debugging past crashes",
    ),
    (
        ".local/share/bottles/temp",
        Severity::Safe,
        "Bottles scratch space — recreated on demand",
    ),
    (
        ".cache/heroic",
        Severity::Safe,
        "Heroic launcher cache — regenerated while the client runs",
    ),
];

const EXTRA_SAFE: &[&str] = &[
    ".var/app/com.valvesoftware.Steam/data/Steam/logs",
    ".var/app/com.valvesoftware.Steam/data/Steam/dumps",
    ".cache/lutris",
    ".cache/legendary",
    ".cache/wine",
];

fn home_rule(dir: &Path) -> Option<Match> {
    let home = home_root()?;
    let rel = dir
        .strip_prefix(&home)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    if EXTRA_SAFE.contains(&rel.as_str()) {
        return Some(Match {
            category: Category::GameLaunchers,
            severity: Severity::Safe,
            note: "game-launcher regenerable state — rebuilt or re-downloaded by the launcher"
                .into(),
        });
    }
    if let Some((_, sev, note)) = HOME_TARGETS.iter().find(|(s, ..)| rel == *s) {
        return Some(Match {
            category: Category::GameLaunchers,
            severity: *sev,
            note: (*note).into(),
        });
    }
    None
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
    fn steam_caches_match_but_game_data_does_not() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());

        let shader = classify(&mk(fake.path(), ".local/share/Steam/steamapps/shadercache"))
            .expect("shadercache");
        assert_eq!(shader.category, Category::GameLaunchers);
        assert_eq!(shader.severity, Severity::Safe);
        for rel in [
            ".local/share/Steam/htmlcache",
            ".local/share/Steam/logs",
            ".local/share/Steam/dumps",
            ".var/app/com.valvesoftware.Steam/data/Steam/htmlcache",
            ".var/app/com.valvesoftware.Steam/data/Steam/logs",
            ".cache/lutris",
            ".cache/legendary",
            ".cache/wine",
        ] {
            assert!(classify(&mk(fake.path(), rel)).is_some(), "{rel}");
        }
        let depot = classify(&mk(fake.path(), ".local/share/Steam/depotcache")).expect("depot");
        assert_eq!(depot.severity, Severity::Moderate);

        // Game data and user saves are never targets.
        for rel in [
            ".local/share/Steam",
            ".local/share/Steam/steamapps",
            ".local/share/Steam/steamapps/common",
            ".local/share/Steam/userdata",
            ".local/share/Steam/config",
        ] {
            assert!(classify(&mk(fake.path(), rel)).is_none(), "{rel}");
        }
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn correctly_named_tree_outside_home_does_not_match() {
        let root = tempdir().unwrap();
        assert!(classify(&mk(root.path(), "games/.cache/lutris")).is_none());
    }
}

//! Messengers & social clients (Telegram, Discord, Slack, Element):
//! media/file caches only; chat history and account data stay untouched.
//!
//! Owner: child agent `rules-comms` on branch `v02/rules-comms`.

use std::path::Path;

use crate::model::{Category, Severity};

use super::{home_root, Match};

/// Evaluate the messengers rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    home_rule(dir)
}

/// Thunderbird chat/mail caches. Deliberately NOT targeted (guard reality):
/// Discord/Slack/Signal/Element/Teams Electron caches live under `.config`
/// which guard::check refuses; TelegramDesktop `tdata` holds session keys.
const HOME_TARGETS: &[&str] = &[".cache/thunderbird"];

fn home_rule(dir: &Path) -> Option<Match> {
    let home = home_root()?;
    let rel = dir
        .strip_prefix(&home)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    if !HOME_TARGETS.contains(&rel.as_str()) {
        return None;
    }
    Some(Match {
        category: Category::Messengers,
        severity: Severity::Safe,
        note: "Thunderbird cache — regenerated on next start; mail profiles live elsewhere".into(),
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
    fn thunderbird_cache_matches_but_profiles_do_not() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());
        let m = classify(&mk(fake.path(), ".cache/thunderbird")).expect("tb cache");
        assert_eq!(m.category, Category::Messengers);
        assert_eq!(m.severity, Severity::Safe);
        assert!(classify(&mk(fake.path(), "thunderbird")).is_none());
        assert!(classify(&mk(fake.path(), ".cache/thunderbird-extra")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }
}

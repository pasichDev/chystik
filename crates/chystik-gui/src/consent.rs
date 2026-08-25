//! GUI adapter for the shared persisted safety acknowledgement.
//!
//! The CLI and GUI intentionally read one versioned policy record. The core
//! store imports the former GUI-only `consent.json` on first use, so existing
//! users are not asked twice just because they switch frontend.

pub(crate) fn is_acknowledged() -> bool {
    chystik_core::config::ConfigStore::default()
        .load()
        .map(|config| config.acknowledges_current_version())
        .unwrap_or(false)
}

pub(crate) fn acknowledge() {
    let store = chystik_core::config::ConfigStore::default();
    if let Err(error) = store.acknowledge_current_version() {
        eprintln!(
            "[chystik] could not record consent at {}: {error}",
            store.path().display()
        );
    }
}

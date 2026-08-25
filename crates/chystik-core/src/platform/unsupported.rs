use std::fs::Metadata;
use std::path::{Path, PathBuf};

use super::{
    app_dir_or_current, Adapter, AppPaths, CleanupSupport, PlatformKind, StorageStats,
    StorageVolume,
};

pub(super) static ADAPTER: Unsupported = Unsupported;

pub(super) struct Unsupported;

impl Adapter for Unsupported {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Unsupported
    }

    fn app_paths(&self) -> AppPaths {
        let home_dir = super::home_dir_or_current();
        let base = Some(home_dir.join(".config"));
        AppPaths {
            home_dir,
            config_dir: app_dir_or_current(base.clone(), &["chystik"]),
            cache_dir: app_dir_or_current(base, &["chystik", "cache"]),
        }
    }

    fn storage_volumes(&self) -> Vec<StorageVolume> {
        Vec::new()
    }

    fn unscannable_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn default_skip_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn storage_stats(&self, _path: &Path) -> Option<StorageStats> {
        None
    }

    fn allocated_bytes(&self, metadata: &Metadata) -> u64 {
        metadata.len()
    }

    fn is_protected_system_path(&self, _path: &Path) -> bool {
        false
    }

    fn cleanup_support(&self) -> CleanupSupport {
        CleanupSupport::ScanOnly {
            reason: "this platform has no verified native-trash adapter",
        }
    }
}

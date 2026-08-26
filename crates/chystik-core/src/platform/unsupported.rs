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

    fn privacy_roots(&self) -> super::PrivacyRoots {
        super::PrivacyRoots {
            home_dir: super::privacy_home_dir_or_current(),
            roaming_dir: None,
            local_dir: None,
        }
    }

    fn rule_roots(&self) -> super::RuleRoots {
        let home_dir = super::privacy_home_dir_or_current();
        super::RuleRoots {
            cache_dir: home_dir.join(".cache"),
            home_dir,
            local_app_data_dir: None,
            library_caches_dir: None,
            developer_dir: None,
            volume_root: None,
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

    fn native_trash_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn storage_stats(&self, _path: &Path) -> Option<StorageStats> {
        None
    }

    fn allocated_bytes(&self, metadata: &Metadata) -> u64 {
        metadata.len()
    }

    fn path_identity(&self, path: &Path) -> Option<super::PathIdentity> {
        super::portable_path_identity(path)
    }

    fn is_protected_system_path(&self, _path: &Path) -> bool {
        false
    }

    fn is_link_or_reparse_point(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(true)
    }

    fn cleanup_support(&self) -> CleanupSupport {
        CleanupSupport::ScanOnly {
            reason: "this platform has no verified native-trash adapter",
        }
    }
}

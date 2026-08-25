use std::fs::Metadata;
use std::path::{Path, PathBuf};

use super::{
    app_dir_or_current, Adapter, AppPaths, CleanupSupport, PlatformKind, StorageStats,
    StorageVolume,
};

pub(super) static ADAPTER: MacOS = MacOS;

pub(super) struct MacOS;

impl Adapter for MacOS {
    fn kind(&self) -> PlatformKind {
        PlatformKind::MacOS
    }

    fn app_paths(&self) -> AppPaths {
        let home_dir = super::home_dir_or_current();
        let application_support = Some(home_dir.join("Library/Application Support"));
        let caches = Some(home_dir.join("Library/Caches"));
        AppPaths {
            home_dir,
            config_dir: app_dir_or_current(application_support, &["Chystik"]),
            cache_dir: app_dir_or_current(caches, &["Chystik"]),
        }
    }

    fn storage_volumes(&self) -> Vec<StorageVolume> {
        let mut roots = vec![PathBuf::from("/")];
        if let Ok(entries) = std::fs::read_dir("/Volumes") {
            roots.extend(
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir()),
            );
        }
        roots
            .into_iter()
            .filter_map(|mount_point| {
                self.storage_stats(&mount_point).map(|stats| StorageVolume {
                    source: mount_point.display().to_string(),
                    mount_point,
                    fs_type: "macos".into(),
                    total_bytes: stats.total_bytes,
                    free_bytes: stats.free_bytes,
                })
            })
            .collect()
    }

    fn unscannable_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn default_skip_roots(&self) -> Vec<PathBuf> {
        [
            "/System",
            "/Library",
            "/Applications",
            "/private",
            "/usr",
            "/bin",
            "/sbin",
            "/etc",
            "/var",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }

    fn storage_stats(&self, path: &Path) -> Option<StorageStats> {
        unix_storage_stats(path)
    }

    fn allocated_bytes(&self, metadata: &Metadata) -> u64 {
        use std::os::unix::fs::MetadataExt;
        metadata.blocks().saturating_mul(512)
    }

    fn is_protected_system_path(&self, path: &Path) -> bool {
        path == Path::new("/")
            || [
                "/System",
                "/Library",
                "/Applications",
                "/private",
                "/usr",
                "/bin",
                "/sbin",
                "/etc",
                "/var",
            ]
            .iter()
            .map(Path::new)
            .any(|root| super::is_under(path, root, false))
    }

    fn cleanup_support(&self) -> CleanupSupport {
        CleanupSupport::ScanOnly {
            reason: "macOS cleanup is disabled until native Trash and link-safety integration tests run on macOS",
        }
    }
}

fn unix_storage_stats(path: &Path) -> Option<StorageStats> {
    use std::os::unix::ffi::OsStrExt;
    let raw_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: raw_path is NUL-terminated and stat is a valid output buffer.
    let result = unsafe { libc::statvfs(raw_path.as_ptr(), &mut stat) };
    if result != 0 {
        return None;
    }
    let fragment = (stat.f_frsize as u64).max(1);
    Some(StorageStats {
        total_bytes: (stat.f_blocks as u64).saturating_mul(fragment),
        free_bytes: (stat.f_bavail as u64).saturating_mul(fragment),
    })
}

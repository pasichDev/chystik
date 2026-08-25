use std::fs::Metadata;
use std::path::{Path, PathBuf};

use super::{
    app_dir_or_current, env_absolute, is_under, Adapter, AppPaths, CleanupSupport, PlatformKind,
    StorageStats, StorageVolume,
};

pub(super) static ADAPTER: Windows = Windows;

pub(super) struct Windows;

impl Adapter for Windows {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Windows
    }

    fn app_paths(&self) -> AppPaths {
        let home_dir = super::home_dir_or_current();
        let roaming = env_absolute("APPDATA").or_else(|| Some(home_dir.join("AppData/Roaming")));
        let local = env_absolute("LOCALAPPDATA").or_else(|| Some(home_dir.join("AppData/Local")));
        AppPaths {
            home_dir,
            config_dir: app_dir_or_current(roaming, &["Chystik"]),
            cache_dir: app_dir_or_current(local, &["Chystik", "Cache"]),
        }
    }

    fn storage_volumes(&self) -> Vec<StorageVolume> {
        logical_drives()
            .into_iter()
            .filter_map(|mount_point| {
                self.storage_stats(&mount_point).map(|stats| StorageVolume {
                    source: mount_point.display().to_string(),
                    mount_point,
                    fs_type: "windows".into(),
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
        protected_roots()
    }

    fn storage_stats(&self, path: &Path) -> Option<StorageStats> {
        storage_stats(path)
    }

    fn allocated_bytes(&self, metadata: &Metadata) -> u64 {
        // Windows does not expose Unix `st_blocks`; logical length is an
        // honest portable fallback until an allocated-cluster adapter lands.
        metadata.len()
    }

    fn is_protected_system_path(&self, path: &Path) -> bool {
        protected_roots()
            .iter()
            .any(|root| is_under(path, root, true))
    }

    fn cleanup_support(&self) -> CleanupSupport {
        CleanupSupport::ScanOnly {
            reason: "Windows cleanup is disabled until Recycle Bin and reparse-point integration tests run on Windows",
        }
    }
}

fn protected_roots() -> Vec<PathBuf> {
    let system_drive = std::env::var_os("SystemDrive")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:"));
    let mut roots = vec![
        env_absolute("SystemRoot").unwrap_or_else(|| system_drive.join("Windows")),
        env_absolute("ProgramFiles").unwrap_or_else(|| system_drive.join("Program Files")),
        env_absolute("ProgramData").unwrap_or_else(|| system_drive.join("ProgramData")),
    ];
    if let Some(program_files_x86) = env_absolute("ProgramFiles(x86)") {
        roots.push(program_files_x86);
    }
    roots
}

fn logical_drives() -> Vec<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDriveStringsW};

    let mut buffer = vec![0u16; 512];
    // SAFETY: buffer is writable and its size is supplied in UTF-16 code units.
    let len = unsafe { GetLogicalDriveStringsW(buffer.len() as u32, buffer.as_mut_ptr()) };
    if len == 0 || len as usize >= buffer.len() {
        return Vec::new();
    }
    buffer[..len as usize]
        .split(|unit| *unit == 0)
        .filter(|drive| !drive.is_empty())
        .filter_map(|drive| {
            let path = PathBuf::from(std::ffi::OsString::from_wide(drive));
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: wide is NUL-terminated for the duration of the call.
            let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
            is_user_visible_drive(kind).then_some(path)
        })
        .collect()
}

fn is_user_visible_drive(kind: u32) -> bool {
    use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};

    matches!(kind, DRIVE_FIXED | DRIVE_REMOVABLE)
}

fn storage_stats(path: &Path) -> Option<StorageStats> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0u64;
    let mut total = 0u64;
    let mut free = 0u64;
    // SAFETY: wide is NUL-terminated; all out-pointers are valid local buffers.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
    (ok != 0 && total > 0).then_some(StorageStats {
        total_bytes: total,
        free_bytes: free,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};

    #[test]
    fn includes_fixed_and_removable_drives_only() {
        assert!(is_user_visible_drive(DRIVE_FIXED));
        assert!(is_user_visible_drive(DRIVE_REMOVABLE));
        assert!(!is_user_visible_drive(4)); // DRIVE_REMOTE
    }
}

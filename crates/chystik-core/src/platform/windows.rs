use std::fs::Metadata;
use std::path::{Path, PathBuf};

use super::{
    app_dir_or_current, env_absolute, is_under, Adapter, AppPaths, CleanupSupport, PlatformKind,
    StorageStats, StorageVolume,
};

pub(super) static ADAPTER: Windows = Windows;

pub(super) struct Windows;

/// Recycle a fully qualified path with the Windows flag that forbids a
/// permanent-delete fallback. `FOF_ALLOWUNDO` alone is only best-effort;
/// `FOFX_RECYCLEONDELETE` is the explicit Windows 8+ contract.
pub(super) fn recycle_to_bin(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName, FOFX_EARLYFAILURE,
        FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO, FOF_NO_UI,
    };

    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("resolve Windows recycle path {}: {error}", path.display()))?;
    let wide: Vec<u16> = canonical.as_os_str().encode_wide().chain(Some(0)).collect();

    // COM may already be initialized in a different apartment by the GUI.
    // That is safe to use; only balance CoUninitialize when this call itself
    // succeeded (including S_FALSE for an already-compatible apartment).
    let initialize = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let must_uninitialize = initialize.is_ok();
    if initialize.is_err() && initialize != RPC_E_CHANGED_MODE {
        return Err(format!("initialize Windows Shell COM: {initialize:?}"));
    }

    let result = (|| -> Result<(), String> {
        let operation: IFileOperation =
            unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }
                .map_err(|error| format!("create Windows Shell operation: {error}"))?;
        // Retain the Shell's normal undo/recycle base mode, then require the
        // stronger Windows 8+ flag that disallows a permanent-delete fallback.
        unsafe {
            operation.SetOperationFlags(
                FOF_NO_UI | FOF_ALLOWUNDO | FOFX_EARLYFAILURE | FOFX_RECYCLEONDELETE,
            )
        }
        .map_err(|error| format!("configure Windows Recycle Bin operation: {error}"))?;
        let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }
            .map_err(|error| format!("open Windows Shell item for Recycle Bin: {error}"))?;
        unsafe { operation.DeleteItem(&item, None) }
            .map_err(|error| format!("stage item for Windows Recycle Bin: {error}"))?;
        unsafe { operation.PerformOperations() }
            .map_err(|error| format!("perform Windows Recycle Bin operation: {error}"))?;
        if unsafe { operation.GetAnyOperationsAborted() }
            .map_err(|error| format!("inspect Windows Recycle Bin operation: {error}"))?
            .as_bool()
        {
            return Err("Windows Shell aborted the Recycle Bin operation".into());
        }
        Ok(())
    })();
    if must_uninitialize {
        unsafe { CoUninitialize() };
    }
    result.map_err(|error| format!("move to Windows Recycle Bin: {error}"))
}

impl Adapter for Windows {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Windows
    }

    fn app_paths(&self) -> AppPaths {
        let home_dir = super::privacy_home_dir_or_current();
        let roaming = env_absolute("APPDATA").or_else(|| Some(home_dir.join("AppData/Roaming")));
        let local = env_absolute("LOCALAPPDATA").or_else(|| Some(home_dir.join("AppData/Local")));
        AppPaths {
            home_dir,
            config_dir: app_dir_or_current(roaming, &["Chystik"]),
            cache_dir: app_dir_or_current(local, &["Chystik", "Cache"]),
        }
    }

    fn privacy_roots(&self) -> super::PrivacyRoots {
        let home_dir = super::home_dir_or_current();
        super::PrivacyRoots {
            home_dir: home_dir.clone(),
            roaming_dir: env_absolute("APPDATA").or_else(|| Some(home_dir.join("AppData/Roaming"))),
            local_dir: env_absolute("LOCALAPPDATA")
                .or_else(|| Some(home_dir.join("AppData/Local"))),
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
        let mut roots = system_roots().to_vec();
        for drive in logical_drives() {
            roots.push(drive.join("$Recycle.Bin"));
            roots.push(drive.join("System Volume Information"));
        }
        roots
    }

    fn storage_stats(&self, path: &Path) -> Option<StorageStats> {
        storage_stats(path)
    }

    fn allocated_bytes(&self, metadata: &Metadata) -> u64 {
        // Windows does not expose Unix `st_blocks`; logical length is an
        // honest portable fallback until an allocated-cluster adapter lands.
        metadata.len()
    }

    fn path_identity(&self, path: &Path) -> Option<super::PathIdentity> {
        file_identity(path)
    }

    fn is_protected_system_path(&self, path: &Path) -> bool {
        system_roots().iter().any(|root| is_under(path, root, true))
            || has_protected_volume_component(path)
    }

    fn is_link_or_reparse_point(&self, path: &Path) -> bool {
        is_reparse_point(path)
    }

    fn cleanup_support(&self) -> CleanupSupport {
        CleanupSupport::NativeTrash
    }
}

fn system_roots() -> &'static [PathBuf] {
    static ROOTS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    ROOTS.get_or_init(|| system_roots_uncached())
}

fn system_roots_uncached() -> Vec<PathBuf> {
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

/// These are per-volume system directories on every Windows drive. Checking
/// the component instead of re-enumerating drives keeps the deletion guard's
/// hot path constant-time even during a million-entry scan.
fn has_protected_volume_component(path: &Path) -> bool {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .any(|component| {
            component.eq_ignore_ascii_case("$Recycle.Bin")
                || component.eq_ignore_ascii_case("System Volume Information")
        })
}

/// Win32 marks symlinks, junctions, mount points and cloud placeholders as
/// reparse points. We refuse every such indirection rather than trying to
/// classify tags that can redirect a traversal outside the approved root.
fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated and survives the Win32 call.
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    attributes == INVALID_FILE_ATTRIBUTES || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Open the object itself (not a junction/symlink target) and read the NTFS
/// volume serial + file index pair. `FILE_FLAG_OPEN_REPARSE_POINT` makes a
/// reparse-point swap fail closed even if it happens between the guard's
/// attribute check and this identity capture.
fn file_identity(path: &Path) -> Option<super::PathIdentity> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: path is NUL-terminated, no security attributes are supplied,
    // and the flags open the directory/file object without following a
    // reparse point.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is valid and `info` is a writable output buffer.
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
    // SAFETY: `handle` is owned by this function and is closed exactly once.
    unsafe { CloseHandle(handle) };
    if ok == 0 || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return None;
    }
    Some(super::PathIdentity::new(
        info.dwVolumeSerialNumber as u64,
        ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    ))
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

    #[test]
    fn protects_recycle_and_volume_metadata_on_every_drive() {
        assert!(has_protected_volume_component(Path::new(
            "C:\\$Recycle.Bin\\S-1-5-21"
        )));
        assert!(has_protected_volume_component(Path::new(
            "D:\\System Volume Information\\IndexerVolumeGuid"
        )));
        assert!(!has_protected_volume_component(Path::new(
            "C:\\Users\\chystik\\cache"
        )));
    }

    #[test]
    fn storage_volumes_report_a_real_local_drive_with_sane_capacity() {
        let volumes = ADAPTER.storage_volumes();
        assert!(
            !volumes.is_empty(),
            "Windows must expose its fixed system volume to the Disks view"
        );
        for volume in volumes {
            assert!(volume.mount_point.is_absolute());
            assert!(volume.total_bytes > 0);
            assert!(volume.free_bytes <= volume.total_bytes);
            assert_eq!(volume.fs_type, "windows");
        }
    }

    #[test]
    fn privacy_roots_are_absolute_and_keep_roaming_separate_from_local() {
        let roots = ADAPTER.privacy_roots();
        assert!(roots.home_dir.is_absolute());
        assert!(roots
            .roaming_dir
            .as_ref()
            .is_some_and(|path| path.is_absolute()));
        assert!(roots
            .local_dir
            .as_ref()
            .is_some_and(|path| path.is_absolute()));
        assert_ne!(roots.roaming_dir, roots.local_dir);
    }
}

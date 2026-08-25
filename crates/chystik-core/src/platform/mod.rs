//! The host seam for filesystem behavior.
//!
//! Scanner, guard, cleaner and frontends use [`Platform`] rather than asking
//! Linux, macOS or Windows questions themselves. This keeps the rule engine
//! and its future CLI/classifier consumers deterministic and portable while
//! making the safety-sensitive parts vary in one place.

use std::fs::Metadata;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

/// The host family Chystik is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformKind {
    Linux,
    MacOS,
    Windows,
    Unsupported,
}

/// Whether Chystik can prove that cleanup reaches a native recovery mechanism.
///
/// There is intentionally no direct-delete variant. A platform must opt in to
/// native-trash cleanup only after its link/reparse-point safety contract has
/// a real integration test; otherwise it remains a useful scan-only app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupSupport {
    NativeTrash,
    ScanOnly { reason: &'static str },
}

impl CleanupSupport {
    pub fn is_available(self) -> bool {
        matches!(self, Self::NativeTrash)
    }

    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::NativeTrash => None,
            Self::ScanOnly { reason } => Some(reason),
        }
    }
}

/// Directories Chystik owns for its own persistent state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub home_dir: PathBuf,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
}

/// A mounted user-visible volume.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageVolume {
    pub source: String,
    pub mount_point: PathBuf,
    pub fs_type: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// Capacity for an arbitrary path, when the host can read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageStats {
    pub total_bytes: u64,
    pub free_bytes: u64,
}

trait Adapter: Send + Sync {
    fn kind(&self) -> PlatformKind;
    fn app_paths(&self) -> AppPaths;
    fn storage_volumes(&self) -> Vec<StorageVolume>;
    fn unscannable_roots(&self) -> Vec<PathBuf>;
    fn default_skip_roots(&self) -> Vec<PathBuf>;
    fn storage_stats(&self, path: &Path) -> Option<StorageStats>;
    fn allocated_bytes(&self, metadata: &Metadata) -> u64;
    fn is_protected_system_path(&self, path: &Path) -> bool;
    fn cleanup_support(&self) -> CleanupSupport;
}

/// A small public interface over the target-selected platform adapter.
///
/// The adapter itself is private: callers learn one interface, while the
/// implementation can use `/proc`, APFS conventions, Win32 drive APIs or a
/// test double without leaking those choices into scanner/GUI/CLI code.
#[derive(Clone, Copy)]
pub struct Platform {
    adapter: &'static dyn Adapter,
}

impl Platform {
    pub fn kind(self) -> PlatformKind {
        self.adapter.kind()
    }

    pub fn app_paths(self) -> AppPaths {
        self.adapter.app_paths()
    }

    pub fn storage_volumes(self) -> Vec<StorageVolume> {
        self.adapter.storage_volumes()
    }

    pub fn unscannable_roots(self) -> Vec<PathBuf> {
        self.adapter.unscannable_roots()
    }

    pub fn default_skip_roots(self) -> Vec<PathBuf> {
        self.adapter.default_skip_roots()
    }

    pub fn storage_stats(self, path: &Path) -> Option<StorageStats> {
        self.adapter.storage_stats(path)
    }

    /// Allocated bytes when the host exposes them, otherwise logical bytes.
    pub fn allocated_bytes(self, metadata: &Metadata) -> u64 {
        self.adapter.allocated_bytes(metadata)
    }

    pub fn is_protected_system_path(self, path: &Path) -> bool {
        self.adapter.is_protected_system_path(path)
    }

    pub fn cleanup_support(self) -> CleanupSupport {
        self.adapter.cleanup_support()
    }
}

/// Return the platform selected at compile time for this binary.
pub fn current() -> Platform {
    Platform {
        adapter: current_adapter(),
    }
}

#[cfg(target_os = "linux")]
fn current_adapter() -> &'static dyn Adapter {
    &linux::ADAPTER
}

#[cfg(target_os = "macos")]
fn current_adapter() -> &'static dyn Adapter {
    &macos::ADAPTER
}

#[cfg(target_os = "windows")]
fn current_adapter() -> &'static dyn Adapter {
    &windows::ADAPTER
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn current_adapter() -> &'static dyn Adapter {
    &unsupported::ADAPTER
}

/// Longest mount point containing `path`.
pub fn mount_of(path: &Path, mounts: &[StorageVolume]) -> Option<String> {
    mounts
        .iter()
        .filter(|mount| path.starts_with(&mount.mount_point))
        .max_by_key(|mount| mount.mount_point.as_os_str().len())
        .map(|mount| mount.mount_point.to_string_lossy().into_owned())
}

fn env_absolute(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn home_dir() -> Option<PathBuf> {
    env_absolute("HOME").or_else(|| env_absolute("USERPROFILE"))
}

fn home_dir_or_current() -> PathBuf {
    home_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn app_dir_or_current(base: Option<PathBuf>, suffix: &[&str]) -> PathBuf {
    let mut path = base
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    for segment in suffix {
        path.push(segment);
    }
    path
}

fn is_under(path: &Path, root: &Path, case_insensitive: bool) -> bool {
    if !case_insensitive {
        return path.starts_with(root);
    }
    let path = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let root = root
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    path == root || path.starts_with(&(root + "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_platform_has_an_absolute_config_directory() {
        let paths = current().app_paths();
        assert!(paths.home_dir.is_absolute());
        assert!(paths.config_dir.is_absolute());
    }

    #[test]
    fn current_platform_advertises_only_native_trash_or_scan_only() {
        assert!(matches!(
            current().cleanup_support(),
            CleanupSupport::NativeTrash | CleanupSupport::ScanOnly { .. }
        ));
    }

    #[test]
    fn cleanup_support_exposes_a_reason_only_when_cleanup_is_unavailable() {
        assert_eq!(CleanupSupport::NativeTrash.reason(), None);
        assert_eq!(
            CleanupSupport::ScanOnly {
                reason: "native trash needs verification",
            }
            .reason(),
            Some("native trash needs verification")
        );
    }

    #[test]
    fn mount_lookup_prefers_the_deepest_path_prefix() {
        let volumes = vec![
            StorageVolume {
                source: "root".into(),
                mount_point: PathBuf::from("/"),
                fs_type: "test".into(),
                total_bytes: 1,
                free_bytes: 1,
            },
            StorageVolume {
                source: "home".into(),
                mount_point: PathBuf::from("/home"),
                fs_type: "test".into(),
                total_bytes: 1,
                free_bytes: 1,
            },
        ];
        assert_eq!(
            mount_of(Path::new("/home/u/cache"), &volumes).as_deref(),
            Some("/home")
        );
    }

    #[test]
    fn mount_lookup_never_confuses_a_sibling_prefix_for_a_mount() {
        let volumes = vec![StorageVolume {
            source: "home".into(),
            mount_point: PathBuf::from("/home"),
            fs_type: "test".into(),
            total_bytes: 1,
            free_bytes: 1,
        }];

        assert_eq!(mount_of(Path::new("/homebrew/cache"), &volumes), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_policy_protects_system_paths_and_keeps_cleanup_native() {
        let host = current();
        assert_eq!(host.kind(), PlatformKind::Linux);
        assert_eq!(host.cleanup_support(), CleanupSupport::NativeTrash);
        assert!(host.is_protected_system_path(Path::new("/var/lib/private")));
        assert!(!host.is_protected_system_path(Path::new("/tmp/chystik-fixture")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_policy_uses_native_trash_and_protects_system_paths() {
        let host = current();
        assert_eq!(host.kind(), PlatformKind::MacOS);
        assert_eq!(host.cleanup_support(), CleanupSupport::NativeTrash);
        assert!(host.is_protected_system_path(Path::new("/System/Library")));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_policy_is_scan_only_and_protects_children_of_every_system_root() {
        let host = current();
        assert_eq!(host.kind(), PlatformKind::Windows);
        assert!(matches!(
            host.cleanup_support(),
            CleanupSupport::ScanOnly { .. }
        ));
        for root in host.default_skip_roots() {
            assert!(
                host.is_protected_system_path(&root.join("chystik-test-child")),
                "{} must protect descendants",
                root.display()
            );
        }
    }

    #[test]
    fn case_insensitive_path_policy_still_respects_component_boundaries() {
        assert!(is_under(
            Path::new("C:\\Program Files\\App"),
            Path::new("c:\\program files"),
            true,
        ));
        assert!(!is_under(
            Path::new("C:\\Program Files Old\\App"),
            Path::new("c:\\program files"),
            true,
        ));
    }
}

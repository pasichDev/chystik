use std::fs::Metadata;
use std::path::{Path, PathBuf};

use super::{
    app_dir_or_current, env_absolute, Adapter, AppPaths, CleanupSupport, PlatformKind,
    StorageStats, StorageVolume,
};

pub(super) static ADAPTER: Linux = Linux;

pub(super) struct Linux;

impl Adapter for Linux {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Linux
    }

    fn app_paths(&self) -> AppPaths {
        let home_dir = super::home_dir_or_current();
        let config_base =
            env_absolute("XDG_CONFIG_HOME").or_else(|| Some(home_dir.join(".config")));
        let cache_base = env_absolute("XDG_CACHE_HOME").or_else(|| Some(home_dir.join(".cache")));
        AppPaths {
            home_dir,
            config_dir: app_dir_or_current(config_base, &["chystik"]),
            cache_dir: app_dir_or_current(cache_base, &["chystik"]),
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
        let cache_dir = env_absolute("XDG_CACHE_HOME").unwrap_or_else(|| home_dir.join(".cache"));
        super::RuleRoots {
            home_dir,
            cache_dir,
            local_app_data_dir: None,
            library_caches_dir: None,
            developer_dir: None,
            volume_root: None,
        }
    }

    fn storage_volumes(&self) -> Vec<StorageVolume> {
        mount_table()
    }

    fn unscannable_roots(&self) -> Vec<PathBuf> {
        unscannable_mount_points()
    }

    fn default_skip_roots(&self) -> Vec<PathBuf> {
        ["/proc", "/sys", "/dev", "/run", "/usr", "/var", "/opt"]
            .into_iter()
            .map(PathBuf::from)
            .collect()
    }

    fn native_trash_roots(&self) -> Vec<PathBuf> {
        // Freedesktop Trash specification: the home trash belongs below the
        // user's XDG data directory. Mounted volumes use either `.Trash/$uid`
        // or `.Trash-$uid`, depending on whether a shared `.Trash` directory
        // is valid on that filesystem. Both forms are exact locations, not
        // broad filename matches.
        let home_dir = super::home_dir_or_current();
        let data_dir =
            env_absolute("XDG_DATA_HOME").unwrap_or_else(|| home_dir.join(".local/share"));
        let uid = unsafe { libc::geteuid() };
        let mut roots = vec![data_dir.join("Trash")];
        for volume in mount_table() {
            roots.push(volume.mount_point.join(".Trash").join(uid.to_string()));
            roots.push(volume.mount_point.join(format!(".Trash-{uid}")));
        }
        roots.sort();
        roots.dedup();
        roots
    }

    fn storage_stats(&self, path: &Path) -> Option<StorageStats> {
        let (total_bytes, free_bytes) = statvfs_bytes(path);
        (total_bytes > 0).then_some(StorageStats {
            total_bytes,
            free_bytes,
        })
    }

    fn allocated_bytes(&self, metadata: &Metadata) -> u64 {
        use std::os::unix::fs::MetadataExt;
        metadata.blocks().saturating_mul(512)
    }

    fn path_identity(&self, path: &Path) -> Option<super::PathIdentity> {
        super::unix_path_identity(path)
    }

    fn is_protected_system_path(&self, path: &Path) -> bool {
        path == Path::new("/")
            || [
                "/boot", "/etc", "/usr", "/var", "/opt", "/proc", "/sys", "/dev",
            ]
            .iter()
            .map(Path::new)
            .any(|root| super::is_under(path, root, false))
    }

    fn is_link_or_reparse_point(&self, path: &Path) -> bool {
        super::unix_is_link(path)
    }

    fn cleanup_support(&self) -> CleanupSupport {
        CleanupSupport::NativeTrash
    }
}

/// Filesystem types that are not real data volumes.
const PSEUDO_FS: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "tmpfs",
    "ramfs",
    "mqueue",
    "cgroup",
    "cgroup2",
    "bpf",
    "debugfs",
    "tracefs",
    "configfs",
    "fusectl",
    "securityfs",
    "pstore",
    "efivarfs",
    "autofs",
    "hugetlbfs",
    "overlay",
    "squashfs",
    "nsfs",
    "binfmt_misc",
    "rpc_pipefs",
    "selinuxfs",
    "systemd-1",
];

const NETWORK_FS: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "afs",
    "ceph",
    "glusterfs",
    "fuse.sshfs",
    "fuse.rclone",
    "fuse.s3fs",
    "fuse.gvfsd-fuse",
    "fuse.davfs",
    "davfs",
    "ftpfs",
    "9p",
    "ncpfs",
    "coda",
];

const RAM_FS: &[&str] = &["tmpfs", "ramfs"];

fn is_pseudo(fs_type: &str) -> bool {
    PSEUDO_FS.contains(&fs_type)
}

fn is_unscannable_fs(fs_type: &str) -> bool {
    !RAM_FS.contains(&fs_type) && (is_pseudo(fs_type) || NETWORK_FS.contains(&fs_type))
}

fn unscannable_mount_points() -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string("/proc/self/mounts") else {
        return Vec::new();
    };
    let mut roots: Vec<PathBuf> = text
        .lines()
        .filter_map(crate::disks::parse_mount_line)
        .filter(|(_, mount_point, fs_type)| {
            mount_point.is_absolute() && mount_point != Path::new("/") && is_unscannable_fs(fs_type)
        })
        .map(|(_, mount_point, _)| mount_point)
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

fn mount_table() -> Vec<StorageVolume> {
    let Ok(text) = std::fs::read_to_string("/proc/self/mounts") else {
        return Vec::new();
    };
    let mut volumes = Vec::new();
    for line in text.lines() {
        let Some((source, mount_point, fs_type)) = crate::disks::parse_mount_line(line) else {
            continue;
        };
        if is_pseudo(&fs_type)
            || !mount_point.is_absolute()
            || mount_point.starts_with("/snap")
            || mount_point.starts_with("/boot")
        {
            continue;
        }
        let (total_bytes, free_bytes) = statvfs_bytes(&mount_point);
        if total_bytes > 0 {
            volumes.push(StorageVolume {
                source,
                mount_point,
                fs_type,
                total_bytes,
                free_bytes,
            });
        }
    }
    volumes.sort_by_key(|volume| std::cmp::Reverse(volume.total_bytes));
    volumes.dedup_by(|left, right| left.mount_point == right.mount_point);
    volumes
}

fn statvfs_bytes(dir: &Path) -> (u64, u64) {
    use std::os::unix::ffi::OsStrExt;

    let path = match std::ffi::CString::new(dir.as_os_str().as_bytes()) {
        Ok(path) => path,
        Err(_) => return (0, 0),
    };
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: path is NUL-terminated and stat is a valid local output buffer.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return (0, 0);
    }
    let fragment = (stat.f_frsize as u64).max(1);
    (
        (stat.f_blocks as u64).saturating_mul(fragment),
        (stat.f_bavail as u64).saturating_mul(fragment),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_and_pseudo_mounts_are_unscannable_but_ram_is_not() {
        assert!(is_unscannable_fs("nfs4"));
        assert!(is_unscannable_fs("fuse.sshfs"));
        assert!(is_unscannable_fs("squashfs"));
        assert!(is_unscannable_fs("autofs"));
        assert!(!is_unscannable_fs("ext4"));
        assert!(!is_unscannable_fs("tmpfs"));
    }

    #[test]
    fn unscannable_mount_points_never_include_root() {
        assert!(unscannable_mount_points()
            .iter()
            .all(|path| path != Path::new("/") && path.is_absolute()));
    }
}

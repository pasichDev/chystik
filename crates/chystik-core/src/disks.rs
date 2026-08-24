//! Mounted-volume discovery — powers the disk-capacity header and
//! multi-disk scans. Linux-only by design: the mount table is read from
//! `/proc/self/mounts` and capacities come from `statvfs(2)` through a
//! direct `libc` call (no heavy dependency).

use std::path::{Path, PathBuf};

/// One mounted filesystem visible to this user.
#[derive(Debug, Clone, PartialEq)]
pub struct DiskInfo {
    /// Device or source (e.g. `/dev/nvme0n1p2`, `tmpfs`, UUID-less bind).
    pub source: String,
    /// Absolute mount point path.
    pub mount_point: PathBuf,
    /// Filesystem type (`ext4`, `btrfs`, `ntfs3`, ...).
    pub fs_type: String,
    /// Total size in bytes.
    pub total_bytes: u64,
    /// Free space in bytes.
    pub free_bytes: u64,
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

fn is_pseudo(fs_type: &str) -> bool {
    PSEUDO_FS.contains(&fs_type)
}

/// Parse one line of `/proc/self/mounts` (`source mount fstype opts...`).
/// Octal escapes (\040 = space) are decoded like `mount(8)` documents.
pub fn parse_mount_line(line: &str) -> Option<(String, PathBuf, String)> {
    let mut it = line.split_ascii_whitespace();
    let raw_source = it.next()?;
    let raw_mount = it.next()?;
    let fs_type = it.next()?.to_string();
    let source = unescape(raw_source);
    let mount_point = PathBuf::from(unescape(raw_mount));
    Some((source, mount_point, fs_type))
}

fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 4 <= bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            // Three octal digits, e.g. \040 = ' ' (see mount(8)).
            let code =
                (bytes[i + 1] - b'0') * 64 + (bytes[i + 2] - b'0') * 8 + (bytes[i + 3] - b'0');
            out.push(code);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Filesystems served over the network. A `stat` on one whose server is
/// unreachable blocks in the kernel for the mount's timeout — minutes, and
/// uninterruptibly — which is indistinguishable from a hung scan.
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

/// RAM-backed filesystems. Pseudo for the capacity header's purposes, but a
/// scan still walks them: `/tmp` is often `tmpfs` and holds real junk (and
/// the test fixtures live there).
const RAM_FS: &[&str] = &["tmpfs", "ramfs"];

/// True if walking into this filesystem can neither free disk space
/// (kernel and read-only mounts: `proc`, `squashfs`, `overlay`, …) nor be
/// relied on to answer promptly (network mounts, `autofs` trigger points).
fn is_unscannable_fs(fs_type: &str) -> bool {
    if RAM_FS.contains(&fs_type) {
        return false;
    }
    is_pseudo(fs_type) || NETWORK_FS.contains(&fs_type)
}

/// Mount points a scan must not descend into — see [`is_unscannable_fs`].
/// `/` is never returned: pruning it would abort every scan.
pub fn unscannable_mount_points() -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string("/proc/self/mounts") else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = text
        .lines()
        .filter_map(parse_mount_line)
        .filter(|(_, mount_point, fs_type)| {
            mount_point.is_absolute() && mount_point != Path::new("/") && is_unscannable_fs(fs_type)
        })
        .map(|(_, mount_point, _)| mount_point)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Read and filter the current mount table (real, writable-looking volumes).
pub fn mount_table() -> Vec<DiskInfo> {
    let Ok(text) = std::fs::read_to_string("/proc/self/mounts") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((source, mount_point, fs_type)) = parse_mount_line(line) else {
            continue;
        };
        if is_pseudo(&fs_type) || !mount_point.is_absolute() {
            continue;
        }
        // Snap flatpak/loop mounts under /snap or /var/lib are noise.
        if mount_point.starts_with("/snap") || mount_point.starts_with("/boot") {
            continue;
        }
        let (total_bytes, free_bytes) = statvfs_bytes(&mount_point);
        // Skip entries where statvfs failed (0/0) — likely stale mounts.
        if total_bytes == 0 {
            continue;
        }
        out.push(DiskInfo {
            source,
            mount_point,
            fs_type,
            total_bytes,
            free_bytes,
        });
    }
    out.sort_by_key(|d| std::cmp::Reverse(d.total_bytes));
    out.dedup_by(|a, b| a.mount_point == b.mount_point);
    out
}

/// `(total, free)` in bytes for `dir`; zeros when unavailable.
pub fn statvfs_bytes(dir: &Path) -> (u64, u64) {
    use std::os::unix::ffi::OsStrExt;
    let c_path = match std::ffi::CString::new(dir.as_os_str().as_bytes()) {
        Ok(p) => p,
        Err(_) => return (0, 0),
    };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c_path is NUL-terminated; st is a valid local buffer.
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut st) };
    if rc != 0 {
        return (0, 0);
    }
    let frsize = st.f_frsize.max(1) as u64;
    (
        st.f_blocks.saturating_mul(frsize),
        st.f_bavail.saturating_mul(frsize),
    )
}

/// Longest mount point that prefixes `path`, looked up in `mounts`.
/// Returns `Some(mount_point_string)` when found.
pub fn mount_of_in(path: &Path, mounts: &[DiskInfo]) -> Option<String> {
    mounts
        .iter()
        .filter(|d| path.starts_with(&d.mount_point))
        .max_by_key(|d| d.mount_point.as_os_str().len())
        .map(|d| d.mount_point.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_and_escaped_lines() {
        let (src, mp, fs) = parse_mount_line("/dev/sda1 /mnt/data ext4 rw,relatime").unwrap();
        assert_eq!(src, "/dev/sda1");
        assert_eq!(mp, PathBuf::from("/mnt/data"));
        assert_eq!(fs, "ext4");

        let (_, mp2, _) = parse_mount_line("//nas/share /media/user/My\\040Disk ntfs3 ro").unwrap();
        assert_eq!(mp2, PathBuf::from("/media/user/My Disk"));
        assert!(parse_mount_line("only-two fields").is_none());
    }

    #[test]
    fn pseudo_fs_filtering() {
        assert!(is_pseudo("tmpfs"));
        assert!(is_pseudo("overlay"));
        assert!(!is_pseudo("ext4"));
    }

    #[test]
    fn root_mount_is_found_with_capacity() {
        let table = mount_table();
        assert!(
            table
                .iter()
                .any(|d| d.mount_point == Path::new("/") && d.total_bytes > 0),
            "expected / with real capacity, got: {table:?}"
        );
    }

    #[test]
    fn network_and_pseudo_mounts_are_unscannable() {
        assert!(is_unscannable_fs("nfs4"));
        assert!(is_unscannable_fs("fuse.sshfs"));
        assert!(is_unscannable_fs("squashfs"));
        assert!(is_unscannable_fs("autofs"));
        assert!(!is_unscannable_fs("ext4"));
        assert!(!is_unscannable_fs("btrfs"));
        // RAM-backed mounts stay scannable: /tmp is real junk to a user
        // even though freeing it does not give disk space back.
        assert!(!is_unscannable_fs("tmpfs"));
        assert!(!is_unscannable_fs("ramfs"));
    }

    #[test]
    fn unscannable_mount_points_never_include_root() {
        assert!(unscannable_mount_points()
            .iter()
            .all(|p| p != Path::new("/") && p.is_absolute()));
    }

    #[test]
    fn mount_of_picks_longest_prefix() {
        let mk = |mp: &str| DiskInfo {
            source: String::new(),
            mount_point: PathBuf::from(mp),
            fs_type: "ext4".into(),
            total_bytes: 1,
            free_bytes: 1,
        };
        let table = vec![mk("/"), mk("/home")];
        assert_eq!(
            mount_of_in(Path::new("/home/u/.cache"), &table).unwrap(),
            "/home"
        );
        assert_eq!(mount_of_in(Path::new("/etc"), &table).unwrap(), "/");
        // No dedicated /opt entry: falls back to the containing root fs.
        assert_eq!(mount_of_in(Path::new("/opt"), &table).unwrap(), "/");
    }
}

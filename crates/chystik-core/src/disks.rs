//! Platform-neutral mounted-volume compatibility API.
//!
//! Host discovery lives behind [`crate::platform`]. This module preserves the
//! compact API consumed by the GUI and block-device view while keeping Linux
//! `/proc` and Unix `statvfs` details out of their callers.

use std::path::{Path, PathBuf};

pub use crate::platform::StorageVolume as DiskInfo;

/// Parse one Linux `/proc/self/mounts` line (`source mount fstype opts...`).
///
/// This is intentionally pure so the Linux platform adapter and block-device
/// discovery can share the documented mount escaping rules without exposing
/// host reads to frontends.
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

/// Mount points a scan must not descend into on this host.
pub fn unscannable_mount_points() -> Vec<PathBuf> {
    crate::platform::current().unscannable_roots()
}

/// User-visible, writable-looking volumes on this host.
pub fn mount_table() -> Vec<DiskInfo> {
    crate::platform::current().storage_volumes()
}

/// `(total, free)` bytes for `dir`; zeros when unavailable.
pub fn statvfs_bytes(dir: &Path) -> (u64, u64) {
    crate::platform::current()
        .storage_stats(dir)
        .map(|stats| (stats.total_bytes, stats.free_bytes))
        .unwrap_or((0, 0))
}

/// Longest mount point that prefixes `path`, looked up in `mounts`.
/// Returns `Some(mount_point_string)` when found.
pub fn mount_of_in(path: &Path, mounts: &[DiskInfo]) -> Option<String> {
    crate::platform::mount_of(path, mounts)
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
    fn mount_of_picks_longest_prefix() {
        let mk = |mp: &str| DiskInfo {
            source: String::new(),
            mount_point: PathBuf::from(mp),
            fs_type: "test".into(),
            total_bytes: 1,
            free_bytes: 1,
        };
        let table = vec![mk("/"), mk("/home")];
        assert_eq!(
            mount_of_in(Path::new("/home/u/.cache"), &table).unwrap(),
            "/home"
        );
        assert_eq!(mount_of_in(Path::new("/etc"), &table).unwrap(), "/");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn root_mount_is_found_with_capacity() {
        let table = mount_table();
        assert!(
            table
                .iter()
                .any(|disk| disk.mount_point == Path::new("/") && disk.total_bytes > 0),
            "expected / with real capacity, got: {table:?}"
        );
    }
}

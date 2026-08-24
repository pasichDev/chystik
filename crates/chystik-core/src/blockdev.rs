//! Physical drives and their partitions, read from `sysfs`.
//!
//! [`disks::mount_table`](crate::disks::mount_table) answers "what is
//! mounted"; this answers "what is attached". The difference is the whole
//! point of the Disks view: a drive that is plugged in and never mounted
//! contributes nothing to `df` and is exactly the capacity a user cannot
//! account for.
//!
//! Everything comes from `/sys/block`, so there is no dependency on
//! `lsblk`, `udev` or root. Sizes are in 512-byte sectors regardless of the
//! device's logical block size — that is what the kernel documents for
//! `size`, and getting it wrong silently multiplies every number.

use std::path::{Path, PathBuf};

use crate::disks;

const SYS_BLOCK: &str = "/sys/block";
const SECTOR: u64 = 512;

/// How a drive stores things. Shown as a badge, and it changes advice: a
/// spinning disk is where cold archives belong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveKind {
    Nvme,
    Ssd,
    Rotational,
    Removable,
    Unknown,
}

impl DriveKind {
    pub fn label(&self) -> &'static str {
        match self {
            DriveKind::Nvme => "NVMe",
            DriveKind::Ssd => "SSD",
            DriveKind::Rotational => "HDD",
            DriveKind::Removable => "Removable",
            DriveKind::Unknown => "Drive",
        }
    }
}

/// One partition on a drive.
#[derive(Debug, Clone, PartialEq)]
pub struct Partition {
    /// Kernel name, e.g. `nvme0n1p2`.
    pub name: String,
    pub size_bytes: u64,
    /// Filesystem and mount point, when it is mounted.
    pub mount: Option<PartitionMount>,
    /// Mounted, swap, or idle. `mount` is the filesystem detail; this is
    /// what the row says about the partition.
    pub usage: PartitionUse,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PartitionMount {
    pub mount_point: PathBuf,
    pub fs_type: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// What a partition is being used for, if anything.
#[derive(Debug, Clone, PartialEq)]
pub enum PartitionUse {
    /// Mounted as a filesystem.
    Filesystem(PartitionMount),
    /// Active swap. Not a filesystem, but very much in use.
    Swap,
    /// Attached and doing nothing.
    Idle,
}

impl PartitionMount {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    /// 0.0 to 1.0. Zero when the filesystem reports no capacity.
    pub fn used_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        self.used_bytes() as f32 / self.total_bytes as f32
    }
}

/// One attached drive.
#[derive(Debug, Clone, PartialEq)]
pub struct Drive {
    /// Kernel name, e.g. `sda`.
    pub name: String,
    /// Vendor string from sysfs; empty for virtual devices.
    pub model: String,
    pub size_bytes: u64,
    pub kind: DriveKind,
    pub partitions: Vec<Partition>,
}

impl Drive {
    /// Capacity in partitions nothing has mounted.
    ///
    /// The headline number of the Disks view: space that is physically
    /// present and invisible to every other tool the user runs.
    pub fn unmounted_bytes(&self) -> u64 {
        self.partitions
            .iter()
            .filter(|p| p.usage == PartitionUse::Idle)
            .map(|p| p.size_bytes)
            .sum()
    }

    pub fn used_bytes(&self) -> u64 {
        self.partitions
            .iter()
            .filter_map(|p| p.mount.as_ref())
            .map(PartitionMount::used_bytes)
            .sum()
    }
}

/// Every attached drive, largest first.
///
/// Loop, RAM and device-mapper devices are excluded: `loop0` is a mounted
/// snap, not a disk the user can act on, and listing sixty of them buries
/// the three that matter.
pub fn drives() -> Vec<Drive> {
    let mounts = MountIndex::read();
    let Ok(entries) = std::fs::read_dir(SYS_BLOCK) else {
        return Vec::new();
    };

    let mut drives: Vec<Drive> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_virtual(&name) {
                return None;
            }
            read_drive(&entry.path(), &name, &mounts)
        })
        .filter(|d| d.size_bytes > 0)
        .collect();
    drives.sort_by_key(|d| std::cmp::Reverse(d.size_bytes));
    drives
}

/// Total attached capacity across every drive.
pub fn total_attached_bytes(drives: &[Drive]) -> u64 {
    drives.iter().map(|d| d.size_bytes).sum()
}

/// Capacity in partitions nothing has mounted, across every drive.
pub fn total_unmounted_bytes(drives: &[Drive]) -> u64 {
    drives.iter().map(Drive::unmounted_bytes).sum()
}

fn is_virtual(name: &str) -> bool {
    const PREFIXES: &[&str] = &["loop", "ram", "zram", "dm-", "md", "sr", "fd"];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

fn read_drive(dir: &Path, name: &str, mounts: &MountIndex) -> Option<Drive> {
    let size_bytes = read_sectors(&dir.join("size"))?;
    let removable = read_string(&dir.join("removable")).as_deref() == Some("1");
    let rotational = read_string(&dir.join("queue/rotational")).as_deref() == Some("1");
    let model = read_string(&dir.join("device/model")).unwrap_or_default();

    let kind = if removable {
        DriveKind::Removable
    } else if name.starts_with("nvme") {
        DriveKind::Nvme
    } else if rotational {
        DriveKind::Rotational
    } else if read_string(&dir.join("queue/rotational")).is_some() {
        DriveKind::Ssd
    } else {
        DriveKind::Unknown
    };

    // Partitions are subdirectories carrying their own `partition` file.
    let mut partitions: Vec<Partition> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().join("partition").is_file())
        .filter_map(|entry| {
            let part_name = entry.file_name().to_string_lossy().into_owned();
            let size = read_sectors(&entry.path().join("size"))?;
            let usage = mounts.usage_of(&part_name);
            Some(Partition {
                mount: match &usage {
                    PartitionUse::Filesystem(m) => Some(m.clone()),
                    _ => None,
                },
                usage,
                name: part_name,
                size_bytes: size,
            })
        })
        .collect();
    partitions.sort_by(|a, b| a.name.cmp(&b.name));

    // A drive with no partition table is one whole volume.
    if partitions.is_empty() {
        let usage = mounts.usage_of(name);
        partitions.push(Partition {
            mount: match &usage {
                PartitionUse::Filesystem(m) => Some(m.clone()),
                _ => None,
            },
            usage,
            name: name.to_owned(),
            size_bytes,
        });
    }

    Some(Drive {
        name: name.to_owned(),
        model,
        size_bytes,
        kind,
        partitions,
    })
}

/// Every device the kernel currently has in use, filesystem or swap.
///
/// Deliberately NOT built from `disks::mount_table`: that one filters out
/// `/boot` and snap mounts because they are useless to a cleaner, which
/// made `/boot/efi` show up here as unused. A drive inventory needs the
/// unfiltered truth, and swap needs `/proc/swaps` — an active swap
/// partition appears in no mount table at all and was being counted as
/// free capacity.
struct MountIndex {
    filesystems: Vec<(String, PartitionMount)>,
    swap_devices: Vec<String>,
}

impl MountIndex {
    fn read() -> Self {
        let filesystems = std::fs::read_to_string("/proc/self/mounts")
            .unwrap_or_default()
            .lines()
            .filter_map(disks::parse_mount_line)
            .filter(|(source, ..)| source.starts_with("/dev/"))
            .map(|(source, mount_point, fs_type)| {
                let (total_bytes, free_bytes) = disks::statvfs_bytes(&mount_point);
                (
                    source,
                    PartitionMount {
                        mount_point,
                        fs_type,
                        total_bytes,
                        free_bytes,
                    },
                )
            })
            .collect();

        // /proc/swaps: a header line, then "Filename Type Size Used Priority".
        let swap_devices = std::fs::read_to_string("/proc/swaps")
            .unwrap_or_default()
            .lines()
            .skip(1)
            .filter_map(|line| line.split_whitespace().next())
            .filter(|name| name.starts_with("/dev/"))
            .map(str::to_owned)
            .collect();

        Self {
            filesystems,
            swap_devices,
        }
    }

    fn usage_of(&self, part_name: &str) -> PartitionUse {
        let device = format!("/dev/{part_name}");
        if self.swap_devices.contains(&device) {
            return PartitionUse::Swap;
        }
        match self
            .filesystems
            .iter()
            .find(|(source, _)| *source == device)
        {
            Some((_, mount)) => PartitionUse::Filesystem(mount.clone()),
            None => PartitionUse::Idle,
        }
    }
}

fn read_string(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// sysfs `size` is always in 512-byte sectors, whatever the device's
/// logical block size happens to be.
fn read_sectors(path: &Path) -> Option<u64> {
    read_string(path)?.parse::<u64>().ok().map(|s| s * SECTOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_devices_are_excluded() {
        for name in ["loop0", "loop42", "ram3", "zram0", "dm-1", "sr0"] {
            assert!(is_virtual(name), "{name} should be filtered out");
        }
        for name in ["sda", "sdb1", "nvme0n1", "vda", "mmcblk0"] {
            assert!(!is_virtual(name), "{name} is a real device");
        }
    }

    #[test]
    fn unmounted_capacity_counts_only_partitions_with_no_mount() {
        let drive = Drive {
            name: "sdb".into(),
            model: "TEST".into(),
            size_bytes: 1000,
            kind: DriveKind::Rotational,
            partitions: vec![
                Partition {
                    name: "sdb1".into(),
                    size_bytes: 400,
                    mount: None,
                    usage: PartitionUse::Idle,
                },
                Partition {
                    name: "sdb2".into(),
                    size_bytes: 600,
                    mount: Some(PartitionMount {
                        mount_point: "/data".into(),
                        fs_type: "ext4".into(),
                        total_bytes: 600,
                        free_bytes: 100,
                    }),
                    usage: PartitionUse::Filesystem(PartitionMount {
                        mount_point: "/data".into(),
                        fs_type: "ext4".into(),
                        total_bytes: 600,
                        free_bytes: 100,
                    }),
                },
            ],
        };
        assert_eq!(drive.unmounted_bytes(), 400);
        assert_eq!(drive.used_bytes(), 500);
    }

    /// Swap is in use, not free capacity. It appears in no mount table, so
    /// counting only `mount.is_none()` reported an active swap partition as
    /// idle space the user could reclaim.
    #[test]
    fn active_swap_is_not_counted_as_unmounted() {
        let drive = Drive {
            name: "sda".into(),
            model: "TEST".into(),
            size_bytes: 300,
            kind: DriveKind::Ssd,
            partitions: vec![
                Partition {
                    name: "sda1".into(),
                    size_bytes: 100,
                    mount: None,
                    usage: PartitionUse::Swap,
                },
                Partition {
                    name: "sda2".into(),
                    size_bytes: 200,
                    mount: None,
                    usage: PartitionUse::Idle,
                },
            ],
        };
        assert_eq!(drive.unmounted_bytes(), 200, "swap was counted as idle");
    }

    #[test]
    fn usage_fraction_handles_a_zero_capacity_filesystem() {
        let mount = PartitionMount {
            mount_point: "/x".into(),
            fs_type: "tmpfs".into(),
            total_bytes: 0,
            free_bytes: 0,
        };
        assert_eq!(mount.used_fraction(), 0.0);
        assert_eq!(mount.used_bytes(), 0);
    }

    /// Reads the machine this runs on. Asserts only invariants that hold
    /// everywhere, so it is meaningful in CI without pinning to one host.
    #[test]
    fn enumerating_this_machine_is_self_consistent() {
        let drives = drives();
        for drive in &drives {
            assert!(!drive.name.is_empty());
            assert!(drive.size_bytes > 0, "{} has no size", drive.name);
            assert!(!drive.partitions.is_empty(), "{} has no volume", drive.name);
            assert!(
                drive.unmounted_bytes() <= drive.size_bytes,
                "{} reports more unmounted than it holds",
                drive.name
            );
            for partition in &drive.partitions {
                assert!(partition.size_bytes <= drive.size_bytes);
                assert_eq!(
                    partition.mount.is_some(),
                    matches!(partition.usage, PartitionUse::Filesystem(_)),
                    "{} disagrees with itself about being mounted",
                    partition.name
                );
                if let Some(mount) = &partition.mount {
                    assert!(mount.mount_point.is_absolute());
                    assert!((0.0..=1.0).contains(&mount.used_fraction()));
                }
            }
        }
        assert_eq!(
            total_attached_bytes(&drives),
            drives.iter().map(|d| d.size_bytes).sum::<u64>()
        );
    }
}

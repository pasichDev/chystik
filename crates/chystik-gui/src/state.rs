//! Application state that is not drawing: scan lifecycle, scan targets,
//! filters, and the cached view the panels read from.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::JoinHandle;

use chystik_core::disks::DiskInfo;
use chystik_core::model::{Category, Finding, Severity};

// ---------------------------------------------------------------------------

pub(crate) enum ScanState {
    /// No scan running.
    Idle,
    /// A scan thread exists; `cancel_flag` requests cancellation.
    Scanning {
        cancel_flag: Arc<AtomicBool>,
        handle: JoinHandle<()>,
    },
}

/// Column the findings table is currently sorted by.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortCol {
    Path,
    Size,
    Severity,
    Age,
}

/// One selectable scan root: a detected mount point or a user-added folder.
#[derive(Clone)]
pub(crate) struct ScanTarget {
    pub(crate) root: PathBuf,
    /// Compact display label (mount point or truncated picked path).
    pub(crate) label: String,
    pub(crate) enabled: bool,
    /// Added via "Add folder…"; survives disk-table refreshes.
    pub(crate) user_added: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CategoryFilter {
    All,
    One(Category),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeverityFilter {
    All,
    One(Severity),
}

#[derive(Clone, Copy, Default)]
pub(crate) struct CleanBuckets {
    pub(crate) safe_bytes: u64,
    pub(crate) moderate_bytes: u64,
    pub(crate) risky_bytes: u64,
}

/// Inputs the cached view depends on. Any change forces a rebuild.
#[derive(PartialEq)]
pub(crate) struct ViewStamp {
    pub(crate) findings_len: usize,
    pub(crate) deleted_len: usize,
    pub(crate) category: CategoryFilter,
    pub(crate) severity: SeverityFilter,
    pub(crate) search: String,
    pub(crate) sort_col: SortCol,
    pub(crate) sort_asc: bool,
}

/// Filtered, sorted view over `findings` plus aggregates derived from it,
/// so the per-frame UI work is O(visible rows) instead of O(all findings).
#[derive(Default)]
pub(crate) struct ViewCache {
    /// Indices into `findings` passing filters, in current sort order.
    pub(crate) rows: Vec<usize>,
    pub(crate) buckets: CleanBuckets,
    /// Per-category rollup over everything the severity/search filters
    /// admit, IGNORING the category filter — the sidebar must keep showing
    /// every category while one of them is selected.
    pub(crate) cat_stats: Vec<CatStat>,
    /// Totals across `cat_stats`.
    pub(crate) all_bytes: u64,
    pub(crate) all_count: usize,
}

/// One sidebar row: what a whole category is worth.
///
/// This is the aggregate the old UI never computed. It knew the per-category
/// *maximum* (for a sparkline) but never the sum, so "package caches are
/// 7 GB, drop them" was a question the interface could not answer without
/// fifteen trips through a combo box.
#[derive(Clone, Copy)]
pub(crate) struct CatStat {
    pub(crate) category: Category,
    pub(crate) bytes: u64,
    pub(crate) count: usize,
    pub(crate) safe_bytes: u64,
    pub(crate) moderate_bytes: u64,
    pub(crate) risky_bytes: u64,
}

impl CatStat {
    pub(crate) fn new(category: Category) -> Self {
        Self {
            category,
            bytes: 0,
            count: 0,
            safe_bytes: 0,
            moderate_bytes: 0,
            risky_bytes: 0,
        }
    }

    pub(crate) fn add(&mut self, f: &Finding) {
        self.bytes += f.size_bytes;
        self.count += 1;
        match f.severity {
            Severity::Safe => self.safe_bytes += f.size_bytes,
            Severity::Moderate => self.moderate_bytes += f.size_bytes,
            Severity::Risky => self.risky_bytes += f.size_bytes,
        }
    }

    /// Dominant severity, used for the category dot.
    pub(crate) fn severity(&self) -> Severity {
        if self.risky_bytes >= self.moderate_bytes && self.risky_bytes >= self.safe_bytes {
            Severity::Risky
        } else if self.moderate_bytes >= self.safe_bytes {
            Severity::Moderate
        } else {
            Severity::Safe
        }
    }
}

// ---------------------------------------------------------------------------

/// Default scan targets from the mount table: `/` once plus the other real
/// volumes, skipping mounts nested inside another non-root volume (e.g. a
/// bind mount living inside a data disk). `/` itself never absorbs the
/// others here — overlapping choices are collapsed later, at scan time.
pub(crate) fn default_roots(disks: &[DiskInfo]) -> Vec<PathBuf> {
    let mut chosen: Vec<PathBuf> = Vec::new();
    if disks.iter().any(|d| d.mount_point == Path::new("/")) {
        chosen.push(PathBuf::from("/"));
    }
    let mut others: Vec<&DiskInfo> = disks
        .iter()
        .filter(|d| d.mount_point != Path::new("/"))
        .collect();
    // Shortest first: a nested mount is always seen after its container.
    others.sort_by_key(|d| d.mount_point.as_os_str().len());
    for d in others {
        let nested = chosen
            .iter()
            .any(|r| r != Path::new("/") && d.mount_point.starts_with(r));
        if !nested {
            chosen.push(d.mount_point.clone());
        }
    }
    chosen
}

/// Drop roots nested inside (or equal to) another root, shortest first, so a
/// scan never walks the same subtree twice.
pub(crate) fn dedup_nested_roots(roots: &mut Vec<PathBuf>) {
    roots.sort_by_key(|p| p.as_os_str().len());
    let mut kept: Vec<PathBuf> = Vec::new();
    for r in roots.drain(..) {
        if !kept.iter().any(|k| r.starts_with(k)) {
            kept.push(r);
        }
    }
    *roots = kept;
}

/// Longest root in `roots` that is a directory-prefix of `path`.
pub(crate) fn longest_containing<'a>(roots: &[&'a Path], path: &Path) -> Option<&'a Path> {
    roots
        .iter()
        .copied()
        .filter(|r| path.starts_with(*r))
        .max_by_key(|r| r.as_os_str().len())
}

/// Case-insensitive substring filter applied to finding paths, split from
/// the app struct so the hot rebuild path can reuse one lowered needle.
pub(crate) fn matches_filter(
    f: &Finding,
    category_filter: CategoryFilter,
    severity_filter: SeverityFilter,
    needle: Option<&str>,
) -> bool {
    let cat_ok = match category_filter {
        CategoryFilter::All => true,
        CategoryFilter::One(c) => f.category == c,
    };
    let sev_ok = match severity_filter {
        SeverityFilter::All => true,
        SeverityFilter::One(s) => f.severity == s,
    };
    let search_ok = match needle {
        None => true,
        Some(n) => f.path.to_string_lossy().to_lowercase().contains(n),
    };
    cat_ok && sev_ok && search_ok
}

/// Split visible finding bytes into the two header buckets:
/// `safe` regenerates automatically, everything else needs review.
/// Production path aggregates inline in `rebuild_view`; kept for the
/// unit tests that pin the bucket semantics.
#[cfg(test)]
pub(crate) fn clean_buckets<'a>(rows: impl IntoIterator<Item = &'a Finding>) -> CleanBuckets {
    let mut b = CleanBuckets::default();
    for f in rows {
        b.add(f);
    }
    b
}

impl CleanBuckets {
    pub(crate) fn add(&mut self, f: &Finding) {
        match f.severity {
            Severity::Safe => self.safe_bytes += f.size_bytes,
            Severity::Moderate => self.moderate_bytes += f.size_bytes,
            Severity::Risky => self.risky_bytes += f.size_bytes,
        }
    }

    /// Everything that deserves a second look before deletion.
    pub(crate) fn risky_total(&self) -> u64 {
        self.moderate_bytes + self.risky_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(mount: &str, total: u64, free: u64) -> DiskInfo {
        DiskInfo {
            source: format!("/dev/{}", mount.trim_matches('/')),
            mount_point: PathBuf::from(mount),
            fs_type: "ext4".into(),
            total_bytes: total,
            free_bytes: free,
        }
    }

    fn finding(sev: Severity, size: u64, mount: Option<&str>, path: &str) -> Finding {
        Finding {
            path: PathBuf::from(path),
            category: Category::BuildArtifacts,
            severity: sev,
            size_bytes: size,
            last_used: None,
            mount: mount.map(String::from),
            note: String::new(),
        }
    }

    #[test]
    fn clean_buckets_split_safe_from_review() {
        let rows = [
            finding(Severity::Safe, 100, Some("/"), "/a"),
            finding(Severity::Safe, 50, None, "/b"),
            finding(Severity::Moderate, 30, Some("/"), "/c"),
            finding(Severity::Risky, 20, Some("/media/ext"), "/d"),
        ];
        let b = clean_buckets(rows.iter());
        assert_eq!(b.safe_bytes, 150);
        assert_eq!(b.moderate_bytes, 30);
        assert_eq!(b.risky_bytes, 20);
        assert_eq!(b.risky_total(), 50);
        let empty = clean_buckets(std::iter::empty::<&Finding>());
        assert_eq!((empty.safe_bytes, empty.risky_total()), (0, 0));
    }

    #[test]
    fn default_roots_keep_top_level_and_skip_nested() {
        let disks = vec![
            disk("/", 100, 1),
            disk("/home", 200, 1),
            disk("/home/bind-nested", 50, 1),
            disk("/mnt/data", 300, 1),
            disk("/mnt/data/archive", 25, 1),
        ];
        assert_eq!(
            default_roots(&disks),
            vec![
                PathBuf::from("/"),
                PathBuf::from("/home"),
                PathBuf::from("/mnt/data")
            ]
        );
    }

    #[test]
    fn default_roots_without_root_mount_lists_volumes() {
        let disks = vec![disk("/home", 1, 1), disk("/home/dev", 1, 1)];
        assert_eq!(default_roots(&disks), vec![PathBuf::from("/home")]);
        assert!(default_roots(&[]).is_empty());
    }

    #[test]
    fn dedup_nested_roots_removes_children_and_duplicates() {
        let mut roots = vec![
            PathBuf::from("/a/inner"),
            PathBuf::from("/a"),
            PathBuf::from("/a"),
            PathBuf::from("/b"),
        ];
        dedup_nested_roots(&mut roots);
        assert_eq!(roots, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn longest_containing_prefers_deepest_match() {
        let a = PathBuf::from("/");
        let b = PathBuf::from("/home");
        let c = PathBuf::from("/home/dev");
        let refs = [a.as_path(), b.as_path(), c.as_path()];
        assert_eq!(
            longest_containing(&refs, Path::new("/home/dev/x")),
            Some(c.as_path())
        );
        assert_eq!(
            longest_containing(&refs, Path::new("/etc")),
            Some(a.as_path())
        );
        assert_eq!(longest_containing(&refs, Path::new("rel/path")), None);
    }
}

//! Application state that is not drawing: scan lifecycle, scan targets,
//! filters, and the cached view the panels read from.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread::JoinHandle;

use chystik_core::cleaner::{CleanEvent, CleanupOutcome};
use chystik_core::model::{Category, Finding, FindingPolicy, RecoveryClass, Severity};
use chystik_core::platform::StorageVolume;

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

/// What the cleaner thread sends back. The events are progress; the single
/// `Done` carries the authoritative tally the notice is built from, so the
/// UI never has to re-derive it from the events it happened to observe.
pub(crate) enum CleanMsg {
    Event(CleanEvent),
    Done(Box<CleanupOutcome>),
}

/// Which model a running cleanup has to update when it lands.
pub(crate) enum CleanScope {
    /// Rows submitted from the findings table, paired with the index each
    /// path came from so the removed ones can be struck off.
    Findings(Vec<(usize, PathBuf)>),
    /// The privacy view keeps no per-row bookkeeping; it re-probes instead.
    Traces,
}

/// Live counters the progress modal paints. Advanced from `CleanMsg`s on
/// the UI thread, never read across threads.
pub(crate) struct CleanProgress {
    pub(crate) total: usize,
    pub(crate) total_bytes: u64,
    /// Items that reached a terminal event, removed or skipped.
    pub(crate) done: usize,
    pub(crate) freed_bytes: u64,
    /// The path currently being validated and moved.
    pub(crate) current: Option<PathBuf>,
}

impl CleanProgress {
    /// 0.0..=1.0 by item count. Byte-weighted progress would be a lie: the
    /// scanner sizes a directory, it does not time the move.
    pub(crate) fn fraction(&self) -> f32 {
        if self.total == 0 {
            return 1.0;
        }
        self.done as f32 / self.total as f32
    }
}

/// Deletion lifecycle, mirroring `ScanState`.
///
/// Cleaning ran inline on the UI thread until it did not: moving a
/// multi-gigabyte cache to the recycle bin froze the window for the whole
/// batch, so the confirmation dialog looked hung and the result notice
/// flashed past. The work belongs on a worker, reporting as it goes.
pub(crate) enum CleanState {
    Idle,
    Running {
        rx: Receiver<CleanMsg>,
        handle: JoinHandle<()>,
        scope: CleanScope,
        progress: CleanProgress,
        /// Rows the UI itself refused before the worker started (excluded
        /// or advisory), folded into the final skipped count.
        pre_skipped: usize,
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

/// User-facing totals for the two independent finding axes. These deliberately
/// overlap: a valuable item can require review, while an automatic recovery
/// class alone never grants cleanup permission.
#[derive(Clone, Copy, Default)]
pub(crate) struct CleanupTotals {
    pub(crate) found_count: usize,
    pub(crate) found_bytes: u64,
    pub(crate) auto_cleanable_count: usize,
    pub(crate) auto_cleanable_bytes: u64,
    pub(crate) review_required_count: usize,
    pub(crate) review_required_bytes: u64,
    pub(crate) manual_count: usize,
    pub(crate) manual_bytes: u64,
}

impl CleanupTotals {
    pub(crate) fn add(&mut self, finding: &Finding) {
        self.found_count += 1;
        self.found_bytes += finding.size_bytes;
        if finding.is_auto_cleanable() {
            self.auto_cleanable_count += 1;
            self.auto_cleanable_bytes += finding.size_bytes;
        }
        if finding.policy() == FindingPolicy::DirectReview {
            self.review_required_count += 1;
            self.review_required_bytes += finding.size_bytes;
        }
        if finding.recovery_class() == RecoveryClass::ManualOrIrreplaceable {
            self.manual_count += 1;
            self.manual_bytes += finding.size_bytes;
        }
    }
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

/// One table row: either a single finding, or several collapsed into one
/// version group. `usize`/`Group` index into `findings`/`version_groups`
/// respectively rather than borrowing, so `ViewCache` stays self-contained
/// and `Copy`-cheap to sort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowRef {
    Single(usize),
    Group(usize),
}

/// Several historical builds of the same tool collapsed into one table row.
///
/// A versioned store (`~/.local/share/claude/versions` and similar — see
/// `chystik_core::rules::GROUP_RULES`) keeps every build it ever fetched
/// while only the newest runs. The scanner already reports each superseded
/// build as its own finding; showing them as N look-alike rows that differ
/// only by a version number in the path buried the actual message ("a
/// newer build exists, these are safe to remove") in repetition. Grouping
/// is purely a display concern — every member is still a real, independent
/// `Finding`, selected and deleted exactly like any other.
pub(crate) struct VersionGroup {
    /// The shared parent directory — `Finding::version_group` — and where
    /// the "exclude this" context action points.
    pub(crate) dir: PathBuf,
    /// Human name for the row's headline, e.g. "Claude Code".
    pub(crate) app_name: String,
    /// The rule's shared note, reused verbatim as the row's sub-line —
    /// same convention as an ordinary finding row.
    pub(crate) note: String,
    /// Indices into `findings`, newest `last_used` first.
    pub(crate) members: Vec<usize>,
    pub(crate) total_bytes: u64,
    /// Shared by every member — a `GroupRule` sets one severity for all
    /// the builds it supersedes.
    pub(crate) severity: Severity,
}

impl VersionGroup {
    pub(crate) fn count(&self) -> usize {
        self.members.len()
    }
}

/// Best-effort human name for a versioned store, derived from its rule note
/// first and its own directory name otherwise. Kept beside `GROUP_RULES`'
/// notes in intent, not in code: a new group rule works today, just with a
/// generic label, until its note text (or this table) is taught the name.
pub(crate) fn friendly_app_name(note: &str, dir: &Path) -> String {
    const KNOWN: &[(&str, &str)] = &[
        ("Claude Code", "Claude Code"),
        ("Codex CLI", "Codex CLI"),
        ("Node.js", "Node.js"),
        ("Toolbox", "JetBrains Toolbox"),
    ];
    for (needle, name) in KNOWN {
        if note.contains(needle) {
            return (*name).to_owned();
        }
    }
    // Fall back to the store's own directory name, skipping a generic leaf
    // that names the mechanism ("versions", "releases") rather than the
    // application that owns it.
    const GENERIC_LEAVES: &[&str] = &["versions", "releases", "apps", "toolchains", "packages"];
    let mut components: Vec<&str> = dir
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    while let Some(last) = components.last() {
        if GENERIC_LEAVES.contains(&last.to_lowercase().as_str()) {
            components.pop();
        } else {
            break;
        }
    }
    components
        .last()
        .map(|s| s.trim_start_matches('.').to_owned())
        .unwrap_or_else(|| "this app".to_owned())
}

/// Filtered, sorted view over `findings` plus aggregates derived from it,
/// so the per-frame UI work is O(visible rows) instead of O(all findings).
#[derive(Default)]
pub(crate) struct ViewCache {
    /// Table rows in current sort order: singles and version groups mixed.
    pub(crate) rows: Vec<RowRef>,
    /// Every currently-filtered finding index, singles and group members
    /// alike — what a bulk "select all" must cover, since collapsing a
    /// group into one `rows` entry must never shrink what bulk selection
    /// reaches.
    pub(crate) all_rows: Vec<usize>,
    pub(crate) version_groups: Vec<VersionGroup>,
    pub(crate) cleanup_totals: CleanupTotals,
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
pub(crate) fn default_roots(disks: &[StorageVolume]) -> Vec<PathBuf> {
    let mut chosen: Vec<PathBuf> = Vec::new();
    if disks.iter().any(|d| d.mount_point == Path::new("/")) {
        chosen.push(PathBuf::from("/"));
    }
    let mut others: Vec<&StorageVolume> = disks
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
    #[cfg(test)]
    pub(crate) fn add(&mut self, f: &Finding) {
        match f.severity {
            Severity::Safe => self.safe_bytes += f.size_bytes,
            Severity::Moderate => self.moderate_bytes += f.size_bytes,
            Severity::Risky => self.risky_bytes += f.size_bytes,
        }
    }

    /// Everything that deserves a second look before deletion.
    #[cfg(test)]
    pub(crate) fn risky_total(&self) -> u64 {
        self.moderate_bytes + self.risky_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_app_name_recognises_every_shipped_group_rule() {
        assert_eq!(
            friendly_app_name(
                "superseded Claude Code build — the current one is kept",
                Path::new("/home/me/.local/share/claude/versions"),
            ),
            "Claude Code"
        );
        assert_eq!(
            friendly_app_name(
                "superseded Codex CLI build — the current one is kept",
                Path::new("/home/me/.codex/packages/standalone/releases"),
            ),
            "Codex CLI"
        );
        assert_eq!(
            friendly_app_name(
                "older Node.js install — reinstall with `nvm install <version>`",
                Path::new("/home/me/.nvm/versions/node"),
            ),
            "Node.js"
        );
        assert_eq!(
            friendly_app_name(
                "superseded Toolbox build — the current one is kept",
                Path::new("/home/me/.local/share/JetBrains/Toolbox/apps"),
            ),
            "JetBrains Toolbox"
        );
    }

    /// A future group rule with no entry in `KNOWN` still gets a readable
    /// name, from its own directory rather than the generic leaf.
    #[test]
    fn friendly_app_name_falls_back_to_the_store_directory() {
        assert_eq!(
            friendly_app_name(
                "superseded Widget build",
                Path::new("/home/me/.widget/versions"),
            ),
            "widget"
        );
        // A path that is nothing but a generic leaf leaves no name to fall
        // back to.
        assert_eq!(
            friendly_app_name("no match here", Path::new("versions")),
            "this app"
        );
    }

    fn disk(mount: &str, total: u64, free: u64) -> StorageVolume {
        StorageVolume {
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
            advice: None,
            provenance: None,
            version_group: None,
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
    fn cleanup_totals_keep_recovery_and_policy_separate() {
        let mut automatic_review = finding(Severity::Safe, 30, None, "/review");
        automatic_review.provenance = Some(chystik_core::model::RuleProvenance {
            rule_id: "fixture.review".into(),
            source_url: "https://example.test/rule".into(),
            policy: FindingPolicy::DirectReview,
            recovery_cost: "fixture".into(),
            reviewed_at: "2026-08-26".into(),
            preconditions: vec!["fixture".into()],
        });
        let rows = [
            finding(Severity::Safe, 10, None, "/auto"),
            automatic_review,
            finding(Severity::Risky, 40, None, "/manual"),
        ];
        let mut totals = CleanupTotals::default();
        for finding in &rows {
            totals.add(finding);
        }

        assert_eq!((totals.found_count, totals.found_bytes), (3, 80));
        assert_eq!(
            (totals.auto_cleanable_count, totals.auto_cleanable_bytes),
            (1, 10)
        );
        assert_eq!(
            (totals.review_required_count, totals.review_required_bytes),
            (2, 70)
        );
        assert_eq!((totals.manual_count, totals.manual_bytes), (1, 40));
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
}

/// Which view the window is showing.
///
/// Navigation follows the "modeless" direction: no permanent tab strip, a
/// compact chip in the command bar showing where you are, and a palette on
/// Ctrl+K. The content area keeps every pixel.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Section {
    #[default]
    Cleanup,
    Disks,
    Privacy,
}

impl Section {
    pub(crate) const ALL: [Section; 3] = [Section::Cleanup, Section::Disks, Section::Privacy];

    pub(crate) fn label(self, s: &crate::i18n::Strings) -> &str {
        match self {
            Section::Cleanup => &s.section_cleanup,
            Section::Disks => &s.section_disks,
            Section::Privacy => &s.section_privacy,
        }
    }

    /// 1-based position, used for the digit shortcuts.
    pub(crate) fn index(self) -> usize {
        Section::ALL.iter().position(|x| *x == self).unwrap_or(0)
    }
}

#[cfg(test)]
mod section_tests {
    use super::*;

    #[test]
    fn every_section_has_a_stable_index() {
        for (i, section) in Section::ALL.iter().enumerate() {
            assert_eq!(section.index(), i);
        }
    }

    #[test]
    fn cleanup_is_where_the_app_opens() {
        assert_eq!(Section::default(), Section::Cleanup);
    }
}

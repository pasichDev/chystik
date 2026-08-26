//! Application-level operations shared by the GUI and command-line frontend.
//!
//! This is deliberately above `scanner` and `cleaner`: frontends decide how
//! to render or confirm a plan, but they cannot silently invent different
//! root ownership, filter, exclusion, or safe-bulk semantics.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::cleaner::{CleanupItem, CleanupOutcome, Remover};
use crate::config::normalize_exclusions;
use crate::guard;
use crate::model::{Category, ChystikError, Finding, FindingPolicy, Severity};
use crate::report::{self, CategorySummary};
use crate::scanner::{self, ScanOptions};

/// The version carried by CLI JSON documents. Increment only for intentional,
/// documented compatibility changes.
pub const MACHINE_SCHEMA_VERSION: u32 = 1;

/// User-selectable presentation filter. It never changes what was scanned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindingFilter {
    pub category: Option<Category>,
    pub severity: Option<Severity>,
}

impl FindingFilter {
    pub fn matches(&self, finding: &Finding) -> bool {
        self.category
            .is_none_or(|category| finding.category == category)
            && self
                .severity
                .is_none_or(|severity| finding.severity == severity)
    }
}

/// Stable sorting modes available to both frontends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Size,
    Age,
    Severity,
    Path,
}

/// Scanner settings plus presentation selection. Exclusions are supplied by
/// persisted policy and repeated by the planner before cleanup.
#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub roots: Vec<PathBuf>,
    pub filter: FindingFilter,
    pub sort: SortKey,
    pub min_finding_bytes: u64,
    pub exclude: Vec<PathBuf>,
    pub include_advisories: bool,
}

impl Default for ScanRequest {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            filter: FindingFilter::default(),
            sort: SortKey::default(),
            min_finding_bytes: ScanOptions::default().min_finding_bytes,
            exclude: Vec::new(),
            include_advisories: false,
        }
    }
}

/// Complete, render-ready result of a finite scan.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub roots: Vec<PathBuf>,
    pub findings: Vec<Finding>,
    pub summaries: Vec<CategorySummary>,
}

/// Terminal counters for a filtered streaming application scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppScanSummary {
    pub roots: Vec<PathBuf>,
    pub findings: u64,
    pub total_bytes: u64,
}

/// Streaming events after shared options and filters have been applied.
/// `Finding` events are emitted as soon as the scanner classifies them; no
/// result vector exists in this path.
#[derive(Debug, Clone)]
pub enum AppScanEvent {
    Started { root: PathBuf },
    DirectoriesScanned { count: u64 },
    Finding(Finding),
    Finished(AppScanSummary),
    Cancelled,
}

impl ScanResult {
    pub fn from_findings(findings: Vec<Finding>, roots: Vec<PathBuf>) -> Self {
        Self {
            summaries: report::summarize(&findings),
            roots,
            findings,
        }
    }
}

/// Classification explanation for one exact path. It is read-only and does
/// not imply that the path is cleanable: cleanup additionally requires an
/// approved scan root, exclusions, and the final guard check.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Explanation {
    Recognized {
        path: PathBuf,
        category: Category,
        severity: Severity,
        note: String,
    },
    Unrecognized {
        path: PathBuf,
    },
}

/// Explain the registered rule for an existing directory without scanning any
/// parents or descendants. Symlinks are rejected instead of silently naming
/// an indirection target that a later cleanup guard would refuse.
pub fn explain(path: &Path) -> Result<Explanation, ChystikError> {
    let absolute = std::fs::canonicalize(path).map_err(|error| {
        ChystikError::InvalidInput(format!(
            "path {} cannot be resolved: {error}",
            path.display()
        ))
    })?;
    if !absolute.is_dir() {
        return Err(ChystikError::InvalidInput(format!(
            "path {} is not a directory",
            absolute.display()
        )));
    }
    if crate::platform::current().is_link_or_reparse_point(path) {
        return Err(ChystikError::InvalidInput(format!(
            "path {} is a symlink or reparse point",
            path.display()
        )));
    }
    Ok(match crate::rules::classify(&absolute) {
        Some(rule) => Explanation::Recognized {
            path: absolute,
            category: rule.category,
            severity: rule.severity,
            note: rule.note,
        },
        None => Explanation::Unrecognized { path: absolute },
    })
}

/// Run a complete scan through the shared root/filter/configuration path.
pub fn scan(request: &ScanRequest, cancel: &Arc<AtomicBool>) -> Result<ScanResult, ChystikError> {
    scan_with_events(request, cancel, |_| {})
}

/// Run a complete scan while exposing progress to a human-oriented frontend.
/// Unlike [`scan_stream`], this keeps the selected findings for a final
/// sorted report. The callback is safe to invoke from scanner worker threads.
pub fn scan_with_events<F>(
    request: &ScanRequest,
    cancel: &Arc<AtomicBool>,
    on_event: F,
) -> Result<ScanResult, ChystikError>
where
    F: Fn(AppScanEvent) + Send + Sync + 'static,
{
    let (roots, options) = prepare_scan(request)?;
    let callback: Arc<dyn Fn(AppScanEvent) + Send + Sync> = Arc::new(on_event);
    let findings = Arc::new(Mutex::new(Vec::new()));
    let collected_findings = findings.clone();
    let event_callback = callback.clone();
    let filter = request.filter;

    scanner::scan_many_stream(&roots, &options, cancel, move |event| match event {
        scanner::ScanStreamEvent::Started { root } => {
            event_callback(AppScanEvent::Started { root });
        }
        scanner::ScanStreamEvent::DirectoriesScanned { count } => {
            event_callback(AppScanEvent::DirectoriesScanned { count });
        }
        scanner::ScanStreamEvent::FindingFound(finding) => {
            if filter.matches(&finding) {
                if let Ok(mut findings) = collected_findings.lock() {
                    findings.push(finding.clone());
                }
                event_callback(AppScanEvent::Finding(finding));
            }
        }
        scanner::ScanStreamEvent::Finished(_) => {}
        scanner::ScanStreamEvent::Cancelled => event_callback(AppScanEvent::Cancelled),
    })?;

    let findings = findings
        .lock()
        .map_err(|_| ChystikError::Io(std::io::Error::other("scan findings lock poisoned")))?
        .clone();
    let findings = filter_and_sort(&findings, &FindingFilter::default(), request.sort);
    let result = ScanResult::from_findings(findings, roots);
    callback(AppScanEvent::Finished(AppScanSummary {
        roots: result.roots.clone(),
        findings: result.findings.len() as u64,
        total_bytes: result
            .findings
            .iter()
            .map(|finding| finding.size_bytes)
            .sum(),
    }));
    Ok(result)
}

/// Run a scan for a streaming frontend such as JSONL. The filter is applied
/// before the callback sees a finding, and only aggregate counters are kept.
pub fn scan_stream<F>(
    request: &ScanRequest,
    cancel: &Arc<AtomicBool>,
    on_event: F,
) -> Result<AppScanSummary, ChystikError>
where
    F: Fn(AppScanEvent) + Send + Sync + 'static,
{
    let (roots, options) = prepare_scan(request)?;
    let callback: Arc<dyn Fn(AppScanEvent) + Send + Sync> = Arc::new(on_event);
    let filter = request.filter;
    let findings = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let total_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let stream_callback = callback.clone();
    let stream_findings = findings.clone();
    let stream_bytes = total_bytes.clone();

    let result = scanner::scan_many_stream(&roots, &options, cancel, move |event| match event {
        scanner::ScanStreamEvent::Started { root } => {
            stream_callback(AppScanEvent::Started { root });
        }
        scanner::ScanStreamEvent::DirectoriesScanned { count } => {
            stream_callback(AppScanEvent::DirectoriesScanned { count });
        }
        scanner::ScanStreamEvent::FindingFound(finding) => {
            if filter.matches(&finding) {
                stream_findings.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                stream_bytes.fetch_add(finding.size_bytes, std::sync::atomic::Ordering::Relaxed);
                stream_callback(AppScanEvent::Finding(finding));
            }
        }
        scanner::ScanStreamEvent::Finished(_) => {}
        scanner::ScanStreamEvent::Cancelled => stream_callback(AppScanEvent::Cancelled),
    });
    result?;
    let summary = AppScanSummary {
        roots,
        findings: findings.load(std::sync::atomic::Ordering::Relaxed),
        total_bytes: total_bytes.load(std::sync::atomic::Ordering::Relaxed),
    };
    callback(AppScanEvent::Finished(summary.clone()));
    Ok(summary)
}

fn prepare_scan(request: &ScanRequest) -> Result<(Vec<PathBuf>, ScanOptions), ChystikError> {
    let roots = if request.roots.is_empty() {
        let cwd = std::env::current_dir().map_err(ChystikError::Io)?;
        normalize_roots(&[cwd])?
    } else {
        normalize_roots(&request.roots)?
    };
    let options = ScanOptions {
        min_finding_bytes: request.min_finding_bytes,
        exclude: normalize_exclusions(request.exclude.clone()),
        include_advisories: request.include_advisories,
        ..ScanOptions::default()
    };
    Ok((roots, options))
}

/// Convert existing input directories to absolute canonical paths and remove
/// roots already covered by a shorter root. A missing file or regular file is
/// invalid input, never an empty scan that might conceal a typo.
pub fn normalize_roots(roots: &[PathBuf]) -> Result<Vec<PathBuf>, ChystikError> {
    let mut canonical: Vec<PathBuf> = roots
        .iter()
        .map(|root| {
            let root = std::fs::canonicalize(root).map_err(|error| {
                ChystikError::InvalidInput(format!(
                    "scan root {} cannot be resolved: {error}",
                    root.display()
                ))
            })?;
            if !root.is_dir() {
                return Err(ChystikError::InvalidInput(format!(
                    "scan root {} is not a directory",
                    root.display()
                )));
            }
            Ok(root)
        })
        .collect::<Result<_, ChystikError>>()?;
    dedup_nested_roots(&mut canonical);
    Ok(canonical)
}

/// Drop duplicate and nested roots without touching the filesystem. The GUI
/// uses this while its target picker is live; `normalize_roots` performs the
/// same operation only after canonicalizing verified directories for CLI runs.
pub fn dedup_nested_roots(roots: &mut Vec<PathBuf>) {
    roots.sort_by_key(|path| path.as_os_str().len());
    let mut kept = Vec::new();
    for root in roots.drain(..) {
        if !kept.iter().any(|parent: &PathBuf| root.starts_with(parent)) {
            kept.push(root);
        }
    }
    *roots = kept;
}

/// Clone, filter and order findings for a frontend view. The original scan is
/// left untouched so a UI can adjust filters without another filesystem walk.
pub fn filter_and_sort(
    findings: &[Finding],
    filter: &FindingFilter,
    sort: SortKey,
) -> Vec<Finding> {
    let mut selected: Vec<Finding> = findings
        .iter()
        .filter(|finding| filter.matches(finding))
        .cloned()
        .collect();
    selected.sort_by(|left, right| match sort {
        SortKey::Size => right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path)),
        // Missing timestamps are explicitly last; older dated items first.
        SortKey::Age => left
            .last_used
            .cmp(&right.last_used)
            .then_with(|| left.path.cmp(&right.path)),
        SortKey::Severity => severity_rank(left.severity)
            .cmp(&severity_rank(right.severity))
            .then_with(|| left.path.cmp(&right.path)),
        SortKey::Path => left.path.cmp(&right.path),
    });
    selected
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Safe => 0,
        Severity::Moderate => 1,
        Severity::Risky => 2,
    }
}

/// A safe candidate plus the root against which the cleaner must revalidate
/// it immediately before native-trash cleanup.
#[derive(Debug, Clone, Serialize)]
pub struct PlannedCleanup {
    pub finding: Finding,
    pub scan_root: PathBuf,
}

impl PlannedCleanup {
    pub fn cleanup_item(&self) -> CleanupItem {
        CleanupItem {
            path: self.finding.path.clone(),
            size_bytes: self.finding.size_bytes,
            scan_root: Some(self.scan_root.clone()),
        }
    }
}

/// Why a scanned result is shown in the manifest but cannot join a safe bulk
/// cleanup. The cleaner independently repeats guard validation at execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanSkipReason {
    Excluded,
    Advisory,
    NotSafe,
    OutsideEveryTarget,
    GuardRefused,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedCleanup {
    pub finding: Finding,
    pub reason: PlanSkipReason,
}

/// A complete manifest before a frontend prompts or passes anything to the
/// remover. `eligible` has exactly the items a `clean --safe` may select.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SafeCleanupPlan {
    pub eligible: Vec<PlannedCleanup>,
    pub skipped: Vec<SkippedCleanup>,
}

impl SafeCleanupPlan {
    pub fn eligible_bytes(&self) -> u64 {
        self.eligible
            .iter()
            .map(|item| item.finding.size_bytes)
            .sum()
    }

    pub fn cleanup_items(&self) -> Vec<CleanupItem> {
        self.eligible
            .iter()
            .map(PlannedCleanup::cleanup_item)
            .collect()
    }
}

/// Execute a previously rendered/confirmed safe manifest through the core
/// native-trash cleaner. The cleaner re-runs the guard and identity checks;
/// planning is never treated as authorization to skip the final safety gate.
pub fn execute_safe_cleanup_plan(plan: &SafeCleanupPlan, remover: &dyn Remover) -> CleanupOutcome {
    crate::cleaner::clean(&plan.cleanup_items(), remover)
}

/// Build the only bulk-cleanable set: actionable `Safe` findings that remain
/// inside one requested root, outside configured exclusions, and accepted by
/// the guard at preview time. No frontend gets a weaker selection primitive.
pub fn build_safe_cleanup_plan(scan: &ScanResult, exclusions: &[PathBuf]) -> SafeCleanupPlan {
    let exclusions = normalize_exclusions(exclusions.to_vec());
    let mut plan = SafeCleanupPlan::default();
    for finding in &scan.findings {
        let reason = if !finding.is_actionable() {
            Some(PlanSkipReason::Advisory)
        } else if finding.severity != Severity::Safe
            || finding.policy() != FindingPolicy::DirectSafe
        {
            Some(PlanSkipReason::NotSafe)
        } else if exclusions.iter().any(|root| finding.path.starts_with(root)) {
            Some(PlanSkipReason::Excluded)
        } else {
            None
        };
        if let Some(reason) = reason {
            plan.skipped.push(SkippedCleanup {
                finding: finding.clone(),
                reason,
            });
            continue;
        }

        let Some(root) = owning_root(&scan.roots, &finding.path) else {
            plan.skipped.push(SkippedCleanup {
                finding: finding.clone(),
                reason: PlanSkipReason::OutsideEveryTarget,
            });
            continue;
        };
        if guard::check(&finding.path, root).is_err() {
            plan.skipped.push(SkippedCleanup {
                finding: finding.clone(),
                reason: PlanSkipReason::GuardRefused,
            });
            continue;
        }
        plan.eligible.push(PlannedCleanup {
            finding: finding.clone(),
            scan_root: root.to_path_buf(),
        });
    }
    plan
}

/// Longest configured root that physically contains `path`.
pub fn owning_root<'a>(roots: &'a [PathBuf], path: &Path) -> Option<&'a Path> {
    roots
        .iter()
        .map(PathBuf::as_path)
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.as_os_str().len())
}

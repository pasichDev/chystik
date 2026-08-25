//! Public application-service contract shared by the GUI and CLI.
//!
//! These fixtures exercise the safety semantics above the raw scanner: root
//! ownership, presentation filtering, persisted never-touch paths, and the
//! invariant that a bulk-safe plan can never contain advice or risky data.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use chystik_core::app::{
    build_safe_cleanup_plan, execute_safe_cleanup_plan, explain, filter_and_sort, normalize_roots,
    scan_stream, scan_with_events, AppScanEvent, Explanation, FindingFilter, PlanSkipReason,
    ScanRequest, ScanResult, SortKey,
};
use chystik_core::cleaner::Remover;
use chystik_core::config::{ConfigStore, UserConfig};
use chystik_core::model::{Category, Finding, Severity};

#[derive(Default)]
struct RecordingRemover {
    paths: Mutex<Vec<PathBuf>>,
}

impl Remover for RecordingRemover {
    fn remove(&self, path: &Path) -> Result<(), chystik_core::model::ChystikError> {
        self.paths.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
}

fn finding(path: impl AsRef<Path>, severity: Severity, size_bytes: u64) -> Finding {
    Finding {
        path: path.as_ref().to_path_buf(),
        category: Category::BuildArtifacts,
        severity,
        size_bytes,
        last_used: None,
        mount: None,
        note: "fixture".into(),
        advice: None,
    }
}

#[test]
fn normalizes_roots_to_absolute_non_overlapping_directories() {
    let sandbox = tempfile::tempdir().unwrap();
    let root = sandbox.path().join("root");
    let nested = root.join("nested");
    let sibling = sandbox.path().join("sibling");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();

    let roots = normalize_roots(&[nested, root.clone(), sibling.clone(), root]).unwrap();

    assert_eq!(roots, vec![sandbox.path().join("root"), sibling]);
    assert!(roots.iter().all(|path| path.is_absolute()));
}

#[test]
fn filters_and_sorts_without_changing_the_input() {
    let original = vec![
        finding("/scan/small", Severity::Safe, 10),
        finding("/scan/large", Severity::Safe, 100),
        finding("/scan/risky", Severity::Risky, 1_000),
    ];
    let filtered = filter_and_sort(
        &original,
        &FindingFilter {
            severity: Some(Severity::Safe),
            ..FindingFilter::default()
        },
        SortKey::Size,
    );

    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].path, PathBuf::from("/scan/large"));
    assert_eq!(filtered[1].path, PathBuf::from("/scan/small"));
    assert_eq!(original.len(), 3, "filters must be a presentation view");
}

#[test]
fn safe_cleanup_plan_never_selects_excluded_advisory_or_risky_findings() {
    let sandbox = tempfile::tempdir().unwrap();
    let root = sandbox.path().join("scan");
    let safe = root.join("safe/node_modules");
    let excluded = root.join("excluded/node_modules");
    let risky = root.join("risky/node_modules");
    let advisory = root.join("advisory");
    for path in [&safe, &excluded, &risky, &advisory] {
        std::fs::create_dir_all(path).unwrap();
    }

    let mut advisory_finding = finding(&advisory, Severity::Safe, 400);
    advisory_finding.advice = Some("run package-manager cleanup".into());
    let scan = ScanResult::from_findings(
        vec![
            finding(&safe, Severity::Safe, 100),
            finding(&excluded, Severity::Safe, 200),
            finding(&risky, Severity::Risky, 300),
            advisory_finding,
        ],
        vec![root.clone()],
    );

    let plan = build_safe_cleanup_plan(&scan, std::slice::from_ref(&excluded));

    assert_eq!(plan.eligible.len(), 1);
    assert_eq!(plan.eligible[0].finding.path, safe);
    assert!(plan
        .skipped
        .iter()
        .any(|item| { item.finding.path == excluded && item.reason == PlanSkipReason::Excluded }));
    assert!(plan
        .skipped
        .iter()
        .any(|item| { item.finding.path == risky && item.reason == PlanSkipReason::NotSafe }));
    assert!(plan
        .skipped
        .iter()
        .any(|item| { item.finding.path == advisory && item.reason == PlanSkipReason::Advisory }));
}

#[test]
fn config_persists_consent_and_normalized_exclusions() {
    let sandbox = tempfile::tempdir().unwrap();
    let config_path = sandbox.path().join("chystik.json");
    let store = ConfigStore::at(&config_path);
    let nested = sandbox.path().join("never-touch/nested");
    let parent = sandbox.path().join("never-touch");

    store
        .save(&UserConfig {
            exclusions: vec![nested, parent.clone(), parent.clone()],
            ..UserConfig::default()
        })
        .unwrap();
    store.acknowledge_current_version().unwrap();

    let loaded = store.load().unwrap();
    assert_eq!(loaded.exclusions, vec![parent]);
    assert!(loaded.acknowledges_current_version());
    assert!(config_path.is_file());
}

#[test]
fn streaming_application_scan_applies_the_shared_filter_without_collecting_results() {
    let sandbox = tempfile::tempdir().unwrap();
    let project = sandbox.path().join("project");
    let modules = project.join("node_modules/left-pad");
    std::fs::create_dir_all(&modules).unwrap();
    std::fs::write(project.join("package.json"), "{}").unwrap();
    std::fs::write(project.join("package-lock.json"), "{}").unwrap();
    std::fs::write(modules.join("index.js"), "x".repeat(2048)).unwrap();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let callback_seen = seen.clone();
    let summary = scan_stream(
        &ScanRequest {
            roots: vec![sandbox.path().to_path_buf()],
            filter: FindingFilter {
                severity: Some(Severity::Moderate),
                ..FindingFilter::default()
            },
            min_finding_bytes: 0,
            ..ScanRequest::default()
        },
        &Arc::new(AtomicBool::new(false)),
        move |event| {
            if let AppScanEvent::Finding(finding) = event {
                callback_seen.lock().unwrap().push(finding);
            }
        },
    )
    .unwrap();

    let findings = seen.lock().unwrap();
    assert_eq!(summary.findings, findings.len() as u64);
    assert!(findings
        .iter()
        .all(|finding| finding.severity == Severity::Moderate));
    assert!(findings
        .iter()
        .any(|finding| finding.path.ends_with("node_modules")));
}

#[test]
fn complete_application_scan_exposes_lifecycle_events_and_a_sorted_final_result() {
    let sandbox = tempfile::tempdir().unwrap();
    let project = sandbox.path().join("project");
    let modules = project.join("node_modules/left-pad");
    std::fs::create_dir_all(&modules).unwrap();
    std::fs::write(project.join("package.json"), "{}").unwrap();
    std::fs::write(project.join("package-lock.json"), "{}").unwrap();
    std::fs::write(modules.join("index.js"), "x".repeat(2048)).unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let callback_events = events.clone();
    let result = scan_with_events(
        &ScanRequest {
            roots: vec![sandbox.path().to_path_buf()],
            min_finding_bytes: 0,
            ..ScanRequest::default()
        },
        &Arc::new(AtomicBool::new(false)),
        move |event| callback_events.lock().unwrap().push(event),
    )
    .unwrap();

    assert_eq!(result.findings.len(), 1);
    let events = events.lock().unwrap();
    assert!(matches!(events.first(), Some(AppScanEvent::Started { .. })));
    assert!(matches!(events.last(), Some(AppScanEvent::Finished(_))));
}

#[test]
fn explain_returns_the_same_rule_metadata_as_a_scan_for_a_known_path() {
    let fixture = tempfile::tempdir().unwrap();
    let project = fixture.path().join("project");
    let modules = project.join("node_modules");
    std::fs::create_dir_all(&modules).unwrap();
    std::fs::write(project.join("package.json"), "{}").unwrap();
    std::fs::write(project.join("package-lock.json"), "{}").unwrap();

    let result = explain(&modules).unwrap();

    assert!(matches!(
        result,
        Explanation::Recognized {
            category: Category::BuildArtifacts,
            severity: Severity::Moderate,
            ..
        }
    ));
}

#[test]
fn execution_receives_only_preplanned_safe_items_and_reuses_core_guard() {
    let sandbox = tempfile::tempdir().unwrap();
    let root = sandbox.path().join("scan");
    let safe = root.join("safe-cache");
    let risky = root.join("risky-cache");
    std::fs::create_dir_all(&safe).unwrap();
    std::fs::create_dir_all(&risky).unwrap();
    let scan = ScanResult::from_findings(
        vec![
            finding(&safe, Severity::Safe, 10),
            finding(&risky, Severity::Risky, 20),
        ],
        vec![root],
    );
    let plan = build_safe_cleanup_plan(&scan, &[]);
    let remover = RecordingRemover::default();

    let outcome = execute_safe_cleanup_plan(&plan, &remover);

    assert_eq!(outcome.removed, vec![safe.clone()]);
    assert_eq!(*remover.paths.lock().unwrap(), vec![safe]);
    assert_eq!(
        plan.skipped.len(),
        1,
        "risky item stayed outside the remover"
    );
}

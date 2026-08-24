//! Real-machine smoke tests.
//!
//! These touch the real `$HOME` and the real Trash, so they are `#[ignore]`d
//! by default — they assert against one particular machine and cannot run in
//! CI. The deletion FLOW itself is covered unconditionally in
//! `tests/deletion.rs`, which substitutes the trash; what remains here is
//! only the part that needs a real desktop.
//!
//! Run them explicitly once per machine:
//!
//! ```sh
//! source ~/.cargo/env
//! cargo test -p chystik-core --test smoke -- --ignored --nocapture
//! ```
//!
//! Expectations for this machine (KDE neon, /home/pasich):
//! - `.ollama/models` ≈ 8.7 GB → AiModels / Risky
//! - `.cache/go-build`, `go/pkg/mod` ≈ 6 GB combined → PackageCaches / Safe
//! - `repo/*/node_modules` → BuildArtifacts / Moderate

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use chystik_core::model::{Category, Finding, Severity};
use chystik_core::scanner;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME must be set for the smoke test")
}

fn find<'a>(findings: &'a [Finding], suffix: &str) -> Option<&'a Finding> {
    findings.iter().find(|f| f.path.ends_with(suffix))
}

/// Scan the real `$HOME` (read-only) and check known heavy hitters, then
/// export a JSON report and parse it back.
#[test]
#[ignore = "touches the real $HOME; run manually with --ignored"]
fn real_home_scan_finds_known_items_and_exports_json() {
    let root = home();
    let (tx, _rx) = mpsc::channel();
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let started = std::time::Instant::now();
    let findings = scanner::scan(&root, &scanner::ScanOptions::default(), tx, &cancel)
        .expect("scan of real $HOME should succeed");
    println!(
        "scan finished in {:.1}s — {} findings, {} total",
        started.elapsed().as_secs_f32(),
        findings.len(),
        findings.iter().map(|f| f.size_bytes).sum::<u64>(),
    );

    let ollama = find(&findings, ".ollama/models").expect(".ollama/models must be found");
    assert_eq!(ollama.category, Category::AiModels);
    assert_eq!(ollama.severity, Severity::Risky);
    println!(".ollama/models: {}", ollama.size_bytes);

    for go_cache in [".cache/go-build", "go/pkg/mod"] {
        let f = find(&findings, go_cache).unwrap_or_else(|| panic!("{go_cache} must be found"));
        assert_eq!(f.category, Category::PackageCaches, "{go_cache}");
        assert_eq!(f.severity, Severity::Safe, "{go_cache}");
        println!("{go_cache}: {}", f.size_bytes);
    }

    let node_modules: Vec<&Finding> = findings
        .iter()
        .filter(|f| {
            f.path
                .file_name()
                .map(|n| n == "node_modules")
                .unwrap_or(false)
        })
        .collect();
    assert!(
        !node_modules.is_empty(),
        "at least one node_modules must be found in $HOME"
    );
    assert!(
        node_modules
            .iter()
            .all(|f| f.category == Category::BuildArtifacts && f.severity == Severity::Moderate),
        "node_modules rows must be BuildArtifacts / Moderate"
    );
    for f in &node_modules {
        println!("node_modules: {} ({})", f.path.display(), f.size_bytes);
    }

    // Cross-check one row's size against `du -sb` (allocated blocks vs apparent
    // size differ, so allow generous tolerance).
    let checked = ollama;
    let du = std::process::Command::new("du")
        .arg("-sb")
        .arg(&checked.path)
        .output()
        .expect("du available");
    let du_out = String::from_utf8_lossy(&du.stdout);
    let du_bytes: u64 = du_out
        .split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
        .expect("du -sb prints a byte count");
    println!(
        "cross-check {}: scanner={} du={}",
        checked.path.display(),
        checked.size_bytes,
        du_bytes
    );
    let diff_pct = checked.size_bytes.abs_diff(du_bytes) as f64 / du_bytes.max(1) as f64 * 100.0;
    assert!(
        diff_pct < 10.0,
        "scanner size differs from du -sb by {diff_pct:.1}%"
    );

    // JSON export round-trip.
    let out_dir = tempfile::tempdir().unwrap();
    let report_path = out_dir.path().join("chystik-smoke.json");
    chystik_core::report::export_json(&findings, &report_path).expect("export_json ok");
    let raw = std::fs::read_to_string(&report_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("report is valid JSON");
    assert_eq!(
        parsed["summaries"][0]["category"],
        serde_json::Value::String(
            chystik_core::report::summarize(&findings)[0]
                .category
                .label()
                .replace(' ', "_")
                .to_lowercase()
        )
    );
    assert_eq!(parsed["findings"].as_array().unwrap().len(), findings.len());
    println!("JSON export OK: {}", report_path.display());
}

/// The one thing `tests/deletion.rs` cannot cover: that the real XDG trash
/// accepts what the flow hands it. Everything around it — guard checks,
/// identity re-check, tallying — is tested there against a fake.
#[test]
#[ignore = "moves files to the real Trash; run manually with --ignored"]
fn delete_flow_moves_demo_project_to_trash() {
    let base = tempfile::tempdir().unwrap();
    let proj = base.path().join("demo-proj");
    let nm = proj.join("node_modules");
    std::fs::create_dir_all(nm.join("left-pad")).unwrap();
    std::fs::write(proj.join("package.json"), "{}").unwrap();
    std::fs::write(nm.join("left-pad/index.js"), "let pad='x'.repeat(1024);").unwrap();

    // Scan the temp base: node_modules is classified via its marker file.
    let (tx, _rx) = mpsc::channel();
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let findings =
        scanner::scan(base.path(), &scanner::ScanOptions::default(), tx, &cancel).unwrap();
    let finding = find(&findings, "node_modules").expect("demo node_modules found");
    assert_eq!(finding.category, Category::BuildArtifacts);

    // The production flow end to end, against the real trash.
    let outcome = chystik_core::cleaner::clean(
        &[chystik_core::cleaner::CleanupItem {
            path: finding.path.clone(),
            size_bytes: finding.size_bytes,
            scan_root: Some(base.path().to_path_buf()),
        }],
        &chystik_core::cleaner::SystemTrash,
    );
    assert_eq!(outcome.removed_count(), 1, "{:?}", outcome.skipped);
    assert!(
        !finding.path.exists(),
        "deleted item must leave the filesystem"
    );
}

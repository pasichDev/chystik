//! Report aggregation and JSON export — TO IMPLEMENT (core-engine agent).

use crate::model::{Category, Finding};
use chrono::Utc;
use serde::Serialize;
use std::path::Path;

/// Aggregate totals grouped by category.
#[derive(Debug, Clone, Serialize)]
pub struct CategorySummary {
    pub category: Category,
    pub count: usize,
    pub total_bytes: u64,
}

/// Build per-category summaries sorted by total size descending.
pub fn summarize(findings: &[Finding]) -> Vec<CategorySummary> {
    let mut out: Vec<CategorySummary> = Category::all()
        .into_iter()
        .filter_map(|category| {
            let matching: Vec<&Finding> =
                findings.iter().filter(|f| f.category == category).collect();
            (!matching.is_empty()).then(|| CategorySummary {
                category,
                count: matching.len(),
                total_bytes: matching.iter().map(|f| f.size_bytes).sum(),
            })
        })
        .collect();
    out.sort_by_key(|summary| std::cmp::Reverse(summary.total_bytes));
    out
}

#[derive(Serialize)]
struct Report<'a> {
    generated_at: chrono::DateTime<Utc>,
    summaries: Vec<CategorySummary>,
    findings: &'a [Finding],
}

/// Write findings + summaries to a JSON file.
pub fn export_json(findings: &[Finding], path: &Path) -> Result<(), std::io::Error> {
    let report = Report {
        generated_at: Utc::now(),
        summaries: summarize(findings),
        findings,
    };
    // Written to a sibling temporary and renamed into place. `rename(2)`
    // within one directory is atomic, so a crash or a full disk leaves the
    // previous report intact rather than a half-written one.
    let directory = path.parent().unwrap_or(Path::new("."));
    let temporary = directory.join(format!(
        ".{}.partial",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "chystik-report".to_owned())
    ));

    let write = (|| -> Result<(), std::io::Error> {
        let file = std::fs::File::create(&temporary)?;
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, &report)?;
        std::io::Write::flush(&mut writer)?;
        // Durable before the rename, or a crash can leave an empty file
        // under the final name.
        writer.get_ref().sync_all()
    })();

    if let Err(e) = write {
        let _ = std::fs::remove_file(&temporary);
        return Err(e);
    }
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed export must not replace a good report with a broken one.
    #[test]
    fn export_leaves_no_partial_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("report.json");

        export_json(&[], &target).expect("first export");
        let first = std::fs::read_to_string(&target).unwrap();
        assert!(first.contains("generated_at"));

        // Nothing but the report should exist: no leftover temporary.
        let strays: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "report.json")
            .collect();
        assert!(strays.is_empty(), "left behind {strays:?}");
    }

    /// Exporting over an existing report replaces it wholesale.
    #[test]
    fn export_replaces_an_existing_report_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("report.json");
        std::fs::write(&target, "PREVIOUS CONTENT").unwrap();

        export_json(&[], &target).expect("export");
        let after = std::fs::read_to_string(&target).unwrap();
        assert!(!after.contains("PREVIOUS"), "the old file survived");
        assert!(after.contains("generated_at"));
    }

    /// An unwritable directory reports an error rather than truncating.
    #[test]
    fn export_into_a_missing_directory_fails_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("no-such-dir/report.json");
        assert!(export_json(&[], &target).is_err());
    }
    use crate::model::{Category, Finding, Severity};
    use std::path::PathBuf;

    fn f(cat: Category, size: u64) -> Finding {
        Finding {
            path: PathBuf::from("/tmp/x"),
            category: cat,
            severity: Severity::Safe,
            size_bytes: size,
            last_used: None,
            mount: None,
            note: String::new(),
            advice: None,
            provenance: None,
        }
    }

    #[test]
    fn summarize_aggregates_and_sorts_desc() {
        let findings = vec![
            f(Category::PackageCaches, 100),
            f(Category::AiModels, 900),
            f(Category::PackageCaches, 300),
        ];
        let s = summarize(&findings);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].category, Category::AiModels);
        assert_eq!(s[0].total_bytes, 900);
        assert_eq!(s[1].category, Category::PackageCaches);
        assert_eq!(s[1].total_bytes, 400);
        assert_eq!(s[1].count, 2);
    }

    #[test]
    fn export_json_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("r.json");
        export_json(&[f(Category::BrowserSystem, 42)], &out).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert!(v.get("generated_at").is_some());
        assert_eq!(v["summaries"][0]["category"], "browser_system");
        assert_eq!(v["findings"][0]["size_bytes"], 42);
    }
}

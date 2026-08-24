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
    let file = std::fs::File::create(path)?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    std::io::Write::flush(&mut writer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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

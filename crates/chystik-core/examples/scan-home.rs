//! Smoke-scan the real `$HOME` and print findings grouped by category.
//! Read-only; useful to eyeball rule coverage on a real machine:
//!
//! ```sh
//! source ~/.cargo/env
//! cargo run -p chystik-core --example scan-home
//! ```

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use chystik_core::model::Category;
use chystik_core::scanner;

fn human(bytes: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < units.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", units[u])
    }
}

fn main() {
    let home = chystik_core::platform::current().app_paths().home_dir;
    let (tx, _rx) = mpsc::channel();
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let started = std::time::Instant::now();
    let findings =
        scanner::scan(&home, &scanner::ScanOptions::default(), tx, &cancel).expect("scan ok");

    println!(
        "scanned {} in {:.1}s — {} findings, {} total\n",
        home.display(),
        started.elapsed().as_secs_f32(),
        findings.len(),
        human(findings.iter().map(|f| f.size_bytes).sum()),
    );

    let mut groups: HashMap<Category, Vec<&chystik_core::Finding>> = HashMap::new();
    for f in &findings {
        groups.entry(f.category).or_default().push(f);
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by_key(|(cat, _)| cat.label().to_string());

    for (cat, items) in groups {
        let total: u64 = items.iter().map(|f| f.size_bytes).sum();
        println!(
            "== {} — {} item(s), {} ==",
            cat.label(),
            items.len(),
            human(total)
        );
        let mut sorted: Vec<_> = items.clone();
        sorted.sort_by_key(|f| std::cmp::Reverse(f.size_bytes));
        for f in sorted.iter().take(8) {
            println!(
                "  {:>9}  [{:?}] {}",
                human(f.size_bytes),
                f.severity,
                f.path.display()
            );
        }
        if sorted.len() > 8 {
            println!("  … and {} more", sorted.len() - 8);
        }
        println!();
    }
}

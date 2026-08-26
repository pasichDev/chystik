#[path = "src/rules/catalog_schema.rs"]
mod catalog_schema;

use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let catalog_dir = manifest_dir.join("rules/catalog");
    println!("cargo:rerun-if-changed={}", catalog_dir.display());

    let mut entries: Vec<_> = fs::read_dir(&catalog_dir)
        .expect("read catalog directory")
        .map(|entry| entry.expect("read catalog entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    entries.sort();
    if entries.is_empty() {
        panic!("catalog directory contains no TOML files");
    }

    let mut rules = Vec::new();
    for path in &entries {
        println!("cargo:rerun-if-changed={}", path.display());
        let source = fs::read_to_string(path).expect("read catalog source");
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("catalog name");
        let parsed = catalog_schema::parse_catalog(name, &source)
            .unwrap_or_else(|error| panic!("catalog validation failed: {error}"));
        rules.extend(parsed);
    }
    catalog_schema::validate_catalog(&rules)
        .unwrap_or_else(|error| panic!("catalog validation failed: {error}"));

    let mut generated = String::from("pub const CATALOG_SOURCES: &[(&str, &str)] = &[\n");
    for path in entries {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("catalog name");
        generated.push_str(&format!(
            "    (\"{name}\", include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/rules/catalog/{name}\"))),\n"
        ));
    }
    generated.push_str("];\n");
    fs::write(
        PathBuf::from(std::env::var("OUT_DIR").expect("out dir")).join("catalog_sources.rs"),
        generated,
    )
    .expect("write generated catalog source list");
}

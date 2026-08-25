#[path = "src/args.rs"]
mod args;

use clap::CommandFactory;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=src/args.rs");
    let out_dir = std::path::PathBuf::from(
        std::env::var_os("OUT_DIR").ok_or("Cargo did not provide OUT_DIR")?,
    );
    let man = clap_mangen::Man::new(args::Cli::command());
    let mut rendered = Vec::new();
    man.render(&mut rendered)?;
    std::fs::write(out_dir.join("chystik.1"), rendered)?;
    Ok(())
}

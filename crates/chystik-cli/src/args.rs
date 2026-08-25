use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum, ValueHint};

/// Safe, scriptable disk cleanup. Every cleanup goes through native Trash.
#[derive(Debug, Parser)]
#[command(
    name = "chystik",
    version,
    about,
    arg_required_else_help = true,
    after_help = "EXAMPLES:\n  chystik scan --safe ~/work\n  chystik report ~/work --format jsonl > chystik-report.jsonl\n  chystik clean ~/work --safe --dry-run\n\nRun `chystik <command> --help` for command-specific options and examples."
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Scan roots and show recognized reclaimable space.
    Scan(ScanArgs),
    /// Explain how Chystik classifies one path.
    #[command(after_help = "EXAMPLE:\n  chystik explain ~/.cache/go-build")]
    Explain { path: PathBuf },
    /// Preview or move explicitly safe findings to native Trash.
    Clean(CleanArgs),
    /// Emit a report for automation.
    Report(ReportArgs),
    /// Inspect or reset local Chystik policy.
    Config(ConfigArgs),
    /// Print a completion script to stdout.
    #[command(
        after_help = "EXAMPLES:\n  source <(chystik completion bash)\n  chystik completion fish > ~/.config/fish/completions/chystik.fish"
    )]
    Completion { shell: CompletionShell },
    /// Print the Chystik version.
    Version,
}

/// Inputs shared by scan, report, and clean. Keeping the parser shape here
/// makes each frontend request the same core application service.
#[derive(Debug, Clone, Args)]
pub(crate) struct SelectionArgs {
    /// Directories to scan. Defaults to the current directory.
    #[arg(value_name = "ROOT", value_hint = ValueHint::DirPath)]
    pub(crate) roots: Vec<PathBuf>,
    /// Add a scan root without relying on positional argument ordering.
    #[arg(long, value_name = "ROOT", value_hint = ValueHint::DirPath)]
    pub(crate) root: Vec<PathBuf>,
    /// Limit displayed findings by safety severity.
    #[arg(long, value_enum)]
    pub(crate) severity: Option<SeverityArg>,
    /// Limit findings to one stable category name, for example `package_caches`.
    #[arg(long, value_name = "CATEGORY")]
    pub(crate) category: Option<String>,
    /// Ignore findings below this size. Accepts bytes or KiB/MiB/GiB suffixes.
    #[arg(long, value_name = "SIZE", value_parser = parse_size)]
    pub(crate) min_size: Option<u64>,
    /// Include system-space advice that Chystik will report but never clean.
    #[arg(long)]
    pub(crate) include_advisories: bool,
    /// Never scan or clean this root; repeat for more than one exclusion.
    #[arg(long, value_name = "PATH", value_hint = ValueHint::AnyPath)]
    pub(crate) exclude: Vec<PathBuf>,
    /// Choose presentation order. JSONL deliberately streams discovery order.
    #[arg(long, value_enum, default_value_t = SortArg::Size)]
    pub(crate) sort: SortArg,
}

#[derive(Debug, Args)]
#[command(
    long_about = "Scan one or more directories and show recognized reclaimable space. The default is read-only human output; no cleanup is ever started by scan.",
    after_help = "EXAMPLES:\n  chystik scan --safe ~/work\n  chystik scan ~/work --no-tui\n  chystik scan ~/work --category build_artifacts --format json\n  chystik scan ~/work --format jsonl\n\nInteractive terminals open a live table with a loader. Use --no-tui or --quiet for line output. JSON writes one document to stdout; JSONL streams progress and findings on stdout."
)]
pub(crate) struct ScanArgs {
    #[command(flatten)]
    pub(crate) selection: SelectionArgs,
    /// Show only findings that regenerate automatically.
    #[arg(long, conflicts_with = "severity")]
    pub(crate) safe: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub(crate) format: OutputFormat,
    #[arg(long)]
    pub(crate) no_color: bool,
    /// Use line-based progress instead of the interactive terminal interface.
    #[arg(long)]
    pub(crate) no_tui: bool,
    #[arg(long, conflicts_with = "verbose")]
    pub(crate) quiet: bool,
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) verbose: bool,
}

#[derive(Debug, Args)]
#[command(
    long_about = "Create an automation-oriented scan report. This command never starts a GUI or cleanup operation.",
    after_help = "EXAMPLES:\n  chystik report ~/work --format json > report.json\n  chystik report ~/work --format jsonl > report.jsonl\n\nUse --format jsonl for streaming progress and findings. JSON is one final document for consumers that need a complete sorted report."
)]
pub(crate) struct ReportArgs {
    #[command(flatten)]
    pub(crate) selection: SelectionArgs,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Args)]
#[command(
    long_about = "Preview or move only explicitly safe findings to the platform native Trash. Direct deletion is not implemented.",
    after_help = "EXAMPLES:\n  chystik clean ~/work --safe --dry-run\n  chystik clean ~/work --safe --no-tui\n  chystik clean ~/work --safe --interactive\n\nEvery cleanup moves items only to native Trash after an explicit confirmation. --no-tui keeps line progress. --yes remains policy-gated and never bypasses exclusions, advisories, risky findings, or the cleanup guard."
)]
pub(crate) struct CleanArgs {
    #[command(flatten)]
    pub(crate) selection: SelectionArgs,
    /// Required acknowledgement that only `safe` findings may be bulk-cleaned.
    #[arg(long, required = true)]
    pub(crate) safe: bool,
    /// Print the manifest but do not ask or modify the filesystem.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Confirm a safe cleanup non-interactively after persisted consent.
    #[arg(long, conflicts_with = "interactive")]
    pub(crate) yes: bool,
    /// Choose every item explicitly from the manifest.
    #[arg(long, conflicts_with = "yes")]
    pub(crate) interactive: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub(crate) format: OutputFormat,
    #[arg(long)]
    pub(crate) no_color: bool,
    /// Use line-based progress instead of the interactive terminal interface.
    #[arg(long)]
    pub(crate) no_tui: bool,
    #[arg(long, conflicts_with = "verbose")]
    pub(crate) quiet: bool,
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) verbose: bool,
}

#[derive(Debug, Args)]
#[command(
    after_help = "EXAMPLES:\n  chystik config show\n  chystik config path\n  chystik config reset"
)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// Print the effective persisted policy as JSON.
    Show,
    /// Print the platform-native configuration file path.
    Path,
    /// Replace stored exclusions and consent with an empty policy.
    Reset,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub(crate) enum SeverityArg {
    Safe,
    Moderate,
    Risky,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub(crate) enum SortArg {
    Size,
    Age,
    Severity,
    Path,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Human,
    Json,
    Jsonl,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

pub(crate) fn parse_size(raw: &str) -> Result<u64, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("size cannot be empty".into());
    }
    let lower = raw.to_ascii_lowercase();
    let suffixes = [
        ("gib", 1024_u64.pow(3)),
        ("gb", 1000_u64.pow(3)),
        ("mib", 1024_u64.pow(2)),
        ("mb", 1000_u64.pow(2)),
        ("kib", 1024),
        ("kb", 1000),
        ("b", 1),
    ];
    let (number, multiplier) = suffixes
        .iter()
        .find_map(|(suffix, multiplier)| {
            lower
                .strip_suffix(suffix)
                .map(|number| (number, *multiplier))
        })
        .unwrap_or((lower.as_str(), 1));
    let number = number.trim();
    let parsed: u64 = number
        .parse()
        .map_err(|_| format!("invalid size {raw:?}; use bytes or KiB/MiB/GiB"))?;
    parsed
        .checked_mul(multiplier)
        .ok_or_else(|| format!("size {raw:?} is too large"))
}

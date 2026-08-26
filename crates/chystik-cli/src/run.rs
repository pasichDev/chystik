use std::io::{self, IsTerminal, Write};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use chrono::{SecondsFormat, Utc};
use clap::CommandFactory;
use clap_complete::aot::{generate, Generator};
use serde::Serialize;

use chystik_core::app::{
    self, AppScanEvent, Explanation, FindingFilter, ScanRequest, ScanResult, SortKey,
    MACHINE_SCHEMA_VERSION,
};
use chystik_core::cleaner::{CleanupOutcome, SkipReason, SystemTrash};
use chystik_core::config::ConfigStore;
use chystik_core::model::{Category, ChystikError, Severity};

use crate::args::{
    CleanArgs, Cli, Command, CompletionShell, ConfigCommand, OutputFormat, ReportArgs, ScanArgs,
    SelectionArgs, SeverityArg, SortArg,
};
use crate::tui::{self, CompletionMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitCode {
    Success = 0,
    Operational = 1,
    InvalidInput = 2,
    Cancelled = 3,
    PolicyRefused = 4,
    Interrupted = 5,
}

impl ExitCode {
    pub(crate) const fn as_i32(self) -> i32 {
        self as i32
    }
}

pub(crate) fn run(cli: Cli) -> Result<ExitCode, CliError> {
    let cancel = Arc::new(AtomicBool::new(false));
    let signal_cancel = cancel.clone();
    ctrlc::set_handler(move || {
        signal_cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    })
    .map_err(|error| CliError::operational(format!("install interrupt handler: {error}")))?;
    run_with_cancel(cli, &cancel)
}

fn run_with_cancel(cli: Cli, cancel: &Arc<AtomicBool>) -> Result<ExitCode, CliError> {
    match cli.command {
        Command::Scan(args) => {
            let machine_output = is_machine_format(args.format);
            run_scan(args, cancel).map_err(|error| error.for_machine_output(machine_output))
        }
        Command::Report(args) => {
            let machine_output = is_machine_format(args.format);
            run_report(args, cancel).map_err(|error| error.for_machine_output(machine_output))
        }
        Command::Explain { path } => run_explain(&path),
        Command::Clean(args) => {
            let machine_output = is_machine_format(args.format);
            run_clean(args, cancel).map_err(|error| error.for_machine_output(machine_output))
        }
        Command::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::Success)
        }
        Command::Completion { shell } => {
            let mut command = Cli::command();
            match shell {
                CompletionShell::Bash => {
                    generate_completion(clap_complete::aot::Bash, &mut command)
                }
                CompletionShell::Zsh => generate_completion(clap_complete::aot::Zsh, &mut command),
                CompletionShell::Fish => {
                    generate_completion(clap_complete::aot::Fish, &mut command)
                }
                CompletionShell::Powershell => {
                    generate_completion(clap_complete::aot::PowerShell, &mut command)
                }
            }
            Ok(ExitCode::Success)
        }
        Command::Config(args) => {
            let machine_output = matches!(&args.command, ConfigCommand::Show);
            run_config(args.command).map_err(|error| error.for_machine_output(machine_output))
        }
    }
}

const fn is_machine_format(format: OutputFormat) -> bool {
    matches!(format, OutputFormat::Json | OutputFormat::Jsonl)
}

fn run_clean(args: CleanArgs, cancel: &Arc<AtomicBool>) -> Result<ExitCode, CliError> {
    debug_assert!(args.safe, "clap requires --safe for every clean invocation");
    if matches!(args.selection.severity, Some(value) if value != SeverityArg::Safe) {
        return Err(CliError::invalid(
            "clean --safe accepts no --severity or only --severity safe",
        ));
    }
    if args.format == OutputFormat::Jsonl {
        return Err(CliError::invalid(
            "clean supports --format human or --format json; JSONL is scan/report-only",
        ));
    }
    if args.format != OutputFormat::Human && !args.dry_run && !args.yes {
        return Err(CliError::invalid(
            "machine-readable clean needs --dry-run or the explicit --safe --yes action",
        ));
    }

    let store = ConfigStore::default();
    let config = store.load().map_err(config_error)?;
    let mut request = request_from_selection_with_exclusions(&args.selection, &config.exclusions)?;
    // The manifest must include every matching severity so that unsafe data is
    // visibly refused rather than silently disappearing behind a CLI filter.
    request.filter.severity = None;
    let scan = if args.format == OutputFormat::Human {
        scan_with_human_progress(
            &request,
            cancel,
            args.quiet,
            args.no_tui,
            CompletionMode::CloseWhenComplete,
            !args.no_color,
        )?
        .0
    } else {
        app::scan(&request, cancel).map_err(core_error)?
    };
    let plan = app::build_safe_cleanup_plan(&scan, &config.exclusions);

    if args.dry_run {
        write_cleanup_preview(args.format, &plan)?;
        return Ok(ExitCode::Success);
    }
    if plan.eligible.is_empty() {
        write_cleanup_preview(args.format, &plan)?;
        return Ok(ExitCode::PolicyRefused);
    }
    if args.yes {
        if !config.acknowledges_current_version() {
            write_cleanup_preview(args.format, &plan)?;
            return Ok(ExitCode::PolicyRefused);
        }
        return execute_clean_plan(args.format, &plan, cancel);
    }

    // Interactive confirmation is deliberately human-only: emitting prompts
    // alongside JSON would corrupt an automation stream.
    write_human_cleanup_manifest(&plan);
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!("chystik: confirmation requires a terminal; use --dry-run or clean --safe --yes after consent");
        return Ok(ExitCode::Cancelled);
    }
    let selected = if args.interactive {
        select_interactively(&plan, cancel)?
    } else {
        plan.eligible.clone()
    };
    if selected.is_empty() {
        return Ok(if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            ExitCode::Interrupted
        } else {
            ExitCode::Cancelled
        });
    }
    if !confirm_cleanup(
        config.acknowledges_current_version(),
        selected.len(),
        cancel,
    )? {
        return Ok(if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            ExitCode::Interrupted
        } else {
            ExitCode::Cancelled
        });
    }
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(ExitCode::Interrupted);
    }
    store.acknowledge_current_version().map_err(config_error)?;
    let selected_plan = app::SafeCleanupPlan {
        eligible: selected,
        skipped: plan.skipped,
    };
    execute_clean_plan(OutputFormat::Human, &selected_plan, cancel)
}

fn write_cleanup_preview(
    format: OutputFormat,
    plan: &app::SafeCleanupPlan,
) -> Result<(), CliError> {
    match format {
        OutputFormat::Human => {
            write_human_cleanup_manifest(plan);
            Ok(())
        }
        OutputFormat::Json => write_json(&CleanupPreviewDocument {
            metadata: MachineMetadata::now(),
            kind: "cleanup_preview",
            plan,
        }),
        OutputFormat::Jsonl => unreachable!("validated by run_clean"),
    }
}

fn write_human_cleanup_manifest(plan: &app::SafeCleanupPlan) {
    println!(
        "Safe cleanup manifest: {} item(s), {} bytes eligible",
        plan.eligible.len(),
        plan.eligible_bytes()
    );
    for item in &plan.eligible {
        println!(
            "  SAFE {:>12}  {}",
            item.finding.size_bytes,
            item.finding.path.display()
        );
    }
    for item in &plan.skipped {
        println!(
            "  SKIP {:<20} {}",
            plan_skip_reason(item.reason),
            item.finding.path.display()
        );
    }
}

fn select_interactively(
    plan: &app::SafeCleanupPlan,
    cancel: &AtomicBool,
) -> Result<Vec<app::PlannedCleanup>, CliError> {
    let mut selected = Vec::new();
    for item in &plan.eligible {
        print!("Clean {}? [y/N] ", item.finding.path.display());
        io::stdout()
            .flush()
            .map_err(|error| CliError::operational(format!("flush prompt: {error}")))?;
        if read_yes()? {
            selected.push(item.clone());
        }
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(Vec::new());
        }
    }
    Ok(selected)
}

fn confirm_cleanup(
    needs_consent: bool,
    selected_count: usize,
    cancel: &AtomicBool,
) -> Result<bool, CliError> {
    if !needs_consent {
        println!("Chystik moves selected paths only to native Trash; no direct deletion exists.");
    }
    print!("Move {selected_count} selected safe item(s) to native Trash? Type yes to continue: ");
    io::stdout()
        .flush()
        .map_err(|error| CliError::operational(format!("flush confirmation: {error}")))?;
    let confirmed = read_yes()?;
    Ok(confirmed && !cancel.load(std::sync::atomic::Ordering::Relaxed))
}

fn read_yes() -> Result<bool, CliError> {
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|error| CliError::operational(format!("read confirmation: {error}")))?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn execute_clean_plan(
    format: OutputFormat,
    plan: &app::SafeCleanupPlan,
    cancel: &AtomicBool,
) -> Result<ExitCode, CliError> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(ExitCode::Interrupted);
    }
    let outcome = app::execute_safe_cleanup_plan(plan, &SystemTrash);
    let exit = cleanup_exit(&outcome);
    match format {
        OutputFormat::Human => write_human_cleanup_outcome(&outcome),
        OutputFormat::Json => write_json(&CleanupDocument::from((&outcome, plan)))?,
        OutputFormat::Jsonl => unreachable!("validated by run_clean"),
    }
    Ok(exit)
}

fn write_human_cleanup_outcome(outcome: &CleanupOutcome) {
    println!(
        "Moved {} item(s) to native Trash; {} bytes recoverable in Trash.",
        outcome.removed_count(),
        outcome.freed_bytes
    );
    for skipped in &outcome.skipped {
        println!(
            "  SKIP {:<20} {}",
            cleanup_skip_reason(&skipped.reason),
            skipped.path.display()
        );
    }
}

fn cleanup_exit(outcome: &CleanupOutcome) -> ExitCode {
    if outcome.skipped.is_empty() {
        return ExitCode::Success;
    }
    if outcome.removed_count() > 0 {
        return ExitCode::Operational;
    }
    if outcome.skipped.iter().all(|skipped| {
        matches!(
            skipped.reason,
            SkipReason::OutsideEveryTarget
                | SkipReason::Refused
                | SkipReason::Advisory
                | SkipReason::CleanupUnavailable(_)
        )
    }) {
        ExitCode::PolicyRefused
    } else {
        ExitCode::Operational
    }
}

fn plan_skip_reason(reason: app::PlanSkipReason) -> &'static str {
    match reason {
        app::PlanSkipReason::Excluded => "excluded",
        app::PlanSkipReason::Advisory => "advisory",
        app::PlanSkipReason::NotSafe => "not_safe",
        app::PlanSkipReason::OutsideEveryTarget => "outside_target",
        app::PlanSkipReason::GuardRefused => "guard_refused",
    }
}

fn cleanup_skip_reason(reason: &SkipReason) -> &'static str {
    match reason {
        SkipReason::OutsideEveryTarget => "outside_target",
        SkipReason::Refused => "guard_refused",
        SkipReason::Advisory => "advisory",
        SkipReason::CleanupUnavailable(_) => "cleanup_unavailable",
        SkipReason::ChangedUnderUs => "changed_under_us",
        SkipReason::RemoverFailed(_) => "native_trash_failed",
    }
}

fn run_report(args: ReportArgs, cancel: &Arc<AtomicBool>) -> Result<ExitCode, CliError> {
    let request = request_from_selection(&args.selection)?;
    match args.format {
        OutputFormat::Human => {
            return Err(CliError::invalid(
                "report supports only --format json or --format jsonl",
            ));
        }
        OutputFormat::Jsonl => write_jsonl_scan(&request, cancel)?,
        OutputFormat::Json => {
            let result = app::scan(&request, cancel).map_err(core_error)?;
            write_json(&ReportDocument::from(&result))?;
        }
    }
    Ok(ExitCode::Success)
}

fn run_explain(path: &std::path::Path) -> Result<ExitCode, CliError> {
    match app::explain(path).map_err(core_error)? {
        Explanation::Recognized {
            path,
            category,
            severity,
            note,
        } => {
            println!("{}", path.display());
            println!("category: {}", category.as_str());
            println!("severity: {}", severity.as_str());
            println!("note: {note}");
        }
        Explanation::Unrecognized { path } => {
            println!("{}", path.display());
            println!("unrecognized: no registered Chystik rule matches this directory");
        }
    }
    Ok(ExitCode::Success)
}

fn run_scan(args: ScanArgs, cancel: &Arc<AtomicBool>) -> Result<ExitCode, CliError> {
    let mut request = request_from_selection(&args.selection)?;
    if args.safe {
        request.filter.severity = Some(Severity::Safe);
    }
    if args.format == OutputFormat::Jsonl {
        write_jsonl_scan(&request, cancel)?;
        return Ok(ExitCode::Success);
    }
    let (result, used_tui) = match args.format {
        OutputFormat::Human => scan_with_human_progress(
            &request,
            cancel,
            args.quiet,
            args.no_tui || args.verbose,
            CompletionMode::BrowseResults,
            !args.no_color,
        )?,
        OutputFormat::Json => (app::scan(&request, cancel).map_err(core_error)?, false),
        OutputFormat::Jsonl => unreachable!("handled above"),
    };
    match args.format {
        OutputFormat::Human => {
            if !used_tui {
                write_human_scan(&result, args.quiet, args.verbose);
            }
            Ok(())
        }
        OutputFormat::Json => write_json(&ScanDocument::from(&result)),
        OutputFormat::Jsonl => unreachable!("handled above"),
    }?;
    Ok(ExitCode::Success)
}

/// Human output is intentionally split across streams: live scan status is
/// stderr, while the final report remains stdout for piping. JSON and JSONL
/// use their own machine contracts and never instantiate this renderer.
fn scan_with_human_progress(
    request: &ScanRequest,
    cancel: &Arc<AtomicBool>,
    quiet: bool,
    no_tui: bool,
    completion_mode: CompletionMode,
    color: bool,
) -> Result<(ScanResult, bool), CliError> {
    if !quiet && !no_tui && tui::is_available() {
        return tui::run_scan(request.clone(), cancel.clone(), completion_mode, color)
            .map(|result| (result, true))
            .map_err(core_error);
    }
    let progress = Arc::new(HumanProgress::new(!quiet));
    let progress_callback = progress.clone();
    match app::scan_with_events(request, cancel, move |event| {
        progress_callback.observe(event);
    }) {
        Ok(result) => Ok((result, false)),
        Err(error) => {
            if !matches!(&error, ChystikError::Cancelled) {
                progress.failed();
            }
            Err(core_error(error))
        }
    }
}

fn write_jsonl_scan(request: &ScanRequest, cancel: &Arc<AtomicBool>) -> Result<(), CliError> {
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let write_error = Arc::new(Mutex::new(None));
    let callback_stdout = stdout.clone();
    let callback_error = write_error.clone();
    let callback_cancel = cancel.clone();
    let result = app::scan_stream(request, cancel, move |event| {
        if callback_error.lock().unwrap().is_some() {
            return;
        }
        let written = {
            let stdout = callback_stdout.lock().unwrap();
            write_jsonl_event(&event, &mut stdout.lock())
        };
        if let Err(error) = written {
            *callback_error.lock().unwrap() = Some(error.to_string());
            callback_cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
    if let Some(error) = write_error.lock().unwrap().take() {
        return Err(CliError::operational(format!(
            "write JSONL output: {error}"
        )));
    }
    result.map_err(core_error)?;
    Ok(())
}

fn write_jsonl_event(event: &AppScanEvent, output: &mut dyn Write) -> io::Result<()> {
    match event {
        AppScanEvent::Started { root } => write_json_line(
            output,
            &ProgressDocument {
                metadata: MachineMetadata::now(),
                kind: "progress",
                event: "started",
                root: Some(root),
                directories_scanned: None,
            },
        ),
        AppScanEvent::DirectoriesScanned { count } => write_json_line(
            output,
            &ProgressDocument {
                metadata: MachineMetadata::now(),
                kind: "progress",
                event: "directories_scanned",
                root: None,
                directories_scanned: Some(*count),
            },
        ),
        AppScanEvent::Finding(finding) => write_json_line(
            output,
            &FindingDocument {
                metadata: MachineMetadata::now(),
                kind: "finding",
                finding,
            },
        ),
        AppScanEvent::Finished(summary) => write_json_line(
            output,
            &SummaryDocument {
                metadata: MachineMetadata::now(),
                kind: "summary",
                summary,
            },
        ),
        AppScanEvent::Cancelled => write_json_line(
            output,
            &ProgressDocument {
                metadata: MachineMetadata::now(),
                kind: "progress",
                event: "cancelled",
                root: None,
                directories_scanned: None,
            },
        ),
    }
}

fn write_json_line<T: Serialize>(output: &mut dyn Write, value: &T) -> io::Result<()> {
    serde_json::to_writer(&mut *output, value).map_err(io::Error::other)?;
    output.write_all(b"\n")
}

fn run_config(command: ConfigCommand) -> Result<ExitCode, CliError> {
    let store = ConfigStore::default();
    match command {
        ConfigCommand::Show => {
            let config = store.load().map_err(config_error)?;
            write_json(&ConfigDocument {
                metadata: MachineMetadata::now(),
                kind: "config",
                config: &config,
            })
        }
        ConfigCommand::Path => {
            println!("{}", store.path().display());
            Ok(())
        }
        ConfigCommand::Reset => {
            store.reset().map_err(config_error)?;
            println!("Reset policy at {}", store.path().display());
            Ok(())
        }
    }?;
    Ok(ExitCode::Success)
}

fn request_from_selection(args: &SelectionArgs) -> Result<ScanRequest, CliError> {
    let config = ConfigStore::default().load().map_err(config_error)?;
    request_from_selection_with_exclusions(args, &config.exclusions)
}

fn request_from_selection_with_exclusions(
    args: &SelectionArgs,
    persisted_exclusions: &[std::path::PathBuf],
) -> Result<ScanRequest, CliError> {
    let mut roots = args.roots.clone();
    roots.extend(args.root.clone());
    let mut exclude = persisted_exclusions.to_vec();
    exclude.extend(args.exclude.clone());
    Ok(ScanRequest {
        roots,
        filter: FindingFilter {
            category: args.category.as_deref().map(parse_category).transpose()?,
            severity: args.severity.map(severity),
        },
        sort: sort(args.sort),
        min_finding_bytes: args
            .min_size
            .unwrap_or_else(|| ScanRequest::default().min_finding_bytes),
        exclude,
        include_advisories: args.include_advisories,
    })
}

fn parse_category(raw: &str) -> Result<Category, CliError> {
    Category::all()
        .into_iter()
        .find(|category| category.as_str() == raw)
        .ok_or_else(|| {
            let choices = Category::all()
                .iter()
                .map(Category::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            CliError::invalid(format!(
                "unknown category {raw:?}; choose one of: {choices}"
            ))
        })
}

const fn severity(value: SeverityArg) -> Severity {
    match value {
        SeverityArg::Safe => Severity::Safe,
        SeverityArg::Moderate => Severity::Moderate,
        SeverityArg::Risky => Severity::Risky,
    }
}

const fn sort(value: SortArg) -> SortKey {
    match value {
        SortArg::Size => SortKey::Size,
        SortArg::Age => SortKey::Age,
        SortArg::Severity => SortKey::Severity,
        SortArg::Path => SortKey::Path,
    }
}

/// Minimal terminal-safe progress renderer for the human scan path. It uses
/// ordinary flushed lines instead of ANSI control sequences, so it remains
/// readable in terminal emulators, CI logs, and stderr redirections alike.
struct HumanProgress {
    enabled: bool,
    output: Mutex<io::Stderr>,
    next_frame: Mutex<usize>,
}

impl HumanProgress {
    const FRAMES: [&'static str; 4] = ["|", "/", "-", "\\"];

    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            output: Mutex::new(io::stderr()),
            next_frame: Mutex::new(0),
        }
    }

    fn observe(&self, event: AppScanEvent) {
        if !self.enabled {
            return;
        }
        match event {
            AppScanEvent::Started { root } => self.write_status(&format!(
                "Scanning {} [{}] started; press Ctrl-C to cancel.",
                root.display(),
                self.frame(),
            )),
            AppScanEvent::DirectoriesScanned { count } => self.write_status(&format!(
                "Scanning [{}] {count} directories visited…",
                self.frame(),
            )),
            AppScanEvent::Finished(summary) => self.write_status(&format!(
                "Scan complete: {} finding(s), {} bytes reclaimable.",
                summary.findings, summary.total_bytes
            )),
            AppScanEvent::Cancelled => self.write_status("Scan interrupted."),
            AppScanEvent::Finding(_) => {}
        }
    }

    fn failed(&self) {
        if self.enabled {
            self.write_status("Scan stopped before completion.");
        }
    }

    fn frame(&self) -> &'static str {
        let Ok(mut next_frame) = self.next_frame.lock() else {
            return Self::FRAMES[0];
        };
        let frame = Self::FRAMES[*next_frame % Self::FRAMES.len()];
        *next_frame += 1;
        frame
    }

    fn write_status(&self, status: &str) {
        let Ok(mut output) = self.output.lock() else {
            return;
        };
        let _ = writeln!(output, "{status}");
        let _ = output.flush();
    }
}

fn write_human_scan(result: &ScanResult, quiet: bool, verbose: bool) {
    if !quiet {
        println!(
            "{} finding(s), {} bytes across {} root(s)",
            result.findings.len(),
            result
                .findings
                .iter()
                .map(|finding| finding.size_bytes)
                .sum::<u64>(),
            result.roots.len()
        );
    }
    for finding in &result.findings {
        println!(
            "{:>12}  {:<8}  {:<18}  {}",
            finding.size_bytes,
            finding.severity.as_str(),
            finding.category.as_str(),
            finding.path.display()
        );
        if verbose {
            println!("              {}", finding.note);
            println!("              policy: {}", finding.policy().as_str());
            if let Some(provenance) = &finding.provenance {
                println!("              rule: {}", provenance.rule_id);
                println!("              recovery: {}", provenance.recovery_cost);
                println!("              source: {}", provenance.source_url);
            }
            if let Some(advice) = &finding.advice {
                println!("              advice: {advice}");
            }
        }
    }
}

fn write_json<T: Serialize>(value: &T) -> Result<(), CliError> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, value)
        .map_err(|error| CliError::operational(format!("write JSON output: {error}")))?;
    output
        .write_all(b"\n")
        .map_err(|error| CliError::operational(format!("write JSON output: {error}")))
}

/// The common, versioned context on every machine-readable document. A
/// timestamp belongs to the presentation boundary: the deterministic core
/// neither needs a clock nor knows which host rendered its findings.
#[derive(Serialize)]
struct MachineMetadata {
    schema_version: u32,
    chystik_version: &'static str,
    platform: &'static str,
    generated_at: String,
}

impl MachineMetadata {
    fn now() -> Self {
        Self {
            schema_version: MACHINE_SCHEMA_VERSION,
            chystik_version: env!("CARGO_PKG_VERSION"),
            platform: std::env::consts::OS,
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        }
    }
}

fn generate_completion<G: Generator>(generator: G, command: &mut clap::Command) {
    generate(generator, command, "chystik", &mut io::stdout());
}

#[derive(Serialize)]
struct ScanDocument<'a> {
    #[serde(flatten)]
    metadata: MachineMetadata,
    kind: &'static str,
    roots: &'a [std::path::PathBuf],
    findings: &'a [chystik_core::model::Finding],
    summary: ScanSummaryDocument<'a>,
}

#[derive(Serialize)]
struct ScanSummaryDocument<'a> {
    finding_count: usize,
    total_bytes: u64,
    categories: &'a [chystik_core::report::CategorySummary],
}

#[derive(Serialize)]
struct ProgressDocument<'a> {
    #[serde(flatten)]
    metadata: MachineMetadata,
    kind: &'static str,
    event: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<&'a std::path::PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    directories_scanned: Option<u64>,
}

#[derive(Serialize)]
struct FindingDocument<'a> {
    #[serde(flatten)]
    metadata: MachineMetadata,
    kind: &'static str,
    finding: &'a chystik_core::model::Finding,
}

#[derive(Serialize)]
struct SummaryDocument<'a> {
    #[serde(flatten)]
    metadata: MachineMetadata,
    kind: &'static str,
    summary: &'a chystik_core::app::AppScanSummary,
}

#[derive(Serialize)]
struct CleanupPreviewDocument<'a> {
    #[serde(flatten)]
    metadata: MachineMetadata,
    kind: &'static str,
    plan: &'a app::SafeCleanupPlan,
}

#[derive(Serialize)]
struct CleanupDocument<'a> {
    #[serde(flatten)]
    metadata: MachineMetadata,
    kind: &'static str,
    plan: &'a app::SafeCleanupPlan,
    outcome: CleanupOutcomeDocument<'a>,
}

impl<'a> From<(&'a CleanupOutcome, &'a app::SafeCleanupPlan)> for CleanupDocument<'a> {
    fn from((outcome, plan): (&'a CleanupOutcome, &'a app::SafeCleanupPlan)) -> Self {
        Self {
            metadata: MachineMetadata::now(),
            kind: "cleanup",
            plan,
            outcome: CleanupOutcomeDocument {
                removed: &outcome.removed,
                freed_bytes: outcome.freed_bytes,
                skipped: outcome
                    .skipped
                    .iter()
                    .map(|skipped| CleanupSkippedDocument {
                        path: &skipped.path,
                        reason: cleanup_skip_reason(&skipped.reason),
                    })
                    .collect(),
            },
        }
    }
}

#[derive(Serialize)]
struct CleanupOutcomeDocument<'a> {
    removed: &'a [std::path::PathBuf],
    freed_bytes: u64,
    skipped: Vec<CleanupSkippedDocument<'a>>,
}

#[derive(Serialize)]
struct CleanupSkippedDocument<'a> {
    path: &'a std::path::PathBuf,
    reason: &'static str,
}

#[derive(Serialize)]
struct ConfigDocument<'a> {
    #[serde(flatten)]
    metadata: MachineMetadata,
    kind: &'static str,
    config: &'a chystik_core::config::UserConfig,
}

impl<'a> From<&'a ScanResult> for ScanDocument<'a> {
    fn from(result: &'a ScanResult) -> Self {
        Self {
            metadata: MachineMetadata::now(),
            kind: "scan",
            roots: &result.roots,
            findings: &result.findings,
            summary: ScanSummaryDocument {
                finding_count: result.findings.len(),
                total_bytes: result
                    .findings
                    .iter()
                    .map(|finding| finding.size_bytes)
                    .sum(),
                categories: &result.summaries,
            },
        }
    }
}

#[derive(Serialize)]
struct ReportDocument<'a> {
    #[serde(flatten)]
    metadata: MachineMetadata,
    kind: &'static str,
    roots: &'a [std::path::PathBuf],
    findings: &'a [chystik_core::model::Finding],
    summary: ScanSummaryDocument<'a>,
}

impl<'a> From<&'a ScanResult> for ReportDocument<'a> {
    fn from(result: &'a ScanResult) -> Self {
        Self {
            metadata: MachineMetadata::now(),
            kind: "report",
            roots: &result.roots,
            findings: &result.findings,
            summary: ScanSummaryDocument {
                finding_count: result.findings.len(),
                total_bytes: result
                    .findings
                    .iter()
                    .map(|finding| finding.size_bytes)
                    .sum(),
                categories: &result.summaries,
            },
        }
    }
}

#[derive(Debug)]
pub(crate) struct CliError {
    pub(crate) code: ExitCode,
    pub(crate) message: String,
    pub(crate) machine_output: bool,
}

impl CliError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: ExitCode::InvalidInput,
            message: message.into(),
            machine_output: false,
        }
    }

    fn operational(message: impl Into<String>) -> Self {
        Self {
            code: ExitCode::Operational,
            message: message.into(),
            machine_output: false,
        }
    }

    fn for_machine_output(mut self, machine_output: bool) -> Self {
        self.machine_output |= machine_output;
        self
    }
}

fn core_error(error: ChystikError) -> CliError {
    let code = match error {
        ChystikError::InvalidInput(_) => ExitCode::InvalidInput,
        ChystikError::Cancelled => ExitCode::Interrupted,
        ChystikError::ProtectedPath(_) | ChystikError::Io(_) => ExitCode::Operational,
    };
    CliError {
        code,
        message: error.to_string(),
        machine_output: false,
    }
}

fn config_error(error: chystik_core::config::ConfigError) -> CliError {
    CliError {
        code: ExitCode::InvalidInput,
        message: error.to_string(),
        machine_output: false,
    }
}

#[derive(Serialize)]
struct ErrorDocument<'a> {
    #[serde(flatten)]
    metadata: MachineMetadata,
    kind: &'static str,
    exit_code: i32,
    message: &'a str,
}

/// Keep machine failures parseable too. This deliberately uses stderr: a
/// failed JSON command must never turn a partially written stdout payload
/// into a misleading success document.
pub(crate) fn write_machine_error(exit_code: i32, message: &str) {
    let stderr = io::stderr();
    let mut output = stderr.lock();
    let document = ErrorDocument {
        metadata: MachineMetadata::now(),
        kind: "error",
        exit_code,
        message,
    };
    if write_json_line(&mut output, &document).is_err() {
        let _ = writeln!(output, "chystik: {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chystik_core::cleaner::Skipped;
    use clap::Parser;

    #[test]
    fn cleanup_exit_codes_distinguish_success_partial_and_policy_refusal() {
        assert_eq!(cleanup_exit(&CleanupOutcome::default()), ExitCode::Success);

        let partial = CleanupOutcome {
            removed: vec!["/safe".into()],
            freed_bytes: 1,
            skipped: vec![Skipped {
                path: "/failed".into(),
                reason: SkipReason::RemoverFailed("fixture".into()),
            }],
        };
        assert_eq!(cleanup_exit(&partial), ExitCode::Operational);

        let refused = CleanupOutcome {
            skipped: vec![Skipped {
                path: "/protected".into(),
                reason: SkipReason::Refused,
            }],
            ..CleanupOutcome::default()
        };
        assert_eq!(cleanup_exit(&refused), ExitCode::PolicyRefused);
    }

    #[test]
    fn an_interrupted_scan_maps_to_exit_code_five() {
        let fixture = tempfile::tempdir().unwrap();
        let cli = Cli::try_parse_from([
            "chystik",
            "scan",
            fixture.path().to_str().unwrap(),
            "--format",
            "json",
        ])
        .unwrap();
        let cancel = Arc::new(AtomicBool::new(true));

        let error = run_with_cancel(cli, &cancel).unwrap_err();

        assert_eq!(error.code, ExitCode::Interrupted);
        assert_eq!(error.code.as_i32(), 5);
    }

    #[test]
    fn documented_exit_codes_are_stable() {
        assert_eq!(ExitCode::Success.as_i32(), 0);
        assert_eq!(ExitCode::Operational.as_i32(), 1);
        assert_eq!(ExitCode::InvalidInput.as_i32(), 2);
        assert_eq!(ExitCode::Cancelled.as_i32(), 3);
        assert_eq!(ExitCode::PolicyRefused.as_i32(), 4);
        assert_eq!(ExitCode::Interrupted.as_i32(), 5);
    }
}

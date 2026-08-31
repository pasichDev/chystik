//! Interactive terminal presentation for human scans.
//!
//! This module is deliberately CLI-local: core emits frontend-neutral scan
//! events, while this renderer owns raw mode, alternate-screen lifecycle, and
//! keyboard input. Machine-readable commands never call into it.

use std::io::{self, IsTerminal};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use chystik_core::app::{self, AppScanEvent, AppScanSummary, ScanRequest, ScanResult};
use chystik_core::model::{ChystikError, Finding, Severity};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

const REFRESH_INTERVAL: Duration = Duration::from_millis(50);
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Whether the scanner result remains on screen for browsing or returns as
/// soon as scanning finishes (the latter is used before a clean manifest).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletionMode {
    BrowseResults,
    CloseWhenComplete,
}

/// Alternate-screen UI is only safe for an interactive terminal. The CLI
/// keeps its line-based fallback for SSH pipes, redirected output, and tests.
pub(crate) fn is_available() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

/// Scan on a worker thread while the calling thread renders a responsive TUI.
/// The worker remains read-only and uses the same shared app service as every
/// other frontend.
pub(crate) fn run_scan(
    request: ScanRequest,
    cancel: Arc<AtomicBool>,
    completion_mode: CompletionMode,
    color: bool,
) -> Result<ScanResult, ChystikError> {
    let mut session = TerminalSession::start().map_err(ChystikError::Io)?;
    let (event_tx, event_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let worker_cancel = cancel.clone();
    let worker = std::thread::spawn(move || {
        let result = app::scan_with_events(&request, &worker_cancel, move |event| {
            let _ = event_tx.send(event);
        });
        let _ = result_tx.send(result);
    });

    let result = run_loop(
        &mut session.terminal,
        event_rx,
        result_rx,
        &cancel,
        completion_mode,
        color,
    );
    if result.is_err() {
        cancel.store(true, Ordering::SeqCst);
    }
    drop(session);
    worker
        .join()
        .map_err(|_| ChystikError::Io(io::Error::other("scan worker panicked")))?;
    result
}

struct TerminalSession {
    terminal: ratatui::DefaultTerminal,
}

impl TerminalSession {
    fn start() -> io::Result<Self> {
        Ok(Self {
            terminal: ratatui::try_init()?,
        })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    event_rx: mpsc::Receiver<AppScanEvent>,
    result_rx: mpsc::Receiver<Result<ScanResult, ChystikError>>,
    cancel: &Arc<AtomicBool>,
    completion_mode: CompletionMode,
    color: bool,
) -> Result<ScanResult, ChystikError> {
    let mut view = ScanView::default();
    let mut completed = None;

    loop {
        while let Ok(event) = event_rx.try_recv() {
            view.apply(event);
        }
        if cancel.load(Ordering::Relaxed) && !view.is_complete() {
            view.cancelling = true;
        }
        if completed.is_none() {
            match result_rx.try_recv() {
                Ok(Ok(result)) => {
                    view.mark_complete(&result);
                    if completion_mode == CompletionMode::CloseWhenComplete {
                        return Ok(result);
                    }
                    completed = Some(result);
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(ChystikError::Io(io::Error::other(
                        "scan worker closed without a result",
                    )));
                }
            }
        }

        terminal
            .draw(|frame| render_scan(frame, &view, color))
            .map_err(ChystikError::Io)?;
        view.tick = view.tick.wrapping_add(1);

        if event::poll(REFRESH_INTERVAL).map_err(ChystikError::Io)? {
            let event = event::read().map_err(ChystikError::Io)?;
            if let Event::Key(key) = event {
                handle_key(&mut view, key, cancel, &completed);
            }
        }
        if view.close_requested {
            if let Some(result) = completed {
                return Ok(result);
            }
        }
    }
}

fn handle_key(
    view: &mut ScanView,
    key: KeyEvent,
    cancel: &Arc<AtomicBool>,
    completed: &Option<ScanResult>,
) {
    if key.kind != KeyEventKind::Press {
        return;
    }
    if completed.is_some() {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => view.close_requested = true,
            KeyCode::Up | KeyCode::Char('k') => view.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => view.select_next(),
            KeyCode::Home => view.select_first(),
            KeyCode::End => view.select_last(),
            _ => {}
        }
        return;
    }

    let interrupt = matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL));
    if interrupt {
        cancel.store(true, Ordering::SeqCst);
        view.cancelling = true;
    }
}

#[derive(Debug, Default)]
struct ScanView {
    root: Option<std::path::PathBuf>,
    directories: u64,
    findings: Vec<Finding>,
    summary: Option<AppScanSummary>,
    tick: usize,
    selected: usize,
    completed: bool,
    cancelling: bool,
    close_requested: bool,
}

impl ScanView {
    fn apply(&mut self, event: AppScanEvent) {
        match event {
            AppScanEvent::Started { root } => self.root = Some(root),
            AppScanEvent::DirectoriesScanned { count } => self.directories = count,
            AppScanEvent::Finding(finding) => {
                self.findings.push(*finding);
            }
            AppScanEvent::Finished(summary) => self.summary = Some(summary),
            AppScanEvent::Cancelled => self.cancelling = true,
        }
    }

    fn mark_complete(&mut self, result: &ScanResult) {
        self.summary = Some(AppScanSummary {
            roots: result.roots.clone(),
            findings: result.findings.len() as u64,
            total_bytes: result
                .findings
                .iter()
                .map(|finding| finding.size_bytes)
                .sum(),
        });
        self.findings = result.findings.clone();
        self.selected = self.selected.min(self.findings.len().saturating_sub(1));
        self.completed = true;
    }

    fn is_complete(&self) -> bool {
        self.completed && !self.cancelling
    }

    fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn select_next(&mut self) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(self.findings.len().saturating_sub(1));
    }

    fn select_first(&mut self) {
        self.selected = 0;
    }

    fn select_last(&mut self) {
        self.selected = self.findings.len().saturating_sub(1);
    }

    #[cfg(test)]
    fn fixture() -> Self {
        let finding = Finding {
            path: "/home/demo/.cache/go-build".into(),
            category: chystik_core::model::Category::PackageCaches,
            severity: Severity::Safe,
            size_bytes: 1_048_576,
            last_used: None,
            mount: None,
            note: "fixture".into(),
            advice: None,
            provenance: None,
            version_group: None,
        };
        Self {
            root: Some("/home/demo".into()),
            directories: 2_048,
            findings: vec![finding],
            summary: None,
            ..Self::default()
        }
    }
}

fn render_scan(frame: &mut Frame, view: &ScanView, color: bool) {
    let area = frame.area();
    if area.width < 60 || area.height < 12 {
        render_narrow(frame, view);
        return;
    }

    let [header, metrics, table, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .areas(area);

    let title = if view.is_complete() {
        " CHYSTIK • SCAN COMPLETE "
    } else if view.cancelling {
        " CHYSTIK • CANCELLING "
    } else {
        " CHYSTIK • SCANNING "
    };
    let spinner = SPINNER[view.tick % SPINNER.len()];
    let root = view
        .root
        .as_ref()
        .map(|path| {
            shorten(
                &path.display().to_string(),
                area.width.saturating_sub(26) as usize,
            )
        })
        .unwrap_or_else(|| "Preparing scan…".into());
    let header_text = if view.is_complete() {
        format!("Completed  {root}")
    } else if view.cancelling {
        format!("{spinner} Stopping safely  {root}")
    } else {
        format!("{spinner} Reading  {root}")
    };
    frame.render_widget(
        Paragraph::new(header_text)
            .block(Block::default().borders(Borders::ALL).title(title))
            .style(
                Style::default()
                    .fg(display_color(color, Color::Cyan))
                    .add_modifier(Modifier::BOLD),
            ),
        header,
    );

    let total_bytes = view
        .summary
        .as_ref()
        .map(|summary| summary.total_bytes)
        .unwrap_or_else(|| view.findings.iter().map(|finding| finding.size_bytes).sum());
    let [dirs, found, reclaimable] =
        Layout::horizontal([Constraint::Percentage(33); 3]).areas(metrics);
    metric(
        frame,
        dirs,
        "DIRECTORIES",
        &format_number(view.directories),
        color,
        Color::Blue,
    );
    metric(
        frame,
        found,
        "FINDINGS",
        &format_number(view.findings.len() as u64),
        color,
        Color::Yellow,
    );
    metric(
        frame,
        reclaimable,
        "RECLAIMABLE",
        &format_bytes(total_bytes),
        color,
        Color::Green,
    );

    render_table(frame, table, view, color);
    let footer_text = if view.is_complete() {
        "↑↓ browse  •  Enter / q / Esc close  •  JSON and JSONL stay scriptable"
    } else if view.cancelling {
        "Waiting for the scanner to stop safely…"
    } else {
        "Esc / q / Ctrl-C cancel safely  •  Results stream into the table"
    };
    frame.render_widget(
        Paragraph::new(footer_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(display_color(color, Color::DarkGray))),
        footer,
    );
}

fn render_narrow(frame: &mut Frame, view: &ScanView) {
    let state = if view.cancelling {
        "Cancelling"
    } else if view.is_complete() {
        "Complete"
    } else {
        "Scanning"
    };
    let text = format!(
        "CHYSTIK {state}\n{} directories • {} finding(s)\n{} reclaimable\n\nTerminal too narrow for the table. Resize or use --no-tui.",
        format_number(view.directories),
        format_number(view.findings.len() as u64),
        format_bytes(view.findings.iter().map(|finding| finding.size_bytes).sum()),
    );
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title(" CHYSTIK "))
            .wrap(Wrap { trim: true }),
        frame.area(),
    );
}

fn metric(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    label: &str,
    value: &str,
    color_enabled: bool,
    accent: Color,
) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                label,
                Style::default()
                    .fg(display_color(color_enabled, Color::DarkGray))
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                value,
                Style::default()
                    .fg(display_color(color_enabled, accent))
                    .add_modifier(Modifier::BOLD),
            )),
        ])
        .block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn render_table(frame: &mut Frame, area: ratatui::layout::Rect, view: &ScanView, color: bool) {
    if view.findings.is_empty() {
        let text = if view.is_complete() {
            "No reclaimable items matched this scan."
        } else {
            "Waiting for recognized reclaimable items…"
        };
        frame.render_widget(
            Paragraph::new(text)
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).title(" FINDINGS ")),
            area,
        );
        return;
    }

    let rows = view.findings.iter().map(|finding| {
        Row::new(vec![
            Cell::from(format_bytes(finding.size_bytes)),
            Cell::from(finding.severity.as_str()),
            Cell::from(finding.category.as_str()),
            Cell::from(shorten(&finding.path.display().to_string(), 64)),
        ])
        .style(Style::default().fg(display_color(color, severity_color(finding.severity))))
    });
    let header = Row::new(["SIZE", "SEVERITY", "CATEGORY", "PATH"])
        .style(
            Style::default()
                .fg(display_color(color, Color::Cyan))
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1);
    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(18),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(" FINDINGS "))
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("› ");
    let mut table_state = TableState::default();
    table_state.select(Some(view.selected));
    frame.render_stateful_widget(table, area, &mut table_state);
}

const fn severity_color(severity: Severity) -> Color {
    match severity {
        Severity::Safe => Color::Green,
        Severity::Moderate => Color::Yellow,
        Severity::Risky => Color::Red,
    }
}

const fn display_color(enabled: bool, color: Color) -> Color {
    if enabled {
        color
    } else {
        Color::Reset
    }
}

fn format_number(value: u64) -> String {
    let text = value.to_string();
    let mut output = String::with_capacity(text.len() + text.len() / 3);
    for (index, character) in text.chars().enumerate() {
        if index > 0 && (text.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    output
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn shorten(value: &str, max_width: usize) -> String {
    if value.chars().count() <= max_width {
        return value.into();
    }
    if max_width <= 3 {
        return value.chars().take(max_width).collect();
    }
    let head = (max_width - 1) / 2;
    let tail = max_width - head - 1;
    format!(
        "{}…{}",
        value.chars().take(head).collect::<String>(),
        value
            .chars()
            .rev()
            .take(tail)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::{render_scan, ScanView};

    #[test]
    fn scan_view_renders_loader_metrics_table_and_keyboard_help() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = ScanView::fixture();

        terminal
            .draw(|frame| render_scan(frame, &state, true))
            .unwrap();

        let rendered = format!("{}", terminal.backend());
        for expected in [
            "SCANNING",
            "DIRECTORIES",
            "RECLAIMABLE",
            "SIZE",
            "SEVERITY",
            "CATEGORY",
            "PATH",
            "q / Ctrl-C",
        ] {
            assert!(
                rendered.contains(expected),
                "missing {expected:?}:\n{rendered}"
            );
        }
    }

    #[test]
    fn long_paths_are_shortened_without_hiding_their_tail() {
        assert_eq!(
            super::shorten("/home/demo/a-very-long-directory-name/cache", 17),
            "/home/de…me/cache"
        );
    }
}

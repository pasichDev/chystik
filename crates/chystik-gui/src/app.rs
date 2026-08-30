//! The application object: state, scan lifecycle, deletion and export.
//!
//! Drawing lives in `panels` and `modals`, both of which add methods to
//! `ChystikApp` in their own files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use chystik_core::app::AppScanEvent;
use chystik_core::model::{Category, Finding};
use chystik_core::platform::{self, StorageVolume};

use crate::format::*;
use crate::i18n::{self, Lang, Strings};
use crate::state::*;
use crate::theme::*;
use crate::widgets::*;

pub(crate) struct ChystikApp {
    /// Interface language; detected from the locale, switchable at runtime.
    pub(crate) lang: Lang,
    /// Real volumes from the target-selected core platform adapter.
    pub(crate) disks: Vec<StorageVolume>,
    /// Scan targets offered in the Targets popover.
    pub(crate) targets: Vec<ScanTarget>,
    /// Canonical roots the last scan actually walked, captured from the scan
    /// summary. Findings are stored in this same canonical form (on Windows a
    /// verbatim `\\?\` path), so guard/owning-root checks must anchor on these
    /// — not on the raw `targets`, whose spelling can differ and would fail a
    /// `Path::starts_with` match across the verbatim boundary.
    pub(crate) scan_roots: Vec<PathBuf>,

    pub(crate) state: ScanState,
    pub(crate) rx: Receiver<AppScanEvent>,

    pub(crate) findings: Vec<Finding>,
    /// Indices into `self.findings` currently ticked by the user.
    pub(crate) selected: HashSet<usize>,
    /// Indices of findings already moved to trash (hidden from the table).
    pub(crate) deleted: HashSet<usize>,

    pub(crate) dir_count: u64,
    pub(crate) live_bytes: u64,
    pub(crate) progress_text: String,

    /// Cached filtered+sorted view over `findings` (see `ViewCache`).
    /// Rebuilt lazily when its inputs change; throttled while scanning.
    pub(crate) view: ViewCache,
    pub(crate) view_stamp: Option<ViewStamp>,
    pub(crate) view_built_at: Option<Instant>,

    /// Enabled-state signature of `targets` + cached toolbar roots label,
    /// so the per-frame toolbar never rebuilds that string.
    pub(crate) roots_sig: u64,
    pub(crate) roots_display: String,
    pub(crate) roots_nonempty: bool,

    pub(crate) category_filter: CategoryFilter,
    pub(crate) severity_filter: SeverityFilter,
    /// Case-insensitive substring filter applied to finding paths.
    pub(crate) search: String,

    pub(crate) sort_col: SortCol,
    pub(crate) sort_asc: bool,

    pub(crate) confirm_delete_open: bool,
    pub(crate) settings_open: bool,
    /// Cleared once the user acknowledges the risk dialog.
    pub(crate) consent_pending: bool,
    /// Ticked in that dialog; `Continue` stays disabled until it is.
    pub(crate) consent_checked: bool,
    /// Paths the user marked never-touch. Passed to the scanner as prunes
    /// and re-checked before any deletion.
    pub(crate) exclusions: Vec<PathBuf>,
    /// False when the stored list could not be read; the UI says so rather
    /// than silently behaving as if nothing was excluded.
    pub(crate) exclusions_readable: bool,
    /// Decoded once on first use by the settings dialog.
    pub(crate) app_mark: Option<egui::TextureHandle>,
    /// Which view is showing.
    pub(crate) section: Section,
    /// Ctrl+K section picker.
    pub(crate) palette_open: bool,
    /// Confirmation before erasing privacy traces. Always shown; there is
    /// deliberately no way to suppress it.
    pub(crate) privacy_confirm_open: bool,
    /// Attached drives, refreshed on entering the Disks view.
    pub(crate) drives: Vec<chystik_core::blockdev::Drive>,
    /// Privacy traces, refreshed on entering the Privacy view.
    pub(crate) traces: Vec<chystik_core::privacy::PrivacyItem>,
    /// Indices into `traces` the user ticked. Never pre-populated.
    pub(crate) traces_selected: HashSet<usize>,
    /// Row whose advisory command was copied, plus the short-lived feedback
    /// deadline. This belongs to the app rather than a rendered table row so
    /// a virtualized row cannot lose the feedback between frames.
    pub(crate) copied_advice: Option<(usize, Instant)>,
    pub(crate) notice: Option<Notice>,
}

pub(crate) struct Notice {
    pub(crate) title: String,
    pub(crate) lines: Vec<String>,
}

const FOOTER_ACTION_HEIGHT: f32 = space(8.0);
const FOOTER_DIVIDER_GAP: f32 = 30.0;
const FOOTER_HEIGHT: f32 = space(20.0);

/// Reserve an explicit action row directly below the footer divider.
/// `horizontal_wrapped` owns its own layout cursor, so its placement must not
/// depend on a preceding `add_space` in the parent UI.
fn footer_actions_rect(footer_rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(footer_rect.left(), footer_rect.top() + FOOTER_DIVIDER_GAP),
        egui::vec2(footer_rect.width(), FOOTER_ACTION_HEIGHT),
    )
}

/// Inputs the cached view depends on. Any change forces a rebuild.
impl Default for ChystikApp {
    fn default() -> Self {
        let exclusions_loaded = crate::exclusions::load();
        // Placeholder receiver replaced on first scan; never yields events.
        let (_tx, rx) = channel::<AppScanEvent>();
        Self {
            lang: i18n::detect(),
            disks: Vec::new(),
            targets: Vec::new(),
            scan_roots: Vec::new(),
            state: ScanState::Idle,
            rx,
            findings: Vec::new(),
            selected: HashSet::new(),
            deleted: HashSet::new(),
            dir_count: 0,
            live_bytes: 0,
            progress_text: String::new(),
            view: ViewCache::default(),
            view_stamp: None,
            view_built_at: None,
            roots_sig: u64::MAX,
            roots_display: String::new(),
            roots_nonempty: false,
            category_filter: CategoryFilter::All,
            severity_filter: SeverityFilter::All,
            search: String::new(),
            sort_col: SortCol::Size,
            sort_asc: false, // size descending by default
            confirm_delete_open: false,
            settings_open: false,
            consent_pending: !crate::consent::is_acknowledged(),
            consent_checked: false,
            exclusions: exclusions_loaded.0,
            exclusions_readable: exclusions_loaded.1,
            app_mark: None,
            section: Section::default(),
            palette_open: false,
            privacy_confirm_open: false,
            drives: Vec::new(),
            traces: Vec::new(),
            traces_selected: HashSet::new(),
            copied_advice: None,
            notice: None,
        }
    }
}

impl ChystikApp {
    pub(crate) fn scanning(&self) -> bool {
        matches!(self.state, ScanState::Scanning { .. })
    }

    pub(crate) fn cleanup_available(&self) -> bool {
        platform::current().cleanup_support().is_available()
    }

    /// Move to another view, loading whatever data it needs.
    ///
    /// Both new views read the machine directly rather than the scan, so
    /// they are refreshed on entry: a drive can be plugged in, and a trace
    /// can grow, while the window sits open.
    pub(crate) fn go_to(&mut self, section: Section) {
        self.section = section;
        self.palette_open = false;
        match section {
            Section::Disks => self.drives = chystik_core::blockdev::drives(),
            Section::Privacy => {
                self.traces = chystik_core::privacy::probe();
                self.traces_selected.clear();
            }
            Section::Cleanup => {}
        }
    }

    /// Bytes of the privacy traces currently ticked.
    pub(crate) fn selected_trace_bytes(&self) -> u64 {
        self.traces_selected
            .iter()
            .filter_map(|i| self.traces.get(*i))
            .map(|t| t.size_bytes)
            .sum()
    }

    /// Send the ticked privacy traces to the trash, through the same
    /// validated flow the cleaner uses.
    pub(crate) fn clear_selected_traces(&mut self) {
        use chystik_core::cleaner::{self, CleanupItem};

        let items: Vec<CleanupItem> = self
            .traces_selected
            .iter()
            .filter_map(|i| self.traces.get(*i))
            .map(|trace| CleanupItem {
                path: trace.path.clone(),
                size_bytes: trace.size_bytes,
                // The core catalogue resolves Windows roaming/local profile
                // roots. It returns a root only for an exact listed trace,
                // so UI state cannot widen cleanup to a guessed parent.
                scan_root: chystik_core::privacy::cleanup_root_for(&trace.path),
            })
            .collect();
        if items.is_empty() {
            return;
        }
        let outcome = cleaner::clean(&items, &cleaner::SystemTrash);
        let loc = self.s();
        let mut lines = vec![i18n::fill(
            loc.trash_moved.as_str(),
            &[
                ("n", &outcome.removed_count().to_string()),
                ("size", &format_size(outcome.freed_bytes)),
            ],
        )];
        if outcome.skipped_count() > 0 {
            lines.push(i18n::fill(
                loc.trash_skipped.as_str(),
                &[("n", &outcome.skipped_count().to_string())],
            ));
        }
        self.notice = Some(Notice {
            title: loc.trash_done_title.clone(),
            lines,
        });
        self.traces = chystik_core::privacy::probe();
        self.traces_selected.clear();
    }

    /// Localised interface strings for the active language.
    pub(crate) fn s(&self) -> &'static Strings {
        i18n::strings(self.lang)
    }

    /// Rebuild the view cache if its inputs changed. While a scan streams
    /// results in, rebuilds are throttled to one per 200 ms — the progress
    /// panel shows live numbers meanwhile, and the final rebuild lands on
    /// `Finished` when the state leaves `Scanning`.
    pub(crate) fn ensure_view(&mut self) {
        let stamp = ViewStamp {
            findings_len: self.findings.len(),
            deleted_len: self.deleted.len(),
            category: self.category_filter,
            severity: self.severity_filter,
            search: self.search.clone(),
            sort_col: self.sort_col,
            sort_asc: self.sort_asc,
        };
        if self.view_stamp.as_ref() == Some(&stamp) {
            return;
        }
        let throttled = self.scanning()
            && self
                .view_built_at
                .map(|t| t.elapsed() < Duration::from_millis(200))
                .unwrap_or(false);
        if throttled {
            return; // stamp intentionally left unset — catch up later
        }
        self.rebuild_view();
        self.view_stamp = Some(stamp);
        self.view_built_at = Some(Instant::now());
    }

    /// One O(N) pass producing the sorted view and every aggregate the UI
    /// shows (bucket chips, per-category peaks, top offender).
    pub(crate) fn rebuild_view(&mut self) {
        let lowered = self.search.trim().to_lowercase();
        let needle = (!lowered.is_empty()).then_some(lowered);
        let mut rows: Vec<usize> = Vec::with_capacity(self.findings.len());
        let mut cleanup_totals = CleanupTotals::default();
        let mut stats: HashMap<Category, CatStat> = HashMap::new();
        let (mut all_bytes, mut all_count) = (0u64, 0usize);
        for (i, f) in self.findings.iter().enumerate() {
            if self.deleted.contains(&i) {
                continue;
            }
            // Sidebar totals ignore the category filter, so selecting a
            // category never blanks out the rest of the list.
            if matches_filter(
                f,
                CategoryFilter::All,
                self.severity_filter,
                needle.as_deref(),
            ) {
                stats
                    .entry(f.category)
                    .or_insert_with(|| CatStat::new(f.category))
                    .add(f);
                all_bytes += f.size_bytes;
                all_count += 1;
            }
            if !matches_filter(
                f,
                self.category_filter,
                self.severity_filter,
                needle.as_deref(),
            ) {
                continue;
            }
            cleanup_totals.add(f);
            rows.push(i);
        }
        let mut cat_stats: Vec<CatStat> = stats.into_values().collect();
        cat_stats.sort_by_key(|c| std::cmp::Reverse(c.bytes));
        let (col, asc) = (self.sort_col, self.sort_asc);
        rows.sort_by(|&a, &b| {
            let (x, y) = (&self.findings[a], &self.findings[b]);
            let ord = match col {
                SortCol::Path => x.path.cmp(&y.path),
                SortCol::Size => x.size_bytes.cmp(&y.size_bytes),
                SortCol::Severity => severity_rank(x.severity).cmp(&severity_rank(y.severity)),
                SortCol::Age => x.last_used.cmp(&y.last_used),
            };
            if asc {
                ord
            } else {
                ord.reverse()
            }
        });
        self.view = ViewCache {
            rows,
            cleanup_totals,
            cat_stats,
            all_bytes,
            all_count,
        };
    }

    /// Visible findings the user ticked (used by footer + confirm modal).
    /// Selection is small, so filtering it directly beats walking all rows.
    pub(crate) fn selected_visible_rows(&self) -> Vec<(usize, &Finding)> {
        let lowered = self.search.trim().to_lowercase();
        let needle = (!lowered.is_empty()).then_some(lowered);
        self.selected
            .iter()
            .filter(|i| !self.deleted.contains(i))
            .filter_map(|i| self.findings.get(*i).map(|f| (*i, f)))
            .filter(|(_, f)| {
                matches_filter(
                    f,
                    self.category_filter,
                    self.severity_filter,
                    needle.as_deref(),
                )
            })
            .collect()
    }

    /// Toggle sort column/direction after a header click.
    pub(crate) fn apply_header_click(&mut self, col: SortCol, default_asc: bool) {
        if self.sort_col == col {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort_col = col;
            self.sort_asc = default_asc;
        }
    }

    // -- targets / disks -------------------------------------------------------

    /// Re-read the mount table (startup + Refresh) and rebuild detected
    /// targets, preserving enable/disable state and user-added folders.
    pub(crate) fn refresh_disks(&mut self) {
        self.disks = platform::current().storage_volumes();
        let defaults = default_roots(&self.disks);
        let mut next: Vec<ScanTarget> = Vec::new();
        for root in defaults {
            let was_enabled = self
                .targets
                .iter()
                .find(|t| !t.user_added && t.root.as_path() == root.as_path())
                .map(|t| t.enabled);
            next.push(ScanTarget {
                label: root.display().to_string(),
                root,
                enabled: was_enabled.unwrap_or(true),
                user_added: false,
            });
        }
        next.extend(self.targets.drain(..).filter(|t| t.user_added));
        self.targets = next;
    }

    /// Enabled targets with nested duplicates removed — what a scan walks.
    pub(crate) fn effective_roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = self
            .targets
            .iter()
            .filter(|t| t.enabled)
            .map(|t| t.root.clone())
            .collect();
        chystik_core::app::dedup_nested_roots(&mut roots);
        roots
    }

    /// Cheap change detector for the toolbar roots label: target count in
    /// the low byte, enabled flags above it. Avoids rebuilding the joined
    /// roots string every frame.
    pub(crate) fn targets_signature(&self) -> u64 {
        let mut sig = (self.targets.len() as u64) & 0xff;
        for (i, t) in self.targets.iter().take(56).enumerate() {
            if t.enabled {
                sig |= 1 << (8 + i);
            }
        }
        sig
    }

    pub(crate) fn refresh_roots_display(&mut self) {
        let sig = self.targets_signature();
        if sig == self.roots_sig {
            return;
        }
        let roots = self.effective_roots();
        self.roots_nonempty = !roots.is_empty();
        self.roots_display = if roots.is_empty() {
            "(no targets selected)".to_string()
        } else {
            roots
                .iter()
                .map(|r| truncate_middle(&r.display().to_string(), 24))
                .collect::<Vec<_>>()
                .join(", ")
        };
        self.roots_sig = sig;
    }

    /// Longest scan root containing `path`; anchors guard checks when several
    /// targets overlap.
    ///
    /// Findings are stored in the canonical form the scan walked (on Windows a
    /// verbatim `\\?\` path). Match them against `scan_roots`, captured in that
    /// same form, rather than the raw `targets`: a target spelled `C:\x` never
    /// prefix-matches a `\\?\C:\x` finding, which made the confirm dialog mark
    /// every item "refused". `effective_roots` is the fallback only when no
    /// scan has recorded its roots yet.
    pub(crate) fn owning_root(&self, path: &Path) -> Option<PathBuf> {
        let roots = if self.scan_roots.is_empty() {
            self.effective_roots()
        } else {
            self.scan_roots.clone()
        };
        chystik_core::app::owning_root(&roots, path).map(Path::to_path_buf)
    }
}

impl eframe::App for ChystikApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_scanner(ctx);
        self.ensure_view();

        // Section shortcuts, ignored while a dialog owns the keyboard.
        if !self.consent_pending && !self.confirm_delete_open && !self.settings_open {
            let jump = ctx.input(|i| {
                if i.modifiers.ctrl && i.key_pressed(egui::Key::K) {
                    return Some(None);
                }
                for (n, key) in [
                    (0, egui::Key::Num1),
                    (1, egui::Key::Num2),
                    (2, egui::Key::Num3),
                ] {
                    // Digits only when no text field has focus, or typing a
                    // filter would teleport the user out of the view.
                    if i.key_pressed(key) {
                        return Some(Some(n));
                    }
                }
                None
            });
            match jump {
                Some(None) => self.palette_open = !self.palette_open,
                Some(Some(n)) if !ctx.wants_keyboard_input() => {
                    self.go_to(Section::ALL[n]);
                }
                _ => {}
            }
        }

        if self.consent_pending {
            // Deliberately first and exclusive: no scan, no selection and no
            // deletion is reachable until this is answered.
            self.show_consent_modal(ctx);
        } else if self.privacy_confirm_open {
            self.show_privacy_confirm(ctx);
        } else if self.palette_open {
            self.show_palette(ctx);
        } else if self.settings_open {
            self.show_settings_modal(ctx);
        } else if self.confirm_delete_open {
            self.show_confirm_modal(ctx);
        } else if let Some(notice) = self.notice.take() {
            self.show_notice_modal(ctx, &notice);
        }

        egui::TopBottomPanel::top("command_bar")
            .exact_height(space(12.0))
            .frame(
                egui::Frame::none()
                    .fill(COL_SURFACE)
                    .stroke(egui::Stroke::NONE)
                    .inner_margin(egui::Margin::symmetric(space(4.0), 0.0)),
            )
            .show(ctx, |ui| {
                hairline_bottom(ui);
                self.command_bar_ui(ui);
            });

        // Fixed height whether or not a scan runs: a panel that appears
        // only while scanning shoved the whole table down and yanked it
        // back, twice per scan.
        // Only the cleanup view has a scan; the others read the machine
        // directly and have nothing to report here.
        if self.section == Section::Cleanup {
            egui::TopBottomPanel::top("scan_status")
                .exact_height(space(9.0))
                .frame(
                    egui::Frame::none()
                        .fill(COL_SURFACE)
                        .inner_margin(egui::Margin::symmetric(space(4.0), 0.0)),
                )
                .show(ctx, |ui| {
                    hairline_bottom(ui);
                    self.scan_status_ui(ui, ctx);
                });
        }

        let footer = match self.section {
            Section::Cleanup | Section::Privacy => true,
            Section::Disks => false,
        };
        if footer {
            egui::TopBottomPanel::bottom("footer")
                .exact_height(FOOTER_HEIGHT)
                // The footer owns one explicit rule at its top edge. Disable
                // egui's panel separator so it cannot add a second rule on a
                // different pixel row around the action buttons.
                .show_separator_line(false)
                .frame(
                    egui::Frame::none()
                        .fill(COL_SURFACE)
                        .inner_margin(egui::Margin::symmetric(space(4.0), space(2.0))),
                )
                .show(ctx, |ui| {
                    hairline_top(ui);
                    let action_row = footer_actions_rect(ui.max_rect());
                    ui.allocate_new_ui(
                        egui::UiBuilder::new().max_rect(action_row),
                        |ui| match self.section {
                            Section::Privacy => self.privacy_footer_ui(ui),
                            _ => self.footer_ui(ui),
                        },
                    );
                });
        }

        // `SidePanel` and `CentralPanel` are rasterized as separate frames.
        // At fractional display scales their shared logical edge can expose
        // one physical pixel of the viewport clear colour (`COL_BG`). Paint
        // one continuous workspace below both panels so that boundary is
        // always surface-coloured, never a black vertical seam.
        ctx.layer_painter(egui::LayerId::background()).rect_filled(
            ctx.available_rect(),
            egui::Rounding::ZERO,
            COL_SURFACE,
        );

        // The category rail belongs to the cleanup view alone. Disks and
        // Privacy are single-column: nothing to filter down.
        if self.section == Section::Cleanup {
            egui::SidePanel::left("categories")
                .exact_width(SIDEBAR_W)
                .resizable(false)
                // The side panel otherwise draws its built-in separator.
                // A second hand-painted line used to sit on the same edge;
                // at fractional display scales the two strokes looked like a
                // dark empty strip between the sidebar and the content list.
                .show_separator_line(false)
                .frame(
                    egui::Frame::none()
                        .fill(COL_SURFACE)
                        .inner_margin(egui::Margin::symmetric(0.0, space(4.0))),
                )
                .show(ctx, |ui| {
                    self.sidebar_ui(ui);
                });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(COL_SURFACE))
            .show(ctx, |ui| match self.section {
                Section::Cleanup => self.detail_ui(ui),
                Section::Disks => self.disks_ui(ui),
                Section::Privacy => self.privacy_ui(ui),
            });
    }
}

/// 1px rule along the bottom of the panel currently being laid out.
impl ChystikApp {
    pub(crate) fn add_folder(&mut self) {
        let initial = self
            .targets
            .iter()
            .find(|t| t.enabled)
            .map(|t| t.root.clone())
            .unwrap_or_else(|| PathBuf::from("/"));
        let picked = rfd::FileDialog::new().set_directory(initial).pick_folder();
        let Some(dir) = picked.filter(|d| !d.as_os_str().is_empty()) else {
            return;
        };
        match self
            .targets
            .iter_mut()
            .find(|t| t.root.as_path() == dir.as_path())
        {
            Some(t) => {
                t.enabled = true;
                t.user_added = true;
            }
            None => self.targets.push(ScanTarget {
                label: truncate_middle(&dir.display().to_string(), 36),
                root: dir,
                enabled: true,
                user_added: true,
            }),
        }
        self.roots_sig = u64::MAX; // force the cached label to rebuild
    }

    // -- scanning ------------------------------------------------------------

    pub(crate) fn reset_results(&mut self) {
        self.findings.clear();
        self.scan_roots.clear();
        self.selected.clear();
        self.deleted.clear();
        self.dir_count = 0;
        self.live_bytes = 0;
        self.progress_text.clear();
        self.category_filter = CategoryFilter::All;
        // The view holds INDICES into `findings`, and this runs from a panel
        // — after `ensure_view` has already built them for this frame. Only
        // clearing the stamp left the table indexing a vector it had just
        // emptied, which is why a second press of Scan panicked.
        self.view = ViewCache::default();
        self.view_stamp = None;
    }

    pub(crate) fn start_scan(&mut self) {
        // Belt and braces: the dialog already blocks the button, but the
        // headless smoke hook calls this directly.
        if self.consent_pending {
            return;
        }
        let roots = self.effective_roots();
        if roots.is_empty() {
            return;
        }
        let (tx, rx) = channel::<AppScanEvent>();
        let cancel_flag = Arc::new(AtomicBool::new(false));

        self.reset_results();
        self.progress_text = format!("Scanning {} target(s)\u{2026}", roots.len());
        self.notice = None;

        let exclusions = self.exclusions.clone();
        let cancel_for_thread = Arc::clone(&cancel_flag);
        let spawned = std::thread::Builder::new()
            .name("chystik-scanner".to_string())
            .spawn(move || {
                let request = chystik_core::app::ScanRequest {
                    roots,
                    exclude: exclusions,
                    // The desktop UI has always shown advisory rows with a
                    // recovery command; retain that contract through the
                    // shared application service used by the CLI.
                    include_advisories: true,
                    ..chystik_core::app::ScanRequest::default()
                };
                let callback_tx = tx.clone();
                let result =
                    chystik_core::app::scan_stream(&request, &cancel_for_thread, move |event| {
                        let _ = callback_tx.send(event);
                    });
                if let Err(e) = result {
                    if !matches!(e, chystik_core::ChystikError::Cancelled) {
                        eprintln!("[chystik] scan error: {e}");
                        // Best-effort terminal event so the UI never hangs.
                        let _ = tx.send(AppScanEvent::Cancelled);
                    }
                }
            });

        let handle = match spawned {
            Ok(h) => h,
            Err(e) => {
                self.notice = Some(Notice {
                    title: self.s().scan_failed.to_string(),
                    lines: vec![e.to_string()],
                });
                return;
            }
        };

        self.rx = rx;
        self.state = ScanState::Scanning {
            cancel_flag,
            handle,
        };
    }

    /// Drain the scanner channel; keep repainting while a scan is active.
    ///
    /// The shared streaming scan emits exactly one terminal event for the
    /// whole run, so
    /// joining the worker here can never block the UI thread mid-scan.
    pub(crate) fn poll_scanner(&mut self, ctx: &egui::Context) {
        let s = self.s();
        loop {
            match self.rx.try_recv() {
                Ok(event) => match event {
                    AppScanEvent::Started { root } => {
                        self.progress_text =
                            format!("{} {}\u{2026}", s.scanning_target.as_str(), root.display());
                        // Capture the canonical root now so a cancelled scan
                        // still resolves the findings it did collect.
                        if !self.scan_roots.contains(&root) {
                            self.scan_roots.push(root);
                        }
                    }
                    AppScanEvent::DirectoriesScanned { count } => {
                        self.dir_count = count.max(self.dir_count);
                    }
                    AppScanEvent::Finding(finding) => {
                        self.live_bytes += finding.size_bytes;
                        self.findings.push(*finding);
                    }
                    AppScanEvent::Finished(summary) => {
                        // The canonical roots that produced these findings.
                        // owning_root/guard checks anchor on these, not the
                        // raw targets whose spelling may differ.
                        self.scan_roots = summary.roots;
                        self.view_stamp = None;
                        self.dir_count = 0;
                        self.refresh_disks(); // free space moved during the walk
                        self.progress_text = format!(
                            "{} {} \u{b7} {} {}",
                            self.findings.len(),
                            s.items_word.as_str(),
                            format_size(self.live_bytes),
                            s.reclaimable_summary.as_str()
                        );
                        self.finish_scan();
                    }
                    AppScanEvent::Cancelled => {
                        self.progress_text = i18n::fill(
                            s.cancelled_summary.as_str(),
                            &[("n", &self.findings.len().to_string())],
                        );
                        self.finish_scan();
                    }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.scanning() {
                        self.state = ScanState::Idle;
                        self.progress_text = s.scanner_died.to_string();
                    }
                    break;
                }
            }
        }

        if self.scanning() {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }

    /// Leave `Scanning` and reap the worker (already finishing by now).
    pub(crate) fn finish_scan(&mut self) {
        if let ScanState::Scanning { handle, .. } =
            std::mem::replace(&mut self.state, ScanState::Idle)
        {
            let _ = handle.join();
        }
    }

    // -- table ---------------------------------------------------------------

    /// The detail table: four columns, not eight.
    ///
    /// Category is gone (the sidebar says it), mount is gone (95% of rows
    /// repeat the same value) and the sparkline is gone — it normalised
    /// against the per-category maximum inside a globally sorted table, so
    /// a 200 MB item could out-draw a 4 GB one.
    pub(crate) fn export_json(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("chystik-report.json")
            .save_file()
        else {
            return;
        };
        match chystik_core::report::export_json(&self.findings, &path) {
            Ok(()) => {
                self.notice = Some(Notice {
                    title: self.s().export_done.to_string(),
                    lines: vec![format!("Report written to {}", path.display())],
                });
            }
            Err(e) => {
                self.notice = Some(Notice {
                    title: self.s().export_failed.to_string(),
                    lines: vec![e.to_string()],
                });
            }
        }
    }

    // -- modals ---------------------------------------------------------------

    /// Add a never-touch path, persist it and drop any finding it covers.
    pub(crate) fn exclude_path(&mut self, path: PathBuf) {
        self.exclusions.push(path);
        self.exclusions = crate::exclusions::normalise(std::mem::take(&mut self.exclusions));
        crate::exclusions::save(&self.exclusions);
        self.drop_excluded_findings();
    }

    pub(crate) fn unexclude(&mut self, path: &Path) {
        self.exclusions.retain(|p| p != path);
        crate::exclusions::save(&self.exclusions);
    }

    /// Hide anything the current exclusions cover, without rescanning.
    fn drop_excluded_findings(&mut self) {
        let excluded: Vec<usize> = self
            .findings
            .iter()
            .enumerate()
            .filter(|(_, f)| crate::exclusions::is_excluded(&f.path, &self.exclusions))
            .map(|(i, _)| i)
            .collect();
        for i in excluded {
            self.deleted.insert(i);
            self.selected.remove(&i);
        }
        self.view_stamp = None;
    }

    pub(crate) fn execute_trash(&mut self, indices: Vec<usize>) {
        use chystik_core::cleaner::{self, CleanupItem, SkipReason};

        // Excluded and advisory rows are unselectable in the UI; filtering
        // here as well means an exclusion added after the scan still holds.
        let mut planned: Vec<(usize, CleanupItem)> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut skipped = 0usize;
        for idx in indices {
            let Some(finding) = self.findings.get(idx) else {
                continue;
            };
            if !finding.is_actionable()
                || crate::exclusions::is_excluded(&finding.path, &self.exclusions)
            {
                skipped += 1;
                continue;
            }
            planned.push((
                idx,
                CleanupItem {
                    path: finding.path.clone(),
                    size_bytes: finding.size_bytes,
                    scan_root: self.owning_root(&finding.path),
                },
            ));
        }

        // The guard checks, the identity re-check and the tallying all live
        // in core, where CI exercises them against a fake remover.
        let items: Vec<CleanupItem> = planned.iter().map(|(_, item)| item.clone()).collect();
        let outcome = cleaner::clean(&items, &cleaner::SystemTrash);

        let removed: std::collections::HashSet<&Path> =
            outcome.removed.iter().map(PathBuf::as_path).collect();
        for (idx, item) in &planned {
            if removed.contains(item.path.as_path()) {
                self.deleted.insert(*idx);
            }
        }
        skipped += outcome.skipped_count();
        for skip in &outcome.skipped {
            let path = truncate_middle(&display_path(&skip.path), 48);
            let detail = match &skip.reason {
                SkipReason::OutsideEveryTarget => "outside every scan target".to_owned(),
                SkipReason::Refused => "refused by the safety guard".to_owned(),
                SkipReason::Advisory => "not Chystik's to delete".to_owned(),
                SkipReason::CleanupUnavailable(reason) => (*reason).to_owned(),
                SkipReason::ChangedUnderUs => "changed on disk during the operation".to_owned(),
                SkipReason::RemoverFailed(e) => e.clone(),
            };
            eprintln!("[chystik] skipped {}: {detail}", display_path(&skip.path));
            errors.push(format!("{path}: {detail}"));
        }
        let (moved, freed) = (outcome.removed_count(), outcome.freed_bytes);

        // Drop stale selections after deletion; the table refreshes
        // automatically because deleted entries leave the cached view.
        self.selected.retain(|i| !self.deleted.contains(i));
        // Free-space numbers changed — re-stat mounts for the header chips.
        self.refresh_disks();

        let loc = self.s();
        let mut info = vec![i18n::fill(
            loc.trash_moved.as_str(),
            &[("n", &moved.to_string()), ("size", &format_size(freed))],
        )];
        if skipped > 0 {
            info.push(i18n::fill(
                loc.trash_skipped.as_str(),
                &[("n", &skipped.to_string())],
            ));
        }
        info.extend(errors);
        self.notice = Some(Notice {
            title: loc.trash_done_title.to_string(),
            lines: info,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chystik_core::model::Severity;

    /// An app seeded with findings, ready for `ensure_view`.
    fn app_with(findings: Vec<Finding>) -> ChystikApp {
        ChystikApp {
            findings,
            ..Default::default()
        }
    }

    fn finding(path: &str, size: u64) -> Finding {
        Finding {
            path: PathBuf::from(path),
            category: Category::PackageCaches,
            severity: Severity::Safe,
            size_bytes: size,
            last_used: None,
            mount: None,
            note: "test".into(),
            advice: None,
            provenance: None,
        }
    }

    #[test]
    fn owning_root_resolves_findings_against_the_canonical_scan_roots() {
        // Findings are stored in the scan's canonical root form. owning_root
        // must consult scan_roots, not the raw targets — otherwise the confirm
        // dialog marks every item "refused" because the raw target spelling
        // fails a starts_with match against the canonical finding path.
        let root = PathBuf::from(if cfg!(windows) {
            r"\\?\C:\scanroot"
        } else {
            "/scanroot"
        });
        let item = root.join("proj").join(".dart_tool");
        let mut app = app_with(vec![]);
        // No raw targets configured; only the canonical scan roots are known.
        app.scan_roots = vec![root.clone()];
        assert_eq!(app.owning_root(&item).as_deref(), Some(root.as_path()));
    }

    /// Reproduces the Windows bug directly: findings carry the verbatim `\\?\`
    /// root from `canonicalize`, while the raw target keeps the plain `C:\`
    /// spelling. The plain target fails `starts_with` on the verbatim finding,
    /// so the guard used to refuse everything. `scan_roots` holds the verbatim
    /// form and bridges the gap.
    #[cfg(target_os = "windows")]
    #[test]
    fn owning_root_bridges_the_verbatim_prefix_gap() {
        let verbatim = PathBuf::from(r"\\?\C:\scanroot");
        let item = verbatim.join("proj").join("node_modules");
        let mut app = app_with(vec![]);
        app.targets = vec![ScanTarget {
            root: PathBuf::from(r"C:\scanroot"),
            label: String::new(),
            enabled: true,
            user_added: true,
        }];
        assert_eq!(
            chystik_core::app::owning_root(&app.effective_roots(), &item),
            None,
            "verbatim finding must not match the plain target — that is the bug"
        );
        app.scan_roots = vec![verbatim.clone()];
        assert_eq!(app.owning_root(&item).as_deref(), Some(verbatim.as_path()));
    }

    #[test]
    fn footer_action_row_keeps_a_thirty_pixel_gap_below_the_divider() {
        let footer = egui::Rect::from_min_size(egui::pos2(0.0, 8.0), egui::vec2(800.0, 64.0));
        let row = footer_actions_rect(footer);

        assert_eq!(row.top(), 38.0);
        assert_eq!(row.bottom(), 70.0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_native_trash_capability_enables_gui_cleanup_actions() {
        assert!(ChystikApp::default().cleanup_available());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_recycle_bin_capability_enables_gui_cleanup_actions() {
        assert!(ChystikApp::default().cleanup_available());
    }

    /// Pressing Scan a second time used to panic.
    ///
    /// The frame order is poll → `ensure_view` → panels, and the Scan button
    /// lives in a panel: `reset_results` therefore emptied `findings` AFTER
    /// the cached view had been built, and the table then indexed a vector
    /// that no longer had those elements. The first press was harmless
    /// because there was nothing to index yet.
    #[test]
    fn resetting_results_invalidates_the_cached_view() {
        let mut app = app_with(vec![finding("/a", 10), finding("/b", 20)]);
        app.ensure_view();
        assert_eq!(app.view.rows.len(), 2, "fixture should produce rows");

        app.reset_results();

        assert!(
            app.view.rows.is_empty(),
            "the view still indexes {} findings that no longer exist",
            app.view.rows.len()
        );
        assert!(app.view.cat_stats.is_empty());
        assert_eq!(app.view.all_bytes, 0);
    }

    /// Replacing the findings wholesale must invalidate the view even when
    /// the length is unchanged — the stamp compares lengths, not contents.
    #[test]
    fn replacing_findings_of_equal_length_still_rebuilds() {
        let mut app = app_with(vec![finding("/a", 10)]);
        app.ensure_view();
        assert_eq!(app.view.all_bytes, 10);

        app.findings = vec![finding("/b", 99)];
        app.view_stamp = None; // what poll_scanner does on Finished
        app.ensure_view();
        assert_eq!(app.view.all_bytes, 99, "the view kept the old contents");
    }

    /// Every cached index must be addressable, whatever the app has done.
    #[test]
    fn cached_rows_never_outlive_their_findings() {
        let mut app = app_with(vec![finding("/a", 10)]);
        app.ensure_view();
        app.reset_results();
        for &i in &app.view.rows {
            assert!(
                app.findings.get(i).is_some(),
                "row {i} points past the end of findings"
            );
        }
    }
}

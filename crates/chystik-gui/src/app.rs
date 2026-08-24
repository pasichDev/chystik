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

use chystik_core::disks::DiskInfo;
use chystik_core::model::{Category, Finding, ScanProgress};

use crate::format::*;
use crate::i18n::{self, Lang, Strings};
use crate::state::*;
use crate::theme::*;
use crate::widgets::*;

pub(crate) struct ChystikApp {
    /// Interface language; detected from the locale, switchable at runtime.
    pub(crate) lang: Lang,
    /// Real volumes from `chystik_core::disks::mount_table()`.
    pub(crate) disks: Vec<DiskInfo>,
    /// Scan targets offered in the Targets popover.
    pub(crate) targets: Vec<ScanTarget>,

    pub(crate) state: ScanState,
    pub(crate) rx: Receiver<ScanProgress>,

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
    pub(crate) notice: Option<Notice>,
}

pub(crate) struct Notice {
    pub(crate) title: String,
    pub(crate) lines: Vec<String>,
}

/// Inputs the cached view depends on. Any change forces a rebuild.
impl Default for ChystikApp {
    fn default() -> Self {
        let exclusions_loaded = crate::exclusions::load();
        // Placeholder receiver replaced on first scan; never yields events.
        let (_tx, rx) = channel::<ScanProgress>();
        Self {
            lang: i18n::detect(),
            disks: Vec::new(),
            targets: Vec::new(),
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
            notice: None,
        }
    }
}

impl ChystikApp {
    pub(crate) fn scanning(&self) -> bool {
        matches!(self.state, ScanState::Scanning { .. })
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
        let mut buckets = CleanBuckets::default();
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
            buckets.add(f);
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
            buckets,
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
        self.disks = chystik_core::disks::mount_table();
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
        dedup_nested_roots(&mut roots);
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

    /// Longest configured target containing `path`; anchors guard checks
    /// when several targets overlap.
    pub(crate) fn owning_root(&self, path: &Path) -> Option<&Path> {
        let refs: Vec<&Path> = self.targets.iter().map(|t| t.root.as_path()).collect();
        longest_containing(&refs, path)
    }
}

impl eframe::App for ChystikApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_scanner(ctx);
        self.ensure_view();

        if self.consent_pending {
            // Deliberately first and exclusive: no scan, no selection and no
            // deletion is reachable until this is answered.
            self.show_consent_modal(ctx);
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

        egui::TopBottomPanel::bottom("footer")
            .exact_height(space(15.0))
            .frame(
                egui::Frame::none()
                    .fill(COL_SURFACE)
                    .inner_margin(egui::Margin::symmetric(space(4.0), space(2.0))),
            )
            .show(ctx, |ui| {
                hairline_top(ui);
                self.footer_ui(ui);
            });

        egui::SidePanel::left("categories")
            .exact_width(SIDEBAR_W)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(COL_SURFACE)
                    .inner_margin(egui::Margin::symmetric(0.0, space(4.0))),
            )
            .show(ctx, |ui| {
                self.sidebar_ui(ui);
                // A hairline, not a void: the panels used to be separated by
                // a black gutter that read as a rendering gap.
                let r = ui.max_rect();
                ui.painter().vline(
                    r.right() - 0.5,
                    r.y_range(),
                    egui::Stroke::new(1.0_f32, COL_LINE),
                );
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(COL_SURFACE))
            .show(ctx, |ui| {
                self.detail_ui(ui);
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
        let (tx, rx) = channel::<ScanProgress>();
        let cancel_flag = Arc::new(AtomicBool::new(false));

        self.reset_results();
        self.progress_text = format!("Scanning {} target(s)\u{2026}", roots.len());
        self.notice = None;

        let exclusions = self.exclusions.clone();
        let cancel_for_thread = Arc::clone(&cancel_flag);
        let spawned = std::thread::Builder::new()
            .name("chystik-scanner".to_string())
            .spawn(move || {
                let options = chystik_core::scanner::ScanOptions {
                    exclude: exclusions,
                    ..chystik_core::scanner::ScanOptions::default()
                };
                let result = chystik_core::scanner::scan_many(
                    &roots,
                    &options,
                    tx.clone(),
                    &cancel_for_thread,
                );
                if let Err(e) = result {
                    if !matches!(e, chystik_core::ChystikError::Cancelled) {
                        eprintln!("[chystik] scan error: {e}");
                        // Best-effort terminal event so the UI never hangs.
                        let _ = tx.send(ScanProgress::Cancelled);
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
    /// `scan_many` emits exactly one terminal event for the whole run, so
    /// joining the worker here can never block the UI thread mid-scan.
    pub(crate) fn poll_scanner(&mut self, ctx: &egui::Context) {
        let s = self.s();
        loop {
            match self.rx.try_recv() {
                Ok(event) => match event {
                    ScanProgress::Started { root } => {
                        self.progress_text =
                            format!("{} {}\u{2026}", s.scanning_target.as_str(), root.display());
                    }
                    ScanProgress::DirectoriesScanned { count } => {
                        self.dir_count = count.max(self.dir_count);
                    }
                    ScanProgress::FindingFound(finding) => {
                        self.live_bytes += finding.size_bytes;
                        self.findings.push(*finding);
                    }
                    ScanProgress::Finished { findings } => {
                        // Wholesale replacement: the streamed list and the
                        // final one can have the same length with different
                        // contents, which the stamp cannot tell apart.
                        self.findings = findings;
                        self.view_stamp = None;
                        self.live_bytes = self.findings.iter().map(|f| f.size_bytes).sum();
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
                    ScanProgress::Cancelled => {
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
        let mut freed = 0u64;
        let mut moved = 0usize;
        let mut skipped = 0usize;
        let mut errors = Vec::new();

        for idx in indices {
            let Some(finding) = self.findings.get(idx) else {
                continue;
            };
            let path = finding.path.clone();
            let size = finding.size_bytes;

            // Advisory findings name system space Chystik cannot reclaim.
            // They are unselectable in the UI; this is the backstop.
            if !finding.is_actionable() {
                skipped += 1;
                continue;
            }
            // Enforced a second time here: an exclusion added after the scan
            // must still hold for findings already on screen.
            if crate::exclusions::is_excluded(&path, &self.exclusions) {
                skipped += 1;
                continue;
            }

            // Safety guard FIRST — refuse and log anything it rejects.
            // Candidates must live under a configured target; that owning
            // root is what the guard validates against.
            let Some(target_root) = self.owning_root(&path) else {
                skipped += 1;
                errors.push(format!(
                    "no scan target contains {}",
                    truncate_middle(&path.display().to_string(), 48)
                ));
                continue;
            };
            if let Err(e) = chystik_core::guard::check(&path, target_root) {
                eprintln!("[chystik] guard refused {}: {e}", path.display());
                skipped += 1;
                errors.push(format!(
                    "guard refused {}: {e}",
                    truncate_middle(&path.display().to_string(), 48)
                ));
                continue;
            }

            match trash::delete(&path) {
                Ok(()) => {
                    freed += size;
                    moved += 1;
                    self.deleted.insert(idx);
                }
                Err(e) => {
                    eprintln!("[chystik] trash::delete failed {}: {e}", path.display());
                    errors.push(format!(
                        "trash failed {}: {e}",
                        truncate_middle(&path.display().to_string(), 48)
                    ));
                }
            }
        }

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
        }
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

//! Panel rendering. One function per region of the window, all of them
//! reading from `ChystikApp` and the cached view.

use eframe::egui;
use egui_extras::Column;

use chrono::Utc;
use chystik_core::model::Severity;

use crate::app::ChystikApp;
use crate::format::*;
use crate::i18n;
use std::sync::atomic::Ordering;

use crate::state::*;
use crate::theme::*;
use crate::widgets::*;

impl ChystikApp {
    pub(crate) fn command_bar_ui(&mut self, ui: &mut egui::Ui) {
        let scanning = self.scanning();
        let (lang, s) = (self.lang, self.s());
        self.refresh_roots_display(); // cached behind `targets_signature`

        ui.horizontal_centered(|ui| {
            ui.label(txt(s.app_name.as_str(), "title", COL_TEXT));
            ui.add_space(space(3.0));

            // Built from the same helper as the settings button, so the
            // two match in height, padding and hover exactly. It marks
            // where you are; the action on this bar is Scan, and an
            // indicator must not compete with it. The shortcut lives in
            // the tooltip rather than taking width on screen.
            let dot_color = match self.section {
                Section::Cleanup => COL_ACCENT,
                Section::Disks => severity_color(Severity::Safe),
                Section::Privacy => severity_color(Severity::Moderate),
            };
            if icon_button(ui, self.section.label(s), move |painter, centre, _| {
                painter.rect_filled(
                    egui::Rect::from_center_size(centre, egui::vec2(7.0, 7.0)),
                    egui::Rounding::same(2.0),
                    dot_color,
                );
            })
            .on_hover_text(s.section_switch_hint.as_str())
            .clicked()
            {
                self.palette_open = true;
            }
            ui.add_space(space(2.0));

            let cleanup = self.section == Section::Cleanup;
            let target_snapshot: Vec<ScanTarget> = if cleanup {
                self.targets.clone()
            } else {
                Vec::new()
            };
            let mut toggle_actions: Vec<(usize, bool)> = Vec::new();
            let mut add_folder_requested = false;
            ui.menu_button(truncate_middle(&self.roots_display, 30), |ui| {
                ui.set_min_width(280.0);
                if target_snapshot.is_empty() {
                    ui.label(txt(s.no_disks.as_str(), "caption", COL_TEXT2));
                }
                for (i, t) in target_snapshot.iter().enumerate() {
                    let mut on = t.enabled;
                    if ui.checkbox(&mut on, &t.label).changed() {
                        toggle_actions.push((i, on));
                    }
                }
                ui.separator();
                if ui
                    .button(s.add_folder.as_str())
                    .on_hover_text(s.add_folder_hint.as_str())
                    .clicked()
                {
                    add_folder_requested = true;
                    ui.close_menu();
                }
            })
            .response
            .on_hover_text(s.targets_hint.as_str());
            for (i, on) in toggle_actions {
                if let Some(t) = self.targets.get_mut(i) {
                    t.enabled = on;
                }
            }
            if add_folder_requested {
                self.add_folder();
            }

            ui.add_space(space(2.0));
            let search_id = egui::Id::new("search_field");
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .id(search_id)
                    .desired_width(190.0)
                    .hint_text(txt(s.filter_hint.as_str(), "caption", COL_TEXT3)),
            )
            .on_hover_text(s.filter_tooltip.as_str());
            if ui.input(|i| i.key_pressed(egui::Key::Slash)) {
                ui.memory_mut(|m| m.request_focus(search_id));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if scanning {
                    if ghost_button(ui, s.cancel.as_str(), true)
                        .on_hover_text(s.cancel_hint.as_str())
                        .clicked()
                    {
                        if let ScanState::Scanning { cancel_flag, .. } = &self.state {
                            cancel_flag.store(true, Ordering::SeqCst);
                            self.progress_text = s.cancelling.to_string();
                        }
                    }
                } else if primary_button(ui, s.scan.as_str(), COL_ACCENT, self.roots_nonempty)
                    .on_hover_text(s.scan_hint.as_str())
                    .clicked()
                {
                    self.start_scan();
                }
                if ghost_button(ui, s.refresh_disks.as_str(), !scanning)
                    .on_hover_text(s.refresh_disks_hint.as_str())
                    .clicked()
                {
                    self.refresh_disks();
                }
                if icon_button(ui, lang.code(), draw_settings_mark)
                    .on_hover_text(s.settings_hint.as_str())
                    .clicked()
                {
                    self.settings_open = true;
                }
            });
        });
    }

    // -- scan status ---------------------------------------------------------

    pub(crate) fn scan_status_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let s = self.s();
        ui.horizontal_centered(|ui| {
            if self.scanning() {
                // A real indeterminate bar: `ProgressBar::new(1.0)` reads as
                // "finished" to every user who sees it.
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(150.0, 4.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, egui::Rounding::same(2.0), COL_LINE);
                let t = (ctx.input(|i| i.time) as f32 * 0.85).fract();
                let sliver_w = rect.width() * 0.3;
                let left = rect.left() - sliver_w + (rect.width() + sliver_w) * t;
                let x0 = left.max(rect.left());
                let x1 = (left + sliver_w).min(rect.right());
                if x1 > x0 {
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(x0, rect.top()),
                            egui::pos2(x1, rect.bottom()),
                        ),
                        egui::Rounding::same(2.0),
                        COL_ACCENT,
                    );
                }
                ui.add_space(space(3.0));
                ui.label(txt(
                    format!(
                        "{} {} \u{b7} {} {}",
                        self.dir_count,
                        s.scanning_dirs.as_str(),
                        format_size(self.live_bytes),
                        s.found_so_far.as_str()
                    ),
                    "caption",
                    COL_TEXT2,
                ));
            } else if self.findings.is_empty() {
                ui.label(txt(s.press_scan.as_str(), "caption", COL_TEXT3));
            } else {
                ui.label(txt(&self.progress_text, "caption", COL_TEXT2));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(txt(capacity_summary(&self.disks), "mono_sm", COL_TEXT3));
            });
        });
    }

    // -- sidebar -------------------------------------------------------------

    pub(crate) fn sidebar_ui(&mut self, ui: &mut egui::Ui) {
        let (lang, s) = (self.lang, self.s());
        let buckets = CleanBuckets {
            safe_bytes: self.view.cat_stats.iter().map(|c| c.safe_bytes).sum(),
            moderate_bytes: self.view.cat_stats.iter().map(|c| c.moderate_bytes).sum(),
            risky_bytes: self.view.cat_stats.iter().map(|c| c.risky_bytes).sum(),
        };

        ui.vertical(|ui| {
            ui.add_space(space(1.0));
            ui.horizontal(|ui| {
                ui.add_space(SIDEBAR_PAD);
                ui.vertical(|ui| {
                    ui.label(txt(s.reclaimable.as_str(), "micro", COL_TEXT3));
                    ui.add_space(space(0.5));
                    ui.label(txt(format_size(self.view.all_bytes), "display", COL_TEXT));
                    ui.add_space(space(1.0));
                    ui.label(txt(
                        i18n::fill(
                            s.items_in_categories.as_str(),
                            &[
                                ("n", &self.view.all_count.to_string()),
                                ("c", &self.view.cat_stats.len().to_string()),
                            ],
                        ),
                        "caption",
                        COL_TEXT2,
                    ));
                    ui.add_space(space(2.5));
                    severity_bar(ui, buckets, SIDEBAR_W - SIDEBAR_PAD * 2.0, 6.0);
                    ui.add_space(space(1.5));
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space(1.0);
                        for (bytes, sev) in [
                            (buckets.safe_bytes, Severity::Safe),
                            (buckets.moderate_bytes, Severity::Moderate),
                            (buckets.risky_bytes, Severity::Risky),
                        ] {
                            paint_severity_glyph(ui, sev, 8.0, severity_color(sev));
                            ui.label(txt(format_size(bytes), "caption", COL_TEXT2))
                                .on_hover_text(format!(
                                    "{} \u{2014} {}",
                                    i18n::severity_label(lang, sev),
                                    i18n::severity_cost(lang, sev)
                                ));
                            ui.add_space(space(1.5));
                        }
                    });
                });
            });

            ui.add_space(space(3.5));
            self.severity_segments_ui(ui);
            ui.add_space(space(2.5));

            let selected = self.category_filter;
            let mut clicked: Option<CategoryFilter> = None;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.view.cat_stats.is_empty() {
                        ui.add_space(space(4.0));
                        ui.horizontal(|ui| {
                            ui.add_space(SIDEBAR_PAD);
                            ui.label(txt(s.nothing_found.as_str(), "caption", COL_TEXT3));
                        });
                        return;
                    }
                    if category_row(
                        ui,
                        None,
                        self.view.all_bytes,
                        self.view.all_count,
                        selected == CategoryFilter::All,
                        lang,
                        s,
                    ) {
                        clicked = Some(CategoryFilter::All);
                    }
                    for stat in &self.view.cat_stats {
                        if category_row(
                            ui,
                            Some(*stat),
                            stat.bytes,
                            stat.count,
                            selected == CategoryFilter::One(stat.category),
                            lang,
                            s,
                        ) {
                            clicked = Some(CategoryFilter::One(stat.category));
                        }
                    }
                    ui.add_space(space(3.0));
                });
            if let Some(next) = clicked {
                self.category_filter = next;
            }
        });
    }

    /// Three-way severity segmented control, replacing the combo box.
    /// Four severity filters as a 2x2 grid.
    ///
    /// A wrapping row put the second line back at the container's left edge
    /// instead of the sidebar indent, so the control had two different left
    /// margins. Ukrainian labels are half again as long as the English ones,
    /// which is what made it wrap in the first place.
    pub(crate) fn severity_segments_ui(&mut self, ui: &mut egui::Ui) {
        let (lang, s) = (self.lang, self.s());
        let options = [
            (
                SeverityFilter::All,
                s.filter_all.as_str(),
                COL_TEXT,
                s.filter_all_hint.as_str(),
            ),
            (
                SeverityFilter::One(Severity::Safe),
                i18n::severity_label(lang, Severity::Safe),
                severity_color(Severity::Safe),
                s.filter_safe_hint.as_str(),
            ),
            (
                SeverityFilter::One(Severity::Moderate),
                s.filter_review.as_str(),
                severity_color(Severity::Moderate),
                s.filter_review_hint.as_str(),
            ),
            (
                SeverityFilter::One(Severity::Risky),
                i18n::severity_label(lang, Severity::Risky),
                severity_color(Severity::Risky),
                s.filter_risky_hint.as_str(),
            ),
        ];

        const GAP: f32 = 6.0;
        let cell = (SIDEBAR_W - SIDEBAR_PAD * 2.0 - GAP) / 2.0;
        let mut chosen: Option<SeverityFilter> = None;

        egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(SIDEBAR_PAD, 0.0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
                egui::Grid::new("severity_segments")
                    .num_columns(2)
                    .spacing(egui::vec2(GAP, GAP))
                    .show(ui, |ui| {
                        for (i, (value, label, color, hint)) in options.into_iter().enumerate() {
                            let active = self.severity_filter == value;
                            let (fill, stroke, fg) = if active {
                                (COL_ACCENT_SOFT, COL_ACCENT, COL_TEXT)
                            } else {
                                (egui::Color32::TRANSPARENT, COL_LINE, color)
                            };
                            if ui
                                .add(
                                    egui::Button::new(txt(label, "micro", fg))
                                        .fill(fill)
                                        .stroke(egui::Stroke::new(1.0_f32, stroke))
                                        .rounding(egui::Rounding::same(R_MD))
                                        .min_size(egui::vec2(cell, space(6.5))),
                                )
                                .on_hover_text(hint)
                                .clicked()
                            {
                                chosen = Some(value);
                            }
                            if i % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
            });

        if let Some(value) = chosen {
            self.severity_filter = value;
        }
    }

    // -- detail --------------------------------------------------------------

    pub(crate) fn detail_ui(&mut self, ui: &mut egui::Ui) {
        let (lang, s) = (self.lang, self.s());
        let (title, subtitle) = match self.category_filter {
            CategoryFilter::All => (
                s.everything_found.to_string(),
                s.everything_found_sub.to_string(),
            ),
            CategoryFilter::One(c) => {
                let stat = self
                    .view
                    .cat_stats
                    .iter()
                    .find(|x| x.category == c)
                    .copied()
                    .unwrap_or_else(|| CatStat::new(c));
                (
                    i18n::category_label(lang, c).to_string(),
                    format!(
                        "{} \u{b7} {} {} \u{b7} {}",
                        format_size(stat.bytes),
                        stat.count,
                        if stat.count == 1 {
                            s.item_word.as_str()
                        } else {
                            s.items_word.as_str()
                        },
                        i18n::category_description(lang, c),
                    ),
                )
            }
        };

        // Rows the bulk action may legitimately touch: never Risky.
        let selectable: Vec<usize> = self
            .view
            .rows
            .iter()
            .copied()
            .filter(|i| is_bulk_safe_finding(&self.findings[*i]))
            .collect();
        let selectable_bytes: u64 = selectable
            .iter()
            .map(|i| self.findings[*i].size_bytes)
            .sum();
        let all_selected =
            !selectable.is_empty() && selectable.iter().all(|i| self.selected.contains(i));

        let mut bulk: Option<bool> = None;
        egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(space(6.0), space(4.0)))
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_max_width(ui.available_width() - 240.0);
                        ui.label(txt(title, "title", COL_TEXT));
                        ui.add_space(space(1.0));
                        ui.label(txt(subtitle, "caption", COL_TEXT2));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        let (label, hint) = if all_selected {
                            (
                                i18n::fill(
                                    s.clear_selection.as_str(),
                                    &[("n", &selectable.len().to_string())],
                                ),
                                s.clear_selection_hint.as_str(),
                            )
                        } else {
                            (
                                i18n::fill(
                                    s.select_safe.as_str(),
                                    &[
                                        ("n", &selectable.len().to_string()),
                                        ("size", &format_size(selectable_bytes)),
                                    ],
                                ),
                                s.select_safe_hint.as_str(),
                            )
                        };
                        if ui
                            .add_enabled(
                                !selectable.is_empty(),
                                egui::Button::new(txt(
                                    label,
                                    "strong",
                                    if selectable.is_empty() {
                                        COL_TEXT3
                                    } else {
                                        COL_TEXT
                                    },
                                ))
                                .fill(COL_RAISED)
                                .stroke(egui::Stroke::new(1.0_f32, COL_LINE_HI))
                                .rounding(egui::Rounding::same(R_MD))
                                .min_size(egui::vec2(0.0, space(8.0))),
                            )
                            .on_hover_text(hint)
                            .clicked()
                        {
                            bulk = Some(!all_selected);
                        }
                    });
                });
            });

        if let Some(select) = bulk {
            if select {
                self.selected.extend(selectable);
            } else {
                for i in &selectable {
                    self.selected.remove(i);
                }
            }
        }

        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.cursor().top(),
            egui::Stroke::new(1.0_f32, COL_LINE),
        );
        self.table_ui(ui);
    }

    pub(crate) fn table_ui(&mut self, ui: &mut egui::Ui) {
        let (lang, s) = (self.lang, self.s());
        let rows = &self.view.rows;
        let scanning = self.scanning();
        let findings = &self.findings;
        let selected = &self.selected;
        let sort_col = self.sort_col;
        let sort_asc = self.sort_asc;
        let mut row_toggles: Vec<(usize, bool)> = Vec::new();
        let mut sort_click: Option<(SortCol, bool)> = None;
        let mut exclude_request: Option<std::path::PathBuf> = None;
        let mut copied = false;

        if rows.is_empty() {
            ui.add_space(space(14.0));
            ui.vertical_centered(|ui| {
                let (title, body) = if scanning {
                    (s.scanning_title.as_str(), s.scanning_body.as_str())
                } else if findings.is_empty() {
                    (s.empty_title.as_str(), s.empty_body.as_str())
                } else {
                    (
                        s.empty_filtered_title.as_str(),
                        s.empty_filtered_body.as_str(),
                    )
                };
                ui.label(txt(title, "title", COL_TEXT2));
                ui.add_space(space(1.5));
                ui.set_max_width(420.0);
                ui.label(txt(body, "caption", COL_TEXT3));
            });
            return;
        }

        let age_now = Utc::now();
        // Breathing room at both edges: rows used to run flush into the
        // panel walls.
        egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(space(4.0), 0.0))
            .show(ui, |ui| {
                egui_extras::TableBuilder::new(ui)
                    .striped(false)
                    // Cells sense `hover` by default, which made every
                    // `header.col(..).1.clicked()` permanently false: the
                    // sort arrows moved but nothing ever sorted.
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::exact(space(8.0)))
                    .column(Column::remainder().at_least(220.0))
                    .column(Column::exact(94.0))
                    .column(Column::exact(84.0))
                    .column(Column::exact(84.0))
                    .vscroll(true)
                    .auto_shrink([false, false])
                    .min_scrolled_height(120.0)
                    .max_scroll_height(f32::INFINITY)
                    .header(space(9.0), |mut header| {
                        let sortable = |header: &mut egui_extras::TableRow<'_, '_>,
                                        col: SortCol,
                                        label: &str,
                                        right: bool| {
                            let resp = header
                                .col(|ui| {
                                    let layout = if right {
                                        egui::Layout::right_to_left(egui::Align::Center)
                                    } else {
                                        egui::Layout::left_to_right(egui::Align::Center)
                                    };
                                    ui.with_layout(layout, |ui| {
                                        ui.spacing_mut().item_spacing.x = space(1.0);
                                        ui.label(txt(label, "micro", COL_TEXT3));
                                        if sort_col == col {
                                            paint_sort_arrow(ui, sort_asc);
                                        }
                                    });
                                })
                                .1;
                            resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(s.sort_hint.as_str())
                                .clicked()
                        };
                        header.col(|_| {});
                        let path_c =
                            sortable(&mut header, SortCol::Path, s.col_path.as_str(), false);
                        let size_c =
                            sortable(&mut header, SortCol::Size, s.col_size.as_str(), true);
                        let sev_c =
                            sortable(&mut header, SortCol::Severity, s.col_risk.as_str(), false);
                        let age_c = sortable(&mut header, SortCol::Age, s.col_age.as_str(), false);

                        sort_click = if size_c {
                            Some((SortCol::Size, false))
                        } else if path_c {
                            Some((SortCol::Path, true))
                        } else if age_c {
                            Some((SortCol::Age, true))
                        } else if sev_c {
                            Some((SortCol::Severity, true))
                        } else {
                            None
                        };
                    })
                    .body(|body| {
                        // Only rows intersecting the scroll window are built.
                        // The per-row `TableBody::row` API renders EVERY row
                        // and froze the window at ~6 fps on a large scan.
                        body.rows(ROW_H, rows.len(), |mut row| {
                            let idx = rows[row.index()];
                            let finding = &findings[idx];
                            let risky = finding.severity == Severity::Risky;
                            let mut checked = selected.contains(&idx);

                            row.col(|ui| {
                                if !finding.is_actionable() {
                                    paint_info_mark(ui, 13.0, COL_ACCENT)
                                        .on_hover_text(finding_tooltip(lang, finding));
                                } else if risky {
                                    // Never bulk-selectable: say why instead
                                    // of showing a dead checkbox.
                                    paint_severity_glyph(
                                        ui,
                                        Severity::Risky,
                                        9.0,
                                        severity_color(Severity::Risky),
                                    );
                                    ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover())
                                        .on_hover_text(s.risky_locked_hint.as_str());
                                } else if ui.checkbox(&mut checked, "").changed() {
                                    row_toggles.push((idx, checked));
                                }
                            });

                            row.col(|ui| {
                                let full = finding.path.display().to_string();
                                // Dim the directory prefix so the eye lands
                                // on the last component, which is what
                                // identifies the item.
                                let (head, tail) = split_path_tail(&full);
                                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                                ui.vertical(|ui| {
                                    ui.add_space(space(1.0));
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        // Enough height for descenders and
                                        // the underscore in `node_modules`,
                                        // which the tight stack clipped.
                                        ui.set_min_height(17.0);
                                        // Truncated by PIXEL width, not
                                        // character count: a fixed character
                                        // budget makes every row end in a
                                        // different place.
                                        ui.add(
                                            egui::Label::new(txt(head, "mono_sm", COL_TEXT3))
                                                .truncate(),
                                        );
                                        ui.add(
                                            egui::Label::new(txt(tail, "strong", COL_TEXT))
                                                .truncate(),
                                        );
                                    });
                                    ui.add_space(1.0);
                                    match finding.advice.as_deref() {
                                        // For advisory rows the command IS
                                        // the useful line; the note is in
                                        // the tooltip.
                                        Some(command) => {
                                            // Clicking copies it: the row is
                                            // useless unless the command can
                                            // reach a terminal.
                                            let hit = ui.add(
                                                egui::Label::new(txt(
                                                    command, "mono_sm", COL_ACCENT,
                                                ))
                                                .truncate()
                                                .sense(egui::Sense::click()),
                                            );
                                            if hit
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .on_hover_text(s.advice_copy.as_str())
                                                .clicked()
                                            {
                                                ui.output_mut(|o| {
                                                    o.copied_text = command.to_owned()
                                                });
                                                copied = true;
                                            }
                                        }
                                        None => {
                                            ui.add(
                                                egui::Label::new(txt(
                                                    &finding.note,
                                                    "micro",
                                                    COL_TEXT3,
                                                ))
                                                .truncate(),
                                            );
                                        }
                                    }
                                })
                                .response
                                .on_hover_text(finding_tooltip(lang, finding))
                                .context_menu(|ui| {
                                    if ui.button(s.exclusions_add.as_str()).clicked() {
                                        exclude_request = Some(finding.path.clone());
                                        ui.close_menu();
                                    }
                                });
                            });

                            row.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(txt(
                                            format_size(finding.size_bytes),
                                            "mono_lg",
                                            COL_TEXT,
                                        ));
                                    },
                                );
                            });

                            row.col(|ui| {
                                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                                    ui.add_space((ROW_H - 19.0) / 2.0);
                                    severity_pill(ui, finding.severity, lang);
                                });
                            });

                            row.col(|ui| {
                                let stale = finding
                                    .last_used
                                    .is_some_and(|t| (age_now - t).num_days() >= 180);
                                let color = if stale {
                                    severity_color(Severity::Moderate)
                                } else {
                                    COL_TEXT3
                                };
                                ui.label(txt(
                                    age_label(finding.last_used, age_now, s),
                                    "caption",
                                    color,
                                ));
                            });
                        });
                    });
            });

        if let Some((col, default_asc)) = sort_click {
            self.apply_header_click(col, default_asc);
        }
        if let Some(path) = exclude_request {
            self.exclude_path(path);
        }
        if copied {
            self.notice = Some(crate::app::Notice {
                title: s.advice_copied.clone(),
                lines: vec![s.advice_run.clone()],
            });
        }
        for (idx, checked) in row_toggles {
            if checked {
                self.selected.insert(idx);
            } else {
                self.selected.remove(&idx);
            }
        }
    }

    // -- footer ---------------------------------------------------------------

    pub(crate) fn footer_ui(&mut self, ui: &mut egui::Ui) {
        let s = self.s();
        let selected = self.selected_visible_rows();
        let sel_count = selected.len();
        let sel_bytes: u64 = selected.iter().map(|(_, f)| f.size_bytes).sum();
        let busy = self.scanning();
        let review_bytes = self.view.buckets.risky_total();

        ui.horizontal_centered(|ui| {
            if sel_count == 0 {
                ui.label(txt(
                    i18n::fill(
                        s.shown_needs_review.as_str(),
                        &[
                            ("n", &self.view.rows.len().to_string()),
                            ("size", &format_size(review_bytes)),
                        ],
                    ),
                    "caption",
                    COL_TEXT3,
                ));
                let advisory_bytes: u64 = self
                    .view
                    .rows
                    .iter()
                    .map(|i| &self.findings[*i])
                    .filter(|f| !f.is_actionable())
                    .map(|f| f.size_bytes)
                    .sum();
                if advisory_bytes > 0 {
                    ui.label(txt("\u{b7}", "caption", COL_TEXT3));
                    ui.label(txt(
                        i18n::fill(
                            s.shown_advisory.as_str(),
                            &[("size", &format_size(advisory_bytes))],
                        ),
                        "caption",
                        COL_ACCENT,
                    ));
                }
            } else {
                ui.label(txt(sel_count.to_string(), "strong", COL_TEXT));
                ui.label(txt(s.selected_word.as_str(), "caption", COL_TEXT2));
                ui.label(txt("\u{b7}", "caption", COL_TEXT3));
                ui.label(txt(format_size(sel_bytes), "mono_lg", COL_TEXT));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cleanup_available = self.cleanup_available();
                let can_delete = sel_count > 0 && !busy && cleanup_available;
                // Destructive action: filled, red, and to the right of the
                // escape hatch. It used to look identical to Cancel.
                // Without the branch the idle button reads "Move 0 B to Trash".
                let trash_label = if can_delete {
                    i18n::fill(
                        s.move_to_trash.as_str(),
                        &[("size", &format_size(sel_bytes))],
                    )
                } else {
                    s.move_to_trash_idle.to_string()
                };
                if primary_button(
                    ui,
                    &trash_label,
                    severity_color(Severity::Risky),
                    can_delete,
                )
                .on_hover_text(if cleanup_available {
                    s.move_to_trash_hint.as_str()
                } else {
                    s.cleanup_unavailable.as_str()
                })
                .clicked()
                {
                    self.confirm_delete_open = true;
                }
                if ghost_button(ui, s.clear.as_str(), sel_count > 0)
                    .on_hover_text(s.clear_hint.as_str())
                    .clicked()
                {
                    self.selected.clear();
                }
                if !self.disks.is_empty() {
                    let summary = self
                        .disks
                        .iter()
                        .map(|d| format!("{}  {}", d.mount_point.display(), disk_usage_label(d)))
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.label(txt(s.disks.as_str(), "caption", COL_TEXT3))
                        .on_hover_text(format!("{}\n\n{}", s.disks_hint.as_str(), summary));
                }
            });
        });
    }
}

impl ChystikApp {
    // -- disks ---------------------------------------------------------------

    /// What is attached, mounted or not.
    ///
    /// `df` and the file manager both answer "what is mounted"; the number
    /// this view exists for is the other one. On the development machine
    /// 1.5 TB of the 1.75 TB attached is in partitions nothing has mounted,
    /// and no other tool says so.
    pub(crate) fn disks_ui(&mut self, ui: &mut egui::Ui) {
        use chystik_core::blockdev::{self, PartitionUse};

        let s = self.s();
        let attached = blockdev::total_attached_bytes(&self.drives);
        let idle = blockdev::total_unmounted_bytes(&self.drives);
        let in_use: u64 = self.drives.iter().map(|d| d.used_bytes()).sum();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none()
                    .inner_margin(egui::Margin::symmetric(space(6.0), space(5.0)))
                    .show(ui, |ui| {
                        if self.drives.is_empty() {
                            ui.label(txt(s.disks_none.as_str(), "caption", COL_TEXT3));
                            return;
                        }

                        ui.horizontal_top(|ui| {
                            for (label, value, color) in [
                                (s.disks_attached.as_str(), attached, COL_TEXT),
                                (s.disks_in_use.as_str(), in_use, COL_TEXT2),
                            ] {
                                ui.vertical(|ui| {
                                    ui.label(txt(label, "micro", COL_TEXT3));
                                    ui.label(txt(format_size(value), "display", color));
                                });
                                ui.add_space(space(8.0));
                            }
                        });

                        if idle > 0 {
                            ui.add_space(space(4.0));
                            egui::Frame::default()
                                .fill(COL_SURFACE)
                                .stroke(egui::Stroke::new(
                                    1.0_f32,
                                    severity_color(Severity::Moderate),
                                ))
                                .rounding(egui::Rounding::same(R_LG))
                                .inner_margin(egui::Margin::same(space(4.0)))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        paint_severity_glyph(
                                            ui,
                                            Severity::Moderate,
                                            11.0,
                                            severity_color(Severity::Moderate),
                                        );
                                        ui.add_space(space(2.0));
                                        ui.vertical(|ui| {
                                            ui.label(txt(
                                                i18n::fill(
                                                    s.disks_idle_banner.as_str(),
                                                    &[("size", &format_size(idle))],
                                                ),
                                                "strong",
                                                COL_TEXT,
                                            ));
                                            ui.add_space(space(0.5));
                                            ui.label(txt(
                                                s.disks_idle_explain.as_str(),
                                                "caption",
                                                COL_TEXT2,
                                            ));
                                        });
                                    });
                                });
                        }

                        ui.add_space(space(5.0));
                        for drive in &self.drives {
                            egui::Frame::default()
                                .fill(COL_SURFACE)
                                .stroke(egui::Stroke::new(1.0_f32, COL_LINE))
                                .rounding(egui::Rounding::same(R_LG))
                                .inner_margin(egui::Margin::symmetric(space(4.5), space(4.0)))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(txt(&drive.name, "mono_lg", COL_TEXT));
                                        ui.add_space(space(1.5));
                                        ui.label(txt(&drive.model, "caption", COL_TEXT2));
                                        ui.add_space(space(1.0));
                                        egui::Frame::default()
                                            .stroke(egui::Stroke::new(1.0_f32, COL_LINE_HI))
                                            .rounding(egui::Rounding::same(R_SM))
                                            .inner_margin(egui::Margin::symmetric(
                                                space(1.5),
                                                space(0.25),
                                            ))
                                            .show(ui, |ui| {
                                                ui.label(txt(
                                                    drive.kind.label(),
                                                    "micro",
                                                    COL_TEXT3,
                                                ));
                                            });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(txt(
                                                    format_size(drive.size_bytes),
                                                    "mono_lg",
                                                    COL_TEXT,
                                                ));
                                            },
                                        );
                                    });
                                    ui.add_space(space(2.5));

                                    for partition in &drive.partitions {
                                        ui.horizontal(|ui| {
                                            ui.set_min_height(22.0);
                                            ui.label(txt(&partition.name, "mono_sm", COL_TEXT2));
                                            ui.add_space(space(2.0));
                                            ui.label(txt(
                                                format_size(partition.size_bytes),
                                                "mono_sm",
                                                COL_TEXT,
                                            ));
                                            ui.add_space(space(3.0));
                                            match &partition.usage {
                                                PartitionUse::Filesystem(m) => {
                                                    ui.label(txt(&m.fs_type, "micro", COL_TEXT3));
                                                    ui.add_space(space(1.5));
                                                    ui.label(txt(
                                                        m.mount_point.display().to_string(),
                                                        "mono_sm",
                                                        COL_TEXT,
                                                    ));
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            ui.label(txt(
                                                                i18n::fill(
                                                                    s.disks_used.as_str(),
                                                                    &[(
                                                                        "pct",
                                                                        &format!(
                                                                            "{:.0}",
                                                                            m.used_fraction()
                                                                                * 100.0
                                                                        ),
                                                                    )],
                                                                ),
                                                                "caption",
                                                                COL_TEXT2,
                                                            ));
                                                            usage_bar(
                                                                ui,
                                                                m.used_fraction(),
                                                                160.0,
                                                                COL_ACCENT,
                                                            );
                                                        },
                                                    );
                                                }
                                                PartitionUse::Swap => {
                                                    ui.label(txt(
                                                        s.disks_swap.as_str(),
                                                        "caption",
                                                        COL_TEXT2,
                                                    ));
                                                }
                                                PartitionUse::Idle => {
                                                    ui.label(txt(
                                                        s.disks_not_mounted.as_str(),
                                                        "caption",
                                                        severity_color(Severity::Moderate),
                                                    ));
                                                }
                                            }
                                        });
                                    }
                                });
                            ui.add_space(space(3.0));
                        }
                    });
            });
    }

    // -- privacy -------------------------------------------------------------

    /// Traces of what you did, measured by what they reveal.
    pub(crate) fn privacy_ui(&mut self, ui: &mut egui::Ui) {
        let s = self.s();
        let mut toggles: Vec<(usize, bool)> = Vec::new();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none()
                    .inner_margin(egui::Margin::symmetric(space(6.0), space(5.0)))
                    .show(ui, |ui| {
                        ui.label(txt(s.privacy_title.as_str(), "title", COL_TEXT));
                        ui.add_space(space(1.5));
                        ui.label(txt(s.privacy_lead.as_str(), "caption", COL_TEXT2));
                        ui.add_space(space(1.0));
                        ui.label(txt(
                            s.privacy_nothing_preselected.as_str(),
                            "micro",
                            COL_TEXT3,
                        ));
                        ui.add_space(space(4.0));

                        if self.traces.is_empty() {
                            ui.label(txt(s.privacy_none.as_str(), "caption", COL_TEXT3));
                            return;
                        }

                        for (i, trace) in self.traces.iter().enumerate() {
                            let mut ticked = self.traces_selected.contains(&i);
                            let color = severity_color(trace.severity);
                            ui.horizontal_top(|ui| {
                                ui.add_space(space(0.5));
                                if ui.checkbox(&mut ticked, "").changed() {
                                    toggles.push((i, ticked));
                                }
                                // A severity-coloured rule: the row's weight
                                // is what it costs, not what it occupies.
                                let (rule, _) = ui.allocate_exact_size(
                                    egui::vec2(4.0, space(13.0)),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .rect_filled(rule, egui::Rounding::same(2.0), color);
                                ui.add_space(space(2.5));
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(txt(trace.kind.label(), "strong", COL_TEXT));
                                        ui.add_space(space(1.5));
                                        ui.label(txt(
                                            short_home_path(&trace.path),
                                            "mono_sm",
                                            COL_TEXT3,
                                        ));
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(txt(
                                                    format_size(trace.size_bytes),
                                                    "mono_lg",
                                                    COL_TEXT,
                                                ));
                                            },
                                        );
                                    });
                                    ui.add_space(space(0.5));
                                    ui.label(txt(
                                        format!(
                                            "{}: {}",
                                            s.privacy_reveals.as_str(),
                                            trace.reveals
                                        ),
                                        "caption",
                                        COL_TEXT2,
                                    ));
                                    ui.label(txt(
                                        format!("{}: {}", s.privacy_cost.as_str(), trace.cost),
                                        "micro",
                                        color,
                                    ));
                                });
                            });
                            ui.add_space(space(2.0));
                            let line = ui.max_rect();
                            ui.painter().hline(
                                line.x_range(),
                                ui.cursor().top(),
                                egui::Stroke::new(1.0_f32, COL_LINE),
                            );
                            ui.add_space(space(2.0));
                        }
                    });
            });

        for (i, ticked) in toggles {
            if ticked {
                self.traces_selected.insert(i);
            } else {
                self.traces_selected.remove(&i);
            }
        }
    }

    /// Footer for the privacy view: what is ticked, and the one action.
    pub(crate) fn privacy_footer_ui(&mut self, ui: &mut egui::Ui) {
        let s = self.s();
        let count = self.traces_selected.len();
        let bytes = self.selected_trace_bytes();
        ui.horizontal_centered(|ui| {
            if count > 0 {
                ui.label(txt(
                    i18n::fill(
                        s.privacy_selected.as_str(),
                        &[("n", &count.to_string()), ("size", &format_size(bytes))],
                    ),
                    "caption",
                    COL_TEXT2,
                ));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if count > 0 {
                    i18n::fill(s.privacy_clear.as_str(), &[("n", &count.to_string())])
                } else {
                    s.privacy_clear_idle.clone()
                };
                if primary_button(
                    ui,
                    &label,
                    severity_color(Severity::Risky),
                    count > 0 && !self.scanning() && self.cleanup_available(),
                )
                .on_hover_text(if self.cleanup_available() {
                    s.move_to_trash_hint.as_str()
                } else {
                    s.cleanup_unavailable.as_str()
                })
                .clicked()
                {
                    // Always confirmed, never acted on directly: this
                    // erases a record of what someone did, and there is no
                    // manifest step in front of it the way there is for a
                    // cleanup.
                    self.privacy_confirm_open = true;
                }
            });
        });
    }
}

/// Explain not only the severity but the authority and evidence behind one
/// finding. Keeping this at the UI boundary leaves the scanner/CLI contract
/// machine-readable while making the same evidence discoverable by hover.
fn finding_tooltip(lang: i18n::Lang, finding: &chystik_core::model::Finding) -> String {
    let strings = i18n::strings(lang);
    let mut lines = vec![
        finding.path.display().to_string(),
        finding.note.clone(),
        format!(
            "{} — {}",
            i18n::severity_label(lang, finding.severity),
            i18n::severity_cost(lang, finding.severity),
        ),
        i18n::policy_label(lang, finding.policy()).to_owned(),
    ];
    if let Some(provenance) = &finding.provenance {
        lines.push(format!("{}: {}", strings.evidence_rule, provenance.rule_id));
        lines.push(format!(
            "{}: {}",
            strings.evidence_recovery, provenance.recovery_cost
        ));
        lines.push(format!(
            "{}: {}",
            strings.evidence_source, provenance.source_url
        ));
        lines.push(format!(
            "{}: {}",
            strings.evidence_reviewed, provenance.reviewed_at
        ));
        if !provenance.preconditions.is_empty() {
            lines.push(format!(
                "{}:\n{}",
                strings.evidence_preconditions,
                provenance
                    .preconditions
                    .iter()
                    .map(|condition| format!("• {condition}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    if let Some(advice) = &finding.advice {
        lines.push(format!("{}\n{}", strings.advice_run, advice));
    }
    lines.join("\n\n")
}

/// Bulk selection is a stricter promise than a manual tick: only findings
/// that are both cheap to recreate and explicitly `DirectSafe` may enter it.
/// A `DirectReview` finding remains available as a deliberate per-row choice.
fn is_bulk_safe_finding(finding: &chystik_core::model::Finding) -> bool {
    finding.severity == Severity::Safe
        && finding.policy() == chystik_core::model::FindingPolicy::DirectSafe
}

/// A horizontal capacity bar.
fn usage_bar(ui: &mut egui::Ui, fraction: f32, width: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    let rounding = egui::Rounding::same(3.0);
    ui.painter().rect_filled(rect, rounding, COL_LINE);
    let filled = (fraction.clamp(0.0, 1.0) * rect.width()).max(2.0);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(filled, rect.height())),
        rounding,
        color,
    );
}

/// The platform home directory collapsed to `~`, which is how paths are recognised.
fn short_home_path(path: &std::path::Path) -> String {
    let full = path.display().to_string();
    let home = chystik_core::platform::current().app_paths().home_dir;
    full.strip_prefix(&home.to_string_lossy().into_owned())
        .map(|tail| format!("~{tail}"))
        .unwrap_or(full)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chystik_core::model::{Category, Finding, FindingPolicy, RuleProvenance, Severity};

    use super::{finding_tooltip, is_bulk_safe_finding};

    fn finding(policy: Option<FindingPolicy>) -> Finding {
        Finding {
            path: PathBuf::from("/tmp/finding"),
            category: Category::PackageCaches,
            severity: Severity::Safe,
            size_bytes: 1,
            last_used: None,
            mount: None,
            note: "fixture".into(),
            advice: None,
            provenance: policy.map(|policy| RuleProvenance {
                rule_id: "fixture".into(),
                source_url: "https://example.test".into(),
                policy,
                recovery_cost: "fixture".into(),
                reviewed_at: "2026-08-26".into(),
                preconditions: vec!["fixture precondition".into()],
            }),
        }
    }

    #[test]
    fn bulk_selection_accepts_only_direct_safe_policy() {
        assert!(is_bulk_safe_finding(&finding(None)));
        assert!(is_bulk_safe_finding(&finding(Some(
            FindingPolicy::DirectSafe
        ))));
        assert!(!is_bulk_safe_finding(&finding(Some(
            FindingPolicy::DirectReview
        ))));
        assert!(!is_bulk_safe_finding(&finding(Some(
            FindingPolicy::VendorCommandOnly
        ))));
    }

    #[test]
    fn tooltip_exposes_catalog_review_and_conditions() {
        let tooltip = finding_tooltip(
            crate::i18n::Lang::En,
            &finding(Some(FindingPolicy::DirectSafe)),
        );
        assert!(tooltip.contains("Last reviewed: 2026-08-26"));
        assert!(tooltip.contains("Conditions:\n• fixture precondition"));
    }
}

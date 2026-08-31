//! Modal dialogs: the deletion manifest, settings/about, result notices,
//! and the first-run risk acknowledgement.

use eframe::egui;

use std::path::PathBuf;

use chystik_core::model::Severity;

use crate::app::{ChystikApp, Notice};
use crate::format::*;
use crate::i18n::{self, Lang};
use crate::state::Section;
use crate::theme::*;
use crate::widgets::*;

impl ChystikApp {
    /// First-run risk acknowledgement.
    ///
    /// Shown before anything else and dismissible only by an explicit tick
    /// plus Continue — no Escape, no click-away. A tool that deletes should
    /// make the user say once, in their own click, that they understand what
    /// that means.
    /// Section picker, opened with Ctrl+K.
    ///
    /// The modeless direction: no permanent tab strip, so the content area
    /// keeps every pixel. The cost is discoverability, which the chip in
    /// the command bar and the digit shortcuts are there to offset.
    /// Confirmation before erasing privacy traces.
    ///
    /// Always shown, with no "do not ask again": the cleanup path has a
    /// manifest in front of it, this one does not, and the thing being
    /// erased is a record of what someone did rather than space they can
    /// get back by rebuilding something.
    pub(crate) fn show_privacy_confirm(&mut self, ctx: &egui::Context) {
        let s = self.s();
        let selected: Vec<&chystik_core::privacy::PrivacyItem> = self
            .traces_selected
            .iter()
            .filter_map(|i| self.traces.get(*i))
            .collect();
        let total: u64 = selected.iter().map(|t| t.size_bytes).sum();
        let irreversible = selected
            .iter()
            .filter(|t| t.severity == Severity::Risky)
            .count();

        let mut confirmed = false;
        let mut cancelled = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        dim_backdrop(ctx, "privacy_confirm_backdrop");
        egui::Window::new("privacy_confirm")
            .title_bar(false)
            .id(egui::Id::new("privacy_confirm_window"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Tooltip)
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(COL_RAISED)
                    .inner_margin(egui::Margin::same(space(6.0))),
            )
            .show(ctx, |ui| {
                ui.set_width(540.0);
                ui.label(txt(s.privacy_confirm_title.as_str(), "title", COL_TEXT));
                ui.add_space(space(1.5));
                ui.label(txt(s.privacy_confirm_lead.as_str(), "caption", COL_TEXT2));

                if irreversible > 0 {
                    ui.add_space(space(3.0));
                    egui::Frame::default()
                        .fill(COL_SURFACE)
                        .stroke(egui::Stroke::new(1.0_f32, severity_color(Severity::Risky)))
                        .rounding(egui::Rounding::same(R_MD))
                        .inner_margin(egui::Margin::same(space(3.0)))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                paint_severity_glyph(
                                    ui,
                                    Severity::Risky,
                                    10.0,
                                    severity_color(Severity::Risky),
                                );
                                ui.add_space(space(1.5));
                                ui.label(txt(
                                    i18n::fill(
                                        s.privacy_confirm_risky.as_str(),
                                        &[("n", &irreversible.to_string())],
                                    ),
                                    "strong",
                                    severity_color(Severity::Risky),
                                ));
                            });
                        });
                }

                ui.add_space(space(4.0));
                egui::ScrollArea::vertical()
                    .id_salt("privacy_confirm_list")
                    .max_height(240.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for trace in &selected {
                            ui.horizontal_top(|ui| {
                                let (rule, _) = ui.allocate_exact_size(
                                    egui::vec2(3.0, space(8.0)),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rule,
                                    egui::Rounding::same(1.5),
                                    severity_color(trace.severity),
                                );
                                ui.add_space(space(2.0));
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(txt(trace.kind.label(), "strong", COL_TEXT));
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(txt(
                                                    format_size(trace.size_bytes),
                                                    "mono_sm",
                                                    COL_TEXT2,
                                                ));
                                            },
                                        );
                                    });
                                    // The cost, not the path: at the moment
                                    // of confirming, what matters is what
                                    // stops working.
                                    ui.label(txt(trace.cost, "micro", COL_TEXT3));
                                });
                            });
                            ui.add_space(space(2.0));
                        }
                    });

                ui.add_space(space(3.0));
                ui.label(txt(s.privacy_confirm_trash.as_str(), "micro", COL_TEXT3));
                ui.add_space(space(4.0));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if primary_button(
                        ui,
                        &i18n::fill(
                            s.privacy_confirm_erase.as_str(),
                            &[
                                ("n", &selected.len().to_string()),
                                ("size", &format_size(total)),
                            ],
                        ),
                        severity_color(Severity::Risky),
                        !selected.is_empty(),
                    )
                    .clicked()
                    {
                        confirmed = true;
                    }
                    if ghost_button(ui, s.cancel.as_str(), true).clicked() {
                        cancelled = true;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(txt(s.esc_to_cancel.as_str(), "micro", COL_TEXT3));
                    });
                });
            });

        if cancelled {
            self.privacy_confirm_open = false;
        } else if confirmed {
            self.privacy_confirm_open = false;
            self.clear_selected_traces();
        }
    }

    pub(crate) fn show_palette(&mut self, ctx: &egui::Context) {
        let s = self.s();
        let current = self.section;
        let mut chosen: Option<Section> = None;

        dim_backdrop(ctx, "palette_backdrop");
        egui::Window::new("palette")
            .title_bar(false)
            .id(egui::Id::new("palette_window"))
            .anchor(egui::Align2::CENTER_TOP, [0.0, 120.0])
            .order(egui::Order::Tooltip)
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(COL_RAISED)
                    .inner_margin(egui::Margin::same(space(1.5))),
            )
            .show(ctx, |ui| {
                ui.set_width(380.0);
                egui::Frame::none()
                    .inner_margin(egui::Margin::symmetric(space(3.0), space(2.5)))
                    .show(ui, |ui| {
                        ui.label(txt(s.palette_title.as_str(), "caption", COL_TEXT3));
                    });
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.cursor().top(),
                    egui::Stroke::new(1.0_f32, COL_LINE),
                );
                ui.add_space(space(1.5));

                for section in Section::ALL {
                    let active = section == current;
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), space(9.5)),
                        egui::Sense::click(),
                    );
                    let inner = rect.shrink2(egui::vec2(space(1.5), 1.0));
                    // The window itself is COL_RAISED, so hovering used to
                    // paint the row in exactly the background colour and
                    // nothing happened on screen.
                    if active || response.hovered() {
                        ui.painter().rect_filled(
                            inner,
                            egui::Rounding::same(R_MD),
                            if active { COL_ACCENT_SOFT } else { COL_LINE },
                        );
                    }
                    if response.hovered() && !active {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let style = |name: &str| {
                        ui.style()
                            .text_styles
                            .get(&ts(name))
                            .cloned()
                            .unwrap_or_default()
                    };
                    ui.painter().text(
                        egui::pos2(inner.left() + space(3.0), inner.center().y),
                        egui::Align2::LEFT_CENTER,
                        section.label(s),
                        style(if active { "strong" } else { "body" }),
                        if active { COL_TEXT } else { COL_TEXT2 },
                    );
                    ui.painter().text(
                        egui::pos2(inner.right() - space(3.0), inner.center().y),
                        egui::Align2::RIGHT_CENTER,
                        (section.index() + 1).to_string(),
                        style("mono_sm"),
                        COL_TEXT3,
                    );
                    if response.clicked() {
                        chosen = Some(section);
                    }
                }
                ui.add_space(space(1.5));
            });

        if let Some(section) = chosen {
            self.go_to(section);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.palette_open = false;
        }
    }

    pub(crate) fn show_consent_modal(&mut self, ctx: &egui::Context) {
        let s = self.s();
        let mut accepted = false;
        let mut quit = false;
        let mut checked = self.consent_checked;

        dim_backdrop(ctx, "consent_backdrop");
        egui::Window::new("consent")
            .title_bar(false)
            .id(egui::Id::new("consent_window"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Tooltip)
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(COL_RAISED)
                    .inner_margin(egui::Margin::same(space(7.0))),
            )
            .show(ctx, |ui| {
                ui.set_width(520.0);
                ui.label(txt(s.consent_title.as_str(), "title", COL_TEXT));
                ui.add_space(space(1.5));
                ui.label(txt(s.consent_lead.as_str(), "caption", COL_TEXT2));
                ui.add_space(space(5.0));

                let points: [(&str, &str, egui::Color32); 5] = [
                    (
                        &s.consent_p1_title,
                        &s.consent_p1_body,
                        severity_color(Severity::Safe),
                    ),
                    (&s.consent_p2_title, &s.consent_p2_body, COL_ACCENT),
                    (
                        &s.consent_p3_title,
                        &s.consent_p3_body,
                        severity_color(Severity::Risky),
                    ),
                    (
                        &s.consent_p4_title,
                        &s.consent_p4_body,
                        severity_color(Severity::Safe),
                    ),
                    (
                        &s.consent_p5_title,
                        &s.consent_p5_body,
                        severity_color(Severity::Moderate),
                    ),
                ];
                for (title, body, accent) in points {
                    ui.horizontal_top(|ui| {
                        // A 3px rule instead of a bullet glyph: IBM Plex has
                        // no dependable bullet, and this scales with the text.
                        let (rect, _) = ui
                            .allocate_exact_size(egui::vec2(3.0, space(9.0)), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(rect, egui::Rounding::same(1.5), accent);
                        ui.add_space(space(2.5));
                        ui.vertical(|ui| {
                            ui.label(txt(title, "strong", COL_TEXT));
                            ui.add_space(space(0.5));
                            ui.label(txt(body, "caption", COL_TEXT2));
                        });
                    });
                    ui.add_space(space(3.0));
                }

                ui.add_space(space(2.0));
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.cursor().top(),
                    egui::Stroke::new(1.0_f32, COL_LINE),
                );
                ui.add_space(space(4.0));

                ui.checkbox(
                    &mut checked,
                    txt(s.consent_checkbox.as_str(), "body", COL_TEXT),
                );
                ui.add_space(space(4.0));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if primary_button(ui, s.consent_continue.as_str(), COL_ACCENT, checked)
                        .clicked()
                    {
                        accepted = true;
                    }
                    if ghost_button(ui, s.consent_quit.as_str(), true).clicked() {
                        quit = true;
                    }
                });
            });

        self.consent_checked = checked;
        if accepted {
            crate::consent::acknowledge();
            self.consent_pending = false;
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub(crate) fn show_confirm_modal(&mut self, ctx: &egui::Context) {
        let (lang, s) = (self.lang, self.s());
        // Own the data so the dialog never holds borrows across `self`
        // mutation below. Guard verdicts are precomputed so the manifest
        // ticks stay stable while the dialog is open.
        let items: Vec<(usize, PathBuf, u64, Severity, bool)> = self
            .selected_visible_rows()
            .into_iter()
            .map(|(i, f)| {
                let passed = match self.owning_root(&f.path) {
                    Some(root) => chystik_core::guard::check(&f.path, &root).is_ok(),
                    None => false,
                };
                (i, f.path.clone(), f.size_bytes, f.severity, passed)
            })
            .collect();
        let total_bytes: u64 = items.iter().map(|item| item.2).sum();
        let refused = items.iter().filter(|(.., passed)| !passed).count();

        let mut confirmed = false;
        let mut cancelled = false;
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                cancelled = true;
            }
            if i.key_pressed(egui::Key::Enter) {
                confirmed = true;
            }
        });

        dim_backdrop(ctx, "confirm_delete_backdrop");
        egui::Window::new("confirm_delete")
            .title_bar(false)
            .id(egui::Id::new("confirm_delete_window"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Tooltip)
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(COL_RAISED)
                    .inner_margin(egui::Margin::same(space(6.0))),
            )
            .show(ctx, |ui| {
                ui.set_width(560.0);
                ui.label(txt(s.confirm_title.as_str(), "title", COL_TEXT));
                ui.add_space(space(1.0));
                ui.label(txt(
                    i18n::fill(
                        s.confirm_sub.as_str(),
                        &[
                            ("n", &items.len().to_string()),
                            (
                                "items",
                                if items.len() == 1 {
                                    s.item_word.as_str()
                                } else {
                                    s.items_word.as_str()
                                },
                            ),
                            ("size", &format_size(total_bytes)),
                        ],
                    ),
                    "caption",
                    COL_TEXT2,
                ));
                ui.add_space(space(3.0));

                // A grid, not a row of horizontals: four ragged columns are
                // unreadable once the list passes a screenful.
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Grid::new("manifest_grid")
                            .num_columns(4)
                            .spacing(egui::vec2(space(3.0), space(1.5)))
                            .striped(false)
                            .show(ui, |ui| {
                                for (_, path, size, sev, passed) in &items {
                                    if *passed {
                                        ui.label(txt(
                                            "\u{2713}",
                                            "strong",
                                            severity_color(Severity::Safe),
                                        ));
                                    } else {
                                        paint_cross(ui, 12.0, severity_color(Severity::Risky));
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(txt(format_size(*size), "mono_sm", COL_TEXT2));
                                        },
                                    );
                                    ui.label(txt(
                                        path_tail(path, 3),
                                        "mono_sm",
                                        if *passed { COL_TEXT } else { COL_TEXT3 },
                                    ))
                                    .on_hover_text(display_path(path));
                                    severity_pill(ui, *sev, lang);
                                    ui.end_row();
                                }
                            });
                    });

                if refused > 0 {
                    ui.add_space(space(3.0));
                    egui::Frame::default()
                        .fill(COL_SURFACE)
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            severity_color(Severity::Moderate),
                        ))
                        .rounding(egui::Rounding::same(R_MD))
                        .inner_margin(egui::Margin::same(space(3.0)))
                        .show(ui, |ui| {
                            ui.label(txt(
                                i18n::fill(
                                    s.guard_will_skip.as_str(),
                                    &[(
                                        "n",
                                        &format!(
                                            "{refused} {}",
                                            if refused == 1 {
                                                s.item_word.as_str()
                                            } else {
                                                s.items_word.as_str()
                                            }
                                        ),
                                    )],
                                ),
                                "caption",
                                severity_color(Severity::Moderate),
                            ));
                        });
                }

                ui.add_space(space(4.0));
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if primary_button(
                            ui,
                            &i18n::fill(
                                s.move_to_trash.as_str(),
                                &[("size", &format_size(total_bytes))],
                            ),
                            severity_color(Severity::Risky),
                            true,
                        )
                        .clicked()
                        {
                            confirmed = true;
                        }
                        if ghost_button(ui, s.cancel.as_str(), true).clicked() {
                            cancelled = true;
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.label(txt(s.esc_to_cancel.as_str(), "micro", COL_TEXT3));
                        });
                    });
                });
            });

        if cancelled {
            self.confirm_delete_open = false;
        } else if confirmed {
            self.confirm_delete_open = false;
            let indices: Vec<usize> = items.into_iter().map(|(i, ..)| i).collect();
            self.execute_trash(indices);
        }
    }

    /// Guard-check then trash-delete every selected item; collects a summary.
    pub(crate) fn show_settings_modal(&mut self, ctx: &egui::Context) {
        let (lang, s) = (self.lang, self.s());
        let mut close = false;
        let mut open_repo = false;
        let mut next_lang: Option<Lang> = None;
        let mut add_exclusion = false;
        let mut export = false;
        if self.app_mark.is_none() {
            self.app_mark = app_mark(ctx);
        }
        let mark = self.app_mark.clone();
        let mut unexclude: Option<std::path::PathBuf> = None;

        dim_backdrop(ctx, "settings_backdrop");
        egui::Window::new("settings")
            .title_bar(false)
            .id(egui::Id::new("settings_window"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(settings_modal_order())
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(COL_RAISED)
                    .inner_margin(egui::Margin::same(space(6.0))),
            )
            .show(ctx, |ui| {
                ui.set_width(400.0);
                ui.horizontal(|ui| {
                    if let Some(mark) = &mark {
                        ui.add(
                            egui::Image::new(mark)
                                .fit_to_exact_size(egui::vec2(40.0, 40.0))
                                .rounding(egui::Rounding::same(R_LG)),
                        );
                        ui.add_space(space(2.5));
                    }
                    ui.vertical(|ui| {
                        ui.label(txt(s.settings.as_str(), "title", COL_TEXT));
                        ui.label(txt(s.app_name.as_str(), "caption", COL_TEXT2));
                    });
                });
                ui.add_space(space(4.0));

                ui.label(txt(s.settings_language.as_str(), "micro", COL_TEXT3));
                ui.add_space(space(1.5));
                egui::ComboBox::from_id_salt("settings_language")
                    .selected_text(txt(lang.name(), "strong", COL_TEXT))
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        let mut selected = lang;
                        for option in Lang::ALL {
                            let label = format!("{}  ·  {}", option.name(), option.code());
                            ui.selectable_value(
                                &mut selected,
                                option,
                                txt(label, "strong", COL_TEXT),
                            );
                        }
                        if selected != lang {
                            next_lang = Some(selected);
                        }
                    });

                ui.add_space(space(5.0));
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.cursor().top(),
                    egui::Stroke::new(1.0_f32, COL_LINE),
                );
                ui.add_space(space(4.0));

                ui.label(txt(s.exclusions_title.as_str(), "micro", COL_TEXT3));
                ui.add_space(space(1.0));
                ui.label(txt(s.exclusions_hint.as_str(), "caption", COL_TEXT2));
                ui.add_space(space(2.0));

                if !self.exclusions_readable {
                    ui.label(txt(
                        s.exclusions_unreadable.as_str(),
                        "caption",
                        severity_color(Severity::Moderate),
                    ));
                    ui.add_space(space(1.5));
                }

                if self.exclusions.is_empty() {
                    ui.label(txt(s.exclusions_empty.as_str(), "caption", COL_TEXT3));
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("exclusions_list")
                        .max_height(120.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for path in self.exclusions.clone() {
                                ui.horizontal(|ui| {
                                    ui.label(txt(
                                        truncate_middle(&display_path(&path), 52),
                                        "mono_sm",
                                        COL_TEXT,
                                    ))
                                    .on_hover_text(display_path(&path));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(txt(
                                                        s.exclusions_remove.as_str(),
                                                        "micro",
                                                        COL_TEXT2,
                                                    ))
                                                    .fill(egui::Color32::TRANSPARENT)
                                                    .stroke(egui::Stroke::new(1.0_f32, COL_LINE))
                                                    .rounding(egui::Rounding::same(R_SM)),
                                                )
                                                .clicked()
                                            {
                                                unexclude = Some(path.clone());
                                            }
                                        },
                                    );
                                });
                            }
                        });
                }
                ui.add_space(space(2.0));
                ui.horizontal(|ui| {
                    if ghost_button(ui, s.exclusions_add.as_str(), true).clicked() {
                        add_exclusion = true;
                    }
                    // Moved off the command bar: exporting is occasional,
                    // and the bar should carry the one action that matters.
                    if ghost_button(ui, s.export.as_str(), !self.findings.is_empty())
                        .on_hover_text(s.export_hint.as_str())
                        .clicked()
                    {
                        export = true;
                    }
                });

                ui.add_space(space(5.0));
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.cursor().top(),
                    egui::Stroke::new(1.0_f32, COL_LINE),
                );
                ui.add_space(space(4.0));

                egui::Grid::new("about_grid")
                    .num_columns(2)
                    .spacing(egui::vec2(space(4.0), space(2.0)))
                    .show(ui, |ui| {
                        ui.label(txt(s.settings_version.as_str(), "micro", COL_TEXT3));
                        ui.label(txt(env!("CARGO_PKG_VERSION"), "mono_sm", COL_TEXT));
                        ui.end_row();

                        ui.label(txt(s.settings_developer.as_str(), "micro", COL_TEXT3));
                        ui.label(txt(env!("CARGO_PKG_AUTHORS"), "strong", COL_TEXT));
                        ui.end_row();

                        ui.label(txt(s.settings_source.as_str(), "micro", COL_TEXT3));
                        if ui
                            .add(
                                egui::Label::new(txt(REPO_URL, "mono_sm", COL_ACCENT))
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(s.settings_open_link.as_str())
                            .clicked()
                        {
                            open_repo = true;
                        }
                        ui.end_row();
                    });

                ui.add_space(space(5.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if primary_button(ui, s.close.as_str(), COL_ACCENT, true).clicked() {
                        close = true;
                    }
                });
            });

        if let Some(next) = next_lang {
            self.lang = next;
        }
        if let Some(path) = unexclude {
            self.unexclude(&path);
        }
        if add_exclusion {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                self.exclude_path(dir);
            }
        }
        if export {
            self.settings_open = false;
            self.export_json();
        }
        if open_repo {
            // eframe only forwards `open_url` when built with a browser
            // backend; going through the desktop handler always works.
            if let Err(e) = std::process::Command::new("xdg-open").arg(REPO_URL).spawn() {
                eprintln!("[chystik] could not open {REPO_URL}: {e}");
            }
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.settings_open = false;
        }
    }

    pub(crate) fn show_notice_modal(&mut self, ctx: &egui::Context, notice: Notice) {
        let mut close = false;
        // 300 ms, once: the clock lives on the notice, not on a frame
        // counter, so it cannot restart just because something else in the
        // window repainted.
        let anim_t = (notice.shown_at.elapsed().as_secs_f32() / 0.3).clamp(0.0, 1.0);
        if anim_t < 1.0 {
            ctx.request_repaint();
        }

        dim_backdrop(ctx, "notice_backdrop");
        egui::Window::new("notice")
            .title_bar(false)
            .id(egui::Id::new("notice_window"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Tooltip)
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(COL_RAISED)
                    .inner_margin(egui::Margin::same(space(6.0))),
            )
            .show(ctx, |ui| {
                ui.set_min_width(440.0);
                if notice.success {
                    ui.horizontal(|ui| {
                        paint_success_check(ui, 28.0, severity_color(Severity::Safe), anim_t);
                        ui.add_space(space(2.0));
                        ui.label(txt(&notice.title, "title", COL_TEXT));
                    });
                } else {
                    ui.label(txt(&notice.title, "title", COL_TEXT));
                }
                ui.add_space(space(2.0));
                for line in &notice.lines {
                    ui.label(txt(line, "caption", COL_TEXT2));
                }
                ui.add_space(space(4.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if primary_button(ui, self.s().ok.as_str(), COL_ACCENT, true).clicked() {
                        close = true;
                    }
                });
            });
        if close
            || ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter))
        {
            self.notice = None;
        } else {
            // Not dismissed: this frame's `take()` must not lose it.
            self.notice = Some(notice);
        }
    }

    /// Cleanup in progress: current item, a bar, and a running count.
    /// No cancel — the same manifest already confirmed everything in the
    /// batch, and the safety guard still runs per item underneath this.
    pub(crate) fn show_clean_progress(&mut self, ctx: &egui::Context) {
        let s = self.s();
        let crate::state::CleanState::Running { progress, .. } = &self.clean else {
            return;
        };
        let done = progress.done;
        let total = progress.total;
        let freed = progress.freed_bytes;
        let total_bytes = progress.total_bytes;
        let fraction = progress.fraction();
        let current = progress
            .current
            .as_ref()
            .map(|p| truncate_middle(&display_path(p), 56));

        ctx.request_repaint(); // an active worker always has more to show

        dim_backdrop(ctx, "clean_progress_backdrop");
        egui::Window::new("clean_progress")
            .title_bar(false)
            .id(egui::Id::new("clean_progress_window"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Tooltip)
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(COL_RAISED)
                    .inner_margin(egui::Margin::same(space(6.0))),
            )
            .show(ctx, |ui| {
                ui.set_width(440.0);
                ui.label(txt(s.trash_progress_title.as_str(), "title", COL_TEXT));
                ui.add_space(space(2.0));
                ui.label(txt(
                    i18n::fill(
                        s.trash_progress_count.as_str(),
                        &[
                            ("done", &done.to_string()),
                            ("n", &total.to_string()),
                            (
                                "size",
                                &format!("{} / {}", format_size(freed), format_size(total_bytes)),
                            ),
                        ],
                    ),
                    "caption",
                    COL_TEXT2,
                ));
                ui.add_space(space(3.0));
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .fill(COL_ACCENT)
                        .rounding(egui::Rounding::same(R_SM)),
                );
                ui.add_space(space(2.0));
                if let Some(current) = current {
                    ui.label(txt(current, "mono_sm", COL_TEXT3));
                } else {
                    // Reserve the line's height so the window doesn't jump
                    // between "no current item yet" and the first one.
                    ui.label(txt(" ", "mono_sm", COL_TEXT3));
                }
            });
    }
}

/// The settings window must not cover a ComboBox popup. egui renders those
/// menus on `Foreground`, while the prior `Tooltip` window layer covered the
/// opened language menu completely.
fn settings_modal_order() -> egui::Order {
    egui::Order::Foreground
}

/// Full-screen dimmed layer that swallows clicks behind a modal window.
pub(crate) fn dim_backdrop(ctx: &egui::Context, id: &str) {
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .interactable(true)
        .anchor(egui::Align2::LEFT_TOP, [0.0, 0.0])
        .show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let response = ui.allocate_response(screen.size(), egui::Sense::click());
            // Fades in rather than snapping.
            let t = ctx.animate_bool_with_time(egui::Id::new("modal_dim"), true, 0.12);
            ui.painter().rect_filled(
                response.rect,
                0.0,
                egui::Color32::from_black_alpha((165.0 * t) as u8),
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_modal_stays_below_its_language_picker_popup() {
        // `ComboBox` uses egui's Foreground popup layer. A settings window
        // above it makes the selector look inert even though it receives the
        // click and opens its menu.
        assert_eq!(settings_modal_order(), egui::Order::Foreground);
    }
}

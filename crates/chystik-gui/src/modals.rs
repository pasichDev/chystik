//! Modal dialogs: the deletion manifest, settings/about, result notices,
//! and the first-run risk acknowledgement.

use eframe::egui;

use std::path::PathBuf;

use chystik_core::model::Severity;

use crate::app::{ChystikApp, Notice};
use crate::format::*;
use crate::i18n::{self, Lang};
use crate::theme::*;
use crate::widgets::*;

impl ChystikApp {
    /// First-run risk acknowledgement.
    ///
    /// Shown before anything else and dismissible only by an explicit tick
    /// plus Continue — no Escape, no click-away. A tool that deletes should
    /// make the user say once, in their own click, that they understand what
    /// that means.
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
                    Some(root) => chystik_core::guard::check(&f.path, root).is_ok(),
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
                                    let (glyph, color) = if *passed {
                                        ("\u{2713}", severity_color(Severity::Safe))
                                    } else {
                                        ("\u{2717}", severity_color(Severity::Risky))
                                    };
                                    ui.label(txt(glyph, "strong", color));
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
                                    .on_hover_text(path.display().to_string());
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

        dim_backdrop(ctx, "settings_backdrop");
        egui::Window::new("settings")
            .title_bar(false)
            .id(egui::Id::new("settings_window"))
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
                ui.set_width(400.0);
                ui.label(txt(s.settings.as_str(), "title", COL_TEXT));
                ui.add_space(space(4.0));

                ui.label(txt(s.settings_language.as_str(), "micro", COL_TEXT3));
                ui.add_space(space(1.5));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space(1.5);
                    for option in Lang::ALL {
                        let active = option == lang;
                        let (fill, stroke, fg) = if active {
                            (COL_ACCENT_SOFT, COL_ACCENT, COL_TEXT)
                        } else {
                            (egui::Color32::TRANSPARENT, COL_LINE_HI, COL_TEXT2)
                        };
                        if ui
                            .add(
                                egui::Button::new(txt(option.name(), "strong", fg))
                                    .fill(fill)
                                    .stroke(egui::Stroke::new(1.0_f32, stroke))
                                    .rounding(egui::Rounding::same(R_MD))
                                    .min_size(egui::vec2(150.0, space(8.5))),
                            )
                            .clicked()
                        {
                            next_lang = Some(option);
                        }
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

    pub(crate) fn show_notice_modal(&mut self, ctx: &egui::Context, notice: &Notice) {
        let mut close = false;
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
                ui.label(txt(&notice.title, "title", COL_TEXT));
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
        }
    }
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

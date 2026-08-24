//! Small painted widgets shared across the panels.
//!
//! The severity marks are PAINTED rather than typed: IBM Plex has no
//! U+25CF / U+25D0 / U+25B2, so drawing them as text rendered a column of
//! missing-glyph boxes.

use eframe::egui;

use chystik_core::model::Severity;

use crate::format::format_size;
use crate::i18n::{self, Lang, Strings};
use crate::state::{CatStat, CleanBuckets};
use crate::theme::*;

pub(crate) fn severity_color(sev: Severity) -> egui::Color32 {
    match sev {
        Severity::Safe => egui::Color32::from_rgb(0x2D, 0xD4, 0xBF),
        Severity::Moderate => egui::Color32::from_rgb(0xF5, 0xA5, 0x24),
        Severity::Risky => egui::Color32::from_rgb(0xF4, 0x59, 0x5B),
    }
}

/// Shape cue paired with every severity colour. Survives any colour vision
/// deficiency and any washed-out display.
/// Shape cue paired with every severity colour: filled disc, half disc,
/// triangle. Survives colour-blindness and any washed-out display.
///
/// PAINTED, not typed. IBM Plex has no U+25CF/U+25D0/U+25B2, so drawing
/// these as text rendered a row of missing-glyph boxes.
pub(crate) fn paint_severity_glyph(
    ui: &mut egui::Ui,
    sev: Severity,
    diameter: f32,
    color: egui::Color32,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::hover());
    let c = rect.center();
    let r = diameter / 2.0;
    let painter = ui.painter();
    match sev {
        Severity::Safe => {
            painter.circle_filled(c, r, color);
        }
        Severity::Moderate => {
            painter.circle_stroke(c, r - 0.7, egui::Stroke::new(1.4_f32, color));
            let _ = painter.add(egui::Shape::convex_polygon(
                (0..=12)
                    .map(|i| {
                        let a = std::f32::consts::PI * (i as f32 / 12.0 - 0.5);
                        egui::pos2(c.x + a.cos() * (r - 0.7), c.y + a.sin() * (r - 0.7))
                    })
                    .collect(),
                color,
                egui::Stroke::NONE,
            ));
        }
        Severity::Risky => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x, c.y - r),
                    egui::pos2(c.x + r, c.y + r * 0.8),
                    egui::pos2(c.x - r, c.y + r * 0.8),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
    }
}

/// Small painted triangle marking the active sort column and direction.
/// `\u{25B2}`/`\u{25BC}` are absent from IBM Plex too.
pub(crate) fn paint_sort_arrow(ui: &mut egui::Ui, ascending: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
    let c = rect.center();
    let (tip, base) = if ascending { (-3.0, 2.0) } else { (2.5, -2.5) };
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x, c.y + tip),
            egui::pos2(c.x + 3.6, c.y + base),
            egui::pos2(c.x - 3.6, c.y + base),
        ],
        COL_TEXT2,
        egui::Stroke::NONE,
    ));
}

/// What deleting this costs, in words. Surfaces `Severity::regeneration_cost`,
/// which the UI never called.
/// Severity as a pill. Safe and Moderate are outlined; Risky is filled, so
/// danger reads as weight even in greyscale.
pub(crate) fn severity_pill(ui: &mut egui::Ui, sev: Severity, lang: Lang) {
    let color = severity_color(sev);
    let filled = sev == Severity::Risky;
    let (fill, fg) = if filled {
        (color, COL_ACCENT_FG)
    } else {
        (egui::Color32::TRANSPARENT, color)
    };
    egui::Frame::default()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0_f32, color))
        .rounding(egui::Rounding::same(999.0))
        .inner_margin(egui::Margin::symmetric(space(2.0), space(0.5)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = space(1.25);
            paint_severity_glyph(ui, sev, 8.0, fg);
            ui.label(txt(i18n::severity_label(lang, sev), "micro", fg));
        });
}

/// Display order for severities in the Severity column sort.
pub(crate) const fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Safe => 0,
        Severity::Moderate => 1,
        Severity::Risky => 2,
    }
}

pub(crate) fn hairline_bottom(ui: &mut egui::Ui) {
    let r = ui.max_rect();
    ui.painter().hline(
        r.x_range(),
        r.bottom() - 0.5,
        egui::Stroke::new(1.0_f32, COL_LINE),
    );
}

pub(crate) fn hairline_top(ui: &mut egui::Ui) {
    let r = ui.max_rect();
    ui.painter().hline(
        r.x_range(),
        r.top() + 0.5,
        egui::Stroke::new(1.0_f32, COL_LINE),
    );
}

/// Proportional teal/amber/red bar. The only place three severities are
/// shown as one shape; always paired with the labelled numbers beside it.
pub(crate) fn severity_bar(ui: &mut egui::Ui, b: CleanBuckets, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let total = b.safe_bytes + b.moderate_bytes + b.risky_bytes;
    let rounding = egui::Rounding::same(height / 2.0);
    if total == 0 {
        ui.painter().rect_filled(rect, rounding, COL_LINE);
        return;
    }
    let mut x = rect.left();
    for (bytes, color) in [
        (b.safe_bytes, severity_color(Severity::Safe)),
        (b.moderate_bytes, severity_color(Severity::Moderate)),
        (b.risky_bytes, severity_color(Severity::Risky)),
    ] {
        if bytes == 0 {
            continue;
        }
        let w = rect.width() * (bytes as f32 / total as f32);
        let seg = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, height));
        ui.painter().rect_filled(seg, rounding, color);
        x += w;
    }
}

/// A filled pill button that actually reads as the primary action.
pub(crate) fn primary_button(
    ui: &mut egui::Ui,
    label: &str,
    fill: egui::Color32,
    enabled: bool,
) -> egui::Response {
    let text = txt(
        label,
        "strong",
        if enabled { COL_ACCENT_FG } else { COL_TEXT3 },
    );
    ui.add_enabled(
        enabled,
        egui::Button::new(text)
            .fill(if enabled { fill } else { COL_RAISED })
            .rounding(egui::Rounding::same(R_MD))
            .min_size(egui::vec2(0.0, space(8.0))),
    )
}

pub(crate) fn ghost_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(txt(label, "strong", COL_TEXT2))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0_f32, COL_LINE_HI))
            .rounding(egui::Rounding::same(R_MD))
            .min_size(egui::vec2(0.0, space(8.0))),
    )
}

pub(crate) fn category_row(
    ui: &mut egui::Ui,
    stat: Option<CatStat>,
    bytes: u64,
    count: usize,
    selected: bool,
    lang: Lang,
    s: &'static Strings,
) -> bool {
    let height = space(13.0);
    // Inside a non-shrinking ScrollArea `available_width` can exceed the
    // panel, which drew the byte column under the detail pane.
    let width = ui.available_width().min(SIDEBAR_W - space(2.0));
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let inner = rect.shrink2(egui::vec2(space(2.5), space(0.25)));
    let painter = ui.painter();

    if selected {
        painter.rect_filled(inner, egui::Rounding::same(R_LG), COL_ACCENT_SOFT);
        painter.rect_filled(
            egui::Rect::from_min_size(inner.left_top(), egui::vec2(3.0, inner.height())),
            egui::Rounding::same(1.5),
            COL_ACCENT,
        );
    } else if resp.hovered() {
        painter.rect_filled(inner, egui::Rounding::same(R_LG), COL_RAISED);
    }

    let text_color = if selected { COL_TEXT } else { COL_TEXT2 };
    let left = inner.left() + space(3.0);
    let dot_r = 5.0;
    let (label, dot, hint) = match stat {
        Some(x) => (
            i18n::category_label(lang, x.category).to_string(),
            Some(severity_color(x.severity())),
            i18n::category_description(lang, x.category).to_string(),
        ),
        None => (
            s.all_categories.to_string(),
            None,
            s.all_categories_hint.to_string(),
        ),
    };
    if let Some(color) = dot {
        painter.rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(left + dot_r, inner.center().y - space(1.5)),
                egui::vec2(dot_r * 2.0, dot_r * 2.0),
            ),
            egui::Rounding::same(2.5),
            color,
        );
    }

    let name_x = left + if dot.is_some() { space(5.0) } else { 0.0 };
    let size_text = format_size(bytes);
    let size_style = ui
        .style()
        .text_styles
        .get(&ts("mono_lg"))
        .cloned()
        .unwrap_or_default();
    let size_w = ui
        .painter()
        .layout_no_wrap(size_text.clone(), size_style.clone(), COL_TEXT)
        .size()
        .x;
    // Clip the name so a long category never runs under the byte count.
    let name_clip = egui::Rect::from_min_max(
        egui::pos2(name_x, inner.top()),
        egui::pos2(
            inner.right() - space(3.0) - size_w - space(2.0),
            inner.bottom(),
        ),
    );
    let name_painter = ui.painter().with_clip_rect(name_clip);
    name_painter.text(
        egui::pos2(name_x, inner.center().y - space(1.5)),
        egui::Align2::LEFT_CENTER,
        label,
        ui.style()
            .text_styles
            .get(&ts(if selected { "strong" } else { "caption" }))
            .cloned()
            .unwrap_or_default(),
        text_color,
    );
    name_painter.text(
        egui::pos2(name_x, inner.center().y + space(2.5)),
        egui::Align2::LEFT_CENTER,
        format!(
            "{count} {}",
            if count == 1 {
                s.item_word.as_str()
            } else {
                s.items_word.as_str()
            }
        ),
        ui.style()
            .text_styles
            .get(&ts("micro"))
            .cloned()
            .unwrap_or_default(),
        COL_TEXT3,
    );
    ui.painter().text(
        egui::pos2(inner.right() - space(3.0), inner.center().y),
        egui::Align2::RIGHT_CENTER,
        size_text,
        size_style,
        if selected { COL_TEXT } else { COL_TEXT2 },
    );

    resp.on_hover_text(hint).clicked()
}

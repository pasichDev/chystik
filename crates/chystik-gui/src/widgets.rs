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
    paint_severity_mark(ui.painter(), sev, rect.center(), diameter / 2.0, color);
}

/// The mark itself: filled disc, half disc, triangle.
pub(crate) fn paint_severity_mark(
    painter: &egui::Painter,
    sev: Severity,
    c: egui::Pos2,
    r: f32,
    color: egui::Color32,
) {
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
    // Painted at an explicit size rather than laid out with `Frame`: inside
    // a table cell the frame took the cell's full height, so the badge grew
    // to 34px next to 13.5px body text and dominated the column.
    const HEIGHT: f32 = 19.0;
    const GLYPH: f32 = 7.0;
    const PAD_X: f32 = 7.0;
    const GAP: f32 = 5.0;

    let color = severity_color(sev);
    let filled = sev == Severity::Risky;
    let fg = if filled { COL_ACCENT_FG } else { color };

    let galley = ui.painter().layout_no_wrap(
        i18n::severity_label(lang, sev).to_owned(),
        ui.style()
            .text_styles
            .get(&ts("pill"))
            .cloned()
            .unwrap_or_default(),
        fg,
    );
    let width = PAD_X * 2.0 + GLYPH + GAP + galley.size().x;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), egui::Sense::hover());

    let painter = ui.painter();
    let rounding = egui::Rounding::same(R_SM);
    if filled {
        painter.rect_filled(rect, rounding, color);
    } else {
        painter.rect_stroke(rect, rounding, egui::Stroke::new(1.0_f32, color));
    }

    // Shape as well as colour: readable without colour vision.
    let glyph_centre = egui::pos2(rect.left() + PAD_X + GLYPH / 2.0, rect.center().y);
    paint_severity_mark(painter, sev, glyph_centre, GLYPH / 2.0, fg);
    painter.galley(
        egui::pos2(
            rect.left() + PAD_X + GLYPH + GAP,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        fg,
    );

    response.on_hover_text(format!(
        "{} \u{2014} {}",
        i18n::severity_label(lang, sev),
        i18n::severity_cost(lang, sev)
    ));
}

/// Stable, compact recovery marker for the findings table. Recovery labels
/// vary substantially by locale, so text pills made the column's geometry
/// depend on the active language. The column header and hover text preserve
/// the full recovery meaning without moving neighbouring columns.
pub(crate) const RECOVERY_DOT_SIZE: f32 = 10.0;

pub(crate) fn recovery_dot(ui: &mut egui::Ui, sev: Severity, lang: Lang) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(RECOVERY_DOT_SIZE, RECOVERY_DOT_SIZE),
        egui::Sense::hover(),
    );
    ui.painter()
        .circle_filled(rect.center(), RECOVERY_DOT_SIZE / 2.0, severity_color(sev));

    response.on_hover_text(format!(
        "{} — {}",
        i18n::severity_label(lang, sev),
        i18n::severity_cost(lang, sev)
    ))
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
    // Geometry is derived from the PANEL, not from whatever width the
    // scroll area happens to offer, so every row lines up with the header
    // above it and with every other row.
    const HEIGHT: f32 = 52.0;
    const DOT: f32 = 10.0;
    /// Reserved for the colour dot on EVERY row, including the one without
    /// a dot. Making it conditional put "All categories" 20px left of every
    /// category under it.
    const DOT_COLUMN: f32 = 20.0;
    /// How far the selected/hovered card extends past the text on each side.
    const CARD_BLEED: f32 = SIDEBAR_PAD / 2.0;

    let width = ui.available_width().min(SIDEBAR_W);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), egui::Sense::click());

    let text_left = rect.left() + SIDEBAR_PAD;
    let text_right = rect.left() + SIDEBAR_W - SIDEBAR_PAD;
    let card = egui::Rect::from_min_max(
        egui::pos2(text_left - CARD_BLEED, rect.top() + 1.0),
        egui::pos2(text_right + CARD_BLEED, rect.bottom() - 1.0),
    );

    let painter = ui.painter();
    if selected {
        painter.rect_filled(card, egui::Rounding::same(R_LG), COL_ACCENT_SOFT);
        painter.rect_filled(
            egui::Rect::from_min_size(card.left_top(), egui::vec2(3.0, card.height())),
            egui::Rounding::same(1.5),
            COL_ACCENT,
        );
    } else if resp.hovered() {
        painter.rect_filled(card, egui::Rounding::same(R_LG), COL_RAISED);
    }

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

    let name_y = card.center().y - 7.0;
    let count_y = card.center().y + 9.0;
    // Every row gets a marker in the same place, including the one with no
    // category colour — without it "All categories" was the only label in
    // the list with nothing under its dot column, and read as misaligned.
    let marker = egui::Rect::from_center_size(
        egui::pos2(text_left + DOT / 2.0, name_y),
        egui::vec2(DOT, DOT),
    );
    match dot {
        Some(color) => {
            painter.rect_filled(marker, egui::Rounding::same(2.5), color);
        }
        None => {
            painter.rect_stroke(
                marker.shrink(0.5),
                egui::Rounding::same(2.5),
                egui::Stroke::new(1.2_f32, COL_TEXT3),
            );
        }
    }

    let style = |name: &str| {
        ui.style()
            .text_styles
            .get(&ts(name))
            .cloned()
            .unwrap_or_default()
    };
    let size_style = style("mono_lg");
    let size_text = format_size(bytes);
    let size_w = painter
        .layout_no_wrap(size_text.clone(), size_style.clone(), COL_TEXT)
        .size()
        .x;

    // Clip the name so a long category never runs under the byte count.
    let name_x = text_left + DOT_COLUMN;
    let names = painter.with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(name_x, card.top()),
        egui::pos2(text_right - size_w - space(2.0), card.bottom()),
    ));
    names.text(
        egui::pos2(name_x, name_y),
        egui::Align2::LEFT_CENTER,
        label,
        style(if selected { "strong" } else { "caption" }),
        if selected { COL_TEXT } else { COL_TEXT2 },
    );
    names.text(
        egui::pos2(name_x, count_y),
        egui::Align2::LEFT_CENTER,
        format!(
            "{count} {}",
            if count == 1 {
                s.item_word.as_str()
            } else {
                s.items_word.as_str()
            }
        ),
        style("micro"),
        COL_TEXT3,
    );
    painter.text(
        egui::pos2(text_right, card.center().y),
        egui::Align2::RIGHT_CENTER,
        size_text,
        size_style,
        if selected { COL_TEXT } else { COL_TEXT2 },
    );

    resp.on_hover_text(hint).clicked()
}

/// A circled "i", painted. `\u{2139}` is not in IBM Plex.
pub(crate) fn paint_info_mark(
    ui: &mut egui::Ui,
    size: f32,
    color: egui::Color32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let c = rect.center();
    let r = size / 2.0 - 0.8;
    let painter = ui.painter();
    painter.circle_stroke(c, r, egui::Stroke::new(1.3_f32, color));
    painter.circle_filled(egui::pos2(c.x, c.y - r * 0.45), 0.9, color);
    painter.line_segment(
        [
            egui::pos2(c.x, c.y - r * 0.05),
            egui::pos2(c.x, c.y + r * 0.5),
        ],
        egui::Stroke::new(1.3_f32, color),
    );
    response
}

/// A cross, painted. `\u{2717}` is not in IBM Plex.
pub(crate) fn paint_cross(ui: &mut egui::Ui, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let r = rect.shrink(size * 0.22);
    let stroke = egui::Stroke::new(1.6_f32, color);
    ui.painter()
        .line_segment([r.left_top(), r.right_bottom()], stroke);
    ui.painter()
        .line_segment([r.right_top(), r.left_bottom()], stroke);
}

/// A bordered button whose content is PAINTED, matching `ghost_button`'s
/// height so it sits on the same baseline as its neighbours.
///
/// `egui::Button` only takes text, and building this out of a `Frame` gave
/// it neither the right height nor any hover feedback.
pub(crate) fn icon_button(
    ui: &mut egui::Ui,
    label: &str,
    draw: impl FnOnce(&egui::Painter, egui::Pos2, egui::Color32),
) -> egui::Response {
    const ICON: f32 = 14.0;
    const PAD_X: f32 = 10.0;
    const GAP: f32 = 6.0;

    let height = space(8.0);
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        ui.style()
            .text_styles
            .get(&ts("micro"))
            .cloned()
            .unwrap_or_default(),
        COL_TEXT2,
    );
    let width = PAD_X * 2.0 + ICON + GAP + galley.size().x;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());

    let hovered = response.hovered();
    let stroke = if hovered { COL_ACCENT } else { COL_LINE };
    let fg = if hovered { COL_TEXT } else { COL_TEXT2 };
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, egui::Rounding::same(R_MD), COL_RAISED);
    }
    painter.rect_stroke(
        rect,
        egui::Rounding::same(R_MD),
        egui::Stroke::new(1.0_f32, stroke),
    );

    draw(
        painter,
        egui::pos2(rect.left() + PAD_X + ICON / 2.0, rect.center().y),
        fg,
    );
    painter.galley(
        egui::pos2(
            rect.left() + PAD_X + ICON + GAP,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        fg,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Sliders, drawn around `c` — the settings mark.
///
/// A gear at 14px degenerates into a sun: six radial spokes around a dot
/// read as rays, not teeth. Two tracks with knobs stay unmistakable at this
/// size, and `\u{2699}` is not in IBM Plex anyway.
pub(crate) fn draw_settings_mark(painter: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.3_f32, color);
    let half = 5.5_f32;
    for (dy, knob_x) in [(-2.6_f32, 1.6_f32), (2.6, -1.6)] {
        painter.line_segment(
            [
                egui::pos2(c.x - half, c.y + dy),
                egui::pos2(c.x + half, c.y + dy),
            ],
            stroke,
        );
        painter.circle(egui::pos2(c.x + knob_x, c.y + dy), 2.0, COL_SURFACE, stroke);
    }
}

/// A ring that draws itself in, then a check-mark that draws itself in,
/// as `t` runs 0.0 (nothing shown yet) to 1.0 (fully drawn). Caller owns
/// the clock — see `Notice::shown_at` — so the animation plays exactly
/// once no matter how many frames repaint while it does.
///
/// PAINTED for the same reason as the other marks: nothing in IBM Plex
/// reads as unambiguously "done" the way a drawn check does, and drawing
/// it also gets the partial-stroke animation for free.
pub(crate) fn paint_success_check(
    ui: &mut egui::Ui,
    size: f32,
    color: egui::Color32,
    t: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let c = rect.center();
    let r = size / 2.0 - 1.2;

    // Ring: eased scale-in over the first 45% of the animation.
    let ring_t = (t / 0.45).clamp(0.0, 1.0);
    let ring_r = r * ease_out_back(ring_t);
    if ring_r > 0.0 {
        ui.painter()
            .circle_stroke(c, ring_r, egui::Stroke::new(1.8_f32, color));
    }

    // Check: two segments, short arm then long arm, drawn as the trailing
    // 65% of the animation so the tick visibly follows the ring.
    let check_t = ((t - 0.35) / 0.65).clamp(0.0, 1.0);
    if check_t > 0.0 {
        let p0 = egui::pos2(c.x - r * 0.5, c.y + r * 0.05);
        let p1 = egui::pos2(c.x - r * 0.12, c.y + r * 0.42);
        let p2 = egui::pos2(c.x + r * 0.55, c.y - r * 0.35);
        let stroke = egui::Stroke::new(2.0_f32, color);
        let short_len = (p1 - p0).length();
        let long_len = (p2 - p1).length();
        let total = short_len + long_len;
        let drawn = total * check_t;
        if drawn <= short_len {
            let f = if short_len > 0.0 {
                drawn / short_len
            } else {
                1.0
            };
            ui.painter().line_segment([p0, p0 + (p1 - p0) * f], stroke);
        } else {
            ui.painter().line_segment([p0, p1], stroke);
            let f = if long_len > 0.0 {
                (drawn - short_len) / long_len
            } else {
                1.0
            };
            ui.painter().line_segment([p1, p1 + (p2 - p1) * f], stroke);
        }
    }
    response
}

/// A small overshoot-then-settle curve, so the ring pops in rather than
/// just growing linearly.
fn ease_out_back(t: f32) -> f32 {
    let c1 = 1.70158_f32;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

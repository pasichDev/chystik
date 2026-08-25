//! Design tokens, typography and the egui style the whole app draws with.
//!
//! Three things carry most of the visual weight and all three were absent
//! before: a real typeface (egui ships Ubuntu-Light 12.5px and nothing
//! else), a spacing rhythm (its default is 3px vertical) and a severity
//! palette that survives colour-blindness.

use eframe::egui;

const COL_BG: egui::Color32 = egui::Color32::from_rgb(0x0F, 0x11, 0x15);
pub(crate) const COL_SURFACE: egui::Color32 = egui::Color32::from_rgb(0x16, 0x1A, 0x20);
pub(crate) const COL_RAISED: egui::Color32 = egui::Color32::from_rgb(0x1D, 0x22, 0x2A);
pub(crate) const COL_LINE: egui::Color32 = egui::Color32::from_rgb(0x23, 0x2A, 0x34);
pub(crate) const COL_LINE_HI: egui::Color32 = egui::Color32::from_rgb(0x33, 0x3B, 0x47);
pub(crate) const COL_TEXT: egui::Color32 = egui::Color32::from_rgb(0xED, 0xEF, 0xF3);
pub(crate) const COL_TEXT2: egui::Color32 = egui::Color32::from_rgb(0xA3, 0xAD, 0xBA);
pub(crate) const COL_TEXT3: egui::Color32 = egui::Color32::from_rgb(0x6D, 0x78, 0x87);
pub(crate) const COL_ACCENT: egui::Color32 = egui::Color32::from_rgb(0x5B, 0x8C, 0xFF);
pub(crate) const COL_ACCENT_FG: egui::Color32 = egui::Color32::from_rgb(0x0B, 0x10, 0x20);
pub(crate) const COL_ACCENT_SOFT: egui::Color32 = egui::Color32::from_rgb(0x1B, 0x24, 0x40);

/// Project home, from the package manifest so the link lives in exactly
/// one place.
pub(crate) const REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");

/// 4px base grid.
pub(crate) const fn space(n: f32) -> f32 {
    4.0 * n
}

pub(crate) const R_SM: f32 = 4.0;
pub(crate) const R_MD: f32 = 6.0;
pub(crate) const R_LG: f32 = 10.0;
pub(crate) const R_XL: f32 = 14.0;

pub(crate) const ROW_H: f32 = 44.0;
pub(crate) const SIDEBAR_W: f32 = 300.0;
/// Single horizontal inset for everything in the sidebar. The header block
/// and the category rows used different values and visibly failed to line up.
pub(crate) const SIDEBAR_PAD: f32 = 18.0;

/// Named text styles beyond egui's built-in five.
pub(crate) fn ts(name: &str) -> egui::TextStyle {
    // egui's five built-ins are NOT `Name(..)` variants, and asking for
    // `Name("body")` panics at draw time rather than falling back. Mapping
    // them here means a caller can name any style as a plain string.
    match name {
        "body" => egui::TextStyle::Body,
        "heading" => egui::TextStyle::Heading,
        "button" => egui::TextStyle::Button,
        "small" => egui::TextStyle::Small,
        "monospace" => egui::TextStyle::Monospace,
        other => egui::TextStyle::Name(other.into()),
    }
}

/// `RichText` in a named style — the app's only typography entry point.
pub(crate) fn txt(text: impl Into<String>, style: &str, color: egui::Color32) -> egui::RichText {
    egui::RichText::new(text.into())
        .text_style(ts(style))
        .color(color)
}

/// IBM Plex Sans + IBM Plex Mono, bundled (SIL OFL 1.1, see
/// `assets/fonts/LICENSE.txt`). Without this the app renders in egui's
/// stock Ubuntu-Light at 12.5px, which is most of why it read as unfinished.
///
/// Static instances, not variable fonts: egui's `ab_glyph` backend reads
/// the default master and cannot instance a variable axis. It also does no
/// OpenType shaping — no `tnum` — so every byte count is monospace and
/// right-aligned instead.
pub(crate) fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &[u8]); 5] = [
        (
            "plex",
            include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf"),
        ),
        (
            "plex-med",
            include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf"),
        ),
        (
            "plex-semi",
            include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"),
        ),
        (
            "plex-mono",
            include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
        ),
        (
            "plex-mono-med",
            include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf"),
        ),
    ];
    for (name, bytes) in faces {
        fonts
            .font_data
            .insert(name.to_owned(), FontData::from_static(bytes));
    }
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "plex".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "plex-mono".to_owned());
    // Every custom family gets the SAME tail as `Proportional`, which
    // carries egui's own fallback faces. Without it a named family has no
    // fallback at all: the settings gear was drawn in `Name("med")` and
    // rendered as a missing-glyph box, while the identical character in
    // `Proportional` resolved fine.
    let fallbacks: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|name| !name.starts_with("plex"))
        .collect();
    for (family, faces) in [
        ("med", vec!["plex-med", "plex"]),
        ("semi", vec!["plex-semi", "plex"]),
        ("mono-med", vec!["plex-mono-med", "plex-mono"]),
    ] {
        let mut chain: Vec<String> = faces.into_iter().map(str::to_owned).collect();
        chain.extend(fallbacks.iter().cloned());
        fonts
            .families
            .insert(FontFamily::Name(family.into()), chain);
    }
    ctx.set_fonts(fonts);
}

/// Type scale and spacing rhythm. egui's defaults are `item_spacing (8, 3)`
/// and `button_padding (4, 1)` — one pixel of vertical padding is what made
/// every button a sliver.
pub(crate) fn install_metrics(ctx: &egui::Context) {
    use egui::{FontFamily as F, FontId, TextStyle::*};
    let mut style = (*ctx.style()).clone();
    let semi = F::Name("semi".into());
    let med = F::Name("med".into());
    style.text_styles = [
        (Heading, FontId::new(17.0, semi.clone())),
        (Body, FontId::new(13.5, F::Proportional)),
        (Button, FontId::new(13.0, med.clone())),
        (Small, FontId::new(11.0, med.clone())),
        (Monospace, FontId::new(13.0, F::Monospace)),
        (ts("display"), FontId::new(30.0, semi.clone())),
        (ts("title"), FontId::new(20.0, semi.clone())),
        (ts("strong"), FontId::new(13.5, med.clone())),
        (ts("caption"), FontId::new(12.0, F::Proportional)),
        (ts("micro"), FontId::new(11.0, med.clone())),
        // Severity pills sit inside a 38px row next to 13.5px body text;
        // at micro size they dominated the column.
        (ts("pill"), FontId::new(9.5, med)),
        (ts("mono_lg"), FontId::new(15.0, F::Name("mono-med".into()))),
        (ts("mono_sm"), FontId::new(12.0, F::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(space(2.5), space(2.0));
    style.spacing.button_padding = egui::vec2(space(3.0), space(1.75));
    style.spacing.interact_size = egui::vec2(space(12.0), space(7.0));
    style.spacing.window_margin = egui::Margin::same(space(4.0));
    style.spacing.menu_margin = egui::Margin::same(space(2.0));
    style.spacing.indent = space(5.0);
    style.spacing.icon_width = 16.0;
    style.spacing.scroll.bar_width = 10.0;
    ctx.set_style(style);
}

/// Slate ground, hairline separation, one restrained interactive blue.
///
/// `override_text_color` is deliberately NOT set: it used to resolve to the
/// same colour as `strong_text_color()`, which made every `.strong()` in
/// the app — table headers, the "Move to Trash" confirm button — render
/// identically to plain text.
pub(crate) fn ledger_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    let hairline = egui::Stroke::new(1.0_f32, COL_LINE);
    v.panel_fill = COL_SURFACE;
    v.window_fill = COL_RAISED;
    v.extreme_bg_color = COL_BG;
    v.faint_bg_color = COL_RAISED;
    v.hyperlink_color = COL_ACCENT;
    v.window_stroke = egui::Stroke::new(1.0_f32, COL_LINE_HI);
    v.window_rounding = egui::Rounding::same(R_XL);
    v.menu_rounding = egui::Rounding::same(R_LG);
    // Light from directly above, wide and soft. egui's default is a hard
    // 10px-right/20px-down offset — the most dated pixel on the screen.
    v.window_shadow = egui::Shadow {
        offset: egui::vec2(0.0, 12.0),
        blur: 40.0,
        spread: 0.0,
        color: egui::Color32::from_black_alpha(120),
    };
    v.popup_shadow = egui::Shadow {
        offset: egui::vec2(0.0, 4.0),
        blur: 16.0,
        spread: 0.0,
        color: egui::Color32::from_black_alpha(80),
    };

    v.widgets.noninteractive.bg_fill = COL_SURFACE;
    v.widgets.noninteractive.weak_bg_fill = COL_SURFACE;
    v.widgets.noninteractive.bg_stroke = hairline;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, COL_TEXT2);

    v.widgets.inactive.bg_fill = COL_RAISED;
    v.widgets.inactive.weak_bg_fill = COL_RAISED;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, COL_LINE_HI);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, COL_TEXT);

    v.widgets.hovered.bg_fill = COL_LINE;
    v.widgets.hovered.weak_bg_fill = COL_LINE;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, COL_ACCENT);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, COL_TEXT);

    v.widgets.active.bg_fill = COL_ACCENT_SOFT;
    v.widgets.active.weak_bg_fill = COL_ACCENT_SOFT;
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, COL_ACCENT);
    // `strong_text_color()` reads this: `.strong()` must be brighter than body.
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);

    v.widgets.open.bg_fill = COL_RAISED;
    v.widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, COL_LINE_HI);
    v.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, COL_TEXT);

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        // Small controls only. Buttons set `R_MD` explicitly; leaving the
        // global value there made checkboxes render as circles.
        w.rounding = egui::Rounding::same(R_SM);
    }

    v.selection.bg_fill = COL_ACCENT_SOFT;
    v.selection.stroke = egui::Stroke::new(1.0_f32, COL_ACCENT);
    v
}

/// The window icon, decoded from the shipped asset.
///
/// This replaced 62 lines of procedural signed-distance drawing. That code
/// thresholded alpha to 0 or 255 with no antialiasing, so every edge was
/// jagged — legible at 128 px, mush in a 48 px taskbar — and it stacked a
/// tile, a disc, a ring and a broom into one square. A broom is also the
/// wrong idea: it says "sweeping", while what makes Chystik different is
/// that it knows what a directory *is*.
///
/// The asset is generated by `packaging/render-icon.py`, which emits both
/// `assets/icon.svg` and every rasterised size from one geometry definition.
/// The app mark as an egui texture, decoded once and cached in the context.
///
/// Same asset as the window icon. Loaded through egui's own store rather
/// than an `image` loader so it costs no extra dependency and no per-frame
/// decode.
pub(crate) fn app_mark(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    const BYTES: &[u8] = include_bytes!("../../../assets/icon.png");
    let image = image::load_from_memory(BYTES).ok()?.into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(ctx.load_texture(
        "app-mark",
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
        egui::TextureOptions::LINEAR,
    ))
}

pub(crate) fn app_icon() -> Option<egui::IconData> {
    const BYTES: &[u8] = include_bytes!("../../../assets/icon.png");
    let image = image::load_from_memory(BYTES)
        .inspect_err(|e| eprintln!("[chystik] icon decode failed: {e}"))
        .ok()?
        .into_rgba8();

    // macOS's Dock compares the opaque canvas, not just the mark's visual
    // weight. Our rounded square fills the source asset more tightly than
    // most native app icons, so it reads slightly oversized beside them.
    // Keep the shared asset unchanged and add platform-specific transparent
    // breathing room only to the native window/Dock icon.
    #[cfg(target_os = "macos")]
    let image = {
        let (width, height) = image.dimensions();
        let inset = width.min(height) / 16;
        let resized = image::imageops::resize(
            &image,
            width.saturating_sub(inset * 2),
            height.saturating_sub(inset * 2),
            image::imageops::FilterType::Lanczos3,
        );
        let mut padded = image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 0]));
        image::imageops::overlay(&mut padded, &resized, i64::from(inset), i64::from(inset));
        padded
    };

    let (width, height) = image.dimensions();
    Some(egui::IconData {
        width,
        height,
        rgba: image.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Context::fonts` panics until the context has run a frame, so the
    /// glyph tests below need one.
    fn font_ready_context() -> egui::Context {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        install_metrics(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        ctx
    }

    /// Every style name the app passes to `txt`.
    ///
    /// egui panics at DRAW time on an unknown `Name(..)`, so a typo here is
    /// invisible until the affected widget is actually shown — the consent
    /// dialog crashed on first launch for exactly this reason. Keep this
    /// list in step with the names used across the GUI.
    const USED_STYLES: &[&str] = &[
        "body",
        "button",
        "caption",
        "display",
        "heading",
        "micro",
        "mono_lg",
        "mono_sm",
        "monospace",
        "pill",
        "small",
        "strong",
        "title",
    ];

    #[test]
    fn every_style_name_resolves_to_a_defined_style() {
        let ctx = egui::Context::default();
        install_metrics(&ctx);
        let style = ctx.style();
        for name in USED_STYLES {
            let resolved = ts(name);
            assert!(
                style.text_styles.contains_key(&resolved),
                "style {name:?} resolves to {resolved:?}, which install_metrics never defines"
            );
        }
    }

    /// Every character the app draws as TEXT must exist in the bundled
    /// fonts.
    ///
    /// egui silently substitutes a missing-glyph box, which only shows up
    /// on screen — this class of bug shipped three times before the test
    /// existed: the severity marks, the sort arrows and the settings gear
    /// all rendered as "?". Anything not covered here is painted instead;
    /// see `widgets::paint_*`.
    #[test]
    fn every_drawn_character_exists_in_the_bundled_fonts() {
        // Characters passed to `txt` / `label` anywhere in the GUI.
        const DRAWN: &str = "\u{2014}\u{2026}\u{00B7}\u{2713}\u{2192}\u{2013}";
        let ctx = font_ready_context();
        let families = [
            egui::FontFamily::Proportional,
            egui::FontFamily::Monospace,
            egui::FontFamily::Name("med".into()),
            egui::FontFamily::Name("semi".into()),
        ];
        ctx.fonts(|fonts| {
            for family in families {
                for c in DRAWN.chars() {
                    assert!(
                        fonts.has_glyph(&egui::FontId::new(13.0, family.clone()), c),
                        "U+{:04X} is missing from {family:?} — paint it instead of \
                         drawing it as text",
                        c as u32
                    );
                }
            }
        });
    }

    /// Named families must fall back exactly like `Proportional`.
    ///
    /// This is what actually broke: `Name("med")` was built from Plex faces
    /// alone, so a character Plex lacks rendered as a box there while the
    /// same character in `Proportional` resolved through egui's fallbacks.
    #[test]
    fn named_families_fall_back_like_the_default_one() {
        let ctx = font_ready_context();
        // Present in egui's fallback faces, absent from IBM Plex.
        const FALLBACK_ONLY: char = '\u{2139}';
        ctx.fonts(|fonts| {
            let base = egui::FontId::new(13.0, egui::FontFamily::Proportional);
            assert!(
                fonts.has_glyph(&base, FALLBACK_ONLY),
                "the premise of this test is stale: U+2139 no longer comes from a fallback"
            );
            for family in ["med", "semi", "mono-med"] {
                let id = egui::FontId::new(13.0, egui::FontFamily::Name(family.into()));
                assert!(
                    fonts.has_glyph(&id, FALLBACK_ONLY),
                    "family {family:?} has no fallback chain — a missing glyph there \
                     renders as a box"
                );
            }
        });
    }

    #[test]
    fn builtin_names_do_not_become_name_variants() {
        // `Name("body")` is not `TextStyle::Body`, and asking for the former
        // panics. The mapping in `ts` is what keeps them apart.
        assert_eq!(ts("body"), egui::TextStyle::Body);
        assert_eq!(ts("heading"), egui::TextStyle::Heading);
        assert_eq!(ts("monospace"), egui::TextStyle::Monospace);
        assert_eq!(ts("title"), egui::TextStyle::Name("title".into()));
    }
}

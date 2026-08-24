//! Chystik — a disk-cleanup tool for Linux developers.
//!
//! Finds caches, build outputs and stale tool data, tells you what each one
//! *is* and what deleting it costs, and moves what you pick to the desktop
//! trash. Deletion is trash-only and every path passes through
//! `chystik_core::guard::check` first.
//!
//! Module layout:
//! - `app`     — application state, scan lifecycle, deletion, export
//! - `consent` — the first-run risk acknowledgement and its record on disk
//! - `exclusions` — paths the user marked never-touch, enforced twice
//! - `panels`  — the window's regions (command bar, sidebar, table, footer)
//! - `modals`  — dialogs, including the first-run risk acknowledgement
//! - `widgets` — small painted pieces shared between panels
//! - `state`   — filters, scan targets and the cached view
//! - `theme`   — design tokens, fonts and the egui style
//! - `format`  — human-readable formatting
//! - `i18n`    — interface localisation, loaded from `locales/*.json`

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod consent;
mod exclusions;
mod format;
mod i18n;
mod modals;
mod panels;
mod state;
mod theme;
mod widgets;

use eframe::egui;

use app::ChystikApp;
use theme::{app_icon, install_fonts, install_metrics, ledger_visuals};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        // `with_icon` covers X11; on Wayland compositors (KWin) the
        // dock/taskbar icon comes from the desktop entry whose id matches
        // `app_id` — packaging/chystik.desktop (Icon=chystik) provides it.
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Chystik")
            .with_app_id("chystik")
            .with_icon(app_icon().unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native(
        "Chystik",
        options,
        Box::new(|cc| {
            let mut app = ChystikApp::default();
            app.refresh_disks(); // mount_table() once at startup
                                 // Variant D theme.
            install_fonts(&cc.egui_ctx);
            install_metrics(&cc.egui_ctx);
            cc.egui_ctx.set_visuals(ledger_visuals());
            // Headless smoke hook: start a scan without a click, so the
            // app can be exercised from a script.
            if std::env::var_os("CHYSTIK_AUTOSCAN").is_some() {
                app.start_scan();
            }
            Ok(Box::new(app))
        }),
    )
}

//! `freerouting-gui` desktop app.
//!
//! Usage: `freerouting-gui [board.dsn [board.ses]]`

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod file_dialog;
mod scene;
mod view;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let open: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Freerouting")
            .with_inner_size([1280.0, 800.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Freerouting",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, open)))),
    )
}

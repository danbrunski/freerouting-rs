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

/// Under WSL, use X11 instead of Wayland. WSLg gives X11 windows the
/// standard Windows title bar and buttons, while a Wayland window has to draw
/// its own, and winit's are a flat imitation. `FREEROUTING_WAYLAND=1` keeps
/// Wayland.
#[cfg(target_os = "linux")]
fn window_system(builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {
    use winit::platform::x11::EventLoopBuilderExtX11;
    let wsl =
        std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some();
    if wsl && std::env::var_os("FREEROUTING_WAYLAND").is_none() {
        builder.with_x11();
    }
}

#[cfg(not(target_os = "linux"))]
fn window_system(_builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {}

fn main() -> eframe::Result {
    let open: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Freerouting")
            .with_inner_size([1280.0, 800.0])
            .with_drag_and_drop(true),
        event_loop_builder: Some(Box::new(window_system)),
        ..Default::default()
    };
    eframe::run_native(
        "Freerouting",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, open)))),
    )
}

//! `freerouting-gui` desktop app.
//!
//! Usage: `freerouting-gui [board.dsn [board.ses]] [--bench pan-zoom]
//! [--frames N] [--screenshot out.png] [--exit]`, or Java's front-end form
//! `freerouting-gui -de board.dsn -do board.ses [-dct seconds] [-dr board.rules]`
//! (G08.4): routes after a countdown, writes the session, exits with 0 (written)
//! or 1 (the write failed or the board did not load).

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use freerouting_gui::App;
use freerouting_gui::args::Args;

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
    let args = Args::parse(std::env::args_os().skip(1)).unwrap_or_else(|e| {
        eprintln!("freerouting-gui: {e}");
        std::process::exit(2);
    });
    // A scripted run (bench, screenshot) neither reads nor writes the user's
    // saved settings: it starts from a throwaway file.
    let persistence_path = args.scripted().then(|| {
        let path = std::env::temp_dir().join(format!("freerouting-gui-{}.ron", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    });
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Freerouting")
            .with_inner_size([1280.0, 800.0])
            .with_drag_and_drop(true),
        event_loop_builder: Some(Box::new(window_system)),
        persist_window: !args.scripted(),
        persistence_path: persistence_path.clone(),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Freerouting",
        options,
        Box::new(|cc| Ok(Box::new(App::build(&cc.egui_ctx, cc.storage, args)))),
    );
    if let Some(path) = persistence_path {
        let _ = std::fs::remove_file(path);
    }
    result?;
    // The front-end mode (`-de`, `-do`) reports a failed write or load.
    let code = freerouting_gui::exit_code();
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

//! Shared helpers for the egui_kittest tests.

#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use freerouting_gui::App;

/// The window size of the app tests.
pub const SIZE: [f32; 2] = [1280.0, 800.0];

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Whether tests that render pixels (snapshots) run on this machine.
///
/// They are checked on Linux with lavapipe (`mesa-vulkan-drivers`), where
/// anti-aliasing is the one the committed PNGs were made with. Elsewhere
/// they are skipped unless `FREEROUTING_GUI_SNAPSHOTS=1`. Interaction tests
/// run everywhere.
pub fn rendering_enabled() -> bool {
    cfg!(target_os = "linux") || std::env::var("FREEROUTING_GUI_SNAPSHOTS").is_ok_and(|v| v == "1")
}

fn paths(files: &[&str]) -> Vec<PathBuf> {
    files.iter().map(|f| fixtures().join(f)).collect()
}

/// An app that has loaded `files` (names in `fixtures/`: a design, then
/// optionally a session), without a renderer. Enough for interaction tests.
pub fn plain_harness(files: &[&str]) -> Harness<'static, App> {
    let open = paths(files);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .build_eframe(|cc| App::new(cc, open));
    wait_for_load(&mut harness);
    harness
}

/// [`plain_harness`] with the inspect tool chosen: the tests of the
/// selection (G03, G05) click on the canvas, and the app starts in route mode
/// like Java (G09).
pub fn inspect_harness(files: &[&str]) -> Harness<'static, App> {
    let mut harness = plain_harness(files);
    let ctx = harness.ctx.clone();
    harness.state_mut().execute(
        &ctx,
        freerouting_gui::commands::Command::SetTool(freerouting_gui::tools::ToolMode::Select),
    );
    let _ = harness.run_ok();
    harness
}

/// Like [`plain_harness`], with the wgpu renderer so `snapshot` and
/// `render` work. On Linux a missing Vulkan adapter fails with a message
/// naming the package to install.
pub fn app_harness(files: &[&str]) -> Harness<'static, App> {
    let open = paths(files);
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Harness::builder()
            .with_size(SIZE)
            .wgpu()
            .build_eframe(|cc| App::new(cc, open))
    }));
    let mut harness = built.unwrap_or_else(|_| {
        panic!(
            "no wgpu adapter for the rendering tests; on Linux install mesa-vulkan-drivers \
             (lavapipe) and set WGPU_BACKEND=vulkan"
        )
    });
    wait_for_load(&mut harness);
    harness
}

/// Steps frames until the background load and the first analysis finish,
/// then until the UI is still.
pub fn wait_for_load(harness: &mut Harness<'_, App>) {
    let start = Instant::now();
    harness.step();
    while harness.state().is_loading() {
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "the board did not load"
        );
        std::thread::sleep(Duration::from_millis(5));
        harness.step();
    }
    // The side panel and the overlays show the background analysis (G03);
    // wait for it so that the UI, and with it every snapshot, is settled.
    while let Some(loaded) = harness.state().loaded() {
        if harness.state().analysis().is_idle(&loaded.session) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "the analysis did not finish"
        );
        std::thread::sleep(Duration::from_millis(5));
        harness.step();
    }
    let _ = harness.run_ok();
    assert!(
        harness.state().error().is_none(),
        "load failed: {:?}",
        harness.state().error()
    );
}

//! The viewer through egui_kittest: loading, the side panel, and snapshots of
//! the window, the File menu and the toolbar.

mod common;

use common::{app_harness, plain_harness, rendering_enabled};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use freerouting::board::structure::Unit;
use freerouting_gui::overlays::OverlayState;
use freerouting_gui::scene::Kind;
use freerouting_gui::toolbar;
use freerouting_gui::tools::ToolMode;

const ISSUE159: &str = "Issue159-setonix_2hp-pcb.dsn";

#[test]
fn loads_issue159() {
    let harness = plain_harness(&[ISSUE159]);
    let loaded = harness.state().loaded().expect("board loaded");
    assert!(!loaded.scene.items.is_empty());
    assert!(loaded.session.paths.design.ends_with(ISSUE159));
    harness.get_by_label("Traces");
    harness.get_by_label("Vias");
}

#[test]
fn kind_checkbox_toggles_state() {
    let mut harness = plain_harness(&[ISSUE159]);
    assert!(harness.state().view().kind_on(Kind::Via));
    harness.get_by_label("Vias").click();
    harness.run();
    assert!(!harness.state().view().kind_on(Kind::Via));
    assert!(harness.state().view().kind_on(Kind::Trace));
    harness.get_by_label("Vias").click();
    harness.run();
    assert!(harness.state().view().kind_on(Kind::Via));
}

#[test]
fn snapshot_issue159() {
    if !rendering_enabled() {
        eprintln!("skipped: rendering tests need Linux or FREEROUTING_GUI_SNAPSHOTS=1");
        return;
    }
    let mut harness = app_harness(&[ISSUE159]);
    // `viewer_issue159.png` is the G01 golden that tests/ui_render.rs compares
    // the GPU canvas with; the full window, with G03's toolbar and Checks
    // section, has its own snapshot.
    harness.snapshot("viewer_issue159_g03");
}

/// The open File menu: its icon column, the shortcuts on the right and the
/// toolbar under it.
#[test]
fn snapshot_file_menu() {
    if !rendering_enabled() {
        eprintln!("skipped: rendering tests need Linux or FREEROUTING_GUI_SNAPSHOTS=1");
        return;
    }
    let mut harness = app_harness(&[ISSUE159]);
    harness.get_by_label("File").click();
    harness.run();
    // The icons are painted, so the labels are the texts alone.
    harness.get_by_label("Reload");
    harness.get_by_label_contains("Open Recent");
    harness.snapshot("file_menu_issue159");
}

/// The toolbar alone, idle and while the autorouter runs (red Cancel, the
/// tools greyed out).
#[test]
fn snapshot_toolbar_states() {
    if !rendering_enabled() {
        eprintln!("skipped: rendering tests need Linux or FREEROUTING_GUI_SNAPSHOTS=1");
        return;
    }
    for (routing, name) in [(false, "toolbar_idle"), (true, "toolbar_routing")] {
        let mut h = Harness::builder()
            .with_size([1000.0, 40.0])
            .wgpu()
            .build_ui(move |ui| {
                let _ = toolbar::ui(
                    ui,
                    OverlayState::default(),
                    true,
                    Unit::Mm,
                    ToolMode::Route,
                    false,
                    routing,
                );
            });
        h.run();
        h.get_by_label(if routing { "Cancel" } else { "Autoroute" });
        h.snapshot(name);
    }
}

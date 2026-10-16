//! The viewer through egui_kittest: loading, the side panel, a snapshot.

mod common;

use common::{app_harness, plain_harness, rendering_enabled};
use egui_kittest::kittest::Queryable;
use freerouting_gui::scene::Kind;

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
    harness.snapshot("viewer_issue159");
}

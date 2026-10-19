//! Persisted settings and the Open Recent menu (G01.8), without rendering.

mod common;

use std::collections::HashMap;

use common::{fixtures, plain_harness, wait_for_load};
use eframe::Storage as _;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use freerouting_gui::App;
use freerouting_gui::args::Args;
use freerouting_gui::commands::Command;
use freerouting_gui::scene::Kind;
use freerouting_gui::settings::{PersistedSettings, SCHEMA, STORAGE_KEY};

const A: &str = "Issue159-setonix_2hp-pcb.dsn";
const B: &str = "Issue313-FastTest.dsn";

/// An in-memory `eframe::Storage`.
#[derive(Default)]
struct MemStorage(HashMap<String, String>);

impl eframe::Storage for MemStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
    fn flush(&mut self) {}
}

fn app_from(storage: &MemStorage) -> Harness<'static, App> {
    Harness::builder()
        .with_size(common::SIZE)
        .build_eframe(|cc| App::build(&cc.egui_ctx, Some(storage), Args::default()))
}

fn run(h: &mut Harness<'static, App>, command: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, command);
    // A load in progress keeps a spinner repainting, so `run` would not end.
    h.run_steps(3);
}

fn open(h: &mut Harness<'static, App>, file: &str) {
    run(h, Command::Open(fixtures().join(file)));
    wait_for_load(h);
}

#[test]
fn settings_round_trip() {
    let mut h = plain_harness(&[A]);
    run(&mut h, Command::ToggleKind(Kind::Via));
    run(&mut h, Command::ToggleLayer(0));
    run(&mut h, Command::ToggleLabels);
    open(&mut h, B);

    let mut storage = MemStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    let saved = h.state().settings_snapshot();
    assert_eq!(saved.recent_files.len(), 2);
    assert!(saved.recent_files[0].ends_with(B) && saved.recent_files[1].ends_with(A));
    assert!(!saved.kind_visible[Kind::Via.label()]);
    assert!(!saved.show_labels);
    assert!(saved.last_dir.is_some() && saved.side_panel_width.is_some());

    let fresh = app_from(&storage);
    assert_eq!(fresh.state().settings_snapshot(), saved);
    assert!(!fresh.state().view().kind_on(Kind::Via));
    assert!(!fresh.state().view().show_labels);
}

#[test]
fn layer_visibility_comes_back_by_name() {
    let mut h = plain_harness(&[A]);
    run(&mut h, Command::ToggleLayer(1));
    let name = h.state().loaded().unwrap().scene.layers[1].name.clone();
    let mut storage = MemStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);

    let mut fresh = app_from(&storage);
    open(&mut fresh, A);
    assert_eq!(fresh.state().view().layer_visible, [true, false], "{name}");
}

#[test]
fn unknown_keys_fall_back() {
    let defaults = app_from(&MemStorage::default()).state().settings_snapshot();
    assert_eq!(defaults, {
        let mut d = defaults.clone();
        d.schema = SCHEMA;
        d
    });

    // A schema this version does not know: everything is ignored.
    let mut storage = MemStorage::default();
    storage.set_string(
        STORAGE_KEY,
        r#"(schema: 999, show_labels: false, unknown_field: 5, recent_files: ["x.dsn"])"#.into(),
    );
    let h = app_from(&storage);
    assert_eq!(h.state().settings_snapshot(), defaults);

    // Garbage that is not even RON.
    storage.set_string(STORAGE_KEY, "not ron at all {{".into());
    assert_eq!(app_from(&storage).state().settings_snapshot(), defaults);

    // The right schema with an unknown field, a layer and a kind that do not
    // exist: what is known is restored, the rest is ignored.
    storage.set_string(
        STORAGE_KEY,
        format!(
            r#"(schema: {SCHEMA}, show_labels: false, future_option: 7,
               layer_visible_by_name: {{"No such layer": false}},
               kind_visible: {{"No such kind": false, "Vias": false}})"#
        ),
    );
    let mut h = app_from(&storage);
    assert!(!h.state().view().show_labels);
    assert!(!h.state().view().kind_on(Kind::Via));
    open(&mut h, A);
    assert!(h.state().view().layer_visible.iter().all(|&v| v));
}

#[test]
fn open_recent_lists_files() {
    let mut h = plain_harness(&[A]);
    open(&mut h, B);
    h.get_by_label("File").click();
    h.run();
    h.get_by_label_contains("Open Recent").hover();
    h.run();
    // The menu bar also shows the open file's name; the entries are buttons.
    let entry = |h: &Harness<'static, App>, name: &str| {
        h.query_all_by_label_contains(name)
            .find(|n| n.accesskit_node().role() == eframe::egui::accesskit::Role::Button)
            .unwrap_or_else(|| panic!("no menu entry for {name}"))
            .rect()
    };
    let (first, second) = (entry(&h, B), entry(&h, A));
    assert!(
        first.min.y < second.min.y,
        "newest first: {first:?} {second:?}"
    );
}

#[test]
fn a_missing_recent_file_is_listed_disabled() {
    let mut storage = MemStorage::default();
    let mut settings = PersistedSettings::default();
    settings.remember_file(fixtures().join("gone-for-good.dsn"));
    settings.save(&mut storage);
    let mut h = app_from(&storage);
    h.get_by_label("File").click();
    h.run();
    h.get_by_label_contains("Open Recent").hover();
    h.run();
    let item = h.get_by_label_contains("gone-for-good.dsn");
    assert!(item.accesskit_node().is_disabled());
    assert_eq!(
        h.state().settings_snapshot().recent_files.len(),
        1,
        "not removed"
    );
}

/// G07: "Stop routing on a canvas click" is on by default (Java's behaviour),
/// and an off value comes back after a save and a restore.
#[test]
fn stop_on_canvas_click_round_trips() {
    let mut h = plain_harness(&[A]);
    assert!(h.state().settings_snapshot().stop_on_canvas_click);
    run(&mut h, Command::SetStopOnCanvasClick(false));
    assert!(!h.state().settings_snapshot().stop_on_canvas_click);
    let mut storage = MemStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);

    let mut fresh = app_from(&storage);
    assert!(!fresh.state().settings_snapshot().stop_on_canvas_click);
    run(&mut fresh, Command::SetStopOnCanvasClick(true));
    assert!(fresh.state().settings_snapshot().stop_on_canvas_click);

    // Settings stored before the preference existed keep Java's behaviour.
    let mut old = MemStorage::default();
    old.set_string(STORAGE_KEY, "(schema: 1, show_labels: false)".into());
    let restored = app_from(&old).state().settings_snapshot();
    assert!(restored.stop_on_canvas_click && !restored.show_labels);
}

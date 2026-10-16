//! G05: inspect mode (click, toggle and region select), the selection
//! settings, extending the selection, the selection toolbar, undo and redo
//! with scene patching, context menus and keepout construction. The tests
//! drive the app with raw pointer events where the item says so, and with
//! commands otherwise; expected values come from engine calls.

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use common::plain_harness;
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use freerouting::board::items::{AreaKind, ItemId, ItemKind, StopConnectionOption};
use freerouting::board::{ItemFilter, RoutingBoard, SelectableChoice};
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{Area, IntBox, IntPoint, Point, Shape};
use freerouting_gui::App;
use freerouting_gui::commands::{Command, ExtendKind, KeepoutKind};
use freerouting_gui::overlays::Frame;
use freerouting_gui::panels::select_settings::{SelectionLayers, SelectionSettings};
use freerouting_gui::panels::{item_info, selection_toolbar};
use freerouting_gui::scene::P;
use freerouting_gui::text;
use freerouting_gui::tools::inspect::board_point;
use freerouting_gui::tools::popup;

const ISSUE159: &str = "Issue159-setonix_2hp-pcb.dsn";
const ISSUE754: &str = "Issue754-avionics_hub.dsn";
const ISSUE684: &str = "Issue684-Autorouter_PCB1_2026-5-8.dsn";
const J2: [&str; 2] = ["Issue026-J2_reference.dsn", "Issue026-J2_reference.ses"];

macro_rules! settle {
    ($h:expr) => {
        let _ = $h.run_ok();
    };
}

fn exec(h: &mut Harness<'_, App>, command: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, command);
    settle!(h);
}

fn board_of(h: &Harness<'_, App>) -> Arc<RoutingBoard> {
    h.state().loaded().unwrap().session.board.clone()
}

fn camera(h: &Harness<'_, App>) -> freerouting_gui::view::Camera {
    h.state().view().camera.expect("camera")
}

fn selected(h: &Harness<'_, App>) -> BTreeSet<ItemId> {
    h.state().selection().items.clone()
}

/// Where the point `mm` of the scene is on the screen.
fn screen(h: &Harness<'_, App>, mm: P) -> Pos2 {
    camera(h).to_screen(h.state().view().canvas, mm)
}

/// The centre of item `id` on the screen.
fn item_screen(h: &Harness<'_, App>, id: ItemId) -> Pos2 {
    let board = board_of(h);
    let b = board.basic().item(id).unwrap().bounding_box();
    let centre = freerouting::geometry::FloatPoint::new(
        (f64::from(b.ll.x) + f64::from(b.ur.x)) / 2.0,
        (f64::from(b.ll.y) + f64::from(b.ur.y)) / 2.0,
    );
    screen(h, Frame::new(board.basic()).pt(centre))
}

/// The board point (rounded) under a screen point, as the tools compute it.
fn board_under(h: &Harness<'_, App>, pos: Pos2) -> IntPoint {
    let mm = camera(h).to_board(h.state().view().canvas, pos);
    board_point(board_of(h).basic(), mm).round()
}

// --- raw pointer events (as in ui_input.rs) ----------------------------------

fn push(h: &mut Harness<'_, App>, event: Event) {
    h.input_mut().events.push(event);
    h.step();
}

fn hover(h: &mut Harness<'_, App>, pos: Pos2) {
    push(h, Event::PointerMoved(pos));
}

fn button(h: &mut Harness<'_, App>, pos: Pos2, b: PointerButton, pressed: bool, m: Modifiers) {
    push(
        h,
        Event::PointerButton {
            pos,
            button: b,
            pressed,
            modifiers: m,
        },
    );
}

fn click_with(h: &mut Harness<'_, App>, pos: Pos2, b: PointerButton, m: Modifiers) {
    h.input_mut().modifiers = m;
    hover(h, pos);
    button(h, pos, b, true, m);
    button(h, pos, b, false, m);
    h.input_mut().modifiers = Modifiers::NONE;
    settle!(h);
}

fn click(h: &mut Harness<'_, App>, pos: Pos2) {
    click_with(h, pos, PointerButton::Primary, Modifiers::NONE);
}

fn drag(h: &mut Harness<'_, App>, from: Pos2, to: Pos2) {
    hover(h, from);
    button(h, from, PointerButton::Primary, true, Modifiers::NONE);
    hover(h, to);
    button(h, to, PointerButton::Primary, false, Modifiers::NONE);
    settle!(h);
}

fn load(files: &[&str]) -> Harness<'static, App> {
    plain_harness(files)
}

/// Pins (by id) that a click at their centre picks alone on the current
/// layer, far enough below the selection toolbar's strip and inside the
/// canvas, nearest the canvas centre first.
fn lonely_pins(h: &Harness<'_, App>) -> Vec<(ItemId, Pos2)> {
    let board = board_of(h);
    let filter = ItemFilter::default();
    let canvas = h.state().view().canvas;
    let layer = h.state().view().active_layer as i32;
    let mut out = Vec::new();
    for item in board.basic().items().filter(|i| i.is_pin()) {
        let id = item.id();
        let pos = item_screen(h, id);
        if !canvas.shrink(40.0).contains(pos) || pos.y < canvas.min.y + 90.0 {
            continue;
        }
        let at = board_under(h, pos);
        let picked = board
            .basic()
            .pick_items_filtered(&Point::from(at), layer, Some(&filter));
        if picked.len() == 1 && picked.contains(&id) {
            out.push((id, pos));
        }
    }
    out.sort_by(|a, b| {
        let da = (a.1 - canvas.center()).length_sq();
        let db = (b.1 - canvas.center()).length_sq();
        da.total_cmp(&db).then(a.0.cmp(&b.0))
    });
    out
}

// ============================================================================
// G05.1 inspect mode
// ============================================================================

#[test]
fn region_select_picks_pins_only() {
    let mut h = load(&[ISSUE159]);
    // Only pins (plus both fixed states), as the item says.
    h.state_mut().tools_mut().select = SelectionSettings::only(SelectableChoice::Pins);
    let board = board_of(&h);
    // The pins of the component with the most pins: a pad cluster.
    let component = board
        .basic()
        .components
        .iter()
        .max_by_key(|c| board.basic().component_pins(c.id.0).len())
        .unwrap()
        .id
        .0;
    let pins = board.basic().component_pins(component);
    assert!(pins.len() >= 3, "a cluster of pads");
    let boxes: Vec<IntBox> = pins
        .iter()
        .map(|id| board.basic().item(*id).unwrap().bounding_box())
        .collect();
    let union = boxes.iter().fold(boxes[0], |a, b| a.union_box(b));
    let frame = Frame::new(board.basic());
    let corner = |x: i32, y: i32| {
        screen(
            &h,
            frame.pt(freerouting::geometry::FloatPoint::new(
                f64::from(x),
                f64::from(y),
            )),
        )
    };
    let (a, b) = (
        corner(union.ll.x, union.ll.y),
        corner(union.ur.x, union.ur.y),
    );
    let (from, to) = (
        a + (if a.x < b.x {
            vec2(-6.0, 0.0)
        } else {
            vec2(6.0, 0.0)
        }),
        b + (if a.x < b.x {
            vec2(6.0, 0.0)
        } else {
            vec2(-6.0, 0.0)
        }),
    );
    drag(&mut h, from, to);

    // The expected set: the engine's overlap query on the same box, through
    // the same filter, on all layers.
    let (p, q) = (board_under(&h, from), board_under(&h, to));
    let area = Area::from(Shape::from(IntBox::new(
        p.x.min(q.x),
        p.y.min(q.y),
        p.x.max(q.x),
        p.y.max(q.y),
    )));
    let expected = ItemFilter::only(SelectableChoice::Pins)
        .filter(board.basic(), &board.basic().overlapping_items(&area, -1));
    assert!(!expected.is_empty());
    for pin in &pins {
        assert!(expected.contains(pin), "pin {pin:?} of the cluster");
    }
    assert_eq!(selected(&h), expected);
    assert!(
        selected(&h)
            .iter()
            .all(|id| board.basic().item(*id).unwrap().is_pin()),
        "only pins"
    );
    assert_eq!(h.state().notice(), Some(text::IN_INSPECT_MODE));
}

#[test]
fn click_on_empty_space_clears_selection() {
    let mut h = load(&[ISSUE159]);
    let (pin, pos) = lonely_pins(&h)[0];
    click(&mut h, pos);
    assert_eq!(selected(&h), BTreeSet::from([pin]));

    // A point where the pick finds nothing on any layer.
    let board = board_of(&h);
    let canvas = h.state().view().canvas;
    let filter = ItemFilter::default();
    let layers = board.basic().layer_structure.layers.len() as i32;
    let empty = (0..40)
        .flat_map(|i| (0..20).map(move |j| (i, j)))
        .map(|(i, j)| {
            Pos2::new(
                canvas.min.x + 10.0 + 14.0 * i as f32,
                canvas.min.y + 100.0 + 14.0 * j as f32,
            )
        })
        .find(|pos| {
            let at = Point::from(board_under(&h, *pos));
            (0..layers).all(|l| {
                board
                    .basic()
                    .pick_items_filtered(&at, l, Some(&filter))
                    .is_empty()
            })
        })
        .expect("an empty spot on the canvas");
    click(&mut h, empty);
    assert!(selected(&h).is_empty(), "the click ended the selection");
    assert_eq!(h.state().notice(), Some(text::IN_INSPECT_MENU));
}

#[test]
fn ctrl_click_toggles() {
    let mut h = load(&[ISSUE159]);
    let pins = lonely_pins(&h);
    let ((a, pa), (b, pb)) = (pins[0], pins[1]);
    assert_ne!(a, b);
    // Nothing selected: a plain click selects what is under the cursor.
    click(&mut h, pa);
    assert_eq!(selected(&h), BTreeSet::from([a]));
    // Ctrl+click adds the second pin ...
    click_with(&mut h, pb, PointerButton::Primary, Modifiers::COMMAND);
    assert_eq!(selected(&h), BTreeSet::from([a, b]));
    // ... and removes it again.
    click_with(&mut h, pb, PointerButton::Primary, Modifiers::COMMAND);
    assert_eq!(selected(&h), BTreeSet::from([a]));
    // With a selection a plain click toggles too (Java `toggleSelect`).
    click(&mut h, pb);
    assert_eq!(selected(&h), BTreeSet::from([a, b]));
    click(&mut h, pb);
    assert_eq!(selected(&h), BTreeSet::from([a]));
    // Removing the last item ends the selection.
    click(&mut h, pa);
    assert!(selected(&h).is_empty());
    // Ctrl+click with nothing selected selects as well.
    click_with(&mut h, pa, PointerButton::Primary, Modifiers::COMMAND);
    assert_eq!(selected(&h), BTreeSet::from([a]));
}

#[test]
fn left_drag_does_not_pan() {
    let mut h = load(&[ISSUE159]);
    let before = camera(&h);
    let canvas = h.state().view().canvas;
    let from = canvas.min + vec2(300.0, 300.0);
    drag(&mut h, from, from + vec2(120.0, 60.0));
    let after = camera(&h);
    assert_eq!((after.center, after.zoom), (before.center, before.zoom));
    // And Esc clears what the drag selected.
    h.key_press(Key::Escape);
    settle!(h);
    assert!(selected(&h).is_empty());
}

// ============================================================================
// G05.2 selection settings
// ============================================================================

/// A trace on layer 1 and a point of it where only that trace is, on any
/// layer: nothing is on layer 0 there.
fn trace_only_on_layer_1(h: &Harness<'_, App>) -> (ItemId, Pos2) {
    let board = board_of(h);
    let filter = ItemFilter::all();
    let frame = Frame::new(board.basic());
    let canvas = h.state().view().canvas;
    for item in board.basic().items() {
        let ItemKind::Trace(trace) = &item.kind else {
            continue;
        };
        if trace.layer != 1 {
            continue;
        }
        let a = trace.polyline.corner_approx(0);
        let b = trace.polyline.corner_approx(1);
        let mid = freerouting::geometry::FloatPoint::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let pos = screen(h, frame.pt(mid));
        if !canvas.shrink(40.0).contains(pos) || pos.y < canvas.min.y + 90.0 {
            continue;
        }
        let at = Point::from(board_under(h, pos));
        let on = |l: i32| board.basic().pick_items_filtered(&at, l, Some(&filter));
        if on(0).is_empty() && on(1) == BTreeSet::from([item.id()]) {
            return (item.id(), pos);
        }
    }
    panic!("no trace that is alone on layer 1");
}

#[test]
fn current_layer_only_ignores_other_layers() {
    let mut h = load(&J2);
    let (trace, pos) = trace_only_on_layer_1(&h);
    assert_eq!(h.state().view().active_layer, 0);

    exec(
        &mut h,
        Command::SetSelectionLayers(SelectionLayers::CurrentOnly),
    );
    click(&mut h, pos);
    assert!(
        selected(&h).is_empty(),
        "layer 0 only: the trace is not found"
    );

    exec(
        &mut h,
        Command::SetSelectionLayers(SelectionLayers::AllVisible),
    );
    click(&mut h, pos);
    assert_eq!(selected(&h), BTreeSet::from([trace]));

    // A hidden layer is not searched either.
    exec(&mut h, Command::ClearSelection);
    exec(&mut h, Command::ToggleLayer(1));
    click(&mut h, pos);
    assert!(selected(&h).is_empty(), "layer 1 is hidden");
}

#[test]
fn selection_settings_persist() {
    #[derive(Default)]
    struct Mem(std::collections::HashMap<String, String>);
    impl eframe::Storage for Mem {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_string(), value);
        }
        fn flush(&mut self) {}
    }
    let mut h = load(&[ISSUE159]);
    exec(
        &mut h,
        Command::SetSelectionLayers(SelectionLayers::CurrentOnly),
    );
    exec(
        &mut h,
        Command::SetSelectable(SelectableChoice::Vias, false),
    );
    exec(
        &mut h,
        Command::SetSelectable(SelectableChoice::Keepout, true),
    );
    let chosen = h.state().tools().select.clone();
    let mut storage = Mem::default();
    eframe::App::save(h.state_mut(), &mut storage);

    let fresh = Harness::builder()
        .with_size(common::SIZE)
        .build_eframe(|cc| {
            App::build(
                &cc.egui_ctx,
                Some(&storage),
                freerouting_gui::args::Args::default(),
            )
        });
    assert_eq!(fresh.state().tools().select, chosen);
    assert_eq!(chosen.layers, SelectionLayers::CurrentOnly);
    assert!(!chosen.is_on(SelectableChoice::Vias) && chosen.is_on(SelectableChoice::Keepout));
    // The defaults are not stored at all.
    let defaults = load(&[ISSUE159]);
    let snapshot = defaults.state().settings_snapshot();
    assert_eq!(
        (snapshot.selection_layers, snapshot.selectable_items),
        (None, None)
    );
}

#[test]
fn the_settings_window_edits_the_choices() {
    let mut h = load(&[ISSUE159]);
    h.get_by_label("Settings").click();
    settle!(h);
    h.get_by_label("Select").click();
    settle!(h);
    assert!(h.state().tools().settings_open);
    h.get_by_label("Current layer only").click();
    h.get_by_label("keepout").click();
    settle!(h);
    let select = &h.state().tools().select;
    assert_eq!(select.layers, SelectionLayers::CurrentOnly);
    assert!(select.is_on(SelectableChoice::Keepout));
}

// ============================================================================
// G05.3 extend the selection
// ============================================================================

#[test]
fn extend_selection_matches_engine() {
    let mut h = load(&[ISSUE754]);
    let board = board_of(&h);
    let b = board.basic();
    // A pin of a net with other pins and wiring around it.
    let pin = b
        .items()
        .filter(|i| i.is_pin() && !i.net_numbers().is_empty() && i.component_id() > 0)
        .map(|i| i.id())
        .find(|id| {
            b.connectable_items(b.item(*id).unwrap().net_numbers()[0])
                .len()
                > 3
        })
        .expect("a pin on a net");
    let filter = ItemFilter::default();
    let start = BTreeSet::from([pin]);

    // Nets: replaced by the connectable items of every net of the selection.
    exec(&mut h, Command::Select(vec![pin]));
    exec(&mut h, Command::ExtendSelection(ExtendKind::Nets));
    let nets: BTreeSet<i32> = b.item(pin).unwrap().net_numbers().iter().copied().collect();
    let expected: Vec<ItemId> = nets.iter().flat_map(|n| b.connectable_items(*n)).collect();
    assert_eq!(selected(&h), filter.filter(b, &expected));
    assert!(selected(&h).len() > 1);

    // Connected sets.
    exec(&mut h, Command::Select(vec![pin]));
    exec(&mut h, Command::ExtendSelection(ExtendKind::ConnectedSets));
    assert_eq!(
        selected(&h),
        filter.filter(b, &b.connected_set(pin, -1, false))
    );

    // Connections.
    exec(&mut h, Command::Select(vec![pin]));
    exec(&mut h, Command::ExtendSelection(ExtendKind::Connections));
    let connection = b.connection_items(pin, StopConnectionOption::None);
    assert_eq!(selected(&h), filter.filter(b, &connection));

    // Components: the selection plus the items of its components, unfiltered.
    exec(&mut h, Command::Select(vec![pin]));
    exec(&mut h, Command::ExtendSelection(ExtendKind::Components));
    let mut components = start.clone();
    components.extend(b.component_items(b.item(pin).unwrap().component_id()));
    assert_eq!(selected(&h), components);
    assert!(selected(&h).len() > 1);
    // The component's outline is in it although the filter never selects it.
    assert!(
        selected(&h)
            .iter()
            .any(|id| b.item(*id).unwrap().selectable_choice().is_none()),
        "unfiltered"
    );

    // An empty result clears the selection: nothing but keepouts passes.
    for choice in SelectableChoice::ALL {
        exec(
            &mut h,
            Command::SetSelectable(choice, choice == SelectableChoice::Keepout),
        );
    }
    exec(&mut h, Command::Select(vec![pin]));
    exec(&mut h, Command::ExtendSelection(ExtendKind::Nets));
    assert!(selected(&h).is_empty());
    assert_eq!(h.state().notice(), Some(text::IN_INSPECT_MENU));
}

#[test]
fn extend_keys_n_s_e() {
    let mut h = load(&[ISSUE754]);
    let board = board_of(&h);
    let b = board.basic();
    let pin = b
        .items()
        .filter(|i| i.is_pin() && !i.net_numbers().is_empty())
        .map(|i| i.id())
        .next()
        .unwrap();
    exec(&mut h, Command::Select(vec![pin]));
    h.key_press(Key::N);
    settle!(h);
    let by_key = selected(&h);
    exec(&mut h, Command::Select(vec![pin]));
    exec(&mut h, Command::ExtendSelection(ExtendKind::Nets));
    assert_eq!(by_key, selected(&h));
    // S and E act on a selection only; B stays redo.
    exec(&mut h, Command::Select(vec![pin]));
    h.key_press(Key::S);
    settle!(h);
    assert_eq!(
        selected(&h),
        b.connected_set(pin, -1, false)
            .into_iter()
            .collect::<BTreeSet<_>>()
            .intersection(&selected(&h))
            .copied()
            .collect()
    );
    assert!(selected(&h).contains(&pin));
}

// ============================================================================
// G05.4 selection toolbar, edits, item info
// ============================================================================

/// How many nodes of the AccessKit tree are buttons with this label on the
/// canvas (the menu bar and the main toolbar are above it).
fn buttons_labelled(h: &Harness<'_, App>, label: &str) -> usize {
    let canvas = h.state().view().canvas;
    h.query_all_by_label(label)
        .filter(|n| n.accesskit_node().role() == eframe::egui::accesskit::Role::Button)
        .filter(|n| n.rect().min.y >= canvas.min.y)
        .count()
}

/// Clicks the button `label` of the selection toolbar.
fn click_toolbar(h: &mut Harness<'_, App>, label: &str) {
    let canvas = h.state().view().canvas;
    h.query_all_by_label(label)
        .find(|n| {
            n.accesskit_node().role() == eframe::egui::accesskit::Role::Button
                && n.rect().min.y >= canvas.min.y
        })
        .unwrap_or_else(|| panic!("toolbar button {label}"))
        .click();
    settle!(h);
}

#[test]
fn toolbar_buttons_match_java_set() {
    let mut h = load(&[ISSUE159]);
    // Java's `BoardToolbarInspectedItem` has these ten, in this order
    // (`BoardToolbarInspectedItem_en.properties`); decision G00 question 6
    // adds Delete, Fix and Unfix after them.
    let java = [
        "Cancel",
        "Info",
        "Nets",
        "Connected Sets",
        "Connections",
        "Components",
        "Violations",
        "Zoom Selection",
        "Zoom All",
        "Zoom Region",
    ];
    let added = ["Delete", "Fix", "Unfix"];
    let labels = selection_toolbar::labels();
    assert_eq!(labels[..10], java);
    assert_eq!(labels[10..], added);

    // Shown while something is selected, and then every label is a button.
    for label in java.iter().chain(&added) {
        assert_eq!(
            buttons_labelled(&h, label),
            0,
            "no toolbar without a selection: {label}"
        );
    }
    let (pin, _) = lonely_pins(&h)[0];
    exec(&mut h, Command::Select(vec![pin]));
    for label in java.iter().chain(&added) {
        assert_eq!(buttons_labelled(&h, label), 1, "button {label}");
    }
    // And it goes away with the selection.
    click_toolbar(&mut h, "Cancel");
    assert!(h.state().selection().is_empty());
    assert_eq!(buttons_labelled(&h, "Fix"), 0);
}

#[test]
fn toolbar_zoom_and_extend_buttons_work() {
    let mut h = load(&[ISSUE754]);
    let board = board_of(&h);
    let pin = board
        .basic()
        .items()
        .find(|i| i.is_pin() && !i.net_numbers().is_empty())
        .unwrap()
        .id();
    exec(&mut h, Command::Select(vec![pin]));
    let before = camera(&h).zoom;
    click_toolbar(&mut h, "Zoom Selection");
    assert!(camera(&h).zoom > before, "zoomed to one pin");
    click_toolbar(&mut h, "Zoom All");
    assert!((camera(&h).zoom - before).abs() < 1e-9 * before);
    click_toolbar(&mut h, "Nets");
    assert!(selected(&h).len() > 1);
}

/// An unfixed trace of the board (the session's traces come fixed, so the
/// first trace is unfixed here when none is).
fn unfixed_trace(h: &mut Harness<'_, App>) -> ItemId {
    use freerouting::board::structure::FixedState;
    let trace = |h: &Harness<'_, App>| {
        let board = board_of(h);
        board
            .basic()
            .items()
            .find(|i| i.is_trace() && i.fixed_state() == FixedState::Unfixed)
            .map(|i| i.id())
    };
    if let Some(id) = trace(h) {
        return id;
    }
    let first = board_of(h)
        .basic()
        .items()
        .find(|i| i.is_trace())
        .unwrap()
        .id();
    exec(h, Command::Select(vec![first]));
    exec(h, Command::UnfixSelection);
    exec(h, Command::ClearSelection);
    trace(h).expect("an unfixed trace")
}

/// The clearance violations of `a` and of `b`.
fn violations_of(board: &RoutingBoard, a: ItemId, b: ItemId) -> usize {
    board.basic().clearance_violations(a).len() + board.basic().clearance_violations(b).len()
}

/// Selects the two items of the clearance violation whose items report the
/// fewest violations between them, presses V, and returns that count.
fn select_violation_pair_and_press_v(h: &mut Harness<'_, App>) -> usize {
    let loaded = h.state().loaded().unwrap();
    let drc = h.state().analysis().drc(&loaded.session).expect("drc");
    let board = board_of(h);
    let (a, b, count) = drc
        .clearance
        .iter()
        .map(|v| {
            (
                v.first_item,
                v.second_item,
                violations_of(&board, v.first_item, v.second_item),
            )
        })
        .min_by_key(|(_, _, n)| *n)
        .expect("a clearance violation");
    exec(h, Command::Select(vec![a, b]));
    h.key_press(Key::V);
    settle!(h);
    assert_eq!(
        h.state().tools().violations.as_ref().map(Vec::len),
        Some(count),
        "the engine's per-item reports, added up"
    );
    count
}

#[test]
fn violations_of_selection_counts() {
    // Issue015 has one clearance violation (`reference_drc.rs` lists the
    // board). Its two items are selected: the violation is reported by one
    // of them (Java's `Item.clearanceViolations` is not symmetric for
    // every pair of items), so Java's text reads "1 clearance violations
    // found".
    let mut h = load(&["Issue015-StackOverflow.dsn"]);
    assert_eq!(select_violation_pair_and_press_v(&mut h), 1);
    assert_eq!(h.state().notice(), Some("1 clearance violations found"));
    // The markers are drawn, and V again hides them.
    h.key_press(Key::V);
    settle!(h);
    assert!(h.state().tools().violations.is_none());
    // Without a selection V is still the global markers' toggle.
    exec(&mut h, Command::ClearSelection);
    let shown = h.state().overlay().show_violations;
    h.key_press(Key::V);
    settle!(h);
    assert_eq!(h.state().overlay().show_violations, !shown);
}

#[test]
fn violations_seen_from_both_items_count_twice() {
    // Java's `aggregateSortedBySeverity` adds up what each selected item
    // reports, without removing duplicates: on this board every violation
    // is reported by both of its items.
    let mut h = load(&[ISSUE684]);
    let count = select_violation_pair_and_press_v(&mut h);
    assert_eq!(count, 2);
    assert_eq!(h.state().notice(), Some("2 clearance violations found"));
}

#[test]
fn delete_keeps_fixed_items_and_undo_restores() {
    let mut h = load(&J2);
    let trace = unfixed_trace(&mut h);
    let board = board_of(&h);
    let b = board.basic();
    let pin = b
        .items()
        .find(|i| i.is_pin() && i.component_id() > 0)
        .unwrap()
        .id();
    let before = b.item_count();
    // The pin belongs to a component: its deletion is forbidden.
    exec(&mut h, Command::Select(vec![trace, pin]));
    click_toolbar(&mut h, "Delete");
    let after = board_of(&h);
    assert_eq!(after.basic().item_count(), before - 1);
    assert!(after.basic().item(trace).is_none() && after.basic().item(pin).is_some());
    assert_eq!(h.state().notice(), Some(text::SOME_ITEMS_FIXED));
    assert!(
        !h.state().selection().contains(trace),
        "the selection forgot the deleted trace"
    );
    // Patched, not rebuilt.
    assert_eq!(h.state().stats().full_rebuilds, 0);
    assert_eq!(h.state().tools().last_sync.removed, 1);
    // Undo brings the trace back.
    h.key_press(Key::U);
    settle!(h);
    assert_eq!(board_of(&h).basic().item_count(), before);
    // Nothing removable: no snapshot, no edit.
    let generation = h.state().loaded().unwrap().session.edit_generation;
    exec(&mut h, Command::Select(vec![pin]));
    exec(&mut h, Command::DeleteSelection);
    assert_eq!(
        h.state().loaded().unwrap().session.edit_generation,
        generation
    );
    assert_eq!(h.state().notice(), Some(text::SOME_ITEMS_FIXED));
}

#[test]
fn fix_and_unfix_set_the_fixed_state() {
    use freerouting::board::structure::FixedState;
    let mut h = load(&J2);
    let trace = unfixed_trace(&mut h);
    let state = |h: &Harness<'_, App>| board_of(h).basic().item(trace).unwrap().fixed_state();
    assert_eq!(state(&h), FixedState::Unfixed);
    exec(&mut h, Command::Select(vec![trace]));
    click_toolbar(&mut h, "Fix");
    assert_eq!(state(&h), FixedState::UserFixed);
    // A fixed trace can't be deleted.
    exec(&mut h, Command::DeleteSelection);
    assert!(board_of(&h).basic().item(trace).is_some());
    assert_eq!(h.state().notice(), Some(text::SOME_ITEMS_FIXED));
    click_toolbar(&mut h, "Unfix");
    assert_eq!(state(&h), FixedState::Unfixed);
}

#[test]
fn item_info_fields() {
    let mut h = load(&J2);
    let board = board_of(&h);
    let b = board.basic();
    let pin = b
        .items()
        .find(|i| i.is_pin() && i.component_id() > 0 && !i.net_numbers().is_empty())
        .unwrap()
        .id();
    let trace = b.items().find(|i| i.is_trace()).unwrap().id();
    exec(&mut h, Command::Select(vec![pin, trace]));
    click_toolbar(&mut h, "Info");
    let windows = h.state().tools().info.windows();
    assert_eq!(windows.len(), 1);
    let lines: Vec<String> = windows[0]
        .lines
        .iter()
        .map(|l| item_info::plain(l))
        .collect();
    assert_eq!(lines.len(), 2);

    // Items come in the selection's order (highest id first). The fields
    // are the engine's `item_info` (B08.2, compared with Java's `printInfo`
    // by `tests/board_probe_item_info.rs`), in its order.
    let frame = Frame::new(b);
    let mm = |p: freerouting::geometry::FloatPoint| {
        let q = frame.pt(p);
        text::format_point(q.x, q.y)
    };
    let len = |v: f64| text::format_number(f64::from(frame.len(v) as f32));
    let pin_info = b.item_info(pin).unwrap();
    let trace_info = b.item_info(trace).unwrap();
    let mut expect_pin = String::from("Pin: ");
    let mut expect_trace = String::from("Trace");
    use freerouting::board::InfoField as F;
    let tail = |out: &mut String, f: &F| match f {
        F::ClearanceClass { name, .. } => *out += &format!(", clearance class {name}"),
        F::Net { name, .. } => *out += &format!(", Net {name}"),
        F::Contacts(c) => *out += &format!(", contacts {}", c.len()),
        F::ClearanceViolations(v) => {
            *out += &format!(
                ", {} clearance violation{}",
                v.len(),
                if v.len() == 1 { "" } else { "s" }
            );
        }
        F::Fixed(state) => {
            *out += match state {
                freerouting::board::structure::FixedState::UserFixed => ", User fixed",
                freerouting::board::structure::FixedState::ShoveFixed => ", Shove fixed",
                freerouting::board::structure::FixedState::SystemFixed => ", System fixed",
                freerouting::board::structure::FixedState::Unfixed => "",
            };
        }
        other => panic!("unexpected {other:?}"),
    };
    for f in &pin_info.fields {
        match f {
            F::Component { name, .. } => expect_pin += &format!("component {name}"),
            F::PinName(n) => expect_pin += &format!(", pin {n}"),
            F::Padstack { name, .. } => expect_pin += &format!(", padstack {name}"),
            F::Location(p) => expect_pin += &format!(" at {}", mm(*p)),
            other => tail(&mut expect_pin, other),
        }
    }
    for f in &trace_info.fields {
        match f {
            F::From(p) => expect_trace += &format!(" from {}", mm(*p)),
            F::To(p) => expect_trace += &format!(" to {}", mm(*p)),
            F::Layer(n) => expect_trace += &format!(" on layer {n}"),
            F::Width(w) => expect_trace += &format!(", width {}", len(*w)),
            F::Length(l) => expect_trace += &format!(", length {}", len(*l)),
            other => tail(&mut expect_trace, other),
        }
    }
    let mut by_id = [(pin, expect_pin), (trace, expect_trace)];
    by_id.sort_by_key(|(id, _)| *id);
    assert_eq!(lines, [by_id[0].1.clone(), by_id[1].1.clone()]);
    // The summary counts the kinds.
    let summary = item_info::plain(&windows[0].summary);
    assert!(
        summary.starts_with("Summary: 1 pin, 1 trace, cumulative length "),
        "{summary}"
    );
    // The window is on the screen with its title.
    h.get_by_label(text::INFO_TITLE);
}

#[test]
fn list_info_button_opens_the_item_info() {
    let mut h = load(&[ISSUE684]);
    exec(
        &mut h,
        Command::ShowList(freerouting_gui::commands::ListKind::Nets),
    );
    h.get_all_by_label_contains("Net #1 (")
        .next()
        .unwrap()
        .click();
    settle!(h);
    // The nets window hides Info (Java), the other lists have it: the
    // incomplete connections list of this board has rows.
    exec(
        &mut h,
        Command::ShowList(freerouting_gui::commands::ListKind::ClearanceViolations),
    );
    settle!(h);
    let row = h
        .get_all_by_label_contains("A clearance violation")
        .next()
        .expect("a violation row");
    row.click();
    settle!(h);
    h.get_all_by_label("Info")
        .find(|n| {
            n.accesskit_node().role() == eframe::egui::accesskit::Role::Button
                && n.rect().min.y > 100.0
        })
        .expect("the list's Info button")
        .click();
    settle!(h);
    let windows = h.state().tools().info.windows();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].items.len(), 2, "both items of the violation");
}

// ============================================================================
// G05.5 undo and redo with scene patching
// ============================================================================

use freerouting_gui::canvas::chunks::{kind_index, slot_index};
use freerouting_gui::scene::Scene;
use freerouting_gui::view::Tessellation;

type DrawKey = (usize, usize, i32, u32);
type DrawData = (Vec<[f32; 2]>, Vec<u32>, Vec<[f32; 5]>);

/// What the chunks draw, item by item: the geometry of every scene item by
/// (slot, kind, engine id, sequence), whichever chunk holds it.
fn draw_list(tess: &Tessellation, layers: usize) -> std::collections::BTreeMap<DrawKey, DrawData> {
    let mut out = std::collections::BTreeMap::new();
    tess.with(|set| {
        for chunk in set.chunks() {
            for e in &chunk.entries {
                let range = |start: u32, n: u32| start as usize..(start + n) as usize;
                let key = (
                    slot_index(chunk.slot, layers),
                    kind_index(chunk.kind),
                    e.id,
                    e.seq,
                );
                let vertices = chunk.mesh.vertices[range(e.v0, e.vn)].to_vec();
                let indices = chunk.mesh.indices[range(e.i0, e.i_n)]
                    .iter()
                    .map(|i| i - e.v0)
                    .collect();
                let lines = chunk.mesh.lines[range(e.l0, e.ln)].to_vec();
                assert!(
                    out.insert(key, (vertices, indices, lines)).is_none(),
                    "{key:?} twice"
                );
            }
        }
    });
    out
}

/// The scene and the chunks equal what a fresh build of the board gives.
fn assert_matches_fresh_scene(h: &Harness<'_, App>, context: &str) {
    let loaded = h.state().loaded().unwrap();
    let fresh_scene = Scene::from_board(&loaded.session.design.name, &loaded.session.board);
    let fresh = Tessellation::new(&fresh_scene);
    let layers = loaded.scene.layers.len();
    assert_eq!(
        draw_list(&loaded.tess, layers),
        draw_list(&fresh, layers),
        "{context}: the chunk draw list"
    );
    assert_eq!(
        loaded.tess.triangle_count(),
        fresh.triangle_count(),
        "{context}"
    );
    let mut patched = loaded.scene.items.clone();
    let mut built = fresh_scene.items.clone();
    patched.sort_by_key(|i| i.id.0);
    built.sort_by_key(|i| i.id.0);
    assert_eq!(patched, built, "{context}: the scene items");
}

fn obstacles(h: &Harness<'_, App>) -> Vec<ItemId> {
    let board = board_of(h);
    board
        .basic()
        .items()
        .filter(|i| matches!(&i.kind, ItemKind::Area(a) if a.kind == AreaKind::Obstacle))
        .map(|i| i.id())
        .collect()
}

/// Corners of a square of side `side` (mm) with its lower left at `at`.
fn square(at: P, side: f64) -> [P; 4] {
    [
        at,
        P::new(at.x + side, at.y),
        P::new(at.x + side, at.y + side),
        P::new(at.x, at.y + side),
    ]
}

/// The lower-left corner (mm) of the `skip`-th square of side `side` that
/// fits on layer 0 (found on the engine's `check_shape`), nearest the middle
/// of the board first, so that a menu opened there stays on the screen.
fn free_square(h: &Harness<'_, App>, side: f64, skip: usize) -> P {
    let board = board_of(h);
    let b = board.basic();
    let bounds = h.state().loaded().unwrap().scene.bounds;
    let middle = bounds.center();
    let class = freerouting::rules::BoardRules::clearance_class_none();
    let mut fits = Vec::new();
    let mut y = bounds.min.y + 1.0;
    while y + side < bounds.max.y - 1.0 {
        let mut x = bounds.min.x + 1.0;
        while x + side < bounds.max.x - 1.0 {
            let corners = square(P::new(x, y), side).map(|p| board_point(b, p).round());
            let ll = IntPoint {
                x: corners.iter().map(|c| c.x).min().unwrap(),
                y: corners.iter().map(|c| c.y).min().unwrap(),
            };
            let ur = IntPoint {
                x: corners.iter().map(|c| c.x).max().unwrap(),
                y: corners.iter().map(|c| c.y).max().unwrap(),
            };
            let area = Area::from(Shape::from(IntBox::new(ll.x, ll.y, ur.x, ur.y)));
            if b.check_shape(&area, 0, &[], class) {
                fits.push((
                    ((x - middle.x).powi(2) + (y - middle.y).powi(2)),
                    P::new(x, y),
                ));
            }
            x += 0.7;
        }
        y += 0.7;
    }
    fits.sort_by(|a, b| a.0.total_cmp(&b.0));
    fits.get(skip)
        .unwrap_or_else(|| panic!("no free square of {side} mm"))
        .1
}

/// Draws a square keepout through the commands a user's clicks give: start at
/// the first corner, three more corners, complete.
fn draw_keepout(h: &mut Harness<'_, App>, kind: KeepoutKind, at: P, side: f64) {
    let c = square(at, side);
    exec(h, Command::StartKeepout { kind, at: c[0] });
    for p in &c[1..] {
        exec(
            h,
            Command::CanvasClick {
                at: *p,
                toggle: false,
            },
        );
    }
    exec(h, Command::Complete);
}

#[test]
fn undo_at_start_says_nothing_more() {
    let mut h = load(&[ISSUE159]);
    h.key_press(Key::U);
    settle!(h);
    assert_eq!(h.state().notice(), Some("Nothing more to undo."));
    h.key_press(Key::B);
    settle!(h);
    assert_eq!(h.state().notice(), Some("Nothing more to redo."));
}

#[test]
fn random_edit_undo_redo_matches_fresh_scene() {
    let mut h = load(&[ISSUE754]);
    assert_matches_fresh_scene(&h, "as loaded");
    let mut rng = JavaRandom::new(7);
    let (mut inserted, mut deleted, mut undone, mut redone) = (0, 0, 0, 0);
    for step in 0..20 {
        let before = obstacles(&h).len();
        match rng.next_int(5) {
            0 | 1 => {
                let at = free_square(&h, 1.0, rng.next_int(40) as usize);
                draw_keepout(&mut h, KeepoutKind::Polygon, at, 1.0);
                if obstacles(&h).len() == before + 1 {
                    inserted += 1;
                }
            }
            2 => {
                let all = obstacles(&h);
                if !all.is_empty() {
                    let id = all[rng.next_int(all.len() as i32) as usize];
                    exec(&mut h, Command::Select(vec![id]));
                    exec(&mut h, Command::DeleteSelection);
                    exec(&mut h, Command::ClearSelection);
                    deleted += 1;
                }
            }
            3 => {
                exec(&mut h, Command::Undo);
                undone += 1;
            }
            _ => {
                exec(&mut h, Command::Redo);
                redone += 1;
            }
        }
        assert_matches_fresh_scene(&h, &format!("after step {step}"));
    }
    assert!(
        inserted > 0 && deleted + undone + redone > 0,
        "{inserted} {deleted} {undone} {redone}"
    );
    assert_eq!(h.state().stats().full_rebuilds, 0, "every edit was patched");
    assert!(h.state().stats().patches >= 1);
}

#[test]
fn delete_all_and_undo_patch_the_scene() {
    let mut h = load(&J2);
    let wiring = board_of(&h)
        .basic()
        .items()
        .filter(|i| i.is_trace() || i.is_via())
        .count();
    assert!(wiring > 0);
    exec(&mut h, Command::DeleteAll);
    exec(&mut h, Command::ConfirmDeleteAll);
    assert_matches_fresh_scene(&h, "after delete all");
    assert_eq!(
        h.state().tools().last_sync.removed,
        wiring,
        "the journal saw delete-all"
    );
    h.key_press(Key::U);
    settle!(h);
    assert_matches_fresh_scene(&h, "after undo");
    assert_eq!(h.state().tools().last_sync.added, wiring);
    h.key_press(Key::B);
    settle!(h);
    assert_matches_fresh_scene(&h, "after redo");
    assert_eq!(h.state().stats().full_rebuilds, 0);
}

#[test]
fn delete_all_unfills_conduction_areas() {
    use freerouting::board::items::AreaKind;
    let filled = |board: &RoutingBoard| {
        board
            .basic()
            .items()
            .filter_map(|i| i.as_conduction_area())
            .filter(|a| {
                matches!(
                    a.kind,
                    AreaKind::Conduction {
                        is_filled: true,
                        ..
                    }
                )
            })
            .count()
    };
    // A board with copper pours that the loader fills.
    let name = [
        "Issue163-pic_programmer.dsn",
        "Issue027-zMRETestFixture.dsn",
        "Issue103-Board-Routed.dsn",
    ]
    .into_iter()
    .find(|name| {
        let loaded = freerouting_gui::session::load(&common::fixtures().join(name), None).unwrap();
        filled(&loaded.session.board) > 0
    })
    .expect("a fixture with a filled conduction area");
    let mut h = load(&[name]);
    let before = filled(&board_of(&h));
    eprintln!("unfill: {name} has {before} filled conduction areas");
    exec(&mut h, Command::DeleteAll);
    exec(&mut h, Command::ConfirmDeleteAll);
    assert_eq!(
        filled(&board_of(&h)),
        0,
        "Java unfills every conduction area"
    );
    assert!(board_of(&h).basic().rules.ignore_conduction());
    assert_matches_fresh_scene(&h, "after delete all");
    // Undo brings the areas back as they were.
    h.key_press(Key::U);
    settle!(h);
    assert_eq!(filled(&board_of(&h)), before);
}

#[test]
fn undo_is_refused_while_a_keepout_is_drawn() {
    let mut h = load(&[ISSUE159]);
    let at = free_square(&h, 1.0, 0);
    exec(
        &mut h,
        Command::StartKeepout {
            kind: KeepoutKind::Polygon,
            at,
        },
    );
    h.key_press(Key::U);
    settle!(h);
    assert_ne!(h.state().notice(), Some("Nothing more to undo."));
    assert!(h.state().tools().keepout.is_some());
    h.key_press(Key::Escape);
    settle!(h);
    assert!(h.state().tools().keepout.is_none());
    h.key_press(Key::U);
    settle!(h);
    assert_eq!(h.state().notice(), Some("Nothing more to undo."));
}

#[test]
fn undo_with_a_selection_works() {
    // Java refuses history outside a menu state; here the toolbar's Undo is
    // always there (docs/DIVERGENCES.md, "History with a selection").
    let mut h = load(&[ISSUE159]);
    let before = obstacles(&h);
    let at = free_square(&h, 1.0, 0);
    draw_keepout(&mut h, KeepoutKind::Polygon, at, 1.0);
    let new: Vec<ItemId> = obstacles(&h)
        .into_iter()
        .filter(|i| !before.contains(i))
        .collect();
    assert_eq!(new.len(), 1);
    exec(&mut h, Command::Select(new.clone()));
    h.key_press(Key::U);
    settle!(h);
    assert_eq!(obstacles(&h), before);
    assert!(
        h.state().selection().is_empty(),
        "the selection forgot the removed item"
    );
    assert_eq!(h.state().notice(), Some("Undo"));
}

/// FT4 on Issue190: one undo that removes 200 items and adds 200, applied
/// to the scene and the chunks. Release only; the number is printed.
#[test]
fn ft4_undo_of_200_and_200_items() {
    let mut loaded = freerouting_gui::session::load(
        &common::fixtures().join("Issue190-processor.Z80.dsn"),
        None,
    )
    .unwrap();
    let class = freerouting::rules::BoardRules::clearance_class_none();
    let traces: Vec<ItemId> = loaded
        .session
        .board
        .basic()
        .items()
        .filter(|i| i.is_trace())
        .take(200)
        .map(|i| i.id())
        .collect();
    assert_eq!(traces.len(), 200, "Issue190 has traces");
    // One snapshot: 200 traces go (undo adds them), 200 keepouts come (undo
    // removes them).
    loaded.session.edit(|board| {
        board.generate_snapshot();
        let bb = board.basic().bounding_box();
        for (n, id) in traces.iter().enumerate() {
            board.unfix(*id);
            assert!(board.remove_item(*id));
            let x = bb.ll.x + 1000 + (n as i32 % 20) * 40;
            let y = bb.ll.y + 1000 + (n as i32 / 20) * 40;
            let area = Area::from(Shape::from(IntBox::new(x, y, x + 20, y + 20)));
            board.insert_obstacle(
                area,
                0,
                class,
                freerouting::board::structure::FixedState::Unfixed,
            );
        }
        true
    });
    let first = loaded.sync_scene();
    assert!(
        !first.rebuilt && first.removed == 200 && first.added == 200,
        "{first:?}"
    );

    let mut times = Vec::new();
    for round in 0..20 {
        let undo = round % 2 == 0;
        loaded.session.edit(|board| {
            if undo {
                board.undo(None)
            } else {
                board.redo(None)
            }
        });
        let started = Instant::now();
        let sync = loaded.sync_scene();
        times.push(started.elapsed().as_secs_f64() * 1000.0);
        assert!(
            !sync.rebuilt && sync.removed == 200 && sync.added == 200,
            "{sync:?}"
        );
    }
    times.sort_by(|a, b| a.total_cmp(b));
    let median = times[times.len() / 2];
    eprintln!(
        "FT4 sync_scene of 200 removed + 200 added: median {median:.3} ms, max {:.3} ms",
        times[times.len() - 1]
    );
    if !cfg!(debug_assertions) {
        assert!(median <= 2.0, "FT4: {median:.3} ms");
    }
}

// ============================================================================
// G05.6 context menus
// ============================================================================

fn right_click(h: &mut Harness<'_, App>, pos: Pos2) {
    click_with(h, pos, PointerButton::Secondary, Modifiers::NONE);
}

/// The buttons with this label anywhere on the screen.
fn buttons(h: &Harness<'_, App>, label: &str) -> usize {
    h.query_all_by_label(label)
        .filter(|n| n.accesskit_node().role() == eframe::egui::accesskit::Role::Button)
        .count()
}

/// The nodes with this label that are not buttons (a menu's heading).
fn plain_labels(h: &Harness<'_, App>, label: &str) -> usize {
    h.query_all_by_label(label)
        .filter(|n| n.accesskit_node().role() == eframe::egui::accesskit::Role::Label)
        .count()
}

#[test]
fn popup_entries_by_mode() {
    let mut h = load(&[ISSUE159]);
    let board = board_of(&h);
    let swap = board.basic().library.logical_parts.count() > 0;
    let canvas = h.state().view().canvas;
    let pos = canvas.center() + vec2(40.0, 40.0);
    let at = camera(&h).to_board(canvas, pos);

    // Inspect mode, nothing selected: Java's `PopupMenuMain`.
    right_click(&mut h, pos);
    assert!(h.state().tools().popup.is_some());
    let main = popup::labels(popup::Mode::Main { swap }, at);
    assert_eq!(
        main,
        if swap {
            vec![
                "Select item (I)",
                "Start route (T)",
                "Create keepout",
                "Swap pin (W)",
                "Center display (C)",
                "Zoom in (Z)",
                "Zoom out (O)",
            ]
        } else {
            vec![
                "Select item (I)",
                "Start route (T)",
                "Create keepout",
                "Center display (C)",
                "Zoom in (Z)",
                "Zoom out (O)",
            ]
        }
    );
    for label in &main {
        assert_eq!(buttons(&h, label), 1, "{label}");
    }
    // The route entry waits for I01 and says so.
    let route = h.get_by_label("Start route (T)");
    assert!(route.accesskit_node().is_disabled());
    assert_eq!(buttons(&h, "Extend selection to:"), 0);
    h.key_press(Key::Escape);
    settle!(h);
    assert!(h.state().tools().popup.is_none());
    assert_eq!(buttons(&h, "Select item (I)"), 0, "closed");

    // With a selection: the four extend items and the display rows.
    let (pin, _) = lonely_pins(&h)[0];
    exec(&mut h, Command::Select(vec![pin]));
    right_click(&mut h, pos);
    let selection = popup::labels(popup::Mode::Selection, at);
    assert_eq!(
        selection,
        [
            "Extend selection to:",
            "Nets",
            "Connected Sets",
            "Connections",
            "Components",
            "Center display (C)",
            "Zoom in (Z)",
            "Zoom out (O)"
        ]
    );
    assert_eq!(plain_labels(&h, "Extend selection to:"), 1);
    for label in ["Nets", "Connected Sets", "Connections", "Components"] {
        // The popup's item and the toolbar's button of the same name.
        assert_eq!(buttons(&h, label), 2, "{label}");
    }
    for label in ["Center display (C)", "Zoom in (Z)", "Zoom out (O)"] {
        assert_eq!(buttons(&h, label), 1, "{label}");
    }
    assert_eq!(
        buttons(&h, "Select item (I)"),
        0,
        "not in a selection's menu"
    );
}

#[test]
fn popup_display_rows_act_on_the_click_location() {
    let mut h = load(&[ISSUE159]);
    let canvas = h.state().view().canvas;
    let pos = canvas.center() + vec2(120.0, 70.0);
    let before = camera(&h);
    let at = before.to_board(canvas, pos);
    right_click(&mut h, pos);
    h.get_by_label("Zoom in (Z)").click();
    settle!(h);
    let zoomed = camera(&h);
    assert!((zoomed.zoom - 2.0 * before.zoom).abs() < 1e-9 * before.zoom);
    let again = zoomed.to_screen(canvas, at);
    assert!(
        (again - pos).length() < 0.01,
        "the clicked point stays put: {again:?} {pos:?}"
    );
    assert!(h.state().tools().popup.is_none(), "the menu closed");

    right_click(&mut h, pos);
    h.get_by_label("Center display (C)").click();
    settle!(h);
    let centre = camera(&h).center;
    assert!((centre.x - at.x).abs() < 1e-6 && (centre.y - at.y).abs() < 1e-6);
}

#[test]
fn popup_select_item_selects_at_the_click() {
    let mut h = load(&[ISSUE159]);
    let (pin, pos) = lonely_pins(&h)[0];
    right_click(&mut h, pos);
    h.get_by_label("Select item (I)").click();
    settle!(h);
    assert_eq!(selected(&h), BTreeSet::from([pin]));
}

#[test]
fn key_i_selects_at_the_cursor_and_shows_info() {
    let mut h = load(&[ISSUE159]);
    let (pin, pos) = lonely_pins(&h)[0];
    hover(&mut h, pos);
    h.key_press(Key::I);
    settle!(h);
    assert_eq!(selected(&h), BTreeSet::from([pin]));
    // With a selection the same key opens the item info.
    h.key_press(Key::I);
    settle!(h);
    assert_eq!(h.state().tools().info.windows().len(), 1);
}

// ============================================================================
// G05.7 keepout construction
// ============================================================================

#[test]
fn polygon_keepout_inserts_one_obstacle() {
    let mut h = load(&[ISSUE159]);
    let before = obstacles(&h);
    let at = free_square(&h, 2.0, 0);
    let c = square(at, 2.0);
    // Right click on the first corner: Create keepout > Polygon.
    let first = screen(&h, c[0]);
    right_click(&mut h, first);
    h.get_by_label("Create keepout").click();
    settle!(h);
    h.get_by_label("Polygon").click();
    settle!(h);
    assert_eq!(h.state().notice(), Some(text::CREATING_POLYGON));
    for corner in &c[1..] {
        let pos = screen(&h, *corner);
        click(&mut h, pos);
    }
    assert_eq!(
        h.state().tools().keepout.as_ref().unwrap().corners().len(),
        4
    );
    h.key_press(Key::Enter);
    settle!(h);

    let new: Vec<ItemId> = obstacles(&h)
        .into_iter()
        .filter(|i| !before.contains(i))
        .collect();
    assert_eq!(new.len(), 1, "exactly one obstacle was added");
    assert_eq!(h.state().notice(), Some(text::KEEPOUT_DONE));
    assert!(h.state().tools().keepout.is_none());
    // It is a square of about 2 mm where the corners were clicked.
    let board = board_of(&h);
    let item = board.basic().item(new[0]).unwrap();
    let frame = Frame::new(board.basic());
    let bb = item.bounding_box();
    let side = frame.len(f64::from(bb.ur.x) - f64::from(bb.ll.x));
    assert!((side - 2.0).abs() < 0.3, "side {side} mm");
    // Patched into the drawing, not rebuilt.
    let sync = &h.state().tools().last_sync;
    assert!(
        !sync.rebuilt && sync.added == 1 && sync.removed == 0,
        "{sync:?}"
    );
    assert_matches_fresh_scene(&h, "after the keepout");
    // U removes it again, patched too.
    h.key_press(Key::U);
    settle!(h);
    assert_eq!(obstacles(&h), before);
    assert_eq!(h.state().stats().full_rebuilds, 0);
    assert_matches_fresh_scene(&h, "after undo");
}

#[test]
fn overlapping_keepout_is_refused() {
    let mut h = load(&[ISSUE159]);
    let before = obstacles(&h);
    let (pin, _) = lonely_pins(&h)[0];
    let board = board_of(&h);
    let b = board.basic().item(pin).unwrap().bounding_box();
    let centre = Frame::new(board.basic()).pt(freerouting::geometry::FloatPoint::new(
        (f64::from(b.ll.x) + f64::from(b.ur.x)) / 2.0,
        (f64::from(b.ll.y) + f64::from(b.ur.y)) / 2.0,
    ));
    let c = square(P::new(centre.x - 0.6, centre.y - 0.6), 1.2);
    let first = screen(&h, c[0]);
    right_click(&mut h, first);
    h.get_by_label("Create keepout").click();
    settle!(h);
    h.get_by_label("Polygon").click();
    settle!(h);
    for corner in &c[1..] {
        let pos = screen(&h, *corner);
        click(&mut h, pos);
    }
    h.key_press(Key::Enter);
    settle!(h);
    assert_eq!(obstacles(&h), before, "nothing was added");
    assert_eq!(
        h.state().notice(),
        Some("Keepout canceled because it overlaps existing items.")
    );
    assert!(h.state().tools().keepout.is_none());
    // No snapshot was taken either.
    h.key_press(Key::U);
    settle!(h);
    assert_eq!(h.state().notice(), Some("Nothing more to undo."));
}

#[test]
fn tile_and_circle_keepouts() {
    let mut h = load(&[ISSUE159]);
    let before = obstacles(&h).len();
    let at = free_square(&h, 2.0, 0);
    draw_keepout(&mut h, KeepoutKind::Tile, at, 1.5);
    assert_eq!(obstacles(&h).len(), before + 1, "a tile");
    let corner = free_square(&h, 2.0, 1);
    let at = P::new(corner.x + 1.0, corner.y + 1.0);
    exec(
        &mut h,
        Command::StartKeepout {
            kind: KeepoutKind::Circle,
            at,
        },
    );
    exec(
        &mut h,
        Command::CanvasClick {
            at: P::new(at.x + 0.8, at.y),
            toggle: false,
        },
    );
    assert_eq!(obstacles(&h).len(), before + 2, "a circle");
    assert_eq!(h.state().notice(), Some(text::KEEPOUT_DONE));
    assert_matches_fresh_scene(&h, "after tile and circle");
    // A circle with no radius is refused.
    let at = free_square(&h, 2.0, 2);
    exec(
        &mut h,
        Command::StartKeepout {
            kind: KeepoutKind::Circle,
            at,
        },
    );
    exec(&mut h, Command::CanvasClick { at, toggle: false });
    assert_eq!(obstacles(&h).len(), before + 2);
    assert_eq!(h.state().notice(), Some(text::KEEPOUT_OVERLAPS));
    // A hole needs a keepout under its start (I03.4's editor command).
    let empty = free_square(&h, 2.0, 3);
    exec(
        &mut h,
        Command::StartKeepout {
            kind: KeepoutKind::Hole,
            at: empty,
        },
    );
    assert!(h.state().tools().keepout.is_none());
    assert_eq!(h.state().notice(), Some(text::NO_ITEM_FOR_HOLE));
    // A hole in a tile: the tile is removed and reinserted with the hole.
    let tile = free_square(&h, 3.0, 4);
    draw_keepout(&mut h, KeepoutKind::Tile, tile, 3.0);
    let with_tile = obstacles(&h).len();
    let hole = square(P::new(tile.x + 1.0, tile.y + 1.0), 1.0);
    exec(
        &mut h,
        Command::StartKeepout {
            kind: KeepoutKind::Hole,
            at: hole[0],
        },
    );
    assert_eq!(h.state().notice(), Some(text::ADDING_HOLE));
    for p in &hole[1..] {
        exec(
            &mut h,
            Command::CanvasClick {
                at: *p,
                toggle: false,
            },
        );
    }
    exec(&mut h, Command::Complete);
    assert_eq!(h.state().notice(), Some(text::HOLE_DONE));
    assert_eq!(obstacles(&h).len(), with_tile, "removed and reinserted");
}

#[test]
fn the_rubber_band_follows_the_cursor() {
    use freerouting_gui::overlays::construction;
    let mut h = load(&[ISSUE159]);
    let at = free_square(&h, 2.0, 0);
    let c = square(at, 2.0);
    exec(
        &mut h,
        Command::StartKeepout {
            kind: KeepoutKind::Polygon,
            at: c[0],
        },
    );
    exec(
        &mut h,
        Command::CanvasClick {
            at: c[1],
            toggle: false,
        },
    );
    let cursor = screen(&h, c[2]);
    hover(&mut h, cursor);
    settle!(h);
    let board = board_of(&h);
    let tool = h.state().tools().keepout.as_ref().unwrap();
    let mm = camera(&h).to_board(h.state().view().canvas, cursor);
    let band = construction::band(tool, &board, construction::cursor_point(&board, Some(mm)));
    // Two corners and the cursor, snapped to the angle of the last line.
    assert_eq!(band.points.len(), 3);
    assert_eq!(&band.points[..2], tool.corners());
    assert!(
        h.state().stats().overlay.tools >= 3,
        "{:?}",
        h.state().stats().overlay
    );
}

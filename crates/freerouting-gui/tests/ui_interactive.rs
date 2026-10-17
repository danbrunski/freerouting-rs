//! G09: route mode, the interactive settings, drag, pin swap, the route
//! overlay, move and rotate. The tests drive the app with raw pointer events
//! where the item says so; every expected value comes from an engine call on
//! a copy of the board (the same commands on an `Editor` that owns its board,
//! `BoardStatistics`, the board's own queries).

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use common::{fixtures, plain_harness};
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use freerouting::board::items::{ItemId, StopConnectionOption};
use freerouting::board::structure::FixedState;
use freerouting::board::{BoardChanges, RoutingBoard};
use freerouting::geometry::{FloatPoint, IntBox};
use freerouting::interactive::{
    Command as Ed, DragSettings, EditSettings, Editor, EditorOptions, ModeKind, Tool,
};
use freerouting::scoring::BoardStatistics;
use freerouting_gui::App;
use freerouting_gui::commands::Command;
use freerouting_gui::overlays::{Frame, route as route_overlay};
use freerouting_gui::scene::P;
use freerouting_gui::settings::{InteractiveSettings, PersistedSettings};
use freerouting_gui::tools::ToolMode;
use freerouting_gui::tools::inspect::board_point;

const J2: &str = "Issue026-J2_reference.dsn";
const J2_SES: &str = "Issue026-J2_reference.ses";

// --- helpers --------------------------------------------------------------

fn exec(h: &mut Harness<'_, App>, command: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, command);
    let _ = h.run_ok();
}

fn board_of(h: &Harness<'_, App>) -> Arc<RoutingBoard> {
    h.state().loaded().unwrap().session.board.clone()
}

fn camera(h: &Harness<'_, App>) -> freerouting_gui::view::Camera {
    h.state().view().camera.expect("camera")
}

fn push(h: &mut Harness<'_, App>, event: Event) {
    h.input_mut().events.push(event);
    h.step();
}

fn hover(h: &mut Harness<'_, App>, pos: Pos2) {
    push(h, Event::PointerMoved(pos));
}

fn button(h: &mut Harness<'_, App>, pos: Pos2, pressed: bool) {
    push(
        h,
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        },
    );
}

fn click(h: &mut Harness<'_, App>, pos: Pos2) {
    hover(h, pos);
    button(h, pos, true);
    button(h, pos, false);
    let _ = h.run_ok();
}

/// [`hover`] over the board point `p`.
fn hover_pt(h: &mut Harness<'_, App>, p: FloatPoint) {
    let pos = screen(h, p);
    hover(h, pos);
}

/// [`click`] on the board point `p`.
fn click_pt(h: &mut Harness<'_, App>, p: FloatPoint) {
    let pos = screen(h, p);
    click(h, pos);
}

/// Press at `from`, move to `to`, release.
fn drag(h: &mut Harness<'_, App>, from: Pos2, to: Pos2) {
    hover(h, from);
    button(h, from, true);
    hover(h, to);
    button(h, to, false);
    let _ = h.run_ok();
}

/// Where the board point `p` is on the screen.
fn screen(h: &Harness<'_, App>, p: FloatPoint) -> Pos2 {
    let board = board_of(h);
    let mm = Frame::new(board.basic()).pt(p);
    camera(h).to_screen(h.state().view().canvas, mm)
}

/// The board point under a screen point, as the tools compute it.
fn board_under(h: &Harness<'_, App>, pos: Pos2) -> FloatPoint {
    let mm: P = camera(h).to_board(h.state().view().canvas, pos);
    board_point(board_of(h).basic(), mm)
}

fn j2() -> Harness<'static, App> {
    plain_harness(&[J2])
}

/// The screen position of `p` and the board point the tools read from it.
fn snapped(h: &Harness<'_, App>, p: FloatPoint) -> FloatPoint {
    board_under(h, screen(h, p))
}

/// A pair of pins of one net (A, B) that a dynamic route from A reaches with
/// two pointer moves (the middle, then B): found by running the same commands
/// on an `Editor` that owns a copy of the board, with the pointer positions
/// the screen gives.
fn start_and_target(h: &Harness<'_, App>) -> (FloatPoint, FloatPoint, FloatPoint) {
    let board = board_of(h);
    let settings = EditSettings::for_board(&board);
    let before = incomplete(&board);
    let pins: Vec<(i32, FloatPoint)> = board
        .items()
        .filter(|i| i.is_pin() && i.net_count() == 1)
        .map(|i| {
            (
                i.net_numbers()[0],
                i.drill().expect("pin").center.to_float(),
            )
        })
        .collect();
    let mut tries = 0;
    for &(net, a) in &pins {
        let mut others: Vec<FloatPoint> = pins
            .iter()
            .filter(|(n, p)| *n == net && *p != a)
            .map(|(_, p)| *p)
            .collect();
        others.sort_by(|p, q| a.distance(*p).total_cmp(&a.distance(*q)));
        for b in others.into_iter().take(1) {
            for fraction in [0.5, 0.3, 0.7, 0.15] {
                tries += 1;
                assert!(tries < 200, "no pin pair that a route reaches");
                let mid =
                    FloatPoint::new(a.x + (b.x - a.x) * fraction, a.y + (b.y - a.y) * fraction);
                let mut editor = Editor::new((*board).clone(), settings.clone());
                editor.apply(Ed::Click(snapped(h, a)));
                // the pointer is still on the pin when the route starts
                editor.apply(Ed::Move(snapped(h, a)));
                editor.apply(Ed::Move(snapped(h, mid)));
                let traced = !editor.board().traces().is_empty()
                    && matches!(editor.mode(), ModeKind::Routing { .. });
                let response = editor.apply(Ed::Move(snapped(h, b)));
                if traced
                    && !matches!(response.mode, ModeKind::Routing { .. })
                    && incomplete(editor.board()) + 1 == before
                {
                    return (a, mid, b);
                }
            }
        }
    }
    panic!("no pin pair that a route reaches");
}

fn pin_at(board: &RoutingBoard, at: FloatPoint) -> ItemId {
    board
        .items()
        .find(|i| i.is_pin() && i.drill().is_some_and(|d| d.center.to_float() == at))
        .expect("pin")
        .id()
}

/// Every item with its bounding box: the board's content for comparisons.
fn content(board: &RoutingBoard) -> BTreeSet<(i32, [i32; 4])> {
    board
        .items()
        .map(|i| {
            let b: IntBox = i.bounding_box();
            (i.id().0, [b.ll.x, b.ll.y, b.ur.x, b.ur.y])
        })
        .collect()
}

fn incomplete(board: &RoutingBoard) -> usize {
    BoardStatistics::new(board).connections.incomplete_count
}

fn start_route(h: &mut Harness<'_, App>, a: FloatPoint) {
    click_pt(h, a);
    assert!(
        matches!(h.state().editor_mode(), Some(ModeKind::Routing { .. })),
        "a click on the pin starts a route, mode {:?}",
        h.state().editor_mode()
    );
}

// --- G09.1 route mode -------------------------------------------------------

#[test]
fn the_app_starts_in_route_mode_like_java() {
    let h = j2();
    assert_eq!(h.state().tools().mode, ToolMode::Route);
}

#[test]
fn route_pin_to_pin_two_layer() {
    let mut h = j2();
    let board = board_of(&h);
    assert_eq!(board.layer_count(), 2);
    let (a, mid, b) = start_and_target(&h);
    let (pin_a, pin_b) = (pin_at(&board, a), pin_at(&board, b));
    let before = incomplete(&board);
    assert!(before > 0);

    start_route(&mut h, a);
    assert_eq!(
        h.state().notice().map(|n| n.starts_with("Routing net ")),
        Some(true)
    );
    hover_pt(&mut h, mid);
    hover_pt(&mut h, b);
    if h.state().editor_busy() {
        h.key_press(Key::Enter);
        let _ = h.run_ok();
    }
    assert!(!h.state().editor_busy(), "the route is over");

    let after = board_of(&h);
    let items = after
        .basic()
        .connection_items(pin_a, StopConnectionOption::None);
    assert!(!items.is_empty(), "A has a trace");
    assert!(
        items
            .iter()
            .any(|i| after.basic().normal_contacts(*i).contains(&pin_b)),
        "A's connection items reach B"
    );
    assert_eq!(
        incomplete(&after),
        before - 1,
        "one connection fewer to route"
    );
}

#[test]
fn escape_cancels_route() {
    let mut h = j2();
    let board = board_of(&h);
    let (a, mid, _) = start_and_target(&h);
    let stats = |b: &RoutingBoard| {
        let s = BoardStatistics::new(b);
        (
            s.items.total_count,
            s.items.trace_count,
            s.items.via_count,
            s.connections.incomplete_count,
        )
    };
    let (before, items) = (stats(&board), content(&board));
    start_route(&mut h, a);
    hover_pt(&mut h, mid);
    assert_ne!(
        content(&board_of(&h)),
        items,
        "the route put a trace on the board"
    );
    h.key_press(Key::Escape);
    let _ = h.run_ok();
    assert!(!h.state().editor_busy());
    let after = board_of(&h);
    assert_eq!(stats(&after), before);
    assert_eq!(content(&after), items);
}

#[test]
fn route_popup_cancels_and_changes_layer() {
    use egui_kittest::kittest::Queryable;
    let mut h = j2();
    let board = board_of(&h);
    let items = content(&board);
    let (a, mid, _) = start_and_target(&h);
    start_route(&mut h, a);
    hover_pt(&mut h, mid);
    let at = screen(&h, mid);
    // right click: End route, Cancel route and Change layer (Java's route popup)
    hover(&mut h, at);
    push(
        &mut h,
        Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    );
    push(
        &mut h,
        Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    );
    let _ = h.run_ok();
    let popup = h.state().tools().popup.as_ref().expect("the route popup");
    assert_eq!(
        popup.layers.iter().map(|l| (l.0, l.2)).collect::<Vec<_>>(),
        [(0, false), (1, true)],
        "the current layer is disabled"
    );
    h.get_by_label("Change layer").click();
    let _ = h.run_ok();
    // the layers panel has a "B.Cu" too; the popup's entry comes last
    h.query_all_by_label("B.Cu").last().expect("B.Cu").click();
    let _ = h.run_ok();
    assert_eq!(
        h.state().view().active_layer,
        1,
        "the layer changed with a via"
    );
    assert_eq!(board_of(&h).vias().len(), 1);
    // Cancel route removes the tail only: the layer change took a snapshot, so
    // the trace before the via and the via stay (Java `RouteState.cancel`)
    let at = screen(&h, mid);
    hover(&mut h, at);
    push(
        &mut h,
        Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    );
    push(
        &mut h,
        Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    );
    let _ = h.run_ok();
    h.get_by_label("Cancel route").click();
    let _ = h.run_ok();
    assert!(!h.state().editor_busy());
    assert_eq!(board_of(&h).vias().len(), 1, "the via stays");
    assert_ne!(content(&board_of(&h)), items);
}

#[test]
fn layer_key_mid_route_adds_via() {
    let mut h = j2();
    let board = board_of(&h);
    let (a, mid, _) = start_and_target(&h);
    start_route(&mut h, a);
    hover_pt(&mut h, mid);
    let vias = board_of(&h).vias().len();
    let name = board.layer_structure.layers[1].name.clone();
    h.key_press(Key::Num2);
    let _ = h.run_ok();
    assert_eq!(board_of(&h).vias().len(), vias + 1, "one via");
    assert_eq!(
        h.state().notice(),
        Some(format!("Layer changed to {name}.").as_str())
    );
    assert_eq!(h.state().view().active_layer, 1);
}

// --- G09.2 interactive settings --------------------------------------------

#[derive(Default)]
struct MemStorage(std::collections::HashMap<String, String>);

impl eframe::Storage for MemStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
    fn flush(&mut self) {}
}

fn interactive(h: &Harness<'_, App>) -> InteractiveSettings {
    h.state().settings_snapshot().interactive
}

fn set_interactive(h: &mut Harness<'_, App>, f: impl FnOnce(&mut InteractiveSettings)) {
    let mut settings = interactive(h);
    f(&mut settings);
    exec(h, Command::UpdateInteractive(settings));
}

#[test]
fn interactive_settings_round_trip() {
    use egui_kittest::kittest::Queryable;
    let mut h = j2();
    assert_eq!(interactive(&h), InteractiveSettings::default());
    exec(
        &mut h,
        Command::ShowRulesDialog(freerouting_gui::dialogs::RulesDialog::RouteSettings),
    );
    // settings -> dialog -> settings: the dialog changes nothing by itself
    set_interactive(&mut h, |s| {
        s.stitch_route = true;
        s.highlight_routing_obstacle = true;
        s.pull_tight_accuracy = 250;
    });
    let _ = h.run_ok();
    let expected = InteractiveSettings {
        stitch_route: true,
        highlight_routing_obstacle: true,
        pull_tight_accuracy: 250,
        ..InteractiveSettings::default()
    };
    assert_eq!(interactive(&h), expected);
    // a click in the dialog changes the one field it belongs to
    h.get_by_label("Push-and-shove enabled").click();
    let _ = h.run_ok();
    assert!(!interactive(&h).push_enabled);
    h.get_by_label("Dynamic").click();
    let _ = h.run_ok();
    assert!(!interactive(&h).stitch_route);
    h.get_by_label("Push-and-shove enabled").click();
    h.get_by_label("Stitching").click();
    let _ = h.run_ok();
    assert_eq!(interactive(&h), expected);

    // save and restore
    let mut storage = MemStorage::default();
    h.state().settings_snapshot().save(&mut storage);
    let restored = PersistedSettings::restore(Some(&storage));
    assert_eq!(restored.interactive, expected);
    // the engine gets the same values
    let mut engine = EditSettings::new(2);
    restored.interactive.apply_to(&mut engine);
    assert!(engine.stitch_route && engine.highlight_routing_obstacle);
    assert_eq!(engine.pull_tight_accuracy, 250);
    assert_eq!(
        engine.trace_tidy_width,
        i32::MAX,
        "the whole board by default"
    );
}

#[test]
fn stitch_mode_does_not_follow_cursor() {
    let traces = |h: &Harness<'_, App>| board_of(h).traces().len();
    // stitch: moving adds no segment, a click adds a corner
    let mut h = j2();
    let (a, mid, b) = start_and_target(&h);
    set_interactive(&mut h, |s| s.stitch_route = true);
    start_route(&mut h, a);
    assert_eq!(
        h.state().editor_mode(),
        Some(ModeKind::Routing { stitch: true })
    );
    for p in [mid, b, mid] {
        hover_pt(&mut h, p);
    }
    let session = h.state().editor().unwrap().route_session().unwrap();
    assert_eq!(session.corners_added(), 0);
    assert_eq!(traces(&h), 0, "no segment without a click");
    click_pt(&mut h, mid);
    let session = h.state().editor().unwrap().route_session().unwrap();
    assert_eq!(session.corners_added(), 1);
    assert!(traces(&h) > 0, "a click adds a corner");

    // dynamic: the same moves put a trace on the board
    let mut h = j2();
    start_route(&mut h, a);
    assert_eq!(
        h.state().editor_mode(),
        Some(ModeKind::Routing { stitch: false })
    );
    hover_pt(&mut h, mid);
    assert!(traces(&h) > 0, "dynamic mode follows the cursor");
}

#[test]
fn p_toggles_push_and_shove() {
    let mut h = j2();
    assert!(interactive(&h).push_enabled);
    h.key_press(Key::P);
    let _ = h.run_ok();
    assert!(!interactive(&h).push_enabled);
    h.key_press(Key::P);
    let _ = h.run_ok();
    assert!(interactive(&h).push_enabled);
    // the setting reaches the engine with the next command
    let (a, _, _) = start_and_target(&h);
    h.key_press(Key::P);
    let _ = h.run_ok();
    start_route(&mut h, a);
    assert!(!h.state().editor().unwrap().settings().push_enabled);
    // Java toggles in a menu state only: during a route P does nothing
    h.key_press(Key::P);
    let _ = h.run_ok();
    assert!(!interactive(&h).push_enabled);
}

// --- G09.3 drag mode ----------------------------------------------------------

/// Unfixes every trace and via through the GUI's own command.
fn unfix_wiring(h: &mut Harness<'_, App>) {
    let ids: Vec<ItemId> = board_of(h)
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.fixed_state() != FixedState::Unfixed)
        .map(|i| i.id())
        .collect();
    if !ids.is_empty() {
        exec(h, Command::Select(ids));
        exec(h, Command::UnfixSelection);
        exec(h, Command::ClearSelection);
    }
}

/// The engine's drag of a copy: press at `from`, move to `to`, release, with
/// the pointer positions the screen gives.
fn engine_drag(board: &RoutingBoard, from: FloatPoint, to: FloatPoint) -> (Editor, ModeKind) {
    let mut editor = Editor::new(board.clone(), EditSettings::for_board(board));
    editor.apply(Ed::SetTool(Tool::Drag));
    let started = editor.apply(Ed::Press(from)).mode;
    editor.apply(Ed::DragTo(to));
    editor.apply(Ed::Release);
    (editor, started)
}

#[test]
fn drag_via_moves_attached_traces() {
    // The checklist names Issue754, but the vias of its DSN wiring are
    // `SystemFixed`, which `unfix` leaves alone and Java does not drag
    // either (`DragStep::Fixed`); J2 with its session has routed vias that
    // move.
    let mut h = plain_harness(&[J2, J2_SES]);
    unfix_wiring(&mut h);
    exec(&mut h, Command::SetTool(ToolMode::Drag));
    let board = board_of(&h);
    let before = content(&board);
    let incomplete_before = incomplete(&board);
    // a via with a trace that the engine drags by a few pixels
    let canvas = h.state().view().canvas;
    let mut found = None;
    for via in board.items().filter(|i| i.is_via()) {
        let center = via.drill().unwrap().center.to_float();
        let from_pos = screen(&h, center);
        if !canvas.shrink(30.0).contains(from_pos)
            || board.basic().normal_contacts(via.id()).is_empty()
        {
            continue;
        }
        for delta in [3.0f32, 6.0, 10.0, 16.0].into_iter().flat_map(|d| {
            [
                eframe::egui::vec2(d, 0.0),
                eframe::egui::vec2(0.0, -d),
                eframe::egui::vec2(-d, 0.0),
                eframe::egui::vec2(0.0, d),
            ]
        }) {
            let to_pos = from_pos + delta;
            let (from, to) = (board_under(&h, from_pos), board_under(&h, to_pos));
            let (editor, started) = engine_drag(&board, from, to);
            let moved = editor
                .board()
                .item(via.id())
                .and_then(|i| i.drill().map(|d| d.center));
            if started == ModeKind::Dragging
                && moved.is_some_and(|c| c != via.drill().unwrap().center)
                && incomplete(editor.board()) == incomplete_before
            {
                found = Some((
                    via.id(),
                    from_pos,
                    to_pos,
                    moved.unwrap(),
                    content(editor.board()),
                ));
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    let (via, from_pos, to_pos, expected_center, expected_content) =
        found.expect("a via the engine can drag");
    let rebuilds = h.state().stats().full_rebuilds;

    drag(&mut h, from_pos, to_pos);

    let after = board_of(&h);
    assert_eq!(
        after.item(via).unwrap().drill().unwrap().center,
        expected_center,
        "the via moved as the engine moves it"
    );
    assert_eq!(
        content(&after),
        expected_content,
        "and so did everything it pushed"
    );
    assert_eq!(
        incomplete(&after),
        incomplete_before,
        "the traces stay connected"
    );
    assert!(!after.basic().normal_contacts(via).is_empty());
    assert_eq!(
        h.state().stats().full_rebuilds,
        rebuilds,
        "patched, not rebuilt"
    );
    assert!(!h.state().editor_busy());

    // U restores the original position exactly
    h.key_press(Key::U);
    let _ = h.run_ok();
    assert_eq!(content(&board_of(&h)), before);
}

#[test]
fn drag_empty_space_makes_space() {
    let mut h = plain_harness(&[J2, J2_SES]);
    unfix_wiring(&mut h);
    exec(&mut h, Command::SetTool(ToolMode::Drag));
    let board = board_of(&h);
    let before = content(&board);
    let half = f64::from(board.min_trace_half_width());
    let mut found = None;
    'search: for trace in board.items().filter_map(|i| i.as_trace()).take(40) {
        let middle = trace
            .polyline
            .corner_approx(0)
            .middle_point(trace.polyline.corner_approx(1));
        for (dx, dy) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
            let at = FloatPoint::new(middle.x + dx * 4.0 * half, middle.y + dy * 4.0 * half);
            let end = FloatPoint::new(middle.x - dx * 4.0 * half, middle.y - dy * 4.0 * half);
            let (from_pos, to_pos) = (screen(&h, at), screen(&h, end));
            let (from, to) = (board_under(&h, from_pos), board_under(&h, to_pos));
            let (editor, started) = engine_drag(&board, from, to);
            let after = content(editor.board());
            if started == ModeKind::MakingSpace
                && after != before
                && editor.board().items().count() == board.items().count()
            {
                found = Some((from_pos, to_pos, after));
                break 'search;
            }
        }
    }
    let (from_pos, to_pos, expected) = found.expect("a make-space drag that pushes a trace");
    drag(&mut h, from_pos, to_pos);
    let after = board_of(&h);
    assert_ne!(content(&after), before, "a trace was pushed");
    assert_eq!(content(&after), expected, "as the engine pushes it");
    assert_eq!(
        after.items().count(),
        board.items().count(),
        "nothing was inserted"
    );
    assert!(
        after.items().all(|i| !i
            .net_numbers()
            .contains(&freerouting::rules::Nets::HIDDEN_NET_NUMBER)),
        "the hidden net is gone"
    );
}

#[test]
fn drag_component_needs_setting() {
    let mut h = plain_harness(&[J2]);
    exec(&mut h, Command::SetTool(ToolMode::Drag));
    let board = board_of(&h);
    let before = content(&board);
    let settings = EditSettings::for_board(&board);
    let on = DragSettings::default();
    let off = DragSettings {
        drag_components_enabled: false,
        ..DragSettings::default()
    };
    // a pad of a placed component that a press drags only as a component
    let pad = board
        .items()
        .filter(|i| i.is_pin() && i.component_id() > 0)
        .map(|i| screen(&h, i.drill().unwrap().center.to_float()))
        .find(|pos| {
            let at = board_under(&h, *pos);
            let picks = |d: &DragSettings| {
                freerouting::interactive::find_item_to_move(&board, &settings, d, at)
            };
            picks(&on).0.is_some() && picks(&off) == (None, true)
        })
        .expect("a pad that only a component drag picks");
    let to = pad + eframe::egui::vec2(12.0, 8.0);

    set_interactive(&mut h, |s| s.drag_components_enabled = false);
    drag(&mut h, pad, to);
    assert_eq!(content(&board_of(&h)), before, "nothing dragged");
    assert!(!h.state().editor_busy());

    set_interactive(&mut h, |s| s.drag_components_enabled = true);
    drag(&mut h, pad, to);
    assert_ne!(content(&board_of(&h)), before, "the component moved");
    let _ = EditorOptions::default();
}

// --- G09.4 pin swap -----------------------------------------------------------

const PART_LIBRARY: &str = "  (part_library\n    (logical_part_mapping \"USB-MINIB-SWAP\" (component \"J3\"))\n    (logical_part \"USB-MINIB-SWAP\"\n      (pin \"D+\" 1 \"A\" 0 \"1\" 1)\n      (pin \"D-\" 2 \"A\" 0 \"2\" 1)\n      (pin \"ID\" 3 \"A\" 0 \"3\" 1)\n      (pin \"VBUS\" 4 \"A\" 0 \"4\" 1)\n    )\n  )\n";

/// rpi_splitter with a logical part for J3 (the engine's pin swap board),
/// written next to the test binary's temporary files.
fn pin_swap_design(tag: &str) -> std::path::PathBuf {
    let text = std::fs::read_to_string(fixtures().join("Issue143-rpi_splitter.dsn")).unwrap();
    let at = text.find("  (network").expect("network scope");
    let patched = format!("{}{}{}", &text[..at], PART_LIBRARY, &text[at..]);
    let path = std::env::temp_dir().join(format!("g09_pin_swap_{tag}_{}.dsn", std::process::id()));
    std::fs::write(&path, patched).unwrap();
    path
}

fn pins_of(board: &RoutingBoard, component: &str) -> Vec<ItemId> {
    board
        .items()
        .filter(|i| i.is_pin() && i.component_id() > 0)
        .filter(|i| {
            board
                .components
                .get(freerouting::board::structure::ComponentId(i.component_id()))
                .is_some_and(|c| c.name == component)
        })
        .map(|i| i.id())
        .collect()
}

fn pin_pos(h: &Harness<'_, App>, id: ItemId) -> Pos2 {
    let board = board_of(h);
    screen(
        h,
        board.item(id).unwrap().drill().unwrap().center.to_float(),
    )
}

fn pin_nets(board: &RoutingBoard, id: ItemId) -> Vec<i32> {
    board.item(id).unwrap().net_numbers().to_vec()
}

#[test]
fn pin_swap_completes() {
    let design = pin_swap_design("completes");
    let mut h = Harness::builder()
        .with_size(common::SIZE)
        .build_eframe(|cc| App::new(cc, vec![design.clone()]));
    common::wait_for_load(&mut h);
    let board = board_of(&h);
    // two swappable pins of J3 with different nets, none connected
    let (from, to) = pins_of(&board, "J3")
        .into_iter()
        .find_map(|p| {
            board
                .swappable_pins(p)
                .into_iter()
                .find(|q| {
                    let (a, b) = (pin_nets(&board, p), pin_nets(&board, *q));
                    a != b && !a.is_empty() && !b.is_empty()
                })
                .map(|q| (p, q))
        })
        .expect("two swappable pins");
    let (nets_from, nets_to) = (pin_nets(&board, from), pin_nets(&board, to));
    let (pf, pt) = (pin_pos(&h, from), pin_pos(&h, to));
    hover(&mut h, pf);
    h.key_press(Key::W);
    let _ = h.run_ok();
    assert_eq!(h.state().editor_mode(), Some(ModeKind::PinSwap));
    assert_eq!(
        h.state().notice(),
        Some("Click the second pin with the left mouse button.")
    );
    assert!(
        h.state().editor_overlay().unwrap().swap.is_some(),
        "the swap candidates are shown"
    );
    click(&mut h, pt);
    assert_eq!(h.state().notice(), Some("Pin swap completed."));
    let after = board_of(&h);
    assert_eq!(pin_nets(&after, from), nets_to);
    assert_eq!(pin_nets(&after, to), nets_from);
    assert!(!h.state().editor_busy());
    let _ = std::fs::remove_file(design);
}

#[test]
fn pin_swap_refused_when_connected() {
    let design = pin_swap_design("refused");
    let ses = "reference/route/Issue143-rpi_splitter.ses";
    let mut h = Harness::builder()
        .with_size(common::SIZE)
        .build_eframe(|cc| App::new(cc, vec![design.clone(), fixtures().join(ses)]));
    common::wait_for_load(&mut h);
    let board = board_of(&h);
    let settings = EditSettings::for_board(&board);
    // a J3 pin with a trace, and a pin it could swap with
    let (from, to) = pins_of(&board, "J3")
        .into_iter()
        .find_map(|p| {
            let connected = !board.basic().normal_contacts(p).is_empty();
            let q = board.swappable_pins(p).into_iter().next()?;
            (connected
                && freerouting::interactive::PinSwap::start(
                    board.basic(),
                    &settings,
                    board.item(p).unwrap().drill().unwrap().center.to_float(),
                )
                .is_ok())
            .then_some((p, q))
        })
        .expect("a connected pin with a swappable pin");
    let nets_before = pin_nets(&board, from);
    let (pf, pt) = (pin_pos(&h, from), pin_pos(&h, to));
    hover(&mut h, pf);
    h.key_press(Key::W);
    let _ = h.run_ok();
    assert_eq!(h.state().editor_mode(), Some(ModeKind::PinSwap));
    click(&mut h, pt);
    assert_eq!(
        h.state().notice(),
        Some("Pin not swapped because it is already connected.")
    );
    assert_eq!(pin_nets(&board_of(&h), from), nets_before);
    let _ = std::fs::remove_file(design);
}

#[test]
fn pin_swap_without_a_pin_says_so() {
    let mut h = j2();
    // the empty corner of the canvas
    let corner = h.state().view().canvas.min + eframe::egui::vec2(3.0, 3.0);
    hover(&mut h, corner);
    h.key_press(Key::W);
    let _ = h.run_ok();
    assert_eq!(h.state().notice(), Some("No pin selected."));
    assert!(!h.state().editor_busy());
}

// --- G09.5 overlays -----------------------------------------------------------

/// A start pin and a pad of another net such that a dynamic route from the
/// one to the other is stopped by that pad, with the pointer positions the
/// screen gives (found with the engine on a copy).
fn obstacle_scenario(h: &Harness<'_, App>) -> (FloatPoint, FloatPoint, ItemId) {
    let board = board_of(h);
    let mut settings = EditSettings::for_board(&board);
    settings.highlight_routing_obstacle = true;
    let pins: Vec<(ItemId, i32, FloatPoint)> = board
        .items()
        .filter(|i| i.is_pin() && i.net_count() == 1)
        .map(|i| {
            (
                i.id(),
                i.net_numbers()[0],
                i.drill().unwrap().center.to_float(),
            )
        })
        .collect();
    let mut tries = 0;
    for &(_, net, a) in &pins {
        let mut others: Vec<(ItemId, FloatPoint)> = pins
            .iter()
            .filter(|(_, n, _)| *n != net)
            .map(|(id, _, p)| (*id, *p))
            .collect();
        others.sort_by(|p, q| a.distance(p.1).total_cmp(&a.distance(q.1)));
        for (id, p) in others.into_iter().take(3) {
            tries += 1;
            assert!(tries < 150, "no obstacle scenario");
            let mut editor = Editor::new((*board).clone(), settings.clone());
            editor.apply(Ed::Click(snapped(h, a)));
            editor.apply(Ed::Move(snapped(h, a)));
            let response = editor.apply(Ed::Move(snapped(h, p)));
            if let Some(obstacle) = response.overlay.route.and_then(|r| r.failing_obstacle)
                && obstacle == id
            {
                return (a, p, id);
            }
        }
    }
    panic!("no obstacle scenario");
}

#[test]
fn obstacle_is_highlighted() {
    let mut h = j2();
    let (a, p, obstacle) = obstacle_scenario(&h);
    // with the setting on, the pad is in the overlay's highlight list
    set_interactive(&mut h, |s| s.highlight_routing_obstacle = true);
    start_route(&mut h, a);
    hover_pt(&mut h, p);
    let overlay = h.state().editor_overlay().expect("overlay");
    assert_eq!(route_overlay::highlights(overlay), vec![obstacle]);
    h.key_press(Key::Escape);
    let _ = h.run_ok();

    // with it off, the list is empty
    set_interactive(&mut h, |s| s.highlight_routing_obstacle = false);
    start_route(&mut h, a);
    hover_pt(&mut h, p);
    let overlay = h.state().editor_overlay().expect("overlay");
    assert!(route_overlay::highlights(overlay).is_empty());
}

/// The golden `route_obstacle_highlight.png`: the rubber band and the pad that
/// stopped the route, on lavapipe.
#[test]
fn snapshot_route_obstacle_highlight() {
    if !common::rendering_enabled() {
        eprintln!("skipped: rendering tests need Linux or FREEROUTING_GUI_SNAPSHOTS=1");
        return;
    }
    let mut h = common::app_harness(&[J2]);
    let (a, p, _) = obstacle_scenario(&h);
    set_interactive(&mut h, |s| s.highlight_routing_obstacle = true);
    start_route(&mut h, a);
    hover_pt(&mut h, p);
    // zoom to the two pins so the highlight is large enough to see
    let (pa, pp) = (screen(&h, a), screen(&h, p));
    let mid = camera(&h).to_board(h.state().view().canvas, pa.lerp(pp, 0.5));
    let canvas = h.state().view().canvas;
    {
        let cam = h.state_mut().view_mut().camera.as_mut().unwrap();
        cam.center = mid;
        cam.zoom *= 4.0;
        let _ = canvas;
    }
    h.state_mut().view_mut().auto_fit = None;
    let _ = h.run_ok();
    hover_pt(&mut h, p);
    h.snapshot("route_obstacle_highlight");
}

#[test]
fn analysis_waits_for_the_route() {
    let mut h = j2();
    let (a, mid, _) = start_and_target(&h);
    let runs = h.state().analysis().runs_started();
    start_route(&mut h, a);
    hover_pt(&mut h, mid);
    for _ in 0..5 {
        let _ = h.run_ok();
    }
    assert_eq!(
        h.state().analysis().runs_started(),
        runs,
        "held during the route"
    );
    h.key_press(Key::Escape);
    let _ = h.run_ok();
    common::wait_for_load(&mut h);
    assert!(
        h.state().analysis().runs_started() > runs,
        "runs again afterwards"
    );
}

// --- G09.6 move, rotate ---------------------------------------------------------

/// The placement and the pad positions of every component: (name, location,
/// rotation, pin centres).
/// One component's placement: name, location, rotation bits, pin centres.
type Placement = (String, Option<[i32; 2]>, u64, Vec<[i32; 2]>);

fn placements(board: &RoutingBoard) -> Vec<Placement> {
    board
        .components
        .iter()
        .map(|c| {
            let pins: Vec<[i32; 2]> = pins_of(board, &c.name)
                .into_iter()
                .map(|id| {
                    let p = board.item(id).unwrap().drill().unwrap().center;
                    [p.x, p.y]
                })
                .collect();
            (
                c.name.clone(),
                c.location().map(|l| [l.x, l.y]),
                c.rotation_in_degree().to_bits(),
                pins,
            )
        })
        .collect()
}

fn component_pins(board: &RoutingBoard) -> Vec<ItemId> {
    let component = board
        .components
        .iter()
        .max_by_key(|c| board.component_pins(c.id.0).len())
        .unwrap();
    board.component_pins(component.id.0)
}

/// A component that turns four times by 90 degrees about its location on a
/// copy of the board (found with the engine): its id and the pole.
fn rotatable_component(h: &Harness<'_, App>) -> (i32, FloatPoint) {
    let board = board_of(h);
    for component in board.components.iter() {
        let Some(location) = component.location() else {
            continue;
        };
        let pole = snapped(h, location.to_float());
        let mut editor = Editor::new((*board).clone(), EditSettings::for_board(&board));
        let mut turns = 0;
        for _ in 0..4 {
            let pins = editor.board().component_pins(component.id.0);
            if pins.is_empty() {
                break;
            }
            editor.apply(Ed::SetSelection(pins.into_iter().collect()));
            editor.apply(Ed::MoveSelection { to: pole });
            editor.apply(Ed::RotateSelection { degrees: 90.0 });
            if editor.mode() != ModeKind::Moving {
                break;
            }
            let done = editor.apply(Ed::Complete);
            if done.status.is_some_and(|s| s.key == "move_completed") {
                turns += 1;
            }
        }
        if turns == 4 {
            return (component.id.0, location.to_float());
        }
    }
    panic!("no component turns four times");
}

#[test]
fn rotate_four_times_is_identity() {
    let mut h = j2();
    let (component, pole_at) = rotatable_component(&h);
    let pole = screen(&h, pole_at);
    let mut states = vec![(placements(&board_of(&h)), content(&board_of(&h)))];
    for _ in 0..4 {
        // the move gives the pieces new ids: select the component again
        let ids = board_of(&h).component_pins(component);
        exec(&mut h, Command::Select(ids));
        hover(&mut h, pole);
        h.key_press(Key::M);
        let _ = h.run_ok();
        assert!(h.state().is_moving(), "M picks the component up");
        exec(&mut h, Command::RotateSelection(90));
        exec(&mut h, Command::Complete);
        assert_eq!(h.state().notice(), Some("Move completed."));
        assert!(!h.state().editor_busy());
        let board = board_of(&h);
        states.push((placements(&board), content(&board)));
    }
    assert_ne!(states[1].0, states[0].0, "one turn changes the placement");
    assert_eq!(states[4].0, states[0].0, "four turns are the identity");
    // one undo per rotation
    for k in (0..4).rev() {
        h.key_press(Key::U);
        let _ = h.run_ok();
        let board = board_of(&h);
        assert_eq!(
            placements(&board),
            states[k].0,
            "after undoing rotation {}",
            k + 1
        );
    }
}

#[test]
fn move_then_click_puts_the_selection_down() {
    let mut h = j2();
    let board = board_of(&h);
    let pins = component_pins(&board);
    let first = board
        .item(pins[0])
        .unwrap()
        .drill()
        .unwrap()
        .center
        .to_float();
    let target = FloatPoint::new(first.x + 20_000.0, first.y + 10_000.0);
    let (p0, p1) = (screen(&h, first), screen(&h, target));
    // the engine's move of a copy, with the pointer positions the screen gives
    let (start, end) = (board_under(&h, p0), board_under(&h, p1));
    let mut editor = Editor::new((*board).clone(), EditSettings::for_board(&board));
    editor.apply(Ed::SetSelection(pins.iter().copied().collect()));
    editor.apply(Ed::MoveSelection { to: start });
    editor.apply(Ed::Move(end));
    editor.apply(Ed::Click(end));
    let expected = placements(editor.board());

    exec(&mut h, Command::Select(pins));
    hover(&mut h, p0);
    h.key_press(Key::M);
    let _ = h.run_ok();
    assert!(h.state().is_moving());
    assert!(
        h.state().selection().is_empty(),
        "the picked-up items left the selection"
    );
    assert!(!h.state().editor_overlay().unwrap().floating.is_empty());
    hover(&mut h, p1);
    click(&mut h, p1);
    assert_eq!(h.state().notice(), Some("Move completed."));
    assert_eq!(placements(&board_of(&h)), expected);
    assert_ne!(expected, placements(&board));
}

#[test]
fn escape_puts_a_moving_selection_back() {
    let mut h = j2();
    let pins = component_pins(&board_of(&h));
    let before = placements(&board_of(&h));
    exec(&mut h, Command::Select(pins));
    exec(&mut h, Command::RotateSelection(90));
    assert!(h.state().is_moving());
    h.key_press(Key::Escape);
    let _ = h.run_ok();
    assert!(!h.state().editor_busy());
    assert_eq!(placements(&board_of(&h)), before);
}

#[test]
fn undo_clears_the_editors_selection_but_not_the_guis() {
    // The engine refuses history while the editor holds a selection; the GUI
    // keeps its selection and undoes anyway (DIVERGENCES).
    let mut h = j2();
    let pins = component_pins(&board_of(&h));
    exec(&mut h, Command::Select(pins.clone()));
    exec(&mut h, Command::RotateSelection(90));
    exec(&mut h, Command::Complete);
    let moved = placements(&board_of(&h));
    exec(&mut h, Command::Select(pins));
    h.key_press(Key::U);
    let _ = h.run_ok();
    assert_ne!(placements(&board_of(&h)), moved, "undone with a selection");
    assert_eq!(h.state().notice(), Some("Undo"));
    assert!(
        !h.state().selection().is_empty(),
        "the GUI's selection stays"
    );
    let _ = BoardChanges::default();
}

#[test]
fn component_movement_settings_set_the_placement_grid() {
    let mut h = j2();
    exec(
        &mut h,
        Command::ShowRulesDialog(freerouting_gui::dialogs::RulesDialog::MoveSettings),
    );
    assert!(
        h.state()
            .dialogs_open(freerouting_gui::dialogs::RulesDialog::MoveSettings)
    );
    exec(
        &mut h,
        Command::SetComponentGrid {
            horizontal: 5_000,
            vertical: 2_500,
        },
    );
    assert_eq!(h.state().tools().component_grid, (5_000, 2_500));
    // the editor gets it with its next command (here a route start)
    let (a, _, _) = start_and_target(&h);
    start_route(&mut h, a);
    assert_eq!(
        h.state().editor().unwrap().options().component_grid,
        (5_000, 2_500)
    );
}

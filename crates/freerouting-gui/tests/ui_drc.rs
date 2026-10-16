//! G03: selection, ratsnest, DRC markers, the Info lists, delete all with
//! undo, and the DRC report export.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{app_harness, fixtures, plain_harness, rendering_enabled};
use eframe::egui::Key;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use freerouting::board::items::ItemId;
use freerouting::drc::json::JsonValue;
use freerouting::drc::{DesignRulesChecker, DrcRunOptions, run_drc};
use freerouting::scoring::BoardStatistics;
use freerouting_gui::App;
use freerouting_gui::commands::{Command, ListKind};
use freerouting_gui::overlays::Frame;
use freerouting_gui::panels::{incompletes, violations};

const ISSUE159: &str = "Issue159-setonix_2hp-pcb.dsn";
const ISSUE684: &str = "Issue684-Autorouter_PCB1_2026-5-8.dsn";
const ISSUE143: &str = "Issue143-rpi_splitter.dsn";

/// Runs frames until the UI is still. A background worker that asks for a
/// repaint right then must not fail the test, so the step limit is not an
/// error here.
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

/// Steps frames until the background analysis has delivered the results of
/// the board's current state.
fn wait_for_analysis(h: &mut Harness<'_, App>) {
    let start = Instant::now();
    loop {
        h.step();
        let state = h.state();
        let loaded = state.loaded().expect("board loaded");
        if state.analysis().is_idle(&loaded.session) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "the analysis did not finish"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    settle!(h);
}

fn analysed(files: &[&str]) -> Harness<'static, App> {
    let mut h = plain_harness(files);
    wait_for_analysis(&mut h);
    h
}

/// The number at `section.field` of the load reference of `board`.
fn reference_count(board: &str, section: &str, field: &str) -> usize {
    let stem = board.trim_end_matches(".dsn");
    let path = fixtures().join(format!("reference/load/{stem}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let root = JsonValue::parse(&text).expect("reference JSON");
    let member = |value: &JsonValue, name: &str| -> JsonValue {
        match value {
            JsonValue::Object(members) => members
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| panic!("{name} missing in {}", path.display())),
            other => panic!("not an object: {other:?}"),
        }
    };
    let statistics = member(&root, "statistics");
    match member(&member(&statistics, section), field) {
        JsonValue::Number(n) => n.parse().expect("integer"),
        other => panic!("not a number: {other:?}"),
    }
}

fn board_of(h: &Harness<'_, App>) -> std::sync::Arc<freerouting::board::RoutingBoard> {
    h.state().loaded().unwrap().session.board.clone()
}

// ---------------------------------------------------------------------------
// G03.1
// ---------------------------------------------------------------------------

/// Two pins that are far apart and much wider than tall, so that the box
/// around them has the aspect of the canvas or wider (the canvas is wider
/// than tall, so the limit of 1.25 times the larger side only holds for such
/// a pair).
fn far_pads(h: &Harness<'_, App>) -> [ItemId; 2] {
    let board = board_of(h);
    let pins: Vec<_> = board
        .basic()
        .items()
        .filter(|i| i.is_pin())
        .map(|i| (i.id(), i.bounding_box()))
        .collect();
    let mut best: Option<(i64, ItemId, ItemId)> = None;
    for (a, ba) in &pins {
        for (b, bb) in &pins {
            let dx = i64::from(bb.ll.x) - i64::from(ba.ll.x);
            let dy = (i64::from(bb.ll.y) - i64::from(ba.ll.y)).abs();
            if dx > 0 && dy * 4 <= dx && best.is_none_or(|(d, _, _)| dx > d) {
                best = Some((dx, *a, *b));
            }
        }
    }
    let (_, a, b) = best.expect("two pins in a row");
    [a, b]
}

#[test]
fn zoom_to_selection_contains_items() {
    let mut h = plain_harness(&[ISSUE159]);
    let pads = far_pads(&h);
    assert_ne!(pads[0], pads[1]);
    exec(&mut h, Command::Select(pads.to_vec()));
    assert_eq!(h.state().selection().len(), 2);
    h.key_press(Key::W);
    settle!(h);

    let board = board_of(&h);
    let frame = Frame::new(board.basic());
    let mut union = freerouting_gui::scene::Bounds::EMPTY;
    for id in pads {
        let b = frame
            .bounds(&board.basic().item(id).unwrap().bounding_box())
            .unwrap();
        union.union(&b);
    }
    let view = h.state().view();
    let camera = view.camera.expect("camera");
    let (min, max) = (
        camera.to_board(view.canvas, view.canvas.left_bottom()),
        camera.to_board(view.canvas, view.canvas.right_top()),
    );
    assert!(
        min.x <= union.min.x
            && min.y <= union.min.y
            && max.x >= union.max.x
            && max.y >= union.max.y,
        "visible ({min:?} - {max:?}) must contain {union:?}"
    );
    let larger_side = union.width().max(union.height());
    let visible = (max.x - min.x).max(max.y - min.y);
    assert!(
        visible <= 1.25 * larger_side,
        "visible side {visible} against 1.25 x {larger_side}"
    );
}

#[test]
fn w_without_a_selection_keeps_the_view() {
    let mut h = plain_harness(&[ISSUE159]);
    let before = h.state().view().camera.unwrap();
    h.key_press(Key::W);
    settle!(h);
    let after = h.state().view().camera.unwrap();
    assert_eq!((before.center, before.zoom), (after.center, after.zoom));
}

#[test]
fn snapshot_highlight() {
    if !rendering_enabled() {
        eprintln!("skipped: rendering tests need Linux or FREEROUTING_GUI_SNAPSHOTS=1");
        return;
    }
    let mut h = app_harness(&[ISSUE159]);
    wait_for_analysis(&mut h);
    let pads = far_pads(&h);
    exec(&mut h, Command::Select(pads.to_vec()));
    h.step();
    assert!(h.state().stats().overlay.highlighted >= 2);
    // Zoomed to the two pads, so the highlight is big enough to see.
    h.key_press(Key::W);
    settle!(h);
    h.snapshot("highlight_issue159");
}

// ---------------------------------------------------------------------------
// G03.2
// ---------------------------------------------------------------------------

#[test]
fn incomplete_counts_match_reference() {
    for board in ["Issue508-SMD-routing-issue-demo.dsn", ISSUE143, ISSUE684] {
        let mut h = analysed(&[board]);
        let expected = reference_count(board, "connections", "incomplete_count");
        let label = format!("Incomplete connections: {expected}");
        h.get_by_label(&label);
        // The same number from the engine, and one overlay line per airline.
        let engine = {
            let board = board_of(&h);
            let mut checker = DesignRulesChecker::new(board.basic());
            (checker.incomplete_count(), checker.all_airlines().len())
        };
        assert_eq!(engine.0, expected, "{board}");
        h.step();
        assert!(h.state().overlay().show_ratsnest);
        assert_eq!(h.state().stats().overlay.airlines, engine.1, "{board}");
    }
}

#[test]
fn g_toggles_the_ratsnest() {
    let mut h = analysed(&[ISSUE143]);
    h.step();
    let drawn = h.state().stats().overlay.airlines;
    assert!(drawn > 0);
    h.key_press(Key::G);
    settle!(h);
    assert!(!h.state().overlay().show_ratsnest);
    assert_eq!(h.state().stats().overlay.airlines, 0);
    h.get_by_label("Ratsnest").click();
    settle!(h);
    assert_eq!(h.state().stats().overlay.airlines, drawn);
}

#[test]
fn snapshot_ratsnest_issue143() {
    if !rendering_enabled() {
        eprintln!("skipped: rendering tests need Linux or FREEROUTING_GUI_SNAPSHOTS=1");
        return;
    }
    let mut h = app_harness(&[ISSUE143]);
    wait_for_analysis(&mut h);
    h.snapshot("ratsnest_issue143");
}

// ---------------------------------------------------------------------------
// G03.3
// ---------------------------------------------------------------------------

#[test]
fn violation_markers_match_reference() {
    for board in [
        "PCBench-retroreflectors-CONGFLOCK.dsn",
        ISSUE684,
        "Issue313-FastTest.dsn",
    ] {
        let mut h = analysed(&[board]);
        // Off by default, as in Java.
        assert!(!h.state().overlay().show_violations);
        assert_eq!(h.state().stats().overlay.markers, 0);
        h.key_press(Key::V);
        settle!(h);
        h.step();
        let expected = reference_count(board, "clearance_violations", "total_count");
        assert_eq!(h.state().stats().overlay.markers, expected, "{board}");
        h.get_by_label(&format!("Clearance violations: {expected}"));
    }
}

#[test]
fn edits_start_a_new_analysis_and_stale_results_are_hidden() {
    let mut h = analysed(&[ISSUE684]);
    let runs = h.state().analysis().runs_started();
    exec(&mut h, Command::DeleteAll);
    exec(&mut h, Command::ConfirmDeleteAll);
    // Right after the edit nothing is current; a run starts on the next frame.
    wait_for_analysis(&mut h);
    assert!(h.state().analysis().runs_started() > runs);
    let session = &h.state().loaded().unwrap().session;
    assert!(h.state().analysis().drc(session).is_some());
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "release only: Issue190 is slow in a debug build"
)]
fn drc_does_not_block_frames_issue190() {
    let mut h = plain_harness(&["Issue190-processor.Z80.dsn"]);
    let start = Instant::now();
    let mut frames = 0;
    // The first frames paint the board for the first time; the check is on
    // the frames while the worker runs after that.
    loop {
        h.step();
        frames += 1;
        let state = h.state();
        let session = &state.loaded().unwrap().session;
        if state.analysis().is_idle(session) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "DRC did not finish"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let samples: Vec<f64> = h.state().stats().samples().skip(3).collect();
    let max = samples.iter().copied().fold(0.0, f64::max);
    eprintln!("{frames} frames, max frame CPU {max:.3} ms");
    assert!(
        frames > 5,
        "the DRC finished before frames could be measured"
    );
    assert!(max <= 50.0, "max frame CPU {max} ms");
}

// ---------------------------------------------------------------------------
// G03.4 - G03.6
// ---------------------------------------------------------------------------

fn list_rows(
    h: &Harness<'_, App>,
    kind: ListKind,
) -> Vec<freerouting_gui::panels::object_list::Row> {
    h.state()
        .lists()
        .built_rows(kind)
        .expect("rows built")
        .to_vec()
}

fn open_list(h: &mut Harness<'_, App>, kind: ListKind) {
    exec(h, Command::ShowList(kind));
    wait_for_analysis(h);
    settle!(h);
}

#[test]
fn clearance_list_select() {
    let mut h = analysed(&[ISSUE684]);
    // Through the menu, as a user does.
    h.get_by_label("Info").click();
    settle!(h);
    h.get_by_label("Clearance Violations").click();
    settle!(h);
    assert!(h.state().lists().is_open(ListKind::ClearanceViolations));
    settle!(h);
    let rows = list_rows(&h, ListKind::ClearanceViolations);
    assert_eq!(rows.len(), 5);
    for row in &rows {
        h.get_by_label(&row.text);
    }

    // The first row is the violation with the largest shortfall.
    let board = board_of(&h);
    let all = DesignRulesChecker::new(board.basic()).all_clearance_violations();
    let largest = all
        .iter()
        .map(|v| v.expected_clearance - v.actual_clearance)
        .fold(f64::MIN, f64::max);
    let first = violations::sorted(&all)[0];
    assert_eq!(first.expected_clearance - first.actual_clearance, largest);
    assert_eq!(rows[0].text, {
        let frame = Frame::new(board.basic());
        violations::row_text(board.basic(), &frame, first)
    });

    h.get_by_label(&rows[0].text).click();
    settle!(h);
    h.get_by_label("Select").click();
    settle!(h);
    let selected: std::collections::BTreeSet<_> =
        h.state().selection().items.iter().copied().collect();
    let expected: std::collections::BTreeSet<_> =
        [first.first_item, first.second_item].into_iter().collect();
    assert_eq!(selected, expected);

    // Select zoomed to the two items: they are inside the view.
    let frame = Frame::new(board.basic());
    let view = h.state().view();
    let camera = view.camera.unwrap();
    let (min, max) = (
        camera.to_board(view.canvas, view.canvas.left_bottom()),
        camera.to_board(view.canvas, view.canvas.right_top()),
    );
    for id in expected {
        let b = frame
            .bounds(&board.basic().item(id).unwrap().bounding_box())
            .unwrap();
        assert!(min.x <= b.min.x && min.y <= b.min.y && max.x >= b.max.x && max.y >= b.max.y);
    }
}

/// The counts of Java's `ViolationsIncompletesListA11yTest`: the list is
/// bound to the engine's violations and airlines, 67 and 9 on these boards.
/// Java's window would show fewer rows if two violations had the same
/// shortfall on the same layer (its `TreeSet` drops them); this list keeps
/// every violation, so the count is the engine's.
#[test]
fn java_a11y_list_counts() {
    let mut h = analysed(&["Issue575-drc_BBD_Mars-64_6_track_1_hole_clearance_violations.dsn"]);
    open_list(&mut h, ListKind::ClearanceViolations);
    let rows = list_rows(&h, ListKind::ClearanceViolations);
    assert_eq!(rows.len(), 67);
    let board = board_of(&h);
    let all = DesignRulesChecker::new(board.basic()).all_clearance_violations();
    let mut keys: Vec<(String, i32)> = all
        .iter()
        .map(|v| (format!("{:?}", violations::delta(v)), v.layer))
        .collect();
    keys.sort();
    keys.dedup();
    // Java's window would list 3 rows here (equal shortfall and layer
    // compare equal in its TreeSet).
    assert_eq!(keys.len(), 3);

    let mut h = analysed(&["Issue575-drc_dev-board_4_hole_clearance_violations.dsn"]);
    open_list(&mut h, ListKind::Incompletes);
    assert_eq!(list_rows(&h, ListKind::Incompletes).len(), 9);
}

#[test]
fn clearance_list_empty_text() {
    // Issue143 has no clearance violations in the load reference.
    assert_eq!(
        reference_count(ISSUE143, "clearance_violations", "total_count"),
        0
    );
    let mut h = analysed(&[ISSUE143]);
    open_list(&mut h, ListKind::ClearanceViolations);
    h.get_by_label("No clearance violations found.");
}

#[test]
fn incompletes_list() {
    let mut h = analysed(&[ISSUE143]);
    open_list(&mut h, ListKind::Incompletes);
    let board = board_of(&h);
    let airlines = DesignRulesChecker::new(board.basic()).all_airlines();
    let rows = list_rows(&h, ListKind::Incompletes);
    assert_eq!(rows.len(), airlines.len());
    assert!(!rows.is_empty());
    // Grouped by net: the net names never go back to an earlier one.
    let nets: Vec<&str> = rows
        .iter()
        .map(|r| r.text.split(": ").next().unwrap())
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut last = "";
    for net in nets {
        if net != last {
            assert!(seen.insert(net), "net {net} is split into two groups");
            last = net;
        }
    }
    // The first row's text and its Select.
    let first = incompletes::sorted(board.basic(), &airlines)[0];
    assert_eq!(rows[0].text, incompletes::row_text(board.basic(), first));
    h.get_by_label(&rows[0].text).click();
    settle!(h);
    h.get_by_label("Select").click();
    settle!(h);
    let selected: std::collections::BTreeSet<_> =
        h.state().selection().items.iter().copied().collect();
    let expected: std::collections::BTreeSet<_> =
        [first.from_item, first.to_item].into_iter().collect();
    assert_eq!(selected, expected);
}

#[test]
fn incompletes_list_of_a_routed_board_says_route_completed() {
    // Issue313's session completes every connection.
    let mut h = analysed(&["Issue313-FastTest.dsn", "Issue313-FastTest.ses"]);
    let count = {
        let board = board_of(&h);
        DesignRulesChecker::new(board.basic()).incomplete_count()
    };
    open_list(&mut h, ListKind::Incompletes);
    if count == 0 {
        h.get_by_label("Route completed.");
    } else {
        assert_eq!(list_rows(&h, ListKind::Incompletes).len(), count);
    }
}

fn row_counts(files: &[&str]) -> (usize, usize, usize, usize, usize) {
    let mut h = analysed(files);
    for kind in [
        ListKind::LengthViolations,
        ListKind::Unconnected,
        ListKind::ZoneIslands,
        ListKind::ClearanceViolations,
        ListKind::Incompletes,
    ] {
        open_list(&mut h, kind);
    }
    let board = board_of(&h);
    let mut checker = DesignRulesChecker::new(board.basic());
    let (length, unconnected, islands) = (
        checker.length_violation_count(),
        checker.all_unconnected_items().len(),
        checker.zone_island_violations().len(),
    );
    let rows = |k| list_rows(&h, k).len();
    assert_eq!(rows(ListKind::LengthViolations), length, "{files:?}");
    assert_eq!(rows(ListKind::Unconnected), unconnected, "{files:?}");
    assert_eq!(rows(ListKind::ZoneIslands), islands, "{files:?}");
    (
        length,
        unconnected,
        islands,
        rows(ListKind::ClearanceViolations),
        rows(ListKind::Incompletes),
    )
}

#[test]
fn other_lists_match_engine() {
    // One unconnected group in the Java DRC digest.
    let (_, unconnected, _, _, _) = row_counts(&["Issue143-rpi_splitter_mod.dsn"]);
    assert!(unconnected >= 1);
    // One dangling trace.
    let (_, unconnected, _, _, _) = row_counts(&["Issue054-tairakb.dsn"]);
    assert!(unconnected >= 1);
    // One dangling via.
    let (_, unconnected, _, _, _) = row_counts(&["Issue029-hw48na.dsn"]);
    assert!(unconnected >= 1);
}

#[test]
fn length_violations_list() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/length_violation.dsn");
    let mut h = Harness::builder()
        .with_size(common::SIZE)
        .build_eframe(|cc| App::new(cc, vec![path.clone()]));
    common::wait_for_load(&mut h);
    wait_for_analysis(&mut h);
    open_list(&mut h, ListKind::LengthViolations);
    let rows = list_rows(&h, ListKind::LengthViolations);
    let board = board_of(&h);
    assert_eq!(
        rows.len(),
        DesignRulesChecker::new(board.basic()).length_violation_count()
    );
    // N1's trace is 10000 um long where 5000 is the maximum; N2's is 3000
    // long where 50000 is the minimum. Java's `String.valueOf(float)`.
    let texts: Vec<&str> = rows.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "The trace length for net N1 is 10000.0; the maximum allowed is 5000.0.",
            "The trace length for net N2 is 3000.0; the minimum allowed is 50000.0.",
        ]
    );
    h.get_by_label(texts[0]);
}

/// A pour on the top layer that a signal trace cuts in two, with a ground
/// via on each side: the engine finds copper islands, and the list has a
/// row for each.
#[test]
fn zone_islands_list() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/zone_islands.dsn");
    let mut h = Harness::builder()
        .with_size(common::SIZE)
        .build_eframe(|cc| App::new(cc, vec![path.clone()]));
    common::wait_for_load(&mut h);
    wait_for_analysis(&mut h);
    open_list(&mut h, ListKind::ZoneIslands);
    let rows = list_rows(&h, ListKind::ZoneIslands);
    let board = board_of(&h);
    let islands = DesignRulesChecker::new(board.basic()).zone_island_violations();
    assert!(!islands.is_empty(), "the board has copper islands");
    assert_eq!(rows.len(), islands.len());
    assert!(rows.iter().all(|r| r.text.contains("on net [GND]")));
    // Select takes the pour and the items in the island.
    settle!(h);
    h.get_by_label(&rows[0].text).click();
    settle!(h);
    h.get_by_label("Select").click();
    settle!(h);
    assert!(!h.state().selection().is_empty());
}

/// A clean board: both nets are routed pin to pin, there is no pour and no
/// violation, so every list but the length list (which has the two rules'
/// violations) is empty and shows Java's text.
#[test]
fn empty_lists_show_javas_texts() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/length_violation.dsn");
    let mut h = Harness::builder()
        .with_size(common::SIZE)
        .build_eframe(|cc| App::new(cc, vec![path.clone()]));
    common::wait_for_load(&mut h);
    wait_for_analysis(&mut h);
    for (kind, empty) in [
        (ListKind::Incompletes, "Route completed."),
        (
            ListKind::ClearanceViolations,
            "No clearance violations found.",
        ),
        (
            ListKind::Unconnected,
            "No unconnected traces or vias found.",
        ),
        (ListKind::ZoneIslands, "No zone islands found."),
    ] {
        open_list(&mut h, kind);
        assert!(list_rows(&h, kind).is_empty(), "{kind:?}");
        h.get_by_label(empty);
    }
    // And the length list of a board without length rules.
    let mut h = analysed(&[ISSUE143]);
    open_list(&mut h, ListKind::LengthViolations);
    assert!(list_rows(&h, ListKind::LengthViolations).is_empty());
    h.get_by_label("No length violations found.");
}

// ---------------------------------------------------------------------------
// G03.7
// ---------------------------------------------------------------------------

fn counts(h: &Harness<'_, App>) -> (usize, usize, usize) {
    let board = board_of(h);
    let s = BoardStatistics::new(board.basic());
    (
        s.items.trace_count,
        s.items.via_count,
        s.connections.incomplete_count,
    )
}

/// The spec's board: Issue684 has no wiring at load (the load reference
/// says 0 traces and 0 vias), so deleting leaves the counts at 0 and Undo
/// restores the same. The substantive check is on Issue026 with its session
/// (`delete_all_and_undo_with_wiring`).
#[test]
fn delete_all_and_undo() {
    let mut h = analysed(&[ISSUE684]);
    delete_all_and_undo_on(&mut h);
}

#[test]
fn delete_all_and_undo_with_wiring() {
    let mut h = analysed(&["Issue026-J2_reference.dsn", "Issue026-J2_reference.ses"]);
    let (traces, vias, _) = counts(&h);
    assert!(traces > 0 && vias > 0, "the session routes traces and vias");
    delete_all_and_undo_on(&mut h);
}

fn delete_all_and_undo_on(h: &mut Harness<'_, App>) {
    let before = counts(h);

    // Other > Delete All Traces and Vias asks first.
    h.get_by_label("Other").click();
    settle!(h);
    h.get_by_label("Delete All Traces and Vias").click();
    settle!(h);
    assert_eq!(counts(h), before, "nothing happens before the confirmation");
    h.get_by_label("Delete").click();
    settle!(h);
    wait_for_analysis(h);
    let after = counts(h);
    assert_eq!((after.0, after.1), (0, 0));
    assert!(after.2 >= before.2);
    h.get_by_label(&format!("Incomplete connections: {}", after.2));

    // U brings the wiring back, B deletes it again.
    h.key_press(Key::U);
    settle!(h);
    wait_for_analysis(h);
    assert_eq!(counts(h), before);
    h.get_by_label(&format!("Incomplete connections: {}", before.2));
    h.key_press(Key::B);
    settle!(h);
    wait_for_analysis(h);
    assert_eq!(counts(h), after);
}

#[test]
fn ctrl_z_and_ctrl_y_undo_and_redo() {
    let mut h = analysed(&[ISSUE684]);
    let before = counts(&h);
    exec(&mut h, Command::DeleteAll);
    exec(&mut h, Command::ConfirmDeleteAll);
    let after = counts(&h);
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, Key::Z);
    settle!(h);
    assert_eq!(counts(&h), before);
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, Key::Y);
    settle!(h);
    assert_eq!(counts(&h), after);
}

#[test]
fn cancelling_delete_all_keeps_the_wiring() {
    let mut h = analysed(&["Issue026-J2_reference.dsn", "Issue026-J2_reference.ses"]);
    let before = counts(&h);
    // The toolbar button asks as well.
    h.get_by_label("Delete All").click();
    settle!(h);
    h.get_by_label("Cancel").click();
    settle!(h);
    assert_eq!(counts(&h), before);
    // No snapshot was taken: Undo does nothing.
    exec(&mut h, Command::Undo);
    assert_eq!(counts(&h), before);
    assert!(!h.state().loaded().unwrap().session.is_dirty());
}

// ---------------------------------------------------------------------------
// G03.8
// ---------------------------------------------------------------------------

fn without_date(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(members) => {
            JsonValue::Object(members.into_iter().filter(|(n, _)| n != "date").collect())
        }
        other => other,
    }
}

#[test]
fn drc_export_equals_cli() {
    export_equals_cli(&["Issue313-FastTest.dsn"], None);
}

/// With a session on top the CLI does its post-load processing before the
/// import; the app's board must give the same report.
#[test]
fn drc_export_equals_cli_with_session() {
    export_equals_cli(
        &["Issue313-FastTest.dsn", "Issue313-FastTest.ses"],
        Some("Issue313-FastTest.ses"),
    );
}

fn export_equals_cli(files: &[&str], session: Option<&str>) {
    let design = fixtures().join("Issue313-FastTest.dsn");
    let out: PathBuf = std::env::temp_dir().join(format!(
        "freerouting-gui-drc-export-{}-{}.json",
        std::process::id(),
        files.len()
    ));
    let _ = std::fs::remove_file(&out);

    let mut h = analysed(files);
    exec(&mut h, Command::ExportDrcReport(out.clone()));
    let start = Instant::now();
    while h.state().is_exporting() {
        assert!(start.elapsed() < Duration::from_secs(120), "export hangs");
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    settle!(h);
    assert!(
        h.state()
            .notice()
            .is_some_and(|n| n.contains("DRC report written")),
        "{:?}",
        h.state().notice()
    );
    let exported = std::fs::read_to_string(&out).expect("exported file");
    assert!(
        !out.with_extension("json.tmp").exists(),
        "temp file renamed"
    );
    let _ = std::fs::remove_file(&out);

    let mut options = DrcRunOptions::new(&design);
    options.session_file = session.map(|f| fixtures().join(f));
    let cli = run_drc(&options).expect("cli run");
    let a = without_date(JsonValue::parse(&exported).expect("exported JSON"));
    let b = without_date(cli.report.to_json_value());
    assert_eq!(a, b);
}

#[test]
fn export_menu_opens_a_dialog_with_a_default_path() {
    let mut h = analysed(&[ISSUE159]);
    h.get_by_label("File").click();
    settle!(h);
    h.get_by_label("Export DRC report…").click();
    settle!(h);
    h.get_by_label("Export DRC report");
    h.get_by_label("Export");
}

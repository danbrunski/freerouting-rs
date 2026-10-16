//! Move, rotate, mirror and copy of a selection against Java (I04).
//!
//! Probe `transform` (`tools/java-reference/probes/board/TransformProbe.java`), mirrored by
//! `tests/support/transform_script.rs`.

mod board_probe;
#[path = "support/edit_script.rs"]
mod edit_script;
#[path = "support/editor_script.rs"]
mod editor_script;
#[path = "support/item_dump.rs"]
mod item_dump;
#[path = "support/transform_script.rs"]
mod transform_script;

use board_probe::{compare, header};
use freerouting::board::RoutingBoard;

const KIND: &str = "transform";

fn check(board: &str, session: bool) {
    let row = board_probe::fixture(board);
    let load = || board_probe::load(&row);
    let import = |b: &mut RoutingBoard| {
        board_probe::import_session(b, &row);
    };
    let mut lines = vec![header(KIND, &row.board)];
    lines.extend(transform_script::transform_lines(
        load,
        session.then_some(&import as &dyn Fn(&mut RoutingBoard)),
    ));
    if let Err(e) = compare(KIND, &row, &lines) {
        panic!("{e}");
    }
}

#[test]
fn transform_matches_java_j2() {
    check("Issue026-J2_reference", true);
}

#[test]
fn transform_matches_java_bm05() {
    check("Issue508-DAC2020_bm05", true);
}

#[test]
fn transform_matches_java_issue955() {
    check("Issue955-rotated-pad-contacts", false);
}

// ---------------------------------------------------------------------------------------------
// fixtures.Issue955PlacementLifecycleTest: the transform cases (I04.2)
// ---------------------------------------------------------------------------------------------
//
// Ported here: quarterTurnPreservesContact, arbitraryTurnPreservesContact and
// mirrorPreservesContact. The other cases of the class are in `tests/item_transform.rs`
// (B05.8): copyConstructionPreservesLocation, copyRebindingInvalidatesCachedPadGeometry,
// copyRebindingInvalidatesCachedKeepoutGeometry (the copy cases),
// backSideRotateFirstQuarterTurnUsesWorldDirection and
// backSideMirrorFirstReflectionUsesWorldDirection (the component transform cases),
// shapeAndPinUseSameAnchor, mirroredExitDirectionsFollowPadOrientation,
// sesPreservesFractionalPlacement and fractionalRotationSurvivesDsnRoundTrip. Not ported:
// dsnRoundTripPreservesBothContacts (C06, `tests/dsn_round_trip.rs`) and
// packageIdentityIncludesFractionalOffsets (needs the private KiCad JSON reader).

use std::collections::BTreeSet;

use freerouting::board::items::{AreaKind, Item, ItemId};
use freerouting::board::structure::{ComponentId, FixedState};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{
    Area, Circle, ConvexShape, FloatPoint, IntBox, IntPoint, IntVector, Point, Polyline, Shape,
    Vector,
};
use freerouting::interactive::{
    Command, CopySession, EditSettings, Editor, ModeKind, Tool, apply_transform,
};
use freerouting::library::PackagePin;

fn issue955() -> RoutingBoard {
    board_probe::load(&board_probe::fixture("Issue955-rotated-pad-contacts"))
}

/// Java's `synthetic`: a component REVIEW with one pin of a 2-layer circle padstack (radius
/// 100) at `offset`, net 1, clearance class 1.
fn synthetic(board: &mut RoutingBoard, placement: FloatPoint, offset: FloatPoint) -> ItemId {
    let circle = ConvexShape::Circle(Circle::new(IntPoint::new(0, 0), 100));
    let pad = board
        .library
        .padstacks
        .add_unnamed(vec![Some(circle.clone()), Some(circle)]);
    let pkg = board
        .library
        .packages
        .add_unnamed(vec![PackagePin::new("1", pad, offset, 0.0)]);
    let component =
        board
            .components
            .add_precise("REVIEW", Some(placement), 0.0, true, pkg, pkg, false, None);
    board
        .insert_pin(component.0, 0, &[1], 1, FixedState::Unfixed)
        .expect("synthetic pin")
}

fn centre(board: &RoutingBoard, id: ItemId) -> IntPoint {
    board.item(id).unwrap().drill().unwrap().center
}

/// Java's `transform(angle, mirror)` of `Issue955PlacementLifecycleTest`.
fn lifecycle_transform(angle: f64, mirror: bool) {
    let mut board = issue955();
    let pin = synthetic(
        &mut board,
        FloatPoint::new(20000.5, 50000.5),
        FloatPoint::new(0.0, 0.0),
    );
    let before = centre(&board, pin);
    let trace = board
        .insert_trace_without_cleaning(
            Polyline::from_points(&[
                Point::Int(before),
                Point::Int(IntPoint::new(before.x + 1000, before.y)),
            ]),
            0,
            10,
            &[1],
            1,
            FixedState::Unfixed,
        )
        .expect("trace");
    assert_eq!(board.normal_contacts(pin).len(), 1);
    // Java hands the detached objects themselves (same ids) to `apply`.
    let mut detached = Vec::new();
    for id in [pin, trace] {
        let item = board.item(id).unwrap().clone();
        board.detach_item_for_move(id).expect("detach");
        detached.push(board.copy_of(&item, Some(id)).expect("copy"));
    }
    let moved = apply_transform(&mut board, &detached, angle, mirror, IntPoint::new(0, 0))
        .expect("the transform is accepted");
    let ids: Vec<ItemId> = moved.into_iter().map(|i| board.insert_item(i)).collect();
    let (pin, trace) = (ids[0], ids[1]);
    let first = board
        .item(trace)
        .and_then(Item::as_trace)
        .unwrap()
        .first_corner();
    assert_eq!(first, Point::Int(centre(&board, pin)));
    assert_eq!(board.normal_contacts(pin).len(), 1);
}

#[test]
fn quarter_turn_preserves_contact() {
    lifecycle_transform(90.0, false);
}

#[test]
fn arbitrary_turn_preserves_contact() {
    lifecycle_transform(45.0, false);
}

#[test]
fn mirror_preserves_contact() {
    lifecycle_transform(0.0, true);
}

// ---------------------------------------------------------------------------------------------
// gui.interactive.CopyItemPreviewTest (I04.3)
// ---------------------------------------------------------------------------------------------
//
// Ported: previewFollowsCursorWithoutMovingSourceComponent for the pin and the keepout. Java
// renders the preview offscreen and compares the pixel bounds; the port compares the exact
// bounding box of the preview geometry (`CopySession::preview_geometry`, what the overlay
// carries), which has to move by the board offset and keep its size, and the source
// component's exact location, which must not change.

fn box_of(geometry: &freerouting::board::ItemGeometry) -> IntBox {
    geometry.bounding_box()
}

fn copy_preview_case(keepout: bool) {
    let mut board = issue955();
    let pin = *board
        .pins()
        .iter()
        .find(|id| {
            let item = board.item(**id).unwrap();
            board.pin_name(item) == Some("2@1")
                && board
                    .components
                    .get(ComponentId(item.component_id()))
                    .is_some_and(|c| c.name == "S21")
        })
        .expect("pin S21 2@1");
    let component = ComponentId(board.item(pin).unwrap().component_id());
    let source = board.components.get(component).unwrap().exact_location();
    let start = board.components.get(component).unwrap().location().unwrap();
    let item = if keepout {
        board.insert_area(
            AreaKind::Obstacle,
            Area::from(Shape::from(Circle::new(IntPoint::new(0, 0), 3000))),
            0,
            Vector::Int(IntVector::new(0, 0)),
            0.0,
            false,
            &[],
            1,
            component.0,
            Some("preview".into()),
            FixedState::Unfixed,
        )
    } else {
        pin
    };
    let selection = BTreeSet::from([item]);
    let mut session = CopySession::start(&mut board, &selection, 0, start.to_float()).unwrap();
    let before = box_of(&session.preview_geometry()[0]);
    assert!(before.ur.x > before.ll.x && before.ur.y > before.ll.y);
    for step in 1..=2 {
        let (dx, dy) = (step * 10000, -step * 20000);
        let target = FloatPoint::new(
            f64::from(start.x) + f64::from(dx),
            f64::from(start.y) + f64::from(dy),
        );
        session.change_position(&board, target);
        let after = box_of(&session.preview_geometry()[0]);
        assert_eq!(after.ll.x, before.ll.x + dx);
        assert_eq!(after.ll.y, before.ll.y + dy);
        assert_eq!(after.ur.x - after.ll.x, before.ur.x - before.ll.x);
        assert_eq!(after.ur.y - after.ll.y, before.ur.y - before.ll.y);
        assert_eq!(
            board.components.get(component).unwrap().exact_location(),
            source
        );
    }
}

#[test]
fn copy_preview_follows_cursor_without_moving_source_pin() {
    copy_preview_case(false);
}

#[test]
fn copy_preview_follows_cursor_without_moving_source_keepout() {
    copy_preview_case(true);
}

// ---------------------------------------------------------------------------------------------
// Behaviour of the editor commands
// ---------------------------------------------------------------------------------------------

fn bm05() -> RoutingBoard {
    board_probe::load(&board_probe::fixture("Issue508-DAC2020_bm05"))
}

/// The pin picked by the committed script, and the location it is picked at.
const SCRIPT_PIN: ItemId = ItemId(26);
const SCRIPT_AT: FloatPoint = FloatPoint {
    x: 1575911.0,
    y: -1067036.0,
};

fn editor(board: RoutingBoard) -> Editor {
    let settings = EditSettings::for_board(&board);
    let mut editor = Editor::new(board, settings);
    editor.apply(Command::SetTool(Tool::Select));
    editor
}

fn selected(editor: &mut Editor) {
    let response = editor.apply(Command::SelectAt(SCRIPT_AT));
    assert_eq!(response.mode, ModeKind::Selecting);
    assert!(editor.selection().contains(&SCRIPT_PIN));
}

/// An item's text without its id.
fn without_id(text: &str) -> &str {
    text.split_once(' ').map_or(text, |(_, rest)| rest)
}

#[test]
fn four_quarter_turns_return_the_selection_to_its_placement() {
    let board = bm05();
    let components_before = transform_script::components_text(&board);
    let originals: BTreeSet<String> = board
        .component_items(board.item(SCRIPT_PIN).unwrap().component_id())
        .into_iter()
        .map(|id| transform_script::item_text(board.item(id).unwrap()))
        .collect();
    let mut editor = editor(board);
    selected(&mut editor);
    let start = editor.apply(Command::MoveSelection { to: SCRIPT_AT });
    assert_eq!(start.mode, ModeKind::Moving);
    assert!(start.overlay.selection.is_empty());
    let floating = start.overlay.floating.clone();
    assert!(!floating.is_empty());
    let mut turned = start;
    for _ in 0..4 {
        turned = editor.apply(Command::RotateSelection { degrees: 90.0 });
        assert_eq!(turned.status, None);
        assert_eq!(turned.mode, ModeKind::Moving);
    }
    // the same geometry and ids (a transform keeps the ids of the floating copies)
    assert_eq!(turned.overlay.floating, floating);
    assert_eq!(
        transform_script::components_text(editor.board()),
        components_before
    );
    // completing inserts copies that equal the originals but for their ids
    let done = editor.apply(Command::Complete);
    assert_eq!(done.status.unwrap().key, "move_completed");
    assert!(editor.board().item(SCRIPT_PIN).is_none());
    let board = editor.board();
    let moved: BTreeSet<String> = done
        .delta
        .inserted
        .iter()
        .map(|id| without_id(&transform_script::item_text(board.item(*id).unwrap())).to_string())
        .collect();
    let originals: BTreeSet<String> = originals
        .iter()
        .map(|text| without_id(text).to_string())
        .collect();
    assert_eq!(moved, originals);
    // undo restores the original items and components
    let undone = editor.apply(Command::Undo);
    assert_eq!(undone.status.unwrap().key, "undo");
    assert_eq!(
        transform_script::components_text(editor.board()),
        components_before
    );
    assert!(editor.board().item(SCRIPT_PIN).is_some());
}

/// Rotating a component by 90 degrees four times leaves no trace: a cancelled move with the four
/// turns hashes like one without (`state_hash`), and the components keep their pose.
#[test]
fn four_quarter_turns_leave_the_board_state_unchanged() {
    let run = |turns: usize| {
        let mut editor = editor(bm05());
        selected(&mut editor);
        editor.apply(Command::MoveSelection { to: SCRIPT_AT });
        for _ in 0..turns {
            let turned = editor.apply(Command::RotateSelection { degrees: 90.0 });
            assert_eq!(turned.status, None);
        }
        editor.apply(Command::Cancel);
        (
            editor.board().state_hash(),
            transform_script::components_text(editor.board()),
            transform_script::items_hash(editor.board()),
        )
    };
    assert_eq!(run(4), run(0));
}

#[test]
fn cancel_restores_the_board_like_an_undone_snapshot() {
    let reference = {
        let mut board = bm05();
        // what a cancelled move does to the undo structure: a snapshot, the detached items'
        // deletions and the undo
        board.generate_snapshot();
        for id in board.component_items(board.item(SCRIPT_PIN).unwrap().component_id()) {
            board.detach_item_for_move(id).unwrap();
        }
        board.undo(None);
        board.state_hash()
    };
    let mut editor = editor(bm05());
    selected(&mut editor);
    editor.apply(Command::MoveSelection {
        to: FloatPoint::new(SCRIPT_AT.x + 700.0, SCRIPT_AT.y - 300.0),
    });
    editor.apply(Command::RotateSelection { degrees: 45.0 });
    let cancelled = editor.apply(Command::Cancel);
    assert_eq!(cancelled.mode, ModeKind::Menu(Tool::Select));
    assert!(cancelled.overlay.floating.is_empty());
    assert!(editor.board().item(SCRIPT_PIN).is_some());
    assert_eq!(editor.board().state_hash(), reference);
}

#[test]
fn a_turn_or_flip_without_a_move_starts_the_session() {
    for command in [
        Command::RotateSelection { degrees: 90.0 },
        Command::MirrorSelection,
        Command::ResetRotation,
    ] {
        let mut editor = editor(bm05());
        selected(&mut editor);
        let response = editor.apply(command.clone());
        assert_eq!(response.mode, ModeKind::Moving, "{command:?}");
        assert!(!response.overlay.floating.is_empty());
        let cancelled = editor.apply(Command::Cancel);
        assert_eq!(cancelled.mode, ModeKind::Menu(Tool::Select));
    }
}

#[test]
fn a_move_onto_another_net_cannot_be_completed() {
    let board = bm05();
    let picked = board.item(SCRIPT_PIN).unwrap();
    let other = board
        .pins()
        .into_iter()
        .map(|id| board.item(id).unwrap())
        .find(|p| {
            p.component_id() != picked.component_id()
                && !p.net_numbers().is_empty()
                && p.net_numbers() != picked.net_numbers()
        })
        .expect("a pin of another net")
        .drill()
        .unwrap()
        .center;
    let mut editor = editor(board);
    selected(&mut editor);
    let moved = editor.apply(Command::MoveSelection {
        to: other.to_float(),
    });
    assert!(!moved.overlay.floating_violations.is_empty());
    let blocked = editor.apply(Command::Complete);
    assert_eq!(
        blocked.status.unwrap().key,
        "insertion_failed_because_of_obstacles"
    );
    assert_eq!(blocked.mode, ModeKind::Moving);
    assert!(editor.board().item(SCRIPT_PIN).is_none(), "still floating");
    // a click puts it down the same way; moving back to a free place completes
    let back = editor.apply(Command::Move(SCRIPT_AT));
    assert!(back.overlay.floating_violations.is_empty());
    let done = editor.apply(Command::Click(SCRIPT_AT));
    assert_eq!(done.status.unwrap().key, "move_completed");
    assert_eq!(done.mode, ModeKind::Menu(Tool::Select));
}

#[test]
fn a_refused_move_adds_the_blocking_items_to_the_selection() {
    let row = board_probe::fixture("Issue026-J2_reference");
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    let pin = board.pins()[0];
    let at = centre(&board, pin).to_float();
    let mut editor = editor(board);
    editor.apply(Command::SelectAt(at));
    let before = editor.selection().len();
    let response = editor.apply(Command::MoveSelection { to: at });
    assert_eq!(response.mode, ModeKind::Selecting);
    let key = response.status.unwrap().key;
    assert!(
        key == "please_unfix_selected_items_before_moving"
            || key == "please_unroute_or_extend_selection_before_moving",
        "{key}"
    );
    assert!(editor.selection().len() > before);
    assert!(response.delta.inserted.is_empty() && response.delta.removed.is_empty());
}

/// `CopyItemState` takes an id for each preview item, in selection order (highest source id
/// first), and one more for each item it inserts, in the same order; blocked items take none.
#[test]
fn copy_burns_ids_in_java_order() {
    let mut checked = 0;
    for (dx, dy) in [(20000.0, 20000.0), (-40000.0, 5000.0), (60000.0, -30000.0)] {
        let mut board = bm05();
        let before = i64::from(board.communication.id_generator.max_generated_id());
        let pins = board.pins();
        let selection: BTreeSet<ItemId> = pins.iter().take(3).copied().collect();
        let sources: Vec<ItemId> = selection.iter().copied().collect();
        assert_eq!(sources, pins[..3], "the selection is highest id first");
        let at = centre(&board, sources[0]).to_float();
        let mut session = CopySession::start(&mut board, &selection, 0, at).unwrap();
        let preview: Vec<i32> = session.items().iter().map(|i| i.id().0).collect();
        let first = before as i32 + 1;
        assert_eq!(preview, vec![first, first + 1, first + 2]);
        session.change_position(&board, FloatPoint::new(at.x + dx, at.y + dy));
        let outcome = session.insert(&mut board).unwrap();
        let inserted: Vec<i32> = outcome.inserted.iter().map(|i| i.0).collect();
        // one id per inserted item, in preview order; a blocked item takes none
        let expected: Vec<i32> = (first + 3..first + 3 + inserted.len() as i32).collect();
        assert_eq!(inserted, expected);
        assert_eq!(
            i64::from(board.communication.id_generator.max_generated_id()),
            before + 3 + inserted.len() as i64
        );
        if inserted.len() == 3 {
            checked += 1;
        }
    }
    assert!(checked > 0, "a place where all three copies fit");
}

#[test]
fn a_layer_change_gives_copied_pins_shifted_padstacks() {
    let mut editor = editor(bm05());
    selected(&mut editor);
    let board = editor.board();
    let padstack = board.item(SCRIPT_PIN).unwrap().drill().unwrap().padstack;
    let from = board.library.padstacks.get(padstack).unwrap().from_layer();
    let layers = board.layer_count() as i32;
    let target = if from + 1 < layers { from + 1 } else { 0 };
    let padstacks = board.library.padstacks.count();
    let packages = board.library.packages.count();
    editor.apply(Command::CopySelection {
        to: FloatPoint::new(SCRIPT_AT.x + 30000.0, SCRIPT_AT.y),
    });
    let changed = editor.apply(Command::SetLayer(target));
    assert_eq!(changed.layer, target);
    let inserted = editor.apply(Command::Click(FloatPoint::new(
        SCRIPT_AT.x + 30000.0,
        SCRIPT_AT.y,
    )));
    assert_eq!(inserted.delta.inserted.len(), 1, "{:?}", inserted.status);
    let board = editor.board();
    assert!(board.library.padstacks.count() > padstacks);
    assert!(board.library.packages.count() > packages);
    let copy = board.item(inserted.delta.inserted[0]).unwrap();
    let new = board
        .library
        .padstacks
        .get(copy.drill().unwrap().padstack)
        .unwrap();
    assert_eq!(new.from_layer(), target);
}

#[test]
fn the_edit_script_replays_identically() {
    let path =
        board_probe::repo_root().join("crates/freerouting/tests/data/edit_script_i04_bm05.json");
    let text = std::fs::read_to_string(path).expect("script");
    assert!(editor_script::is_editor_script(&text));
    let board = bm05();
    let script = editor_script::parse_editor_script(&text, board.layer_count()).expect("parse");
    assert_eq!(script.commands.len(), 22);
    let options = freerouting::interactive::EditorOptions::default();
    let first = editor_script::replay_editor(board.clone(), &script, options.clone());
    let again = editor_script::replay_editor(board, &script, options);
    assert_eq!(first, again);
    let has = |cmd: &str, needle: &str| {
        first
            .iter()
            .any(|l| l.contains(&format!("\"cmd\":\"{cmd}\"")) && l.contains(needle))
    };
    assert!(has("move_selection", "\"mode\":\"moving\""));
    assert!(has("complete", "move_completed"));
    assert!(has("copy_selection", "\"mode\":\"copying\""));
    assert!(has("click", "allItemsInserted"));
    assert!(has("undo", "\"status\":\"undo\""));
}

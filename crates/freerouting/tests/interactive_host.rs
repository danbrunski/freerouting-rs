//! The editor on a board the host keeps (`Editor::detached`, `Editor::apply_on`) and the
//! `SetSelection` command: what the GUI relies on (G09). The expected values come from running the
//! same commands on an editor that owns its board.

mod board_probe;

use std::collections::BTreeSet;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::geometry::FloatPoint;
use freerouting::interactive::{Command, EditSettings, Editor, ModeKind, RouteSession, Tool};

fn j2() -> RoutingBoard {
    board_probe::load(&board_probe::fixture("Issue026-J2_reference"))
}

/// A pin of J2 with a target pin: (start, target) centres.
fn start_and_target(board: &RoutingBoard) -> (FloatPoint, FloatPoint) {
    let settings = EditSettings::for_board(board);
    let pins: Vec<FloatPoint> = board
        .items()
        .filter(|i| i.is_pin() && i.net_count() == 1)
        .map(|i| i.drill().expect("pin").center.to_float())
        .collect();
    for at in pins {
        let mut copy = board.clone();
        let Ok((session, _)) = RouteSession::start(&mut copy, &settings, at) else {
            continue;
        };
        let target = session
            .target_set()
            .iter()
            .filter_map(|&id| copy.item(id))
            .find(|i| i.is_pin())
            .map(|i| i.drill().expect("pin").center.to_float());
        if let Some(target) = target {
            return (at, target);
        }
    }
    panic!("no pin with a target");
}

fn item_ids(board: &RoutingBoard) -> BTreeSet<ItemId> {
    board.items().map(|i| i.id()).collect()
}

/// The same pointer script on an owning editor and on a detached one that borrows the board:
/// the same responses and the same board, and the sessions go on across the commands.
#[test]
fn apply_on_matches_an_owning_editor() {
    let board = j2();
    let (start, target) = start_and_target(&board);
    let script = [
        Command::Click(start),
        Command::Move(FloatPoint::new(
            start.x + (target.x - start.x) / 2.0,
            start.y + (target.y - start.y) / 2.0,
        )),
        Command::Move(target),
        Command::Complete,
    ];
    let settings = EditSettings::for_board(&board);

    let mut owning = Editor::new(board.clone(), settings.clone());
    let owned: Vec<_> = script.iter().cloned().map(|c| owning.apply(c)).collect();

    let mut lent = board.clone();
    let mut detached = Editor::detached(settings);
    let borrowed: Vec<_> = script
        .iter()
        .cloned()
        .map(|c| detached.apply_on(&mut lent, c))
        .collect();

    assert_eq!(owned, borrowed);
    assert!(matches!(
        borrowed[0].mode,
        ModeKind::Routing { stitch: false }
    ));
    assert!(
        borrowed[0]
            .status
            .as_ref()
            .is_some_and(|s| s.key == "routing_net_message")
    );
    assert_eq!(item_ids(owning.board()), item_ids(&lent));
    assert!(
        lent.items().count() > board.items().count(),
        "a trace was added"
    );
    // the detached editor's own board is the placeholder again
    assert_eq!(detached.board().items().filter(|i| i.is_pin()).count(), 0);
}

/// An undo through the lent board restores it; a selection set by the host makes the editor
/// refuse the undo (Java allows history in a menu state only), so the host clears it first.
#[test]
fn set_selection_enters_and_leaves_the_inspected_state() {
    let mut board = j2();
    let (start, target) = start_and_target(&board);
    let mut editor = Editor::detached(EditSettings::for_board(&board));
    let before = item_ids(&board);
    editor.apply_on(&mut board, Command::Click(start));
    editor.apply_on(&mut board, Command::Move(target));
    editor.apply_on(&mut board, Command::Complete);
    assert_eq!(editor.mode(), ModeKind::Menu(Tool::Route));
    assert!(item_ids(&board) != before);

    let pin = board.items().find(|i| i.is_pin()).expect("pin").id();
    let r = editor.apply_on(&mut board, Command::SetSelection([pin].into()));
    assert_eq!(r.mode, ModeKind::Selecting);
    assert_eq!(r.overlay.selection, vec![pin]);
    // refused with a selection, as in Java ...
    let r = editor.apply_on(&mut board, Command::Undo);
    assert!(r.status.is_none() && item_ids(&board) != before);
    // ... and done once the host has cleared it
    editor.apply_on(&mut board, Command::SetSelection(BTreeSet::new()));
    let r = editor.apply_on(&mut board, Command::Undo);
    assert_eq!(r.status.expect("status").key, "undo");
    assert_eq!(item_ids(&board), before);
    assert!(!r.delta.is_empty());
}

/// A session in progress ignores `SetSelection`.
#[test]
fn set_selection_is_ignored_during_a_route() {
    let mut board = j2();
    let (start, _) = start_and_target(&board);
    let mut editor = Editor::detached(EditSettings::for_board(&board));
    editor.apply_on(&mut board, Command::Click(start));
    let pin = board.items().find(|i| i.is_pin()).expect("pin").id();
    let r = editor.apply_on(&mut board, Command::SetSelection([pin].into()));
    assert!(matches!(r.mode, ModeKind::Routing { .. }));
    assert!(r.overlay.selection.is_empty());
}

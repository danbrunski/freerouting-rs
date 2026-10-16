//! The autoroute change log (B02.8): one test per Java hook point, each with
//! the sequence the cited Java lines produce.

use freerouting::board::items::ItemId;
use freerouting::board::structure::{FixedState, Layer, LayerStructure};
use freerouting::board::{AutorouteChange, ItemSideTable, LineReuse, RoutingBoard};
use freerouting::geometry::{IntBox, IntPoint, Point, Polyline, PolylineShape, TileShape};
use freerouting::rules::{BoardRules, ClearanceMatrix};

use AutorouteChange::*;

fn board() -> RoutingBoard {
    let layers = LayerStructure::new(vec![Layer::new("Top", true)]);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    let outline = IntBox::new(0, 0, 2_000_000, 2_000_000);
    let mut board = RoutingBoard::new(
        outline,
        layers,
        vec![PolylineShape::Tile(TileShape::from(outline))],
        0,
        rules,
        Default::default(),
    );
    let class = board.rules.default_net_class();
    board.rules.nets.add("A", 1, false, class);
    board.set_autoroute_change_log(true);
    board
}

fn trace(board: &mut RoutingBoard, corners: &[(i32, i32)]) -> ItemId {
    let points: Vec<Point> = corners
        .iter()
        .map(|&(x, y)| Point::Int(IntPoint::new(x, y)))
        .collect();
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&points),
            0,
            1_000,
            &[1],
            1,
            FixedState::Unfixed,
        )
        .expect("trace")
}

/// `BoardItemRepository.insertItem` (`:162-165`): after the tree insert.
#[test]
fn insert_is_logged() {
    let mut board = board();
    let id = trace(&mut board, &[(10_000, 10_000), (20_000, 10_000)]);
    assert_eq!(board.take_autoroute_changes(), vec![Inserted(id)]);
}

/// `BoardItemRepository.removeItem` (`:206-207`): before the tree remove.
#[test]
fn remove_is_logged() {
    let mut board = board();
    let id = trace(&mut board, &[(10_000, 10_000), (20_000, 10_000)]);
    board.take_autoroute_changes();
    assert!(board.remove_item(id));
    assert_eq!(board.take_autoroute_changes(), vec![Removed(id)]);
}

/// `PolylineTrace.combine` (`:189`): the other trace is removed inside
/// `combineAtEnd`, then the combine loop updates `this`.
#[test]
fn combine_is_logged_once_per_combine() {
    let mut board = board();
    let first = trace(&mut board, &[(10_000, 10_000), (20_000, 10_000)]);
    let second = trace(&mut board, &[(20_000, 10_000), (20_000, 30_000)]);
    board.take_autoroute_changes();
    assert!(board.combine_trace(first));
    assert_eq!(
        board.take_autoroute_changes(),
        vec![Removed(second), Combined(first)]
    );
}

/// `PolylineTrace.split(IntOctagon)` (`:687-691`): each piece, when there
/// is more than one, in the order of the result.
#[test]
fn split_pieces_are_logged() {
    let mut board = board();
    let crossed = trace(&mut board, &[(0, 100_000), (200_000, 100_000)]);
    trace(&mut board, &[(100_000, 100_000), (100_000, 300_000)]);
    board.take_autoroute_changes();
    let pieces = board.split_trace(crossed, None);
    assert!(pieces.len() > 1, "{pieces:?}");
    let log = board.take_autoroute_changes();
    let tail: Vec<_> = log[log.len() - pieces.len()..].to_vec();
    let expected: Vec<_> = pieces.iter().map(|p| SplitPiece(*p)).collect();
    assert_eq!(tail, expected, "{log:?}");

    // A trace that is not split logs no piece.
    let lone = trace(&mut board, &[(500_000, 500_000), (600_000, 500_000)]);
    board.take_autoroute_changes();
    assert_eq!(board.split_trace(lone, None), vec![lone]);
    assert!(board.take_autoroute_changes().is_empty());
}

/// `PolylineTrace.change` (`:944`): before `saveForUndo`, so also when
/// nothing changes.
#[test]
fn change_is_logged_first() {
    let mut board = board();
    let id = trace(
        &mut board,
        &[(10_000, 10_000), (20_000, 10_000), (20_000, 20_000)],
    );
    board.take_autoroute_changes();
    let new = Polyline::from_points(&[
        Point::Int(IntPoint::new(10_000, 10_000)),
        Point::Int(IntPoint::new(25_000, 10_000)),
        Point::Int(IntPoint::new(25_000, 20_000)),
    ]);
    board.change_trace(id, new, LineReuse::NONE);
    let log = board.take_autoroute_changes();
    assert_eq!(log.first(), Some(&Changed(id)), "{log:?}");

    let same = board.item(id).unwrap().as_trace().unwrap().polyline.clone();
    let all = same.lines().len();
    board.change_trace(
        id,
        same,
        LineReuse {
            prefix: all,
            suffix: all,
        },
    );
    assert_eq!(board.take_autoroute_changes(), vec![Changed(id)]);
}

/// `ShapeTraceEntries.fastCutoutTrace` (`:113`): O03 calls the hook.
#[test]
fn cutout_is_logged() {
    let mut board = board();
    let id = trace(&mut board, &[(10_000, 10_000), (20_000, 10_000)]);
    board.take_autoroute_changes();
    board.note_autoroute_cutout(id);
    assert_eq!(board.take_autoroute_changes(), vec![CutOut(id)]);
}

/// `BasicBoard.applyUndoRedoSideEffects`: each restored item's autoroute
/// info is cleared after its tree insert.
#[test]
fn undo_and_redo_restores_clear_autoroute_info() {
    let mut board = board();
    let id = trace(&mut board, &[(10_000, 10_000), (20_000, 10_000)]);
    board.generate_snapshot();
    assert!(board.remove_item(id));
    board.take_autoroute_changes();
    assert!(board.undo(None));
    assert_eq!(board.take_autoroute_changes(), vec![InfoCleared(id)]);

    let mut board = board_with_snapshot_then_insert();
    let id = *board.traces().first().expect("trace");
    assert!(board.undo(None));
    board.take_autoroute_changes();
    assert!(board.redo(None));
    assert_eq!(board.take_autoroute_changes(), vec![InfoCleared(id)]);
}

fn board_with_snapshot_then_insert() -> RoutingBoard {
    let mut board = board();
    board.generate_snapshot();
    trace(&mut board, &[(10_000, 10_000), (20_000, 10_000)]);
    board
}

/// `SearchTreeManager.reinsertTreeItems` (`:186-232`): every item's derived
/// data is cleared before the reinsertion, then again in
/// `insertAllBoardItems`, in item-list order.
#[test]
fn reinsert_tree_items_clears_every_item_twice() {
    let mut board = board();
    trace(&mut board, &[(10_000, 10_000), (20_000, 10_000)]);
    trace(&mut board, &[(50_000, 10_000), (60_000, 10_000)]);
    board.take_autoroute_changes();
    board.reinsert_tree_items();
    let ids = board.item_ids();
    let expected: Vec<_> = ids
        .iter()
        .chain(ids.iter())
        .map(|id| InfoCleared(*id))
        .collect();
    assert_eq!(board.take_autoroute_changes(), expected);
}

#[test]
fn log_off_records_nothing() {
    let mut board = board();
    board.set_autoroute_change_log(false);
    let id = trace(&mut board, &[(10_000, 10_000), (20_000, 10_000)]);
    board.remove_item(id);
    assert!(!board.is_autoroute_change_log_on());
    assert!(board.take_autoroute_changes().is_empty());
}

#[test]
fn side_table_clears_in_one_step() {
    let mut table: ItemSideTable<u32> = ItemSideTable::new();
    table.insert(ItemId(5), 50);
    table.insert(ItemId(2), 20);
    assert_eq!(table.get(ItemId(5)), Some(&50));
    assert_eq!(table.remove(ItemId(2)), Some(20));
    assert!(!table.contains(ItemId(2)));
    table.clear();
    assert!(table.get(ItemId(5)).is_none());
    *table.get_or_insert_with(ItemId(5), || 7).unwrap() += 1;
    assert_eq!(table.get(ItemId(5)), Some(&8));
    assert!(table.get(ItemId(-1)).is_none());
}

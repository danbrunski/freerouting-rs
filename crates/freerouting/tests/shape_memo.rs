//! P02.5: the trace-shape memo (`board::search::memo`). Trees built from
//! the memo (autoroute trees, restores) are the trees built without it,
//! leaf for leaf; a board's restores share its memo.

mod board_probe;

use std::sync::Arc;

use freerouting::board::RoutingBoard;
use freerouting::board::search::{SearchTreeManager, ShapeMemo, ShapeSearchTree, TreeKind};

fn j2_with_session() -> RoutingBoard {
    let row = board_probe::fixture("Issue026-J2_reference");
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row).expect("J2 has a session");
    board
}

/// The tree's walk (every node with its bounds, leaves with their item and
/// shape index) and its stored shapes, item by item.
fn tree_state(tree: &ShapeSearchTree, board: &RoutingBoard) -> (Vec<String>, Vec<String>) {
    let shapes = board
        .items()
        .map(|item| format!("{:?}", tree.tree_shapes(item, &board.shape_context())))
        .collect();
    (tree.walk_lines(), shapes)
}

/// A tree of `kind` and `class` built from `board`'s items without a memo.
fn uncached(board: &RoutingBoard, kind: TreeKind, class: i32) -> ShapeSearchTree {
    let mut tree = ShapeSearchTree::new(kind, class);
    for item in board.items() {
        tree.insert(item, &board.shape_context());
    }
    tree
}

#[test]
fn memoized_trees_equal_uncached_builds() {
    let board = j2_with_session();
    let traces = board.items().filter(|i| i.is_trace()).count();
    assert!(traces > 0);
    let ctx = board.shape_context();
    let class = board.rules.clearance_matrix.class_count() as i32 - 1;
    assert!(class > 0, "a compensated tree needs a class above 0");
    let memo = Arc::new(ShapeMemo::new());
    // The first manager's autoroute tree fills the memo, the second's reads
    // it. Single insertions (the default tree here) do not use it.
    for round in 0..2 {
        let mut trees = SearchTreeManager::with_memo(memo.clone());
        for item in board.items() {
            trees.insert(item, &ctx);
        }
        let expected = tree_state(&uncached(&board, TreeKind::Plain, 0), &board);
        assert_eq!(
            tree_state(trees.default_tree(), &board),
            expected,
            "round {round}"
        );
        let compensated = trees.get_autoroute_tree(class, board.item_list(), &ctx);
        let kind = compensated.kind();
        assert_eq!(
            tree_state(compensated, &board),
            tree_state(&uncached(&board, kind, class), &board),
            "round {round}, class {class}"
        );
    }
    let counts = memo.counts();
    assert_eq!(counts.misses as usize, traces, "{counts:?}");
    assert_eq!(counts.hits as usize, traces, "{counts:?}");
    assert_eq!(counts.evictions, 0);
}

#[test]
fn restores_share_the_board_memo() {
    let board = j2_with_session();
    let memo = board.search_trees().shape_memo().clone();
    let before = memo.counts();
    let snapshot = board.snapshot();
    let restored = snapshot.restore();
    assert!(Arc::ptr_eq(restored.search_trees().shape_memo(), &memo));
    let traces = board.items().filter(|i| i.is_trace()).count() as u64;
    // The import inserted the traces one by one, past the memo; the first
    // restore's build fills it, a rebuild finds every trace.
    let after = memo.counts();
    assert_eq!(after.misses - before.misses, traces);
    assert_eq!(after.hits, before.hits);
    let rebuilt = snapshot.rebuild();
    assert_eq!(memo.counts().hits - after.hits, traces);
    assert_eq!(
        tree_state(rebuilt.search_trees().default_tree(), &rebuilt),
        tree_state(restored.search_trees().default_tree(), &restored)
    );
    assert_eq!(
        tree_state(restored.search_trees().default_tree(), &restored),
        tree_state(&uncached(&board, TreeKind::Plain, 0), &board)
    );
}

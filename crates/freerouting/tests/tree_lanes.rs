//! P05.1: the search tree's overlap queries on the lane arrays give the
//! result sets and work counts of the scalar traversal they replace.
//!
//! Per fixture of `tools/java-reference/probes/board/fixtures.tsv` (a debug
//! build runs the boards of `DEBUG_BOARDS`, release runs all; the
//! `TREE_PROBE_BOARDS` and `BOARD_PROBE_ALL` variables of `board_probe`
//! choose others), every search tree of the loaded board and its autoroute
//! trees (45 degree octagon bounds), and an orthogonal tree built from the
//! default tree's leaves (box bounds), answer a fixed query sample: each
//! sampled leaf's bound, the bound grown by a margin and shifted, a point of
//! it, and seeded boxes or octagons over the root's extent. For each query
//! `ShapeTree::overlaps` (the lane path) must return the leaves
//! `ShapeTree::overlaps_reference` (the scalar path) returns, and add the
//! node count the scalar path pops to the work meter.

mod board_probe;

use std::collections::BTreeSet;

use freerouting::board::RoutingBoard;
use freerouting::board::search::WorkUnit;
use freerouting::datastructures::ShapeTree;
use freerouting_geometry::jmath::JavaRandom;
use freerouting_geometry::{IntBox, IntOctagon, RegularTileShape, ShapeBoundingDirections};

/// The boards a debug build runs (the workspace gate); release runs all.
const DEBUG_BOARDS: &[&str] = &[
    "Issue145-smoothieboard",
    "Issue413-test",
    "Issue433-my-board",
    "Issue508-DAC2020_bm08",
];

/// Leaves sampled per tree (every n-th leaf, so about this many).
const SAMPLED_LEAVES: usize = 400;

/// Seeded queries over the root's extent per tree.
const SPREAD_QUERIES: usize = 400;

/// The distinct trace clearance classes of the net classes, ascending.
fn trace_clearance_classes(board: &RoutingBoard) -> BTreeSet<i32> {
    let classes = &board.rules.net_classes;
    (0..classes.count())
        .filter_map(|i| classes.nth(i))
        .map(|id| classes.get(id).trace_clearance_class())
        .collect()
}

/// `bound` grown by `margin` on every side and moved by `(dx, dy)`.
fn moved(bound: &RegularTileShape, margin: i32, dx: i32, dy: i32) -> RegularTileShape {
    match bound {
        RegularTileShape::IntBox(b) => RegularTileShape::IntBox(IntBox::new(
            b.ll.x - margin + dx,
            b.ll.y - margin + dy,
            b.ur.x + margin + dx,
            b.ur.y + margin + dy,
        )),
        RegularTileShape::IntOctagon(o) => RegularTileShape::IntOctagon(IntOctagon::new(
            o.left_x - margin + dx,
            o.bottom_y - margin + dy,
            o.right_x + margin + dx,
            o.top_y + margin + dy,
            o.upper_left_diagonal_x - 2 * margin + dx - dy,
            o.lower_right_diagonal_x + 2 * margin + dx - dy,
            o.lower_left_diagonal_x - 2 * margin + dx + dy,
            o.upper_right_diagonal_x + 2 * margin + dx + dy,
        )),
    }
}

/// The bounding box of a bound.
fn bounding_box(bound: &RegularTileShape) -> IntBox {
    match bound {
        RegularTileShape::IntBox(b) => *b,
        RegularTileShape::IntOctagon(o) => {
            match ShapeBoundingDirections::Orthogonal.bounds_octagon(o) {
                RegularTileShape::IntBox(b) => b,
                RegularTileShape::IntOctagon(_) => unreachable!("orthogonal bounds are boxes"),
            }
        }
    }
}

/// A point of `bound`'s lower left corner, as a bound of the same kind.
fn corner(bound: &RegularTileShape) -> RegularTileShape {
    let (x, y) = match bound {
        RegularTileShape::IntBox(b) => (b.ll.x, b.ll.y),
        RegularTileShape::IntOctagon(o) => (o.left_x, o.bottom_y),
    };
    let point = IntBox::new(x, y, x, y);
    match bound {
        RegularTileShape::IntBox(_) => RegularTileShape::IntBox(point),
        RegularTileShape::IntOctagon(_) => RegularTileShape::IntOctagon(point.to_int_octagon()),
    }
}

/// The query sample of `tree`.
fn queries<K: Ord + Copy>(tree: &ShapeTree<K>, random: &mut JavaRandom) -> Vec<RegularTileShape> {
    let leaves = tree.to_array();
    let Some(&first) = leaves.first() else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let step = leaves.len().div_ceil(SAMPLED_LEAVES).max(1);
    let mut extent = IntBox::EMPTY;
    for (i, &leaf) in leaves.iter().enumerate() {
        let bound = tree.leaf_bounds(leaf);
        let b = bounding_box(&bound);
        extent = extent.union_box(&b);
        if i % step == 0 {
            let margin = random.next_int(2000);
            let dx = random.next_int(4001) - 2000;
            let dy = random.next_int(4001) - 2000;
            result.push(bound);
            result.push(moved(&bound, margin, dx, dy));
            result.push(corner(&bound));
        }
    }
    let kind = tree.leaf_bounds(first);
    let (w, h) = (
        i64::from(extent.ur.x) - i64::from(extent.ll.x),
        i64::from(extent.ur.y) - i64::from(extent.ll.y),
    );
    let w = w.clamp(1, i64::from(i32::MAX / 2)) as i32;
    let h = h.clamp(1, i64::from(i32::MAX / 2)) as i32;
    for _ in 0..SPREAD_QUERIES {
        let x = extent.ll.x + random.next_int(w);
        let y = extent.ll.y + random.next_int(h);
        let size = random.next_int((w / 10).max(1));
        let b = IntBox::new(x, y, x + size, y + size);
        result.push(match kind {
            RegularTileShape::IntBox(_) => RegularTileShape::IntBox(b),
            RegularTileShape::IntOctagon(_) => RegularTileShape::IntOctagon(b.to_int_octagon()),
        });
    }
    result
}

/// Every query of the sample: the lane path equals the scalar path. Returns
/// (queries, queries with a hit).
fn check_tree<K: Ord + Copy + std::fmt::Debug>(
    board: &str,
    name: &str,
    tree: &ShapeTree<K>,
    random: &mut JavaRandom,
) -> (usize, usize) {
    let mut hits = 0;
    let sample = queries(tree, random);
    for query in &sample {
        let (expected, popped): (Vec<_>, u64) = tree.overlaps_reference(query);
        let before = tree.meter().units();
        let found = tree.overlaps(query);
        let counted = tree.meter().units() - before;
        assert_eq!(found, expected, "{board} {name}: leaves of {query:?}");
        let units = match tree.meter().unit() {
            WorkUnit::Nodes => popped,
            WorkUnit::Leaves => expected.len() as u64,
        };
        assert_eq!(counted, units, "{board} {name}: work of {query:?}");
        hits += usize::from(!found.is_empty());
    }
    (sample.len(), hits)
}

/// The orthogonal tree of the default tree's leaves (their bounding boxes).
fn box_tree(board: &RoutingBoard) -> ShapeTree<u32> {
    let source = board.search_trees().default_tree().shape_tree();
    let mut tree = ShapeTree::new(ShapeBoundingDirections::Orthogonal);
    for (i, leaf) in source.to_array().into_iter().enumerate() {
        let b = bounding_box(&source.leaf_bounds(leaf));
        tree.insert(i as u32, 0, RegularTileShape::IntBox(b));
    }
    tree
}

#[test]
fn lane_queries_equal_scalar_queries_on_fixtures() {
    let only = board_probe::board_filter(DEBUG_BOARDS);
    let mut boards = 0;
    let (mut total, mut hits) = (0, 0);
    for row in board_probe::fixtures() {
        if only.as_ref().is_some_and(|o| !o.contains(&row.board)) {
            continue;
        }
        let mut board = board_probe::load(&row);
        for class in trace_clearance_classes(&board) {
            board.autoroute_search_tree(class);
        }
        let mut random = JavaRandom::new(1105);
        for (i, tree) in board.search_trees().trees().iter().enumerate() {
            let (n, h) = check_tree(
                &row.board,
                &format!("tree {i}"),
                tree.shape_tree(),
                &mut random,
            );
            total += n;
            hits += h;
        }
        let boxes = box_tree(&board);
        let (n, h) = check_tree(&row.board, "box tree", &boxes, &mut random);
        total += n;
        hits += h;
        boards += 1;
    }
    eprintln!("tree_lanes: {boards} boards, {total} queries, {hits} with a hit");
    assert!(
        boards >= DEBUG_BOARDS.len().min(1),
        "no fixture was checked"
    );
    assert!(hits * 4 > total, "too few queries hit: {hits} of {total}");
}

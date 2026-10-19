//! N06 follow-up (decided 2026-10-18, option (b) of N06.3's audit): every `Area`
//! split of the board follows the board's cutout switches
//! (`router.java_quirks.octagon_box_cutout_branch3`, `cutout_hole_crosses_border`,
//! `simplex_octagon_half_corner` and the inert `simplex_prev_division_line`), not only
//! the drill pages and the tie-pin cutout.
//!
//! Each site gets both behaviours on a shape where one of the three cases occurs
//! (the shapes of the geometry crate's cutout tests):
//! - branch 3: the box (0, 0, 100, 100) minus the octagon
//!   (40, 10, 60, 30, 10, 50, 50, 80), `box_cutout_branch3_switch_both_behaviours`;
//! - the border: a triangle crossing the bottom and left sides of a box,
//!   `java_breaks_the_invariant_outside_the_two_rows`;
//! - the half corner: an odd-parity octagon inside a pentagon, the same test.
//!
//! The sites: the search trees' area shapes and the outline keepout's shapes and
//! piece count (`board/search/mod.rs`, `board/items/mod.rs`), `check_shape`
//! (`board/queries.rs`), `overlapping_items` (`board/basic_board.rs`), the
//! interactive hole (`interactive/construct.rs`), and the switch change after
//! insertion (`board/split_switches.rs`). The spring-over shape, the plane
//! adjustment and the route's nearest target point have unit tests in their modules.

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::{FixedState, Layer, LayerStructure};
use freerouting::geometry::shapes::{Area, PolylineArea, PolylineShape};
use freerouting::geometry::{
    FloatPoint, IntBox, IntOctagon, IntPoint, Line, PolylineShapeOps, Simplex, TileShape,
    TileShapeOps, Vector,
};
use freerouting::io::specctra::{BoardReadResult, build_board_with};
use freerouting::rules::{BoardRules, ClearanceMatrix};
use freerouting::settings::JavaQuirks;

/// The four split switches `false`, every other switch Java's.
fn split_fixed() -> JavaQuirks {
    [
        "simplex_prev_division_line",
        "octagon_box_cutout_branch3",
        "cutout_hole_crosses_border",
        "simplex_octagon_half_corner",
    ]
    .into_iter()
    .fold(JavaQuirks::JAVA, |q, id| q.with(id, false).unwrap())
}

fn line(ax: i32, ay: i32, bx: i32, by: i32) -> Line {
    Line::new(IntPoint::new(ax, ay), IntPoint::new(bx, by))
}

fn branch3_case() -> (TileShape, TileShape) {
    (
        TileShape::IntBox(IntBox::new(0, 0, 100, 100)),
        TileShape::IntOctagon(IntOctagon::new(40, 10, 60, 30, 10, 50, 50, 80).normalize()),
    )
}

fn border_case() -> (TileShape, TileShape) {
    (
        TileShape::IntBox(IntBox::new(-77823, -69103, 64463, 7809)),
        TileShape::Simplex(Simplex::new(vec![
            line(-44154, -109768, 10312, -28116),
            line(10312, -28116, -82325, -30905),
            line(-82325, -30905, -44154, -109768),
        ])),
    )
}

fn half_corner_case() -> (TileShape, TileShape) {
    (
        TileShape::Simplex(Simplex::new(vec![
            line(18566, -24528, 11681, 25069),
            line(11681, 25069, -24538, 23198),
            line(-24538, 23198, -37153, 807),
            line(-37153, 807, -35807, -12313),
            line(-35807, -12313, 18566, -24528),
        ])),
        TileShape::IntOctagon(
            IntOctagon::new(8306, -2880, 12762, -1432, 10511, 15050, 5426, 10473).normalize(),
        ),
    )
}

fn cases() -> [(&'static str, TileShape, TileShape); 3] {
    let (a, b) = branch3_case();
    let (c, d) = border_case();
    let (e, f) = half_corner_case();
    [("branch 3", a, b), ("border", c, d), ("half corner", e, f)]
}

fn area_of(outer: &TileShape, hole: &TileShape) -> Area {
    Area::PolylineArea(PolylineArea::new(
        PolylineShape::from(outer.clone()),
        vec![PolylineShape::from(hole.clone())],
    ))
}

/// The area of `outer` outside `hole`, the pieces' total when they tile it.
fn rest_of(outer: &TileShape, hole: &TileShape) -> f64 {
    outer.area() - outer.intersection(hole).area()
}

fn board(bounds: IntBox, outline: TileShape, quirks: JavaQuirks) -> RoutingBoard {
    let layers = LayerStructure::new(vec![Layer::new("Top", true), Layer::new("Bottom", true)]);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    let mut board = RoutingBoard::new_with_java_quirks(
        bounds,
        layers,
        vec![PolylineShape::Tile(outline)],
        0,
        rules,
        Default::default(),
        quirks,
    );
    let class = board.rules.default_net_class();
    board.rules.nets.add("A", 1, false, class);
    board
}

const BIG: IntBox = IntBox {
    ll: IntPoint {
        x: -200_000,
        y: -200_000,
    },
    ur: IntPoint {
        x: 200_000,
        y: 200_000,
    },
};

fn big_board(quirks: JavaQuirks) -> RoutingBoard {
    board(BIG, TileShape::IntBox(BIG), quirks)
}

/// Every tree's shapes of item `id`, as text.
fn tree_pieces(board: &RoutingBoard, id: ItemId) -> Vec<String> {
    let item = board.item(id).expect("item");
    let ctx = board.shape_context();
    board
        .search_trees()
        .trees()
        .iter()
        .map(|t| format!("{:?}", t.tree_shapes(item, &ctx)))
        .collect()
}

fn default_tree_total(board: &RoutingBoard, id: ItemId) -> f64 {
    let item = board.item(id).expect("item");
    let ctx = board.shape_context();
    board
        .search_trees()
        .default_tree()
        .tree_shapes(item, &ctx)
        .iter()
        .flatten()
        .map(TileShapeOps::area)
        .sum()
}

/// Equal up to one unit² and the float sums' rounding (Java's half-corner overlap
/// on the pinned case is 755,786 units² of 2e9).
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1.0 + 1e-9 * b.abs()
}

/// `board/search/mod.rs`, the tree shapes of an obstacle area with a hole: Java's
/// pieces under `true`, pieces that tile the rest under `false`.
#[test]
fn tree_shapes_of_areas_follow_the_switches() {
    for (name, outer, hole) in cases() {
        let rest = rest_of(&outer, &hole);
        let mut totals = Vec::new();
        for quirks in [JavaQuirks::JAVA, split_fixed()] {
            let mut board = big_board(quirks);
            let id = board.insert_obstacle(area_of(&outer, &hole), 0, 0, FixedState::UserFixed);
            totals.push(default_tree_total(&board, id));
        }
        println!(
            "{name}: rest {rest}, Java {}, fixed {}",
            totals[0], totals[1]
        );
        assert!(
            close(totals[1], rest),
            "{name}: fixed {totals:?}, rest {rest}"
        );
        assert!(
            !close(totals[0], rest),
            "{name}: Java {totals:?}, rest {rest}"
        );
    }
}

/// `board/search/mod.rs` and `board/items/mod.rs`: the outline's generated keepout
/// (the bounding box minus the outline) is split under the board's switches, for its
/// tree shapes, its piece count and after a move.
#[test]
fn outline_keepout_follows_the_switches() {
    let (outer, octagon) = branch3_case();
    let bounds = outer.bounding_box();
    let rest = rest_of(&outer, &octagon);
    let mut totals = Vec::new();
    for quirks in [JavaQuirks::JAVA, split_fixed()] {
        let mut board = board(bounds, octagon.clone(), quirks);
        board.generate_keepout_outside_outline(true);
        let id = board.outline().expect("outline").id();
        let outline = board.item(id).unwrap().as_board_outline().unwrap();
        assert_eq!(outline.keepout_split(), quirks.split());
        let pieces = outline.keepout_pieces().expect("pieces");
        assert_eq!(outline.tile_shape_count(), 2 * pieces.len());
        let total = default_tree_total(&board, id);
        assert!(close(
            total,
            2.0 * pieces.iter().map(TileShapeOps::area).sum::<f64>()
        ));
        assert!(board.move_item(id, &Vector::get_instance(1000, 0)));
        let moved = board.item(id).unwrap().as_board_outline().unwrap();
        assert_eq!(moved.keepout_split(), quirks.split());
        let moved_total = default_tree_total(&board, id);
        assert!(close(moved_total, total), "{moved_total} {total}");
        totals.push(total / 2.0);
    }
    println!(
        "outline keepout: rest {rest}, Java {}, fixed {}",
        totals[0], totals[1]
    );
    assert!(close(totals[1], rest), "{totals:?} {rest}");
    assert!(
        totals[0] > rest + 1.0,
        "Java's pieces overlap: {totals:?} {rest}"
    );
}

/// A point inside the pieces under `fixed` that no piece under Java's split covers:
/// where Java's border-crossing cutout loses the rest.
fn point_java_loses(area: &Area) -> FloatPoint {
    let java = area.split_to_convex().unwrap();
    let fixed = area
        .split_to_convex_with(None, split_fixed().split())
        .unwrap();
    for piece in &fixed {
        let c = piece.centre_of_gravity();
        if !java.iter().any(|j| j.contains_float(c)) {
            return c;
        }
    }
    panic!("Java covers every fixed piece's centre")
}

/// `board/queries.rs` `check_shape` and `board/basic_board.rs` `overlapping_items`: an
/// obstacle where Java's split of a border-crossing area lost its pieces is found
/// only under `false`.
#[test]
fn check_shape_and_overlapping_items_follow_the_switches() {
    let (outer, hole) = border_case();
    let area = area_of(&outer, &hole);
    let at = point_java_loses(&area);
    let (x, y) = (at.x.round() as i32, at.y.round() as i32);
    for (quirks, java) in [(JavaQuirks::JAVA, true), (split_fixed(), false)] {
        let mut board = big_board(quirks);
        let obstacle = board.insert_obstacle(
            Area::from(freerouting::geometry::Shape::from(IntBox::new(
                x - 10,
                y - 10,
                x + 10,
                y + 10,
            ))),
            0,
            0,
            FixedState::UserFixed,
        );
        let found = board.overlapping_items(&area, 0).contains(&obstacle);
        assert_eq!(found, !java, "overlapping_items, Java {java}");
        assert_eq!(
            board.check_shape(&area, 0, &[1], 0),
            java,
            "check_shape, Java {java}"
        );
    }
}

/// `interactive/construct.rs`: a hole added interactively enters the trees split
/// under the board's switches.
#[test]
fn interactive_hole_follows_the_switches() {
    use freerouting::interactive::construct::Construction;
    let (outer, octagon) = branch3_case();
    let rest = rest_of(&outer, &octagon);
    let mut totals = Vec::new();
    for quirks in [JavaQuirks::JAVA, split_fixed()] {
        let mut board = big_board(quirks);
        let area = board.insert_obstacle(
            Area::from(freerouting::geometry::Shape::from(outer.bounding_box())),
            0,
            0,
            FixedState::Unfixed,
        );
        let corners = octagon.corner_approx_arr();
        let mut construction = Construction::hole_in(&board, area, corners[0]).expect("hole");
        for corner in &corners[1..] {
            construction.click(&board, *corner);
        }
        let outcome = construction.complete(&mut board, 0);
        let id = outcome.inserted.expect("the hole is added");
        totals.push(default_tree_total(&board, id));
    }
    println!(
        "interactive hole: rest {rest}, Java {}, fixed {}",
        totals[0], totals[1]
    );
    assert!(close(totals[1], rest), "{totals:?} {rest}");
    assert!(totals[0] > rest + 1.0, "{totals:?} {rest}");
}

/// `board/split_switches.rs`: switching after insertion gives the trees the pieces
/// of a board built with the switches, and a change that leaves the split switches
/// alone touches no tree.
#[test]
fn switching_afterwards_gives_the_pieces_of_a_board_built_switched() {
    let build = |quirks: JavaQuirks| {
        let (outer, octagon) = branch3_case();
        let mut board = board(outer.bounding_box(), octagon, quirks);
        board.generate_keepout_outside_outline(true);
        let mut ids = vec![board.outline().unwrap().id()];
        for (_, outer, hole) in cases() {
            ids.push(board.insert_obstacle(area_of(&outer, &hole), 0, 0, FixedState::UserFixed));
        }
        (board, ids)
    };
    let (built, ids) = build(split_fixed());
    let (mut switched, switched_ids) = build(JavaQuirks::JAVA);
    assert_eq!(ids, switched_ids);
    let java: Vec<_> = ids.iter().map(|id| tree_pieces(&switched, *id)).collect();
    let walk_before = switched.search_trees().default_tree().walk_lines();
    // A switch outside the split: no tree changes.
    switched.set_java_quirks(JavaQuirks::JAVA.with("dsn_via_alias", false).unwrap());
    assert_eq!(
        switched.search_trees().default_tree().walk_lines(),
        walk_before
    );
    switched.set_java_quirks(split_fixed());
    for (index, id) in ids.iter().enumerate() {
        assert_eq!(
            tree_pieces(&switched, *id),
            tree_pieces(&built, *id),
            "item {index}"
        );
        assert_ne!(
            tree_pieces(&switched, *id),
            java[index],
            "item {index} changes"
        );
    }
    // And back.
    switched.set_java_quirks(JavaQuirks::JAVA);
    for (index, id) in ids.iter().enumerate() {
        assert_eq!(tree_pieces(&switched, *id), java[index], "item {index}");
    }
}

/// The reader: a corpus board where the area splits reach branch 3
/// (`Issue756-tomu-fpga9`, 6 cutouts in N06.4's inventory) read with every switch
/// `false` has the tree pieces of the same board read with Java's switches and
/// switched afterwards, and some differ from Java's.
#[test]
fn a_board_read_switched_has_the_pieces_of_one_switched_after_reading() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/Issue756-tomu-fpga9.dsn"
    );
    let bytes = std::fs::read(path).unwrap();
    let text = freerouting_dsn::decode(&bytes);
    let pcb = freerouting_dsn::Pcb::parse(&text).unwrap();
    let read = |quirks: JavaQuirks| match build_board_with(&pcb, quirks) {
        BoardReadResult::Success { board, .. } => *board,
        _ => panic!("fixture"),
    };
    let built = read(JavaQuirks::ALL_FIXED);
    let mut switched = read(JavaQuirks::JAVA);
    let ids: Vec<ItemId> = built.items().map(|i| i.id()).collect();
    assert_eq!(ids, switched.items().map(|i| i.id()).collect::<Vec<_>>());
    let java: Vec<_> = ids.iter().map(|id| tree_pieces(&switched, *id)).collect();
    switched.set_java_quirks(JavaQuirks::ALL_FIXED);
    let mut differ = 0;
    for (index, id) in ids.iter().enumerate() {
        let pieces = tree_pieces(&built, *id);
        assert_eq!(tree_pieces(&switched, *id), pieces, "item {id:?}");
        differ += usize::from(pieces != java[index]);
    }
    println!(
        "tomu-fpga9: {differ} of {} items have other tree pieces",
        ids.len()
    );
    assert!(differ > 0);
}

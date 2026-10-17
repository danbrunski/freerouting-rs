//! The fill model of conduction areas and own-net connectivity (B09.3).
//!
//! - The four zone cases of `tests/drc.rs` (continuous pour, severed pour,
//!   dead copper, hole) give the expected islands, outlines and anchors, and
//!   the anchors equal the DRC's `items_in_island`.
//! - On real boards every outline closes, its area equals the trapezoids'
//!   area, and the anchors equal the DRC's.
//! - `net_components` equals `connected_sets` on every `fixtures.tsv` board
//!   for every net; with `stop_at_plane` it is a partition of the net's
//!   connectable items.

mod board_probe;

use std::collections::BTreeSet;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::{FixedState, Layer, LayerStructure, Unit};
use freerouting::drc::{
    DesignRulesChecker, FillIsland, ZoneIslandKind, conduction_fill, net_components,
};
use freerouting::geometry::{
    Area, Circle, ConvexShape, IntBox, IntPoint, Point, Polyline, PolylineShape, Shape, TileShape,
};
use freerouting::rules::{BoardRules, ClearanceMatrix};

// ---------------------------------------------------------------------------
// The zone board of `tests/drc.rs` (drc.ZoneIslandConnectivityTest).

struct ZoneBoard {
    board: RoutingBoard,
    gnd: i32,
    sig: i32,
    padstack: freerouting::library::PadstackId,
}

fn zone_board() -> ZoneBoard {
    let layers = LayerStructure::new(vec![Layer::new("Top", false), Layer::new("GND", false)]);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    let outline = IntBox::new(0, 0, 10_000_000, 10_000_000);
    let mut board = RoutingBoard::new(
        outline,
        layers,
        vec![PolylineShape::Tile(TileShape::from(outline))],
        0,
        rules,
        Default::default(),
    );
    let class = board.rules.default_net_class();
    let gnd = board.rules.nets.add("GND", 1, true, class);
    let sig = board.rules.nets.add("SIG", 1, false, class);
    let round = ConvexShape::Circle(Circle::new(IntPoint::new(0, 0), 100_000));
    let padstack = board.library.padstacks.add(
        "TestViaPadstack",
        vec![Some(round.clone()), Some(round)],
        vec![None, None],
        true,
        false,
    );
    ZoneBoard {
        board,
        gnd,
        sig,
        padstack,
    }
}

fn pour(z: &mut ZoneBoard, layer: i32, b: IntBox) -> ItemId {
    let gnd = z.gnd;
    z.board.insert_conduction_area(
        Area::from(Shape::Polyline(PolylineShape::Tile(TileShape::from(b)))),
        layer,
        &[gnd],
        0,
        false,
        FixedState::Unfixed,
    );
    *z.board.conduction_areas().first().expect("the pour")
}

fn gnd_via(z: &mut ZoneBoard, x: i32, y: i32) -> ItemId {
    let (padstack, gnd) = (z.padstack, z.gnd);
    z.board.insert_via(
        padstack,
        IntPoint::new(x, y),
        &[gnd],
        0,
        FixedState::Unfixed,
        true,
    )
}

fn sig_trace(z: &mut ZoneBoard, corners: &[(i32, i32)]) {
    let points: Vec<Point> = corners
        .iter()
        .map(|&(x, y)| Point::Int(IntPoint::new(x, y)))
        .collect();
    let sig = z.sig;
    z.board.insert_trace(
        Polyline::from_points(&points),
        0,
        100_000,
        &[sig],
        0,
        FixedState::Unfixed,
    );
}

/// The area of the island's trapezoids.
fn trapezoid_area(island: &FillIsland) -> f64 {
    island
        .island
        .trapezoids
        .iter()
        .map(|t| {
            (t.x_right - t.x_left) * ((t.left[1] - t.left[0]) + (t.right[1] - t.right[0])) / 2.0
        })
        .sum()
}

/// Every outline closes into rings whose signed areas add up to the
/// trapezoids' area, outer rings first; returns the hole count.
fn check_outline(island: &FillIsland) -> usize {
    assert!(!island.outline.is_empty(), "an island without an outline");
    assert!(!island.outline[0].is_hole(), "the first ring is a hole");
    let mut seen_hole = false;
    for ring in &island.outline {
        assert!(ring.points.len() >= 3);
        if ring.is_hole() {
            seen_hole = true;
        } else {
            assert!(!seen_hole, "an outer ring after a hole");
        }
    }
    let outline: f64 = island.outline.iter().map(|r| r.signed_area()).sum();
    let trapezoids = trapezoid_area(island);
    assert!(
        (outline - trapezoids).abs() <= 1e-6 * trapezoids.max(1.0),
        "outline area {outline} vs trapezoids {trapezoids}"
    );
    island.outline.iter().filter(|r| r.is_hole()).count()
}

/// The DRC's `items_in_island` of every island finding equals the anchors
/// of the same island of `conduction_fill`.
fn check_anchors_match_drc(board: &RoutingBoard) -> usize {
    let mut checked = 0;
    for v in DesignRulesChecker::new(board).zone_island_violations() {
        let fill = conduction_fill(board, v.conduction_area);
        let island = fill
            .iter()
            .find(|i| i.island == v.island)
            .expect("the finding's island is in the fill");
        assert_eq!(island.anchors, v.items_in_island);
        checked += 1;
    }
    checked
}

#[test]
fn a_continuous_pour_is_one_island_without_anchors() {
    let mut z = zone_board();
    let area = pour(
        &mut z,
        1,
        IntBox::new(100_000, 100_000, 9_000_000, 9_000_000),
    );
    let fill = conduction_fill(&z.board, area);
    assert_eq!(fill.len(), 1);
    assert!(fill[0].anchors.is_empty());
    assert_eq!(check_outline(&fill[0]), 0);
    assert_eq!(fill[0].outline.len(), 1);
    assert_eq!(fill[0].outline[0].points.len(), 4, "{:?}", fill[0].outline);
}

#[test]
fn a_severed_pour_is_two_islands_with_one_via_each() {
    let mut z = zone_board();
    let area = pour(&mut z, 0, IntBox::new(0, 0, 4_000_000, 4_000_000));
    let left = gnd_via(&mut z, 500_000, 2_000_000);
    let right = gnd_via(&mut z, 3_500_000, 2_000_000);
    sig_trace(&mut z, &[(2_000_000, 0), (2_000_000, 4_000_000)]);
    let fill = conduction_fill(&z.board, area);
    assert_eq!(fill.len(), 2);
    let anchors: Vec<&[ItemId]> = fill.iter().map(|i| i.anchors.as_slice()).collect();
    // Lowest y, then lowest x: the left island first.
    assert_eq!(anchors, vec![&[left][..], &[right][..]]);
    // The via's clearance, crossed by its four thermal spokes, leaves four
    // holes around each via.
    for island in &fill {
        assert_eq!(check_outline(island), 4);
    }
    assert!(check_anchors_match_drc(&z.board) >= 1);
}

#[test]
fn a_cut_off_piece_is_an_island_without_anchors() {
    let mut z = zone_board();
    let area = pour(&mut z, 0, IntBox::new(0, 0, 4_000_000, 4_000_000));
    let via = gnd_via(&mut z, 500_000, 2_000_000);
    sig_trace(&mut z, &[(2_500_000, 0), (2_500_000, 4_000_000)]);
    let fill = conduction_fill(&z.board, area);
    assert_eq!(fill.len(), 2);
    assert_eq!(fill[0].anchors, vec![via]);
    assert!(fill[1].anchors.is_empty());
    for island in &fill {
        check_outline(island);
    }
    let violations = DesignRulesChecker::new(&z.board).zone_island_violations();
    assert!(
        violations
            .iter()
            .any(|v| v.kind == ZoneIslandKind::IsolatedIslandDeadCopper
                && v.island == fill[1].island)
    );
    check_anchors_match_drc(&z.board);
}

#[test]
fn a_hole_is_a_clockwise_ring_of_the_one_island() {
    let mut z = zone_board();
    let area = pour(&mut z, 0, IntBox::new(0, 0, 4_000_000, 4_000_000));
    let via = gnd_via(&mut z, 500_000, 2_000_000);
    sig_trace(
        &mut z,
        &[
            (2_000_000, 2_000_000),
            (2_500_000, 2_000_000),
            (2_500_000, 2_500_000),
        ],
    );
    let fill = conduction_fill(&z.board, area);
    assert_eq!(fill.len(), 1);
    assert_eq!(fill[0].anchors, vec![via]);
    // The via's clearance is a hole too (its spokes reach the fill), and so
    // is the trace's.
    assert!(check_outline(&fill[0]) >= 2, "{:?}", fill[0].outline.len());
}

// ---------------------------------------------------------------------------
// Real boards.

/// The `fixtures.tsv` rows to check: all in a release build, two boards in
/// a debug build (`board_probe::board_filter`).
fn rows() -> (Vec<board_probe::Row>, bool) {
    let filter = board_probe::board_filter(&["Issue026-J2_reference", "Issue145-smoothieboard"]);
    let all = filter.is_none();
    let rows = board_probe::fixtures()
        .into_iter()
        .filter(|row| filter.as_ref().is_none_or(|f| f.contains(&row.board)))
        .collect();
    (rows, all)
}

#[test]
fn net_components_equal_connected_sets_on_every_fixture() {
    let (mut boards, mut nets, mut planes) = (0, 0, 0);
    let (rows, all) = rows();
    for row in rows {
        let board = board_probe::load(&row);
        boards += 1;
        for net in 1..=board.rules.nets.max_net_number() {
            let expected: Vec<Vec<ItemId>> = board
                .connected_sets(net)
                .into_iter()
                .map(|s| s.into_iter().collect())
                .collect();
            assert_eq!(
                net_components(&board, net, false),
                expected,
                "{} net {net}",
                row.board
            );
            nets += 1;
            // With stop_at_plane: a partition of the connectable items,
            // ordered by the highest member.
            let split = net_components(&board, net, true);
            let mut all = BTreeSet::new();
            for component in &split {
                for id in component {
                    assert!(all.insert(*id), "{} net {net}: {id:?} twice", row.board);
                }
            }
            let connectable: BTreeSet<ItemId> = board.connectable_items(net).into_iter().collect();
            assert_eq!(all, connectable, "{} net {net}", row.board);
            assert!(split.windows(2).all(|w| w[0][0] < w[1][0]));
            if split.len() > expected.len() {
                planes += 1;
            }
        }
    }
    eprintln!("{boards} boards, {nets} nets, {planes} nets split at planes");
    assert!(boards >= 2);
    if all {
        assert!(boards > 100);
    }
}

#[test]
fn fills_of_real_boards_close_and_match_the_drc() {
    let (mut areas, mut islands, mut holes, mut findings) = (0, 0, 0, 0);
    for name in [
        "Issue145-smoothieboard",
        "Issue026-J2_reference",
        "Issue029-hw48na",
    ] {
        let board = board_probe::load(&board_probe::fixture(name));
        for area in board.conduction_areas() {
            let fill = conduction_fill(&board, area);
            areas += 1;
            islands += fill.len();
            for island in &fill {
                holes += check_outline(island);
            }
            assert_eq!(fill, conduction_fill(&board, area));
        }
        findings += check_anchors_match_drc(&board);
    }
    eprintln!("{areas} areas, {islands} islands, {holes} holes, {findings} DRC findings");
    assert!(islands > 0);
}

/// Prints the islands of every conduction area of the DSN in
/// `FILL_MODEL_DSN`, for comparing with KiCad's refill (the B09.3 done note):
/// `cargo test --release -p freerouting --test fill_model -- --ignored --nocapture`.
#[test]
#[ignore = "a report for a DSN named in FILL_MODEL_DSN"]
fn report_islands_of_a_dsn() {
    let Ok(path) = std::env::var("FILL_MODEL_DSN") else {
        eprintln!("FILL_MODEL_DSN is not set");
        return;
    };
    let bytes = std::fs::read(&path).expect("the DSN");
    let board = freerouting::drc::load_design(&bytes).expect("a loadable DSN");
    for area in board.conduction_areas() {
        let item = board.item(area).expect("area");
        let net = (item.net_count() > 0)
            .then(|| board.rules.nets.get(item.net_number(0)))
            .flatten()
            .map_or("-".to_string(), |n| n.name.clone());
        let layer = &board.layer_structure.layers[item.first_layer() as usize].name;
        let fill = conduction_fill(&board, area);
        let anchored = fill.iter().filter(|i| !i.anchors.is_empty()).count();
        // Java's dead-copper threshold: a bounding box of at least 1 mm².
        let mm = board.communication.get_resolution(Unit::Mm);
        let large = fill
            .iter()
            .filter(|i| i.island.bounding_box_area() >= mm * mm)
            .count();
        println!(
            "area\t{}\t{net}\t{layer}\t{} islands\t{anchored} with anchors\t{large} of >= 1 mm2",
            area.0,
            fill.len()
        );
    }
}

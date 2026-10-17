//! X04: the obstacle raster of the grid connection router.
//!
//! - X04.2–X04.5 on a hand-built two-layer board (`hand_board`): an
//!   L-shaped outline, a 1 mm square pad of net P, two pads of net A, a
//!   copper keepout and a via keepout, a 0.25 mm QFN-like pad and a 0.05 mm
//!   sliver pad;
//! - X04.3's distance check: `Shape::distance` against a brute-force
//!   point-to-edge distance on random tile shapes;
//! - X04.4 over every pad of `Issue145-smoothieboard`;
//! - X04.7, the conservative property checked exactly, on
//!   `Issue145-smoothieboard` and `canary-pwr-pours2`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use freerouting::board::items::{Item, ItemId, ItemKind};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::drc::load_design;
use freerouting::geometry::jmath::JavaRandom;
#[allow(unused_imports)]
use freerouting::geometry::{
    FloatPoint, IntBox, IntPoint, Point, PolylineShapeOps, Shape, TileShape, TileShapeOps,
};
use freerouting_krt::grid::frame::{Frame, window_for};
use freerouting_krt::grid::raster::{
    NetRules, build, build_raster, is_via_obstacle, outside_cells,
};
use freerouting_krt::grid::terminals::{collect, mark};
use freerouting_krt::grid::{DEFAULT_GRID_STEP_UM, GridTask, half_diagonal, step_in_board_units};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The fixture `name` (relative to `fixtures/`).
fn fixture(name: &str) -> RoutingBoard {
    let path = repo_root().join("fixtures").join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    load_design(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// 10 board units per µm.
const UM: i32 = 10;

/// The hand-built board (coordinates in µm):
/// - outline: (0,0)-(40000,0)-(40000,30000)-(20000,30000)-(20000,20000)-(0,20000);
/// - P1, net P: a 1000 x 1000 square SMD pad on F.Cu at (10000, 10000);
/// - A1, A2, net A: square pads at (5000, 5000) and (15000, 5000);
/// - a copper keepout on F.Cu over (25000, 3000)-(27000, 5000) and a via
///   keepout on both layers over (30000, 3000)-(32000, 5000);
/// - Q1, net Q: a 250 x 800 pad at (35000, 15000); S1, net S: a 50 x 500
///   sliver at (35050, 25000);
/// - 250 µm traces, 200 µm clearance, a 600 µm through via.
fn hand_board() -> RoutingBoard {
    let dsn = "(pcb x04.dsn
  (parser (string_quote \") (space_in_quoted_tokens on) (host_cad \"KiCad's Pcbnew\") (host_version \"x04 unit test\"))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer B.Cu (type signal) (property (index 1)))
    (boundary (path pcb 0  0 0  40000 0  40000 30000  20000 30000  20000 20000  0 20000  0 0))
    (keepout \"\" (rect F.Cu 25000 3000 27000 5000))
    (via_keepout \"\" (rect signal 30000 3000 32000 5000))
    (via \"Via[0-1]_600:300_um\")
    (rule (width 250) (clearance 200))
  )
  (placement
    (component \"SQ\"
      (place P1 10000 10000 front 0)
      (place A1 5000 5000 front 0)
      (place A2 15000 5000 front 0)
    )
    (component \"QFN\"
      (place Q1 35000 15000 front 0)
    )
    (component \"SLIVER\"
      (place S1 35050 25000 front 0)
    )
  )
  (library
    (image \"SQ\" (pin Sq 1 0 0))
    (image \"QFN\" (pin Qp 1 0 0))
    (image \"SLIVER\" (pin Sl 1 0 0))
    (padstack Sq (shape (rect F.Cu -500 -500 500 500)) (attach off))
    (padstack Qp (shape (rect F.Cu -125 -400 125 400)) (attach off))
    (padstack Sl (shape (rect F.Cu -25 -250 25 250)) (attach off))
    (padstack \"Via[0-1]_600:300_um\" (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
    (net P (pins P1-1))
    (net A (pins A1-1 A2-1))
    (net Q (pins Q1-1))
    (net S (pins S1-1))
    (class kicad_default \"\" A P Q S
      (circuit (use_via \"Via[0-1]_600:300_um\"))
      (rule (width 250) (clearance 200))
    )
  )
  (wiring)
)
";
    load_design(dsn.as_bytes()).unwrap_or_else(|e| panic!("{e}"))
}

fn net(board: &BasicBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("net {name}"))
        .net_number
}

fn pin_of(board: &BasicBoard, net_name: &str, index: usize) -> ItemId {
    board.net_pins(net(board, net_name))[index]
}

/// The task routing net A from A1 to A2.
fn task_a(board: &BasicBoard) -> GridTask {
    GridTask {
        net: net(board, "A"),
        start: BTreeSet::from([pin_of(board, "A", 0)]),
        dest: BTreeSet::from([pin_of(board, "A", 1)]),
    }
}

/// A frame over the whole hand board at 0.1 mm.
fn whole_frame(board: &BasicBoard) -> Frame {
    let step = step_in_board_units(board, DEFAULT_GRID_STEP_UM);
    let outline = board.outline().expect("outline").bounding_box();
    Frame::covering(step, &outline, board.layer_count())
}

/// The cell of a point given in µm.
fn cell_um(frame: &Frame, x: i32, y: i32) -> (i32, i32) {
    frame.cell_of(IntPoint::new(x * UM, y * UM))
}

// ---------------------------------------------------------------------
// X04.2
// ---------------------------------------------------------------------

#[test]
fn windows_of_overlapping_connections_share_cells() {
    let board = hand_board();
    let step = step_in_board_units(&board, DEFAULT_GRID_STEP_UM);
    assert_eq!(step, 1000);
    let a = task_a(&board);
    let p = GridTask {
        net: net(&board, "P"),
        start: BTreeSet::from([pin_of(&board, "P", 0)]),
        dest: BTreeSet::from([pin_of(&board, "Q", 0)]),
    };
    let fa = window_for(&board, &a, step);
    let fp = window_for(&board, &p, step);
    // A: pads 4.5..15.5 x 4.5..5.5 mm, + 5 mm, clipped to the outline box.
    assert_eq!((fa.i0, fa.j0, fa.nx, fa.ny), (0, 0, 206, 106));
    // P..Q: 9.5..35.125 x 9.5..15.4 mm, + 5 mm.
    assert_eq!((fp.i0, fp.j0, fp.nx, fp.ny), (45, 45, 356, 160));
    let mut shared = 0;
    for j in fa.j0..fa.j0 + fa.ny {
        for i in fa.i0..fa.i0 + fa.nx {
            if fp.contains_cell(i, j) {
                assert_eq!(fa.centre(i, j), fp.centre(i, j));
                shared += 1;
            }
        }
    }
    assert_eq!(shared, (206 - 45) * (106 - 45));
    // A boundary point between cells 100 and 101 belongs to 100.
    assert_eq!(fa.cell_of(IntPoint::new(100_500, 100_500)), (100, 100));
    assert_eq!(fp.cell_of(IntPoint::new(100_500, 100_501)), (100, 101));
}

// ---------------------------------------------------------------------
// X04.3
// ---------------------------------------------------------------------

#[test]
fn pad_blocks_the_expected_cell_count() {
    let board = hand_board();
    let task = task_a(&board);
    let frame = whole_frame(&board);
    let map = build(&board, &task, &frame);
    let rules = NetRules::of_net(&board, task.net);
    let pad = board.item(pin_of(&board, "P", 0)).unwrap();
    let c = board.clearance_value(rules.trace_class, pad.clearance_class(), 0);
    let r = i64::from(c + rules.half_width[0] + half_diagonal(frame.step));
    assert_eq!(
        (c, rules.half_width[0], half_diagonal(1000)),
        (2016, 1250, 708)
    );
    // Expected: the grid points within r of the 10000-unit square about
    // cell (100, 100), in exact integers.
    let half = 5000i64;
    let mut expected = 0;
    let mut counted = 0;
    for dj in -20i64..=20 {
        for di in -20i64..=20 {
            let dx = ((di * 1000).abs() - half).max(0);
            let dy = ((dj * 1000).abs() - half).max(0);
            let inside = dx * dx + dy * dy < r * r;
            expected += usize::from(inside);
            let (i, j) = (100 + di as i32, 100 + dj as i32);
            counted += usize::from(map.is_blocked(i, j, 0));
            assert!(!map.is_blocked(i, j, 1), "B.Cu has no pad at {i},{j}");
        }
    }
    assert_eq!(expected, 285);
    assert_eq!(counted, expected);
}

#[test]
fn keepouts_and_outline() {
    let board = hand_board();
    let task = task_a(&board);
    let frame = whole_frame(&board);
    let map = build(&board, &task, &frame);
    // The copper keepout blocks traces on F.Cu and vias, not B.Cu traces.
    let (i, j) = cell_um(&frame, 26000, 4000);
    assert!(map.is_blocked(i, j, 0));
    assert!(!map.is_blocked(i, j, 1));
    assert!(map.is_via_blocked(i, j));
    // Within c + hw + h of the keepout's edge, outside it.
    let (i, j) = cell_um(&frame, 27300, 4000);
    assert!(map.is_blocked(i, j, 0));
    let (i, j) = cell_um(&frame, 27500, 4000);
    assert!(!map.is_blocked(i, j, 0));
    // The via keepout blocks only vias.
    let (i, j) = cell_um(&frame, 31000, 4000);
    assert!(map.is_via_blocked(i, j));
    assert!(!map.is_blocked(i, j, 0));
    assert!(!map.is_blocked(i, j, 1));
    // Outside the L-shaped outline: blocked on both layers and for vias.
    let (i, j) = cell_um(&frame, 10000, 25000);
    assert!(map.is_blocked(i, j, 0) && map.is_blocked(i, j, 1));
    assert!(map.is_via_blocked(i, j));
    // Open board: free.
    let (i, j) = cell_um(&frame, 30000, 15000);
    assert!(!map.is_blocked(i, j, 0) && !map.is_blocked(i, j, 1));
    assert!(!map.is_via_blocked(i, j));
    // Near the outline lines (half-width 100 µm plus the clearance).
    let (i, j) = cell_um(&frame, 30000, 200);
    assert!(map.is_blocked(i, j, 0));
    // The ring outside the frame is blocked.
    assert!(map.is_blocked(frame.i0 - 1, frame.j0 + 5, 0));
    assert!(map.is_via_blocked(frame.i0 + frame.nx, frame.j0 + 5));
}

#[test]
fn own_net_pads_are_free_for_traces_but_not_for_vias() {
    let board = hand_board();
    let task = task_a(&board);
    let frame = whole_frame(&board);
    let map = build(&board, &task, &frame);
    let (i, j) = cell_um(&frame, 5000, 5000);
    assert!(!map.is_blocked(i, j, 0));
    assert!(map.is_via_blocked(i, j), "no via in a pad (D8)");
}

/// The corners of `shape`, counter-clockwise.
fn brute_distance(shape: &TileShape, p: FloatPoint) -> f64 {
    let corners = shape.corner_approx_arr();
    let n = corners.len();
    let cross = |a: FloatPoint, b: FloatPoint, q: FloatPoint| {
        (b.x - a.x) * (q.y - a.y) - (b.y - a.y) * (q.x - a.x)
    };
    let inside = (0..n).all(|k| cross(corners[k], corners[(k + 1) % n], p) >= 0.0);
    if inside {
        return 0.0;
    }
    (0..n)
        .map(|k| segment_distance(p, corners[k], corners[(k + 1) % n]))
        .fold(f64::INFINITY, f64::min)
}

fn segment_distance(p: FloatPoint, a: FloatPoint, b: FloatPoint) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    };
    let (ex, ey) = (p.x - (a.x + t * dx), p.y - (a.y + t * dy));
    (ex * ex + ey * ey).sqrt()
}

/// A random convex tile: a box, the bounding octagon of a random convex
/// polygon, or the polygon itself.
fn random_tile(random: &mut JavaRandom, kind: usize) -> TileShape {
    let cx = random.next_int(2_000_000) - 1_000_000;
    let cy = random.next_int(2_000_000) - 1_000_000;
    if kind == 0 {
        let w = random.next_int(50_000) + 1;
        let h = random.next_int(50_000) + 1;
        return TileShape::IntBox(IntBox::new(cx, cy, cx + w, cy + h));
    }
    let radius = f64::from(random.next_int(40_000) + 2_000);
    let n = 3 + random.next_int(6) as usize;
    let mut angles: Vec<f64> = (0..n)
        .map(|_| random.next_double() * std::f64::consts::TAU)
        .collect();
    angles.sort_by(f64::total_cmp);
    let mut corners: Vec<Point> = Vec::new();
    for a in angles {
        let p = IntPoint::new(
            cx + (radius * a.cos()).round() as i32,
            cy + (radius * a.sin()).round() as i32,
        );
        if corners.last() != Some(&Point::from(p)) {
            corners.push(Point::from(p));
        }
    }
    let polygon = TileShape::get_instance_from_points(&corners);
    if kind == 1 {
        TileShape::IntOctagon(polygon.bounding_octagon().expect("bounded"))
    } else {
        polygon
    }
}

#[test]
fn shape_distance_matches_brute_force() {
    let mut random = JavaRandom::new(0x0045_0403);
    let mut max_error = [0.0f64; 3];
    let mut points = 0;
    let mut shapes = 0;
    while points < 1000 {
        let kind = shapes % 3;
        let shape = random_tile(&mut random, kind);
        shapes += 1;
        if shape.is_empty() || shape.dimension() < 2 {
            continue;
        }
        let b = shape.bounding_box();
        let (w, h) = (b.ur.x - b.ll.x, b.ur.y - b.ll.y);
        for _ in 0..10 {
            let p = FloatPoint::new(
                f64::from(b.ll.x - w) + 3.0 * f64::from(w) * random.next_double(),
                f64::from(b.ll.y - h) + 3.0 * f64::from(h) * random.next_double(),
            );
            let error = (shape.distance(p) - brute_distance(&shape, p)).abs();
            max_error[kind] = max_error[kind].max(error);
            points += 1;
        }
    }
    println!("max |distance - brute force| per kind (box, octagon, simplex): {max_error:?}");
    assert!(max_error.iter().all(|e| *e <= 1.0), "{max_error:?}");
}

// ---------------------------------------------------------------------
// X04.4
// ---------------------------------------------------------------------

#[test]
fn via_ring_near_a_foreign_pad() {
    let board = hand_board();
    let task = task_a(&board);
    let frame = whole_frame(&board);
    let raster = build_raster(&board, &task, &frame);
    let via = raster.rules.via.clone().expect("net A has a via");
    assert_eq!(via.radius, vec![3000, 3000]);
    let pad = board.item(pin_of(&board, "P", 0)).unwrap();
    let c = board.clearance_value(via.clearance_class, pad.clearance_class(), 0);
    let limit = c + via.radius[0] + half_diagonal(frame.step);
    // Along row 100, right of P1's edge at x = 105000.
    let mut blocked_next_to_pad = 0;
    for i in 106..120 {
        let d = i * 1000 - 105_000;
        assert_eq!(
            raster.map.is_via_blocked(i, 100),
            d < limit,
            "cell {i}: d {d}"
        );
        // A via ring within clearance of the pad is never free.
        if d < c + via.radius[0] {
            assert!(raster.map.is_via_blocked(i, 100));
            blocked_next_to_pad += 1;
        }
    }
    assert_eq!(blocked_next_to_pad, 5);
    // The first free cell's via ring keeps the clearance.
    let first_free = (106..120)
        .find(|i| !raster.map.is_via_blocked(*i, 100))
        .unwrap();
    assert!(first_free * 1000 - 105_000 - via.radius[0] >= c);
}

#[test]
fn smoothieboard_via_cells_are_never_in_a_pad() {
    let board = fixture("Issue145-smoothieboard.dsn");
    let step = step_in_board_units(&board, DEFAULT_GRID_STEP_UM);
    let outline = board.outline().expect("outline").bounding_box();
    let frame = Frame::covering(step, &outline, board.layer_count());
    // The first net with a pin, routed from its first pin.
    let net = (1..=board.rules.nets.max_net_number())
        .find(|n| !board.net_pins(*n).is_empty())
        .unwrap();
    let task = GridTask {
        net,
        start: BTreeSet::from([board.net_pins(net)[0]]),
        dest: BTreeSet::new(),
    };
    let raster = build_raster(&board, &task, &frame);
    let via = raster.rules.via.clone().expect("a via rule");
    let mut pads = 0;
    let mut cells = 0;
    for pin in board.items().filter(|i| i.is_pin()) {
        let drill = pin.drill().unwrap();
        pads += 1;
        for layer in via.from_layer..=via.to_layer {
            let Some(shape) = drill.shape_on_layer(layer) else {
                continue;
            };
            let Some((i_lo, i_hi, j_lo, j_hi)) = frame.cells_near(&shape.bounding_box(), 0) else {
                continue;
            };
            for j in j_lo..=j_hi {
                for i in i_lo..=i_hi {
                    if shape.contains(&Point::from(frame.centre(i, j))) {
                        cells += 1;
                        assert!(
                            raster.map.is_via_blocked(i, j),
                            "via cell ({i}, {j}) inside pad {:?}",
                            pin.id()
                        );
                    }
                }
            }
        }
    }
    // And the converse sample: no via-free cell has a pad at its centre.
    let mut free = 0;
    for j in frame.j0..frame.j0 + frame.ny {
        for i in frame.i0..frame.i0 + frame.nx {
            if !raster.map.is_via_blocked(i, j) {
                free += 1;
                let p = Point::from(frame.centre(i, j));
                for layer in via.from_layer..=via.to_layer {
                    assert!(!pads_at_fast(&board, &p, layer, &frame, i, j));
                }
            }
        }
    }
    println!(
        "smoothieboard: {pads} pads, {cells} pad cells all via-blocked; {free} via-free cells of {}",
        frame.cell_count()
    );
    assert!(pads > 500 && cells > 10_000 && free > 0);
}

/// True if a pad on `layer` contains `p`, the centre of cell `(i, j)`.
fn pads_at_fast(board: &BasicBoard, p: &Point, layer: i32, frame: &Frame, i: i32, j: i32) -> bool {
    let c = frame.centre(i, j);
    let probe = TileShape::IntBox(IntBox::new(c.x, c.y, c.x, c.y));
    board
        .overlapping_objects(&probe, layer)
        .into_iter()
        .any(|id| {
            board.item(id).is_some_and(|pin| {
                pin.is_pin()
                    && pin
                        .drill()
                        .and_then(|d| d.shape_on_layer(layer))
                        .is_some_and(|s| s.contains(p))
            })
        })
}

// ---------------------------------------------------------------------
// X04.5
// ---------------------------------------------------------------------

#[test]
fn qfn_pad_has_source_cells_and_a_sliver_has_none() {
    let board = hand_board();
    let frame = whole_frame(&board);
    let qfn = pin_of(&board, "Q", 0);
    let sliver = pin_of(&board, "S", 0);
    let t = collect(&board, &frame, &BTreeSet::from([qfn]));
    // x in [-1250, 1250], y in [-4000, 4000] about (350000, 150000).
    assert_eq!(t.cells.len(), 3 * 9);
    assert!(t.empty.is_empty());
    assert!(t.cells.iter().all(|c| c[2] == 0));
    let s = collect(&board, &frame, &BTreeSet::from([qfn, sliver]));
    assert_eq!(s.cells, t.cells);
    assert_eq!(s.empty, vec![sliver]);
    // Marked cells override the trace blocking of neighbouring copper.
    let task = GridTask {
        net: net(&board, "Q"),
        start: BTreeSet::from([qfn]),
        dest: BTreeSet::new(),
    };
    let mut map = build(&board, &task, &frame);
    mark(&mut map, &t.cells);
    assert!(t.cells.iter().all(|c| !map.is_blocked(c[0], c[1], 0)));
}

// ---------------------------------------------------------------------
// X04.7
// ---------------------------------------------------------------------

/// The exact copper distance from `p` to `item` on `layer`: pads with their
/// exact shape, areas with their exact convex pieces, traces and outline
/// lines with their tile shapes.
fn exact_distance(board: &BasicBoard, item: &Item, layer: i32, p: FloatPoint) -> f64 {
    match &item.kind {
        ItemKind::Pin(_) | ItemKind::Via(_) => item
            .drill()
            .and_then(|d| d.shape_on_layer(layer))
            .map_or(f64::INFINITY, |s| s.distance(p)),
        ItemKind::Area(area) if area.layer() == layer => area
            .area()
            .split_to_convex()
            .unwrap_or_default()
            .iter()
            .map(|t| t.distance(p))
            .fold(f64::INFINITY, f64::min),
        ItemKind::Area(_) => f64::INFINITY,
        _ => (0..board.tile_shape_count(item))
            .filter(|k| item.shape_layer(*k) == layer)
            .filter_map(|k| board.tile_shape(item, k))
            .map(|t| t.distance(p))
            .fold(f64::INFINITY, f64::min),
    }
}

/// The smallest `distance - (c + hw)` over the trace obstacles near `p`
/// on `layer` (negative: a violation), with the item.
fn worst_margin(
    board: &BasicBoard,
    rules: &NetRules,
    net: i32,
    layer: i32,
    points: &[FloatPoint],
    reach: i32,
) -> Option<(f64, ItemId)> {
    let (mut lo_x, mut lo_y, mut hi_x, mut hi_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in points {
        lo_x = lo_x.min(p.x);
        lo_y = lo_y.min(p.y);
        hi_x = hi_x.max(p.x);
        hi_y = hi_y.max(p.y);
    }
    let probe = TileShape::IntBox(IntBox::new(
        lo_x.floor() as i32 - reach,
        lo_y.floor() as i32 - reach,
        hi_x.ceil() as i32 + reach,
        hi_y.ceil() as i32 + reach,
    ));
    let mut worst: Option<(f64, ItemId)> = None;
    for id in board.overlapping_objects(&probe, layer) {
        let item = board.item(id).unwrap();
        if !board.is_trace_obstacle(item, net) {
            continue;
        }
        let need = f64::from(
            board.clearance_value(rules.trace_class, item.clearance_class(), layer)
                + rules.half_width_on(layer),
        );
        for p in points {
            let margin = exact_distance(board, item, layer, *p) - need;
            if worst.is_none_or(|(w, _)| margin < w) {
                worst = Some((margin, id));
            }
        }
    }
    worst
}

/// Up to `limit` connections of `board`: per net with at least two
/// connected sets, its first two sets (nets ascending).
fn connections(board: &BasicBoard, limit: usize) -> Vec<GridTask> {
    let mut tasks = Vec::new();
    for net in 1..=board.rules.nets.max_net_number() {
        let sets = board.connected_sets(net);
        if sets.len() >= 2 {
            tasks.push(GridTask {
                net,
                start: sets[0].clone(),
                dest: sets[1].clone(),
            });
        }
        if tasks.len() == limit {
            break;
        }
    }
    tasks
}

const NEIGHBOURS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// X04.7 on `name`: 10,000 free cells and 10,000 adjacent free pairs,
/// sampled with `JavaRandom(seed)` over the rasters of up to 20 connections.
fn check_conservative(name: &str, seed: i64) {
    let board = fixture(name);
    let step = step_in_board_units(&board, DEFAULT_GRID_STEP_UM);
    let tasks = connections(&board, 20);
    assert!(!tasks.is_empty(), "{name}: no open connection");
    let mut random = JavaRandom::new(seed);
    let mut times = Vec::new();
    let (mut cells_checked, mut pairs_checked) = (0usize, 0usize);
    let (mut cell_worst, mut pair_worst) = (f64::INFINITY, f64::INFINITY);
    let per_task_cells = 10_000usize.div_ceil(tasks.len());
    for task in &tasks {
        let frame = window_for(&board, task, step);
        let start = std::time::Instant::now();
        let raster = build_raster(&board, task, &frame);
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        let rules = &raster.rules;
        let reach = (0..board.layer_count() as i32)
            .map(|l| {
                (0..board.rules.clearance_matrix.class_count() as i32)
                    .map(|k| board.clearance_value(rules.trace_class, k, l))
                    .max()
                    .unwrap_or(0)
                    + rules.half_width_on(l)
            })
            .max()
            .unwrap()
            + step;
        let free =
            |i: i32, j: i32, l: usize| frame.contains_cell(i, j) && !raster.map.is_blocked(i, j, l);
        let (mut got_cells, mut got_pairs, mut tries) = (0, 0, 0);
        while (got_cells < per_task_cells || got_pairs < per_task_cells)
            && tries < 200 * per_task_cells
        {
            tries += 1;
            let i = frame.i0 + random.next_int(frame.nx);
            let j = frame.j0 + random.next_int(frame.ny);
            let l = random.next_int(frame.layers as i32) as usize;
            let (di, dj) = NEIGHBOURS[random.next_int(8) as usize];
            if !free(i, j, l) {
                continue;
            }
            let p = frame.centre(i, j).to_float();
            if got_cells < per_task_cells {
                if let Some((m, id)) = worst_margin(&board, rules, task.net, l as i32, &[p], reach)
                {
                    assert!(
                        m >= 0.0,
                        "{name}: free cell ({i},{j},{l}) is {m} too close to {id:?}"
                    );
                    cell_worst = cell_worst.min(m);
                }
                got_cells += 1;
            }
            if got_pairs < per_task_cells && free(i + di, j + dj, l) {
                let q = frame.centre(i + di, j + dj).to_float();
                // The segment sampled every s/8.
                let n = if di != 0 && dj != 0 { 12 } else { 8 };
                let samples: Vec<FloatPoint> = (0..=n)
                    .map(|k| {
                        let t = f64::from(k) / f64::from(n);
                        FloatPoint::new(p.x + t * (q.x - p.x), p.y + t * (q.y - p.y))
                    })
                    .collect();
                if let Some((m, id)) =
                    worst_margin(&board, rules, task.net, l as i32, &samples, reach)
                {
                    assert!(
                        m >= 0.0,
                        "{name}: move ({i},{j})->({},{}) on {l} is {m} too close to {id:?}",
                        i + di,
                        j + dj
                    );
                    pair_worst = pair_worst.min(m);
                }
                got_pairs += 1;
            }
        }
        cells_checked += got_cells;
        pairs_checked += got_pairs;
    }
    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2];
    let max = times[times.len() - 1];
    println!(
        "{name}: {} connections; {cells_checked} free cells (worst margin {cell_worst:.1} units), \
         {pairs_checked} adjacent pairs (worst margin {pair_worst:.1}); build ms median {median:.2} max {max:.2}",
        tasks.len()
    );
    assert!(
        cells_checked >= 10_000 && pairs_checked >= 10_000,
        "{cells_checked} {pairs_checked}"
    );
    let _ = is_via_obstacle;
}

#[test]
fn conservative_on_smoothieboard() {
    check_conservative("Issue145-smoothieboard.dsn", 4_045_007);
}

#[test]
fn conservative_on_canary_pwr() {
    check_conservative("canary-pwr/canary-pwr-pours2.dsn", 4_045_007);
}

/// The block shortcut of `outside_cells` gives exactly the per-cell test,
/// on the L-shaped hand board and on both real boards' whole outlines.
#[test]
fn outside_cells_match_the_per_cell_test() {
    for board in [
        hand_board(),
        fixture("Issue145-smoothieboard.dsn"),
        fixture("canary-pwr/canary-pwr-pours2.dsn"),
    ] {
        let outline_item = board.outline().expect("outline");
        let outline = outline_item.as_board_outline().unwrap();
        let step = step_in_board_units(&board, DEFAULT_GRID_STEP_UM);
        // A frame reaching 2 mm beyond the outline, off the block grid.
        let b = outline_item.bounding_box();
        let m = 20 * step + step / 3;
        let window = IntBox::new(b.ll.x - m, b.ll.y - m, b.ur.x + m, b.ur.y + m);
        let frame = Frame::covering(step, &window, board.layer_count());
        let mut expected = Vec::new();
        for j in frame.j0..frame.j0 + frame.ny {
            for i in frame.i0..frame.i0 + frame.nx {
                if !outline.contains(&Point::from(frame.centre(i, j))) {
                    expected.push([i, j]);
                }
            }
        }
        let got = outside_cells(outline, &frame);
        assert!(!expected.is_empty());
        assert_eq!(got, expected);
    }
}

/// Two builds of the same connection give the same raster, cell by cell.
#[test]
fn raster_builds_are_deterministic() {
    let board = fixture("Issue145-smoothieboard.dsn");
    let step = step_in_board_units(&board, DEFAULT_GRID_STEP_UM);
    let task = connections(&board, 3).pop().expect("a connection");
    let frame = window_for(&board, &task, step);
    let a = build_raster(&board, &task, &frame);
    let b = build_raster(&board, &task, &frame);
    assert_eq!(a.stats, b.stats);
    for j in frame.j0 - 1..=frame.j0 + frame.ny {
        for i in frame.i0 - 1..=frame.i0 + frame.nx {
            assert_eq!(a.map.is_via_blocked(i, j), b.map.is_via_blocked(i, j));
            for l in 0..frame.layers {
                assert_eq!(a.map.is_blocked(i, j, l), b.map.is_blocked(i, j, l));
            }
        }
    }
    assert!(a.stats.blocked.iter().sum::<usize>() > 0);
}

//! The read-only board checks and picks against Java (B04).
//!
//! The probe `board-queries` (`tools/java-reference/probes/board/BoardQueriesProbe.java`)
//! samples points, segments and pin pairs with `java.util.Random(20261015)` and records
//! `checkShape`, `checkTraceShape`, `checkPolylineTrace`, `checkTraceSegment`, filtered
//! `pickItems`, `pickNearestRoutingItem`, `checkMoveItem`, `checkChangeNet`, the item helpers
//! of every pin, via, trace and conduction area, and `offsetShapes` of the three tree kinds.
//! The sampler is code shared by name: this file implements it from the probe's doc comment,
//! and `inputs_match_java` checks the echoed inputs before any result is compared.
//!
//! Each output section (`<stage>:<method>`) is compared on its own, one `#[test]` per method,
//! so a later item can switch its test on without the others. A board whose Java output passed
//! 1 MiB is committed as a digest; its sections are compared by digest line. On a mismatch the
//! Rust section is written to `target/probe/board-queries/<board>.<section>.jsonl`.

mod board_probe;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

use board_probe::{Line, Row, bits, header, repo_root};
use freerouting::board::items::{Item, ItemId, ItemKind};
use freerouting::board::queries::point_octagon_area;
use freerouting::board::search::{ShapeSearchTree, TreeKind};
use freerouting::board::{ItemFilter, RoutingBoard, SelectableChoice};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{
    IntPoint, IntVector, LineSegment, Point, Polyline, PolylineShapeOps, TileShape, Vector,
};

const KIND: &str = "board-queries";
const SEED: i64 = 20261015;
const SAMPLES: usize = 300;
const MAX_PAIRS: usize = 200;
const OFFSET_POLYLINES: usize = 200;
const J2: &str = "Issue026-J2_reference";

/// Board -> session (below the repository root) or `None` for load only, as the probe's
/// `PLAN`.
const PLAN: &[(&str, Option<&str>)] = &[
    (J2, Some("fixtures/Issue026-J2_reference.ses")),
    (
        "Issue508-DAC2020_bm08",
        Some("fixtures/Issue508-DAC2020_bm08-routed.ses"),
    ),
    ("Issue413-test", None),
    ("Issue957-smoke-ldo-led", None),
    ("Issue145-smoothieboard", None),
    ("Issue676-ch32v-tx118s", None),
];

/// `(board, section, reason)` of sections known to differ from Java.
const EXPECTED_MISMATCHES: &[(&str, &str, &str)] = &[];

// ---------------------------------------------------------------------------------------------
// The sampler (the probe's doc comment)
// ---------------------------------------------------------------------------------------------

struct Sample {
    x: i32,
    y: i32,
    layer: i32,
    net: i32,
    hw: i32,
    cl: i32,
    x2: i32,
    y2: i32,
}

struct Pair {
    a: ItemId,
    b: ItemId,
    layer: i32,
    net: i32,
    hw: i32,
    cl: i32,
}

/// A segment or pick point: a random sample (`"i"`) or a pin pair (`"pair"`).
struct Segment {
    key: &'static str,
    index: usize,
    from: Point,
    to: Point,
    layer: i32,
    net: i32,
    hw: i32,
    cl: i32,
}

fn int_point(x: i32, y: i32) -> Point {
    Point::Int(IntPoint { x, y })
}

fn samples(board: &RoutingBoard) -> Vec<Sample> {
    let bb = board.bounding_box();
    let width = bb.ur.x - bb.ll.x;
    let height = bb.ur.y - bb.ll.y;
    let layers = board.layer_count() as i32;
    let max_net = board.rules.nets.max_net_number();
    let max_half_width = board.max_trace_half_width();
    let classes = board.rules.clearance_matrix.class_count() as i32;
    let r = width.min(height) / 10;
    let mut random = JavaRandom::new(SEED);
    (0..SAMPLES)
        .map(|_| {
            let x = bb.ll.x + random.next_int(width + 1);
            let y = bb.ll.y + random.next_int(height + 1);
            let layer = random.next_int(layers);
            let net = if max_net > 0 {
                random.next_int(max_net) + 1
            } else {
                0
            };
            let hw = 1 + random.next_int(2 * max_half_width);
            let cl = random.next_int(classes);
            let x2 = x - r + random.next_int(2 * r + 1);
            let y2 = y - r + random.next_int(2 * r + 1);
            Sample {
                x,
                y,
                layer,
                net,
                hw,
                cl,
                x2,
                y2,
            }
        })
        .collect()
}

fn pin_pairs(board: &RoutingBoard) -> Vec<Pair> {
    let pins = board.pins();
    let mut result = Vec::new();
    for (i, a_id) in pins.iter().enumerate() {
        let a = board.item(*a_id).expect("pin");
        for b_id in &pins[i + 1..] {
            let b = board.item(*b_id).expect("pin");
            let Some(net) = a.net_numbers().iter().copied().find(|n| b.contains_net(*n)) else {
                continue;
            };
            let first = a.first_layer().max(b.first_layer());
            let last = a.last_layer().min(b.last_layer());
            let Some(layer) =
                (first..=last).find(|l| board.layer_structure.layers[*l as usize].is_signal)
            else {
                continue;
            };
            if a.drill().expect("pin").center == b.drill().expect("pin").center {
                continue;
            }
            let hw = board.rules.trace_half_width(net, layer);
            let cl = board
                .rules
                .net_class_of(net)
                .expect("net class")
                .trace_clearance_class();
            result.push(Pair {
                a: *a_id,
                b: *b_id,
                layer,
                net,
                hw,
                cl,
            });
            if result.len() >= MAX_PAIRS {
                return result;
            }
        }
    }
    result
}

fn segments(board: &RoutingBoard, samples: &[Sample], pairs: &[Pair]) -> Vec<Segment> {
    let mut result: Vec<Segment> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| Segment {
            key: "i",
            index: i,
            from: int_point(s.x, s.y),
            to: int_point(s.x2, s.y2),
            layer: s.layer,
            net: s.net,
            hw: s.hw,
            cl: s.cl,
        })
        .collect();
    for (k, p) in pairs.iter().enumerate() {
        let center = |id: ItemId| Point::Int(board.item(id).expect("pin").drill().unwrap().center);
        result.push(Segment {
            key: "pair",
            index: k,
            from: center(p.a),
            to: center(p.b),
            layer: p.layer,
            net: p.net,
            hw: p.hw,
            cl: p.cl,
        });
    }
    result
}

// ---------------------------------------------------------------------------------------------
// Text as the probe writes it
// ---------------------------------------------------------------------------------------------

fn ids<'a>(items: impl IntoIterator<Item = &'a ItemId>) -> String {
    let list: Vec<String> = items.into_iter().map(|id| id.0.to_string()).collect();
    format!("[{}]", list.join(","))
}

fn strings(values: &[String]) -> String {
    let list: Vec<String> = values.iter().map(|v| format!("\"{v}\"")).collect();
    format!("[{}]", list.join(","))
}

/// The probe's `point`: `x,y`, or `f<x bits>,<y bits>` for a rational point.
fn point_text(p: &Point) -> String {
    match p {
        Point::Int(ip) => format!("{},{}", ip.x, ip.y),
        _ => {
            let f = p.to_float();
            format!("f{},{}", bits(f.x), bits(f.y))
        }
    }
}

/// The probe's `shape`.
fn shape_text(s: Option<&TileShape>) -> String {
    match s {
        None => "null".to_string(),
        Some(TileShape::IntBox(b)) => format!("box {} {} {} {}", b.ll.x, b.ll.y, b.ur.x, b.ur.y),
        Some(TileShape::IntOctagon(o)) => format!(
            "oct {} {} {} {} {} {} {} {}",
            o.left_x,
            o.bottom_y,
            o.right_x,
            o.top_y,
            o.upper_left_diagonal_x,
            o.lower_right_diagonal_x,
            o.lower_left_diagonal_x,
            o.upper_right_diagonal_x
        ),
        Some(other) => {
            let mut text = String::from("simplex");
            for i in 0..other.border_line_count() {
                let line = other.border_line(i);
                let (a, b) = (line.a(), line.b());
                text.push_str(&format!(" {},{} {},{}", a.x, a.y, b.x, b.y));
            }
            text
        }
    }
}

fn shapes_text(shapes: &[TileShape]) -> String {
    let texts: Vec<String> = shapes.iter().map(|s| shape_text(Some(s))).collect();
    strings(&texts)
}

// ---------------------------------------------------------------------------------------------
// The Rust side of each section
// ---------------------------------------------------------------------------------------------

type Sections = Vec<(String, Vec<String>)>;

fn max_generated_id(board: &RoutingBoard) -> i64 {
    i64::from(board.communication.id_generator.max_generated_id())
}

/// Starts section `<stage>:<method>` with its header line.
fn section(sections: &mut Sections, board: &RoutingBoard, stage: &str, method: &str) -> Line {
    sections.push((format!("{stage}:{method}"), Vec::new()));
    Line::new()
        .str("stage", &format!("{stage}:{method}"))
        .num("maxGeneratedId", max_generated_id(board))
}

fn push(sections: &mut Sections, line: String) {
    sections.last_mut().expect("a section").1.push(line);
}

fn segment_line(g: &Segment) -> Line {
    Line::new().num(g.key, g.index as i64)
}

fn run_stage(sections: &mut Sections, board: &mut RoutingBoard, stage: &str, offsets: bool) {
    let samples = samples(board);
    let pairs = pin_pairs(board);
    let segments = segments(board, &samples, &pairs);
    inputs_section(sections, board, stage, &samples, &pairs);

    let line = section(sections, board, stage, "checkShape");
    push(sections, line.end());
    for (i, s) in samples.iter().enumerate() {
        let area = point_octagon_area(&int_point(s.x, s.y), s.hw);
        let ok = board.check_shape(&area, s.layer, &[s.net], s.cl);
        push(
            sections,
            Line::new().num("i", i as i64).bool("ok", ok).end(),
        );
    }

    let line = section(sections, board, stage, "checkTraceShape");
    push(sections, line.end());
    for g in &segments {
        if g.from == g.to {
            push(sections, segment_line(g).bool("skip", true).end());
            continue;
        }
        let polyline = Polyline::from_corners(&g.from, &g.to);
        let shape = polyline.offset_shape(g.hw, 0).expect("a segment shape");
        let pins =
            board.touching_pins_at_polyline_end_corners(&polyline, g.layer, g.hw, &[g.net], g.cl);
        let ok = board.check_trace_shape(&shape, g.layer, &[g.net], g.cl, Some(&pins));
        push(
            sections,
            segment_line(g)
                .raw("pins", &ids(&pins))
                .bool("ok", ok)
                .end(),
        );
    }

    let line = section(sections, board, stage, "checkPolylineTrace");
    push(sections, line.end());
    for g in &segments {
        if g.from == g.to {
            push(sections, segment_line(g).bool("skip", true).end());
            continue;
        }
        let polyline = Polyline::from_corners(&g.from, &g.to);
        let ok = board.check_polyline_trace(&polyline, g.layer, g.hw, &[g.net], g.cl);
        let line = segment_line(g)
            .bool("ok", ok)
            .num("maxGeneratedId", max_generated_id(board));
        push(sections, line.end());
    }

    let line = section(sections, board, stage, "checkTraceSegment");
    push(sections, line.end());
    for g in &segments {
        let nets = [g.net];
        let point: Vec<String> = [false, true]
            .iter()
            .map(|only| {
                let value =
                    board.check_trace_segment(&g.from, &g.to, g.layer, &nets, g.hw, g.cl, *only);
                bits(value)
            })
            .collect();
        let mut line = segment_line(g).raw("point", &strings(&point));
        if g.from != g.to {
            let segment = LineSegment::from_polyline(&Polyline::from_corners(&g.from, &g.to), 1);
            let values: Vec<String> = [false, true]
                .iter()
                .map(|only| {
                    let value =
                        board.check_trace_segment_line(&segment, g.layer, &nets, g.hw, g.cl, *only);
                    bits(value)
                })
                .collect();
            line = line.raw("segment", &strings(&values));
        }
        push(sections, line.end());
    }

    pick_sections(sections, board, stage, &segments);
    items_section(sections, board, stage);
    if offsets {
        offset_shapes_section(sections, board, stage);
    }
}

fn inputs_section(
    sections: &mut Sections,
    board: &RoutingBoard,
    stage: &str,
    samples: &[Sample],
    pairs: &[Pair],
) {
    let bb = board.bounding_box();
    let multi_net_pins: Vec<ItemId> = board
        .pins()
        .into_iter()
        .filter(|id| board.item(*id).expect("pin").net_count() > 1)
        .collect();
    let line = section(sections, board, stage, "inputs")
        .num("layers", board.layer_count() as i64)
        .num("maxNetNo", i64::from(board.rules.nets.max_net_number()))
        .num("maxTraceHalfWidth", i64::from(board.max_trace_half_width()))
        .num("classes", board.rules.clearance_matrix.class_count() as i64)
        .raw(
            "box",
            &format!("[{},{},{},{}]", bb.ll.x, bb.ll.y, bb.ur.x, bb.ur.y),
        )
        .bool(
            "compensated",
            board.search_trees().is_clearance_compensation_used(),
        )
        .raw("multiNetPins", &ids(&multi_net_pins))
        .num("samples", samples.len() as i64)
        .num("pairs", pairs.len() as i64);
    push(sections, line.end());
    for (i, s) in samples.iter().enumerate() {
        let line = Line::new()
            .num("i", i as i64)
            .num("x", i64::from(s.x))
            .num("y", i64::from(s.y))
            .num("layer", i64::from(s.layer))
            .num("net", i64::from(s.net))
            .num("hw", i64::from(s.hw))
            .num("cl", i64::from(s.cl))
            .num("x2", i64::from(s.x2))
            .num("y2", i64::from(s.y2));
        push(sections, line.end());
    }
    for (k, p) in pairs.iter().enumerate() {
        let line = Line::new()
            .num("pair", k as i64)
            .num("a", i64::from(p.a.0))
            .num("b", i64::from(p.b.0))
            .num("layer", i64::from(p.layer))
            .num("net", i64::from(p.net))
            .num("hw", i64::from(p.hw))
            .num("cl", i64::from(p.cl));
        push(sections, line.end());
    }
}

fn highest_pick(board: &RoutingBoard, g: &Segment) -> Option<ItemId> {
    board.pick_items(&g.from, g.layer).into_iter().next()
}

fn pick_sections(sections: &mut Sections, board: &RoutingBoard, stage: &str, picks: &[Segment]) {
    let line = section(sections, board, stage, "pickItems");
    push(sections, line.end());
    for g in picks {
        let mut line = segment_line(g).raw(
            "none",
            &ids(&board.pick_items_filtered(&g.from, g.layer, None)),
        );
        for choice in SelectableChoice::ALL {
            let filter = ItemFilter::only(choice);
            let picked = board.pick_items_filtered(&g.from, g.layer, Some(&filter));
            line = line.raw(choice.java_name(), &ids(&picked));
        }
        let picked = board.pick_items_filtered(&g.from, g.layer, Some(&ItemFilter::default()));
        push(sections, line.raw("default", &ids(&picked)).end());
    }

    let line = section(sections, board, stage, "pickNearestRoutingItem");
    push(sections, line.end());
    for g in picks {
        let nearest = board.pick_nearest_routing_item(&g.from, g.layer, None);
        let mut line = segment_line(g).num("nearest", nearest.map_or(-1, |id| i64::from(id.0)));
        if let Some(n) = nearest {
            let from = board.pick_nearest_routing_item(&g.from, g.layer, Some(n));
            line = line.num("from", from.map_or(-1, |id| i64::from(id.0)));
        }
        push(sections, line.end());
    }

    let line = section(sections, board, stage, "checkMoveItem");
    push(sections, line.end());
    for g in picks {
        let item = highest_pick(board, g);
        let mut line = segment_line(g).num("item", item.map_or(-1, |id| i64::from(id.0)));
        if let Some(id) = item {
            let (Point::Int(a), Point::Int(b)) = (&g.from, &g.to) else {
                panic!("integer sample points");
            };
            let vector = Vector::Int(IntVector {
                x: b.x - a.x,
                y: b.y - a.y,
            });
            let mut ignore = BTreeSet::new();
            line = line
                .bool("plain", board.check_move_item(id, &vector, None))
                .bool(
                    "ignore",
                    board.check_move_item(id, &vector, Some(&mut ignore)),
                )
                .raw("ignored", &ids(&ignore));
        }
        push(sections, line.end());
    }

    let line = section(sections, board, stage, "checkChangeNet");
    push(sections, line.end());
    for g in picks {
        let item = highest_pick(board, g);
        let mut line = segment_line(g).num("item", item.map_or(-1, |id| i64::from(id.0)));
        if let Some(id) = item {
            line = line.bool("ok", board.check_change_net(id, g.net));
        }
        push(sections, line.end());
    }
}

fn java_type(item: &Item) -> &'static str {
    match item.kind {
        ItemKind::Pin(_) => "Pin",
        ItemKind::Via(_) => "Via",
        _ => "?",
    }
}

fn items_section(sections: &mut Sections, board: &RoutingBoard, stage: &str) {
    let line = section(sections, board, stage, "items");
    push(sections, line.end());
    let tree = board.search_trees().default_tree();
    let bb = board.bounding_box();
    let layers = board.layer_count() as i32;
    for item in board.items() {
        let id = item.id();
        let mut line = Line::new().num("item", i64::from(id.0));
        match &item.kind {
            ItemKind::Pin(_) | ItemKind::Via(_) => {
                let drill = item.drill().expect("drill");
                line = line
                    .str("type", java_type(item))
                    .dbl("minWidth", board.min_width(id).expect("a drill item"))
                    .str(
                        "conn",
                        &shape_text(board.trace_connection_shape(tree, id, 0).as_ref()),
                    );
                let tiles: Vec<String> = (drill.first_layer..=drill.last_layer)
                    .map(|l| shape_text(board.tile_shape_on_layer(id, l).as_ref()))
                    .collect();
                line = line.raw("tiles", &strings(&tiles));
                if item.is_pin() {
                    line = line.bool("drillAllowed", item.pin_drill_allowed());
                }
            }
            ItemKind::Trace(trace) => {
                let corner = |p: IntPoint| {
                    point_text(&board.nearest_end_point(id, &Point::Int(p)).expect("trace"))
                };
                let conn: Vec<String> = (0..trace.tile_shape_count())
                    .map(|i| shape_text(board.trace_connection_shape(tree, id, i).as_ref()))
                    .collect();
                line = line
                    .str("type", "PolylineTrace")
                    .num(
                        "compensatedHalfWidth",
                        i64::from(board.compensated_half_width(tree, id).expect("trace")),
                    )
                    .str("nearestLl", &corner(bb.ll))
                    .str("nearestUr", &corner(bb.ur))
                    .raw(
                        "touchingPins",
                        &ids(&board.touching_pins_at_end_corners(id)),
                    )
                    .raw("conn", &strings(&conn));
            }
            ItemKind::Area(_) if item.is_conduction_area() => {
                let count = tree.tree_shape_count(item, &board.shape_context());
                let conn: Vec<String> = (0..count)
                    .map(|i| shape_text(board.trace_connection_shape(tree, id, i).as_ref()))
                    .collect();
                line = line
                    .str("type", "ConductionArea")
                    .raw("conn", &strings(&conn));
            }
            _ => continue,
        }
        let connected: Vec<String> = (0..layers)
            .filter(|l| board.is_connected_on_layer(id, *l))
            .map(|l| l.to_string())
            .collect();
        line = line.raw("connectedOnLayers", &format!("[{}]", connected.join(",")));
        push(sections, line.end());
    }
}

/// `ShapeSearchTree.offsetShapes(polyline, hw, from, to)` of each tree kind (Java
/// `ShapeSearchTree.java:1204-1218`, `ShapeSearchTree90Degree.java:507-520`; the 45 degree
/// tree inherits the plain one), from the geometry the trees use.
// TODO(B02.6): call the trees' public `offset_shapes` once B02.6 has made it public.
fn tree_offset_shapes(
    kind: TreeKind,
    polyline: &Polyline,
    hw: i32,
    from: i32,
    to: i32,
) -> Vec<TileShape> {
    // Java clamps `fromNo` to 0; a negative `toNo` gives no shapes, as 0 does.
    ShapeSearchTree::new(kind, 0).offset_shapes(
        polyline,
        hw,
        from.max(0) as usize,
        to.max(0) as usize,
    )
}

fn offset_shapes_section(sections: &mut Sections, board: &RoutingBoard, stage: &str) {
    let line = section(sections, board, stage, "offsetShapes");
    push(sections, line.end());
    let bb = board.bounding_box();
    let width = bb.ur.x - bb.ll.x;
    let height = bb.ur.y - bb.ll.y;
    let r = width.min(height) / 10;
    let max_half_width = board.max_trace_half_width();
    let kinds = [
        board.search_trees().default_tree().kind(),
        TreeKind::FortyfiveDegree,
        TreeKind::NinetyDegree,
    ];
    let mut random = JavaRandom::new(SEED);
    for k in 0..OFFSET_POLYLINES {
        let count = 2 + random.next_int(4);
        let mut x = bb.ll.x + random.next_int(width + 1);
        let mut y = bb.ll.y + random.next_int(height + 1);
        let mut corners = Vec::new();
        let mut echo = Vec::new();
        for c in 0..count {
            if c > 0 {
                x = x - r + random.next_int(2 * r + 1);
                y = y - r + random.next_int(2 * r + 1);
            }
            corners.push(int_point(x, y));
            echo.push(format!("{x},{y}"));
        }
        let hw = 1 + random.next_int(2 * max_half_width);
        let polyline = Polyline::from_points(&corners);
        let n = polyline.lines().len() as i32;
        let from = random.next_int(n + 2) - 1;
        let to = from + random.next_int(n + 2);
        let mut line = Line::new()
            .num("k", k as i64)
            .raw("corners", &format!("[{}]", echo.join(",")))
            .num("hw", i64::from(hw))
            .num("lines", i64::from(n))
            .num("from", i64::from(from))
            .num("to", i64::from(to));
        for (name, kind) in ["plain", "deg45", "deg90"].iter().zip(&kinds) {
            line = line.raw(
                name,
                &shapes_text(&tree_offset_shapes(*kind, &polyline, hw, from, to)),
            );
        }
        push(sections, line.end());
    }
}

// ---------------------------------------------------------------------------------------------
// Boards and comparison
// ---------------------------------------------------------------------------------------------

/// A board of the probe: its row, its session and whether it is the tie-pin board.
struct ProbeBoard {
    row: Row,
    ses: Option<String>,
}

/// The `PLAN` boards, then the boards with committed output that `PLAN` does not name (the
/// first fixture with a multi-net pin, which the probe finds itself and probes with the
/// session of its `fixtures.tsv` row).
fn probe_boards() -> Vec<ProbeBoard> {
    let mut result: Vec<ProbeBoard> = PLAN
        .iter()
        .map(|(board, ses)| ProbeBoard {
            row: board_probe::fixture(board),
            ses: ses.map(str::to_string),
        })
        .collect();
    for board in committed_boards() {
        if !PLAN.iter().any(|(b, _)| *b == board) {
            let row = board_probe::fixture(&board);
            let ses = row.ses.clone();
            result.push(ProbeBoard { row, ses });
        }
    }
    result
}

/// The boards with a committed `board-queries` output.
fn committed_boards() -> Vec<String> {
    let base = repo_root().join("fixtures/reference/probe").join(KIND);
    let mut result = Vec::new();
    collect_boards(&base, &base, &mut result);
    result.sort();
    result
}

fn collect_boards(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_boards(base, &path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && (ext == "jsonl" || ext == "digest")
        {
            let rel = path
                .strip_prefix(base)
                .expect("below base")
                .with_extension("");
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn load(board: &ProbeBoard) -> RoutingBoard {
    let mut routing_board = board_probe::load(&board.row);
    if let Some(ses) = &board.ses {
        let row = Row {
            ses: Some(ses.clone()),
            ..board.row.clone()
        };
        board_probe::import_session(&mut routing_board, &row);
    }
    routing_board
}

/// The Rust header and sections of `board`, as the probe writes them.
fn rust_output(board: &ProbeBoard) -> (String, Sections) {
    let mut sections = Sections::new();
    let is_j2 = board.row.board == J2;
    let stage = if board.ses.is_some() { "ses" } else { "load" };
    let mut routing_board = load(board);
    run_stage(&mut sections, &mut routing_board, stage, is_j2);
    if is_j2 {
        let mut compensated = load(board);
        compensated.set_clearance_compensation_used(true);
        run_stage(&mut sections, &mut compensated, "compensated", false);
    }
    (header(KIND, &board.row.board), sections)
}

/// The committed Java output of a board: the header and each section, as lines or as the
/// digest line.
struct JavaOutput {
    header: String,
    lines: BTreeMap<String, Vec<String>>,
    digests: BTreeMap<String, String>,
}

fn java_output(board: &str) -> Option<JavaOutput> {
    let base = repo_root().join("fixtures/reference/probe").join(KIND);
    let mut output = JavaOutput {
        header: String::new(),
        lines: BTreeMap::new(),
        digests: BTreeMap::new(),
    };
    if let Ok(text) = std::fs::read_to_string(base.join(format!("{board}.jsonl"))) {
        let mut current: Option<String> = None;
        for (i, line) in text.lines().enumerate() {
            if i == 0 {
                output.header = line.to_string();
            } else if line.starts_with("{\"stage\":\"") {
                let name = line.split('"').nth(3).unwrap_or_default().to_string();
                output.lines.insert(name.clone(), vec![line.to_string()]);
                current = Some(name);
            } else if let Some(name) = &current {
                output
                    .lines
                    .get_mut(name)
                    .expect("section")
                    .push(line.to_string());
            } else {
                output
                    .lines
                    .entry(String::new())
                    .or_default()
                    .push(line.to_string());
            }
        }
        return Some(output);
    }
    let text = std::fs::read_to_string(base.join(format!("{board}.digest"))).ok()?;
    for (i, line) in text.lines().enumerate() {
        if i == 0 {
            output.header = line.to_string();
        } else {
            let name = line.split(' ').next().unwrap_or_default().to_string();
            output.digests.insert(name, line.to_string());
        }
    }
    Some(output)
}

/// The outputs of every probe board, computed once for all tests of this file.
fn outputs() -> &'static Vec<(ProbeBoard, String, Sections)> {
    static OUTPUTS: OnceLock<Vec<(ProbeBoard, String, Sections)>> = OnceLock::new();
    OUTPUTS.get_or_init(|| {
        probe_boards()
            .into_iter()
            .map(|board| {
                let (header, sections) = rust_output(&board);
                if let Ok(dir) = std::env::var("BOARD_PROBE_OUT") {
                    write_output(&board, &header, &sections, Path::new(&dir).join(KIND));
                }
                (board, header, sections)
            })
            .collect()
    })
}

fn write_output(board: &ProbeBoard, header: &str, sections: &Sections, dir: std::path::PathBuf) {
    let path = dir.join(format!("{}.jsonl", board.row.board));
    std::fs::create_dir_all(path.parent().expect("parent")).expect("output directory");
    let mut text = format!("{header}\n");
    for (_, lines) in sections {
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
    }
    std::fs::write(path, text).expect("probe output");
}

fn header_identity(line: &str) -> String {
    line.split(",\"javaCommit\"")
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Compares section `<stage>:<method>` of every stage of every board; returns the
/// differences not listed in `EXPECTED_MISMATCHES`.
fn compare_method(method: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for (board, header, sections) in outputs() {
        let name = &board.row.board;
        let Some(java) = java_output(name) else {
            failures.push(format!(
                "{KIND}/{name}: no Java output; generate it with {}",
                board_probe::probe_command(KIND, &board.row)
            ));
            continue;
        };
        if header_identity(&java.header) != header_identity(header) {
            failures.push(format!("{KIND}/{name}: header {} vs {header}", java.header));
            continue;
        }
        let mut compared = 0;
        for (section, rust) in sections {
            if section.split(':').nth(1) != Some(method) {
                continue;
            }
            compared += 1;
            if let Err(message) = compare_section(name, &java, section, rust) {
                let listed = EXPECTED_MISMATCHES
                    .iter()
                    .any(|(b, s, _)| b == name && s == section);
                if !listed {
                    failures.push(message);
                }
            }
        }
        let java_count = java
            .lines
            .keys()
            .chain(java.digests.keys())
            .filter(|s| s.split(':').nth(1) == Some(method))
            .count();
        if java_count != compared {
            failures.push(format!(
                "{KIND}/{name}: Java has {java_count} {method} sections, Rust {compared}"
            ));
        }
    }
    failures
}

fn compare_section(
    board: &str,
    java: &JavaOutput,
    section: &str,
    rust: &[String],
) -> Result<(), String> {
    let out = repo_root()
        .join("target/probe")
        .join(KIND)
        .join(format!("{board}.{}.jsonl", section.replace(':', ".")));
    if let Some(java_lines) = java.lines.get(section) {
        let n = java_lines.len().max(rust.len());
        let Some(index) = (0..n).find(|&i| java_lines.get(i) != rust.get(i)) else {
            return Ok(());
        };
        write_lines(&out, rust);
        let context: Vec<String> = rust[index.saturating_sub(3)..index.min(rust.len())]
            .iter()
            .map(|l| format!("    {l}"))
            .collect();
        return Err(format!(
            "{KIND}/{board} {section}: line {} differs\n  Java: {}\n  Rust: {}\n  before it:\n{}\n  Rust section: {}",
            index + 1,
            java_lines.get(index).map_or("<end>", String::as_str),
            rust.get(index).map_or("<end>", String::as_str),
            context.join("\n"),
            out.display()
        ));
    }
    let Some(java_digest) = java.digests.get(section) else {
        return Err(format!("{KIND}/{board}: Java has no section {section}"));
    };
    let mut lines = vec![String::new()];
    lines.extend(rust.iter().cloned());
    let rust_digest = board_probe::digest(&lines).pop().unwrap_or_default();
    if &rust_digest == java_digest {
        return Ok(());
    }
    write_lines(&out, rust);
    Err(format!(
        "{KIND}/{board} {section}: digest differs\n  Java: {java_digest}\n  Rust: {rust_digest}\n  Rust section: {}",
        out.display()
    ))
}

fn write_lines(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("output directory");
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(path, text).expect("section output");
}

fn assert_method(method: &str) {
    let failures = compare_method(method);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

/// B04.1: the Rust sampler reproduces Java's samples and pin pairs on every board.
#[test]
fn inputs_match_java() {
    assert_method("inputs");
}

/// B04.1: the probe ran on the `PLAN` boards plus exactly one more, the first fixture with a
/// multi-net pin, and every probed board has its committed output.
#[test]
fn probed_boards_match_java() {
    let committed = committed_boards();
    let extra: Vec<&String> = committed
        .iter()
        .filter(|b| !PLAN.iter().any(|(p, _)| p == b))
        .collect();
    assert_eq!(extra.len(), 1, "one tie-pin board, got {extra:?}");
    for (board, _) in PLAN {
        assert!(
            committed.iter().any(|b| b == board),
            "{board} has no output"
        );
    }
    let tie = probe_boards().pop().expect("a board");
    let routing_board = load(&tie);
    assert!(
        routing_board
            .pins()
            .iter()
            .any(|id| routing_board.item(*id).expect("pin").net_count() > 1),
        "{} has no multi-net pin",
        tie.row.board
    );
}

/// B04.2: `pick_items_filtered` with no filter, each single-choice filter and the default one.
#[test]
fn pick_items_filtered_matches_java() {
    assert_method("pickItems");
}

/// B04.3.
#[test]
fn check_shape_matches_java() {
    assert_method("checkShape");
}

/// B04.3 (with B04.4's touching pins of the unsaved trace as contact pins).
#[test]
fn check_trace_shape_matches_java() {
    assert_method("checkTraceShape");
}

/// B04.4: the booleans and `maxGeneratedId` after every call.
#[test]
fn check_polyline_trace_matches_java() {
    assert_method("checkPolylineTrace");
}

/// B04.5: both forms, both `only_not_shovable` values, bit for bit.
#[test]
fn check_trace_segment_matches_java() {
    assert_method("checkTraceSegment");
}

/// B04.6.
#[test]
fn pick_nearest_routing_item_matches_java() {
    assert_method("pickNearestRoutingItem");
}

/// B04.6.
#[test]
fn check_move_item_matches_java() {
    assert_method("checkMoveItem");
}

/// B04.6.
#[test]
fn check_change_net_matches_java() {
    assert_method("checkChangeNet");
}

/// B04.7: the helpers of every pin, via, trace and conduction area.
#[test]
fn item_helpers_match_java() {
    assert_method("items");
}

/// B04.7: `offsetShapes` of the three tree kinds on 200 seeded polylines.
#[test]
fn offset_shapes_match_java() {
    assert_method("offsetShapes");
}

/// B04.6: `check_move_item` adds the item to the caller's ignore set, also when the move
/// fails.
#[test]
fn check_move_item_adds_item_to_ignore_set() {
    let board = load(&ProbeBoard {
        row: board_probe::fixture(J2),
        ses: Some("fixtures/Issue026-J2_reference.ses".to_string()),
    });
    let via = board.vias()[0];
    let zero = Vector::Int(IntVector::ZERO);
    let far = Vector::Int(IntVector {
        x: i32::MAX / 2,
        y: 0,
    });
    let mut ignore = BTreeSet::new();
    let _ = board.check_move_item(via, &zero, Some(&mut ignore));
    assert_eq!(ignore, BTreeSet::from([via]));
    let mut ignore = BTreeSet::from([ItemId(1)]);
    assert!(!board.check_move_item(via, &far, Some(&mut ignore)));
    assert_eq!(ignore, BTreeSet::from([via, ItemId(1)]));
    // Multi-net items and connected traces are refused before the item is added.
    let trace = board
        .traces()
        .into_iter()
        .find(|t| board.is_connected(*t))
        .expect("a connected trace");
    let mut ignore = BTreeSet::new();
    assert!(!board.check_move_item(trace, &zero, Some(&mut ignore)));
    assert!(ignore.is_empty());
}

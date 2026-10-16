//! The shove-check building blocks against Java (O03.1-O03.3).
//!
//! Probes `entry-side` (`EntrySideProbe.java`: the four `ShapeEntrySide`
//! constructors and `ShapeAndEntrySide`) and `trace-entries`
//! (`TraceEntriesProbe.java`: `ShapeTraceEntries.storeItems` with its
//! results, the substitute pieces, and the `cutout` stage of
//! `cutoutTraces` with the default tree's walk hash after every 10 cuts), on
//! the shove sampler of `tests/support/shove_sampler.rs`. Every line is
//! compared, doubles by raw bits.

mod board_probe;
#[path = "support/shove_sampler.rs"]
mod shove_sampler;

use board_probe::{Line, Row, header};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::datastructures::IdGenerator;
use freerouting::drc::DesignRulesChecker;
use freerouting::shove::{CutoutResult, ShapeAndEntrySide, ShapeEntrySide, ShapeTraceEntries};
use shove_sampler::{
    Sample, float_point_text, ids_text, ints_text, lines_text, samples, shape_text,
};

const ENTRY_SIDE: &str = "entry-side";
const TRACE_ENTRIES: &str = "trace-entries";
const ENTRY_SIDE_SAMPLES: usize = 300;
const STORE_SAMPLES: usize = 300;
const CUTOUT_SAMPLES: usize = 100;

/// The probe's boards: `(board, with the fixtures.tsv session)`.
const ENTRY_SIDE_BOARDS: &[(&str, bool)] =
    &[("Issue026-J2_reference", true), ("Issue413-test", false)];
const TRACE_ENTRIES_BOARDS: &[(&str, bool)] = &[
    ("Issue026-J2_reference", true),
    ("Issue413-test", false),
    ("Issue676-ch32v-tx118s", false),
    ("Issue957-smoke-ldo-led", false),
];

fn load(board: &str, with_session: bool) -> (Row, RoutingBoard) {
    let row = board_probe::fixture(board);
    let mut routing_board = board_probe::load(&row);
    if with_session {
        board_probe::import_session(&mut routing_board, &row);
    }
    (row, routing_board)
}

fn side_line(line: Line, side: Option<ShapeEntrySide>) -> Line {
    match side {
        None => line.str("side", "null"),
        Some(side) => line
            .num("no", side.no.map_or(-1, |n| n as i64))
            .str("at", &float_point_text(side.border_intersection)),
    }
}

fn sample_line(i: usize, s: &Sample) -> String {
    Line::new()
        .num("i", i as i64)
        .num("trace", i64::from(s.trace.0))
        .num("segment", s.segment as i64)
        .str("from", &format!("{},{}", s.from.x, s.from.y))
        .str("to", &format!("{},{}", s.to.x, s.to.y))
        .str("shape", &shape_text(s.shape.as_ref()))
        .end()
}

// ---------------------------------------------------------------------------------------------
// entry-side (O03.1)
// ---------------------------------------------------------------------------------------------

fn entry_side_lines(board_name: &str, board: &RoutingBoard) -> Vec<String> {
    let mut lines = vec![header(ENTRY_SIDE, board_name)];
    for (i, s) in samples(board, ENTRY_SIDE_SAMPLES).iter().enumerate() {
        lines.push(sample_line(i, s));
        let (Some(shape), Some(segment)) = (&s.shape, &s.line_segment) else {
            continue;
        };
        let item = board.item(s.trace).expect("trace");
        let polyline = &item.as_trace().expect("trace").polyline;
        let ctor = |name: &str, side: ShapeEntrySide| {
            side_line(Line::new().num("i", i as i64).str("ctor", name), Some(side)).end()
        };
        lines.push(ctor(
            "polyline",
            ShapeEntrySide::from_polyline(polyline, s.segment, shape),
        ));
        lines.push(ctor(
            "point",
            ShapeEntrySide::from_point(&polyline.corner(s.segment), shape),
        ));
        lines.push(ctor(
            "left",
            ShapeEntrySide::from_segment(segment, shape, true),
        ));
        lines.push(ctor(
            "right",
            ShapeEntrySide::from_segment(segment, shape, false),
        ));
        lines.push(ctor(
            "values",
            ShapeEntrySide::from_values(Some(s.segment), Some(polyline.corner_approx(s.segment))),
        ));
        for k in 0..4 {
            let orthogonal = k >= 2;
            let check = k % 2 == 1;
            let line = Line::new()
                .num("i", i as i64)
                .str("ctor", "shapeAndSide")
                .bool("orthogonal", orthogonal)
                .bool("check", check);
            let line = match ShapeAndEntrySide::new(board, item, s.segment - 1, orthogonal, check) {
                Some(result) => side_line(
                    line.str("shape", &shape_text(Some(&result.shape))),
                    result.from_side,
                ),
                None => line.bool("error", true),
            };
            lines.push(line.end());
        }
    }
    lines
}

#[test]
fn entry_sides_match_java() {
    let mut failures = Vec::new();
    for (board_name, with_session) in ENTRY_SIDE_BOARDS {
        let (row, board) = load(board_name, *with_session);
        let lines = entry_side_lines(&row.board, &board);
        if let Err(e) = board_probe::compare(ENTRY_SIDE, &row, &lines) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------------------------
// trace-entries (O03.2, O03.3)
// ---------------------------------------------------------------------------------------------

fn max_generated_id(board: &RoutingBoard) -> i64 {
    i64::from(board.communication.id_generator.max_generated_id())
}

/// The probe's per-sample variants: the from side (`i % 3`: left, right,
/// none), the pad check (`i % 5 == 4`) and copper sharing (`i % 7 != 6`).
fn store_variant(
    i: usize,
    segment: &freerouting::geometry::LineSegment,
    shape: &freerouting::geometry::TileShape,
) -> (Option<ShapeEntrySide>, bool, bool) {
    let from_side = match i % 3 {
        0 => Some(ShapeEntrySide::from_segment(segment, shape, true)),
        1 => Some(ShapeEntrySide::from_segment(segment, shape, false)),
        _ => None,
    };
    (from_side, i % 5 == 4, i % 7 != 6)
}

/// The probe's `foreign`: the net after the first net of `nets` (wrapping),
/// so that the sampled trace is an obstacle of another net.
fn foreign(board: &RoutingBoard, nets: &[i32]) -> Vec<i32> {
    let max = board.rules.nets.max_net_number();
    let first = nets.first().copied().unwrap_or(0);
    vec![if max > 0 { first % max + 1 } else { 1 }]
}

fn store_lines(board: &mut RoutingBoard, lines: &mut Vec<String>) {
    lines.push(
        Line::new()
            .str("stage", "store")
            .num("maxGeneratedId", max_generated_id(board))
            .end(),
    );
    for (i, s) in samples(board, STORE_SAMPLES).iter().enumerate() {
        lines.push(sample_line(i, s));
        let (Some(shape), Some(segment)) = (&s.shape, &s.line_segment) else {
            continue;
        };
        let (from_side, pad_check, sharing) = store_variant(i, segment, shape);
        let obstacles: Vec<ItemId> = board
            .overlapping_items_with_clearance(shape, s.layer, &[], s.clearance_class)
            .into_iter()
            .collect();
        let nets = if i % 2 == 0 {
            s.nets.clone()
        } else {
            foreign(board, &s.nets)
        };
        let mut entries =
            ShapeTraceEntries::new(shape.clone(), s.layer, &nets, s.clearance_class, from_side);
        let ok = entries.store_items(board, obstacles.iter().copied(), pad_check, sharing);
        lines.push(
            Line::new()
                .num("i", i as i64)
                .raw("obstacles", &ids_text(obstacles.iter().copied()))
                .bool("ok", ok)
                .num(
                    "found",
                    entries.found_obstacle().map_or(-1, |id| i64::from(id.0)),
                )
                .bool("tails", entries.trace_tails_in_shape())
                .num("depth", i64::from(entries.stack_depth()))
                .num("pieces", i64::from(entries.substitute_trace_count()))
                .raw("vias", &ids_text(entries.shove_via_list.iter().copied()))
                .end(),
        );
        if ok {
            let mut k = 0;
            while let Some(piece) = entries.next_substitute_trace_piece(board) {
                let trace = piece.as_trace().expect("piece");
                lines.push(
                    Line::new()
                        .num("i", i as i64)
                        .num("piece", k)
                        .num("id", i64::from(piece.id().0))
                        .num("hw", i64::from(trace.half_width))
                        .raw("nets", &ints_text(piece.net_numbers()))
                        .str("lines", &lines_text(&trace.polyline))
                        .end(),
                );
                k += 1;
            }
        }
        lines.push(
            Line::new()
                .num("i", i as i64)
                .num("maxGeneratedId", max_generated_id(board))
                .end(),
        );
    }
}

/// How the cutouts of one board went.
#[derive(Debug, Default)]
struct CutoutCounts {
    fast: usize,
    slow: usize,
    untouched: usize,
}

fn cutout_lines(board: &mut RoutingBoard, lines: &mut Vec<String>, counts: &mut CutoutCounts) {
    lines.push(
        Line::new()
            .str("stage", "cutout")
            .num("maxGeneratedId", max_generated_id(board))
            .end(),
    );
    for (k, s) in samples(board, CUTOUT_SAMPLES).iter().enumerate() {
        let Some(shape) = &s.shape else {
            continue;
        };
        let before = board.communication.id_generator.max_generated_id();
        let obstacles: Vec<ItemId> = board
            .overlapping_items_with_clearance(shape, s.layer, &[], s.clearance_class)
            .into_iter()
            .collect();
        let nets = foreign(board, &s.nets);
        let entries =
            ShapeTraceEntries::new(shape.clone(), s.layer, &nets, s.clearance_class, None);
        for result in entries.cutout_traces(board, obstacles.iter().copied()) {
            match result {
                CutoutResult::Fast { start, end } => {
                    assert!(start.0 < end.0, "fast cutout piece ids {start} < {end}");
                    counts.fast += 1;
                }
                CutoutResult::Slow(_) => counts.slow += 1,
                CutoutResult::Untouched | CutoutResult::NotOnBoard => counts.untouched += 1,
            }
        }
        let gone: Vec<ItemId> = obstacles
            .iter()
            .copied()
            .filter(|id| !board.item(*id).is_some_and(|i| i.is_on_the_board()))
            .collect();
        let mut new_traces: Vec<ItemId> = board
            .traces()
            .into_iter()
            .filter(|id| id.0 > before)
            .collect();
        new_traces.sort_by_key(|id| id.0);
        let texts: Vec<String> = new_traces
            .iter()
            .map(|id| {
                let trace = board.item(*id).and_then(|i| i.as_trace()).expect("trace");
                format!("\"{}:{}\"", id.0, lines_text(&trace.polyline))
            })
            .collect();
        lines.push(
            Line::new()
                .num("k", k as i64)
                .raw("obstacles", &ids_text(obstacles.iter().copied()))
                .raw("gone", &ids_text(gone))
                .raw("new", &format!("[{}]", texts.join(",")))
                .num("maxGeneratedId", max_generated_id(board))
                .end(),
        );
        if k % 10 == 9 {
            let fingerprint = board.search_trees().default_tree().fingerprint();
            lines.push(
                Line::new()
                    .num("k", k as i64)
                    .str("walk", &board_probe::hex(fingerprint.shape_hash))
                    .num("nodes", fingerprint.nodes as i64)
                    .num("leaves", fingerprint.leaves as i64)
                    .end(),
            );
        }
    }
}

/// `(board, first Rust line of the sample where Java throws, reason)`: Java
/// exceptions in the `cutout` stage. Java's probe stops at the exception, so
/// the Rust lines from that sample on are replaced by Java's exception line
/// (which must name the expected exception); every line before it is
/// compared. Each has a line in `docs/DIVERGENCES.md` ("The fast cutout").
const JAVA_CUTOUT_EXCEPTIONS: &[(&str, &str, &str)] = &[(
    "Issue026-J2_reference",
    "{\"k\":37,",
    "Java's fastCutoutTrace throws a NullPointerException in \
     ShapeSearchTree.reuseEntriesAfterCutout (:398, the end piece's leaves): the cut trace, \
     an earlier cutout piece, has a zero-length segment without a tree leaf; the port skips \
     the missing leaf",
)];

fn trace_entries_lines(board_name: &str, with_session: bool) -> (Row, Vec<String>, CutoutCounts) {
    let (row, mut board) = load(board_name, with_session);
    let mut lines = vec![header(TRACE_ENTRIES, &row.board)];
    store_lines(&mut board, &mut lines);
    let (_, mut fresh) = load(board_name, with_session);
    let mut counts = CutoutCounts::default();
    cutout_lines(&mut fresh, &mut lines, &mut counts);
    if let Some((_, prefix, _)) = JAVA_CUTOUT_EXCEPTIONS
        .iter()
        .find(|(b, _, _)| *b == row.board)
    {
        let at = lines
            .iter()
            .position(|l| l.starts_with(prefix))
            .unwrap_or_else(|| panic!("{}: no Rust line {prefix}", row.board));
        let java = std::fs::read_to_string(
            board_probe::repo_root()
                .join("fixtures/reference/probe")
                .join(TRACE_ENTRIES)
                .join(format!("{}.jsonl", row.board)),
        )
        .expect("Java output");
        let exception = java.lines().nth(at).unwrap_or_default().to_string();
        assert!(
            exception.contains("NullPointerException")
                && exception.contains("reuseEntriesAfterCutout"),
            "{}: Java line {} is not the expected exception: {exception}",
            row.board,
            at + 1
        );
        lines.truncate(at);
        lines.push(exception);
    }
    (row, lines, counts)
}

#[test]
fn trace_entries_match_java() {
    let mut failures = Vec::new();
    for (board_name, with_session) in TRACE_ENTRIES_BOARDS {
        let (row, lines, counts) = trace_entries_lines(board_name, *with_session);
        println!("{}: {} lines, cutouts {counts:?}", row.board, lines.len());
        if let Err(e) = board_probe::compare(TRACE_ENTRIES, &row, &lines) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// O03.3: the fast cutout puts its pieces straight into the item list, as
/// Java does: the revision moves only for the removed trace, the trace
/// half-width bounds stay, and the routing ledger still equals a full
/// recount (B01.6), because the port notes both pieces as changed.
#[test]
fn fast_cutout_skips_insert_side_effects() {
    let (_, mut board) = load("Issue413-test", false);
    let mut checked = 0;
    for s in samples(&board, CUTOUT_SAMPLES) {
        let Some(shape) = &s.shape else {
            continue;
        };
        let obstacles: Vec<ItemId> = board
            .overlapping_items_with_clearance(shape, s.layer, &[], s.clearance_class)
            .into_iter()
            .collect();
        let nets = foreign(&board, &s.nets);
        let entries =
            ShapeTraceEntries::new(shape.clone(), s.layer, &nets, s.clearance_class, None);
        let revision = board.revision();
        let bounds = (board.max_trace_half_width(), board.min_trace_half_width());
        let _ = board.ledger_incomplete_count();
        let results = entries.cutout_traces(&mut board, obstacles.iter().copied());
        if results.is_empty()
            || !results
                .iter()
                .all(|r| matches!(r, CutoutResult::Fast { .. }))
        {
            continue;
        }
        assert_eq!(
            board.revision(),
            revision + results.len() as u64,
            "one revision per removed trace, none for the pieces"
        );
        assert_eq!(
            (board.max_trace_half_width(), board.min_trace_half_width()),
            bounds
        );
        let mut checker = DesignRulesChecker::new(&board);
        assert_eq!(board.ledger_incomplete_count(), checker.incomplete_count());
        checked += 1;
    }
    assert!(
        checked >= 5,
        "only {checked} samples cut on the fast path alone"
    );
}

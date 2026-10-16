//! Board probe `tree-ops` (B02.4, B02.5): the search trees after Java's
//! tree-changing board operations have Java's shape (decision D5). Each
//! stage's lines equal `fixtures/reference/probe/tree-ops/<board>.jsonl`,
//! which `tools/java-reference/probes/board/TreeOpsProbe.java` writes; its
//! doc comment lists the stages.

mod board_probe;

use std::collections::BTreeSet;

use board_probe::{FNV_OFFSET, Line, Row, compare, fixtures, fnv1a64, header, hex};
use freerouting::board::items::{AreaKind, ItemId, ItemKind};
use freerouting::board::search::ShapeSearchTree;
use freerouting::board::structure::FixedState;
use freerouting::board::{ChangeOutcome, LineReuse, RoutingBoard};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{Point, Polyline, PolylineShapeOps};

/// The boards a debug build runs (the workspace gate); release runs all.
const DEBUG_BOARDS: &[&str] = &[
    "Issue026-J2_reference",
    "Issue145-smoothieboard",
    "Issue413-test",
    "Issue433-my-board",
    "Issue508-DAC2020_bm08",
    "Issue676-ch32v-tx118s",
    "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
    "Issue723-CombineStackOverflow",
];

/// (board, first differing stage, reason) where Rust's trees differ from
/// Java's today; the same two boards as in `board_probe_tree.rs`.
const EXPECTED_MISMATCHES: &[(&str, &str, &str)] = &[
    (
        "Issue676-ch32v-tx118s",
        "insert-via",
        "Java's SesReader fails on all 48 wires with a ClassCastException (RationalVector to \
         IntVector) and imports none; the port imports them (docs/DIVERGENCES.md), so the \
         traces the vias are seeded on differ",
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
        "insert-via",
        "not a tree difference: the DSN reader gives the pins and vias of nets in a net class \
         that class's clearance class, Java keeps the default ones, so the compensated trees \
         cc3..cc33 differ from the load on (board_probe_tree.rs, C04's reader)",
    ),
];

const BIG: &[&str] = &[
    "Issue026-J2_reference",
    "Issue145-smoothieboard",
    "Issue413-test",
];
const CHANGE: &[&str] = &["Issue026-J2_reference", "Issue413-test"];

fn tree_line(stage: &str, tree: &ShapeSearchTree) -> String {
    let fingerprint = tree.fingerprint();
    Line::new()
        .str("stage", stage)
        .str("tree", &tree.key())
        .num("nodes", fingerprint.nodes as i64)
        .num("leaves", fingerprint.leaves as i64)
        .num("maxDepth", fingerprint.max_depth as i64)
        .str("shapeHash", &hex(fingerprint.shape_hash))
        .str("contentDigest", &hex(tree.content_digest()))
        .end()
}

fn hash_all(lines: &mut Vec<String>, stage: &str, step: &str, board: &RoutingBoard) {
    lines.push(
        Line::new()
            .str("stage", stage)
            .str("step", step)
            .num(
                "maxGeneratedId",
                i64::from(board.communication.id_generator.max_generated_id()),
            )
            .num("items", board.items().count() as i64)
            .end(),
    );
    for tree in board.search_trees().trees() {
        lines.push(tree_line(stage, tree));
    }
}

fn autoroute_trees(board: &mut RoutingBoard) {
    let classes = &board.rules.net_classes;
    let set: BTreeSet<i32> = (0..classes.count())
        .filter_map(|i| classes.nth(i))
        .map(|id| classes.get(id).trace_clearance_class())
        .collect();
    for class in set {
        board.autoroute_search_tree(class);
    }
}

fn polyline(board: &RoutingBoard, id: ItemId) -> Polyline {
    board
        .item(id)
        .and_then(|i| i.as_trace())
        .expect("trace")
        .polyline
        .clone()
}

fn insert_vias(lines: &mut Vec<String>, board: &mut RoutingBoard) {
    let rules = &board.rules;
    let info = rules
        .default_via_rule()
        .and_then(|rule| rules.via_rules.get(rule).get_via(0))
        .map(|info| rules.via_infos.get(info).clone());
    if let Some(info) = info {
        let mut random = JavaRandom::new(2);
        for k in 0..20 {
            let traces = board.traces();
            if traces.is_empty() {
                break;
            }
            let trace = traces[random.next_int(traces.len() as i32) as usize];
            let polyline = polyline(board, trace);
            let corner = polyline.corner(random.next_int(polyline.corner_count() as i32) as usize);
            let inserted = if let Point::Int(center) = corner {
                let nets = board.item(trace).expect("trace").net_numbers().to_vec();
                board.insert_via(
                    info.padstack,
                    center,
                    &nets,
                    info.clearance_class,
                    FixedState::Unfixed,
                    info.attach_smd_allowed,
                );
                true
            } else {
                false
            };
            lines.push(
                Line::new()
                    .str("stage", "insert-via")
                    .num("via", k)
                    .num("trace", i64::from(trace.0))
                    .bool("inserted", inserted)
                    .end(),
            );
        }
    }
    hash_all(lines, "insert-via", "insert-via", board);
}

/// Unfixes and removes the 30 highest-id traces.
fn remove_traces(board: &mut RoutingBoard) {
    let mut traces = board.traces();
    traces.sort_by_key(|id| std::cmp::Reverse(id.0));
    traces.truncate(30);
    for id in &traces {
        board.unfix(*id);
    }
    board.remove_items(traces);
}

fn make_conductive(lines: &mut Vec<String>, board: &mut RoutingBoard) {
    let area = board
        .items()
        .find(|item| matches!(&item.kind, ItemKind::Area(a) if a.kind == AreaKind::Obstacle))
        .map(|item| item.id());
    lines.push(
        Line::new()
            .str("stage", "make-conductive")
            .num("area", area.map_or(-1, |id| i64::from(id.0)))
            .end(),
    );
    if let Some(area) = area {
        board.make_conductive(area, 1);
    }
    hash_all(lines, "make-conductive", "make-conductive", board);
}

fn change_traces(lines: &mut Vec<String>, board: &mut RoutingBoard) {
    let mut random = JavaRandom::new(4);
    for k in 0..50 {
        let candidates: Vec<ItemId> = board
            .traces()
            .into_iter()
            .filter(|id| polyline(board, *id).corner_count() >= 4)
            .collect();
        if candidates.is_empty() {
            break;
        }
        let trace = candidates[random.next_int(candidates.len() as i32) as usize];
        let old = polyline(board, trace);
        let old_lines = old.lines();
        let mut new_lines = old_lines.to_vec();
        let count = 1 + random.next_int(2);
        let mut indexes = String::new();
        for _ in 0..count {
            let index = 1 + random.next_int(old_lines.len() as i32 - 2) as usize;
            let dist = (random.next_int(4) + 1) * 250;
            // Rust lines always have integer points (Java checks for
            // `IntPoint`).
            new_lines[index] = old_lines[index].translate(f64::from(dist));
            if !indexes.is_empty() {
                indexes.push(',');
            }
            indexes.push_str(&format!("{index}:{dist}"));
        }
        let new = Polyline::new(&new_lines);
        // PARITY: Java's `new Polyline(Line[])` keeps the given `Line`
        // objects except where it turns a line around or drops one, and
        // `Line.translate` makes a new object, so the reused lines are
        // exactly the ones equal by value at the same index.
        let reuse = LineReuse::by_value(&old, &new);
        let mut hash = FNV_OFFSET;
        for line in new.lines() {
            hash = fnv1a64(
                hash,
                &format!(
                    "{} {} {} {}\n",
                    line.a().x,
                    line.a().y,
                    line.b().x,
                    line.b().y
                ),
            );
        }
        lines.push(
            Line::new()
                .str("stage", "change")
                .num("k", k)
                .num("trace", i64::from(trace.0))
                .str("indexes", &indexes)
                .num("p", reuse.prefix as i64)
                .num("s", reuse.suffix as i64)
                .num("lines", new.lines().len() as i64)
                .str("linesHash", &hex(hash))
                .end(),
        );
        if new.lines().len() >= 3 {
            board.change_trace(trace, new, reuse);
        }
        hash_all(lines, "change", "change", board);
    }
}

fn rust_lines(row: &Row) -> Vec<String> {
    let mut lines = vec![header("tree-ops", &row.board)];
    let mut board = board_probe::load(row);
    autoroute_trees(&mut board);
    board_probe::import_session(&mut board, row);
    insert_vias(&mut lines, &mut board);
    remove_traces(&mut board);
    hash_all(&mut lines, "remove", "remove", &board);
    board.generate_snapshot();
    remove_traces(&mut board);
    hash_all(&mut lines, "undo-redo", "remove", &board);
    board.undo(None);
    hash_all(&mut lines, "undo-redo", "undo", &board);
    board.redo(None);
    hash_all(&mut lines, "undo-redo", "redo", &board);
    if BIG.contains(&row.board.as_str()) {
        board.set_clearance_compensation_used(true);
        hash_all(&mut lines, "compensation", "on", &board);
        board.set_clearance_compensation_used(false);
        hash_all(&mut lines, "compensation", "off", &board);
        autoroute_trees(&mut board);
        hash_all(&mut lines, "compensation", "autoroute", &board);
        board.change_plane_as_obstacle(true);
        hash_all(&mut lines, "plane-obstacle", "plane-obstacle", &board);
        make_conductive(&mut lines, &mut board);
    }
    if CHANGE.contains(&row.board.as_str()) {
        let mut changed = board_probe::load(row);
        autoroute_trees(&mut changed);
        board_probe::import_session(&mut changed, row);
        change_traces(&mut lines, &mut changed);
    }
    tie_pins(&mut lines, row);
    lines
}

/// The pins of more than one net, in item-list order.
fn tie_pins_of(board: &RoutingBoard) -> Vec<ItemId> {
    board
        .items()
        .filter(|i| matches!(i.kind, ItemKind::Pin(_)) && i.net_numbers().len() > 1)
        .map(|i| i.id())
        .collect()
}

/// Stage `tie-pin` (B02.6), as `TreeOpsProbe.tiePins`.
fn tie_pins(lines: &mut Vec<String>, row: &Row) {
    let mut board = board_probe::load(row);
    if tie_pins_of(&board).is_empty() {
        return;
    }
    autoroute_trees(&mut board);
    board_probe::import_session(&mut board, row);
    let classes = &board.rules.net_classes;
    let set: BTreeSet<i32> = (0..classes.count())
        .filter_map(|i| classes.nth(i))
        .map(|id| classes.get(id).trace_clearance_class())
        .collect();
    for class in set {
        board.autoroute_search_tree(class);
        for pin in tie_pins_of(&board) {
            let center = Point::Int(board.item(pin).and_then(|p| p.drill()).expect("pin").center);
            for trace in board.traces() {
                let polyline = polyline(&board, trace);
                let shape_no = if polyline.first_corner() == center {
                    0
                } else if polyline.last_corner() == center {
                    polyline.corner_count() - 2
                } else {
                    continue;
                };
                board.reduce_trace_shape_at_tie_pin(class, pin, trace);
                let tree = board.search_trees().tree_by_class(class).expect("tree");
                let shape = tree
                    .tree_shape(
                        board.item(trace).expect("trace"),
                        shape_no,
                        &board.shape_context(),
                    )
                    .expect("shape");
                let octagon = shape.bounding_octagon().expect("bounded");
                lines.push(
                    Line::new()
                        .str("stage", "tie-pin")
                        .str("tree", &tree.key())
                        .num("pin", i64::from(pin.0))
                        .num("trace", i64::from(trace.0))
                        .num("shapeNo", shape_no as i64)
                        .num("dimension", i64::from(shape.dimension()))
                        .str(
                            "bounds",
                            &format!(
                                "O {} {} {} {} {} {} {} {}",
                                octagon.left_x,
                                octagon.bottom_y,
                                octagon.right_x,
                                octagon.top_y,
                                octagon.upper_left_diagonal_x,
                                octagon.lower_right_diagonal_x,
                                octagon.lower_left_diagonal_x,
                                octagon.upper_right_diagonal_x
                            ),
                        )
                        .end(),
                );
            }
        }
        let tree = board.search_trees().tree_by_class(class).expect("tree");
        lines.push(tree_line("tie-pin", tree));
    }
}

/// The stage of the first line that differs, from `compare`'s message.
fn differing_stage(message: &str) -> Option<String> {
    let java_line = message
        .lines()
        .find_map(|l| l.trim().strip_prefix("Java: {"))?;
    let rest = java_line.strip_prefix("\"stage\":\"")?;
    Some(rest.split('"').next()?.to_string())
}

#[test]
fn tree_ops_match_java() {
    // A debug build runs `DEBUG_BOARDS` only (`board_probe::board_filter`).
    let only = board_probe::board_filter(DEBUG_BOARDS);
    let mut failures = Vec::new();
    let mut seen = Vec::new();
    let mut boards = 0;
    for row in fixtures() {
        if row.ses.is_none() || only.as_ref().is_some_and(|o| !o.contains(&row.board)) {
            continue;
        }
        boards += 1;
        let Err(message) = compare("tree-ops", &row, &rust_lines(&row)) else {
            continue;
        };
        let stage = differing_stage(&message).unwrap_or_default();
        match EXPECTED_MISMATCHES
            .iter()
            .find(|(board, s, _)| *board == row.board && *s == stage)
        {
            Some(entry) => seen.push(*entry),
            None => failures.push(message),
        }
    }
    for entry in EXPECTED_MISMATCHES {
        let ran = only.as_ref().is_none_or(|o| o.iter().any(|b| b == entry.0));
        if ran && !seen.contains(entry) {
            failures.push(format!(
                "listed mismatch {} {} no longer occurs; remove it from EXPECTED_MISMATCHES",
                entry.0, entry.1
            ));
        }
    }
    assert!(boards > 0);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Java's `PolylineTrace.change` calls `saveForUndo` before it finds that
/// nothing changed, so an undo after an unchanged change still restores the
/// trace (and reports its net).
#[test]
fn change_trace_unchanged_still_saves_for_undo() {
    let row = board_probe::fixture("Issue413-test");
    let mut board = board_probe::load(&row);
    let trace = board.traces()[0];
    let net = board.item(trace).expect("trace").net_numbers()[0];
    board.generate_snapshot();
    let same = polyline(&board, trace);
    let all = same.lines().len();
    let outcome = board.change_trace(
        trace,
        same,
        LineReuse {
            prefix: all,
            suffix: all,
        },
    );
    assert_eq!(outcome, ChangeOutcome::Unchanged);
    let mut changed_nets = BTreeSet::new();
    assert!(board.undo(Some(&mut changed_nets)));
    assert!(
        changed_nets.contains(&net),
        "the unchanged trace was saved for undo: {changed_nets:?}"
    );
}

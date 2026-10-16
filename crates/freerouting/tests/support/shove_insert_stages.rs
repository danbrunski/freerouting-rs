//! The stages of probe `shove-insert` (O04.7), shared by
//! `tests/reference_shove_insert.rs` and `examples/shove_insert_dump.rs`.
//!
//! Mirrors `tools/java-reference/probes/board/ShoveInsertProbe.java`, whose doc
//! comment is the specification: one `java.util.Random(20261016)` drives the
//! stages `insert` (forced trace segments), `via` (forced vias), `move` (drill
//! item moves) and `attach` (a forced via onto an SMD pin with the via's attach
//! flag off, then on). A snapshot is taken before each step and undone after a
//! failed one. Each step line starts with `"stage"`; the canonical item dump
//! (`tests/support/item_dump.rs`) follows every 10th step and each stage.

#![allow(dead_code)]

use freerouting::board::RoutingBoard;
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{IntPoint, IntVector, Point, Vector};
use freerouting::rules::ViaInfo;
use freerouting::shove::{ForcedInsert, ForcedTrace, ForcedTraceParams};

use crate::board_probe::Line;
use crate::item_dump;
use crate::shove_sampler::{Start, draw_start, insert_starts, point_text};

/// Java `ShoveInsertProbe.SEED`.
pub const SEED: i64 = 20261016;
/// Java `STEPS` and `J2_STEPS`.
pub const STEPS: usize = 30;
pub const J2_STEPS: usize = 200;

const DIRS45: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// The step count of stage `insert` on `board_name`.
pub fn insert_steps(board_name: &str) -> usize {
    if board_name == "Issue026-J2_reference" {
        J2_STEPS
    } else {
        STEPS
    }
}

/// Runs every stage on `board` and appends the lines to `lines`.
pub fn shove_insert_lines(board: &mut RoutingBoard, steps: usize, lines: &mut Vec<String>) {
    let mut random = JavaRandom::new(SEED);
    insert(board, &mut random, steps, lines);
    via(board, &mut random, lines);
    move_vias(board, &mut random, lines);
    attach(board, lines);
}

fn state(line: Line, board: &RoutingBoard) -> String {
    line.num(
        "failing",
        board
            .shove_failing_obstacle()
            .map_or(-1, |id| i64::from(id.0)),
    )
    .num("failingLayer", i64::from(board.shove_failing_layer()))
    .num(
        "maxGeneratedId",
        i64::from(board.communication.id_generator.max_generated_id()),
    )
    .end()
}

fn trace_class(board: &RoutingBoard, net: i32) -> i32 {
    let class = board.rules.nets.get(net).expect("net").net_class();
    board.rules.net_classes.get(class).trace_clearance_class()
}

fn first_via(board: &RoutingBoard, net: i32) -> Option<ViaInfo> {
    let class = board.rules.nets.get(net)?.net_class();
    let rule = board.rules.net_classes.get(class).via_rule()?;
    let via = board.rules.via_rules.get(rule).get_via(0)?;
    Some(board.rules.via_infos.get(via).clone())
}

fn pens(board: &RoutingBoard, net: i32) -> Vec<i32> {
    (0..board.layer_count() as i32)
        .map(|layer| board.rules.trace_half_width(net, layer))
        .collect()
}

fn offset(point: IntPoint, dir: (i32, i32), dist: i32) -> IntPoint {
    IntPoint {
        x: point.x + dir.0 * dist,
        y: point.y + dir.1 * dist,
    }
}

fn outcome_text(outcome: &ForcedInsert) -> String {
    match outcome {
        ForcedInsert::DatabaseDamaged => "damaged".to_string(),
        ForcedInsert::Unchanged => "unchanged".to_string(),
        ForcedInsert::Reached => "reached".to_string(),
        ForcedInsert::Partial(p) => format!("partial {}", point_text(&Point::Int(*p))),
    }
}

fn skip(stage: &str, step: usize, reason: &str) -> String {
    Line::new()
        .str("stage", stage)
        .num("step", step as i64)
        .str("skip", reason)
        .end()
}

fn insert(
    board: &mut RoutingBoard,
    random: &mut JavaRandom,
    steps: usize,
    lines: &mut Vec<String>,
) {
    for step in 0..steps {
        let starts = insert_starts(board);
        if starts.is_empty() {
            lines.push(skip("insert", step, "no starts"));
            break;
        }
        let s: Start = draw_start(&starts, random);
        let dir = DIRS45[random.next_int(8) as usize];
        let unit = board.min_trace_half_width();
        let length = (1 + random.next_int(60)) * unit;
        let from = s.point;
        let to = offset(s.point, dir, length);
        let half_width = board.rules.trace_half_width(s.net, s.layer);
        let class = trace_class(board, s.net);
        let tidy = if step % 2 == 0 { i32::MAX } else { 0 };
        board.generate_snapshot();
        let nets = [s.net];
        let outcome = board.insert_forced_trace_segment(
            &Point::Int(from),
            &Point::Int(to),
            ForcedTrace {
                half_width,
                layer: s.layer,
                net_numbers: &nets,
                clearance_class: class,
            },
            ForcedTraceParams {
                max_recursion_depth: 20,
                max_via_recursion_depth: 8,
                max_spring_over_recursion_depth: 5,
                tidy_width: tidy,
                pull_tight_accuracy: 500,
                with_check: true,
            },
            None,
        );
        if outcome == ForcedInsert::DatabaseDamaged {
            board.undo(None);
        }
        let line = Line::new()
            .str("stage", "insert")
            .num("step", step as i64)
            .num("net", i64::from(s.net))
            .num("layer", i64::from(s.layer))
            .str("from", &point_text(&Point::Int(from)))
            .str("to", &point_text(&Point::Int(to)))
            .num("halfWidth", i64::from(half_width))
            .num("class", i64::from(class))
            .num("tidy", i64::from(tidy))
            .str("outcome", &outcome_text(&outcome));
        lines.push(state(line, board));
        if step % 10 == 9 {
            lines.extend(item_dump::dump(board));
        }
    }
    lines.extend(item_dump::dump(board));
}

fn via(board: &mut RoutingBoard, random: &mut JavaRandom, lines: &mut Vec<String>) {
    for step in 0..10 {
        let starts = insert_starts(board);
        if starts.is_empty() {
            lines.push(skip("via", step, "no starts"));
            break;
        }
        let s = draw_start(&starts, random);
        let dir = DIRS45[random.next_int(8) as usize];
        let unit = board.min_trace_half_width();
        let dist = random.next_int(21) * unit;
        let location = offset(s.point, dir, dist);
        let line = Line::new()
            .str("stage", "via")
            .num("step", step as i64)
            .num("net", i64::from(s.net))
            .str("location", &point_text(&Point::Int(location)));
        let Some(via) = first_via(board, s.net) else {
            lines.push(line.str("skip", "no via").end());
            continue;
        };
        let class = trace_class(board, s.net);
        let tidy = if step % 2 == 0 { i32::MAX } else { 0 };
        board.generate_snapshot();
        let pens = pens(board, s.net);
        let ok = board.forced_via(&via, location, &[s.net], class, &pens, 20, 8, tidy, 500, 0);
        if !ok {
            board.undo(None);
        }
        let line = line
            .str("via", &via.name)
            .num("tidy", i64::from(tidy))
            .bool("ok", ok);
        lines.push(state(line, board));
        if step % 10 == 9 {
            lines.extend(item_dump::dump(board));
        }
    }
}

fn move_vias(board: &mut RoutingBoard, random: &mut JavaRandom, lines: &mut Vec<String>) {
    for step in 0..10 {
        let mut vias: Vec<_> = board
            .items()
            .filter(|i| i.is_via())
            .map(|i| i.id())
            .collect();
        vias.sort_by(|a, b| b.0.cmp(&a.0));
        if vias.is_empty() {
            lines.push(skip("move", step, "no vias"));
            break;
        }
        let via = vias[random.next_int(vias.len() as i32) as usize];
        let dir = DIRS45[random.next_int(8) as usize];
        let unit = board.min_trace_half_width();
        let dist = (1 + random.next_int(10)) * unit;
        let vector = Vector::Int(IntVector {
            x: dir.0 * dist,
            y: dir.1 * dist,
        });
        let tidy = if step % 2 == 0 { i32::MAX } else { 100 };
        board.generate_snapshot();
        let ok = board.move_drill_item(via, &vector, 20, 8, tidy, 500, 0);
        if !ok {
            board.undo(None);
        }
        let line = Line::new()
            .str("stage", "move")
            .num("step", step as i64)
            .num("via", i64::from(via.0))
            .str("vector", &format!("{},{}", dir.0 * dist, dir.1 * dist))
            .num("tidy", i64::from(tidy))
            .bool("ok", ok);
        lines.push(state(line, board));
    }
    lines.extend(item_dump::dump(board));
}

/// The results of stage `attach` (attach off, attach on), if it found a pin.
pub fn attach(board: &mut RoutingBoard, lines: &mut Vec<String>) -> Option<(bool, bool)> {
    let mut items: Vec<_> = board
        .items()
        .filter(|i| i.is_pin() && i.first_layer() == i.last_layer())
        .filter(|i| i.net_numbers().len() == 1 && i.net_numbers()[0] > 0)
        .map(|i| (i.id(), i.net_numbers()[0], i.drill().expect("pin").center))
        .collect();
    items.sort_by(|a, b| b.0.0.cmp(&a.0.0));
    for (pin, net, center) in items {
        let Some(via) = first_via(board, net) else {
            continue;
        };
        let class = trace_class(board, net);
        let mut results = [false; 2];
        for (k, attach_allowed) in [false, true].into_iter().enumerate() {
            let copy = ViaInfo {
                attach_smd_allowed: attach_allowed,
                ..via.clone()
            };
            board.generate_snapshot();
            let pens = pens(board, net);
            let ok = board.forced_via(&copy, center, &[net], class, &pens, 20, 8, i32::MAX, 500, 0);
            board.undo(None);
            results[k] = ok;
            let line = Line::new()
                .str("stage", "attach")
                .num("pin", i64::from(pin.0))
                .num("net", i64::from(net))
                .str("via", &via.name)
                .bool("attach", attach_allowed)
                .bool("ok", ok);
            lines.push(state(line, board));
        }
        return Some((results[0], results[1]));
    }
    lines.push(
        Line::new()
            .str("stage", "attach")
            .str("skip", "no smd pin")
            .end(),
    );
    None
}

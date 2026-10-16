//! The stages of probe `shove-check` on the Rust side (O03.8), shared by
//! `tests/reference_shove_check.rs` and `examples/shove_check_dump.rs`. The stages and their
//! order are documented in `tools/java-reference/probes/board/ShoveCheckProbe.java`.

#![allow(dead_code)]

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::search::WorkLimit;
use freerouting::board::structure::AngleRestriction;
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{IntBox, IntOctagon, IntPoint, Point, Polyline, TileShape};
use freerouting::rules::ViaInfo;
use freerouting::shove::{CheckDrillResult, ShapeEntrySide, ShoveDepths, TraceShover};

use crate::board_probe::Line;
use crate::shove_sampler::{samples, shape_text};

/// The probe's `SAMPLES`.
pub const SAMPLES: usize = 300;

/// The probe's `foreign`: the net after the first net (wrapping).
fn foreign(board: &RoutingBoard, nets: &[i32]) -> Vec<i32> {
    let max = board.rules.nets.max_net_number();
    let first = nets.first().copied().unwrap_or(0);
    vec![if max > 0 { first % max + 1 } else { 1 }]
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

fn trace_check_lines(board: &mut RoutingBoard, stage: &str, lines: &mut Vec<String>) {
    lines.push(Line::new().str("stage", stage).end());
    for (i, s) in samples(board, SAMPLES).iter().enumerate() {
        lines.push(
            Line::new()
                .num("i", i as i64)
                .num("trace", i64::from(s.trace.0))
                .num("segment", s.segment as i64)
                .str("from", &format!("{},{}", s.from.x, s.from.y))
                .str("to", &format!("{},{}", s.to.x, s.to.y))
                .str("shape", &shape_text(s.shape.as_ref()))
                .end(),
        );
        let (Some(shape), Some(segment)) = (&s.shape, &s.line_segment) else {
            continue;
        };
        let nets = if i % 2 == 0 {
            s.nets.clone()
        } else {
            foreign(board, &s.nets)
        };
        let from_side = ShapeEntrySide::from_segment(segment, shape, i % 2 == 0);
        for (name, depths) in [("deep", (20, 8, 5)), ("flat", (0, 0, 0))] {
            let ok = TraceShover::new(board).check(
                shape,
                Some(from_side),
                None,
                s.layer,
                &nets,
                s.clearance_class,
                depths.0,
                depths.1,
                depths.2,
                None,
            );
            let line = Line::new().num("i", i as i64).str("q", name).bool("ok", ok);
            lines.push(state(line, board));
        }
        for (via_depth, suffix) in [(8, ""), (0, "0")] {
            for left in [true, false] {
                let length = TraceShover::new(board).check_segment_ok_length(
                    segment,
                    left,
                    s.layer,
                    &nets,
                    s.half_width,
                    s.clearance_class,
                    20,
                    via_depth,
                );
                let name = format!("{}{suffix}", if left { "okLeft" } else { "okRight" });
                let line = Line::new()
                    .num("i", i as i64)
                    .str("q", &name)
                    .dbl("length", length);
                lines.push(state(line, board));
            }
        }
        let polyline = Polyline::from_corners(&Point::Int(s.from), &Point::Int(s.to));
        for (name, depths) in [("forcedDeep", (20, 8, 5)), ("forcedFlat", (0, 0, 0))] {
            let ok = board.check_forced_trace_polyline(
                &polyline,
                s.half_width,
                s.layer,
                &nets,
                s.clearance_class,
                depths.0,
                depths.1,
                depths.2,
            );
            let line = Line::new().num("i", i as i64).str("q", name).bool("ok", ok);
            lines.push(state(line, board));
        }
    }
}

/// The probe's `GRID`.
const GRID: i64 = 20;

fn drill_result_name(result: CheckDrillResult) -> &'static str {
    match result {
        CheckDrillResult::NotDrillable => "NOT_DRILLABLE",
        CheckDrillResult::Drillable => "DRILLABLE",
        CheckDrillResult::DrillableWithAttachSmd => "DRILLABLE_WITH_ATTACH_SMD",
    }
}

/// The probe's `room`.
fn room(k: i64, p: IntPoint, s: i32) -> TileShape {
    let (x, y) = (p.x, p.y);
    match k % 7 {
        0 => TileShape::IntBox(IntBox::new(x - s, y - s, x + s, y + s)),
        1 => TileShape::IntBox(IntBox::new(x - 1, y - 1, x + 1, y + s)),
        2 => TileShape::IntBox(IntBox::new(x - s, y - 1, x + 1, y + 1)),
        3 => TileShape::IntBox(IntBox::new(x - 1, y - 1, x + s, y + 1)),
        4 => TileShape::IntOctagon(
            IntOctagon::new(
                x - s,
                y - s,
                x + s,
                y + s,
                x - y - 1,
                x - y + 1,
                x + y - 2 * s,
                x + y + 2 * s,
            )
            .normalize(),
        ),
        5 => TileShape::IntOctagon(
            IntOctagon::new(
                x - s,
                y - s,
                x + s,
                y + s,
                x - y - 2 * s,
                x - y + 2 * s,
                x + y - 1,
                x + y + 1,
            )
            .normalize(),
        ),
        _ => TileShape::IntBox(IntBox::new(x - 1, y - 1, x + 1, y + 1)),
    }
}

/// The probe's `firstVia`: the first via of the board's first via rule.
fn first_via(board: &RoutingBoard) -> Option<ViaInfo> {
    let rules = &board.rules;
    let id = rules
        .via_rules
        .nth(0)
        .and_then(|id| rules.via_rules.get(id).get_via(0))?;
    Some(rules.via_infos.get(id).clone())
}

fn via_grid_lines(board: &mut RoutingBoard, stage: &str, lines: &mut Vec<String>) {
    lines.push(Line::new().str("stage", stage).end());
    let Some(via) = first_via(board) else {
        return;
    };
    let bbox = board.bounding_box();
    let max_net = board.rules.nets.max_net_number();
    for k in 0..GRID * GRID {
        let (row, column) = (k / GRID, k % GRID);
        let x = (i64::from(bbox.ll.x)
            + (2 * column + 1) * (i64::from(bbox.ur.x) - i64::from(bbox.ll.x)) / (2 * GRID))
            as i32;
        let y = (i64::from(bbox.ll.y)
            + (2 * row + 1) * (i64::from(bbox.ur.y) - i64::from(bbox.ll.y)) / (2 * GRID))
            as i32;
        let nets = vec![if max_net > 0 {
            1 + (k as i32) % max_net
        } else {
            1
        }];
        lines.push(Line::new().num("k", k).str("at", &format!("{x},{y}")).end());
        via_queries(board, &via, k, IntPoint { x, y }, &nets, false, lines);
    }
}

/// The probe's `PINS`.
const PINS: usize = 100;

fn via_pins_lines(board: &mut RoutingBoard, stage: &str, lines: &mut Vec<String>) {
    lines.push(Line::new().str("stage", stage).end());
    let Some(via) = first_via(board) else {
        return;
    };
    let mut pins: Vec<ItemId> = board
        .items()
        .filter(|item| item.is_pin() && item.net_count() > 0)
        .map(|item| item.id())
        .collect();
    // Ascending raw ids (`ItemId` itself orders like Java's item `TreeSet`, highest first).
    pins.sort_by_key(|id| id.0);
    for (k, pin) in pins.into_iter().take(PINS).enumerate() {
        let item = board.item(pin).expect("pin");
        let location = item.drill().expect("pin").center;
        let nets = item.net_numbers().to_vec();
        lines.push(
            Line::new()
                .num("k", k as i64)
                .num("pin", i64::from(pin.0))
                .str("at", &format!("{},{}", location.x, location.y))
                .end(),
        );
        via_queries(board, &via, k as i64, location, &nets, true, lines);
    }
}

/// The probe's `viaQueries`; `attach_too` adds the `checkLayer` with
/// `attachSmdAllowed` true.
fn via_queries(
    board: &mut RoutingBoard,
    via: &ViaInfo,
    k: i64,
    location: IntPoint,
    nets: &[i32],
    attach_too: bool,
    lines: &mut Vec<String>,
) {
    let padstack = board
        .library
        .padstacks
        .get(via.padstack)
        .expect("via padstack")
        .clone();
    let half_width = board.min_trace_half_width();
    let pens = vec![half_width; board.layer_count()];
    let class = via.clearance_class;
    let ok = TraceShover::new(board).check_via(
        via,
        &Point::Int(location),
        nets,
        ShoveDepths {
            recursion: 20,
            via_recursion: 8,
        },
        (k % 2 == 0).then_some(pens.as_slice()),
        class,
    );
    let line = Line::new().num("k", k).str("q", "via").bool("ok", ok);
    lines.push(state(line, board));
    for layer in padstack.from_layer()..=padstack.to_layer() {
        let via_radius = padstack
            .get_shape(layer)
            .map_or(0.0, |shape| 0.5 * shape.max_width());
        let radius = via_radius.max(f64::from(half_width));
        let clearance = board.clearance_value(class, class, layer);
        let room = room(
            k,
            location,
            4 * radius.ceil() as i32 + 4 * half_width + clearance,
        );
        let attaches: &[bool] = if attach_too {
            &[via.attach_smd_allowed, true]
        } else {
            &[via.attach_smd_allowed]
        };
        for &attach in attaches {
            let result = TraceShover::new(board).check_via_layer(
                radius,
                class,
                attach,
                &room,
                &Point::Int(location),
                layer,
                nets,
                ShoveDepths {
                    recursion: 20,
                    via_recursion: 0,
                },
                half_width,
                class,
            );
            let q = if attach != via.attach_smd_allowed {
                "layerAttach"
            } else {
                "layer"
            };
            let line = Line::new()
                .num("k", k)
                .str("q", q)
                .num("layer", i64::from(layer))
                .str("room", &shape_text(Some(&room)))
                .str("result", drill_result_name(result));
            lines.push(state(line, board));
        }
    }
}

/// The probe's `LIMIT_SAMPLES`.
const LIMIT_SAMPLES: usize = 50;

/// Java's units of one nominal millisecond (`WorkLimit.UNITS_PER_NOMINAL_MILLISECOND`).
const UNITS_PER_MS: u64 = 64_000;

fn limit_lines(board: &mut RoutingBoard, lines: &mut Vec<String>) {
    lines.push(Line::new().str("stage", "limit").end());
    let units = |board: &RoutingBoard| board.search_trees().work_meter().units();
    let mut done = 0;
    for (i, s) in samples(board, SAMPLES).iter().enumerate() {
        if done >= LIMIT_SAMPLES {
            break;
        }
        let (Some(shape), Some(segment)) = (&s.shape, &s.line_segment) else {
            continue;
        };
        done += 1;
        let nets = if i % 2 == 0 {
            s.nets.clone()
        } else {
            foreign(board, &s.nets)
        };
        let from_side = ShapeEntrySide::from_segment(segment, shape, i % 2 == 0);
        let check = |board: &mut RoutingBoard, limit: Option<&mut WorkLimit>| {
            TraceShover::new(board).check(
                shape,
                Some(from_side),
                None,
                s.layer,
                &nets,
                s.clearance_class,
                20,
                8,
                5,
                limit,
            )
        };
        let before = units(board);
        let free = check(board, None);
        let used = units(board) - before;
        let line = Line::new()
            .num("i", i as i64)
            .str("q", "free")
            .bool("ok", free)
            .num("u", used as i64);
        lines.push(state(line, board));
        let start = units(board);
        let mut limit = WorkLimit::with_site(board.search_trees().work_meter(), 1, "probe");
        // Java's long arithmetic: a check longer than the limit pads nothing.
        let pad = UNITS_PER_MS as i64 - used as i64 * (1 + i as i64 % 3) / 4;
        while ((units(board) - start) as i64) < pad {
            let last = units(board);
            board.overlapping_items_with_clearance(shape, s.layer, &[], s.clearance_class);
            if units(board) == last {
                break;
            }
        }
        let limited = check(board, Some(&mut limit));
        let used = units(board) - start;
        let exceeded = limit.exceeded(board.search_trees().work_meter());
        let line = Line::new()
            .num("i", i as i64)
            .str("q", "limited")
            .bool("ok", limited)
            .num("u", used as i64)
            .bool("exceeded", exceeded);
        lines.push(state(line, board));
    }
}

/// All stages of the probe on one board, in the probe's order.
pub fn shove_check_lines(board: &mut RoutingBoard, lines: &mut Vec<String>) {
    trace_check_lines(board, "trace-check", lines);
    via_grid_lines(board, "via-grid", lines);
    via_pins_lines(board, "via-pins", lines);
    limit_lines(board, lines);
    board
        .rules
        .set_trace_angle_restriction(AngleRestriction::NinetyDegree);
    trace_check_lines(board, "trace-check-90", lines);
    via_grid_lines(board, "via-grid-90", lines);
    via_pins_lines(board, "via-pins-90", lines);
}

//! The interactive route session against Java (I01.2-I01.7) and its own
//! behaviour (I01.4, I01.5, I01.6).
//!
//! Probe `route-script` (`tools/java-reference/probes/board/RouteScriptProbe.java`): per board
//! the stage `load` (as loaded) and, on J2 and DAC2020_bm08, `ses` (with the fixtures.tsv
//! session), each in four variants on a fresh board (push on and off, dynamic and stitch). One
//! `JavaRandom(20261016)` per variant draws 20 start pins and 4 trace starts; each route steps
//! towards the nearest target pin in 12 steps with a layer change to the next signal layer before
//! step 9, and every fifth route that does not end itself is cancelled. Each start, step and
//! layer change is one line (outcome, the route's and the current layer, last corner,
//! shove-failing obstacle, nearest target and `maxGeneratedId`); the canonical item dump follows
//! each route. On a mismatch the message names the first differing line (for a digest: the
//! section, one per line that starts with `"stage"`).

mod board_probe;
#[path = "support/edit_script.rs"]
mod edit_script;
#[path = "support/item_dump.rs"]
mod item_dump;

use board_probe::{Line, bits, header};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::FloatPoint;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::interactive::{
    EditSettings, LayerChange, RouteBudget, RouteEvent, RouteInput, RouteMode, RouteSession,
    RouteStep, StartError,
};
use item_dump::point_text;

const KIND: &str = "route-script";
const SEED: i64 = 20261016;
const PIN_STARTS: usize = 20;
const TRACE_STARTS: usize = 4;
const VARIANTS: [&str; 4] = [
    "push-dynamic",
    "nopush-dynamic",
    "push-stitch",
    "nopush-stitch",
];

fn fpoint(p: FloatPoint) -> String {
    format!("f{},{}", bits(p.x), bits(p.y))
}

fn max_id(board: &RoutingBoard) -> i64 {
    i64::from(board.communication.id_generator.max_generated_id())
}

fn opt_id(id: Option<ItemId>) -> i64 {
    id.map_or(-1, |i| i64::from(i.0))
}

fn join<T: ToString>(values: &[T]) -> String {
    values
        .iter()
        .map(T::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// The probe's `state` fields.
fn state(line: Line, board: &RoutingBoard, session: &RouteSession) -> Line {
    line.num("layer", i64::from(session.layer()))
        .num("current", i64::from(session.current_layer()))
        .str("lastCorner", &point_text(&session.last_corner().into()))
        .num("failing", opt_id(session.shove_failing_obstacle()))
        .num("nearest", opt_id(session.nearest_target_item()))
        .num("maxGeneratedId", max_id(board))
}

/// The probe's `nextSignalLayer`: the next signal layer after `layer`, else
/// the previous one.
fn next_signal_layer(board: &RoutingBoard, layer: i32) -> i32 {
    let layers = &board.layer_structure.layers;
    let count = layers.len() as i32;
    ((layer + 1)..count)
        .find(|&i| layers[i as usize].is_signal)
        .or_else(|| (0..layer).rev().find(|&i| layers[i as usize].is_signal))
        .unwrap_or(layer)
}

fn start_error_text(e: StartError) -> &'static str {
    match e {
        StartError::NothingPicked => "nothing",
        StartError::NoNets => "nonets",
        StartError::ConnectToTraceFailed => "connect",
        StartError::UnknownNet => "unknown-net",
    }
}

fn layer_change_text(change: &LayerChange) -> &'static str {
    match change {
        LayerChange::Ignored => "ignored",
        LayerChange::Changed { .. } => "changed",
        LayerChange::PlaneReached => "plane",
        LayerChange::Failed { .. } => "failed",
    }
}

/// One route of the script (the probe's `route`).
fn route(
    board: &mut RoutingBoard,
    stage: &str,
    settings: &mut EditSettings,
    r: usize,
    at: FloatPoint,
    lines: &mut Vec<String>,
) {
    settings.layer = 0;
    let line = Line::new()
        .str("stage", stage)
        .num("route", r as i64)
        .str("at", &fpoint(at));
    let (mut session, info) = match RouteSession::start(board, settings, at) {
        Ok(started) => started,
        Err(e) => {
            lines.push(
                line.str("start", start_error_text(e))
                    .num("maxGeneratedId", max_id(board))
                    .end(),
            );
            return;
        }
    };
    let via_rule = info.params.via_rule.map_or_else(
        || "-".to_string(),
        |id| board.rules.via_rules.get(id).name.clone(),
    );
    let line = line
        .str("start", "ok")
        .num("item", i64::from(info.item.0))
        .str("point", &point_text(&info.start.into()))
        .str("nets", &join(&info.nets))
        .str("halfWidths", &join(&info.params.half_widths))
        .num("class", i64::from(info.params.clearance_class))
        .str("viaRule", &via_rule)
        .bool("stitch", info.mode == RouteMode::Stitch)
        .num("targets", session.target_set().len() as i64);
    let from = session.last_corner().to_float();
    let mut target: Option<FloatPoint> = None;
    let mut min_dist = f64::MAX;
    for &id in session.target_set() {
        let Some(item) = board.item(id) else { continue };
        if !item.is_pin() {
            continue;
        }
        let c = item.drill().expect("pin").center.to_float();
        let d = from.distance(c);
        if d < min_dist {
            min_dist = d;
            target = Some(c);
        }
    }
    lines.push(
        state(
            line.str("target", &target.map_or_else(|| "-".to_string(), fpoint)),
            board,
            &session,
        )
        .end(),
    );
    let mut ended = false;
    if let Some(target) = target {
        for k in 1..=12 {
            if k == 9 {
                let to = next_signal_layer(board, session.current_layer());
                let (change, _) = session.change_layer(board, to, RouteBudget::JAVA);
                ended = change.ends_route();
                lines.push(
                    state(
                        Line::new()
                            .str("stage", stage)
                            .num("route", r as i64)
                            .str("kind", "layer")
                            .num("to", i64::from(to))
                            .str("outcome", layer_change_text(&change))
                            .num("failingLayer", i64::from(board.shove_failing_layer())),
                        board,
                        &session,
                    )
                    .end(),
                );
                if ended {
                    break;
                }
            }
            let kf = f64::from(k);
            let p = FloatPoint::new(
                from.x + (target.x - from.x) * kf / 12.0,
                from.y + (target.y - from.y) * kf / 12.0,
            );
            let input = match session.mode() {
                RouteMode::Dynamic => RouteInput::Move(p),
                RouteMode::Stitch => RouteInput::Click(p),
            };
            let event = session.handle(board, input, RouteBudget::JAVA);
            assert!(matches!(event, RouteEvent::Step(_)), "{event:?}");
            ended = event.ends_route();
            lines.push(
                state(
                    Line::new()
                        .str("stage", stage)
                        .num("route", r as i64)
                        .str("kind", "corner")
                        .num("k", i64::from(k))
                        .str("at", &fpoint(p))
                        .bool("ended", ended),
                    board,
                    &session,
                )
                .end(),
            );
            if ended {
                break;
            }
        }
    }
    if !ended && r % 5 == 4 {
        session.cancel(board, settings);
        lines.push(
            Line::new()
                .str("stage", stage)
                .num("route", r as i64)
                .str("kind", "cancel")
                .num("maxGeneratedId", max_id(board))
                .end(),
        );
    }
    lines.extend(item_dump::dump(board));
}

/// The probe's `script` on one board and variant.
fn script(
    board: &mut RoutingBoard,
    stage: &str,
    settings: &mut EditSettings,
    lines: &mut Vec<String>,
) {
    let mut random = JavaRandom::new(SEED);
    let pins: Vec<FloatPoint> = board
        .items()
        .filter(|i| i.is_pin() && i.net_count() > 0)
        .map(|i| i.drill().expect("pin").center.to_float())
        .collect();
    for r in 0..PIN_STARTS + TRACE_STARTS {
        let at = if r < PIN_STARTS {
            if pins.is_empty() {
                lines.push(skip(stage, r, "no pins"));
                continue;
            }
            pins[random.next_int(pins.len() as i32) as usize]
        } else {
            let traces: Vec<_> = board
                .items()
                .filter_map(|i| i.as_trace())
                .map(|t| t.polyline.clone())
                .collect();
            if traces.is_empty() {
                lines.push(skip(stage, r, "no traces"));
                continue;
            }
            let polyline = &traces[random.next_int(traces.len() as i32) as usize];
            polyline
                .corner_approx(0)
                .middle_point(polyline.corner_approx(1))
        };
        route(board, stage, settings, r, at, lines);
    }
}

fn skip(stage: &str, r: usize, why: &str) -> String {
    Line::new()
        .str("stage", stage)
        .num("route", r as i64)
        .str("skip", why)
        .end()
}

/// Every stage and variant of the probe on `board_name`.
fn route_script_lines(board_name: &str, with_session: bool) -> (board_probe::Row, Vec<String>) {
    let row = board_probe::fixture(board_name);
    let mut lines = vec![header(KIND, &row.board)];
    let stages: &[&str] = if with_session {
        &["load", "ses"]
    } else {
        &["load"]
    };
    for stage in stages {
        for variant in VARIANTS {
            let mut board = board_probe::load(&row);
            if *stage == "ses" {
                board_probe::import_session(&mut board, &row);
            }
            let mut settings = EditSettings::for_board(&board);
            settings.push_enabled = variant.starts_with("push");
            settings.stitch_route = variant.ends_with("stitch");
            script(
                &mut board,
                &format!("{stage}/{variant}"),
                &mut settings,
                &mut lines,
            );
        }
    }
    (row, lines)
}

fn check_board(board_name: &str, with_session: bool) {
    let (row, lines) = route_script_lines(board_name, with_session);
    if let Err(e) = board_probe::compare(KIND, &row, &lines) {
        panic!("{e}");
    }
}

#[test]
fn route_script_matches_java_j2() {
    check_board("Issue026-J2_reference", true);
}

#[test]
fn route_script_matches_java_bm08() {
    check_board("Issue508-DAC2020_bm08", true);
}

#[test]
fn route_script_matches_java_bm05() {
    check_board("Issue508-DAC2020_bm05", false);
}

#[test]
fn route_script_matches_java_rpi_splitter() {
    check_board("Issue143-rpi_splitter", false);
}

#[test]
fn route_script_matches_java_issue413() {
    check_board("Issue413-test", false);
}

#[test]
fn route_script_matches_java_issue676() {
    check_board("Issue676-ch32v-tx118s", false);
}

#[test]
fn route_script_matches_java_issue957() {
    check_board("Issue957-smoke-ldo-led", false);
}

#[test]
fn route_script_matches_java_smd_demo() {
    check_board("Issue508-SMD-routing-issue-demo", false);
}

#[test]
fn route_script_matches_java_scoring_mixed_layer() {
    check_board("scoring-mixed-layer", false);
}

#[test]
fn route_script_matches_java_issue110() {
    check_board("Issue110-testProjectFromFreeroutingBugTest", false);
}

// ---- I01.4, I01.5, I01.6: the session's own behaviour ----

/// J2 as loaded with the default settings.
fn j2() -> RoutingBoard {
    board_probe::load(&board_probe::fixture("Issue026-J2_reference"))
}

/// The number of snapshots on the undo stack: undos until none is left, on
/// a copy.
fn undo_depth(board: &RoutingBoard) -> usize {
    let mut copy = board.clone();
    let mut depth = 0;
    while copy.undo(None) {
        depth += 1;
    }
    depth
}

/// A one-net pin of J2 with a target pin: (start, target) centres.
fn j2_start_and_target(board: &mut RoutingBoard) -> (FloatPoint, FloatPoint) {
    let settings = EditSettings::for_board(board);
    let pins: Vec<FloatPoint> = board
        .items()
        .filter(|i| i.is_pin() && i.net_count() == 1)
        .map(|i| i.drill().expect("pin").center.to_float())
        .collect();
    for at in pins {
        let mut copy = board.clone();
        let Ok((session, _)) = RouteSession::start(&mut copy, &settings, at) else {
            continue;
        };
        let target = session
            .target_set()
            .iter()
            .filter_map(|&id| copy.item(id))
            .find(|i| i.is_pin())
            .map(|i| i.drill().expect("pin").center.to_float());
        if let Some(target) = target {
            return (at, target);
        }
    }
    panic!("no J2 pin with a target pin");
}

/// I01.5: the same pointer sequence (six moves and a click) in dynamic and
/// in stitch mode. Dynamic: each move is a corner step, the click ends the
/// route, only the start snapshot is taken. Stitch: the moves only update
/// the nearest target point, the click takes a snapshot and adds a corner.
#[test]
fn dynamic_and_stitch_modes_replay_the_same_pointer_sequence() {
    let mut board = j2();
    let (start, target) = j2_start_and_target(&mut board);
    let at = |k: f64| {
        FloatPoint::new(
            start.x + (target.x - start.x) * k / 8.0,
            start.y + (target.y - start.y) * k / 8.0,
        )
    };
    let inputs: Vec<RouteInput> = (1..=6)
        .map(|k| RouteInput::Move(at(f64::from(k))))
        .chain([RouteInput::Snapshot, RouteInput::Click(at(7.0))])
        .collect();
    let mut results = Vec::new();
    for stitch in [false, true] {
        let mut board = j2();
        let depth_before = undo_depth(&board);
        let mut settings = EditSettings::for_board(&board);
        settings.stitch_route = stitch;
        let (mut session, info) = RouteSession::start(&mut board, &settings, start).expect("start");
        assert_eq!(
            info.mode,
            if stitch {
                RouteMode::Stitch
            } else {
                RouteMode::Dynamic
            }
        );
        let mut events = Vec::new();
        for input in &inputs {
            let event = session.handle(&mut board, *input, RouteBudget::JAVA);
            events.push(event);
            if event.ends_route() {
                break;
            }
        }
        let depth = undo_depth(&board) - depth_before;
        assert_eq!(depth, session.snapshots_taken(), "stitch {stitch}");
        results.push((events, session.snapshots_taken(), session.corners_added()));
    }
    let (dynamic, stitch) = (&results[0], &results[1]);
    // dynamic: 6 corner steps, a snapshot, the click ends the route
    assert_eq!(dynamic.0.len(), 8, "{:?}", dynamic.0);
    assert!(
        dynamic.0[..6]
            .iter()
            .all(|e| matches!(e, RouteEvent::Step(_)))
    );
    assert_eq!(dynamic.0[6], RouteEvent::SnapshotTaken);
    assert_eq!(dynamic.0[7], RouteEvent::Ended);
    assert_eq!(dynamic.1, 2, "start and Snapshot");
    assert!(dynamic.2 >= 1, "dynamic corners {}", dynamic.2);
    // stitch: the moves only update the target, Snapshot means nothing, the
    // click takes a snapshot and adds one corner
    assert!(
        stitch.0[..6]
            .iter()
            .all(|e| *e == RouteEvent::TargetUpdated)
    );
    assert_eq!(stitch.0[6], RouteEvent::Ignored);
    assert!(matches!(stitch.0[7], RouteEvent::Step(_)));
    assert_eq!(stitch.1, 2, "start and the click");
    assert_eq!(stitch.2, 1, "one corner from the click");
}

/// I01.4: cancel removes the trace tail at the last corner up to the next
/// via and nothing else. A J2 route of three corners is cancelled: the board
/// has the traces it had before the route, the other items unchanged.
#[test]
fn cancel_removes_tail_only() {
    for push in [true, false] {
        let mut board = j2();
        let (start, target) = j2_start_and_target(&mut board);
        let before = item_dump::dump(&board);
        let mut settings = EditSettings::for_board(&board);
        settings.push_enabled = push;
        let (mut session, _) = RouteSession::start(&mut board, &settings, start).expect("start");
        let mut advanced = 0;
        for k in 1..=3 {
            let p = FloatPoint::new(
                start.x + (target.x - start.x) * f64::from(k) / 8.0,
                start.y + (target.y - start.y) * f64::from(k) / 8.0,
            );
            if session.next_corner(&mut board, p, RouteBudget::JAVA) == RouteStep::Advanced {
                advanced += 1;
            }
        }
        assert!(advanced > 0, "push {push}: the route must advance");
        let routed = item_dump::dump(&board);
        assert_ne!(routed, before);
        session.cancel(&mut board, &settings);
        let after = item_dump::dump(&board);
        // geometry and data equal to the state before the route (ids moved on)
        let strip = |lines: &[String]| -> Vec<String> {
            let mut v: Vec<String> = lines
                .iter()
                .filter(|l| !l.starts_with("{\"maxGeneratedId\""))
                .map(|l| {
                    l.split_once(',')
                        .map_or(l.clone(), |(_, rest)| rest.to_string())
                })
                .collect();
            v.sort();
            v
        };
        assert_eq!(strip(&after), strip(&before), "push {push}");
    }
}

/// I01.4, kept Java quirk (DIVERGENCES "Java bugs kept on purpose"): when
/// no via of the route's via rule spans both layers, `Route.changeLayer`
/// returns true without inserting a via or changing the route's layer, and
/// the route state still moves the current layer and takes a snapshot.
#[test]
fn layer_change_without_spanning_via_quirk() {
    for java in [true, false] {
        let mut board = j2();
        board.set_java_quirks(
            freerouting::settings::JavaQuirks::JAVA
                .with("route_change_layer_no_via", java)
                .unwrap(),
        );
        let (start, _) = j2_start_and_target(&mut board);
        let settings = EditSettings::for_board(&board);
        let (mut session, info) = RouteSession::start(&mut board, &settings, start).expect("start");
        let rule = info.params.via_rule.expect("J2 has a via rule");
        // a via rule whose vias span no layer pair: clear it
        let vias: Vec<_> = board.rules.via_rules.get(rule).vias().to_vec();
        for via in vias {
            board.rules.via_rules.get_mut(rule).remove_via(via);
        }
        let items_before = item_dump::dump(&board);
        let depth_before = undo_depth(&board);
        let to = next_signal_layer(&board, session.current_layer());
        assert_ne!(to, session.layer());
        let (change, status) = session.change_layer(&mut board, to, RouteBudget::JAVA);
        if !java {
            assert!(matches!(change, LayerChange::Failed { .. }));
            assert_eq!(session.current_layer(), info.layer);
            assert_eq!(undo_depth(&board), depth_before);
            assert_eq!(item_dump::dump(&board), items_before);
            continue;
        }
        assert_eq!(change, LayerChange::Changed { via: false });
        assert_eq!(session.layer(), info.layer, "the route stays on its layer");
        assert_eq!(session.current_layer(), to, "the current layer moves");
        assert_eq!(undo_depth(&board), depth_before + 1, "a snapshot is taken");
        assert_eq!(item_dump::dump(&board), items_before, "no via");
        assert_eq!(status.expect("status").key, "layer_changed_to_message");
    }
}

/// I01.4: a layer change with a spanning via inserts it, moves both layers
/// and leaves one snapshot per successful try plus the one after the change.
#[test]
fn layer_change_with_a_via() {
    let mut board = j2();
    let (start, target) = j2_start_and_target(&mut board);
    let settings = EditSettings::for_board(&board);
    let (mut session, _) = RouteSession::start(&mut board, &settings, start).expect("start");
    let p = FloatPoint::new(
        start.x + (target.x - start.x) / 2.0,
        start.y + (target.y - start.y) / 2.0,
    );
    session.next_corner(&mut board, p, RouteBudget::JAVA);
    let vias_before = board.vias().len();
    let to = next_signal_layer(&board, session.current_layer());
    let (change, status) = session.change_layer(&mut board, to, RouteBudget::JAVA);
    assert_eq!(change, LayerChange::Changed { via: true });
    assert_eq!((session.layer(), session.current_layer()), (to, to));
    assert_eq!(board.vias().len(), vias_before + 1);
    assert_eq!(status.expect("status").key, "layer_changed_to_message");
    // out of range is ignored
    let (change, status) = session.change_layer(&mut board, 99, RouteBudget::JAVA);
    assert_eq!((change, status), (LayerChange::Ignored, None));
}

/// A start on nothing is refused.
#[test]
fn start_on_nothing_fails() {
    let mut board = j2();
    let settings = EditSettings::for_board(&board);
    let outside = FloatPoint::new(-1.0e9, -1.0e9);
    assert_eq!(
        RouteSession::start(&mut board, &settings, outside).err(),
        Some(StartError::NothingPicked)
    );
}

/// I01.6: the overlay of a stitch route on J2 started at pin 15 (net 1,
/// centre (1275650, -903250)) after a move onto the start: the nearest
/// target is pin 21 at (1275650, -879250), 24000 away (pin 9 at the same
/// distance comes later in target order). Net 1 has no traces as loaded, so
/// the remaining lengths are the limits written here. By hand: centre
/// (1275650, -891250), rotation pi/2, radii `0.5 L` and
/// `0.5 sqrt(L^2 - 24000^2)`.
#[test]
fn overlay_ellipses_and_targets_on_j2() {
    use freerouting::interactive::RouteOverlay;
    use std::f64::consts::FRAC_PI_2;

    let at = FloatPoint::new(1275650.0, -903250.0);
    let overlay = |min: f64, max: f64, visible: bool| {
        let mut board = j2();
        let net = board.rules.nets.get_mut(1).expect("net 1");
        net.set_minimum_trace_length(min);
        net.set_maximum_trace_length(max);
        let mut settings = EditSettings::for_board(&board);
        settings.stitch_route = true;
        let (mut session, info) = RouteSession::start(&mut board, &settings, at).expect("start");
        assert_eq!((info.item, info.nets.as_slice()), (ItemId(15), &[1][..]));
        session.handle(&mut board, RouteInput::Move(at), RouteBudget::JAVA);
        assert_eq!(session.nearest_target_item(), Some(ItemId(21)));
        RouteOverlay::new(&session, &board, visible)
    };
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    let check = |e: &freerouting::interactive::LengthEllipse, bigger: f64, smaller: f64| {
        assert_eq!((e.center.x, e.center.y), (1275650.0, -891250.0));
        assert!(close(e.rotation, FRAC_PI_2), "rotation {}", e.rotation);
        assert!(
            close(e.bigger_radius, bigger),
            "{} != {bigger}",
            e.bigger_radius
        );
        assert!(
            close(e.smaller_radius, smaller),
            "{} != {smaller}",
            e.smaller_radius
        );
    };

    // no length limits: no ellipse, the incomplete 100 wide (resolution
    // 254 per mil, capped at 100)
    let plain = overlay(0.0, 0.0, true);
    let targets: Vec<i32> = plain.targets.iter().map(|id| id.0).collect();
    assert_eq!(
        targets,
        [
            74, 71, 68, 65, 43, 42, 37, 36, 31, 30, 21, 20, 14, 9, 8, 7, 6, 5, 4, 3, 2
        ]
    );
    assert!(plain.swap_pins.is_empty() && plain.failing_obstacle.is_none());
    assert!(plain.length_ellipses.is_empty());
    let incomplete = plain.incomplete.expect("incomplete");
    assert_eq!(
        (incomplete.from.x, incomplete.from.y),
        (1275650.0, -903250.0)
    );
    assert_eq!((incomplete.to.x, incomplete.to.y), (1275650.0, -879250.0));
    assert_eq!(
        (incomplete.width, incomplete.length_violation),
        (100.0, false)
    );
    assert_eq!(plain.layer_change_marker, None);

    // maximum 100000: one ellipse
    let one = overlay(0.0, 100000.0, true);
    assert_eq!(one.length_ellipses.len(), 1);
    check(&one.length_ellipses[0], 50000.0, 48538.64439804639);

    // minimum 60000, maximum 100000: a ring
    let ring = overlay(60000.0, 100000.0, true);
    assert_eq!(ring.length_ellipses.len(), 2);
    check(&ring.length_ellipses[0], 50000.0, 48538.64439804639);
    check(&ring.length_ellipses[1], 30000.0, 27495.45416973504);

    // maximum 20000 < 24000: violated, no ellipse, three times as wide
    let short = overlay(0.0, 20000.0, true);
    assert!(short.length_ellipses.is_empty());
    let incomplete = short.incomplete.expect("incomplete");
    assert_eq!(
        (incomplete.width, incomplete.length_violation),
        (300.0, true)
    );

    // only a minimum of 50000: the maximum becomes 0.3 * CRIT_INT, a ring;
    // not drawn when the length-matching area is invisible
    let min_only = overlay(50000.0, 0.0, true);
    assert_eq!(min_only.length_ellipses.len(), 2);
    check(&min_only.length_ellipses[0], 5033164.8, 5033150.494864925);
    check(&min_only.length_ellipses[1], 25000.0, 21931.712199461308);
    assert!(overlay(50000.0, 0.0, false).length_ellipses.is_empty());
}

/// I01.8: the committed script `tests/data/edit_script_j2.json` is the
/// commands of routes 0-4 of the probe's `load/push-dynamic` variant on J2
/// (60 commands: 5 starts, 48 moves, 4 layer changes, a cancel and 2
/// completes), written from the probe's `at` points. Replayed with Java's
/// budgets, every command's state (layers, last corner, shove-failing
/// obstacle, nearest target, `maxGeneratedId`) and the final item dump equal
/// the probe mirror's after route 4, which `route_script_matches_java_j2`
/// proves equal to Java's; without budgets the output is the same (no limit
/// fires).
#[test]
fn edit_script_replays_the_probe_routes() {
    let text = std::fs::read_to_string(
        board_probe::repo_root().join("crates/freerouting/tests/data/edit_script_j2.json"),
    )
    .expect("script");
    let edits = edit_script::parse_script(&text).expect("parse");
    assert_eq!(edits.commands.len(), 60);
    let row = board_probe::fixture("Issue026-J2_reference");
    let mut board = board_probe::load(&row);
    let replayed = edit_script::replay(&mut board, &edits, RouteBudget::JAVA);
    let mut board = board_probe::load(&row);
    let unlimited = edit_script::replay(&mut board, &edits, RouteBudget::UNLIMITED);
    assert_eq!(replayed, unlimited);

    // the probe mirror's lines of routes 0-4 and the dump after route 4
    let mut board = board_probe::load(&row);
    let mut settings = EditSettings::for_board(&board);
    let mut mirror: Vec<String> = Vec::new();
    script(&mut board, "load/push-dynamic", &mut settings, &mut mirror);
    let is_step = |l: &str| l.starts_with("{\"stage\"");
    let route_of = |l: &str| -> i64 {
        let rest = &l[l.find("\"route\":").expect("route") + 8..];
        rest[..rest.find(',').expect(",")].parse().expect("number")
    };
    let steps: Vec<&String> = mirror
        .iter()
        .filter(|l| is_step(l) && route_of(l) <= 4)
        .collect();
    let state_of = |l: &str| l[l.find("\"layer\":").map_or(l.len(), |i| i)..].to_string();
    let replayed_steps: Vec<&String> = replayed
        .iter()
        .filter(|l| l.starts_with("{\"command\""))
        .filter(|l| !l.contains("\"cmd\":\"complete\""))
        .collect();
    assert_eq!(replayed_steps.len(), steps.len());
    for (r, m) in replayed_steps.iter().zip(&steps) {
        if !m.contains("\"kind\":\"cancel\"") && m.contains("\"layer\":") {
            assert_eq!(state_of(r), state_of(m), "\n  replay: {r}\n  mirror: {m}");
        }
    }
    let last = mirror
        .iter()
        .rposition(|l| is_step(l) && route_of(l) == 4)
        .expect("route 4");
    let end = mirror[last + 1..]
        .iter()
        .position(|l| is_step(l))
        .map_or(mirror.len(), |i| last + 1 + i);
    let mirror_dump = &mirror[last + 1..end];
    let replay_dump: Vec<String> = replayed
        .iter()
        .filter(|l| !l.starts_with("{\"command\""))
        .cloned()
        .collect();
    assert_eq!(replay_dump, mirror_dump);
}

//! The line writers of the `insert` and `insert-routed` probes (A06.7), shared by
//! `tests/autoroute_probe_insert.rs` and `examples/route_connections.rs` (A06.8).
//! Both include `tests/autoroute_common/mod.rs` as `crate::autoroute_common`.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use crate::autoroute_common::{
    Conn, Loaded, board_name, ids, load, router_control, sample, sample_line, writer,
};
use freerouting::autoroute::connection::{Ripped, RouteLimits, route_connection};
use freerouting::autoroute::path::RipupCosts;
use freerouting::autoroute::probe::{ProbeValue, record_line};
use freerouting::autoroute::session::search::SearchSettings;
use freerouting::autoroute::session::sink::NoopSink;
use freerouting::board::RoutingBoard;
use freerouting::board::items::{AreaKind, Item, ItemId, ItemKind};
use freerouting::board::search::WorkLimit;
use freerouting::board::structure::FixedState;
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::Point;
use freerouting::scoring::BoardStatistics;

/// At most this many nets are cut and routed per board (as `maze-routed`).
pub const ROUTED_NETS: usize = 12;

/// The maze search's work limit at ripup pass 1, in nominal milliseconds
/// (`100000 * 2^(ripupPassNo - 1)`, `AutorouteConnectionRouter.java:83-89`).
pub const MAZE_LIMIT_MS: i64 = 100_000;

fn rec(out: &mut Vec<String>, fields: &[(&str, ProbeValue)]) {
    out.push(record_line(fields));
}

/// A corner as `[x,y]`, or `["r",xbits,ybits]` when it is not an integer
/// point (Java `cornerJson`).
fn corner_value(point: &Point) -> ProbeValue {
    match point {
        Point::Int(p) => ProbeValue::ints([i64::from(p.x), i64::from(p.y)]),
        other => {
            let f = other.to_float();
            ProbeValue::List(vec!["r".into(), f.x.into(), f.y.into()])
        }
    }
}

fn fixed_name(state: FixedState) -> &'static str {
    match state {
        FixedState::Unfixed => "UNFIXED",
        FixedState::ShoveFixed => "SHOVE_FIXED",
        FixedState::UserFixed => "USER_FIXED",
        FixedState::SystemFixed => "SYSTEM_FIXED",
    }
}

/// Java's `getClass().getSimpleName()` of the item kinds a probe can meet.
fn class_name(item: &Item) -> &'static str {
    match &item.kind {
        ItemKind::Pin(_) => "Pin",
        ItemKind::Via(_) => "Via",
        ItemKind::Trace(_) => "PolylineTrace",
        ItemKind::Area(area) => match area.kind {
            AreaKind::Obstacle => "ObstacleArea",
            AreaKind::ViaObstacle => "ViaObstacleArea",
            AreaKind::ComponentObstacle => "ComponentObstacleArea",
            AreaKind::Conduction { .. } => "ConductionArea",
        },
        ItemKind::ComponentOutline(_) => "ComponentOutline",
        ItemKind::BoardOutline(_) => "BoardOutline",
    }
}

/// The fields of an item after its id (Java `itemFields`).
pub fn item_fields(board: &RoutingBoard, item: &Item) -> Vec<(&'static str, ProbeValue)> {
    let nets = ProbeValue::ints(item.net_numbers().iter().map(|n| i64::from(*n)));
    let class = i64::from(item.clearance_class());
    let fixed = fixed_name(item.fixed_state());
    if let Some(trace) = item.as_trace() {
        let corners = (0..trace.polyline.corner_count())
            .map(|i| corner_value(&trace.polyline.corner(i)))
            .collect();
        return vec![
            ("kind", "trace".into()),
            ("nets", nets),
            ("layer", trace.layer.into()),
            ("halfWidth", trace.half_width.into()),
            ("class", class.into()),
            ("fixed", fixed.into()),
            ("corners", ProbeValue::List(corners)),
        ];
    }
    if item.as_via().is_some() {
        let drill = item.drill().expect("a via has drill data");
        let padstack = board
            .library
            .padstacks
            .get(drill.padstack)
            .map_or(String::new(), |p| p.name.clone());
        return vec![
            ("kind", "via".into()),
            ("nets", nets),
            ("padstack", padstack.into()),
            ("center", corner_value(&Point::Int(drill.center))),
            ("class", class.into()),
            ("fixed", fixed.into()),
        ];
    }
    vec![("kind", class_name(item).into()), ("nets", nets)]
}

/// Every item's line text without its id key, by id.
fn snapshot(board: &RoutingBoard) -> BTreeMap<i32, Vec<(&'static str, ProbeValue)>> {
    board
        .items()
        .map(|item| (item.id().0, item_fields(board, item)))
        .collect()
}

fn keyed_line(key: &str, id: i32, fields: &[(&'static str, ProbeValue)]) -> String {
    let mut all: Vec<(&str, ProbeValue)> = vec![(key, id.into())];
    all.extend(fields.iter().cloned());
    record_line(&all)
}

fn costs_value(costs: &RipupCosts) -> ProbeValue {
    ProbeValue::List(
        costs
            .iter()
            .map(|(id, cost)| ProbeValue::ints([i64::from(id.0), i64::from(*cost)]))
            .collect(),
    )
}

/// One sampled connection on its own load (Java `insertConnection`).
pub fn connection_lines(out: &mut Vec<String>, loaded: &mut Loaded, conn: &Conn) {
    let stage = format!("insert:{}", conn.net);
    if let Some(state) = conn.state {
        rec(
            out,
            &[
                ("stage", stage.as_str().into()),
                ("net", conn.net.into()),
                ("state", state.java_name().into()),
            ],
        );
        return;
    }
    let control = match router_control(loaded, conn.net, true) {
        Ok(control) => control,
        Err(e) => {
            rec(
                out,
                &[
                    ("stage", stage.as_str().into()),
                    ("net", conn.net.into()),
                    ("exception", e.java_exception().into()),
                ],
            );
            return;
        }
    };
    rec(
        out,
        &[
            ("stage", stage.as_str().into()),
            ("net", conn.net.into()),
            ("start", ids(&conn.start)),
            ("dest", ids(&conn.dest)),
        ],
    );
    let settings = SearchSettings::from_router(&loaded.router);
    let board = &mut loaded.board;
    let before = snapshot(board);
    let stats_before = BoardStatistics::new(board);
    let mut ripped: BTreeSet<ItemId> = BTreeSet::new();
    let mut costs = RipupCosts::new();
    let meter = board.search_trees().work_meter();
    let start_units = meter.units();
    let limit = WorkLimit::with_site(meter, MAZE_LIMIT_MS, "maze_search");
    let limits = RouteLimits {
        settings,
        stop: None,
        work_limit: Some(limit),
    };
    let ripped_out = Ripped {
        items: &mut ripped,
        costs: Some(&mut costs),
    };
    let (result, _) = route_connection(
        board,
        &conn.start,
        &conn.dest,
        &control,
        limits,
        ripped_out,
        NoopSink,
    );
    let units = board.search_trees().work_meter().units() - start_units;
    rec(
        out,
        &[
            ("state", result.state.java_name().into()),
            ("details", result.details.as_str().into()),
        ],
    );
    rec(
        out,
        &[("ripped", ids(&ripped)), ("costs", costs_value(&costs))],
    );
    let after = snapshot(board);
    let mut removed = Vec::new();
    for (id, fields) in &before {
        match after.get(id) {
            None => removed.push(*id),
            Some(now) if now != fields => out.push(keyed_line("changed", *id, now)),
            Some(_) => {}
        }
    }
    for (id, fields) in &after {
        if !before.contains_key(id) {
            out.push(keyed_line("added", *id, fields));
        }
    }
    rec(
        out,
        &[(
            "removed",
            ProbeValue::ints(removed.iter().map(|id| i64::from(*id))),
        )],
    );
    let stats_after = BoardStatistics::new(board);
    let incomplete = stats_after.connections.incomplete_count as i64
        - stats_before.connections.incomplete_count as i64;
    let vias = stats_after.vias.total_count as i64 - stats_before.vias.total_count as i64;
    rec(
        out,
        &[
            (
                "maxGeneratedId",
                board.communication.id_generator.max_generated_id().into(),
            ),
            ("incomplete", incomplete.into()),
            ("vias", vias.into()),
            (
                "lengthBefore",
                f64::from(stats_before.traces.total_length).into(),
            ),
            (
                "lengthAfter",
                f64::from(stats_after.traces.total_length).into(),
            ),
            ("units", (units as i64).into()),
        ],
    );
}

/// The `insert` lines of `dsn`: every sampled connection on a fresh load.
pub fn insert_lines(dsn: &str) -> Vec<String> {
    let mut first = Some(load(dsn));
    let conns = sample(&first.as_ref().expect("loaded").board);
    let mut writer = writer("insert", &board_name(dsn));
    sample_line(&mut writer, &first.as_ref().expect("loaded").board, &conns);
    let mut out = writer.into_lines();
    for i in 0..conns.len() {
        let mut loaded = first.take().unwrap_or_else(|| load(dsn));
        let conn = sample(&loaded.board).swap_remove(i);
        connection_lines(&mut out, &mut loaded, &conn);
    }
    out
}

fn is_route_item(item: &Item) -> bool {
    item.is_trace() || item.is_via()
}

/// The `insert-routed` lines of `dsn` (Java `insertRouted`).
pub fn routed_lines(dsn: &str) -> Vec<String> {
    let mut loaded = load(dsn);
    let mut out = writer("insert-routed", &board_name(dsn)).into_lines();
    let unfixed: Vec<ItemId> = loaded
        .board
        .items()
        .filter(|i| is_route_item(i))
        .map(|i| i.id())
        .collect();
    for id in &unfixed {
        loaded.board.unfix(*id);
    }
    rec(
        &mut out,
        &[("stage", "unfix".into()), ("items", ids(&unfixed))],
    );
    let mut done = 0;
    let max_net = loaded.board.rules.nets.max_net_number();
    for net in 1..=max_net {
        if done >= ROUTED_NETS {
            break;
        }
        let routed: Vec<ItemId> = loaded
            .board
            .items()
            .filter(|i| is_route_item(i) && i.contains_net(net))
            .map(|i| i.id())
            .collect();
        if routed.is_empty() {
            continue;
        }
        for id in &routed {
            loaded.board.remove_item(*id);
        }
        let conn = sample(&loaded.board).into_iter().rfind(|c| c.net == net);
        rec(
            &mut out,
            &[
                ("stage", format!("cut:{net}").into()),
                ("removed", ids(&routed)),
                ("connection", conn.is_some().into()),
            ],
        );
        let Some(conn) = conn else {
            continue;
        };
        done += 1;
        connection_lines(&mut out, &mut loaded, &conn);
    }
    out
}

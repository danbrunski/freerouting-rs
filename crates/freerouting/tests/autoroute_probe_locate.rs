//! The `locate` probe (A05.5): the located path of every sampled
//! connection, line for line against
//! `tools/java-reference/probes/autoroute/AutorouteProbe.java`.
//!
//! Per sampled connection, ripup off and on, on one loaded board:
//! `MazeSearchEngine.getInstance`, `findConnection`, then
//! `FoundConnectionLocator.getInstance` with a `TreeSet` of ripped items and
//! a `LinkedHashMap` of costs, as `AutorouteEngine.autorouteConnection`
//! calls it. Printed: the backtrack array (door kind and id, section, next
//! room), the result items (corners as integer points, layer), the ripped
//! item ids in set order with their costs in map order, the start and
//! target items and layers, then the autoroute tree's walk hash after
//! `clear()`. A failed search prints one `{"result":"none"}` line.
//!
//! - `locate_matches_java`: the fixture ladder (to `Issue026-J2_reference`
//!   and the pre-routed boards after it), both variants.
//! - `locate_routed_matches_java`: `locate` through pre-routed traces, as
//!   `maze-routed` walks them (the board's traces and vias unfixed, nets cut
//!   one by one), so the ripped items and their costs of A05.1 are
//!   compared: the ladder's ripup variant enters no obstacle room.

mod autoroute_common;

use std::collections::BTreeSet;

use autoroute_common::{
    Conn, Loaded, board_name, compare_probe, ids, kind_fixtures, load, router_control, sample,
    sample_line, writer,
};
use freerouting::autoroute::path::{Located, RipupCosts, locate};
use freerouting::autoroute::probe::{ProbeValue, record_line};
use freerouting::autoroute::session::search::{MazeSearch, SearchSettings};
use freerouting::autoroute::session::sink::expandable_kind;
use freerouting::autoroute::session::{Room, RoomError, RoomId, Session};
use freerouting::board::items::ItemId;
use freerouting::board::search::ShapeSearchTree;

/// Boards of the ladder whose `locate` output differs from Java's, with
/// the reason. Empty: every board is required.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

/// Boards of the `locate-routed` row whose output differs from Java's,
/// with the reason. Empty: every board is required.
const ROUTED_EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

/// At most this many nets are cut and searched per board (as
/// `maze-routed`).
const ROUTED_NETS: usize = 12;

fn rec(out: &mut Vec<String>, fields: &[(&str, ProbeValue)]) {
    out.push(record_line(fields));
}

fn tree_line(out: &mut Vec<String>, tree: &ShapeSearchTree) {
    let f = tree.fingerprint();
    rec(
        out,
        &[
            ("cleared", tree.key().into()),
            ("nodes", f.nodes.into()),
            ("leaves", f.leaves.into()),
            ("maxDepth", f.max_depth.into()),
            ("shapeHash", format!("{:016x}", f.shape_hash).into()),
        ],
    );
}

/// `[kind, id, layer]` of a room, `[]` for none.
fn room_value(session: &Session<'_>, room: Option<RoomId>) -> ProbeValue {
    let Some(room) = room else {
        return ProbeValue::List(Vec::new());
    };
    let kind = match session.room(room) {
        Room::Obstacle { .. } => "ObstacleExpansionRoom",
        Room::CompleteFree { .. } => "CompleteFreeSpaceExpansionRoom",
        Room::Incomplete { .. } => "IncompleteFreeSpaceExpansionRoom",
    };
    ProbeValue::List(vec![
        kind.into(),
        session.room_java_id(room).unwrap_or(0).into(),
        session.room(room).layer().into(),
    ])
}

fn costs_value(costs: &RipupCosts) -> ProbeValue {
    ProbeValue::List(
        costs
            .iter()
            .map(|(id, cost)| ProbeValue::ints([i64::from(id.0), i64::from(*cost)]))
            .collect(),
    )
}

fn item_id(item: Option<ItemId>) -> ProbeValue {
    item.map_or(-1, |id| id.0).into()
}

fn located_lines(out: &mut Vec<String>, session: &Session<'_>, located: &Located) {
    for (i, element) in located.backtrack.iter().enumerate() {
        rec(
            out,
            &[
                ("back", (i as i64).into()),
                ("kind", expandable_kind(element.door).into()),
                (
                    "door",
                    session.expandable_java_id(element.door).unwrap_or(0).into(),
                ),
                ("section", element.section.into()),
                ("room", room_value(session, element.next_room)),
            ],
        );
    }
    for (i, item) in located.items.iter().enumerate() {
        let corners = item
            .corners
            .iter()
            .map(|p| ProbeValue::ints([i64::from(p.x), i64::from(p.y)]))
            .collect();
        rec(
            out,
            &[
                ("item", (i as i64).into()),
                ("layer", item.layer.into()),
                ("corners", ProbeValue::List(corners)),
            ],
        );
    }
}

/// The stage line, the search, then the locator.
fn search_and_locate(
    out: &mut Vec<String>,
    stage: &str,
    session: &mut Session<'_>,
    conn: &Conn,
    settings: SearchSettings,
) -> Result<(), RoomError> {
    let result = {
        let mut search = MazeSearch::plain(session, settings);
        let ok = search.init(&conn.start, &conn.dest)?;
        rec(
            out,
            &[
                ("stage", stage.into()),
                ("net", conn.net.into()),
                ("ok", ok.into()),
            ],
        );
        if !ok {
            return Ok(());
        }
        search.find_connection()?
    };
    let Some(result) = result else {
        rec(out, &[("result", "none".into())]);
        return Ok(());
    };
    let angle = session.board().rules.trace_angle_restriction();
    let mut ripped: BTreeSet<ItemId> = BTreeSet::new();
    let mut costs = RipupCosts::new();
    match locate(session, &result, angle, &mut ripped, Some(&mut costs)) {
        Ok(located) => {
            located_lines(out, session, &located);
            rec(
                out,
                &[("ripped", ids(&ripped)), ("costs", costs_value(&costs))],
            );
            rec(
                out,
                &[
                    ("start", item_id(located.start_item)),
                    ("target", item_id(located.target_item)),
                    ("startLayer", located.start_layer.into()),
                    ("targetLayer", located.target_layer.into()),
                ],
            );
        }
        Err(e) => rec(
            out,
            &[
                ("exception", e.java_exception().into()),
                ("ripped", ids(&ripped)),
                ("costs", costs_value(&costs)),
            ],
        ),
    }
    Ok(())
}

/// One connection of `locate` and `locate-routed` in one variant (Java
/// `locateConnection`).
fn connection_lines(
    out: &mut Vec<String>,
    loaded: &mut Loaded,
    conn: &Conn,
    variant: &str,
    settings: SearchSettings,
) {
    let ripup = variant == "on";
    let stage = format!("{variant}:{}", conn.net);
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
    let control = match router_control(loaded, conn.net, ripup) {
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
    let class = control.trace_clearance_class_index;
    {
        let Loaded { board, .. } = loaded;
        let mut session = Session::new(board, &control, None, None);
        // An exception of the initialisation comes before the stage line,
        // as in Java.
        if let Err(e) = search_and_locate(out, &stage, &mut session, conn, settings) {
            rec(out, &[("exception", e.java_exception().into())]);
        }
        session.clear();
    }
    let tree = loaded
        .board
        .search_trees()
        .tree_by_class(class)
        .expect("the autoroute tree");
    tree_line(out, tree);
}

/// The `locate` lines of `dsn`.
fn locate_lines(dsn: &str) -> Vec<String> {
    let mut loaded = load(dsn);
    let conns = sample(&loaded.board);
    let settings = SearchSettings::from_router(&loaded.router);
    let mut writer = writer("locate", &board_name(dsn));
    sample_line(&mut writer, &loaded.board, &conns);
    let mut out = writer.into_lines();
    for variant in ["off", "on"] {
        for conn in &conns {
            connection_lines(&mut out, &mut loaded, conn, variant, settings);
        }
    }
    out
}

fn is_route_item(item: &freerouting::board::items::Item) -> bool {
    item.is_trace() || item.is_via()
}

/// The `locate-routed` lines of `dsn` (Java `locateRouted`, the walk of
/// `maze-routed`).
fn routed_lines(dsn: &str) -> Vec<String> {
    let mut loaded = load(dsn);
    let settings = SearchSettings::from_router(&loaded.router);
    let mut out = writer("locate-routed", &board_name(dsn)).into_lines();
    let unfixed: Vec<_> = loaded
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
        let routed: Vec<_> = loaded
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
        for variant in ["off", "on"] {
            connection_lines(&mut out, &mut loaded, &conn, variant, settings);
        }
    }
    out
}

fn compare_kind(kind: &str, expected: &[(&str, &str)], lines: impl Fn(&str) -> Vec<String>) {
    let mut failures = Vec::new();
    let mut boards = 0;
    for dsn in kind_fixtures(kind) {
        let board = board_name(&dsn);
        let result = compare_probe(kind, &dsn, &lines(&dsn));
        boards += 1;
        let listed = expected.iter().any(|(b, _)| *b == board);
        match result {
            Err(report) if !listed => failures.push(report),
            Ok(()) if listed => failures.push(format!(
                "{board}: listed as an expected mismatch but matches Java; remove it"
            )),
            _ => {}
        }
    }
    assert!(boards > 0, "no {kind} fixture");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn locate_matches_java() {
    compare_kind("locate", EXPECTED_MISMATCHES, locate_lines);
}

#[test]
fn locate_routed_matches_java() {
    compare_kind("locate-routed", ROUTED_EXPECTED_MISMATCHES, routed_lines);
}

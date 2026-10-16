//! The `maze-log` writer shared by `autoroute_probe_maze.rs` (A04.8) and
//! `autoroute_search_sinks.rs`: the log of a search stepped by walking the
//! queue between steps (`Mode::Plain`, as the Java probe does) or written
//! from the hooks of `Tee<ProbeSink, WorkCounters>` (`Mode::Hooked`).
#![allow(dead_code)]

use crate::autoroute_common::{
    Loaded, board_name, ids, load, router_control, sample, sample_line, writer,
};
use freerouting::autoroute::probe::{ProbeValue, record_line};
use freerouting::autoroute::session::Session;
use freerouting::autoroute::session::search::{MazeSearch, SearchSettings};
use freerouting::autoroute::session::sink::{
    ExtraCost, NoExtraCost, ProbeSink, SearchSink, Step, Tee, WorkCounters, door_value,
    maze_step_fields,
};
use freerouting::board::search::ShapeSearchTree;

/// The most `occupyNextElement` calls per connection, on both sides.
pub const MAX_STEPS: u64 = 200_000;

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

fn rec(out: &mut Vec<String>, fields: &[(&str, ProbeValue)]) {
    out.push(record_line(fields));
}

fn units(session: &Session<'_>) -> u64 {
    session.board().search_trees().work_meter().units()
}

/// How the step lines are written.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// By walking the queue between steps (`NoopSink`).
    Plain,
    /// From the hooks of `Tee<ProbeSink, WorkCounters>`.
    Hooked,
}

/// The step lines of a search stepped by walking the queue before each
/// step, as the Java probe does.
fn plain_steps<S: SearchSink, C: ExtraCost>(
    search: &mut MazeSearch<'_, '_, S, C>,
    start: u64,
    lines: &mut Vec<String>,
) -> Result<(u64, bool), String> {
    let mut step = 0u64;
    let mut more = true;
    while more && step < MAX_STEPS {
        let s = search.session();
        let mut skipped = 0i64;
        let mut next = None;
        for (_, element) in s.queue().iter() {
            let occupied = s
                .section_state(element.door, element.section)
                .is_some_and(|state| state.occupied);
            if !occupied {
                next = Some(element.clone());
                break;
            }
            skipped += 1;
        }
        let mut fields = maze_step_fields(s, step, next.as_ref());
        more = search.step().map_err(|e| e.java_exception().to_string())? == Step::Continue;
        let s = search.session();
        fields.push(("skipped", skipped.into()));
        if let Some(next) = &next {
            let occupied = s
                .section_state(next.door, next.section)
                .is_some_and(|state| state.occupied);
            fields.push(("occupied", occupied.into()));
        }
        fields.push(("units", ((units(s) - start) as i64).into()));
        lines.push(record_line(&fields));
        step += 1;
    }
    Ok((step, more))
}

/// The hooked search: steps without looking, the sink writes the lines.
fn hooked_steps<C: ExtraCost>(
    search: &mut MazeSearch<'_, '_, Tee<ProbeSink, WorkCounters>, C>,
) -> Result<(u64, bool), String> {
    let mut step = 0u64;
    let mut more = true;
    while more && step < MAX_STEPS {
        more = search.step().map_err(|e| e.java_exception().to_string())? == Step::Continue;
        step += 1;
    }
    Ok((step, more))
}

fn end_line<S: SearchSink, C: ExtraCost>(
    search: &MazeSearch<'_, '_, S, C>,
    steps: u64,
    capped: bool,
    start: u64,
) -> String {
    let s = search.session();
    record_line(&[
        ("steps", (steps as i64).into()),
        ("capped", capped.into()),
        ("destination", door_value(s, search.destination())),
        ("roomCount", s.room_count().into()),
        ("queue", s.queue().len().into()),
        ("units", ((units(s) - start) as i64).into()),
    ])
}

/// Whether the initialisation succeeded, the units it took, and the step
/// and end lines of one connection's search on `session`; or the Java
/// exception name.
type SearchLines = Result<(bool, u64, Vec<String>), String>;

fn search_lines(
    session: &mut Session<'_>,
    conn: &crate::autoroute_common::Conn,
    settings: SearchSettings,
    mode: Mode,
) -> SearchLines {
    let mut lines = Vec::new();
    let exception = |e: freerouting::autoroute::session::RoomError| e.java_exception().to_string();
    match mode {
        Mode::Plain => {
            let start = units(session);
            let mut search = MazeSearch::plain(session, settings);
            let ok = search.init(&conn.start, &conn.dest).map_err(exception)?;
            let init_units = units(search.session()) - start;
            if ok {
                match plain_steps(&mut search, start, &mut lines) {
                    Ok((steps, capped)) => lines.push(end_line(&search, steps, capped, start)),
                    Err(e) => lines.push(record_line(&[("exception", e.into())])),
                }
            }
            Ok((ok, init_units, lines))
        }
        Mode::Hooked => {
            let sink = Tee(ProbeSink::new(), WorkCounters::default());
            let mut search = MazeSearch::new(session, sink, NoExtraCost, settings);
            let start = search.sink().0.start_units();
            let ok = search.init(&conn.start, &conn.dest).map_err(exception)?;
            let init_units = units(search.session()) - start;
            if ok {
                let result = hooked_steps(&mut search);
                lines.extend(search.sink().0.lines().iter().cloned());
                match result {
                    Ok((steps, capped)) => lines.push(end_line(&search, steps, capped, start)),
                    Err(e) => lines.push(record_line(&[("exception", e.into())])),
                }
            }
            Ok((ok, init_units, lines))
        }
    }
}

/// One connection of `maze-log` and `maze-routed` in one variant (Java
/// `mazeConnection`).
fn connection_lines(
    out: &mut Vec<String>,
    loaded: &mut Loaded,
    conn: &crate::autoroute_common::Conn,
    variant: &str,
    settings: SearchSettings,
    mode: Mode,
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
        match search_lines(&mut session, conn, settings, mode) {
            Ok((ok, init_units, lines)) => {
                rec(
                    out,
                    &[
                        ("stage", stage.as_str().into()),
                        ("net", conn.net.into()),
                        ("ok", ok.into()),
                        ("units", (init_units as i64).into()),
                    ],
                );
                out.extend(lines);
            }
            Err(exception) => rec(out, &[("exception", exception.into())]),
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

/// The `maze-log` lines of `dsn`.
pub fn maze_lines(dsn: &str, mode: Mode) -> Vec<String> {
    let mut loaded = load(dsn);
    let conns = sample(&loaded.board);
    let settings = SearchSettings::from_router(&loaded.router);
    let mut writer = writer("maze-log", &board_name(dsn));
    sample_line(&mut writer, &loaded.board, &conns);
    let mut out = writer.into_lines();
    for variant in ["off", "on"] {
        for conn in &conns {
            connection_lines(&mut out, &mut loaded, conn, variant, settings, mode);
        }
    }
    out
}

/// At most this many nets are cut and searched per board by `maze-routed`.
pub const MAZE_ROUTED_NETS: usize = 12;

fn is_route_item(item: &freerouting::board::items::Item) -> bool {
    item.is_trace() || item.is_via()
}

/// The `maze-routed` lines of `dsn` (Java `mazeRouted`): the board's
/// traces and vias unfixed, then net after net (up to 12 nets with route
/// items) the net's route items removed in item order and its sampled
/// connection searched as in `maze-log`, through the other nets' routes.
pub fn routed_lines(dsn: &str, mode: Mode) -> Vec<String> {
    let mut loaded = load(dsn);
    let settings = SearchSettings::from_router(&loaded.router);
    let mut out = writer("maze-routed", &board_name(dsn)).into_lines();
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
        if done >= MAZE_ROUTED_NETS {
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
            connection_lines(&mut out, &mut loaded, &conn, variant, settings, mode);
        }
    }
    out
}

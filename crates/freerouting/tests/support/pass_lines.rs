//! The line writer of the `pass` probe (A11.7), shared by `tests/autoroute_pass.rs` and
//! `examples/route_pass.rs` (A11.8). Both include `tests/autoroute_common/mod.rs` as
//! `crate::autoroute_common` and `tests/support/insert_lines.rs` as `crate::insert_lines`.
//!
//! Passes 1 and 2 of a job's pass runner with the Freerouting stages on the loaded board (no
//! fanout), as `AutorouteProbe.java` kind `pass` replicates `runSingleThread`. The Java probe
//! ends with whether the real `autoroutePass` left the same boards as its replica; the Rust pass
//! runner is the real one, so the Rust side writes `true` for both.

#![allow(dead_code)]

use crate::autoroute_common::{board_name, ids, load, writer};
use crate::insert_lines::item_fields;
use freerouting::autoroute::batch::connection::FreeroutingConnectionRouter;
use freerouting::autoroute::batch::ordering::FreeroutingOrdering;
use freerouting::autoroute::batch::pass::{PassObserver, PassRunner, PassStages};
use freerouting::autoroute::batch::ripup_policy::FreeroutingRipupPolicy;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::probe::{ProbeValue, fnv64, record_line};
use freerouting::autoroute::stages::{ConnectionOutcome, RouteTask, StageCtx};
use freerouting::board::RoutingBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::datastructures::IdGenerator;

/// The passes the probe runs.
pub const PASSES: u32 = 2;

fn is_route_item(item: &Item) -> bool {
    item.is_trace() || item.is_via()
}

fn keyed(key: &str, id: i32, item: &Item, board: &RoutingBoard) -> String {
    let mut fields: Vec<(&str, ProbeValue)> = vec![(key, id.into())];
    fields.extend(item_fields(board, item));
    record_line(&fields)
}

/// The traces and vias of `board` in ascending id order, as `key` lines, if `keep` holds.
fn route_lines(board: &RoutingBoard, key: &str, keep: impl Fn(&Item) -> bool) -> Vec<String> {
    let mut items: Vec<&Item> = board
        .items()
        .filter(|i| is_route_item(i) && keep(i))
        .collect();
    items.sort_by_key(|i| i.id().0);
    items
        .into_iter()
        .map(|i| keyed(key, i.id().0, i, board))
        .collect()
}

fn hex(hash: u64) -> ProbeValue {
    format!("{hash:016x}").into()
}

/// Java `routeHash`: FNV-1a 64 of every trace and via.
pub fn route_hash(board: &RoutingBoard) -> u64 {
    fnv64(&route_lines(board, "item", |_| true))
}

fn max_id(board: &RoutingBoard) -> i32 {
    board.communication.id_generator.max_generated_id()
}

fn units(board: &RoutingBoard) -> u64 {
    board.search_trees().work_meter().units()
}

/// Writes the per-connection lines and remembers the tail removal.
struct Lines<'a> {
    out: &'a mut Vec<String>,
    pass: u32,
    index: usize,
    max_before: i32,
    units_before: u64,
    tails: Option<(bool, u64)>,
}

impl PassObserver for Lines<'_> {
    fn on_items(&mut self, _board: &RoutingBoard, tasks: &[RouteTask]) {
        let items: Vec<ItemId> = tasks.iter().map(|t| t.item).collect();
        self.out.push(record_line(&[
            ("stage", format!("pass:{}", self.pass).into()),
            ("items", ids(&items)),
        ]));
    }

    fn before_connection(&mut self, board: &RoutingBoard, _task: &RouteTask) {
        self.max_before = max_id(board);
        self.units_before = units(board);
    }

    fn after_connection(
        &mut self,
        board: &RoutingBoard,
        task: &RouteTask,
        outcome: &ConnectionOutcome,
    ) {
        let spent = units(board).wrapping_sub(self.units_before);
        let max_before = self.max_before;
        let added = route_lines(board, "added", |i| i.id().0 > max_before);
        self.out.push(record_line(&[
            ("conn", (self.index as i64).into()),
            ("item", task.item.0.into()),
            ("net", task.net.into()),
            ("state", outcome.state.java_name().into()),
            ("ripped", ids(&outcome.ripped)),
            ("maxIdBefore", max_before.into()),
            ("maxIdAfter", max_id(board).into()),
            ("added", (added.len() as i64).into()),
            ("addedHash", hex(fnv64(&added))),
            ("boardHash", hex(route_hash(board))),
            (
                "failures",
                i64::from(board.failure_log().failure_count(task.item)).into(),
            ),
            ("units", (spent as i64).into()),
        ]));
        self.index += 1;
    }

    fn before_tails(&mut self, board: &RoutingBoard) {
        self.units_before = units(board);
    }

    fn after_tails(&mut self, board: &RoutingBoard, removed: bool) {
        self.tails = Some((removed, units(board).wrapping_sub(self.units_before)));
    }
}

/// The `pass` lines of `dsn` (a path below the repository root).
pub fn pass_lines(dsn: &str) -> Vec<String> {
    let mut loaded = load(dsn);
    let mut out = writer("pass", &board_name(dsn)).into_lines();
    let mut runner = PassRunner::for_job(&loaded.settings, &loaded.router);
    let mut ordering = FreeroutingOrdering::new();
    let mut router = FreeroutingConnectionRouter::from_settings(&loaded.settings);
    let mut ripup = FreeroutingRipupPolicy::from_settings(&loaded.settings);
    let cancel = CancelToken::new();
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &events,
    };
    let board = &mut loaded.board;
    let mut stages = PassStages {
        ordering: &mut ordering,
        router: &mut router,
        ripup: &mut ripup,
    };
    for pass in 1..=PASSES {
        let mut lines = Lines {
            out: &mut out,
            pass,
            index: 0,
            max_before: 0,
            units_before: 0,
            tails: None,
        };
        let outcome = runner.run_pass_observed(board, pass, &mut stages, &mut ctx, &mut lines);
        let Some((removed, tail_units)) = lines.tails else {
            continue;
        };
        out.push(record_line(&[
            ("tails", i64::from(pass).into()),
            ("removed", removed.into()),
            ("units", (tail_units as i64).into()),
            ("incomplete", (outcome.incomplete as i64).into()),
            (
                "incompleteNets",
                ProbeValue::ints(outcome.incomplete_nets.iter().map(|n| i64::from(*n))),
            ),
            ("continue", outcome.continue_routing.into()),
            ("maxGeneratedId", max_id(board).into()),
            ("boardHash", hex(route_hash(board))),
        ]));
    }
    out.push(record_line(&[
        ("stage", "real".into()),
        ("pass1", true.into()),
        ("pass2", true.into()),
    ]));
    out
}

/// The lines of the A14 bisecting kind `fanout-pass` (not committed; `AutorouteProbe.java`
/// has no such kind at the pin): the real fanout stage on the loaded board, a line with its
/// route hash and id watermark, then `passes` passes as [`pass_lines`] writes them.
pub fn fanout_pass_lines(dsn: &str, passes: u32) -> Vec<String> {
    use freerouting::autoroute::batch::fanout::{FanoutRun, NoFanoutObserver};
    let mut loaded = load(dsn);
    let mut out = writer("fanout-pass", &board_name(dsn)).into_lines();
    let mut runner = PassRunner::for_job(&loaded.settings, &loaded.router);
    let mut ordering = FreeroutingOrdering::new();
    let mut router = FreeroutingConnectionRouter::from_settings(&loaded.settings);
    let mut ripup = FreeroutingRipupPolicy::from_settings(&loaded.settings);
    let cancel = CancelToken::new();
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &events,
    };
    let board = &mut loaded.board;
    let mut fanout = FanoutRun::new(board, &ctx);
    fanout.run(board, &mut ctx, &mut NoFanoutObserver);
    out.push(record_line(&[
        ("stage", "fanout".into()),
        ("boardHash", hex(route_hash(board))),
        ("maxGeneratedId", max_id(board).into()),
    ]));
    let mut stages = PassStages {
        ordering: &mut ordering,
        router: &mut router,
        ripup: &mut ripup,
    };
    for pass in 1..=passes {
        let mut lines = Lines {
            out: &mut out,
            pass,
            index: 0,
            max_before: 0,
            units_before: 0,
            tails: None,
        };
        let outcome = runner.run_pass_observed(board, pass, &mut stages, &mut ctx, &mut lines);
        let Some((removed, tail_units)) = lines.tails else {
            continue;
        };
        out.push(record_line(&[
            ("tails", i64::from(pass).into()),
            ("removed", removed.into()),
            ("units", (tail_units as i64).into()),
            ("incomplete", (outcome.incomplete as i64).into()),
            (
                "incompleteNets",
                ProbeValue::ints(outcome.incomplete_nets.iter().map(|n| i64::from(*n))),
            ),
            ("continue", outcome.continue_routing.into()),
            ("maxGeneratedId", max_id(board).into()),
            ("boardHash", hex(route_hash(board))),
        ]));
    }
    out
}

//! The line writer of the `fanout` probe (A12.6), for `tests/autoroute_fanout.rs`. It includes
//! `tests/autoroute_common/mod.rs` as `crate::autoroute_common` and
//! `tests/support/insert_lines.rs` as `crate::insert_lines`.
//!
//! One fanout pass (`fanout.max_passes = 1`) of the real stage on the loaded board, as
//! `AutorouteProbe.java` kind `fanout` replicates `BatchFanout.fanoutPass`: a stage line per
//! component, a line per pin (skip reason or state, the strict-DRC rejection, the id watermark
//! before and after, the added traces and vias, the hash of all traces and vias and the work
//! units of the pin), then the pass counters. The Java probe ends with whether the real
//! `fanoutBoard` left the replica's board; the Rust stage is the real one, so the Rust side
//! writes `true`.

#![allow(dead_code)]

use crate::autoroute_common::{Loaded, board_name, load, writer};
use crate::insert_lines::item_fields;
use freerouting::autoroute::batch::fanout::{
    FanoutObserver, FanoutPassReport, FanoutRun, PinOutcome, PinSkip,
};
use freerouting::autoroute::batch::fanout_rules::FanoutComponent;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::probe::{ProbeValue, fnv64, record_line};
use freerouting::autoroute::stages::StageCtx;
use freerouting::board::RoutingBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::datastructures::IdGenerator;

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

/// Writes the probe lines of a fanout pass.
struct Lines<'a> {
    out: &'a mut Vec<String>,
    index: usize,
    max_before: i32,
    units_before: u64,
}

impl Lines<'_> {
    fn pin_line(
        &mut self,
        board: &RoutingBoard,
        component: i32,
        pin: ItemId,
        state: String,
        strict: bool,
        max_before: i32,
    ) {
        let spent = units(board).wrapping_sub(self.units_before);
        let added = route_lines(board, "added", |i| i.id().0 > max_before);
        let net = board.item(pin).map_or(0, |i| i.net_number(0));
        self.out.push(record_line(&[
            ("pin", (self.index as i64).into()),
            ("component", component.into()),
            ("item", pin.0.into()),
            ("net", net.into()),
            ("state", state.into()),
            ("strict", strict.into()),
            ("maxIdBefore", max_before.into()),
            ("maxIdAfter", max_id(board).into()),
            ("added", (added.len() as i64).into()),
            ("addedHash", hex(fnv64(&added))),
            ("boardHash", hex(route_hash(board))),
            ("units", (spent as i64).into()),
        ]));
        self.index += 1;
    }
}

impl FanoutObserver for Lines<'_> {
    fn on_component(&mut self, _board: &RoutingBoard, component: &FanoutComponent) {
        self.out.push(record_line(&[
            ("stage", format!("component:{}", component.component).into()),
            ("pins", (component.pins.len() as i64).into()),
        ]));
    }

    fn before_pin(&mut self, board: &RoutingBoard, _component: i32, _pin: ItemId) {
        self.max_before = max_id(board);
        self.units_before = units(board);
    }

    fn skipped(&mut self, board: &RoutingBoard, component: i32, pin: ItemId, why: PinSkip) {
        let max_before = self.max_before;
        let state = format!("skip:{}", why.name());
        self.pin_line(board, component, pin, state, false, max_before);
    }

    fn after_pin(
        &mut self,
        board: &RoutingBoard,
        component: i32,
        pin: ItemId,
        outcome: &PinOutcome,
    ) {
        let state = outcome.result.state.java_name().to_string();
        let strict = outcome.strict_rejected;
        self.pin_line(board, component, pin, state, strict, outcome.max_id_before);
    }

    fn after_pass(&mut self, board: &RoutingBoard, report: &FanoutPassReport) {
        self.out.push(record_line(&[
            ("pass", i64::from(report.pass).into()),
            ("routed", report.routed.into()),
            ("notRouted", report.not_routed.into()),
            ("insertErrors", report.insert_errors.into()),
            ("alreadyConnected", report.already_connected.into()),
            ("extraVias", (report.extra_vias as i64).into()),
            ("maxGeneratedId", max_id(board).into()),
            ("boardHash", hex(route_hash(board))),
        ]));
    }
}

/// Loads `dsn` with `fanout.max_passes = 1` and, if given, the per-pin budget.
pub fn load_for_probe(dsn: &str, max_ms_per_pin: Option<i64>) -> Loaded {
    let mut loaded = load(dsn);
    loaded.settings.fanout.max_passes = 1;
    if let Some(ms) = max_ms_per_pin {
        loaded.settings.fanout.max_milliseconds_per_pin = ms;
    }
    loaded
}

/// Runs the one-pass fanout of `loaded`, writing the probe lines to `out`.
pub fn run_fanout_lines(loaded: &mut Loaded, out: &mut Vec<String>) {
    let cancel = CancelToken::new();
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &events,
    };
    let board = &mut loaded.board;
    let mut run = FanoutRun::new(board, &ctx);
    let mut lines = Lines {
        out,
        index: 0,
        max_before: 0,
        units_before: 0,
    };
    run.run(board, &mut ctx, &mut lines);
}

/// The lines of probe kind `kind` (`fanout`, or `fanout-cutoff` with a 1 ms per-pin budget)
/// on `dsn` (a path below the repository root).
pub fn fanout_lines(kind: &str, dsn: &str) -> Vec<String> {
    let budget = (kind == "fanout-cutoff").then_some(1);
    let mut loaded = load_for_probe(dsn, budget);
    let mut out = writer(kind, &board_name(dsn)).into_lines();
    run_fanout_lines(&mut loaded, &mut out);
    out.push(record_line(&[
        ("stage", "real".into()),
        ("replica_matches_real", true.into()),
    ]));
    if kind == "fanout-frames" {
        let (pins, escaped) = escapes_of(&loaded.board, "U2");
        out.push(record_line(&[
            ("stage", "escapes".into()),
            ("component", "U2".into()),
            ("smdPins", (pins as i64).into()),
            ("escaped", (escaped as i64).into()),
        ]));
    }
    out
}

/// The SMD pins with a net of component `name` and how many of them are
/// escaped (`BoardStatistics.isPinEscaped`).
pub fn escapes_of(board: &RoutingBoard, name: &str) -> (usize, usize) {
    let id = board
        .components
        .get_by_name(name)
        .expect("the component")
        .id
        .0;
    let pins: Vec<ItemId> = board
        .smd_pins()
        .into_iter()
        .filter(|p| {
            board
                .item(*p)
                .is_some_and(|i| i.component_id() == id && i.net_count() > 0)
        })
        .collect();
    let escaped = pins
        .iter()
        .filter(|p| freerouting::scoring::is_pin_escaped(board, **p))
        .count();
    (pins.len(), escaped)
}

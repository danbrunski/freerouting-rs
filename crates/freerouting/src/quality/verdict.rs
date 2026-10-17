//! One verdict per incomplete connection (Q06.3), and the `verdicts`
//! diagnostic that computes them (`router.special.diagnostics=verdicts`).
//!
//! **Precedence.** Each incomplete connection of the final board (an
//! airline of `calculate_all_incompletes`) gets exactly one [`Verdict`]:
//! 1. [`Verdict::Infeasible`]: Q03 says an end pin cannot be reached at its
//!    class width. The check runs once per job on the input board (before
//!    the fanout), for the end pins of incomplete connections that are
//!    Q03 candidates ([`candidate_pins`]) only;
//! 2. [`Verdict::WorkLimit`]: the connection's last attempt in the run
//!    failed with its maze search cut off by the work limit;
//! 3. [`Verdict::BlockedBy`]: the connection's analysis search exhausted and
//!    at least one blocking item is on a routed (non-fixed) net;
//! 4. [`Verdict::Unrouted`]: anything else, in particular a search that
//!    exhausted with only fixed copper, keepouts or the outline in the way
//!    (KRT's "boxed in").
//!
//! **The analysis.** After the optimizer, every incomplete connection is
//! searched once more by [`analyze_connection`] on a copy of the final
//! board, from its `from` item's connected set to the rest of its net, with
//! ripup off and the parameters of its last attempt (the pass-1 parameters
//! when it has none). Ripup off makes routed copper an obstacle, so the
//! record names the routed nets that wall the connection in; with Java's
//! ripup on, routed traces are entered by ripping and never block a search.
//! Analysing the final board also covers connections that are incomplete
//! because a later connection ripped them, which have no failed attempt.
//! One analysis per connection, so the last analysis wins trivially.
//!
//! **The last attempt** of a connection is the latest connection-router
//! call (in call order) that searched (its state is `Routed`, `Failed` or
//! `InsertError`; skips are not attempts) for a task of its net whose item
//! is in the connection's `from` or `to` connected set on the final board.
//!
//! **Orders.** Connections by ascending `(from item id, to item id)`; every
//! map is a `BTreeMap`; the attribution's own order is in
//! [`super::attribution`]. Q03 runs sequentially here.
//!
//! **Reports change nothing.** The diagnostic reads boards by shared
//! reference, searches on copies with their own work meters, and writes
//! only [`RunSummary::verdicts`]; the SES is byte-identical with it on and
//! off (`tests/quality_attribution.rs`). The JSON schema is
//! `freerouting-verdicts/1`, documented in `docs/development/verdicts.md`.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::FloatPoint;

use super::attribution::{
    Analysis, Attribution, Coverage, NetBlocking, SearchEnd, analyze_connection,
};
use super::feasibility::{
    FeasibilityOptions, PinFeasibility, Verdict as PinVerdict, candidate_pins, check_pin,
    units_per_mm,
};
use crate::autoroute::AttemptState;
use crate::autoroute::batch::connection::{maze_work_budget, net_contains_plane};
use crate::autoroute::batch::report::describe_item;
use crate::autoroute::events::RunSummary;
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, Diagnostics, RouteTask, StageCtx,
};
use crate::board::BasicBoard;
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::structure::Unit;
use crate::drc::AirLine;
use crate::drc::DesignRulesChecker;
use crate::drc::json::JsonValue;
use crate::settings::RouterSettings;
use crate::settings::board::BoardRouterSettings;

/// The schema of [`VerdictReport::to_json`].
pub const SCHEMA: &str = "freerouting-verdicts/1";

/// The registry id of the diagnostic.
pub const DIAGNOSTIC_ID: &str = "verdicts";

/// Why one connection is incomplete.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// Q03: an end pin cannot be reached at its class width (the first
    /// such pin by item id).
    Infeasible(Box<PinFeasibility>),
    /// The last attempt was cut off by its work limit.
    WorkLimit {
        /// The pass of that attempt.
        pass: u32,
        /// The work units it used.
        work: u64,
    },
    /// Routed nets block the connection.
    BlockedBy(Attribution),
    /// Nothing routed is in the way, or the analysis found no wall.
    Unrouted(Coverage),
}

impl Verdict {
    /// The JSON name.
    pub fn name(&self) -> &'static str {
        match self {
            Verdict::Infeasible(_) => "infeasible",
            Verdict::WorkLimit { .. } => "work_limit",
            Verdict::BlockedBy(_) => "blocked_by",
            Verdict::Unrouted(_) => "unrouted",
        }
    }
}

/// One end of an incomplete connection.
#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    pub item: ItemId,
    /// Java's item class name (`pin`, `via`, `polylinetrace`, ...).
    pub kind: &'static str,
    /// `Component-Pin` for a pin, else Java's item string.
    pub name: String,
    /// The names of the first and last layer.
    pub layers: [String; 2],
    /// The airline's end, in mm (DSN coordinates).
    pub at_mm: (f64, f64),
    /// The smallest pad width of a pin or via, mm.
    pub pad_min_width_mm: Option<f64>,
}

/// The last connection-router call of a connection.
#[derive(Clone, Debug, PartialEq)]
pub struct Attempt {
    pub pass: u32,
    pub state: AttemptState,
    pub work: u64,
    pub work_limit_hit: bool,
}

/// The verdict of one incomplete connection, with its evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionVerdict {
    pub net: i32,
    pub net_name: String,
    pub net_class: String,
    /// The class trace width on the `from` item's first layer, mm.
    pub trace_width_mm: f64,
    pub from: Endpoint,
    pub to: Endpoint,
    /// The airline length, mm.
    pub length_mm: f64,
    pub verdict: Verdict,
    /// The last attempt, if the router tried the connection.
    pub attempt: Option<Attempt>,
    /// How the analysis search ended.
    pub search: SearchEnd,
    /// The analysis's attribution (also for verdicts other than
    /// `blocked_by`; Q05 reads it).
    pub attribution: Attribution,
    /// The names of the nets in `attribution`.
    pub blocker_names: BTreeMap<i32, String>,
}

/// The verdicts of a job.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VerdictReport {
    /// One per incomplete connection, by ascending `(from, to)` item id.
    pub connections: Vec<ConnectionVerdict>,
    /// The board's units per mm.
    pub units_per_mm: f64,
}

impl VerdictReport {
    /// The number of connections per verdict name, sorted by name.
    pub fn counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for c in &self.connections {
            *counts.entry(c.verdict.name()).or_insert(0) += 1;
        }
        counts
    }

    /// The report as `freerouting-verdicts/1` JSON.
    pub fn to_json(&self) -> JsonValue {
        let counts = ["infeasible", "work_limit", "blocked_by", "unrouted"]
            .iter()
            .map(|name| {
                let n = self.counts().get(name).copied().unwrap_or(0);
                (name.to_string(), JsonValue::int(n as i64))
            })
            .collect();
        JsonValue::Object(vec![
            ("schema".into(), JsonValue::string(SCHEMA)),
            (
                "count".into(),
                JsonValue::int(self.connections.len() as i64),
            ),
            ("counts".into(), JsonValue::Object(counts)),
            (
                "connections".into(),
                JsonValue::Array(
                    self.connections
                        .iter()
                        .map(|c| c.to_json(self.units_per_mm))
                        .collect(),
                ),
            ),
        ])
    }
}

fn mm4(v: f64) -> JsonValue {
    if v.is_finite() {
        JsonValue::Number(format!("{v:.4}"))
    } else {
        JsonValue::Null
    }
}

fn obj(members: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        members
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

fn int(v: impl Into<i64>) -> JsonValue {
    JsonValue::int(v.into())
}

impl Endpoint {
    fn to_json(&self) -> JsonValue {
        obj(vec![
            ("item", int(self.item.0)),
            ("kind", JsonValue::string(self.kind)),
            ("name", JsonValue::string(&self.name)),
            (
                "layers",
                JsonValue::Array(self.layers.iter().map(JsonValue::string).collect()),
            ),
            ("x_mm", mm4(self.at_mm.0)),
            ("y_mm", mm4(self.at_mm.1)),
            (
                "pad_min_width_mm",
                self.pad_min_width_mm.map_or(JsonValue::Null, mm4),
            ),
        ])
    }
}

fn coverage_json(c: &Coverage) -> JsonValue {
    obj(vec![
        ("entries", int(c.entries)),
        ("routed", int(c.routed)),
        ("fixed", int(c.fixed)),
        ("dropped", int(c.dropped)),
    ])
}

impl ConnectionVerdict {
    fn blocker_json(&self, n: &NetBlocking, names: &dyn Fn(i32) -> String) -> JsonValue {
        let _ = self;
        obj(vec![
            ("net", int(n.net)),
            ("name", JsonValue::string(names(n.net))),
            ("blocked", int(n.blocked)),
            ("unique", int(n.unique)),
            ("near_start", int(n.near_start)),
            ("near_dest", int(n.near_dest)),
        ])
    }

    fn to_json(&self, units_per_mm: f64) -> JsonValue {
        let attempt = self.attempt.as_ref().map_or(JsonValue::Null, |a| {
            obj(vec![
                ("pass", int(a.pass)),
                ("state", JsonValue::string(a.state.java_name())),
                ("work_units", JsonValue::Number(a.work.to_string())),
                ("work_limit_hit", JsonValue::Bool(a.work_limit_hit)),
            ])
        });
        let names = |net: i32| self.blocker_names.get(&net).cloned().unwrap_or_default();
        let blockers = JsonValue::Array(
            self.attribution
                .nets
                .iter()
                .map(|n| self.blocker_json(n, &names))
                .collect(),
        );
        let evidence = match &self.verdict {
            Verdict::Infeasible(pin) => pin.to_json(units_per_mm),
            Verdict::WorkLimit { pass, work } => obj(vec![
                ("pass", int(*pass)),
                ("work_units", JsonValue::Number(work.to_string())),
            ]),
            Verdict::BlockedBy(a) => obj(vec![
                (
                    "top_net",
                    a.nets.first().map_or(JsonValue::Null, |n| int(n.net)),
                ),
                (
                    "top_net_name",
                    a.nets
                        .first()
                        .map_or(JsonValue::Null, |n| JsonValue::string(names(n.net))),
                ),
            ]),
            Verdict::Unrouted(c) => coverage_json(c),
        };
        obj(vec![
            ("net", int(self.net)),
            ("net_name", JsonValue::string(&self.net_name)),
            ("net_class", JsonValue::string(&self.net_class)),
            ("trace_width_mm", mm4(self.trace_width_mm)),
            ("from", self.from.to_json()),
            ("to", self.to.to_json()),
            ("length_mm", mm4(self.length_mm)),
            ("verdict", JsonValue::string(self.verdict.name())),
            ("evidence", evidence),
            ("attempt", attempt),
            ("search", JsonValue::string(self.search.name())),
            ("coverage", coverage_json(&self.attribution.coverage)),
            ("blockers", blockers),
        ])
    }
}

/// The last call of the connection router for a task.
#[derive(Clone, Debug)]
struct LastAttempt {
    /// The call number, from 0.
    seq: u64,
    attempt: Attempt,
    params: ConnectionParams,
}

/// The `verdicts` diagnostic.
#[derive(Debug, Default)]
pub struct VerdictDiagnostics {
    /// The board before the fanout, for Q03.
    input: Option<RoutingBoard>,
    /// The last call per `(net, task item)`.
    attempts: BTreeMap<(i32, ItemId), LastAttempt>,
    calls: u64,
}

impl VerdictDiagnostics {
    /// The diagnostic; it has no settings of its own.
    pub fn from_settings(_settings: &RouterSettings) -> Self {
        VerdictDiagnostics::default()
    }

    /// The verdicts of `board`'s incomplete connections.
    pub fn report(
        &self,
        board: &RoutingBoard,
        board_settings: &BoardRouterSettings,
    ) -> VerdictReport {
        // The queries run on a copy, so the board's meter never counts them.
        let copy = board.clone();
        let board = &copy;
        let mut drc = DesignRulesChecker::new(board);
        drc.calculate_all_incompletes();
        let mut airlines = drc.all_airlines();
        // Item ids ascending (`ItemId`'s own order is Java's descending one).
        airlines.sort_by_key(|a| (a.from_item.0, a.to_item.0));
        let feasibility = self.feasibility(&airlines);
        let connections = airlines
            .iter()
            .map(|airline| self.verdict(board, board_settings, airline, &feasibility))
            .collect();
        VerdictReport {
            connections,
            units_per_mm: units_per_mm(board),
        }
    }

    /// Q03 for the candidate end pins of `airlines`, on the input board.
    fn feasibility(&self, airlines: &[AirLine]) -> BTreeMap<ItemId, PinFeasibility> {
        let Some(input) = &self.input else {
            return BTreeMap::new();
        };
        let ends: BTreeSet<ItemId> = airlines
            .iter()
            .flat_map(|a| [a.from_item, a.to_item])
            .collect();
        let opts = FeasibilityOptions {
            parallel: false,
            ..FeasibilityOptions::default()
        };
        let step = ((opts.step_um * units_per_mm(input) / 1000.0).round() as i32).max(1);
        candidate_pins(input, opts.all_pins)
            .into_iter()
            .filter(|id| ends.contains(id))
            .filter_map(|id| check_pin(input, id, step, &opts).map(|pin| (id, pin)))
            .collect()
    }

    /// The last attempt of the connection `airline`.
    fn last_attempt(&self, board: &RoutingBoard, airline: &AirLine) -> Option<&LastAttempt> {
        let net = airline.net_number;
        let mut items = board.connected_set(airline.from_item, net, false);
        items.extend(board.connected_set(airline.to_item, net, false));
        items.insert(airline.from_item);
        items.insert(airline.to_item);
        self.attempts
            .iter()
            .filter(|((n, item), _)| *n == net && items.contains(item))
            .map(|(_, last)| last)
            .max_by_key(|last| last.seq)
    }

    fn verdict(
        &self,
        board: &RoutingBoard,
        board_settings: &BoardRouterSettings,
        airline: &AirLine,
        feasibility: &BTreeMap<ItemId, PinFeasibility>,
    ) -> ConnectionVerdict {
        let net = airline.net_number;
        let last = self.last_attempt(board, airline);
        let params = last.map_or_else(
            || default_params(board, net, board_settings),
            |l| l.params.clone(),
        );
        let analysis: Analysis =
            analyze_connection(board, net, airline.from_item, board_settings, &params);
        let mut ends = [airline.from_item, airline.to_item];
        ends.sort_by_key(|id| id.0);
        let infeasible: Option<&PinFeasibility> = ends.iter().find_map(|id| {
            feasibility
                .get(id)
                .filter(|pin| pin.verdict == PinVerdict::Infeasible)
        });
        let attempt = last.map(|l| l.attempt.clone());
        let verdict = if let Some(pin) = infeasible {
            Verdict::Infeasible(Box::new(pin.clone()))
        } else if let Some(a) = attempt.as_ref().filter(|a| {
            a.work_limit_hit && matches!(a.state, AttemptState::Failed | AttemptState::InsertError)
        }) {
            Verdict::WorkLimit {
                pass: a.pass,
                work: a.work,
            }
        } else if analysis.end == SearchEnd::Exhausted && !analysis.attribution.nets.is_empty() {
            Verdict::BlockedBy(analysis.attribution.clone())
        } else {
            Verdict::Unrouted(analysis.attribution.coverage)
        };
        let rules = &board.rules;
        let net_name = rules
            .nets
            .get(net)
            .map_or_else(String::new, |n| n.name.clone());
        let net_class = rules
            .net_class_of(net)
            .map_or_else(String::new, |c| c.name().to_string());
        let upm = units_per_mm(board);
        let mut blocker_names = BTreeMap::new();
        for n in &analysis.attribution.nets {
            let name = rules
                .nets
                .get(n.net)
                .map_or_else(String::new, |x| x.name.clone());
            blocker_names.insert(n.net, name);
        }
        ConnectionVerdict {
            net,
            net_name,
            net_class,
            trace_width_mm: 2.0 * f64::from(rules.trace_half_width(net, airline.from_layers[0]))
                / upm,
            from: endpoint(
                board,
                airline.from_item,
                airline.from_corner,
                airline.from_layers,
            ),
            to: endpoint(board, airline.to_item, airline.to_corner, airline.to_layers),
            length_mm: airline.from_corner.distance(airline.to_corner) / upm,
            verdict,
            attempt,
            search: analysis.end,
            attribution: analysis.attribution,
            blocker_names,
        }
    }
}

/// The pass-1 parameters of a connection of `net` with ripup off.
fn default_params(board: &RoutingBoard, net: i32, s: &BoardRouterSettings) -> ConnectionParams {
    ConnectionParams {
        pass: 1,
        ripup_allowed: false,
        ripup_cost: 0,
        via_cost: if net_contains_plane(board, net) {
            s.plane_via_costs()
        } else {
            s.via_costs()
        },
        trace_costs: s.trace_costs().to_vec(),
        remove_unconnected_vias: false,
        work_budget: maze_work_budget(1),
        pull_tight_accuracy: 0,
        strict_check: false,
    }
}

fn endpoint(board: &BasicBoard, id: ItemId, at: FloatPoint, layers: [i32; 2]) -> Endpoint {
    let layer_name = |l: i32| {
        board
            .layer_structure
            .layers
            .get(l as usize)
            .map_or_else(|| l.to_string(), |layer| layer.name.clone())
    };
    let upm = units_per_mm(board);
    Endpoint {
        item: id,
        kind: board.item(id).map_or("(unknown)", |i| i.type_name()),
        name: describe_item(board, id),
        layers: [layer_name(layers[0]), layer_name(layers[1])],
        at_mm: to_mm(board, at),
        pad_min_width_mm: board
            .min_width(id)
            .filter(|w| *w < f64::from(i32::MAX))
            .map(|w| w / upm),
    }
}

/// `p` in DSN mm.
fn to_mm(board: &BasicBoard, p: FloatPoint) -> (f64, f64) {
    let comm = &board.communication;
    let [x, y] = comm.coordinate_transform.board_to_dsn_point(p);
    (
        Unit::scale(x, comm.unit, Unit::Mm),
        Unit::scale(y, comm.unit, Unit::Mm),
    )
}

impl Diagnostics for VerdictDiagnostics {
    fn id(&self) -> &'static str {
        DIAGNOSTIC_ID
    }

    fn on_input(&mut self, board: &RoutingBoard, _ctx: &StageCtx) {
        self.input = Some(board.clone());
    }

    fn after_connection(
        &mut self,
        _board: &RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        outcome: &ConnectionOutcome,
        _ctx: &StageCtx,
    ) {
        // A skip (nothing to connect, already connected, on a plane) is not
        // an attempt: only calls that searched are recorded.
        if !matches!(
            outcome.state,
            AttemptState::Routed | AttemptState::Failed | AttemptState::InsertError
        ) {
            return;
        }
        let seq = self.calls;
        self.calls += 1;
        self.attempts.insert(
            (task.net, task.item),
            LastAttempt {
                seq,
                attempt: Attempt {
                    pass: p.pass,
                    state: outcome.state,
                    work: outcome.work.units,
                    work_limit_hit: outcome.work_limit_hit,
                },
                params: p.clone(),
            },
        );
    }

    fn finish(&mut self, board: &RoutingBoard, ctx: &StageCtx, summary: &mut RunSummary) {
        summary.verdicts = Some(self.report(board, ctx.board_settings));
    }
}

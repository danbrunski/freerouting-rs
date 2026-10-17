//! Rip instrumentation (Q05.1): the `rip-stats` diagnostic
//! (`router.special.diagnostics=rip-stats`).
//!
//! For every net it records, in a `BTreeMap<i32, NetRipStats>`:
//! - **rips**: connection attempts of the pass loop that removed copper of
//!   the net (one per attempt and net, however many items went);
//! - **torn length**: the trace length the net lost in those attempts;
//! - **reroutes**: routed connections of the net that closed an earlier
//!   rip (each routed attempt of a net with an open rip closes one rip);
//! - **pins** and **span** (the diagonal of the net's pin-centre bounding
//!   box on the input board), and KRT's size factor
//!   `sqrt(pins)·sqrt(span_mm)` (`py_router/net_cost.py` at `0486e799`,
//!   credited from the idea; no code follows it).
//!
//! **How a rip is seen.** The diagnostic sees the board before and after
//! each connection-router call ([`Diagnostics::before_connection`],
//! [`Diagnostics::after_connection`]); it never sees the ripup itself.
//! Before the call it keeps the id, net and polyline (an `Arc`, so cheap)
//! of every trace and via. After a call whose outcome has a non-empty
//! ripped set, the *ripped nets* are the nets of the ripped items, and for
//! each of them:
//! - removed = its traces and vias from before the call that are gone;
//! - added = its traces with an id from the call's first new id on;
//! - torn length = removed trace length − added trace length, at least 0.
//!
//! The subtraction removes what the pull-tight after a routed connection
//! did to the ripped net's other traces (`opt_changed_area` runs on every
//! net and replaces a trace by a shorter one with a new id): those come
//! back as added. What remains of it is the shortening, which the torn
//! length overstates by. A net counts a rip only if something of it was
//! removed, so a strict-DRC rollback or a ripped set left filled by a
//! failed attempt (A06's quirk) counts nothing.
//!
//! **Scope.** Only the connection router's calls are seen (the batch
//! passes). The fanout's pin attempts rip too but are not a
//! [`ConnectionRouter`](crate::autoroute::stages::ConnectionRouter) call;
//! the default settings do not run the fanout. History restores between
//! passes are not undone in the counts: they count what the router did,
//! not what is on the final board.
//!
//! **Orders.** Calls in call order; within a call, items in ascending id;
//! every map is a `BTreeMap`. The top nets are by torn length descending,
//! ties by ascending net number. Lengths are summed in board units in that
//! order, then divided by the board's units per mm, so a report is a
//! function of the input and the settings.
//!
//! **Reports change nothing.** The diagnostic reads the board by shared
//! reference and writes only `RunSummary::rip_stats`; the SES is
//! byte-identical with it on and off (`tests/quality_rip_stats.rs`). The
//! JSON schema is `freerouting-rip-stats/1` ([`RipReport::to_json`]); the
//! CLI writes it under `rip_stats` in the result manifest and
//! `freerouting-dev bench --rip-stats` in each Rust run record.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::Polyline;

use crate::autoroute::AttemptState;
use crate::autoroute::events::RunSummary;
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, Diagnostics, RouteTask, StageCtx,
};
use crate::board::RoutingBoard;
use crate::board::items::{ItemId, ItemKind};
use crate::drc::json::JsonValue;
use crate::quality::feasibility::units_per_mm;
use crate::settings::RouterSettings;

/// The schema of [`RipReport::to_json`].
pub const SCHEMA: &str = "freerouting-rip-stats/1";

/// The registry id of the diagnostic.
pub const DIAGNOSTIC_ID: &str = "rip-stats";

/// How many nets [`RipReport::top_nets`] lists in the JSON.
pub const TOP_NETS: usize = 5;

/// What one net went through (lengths in board units).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetRipStats {
    /// Connection attempts that removed copper of the net.
    pub rips: u32,
    /// Trace length the net lost, board units.
    pub torn_length: f64,
    /// Items (traces and vias) the net lost, before the pull-tight
    /// correction (an upper bound).
    pub removed_items: u32,
    /// Routed connections of the net that closed an earlier rip.
    pub reroutes: u32,
    /// Pins of the net on the input board.
    pub pins: u32,
    /// The diagonal of the net's pin-centre bounding box, board units.
    pub span: f64,
}

/// One pass's totals (lengths in board units).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PassRipStats {
    /// Connection attempts that removed copper of another net.
    pub ripping_attempts: u32,
    /// Rips, summed over the nets.
    pub rips: u32,
    /// Torn trace length, board units.
    pub torn_length: f64,
    /// Reroutes.
    pub reroutes: u32,
}

/// One net in [`RipReport::top_nets`].
#[derive(Clone, Debug, PartialEq)]
pub struct TopNet<'a> {
    pub net: i32,
    pub stats: &'a NetRipStats,
    /// Its share of the run's torn length (0 when nothing was torn).
    pub share: f64,
}

/// The rip statistics of a run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RipReport {
    /// Per net, every net with a pin on the input board or a rip.
    pub nets: BTreeMap<i32, NetRipStats>,
    /// Net names, for the JSON.
    pub net_names: BTreeMap<i32, String>,
    /// Per pass number.
    pub passes: BTreeMap<u32, PassRipStats>,
    /// The board's units per mm.
    pub units_per_mm: f64,
}

impl RipReport {
    /// The run's torn length, board units (nets in ascending order).
    pub fn torn_length(&self) -> f64 {
        self.nets.values().map(|n| n.torn_length).sum()
    }

    /// The run's rips, summed over the nets.
    pub fn rips(&self) -> u64 {
        self.nets.values().map(|n| u64::from(n.rips)).sum()
    }

    /// The run's reroutes.
    pub fn reroutes(&self) -> u64 {
        self.nets.values().map(|n| u64::from(n.reroutes)).sum()
    }

    /// The nets that lost copper, by torn length descending, ties by
    /// ascending net number, at most `limit`.
    pub fn top_nets(&self, limit: usize) -> Vec<TopNet<'_>> {
        let total = self.torn_length();
        let mut nets: Vec<(i32, &NetRipStats)> = self
            .nets
            .iter()
            .filter(|(_, s)| s.torn_length > 0.0)
            .map(|(&net, s)| (net, s))
            .collect();
        nets.sort_by(|a, b| {
            b.1.torn_length
                .total_cmp(&a.1.torn_length)
                .then(a.0.cmp(&b.0))
        });
        nets.into_iter()
            .take(limit)
            .map(|(net, stats)| TopNet {
                net,
                stats,
                share: if total > 0.0 {
                    stats.torn_length / total
                } else {
                    0.0
                },
            })
            .collect()
    }

    /// The largest share of the torn length one net has (Q05's decision
    /// rule reads "more than 25 %"), `None` when nothing was torn.
    pub fn top_share(&self) -> Option<f64> {
        self.top_nets(1).first().map(|t| t.share)
    }

    /// The `freerouting-rip-stats/1` JSON. `top_share` is omitted when
    /// nothing was torn (the writer drops null members, as Gson does).
    pub fn to_json(&self) -> JsonValue {
        let upm = self.units_per_mm;
        let ripping_attempts: u64 = self
            .passes
            .values()
            .map(|p| u64::from(p.ripping_attempts))
            .sum();
        let nets = self
            .nets
            .iter()
            .filter(|(_, s)| s.rips > 0 || s.reroutes > 0)
            .map(|(&net, s)| self.net_json(net, s, None))
            .collect();
        let top = self
            .top_nets(TOP_NETS)
            .into_iter()
            .map(|t| self.net_json(t.net, t.stats, Some(t.share)))
            .collect();
        let passes = self
            .passes
            .iter()
            .map(|(&pass, p)| {
                obj(vec![
                    ("pass", JsonValue::int(i64::from(pass))),
                    (
                        "ripping_attempts",
                        JsonValue::int(i64::from(p.ripping_attempts)),
                    ),
                    ("rips", JsonValue::int(i64::from(p.rips))),
                    ("torn_length_mm", num4(p.torn_length / upm)),
                    ("reroutes", JsonValue::int(i64::from(p.reroutes))),
                ])
            })
            .collect();
        obj(vec![
            ("schema", JsonValue::string(SCHEMA)),
            ("ripping_attempts", JsonValue::int(ripping_attempts as i64)),
            ("rips", JsonValue::int(self.rips() as i64)),
            ("torn_length_mm", num4(self.torn_length() / upm)),
            ("reroutes", JsonValue::int(self.reroutes() as i64)),
            ("top_share", self.top_share().map_or(JsonValue::Null, num4)),
            ("top_nets", JsonValue::Array(top)),
            ("passes", JsonValue::Array(passes)),
            ("nets", JsonValue::Array(nets)),
        ])
    }

    fn net_json(&self, net: i32, s: &NetRipStats, share: Option<f64>) -> JsonValue {
        let upm = self.units_per_mm;
        let span_mm = s.span / upm;
        let mut members = vec![
            ("net", JsonValue::int(i64::from(net))),
            (
                "net_name",
                JsonValue::string(self.net_names.get(&net).cloned().unwrap_or_default()),
            ),
            ("torn_length_mm", num4(s.torn_length / upm)),
        ];
        if let Some(share) = share {
            members.push(("share", num4(share)));
        }
        members.extend([
            ("rips", JsonValue::int(i64::from(s.rips))),
            ("removed_items", JsonValue::int(i64::from(s.removed_items))),
            ("reroutes", JsonValue::int(i64::from(s.reroutes))),
            ("pins", JsonValue::int(i64::from(s.pins))),
            ("span_mm", num4(span_mm)),
            ("size_factor", num4(size_factor(s.pins, span_mm))),
        ]);
        obj(members)
    }
}

/// KRT's net size `sqrt(pins)·sqrt(span_mm)`.
pub fn size_factor(pins: u32, span_mm: f64) -> f64 {
    f64::from(pins).sqrt() * span_mm.max(0.0).sqrt()
}

fn num4(v: f64) -> JsonValue {
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

/// A trace or via of one net, as it was before a call.
#[derive(Clone, Debug)]
struct Copper {
    id: ItemId,
    net: i32,
    /// `None` for a via.
    polyline: Option<Polyline>,
}

/// What one call removed and added of one ripped net (board units).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TornNet {
    /// Traces and vias from before the call that are gone.
    pub removed_items: u32,
    /// Their trace length.
    pub removed_length: f64,
    /// The length of the net's traces the call added.
    pub added_length: f64,
}

impl TornNet {
    /// The torn length: removed minus added, at least 0.
    pub fn torn_length(&self) -> f64 {
        (self.removed_length - self.added_length).max(0.0)
    }
}

/// The single net of a trace or via, if it has exactly one.
fn single_net(nets: &[i32]) -> Option<i32> {
    match nets {
        [net] => Some(*net),
        _ => None,
    }
}

/// The traces and vias of `board`, by ascending id.
fn copper_of(board: &RoutingBoard) -> Vec<Copper> {
    let mut copper: Vec<Copper> = board
        .items()
        .filter_map(|item| {
            let polyline = match &item.kind {
                ItemKind::Trace(t) => Some(t.polyline.clone()),
                ItemKind::Via(_) => None,
                _ => return None,
            };
            Some(Copper {
                id: item.id(),
                net: single_net(item.net_numbers())?,
                polyline,
            })
        })
        .collect();
    copper.sort_by_key(|c| c.id);
    copper
}

/// What the call that went from `before` to `board` removed and added of
/// the nets of `ripped`, by ascending net.
fn torn_nets(
    before: &[Copper],
    board: &RoutingBoard,
    ripped: &BTreeSet<ItemId>,
    first_new_id: i32,
) -> BTreeMap<i32, TornNet> {
    let find = |id: ItemId| before.binary_search_by_key(&id, |c| c.id).ok();
    let nets: BTreeSet<i32> = ripped
        .iter()
        .filter_map(|&id| find(id).map(|i| before[i].net))
        .collect();
    let mut torn: BTreeMap<i32, TornNet> = BTreeMap::new();
    for c in before.iter().filter(|c| nets.contains(&c.net)) {
        if board.item(c.id).is_some() {
            continue;
        }
        let entry = torn.entry(c.net).or_default();
        entry.removed_items += 1;
        if let Some(polyline) = &c.polyline {
            entry.removed_length += polyline.length_approx();
        }
    }
    // `items()` is highest id first: the call's new items lead.
    let mut added: Vec<(ItemId, i32, f64)> = board
        .items()
        .take_while(|item| item.id().0 >= first_new_id)
        .filter_map(|item| {
            let trace = item.as_trace()?;
            let net = single_net(item.net_numbers())?;
            torn.contains_key(&net)
                .then(|| (item.id(), net, trace.polyline.length_approx()))
        })
        .collect();
    added.sort_by_key(|a| a.0);
    for (_, net, length) in added {
        if let Some(entry) = torn.get_mut(&net) {
            entry.added_length += length;
        }
    }
    torn
}

/// The `rip-stats` diagnostic.
#[derive(Debug, Default)]
pub struct RipStats {
    report: RipReport,
    /// The copper before the current call.
    before: Vec<Copper>,
    /// Rips not yet closed by a reroute, per net.
    open: BTreeMap<i32, u32>,
}

impl RipStats {
    /// The diagnostic for `settings` (it has no settings of its own).
    pub fn from_settings(_settings: &RouterSettings) -> Self {
        RipStats::default()
    }

    /// The report so far.
    pub fn report(&self) -> &RipReport {
        &self.report
    }

    /// Records one call: `before` is the board before it, `board` after.
    fn record(
        &mut self,
        board: &RoutingBoard,
        task: &RouteTask,
        pass: u32,
        outcome: &ConnectionOutcome,
    ) {
        let pass_stats = self.report.passes.entry(pass).or_default();
        if outcome.state == AttemptState::Routed
            && let Some(open) = self.open.get_mut(&task.net)
            && *open > 0
        {
            *open -= 1;
            self.report.nets.entry(task.net).or_default().reroutes += 1;
            pass_stats.reroutes += 1;
        }
        if outcome.ripped.is_empty() {
            return;
        }
        let torn = torn_nets(&self.before, board, &outcome.ripped, outcome.first_new_id);
        if torn.is_empty() {
            return;
        }
        pass_stats.ripping_attempts += 1;
        for (net, t) in torn {
            let stats = self.report.nets.entry(net).or_default();
            stats.rips += 1;
            stats.removed_items += t.removed_items;
            stats.torn_length += t.torn_length();
            pass_stats.rips += 1;
            pass_stats.torn_length += t.torn_length();
            *self.open.entry(net).or_default() += 1;
        }
    }
}

/// Pins and pin-centre span per net of `board`.
fn pins_and_spans(board: &RoutingBoard) -> BTreeMap<i32, (u32, f64)> {
    let mut boxes: BTreeMap<i32, (u32, [f64; 4])> = BTreeMap::new();
    let mut pins: Vec<(ItemId, Vec<i32>, f64, f64)> = board
        .items()
        .filter_map(|item| {
            let pin = item.as_pin()?;
            let c = pin.drill.center.to_float();
            Some((item.id(), item.net_numbers().to_vec(), c.x, c.y))
        })
        .collect();
    pins.sort_by_key(|p| p.0);
    for (_, nets, x, y) in pins {
        for net in nets {
            let (count, b) = boxes
                .entry(net)
                .or_insert((0, [f64::MAX, f64::MAX, f64::MIN, f64::MIN]));
            *count += 1;
            *b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
    }
    boxes
        .into_iter()
        .map(|(net, (count, b))| {
            let (dx, dy) = (b[2] - b[0], b[3] - b[1]);
            (net, (count, (dx * dx + dy * dy).sqrt()))
        })
        .collect()
}

impl Diagnostics for RipStats {
    fn id(&self) -> &'static str {
        DIAGNOSTIC_ID
    }

    fn on_input(&mut self, board: &RoutingBoard, _ctx: &StageCtx) {
        for (net, (pins, span)) in pins_and_spans(board) {
            let stats = self.report.nets.entry(net).or_default();
            stats.pins = pins;
            stats.span = span;
        }
    }

    fn before_connection(
        &mut self,
        board: &RoutingBoard,
        _task: &RouteTask,
        _p: &ConnectionParams,
        _ctx: &StageCtx,
    ) {
        self.before = copper_of(board);
    }

    fn after_connection(
        &mut self,
        board: &RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        outcome: &ConnectionOutcome,
        _ctx: &StageCtx,
    ) {
        self.record(board, task, p.pass, outcome);
        self.before.clear();
    }

    fn finish(&mut self, board: &RoutingBoard, _ctx: &StageCtx, summary: &mut RunSummary) {
        self.report.units_per_mm = units_per_mm(board);
        self.report.net_names = self
            .report
            .nets
            .keys()
            .filter_map(|&net| board.rules.nets.get(net).map(|n| (net, n.name.clone())))
            .collect();
        summary.rip_stats = Some(self.report.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(torn_mm: f64, rips: u32) -> NetRipStats {
        NetRipStats {
            rips,
            torn_length: torn_mm * 10_000.0,
            ..NetRipStats::default()
        }
    }

    fn report(nets: &[(i32, NetRipStats)]) -> RipReport {
        RipReport {
            nets: nets.iter().cloned().collect(),
            units_per_mm: 10_000.0,
            ..RipReport::default()
        }
    }

    #[test]
    fn top_nets_are_by_torn_length_then_net_number() {
        let r = report(&[
            (7, stats(2.0, 1)),
            (3, stats(5.0, 2)),
            (5, stats(2.0, 4)),
            (9, stats(0.0, 0)),
            (1, stats(1.0, 1)),
        ]);
        let top: Vec<i32> = r.top_nets(TOP_NETS).iter().map(|t| t.net).collect();
        assert_eq!(top, vec![3, 5, 7, 1], "net 9 tore nothing");
        let shares: Vec<f64> = r.top_nets(2).iter().map(|t| t.share).collect();
        assert_eq!(shares, vec![0.5, 0.2]);
        assert_eq!(r.top_share(), Some(0.5));
        assert_eq!(r.rips(), 8);
    }

    #[test]
    fn nothing_torn_has_no_top_share() {
        let r = report(&[(1, stats(0.0, 0))]);
        assert_eq!(r.top_share(), None);
        assert!(r.top_nets(TOP_NETS).is_empty());
        let json = r.to_json().to_pretty_string();
        // The writer omits null members (Gson's default).
        assert!(!json.contains("top_share"), "{json}");
        assert!(json.contains("\"schema\": \"freerouting-rip-stats/1\""));
    }

    #[test]
    fn torn_length_subtracts_what_the_call_added_and_never_goes_negative() {
        let t = TornNet {
            removed_items: 2,
            removed_length: 30.0,
            added_length: 12.5,
        };
        assert_eq!(t.torn_length(), 17.5);
        let pulled = TornNet {
            removed_length: 10.0,
            added_length: 11.0,
            ..t
        };
        assert_eq!(pulled.torn_length(), 0.0);
    }

    #[test]
    fn size_factor_is_sqrt_pins_times_sqrt_span() {
        assert_eq!(size_factor(4, 9.0), 6.0);
        assert_eq!(size_factor(0, 9.0), 0.0);
        assert_eq!(size_factor(87, 0.0), 0.0);
    }

    #[test]
    fn json_lists_the_top_five_with_shares_and_every_ripped_net() {
        let nets: Vec<(i32, NetRipStats)> = (1..=7).map(|n| (n, stats(f64::from(n), 1))).collect();
        let r = report(&nets);
        let json = r.to_json();
        let JsonValue::Object(members) = &json else {
            panic!("an object");
        };
        let get = |key: &str| members.iter().find(|(k, _)| k == key).map(|(_, v)| v);
        let Some(JsonValue::Array(top)) = get("top_nets") else {
            panic!("top_nets");
        };
        assert_eq!(top.len(), TOP_NETS);
        let Some(JsonValue::Array(all)) = get("nets") else {
            panic!("nets");
        };
        assert_eq!(all.len(), 7);
        assert_eq!(
            get("torn_length_mm"),
            Some(&JsonValue::Number("28.0000".into()))
        );
        assert_eq!(get("top_share"), Some(&JsonValue::Number("0.2500".into())));
    }
}

//! Pour region repair (X02.3): straps that re-join a plane net's split pour
//! regions, a board stage after the routing passes (`krt-region-repair`).
//!
//! A pour can end up split into islands after routing (or before it: a
//! pre-routed trace across it), and Freerouting does not notice, because its
//! connectivity treats a plane as one conductor. [`RegionRepair`] runs at
//! [`StagePhase::AfterRouting`], before the optimizer, when a job lists its
//! id in `router.special.stages`. Default off: with the id unlisted nothing
//! here runs.
//!
//! For each plane net, ascending net number, and each layer of its planes,
//! ascending:
//! 1. the regions: the net's islands on the layer that have anchors, grouped
//!    by electrical group ([`PlaneComponents::regions`]); one region needs
//!    nothing. Islands without anchors (dead copper) are not joined;
//! 2. the edges between every two regions, weighted by the smallest squared
//!    anchor-to-anchor distance (`i64`, exact: coordinates are within
//!    `CRIT_INT`, so a squared distance is at most 2^53), in Kruskal order:
//!    weight, then the pair of region indices; regions are indexed by their
//!    lowest anchor `ItemId`;
//! 3. for each edge in that order whose regions are not yet joined, a strap
//!    from the anchors of one region to the anchors of the other, routed by
//!    Freerouting's connection router ([`route_connection`]) with only the
//!    other region's anchors as destinations
//!    ([`AutorouteControl::destinations_only`]: the net's own plane, which
//!    Freerouting counts as connecting everything, is not a target), on the
//!    net's routing layers, without vias (so never a via in a pad, D8),
//!    without ripup and without neck-down, at each width of the ladder in
//!    turn: 2.0, 1.0, 0.5 mm (those wider than the net class width), then
//!    the class width. Never narrower than the class (D8, Q00 rule 7). The
//!    anchors are drill items or pads on the plane layer, so a strap on
//!    another layer between two through-hole anchors joins the islands too;
//!    the router picks the layer by its costs. Each attempt is a B09 transaction
//!    ([`RoutingBoard::try_change`], new items only): it is kept when the
//!    exact clearance check of the new items finds nothing, else undone. The
//!    first width kept wins; a strap that fails at every width is reported
//!    and not retried (a later edge may still join its regions).
//!
//! After a net layer that got a strap, the plane components are rebuilt, so
//! straps that join regions on another layer (through through-hole pins)
//! are seen.
//!
//! The strap is copper of the plane's net: Java's fill ignores same-net
//! traces, so the fill keeps its island count, but the islands are one
//! electrical group ([`PlaneComponents::joined`]). To Freerouting the strap
//! is a cycle through the plane, so it is routed with the net class's
//! `ignore_cycles_with_areas` set (restored at once) and then user fixed
//! (see `route_strap`): no normalization or optimizer pass removes it,
//! and the SES writes it as `protect`.
//!
//! Each strap search has the maze budget of a pass-1 connection
//! ([`maze_budget_ms`]`(1)`), counted in work units on the board's meter;
//! the cancel token only stops between straps and inside the search.
//!
//! The idea (regions from anchors, a Kruskal MST over regions, a strap per
//! MST edge, a width ladder) is KiCadRoutingTools' plane finalize
//! (`repair_planes.py`, `plane_region_connector.py`:
//! `find_disconnected_zone_regions`, `route_plane_connection_wide`, at
//! `0486e799`); no code is ported: the routing is Freerouting's.

use std::collections::BTreeSet;

use freerouting::autoroute::AttemptState;
use freerouting::autoroute::batch::connection::maze_budget_ms;
use freerouting::autoroute::connection::{Ripped, RouteLimits, route_connection};
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::session::search::SearchSettings;
use freerouting::autoroute::session::sink::NoopSink;
use freerouting::autoroute::stages::{BoardStage, StageCtx, StagePhase, StageReport};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::search::WorkLimit;
use freerouting::board::structure::{FixedState, Unit};
use freerouting::board::transaction::{CheckPolicy, TransactionResult};
use freerouting::geometry::IntPoint;
use freerouting::geometry::jmath::round_to_i32;

use super::components::PlaneComponents;
use super::verify::layer_name;

/// The registry id of [`RegionRepair`].
pub const REGION_REPAIR_ID: &str = "krt-region-repair";

/// The strap widths tried before the net class width, in mm, widest first.
pub const WIDTH_LADDER_MM: [f64; 3] = [2.0, 1.0, 0.5];

/// The site name of a strap search's work limit.
const STRAP_SITE: &str = "krt_region_repair";

/// An edge between two regions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MstEdge {
    /// The lower region index.
    pub a: usize,
    /// The higher region index.
    pub b: usize,
    /// The smallest squared anchor-to-anchor distance between the two.
    pub weight: i64,
}

/// The edges between every two of `regions` (each a list of anchor
/// centres), in Kruskal order: weight, then `(a, b)`. A region without
/// points has no edges.
pub fn kruskal_edges(regions: &[Vec<IntPoint>]) -> Vec<MstEdge> {
    let mut edges = Vec::new();
    for a in 0..regions.len() {
        for b in a + 1..regions.len() {
            let weight = regions[a]
                .iter()
                .flat_map(|p| regions[b].iter().map(move |q| distance_square(*p, *q)))
                .min();
            if let Some(weight) = weight {
                edges.push(MstEdge { a, b, weight });
            }
        }
    }
    edges.sort_by_key(|e| (e.weight, e.a, e.b));
    edges
}

/// The squared distance of `p` and `q`, exact for coordinates within
/// `±CRIT_INT` (2^25): each difference is at most 2^26, the sum of squares
/// at most 2^53, far inside `i64`.
pub fn distance_square(p: IntPoint, q: IntPoint) -> i64 {
    let dx = i64::from(p.x) - i64::from(q.x);
    let dy = i64::from(p.y) - i64::from(q.y);
    dx * dx + dy * dy
}

/// Kruskal over `n` regions with `edges` in order: each edge whose regions
/// are not yet joined is tried with `try_edge`, and joins them when that
/// returns true. Returns every tried edge with its outcome, in order. With a
/// `try_edge` that always succeeds the successes are the minimum spanning
/// tree.
pub fn kruskal_with(
    n: usize,
    edges: &[MstEdge],
    mut try_edge: impl FnMut(&MstEdge) -> bool,
) -> Vec<(MstEdge, bool)> {
    let mut parent: Vec<usize> = (0..n).collect();
    let find = |parent: &[usize], mut x: usize| {
        while parent[x] != x {
            x = parent[x];
        }
        x
    };
    let mut tried = Vec::new();
    for edge in edges {
        let (ra, rb) = (find(&parent, edge.a), find(&parent, edge.b));
        if ra == rb {
            continue;
        }
        let ok = try_edge(edge);
        if ok {
            parent[ra.max(rb)] = ra.min(rb);
        }
        tried.push((*edge, ok));
    }
    tried
}

/// One strap the stage tried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrapAttempt {
    /// The plane net.
    pub net: i32,
    /// The layer.
    pub layer: i32,
    /// The lowest anchor of each of the two regions.
    pub from: ItemId,
    pub to: ItemId,
    /// The width kept, in board units; `None` if every width failed.
    pub width: Option<i32>,
    /// One line per failed width: the width and why.
    pub failures: Vec<String>,
}

/// What the stage did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepairReport {
    /// The net layers that had more than one region: (net, layer, regions).
    pub split: Vec<(i32, i32, usize)>,
    /// The straps tried, in order.
    pub straps: Vec<StrapAttempt>,
}

impl RepairReport {
    /// The log lines of the report.
    pub fn log(&self, board: &RoutingBoard) -> Vec<String> {
        let mut log: Vec<String> = self
            .split
            .iter()
            .map(|(net, layer, n)| {
                format!(
                    "{REGION_REPAIR_ID}: net {} on {}: {n} regions",
                    net_name(board, *net),
                    layer_name(board, *layer)
                )
            })
            .collect();
        for s in &self.straps {
            let what = match s.width {
                Some(w) => format!("strap at width {w}"),
                None => format!("no strap ({})", s.failures.join("; ")),
            };
            log.push(format!(
                "{REGION_REPAIR_ID}: net {} on {}: #{} to #{}: {what}",
                net_name(board, s.net),
                layer_name(board, s.layer),
                s.from.0,
                s.to.0
            ));
        }
        log
    }
}

/// Repairs the split pour regions of `board` (module docs).
pub fn repair_regions(board: &mut RoutingBoard, ctx: &StageCtx) -> RepairReport {
    let mut report = RepairReport::default();
    let mut pc = PlaneComponents::build(board);
    for net in pc.nets() {
        for layer in pc.layers_of(net) {
            if ctx.cancel.is_stop_requested() {
                return report;
            }
            let regions = pc.regions(net, layer);
            if regions.len() < 2 {
                continue;
            }
            report.split.push((net, layer, regions.len()));
            let points: Vec<Vec<IntPoint>> = regions
                .iter()
                .map(|r| {
                    r.anchors
                        .iter()
                        .filter_map(|a| pc.anchor_centre(*a))
                        .collect()
                })
                .collect();
            let edges = kruskal_edges(&points);
            let widths = strap_half_widths(board, net, layer);
            let mut changed = false;
            kruskal_with(regions.len(), &edges, |edge| {
                if ctx.cancel.is_stop_requested() {
                    return false;
                }
                let (from, to) = (&regions[edge.a].anchors, &regions[edge.b].anchors);
                let attempt = strap(board, ctx, net, layer, from, to, &widths);
                let ok = attempt.width.is_some();
                changed |= ok;
                report.straps.push(attempt);
                ok
            });
            if changed {
                pc = PlaneComponents::build(board);
            }
        }
    }
    report
}

/// The strap half widths of net `net` on `layer`, in ladder order (module
/// docs).
pub fn strap_half_widths(board: &RoutingBoard, net: i32, layer: i32) -> Vec<i32> {
    let class = board.rules.trace_half_width(net, layer);
    let units_per_mm = board.communication.get_resolution(Unit::Mm);
    let mut widths: Vec<i32> = WIDTH_LADDER_MM
        .iter()
        .map(|mm| round_to_i32(mm * units_per_mm / 2.0))
        .filter(|hw| *hw > class)
        .collect();
    widths.push(class);
    widths
}

/// Tries one strap at each of `half_widths` in turn.
fn strap(
    board: &mut RoutingBoard,
    ctx: &StageCtx,
    net: i32,
    layer: i32,
    from: &[ItemId],
    to: &[ItemId],
    half_widths: &[i32],
) -> StrapAttempt {
    let mut attempt = StrapAttempt {
        net,
        layer,
        from: from[0],
        to: to[0],
        width: None,
        failures: Vec::new(),
    };
    let start: BTreeSet<ItemId> = from.iter().copied().collect();
    let dest: BTreeSet<ItemId> = to.iter().copied().collect();
    for &hw in half_widths {
        if ctx.cancel.is_stop_requested() {
            break;
        }
        let Some(control) = strap_control(board, ctx, net, hw, &dest) else {
            attempt
                .failures
                .push(format!("width {}: no routing layer", 2 * hw));
            break;
        };
        let result = board.try_change(
            |b| route_strap(b, ctx, &start, &dest, &control),
            CheckPolicy::NewItemsOnly,
        );
        match result {
            TransactionResult::Committed(()) => {
                attempt.width = Some(2 * hw);
                break;
            }
            TransactionResult::Reverted { violations, .. } => attempt.failures.push(format!(
                "width {}: {} clearance violations",
                2 * hw,
                violations.len()
            )),
            TransactionResult::Aborted(reason) => {
                attempt.failures.push(format!("width {}: {reason}", 2 * hw))
            }
        }
    }
    attempt
}

/// The control of a strap of net `net` at half width `hw` to the items
/// `dest`: the net's control (its routing layers) with no vias, no ripup,
/// no neck-down, and only `dest` as destinations; `None` if the net has no
/// routing layer.
fn strap_control(
    board: &RoutingBoard,
    ctx: &StageCtx,
    net: i32,
    hw: i32,
    dest: &BTreeSet<ItemId>,
) -> Option<AutorouteControl> {
    let mut control = AutorouteControl::for_net_default(board, net, ctx.board_settings).ok()?;
    if !control.layer_active.iter().any(|a| *a) {
        return None;
    }
    let class = control.trace_clearance_class_index;
    for l in 0..control.layer_count {
        control.trace_half_width[l] = hw;
        control.compensated_trace_half_width[l] = hw
            + board
                .rules
                .clearance_matrix
                .clearance_compensation_value(class, l as i32);
    }
    control.vias_allowed = false;
    control.ripup_allowed = false;
    control.with_neckdown = false;
    // The net's own plane (and every other item of the net) is not a
    // target: the strap must reach the other region's anchors.
    control.destinations_only = Some(dest.clone());
    Some(control)
}

/// Routes one strap; `Err` with the router's details unless it routed and
/// added copper.
///
/// Freerouting's connectivity treats the pour as one conductor, so a strap
/// between two of its islands is a cycle to it, and the insertion's
/// normalization would remove it at once (`remove_if_cycle`). While the
/// strap is routed, the net class ignores cycles through areas (Java's
/// `ignoreCyclesWithAreas`, restored right after), and the strap's traces
/// are then user fixed, so no later normalization or optimizer pass removes
/// or moves them; the SES marks them `protect`.
fn route_strap(
    board: &mut RoutingBoard,
    ctx: &StageCtx,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    control: &AutorouteControl,
) -> Result<(), String> {
    let net = control.net_number;
    let class = board
        .rules
        .nets
        .get(net)
        .map(|n| n.net_class())
        .ok_or_else(|| format!("no net {net}"))?;
    let last_id = board.items().map(|i| i.id().0).max().unwrap_or(0);
    let limits = RouteLimits {
        settings: SearchSettings::from_router(ctx.board_settings),
        stop: Some(ctx.cancel),
        work_limit: Some(WorkLimit::with_site(
            board.search_trees().work_meter(),
            maze_budget_ms(1),
            STRAP_SITE,
        )),
    };
    let mut ripped = BTreeSet::new();
    let ripped = Ripped {
        items: &mut ripped,
        costs: None,
    };
    let ignored_before = board
        .rules
        .net_classes
        .get(class)
        .ignore_cycles_with_areas();
    board
        .rules
        .net_classes
        .get_mut(class)
        .set_ignore_cycles_with_areas(true);
    let (result, _) = route_connection(board, start, dest, control, limits, ripped, NoopSink);
    board
        .rules
        .net_classes
        .get_mut(class)
        .set_ignore_cycles_with_areas(ignored_before);
    if result.state != AttemptState::Routed {
        return Err(format!("{:?} {}", result.state, result.details)
            .trim()
            .to_string());
    }
    let strap: Vec<ItemId> = board
        .items()
        .filter(|i| i.id().0 > last_id && i.is_trace())
        .map(|i| i.id())
        .collect();
    if strap.is_empty() {
        return Err("routed without new traces".to_string());
    }
    for id in strap {
        board.set_fixed_state(id, FixedState::UserFixed);
    }
    Ok(())
}

fn net_name(board: &RoutingBoard, net: i32) -> String {
    board
        .rules
        .nets
        .get(net)
        .map_or_else(|| format!("net {net}"), |n| n.name.clone())
}

/// Re-joins split pour regions after the routing passes (module docs).
#[derive(Clone, Copy, Debug, Default)]
pub struct RegionRepair;

impl BoardStage for RegionRepair {
    fn id(&self) -> &'static str {
        REGION_REPAIR_ID
    }

    fn phase(&self) -> StagePhase {
        StagePhase::AfterRouting
    }

    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> StageReport {
        let report = repair_regions(board, ctx);
        StageReport {
            log: report.log(board),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting::geometry::CRIT_INT;

    fn p(x: i32, y: i32) -> IntPoint {
        IntPoint::new(x, y)
    }

    /// Four islands at the corners of a 4 x 3 rectangle, the last one with
    /// a second anchor closer to the third.
    fn four() -> Vec<Vec<IntPoint>> {
        vec![
            vec![p(0, 0)],
            vec![p(400, 0)],
            vec![p(0, 300)],
            vec![p(400, 300), p(100, 300)],
        ]
    }

    #[test]
    fn the_edges_are_in_kruskal_order_with_the_closest_anchor_pair() {
        let edges = kruskal_edges(&four());
        let keys: Vec<(usize, usize, i64)> = edges.iter().map(|e| (e.a, e.b, e.weight)).collect();
        // (0, 3) uses the second anchor of island 3; (0, 2) and (1, 3) tie
        // and go by index pair.
        assert_eq!(
            keys,
            vec![
                (2, 3, 10_000),
                (0, 2, 90_000),
                (1, 3, 90_000),
                (0, 3, 100_000),
                (0, 1, 160_000),
                (1, 2, 250_000),
            ]
        );
    }

    #[test]
    fn kruskal_takes_the_minimum_spanning_tree_in_order() {
        let edges = kruskal_edges(&four());
        let tried = kruskal_with(4, &edges, |_| true);
        let taken: Vec<(usize, usize)> = tried.iter().map(|(e, _)| (e.a, e.b)).collect();
        // (2,3) 10000, then the tie at 90000 by index pair: (0,2), (1,3).
        assert_eq!(taken, vec![(2, 3), (0, 2), (1, 3)]);
        assert!(tried.iter().all(|(_, ok)| *ok));
    }

    #[test]
    fn a_failed_edge_is_not_retried_and_the_next_one_joins_its_regions() {
        let edges = kruskal_edges(&four());
        let tried = kruskal_with(4, &edges, |e| (e.a, e.b) != (0, 2));
        let taken: Vec<(usize, usize, bool)> =
            tried.iter().map(|(e, ok)| (e.a, e.b, *ok)).collect();
        assert_eq!(
            taken,
            vec![(2, 3, true), (0, 2, false), (1, 3, true), (0, 3, true)]
        );
    }

    #[test]
    fn the_squared_distance_is_exact_at_the_coordinate_limits() {
        let (lo, hi) = (p(-CRIT_INT, -CRIT_INT), p(CRIT_INT, CRIT_INT));
        let d = 2 * i64::from(CRIT_INT);
        assert_eq!(distance_square(lo, hi), 2 * d * d);
        assert!(distance_square(lo, hi) <= 1 << 53);
        let edges = kruskal_edges(&[vec![lo], vec![hi], vec![p(0, 0)]]);
        assert_eq!(
            edges[0].weight,
            2 * i64::from(CRIT_INT) * i64::from(CRIT_INT)
        );
    }
}

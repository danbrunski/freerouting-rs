//! The fanout of one SMD pin (A12.1).
//!
//! Port of `RoutingBoard.fanout(Pin, RouterSettings, ripupCosts,
//! stoppableThread, workLimit)` (`RoutingBoard.java:991-1145`) at
//! `a773a70ec`: a short escape from a single-layer pin to the first via (or
//! straight to the net's plane), for [`batch::fanout`](super::batch::fanout).
//!
//! **Orders that decide results**, in Java's order:
//! - the guards: a pin spanning layers or with other than one net is
//!   `AlreadyConnected`; a pin with an ignored net `Skipped`; a pin whose
//!   connected set holds an item off the pin's layer `AlreadyConnected`; a
//!   pin without unconnected items `NoUnconnectedNets`;
//! - the targets: the unconnected set (a `TreeSet<Item>`, descending id)
//!   copied to a list and **stably** sorted by the squared distance from the
//!   pin centre to each item's box centre, computed as
//!   `(ll.x + ur.x) / 2.0` (an `int` sum that wraps, then a `double`
//!   divide) and compared as `Double.compare` ([`f64::total_cmp`]);
//! - the control: the three-argument constructor (the via costs and trace
//!   costs of the settings), `is_fanout`, the merged via rule of
//!   `fanout.fallback_to_board_vias` (the class's vias, then each via of the
//!   board's first via rule it lacks, by identity) with a second
//!   `rebuild_via_info`, the start pin's centre and layer,
//!   `remove_unconnected_vias = false`, ripup with the given cost when it is
//!   not negative;
//! - the attempts, all on one ripped set and one work limit (the caller's,
//!   created before the call): the net's planes first, priced with the plane
//!   via costs and `is_plane_fanout`, then the normal via costs again; a
//!   plane attempt counts when it is `Routed` or `AlreadyConnected`. Then
//!   with at most 4 targets the closest target alone and, if that failed
//!   and there are more, all targets; with more than 4 all targets at once;
//! - after `Routed`, `opt_changed_area` on the pin's net with the pull-tight
//!   accuracy, the control's trace costs and a limit of 1,000 nominal ms.
//!
//! `// PARITY(quirk):` Java initialises one `AutorouteEngine` and calls
//! `autorouteConnection` up to three times on it, sharing one ripped set
//! (`:1074-1126`). Each call clears its rooms and resets the room counter,
//! so a fresh [`Session`](super::session::Session) per call matches it, but
//! the ripped set accumulates: a later attempt rips what an earlier failed
//! one collected.
//!
//! Where Java's `AutorouteControl` constructor or `rebuildViaInfo` throws
//! (no via rule at all, a class without one) the exception would leave
//! `BatchFanout`; the port returns `Failed` with the error text. The
//! debug-only `fanoutStartPinName` is not carried, and the pull-tight runs
//! without the cancel token, as in A11's connection router.

use std::collections::BTreeSet;

use freerouting_geometry::{FloatPoint, IntBox};

use super::connection::{Ripped, RouteLimits, java_item_text, route_connection};
use super::control::AutorouteControl;
use super::session::search::SearchSettings;
use super::session::sink::NoopSink;
use super::stages::StageCtx;
use super::{AttemptResult, AttemptState};
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::search::WorkLimit;
use crate::optimize::PullTightParams;

/// Targets up to this many are tried closest first (`RoutingBoard.java:1100`).
pub const CLOSEST_FIRST_TARGET_LIMIT: usize = 4;

/// Java's lambda comparator key (`:1020-1037`): the squared distance from
/// `pin_center` to the centre of `bounds`.
pub fn box_distance_square(pin_center: FloatPoint, bounds: &IntBox) -> f64 {
    // PARITY: `(box.ll.x + box.ur.x) / 2.0`, an int sum, then a double divide.
    let cx = f64::from(bounds.ll.x.wrapping_add(bounds.ur.x)) / 2.0;
    let cy = f64::from(bounds.ll.y.wrapping_add(bounds.ur.y)) / 2.0;
    let dx = cx - pin_center.x;
    let dy = cy - pin_center.y;
    dx * dx + dy * dy
}

/// Sorts `targets` (in `TreeSet<Item>` order) stably by
/// [`box_distance_square`] from `pin_center` (`List.sort`, a stable merge
/// sort, with `Double.compare`).
pub fn sort_by_box_distance(pin_center: FloatPoint, targets: &mut [(ItemId, IntBox)]) {
    targets.sort_by(|a, b| {
        let da = box_distance_square(pin_center, &a.1);
        let db = box_distance_square(pin_center, &b.1);
        da.total_cmp(&db)
    });
}

/// The unconnected items of a fanned-out pin, closest first.
fn sorted_targets(
    board: &RoutingBoard,
    pin_center: FloatPoint,
    unconnected: &BTreeSet<ItemId>,
) -> Vec<ItemId> {
    let mut targets: Vec<(ItemId, IntBox)> = unconnected
        .iter()
        .filter_map(|id| board.item(*id).map(|item| (*id, item.bounding_box())))
        .collect();
    sort_by_box_distance(pin_center, &mut targets);
    targets.into_iter().map(|(id, _)| id).collect()
}

/// The fanout's control of `net` (`:1039-1072`).
fn fanout_control(
    board: &RoutingBoard,
    net: i32,
    pin_center: crate::geometry::Point,
    pin_layer: i32,
    ripup_cost: i32,
    ctx: &StageCtx,
) -> Result<AutorouteControl, String> {
    let settings = ctx.board_settings;
    let mut control =
        AutorouteControl::for_net_default(board, net, settings).map_err(|e| e.to_string())?;
    control.is_fanout = true;
    if settings.fallback_to_board_vias() {
        // PARITY: :1042-1058, the class's vias, then the first board rule's
        // that it lacks (`ViaRule.contains` compares identities).
        let mut combined = control.via_rule_vias(&board.rules).to_vec();
        if let Some(first) = board.rules.via_rules.nth(0) {
            for via in board.rules.via_rules.get(first).vias() {
                if !combined.contains(via) {
                    combined.push(*via);
                }
            }
        }
        control.merged_via_rule = Some(combined);
        control
            .rebuild_via_info(board, settings.via_costs(), net)
            .map_err(|e| e.to_string())?;
    }
    control.fanout_start_pin_center = Some(pin_center);
    control.fanout_start_pin_layer = pin_layer;
    control.remove_unconnected_vias = false;
    if ripup_cost >= 0 {
        control.ripup_allowed = true;
        control.ripup_costs = ripup_cost;
    }
    Ok(control)
}

/// One `autorouteConnection` of the pin's engine, adding to `ripped`.
fn attempt(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    control: &AutorouteControl,
    limit: &WorkLimit,
    ctx: &StageCtx,
    ripped: &mut BTreeSet<ItemId>,
) -> AttemptResult {
    let limits = RouteLimits {
        settings: SearchSettings::from_router(ctx.board_settings),
        stop: Some(ctx.cancel),
        work_limit: Some(limit.clone()),
    };
    // PARITY: :1083-1126, `ripupCosts` is null here.
    let ripped = Ripped {
        items: ripped,
        costs: None,
    };
    route_connection(board, start, dest, control, limits, ripped, NoopSink).0
}

/// True if a plane or closest-target attempt ended the fanout.
fn connected(result: &AttemptResult) -> bool {
    matches!(
        result.state,
        AttemptState::Routed | AttemptState::AlreadyConnected
    )
}

/// Java `RoutingBoard.fanout`: routes from the single-layer pin `pin` to
/// its first via, ripping up with `ripup_cost` when it is not negative,
/// within `limit` (the per-pin [`WorkLimit`] the caller created on this
/// board's meter).
pub fn fanout_pin(
    board: &mut RoutingBoard,
    pin: ItemId,
    ripup_cost: i32,
    limit: &WorkLimit,
    ctx: &StageCtx,
) -> AttemptResult {
    let Some(item) = board.item(pin) else {
        return AttemptResult::with_details(AttemptState::Failed, "The pin is not on the board.");
    };
    let text = java_item_text(item);
    let already = || {
        AttemptResult::with_details(
            AttemptState::AlreadyConnected,
            format!("The pin '{text}' is already connected."),
        )
    };
    if item.first_layer() != item.last_layer() || item.net_count() != 1 {
        return already();
    }
    if board.has_ignored_nets(item) {
        return AttemptResult::with_details(
            AttemptState::Skipped,
            format!("The pin '{text}' belongs to an ignored net class."),
        );
    }
    let net = item.net_number(0);
    let layer = item.first_layer();
    let Some(center) = item.drill().map(|drill| drill.center) else {
        return AttemptResult::with_details(AttemptState::Failed, "The pin has no centre.");
    };
    let start = board.connected_set(pin, net, false);
    let off_layer = start
        .iter()
        .filter_map(|id| board.item(*id))
        .any(|other| other.first_layer() != layer || other.last_layer() != layer);
    if off_layer {
        return already();
    }
    let unconnected = board.unconnected_set(pin, net);
    if unconnected.is_empty() {
        return AttemptResult::with_details(
            AttemptState::NoUnconnectedNets,
            format!("The pin '{text}' is already connected."),
        );
    }
    let sorted = sorted_targets(board, center.to_float(), &unconnected);
    let mut control = match fanout_control(board, net, center.into(), layer, ripup_cost, ctx) {
        Ok(control) => control,
        Err(error) => return AttemptResult::with_details(AttemptState::Failed, error),
    };
    let mut ripped = BTreeSet::new();
    let result = match route_attempts(
        board,
        &mut control,
        &start,
        &unconnected,
        &sorted,
        limit,
        ctx,
        &mut ripped,
    ) {
        Ok(result) => result,
        Err(error) => return AttemptResult::with_details(AttemptState::Failed, error),
    };
    if result.state == AttemptState::Routed {
        // PARITY: :1129-1137, the pin's net only, a 1,000 ms guard.
        let params = PullTightParams {
            only_nets: vec![net],
            ..PullTightParams::after_connection(
                ctx.board_settings.trace_pull_tight_accuracy(),
                control.trace_costs.clone(),
            )
        };
        board.opt_changed_area(&params);
    }
    result
}

/// The plane attempt and the target attempts (`:1074-1126`).
#[allow(clippy::too_many_arguments)]
fn route_attempts(
    board: &mut RoutingBoard,
    control: &mut AutorouteControl,
    start: &BTreeSet<ItemId>,
    unconnected: &BTreeSet<ItemId>,
    sorted: &[ItemId],
    limit: &WorkLimit,
    ctx: &StageCtx,
    ripped: &mut BTreeSet<ItemId>,
) -> Result<AttemptResult, String> {
    let settings = ctx.board_settings;
    let net = control.net_number;
    let plane_targets = board.power_plane_areas(unconnected.iter().copied());
    if !plane_targets.is_empty() {
        control
            .rebuild_via_info(board, settings.plane_via_costs(), net)
            .map_err(|e| e.to_string())?;
        control.is_plane_fanout = true;
        let plane = attempt(board, start, &plane_targets, control, limit, ctx, ripped);
        control.is_plane_fanout = false;
        control
            .rebuild_via_info(board, settings.via_costs(), net)
            .map_err(|e| e.to_string())?;
        if connected(&plane) {
            return Ok(plane);
        }
    }
    let Some(closest) = sorted.first() else {
        return Ok(AttemptResult::with_details(
            AttemptState::Failed,
            "No target items to route connection.",
        ));
    };
    if sorted.len() > CLOSEST_FIRST_TARGET_LIMIT {
        return Ok(attempt(
            board,
            start,
            unconnected,
            control,
            limit,
            ctx,
            ripped,
        ));
    }
    let closest = BTreeSet::from([*closest]);
    let result = attempt(board, start, &closest, control, limit, ctx, ripped);
    if !connected(&result) && sorted.len() > 1 {
        return Ok(attempt(
            board,
            start,
            unconnected,
            control,
            limit,
            ctx,
            ripped,
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting_geometry::IntPoint;
    use std::cmp::Ordering;

    fn ids(targets: &[(ItemId, IntBox)]) -> Vec<i32> {
        targets.iter().map(|(id, _)| id.0).collect()
    }

    fn bounds(x0: i32, y0: i32, x1: i32, y1: i32) -> IntBox {
        IntBox {
            ll: IntPoint::new(x0, y0),
            ur: IntPoint::new(x1, y1),
        }
    }

    #[test]
    fn the_target_sort_is_stable_for_equal_distances() {
        let center = FloatPoint::new(0.0, 0.0);
        // Ids in TreeSet order (descending); 9 and 4 are both 10 away
        // (box centres (10, 0) and (0, -10)), 7 is nearer, 2 farther.
        let mut targets = vec![
            (ItemId(9), bounds(5, -5, 15, 5)),
            (ItemId(7), bounds(-1, -1, 3, 1)),
            (ItemId(4), bounds(-5, -15, 5, -5)),
            (ItemId(2), bounds(100, 100, 102, 102)),
        ];
        sort_by_box_distance(center, &mut targets);
        assert_eq!(ids(&targets), vec![7, 9, 4, 2]);
        // The other input order of the tie keeps its own order.
        targets.swap(1, 2);
        sort_by_box_distance(center, &mut targets);
        assert_eq!(ids(&targets), vec![7, 4, 9, 2]);
    }

    #[test]
    fn the_box_centre_sum_wraps_as_a_java_int() {
        let wide = bounds(i32::MAX, 0, 1, 0);
        // i32::MAX + 1 wraps to i32::MIN before the divide.
        let d = box_distance_square(FloatPoint::new(0.0, 0.0), &wide);
        let cx = f64::from(i32::MIN) / 2.0;
        assert_eq!(d.to_bits(), (cx * cx).to_bits());
    }

    #[test]
    fn negative_zero_sorts_before_zero_as_double_compare() {
        // Double.compare and total_cmp agree on the signed zeros.
        assert_eq!((-0.0f64).total_cmp(&0.0), Ordering::Less);
    }
}

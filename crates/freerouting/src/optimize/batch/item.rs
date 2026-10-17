//! Rerouting one optimizer candidate (O06.3).
//!
//! Port of `BatchOptimizer.optRouteItemOnBoard` (`BatchOptimizer.java:862-952`
//! at `a773a70ec`). The steps, in Java's order:
//!
//! 1. the cheap statistics before (no clearance check; Java's
//!    `new BoardStatistics(board, null, false)`, lengths in mm);
//! 2. the baseline length: the pass's, or the board's weighted length when
//!    the pass gave none;
//! 3. an item with an ignored net is unimproved;
//! 4. the incomplete count from the routing ledger;
//! 5. the rip set: the item, plus its start and its end contacts when they
//!    are only unfixed traces, each expanded to its connection items (stop
//!    option NONE), in id order (Java's `TreeSet<Item>`, highest id first);
//!    a user-fixed item or an ignored net in it makes the result unimproved
//!    before anything changes;
//! 6. remove the rip set, combine the traces of each net of the item;
//! 7. the ripup cost: `start_ripup_costs`, times
//!    `additional_ripup_cost_factor_at_start` (`int` arithmetic) while the
//!    increased costs are on, then for a trace
//!    `(int) Math.round(trace_ripup_cost_factor × cost)` in `double`;
//! 8. A11.6's [`route_passes_for_optimizer`] with `max_autoroute_passes`
//!    and the pull-tight accuracy;
//! 9. the cheap statistics and the incomplete count after, and the
//!    [`ItemRouteResult`]. It is improved only when no full stop was
//!    requested and the metrics improved.
//!
//! Java also forces "not improved" once the optimizer's wall-clock deadline
//! has passed. The port does not read a clock here: the deadline is a stop
//! request on the job's token (`StopLevel::All`), which this step sees as
//! the stop above (DIVERGENCES, "Time").
//!
//! The work of every search counts on the candidate board's own meter
//! (B02.7), which a restore starts at zero.

use std::collections::BTreeSet;

use freerouting_geometry::jmath::round_to_i32;

use crate::autoroute::batch::pass::route_passes_for_optimizer;
use crate::autoroute::item_route_result::ItemRouteResult;
use crate::autoroute::stages::StageCtx;
use crate::board::basic_board::BasicBoard;
use crate::board::items::{ItemId, StopConnectionOption};
use crate::board::routing_board::RoutingBoard;
use crate::board::structure::Unit;
use crate::scoring::BoardStatistics;
use crate::settings::RouterSettings;

/// The candidate's switches that change per pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemPassParams {
    /// The pass's weighted trace length (Java `minCumulativeTraceLength`);
    /// 0 or less means "use the board's".
    pub baseline_length: f64,
    /// Odd passes use the preferred directions.
    pub with_preferred_directions: bool,
    /// The first passes use the increased ripup costs.
    pub use_increased_ripup_costs: bool,
}

/// The via count and the weighted trace length of `board` (the cheap
/// statistics: no clearance check, no connection count).
fn cheap_figures(board: &BasicBoard) -> (i32, f64) {
    let stats = BoardStatistics::with_options(board, Unit::Mm, false, false);
    (
        i32::try_from(stats.items.via_count).unwrap_or(i32::MAX),
        f64::from(stats.traces.total_weighted_length),
    )
}

fn incomplete_count(board: &BasicBoard) -> i32 {
    i32::try_from(board.routing_ledger().incomplete_count(board)).unwrap_or(i32::MAX)
}

/// Java `containsOnlyUnfixedTraces` (true for an empty set).
fn only_unfixed_traces(board: &BasicBoard, ids: &BTreeSet<ItemId>) -> bool {
    ids.iter().all(|id| {
        board
            .item(*id)
            .is_some_and(|item| item.is_trace() && !item.is_user_fixed())
    })
}

/// The ripup cost of a candidate (step 7).
pub fn candidate_ripup_costs(settings: &RouterSettings, is_trace: bool, increased: bool) -> i32 {
    let mut costs = settings.scoring.start_ripup_costs;
    if increased {
        costs = costs.wrapping_mul(settings.optimizer.additional_ripup_cost_factor_at_start);
    }
    if is_trace {
        costs =
            round_to_i32(f64::from(settings.optimizer.trace_ripup_cost_factor) * f64::from(costs));
    }
    costs
}

/// Java `optRouteItemOnBoard`: rips up `item` with its connections and
/// reroutes them on `board` (see the module docs). `ctx` carries the
/// settings, the cancel token and the event sink of the inner autorouter.
pub fn opt_route_item(
    board: &mut RoutingBoard,
    item: ItemId,
    params: &ItemPassParams,
    settings: &RouterSettings,
    ctx: &mut StageCtx,
) -> ItemRouteResult {
    let (vias_before, length_before) = cheap_figures(board);
    let baseline = if params.baseline_length > 0.0 {
        params.baseline_length
    } else {
        length_before
    };
    let Some(the_item) = board.item(item) else {
        return ItemRouteResult::unimproved(item);
    };
    if board.has_ignored_nets(the_item) {
        return ItemRouteResult::unimproved(item);
    }
    let is_trace = the_item.is_trace();
    let nets: Vec<i32> = the_item.net_numbers().to_vec();
    let incomplete_before = incomplete_count(board);

    let mut ripped: BTreeSet<ItemId> = BTreeSet::from([item]);
    if is_trace {
        for contacts in [
            board.trace_start_contacts(item),
            board.trace_end_contacts(item),
        ] {
            if only_unfixed_traces(board, &contacts) {
                ripped.extend(contacts);
            }
        }
    }
    let mut connections: BTreeSet<ItemId> = BTreeSet::new();
    for id in &ripped {
        connections.extend(board.connection_items(*id, StopConnectionOption::None));
    }
    let protected = connections.iter().any(|id| {
        board
            .item(*id)
            .is_some_and(|i| i.is_user_fixed() || board.has_ignored_nets(i))
    });
    if protected {
        return ItemRouteResult::unimproved(item);
    }

    board.remove_items(connections.iter().copied());
    for net in nets {
        board.combine_traces(net);
    }

    let ripup_costs = candidate_ripup_costs(settings, is_trace, params.use_increased_ripup_costs);
    let max_passes = u32::try_from(settings.optimizer.max_autoroute_passes).unwrap_or(0);
    route_passes_for_optimizer(
        board,
        max_passes,
        ripup_costs,
        settings.trace_pull_tight_accuracy,
        params.with_preferred_directions,
        settings,
        ctx,
    );

    let (vias_after, length_after) = cheap_figures(board);
    let incomplete_after = incomplete_count(board);
    let mut result = ItemRouteResult::new(
        item,
        vias_before,
        vias_after,
        baseline,
        length_after,
        incomplete_before,
        incomplete_after,
    );
    // PARITY: :946-950, a full stop makes the result unimproved (the
    // deadline is a stop request here, see the module docs).
    let improved = !ctx.cancel.is_stop_requested() && result.improved();
    result.update_improved(improved);
    result
}

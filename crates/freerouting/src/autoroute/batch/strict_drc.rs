//! Strict DRC after a routed connection (A11.3).
//!
//! Port of `BatchAutorouter.enforceStrictDrc` (`BatchAutorouter.java:313-337`)
//! and `AutorouteConnectionRouter.applyStrictDrcAfterRoute` (`:274-294`) at
//! `a773a70ec`. A connection is checked when the pass's strict check holds
//! (`strict_drc`, or pass 3 and later: [`RipupPolicy::strict_check`]).
//!
//! The rollback has Java's two forms, and both are kept:
//! - always: the new traces and vias of the routed net (id above the
//!   watermark taken before the route) are removed with one
//!   `remove_items` when any of them has a clearance violation;
//! - with `strict_drc` on, the board is then replaced by the
//!   [`BoardSnapshot`] taken before the route (Java's `serialize(false)` and
//!   `deserialize`), which also brings back what the route ripped. The
//!   replacement is announced to the listeners with
//!   [`RouterEvents::replaced`]. The restored board has a fresh work meter;
//!   no limit of the connection is checked after this point.
//!
//! The scan asks for the clearance violations of each new item in
//! `connectable_items` order only until the first item with one, as Java's
//! `!hasViolation &&` does: the queries count work (B02.7).
//!
//! [`RipupPolicy::strict_check`]: crate::autoroute::stages::RipupPolicy::strict_check
//! [`RouterEvents::replaced`]: crate::autoroute::events::RouterEvents::replaced

use crate::autoroute::events::RouterEvents;
use crate::autoroute::{AttemptResult, AttemptState};
use crate::board::items::ItemId;
use crate::board::{BoardSnapshot, RoutingBoard};

/// Java `enforceStrictDrc(board, routeNetNo, maxItemIdBefore)`: if a trace
/// or via of `net` with an id above `max_id_before` has a clearance
/// violation, removes all of those items with one `remove_items` and
/// returns `Failed`; `None` when the new items are clean.
pub fn enforce_strict_drc(
    board: &mut RoutingBoard,
    net: i32,
    max_id_before: i32,
) -> Option<AttemptResult> {
    let mut new_items: Vec<ItemId> = Vec::new();
    let mut has_violation = false;
    // PARITY: BatchAutorouter.java:316-326, getConnectableItems order.
    for id in board.connectable_items(net) {
        if id.0 <= max_id_before {
            continue;
        }
        let Some(item) = board.item(id) else {
            continue;
        };
        if !(item.is_trace() || item.is_via()) {
            continue;
        }
        new_items.push(id);
        if !has_violation && !board.clearance_violations(id).is_empty() {
            has_violation = true;
        }
    }
    if !has_violation {
        return None;
    }
    let count = new_items.len();
    board.remove_items(new_items);
    Some(AttemptResult::with_details(
        AttemptState::Failed,
        format!(
            "strict_drc: connection ripped because {count} new item(s) included clearance \
             violations"
        ),
    ))
}

/// Java `applyStrictDrcAfterRoute`: when `strict_check` holds, runs
/// [`enforce_strict_drc`]; on a rejection with a `snapshot` (taken when
/// `strict_drc` is on) the board is replaced by it and the replacement is
/// sent to `events`. Returns the rejection.
pub fn apply_strict_drc_after_route(
    board: &mut RoutingBoard,
    net: i32,
    max_id_before: i32,
    snapshot: Option<BoardSnapshot>,
    strict_check: bool,
    events: &dyn RouterEvents,
) -> Option<AttemptResult> {
    if !strict_check {
        return None;
    }
    let rejection = enforce_strict_drc(board, net, max_id_before)?;
    if let Some(snapshot) = snapshot {
        // PARITY: AutorouteConnectionRouter.java:289-291, the whole board.
        *board = snapshot.restore();
        if events.is_listening() {
            events.replaced(snapshot);
        }
    }
    Some(rejection)
}

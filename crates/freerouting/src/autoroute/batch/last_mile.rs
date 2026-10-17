//! The last-mile blocker ripup (A13.3): `LastMileBlockerRipup.ripBlockers`
//! (`LastMileBlockerRipup.java:28-63` at `a773a70ec`).
//!
//! When only a few connections are left and the score stopped improving,
//! the batch loop removes up to [`MAX_ITEMS`] unfixed foreign traces and
//! vias that lie in the corridors of the remaining airlines, so the next
//! passes can route through them.
//!
//! **Order, in Java's.** The airlines come from a fresh DRC
//! ([`DesignRulesChecker::all_airlines`]), net by net and within a net in
//! input order (Java's `NetIncompletes` keeps input order since
//! `30baad7e2`; the Rust DRC always did). For each airline,
//! [`corridor`] (A10.7) and the default tree's overlapping objects on all
//! layers in `TreeObject` order (`TreeSet<SearchTreeObject>`: highest item
//! id first); [`is_removable_blocker`] items go into an insertion-ordered
//! set until it holds [`MAX_ITEMS`]. Then one `remove_items` call with them
//! in that order.

use freerouting_geometry::TileShape;

use crate::autoroute::batch::report::{corridor, is_removable_blocker};
use crate::board::BasicBoard;
use crate::board::items::ItemId;
use crate::drc::{AirLine, DesignRulesChecker};

/// The most items one ripup removes (`LastMileBlockerRipup.MAX_ITEMS`).
pub const MAX_ITEMS: usize = 24;

/// What a last-mile ripup chose, before it removes anything.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LastMilePlan {
    /// The airlines, in the DRC's order.
    pub airlines: Vec<AirLine>,
    /// The blockers to remove, in insertion order, at most [`MAX_ITEMS`].
    pub blockers: Vec<ItemId>,
}

/// The airlines and blockers [`rip_blockers`] would remove from `board`.
pub fn plan_last_mile(board: &BasicBoard) -> LastMilePlan {
    let mut drc = DesignRulesChecker::new(board);
    let airlines = drc.all_airlines();
    let mut blockers: Vec<ItemId> = Vec::new();
    'airlines: for airline in &airlines {
        // PARITY: LastMileBlockerRipup.java:37, `airline.net == null`: the
        // Rust airline always has a net number; a non-positive one is Java's
        // missing net.
        if airline.net_number <= 0 {
            continue;
        }
        let shape = TileShape::IntBox(corridor(airline.from_corner, airline.to_corner));
        // PARITY: :41, `overlappingObjects(corridor, -1)` on the default tree,
        // a `TreeSet<SearchTreeObject>` (highest item id first).
        for id in board.overlapping_objects(&shape, -1) {
            if blockers.len() >= MAX_ITEMS {
                break;
            }
            let Some(item) = board.item(id) else {
                continue;
            };
            // PARITY: :49-51, a `LinkedHashSet`: the first insertion decides
            // the position.
            if is_removable_blocker(board, item, airline.net_number) && !blockers.contains(&id) {
                blockers.push(id);
            }
        }
        if blockers.len() >= MAX_ITEMS {
            break 'airlines;
        }
    }
    LastMilePlan { airlines, blockers }
}

/// Java `ripBlockers`: removes the blockers of [`plan_last_mile`] in one
/// `remove_items` call and returns how many it handed over (Java returns
/// the list size, whatever `removeItems` reports).
pub fn rip_blockers(board: &mut BasicBoard) -> usize {
    let plan = plan_last_mile(board);
    if plan.blockers.is_empty() {
        return 0;
    }
    board.remove_items(plan.blockers.iter().copied());
    plan.blockers.len()
}

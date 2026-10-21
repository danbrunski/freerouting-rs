//! K15.5: the DRC report counted as KiCad counts.
//!
//! Not Java. [`DesignRulesChecker::generate_report`] writes Java's violation
//! types, and under [`ReportRules::Kicad`] (the default) it counts three of
//! them as KiCad 10's DRC does:
//!
//! - **`clearance` and `holeClearance`** are between copper of different nets
//!   only. Java's `Item.isObstacle` makes a via an obstacle for a pin (or a via)
//!   of its own net when the via's padstack does not allow attaching, and the
//!   KiCad reader writes `(attach off)` on every padstack, so every via of a
//!   pad's net inside or beside the pad is a clearance violation for Java. A
//!   pair whose items share a net is left out ([`reports_clearance`]) unless
//!   one of its items is copper the router placed (a trace or via that is not
//!   user or system fixed): a via the run put in a pad stays in the report, as
//!   the router must never place one there (D8). The search's obstacle test
//!   (`BasicBoard::is_obstacle`) is unchanged.
//! - **`via_dangling`**: a via is dangling when its copper meets copper of its
//!   net on fewer than two layers ([`via_dangling`], Q16's
//!   [`via_contact_layers`](crate::quality::stub_contacts::via_contact_layers)),
//!   not when Java's contacts (items of its net whose corner or centre is the
//!   via's centre, and areas that contain it) are fewer than two or all on one
//!   layer (`Item.isTail`). So a via that joins an SMD pad, off the pad's
//!   centre, to its net's plane on an inner layer is connected.
//! - **`track_dangling`**: a trace end is open when its end point lies on no
//!   copper of its net on its layer ([`track_dangling`], Q20's KiCad anchor
//!   test), not when no item of its net has a corner or centre exactly there.
//!   So a trace that ends on another trace's segment (a T junction) or inside a
//!   pad off its centre is connected.
//!
//! [`ReportRules::Java`] keeps Java's report: `tests/reference_drc.rs`
//! compares it with Java's DRC command line. The board-level findings
//! ([`DesignRulesChecker::check`], [`DesignRulesChecker::all_unconnected_items`])
//! that a viewer draws stay Java's, and so do the report's scores (they are the
//! router's: see `router.java_quirks.same_net_via_pad_violation` for its count
//! of the same-net pairs).
//!
//! [`DesignRulesChecker::generate_report`]: super::DesignRulesChecker::generate_report
//! [`DesignRulesChecker::check`]: super::DesignRulesChecker::check
//! [`DesignRulesChecker::all_unconnected_items`]: super::DesignRulesChecker::all_unconnected_items

use super::clearance::ClearanceViolation;
use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::structure::FixedState;
use crate::quality::stub_contacts;

/// Which DRC's counting a report follows (see the module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReportRules {
    /// KiCad's: clearance between different nets only, dangling vias and
    /// tracks by copper overlap.
    #[default]
    Kicad,
    /// Java's `DesignRulesChecker.generateReport`.
    Java,
}

/// True if `item` is copper the router placed or may move: a trace or via
/// that is neither user nor system fixed. Input copper read from a
/// `.kicad_pcb` is user fixed.
pub(crate) fn is_routed_copper(item: &Item) -> bool {
    (item.is_trace() || item.is_via()) && item.fixed_state() < FixedState::UserFixed
}

/// True if violation `v` is between items of a common net and neither of them
/// is copper the router placed ([`is_routed_copper`]): a via of the input in
/// or beside a pad of its own net.
pub(crate) fn is_same_net_input_pair(board: &BasicBoard, v: &ClearanceViolation) -> bool {
    let (Some(first), Some(second)) = (board.item(v.first_item), board.item(v.second_item)) else {
        return false;
    };
    first.shares_net(second) && !is_routed_copper(first) && !is_routed_copper(second)
}

/// True if a KiCad-counted report keeps clearance violation `v` (see the
/// module docs).
pub(crate) fn reports_clearance(board: &BasicBoard, v: &ClearanceViolation) -> bool {
    !is_same_net_input_pair(board, v)
}

/// True if via `id` meets copper of its net on fewer than two layers.
pub(crate) fn via_dangling(board: &BasicBoard, id: ItemId) -> bool {
    board
        .item(id)
        .is_some_and(|via| stub_contacts::via_contact_layers(board, via).len() <= 1)
}

/// True if an end point of trace `id` lies on no copper of its net on its
/// layer.
pub(crate) fn track_dangling(board: &BasicBoard, id: ItemId) -> bool {
    let Some(item) = board.item(id) else {
        return false;
    };
    let Some(trace) = item.as_trace() else {
        return false;
    };
    [trace.first_corner(), trace.last_corner()]
        .iter()
        .any(|end| !super::board_rules::end_on_copper(board, item, end))
}

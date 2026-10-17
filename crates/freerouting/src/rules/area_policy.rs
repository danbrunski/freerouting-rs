//! A per-net obstacle hook for conduction areas (B09.4).
//!
//! Java decides once per area whether a conduction area is an obstacle to
//! the traces and vias of foreign nets (`ConductionArea.isObstacle`, the DSN
//! `(type power)` planes are not). A quality stage such as Q01's pour guard
//! wants the answer per (area, net): a pour of a listed net class blocks the
//! nets that have no pin inside it. [`AreaObstaclePolicy`] is that hook. It
//! lives on the board rules as [`BoardRules::area_obstacle_policy`], which is
//! `None` on the parity path, and is consulted only in the conduction-area
//! branches of [`BasicBoard::is_trace_obstacle`] and
//! [`BasicBoard::is_obstacle`]:
//!
//! - an area is an obstacle for a net iff Java's rule says so **or** the
//!   policy [`blocks`](AreaObstaclePolicy::blocks) the net;
//! - the policy is asked only for foreign nets: an item that shares a net
//!   with the area is never blocked by it;
//! - in `is_obstacle` the policy applies to traces and vias (the items a
//!   router creates); pins, other areas and outlines keep Java's rule;
//! - with `None` the code path is Java's rule exactly: one `Option` test in
//!   a branch that only conduction areas reach.
//!
//! The callers ask at query time, so a policy never changes the contents of
//! a search tree. Not every Java call site goes through the two predicates:
//! `Item::is_drillable` and the shover's direct reads of the area flag
//! (`shove/spring.rs`, `shove/trace_entries.rs`) still use Java's flag only.
//!
//! [`BoardRules::area_obstacle_policy`]: crate::rules::BoardRules::area_obstacle_policy
//! [`BasicBoard::is_trace_obstacle`]: crate::board::BasicBoard::is_trace_obstacle
//! [`BasicBoard::is_obstacle`]: crate::board::BasicBoard::is_obstacle

use std::fmt;

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId};

/// Decides whether conduction area `area` blocks the traces and vias of net
/// `net`, beyond Java's per-area rule.
///
/// Implementations must be pure: the same board, area and net always give
/// the same answer (D2), because the maze search, the checks and the shover
/// may ask in any order and any number of times.
pub trait AreaObstaclePolicy: Send + Sync {
    /// True if `area` is an obstacle for net `net`. Only asked for nets the
    /// area does not contain.
    fn blocks(&self, area: ItemId, net: i32) -> bool;
}

impl fmt::Debug for dyn AreaObstaclePolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AreaObstaclePolicy")
    }
}

impl BasicBoard {
    /// True if the area policy makes conduction area `area` an obstacle for
    /// net `net`. False without a policy and for a net of the area.
    #[inline]
    pub(crate) fn area_policy_blocks_net(&self, area: &Item, net: i32) -> bool {
        match &self.rules.area_obstacle_policy {
            None => false,
            Some(policy) => !area.contains_net(net) && policy.blocks(area.id(), net),
        }
    }

    /// True if the area policy makes conduction area `area` an obstacle for
    /// `other`: a trace or via with a net, sharing none with the area, of
    /// which the policy blocks at least one. False without a policy.
    #[inline]
    pub(crate) fn area_policy_blocks_item(&self, area: &Item, other: &Item) -> bool {
        match &self.rules.area_obstacle_policy {
            None => false,
            Some(policy) => {
                (other.is_trace() || other.is_via())
                    && !other.shares_net(area)
                    && other
                        .net_numbers()
                        .iter()
                        .any(|n| policy.blocks(area.id(), *n))
            }
        }
    }
}

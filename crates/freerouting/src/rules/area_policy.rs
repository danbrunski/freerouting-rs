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
//! A policy may block a net's traces but not its vias (Q09.4, the pour
//! guard's `traces` block mode): [`AreaObstaclePolicy::blocks_vias`] answers
//! for vias, and defaults to [`AreaObstaclePolicy::blocks`]. The via side is
//! read by `is_obstacle` for a via, by the drill page
//! ([`BasicBoard::is_drillable_with_policy`]) and by the shover's pad checks
//! ([`BasicBoard::area_policy_blocks_any_net_vias`]). The maze search lets a
//! drill pass a layer on which only such areas (and Q09's trace-only
//! keepouts) cover its location ([`BasicBoard::lets_vias_through`],
//! `autoroute/session/drills.rs`); that is only looked at when
//! [`AreaObstaclePolicy::lets_vias_through`] says some area may do so.
//!
//! The callers ask at query time, so a policy never changes the contents of
//! a search tree. The Java call sites that read the area flag directly go
//! through the policy as well (Q01): the drill page's `isDrillable` test
//! ([`BasicBoard::is_drillable_with_policy`], `autoroute/session/drills.rs`)
//! and the shover's two reads (`shove/spring.rs`, `shove/trace_entries.rs`,
//! [`BasicBoard::area_policy_blocks_any_net`]). Each is one `Option` test
//! when no policy is installed.
//!
//! [`BoardRules::area_obstacle_policy`]: crate::rules::BoardRules::area_obstacle_policy
//! [`BasicBoard::is_trace_obstacle`]: crate::board::BasicBoard::is_trace_obstacle
//! [`BasicBoard::is_obstacle`]: crate::board::BasicBoard::is_obstacle

use std::fmt;

use crate::board::BasicBoard;
use crate::board::items::{AreaKind, Item, ItemId};

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

    /// True if `area` is an obstacle for the vias of net `net` (Q09.4). Only
    /// asked for nets the area does not contain. The default is
    /// [`blocks`](Self::blocks): traces and vias alike.
    fn blocks_vias(&self, area: ItemId, net: i32) -> bool {
        self.blocks(area, net)
    }

    /// True if some area may block a net's traces but not its vias, so that
    /// the maze search must look for drills that pass such an area (Q09.4).
    /// False by default.
    fn lets_vias_through(&self) -> bool {
        false
    }
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
    /// which the policy blocks at least one (for a via, its vias). False
    /// without a policy.
    #[inline]
    pub(crate) fn area_policy_blocks_item(&self, area: &Item, other: &Item) -> bool {
        match &self.rules.area_obstacle_policy {
            None => false,
            Some(policy) => {
                let is_via = other.is_via();
                (other.is_trace() || is_via)
                    && !other.shares_net(area)
                    && other.net_numbers().iter().any(|n| {
                        if is_via {
                            policy.blocks_vias(area.id(), *n)
                        } else {
                            policy.blocks(area.id(), *n)
                        }
                    })
            }
        }
    }

    /// True if the area policy makes conduction area `area` an obstacle for
    /// at least one of `nets`, none of which the area contains (the
    /// shover's test for the nets of the item it moves). False without a
    /// policy.
    #[inline]
    pub(crate) fn area_policy_blocks_any_net(&self, area: &Item, nets: &[i32]) -> bool {
        match &self.rules.area_obstacle_policy {
            None => false,
            Some(policy) => {
                !area.shares_net_no(nets) && nets.iter().any(|n| policy.blocks(area.id(), *n))
            }
        }
    }

    /// [`area_policy_blocks_any_net`](Self::area_policy_blocks_any_net) for
    /// vias: the shover's pad checks (Q09.4). False without a policy.
    #[inline]
    pub(crate) fn area_policy_blocks_any_net_vias(&self, area: &Item, nets: &[i32]) -> bool {
        match &self.rules.area_obstacle_policy {
            None => false,
            Some(policy) => {
                !area.shares_net_no(nets) && nets.iter().any(|n| policy.blocks_vias(area.id(), *n))
            }
        }
    }

    /// Java `Item.isDrillable(net)` with the area policy: a conduction area
    /// the policy blocks for the vias of `net` is not drillable for it.
    /// Without a policy this is [`Item::is_drillable`].
    #[inline]
    pub(crate) fn is_drillable_with_policy(&self, item: &Item, net: i32) -> bool {
        item.is_drillable(net)
            && !(item.is_conduction_area()
                && match &self.rules.area_obstacle_policy {
                    None => false,
                    Some(policy) => !item.contains_net(net) && policy.blocks_vias(item.id(), net),
                })
    }

    /// True if `item` is a trace obstacle for net `net` that lets the net's
    /// vias through: a trace-only keepout (Q09.2), or a conduction area that
    /// is a trace obstacle for the net only through the area policy, which
    /// lets its vias through (Q09.4).
    pub(crate) fn lets_vias_through(&self, item: &Item, net: i32) -> bool {
        let Some(area) = item.as_area() else {
            return false;
        };
        match area.kind {
            AreaKind::TraceObstacle => true,
            AreaKind::Conduction {
                is_obstacle: false, ..
            } => match &self.rules.area_obstacle_policy {
                None => false,
                Some(policy) => {
                    !item.contains_net(net)
                        && policy.blocks(item.id(), net)
                        && !policy.blocks_vias(item.id(), net)
                }
            },
            _ => false,
        }
    }

    /// True if the board may have trace obstacles that let vias through
    /// ([`lets_vias_through`](Self::lets_vias_through)): a trace-only
    /// keepout, or an area policy that says so. One scan of the items; the
    /// maze search asks once per connection.
    pub fn may_let_vias_through(&self) -> bool {
        self.rules
            .area_obstacle_policy
            .as_ref()
            .is_some_and(|p| p.lets_vias_through())
            || self.items().any(|item| {
                item.as_area()
                    .is_some_and(|a| a.kind == AreaKind::TraceObstacle)
            })
    }
}

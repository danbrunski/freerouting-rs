//! `FreeroutingRipupPolicy` (A10.5): Java Freerouting's ripup cost
//! schedule, strict-DRC rule and repeated-failure ripup, at `a773a70ec`.
//!
//! - Autoroute passes: `startRipupCosts * passNo`
//!   (`AutorouteConnectionRouter.java:46`), pass counted from 1.
//! - Fanout passes: `startRipupCosts * (passNo + 1)` with the pass counted
//!   from 0, or -1 ("no ripup") when `fanout.ripup_allowed` is off
//!   (`BatchFanout.fanoutPass`, `BatchFanout.java:181-191`).
//! - The optimizer's ripup factors belong to O06; until it lands the
//!   optimizer phase uses the autoroute formula.
//! - Strict DRC applies when `strict_drc` is set or from pass 3 on
//!   (`AutorouteConnectionRouter.java:280`).
//! - After an item's second failure, every trace and via of the routed net
//!   that is not user-fixed and has exactly one net is removed, in
//!   `getConnectableItems` order (highest id first), with one
//!   `removeItems` call (`AutoroutePassRunner.java:167-179`).
//!
//! The int products wrap as Java's do.

use crate::autoroute::events::Phase;
use crate::autoroute::stages::{RipupPolicy, RouteTask};
use crate::board::items::ItemId;
use crate::board::routing_board::RoutingBoard;
use crate::settings::RouterSettings;

/// The failure count from which a failed item's net is ripped
/// (`AutoroutePassRunner.java:167`).
pub const RIP_NET_AFTER_FAILURES: u32 = 2;

/// The pass from which strict DRC applies without `strict_drc`.
pub const STRICT_FROM_PASS: u32 = 3;

/// Java Freerouting's ripup policy (`freerouting-ripup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreeroutingRipupPolicy {
    fanout_ripup_allowed: bool,
}

impl FreeroutingRipupPolicy {
    /// A policy; `fanout_ripup_allowed` is `router.fanout.ripup_allowed`.
    pub fn new(fanout_ripup_allowed: bool) -> Self {
        FreeroutingRipupPolicy {
            fanout_ripup_allowed,
        }
    }

    /// The policy for `settings`.
    pub fn from_settings(settings: &RouterSettings) -> Self {
        FreeroutingRipupPolicy::new(settings.fanout.ripup_allowed)
    }

    /// The items [`after_failure`](RipupPolicy::after_failure) removes for
    /// a failure of an item on `net` with `failures` failures, in removal
    /// order.
    pub fn net_items_to_rip(board: &RoutingBoard, net: i32, failures: u32) -> Vec<ItemId> {
        if failures < RIP_NET_AFTER_FAILURES {
            return Vec::new();
        }
        // PARITY: AutoroutePassRunner.java:169-175, getConnectableItems order.
        board
            .connectable_items(net)
            .into_iter()
            .filter(|id| {
                board.item(*id).is_some_and(|item| {
                    (item.is_trace() || item.is_via())
                        && !item.is_user_fixed()
                        && item.net_count() == 1
                })
            })
            .collect()
    }
}

impl RipupPolicy for FreeroutingRipupPolicy {
    fn id(&self) -> &'static str {
        "freerouting-ripup"
    }

    fn ripup_cost(&self, phase: Phase, pass: u32, start_ripup_costs: i32) -> i32 {
        let pass = pass as i32;
        match phase {
            // PARITY: BatchFanout.java:181, 187-191.
            Phase::Fanout if !self.fanout_ripup_allowed => -1,
            Phase::Fanout => start_ripup_costs.wrapping_mul(pass.wrapping_add(1)),
            // PARITY: AutorouteConnectionRouter.java:46.
            Phase::Autoroute | Phase::Optimize => start_ripup_costs.wrapping_mul(pass),
        }
    }

    fn strict_check(&self, pass: u32, strict_drc: bool) -> bool {
        // PARITY: AutorouteConnectionRouter.java:280.
        strict_drc || pass >= STRICT_FROM_PASS
    }

    fn after_failure(&mut self, board: &mut RoutingBoard, task: &RouteTask, failures: u32) {
        let rip = Self::net_items_to_rip(board, task.net, failures);
        // PARITY: AutoroutePassRunner.java:177-179, one call, skipped when
        // empty.
        if !rip.is_empty() {
            board.remove_items(rip);
        }
    }
}

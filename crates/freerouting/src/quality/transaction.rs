//! Q11: transactional rip-up, the `transactional-ripup` connection router
//! (`router.autorouter.connection_router=transactional-ripup`).
//!
//! In Java's pass 1 a rip whose victim does not re-land is permanent: the
//! search's rip cost (`MazeRipupResolver.checkRipup`) ignores whether the
//! ripped connection can be routed again. This router wraps Java's
//! connection router (`freerouting-connection`, with the plane-tap and
//! neck-down policies the settings select) and judges each outcome; the
//! search and its rip costs stay Java's.
//!
//! **One transaction**, for a connection whose parameters allow rip-up:
//! 1. The board is kept as it is (a `Clone`, so a rollback brings back the
//!    search tree's shape and every id as they were; see *Snapshot*). The
//!    inner router routes the connection.
//! 2. The *victims* are the nets of the ripped items that the attempt took
//!    off the board, the routed net excepted. Without a victim the outcome is
//!    returned as it is, and nothing else happens.
//! 3. Each victim net is routed again at once with rip-up off: one attempt
//!    per connected piece of the net, from the piece's lowest-id pin (any
//!    connectable item that is not a trace or via), all victims in ascending
//!    order of that item id, then net. Each attempt gets the pass's own
//!    parameters for its net (Java's `connection_params`: plane via costs,
//!    maze budget, pull-tight, strict check) and its own changed area, as a
//!    pass connection does.
//! 4. The arm's rule ([`CommitRule`]) keeps the board or brings the one of
//!    step 1 back. After a rollback the connection is routed once more with
//!    rip-up off, and that outcome is returned: the pass records a failure
//!    as Java would, and Java's repeated-failure rip
//!    ([`RipupPolicy::after_failure`](crate::autoroute::stages::RipupPolicy::after_failure))
//!    is untouched.
//!
//! **Arms** (`router.transaction.rule`, unset `strict`):
//! - `strict` (Q11.2): commit only if every victim net has no more
//!   incomplete connections than before the transaction;
//! - `count` (Q11.3): commit only if the board's incomplete count (the
//!   routing ledger's) is strictly lower than before the transaction, so a
//!   ripped multi-pin net may re-land along another topology, or not at all
//!   if something else gained more.
//!
//! **Depth** (`router.transaction.depth`, unset 0; Q11.4): with depth 1 the
//! victims of step 3 route with rip-up on. Nets that a victim's attempt rips
//! are victims of the second wave, which routes with rip-up off. No net is
//! ripped twice in one transaction: a victim attempt that rips the routed
//! net or a net the transaction already ripped is undone (the board before
//! that attempt comes back) and routed again with rip-up off. So a
//! transaction routes at most two waves and always ends; the commit rule
//! judges every victim of both waves.
//!
//! **Rip-count protection** (`router.transaction.rip_factor`, unset 0, off;
//! Q11.5): with factor `f > 0`, the rip cost of an item is multiplied by
//! `1 + f·k`, where `k` counts the committed transactions that ripped the
//! item's net this run (KRT measured P(ripped again | ripped k times) ≈ 0.9).
//! The factors reach the maze search through
//! [`ConnectionParams::ripup_factors`], which every other caller leaves
//! `None`, so the Java search never consults them.
//!
//! **Snapshot.** The board is cloned before every connection that may rip;
//! a connection with rip-up off goes to the inner router untouched. A
//! [`BoardSnapshot`](crate::board::BoardSnapshot) restore rebuilds the
//! search tree in Java's deserialization order and so changes the work
//! counts of every later search (D5); the undo journal
//! ([`RoutingBoard::try_change`]) keeps the ids the attempt took and
//! reinserts the undone items, which changes the tree as well. A `Clone`
//! restores the board exactly, as the Q10 sweep does. The work a rolled-back
//! attempt did is added back to the restored board's meter, so the run's
//! work total counts it. A listener is told of the restored board with
//! [`RouterEvents::replaced`](crate::autoroute::events::RouterEvents::replaced).
//!
//! **Orders.** Victims in ascending `(item id, net)`; every set is a
//! `BTreeSet`; the commit test reads the routing ledger. Same board and
//! settings, same routes (D2).
//!
//! **Scope.** The router wraps every connection the configured router
//! routes: the batch passes, the Q10 sweep (whose connections have rip-up
//! off, so they pass through) and the optimizer's re-routes. The
//! [`log_lines`](ConnectionRouter::log_lines) tally goes into the job log
//! after the routing passes.
//!
//! Off by default and in no profile: with `router.autorouter.connection_router`
//! unset nothing here runs. The idea credits: KiCadRoutingTools' custody
//! rips (`docs/rip-up-reroute.md` at `0486e799`), rjwalters/kicad-tools'
//! all-or-nothing rollback (#6342, #6355) and Shaw's Large Neighbourhood
//! Search (CP-98); no code follows any of them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::autoroute::AttemptState;
use crate::autoroute::batch::connection::{FreeroutingConnectionRouter, connection_params};
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, ConnectionRouter, RouteTask, StageCtx, UnknownStage,
};
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::settings::RouterSettings;

/// The registry id of the router.
pub const ROUTER_ID: &str = "transactional-ripup";

/// The settings key of the commit rule.
pub const RULE_KEY: &str = "router.transaction.rule";

/// The settings key of the rip depth.
pub const DEPTH_KEY: &str = "router.transaction.depth";

/// The settings key of the rip-count factor.
pub const RIP_FACTOR_KEY: &str = "router.transaction.rip_factor";

/// The rule values, in the order the error message names them.
pub const RULES: [&str; 2] = ["strict", "count"];

/// The deepest rip depth.
pub const MAX_DEPTH: u32 = 1;

/// When a transaction keeps its board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CommitRule {
    /// Every victim net is no worse than before (Q11.2).
    #[default]
    Strict,
    /// The board's incomplete count fell (Q11.3).
    Count,
}

impl CommitRule {
    /// The rule of a setting value; unset is [`CommitRule::Strict`].
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim) {
            None | Some("") | Some("strict") => Ok(CommitRule::Strict),
            Some("count") => Ok(CommitRule::Count),
            Some(other) => Err(format!(
                "{RULE_KEY}: unknown value '{other}' (expected 'strict' or 'count')"
            )),
        }
    }

    /// The setting value of the rule.
    pub fn as_str(self) -> &'static str {
        match self {
            CommitRule::Strict => "strict",
            CommitRule::Count => "count",
        }
    }
}

/// The three transaction settings, resolved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransactionConfig {
    /// `router.transaction.rule`.
    pub rule: CommitRule,
    /// `router.transaction.depth`: 0 or 1.
    pub depth: u32,
    /// `router.transaction.rip_factor`: 0 is off.
    pub rip_factor: u32,
}

impl TransactionConfig {
    /// The settings of `settings`; fails on an unknown rule, a depth other
    /// than 0 or 1, or a negative factor, naming the key.
    pub fn from_settings(settings: &RouterSettings) -> Result<Self, String> {
        let t = &settings.transaction;
        let rule = CommitRule::parse(t.rule.as_deref())?;
        let depth = match t.depth {
            None => 0,
            Some(d) if (0..=MAX_DEPTH as i32).contains(&d) => d as u32,
            Some(d) => return Err(format!("{DEPTH_KEY}: {d} is not 0 or 1")),
        };
        let rip_factor = match t.rip_factor {
            None => 0,
            Some(f) if f >= 0 => f as u32,
            Some(f) => return Err(format!("{RIP_FACTOR_KEY}: {f} is negative")),
        };
        Ok(TransactionConfig {
            rule,
            depth,
            rip_factor,
        })
    }
}

/// Refuses an invalid `router.transaction.*` value before routing, as an
/// unknown stage id is refused (`run_pipeline` calls it).
pub fn check_settings(settings: &RouterSettings) -> Result<(), UnknownStage> {
    let t = &settings.transaction;
    if CommitRule::parse(t.rule.as_deref()).is_err() {
        return Err(UnknownStage {
            key: RULE_KEY,
            id: t.rule.clone().unwrap_or_default(),
            known: RULES.to_vec(),
        });
    }
    if let Err(e) = TransactionConfig::from_settings(settings) {
        let (key, id, known) = if e.starts_with(DEPTH_KEY) {
            (DEPTH_KEY, t.depth.unwrap_or(0).to_string(), vec!["0", "1"])
        } else {
            (
                RIP_FACTOR_KEY,
                t.rip_factor.unwrap_or(0).to_string(),
                vec!["0 or more"],
            )
        };
        return Err(UnknownStage { key, id, known });
    }
    Ok(())
}

/// What the transactions of a run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransactionStats {
    /// Connections whose attempt ripped copper of another net.
    pub transactions: u64,
    /// Transactions whose board was kept.
    pub committed: u64,
    /// Transactions whose board was rolled back.
    pub rolled_back: u64,
    /// Victim nets, summed over the transactions.
    pub victims: u64,
    /// Victim nets that ended with no more incomplete connections than
    /// before their transaction.
    pub victims_relanded: u64,
    /// Rollbacks whose rip-up-free retry routed the connection.
    pub retries_routed: u64,
    /// Depth-1 victim attempts undone because they ripped a net of their
    /// transaction again.
    pub refused_rerips: u64,
}

/// The `transactional-ripup` connection router (see the module docs).
pub struct TransactionalRipup {
    inner: Box<dyn ConnectionRouter>,
    config: TransactionConfig,
    stats: TransactionStats,
    /// Per net, the committed transactions that ripped it.
    rip_counts: BTreeMap<i32, u32>,
}

impl TransactionalRipup {
    /// The router around `inner` with `config`.
    pub fn new(inner: Box<dyn ConnectionRouter>, config: TransactionConfig) -> Self {
        TransactionalRipup {
            inner,
            config,
            stats: TransactionStats::default(),
            rip_counts: BTreeMap::new(),
        }
    }

    /// The router around Java's connection router with the configuration of
    /// `settings` (an invalid value, which [`check_settings`] refuses for a
    /// job, reads as the default).
    pub fn from_settings(settings: &RouterSettings) -> Self {
        let config = TransactionConfig::from_settings(settings).unwrap_or_default();
        TransactionalRipup::new(
            Box::new(FreeroutingConnectionRouter::from_settings(settings)),
            config,
        )
    }

    /// The tally so far.
    pub fn stats(&self) -> TransactionStats {
        self.stats
    }

    /// The committed transactions that ripped each net, ascending by net.
    pub fn rip_counts(&self) -> &BTreeMap<i32, u32> {
        &self.rip_counts
    }

    /// `p` with the rip-count factors, when the protection is on and some
    /// net was ripped.
    fn protected(&self, p: &ConnectionParams) -> Option<ConnectionParams> {
        if self.config.rip_factor == 0 || self.rip_counts.is_empty() || !p.ripup_allowed {
            return None;
        }
        let f = i64::from(self.config.rip_factor);
        let factors: BTreeMap<i32, i32> = self
            .rip_counts
            .iter()
            .map(|(net, k)| {
                let factor = (1 + f * i64::from(*k)).min(i64::from(i32::MAX));
                (*net, factor as i32)
            })
            .collect();
        Some(ConnectionParams {
            ripup_factors: Some(Arc::new(factors)),
            ..p.clone()
        })
    }

    /// The inner router on `task`, with the rip-count factors when on.
    fn inner_route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        match self.protected(p) {
            Some(protected) => self.inner.route(board, task, &protected, ctx),
            None => self.inner.route(board, task, p, ctx),
        }
    }

    /// Routes the victim nets `nets` of wave `depth` (module docs, step 3),
    /// adding the nets they rip to `txn`.
    fn reland(
        &mut self,
        board: &mut RoutingBoard,
        nets: &BTreeSet<i32>,
        depth: u32,
        txn: &mut Transaction,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) {
        let mut tasks: Vec<RouteTask> = nets
            .iter()
            .flat_map(|net| victim_tasks(board, *net))
            .collect();
        tasks.sort_by_key(|t| (t.item, t.net));
        let may_rip = depth < self.config.depth;
        let mut next_wave = BTreeSet::new();
        for task in tasks {
            if ctx.cancel.is_router_stop_requested() {
                break;
            }
            board.start_marking_changed_area();
            let mut params = victim_params(board, &task, p, ctx);
            params.ripup_allowed = may_rip;
            if !may_rip {
                self.inner_route(board, &task, &params, ctx);
                continue;
            }
            let saved = board.clone();
            let units = meter_units(board);
            let outcome = self.inner_route(board, &task, &params, ctx);
            let ripped = victim_nets(&saved, board, &outcome, task.net);
            if ripped.is_empty() {
                continue;
            }
            if ripped.iter().any(|net| txn.touched.contains(net)) {
                // No net is ripped twice in one transaction.
                self.stats.refused_rerips += 1;
                restore(board, saved, units, ctx);
                board.start_marking_changed_area();
                params.ripup_allowed = false;
                self.inner_route(board, &task, &params, ctx);
                continue;
            }
            txn.touched.extend(ripped.iter().copied());
            txn.victims.extend(ripped.iter().copied());
            next_wave.extend(ripped);
        }
        if !next_wave.is_empty() {
            self.reland(board, &next_wave, depth + 1, txn, p, ctx);
        }
    }
}

/// The nets one transaction ripped, and those it may not rip again.
struct Transaction {
    /// Every victim net of both waves.
    victims: BTreeSet<i32>,
    /// The routed net and every victim net.
    touched: BTreeSet<i32>,
}

impl ConnectionRouter for TransactionalRipup {
    fn id(&self) -> &'static str {
        ROUTER_ID
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        if !p.ripup_allowed {
            return self.inner.route(board, task, p, ctx);
        }
        let before = board.clone();
        let units = meter_units(board);
        let mut outcome = self.inner_route(board, task, p, ctx);
        let victims = victim_nets(&before, board, &outcome, task.net);
        if victims.is_empty() {
            return outcome;
        }
        self.stats.transactions += 1;
        let mut txn = Transaction {
            touched: victims.iter().copied().chain([task.net]).collect(),
            victims: victims.clone(),
        };
        self.reland(board, &victims, 0, &mut txn, p, ctx);
        let relanded = txn
            .victims
            .iter()
            .filter(|net| net_incomplete(board, **net) <= net_incomplete(&before, **net))
            .count();
        self.stats.victims += txn.victims.len() as u64;
        self.stats.victims_relanded += relanded as u64;
        let commit = match self.config.rule {
            CommitRule::Strict => relanded == txn.victims.len(),
            CommitRule::Count => incomplete(board) < incomplete(&before),
        };
        if commit {
            self.stats.committed += 1;
            for net in &txn.victims {
                *self.rip_counts.entry(*net).or_insert(0) += 1;
            }
            // The outcome's work is the transaction's, victims included.
            outcome.work.units = meter_units(board).wrapping_sub(units);
            return outcome;
        }
        self.stats.rolled_back += 1;
        let spent = meter_units(board).wrapping_sub(units);
        restore(board, before, units, ctx);
        let retry_params = ConnectionParams {
            ripup_allowed: false,
            ..p.clone()
        };
        let mut retry = self.inner.route(board, task, &retry_params, ctx);
        if retry.state == AttemptState::Routed {
            self.stats.retries_routed += 1;
        }
        retry.work.units = retry.work.units.wrapping_add(spent);
        retry
    }

    fn log_lines(&self) -> Vec<String> {
        let s = &self.stats;
        vec![format!(
            "Transactional rip-up (rule {}, depth {}, rip factor {}): {} transactions, {} \
             committed, {} rolled back ({} retries routed); {} victim nets, {} re-landed; {} \
             re-rips refused",
            self.config.rule.as_str(),
            self.config.depth,
            self.config.rip_factor,
            s.transactions,
            s.committed,
            s.rolled_back,
            s.retries_routed,
            s.victims,
            s.victims_relanded,
            s.refused_rerips,
        )]
    }
}

/// The board's meter count.
fn meter_units(board: &RoutingBoard) -> u64 {
    board.search_trees().work_meter().units()
}

/// Puts `saved` back as the board, keeps the work done since `units` on
/// its meter, and tells a listener.
fn restore(board: &mut RoutingBoard, saved: RoutingBoard, units: u64, ctx: &StageCtx) {
    let spent = meter_units(board).wrapping_sub(units);
    *board = saved;
    board.search_trees().work_meter().add(spent);
    if ctx.events.is_listening() {
        ctx.events.replaced(board.snapshot());
    }
}

/// The incomplete connections of `board` (the routing ledger's).
fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

/// The incomplete connections of net `net` on `board`.
fn net_incomplete(board: &RoutingBoard, net: i32) -> usize {
    board.routing_ledger().net_incomplete_count(board, net)
}

/// The nets of the items of `outcome.ripped` that were on `before` and are
/// not on `after`, without `own_net`.
pub fn victim_nets(
    before: &RoutingBoard,
    after: &RoutingBoard,
    outcome: &ConnectionOutcome,
    own_net: i32,
) -> BTreeSet<i32> {
    outcome
        .ripped
        .iter()
        .filter(|id| after.item(**id).is_none())
        .filter_map(|id| before.item(*id))
        .flat_map(|item| item.net_numbers().iter().copied())
        .filter(|net| *net > 0 && *net != own_net)
        .collect()
}

/// One task per connected piece of net `net` when the net is in pieces:
/// from the piece's lowest-id item that is not a trace or via. A piece of
/// traces and vias only is left alone.
pub fn victim_tasks(board: &RoutingBoard, net: i32) -> Vec<RouteTask> {
    let sets = board.connected_sets(net);
    if sets.len() < 2 {
        return Vec::new();
    }
    let is_plane = board
        .rules
        .nets
        .get(net)
        .is_some_and(|n| n.contains_plane());
    sets.iter()
        .filter_map(|set| {
            set.iter()
                .copied()
                .find(|id| board.item(*id).is_some_and(|item| !item.is_routable()))
        })
        .map(|item: ItemId| RouteTask {
            item,
            net,
            is_plane,
        })
        .collect()
}

/// The pass's parameters for a victim `task` (Java's for its net).
fn victim_params(
    board: &RoutingBoard,
    task: &RouteTask,
    p: &ConnectionParams,
    ctx: &StageCtx,
) -> ConnectionParams {
    let mut params = connection_params(
        board,
        task,
        p.pass,
        ctx,
        p.ripup_cost,
        &p.trace_costs,
        p.remove_unconnected_vias,
        p.pull_tight_accuracy,
        p.strict_check,
    );
    params.work_budget = p.work_budget;
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Never;

    impl ConnectionRouter for Never {
        fn id(&self) -> &'static str {
            "never"
        }
        fn route(
            &mut self,
            _: &mut RoutingBoard,
            _: &RouteTask,
            _: &ConnectionParams,
            _: &mut StageCtx,
        ) -> ConnectionOutcome {
            unreachable!("not routed")
        }
    }

    fn params(ripup_allowed: bool) -> ConnectionParams {
        ConnectionParams {
            pass: 1,
            ripup_allowed,
            ripup_cost: 1000,
            via_cost: 50,
            trace_costs: Vec::new(),
            remove_unconnected_vias: false,
            work_budget: 1,
            pull_tight_accuracy: 500,
            strict_check: false,
            ripup_factors: None,
        }
    }

    #[test]
    fn the_factors_are_one_plus_f_times_k() {
        let config = TransactionConfig {
            rip_factor: 2,
            ..TransactionConfig::default()
        };
        let mut router = TransactionalRipup::new(Box::new(Never), config);
        assert_eq!(router.protected(&params(true)), None, "nothing ripped yet");
        router.rip_counts.insert(4, 1);
        router.rip_counts.insert(9, 3);
        let p = router.protected(&params(true)).expect("protected");
        let factors: Vec<(i32, i32)> = p
            .ripup_factors
            .unwrap()
            .iter()
            .map(|(n, f)| (*n, *f))
            .collect();
        assert_eq!(factors, [(4, 3), (9, 7)]);
        assert_eq!(
            router.protected(&params(false)),
            None,
            "no rip-up, no factors"
        );
        let off = TransactionalRipup {
            config: TransactionConfig::default(),
            ..router
        };
        assert_eq!(off.protected(&params(true)), None, "factor 0 is off");
    }

    #[test]
    fn the_rule_parses() {
        assert_eq!(CommitRule::parse(None), Ok(CommitRule::Strict));
        assert_eq!(CommitRule::parse(Some("")), Ok(CommitRule::Strict));
        assert_eq!(CommitRule::parse(Some(" count ")), Ok(CommitRule::Count));
        assert!(
            CommitRule::parse(Some("all"))
                .unwrap_err()
                .contains(RULE_KEY)
        );
        for rule in RULES {
            assert_eq!(CommitRule::parse(Some(rule)).unwrap().as_str(), rule);
        }
    }
}

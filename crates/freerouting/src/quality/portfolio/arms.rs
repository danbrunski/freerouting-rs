//! The arms of Q12's pass-1 portfolio: their fixed list, their pass-1
//! orders, and the recorders that note what an arm's pass 1 did (its order,
//! its failed connections and the nets it ripped) for the squeaky-wheel
//! restarts.
//!
//! **The arm list** ([`portfolio_arms`]) is fixed, in this order:
//! 0. the configured ordering (`router.autorouter.ordering`; Java's item
//!    order unless it is set);
//! 1. Q07's `conflict-rounds` order;
//! 2. Q07's `inside-out` order;
//! 3. and on: Java's pass-1 list with its signal nets shuffled by
//!    `Collections.shuffle(nets, new Random(seed))`, seeds 1, 2, … in turn.
//!
//! Arms 1 and on reorder pass 1 only and are Freerouting's ordering
//! afterwards ([`PassOneOrdering`](crate::quality::ordering::PassOneOrdering) for Q07's orders, [`ArmOrdering`] for the
//! others), whatever ordering the job configured. Every reordering moves
//! whole nets and keeps the plane-net tasks first in Java's order (A10's
//! invariant, as Q07's orders do).
//!
//! **A restart** ([`Arm::Restart`]) routes pass 1 in a given item order: the
//! winner's pass-1 order, stable-sorted with that run's failed nets first
//! (a connection of theirs failed in pass 1), then its victim nets (another
//! net's pass-1 connection ripped their traces or vias), then the rest
//! ([`squeaky_order`]); Joslin and Clements' squeaky wheel (credit only, no
//! code followed). Failed nets go before victims: a net that ripped a victim
//! routed, and the victim first again would repeat the run.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use freerouting_geometry::jmath::JavaRandom;

use crate::autoroute::AttemptState;
use crate::autoroute::batch::ordering::{FreeroutingOrdering, select_items};
use crate::autoroute::events::CancelToken;
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, ConnectionRouter, NetOrdering, PassReport, RouteTask,
    StageCtx,
};
use crate::board::items::ItemId;
use crate::board::routing_board::RoutingBoard;
use crate::quality::ordering::{arrange_by_nets, conflict_rounds, inside_out};

/// The arm index of the first seeded shuffle.
pub const FIRST_SHUFFLE_ARM: usize = 3;

/// One arm of the portfolio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arm {
    /// The job's configured ordering.
    Configured,
    /// Q07's `conflict-rounds` pass-1 order.
    ConflictRounds,
    /// Q07's `inside-out` pass-1 order.
    InsideOut,
    /// Java's pass-1 list with the signal nets shuffled with this seed.
    Shuffle(i64),
    /// Squeaky-wheel generation `generation` (from 1): pass 1 in `order`.
    Restart {
        /// The generation, from 1.
        generation: u32,
        /// The pass-1 item order.
        order: Vec<ItemId>,
    },
}

impl Arm {
    /// The arm's label in the job log and the outcome.
    pub fn label(&self) -> String {
        match self {
            Arm::Configured => "configured".to_string(),
            Arm::ConflictRounds => conflict_rounds::ORDERING_ID.to_string(),
            Arm::InsideOut => inside_out::ORDERING_ID.to_string(),
            Arm::Shuffle(seed) => format!("shuffle-{seed}"),
            Arm::Restart { generation, .. } => format!("restart-{generation}"),
        }
    }

    /// The ordering the arm routes with; `configured` is the job's.
    pub fn ordering(&self, configured: Box<dyn NetOrdering>) -> Box<dyn NetOrdering> {
        match self {
            Arm::Configured => configured,
            Arm::ConflictRounds => Box::new(conflict_rounds::ordering()),
            Arm::InsideOut => Box::new(inside_out::ordering()),
            Arm::Shuffle(seed) => Box::new(ArmOrdering::new(PassOne::Shuffle(*seed))),
            Arm::Restart { order, .. } => Box::new(ArmOrdering::new(PassOne::Items(
                order
                    .iter()
                    .enumerate()
                    .map(|(rank, id)| (*id, rank))
                    .collect(),
            ))),
        }
    }
}

/// The first `k` arms of the fixed list (module docs).
pub fn portfolio_arms(k: usize) -> Vec<Arm> {
    (0..k)
        .map(|i| match i {
            0 => Arm::Configured,
            1 => Arm::ConflictRounds,
            2 => Arm::InsideOut,
            _ => Arm::Shuffle(i64::try_from(i - FIRST_SHUFFLE_ARM + 1).unwrap_or(i64::MAX)),
        })
        .collect()
}

/// A pass-1 reordering of [`ArmOrdering`].
#[derive(Clone, Debug)]
enum PassOne {
    /// The signal nets shuffled with this seed.
    Shuffle(i64),
    /// The tasks sorted by their item's rank; items without one keep their
    /// order after the ranked ones.
    Items(BTreeMap<ItemId, usize>),
}

/// A [`NetOrdering`] that reorders pass 1 and is Freerouting's ordering
/// afterwards, as [`PassOneOrdering`](crate::quality::ordering::PassOneOrdering)
/// is for Q07's orders.
#[derive(Clone, Debug)]
pub struct ArmOrdering {
    pass_one: PassOne,
    inner: FreeroutingOrdering,
    first_pass_done: bool,
}

impl ArmOrdering {
    fn new(pass_one: PassOne) -> Self {
        ArmOrdering {
            pass_one,
            inner: FreeroutingOrdering::new(),
            first_pass_done: false,
        }
    }
}

/// Java's list with its signal nets (in order of first appearance)
/// shuffled by `Collections.shuffle(nets, new Random(seed))`.
pub fn shuffled_order(tasks: Vec<RouteTask>, seed: i64) -> Vec<RouteTask> {
    let mut nets: Vec<i32> = Vec::new();
    let mut seen = BTreeSet::new();
    for task in tasks.iter().filter(|t| !t.is_plane) {
        if seen.insert(task.net) {
            nets.push(task.net);
        }
    }
    JavaRandom::new(seed).shuffle(&mut nets);
    arrange_by_nets(tasks, &nets)
}

/// `tasks` stable-sorted by the rank of their item; plane tasks stay first
/// in their order, unranked signal tasks keep their order at the end.
pub fn ranked_order(tasks: Vec<RouteTask>, rank: &BTreeMap<ItemId, usize>) -> Vec<RouteTask> {
    let (mut out, signal): (Vec<RouteTask>, Vec<RouteTask>) =
        tasks.into_iter().partition(|t| t.is_plane);
    let (mut ranked, rest): (Vec<RouteTask>, Vec<RouteTask>) =
        signal.into_iter().partition(|t| rank.contains_key(&t.item));
    ranked.sort_by_key(|t| rank[&t.item]);
    out.extend(ranked);
    out.extend(rest);
    out
}

/// The squeaky-wheel order of `record`: its pass-1 items, the plane tasks
/// first as they were, then the signal tasks of the failed nets, of the
/// victim nets and of the other nets, each group in pass 1's order.
pub fn squeaky_order(record: &PassOneRecord) -> Vec<ItemId> {
    let tier = |t: &RouteTask| {
        if t.is_plane {
            0
        } else if record.failed.contains(&t.net) {
            1
        } else if record.victims.contains(&t.net) {
            2
        } else {
            3
        }
    };
    let mut tasks: Vec<&RouteTask> = record.order.iter().collect();
    // A stable sort: each tier keeps pass 1's order.
    tasks.sort_by_key(|t| tier(t));
    tasks.into_iter().map(|t| t.item).collect()
}

impl NetOrdering for ArmOrdering {
    fn id(&self) -> &'static str {
        "portfolio-arm"
    }

    fn select(&mut self, board: &RoutingBoard) -> Vec<RouteTask> {
        let tasks = select_items(board);
        if self.first_pass_done {
            return tasks;
        }
        match &self.pass_one {
            PassOne::Shuffle(seed) => shuffled_order(tasks, *seed),
            PassOne::Items(rank) => ranked_order(tasks, rank),
        }
    }

    fn order(&mut self, tasks: Vec<RouteTask>, board: &RoutingBoard, pass: u32) -> Vec<RouteTask> {
        self.inner.reorder(tasks, board, pass)
    }

    fn on_pass_end(&mut self, report: &PassReport) {
        self.first_pass_done = true;
        self.inner.on_pass_end(report);
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

/// What an arm's pass 1 did.
#[derive(Clone, Debug, Default)]
pub struct PassOneRecord {
    /// Pass 1's task list, in the order it was routed.
    pub order: Vec<RouteTask>,
    /// The nets with a failed connection in pass 1.
    pub failed: BTreeSet<i32>,
    /// The other nets whose traces or vias a pass-1 connection ripped.
    pub victims: BTreeSet<i32>,
    /// True once pass 1 has ended.
    pub done: bool,
}

/// The record an arm's two recorders share (one thread only).
pub type SharedRecord = Rc<RefCell<PassOneRecord>>;

/// The arm's ordering, noting pass 1's list. Decides nothing.
pub struct RecordingOrdering {
    /// The arm's ordering.
    pub inner: Box<dyn NetOrdering>,
    /// The arm's record.
    pub record: SharedRecord,
}

impl NetOrdering for RecordingOrdering {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn select(&mut self, board: &RoutingBoard) -> Vec<RouteTask> {
        let tasks = self.inner.select(board);
        let mut record = self.record.borrow_mut();
        if !record.done && record.order.is_empty() {
            record.order = tasks.clone();
        }
        tasks
    }

    fn order(&mut self, tasks: Vec<RouteTask>, board: &RoutingBoard, pass: u32) -> Vec<RouteTask> {
        self.inner.order(tasks, board, pass)
    }

    fn on_pass_end(&mut self, report: &PassReport) {
        self.record.borrow_mut().done = true;
        self.inner.on_pass_end(report);
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

/// The arm's connection router: forwards the job's stop requests to the
/// arm's own token before every connection (an arm's own router stop, at
/// its pass limit, must not stop the other arms) and notes pass 1's hot
/// nets. The route itself is the inner router's.
pub struct RecordingRouter<'a> {
    /// The arm's connection router.
    pub inner: &'a mut dyn ConnectionRouter,
    /// The job's stop request.
    pub job: &'a CancelToken,
    /// The arm's record.
    pub record: SharedRecord,
    /// The nets of the board's traces and vias, by id, kept up to date
    /// with the items each connection adds.
    nets: BTreeMap<ItemId, Vec<i32>>,
}

impl<'a> RecordingRouter<'a> {
    /// A recorder over `inner` for an arm starting on `board`.
    pub fn new(
        inner: &'a mut dyn ConnectionRouter,
        job: &'a CancelToken,
        record: SharedRecord,
        board: &RoutingBoard,
    ) -> Self {
        let nets = board
            .items()
            .filter(|item| item.is_routable())
            .map(|item| (item.id(), item.net_numbers().to_vec()))
            .collect();
        RecordingRouter {
            inner,
            job,
            record,
            nets,
        }
    }

    fn note(&mut self, board: &RoutingBoard, task: &RouteTask, outcome: &ConnectionOutcome) {
        // The items are walked highest id first: the new ones come first.
        for item in board.items() {
            if item.id().0 < outcome.first_new_id {
                break;
            }
            if item.is_routable() {
                self.nets.insert(item.id(), item.net_numbers().to_vec());
            }
        }
        let mut record = self.record.borrow_mut();
        if record.done {
            return;
        }
        if outcome.state == AttemptState::Failed {
            record.failed.insert(task.net);
        }
        for id in &outcome.ripped {
            for net in self.nets.get(id).into_iter().flatten() {
                if *net > 0 && *net != task.net {
                    record.victims.insert(*net);
                }
            }
        }
    }
}

impl ConnectionRouter for RecordingRouter<'_> {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        // The job's stops only stop work.
        if self.job.is_stop_requested() {
            ctx.cancel.stop_all();
        } else if self.job.is_router_stop_requested() {
            ctx.cancel.stop_router();
        }
        let outcome = self.inner.route(board, task, p, ctx);
        self.note(board, task, &outcome);
        outcome
    }

    fn log_lines(&self) -> Vec<String> {
        self.inner.log_lines()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(item: i32, net: i32, is_plane: bool) -> RouteTask {
        RouteTask {
            item: ItemId(item),
            net,
            is_plane,
        }
    }

    #[test]
    fn the_arm_list_is_fixed() {
        let labels: Vec<String> = portfolio_arms(6).iter().map(Arm::label).collect();
        assert_eq!(
            labels,
            [
                "configured",
                "conflict-rounds",
                "inside-out",
                "shuffle-1",
                "shuffle-2",
                "shuffle-3"
            ]
        );
        assert_eq!(portfolio_arms(1), [Arm::Configured]);
        assert!(portfolio_arms(0).is_empty());
    }

    #[test]
    fn squeaky_order_puts_failed_then_victim_nets_first_and_keeps_planes_first() {
        let record = |failed: &[i32], victims: &[i32]| PassOneRecord {
            order: vec![
                task(9, 5, true),
                task(8, 1, false),
                task(7, 2, false),
                task(6, 3, false),
                task(5, 2, false),
            ],
            failed: failed.iter().copied().collect(),
            victims: victims.iter().copied().collect(),
            done: true,
        };
        let items =
            |r: &PassOneRecord| -> Vec<i32> { squeaky_order(r).iter().map(|i| i.0).collect() };
        assert_eq!(items(&record(&[3], &[2])), [9, 6, 7, 5, 8]);
        assert_eq!(items(&record(&[2, 3], &[])), [9, 7, 6, 5, 8]);
        // A failed net that is also a victim counts as failed.
        assert_eq!(items(&record(&[1], &[1, 3])), [9, 8, 6, 7, 5]);
        // Nothing hot: pass 1's order.
        assert_eq!(items(&record(&[], &[])), [9, 8, 7, 6, 5]);
    }

    #[test]
    fn ranked_order_follows_the_ranks_and_keeps_the_rest() {
        let tasks = vec![
            task(9, 5, true),
            task(8, 1, false),
            task(7, 2, false),
            task(6, 3, false),
        ];
        let rank = BTreeMap::from([(ItemId(6), 0), (ItemId(8), 1)]);
        let items: Vec<i32> = ranked_order(tasks, &rank)
            .iter()
            .map(|t| t.item.0)
            .collect();
        assert_eq!(items, [9, 6, 8, 7]);
    }

    #[test]
    fn a_shuffle_moves_whole_nets_and_depends_on_the_seed_only() {
        let tasks: Vec<RouteTask> = (0..20).map(|i| task(100 - i, 1 + i / 2, i == 0)).collect();
        let a = shuffled_order(tasks.clone(), 1);
        assert_eq!(a, shuffled_order(tasks.clone(), 1));
        assert_ne!(a, shuffled_order(tasks.clone(), 2));
        assert_eq!(a[0], tasks[0], "the plane task stays first");
        let mut sorted = a.clone();
        sorted.sort_by_key(|t| t.item);
        let mut expected = tasks.clone();
        expected.sort_by_key(|t| t.item);
        assert_eq!(sorted, expected, "a permutation");
        // The two tasks of a net stay together, in their order.
        for pair in a.windows(2).filter(|w| w[0].net == w[1].net) {
            assert!(pair[0].item.0 > pair[1].item.0, "{a:?}");
        }
    }
}

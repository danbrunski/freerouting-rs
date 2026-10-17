//! The `krt-astar` bundle (X06): Freerouting's ordering, ripup policy,
//! pass controller, plane taps and neck-down around the grid A* connection
//! router, with Freerouting's pull-tight of the changed area after each
//! routed connection, then Freerouting's optimizer.
//!
//! Decision D9's example of combining algorithms: KRT's paths are
//! octilinear, as Freerouting's 45° style, so the pull-tight (O01, Java
//! `TraceTightener45`) and the via optimizer it calls (O05, Java
//! `ViaOptimizer`) take them as input. There is nothing in Java to be at
//! parity with; X06 measures it.
//!
//! **Selection.** `router.autorouter.algorithm = krt-astar` picks
//! [`KRT_ASTAR_BUNDLE`]: every stage is Freerouting's except the
//! connection router, [`KrtAstar`] (also registered under
//! [`KRT_ASTAR_ID`], so `router.autorouter.connection_router = krt-astar`
//! selects it inside any bundle). The optimizer is chosen by
//! `router.optimizer.algorithm` as for every bundle, and runs when
//! `router.optimizer.enabled` is on. Without [`register`] the id is
//! unknown and Java's normalisation falls back to `freerouting-router`
//! with its warning (A10).
//!
//! **One connection** ([`KrtAstar::route`]): the `krt-grid` stage
//! ([`KrtGrid`], X05: 0.1, 0.05, then 0.025 mm, each path behind the exact
//! DRC); then, if it routed, [`RoutingBoard::opt_changed_area`] with
//! [`PullTightParams::after_connection`] at the pass's pull-tight accuracy
//! and trace costs, exactly as Freerouting's connection router does after
//! a routed connection (`AutorouteConnectionRouter.java:121-128` at
//! `a773a70ec`, `freerouting::autoroute::batch::connection`). The pass
//! loop marks the changed area before each connection and `krt-grid`'s
//! insert joins its trace corners into it, so the pull-tight covers the
//! new path, its vias (each at a trace corner) and the copper around it.
//! The outcome's work is the board meter's delta over the whole call,
//! pull-tight included, as for Freerouting's router. Like that router the
//! pull-tight runs without the cancel token (its 1,000 nominal ms work
//! limit bounds it).
//!
//! Not done here: the strict-DRC rollback of Freerouting's router (the
//! grid insert is already a DRC-checked transaction, and the pull-tight
//! only accepts moves that keep clearance), and ripup inside the stage
//! (the pass loop's ripup policy still runs between passes).
//!
//! Determinism (D2): [`KrtGrid`] is deterministic, and the pull-tight is
//! Freerouting's; nothing here reads a clock or iterates a hash.

use freerouting::autoroute::AttemptState;
use freerouting::autoroute::stages::{
    Bundle, ConnectionOutcome, ConnectionParams, ConnectionRouter, FREEROUTING_BUNDLE, RouteTask,
    StageCtx, StageRegistry, WorkStats,
};
use freerouting::board::RoutingBoard;
use freerouting::optimize::PullTightParams;
use freerouting::settings::RouterSettings;

use super::router::{GridRun, KrtGrid};

/// The bundle id (`router.autorouter.algorithm`) and the connection
/// router id (`router.autorouter.connection_router`).
pub const KRT_ASTAR_ID: &str = "krt-astar";

/// The `krt-astar` bundle: Freerouting's stages with [`KrtAstar`] as the
/// connection router.
pub const KRT_ASTAR_BUNDLE: Bundle = Bundle {
    connection_router: KRT_ASTAR_ID,
    ..FREEROUTING_BUNDLE
};

/// Registers the `krt-astar` bundle and its connection router.
pub fn register(registry: &mut StageRegistry) {
    registry.bundles.insert(KRT_ASTAR_ID, KRT_ASTAR_BUNDLE);
    registry
        .connection_router
        .insert(KRT_ASTAR_ID, |s| Box::new(KrtAstar::from_settings(s)));
}

/// The `krt-astar` connection router: `krt-grid`, then the pull-tight of
/// the changed area.
#[derive(Clone, Copy, Debug, Default)]
pub struct KrtAstar {
    /// The grid connection router.
    pub grid: KrtGrid,
}

impl KrtAstar {
    /// The router around `grid`.
    pub fn new(grid: KrtGrid) -> Self {
        KrtAstar { grid }
    }

    /// The router for a job (`krt-grid`'s settings).
    pub fn from_settings(settings: &RouterSettings) -> Self {
        KrtAstar::new(KrtGrid::from_settings(settings))
    }

    /// Routes `task` and returns the outcome with `krt-grid`'s attempt
    /// log.
    pub fn route_logged(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &StageCtx,
    ) -> GridRun {
        let meter_start = board.search_trees().work_meter().units();
        let mut run = self.grid.route_logged(board, task, p, ctx);
        if run.outcome.state == AttemptState::Routed {
            board.opt_changed_area(&PullTightParams::after_connection(
                p.pull_tight_accuracy,
                p.trace_costs.clone(),
            ));
            let meter = board.search_trees().work_meter();
            run.outcome.work = WorkStats {
                units: meter.units().wrapping_sub(meter_start),
            };
        }
        run
    }
}

impl ConnectionRouter for KrtAstar {
    fn id(&self) -> &'static str {
        KRT_ASTAR_ID
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        self.route_logged(board, task, p, ctx).outcome
    }
}

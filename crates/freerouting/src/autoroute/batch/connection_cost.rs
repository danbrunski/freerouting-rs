//! A per-connection cost in the maze search of the Freerouting connection
//! router (X11.3): the A04 cost hook ([`ExtraCost`]) reached from a
//! [`ConnectionRouter`](crate::autoroute::stages::ConnectionRouter).
//!
//! [`FreeroutingConnectionRouter::with_cost`](super::connection::FreeroutingConnectionRouter::with_cost)
//! takes a [`ConnectionCost`]. Before a connection's first attempt the
//! router asks it whether it applies ([`ConnectionCost::begin`]); if it
//! does, that attempt and its plane fallback search with the cost added to
//! every door section they enter. The neck-down retry searches without it.
//! Without a cost, or when `begin` declines, the router calls the search
//! exactly as before, monomorphized over [`NoExtraCost`]
//! (`crate::autoroute::session::sink`), so Freerouting's results do not
//! change.
//!
//! A cost must be finite and never negative, so the search's distance
//! estimate stays a lower bound; it must not read the board's search trees
//! (their work meter counts towards the connection's budget), and the same
//! board and task must give the same costs (D2).

use std::collections::BTreeSet;

use freerouting_geometry::{FloatPoint, PolylineShapeOps, TileShape};

use crate::autoroute::AttemptResult;
use crate::autoroute::connection::{Ripped, RouteLimits, route_connection_with_cost};
use crate::autoroute::control::AutorouteControl;
use crate::autoroute::path::RipupCosts;
use crate::autoroute::session::drills::DrillCarry;
use crate::autoroute::session::search::SearchSettings;
use crate::autoroute::session::sink::{ExtraCost, NoopSink, WorkCounters};
use crate::autoroute::stages::{ConnectionParams, RouteTask, StageCtx};
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::search::WorkLimit;

/// A cost added to the door sections a connection's search enters (module
/// docs).
pub trait ConnectionCost {
    /// A name for logs.
    fn id(&self) -> &'static str;
    /// Prepares the cost of `task` on `board` (before any search of the
    /// connection), with the pass's parameters `p` and the connection's
    /// `control`; true if it applies.
    fn begin(
        &mut self,
        board: &RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        control: &AutorouteControl,
    ) -> bool;
    /// The cost of entering a section of the door `door_shape` on `layer`
    /// at `entry`, coming from `from`: finite and at least 0.
    fn section_cost(
        &mut self,
        layer: i32,
        door_shape: &TileShape,
        from: FloatPoint,
        entry: FloatPoint,
    ) -> f64;
}

/// The A04 hook over a [`ConnectionCost`].
struct DynCost<'a>(&'a mut dyn ConnectionCost);

impl ExtraCost for DynCost<'_> {
    const ENABLED: bool = true;

    fn door_cost(&mut self, layer: i32, door_shape: &TileShape) -> f64 {
        let centre = door_shape.centre_of_gravity();
        self.0.section_cost(layer, door_shape, centre, centre)
    }

    fn section_cost(
        &mut self,
        layer: i32,
        door_shape: &TileShape,
        from: FloatPoint,
        entry: FloatPoint,
    ) -> f64 {
        self.0.section_cost(layer, door_shape, from, entry)
    }
}

/// [`super::connection`]'s `attempt` with `cost` in the search: one
/// `initAutoroute` plus `autorouteConnection` with `limit`, adding to the
/// caller's ripped set. While the run statistics are on (P01.2) the search
/// counts its work as `route_connection_counted` does.
#[allow(clippy::too_many_arguments)]
pub(crate) fn attempt_with_cost(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    control: &AutorouteControl,
    limit: &WorkLimit,
    ctx: &StageCtx,
    ripped: &mut BTreeSet<ItemId>,
    costs: &mut RipupCosts,
    cost: &mut dyn ConnectionCost,
) -> AttemptResult {
    let limits = RouteLimits {
        settings: SearchSettings::from_router(ctx.board_settings),
        stop: Some(ctx.cancel),
        work_limit: Some(limit.clone()),
    };
    let ripped_before = ripped.len();
    let mut engine = DrillCarry::default();
    if !crate::run_stats::enabled() {
        let ripped = Ripped {
            items: ripped,
            costs: Some(costs),
        };
        let (result, _) = route_connection_with_cost(
            board,
            start,
            dest,
            control,
            limits,
            ripped,
            NoopSink,
            &mut engine,
            DynCost(cost),
        );
        return result;
    }
    let (result, counters) = route_connection_with_cost(
        board,
        start,
        dest,
        control,
        limits,
        Ripped {
            items: &mut *ripped,
            costs: Some(costs),
        },
        WorkCounters::default(),
        &mut engine,
        DynCost(cost),
    );
    let added = ripped.len().saturating_sub(ripped_before);
    crate::run_stats::record_connection(&counters, result.state, added);
    result
}

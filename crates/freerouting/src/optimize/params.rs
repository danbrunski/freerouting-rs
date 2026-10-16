//! The parameters of a pull-tight run and Java's work budgets for it.
//!
//! Port of the arguments of `TraceTightener.getInstance` and
//! `RoutingBoard.optChangedArea` (`board/optimize/TraceTightener.java:87-114`,
//! `board/facade/RoutingBoard.java:151-191`), and of the pull-tight
//! `WorkLimit` constants of the call sites at the pin (`a773a70ec`; the
//! fork's `30baad7e2` replaced the former `TimeLimit`s).

use std::fmt;
use std::sync::Arc;

use freerouting_geometry::{IntOctagon, Point, PolylineShapeOps, Stoppable};

use crate::autoroute::control::ExpansionCostFactor;
use crate::board::search::UNITS_PER_NOMINAL_MS;

/// The pull-tight guard after every routed connection and in `removeTails`
/// (Java `AutorouteConnectionRouter.ENDLESS_LOOP_GUARD_NOMINAL_MS`,
/// `BatchAutorouter.ENDLESS_LOOP_GUARD_NOMINAL_MS`).
pub const ENDLESS_LOOP_GUARD_NOMINAL_MS: i64 = 1000;

/// The limit of `remove_items_and_pull_tight` (Java
/// `RoutingBoardOperations.PULL_TIGHT_LIMIT_NOMINAL_MS`).
pub const REMOVE_ITEMS_PULL_TIGHT_NOMINAL_MS: i64 = 2000;

/// The pull-tight limit of interactive routing (Java
/// `gui.interactive.Route.PULL_TIGHT_LIMIT_NOMINAL_MS`).
pub const ROUTE_PULL_TIGHT_NOMINAL_MS: i64 = 2000;

/// The pull-tight limit after moving a component (Java
/// `board.actions.MoveComponent.PULL_TIGHT_LIMIT_NOMINAL_MS`).
pub const MOVE_COMPONENT_PULL_TIGHT_NOMINAL_MS: i64 = 1000;

/// The work units a limit of `nominal_ms` allows (Java `WorkLimit`:
/// `nominal_ms × UNITS_PER_NOMINAL_MILLISECOND`; exceeded when the units
/// done are strictly greater).
pub const fn budget_units(nominal_ms: i64) -> u64 {
    nominal_ms as u64 * UNITS_PER_NOMINAL_MS
}

/// The region a pull-tight run may change.
///
/// Java passes an `IntOctagon` clip shape that may be `null` (no
/// restriction) or the `IntOctagon.EMPTY` instance, which
/// `RoutingBoardOperations.optChangedArea` compares by reference to skip
/// the tightener altogether. `IntOctagon.offset` normalizes its result and
/// `normalize` returns the `EMPTY` instance for an empty octagon, so an empty
/// tidy region enlarged by any width is still `EMPTY`:
/// [`Clip::from_tidy_region`] maps an empty octagon to `Skip`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Clip {
    /// No restriction (Java `null`).
    Unbounded,
    /// Don't pull tight at all (Java `IntOctagon.EMPTY`). A tightener built
    /// with it anyway clips with the empty octagon, as Java's does.
    Skip,
    /// Only corners inside the octagon move.
    Octagon(IntOctagon),
}

impl Clip {
    /// The clip of a tidy region computed as Java computes them (`EMPTY`,
    /// `union`, `enlarge`): `Skip` for an empty octagon.
    pub fn from_tidy_region(region: IntOctagon) -> Clip {
        if region.is_empty() {
            Clip::Skip
        } else {
            Clip::Octagon(region)
        }
    }

    /// The octagon the tightener's clip tests use; `None` for no clip.
    pub fn octagon(&self) -> Option<IntOctagon> {
        match self {
            Clip::Unbounded => None,
            Clip::Skip => Some(IntOctagon::EMPTY),
            Clip::Octagon(o) => Some(*o),
        }
    }
}

/// The arguments of a pull-tight run (Java `TraceTightener.getInstance` and
/// `optChangedArea`).
#[derive(Clone)]
pub struct PullTightParams {
    /// Only traces with exactly these nets are changed; empty for all
    /// (Java `onlyNetNoArr`).
    pub only_nets: Vec<i32>,
    /// The region corners may move in (Java `clipShape`).
    pub clip: Clip,
    /// The smallest translation tried; the tightener uses at least 100
    /// (Java `minTranslateDist`, the router's `tracePullTightAccuracy`).
    pub accuracy: i32,
    /// A point a trace through it must still contain afterwards, and its
    /// layer (Java `keepPoint`, `keepPointLayer`).
    pub keep_point: Option<(Point, i32)>,
    /// The work budget in nominal milliseconds (Java `workLimitMs`; `None`
    /// for Java's `≤ 0`). The limit starts counting when the tightener is
    /// built, as Java's `WorkLimit` starts in the `TraceTightener`
    /// constructor.
    pub limit: Option<i64>,
    /// The cancel token, checked before the limit (Java `stoppableThread`).
    pub stop: Option<Arc<dyn Stoppable + Send + Sync>>,
    /// The router's trace costs, one entry per layer (Java `traceCosts`).
    /// With costs, the scan also moves the vias in the changed area
    /// ([`RoutingBoard::opt_via_location`](crate::board::RoutingBoard::opt_via_location),
    /// O05); `None` skips them, as Java does with `traceCosts == null`.
    pub trace_costs: Option<Arc<[ExpansionCostFactor]>>,
}

impl PullTightParams {
    /// All nets, no clip, no keep point, no limit and no cancel token.
    pub fn new(accuracy: i32) -> Self {
        PullTightParams {
            only_nets: Vec::new(),
            clip: Clip::Unbounded,
            accuracy,
            keep_point: None,
            limit: None,
            stop: None,
            trace_costs: None,
        }
    }

    /// The pull-tight Java runs after each routed connection
    /// (`AutorouteConnectionRouter.java:126-132`, `:258-264`) and in
    /// `removeTails` (`BatchAutorouter.java:539-545`): all nets, no clip,
    /// the router's pull-tight `accuracy` and `trace_costs`, and the
    /// [`ENDLESS_LOOP_GUARD_NOMINAL_MS`] work limit. The caller sets
    /// [`stop`](Self::stop) to the router's cancel token; fanout and the
    /// interactive autoroute also set [`only_nets`](Self::only_nets) to the
    /// routed net (`RoutingBoard.java:975-981`, `:1136-1142`).
    pub fn after_connection(
        accuracy: i32,
        trace_costs: impl Into<Arc<[ExpansionCostFactor]>>,
    ) -> Self {
        PullTightParams {
            limit: Some(ENDLESS_LOOP_GUARD_NOMINAL_MS),
            trace_costs: Some(trace_costs.into()),
            ..PullTightParams::new(accuracy)
        }
    }
}

impl fmt::Debug for PullTightParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PullTightParams")
            .field("only_nets", &self.only_nets)
            .field("clip", &self.clip)
            .field("accuracy", &self.accuracy)
            .field("keep_point", &self.keep_point)
            .field("limit", &self.limit)
            .field("stop", &self.stop.is_some())
            .field("trace_costs", &self.trace_costs)
            .finish()
    }
}

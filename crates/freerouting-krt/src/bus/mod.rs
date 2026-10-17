//! Bus routing (X11): groups of nets that run side by side between the same
//! two places, routed middle out, each member drawn to its routed
//! neighbour.
//!
//! - [`detect`] (X11.1): two-pin connections whose start pins and end pins
//!   lie pairwise within 5 mm, grouped into greedy maximal cliques in
//!   net-number order, members in physical order.
//! - [`order`] (X11.2): the `krt-bus` net ordering, Freerouting's with the
//!   pass-1 list regrouped into bus blocks, middle member first.
//! - [`attraction`] (X11.3): the attraction as a non-negative maze-search
//!   cost rising with the distance from the routed neighbour's trace beyond
//!   one pitch, and the spacing measure.
//!
//! **Selection** (all off by default; unregistered or unselected, nothing
//! here runs and sessions stay Freerouting's):
//! - `router.autorouter.ordering = krt-bus`: the ordering alone;
//! - `router.autorouter.connection_router = krt-bus-attraction`:
//!   Freerouting's connection router with [`BusAttraction`];
//! - `router.autorouter.algorithm = krt-bus`: the bundle of both, every
//!   other stage Freerouting's.

use freerouting::autoroute::batch::connection::FreeroutingConnectionRouter;
use freerouting::autoroute::stages::{
    Bundle, ConnectionOutcome, ConnectionParams, ConnectionRouter, FREEROUTING_BUNDLE, RouteTask,
    StageCtx, StageRegistry,
};
use freerouting::board::RoutingBoard;
use freerouting::settings::RouterSettings;

pub mod attraction;
pub mod detect;
pub mod order;

pub use attraction::{AttractionParams, BusAttraction, Spacing, spacing};
pub use detect::{BusGroup, BusParams, Connection, detect};
pub use order::{BUS_ORDERING_ID, KrtBusOrdering, middle_out, routing_order};

/// The connection router id of Freerouting's router with the attraction.
pub const BUS_ATTRACTION_ID: &str = "krt-bus-attraction";

/// The bundle id (`router.autorouter.algorithm`).
pub const BUS_BUNDLE_ID: &str = "krt-bus";

/// The `krt-bus` bundle: the bus ordering and the attracted router.
pub const BUS_BUNDLE: Bundle = Bundle {
    ordering: BUS_ORDERING_ID,
    connection_router: BUS_ATTRACTION_ID,
    ..FREEROUTING_BUNDLE
};

/// Freerouting's connection router with [`BusAttraction`]
/// (`krt-bus-attraction`).
#[derive(Debug)]
pub struct KrtBusAttractionRouter {
    inner: FreeroutingConnectionRouter,
}

impl KrtBusAttractionRouter {
    /// The router with the plane-tap and neck-down policies `settings`
    /// select and the default attraction.
    pub fn from_settings(settings: &RouterSettings) -> Self {
        Self::with_params(settings, AttractionParams::default())
    }

    /// The router with the attraction `params`.
    pub fn with_params(settings: &RouterSettings, params: AttractionParams) -> Self {
        KrtBusAttractionRouter {
            inner: FreeroutingConnectionRouter::from_settings(settings)
                .with_cost(Box::new(BusAttraction::new(params))),
        }
    }
}

impl ConnectionRouter for KrtBusAttractionRouter {
    fn id(&self) -> &'static str {
        BUS_ATTRACTION_ID
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        self.inner.route(board, task, p, ctx)
    }
}

/// Registers the `krt-bus` ordering, the `krt-bus-attraction` connection
/// router and the `krt-bus` bundle.
pub(crate) fn register(registry: &mut StageRegistry) {
    registry
        .ordering
        .insert(BUS_ORDERING_ID, |_| Box::new(KrtBusOrdering::new()));
    registry.connection_router.insert(BUS_ATTRACTION_ID, |s| {
        Box::new(KrtBusAttractionRouter::from_settings(s))
    });
    registry.bundles.insert(BUS_BUNDLE_ID, BUS_BUNDLE);
}

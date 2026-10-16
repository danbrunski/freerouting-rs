//! The autorouter.
//!
//! Port of Java Freerouting's `app.freerouting.autoroute` (the maze search
//! that routes one connection) and `autoroute.pipeline` (the batch fanout,
//! routing and optimizer passes). The series plan is
//! `docs/roadmap/tasks/A00-autorouter-README.md`.
//!
//! What is here so far are the support types of the batch pipeline (A08):
//!
//! - [`history`]: `BoardHistory`, the bounded set of best boards a run can
//!   go back to;
//! - [`failure_log`]: `RoutingFailureLog`, the per-item failure counts that
//!   live on the [`RoutingBoard`](crate::board::RoutingBoard) and roll back
//!   with it;
//! - [`item_route_result`]: `ItemRouteResult`, the optimizer's ranking of a
//!   rerouted item;
//! - [`events`]: the router's progress events over a bounded channel, and
//!   the two-level [`CancelToken`](events::CancelToken) (Java's
//!   `StoppableThread`).
//!
//! The D9 stage pipeline (A10):
//!
//! - [`stages`]: the stage traits, the [`StageRegistry`](stages::StageRegistry)
//!   and the [`StageSelection`](stages::StageSelection) read from the
//!   settings;
//! - [`batch`]: the Freerouting stages' decision logic that needs no
//!   router: the pass controller, the item ordering with its seeded
//!   shuffle, the ripup policy, the fanout rules and the unrouted report.

pub mod batch;
pub mod events;
pub mod failure_log;
pub mod history;
pub mod item_route_result;
pub mod stages;

/// The possible results of autorouting a connection (Java
/// `AutorouteAttemptState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AttemptState {
    /// Unknown result.
    Unknown,
    /// The item was skipped.
    Skipped,
    /// The item has no unconnected nets.
    NoUnconnectedNets,
    /// The item is connected to a conduction plane.
    ConnectedToPlane,
    /// The item is already connected.
    AlreadyConnected,
    /// The item has no connections to nets.
    NoConnections,
    /// The item was routed.
    Routed,
    /// Routing failed.
    Failed,
    /// Inserting the found connection failed.
    InsertError,
}

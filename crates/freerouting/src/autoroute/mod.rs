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
//!
//! The search parameters and the probe harness (A01):
//!
//! - [`control`]: `AutorouteControl`, the per-net search parameters (layer
//!   activity, trace widths, via masks and radii, via costs);
//! - [`distance`]: `DestinationDistance`, the lower bound of the cost to
//!   the destination that orders the maze queue;
//! - [`java_ids`]: Java's `getId` hash formulas of rooms, doors, drills and
//!   drill pages, which break queue ties;
//! - [`probe`]: the line format of the Java probes
//!   (`tools/java-reference/probes/autoroute/AutorouteProbe.java`).
//!
//! The room graph of one connection search (A02):
//!
//! - [`session`]: the rooms, doors and maze queue of one search, the room
//!   database, the 45 degree neighbour calculation and the search
//!   initialisation (`MazeSearchEngine.getInstance`).

pub mod batch;
pub mod control;
pub mod distance;
pub mod events;
pub mod failure_log;
pub mod history;
pub mod item_route_result;
pub mod java_ids;
pub mod probe;
pub mod session;
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

impl AttemptState {
    /// The Java enum constant name (`AutorouteAttemptState.name()`).
    pub fn java_name(self) -> &'static str {
        match self {
            AttemptState::Unknown => "UNKNOWN",
            AttemptState::Skipped => "SKIPPED",
            AttemptState::NoUnconnectedNets => "NO_UNCONNECTED_NETS",
            AttemptState::ConnectedToPlane => "CONNECTED_TO_PLANE",
            AttemptState::AlreadyConnected => "ALREADY_CONNECTED",
            AttemptState::NoConnections => "NO_CONNECTIONS",
            AttemptState::Routed => "ROUTED",
            AttemptState::Failed => "FAILED",
            AttemptState::InsertError => "INSERT_ERROR",
        }
    }
}

/// The outcome of autorouting a connection: its state and a detail message
/// (Java `AutorouteAttemptResult`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptResult {
    pub state: AttemptState,
    pub details: String,
}

impl AttemptResult {
    /// A result with empty details.
    pub fn new(state: AttemptState) -> AttemptResult {
        AttemptResult {
            state,
            details: String::new(),
        }
    }

    /// A result with a detail message.
    pub fn with_details(state: AttemptState, details: impl Into<String>) -> AttemptResult {
        AttemptResult {
            state,
            details: details.into(),
        }
    }
}

impl std::fmt::Display for AttemptResult {
    /// Java's `toString`: `STATE: details`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.state.java_name(), self.details)
    }
}

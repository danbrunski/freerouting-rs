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
//!
//! Path location (A05):
//!
//! - [`path`]: the backtrack chain of a found connection, its ripped items
//!   and costs, and the 45 degree trace corners (`FoundConnectionLocator`,
//!   `FoundConnectionLocator45Degree`).
//!
//! Connection insertion (A06):
//!
//! - [`path::insert`], the traces and vias of a located connection
//!   (`FoundConnectionInserter`);
//! - [`connection`], with [`route_connection`](connection::route_connection):
//!   one connection searched, located, ripped and inserted
//!   (`AutorouteEngine.autorouteConnection`, `ripConnections`).
//!
//! The connection routing of the batch passes (A11):
//!
//! - [`batch::connection`]: the Freerouting connection router with its
//!   plane-tap and neck-down policies (`AutorouteConnectionRouter`);
//! - [`batch::strict_drc`]: the strict-DRC rejection and rollback;
//! - [`batch::pass`]: one routing pass (`AutoroutePassRunner.runSingleThread`),
//!   `remove_tails` and the optimizer's `route_passes_for_optimizer`.
//!
//! The fanout (A12):
//!
//! - [`fanout_pin`]: the escape of one SMD pin (`RoutingBoard.fanout`);
//! - [`batch::fanout`]: the fanout passes, `FreeroutingFanout`
//!   (`BatchFanout`).
//!
//! Beyond Java (Q13):
//!
//! - [`power_layer_traces`]: `router.power_layer_traces`, whether a plane
//!   net's trace may lie anywhere on its power layer (Java) or only inside
//!   its own plane there.
//!
//! The batch loop and the routing job (A13):
//!
//! - [`batch::batch_loop`]: `AutorouteBatchLoop.run`, with B01 snapshots in
//!   the history ([`batch::snapshot`]) and the last-mile ripup
//!   ([`batch::last_mile`]);
//! - [`pipeline`]: [`run_pipeline`](pipeline::run_pipeline), the job the
//!   CLI, the GUI and the plugin run (`RoutingPipeline.run`).
//!
//! Beyond Java (Q14):
//!
//! - [`via_clearance`]: `router.via_clearance`, whether a routed via takes
//!   its via type's clearance class (Java) or its net class's.

pub mod batch;
pub mod connection;
pub mod control;
pub mod distance;
pub mod events;
pub mod failure_log;
pub mod fanout_pin;
pub mod history;
pub mod item_route_result;
pub mod java_ids;
pub mod path;
pub mod pipeline;
pub mod power_layer_traces;
pub mod probe;
pub mod session;
pub mod stages;
pub mod via_clearance;
pub mod width_policy;

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
    /// Java's detail text. The insert probe compares it with Java's, so it
    /// keeps Java's `Item.toString` wording.
    pub details: String,
    /// The text for the log in place of `details`, when the port words it
    /// differently (K15.7: reference designators and pad numbers, not item
    /// and component ids). `None`: `details` is the log text.
    pub log_details: Option<String>,
}

impl AttemptResult {
    /// A result with empty details.
    pub fn new(state: AttemptState) -> AttemptResult {
        AttemptResult {
            state,
            details: String::new(),
            log_details: None,
        }
    }

    /// A result with a detail message.
    pub fn with_details(state: AttemptState, details: impl Into<String>) -> AttemptResult {
        AttemptResult {
            state,
            details: details.into(),
            log_details: None,
        }
    }

    /// This result with `text` as its log text.
    pub fn with_log_details(mut self, text: String) -> AttemptResult {
        self.log_details = Some(text);
        self
    }

    /// What the log and the failure log show: [`Self::log_details`] if set,
    /// else [`Self::details`].
    pub fn log_text(&self) -> &str {
        self.log_details.as_deref().unwrap_or(&self.details)
    }
}

impl std::fmt::Display for AttemptResult {
    /// Java's `toString`: `STATE: details`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.state.java_name(), self.details)
    }
}

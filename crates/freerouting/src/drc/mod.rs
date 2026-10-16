//! Design rule checking and connectivity.
//!
//! Port of `app.freerouting.drc`: clearance violations
//! ([`ClearanceViolation`], [`BasicBoard::clearance_violations`]),
//! incomplete connections ([`NetIncompletes`], [`AirLine`]), unconnected
//! items, copper islands of conduction areas and the
//! [`DesignRulesChecker`] with its KiCad JSON report ([`DrcReport`]) and
//! diagnostic summary ([`DrcSummary`]).
//!
//! [`BasicBoard::clearance_violations`]: crate::board::BasicBoard::clearance_violations

mod checker;
mod clearance;
mod incompletes;
pub mod json;
mod ledger;
mod report;
mod runner;
mod summary;
mod unconnected;
mod zone_islands;

pub use checker::{DesignRulesChecker, DrcFindings};
pub use clearance::{ClearanceViolation, ClearanceViolationCategory, aggregate_sorted_by_severity};
pub use incompletes::{AirLine, NetIncompletes};
pub use ledger::RoutingLedger;
pub use report::{DrcPosition, DrcReport, DrcViolation, DrcViolationItem, FREEROUTING_VERSION};
pub use runner::{DrcRun, DrcRunOptions, load_design, run_drc};
pub use summary::{CongestionZone, DiagnosticViolation, DrcSummary};
pub use unconnected::{UnconnectedItems, UnconnectedKind};
pub use zone_islands::{Island, Trapezoid, ZoneIslandKind, ZoneIslandViolation, fill_islands};

/// Ordered parallel map, shared with the feasibility diagnostic (Q03).
pub(crate) use clearance::parallel_map;

// B08.3: the route stubs list of the GUI.
pub mod route_stubs;
pub use route_stubs::{RouteStub, RouteStubKind, route_stubs};

//! Design rule checking and connectivity.
//!
//! Port of `app.freerouting.drc`: clearance violations
//! ([`ClearanceViolation`], [`BasicBoard::clearance_violations`]),
//! incomplete connections ([`NetIncompletes`], [`AirLine`]), unconnected
//! items, copper islands of conduction areas and the
//! [`DesignRulesChecker`] with its KiCad JSON report ([`DrcReport`]) and
//! diagnostic summary ([`DrcSummary`]). The report counts clearance, hole
//! clearance, dangling items, split nets and zone islands as KiCad does
//! unless asked for Java's ([`ReportRules`], K15.5, K15.11, K15.16), with the
//! KiCad rules and zone fills of a `.kicad_pcb` ([`KicadReportRules`]).
//!
//! [`BasicBoard::clearance_violations`]: crate::board::BasicBoard::clearance_violations

mod checker;
mod clearance;
mod incompletes;
pub mod json;
mod kicad_connectivity;
mod kicad_copper;
mod kicad_report;
mod kicad_rules;
mod ledger;
mod report;
mod runner;
mod summary;
mod unconnected;
mod zone_islands;

pub use checker::{DesignRulesChecker, DrcFindings};
pub use clearance::{ClearanceViolation, ClearanceViolationCategory, aggregate_sorted_by_severity};
pub use incompletes::{AirLine, NetIncompletes};
pub use kicad_report::ReportRules;
pub use kicad_rules::{
    BoardHole, BoardZone, IslandRemoval, KicadReportInputs, KicadReportRules,
    unconditional_clearance_nm,
};
pub use ledger::RoutingLedger;
pub use report::{DrcPosition, DrcReport, DrcViolation, DrcViolationItem, FREEROUTING_VERSION};
pub use runner::{DrcRun, DrcRunOptions, load_design, run_drc};
pub use summary::{CongestionZone, DiagnosticViolation, DrcSummary};
pub use unconnected::{UnconnectedItems, UnconnectedKind};
pub use zone_islands::{Island, Trapezoid, ZoneIslandKind, ZoneIslandViolation, fill_islands};

pub(crate) use checker::counts_clearance_violation;
/// Ordered parallel map, shared with the feasibility diagnostic (Q03).
pub(crate) use clearance::parallel_map;

// B08.3: the route stubs list of the GUI.
pub mod route_stubs;
pub use route_stubs::{RouteStub, RouteStubKind, route_stubs};

// B09.3: the fill model of conduction areas and own-net connectivity.
pub mod fill_model;
pub use fill_model::{FillIsland, FillPolygon, conduction_fill, island_outline, net_components};

// Q20.1: the board's manufacturing rules on routed and input copper.
pub mod board_rules;
pub use board_rules::{
    BoardRule, BoardRuleCounts, BoardRuleLimits, BoardRuleViolation, InputCopper, Origin,
    check_board_rules,
};

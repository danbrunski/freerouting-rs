//! Plane nets (X02): islands and main components, plane-tap verification,
//! and pour region repair.
//!
//! - [`components`]: the islands of each filled plane (Java's detailed fill),
//!   their anchors, the main island and the fill-aware electrical groups;
//! - [`verify`]: which plane-net pins reach the main component through their
//!   via (D8), a report that changes nothing (`krt-plane-tap-report`);
//! - [`repair`]: straps that re-join a net's split pour regions, a board
//!   stage after the routing passes (`krt-region-repair`).
//!
//! Written from KiCadRoutingTools' ideas (`plane_component_oracle.py`,
//! `repair_planes.py`, `plane_region_connector.py` at `0486e799`); the
//! routing is Freerouting's connection router, and no KRT code is ported.

use freerouting::autoroute::stages::StageRegistry;

pub mod components;
pub mod repair;
pub mod verify;

#[cfg(test)]
mod test_boards;

pub use components::{PlaneArea, PlaneComponents, PlaneIsland, Region};

/// Registers the plane stages.
pub(crate) fn register(registry: &mut StageRegistry) {
    registry
        .board_stages
        .insert(verify::TAP_REPORT_ID, |_| Box::new(verify::TapReportStage));
    registry
        .board_stages
        .insert(repair::REGION_REPAIR_ID, |_| Box::new(repair::RegionRepair));
}

//! Plane nets (X02): islands and main components and plane-tap
//! verification.
//!
//! - [`components`]: the islands of each filled plane (Java's detailed fill),
//!   their anchors, the main island and the fill-aware electrical groups;
//! - [`verify`]: which plane-net pins reach the main component through their
//!   via (D8), a report that changes nothing (`krt-plane-tap-report`).
//!
//! X02.3's region repair (`krt-region-repair`: straps re-joining a net's
//! split pour regions) was removed after X02.5's registered run (2026-10-17):
//! its straps added KiCad DRC items (three shorts on pic_programmer, dangling
//! tracks and starved thermals on complex_hierarchy).
//!
//! Written from KiCadRoutingTools' ideas (`plane_component_oracle.py` at
//! `0486e799`); no KRT code is ported.

use freerouting::autoroute::stages::StageRegistry;

pub mod components;
pub mod verify;

#[cfg(test)]
mod test_boards;

pub use components::{PlaneArea, PlaneComponents, PlaneIsland, Region};

/// Registers the plane stages.
pub(crate) fn register(registry: &mut StageRegistry) {
    registry
        .board_stages
        .insert(verify::TAP_REPORT_ID, |_| Box::new(verify::TapReportStage));
}

//! Routing stages drawn from KiCadRoutingTools (KRT) for the freerouting-rs
//! pipeline (decision D9, the X series).
//!
//! Each stage implements a trait of [`freerouting::autoroute::stages`] and is
//! registered by [`register`] under its id. The CLI and the GUI call
//! [`register`] once on their [`StageRegistry`]; a job then selects a stage
//! with A10's settings keys. Nothing here runs unless a job selects it, so
//! default results stay Freerouting's.
//!
//! Licence: GPL-3.0-only for code written here, MIT for files carrying KRT
//! code structure (each with the F04 SPDX header and a `docs/PROVENANCE.md`
//! row); see the crate's `README.md`.
//!
//! Modules:
//! - [`planes`] (X02): islands, anchors and main components of plane nets,
//!   the plane-tap report and pour region repair.
//! - [`grid`] (X04, X05, X06): the conservative obstacle raster of one
//!   connection for the vendored grid A* (`krt-grid-router`), the
//!   `krt-grid` connection router on it, and the `krt-astar` bundle (grid
//!   path, then Freerouting's pull-tight and optimizer) with the region
//!   classes that measure it.
//! - [`length`] (X07): length and time matching with trombone meanders,
//!   the `krt-meander` board stage.
//! - [`diffpair`] (X08): differential pairs, centreline and P/N offset, the
//!   `krt-diffpair` board stage.

use freerouting::autoroute::stages::StageRegistry;

pub mod diffpair;
pub mod grid;
pub mod length;
pub mod planes;

/// Registers this crate's stages on `registry`. Each X checklist appends
/// its own registration lines here.
pub fn register(registry: &mut StageRegistry) {
    planes::register(registry);
    grid::register(registry);
    grid::bundle::register(registry);
    length::register(registry);
    diffpair::register(registry);
}

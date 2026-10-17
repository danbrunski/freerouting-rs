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
//! - [`grid`] (X04): the conservative obstacle raster of one connection for
//!   the vendored grid A* (`krt-grid-router`). No stage yet; X05 adds
//!   `krt-grid`.

use freerouting::autoroute::stages::StageRegistry;

pub mod grid;
pub mod planes;

/// Registers this crate's stages on `registry`. Each X checklist appends
/// its own registration lines here.
pub fn register(registry: &mut StageRegistry) {
    planes::register(registry);
}

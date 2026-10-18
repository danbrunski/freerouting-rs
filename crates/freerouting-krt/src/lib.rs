//! Routing stages drawn from KiCadRoutingTools (KRT) for the freerouting-rs
//! pipeline (decision D9, the X series).
//!
//! Each stage implements a trait of [`freerouting::autoroute::stages`] and is
//! registered by [`register`] under its id. The `freerouting-stages` crate
//! calls [`register`] once on a [`StageRegistry`] (this crate depends on the
//! engine, so the engine cannot) and the CLI and the GUI route with that
//! registry (N05.1); a job then selects a stage with A10's settings keys. Nothing here runs unless a job selects it, so
//! default results stay Freerouting's.
//!
//! Licence: GPL-3.0-only for code written here, MIT for files carrying KRT
//! code structure (each with the F04 SPDX header and a `docs/PROVENANCE.md`
//! row); see the crate's `README.md`.
//!
//! Modules:
//! - [`planes`] (X02): islands, anchors and main components of plane nets,
//!   and the plane-tap report (X02.3's region repair was rejected by X02.5
//!   and removed).
//! - [`grid`] (X04, X05, X06): the conservative obstacle raster of one
//!   connection for the vendored grid A* (`krt-grid-router`), the
//!   `krt-grid` connection router on it, and the region classes X06
//!   measured with (X06.1's `krt-astar` bundle was rejected by X06.3 and
//!   removed).
//! - [`length`] (X07): length and time matching with trombone meanders,
//!   the `krt-meander` board stage.
//! - [`diffpair`] (X08, X09): differential pairs, centreline and P/N
//!   offset, coupled via pairs, polarity, skew and the optimizer exclusion,
//!   the `krt-diffpair` board stage.
//! - [`fanout`] (X10, X12): the `krt-qfn-escape` and `krt-bga-escape`
//!   fanout stages.
//!
//! X11's bus ordering and attraction (`krt-bus`) were rejected by X11.4's
//! registered run and removed, with the engine's search-cost hook.

use freerouting::autoroute::stages::StageRegistry;

pub mod diffpair;
pub mod fanout;
pub mod grid;
pub mod length;
pub mod planes;

/// Registers this crate's stages on `registry`. Each X checklist appends
/// its own registration lines here.
pub fn register(registry: &mut StageRegistry) {
    planes::register(registry);
    grid::register(registry);
    length::register(registry);
    diffpair::register(registry);
    fanout::register(registry);
}

//! The Freerouting engine: board model, rules and DSN board builder.
//!
//! Port of the board model of the Java Freerouting autorouter
//! (`app.freerouting.rules`, `core.library`, `board.*`, parts of
//! `datastructures`, `core.scoring.BoardStatistics`) and of the board-building
//! half of `io.specctra.DsnReader`.

pub use freerouting_dsn as dsn;
pub use freerouting_geometry as geometry;

pub mod board;
pub mod datastructures;
pub mod drc;
pub mod io;
pub mod library;
pub mod rules;
pub mod scoring;
mod util;

//! The Freerouting engine: board model, rules and DSN board builder.
//!
//! Port of the board model of the Java Freerouting autorouter
//! (`app.freerouting.rules`, `core.library`, `board.*`, parts of
//! `datastructures`, `core.scoring.BoardStatistics`) and of the board-building
//! half of `io.specctra.DsnReader`.

pub use freerouting_dsn as dsn;
pub use freerouting_geometry as geometry;

pub mod autoroute;
pub mod board;
pub mod build_info;
pub mod datastructures;
pub mod drc;
pub mod interactive;
pub mod io;
pub mod job;
pub mod library;
pub mod optimize;
pub mod quality;
pub mod rules;
pub mod run_stats;
pub mod scoring;
pub mod settings;
pub mod shove;
mod util;

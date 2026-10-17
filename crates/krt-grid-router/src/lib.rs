// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)

//! KiCadRoutingTools' grid router, vendored.
//!
//! This crate is the Rust core of KiCadRoutingTools (KRT, MIT, Copyright (c)
//! 2026 drandyhaas) `rust_router/` (crate `grid_router` 0.23.0) at commit
//! `0486e799`, with the Python binding (extension-module glue,
//! array parameters) and the global allocator removed and nothing else changed. It holds the grid obstacle
//! map, the octilinear grid A* ([`GridRouter`]) and the pose A* with a Dubins
//! heuristic ([`PoseRouter`]). `VENDORED.diff` is the record of every change
//! against KRT; `tools/krt/vendor-diff.sh` regenerates it.
//!
//! Not vendored: `lane_search.rs` (used only by KRT's `awx/` planner),
//! `identify_blocking_obstacles` and the `#[pymodule]` (Python glue).
//!
//! The vendored files keep KRT's formatting (`#[rustfmt::skip]` on each
//! module) so a later KRT version can be re-vendored by diff, and the
//! `#![allow]` list below names the clippy lints that code trips.

// Lints the vendored code trips under the workspace's `clippy -D warnings`.
// Each is named; none of KRT's lines were edited to satisfy them.
#![allow(
    clippy::needless_range_loop,
    clippy::field_reassign_with_default,
    clippy::collapsible_if,
    clippy::doc_lazy_continuation,
    clippy::identity_op,
    clippy::manual_clamp,
    clippy::manual_div_ceil,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unnecessary_cast,
    clippy::unnecessary_map_or,
    clippy::unwrap_or_default
)]

#[rustfmt::skip]
mod types;
#[rustfmt::skip]
pub mod obstacle_map;
#[rustfmt::skip]
pub mod router;
#[rustfmt::skip]
mod dubins;
#[rustfmt::skip]
pub mod pose_router;

pub use obstacle_map::GridObstacleMap;
pub use pose_router::PoseRouter;
pub use router::{GridRouter, SearchStats, TrackMarginArg};

//! The obstacle raster of the grid connection router (X04): board items
//! turned into a conservative [`GridObstacleMap`] for one connection.
//!
//! Written from the design of KiCadRoutingTools' raster
//! (`py_router/obstacle_map.py`, `obstacle_cache.py` at `0486e799`): items
//! inflated by clearance plus the routed half-width, a separate via map,
//! keepouts and terminal cells. No KRT code is ported; the inflation here is
//! an exact point-to-shape distance test on the board's own shapes.
//!
//! **Conservative.** With `h = ceil(s·√2 / 2)` board units for step `s`, a
//! cell is blocked on a layer when the distance from its centre to a foreign
//! item's shape is less than `c + hw + h` (clearance with the safety margin
//! plus the net's half-width plus `h`). Every point of a move between two
//! adjacent (also diagonal) free centres is within `h` of one of them, so it
//! is at least `c + hw` from every foreign item. The exact DRC of the stage
//! (X05) is still the judge; the raster only never invites a violation.
//!
//! **One raster per connection, from scratch** ([`raster::build`]), in a
//! window around the connection ([`frame::window_for`]). The grid origin is
//! fixed (cell `(i, j)` has its centre at `(i·s, j·s)`), so the windows of
//! two connections agree where they overlap. Grid coordinates handed to the
//! vendored map are these absolute cell indices.
//!
//! Nothing in the default pipeline uses this module. The `krt-grid` stage
//! ([`router`], X05) routes one connection on the raster and inserts the
//! path ([`path`]) behind the exact DRC; [`register`] adds it to a registry,
//! where `router.autorouter.connection_router = krt-grid` selects it.
//! [`regions`] (X06) classes a board's connections as `pin-field` or
//! `open`. X06.1's `krt-astar` bundle (this router, then Freerouting's
//! pull-tight of the changed area) was rejected by X06.3's registered run
//! and removed.

use std::collections::BTreeSet;

use freerouting::autoroute::stages::StageRegistry;
use freerouting::board::BasicBoard;
use freerouting::board::items::ItemId;
use freerouting::job::overrides::um_to_board_units;

pub mod frame;
pub mod frontier;
pub mod path;
pub mod raster;
pub mod regions;
pub mod router;
pub mod terminals;

pub use krt_grid_router::GridObstacleMap;

/// The settings key of the grid step in µm (read by the `krt-grid` stage,
/// X05).
pub const GRID_STEP_KEY: &str = "router.krt.grid_step_um";

/// The default grid step in µm (KRT `routing_defaults.GRID_STEP`, 0.1 mm).
pub const DEFAULT_GRID_STEP_UM: u32 = 100;

/// The finer steps KRT uses for plane taps and for rescue routing, in µm.
pub const FINER_GRID_STEPS_UM: [u32; 2] = [50, 25];

/// How far the window reaches beyond the connection's items, in µm.
pub const WINDOW_MARGIN_UM: f64 = 5000.0;

/// One connection to rasterize: the net routed, the items the path starts
/// from and the items it may end on (all of the routed net).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridTask {
    /// The net being routed.
    pub net: i32,
    /// The connection's start items (sources).
    pub start: BTreeSet<ItemId>,
    /// The connection's destination items (targets).
    pub dest: BTreeSet<ItemId>,
}

impl GridTask {
    /// The start and destination items, ascending by id.
    pub fn items(&self) -> Vec<ItemId> {
        self.start.union(&self.dest).copied().collect()
    }
}

/// Registers the `krt-grid` connection router (X05).
pub fn register(registry: &mut StageRegistry) {
    registry.connection_router.insert(router::KRT_GRID_ID, |s| {
        Box::new(router::KrtGrid::from_settings(s))
    });
}

/// A grid step of `step_um` µm in board units, at least 1.
pub fn step_in_board_units(board: &BasicBoard, step_um: u32) -> i32 {
    um_to_board_units(board, f64::from(step_um)).max(1)
}

/// `h = ceil(s·√2 / 2)` for step `s`, computed exactly: the smallest
/// integer with `2·h² ≥ s²`.
pub fn half_diagonal(step: i32) -> i32 {
    let s = i64::from(step.max(0));
    let target = s * s;
    // The float estimate is within one of the answer; the loops make it
    // exact.
    let mut h = ((s as f64) * std::f64::consts::FRAC_1_SQRT_2).ceil() as i64;
    while h > 0 && 2 * (h - 1) * (h - 1) >= target {
        h -= 1;
    }
    while 2 * h * h < target {
        h += 1;
    }
    h as i32
}

#[cfg(test)]
mod tests {
    use super::half_diagonal;

    #[test]
    fn half_diagonal_is_the_exact_ceiling() {
        for s in 0..20_000i64 {
            let h = i64::from(half_diagonal(s as i32));
            assert!(2 * h * h >= s * s, "s {s} h {h}");
            assert!(h == 0 || 2 * (h - 1) * (h - 1) < s * s, "s {s} h {h}");
        }
        // 0.1 mm at 10 units per µm: 1000·√2/2 = 707.1…
        assert_eq!(half_diagonal(1000), 708);
        let big = i64::from(i32::MAX / 2);
        let h = i64::from(half_diagonal(i32::MAX / 2));
        assert!(2 * h * h >= big * big && 2 * (h - 1) * (h - 1) < big * big);
    }
}

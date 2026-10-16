//! The forced-trace check (O03.7).
//!
//! Port of `RoutingBoard.checkForcedTracePolyline` (Java at `a773a70ec`):
//! could a trace along a polyline go on a layer if the traces and vias in
//! the way were shoved aside? The maze search asks it for every door it
//! wants to pass with ripup off. Java passes no work limit here.

use freerouting_geometry::{Polyline, PolylineShapeOps, TileShape};

use crate::board::RoutingBoard;
use crate::board::structure::AngleRestriction;

use super::entry_side::ShapeEntrySide;
use super::trace_check::TraceShover;

impl RoutingBoard {
    /// Java `RoutingBoard.checkForcedTracePolyline(polyline, halfWidth,
    /// layer, netNumbers, clearanceClassIndex, maxRecursionDepth,
    /// maxViaRecursionDepth, maxSpringOverRecursionDepth)`: shove-checks the
    /// offset shapes of `polyline` with the compensated half width one by
    /// one (their bounding boxes in 90 degree mode), each entered from the
    /// side where the polyline enters it. False at the first failing shape.
    #[allow(clippy::too_many_arguments)]
    pub fn check_forced_trace_polyline(
        &mut self,
        polyline: &Polyline,
        half_width: i32,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        max_spring_over_recursion_depth: i32,
    ) -> bool {
        let compensated_half_width = half_width
            + self
                .search_trees()
                .default_tree()
                .clearance_compensation_value(&self.rules.clearance_matrix, clearance_class, layer);
        let line_count = polyline.lines().len();
        let trace_shapes =
            polyline.offset_shapes_range(compensated_half_width, 0, line_count.saturating_sub(1));
        let orthogonal = self.rules.trace_angle_restriction() == AngleRestriction::NinetyDegree;
        let mut shover = TraceShover::new(self);
        for (i, shape) in trace_shapes.iter().enumerate() {
            let current = if orthogonal {
                TileShape::IntBox(shape.bounding_box())
            } else {
                shape.clone()
            };
            let from_side = ShapeEntrySide::from_polyline(polyline, i + 1, &current);
            if !shover.check(
                &current,
                Some(from_side),
                None,
                layer,
                net_numbers,
                clearance_class,
                max_recursion_depth,
                max_via_recursion_depth,
                max_spring_over_recursion_depth,
                None,
            ) {
                return false;
            }
        }
        true
    }
}

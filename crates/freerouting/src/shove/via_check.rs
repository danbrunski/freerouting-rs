//! Via checks (O03.7).
//!
//! Port of the check half of `board.actions.ForcedViaInserter` (Java at
//! `a773a70ec`): `checkLayer` ([`TraceShover::check_via_layer`], what the
//! maze search asks before it expands through a drill), `check`
//! ([`TraceShover::check_via`], what the connection inserter asks before it
//! places a via), `holeCheckShape` and `calculateFromSide`.
//!
//! Both pass Java's `null` for the work limit and the ignore items of
//! `checkForcedPad`; a failed [`check_via`](TraceShover::check_via) sets the
//! board's shove-failing layer, as in Java.

use freerouting_geometry::jmath::d2i;
use freerouting_geometry::limits::SQRT2;
use freerouting_geometry::{
    Circle, FloatPoint, IntOctagon, Point, PolylineShapeOps, Shape, Simplex, TileShape,
    TileShapeOps,
};

use crate::board::RoutingBoard;
use crate::board::structure::AngleRestriction;
use crate::library::Padstack;
use crate::rules::ViaInfo;

use super::drill_check::CheckDrillResult;
use super::entry_side::ShapeEntrySide;
use super::trace_check::TraceShover;

/// The depths of a shove check: Java's `maxRecursionDepth` and
/// `maxViaRecursionDepth`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShoveDepths {
    pub recursion: i32,
    pub via_recursion: i32,
}

impl TraceShover<'_> {
    /// Java `ForcedViaInserter.checkLayer(viaRadius, clearanceClassIndex,
    /// attachSmdAllowed, roomShape, location, layer, netNumbers,
    /// maxRecursionDepth, maxViaRecursionDepth, board, traceHalfWidth,
    /// traceClearanceClass)`: is a via of radius `via_radius` possible at
    /// `location` on `layer` after shoving the traces in the way aside? The
    /// shove comes from the first side of the via shape that leads into
    /// `room_shape`; with a positive `trace_half_width` the start of a trace
    /// of that width is checked too.
    #[allow(clippy::too_many_arguments)]
    pub fn check_via_layer(
        &mut self,
        via_radius: f64,
        clearance_class: i32,
        attach_smd_allowed: bool,
        room_shape: &TileShape,
        location: &Point,
        layer: i32,
        net_numbers: &[i32],
        depths: ShoveDepths,
        trace_half_width: i32,
        trace_clearance_class: i32,
    ) -> CheckDrillResult {
        if via_radius <= 0.0 {
            return CheckDrillResult::Drillable;
        }
        let &Point::Int(int_location) = location else {
            return CheckDrillResult::NotDrillable;
        };
        let via_shape = Circle::new(int_location, d2i(via_radius.ceil()));
        let check_radius = via_radius
            + 0.5
                * f64::from(
                    self.board
                        .clearance_value(clearance_class, clearance_class, layer),
                )
            + f64::from(self.board.min_trace_half_width());
        let ninety = is_ninety_degree(self.board);
        let tile_shape = circle_tile(&via_shape, ninety);
        let Some(from_side) = calculate_from_side(
            int_location.to_float(),
            &tile_shape,
            &room_shape.to_simplex(),
            check_radius,
            ninety,
        ) else {
            return CheckDrillResult::NotDrillable;
        };
        let via_result = self.check_forced_pad(
            &tile_shape,
            Some(from_side),
            layer,
            net_numbers,
            clearance_class,
            attach_smd_allowed,
            &[],
            depths.recursion,
            depths.via_recursion,
            false,
            None,
        );
        if via_result == CheckDrillResult::NotDrillable || trace_half_width <= 0 {
            return via_result;
        }
        let start_trace_shape = circle_tile(&Circle::new(int_location, trace_half_width), ninety);
        let trace_result = self.check_forced_pad(
            &start_trace_shape,
            Some(from_side),
            layer,
            net_numbers,
            trace_clearance_class,
            true,
            &[],
            depths.recursion,
            depths.via_recursion,
            false,
            None,
        );
        if trace_result == CheckDrillResult::NotDrillable {
            return trace_result;
        }
        if via_result == CheckDrillResult::DrillableWithAttachSmd
            || trace_result == CheckDrillResult::DrillableWithAttachSmd
        {
            return CheckDrillResult::DrillableWithAttachSmd;
        }
        CheckDrillResult::Drillable
    }

    /// Java `ForcedViaInserter.check(viaInfo, location, netNumbers,
    /// maxRecursionDepth, maxViaRecursionDepth, board, tracePenHalfwidthArr,
    /// traceClearanceClassIndex)`: is a via of type `via_info` possible at
    /// `location` after shoving the traces in the way aside, on every layer
    /// of its padstack? Copper-less layers check the drill hole with the
    /// hole clearance, pad layers the hole as well; `trace_pen_half_widths`
    /// (by layer) adds the start of a trace. On failure the board's
    /// shove-failing layer is the failing layer.
    pub fn check_via(
        &mut self,
        via_info: &ViaInfo,
        location: &Point,
        net_numbers: &[i32],
        depths: ShoveDepths,
        trace_pen_half_widths: Option<&[i32]>,
        trace_clearance_class: i32,
    ) -> bool {
        let translate_vector = location.difference_by(&Point::ZERO);
        let calc_from_side_offset = self.board.min_trace_half_width();
        let Some(padstack) = self.board.library.padstacks.get(via_info.padstack).cloned() else {
            return false;
        };
        let hole_shape = hole_check_shape(self.board, &padstack, location);
        let ninety = is_ninety_degree(self.board);
        for layer in padstack.from_layer()..=padstack.to_layer() {
            let (pad_shape, class) = match padstack.get_shape(layer) {
                Some(shape) => (
                    shape.to_shape().translate_by(&translate_vector),
                    via_info.clearance_class,
                ),
                None => match &hole_shape {
                    // The drill hole itself keeps the hole clearance from
                    // the copper on this layer.
                    Some(hole) => (hole.clone(), 0),
                    None => continue,
                },
            };
            let tile_shape = shape_tile(&pad_shape, ninety);
            let from_side =
                self.calc_from_side(&tile_shape, location, layer, calc_from_side_offset, class);
            if !self.forced_pad_ok(
                &tile_shape,
                from_side,
                layer,
                net_numbers,
                class,
                via_info.attach_smd_allowed,
                depths,
            ) {
                return false;
            }
            if class != 0
                && let Some(hole) = &hole_shape
            {
                // The hole also keeps the (larger) hole clearance from the
                // foreign copper of the layers with a pad.
                let hole_tile = shape_tile(hole, ninety);
                if !self.forced_pad_ok(
                    &hole_tile,
                    from_side,
                    layer,
                    net_numbers,
                    0,
                    via_info.attach_smd_allowed,
                    depths,
                ) {
                    return false;
                }
            }
            let pen = trace_pen_half_widths
                .and_then(|widths| usize::try_from(layer).ok().and_then(|l| widths.get(l)))
                .copied()
                .unwrap_or(0);
            if pen > 0
                && let &Point::Int(trace_point) = location
            {
                let start_trace_shape = circle_tile(&Circle::new(trace_point, pen), ninety);
                if !self.forced_pad_ok(
                    &start_trace_shape,
                    from_side,
                    layer,
                    net_numbers,
                    trace_clearance_class,
                    true,
                    depths,
                ) {
                    return false;
                }
            }
        }
        true
    }

    /// One `checkForcedPad` of [`check_via`](Self::check_via); sets the
    /// shove-failing layer when it is not drillable.
    #[allow(clippy::too_many_arguments)]
    fn forced_pad_ok(
        &mut self,
        shape: &TileShape,
        from_side: ShapeEntrySide,
        layer: i32,
        net_numbers: &[i32],
        class: i32,
        copper_sharing_allowed: bool,
        depths: ShoveDepths,
    ) -> bool {
        let result = self.check_forced_pad(
            shape,
            Some(from_side),
            layer,
            net_numbers,
            class,
            copper_sharing_allowed,
            &[],
            depths.recursion,
            depths.via_recursion,
            false,
            None,
        );
        if result == CheckDrillResult::NotDrillable {
            self.board.set_shove_failing_layer(layer);
            return false;
        }
        true
    }
}

fn is_ninety_degree(board: &RoutingBoard) -> bool {
    board.rules.trace_angle_restriction() == AngleRestriction::NinetyDegree
}

/// A circle's bounding box in 90 degree mode, else its bounding octagon.
fn circle_tile(circle: &Circle, ninety: bool) -> TileShape {
    if ninety {
        TileShape::IntBox(circle.bounding_box())
    } else {
        TileShape::IntOctagon(circle.bounding_octagon())
    }
}

/// A shape's bounding box in 90 degree mode, else its bounding octagon.
fn shape_tile(shape: &Shape, ninety: bool) -> TileShape {
    if ninety {
        TileShape::IntBox(shape.bounding_box())
    } else {
        TileShape::IntOctagon(shape.bounding_octagon().unwrap_or(IntOctagon::EMPTY))
    }
}

/// Java `ForcedViaInserter.holeCheckShape(padstack, location, board)`: the
/// substitute shape of the drill on a copper-less layer, a circle of radius
/// `ceil(drillRadius + holeClearance + 10)`, checked with the null clearance
/// class. `None` when the hole clearance rule is off, the location is not
/// integer or no drill radius is known.
pub fn hole_check_shape(
    board: &RoutingBoard,
    padstack: &Padstack,
    location: &Point,
) -> Option<Shape> {
    let hole_clearance = board.rules.hole_clearance();
    let &Point::Int(center) = location else {
        return None;
    };
    if hole_clearance <= 0 {
        return None;
    }
    let drill_radius = padstack.get_drill_radius();
    if drill_radius <= 0.0 {
        return None;
    }
    let radius = d2i((drill_radius + f64::from(hole_clearance) + 10.0).ceil());
    Some(Shape::Circle(Circle::new(center, radius)))
}

/// Java `ForcedViaInserter.calculateFromSide(viaLocation, viaShape,
/// roomShape, dist, is90Degree)`: the first of the four orthogonal
/// directions (down, right, up, left), then in 45 degree mode the four
/// diagonals, whose point at `dist` from the via lies in `room_shape`, as
/// the side of the via shape the shove comes from.
pub fn calculate_from_side(
    via_location: FloatPoint,
    via_shape: &TileShape,
    room_shape: &Simplex,
    dist: f64,
    is_90_degree: bool,
) -> Option<ShapeEntrySide> {
    let via_box = via_shape.bounding_box();
    let (x, y) = (via_location.x, via_location.y);
    for i in 0..4 {
        let (check_point, border) = match i {
            0 => ((x, y - dist), (x, f64::from(via_box.ll.y))),
            1 => ((x + dist, y), (f64::from(via_box.ur.x), y)),
            2 => ((x, y + dist), (x, f64::from(via_box.ur.y))),
            _ => ((x - dist, y), (f64::from(via_box.ll.x), y)),
        };
        if room_shape.contains_float(FloatPoint::new(check_point.0, check_point.1)) {
            let no = if is_90_degree { i } else { 2 * i };
            return Some(ShapeEntrySide::from_values(
                Some(no),
                Some(FloatPoint::new(border.0, border.1)),
            ));
        }
    }
    if is_90_degree {
        return None;
    }
    // The diagonal directions.
    let dist = dist / SQRT2;
    let border_dist = via_box.max_width() / (2.0 * SQRT2);
    for i in 0..4 {
        let (sx, sy) = match i {
            0 => (1.0, -1.0),
            1 => (1.0, 1.0),
            2 => (-1.0, 1.0),
            _ => (-1.0, -1.0),
        };
        let check_point = FloatPoint::new(x + sx * dist, y + sy * dist);
        if room_shape.contains_float(check_point) {
            let border = FloatPoint::new(x + sx * border_dist, y + sy * border_dist);
            return Some(ShapeEntrySide::from_values(Some(2 * i + 1), Some(border)));
        }
    }
    None
}

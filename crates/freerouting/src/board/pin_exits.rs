//! Pin exit restrictions, pin widths and pin swap (B05.3, B05.4, B05.7).
//!
//! Port of `Pin.getTraceExitRestrictions` (`Pin.java:243-309`),
//! `hasTraceExitRestrictions` (`:310`), `isPlacedOnFront` (`:557`),
//! `getMinWidth` (`:567`), `getTraceNeckdownHalfwidth` (`:586`),
//! `getMaxWidth` (`:592`), `calcNearestExitRestrictionDirection` (`:644`),
//! `nearestTraceExitCorner` (`:713`), `getPadstackLayer`,
//! `getSwappablePins` (`:467`) and `swap` (`:513`), plus the two
//! `TileShape` helpers they need (`intersectingBorderLineNo`,
//! `entrancePoints`), kept here because only the pin code uses them.
//!
//! The exit directions keep Java's expressions: the padstack direction is
//! rotated by the package pin's rotation (`PlacementTransform.rotate`),
//! placed by the component's transform, and turned back into a direction by
//! `Direction.getInstanceApprox(Math.atan2(y, x))`. `getInstanceApprox`
//! rounds `cos` and `sin` of the angle scaled by 10 000, so a one-ulp
//! difference of `atan2` (Rust calls the platform's, Java fdlibm's) changes
//! a direction only on a rounding boundary (D6); the `pin-exits` probe
//! reports such lines.

use std::collections::BTreeSet;

use freerouting_geometry::{
    Direction, FloatPoint, IntPoint, Line, LineSegment, PlacementTransform, Point, Polyline,
    PolylineShape, PolylineShapeOps, Shape, Side, TileShape, TileShapeOps,
};

use crate::board::basic_board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::board::structure::ComponentId;

/// An allowed direction for a trace to leave a pin, with the minimal length
/// of the trace's first line in that direction (Java
/// `Pin.TraceExitRestriction`).
#[derive(Clone, Debug, PartialEq)]
pub struct TraceExitRestriction {
    pub direction: Direction,
    /// The distance from the pin centre to the pad border along `direction`.
    pub min_length: f64,
}

impl BasicBoard {
    fn pin_item(&self, pin: ItemId) -> Option<&Item> {
        self.item(pin).filter(|i| i.is_pin())
    }

    /// Java `Pin.getPadstackLayer(index)`: the padstack layer of the pin's
    /// shape with index `index`.
    pub fn pin_padstack_layer(&self, pin: ItemId, index: i32) -> Option<i32> {
        let item = self.pin_item(pin)?;
        let drill = item.drill()?;
        let padstack = self.library.padstacks.get(drill.padstack)?;
        let first = drill.first_layer;
        Some(
            if self.pin_is_placed_on_front(pin) || padstack.placed_absolute {
                index + first
            } else {
                padstack.board_layer_count() as i32 - index - first - 1
            },
        )
    }

    /// Java `Pin.isPlacedOnFront`: false if the pin's component is on the
    /// back side.
    pub fn pin_is_placed_on_front(&self, pin: ItemId) -> bool {
        self.item(pin)
            .and_then(|i| self.components.get(ComponentId(i.component_id())))
            .is_none_or(|c| c.placed_on_front())
    }

    /// The pad shape of pin `pin` on board layer `layer` as a tile shape, if
    /// it is one (Java `getShape(layer - firstLayer()) instanceof TileShape`).
    fn pin_tile_shape(&self, pin: ItemId, layer: i32) -> Option<&TileShape> {
        match self.pin_item(pin)?.drill()?.shape_on_layer(layer)? {
            Shape::Polyline(PolylineShape::Tile(tile)) => Some(tile),
            _ => None,
        }
    }

    /// The allowed trace exit directions of pin `pin` on `layer`, with the
    /// minimal trace line lengths into them, in Java's order (Java
    /// `Pin.getTraceExitRestrictions`). Only box and octagon pads have
    /// restrictions: traces may leave them on the short sides only.
    pub fn trace_exit_restrictions(&self, pin: ItemId, layer: i32) -> Vec<TraceExitRestriction> {
        let mut result = Vec::new();
        let Some(item) = self.pin_item(pin) else {
            return result;
        };
        let (Some(drill), Some(pin_data)) = (item.drill(), item.as_pin()) else {
            return result;
        };
        if layer < drill.first_layer || layer > drill.last_layer {
            return result;
        }
        let Some(padstack_layer) = self.pin_padstack_layer(pin, layer - drill.first_layer) else {
            return result;
        };
        let component = self.components.get(ComponentId(item.component_id()));
        let package = component.and_then(|c| self.library.packages.get(c.package()));
        // Setting 1.5 to a higher factor may hinder the shove algorithm of
        // the autorouter between the pins of SMD components (Java's note).
        let mut pad_xy_factor = 1.5;
        if package.is_some_and(|p| p.pin_count() <= 3) {
            pad_xy_factor *= 2.0;
        }
        let Some(padstack) = self.library.padstacks.get(drill.padstack) else {
            return result;
        };
        let exit_directions = padstack.get_trace_exit_directions(padstack_layer, pad_xy_factor);
        let (false, Some(component)) = (exit_directions.is_empty(), component) else {
            return result;
        };
        let Some(pad_shape) = self.pin_tile_shape(pin, layer) else {
            return result;
        };
        let center = drill.center;
        let center_approx = center.to_float();
        let transform = component.placement_transform(self.components.flip_style_rotate_first());
        for padstack_direction in exit_directions {
            let Some(package_pin) = package.and_then(|p| p.get_pin(pin_data.pin_index)) else {
                continue;
            };
            let local_direction = PlacementTransform::rotate(
                padstack_direction.get_vector().to_float(),
                package_pin.rotation_in_degree,
            );
            let world_direction = transform.direction(local_direction);
            let exit_direction =
                Direction::get_instance_approx(world_direction.y.atan2(world_direction.x));
            let Some(border_no) = intersecting_border_line_no(pad_shape, center, &exit_direction)
            else {
                continue;
            };
            let exit_line = Line::from_point_and_direction(center, exit_direction.expect_int());
            let nearest = exit_line.intersection_approx(&pad_shape.border_line(border_no));
            result.push(TraceExitRestriction {
                direction: exit_direction,
                min_length: center_approx.distance(nearest),
            });
        }
        result
    }

    /// True if pin `pin` has exit restrictions on some layer (Java
    /// `Pin.hasTraceExitRestrictions`).
    pub fn has_trace_exit_restrictions(&self, pin: ItemId) -> bool {
        let Some(item) = self.pin_item(pin) else {
            return false;
        };
        (item.first_layer()..=item.last_layer())
            .any(|layer| !self.trace_exit_restrictions(pin, layer).is_empty())
    }

    /// The exit restriction direction of pin `pin` nearest to where
    /// `trace_polyline`, which starts at the pin centre, enters the pin
    /// shape offset by the pin edge-to-turn distance and `trace_half_width`
    /// (Java `Pin.calcNearestExitRestrictionDirection`). `None` without a
    /// matching restriction.
    pub fn calc_nearest_exit_restriction_direction(
        &self,
        pin: ItemId,
        trace_polyline: &Polyline,
        trace_half_width: i32,
        layer: i32,
    ) -> Option<Direction> {
        let restrictions = self.trace_exit_restrictions(pin, layer);
        if restrictions.is_empty() {
            return None;
        }
        let pin_shape = self.pin_tile_shape(pin, layer)?;
        let edge_to_turn_dist = self.rules.pin_edge_to_turn_dist();
        if edge_to_turn_dist < 0.0 {
            return None;
        }
        let offset_shape = pin_shape.offset(edge_to_turn_dist + f64::from(trace_half_width));
        let entries = entrance_points(&offset_shape, trace_polyline);
        let &(entry_line, entry_edge) = entries.last()?;
        let trace_entry = trace_polyline.lines()[entry_line]
            .intersection_approx(&offset_shape.border_line(entry_edge));
        let pin_center = self.pin_item(pin)?.drill()?.center;
        let tolerance = 1.0;
        let mut min_distance = f64::MAX;
        let mut nearest_corner: Option<FloatPoint> = None;
        let mut result = None;
        for restriction in &restrictions {
            let exit_corner = exit_corner(&offset_shape, pin_center, &restriction.direction)?;
            let distance = exit_corner.distance_square(trace_entry);
            let mut found = false;
            if distance + tolerance < min_distance {
                found = true;
            } else if distance < min_distance + tolerance {
                // The distances are nearly equal: compare with the previous
                // corners of the trace.
                found = nearer_to_trace(trace_polyline, exit_corner, nearest_corner, tolerance);
            }
            if found {
                min_distance = distance;
                result = Some(restriction.direction.clone());
                nearest_corner = Some(exit_corner);
            }
        }
        result
    }

    /// The legal trace exit point of pin `pin` on `layer` nearest to
    /// `from_point` (Java `Pin.nearestTraceExitCorner`). `None` if the pin has
    /// no exit restrictions there.
    pub fn nearest_trace_exit_corner(
        &self,
        pin: ItemId,
        from_point: FloatPoint,
        trace_half_width: i32,
        layer: i32,
    ) -> Option<FloatPoint> {
        let restrictions = self.trace_exit_restrictions(pin, layer);
        if restrictions.is_empty() {
            return None;
        }
        let pin_shape = self.pin_tile_shape(pin, layer)?;
        let pin_center = self.pin_item(pin)?.drill()?.center;
        let edge_to_turn_dist = self.rules.pin_edge_to_turn_dist();
        if edge_to_turn_dist < 0.0 {
            return None;
        }
        let offset_shape = pin_shape.offset(edge_to_turn_dist + f64::from(trace_half_width));
        let mut min_distance = f64::MAX;
        let mut nearest = None;
        for restriction in &restrictions {
            let corner = exit_corner(&offset_shape, pin_center, &restriction.direction)?;
            let distance = corner.distance_square(from_point);
            if distance < min_distance {
                min_distance = distance;
                nearest = Some(corner);
            }
        }
        nearest
    }

    /// The bounding box of the padstack shape of pin `pin` on `layer`, as
    /// `Pin.getMinWidth` and `getMaxWidth` read it.
    fn pin_padstack_box_width(&self, pin: ItemId, layer: i32, max: bool) -> f64 {
        let Some(item) = self.pin_item(pin) else {
            return 0.0;
        };
        let Some(padstack_layer) = self.pin_padstack_layer(pin, layer - item.first_layer()) else {
            return 0.0;
        };
        let shape = item
            .drill()
            .and_then(|d| self.library.padstacks.get(d.padstack))
            .and_then(|p| p.get_shape(padstack_layer));
        let Some(shape) = shape else {
            return 0.0;
        };
        let bounds = shape.to_shape().bounding_box();
        let (w, h) = (bounds.ur.x - bounds.ll.x, bounds.ur.y - bounds.ll.y);
        f64::from(if max { w.max(h) } else { w.min(h) })
    }

    /// The smallest width of the padstack shape of pin `pin` on `layer`
    /// (Java `Pin.getMinWidth`; 0 without a shape).
    pub fn pin_min_width(&self, pin: ItemId, layer: i32) -> f64 {
        self.pin_padstack_box_width(pin, layer, false)
    }

    /// The largest width of the padstack shape of pin `pin` on `layer`
    /// (Java `Pin.getMaxWidth`; 0 without a shape).
    pub fn pin_max_width(&self, pin: ItemId, layer: i32) -> f64 {
        self.pin_padstack_box_width(pin, layer, true)
    }

    /// The half width of a trace that necks down to enter pin `pin` on
    /// `layer` (Java `Pin.getTraceNeckdownHalfwidth`:
    /// `(int) max(0.5·getMinWidth(layer) − 1, 1)`).
    pub fn trace_neckdown_half_width(&self, pin: ItemId, layer: i32) -> i32 {
        freerouting_geometry::jmath::d2i((0.5 * self.pin_min_width(pin, layer) - 1.0).max(1.0))
    }

    /// The pins pin `pin` can be swapped with: the other pins of its
    /// component's logical part with the same gate name and the same
    /// positive gate pin swap code, highest id first (Java
    /// `Pin.getSwappablePins`).
    pub fn swappable_pins(&self, pin: ItemId) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let Some(item) = self.pin_item(pin) else {
            return result;
        };
        let component_id = item.component_id();
        let pin_index = item.as_pin().map_or(0, |p| p.pin_index);
        let part = self
            .components
            .get(ComponentId(component_id))
            .and_then(|c| c.logical_part())
            .and_then(|id| self.library.logical_parts.get(id));
        let Some(part) = part else {
            return result;
        };
        let Some(this_pin) = part.get_pin(pin_index) else {
            return result;
        };
        if this_pin.gate_pin_swap_code <= 0 {
            return result;
        }
        for i in 0..part.pin_count() {
            if i == pin_index {
                continue;
            }
            let Some(other) = part.get_pin(i) else {
                continue;
            };
            if other.gate_pin_swap_code == this_pin.gate_pin_swap_code
                && other.gate_name == this_pin.gate_name
            {
                // Java logs a missing pin and goes on.
                if let Some(id) = self.get_pin(component_id, other.pin_index) {
                    result.insert(id);
                }
            }
        }
        result
    }

    /// Swaps the nets of pins `a` and `b` and their `changed_to` (Java
    /// `Pin.swap`). False, and nothing changes, if a pin belongs to more
    /// than one net.
    pub fn swap_pins(&mut self, a: ItemId, b: ItemId) -> bool {
        let (Some(pin_a), Some(pin_b)) = (self.pin_item(a), self.pin_item(b)) else {
            return false;
        };
        if pin_a.net_count() > 1 || pin_b.net_count() > 1 {
            self.log
                .push("Pin.swap not yet implemented for pins belonging to more than 1 net ".into());
            return false;
        }
        let net_a = pin_a.net_numbers().first().copied().unwrap_or(0);
        let net_b = pin_b.net_numbers().first().copied().unwrap_or(0);
        self.assign_net_no(a, net_b);
        self.assign_net_no(b, net_a);
        let changed = |board: &BasicBoard, id: ItemId| {
            board.item(id).and_then(Item::as_pin).map(|p| p.changed_to)
        };
        let (Some(to_a), Some(to_b)) = (changed(self, a), changed(self, b)) else {
            return false;
        };
        // PARITY: no extra undo snapshot. Java changes `changedTo` on the
        // live objects, which `assignNetNo` saved unless it returned early
        // (net 0 is not a normal net).
        for (id, to) in [(a, to_b), (b, to_a)] {
            if let Some(ItemKind::Pin(pin)) = self.items.get_mut(&id).map(|i| &mut i.kind) {
                pin.changed_to = to;
            }
        }
        true
    }
}

/// Java `currentPinExitRay.intersectionApprox(offsetPinShape.borderLine(n))`
/// for the border line the exit ray from `center` in `direction` crosses.
/// `None` where Java would fail on a border line index of -1.
fn exit_corner(shape: &TileShape, center: IntPoint, direction: &Direction) -> Option<FloatPoint> {
    let border_no = intersecting_border_line_no(shape, center, direction)?;
    let ray = Line::from_point_and_direction(center, direction.expect_int());
    Some(ray.intersection_approx(&shape.border_line(border_no)))
}

/// The tie-break of `calcNearestExitRestrictionDirection` and
/// `correctConnectionToPin` for exit corners at nearly equal distance: true
/// if the first trace corner (from the second on) that tells them apart is
/// nearer to `candidate` than to `nearest`.
pub(crate) fn nearer_to_trace(
    trace_polyline: &Polyline,
    candidate: FloatPoint,
    nearest: Option<FloatPoint>,
    tolerance: f64,
) -> bool {
    for i in 1..trace_polyline.corner_count() {
        let corner = trace_polyline.corner_approx(i);
        let candidate_distance = corner.distance_square(candidate);
        // Java's `distanceSquare(null)` would throw; a first candidate never
        // gets here because the distance starts at Double.MAX_VALUE.
        let old_distance = nearest.map_or(f64::MAX, |n| corner.distance_square(n));
        if candidate_distance + tolerance < old_distance {
            return true;
        } else if candidate_distance > old_distance + tolerance {
            return false;
        }
    }
    false
}

/// Java `TileShape.intersectingBorderLineNo`: the border line of `shape`
/// that the ray from `point` in `direction` crosses, `None` if `point` is
/// not in `shape`.
pub(crate) fn intersecting_border_line_no(
    shape: &TileShape,
    point: IntPoint,
    direction: &Direction,
) -> Option<usize> {
    if !shape.contains(&Point::Int(point)) {
        return None;
    }
    let from_point = point.to_float();
    let intersection_line = Line::from_point_and_direction(point, direction.expect_int());
    let second_line_point = intersection_line.b().to_float();
    let mut result = None;
    let mut min_distance = f64::from(f32::MAX);
    for i in 0..shape.border_line_count() {
        let border_line = shape.border_line(i);
        let intersection = border_line.intersection_approx(&intersection_line);
        if intersection.x >= f64::from(i32::MAX) {
            continue; // the lines are parallel
        }
        let distance = intersection.distance_square(from_point);
        if distance < min_distance {
            let direction_ok = border_line.side_of_float(second_line_point) == Side::OnTheLeft
                || second_line_point.distance_square(intersection) < distance;
            if direction_ok {
                result = Some(i);
                min_distance = distance;
            }
        }
    }
    result
}

/// Java `TileShape.entrancePoints`: per crossing of `polyline`'s segments
/// with the border of `shape`, the polyline line index and the border line
/// index, without repeats of the same pair in a row.
pub(crate) fn entrance_points(shape: &TileShape, polyline: &Polyline) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut previous: Option<(usize, usize)> = None;
    let line_count = polyline.lines().len();
    for line_index in 1..line_count.saturating_sub(1) {
        let segment = LineSegment::from_polyline(polyline, line_index);
        for edge_index in segment.border_intersections(shape) {
            if previous != Some((line_index, edge_index)) {
                result.push((line_index, edge_index));
                previous = Some((line_index, edge_index));
            }
        }
    }
    result
}

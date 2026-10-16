//! The three pin-connection fixes `PolylineTrace.pullTight` runs after the
//! tightener (B05.5).
//!
//! Port of `PolylineTrace.checkConnectionToPin` (`PolylineTrace.java:1014-1080`),
//! `correctConnectionToPin` (`:1082-1250`) and `swapConnectionToPin`
//! (`:1252-1313`). `pullTight` (O01) runs them only when the angle
//! restriction is not 90 degrees and the pin edge-to-turn distance is
//! positive, in the order: swap start, swap end, correct start, correct end
//! (`:841-861`). Headless Java leaves that distance at 0, so they matter for
//! interactive work and for O01's recursion.
//!
//! `PolylineTrace.smoothenEndCornersFork` (`:893`) has no caller at the pin
//! and is not ported.

use freerouting_geometry::{
    Direction, FloatPoint, Line, Polyline, PolylineShapeOps, Shape, Signum, TileShape, TileShapeOps,
};

use crate::board::items::{ItemId, PolylineTrace};
use crate::board::pin_exits::{entrance_points, intersecting_border_line_no, nearer_to_trace};
use crate::board::routing_board::RoutingBoard;
use crate::board::structure::{AngleRestriction, FixedState};
use crate::board::{LineReuse, TraceExitRestriction};

/// What the fixes read of a trace (Java reads the fields of the trace
/// object, which stay readable after `change` removes it from the board).
#[derive(Clone, Debug)]
struct TraceData {
    polyline: Polyline,
    layer: i32,
    half_width: i32,
    nets: Vec<i32>,
    class: i32,
    fixed: FixedState,
}

/// The nearest legal exit found by `correctConnectionToPin`.
struct NearestExit {
    ray: Line,
    border_no: usize,
    direction: Direction,
}

impl RoutingBoard {
    fn trace_data(&self, trace: ItemId) -> Option<TraceData> {
        let item = self.item(trace)?;
        let t: &PolylineTrace = item.as_trace()?;
        Some(TraceData {
            polyline: t.polyline.clone(),
            layer: t.layer,
            half_width: t.half_width,
            nets: item.net_numbers().to_vec(),
            class: item.clearance_class(),
            fixed: item.fixed_state(),
        })
    }

    /// The first pin (highest id) among the contacts at the start or end of
    /// `trace`.
    fn end_contact_pin(&self, trace: ItemId, at_start: bool) -> Option<ItemId> {
        let contacts = if at_start {
            self.trace_start_contacts(trace)
        } else {
            self.trace_end_contacts(trace)
        };
        contacts
            .into_iter()
            .find(|&id| self.item(id).is_some_and(|i| i.is_pin()))
    }

    /// Java `Math.max(edgeToTurnDist, clearance + 1)` for `trace` at `pin`.
    fn add_width(&self, data: &TraceData, pin: ItemId, edge_to_turn_dist: f64) -> f64 {
        let pin_class = self.item(pin).map_or(0, |i| i.clearance_class());
        let clearance = f64::from(self.clearance_value(data.class, pin_class, data.layer));
        edge_to_turn_dist.max(clearance + 1.0)
    }

    /// True unless a pin with exit restrictions is at the start (or end) of
    /// trace `trace` and the trace leaves it in a direction or with a first
    /// line length the restrictions don't allow (Java
    /// `PolylineTrace.checkConnectionToPin`).
    pub fn check_connection_to_pin(&self, trace: ItemId, at_start: bool) -> bool {
        let Some(data) = self.trace_data(trace) else {
            return true;
        };
        let corner_count = data.polyline.corner_count();
        if corner_count < 2 {
            return true;
        }
        let Some(pin) = self.end_contact_pin(trace, at_start) else {
            return true;
        };
        let restrictions = self.trace_exit_restrictions(pin, data.layer);
        if restrictions.is_empty() {
            return true;
        }
        let (end_corner, prev_end_corner) = if at_start {
            (data.polyline.corner(0), data.polyline.corner(1))
        } else {
            (
                data.polyline.corner(corner_count - 1),
                data.polyline.corner(corner_count - 2),
            )
        };
        let Some(trace_end_direction) =
            Direction::get_instance_from_points(&end_corner, &prev_end_corner)
        else {
            return true;
        };
        let Some(matching) = restrictions
            .iter()
            .find(|r| r.direction == trace_end_direction)
        else {
            return false;
        };
        let edge_to_turn_dist = self.rules.pin_edge_to_turn_dist();
        if edge_to_turn_dist < 0.0 {
            return false;
        }
        let end_line_length = end_corner.to_float().distance(prev_end_corner.to_float());
        let add_width = self.add_width(&data, pin, edge_to_turn_dist);
        let preserve_length = matching.min_length + f64::from(data.half_width) + add_width;
        preserve_length <= end_line_length
    }

    /// Reroutes the start (or end) of trace `trace` so that it leaves its pin
    /// through the nearest legal exit, and inserts a shove-fixed exit stub.
    /// True if the trace changed (Java `PolylineTrace.correctConnectionToPin`).
    pub fn correct_connection_to_pin(
        &mut self,
        trace: ItemId,
        at_start: bool,
        angle_restriction: AngleRestriction,
    ) -> bool {
        if self.check_connection_to_pin(trace, at_start) {
            return false;
        }
        let Some(data) = self.trace_data(trace) else {
            return false;
        };
        let trace_polyline = if at_start {
            data.polyline.clone()
        } else {
            data.polyline.reverse()
        };
        let Some(pin) = self.end_contact_pin(trace, at_start) else {
            return false;
        };
        let restrictions = self.trace_exit_restrictions(pin, data.layer);
        if restrictions.is_empty() {
            return false;
        }
        let Some(offset_shape) = self.corrected_offset_shape(&data, pin, angle_restriction) else {
            return false;
        };
        let entries = entrance_points(&offset_shape, &trace_polyline);
        let Some(&(entry_line, entry_edge)) = entries.last() else {
            return false;
        };
        let trace_entry = trace_polyline.lines()[entry_line]
            .intersection_approx(&offset_shape.border_line(entry_edge));
        let Some(pin_center) = self.item(pin).and_then(|i| i.drill()).map(|d| d.center) else {
            return false;
        };
        let Some(nearest) = nearest_exit(
            &offset_shape,
            pin_center,
            &restrictions,
            trace_entry,
            &trace_polyline,
        ) else {
            return false;
        };
        // Append the polygon piece around the border of the pin shape.
        let current_lines = border_lines(
            &offset_shape,
            &nearest,
            entry_edge,
            &trace_polyline,
            entry_line,
        );
        let border_polyline = Polyline::new(&current_lines);
        if !self.check_polyline_trace(
            &border_polyline,
            data.layer,
            data.half_width,
            &data.nets,
            data.class,
        ) {
            return false;
        }
        let mut cut_lines = vec![current_lines[current_lines.len() - 2]];
        cut_lines.extend_from_slice(&trace_polyline.lines()[entry_line..]);
        let cut_polyline = Polyline::new(&cut_lines);
        let mut changed = if cut_polyline.first_corner() == cut_polyline.last_corner() {
            border_polyline
        } else {
            border_polyline.combine(&cut_polyline)
        };
        if !at_start {
            changed = changed.reverse();
        }
        // PARITY: Java's `change` keeps the leaves of the lines that are the
        // trace's own `Line` objects. At the start the new polyline begins
        // with the new exit ray and border lines and ends with the trace's
        // lines from the entry line on, which `combine` and `new Polyline`
        // copy by reference unless they turn one around (a new object with
        // swapped points, so different by value). So the reused suffix is
        // the by-value suffix, capped at the lines that came from the trace.
        // At the end both `reverse` calls create new lines: nothing is reused.
        let reuse = if at_start {
            let by_value = LineReuse::by_value(&data.polyline, &changed);
            LineReuse {
                prefix: 0,
                suffix: by_value
                    .suffix
                    .min(trace_polyline.lines().len() - entry_line),
            }
        } else {
            LineReuse::NONE
        };
        self.change_trace(trace, changed, reuse);
        // Create a shove-fixed exit line.
        let exit_lines = [
            Line::from_point_and_direction(
                pin_center,
                nearest.direction.turn_45_degree(2).expect_int(),
            ),
            nearest.ray,
            offset_shape.border_line(nearest.border_no),
        ];
        self.insert_trace(
            Polyline::new(&exit_lines),
            data.layer,
            data.half_width,
            &data.nets,
            data.class,
            FixedState::ShoveFixed,
        );
        true
    }

    /// The pin shape of `pin` on the trace's layer offset by the half width
    /// and the edge-to-turn distance (or clearance), as a box for 90 degrees
    /// or a box pad, as an octagon for 45 degrees.
    fn corrected_offset_shape(
        &self,
        data: &TraceData,
        pin: ItemId,
        angle_restriction: AngleRestriction,
    ) -> Option<TileShape> {
        let pin_shape = match self.item(pin)?.drill()?.shape_on_layer(data.layer)? {
            Shape::Polyline(freerouting_geometry::PolylineShape::Tile(tile)) => tile.clone(),
            _ => return None,
        };
        let edge_to_turn_dist = self.rules.pin_edge_to_turn_dist();
        if edge_to_turn_dist < 0.0 {
            return None;
        }
        let add_width = self.add_width(data, pin, edge_to_turn_dist);
        let offset = pin_shape.offset(f64::from(data.half_width) + add_width);
        Some(
            if angle_restriction == AngleRestriction::NinetyDegree || offset.is_int_box() {
                TileShape::IntBox(offset.bounding_box())
            } else if angle_restriction == AngleRestriction::FortyfiveDegree {
                TileShape::IntOctagon(offset.bounding_octagon()?)
            } else {
                offset
            },
        )
    }

    /// Changes the start (or end) of trace `trace` to the shove-fixed exit
    /// stub it makes a sharp angle with, when the stub's pin has a better
    /// fitting exit, by combining the two (Java
    /// `PolylineTrace.swapConnectionToPin`). True if the trace changed.
    pub fn swap_connection_to_pin(&mut self, trace: ItemId, at_start: bool) -> bool {
        let Some(data) = self.trace_data(trace) else {
            return false;
        };
        let (trace_polyline, contacts) = if at_start {
            (data.polyline.clone(), self.trace_start_contacts(trace))
        } else {
            (data.polyline.reverse(), self.trace_end_contacts(trace))
        };
        if contacts.len() != 1 {
            return false;
        }
        let contact = *contacts.first().expect("one contact");
        let Some(contact_item) = self.item(contact) else {
            return false;
        };
        let Some(contact_trace) = contact_item.as_trace() else {
            return false;
        };
        if contact_item.fixed_state() != FixedState::ShoveFixed {
            return false;
        }
        let contact_polyline = contact_trace.polyline.clone();
        let contact_lines = contact_polyline.lines();
        let contact_last_line = contact_lines[contact_lines.len() - 2];
        let lines = trace_polyline.lines();
        // Look if this trace has a sharp angle with the contact trace.
        let sharp = |line: &Line| {
            contact_last_line.direction().projection(&line.direction()) == Signum::Negative
        };
        let mut check_swap = sharp(&lines[1]);
        if !check_swap {
            let half_width = f64::from(data.half_width);
            if lines.len() > 3
                && trace_polyline
                    .corner_approx(0)
                    .distance_square(trace_polyline.corner_approx(1))
                    <= half_width * half_width
            {
                // Check also for a sharp angle with the second line.
                check_swap = sharp(&lines[2]);
            }
        }
        if !check_swap {
            return false;
        }
        let Some(contact_pin) = self
            .trace_start_contacts(contact)
            .into_iter()
            .find(|&id| self.item(id).is_some_and(|i| i.is_pin()))
        else {
            return false;
        };
        let combined = contact_polyline.combine(&trace_polyline);
        let nearest = self.calc_nearest_exit_restriction_direction(
            contact_pin,
            &combined,
            data.half_width,
            data.layer,
        );
        match nearest {
            Some(direction) if direction != contact_lines[1].direction() => {}
            _ => return false, // the direction would not change
        }
        self.set_fixed_state(contact, data.fixed);
        self.combine_trace(trace);
        true
    }
}

/// The exit restriction whose exit corner on `offset_shape` is nearest to
/// `trace_entry`, with Java's tolerance and tie-break (the loop of
/// `correctConnectionToPin`). `None` where Java would fail (no border line
/// crossed, or no restriction chosen).
fn nearest_exit(
    offset_shape: &TileShape,
    pin_center: freerouting_geometry::IntPoint,
    restrictions: &[TraceExitRestriction],
    trace_entry: FloatPoint,
    trace_polyline: &Polyline,
) -> Option<NearestExit> {
    let tolerance = 1.0;
    let mut min_distance = f64::MAX;
    let mut nearest_corner: Option<FloatPoint> = None;
    let mut result = None;
    for restriction in restrictions {
        let border_no =
            intersecting_border_line_no(offset_shape, pin_center, &restriction.direction)?;
        let ray = Line::from_point_and_direction(pin_center, restriction.direction.expect_int());
        let exit_corner = ray.intersection_approx(&offset_shape.border_line(border_no));
        let distance = exit_corner.distance_square(trace_entry);
        let mut found = false;
        if distance + tolerance < min_distance {
            found = true;
        } else if distance < min_distance + tolerance {
            found = nearer_to_trace(trace_polyline, exit_corner, nearest_corner, tolerance);
        }
        if found {
            min_distance = distance;
            nearest_corner = Some(exit_corner);
            result = Some(NearestExit {
                ray,
                border_no,
                direction: restriction.direction.clone(),
            });
        }
    }
    result
}

/// Java's `currentLines`: the exit ray, the border lines of `shape` from
/// the exit's border line to the entry's, the shorter way round, and the
/// trace line that enters the shape.
fn border_lines(
    shape: &TileShape,
    nearest: &NearestExit,
    entry_edge: usize,
    trace_polyline: &Polyline,
    entry_line: usize,
) -> Vec<Line> {
    let corner_count = shape.border_line_count();
    let clockwise_diff = (nearest.border_no + corner_count - entry_edge) % corner_count;
    let counter_clockwise_diff = (entry_edge + corner_count - nearest.border_no) % corner_count;
    let mut current = nearest.border_no;
    let mut lines = vec![nearest.ray];
    if counter_clockwise_diff <= clockwise_diff {
        for _ in 0..=counter_clockwise_diff {
            lines.push(shape.border_line(current));
            current = (current + 1) % corner_count;
        }
    } else {
        for _ in 0..=clockwise_diff {
            lines.push(shape.border_line(current));
            current = (current + corner_count - 1) % corner_count;
        }
    }
    lines.push(trace_polyline.lines()[entry_line]);
    lines
}

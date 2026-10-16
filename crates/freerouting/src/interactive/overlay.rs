//! What a route shows while it is in progress (I01.6): the data Java's
//! `Route.draw` uses (`gui/interactive/Route.java` at `a773a70ec`), without
//! colours or pixels. Lengths and points are in board units.
//!
//! - the shove-failing obstacle, only with "highlight obstacle";
//! - the swappable pins and their incompletes;
//! - the targets that are not conduction areas;
//! - the incomplete from the last corner to the nearest target point, its
//!   width (`min(resolution in mil, 100)`, three times that when the net's
//!   length limits are violated) and the violation flag;
//! - the length-matching ellipses from the net's minimum and maximum length:
//!   none, one, or two for a ring when the minimum is not reached yet. A net
//!   without a maximum gets one containing the whole board
//!   (`0.3 * CRIT_INT`). As in Java, the incomplete counts as violating when
//!   it is longer than the remaining maximum, or the minimum exceeds the
//!   maximum;
//! - the layer-change marker at the last corner when the nearest target is
//!   not on the route's layer (radius `4 ×` the pen half width on layer 0).
//!
//! Java draws the ellipses only for a positive maximum, or a positive
//! minimum with a visible length-matching colour;
//! [`RouteOverlay::new`]'s `length_matching_visible` is that colour's
//! visibility.

use freerouting_geometry::{CRIT_INT, Ellipse, FloatPoint};

use super::route::{RouteSession, SwapPinInfo};
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::structure::Unit;

/// A length-matching ellipse (Java `geometry.planar.Ellipse`).
pub type LengthEllipse = Ellipse;

/// The incomplete from the last corner to the nearest target point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteIncomplete {
    pub from: FloatPoint,
    pub to: FloatPoint,
    /// The drawing width in board units.
    pub width: f64,
    /// The net's length limits are violated (Java draws it in the
    /// violations colour).
    pub length_violation: bool,
}

/// The overlay data of a route session.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RouteOverlay {
    /// The obstacle the last shove failed at (only with "highlight
    /// obstacle").
    pub failing_obstacle: Option<ItemId>,
    /// The swappable pins and their incompletes, highest id first.
    pub swap_pins: Vec<SwapPinInfo>,
    /// The targets except conduction areas, highest id first.
    pub targets: Vec<ItemId>,
    pub incomplete: Option<RouteIncomplete>,
    /// Zero, one or two (a ring) length-matching ellipses.
    pub length_ellipses: Vec<LengthEllipse>,
    /// The layer-change marker: its centre and radius.
    pub layer_change_marker: Option<(FloatPoint, f64)>,
}

impl RouteOverlay {
    /// The overlay of `session` on `board`. `length_matching_visible` is
    /// whether the GUI shows the length-matching area (Java's
    /// `lengthMatchingColorIntensity > 0`).
    pub fn new(
        session: &RouteSession,
        board: &RoutingBoard,
        length_matching_visible: bool,
    ) -> Self {
        let mut overlay = RouteOverlay {
            failing_obstacle: session
                .highlights_obstacle()
                .then(|| session.shove_failing_obstacle())
                .flatten(),
            ..RouteOverlay::default()
        };
        let Some(&first_net) = session.net_numbers().first() else {
            return overlay;
        };
        let Some(net) = board.rules.nets.get(first_net) else {
            return overlay;
        };
        overlay.swap_pins = session.swap_pins().copied().collect();
        overlay.targets = session
            .target_set()
            .iter()
            .copied()
            .filter(|&id| !session.is_area_target(id))
            .collect();
        let Some(nearest) = session.nearest_target_point() else {
            return overlay;
        };
        let prev_corner = session.last_corner();
        let from = prev_corner.to_float();
        let mut length_ok = true;
        let mut max_length = net.maximum_trace_length(&board.rules.net_classes);
        let min_length = net.minimum_trace_length(&board.rules.net_classes);
        if max_length > 0.0 || min_length > 0.0 && length_matching_visible {
            // != 0 only in stitching mode in Java's comment; always 0
            let trace_length_add = from.distance(prev_corner.to_float());
            if max_length <= 0.0 {
                // no maximum: an ellipse containing the whole board
                max_length = 0.3 * f64::from(CRIT_INT);
            }
            let trace_length = board.net_trace_length(first_net);
            let current_max = max_length - (trace_length + trace_length_add);
            let current_min = min_length - (trace_length + trace_length_add);
            let incomplete_length = nearest.distance(from);
            if incomplete_length < current_max && min_length <= max_length {
                let rotation = nearest
                    .round()
                    .difference_by(&freerouting_geometry::Point::Int(prev_corner))
                    .angle_approx();
                let center = from.middle_point(nearest);
                let ellipse = |length: f64| {
                    let bigger = 0.5 * length;
                    // dist_focus_to_center^2 = bigger^2 - smaller^2
                    let smaller =
                        0.5 * (length * length - incomplete_length * incomplete_length).sqrt();
                    Ellipse::new(center, rotation, bigger, smaller)
                };
                overlay.length_ellipses.push(ellipse(current_max));
                if min_length > 0.0 && incomplete_length < current_min {
                    // an ellipse ring
                    overlay.length_ellipses.push(ellipse(current_min));
                }
            } else {
                length_ok = false;
            }
        }
        let mut width = board.communication.get_resolution(Unit::Mil).min(100.0);
        if !length_ok {
            width *= 3.0;
        }
        overlay.incomplete = Some(RouteIncomplete {
            from,
            to: nearest,
            width,
            length_violation: !length_ok,
        });
        let off_layer = session
            .nearest_target_item()
            .and_then(|id| board.item(id))
            .is_some_and(|i| !i.is_on_layer(session.layer()));
        if off_layer {
            let pen = session.params().half_widths.first().copied().unwrap_or(0);
            overlay.layer_change_marker = Some((from, f64::from(4 * pen)));
        }
        overlay
    }
}

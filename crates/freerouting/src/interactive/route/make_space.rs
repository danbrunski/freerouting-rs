//! The route of make-space (I02.3): Java `MakeSpaceState`'s constructor,
//! which builds a `Route` on the hidden net with no start item and no
//! target set.

use std::collections::BTreeMap;

use freerouting_geometry::FloatPoint;

use super::{RouteMode, RouteSession, ShoveDepths};
use crate::board::RoutingBoard;
use crate::interactive::settings::{EditSettings, RouteParams};
use crate::rules::Nets;

/// Java `MakeSpaceState`'s half width of the shoving trace:
/// `max(min(100, minTraceHalfWidth / 10), 5)`.
pub fn make_space_half_width(board: &RoutingBoard) -> i32 {
    (board.min_trace_half_width() / 10).clamp(5, 100)
}

impl RouteSession {
    /// Java `new MakeSpaceState(location, ..)`: a route from `at` on the
    /// current layer of the hidden net ([`Nets::HIDDEN_NET_NUMBER`]) with the
    /// make-space half width on every layer, all layers active, clearance
    /// class 0, no via rule, push on, the settings' tidy width, accuracy and
    /// obstacle highlight, no neckdown and no via snap, in dynamic mode. It
    /// has no start item and no target set (Java's `null`s): nothing is a
    /// target and no swap pins are collected. No snapshot is taken. Java's
    /// `ViaRule.EMPTY` is `None` here, so [`change_layer`](Self::change_layer)
    /// fails where Java's would "succeed" without a via; Java's make-space
    /// state never changes the route's layer.
    pub fn make_space(board: &RoutingBoard, settings: &EditSettings, at: FloatPoint) -> Self {
        let layer_count = board.layer_count();
        let half_width = make_space_half_width(board);
        let params = RouteParams {
            half_widths: vec![half_width; layer_count],
            layer_active: vec![true; layer_count],
            clearance_class: 0,
            via_rule: None,
        };
        RouteSession {
            net_numbers: vec![Nets::HIDDEN_NET_NUMBER],
            start_item: None,
            target_set: None,
            swap_pins: BTreeMap::new(),
            params,
            depths: ShoveDepths::for_push(true),
            tidy_width: settings.trace_tidy_width,
            accuracy: settings.pull_tight_accuracy,
            mode: RouteMode::Dynamic,
            neckdown: false,
            via_snap_to_smd_center: false,
            highlight_obstacle: settings.highlight_routing_obstacle,
            prev_corner: at.round(),
            layer: settings.layer,
            current_layer: settings.layer,
            target_points: Vec::new(),
            target_traces_and_areas: Vec::new(),
            target_shapes: BTreeMap::new(),
            nearest_target_point: None,
            nearest_target_item: None,
            shove_failing_obstacle: None,
            snapshots: 0,
            corners: 0,
        }
    }

    /// True for a make-space route (no target set).
    pub fn is_make_space(&self) -> bool {
        self.target_set.is_none()
    }
}

//! Layer change and cancel: Java `RouteState.changeLayerAction` with
//! `Route.changeLayer` and `snapToSmdCenter`, and `RouteState.cancel`.

use freerouting_geometry::{IntPoint, Point};

use super::start::pins_filter;
use super::{RouteBudget, RouteSession};
use crate::board::RoutingBoard;
use crate::board::items::{ItemId, StopConnectionOption};
use crate::board::selection_filter::{ItemFilter, SelectableChoice};
use crate::interactive::Status;
use crate::interactive::settings::EditSettings;

/// The result of [`RouteSession::change_layer`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerChange {
    /// The layer is not a board layer: nothing happened (Java ignores it).
    Ignored,
    /// The layer change succeeded and the current layer is the new one.
    /// `via` is false when no via was inserted: the same layer, or the kept
    /// quirk of a via rule without a via spanning both layers, which leaves
    /// the route on its old layer.
    Changed { via: bool },
    /// The new via contacts a plane of the route nets between the layers:
    /// the route is over.
    PlaneReached,
    /// No via could be inserted (or the layer is inactive or there is no via
    /// rule); `failing_layer` is the board's shove-failing layer.
    Failed { failing_layer: i32 },
}

impl LayerChange {
    /// True if the route is over.
    pub fn ends_route(&self) -> bool {
        matches!(self, LayerChange::PlaneReached)
    }
}

/// Java `Route.changeLayer`'s result: whether it returned true, and whether
/// a via was inserted.
struct RouteLayerChange {
    ok: bool,
    via: bool,
}

impl RouteSession {
    /// Changes the route to layer `to` with a via at the last corner (Java
    /// `RouteState.changeLayerAction`). Returns the outcome and Java's status
    /// message.
    pub fn change_layer(
        &mut self,
        board: &mut RoutingBoard,
        to: i32,
        budget: RouteBudget,
    ) -> (LayerChange, Option<Status>) {
        if to < 0 || to >= board.layer_count() as i32 {
            return (LayerChange::Ignored, None);
        }
        let layer_name = |board: &RoutingBoard, layer: i32| {
            board.layer_structure.layers[layer as usize].name.clone()
        };
        let mut status = None;
        if !self.params.is_layer_active(to) {
            status = Some(Status::new(
                "layer_not_changed_inactive_net_message",
                [layer_name(board, to)],
            ));
        }
        let result = self.route_change_layer(board, to, budget);
        if !result.ok {
            let failing_layer = board.shove_failing_layer();
            if failing_layer >= 0 {
                status = Some(Status::new(
                    "layer_not_changed_obstacle_message",
                    [layer_name(board, failing_layer)],
                ));
            }
            return (LayerChange::Failed { failing_layer }, status);
        }
        // check, if the layer change resulted in a connection to a power
        // plane
        let old_layer = self.current_layer;
        if self.reaches_plane(board, old_layer, to) {
            return (LayerChange::PlaneReached, status);
        }
        self.current_layer = to;
        // make the current situation restorable by undo
        board.generate_snapshot();
        self.snapshots += 1;
        let status = Status::new("layer_changed_to_message", [layer_name(board, to)]);
        (LayerChange::Changed { via: result.via }, Some(status))
    }

    /// True if a via of the route nets at the last corner on `old_layer`
    /// (the first one, highest id first) contacts a conduction area on a
    /// layer from after `old_layer` up to `new_layer`.
    fn reaches_plane(&self, board: &RoutingBoard, old_layer: i32, new_layer: i32) -> bool {
        let vias = ItemFilter::only(SelectableChoice::Vias);
        let picked =
            board.pick_items_filtered(&Point::Int(self.prev_corner), old_layer, Some(&vias));
        let Some(via) = picked.into_iter().find(|&id| {
            board
                .item(id)
                .is_some_and(|i| i.shares_net_no(&self.net_numbers))
        }) else {
            return false;
        };
        let (from_layer, to_layer) = if old_layer < new_layer {
            (old_layer + 1, new_layer)
        } else {
            (new_layer, old_layer - 1)
        };
        board.normal_contacts(via).into_iter().any(|id| {
            board
                .item(id)
                .and_then(|i| i.as_conduction_area())
                .is_some_and(|area| area.layer() >= from_layer && area.layer() <= to_layer)
        })
    }

    /// Java `Route.changeLayer`: tries the vias of the via rule that span
    /// both layers in order, each after a snapshot that is undone when the
    /// via fails.
    fn route_change_layer(
        &mut self,
        board: &mut RoutingBoard,
        to: i32,
        budget: RouteBudget,
    ) -> RouteLayerChange {
        let fail = RouteLayerChange {
            ok: false,
            via: false,
        };
        if self.layer == to {
            return RouteLayerChange {
                ok: true,
                via: false,
            };
        }
        if to < 0
            || to as usize >= self.params.layer_active.len()
            || !self.params.is_layer_active(to)
        {
            return fail;
        }
        let Some(rule) = self.params.via_rule else {
            return fail;
        };
        self.shove_failing_obstacle = None;
        if self.via_snap_to_smd_center && !self.snap_to_smd_center(board, to, budget) {
            self.snap_to_smd_center(board, self.layer, budget);
        }
        let (min_layer, max_layer) = (self.layer.min(to), self.layer.max(to));
        let vias: Vec<_> = board.rules.via_rules.get(rule).vias().to_vec();
        // PARITY(quirk, java_quirks.route_change_layer_no_via): no spanning via
        // must fail in the fixed arm, including an empty via rule.
        let mut result = board.java_quirks().route_change_layer_no_via;
        let mut via_found = false;
        for id in vias {
            let info = board.rules.via_infos.get(id).clone();
            let Some(padstack) = board.library.padstacks.get(info.padstack) else {
                continue;
            };
            if min_layer < padstack.from_layer() || max_layer > padstack.to_layer() {
                continue;
            }
            // make the current situation restorable by undo
            board.generate_snapshot();
            self.snapshots += 1;
            result = board.forced_via(
                &info,
                self.prev_corner,
                &self.net_numbers,
                self.params.clearance_class,
                &self.params.half_widths,
                self.depths.trace,
                0,
                self.tidy_width,
                self.accuracy,
                budget.pull_tight_ms.unwrap_or(0),
            );
            if result {
                via_found = true;
                break;
            }
            self.shove_failing_obstacle = board.shove_failing_obstacle();
            board.undo(None);
            self.snapshots -= 1;
        }
        if via_found {
            self.layer = to;
        }
        RouteLayerChange {
            ok: result,
            via: via_found,
        }
    }

    /// Java `Route.snapToSmdCenter`: connects the last corner to the centre
    /// of the first own-net SMD pin there on `layer`. False if there is no
    /// such pin.
    fn snap_to_smd_center(
        &mut self,
        board: &mut RoutingBoard,
        layer: i32,
        budget: RouteBudget,
    ) -> bool {
        let picked =
            board.pick_items_filtered(&Point::Int(self.prev_corner), layer, Some(&pins_filter()));
        let found: Option<IntPoint> = picked.into_iter().find_map(|id| {
            let item = board.item(id)?;
            let smd = item.is_pin()
                && item.shares_net_no(&self.net_numbers)
                && item.first_layer() == layer
                && item.last_layer() == layer;
            smd.then(|| item.drill().map(|d| d.center)).flatten()
        });
        let Some(center) = found else {
            return false;
        };
        if self.connect(board, self.prev_corner, center, budget) {
            self.prev_corner = center;
        }
        true
    }

    /// Ends the route and removes the trace tail at the last corner on the
    /// current layer up to the next via (Java `RouteState.cancel`): with
    /// pull-tight when push is on, else plainly.
    pub fn cancel(self, board: &mut RoutingBoard, settings: &EditSettings) {
        let Some(tail) = board.get_trace_tail(
            &Point::Int(self.prev_corner),
            self.current_layer,
            &self.net_numbers,
        ) else {
            return;
        };
        let remove: Vec<ItemId> = board
            .connection_items(tail, StopConnectionOption::Via)
            .into_iter()
            .collect();
        if settings.push_enabled {
            board.remove_items_and_pull_tight(
                &remove,
                settings.trace_tidy_width,
                settings.pull_tight_accuracy,
            );
        } else {
            board.remove_items(remove);
        }
    }
}

//! Route start: Java `RouteState.getInstance`, `startOk`, `pickRoutingItem`
//! and `getRouteNetNumbersAtTiePin`, and `Route`'s constructor
//! (`calculateTargetPointsAndAreas`, `calculateSwapPinInfos`).

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{FloatPoint, IntPoint, Point};

use super::{RouteMode, RouteSession, ShoveDepths, SwapPinInfo};
use crate::board::RoutingBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::board::selection_filter::{ItemFilter, SelectableChoice};
use crate::interactive::Status;
use crate::interactive::settings::{EditSettings, RouteParams, resolve_route_params};

/// Why a route could not start (Java `RouteState.getInstance` returns
/// `null`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    /// No routing item at the location on the layers searched.
    NothingPicked,
    /// The picked item has no nets, or a tie pin whose nets are all
    /// connected on the current layer.
    NoNets,
    /// The connection from the location to the picked trace failed.
    ConnectToTraceFailed,
    /// The first route net is not a net of the board.
    UnknownNet,
}

/// What [`RouteSession::start`] found.
#[derive(Clone, Debug, PartialEq)]
pub struct StartInfo {
    /// The item the route starts at.
    pub item: ItemId,
    /// The start corner.
    pub start: IntPoint,
    pub nets: Vec<i32>,
    /// The half widths, clearance class and via rule after a picked trace's
    /// own were taken over.
    pub params: RouteParams,
    pub mode: RouteMode,
    /// The current layer; differs from the settings' layer when the item
    /// was found on another layer.
    pub layer: i32,
    /// Java's `routing_net_message` with the net name.
    pub status: Status,
}

/// The filter of Java's `new ItemSelectionFilter(SelectableChoices.PINS)`.
pub(super) fn pins_filter() -> ItemFilter {
    ItemFilter::only(SelectableChoice::Pins)
}

impl RouteSession {
    /// Starts a route at `at` (Java `RouteState.getInstance`): picks the
    /// start item, resolves the route nets and rules, takes a snapshot and
    /// collects the targets.
    pub fn start(
        board: &mut RoutingBoard,
        settings: &EditSettings,
        at: FloatPoint,
    ) -> Result<(RouteSession, StartInfo), StartError> {
        let mut location = at.round();
        let (picked, layer) = start_ok(board, settings, location);
        let picked = picked.ok_or(StartError::NothingPicked)?;
        let item = board.item(picked).ok_or(StartError::NothingPicked)?;
        if item.net_count() == 0 {
            return Err(StartError::NoNets);
        }
        let nets = if item.is_pin() && item.net_count() > 1 {
            // tie pin, remove nets, which are already connected to this pin
            // on the current layer
            route_net_numbers_at_tie_pin(board, picked, layer)
        } else {
            item.net_numbers().to_vec()
        };
        if nets.is_empty() {
            return Err(StartError::NoNets);
        }
        let mut params = resolve_route_params(board, settings, &nets);
        let picked_item = item.clone();
        match &item.kind {
            ItemKind::Trace(trace) => {
                let (half_width, class, trace_layer) =
                    (trace.half_width, item.clearance_class(), trace.layer);
                let polyline = trace.polyline.clone();
                let end = board.nearest_end_point(picked, &Point::Int(location));
                match end {
                    Some(Point::Int(corner))
                        if at.distance(corner.to_float())
                            < f64::from(5i32.wrapping_mul(half_width)) =>
                    {
                        location = corner;
                    }
                    _ => {
                        if let Some(nearest) = polyline.nearest_point_approx(at) {
                            location = nearest.round();
                        }
                        if !board.connect_to_trace(location, picked, half_width, class) {
                            return Err(StartError::ConnectToTraceFailed);
                        }
                    }
                }
                if !settings.manual_rule_selection {
                    // pick up the half width and the clearance class of the
                    // found trace
                    if let Some(w) = params.half_widths.get_mut(trace_layer as usize) {
                        *w = half_width;
                    }
                    params.clearance_class = class;
                }
            }
            ItemKind::Pin(_) | ItemKind::Via(_) => {
                if let Some(drill) = item.drill() {
                    location = drill.center;
                }
            }
            _ => {}
        }
        let net = board
            .rules
            .nets
            .get(nets[0])
            .ok_or(StartError::UnknownNet)?;
        let net_name = net.name.clone();
        let class = board.rules.net_classes.get(net.net_class());
        // switch to stitch mode for nets, which are shove fixed
        let stitch = settings.stitch_route || class.is_shove_fixed() || !class.pull_tight();
        board.generate_snapshot();
        let target_set = if board.item(picked).is_some() {
            board.unconnected_set(picked, -1)
        } else {
            // connect_to_trace split the picked trace; Java asks the removed
            // trace object
            removed_trace_unconnected_set(board, &picked_item)
        };
        let mut session = RouteSession {
            net_numbers: nets.clone(),
            start_item: Some(picked),
            target_set: Some(target_set),
            swap_pins: BTreeMap::new(),
            params: params.clone(),
            depths: ShoveDepths::for_push(settings.push_enabled),
            tidy_width: settings.trace_tidy_width,
            accuracy: settings.pull_tight_accuracy,
            mode: if stitch {
                RouteMode::Stitch
            } else {
                RouteMode::Dynamic
            },
            neckdown: settings.automatic_neckdown,
            via_snap_to_smd_center: settings.via_snap_to_smd_center,
            highlight_obstacle: settings.highlight_routing_obstacle,
            prev_corner: location,
            layer,
            current_layer: layer,
            target_points: Vec::new(),
            target_traces_and_areas: Vec::new(),
            target_shapes: BTreeMap::new(),
            nearest_target_point: None,
            nearest_target_item: None,
            shove_failing_obstacle: None,
            snapshots: 1,
            corners: 0,
        };
        session.calculate_target_points_and_areas(board);
        session.swap_pins = session.calculate_swap_pin_infos(board);
        let info = StartInfo {
            item: picked,
            start: location,
            nets,
            params,
            mode: session.mode,
            layer,
            status: Status::new("routing_net_message", [net_name]),
        };
        Ok((session, info))
    }

    /// Java `Route.calculateTargetPointsAndAreas`: the centres of the drill
    /// targets, and the trace and conduction-area targets, in target order.
    fn calculate_target_points_and_areas(&mut self, board: &RoutingBoard) {
        for &id in self.target_set.iter().flatten() {
            let Some(item) = board.item(id) else {
                continue;
            };
            if let Some(drill) = item.drill() {
                self.target_points.push((drill.center.to_float(), id));
            } else if item.is_trace() || item.is_conduction_area() {
                self.target_traces_and_areas.push(id);
            }
        }
        for i in 0..self.target_traces_and_areas.len() {
            self.refresh_target_shape(board, self.target_traces_and_areas[i]);
        }
    }

    /// Java `Route.calculateSwapPinInfos`: the pins swappable with a target
    /// pin, then with the pins at the start corner on the route's layer.
    fn calculate_swap_pin_infos(&self, board: &RoutingBoard) -> BTreeMap<ItemId, SwapPinInfo> {
        let mut result = BTreeMap::new();
        let target_pins = self
            .target_set
            .iter()
            .flatten()
            .copied()
            .filter(|&id| board.item(id).is_some_and(|i| i.is_pin()));
        let picked = board.pick_items_filtered(
            &Point::Int(self.prev_corner),
            self.layer,
            Some(&pins_filter()),
        );
        let start_pins: Vec<ItemId> = picked
            .into_iter()
            .filter(|&id| board.item(id).is_some_and(|i| i.is_pin()))
            .collect();
        for pin in target_pins.chain(start_pins) {
            for swappable in board.swappable_pins(pin) {
                result
                    .entry(swappable)
                    .or_insert_with(|| swap_pin_info(board, swappable));
            }
        }
        result
    }
}

/// Java `SwapPinInfo(pin)`: the incomplete from an unconnected one-net
/// pin to the nearest other drill item of its net (squared distance, the
/// first of equals).
fn swap_pin_info(board: &RoutingBoard, pin: ItemId) -> SwapPinInfo {
    let mut info = SwapPinInfo {
        pin,
        incomplete: None,
    };
    let Some(item) = board.item(pin) else {
        return info;
    };
    if board.is_connected(pin) || item.net_count() != 1 {
        return info;
    }
    let Some(center) = item.drill().map(|d| d.center.to_float()) else {
        return info;
    };
    let mut min_dist = f64::MAX;
    let mut nearest = None;
    for id in board.connectable_items(item.net_number(0)) {
        if id == pin {
            continue;
        }
        let Some(point) = board
            .item(id)
            .and_then(|i| i.drill())
            .map(|d| d.center.to_float())
        else {
            continue;
        };
        let distance = center.distance_square(point);
        if distance < min_dist {
            min_dist = distance;
            nearest = Some(point);
        }
    }
    info.incomplete = nearest.map(|p| (center, p));
    info
}

/// Java `item.getUnconnectedSet(-1)` on a trace `stale` that is no longer
/// on the board: the connectable items of its nets without those connected
/// to its contacts at its end corners (Java `Trace.getNormalContacts(point,
/// false)` through the board's search tree).
fn removed_trace_unconnected_set(board: &RoutingBoard, stale: &Item) -> BTreeSet<ItemId> {
    let mut result: BTreeSet<ItemId> = stale
        .net_numbers()
        .iter()
        .flat_map(|&net| board.connectable_items(net))
        .collect();
    let Some(trace) = stale.as_trace() else {
        return result;
    };
    let mut connected = BTreeSet::new();
    for corner in [trace.first_corner(), trace.last_corner()] {
        for id in board.pick_items(&corner, trace.layer) {
            let Some(item) = board.item(id) else {
                continue;
            };
            if !item.shares_layer(stale) || !item.shares_net(stale) {
                continue;
            }
            let contact = match &item.kind {
                ItemKind::Trace(t) => corner == t.first_corner() || corner == t.last_corner(),
                ItemKind::Pin(_) | ItemKind::Via(_) => {
                    item.drill().is_some_and(|d| corner == Point::Int(d.center))
                }
                ItemKind::Area(area) if area.is_conduction_area() => area.area().contains(&corner),
                _ => false,
            };
            if contact && !connected.contains(&id) {
                connected.extend(board.connected_set(id, -1, false));
            }
        }
    }
    result.retain(|id| !connected.contains(id));
    result
}

/// Java `RouteState.startOk` with `pickRoutingItem`: the routing item at
/// `location` on the current layer, else (with "select on all visible
/// layers") on layer 0, the last layer, the inner signal layers and then the
/// inner non-signal layers, skipping the current and the invisible layers.
/// Returns the item and the current layer, which moves to the item's first
/// layer when it was found on another layer.
fn start_ok(
    board: &RoutingBoard,
    settings: &EditSettings,
    location: IntPoint,
) -> (Option<ItemId>, i32) {
    let point = Point::Int(location);
    let current = settings.layer;
    if let Some(found) = board.pick_nearest_routing_item(&point, current, None) {
        return (Some(found), current);
    }
    if !settings.select_on_all_visible_layers {
        return (None, current);
    }
    let layer_count = board.layer_count() as i32;
    let is_signal = |i: i32| board.layer_structure.layers[i as usize].is_signal;
    let inner_signal = (1..layer_count - 1).filter(|&i| is_signal(i));
    let inner_other = (1..layer_count - 1).filter(|&i| !is_signal(i));
    // prefer the outer layers, then the signal layers
    let order = [0, layer_count - 1]
        .into_iter()
        .chain(inner_signal)
        .chain(inner_other);
    for layer in order {
        if layer == current || !settings.visible_layers.is_visible(layer) {
            continue;
        }
        if let Some(found) = board.pick_nearest_routing_item(&point, layer, None) {
            let first = board.item(found).map_or(current, |i| i.first_layer());
            return (Some(found), first);
        }
    }
    (None, current)
}

/// Java `RouteState.getRouteNetNumbersAtTiePin`: the nets of tie pin `pin`
/// without those of its contacts on `layer`, ascending.
fn route_net_numbers_at_tie_pin(board: &RoutingBoard, pin: ItemId, layer: i32) -> Vec<i32> {
    let Some(item) = board.item(pin) else {
        return Vec::new();
    };
    let mut nets: BTreeSet<i32> = item.net_numbers().iter().copied().collect();
    for contact in board.normal_contacts(pin) {
        let Some(c) = board.item(contact) else {
            continue;
        };
        if c.first_layer() <= layer && c.last_layer() >= layer {
            for net in c.net_numbers() {
                nets.remove(net);
            }
        }
    }
    nets.into_iter().collect()
}

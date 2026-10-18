//! Ripup of obstacle rooms (A04.6).
//!
//! Port of `MazeRipupResolver` (`MazeRipupResolver.java`) and
//! `path/Connection.java` at `a773a70ec`.
//!
//! The ripup cost of an obstacle is the control's ripup costs times the
//! obstacle's width (a via: its widest trace, halved per extra contact),
//! divided by the detour of the connection it belongs to, and multiplied by
//! a fanout-via protection factor. Q11.5's rip-count protection (not Java)
//! then multiplies the rounded cost by the largest factor of the obstacle's
//! nets when [`AutorouteControl::ripup_factors`](crate::autoroute::control::AutorouteControl::ripup_factors)
//! is set; it is `None` on the parity path. The random detour factor `0.5 + r * r`
//! is drawn from the search's generator only from ripup pass 4 on, outside
//! every third pass (`:158-162`); the batch router never sets the pass
//! number, so the draw is dead at the pin (`// PARITY(quirk, java_quirks.ripup_pass_no_unset):`, A00
//! question 3).
//!
//! `Connection.get` caches the connection of every item it walks, as Java
//! caches it in `ItemAutorouteInfo` (`precalculatedConnection`), so the
//! first item asked decides the connection of all its items. The cache
//! lives for one search, as Java's lives for one engine.
//!
//! Java's walk along a closed loop of traces never ends; the port stops a
//! walk after `4 * (item count + 1)` steps, far more than any walk that
//! ends takes, so it only changes runs on which Java never returns.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::jmath::d2i;
use freerouting_geometry::{IntPoint, Line, Point, Polyline, PolylineShapeOps};

use super::doors::TRACE_WIDTH_TOLERANCE;
use super::queue::MazeListElement;
use super::search::{ALREADY_RIPPED_COSTS, MazeSearch};
use super::sink::{ExtraCost, SearchSink};
use super::{Adjustment, ExpandableRef, Room, RoomError};
use crate::board::basic_board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::search::TreeObject;
use crate::board::structure::FixedState;

/// Java `MazeRipupResolver.FANOUT_COST_CONSTANT`.
const FANOUT_COST_CONSTANT: f64 = 20000.0;
/// Java `Connection.DETOUR_ADD`.
const DETOUR_ADD: f64 = 100.0;
/// Java `Connection.DETOUR_ITEM_COST`.
const DETOUR_ITEM_COST: f64 = 0.1;

/// A routing connection ending at the next fork or terminal item (Java
/// `Connection`).
#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    /// `None` where the connection ends in empty space.
    pub start_point: Option<Point>,
    pub start_layer: i32,
    pub end_point: Option<Point>,
    pub end_layer: i32,
    /// The items, in Java's `TreeSet<Item>` order (descending id).
    pub items: BTreeSet<ItemId>,
}

impl Connection {
    /// Java `traceLength`: the trace lengths summed in item order.
    pub fn trace_length(&self, board: &BasicBoard) -> f64 {
        let mut result = 0.0;
        for id in &self.items {
            if let Some(trace) = board.item(*id).and_then(Item::as_trace) {
                result += trace.length();
            }
        }
        result
    }

    /// Java `getDetour` (`Connection.java:147-153`): `(length + 100) /
    /// (minimal length + 100) + 0.1 * (items - 1)`, or `Integer.MAX_VALUE`
    /// when an end point is missing.
    pub fn detour(&self, board: &BasicBoard) -> f64 {
        let (Some(start), Some(end)) = (&self.start_point, &self.end_point) else {
            return f64::from(i32::MAX);
        };
        let min_length = start.to_float().distance(end.to_float());
        // PARITY: Connection.java:152, in Java's operation order.
        (self.trace_length(board) + DETOUR_ADD) / (min_length + DETOUR_ADD)
            + DETOUR_ITEM_COST * (self.items.len() as f64 - 1.0)
    }
}

/// The connections of one search by item (Java's
/// `ItemAutorouteInfo.precalculatedConnection`).
#[derive(Clone, Debug, Default)]
pub struct ConnectionCache {
    connections: Vec<Connection>,
    by_item: BTreeMap<ItemId, usize>,
}

impl ConnectionCache {
    /// Java `Connection.get(item)` (`Connection.java:39-131`): the
    /// connection of `item`, computed and cached for all its items on first
    /// use. `None` for an item that is not routable.
    pub fn get(&mut self, board: &BasicBoard, item: ItemId) -> Option<&Connection> {
        if !board.item(item).is_some_and(Item::is_routable) {
            return None;
        }
        if let Some(&index) = self.by_item.get(&item) {
            return Some(&self.connections[index]);
        }
        let connection = walk_connection(board, item)?;
        let index = self.connections.len();
        for id in &connection.items {
            self.by_item.insert(*id, index);
        }
        self.connections.push(connection);
        Some(&self.connections[index])
    }
}

/// The walk of `Connection.get` from `start` along its contacts.
fn walk_connection(board: &BasicBoard, start: ItemId) -> Option<Connection> {
    let item = board.item(start)?;
    let mut items = BTreeSet::from([start]);
    let mut start_point: Option<Point> = None;
    let mut start_layer = 0;
    let mut end_point: Option<Point> = None;
    let mut end_layer = 0;
    for first_contact in board.normal_contacts(start) {
        let Some(contact) = board.item(first_contact) else {
            continue;
        };
        let Some(mut prev_point) = board.normal_contact_point(item, contact) else {
            // No unique contact point.
            continue;
        };
        let mut prev_layer = item.first_common_layer(contact);
        let mut fork =
            item.is_trace() && board.trace_normal_contacts(start, &prev_point, false).len() != 1;
        let mut current = first_contact;
        let mut steps_left = 4 * (board.item_count() + 1);
        loop {
            if steps_left == 0 {
                // Java loops forever on a closed loop of traces.
                break;
            }
            steps_left -= 1;
            let current_item = board.item(current)?;
            // N04.4: a trace reached in its middle (a T junction) ends the
            // connection, as the fork at the split trace does in Java.
            fork = fork || board.is_trace_body_contact(current_item, &prev_point);
            if !current_item.is_routable() || fork {
                // The connection ends.
                match &start_point {
                    None => {
                        start_point = Some(prev_point.clone());
                        start_layer = prev_layer;
                    }
                    Some(s) if *s != prev_point => {
                        end_point = Some(prev_point.clone());
                        end_layer = prev_layer;
                    }
                    Some(_) => {}
                }
                break;
            }
            items.insert(current);
            let mut next_point: Option<Point> = None;
            let mut next_layer = -1;
            let mut next: Option<ItemId> = None;
            for tmp in board.normal_contacts(current) {
                let Some(tmp_item) = board.item(tmp) else {
                    continue;
                };
                let tmp_layer = current_item.first_common_layer(tmp_item);
                if tmp_layer < 0 {
                    continue;
                }
                let Some(tmp_point) = board.normal_contact_point(current_item, tmp_item) else {
                    fork = true;
                    break;
                };
                if prev_layer != tmp_layer || prev_point != tmp_point {
                    // PARITY: Connection.java:105-113, the point is taken
                    // before the second new contact is found.
                    next_point = Some(tmp_point);
                    next_layer = tmp_layer;
                    if next.is_some() {
                        fork = true;
                        break;
                    }
                    next = Some(tmp);
                }
            }
            let Some(next_item) = next else {
                break;
            };
            current = next_item;
            prev_point = next_point.expect("a next contact has a point");
            prev_layer = next_layer;
        }
    }
    Some(Connection {
        start_point,
        start_layer,
        end_point,
        end_layer,
        items,
    })
}

/// Java `MazeRipupResolver.calcFanoutViaRipupCostFactor` (`:35-70`): a
/// factor above 1 protects the fanout via at an end of `trace` that ends at
/// an SMD pin or a shove-fixed two-corner trace.
pub fn calc_fanout_via_ripup_cost_factor(board: &BasicBoard, trace_id: ItemId) -> f64 {
    let Some(trace_item) = board.item(trace_id) else {
        return 1.0;
    };
    let Some(trace) = trace_item.as_trace() else {
        return 1.0;
    };
    for i in 0..2 {
        let contacts = if i == 0 {
            board.trace_start_contacts(trace_id)
        } else {
            board.trace_end_contacts(trace_id)
        };
        if contacts.len() != 1 {
            continue;
        }
        let Some(contact) = contacts.iter().next().and_then(|id| board.item(*id)) else {
            continue;
        };
        let protect = if contact.is_pin() && contact.first_layer() == contact.last_layer() {
            true
        } else if let Some(contact_trace) = contact.as_trace() {
            contact.fixed_state() == FixedState::ShoveFixed
                && contact_trace.polyline.corner_count() == 2
        } else {
            false
        };
        if protect {
            let mut factor = f64::from(trace.half_width) / trace.length();
            factor *= factor;
            factor *= FANOUT_COST_CONSTANT;
            return freerouting_geometry::jmath::max_f64(factor, 1.0);
        }
    }
    1.0
}

/// True if `ripup_pass_no` draws the random detour factor
/// (`MazeRipupResolver.java:158`).
pub fn draws_random_factor(ripup_pass_no: i32) -> bool {
    ripup_pass_no >= 4 && ripup_pass_no % 3 != 0
}

impl<S: SearchSink, C: ExtraCost> MazeSearch<'_, '_, S, C> {
    /// `checkRipup` (`:72-198`): the cost of ripping `obstacle` to enter the
    /// element's room, -1 if it cannot be ripped.
    pub(crate) fn check_ripup(
        &mut self,
        element: &MazeListElement,
        obstacle: ItemId,
        door_is_small: bool,
    ) -> Result<i32, RoomError> {
        let control = self.s.control;
        let Some(obstacle_item) = self.s.board.item(obstacle) else {
            return Ok(-1);
        };
        if !obstacle_item.is_routable() {
            return Ok(-1);
        }
        if door_is_small && !self.enter_through_small_door(element, obstacle)? {
            return Ok(-1);
        }
        let room = element.next_room.expect("the element has a next room");
        let previous_item =
            self.s
                .expandable_other_room(element.door, room)
                .and_then(|r| match self.s.room(r) {
                    Room::Obstacle { item, .. } => Some(*item),
                    _ => None,
                });
        let board = &*self.s.board;
        let obstacle_item = board.item(obstacle).expect("the obstacle exists");
        if element.adjustment != Adjustment::None {
            // The room was shoved.
            if let Some(previous) = previous_item
                && previous != obstacle
                && board
                    .item(previous)
                    .is_some_and(|p| p.shares_net(obstacle_item))
            {
                return Ok(-1);
            }
        } else if previous_item == Some(obstacle) {
            return Ok(ALREADY_RIPPED_COSTS);
        }
        let mut fanout_via_cost_factor = 1.0;
        let mut cost_factor = 1.0;
        let preserve_fanout_protection = !control.remove_unconnected_vias
            && control.ripup_costs <= self.settings.start_ripup_costs * 2;
        if let Some(trace) = obstacle_item.as_trace() {
            cost_factor = f64::from(trace.half_width);
            if preserve_fanout_protection {
                fanout_via_cost_factor = calc_fanout_via_ripup_cost_factor(board, obstacle);
            }
        } else if obstacle_item.is_via() {
            let mut look_if_fanout_via = preserve_fanout_protection;
            let mut contact_count = 0;
            for contact in board.normal_contacts(obstacle) {
                let Some(contact_item) = board.item(contact) else {
                    continue;
                };
                let Some(contact_trace) = contact_item.as_trace() else {
                    return Ok(-1);
                };
                if contact_item.is_user_fixed() {
                    return Ok(-1);
                }
                contact_count += 1;
                cost_factor = freerouting_geometry::jmath::max_f64(
                    cost_factor,
                    f64::from(contact_trace.half_width),
                );
                if look_if_fanout_via && !control.is_fanout {
                    let factor = calc_fanout_via_ripup_cost_factor(board, contact);
                    if factor > 1.0 {
                        fanout_via_cost_factor = factor;
                        look_if_fanout_via = false;
                    }
                }
            }
            if fanout_via_cost_factor <= 1.0 {
                cost_factor *= 0.5 * f64::from((contact_count - 1).max(0));
            }
        }
        let mut ripup_cost = f64::from(control.ripup_costs) * cost_factor;
        let mut detour = 1.0;
        if fanout_via_cost_factor <= 1.0 && !control.is_fanout {
            let board = &*self.s.board;
            if let Some(connection) = self.connections.get(board, obstacle) {
                detour = connection.detour(board);
            }
        }
        // PARITY(quirk, java_quirks.ripup_pass_no_unset):
        // MazeRipupResolver.java:158-162; the batch router never sets
        // ripupPassNo, so this is dead at the pin (`batch::connection` sets it
        // under `false`).
        if draws_random_factor(control.ripup_pass_no) {
            let random = self.random.next_double();
            let random_factor = 0.5 + random * random;
            detour *= random_factor;
        }
        ripup_cost /= detour;
        ripup_cost *= fanout_via_cost_factor;
        let mut result = d2i(ripup_cost).max(1);
        if let Some(factors) = &control.ripup_factors {
            // Q11.5 (not Java): rip-count protection, the largest factor of
            // the obstacle's nets; `None` on the parity path.
            let factor = self.s.board.item(obstacle).map_or(1, |item| {
                item.net_numbers()
                    .iter()
                    .filter_map(|net| factors.get(net))
                    .copied()
                    .max()
                    .unwrap_or(1)
            });
            result = (i64::from(result) * i64::from(factor)).min(i64::from(i32::MAX)) as i32;
        }
        Ok(result.min(i32::MAX / 100))
    }

    /// `checkLeavingRippedItem` (`:200-217`): entering a thick room from a
    /// ripped via or trace through a small door.
    pub(crate) fn check_leaving_ripped_item(
        &mut self,
        element: &MazeListElement,
    ) -> Result<bool, RoomError> {
        let ExpandableRef::Door(door) = element.door else {
            return Ok(false);
        };
        let room = element.next_room.expect("the element has a next room");
        let Some(from_room) = self.s.other_complete_room(door, room) else {
            return Ok(false);
        };
        let Room::Obstacle { item, .. } = self.s.room(from_room) else {
            return Ok(false);
        };
        let item = *item;
        if !self.s.board.item(item).is_some_and(Item::is_routable) {
            return Ok(false);
        }
        self.enter_through_small_door(element, item)
    }

    /// `enterThroughSmallDoor` (`:219-268`): the door can be entered when
    /// `ignore` and the items directly connected to it are ignored.
    fn enter_through_small_door(
        &self,
        element: &MazeListElement,
        ignore: ItemId,
    ) -> Result<bool, RoomError> {
        if self.s.expandable_dimension(element.door) != 1 {
            return Ok(false);
        }
        let door_shape = self.s.expandable_shape(element.door)?;
        let mut door_line: Option<Line> = None;
        let mut prev_corner = door_shape.corner_approx(0);
        for i in 1..door_shape.border_line_count() {
            let next_corner = door_shape.corner_approx(i);
            if next_corner.distance_square(prev_corner) > 1.0 {
                door_line = Some(door_shape.border_line(i - 1));
                break;
            }
            prev_corner = next_corner;
        }
        let Some(door_line) = door_line else {
            return Ok(false);
        };
        let door_center: IntPoint = door_shape.centre_of_gravity().round();
        let room = element.next_room.expect("the element has a next room");
        let layer = self.s.room(room).layer();
        // Java's TRACE_WIDTH_TOLERANCE is the int 2.
        let check_radius =
            self.s.room_compensated_half_width(room, layer as usize) + TRACE_WIDTH_TOLERANCE as i32;
        let freerouting_geometry::Direction::Int(direction) =
            door_line.direction().turn_45_degree(2)
        else {
            return Err(RoomError::NullPointer {
                at: "MazeRipupResolver.enterThroughSmallDoor",
            });
        };
        let lines = [
            door_line.translate(f64::from(check_radius)),
            Line::from_point_and_direction(door_center, direction),
            door_line.translate(f64::from(-check_radius)),
        ];
        let polyline = Polyline::new(&lines);
        let Some(check_shape) = polyline.offset_shape(check_radius, 0) else {
            return Ok(false);
        };
        let board = &*self.s.board;
        let net = [self.s.control.net_number];
        let objects = self
            .s
            .tree()
            .overlapping_objects(&check_shape, layer, &net, &board.items);
        let Some(ignore_item) = board.item(ignore) else {
            return Ok(false);
        };
        for object in objects {
            let TreeObject::Item(id) = object else {
                continue;
            };
            if id == ignore {
                continue;
            }
            let Some(current) = board.item(id) else {
                continue;
            };
            if !current.shares_net(ignore_item) {
                return Ok(false);
            }
            if !board.normal_contacts(id).contains(&ignore) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::prepare::{JobInput, RulesChoice, prepare_job};
    use crate::settings::ResolveContext;

    const BOARD: &str = r#"(pcb unit-ripup
  (parser (string_quote ") (space_in_quoted_tokens on))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer B.Cu (type signal) (property (index 1)))
    (boundary (path pcb 0  0 0  10000 0  10000 10000  0 10000  0 0))
    (via V600)
    (rule (width 250) (clearance 200))
  )
  (placement (component SMD (place U1 2000 2000 front 0) (place U2 8000 8000 front 0)))
  (library
    (image SMD (pin SmdPad 1 0 0))
    (padstack SmdPad (shape (rect F.Cu -400 -400 400 400)) (attach off))
    (padstack V600 (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network (net A (pins U1-1 U2-1)) (class kicad_default "" A (circuit (use_via V600))))
)
"#;

    /// The divergence of `Connection.get` (DIVERGENCES.md, "Maze search"):
    /// on a closed loop of three traces without forks Java's walk never
    /// ends; the port's ends after its step bound, with every trace of the
    /// loop in the connection and no end point.
    #[test]
    fn a_closed_loop_of_traces_ends_the_connection_walk() {
        let input = {
            let mut input = JobInput::new(BOARD.as_bytes().to_vec(), "unit-ripup");
            input.rules = RulesChoice::None;
            input
        };
        let mut board = prepare_job(&input, &ResolveContext { cores: 1 })
            .expect("the board loads")
            .board;
        let net = board
            .rules
            .nets
            .get_by_name("A", 1)
            .expect("net A")
            .net_number;
        let corners = [(4000, 4000), (6000, 4000), (5000, 6000)];
        let mut traces = Vec::new();
        for i in 0..3 {
            let (a, b) = (corners[i], corners[(i + 1) % 3]);
            let polyline = Polyline::from_points(&[
                IntPoint::new(a.0, a.1).into(),
                IntPoint::new(b.0, b.1).into(),
            ]);
            let id = board
                .insert_trace_without_cleaning(polyline, 0, 125, &[net], 1, FixedState::Unfixed)
                .expect("a trace");
            traces.push(id);
        }
        let mut cache = ConnectionCache::default();
        let connection = cache
            .get(&board, traces[0])
            .expect("a routable trace")
            .clone();
        assert_eq!(connection.items, traces.iter().copied().collect());
        assert_eq!(connection.end_point, None);
        // The connection is cached for every item it holds.
        assert_eq!(cache.get(&board, traces[2]), Some(&connection));
    }

    /// A04.6: the random detour factor is never drawn at the pin's pass 1,
    /// and is drawn at pass 4 (not at 3 or 6, every third pass).
    #[test]
    fn the_random_factor_is_drawn_only_from_pass_4_outside_every_third() {
        assert!(!draws_random_factor(1));
        assert!(!draws_random_factor(2));
        assert!(!draws_random_factor(3));
        assert!(draws_random_factor(4));
        assert!(draws_random_factor(5));
        assert!(!draws_random_factor(6));
        assert!(draws_random_factor(7));
    }
}

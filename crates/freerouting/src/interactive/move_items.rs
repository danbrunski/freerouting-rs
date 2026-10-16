//! Moving, turning and mirroring a selection (I04.2): the engine part of
//! Java's `gui.interactive.MoveItemState` at `a773a70ec`.
//!
//! Disabled in Java's GUI at the pin (`GuiBoardLegacyEditActions` makes
//! "move selected" a no-op and the state has no callers); enabled here by
//! decision (DIVERGENCES, "Interactive editor").
//!
//! A [`MoveSession`] is created from a selection ([`MoveSession::start`],
//! `MoveItemState.getInstance` and its constructor):
//! - the selection grows to the whole components of its items (a component
//!   without items stays out), and to the connected sets of its connected
//!   items (conduction areas ignored, so components with pins on a plane can
//!   move);
//! - it is refused if an item is user fixed or in a component with a fixed
//!   position, or if a connected item is outside the set (a trace that ends
//!   on a pin of another component, say). The refusal names the items to add
//!   to the selection: the fixed ones if there are any, else the obstacles;
//! - otherwise one snapshot is taken, the items are detached
//!   (`detach_item_for_move`) and copied with `copy(0)` (one id burned per
//!   item, highest id first). The copies float: they are not on the board,
//!   not in a search tree, and not in the undo list.
//!
//! The session then moves the floating copies and their components
//! ([`MoveSession::move_to`]), turns them ([`MoveSession::turn_90_degree`],
//! [`MoveSession::rotate`]), flips them to the other side
//! ([`MoveSession::change_placement_side`]) and resets the rotation of
//! their components ([`MoveSession::reset_rotation`]), the last three through
//! [`apply_transform`]; a transform that is refused leaves everything as it
//! was and gives `insertion_failed_because_of_obstacles`. [`complete`]
//! inserts the copies if none has a clearance violation, [`cancel`] undoes
//! the snapshot. Both end the session.
//!
//! [`complete`]: MoveSession::complete
//! [`cancel`]: MoveSession::cancel

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{FloatPoint, IntPoint, IntVector, Vector};

use super::Status;
use super::transform::{apply_transform, with_in_map};
use crate::board::items::{Item, ItemId};
use crate::board::structure::ComponentId;
use crate::board::{BasicBoard, ItemTransform, RoutingBoard};
use crate::drc::ClearanceViolation;

/// A move that was refused at the start (`MoveItemState.getInstance`
/// returns `null`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveRefusal {
    /// Items Java adds to the selection so that the user sees why: the user
    /// fixed items if there are any, else the items outside the move set.
    pub select: BTreeSet<ItemId>,
    /// Java's message key.
    pub status: Status,
}

impl MoveRefusal {
    fn status_only(key: &'static str) -> MoveRefusal {
        MoveRefusal {
            select: BTreeSet::new(),
            status: Status::new(key, []),
        }
    }
}

/// How a move session ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveOutcome {
    /// False if [`MoveSession::complete`] found a clearance violation: the
    /// session goes on.
    pub done: bool,
    /// The nets of the moved items (the ratsnest of these nets changes).
    pub nets: BTreeSet<i32>,
    pub status: Status,
}

/// The move of a selection: floating copies and their components.
#[derive(Clone, Debug)]
pub struct MoveSession {
    /// Java's `itemList`: the floating copies, highest id first.
    items: BTreeMap<ItemId, Item>,
    /// Java's `componentList`, in id order.
    components: Vec<ComponentId>,
    /// In case of a component grid the first component is aligned to it.
    grid_snap_component: Option<ComponentId>,
    grid: (i32, i32),
    /// The nets of the items (Java's `netItemsList`).
    nets: BTreeSet<i32>,
    current: IntPoint,
    previous: IntPoint,
    violations: Vec<ClearanceViolation>,
}

impl MoveSession {
    /// Java `MoveItemState.getInstance` and its constructor. `grid` is the
    /// workspace's horizontal and vertical component grid (0: none), `at`
    /// the pointer position.
    pub fn start(
        board: &mut RoutingBoard,
        selection: &BTreeSet<ItemId>,
        grid: (i32, i32),
        at: FloatPoint,
    ) -> Result<MoveSession, MoveRefusal> {
        if selection.is_empty() {
            return Err(MoveRefusal::status_only(
                "move_component_failed_because_no_item_selected",
            ));
        }
        let (all_items, components, grid_snap_component) =
            extend_to_components(board, selection, grid)?;
        let add_items = check_movable(board, &all_items, &components)?;
        let mut all_items = all_items;
        all_items.extend(add_items);
        Self::detach(board, &all_items, components, grid_snap_component, grid, at)
    }

    /// The constructor: snapshot, detach, copy.
    fn detach(
        board: &mut RoutingBoard,
        all_items: &BTreeSet<ItemId>,
        components: Vec<ComponentId>,
        grid_snap_component: Option<ComponentId>,
        grid: (i32, i32),
        at: FloatPoint,
    ) -> Result<MoveSession, MoveRefusal> {
        // Items the board keeps when detached (the conduction area of a
        // power plane) would be inserted twice; Java does that. They are
        // refused here (DIVERGENCES, "Interactive editor").
        let keeps = |id: &ItemId| {
            board
                .item(*id)
                .is_some_and(|i| i.component_id() <= 0 && board.is_deletion_forbidden(i))
        };
        if all_items.iter().any(keeps) {
            return Err(MoveRefusal::status_only(
                "some_items_cannot_be_moved_because_they_are_fixed",
            ));
        }
        // make the situation restorable by undo
        board.generate_snapshot();
        let originals: Vec<Item> = all_items
            .iter()
            .filter_map(|id| board.item(*id).cloned())
            .collect();
        for original in &originals {
            if board.detach_item_for_move(original.id()).is_err() {
                board.undo(None);
                return Err(MoveRefusal::status_only(
                    "some_items_cannot_be_moved_because_they_are_fixed",
                ));
            }
        }
        let mut items = BTreeMap::new();
        let mut nets = BTreeSet::new();
        for original in &originals {
            // Copy the items, because otherwise the undo algorithm will not work.
            let Some(copied) = board.copy_of(original, None) else {
                board.undo(None);
                return Err(MoveRefusal::status_only(
                    "some_items_cannot_be_moved_because_they_are_fixed",
                ));
            };
            nets.extend(copied.net_numbers().iter().copied());
            items.insert(copied.id(), copied);
        }
        let current = at.round();
        Ok(MoveSession {
            items,
            components,
            grid_snap_component,
            grid,
            nets,
            current,
            previous: current,
            violations: Vec::new(),
        })
    }

    /// The floating copies, highest id first.
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.items.values()
    }

    /// The components that move with the items.
    pub fn components(&self) -> &[ComponentId] {
        &self.components
    }

    /// The nets of the moved items, ascending.
    pub fn nets(&self) -> &BTreeSet<i32> {
        &self.nets
    }

    /// The clearance violations of the floating copies at the current
    /// position.
    pub fn violations(&self) -> &[ClearanceViolation] {
        &self.violations
    }

    /// The current position (the pole of a turn).
    pub fn position(&self) -> IntPoint {
        self.current
    }

    /// Java `MoveItemState.move`: moves the items and their components to
    /// `new_position` (rounded); nothing happens if the rounded position is
    /// the one before.
    pub fn move_to(&mut self, board: &mut RoutingBoard, new_position: FloatPoint) {
        self.current = new_position.round();
        if self.current == self.previous {
            return;
        }
        let mut translate = self.current.difference_by_int(self.previous);
        if let Some(snap) = self.grid_snap_component {
            translate = self.adjust_to_placement_grid(board, snap, translate);
        }
        let vector = Vector::Int(translate);
        for component in &self.components {
            board.components.move_by(*component, &vector);
        }
        let transform = ItemTransform::Translate(vector);
        for item in self.items.values_mut() {
            board.transform_detached(item, &transform);
        }
        self.violations = floating_violations(board, self.items.values());
        self.previous = self.current;
    }

    /// Java `adjustToPlacementGrid`: rounds the first component's new
    /// location to the grid and moves the current position by the same.
    fn adjust_to_placement_grid(
        &mut self,
        board: &BasicBoard,
        snap: ComponentId,
        vector: IntVector,
    ) -> IntVector {
        let Some(location) = board.components.get(snap).and_then(|c| c.location()) else {
            return vector;
        };
        let new_location = location.translate_by_int(vector);
        let rounded = new_location
            .to_float()
            .round_to_grid(self.grid.0, self.grid.1);
        let adjustment = rounded.difference_by_int(new_location);
        let result = IntVector::new(
            vector.x.wrapping_add(adjustment.x),
            vector.y.wrapping_add(adjustment.y),
        );
        self.current = self.previous.translate_by_int(result).to_float().round();
        result
    }

    /// Java `turn90Degree(factor)`: turns by `factor` times 90 degrees
    /// around the current position.
    pub fn turn_90_degree(&mut self, board: &mut RoutingBoard, factor: i32) -> Option<Status> {
        if factor == 0 {
            return None;
        }
        self.transform_items(board, f64::from(factor) * 90.0, false)
    }

    /// Java `rotate(angleInDegree)` around the current position; 0 does
    /// nothing.
    pub fn rotate(&mut self, board: &mut RoutingBoard, degrees: f64) -> Option<Status> {
        if degrees == 0.0 {
            return None;
        }
        self.transform_items(board, degrees, false)
    }

    /// Java `changePlacementSide`: flips the items to the other side.
    pub fn change_placement_side(&mut self, board: &mut RoutingBoard) -> Option<Status> {
        // Check that all items can be mirrored (Java's loop: a via by its
        // padstack, any other single-layer item by the layer it would land on).
        let layer_count = board.layer_count() as i32;
        for item in self.items.values() {
            let mirrored_ok = if let Some(via) = item.as_via() {
                board
                    .library
                    .get_mirrored_via_padstack(via.drill.padstack)
                    .is_some()
            } else if item.first_layer() == item.last_layer() {
                let other = layer_count - item.first_layer() - 1;
                usize::try_from(other)
                    .ok()
                    .and_then(|i| board.layer_structure.layers.get(i))
                    .is_some_and(|l| l.is_signal)
            } else {
                true
            };
            if !mirrored_ok {
                return Some(Status::new("cannot_change_placement_side", []));
            }
        }
        self.transform_items(board, 0.0, true)
    }

    /// Java `resetRotation`: turns the components back to rotation 0. Gives
    /// `unable_to_reset_components_with_different_rotations` if they differ.
    pub fn reset_rotation(&mut self, board: &mut RoutingBoard) -> Option<Status> {
        let mut to_reset: Option<f64> = None;
        let mut on_front = true;
        for id in &self.components {
            let component = board.components.get(*id)?;
            match to_reset {
                None => {
                    to_reset = Some(component.rotation_in_degree());
                    on_front = component.placed_on_front();
                }
                Some(rotation) if rotation != component.rotation_in_degree() => {
                    return Some(Status::new(
                        "unable_to_reset_components_with_different_rotations",
                        [],
                    ));
                }
                Some(_) => {}
            }
        }
        let mut rotation = to_reset?;
        if !board.components.flip_style_rotate_first() || on_front {
            rotation = 360.0 - rotation;
        }
        self.rotate(board, rotation)
    }

    /// Java `transformItems`.
    fn transform_items(
        &mut self,
        board: &mut RoutingBoard,
        angle: f64,
        mirror: bool,
    ) -> Option<Status> {
        let floating: Vec<Item> = self.items.values().cloned().collect();
        match apply_transform(board, &floating, angle, mirror, self.current) {
            Err(_) => Some(Status::new("insertion_failed_because_of_obstacles", [])),
            Ok(transformed) => {
                self.items = transformed.into_iter().map(|i| (i.id(), i)).collect();
                self.nets = self
                    .items
                    .values()
                    .flat_map(|i| i.net_numbers().iter().copied())
                    .collect();
                self.violations = floating_violations(board, self.items.values());
                None
            }
        }
    }

    /// Java `complete`: inserts the copies unless one has a clearance
    /// violation (then `insertion_failed_because_of_obstacles`, `done` false
    /// and the session goes on).
    pub fn complete(&mut self, board: &mut RoutingBoard) -> MoveOutcome {
        let blocked = floating_violations(board, self.items.values());
        if !blocked.is_empty() {
            self.violations = blocked;
            return MoveOutcome {
                done: false,
                nets: self.nets.clone(),
                status: Status::new("insertion_failed_because_of_obstacles", []),
            };
        }
        for item in std::mem::take(&mut self.items).into_values() {
            board.insert_item(item);
        }
        MoveOutcome {
            done: true,
            nets: self.nets.clone(),
            status: Status::new("move_completed", []),
        }
    }

    /// Java `cancel`: undoes the snapshot of the start, which restores the
    /// items and the components.
    pub fn cancel(self, board: &mut RoutingBoard) -> BTreeSet<i32> {
        board.undo(None);
        self.nets
    }
}

/// The clearance violations of floating items against the board (Java
/// `Item.clearanceViolations` on items that are in no search tree).
pub(crate) fn floating_violations<'a>(
    board: &mut BasicBoard,
    items: impl Iterator<Item = &'a Item>,
) -> Vec<ClearanceViolation> {
    let items: Vec<Item> = items.cloned().collect();
    with_in_map(board, &items, |b| {
        items
            .iter()
            .flat_map(|item| b.clearance_violations(item.id()))
            .collect()
    })
}

/// The first half of `getInstance`: the selection extended to the whole
/// components, the components, and the component that snaps to the grid.
#[allow(clippy::type_complexity)]
fn extend_to_components(
    board: &BasicBoard,
    selection: &BTreeSet<ItemId>,
    grid: (i32, i32),
) -> Result<(BTreeSet<ItemId>, Vec<ComponentId>, Option<ComponentId>), MoveRefusal> {
    let mut all_items = BTreeSet::new();
    let mut components: BTreeSet<ComponentId> = BTreeSet::new();
    let mut grid_snap = None;
    for id in selection {
        let Some(item) = board.item(*id) else {
            continue;
        };
        if item.component_id() > 0 {
            let component = ComponentId(item.component_id());
            if board.components.get(component).is_none() {
                // Java logs "inconsistent component number" and gives up.
                return Err(MoveRefusal::status_only(
                    "move_component_failed_because_no_item_selected",
                ));
            }
            if grid_snap.is_none() && (grid.0 > 0 || grid.1 > 0) {
                grid_snap = Some(component);
            }
            if !components.contains(&component) {
                for item_id in board.component_items(component.0) {
                    components.insert(component);
                    all_items.insert(item_id);
                }
            }
        } else {
            all_items.insert(*id);
        }
    }
    Ok((all_items, components.into_iter().collect(), grid_snap))
}

/// The second half of `getInstance`: the fixed-state and connectivity
/// checks. Returns the items of the connected sets to add.
fn check_movable(
    board: &BasicBoard,
    all_items: &BTreeSet<ItemId>,
    components: &[ComponentId],
) -> Result<BTreeSet<ItemId>, MoveRefusal> {
    let mut move_ok = true;
    let mut fixed_items = BTreeSet::new();
    let mut obstacle_items = BTreeSet::new();
    let mut add_items = BTreeSet::new();
    for id in all_items {
        let Some(item) = board.item(*id) else {
            continue;
        };
        let position_fixed = item.component_id() > 0
            && board
                .components
                .get(ComponentId(item.component_id()))
                .is_some_and(|c| c.position_fixed);
        if item.is_user_fixed() || position_fixed {
            move_ok = false;
            obstacle_items.insert(*id);
            fixed_items.insert(*id);
        } else if board.is_connected(*id) {
            // Check if the whole connected set is inside the selected items,
            // and add the items of the connected set to the move list in
            // this case. Conduction areas are ignored, because otherwise
            // components with pins contacted to a plane could never be moved.
            let mut movable = true;
            for contact_id in board.connected_set(*id, -1, true) {
                let Some(contact) = board.item(contact_id) else {
                    continue;
                };
                if contact.is_conduction_area() {
                    continue;
                }
                if contact.is_user_fixed() {
                    movable = false;
                    fixed_items.insert(contact_id);
                } else if contact.component_id() != 0
                    && !components.contains(&ComponentId(contact.component_id()))
                {
                    movable = false;
                }
                if movable {
                    add_items.insert(contact_id);
                } else {
                    obstacle_items.insert(contact_id);
                }
            }
            if !movable {
                move_ok = false;
            }
        }
    }
    if move_ok {
        return Ok(add_items);
    }
    Err(if fixed_items.is_empty() {
        MoveRefusal {
            select: obstacle_items,
            status: Status::new("please_unroute_or_extend_selection_before_moving", []),
        }
    } else {
        MoveRefusal {
            select: fixed_items,
            status: Status::new("please_unfix_selected_items_before_moving", []),
        }
    })
}

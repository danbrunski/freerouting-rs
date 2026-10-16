//! Moving a component or a single item while shoving traces aside (I02.1).
//!
//! Port of `board.actions.MoveComponent` at `a773a70ec`: the group of a
//! drag (the item's component, or the item alone), sorted so that the items
//! in front in the move direction move first, a check that every item can
//! move by the vector (shoving obstacle traces and vias aside where needed)
//! and the insert that moves them.
//!
//! Orders that decide results, as in Java:
//! - the group is the component's items in item-list order (highest id
//!   first), or the item alone;
//! - the gravity point is the mean of the drill centres (else the component
//!   location, else the bounding-box centres), each coordinate summed in
//!   `f64` in group order and rounded with Java's `Math.round`;
//! - the group is sorted by `(gravity - centre) . vector` with a stable sort
//!   and Java's `Signum` comparison, so ties keep the item-list order.
//!
//! Java keeps the group, the items to shove and the check's ignore
//! collection in `HashSet<Item>`s, whose iteration follows identity hashes.
//! At the pin they are used only for membership (`contains`, `removeAll` and
//! a copy whose elements are only tested for membership), and the ignore
//! collection holds the whole group from the start, so the drill items the
//! check adds change nothing. The port keeps them as sorted sets; no result
//! depends on their order (`move_component_order_is_item_id_order`).
//!
//! Budgets are Java's work limits ([`MoveBudget`]): one limit of 3000
//! nominal ms for the whole check and the 1000 nominal ms pull-tight limit
//! of each `move_drill_item`. The clock is never read.

use std::collections::BTreeSet;

use freerouting_geometry::jmath::round_to_i32;
use freerouting_geometry::{FloatPoint, IntPoint, Point, PolylineShapeOps, TileShape, Vector};

use crate::board::RoutingBoard;
use crate::board::items::{ItemId, ItemKind};
use crate::board::search::WorkLimit;
use crate::board::structure::ComponentId;
use crate::shove::TraceShover;

/// Java `MoveComponent.CHECK_LIMIT_NOMINAL_MS`.
pub const MOVE_COMPONENT_CHECK_NOMINAL_MS: i64 = 3000;
/// Java `MoveComponent.PULL_TIGHT_LIMIT_NOMINAL_MS`.
pub const MOVE_COMPONENT_PULL_TIGHT_NOMINAL_MS: i64 = 1000;

/// The work limits of a component move, in nominal milliseconds (`None`:
/// unlimited).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveBudget {
    /// The one limit of the whole check (Java 3000).
    pub check_ms: Option<i64>,
    /// The pull-tight limit of each shoving drill-item move (Java 1000).
    pub pull_tight_ms: Option<i64>,
}

impl MoveBudget {
    /// Java's limits at the pin.
    pub const JAVA: MoveBudget = MoveBudget {
        check_ms: Some(MOVE_COMPONENT_CHECK_NOMINAL_MS),
        pull_tight_ms: Some(MOVE_COMPONENT_PULL_TIGHT_NOMINAL_MS),
    };
    /// No limits.
    pub const UNLIMITED: MoveBudget = MoveBudget {
        check_ms: None,
        pull_tight_ms: None,
    };
}

impl Default for MoveBudget {
    fn default() -> Self {
        MoveBudget::JAVA
    }
}

/// A group item with its projection on the move vector (Java
/// `MoveComponent.SortedItem`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SortedItem {
    pub item: ItemId,
    /// `(gravity point - item centre) . vector`; smaller is further in front.
    pub projection: f64,
}

/// A planned move of an item's group by a vector (Java `MoveComponent`).
#[derive(Clone, Debug)]
pub struct MoveComponent {
    vector: Vector,
    max_recursion_depth: i32,
    max_via_recursion_depth: i32,
    /// The group in move order.
    group: Vec<SortedItem>,
    group_set: BTreeSet<ItemId>,
    items_to_shove: BTreeSet<ItemId>,
    all_items_movable: bool,
    component: Option<ComponentId>,
}

/// Java `MoveComponent.requiresDrillItemMover`: drill item `id` has a trace
/// among its normal contacts or more than one net, so it moves with
/// [`RoutingBoard::move_drill_item`].
pub fn requires_drill_item_mover(board: &RoutingBoard, id: ItemId) -> bool {
    let Some(item) = board.item(id) else {
        return false;
    };
    let has_connecting_traces = board
        .normal_contacts(id)
        .into_iter()
        .any(|c| board.item(c).is_some_and(|c| c.is_trace()));
    has_connecting_traces || item.net_count() > 1
}

/// Java `Signum.asInt(a - b)` as an ordering.
fn signum_order(a: f64, b: f64) -> std::cmp::Ordering {
    let d = a - b;
    if d > 0.0 {
        std::cmp::Ordering::Greater
    } else if d < 0.0 {
        std::cmp::Ordering::Less
    } else {
        std::cmp::Ordering::Equal
    }
}

impl MoveComponent {
    /// Java `new MoveComponent(item, translateVector, maxRecursionDepth,
    /// maxViaRecursionDepth)`: the group of `item` sorted for a move by
    /// `vector`.
    pub fn new(
        board: &RoutingBoard,
        item: ItemId,
        vector: Vector,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
    ) -> Self {
        let mut result = MoveComponent {
            vector,
            max_recursion_depth,
            max_via_recursion_depth,
            group: Vec::new(),
            group_set: BTreeSet::new(),
            items_to_shove: BTreeSet::new(),
            all_items_movable: true,
            component: None,
        };
        let Some(picked) = board.item(item) else {
            result.all_items_movable = false;
            return result;
        };
        let component_id = picked.component_id();
        let group_list = if component_id > 0 {
            let id = ComponentId(component_id);
            result.component = board.components.get(id).map(|_| id);
            board.component_items(component_id)
        } else {
            vec![item]
        };
        let mut centers: Vec<FloatPoint> = Vec::new();
        for &id in &group_list {
            let current = board.item_ref(id);
            let movable = !current.is_user_fixed()
                && matches!(
                    current.kind,
                    ItemKind::Pin(_)
                        | ItemKind::Via(_)
                        | ItemKind::Area(_)
                        | ItemKind::ComponentOutline(_)
                );
            if !movable {
                // MoveItemGroup currently only implemented for DrillItems
                result.all_items_movable = false;
                return result;
            }
            if let Some(drill) = current.drill() {
                centers.push(drill.center.to_float());
            }
        }
        result.group_set = group_list.iter().copied().collect();
        if centers.is_empty() {
            match result
                .component
                .and_then(|c| board.components.get(c))
                .and_then(|c| c.location())
            {
                Some(location) => centers.push(location.to_float()),
                None => {
                    for &id in &group_list {
                        centers.push(box_centre(board, id));
                    }
                }
            }
        }
        // calculate the gravity point of all item centers
        let (mut gravity_x, mut gravity_y) = (0.0f64, 0.0f64);
        for center in &centers {
            gravity_x += center.x;
            gravity_y += center.y;
        }
        if !centers.is_empty() {
            gravity_x /= centers.len() as f64;
            gravity_y /= centers.len() as f64;
        }
        let gravity = Point::Int(IntPoint::new(
            round_to_i32(gravity_x),
            round_to_i32(gravity_y),
        ));
        result.group = group_list
            .iter()
            .map(|&id| {
                let center = match board.item_ref(id).drill() {
                    Some(drill) => Point::Int(drill.center),
                    None => Point::Int(box_centre(board, id).round()),
                };
                let compare = gravity.difference_by(&center);
                SortedItem {
                    item: id,
                    projection: compare.scalar_product(&result.vector),
                }
            })
            .collect();
        // sort the items in the direction of the vector, so that the items
        // in front come first (stable, as Java's Arrays.sort of objects)
        result
            .group
            .sort_by(|a, b| signum_order(a.projection, b.projection));
        result
    }

    /// The move vector.
    pub fn vector(&self) -> &Vector {
        &self.vector
    }

    /// The group in move order (empty if it is not movable).
    pub fn group(&self) -> &[SortedItem] {
        &self.group
    }

    /// The component of the group, if the item belongs to one.
    pub fn component(&self) -> Option<ComponentId> {
        self.component
    }

    /// False if an item of the group is user fixed or is no drill item,
    /// obstacle area or component outline.
    pub fn is_movable(&self) -> bool {
        self.all_items_movable
    }

    /// The drill items the last [`check`](Self::check) found that need
    /// shoving although they have no traces and one net.
    pub fn items_to_shove(&self) -> &BTreeSet<ItemId> {
        &self.items_to_shove
    }

    /// Java `getMinDrillItemWidthWithTraces`: the smallest minimum width of
    /// the group's drill items that need the drill-item mover, or -1.
    pub fn min_drill_item_width_with_traces(&self, board: &RoutingBoard) -> f64 {
        let mut min = f64::MAX;
        for sorted in &self.group {
            if board.item(sorted.item).is_some_and(|i| i.is_drill_item())
                && requires_drill_item_mover(board, sorted.item)
                && let Some(width) = board.min_width(sorted.item)
            {
                min = min.min(width);
            }
        }
        if min == f64::MAX { -1.0 } else { min }
    }

    /// Java `MoveComponent.check`: true if every item of the group can move
    /// by the vector, shoving obstacle traces aside, without clearance
    /// violations. Does not change the board.
    pub fn check(&mut self, board: &mut RoutingBoard, budget: MoveBudget) -> bool {
        if !self.all_items_movable {
            return false;
        }
        self.items_to_shove.clear();
        let mut limit = budget
            .check_ms
            .map(|ms| WorkLimit::with_site(board.search_trees.work_meter(), ms, "move_component"));
        let mut ignore = self.group_set.clone();
        let length = self.vector.length_approx();
        for i in 0..self.group.len() {
            let id = self.group[i].item;
            let move_ok = if board.item(id).is_some_and(|it| it.is_drill_item()) {
                let min_width = board.min_width(id).unwrap_or(f64::from(i32::MAX));
                if requires_drill_item_mover(board, id) {
                    if length >= min_width {
                        // a clearance violation with a connecting trace may occur
                        false
                    } else {
                        self.check_drill_mover(board, id, &mut ignore, limit.as_mut())
                    }
                } else {
                    // a drill item with at most one net and no connecting
                    // traces can move freely if the target space has no
                    // obstacles
                    let free = board.check_move_item(id, &self.vector, Some(&mut ignore));
                    if !free && length < min_width {
                        // the target space has obstacles; can they be shoved aside?
                        self.check_drill_mover(board, id, &mut ignore, limit.as_mut())
                    } else {
                        free
                    }
                }
            } else {
                board.check_move_item(id, &self.vector, Some(&mut ignore))
            };
            if !move_ok {
                return false;
            }
        }
        true
    }

    /// `DrillItemMover.check` of drill item `id` with the shared ignore
    /// collection; a success adds `id` to the items to shove.
    fn check_drill_mover(
        &mut self,
        board: &mut RoutingBoard,
        id: ItemId,
        ignore: &mut BTreeSet<ItemId>,
        limit: Option<&mut WorkLimit>,
    ) -> bool {
        let mut ignore_list: Vec<ItemId> = ignore.iter().copied().collect();
        let ok = TraceShover::new(board).check_drill_item_move(
            id,
            &self.vector,
            self.max_recursion_depth,
            self.max_via_recursion_depth,
            &mut ignore_list,
            limit,
        );
        ignore.extend(ignore_list);
        if ok {
            self.items_to_shove.insert(id);
        }
        ok
    }

    /// Java `MoveComponent.insert(tidyWidth, pullTightAccuracy)`: moves the
    /// group by the vector, shoving obstacle traces aside. False if that
    /// failed; the board may then hold a partial move (undo it).
    pub fn insert(
        &self,
        board: &mut RoutingBoard,
        tidy_width: i32,
        pull_tight_accuracy: i32,
        budget: MoveBudget,
    ) -> bool {
        if !self.all_items_movable {
            return false;
        }
        if let Some(component) = self.component {
            // the component must move first, so that the new pin shapes
            // are calculated correctly
            board.components.move_by(component, &self.vector);
        }
        for sorted in &self.group {
            let id = sorted.item;
            if board.item(id).is_some_and(|i| i.is_drill_item()) {
                let move_ok =
                    if self.items_to_shove.contains(&id) || requires_drill_item_mover(board, id) {
                        board.move_drill_item(
                            id,
                            &self.vector,
                            self.max_recursion_depth,
                            self.max_via_recursion_depth,
                            tidy_width,
                            pull_tight_accuracy,
                            budget.pull_tight_ms.unwrap_or(0),
                        )
                    } else {
                        board.move_item(id, &self.vector);
                        true
                    };
                if !move_ok {
                    if let Some(component) = self.component {
                        // otherwise the undo does not restore the outline
                        board.components.move_by(component, &self.vector.negate());
                    }
                    return false;
                }
            } else {
                board.move_item(id, &self.vector);
            }
        }
        if board.outline().is_some() {
            board.invalidate_edge_pin_nets();
        }
        true
    }
}

/// Java `item.boundingBox().centreOfGravity()`: the mean of the box corners.
fn box_centre(board: &RoutingBoard, id: ItemId) -> FloatPoint {
    TileShape::from(board.item_ref(id).bounding_box()).centre_of_gravity()
}

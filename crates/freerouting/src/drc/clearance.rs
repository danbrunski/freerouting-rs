//! Clearance violations between items.
//!
//! Port of `drc.ClearanceViolation` and of `Item.clearanceViolations`
//! (with the `Via` override).

use std::collections::HashSet;

use freerouting_geometry::jmath::round_to_i32;
use freerouting_geometry::{FloatPoint, PolylineShapeOps, TileShape};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::board::structure::Unit;

/// The category of a clearance violation by fixability and item types.
///
/// Port of `ClearanceViolation.Category`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClearanceViolationCategory {
    PinToPin,
    PinToOutlineOrKeepout,
    FixedRoute,
    OtherUnfixable,
    PotentiallyFixable,
}

/// A clearance violation between two items.
///
/// Port of `drc.ClearanceViolation`. A viewer draws [`shape`](Self::shape)
/// on [`layer`](Self::layer): it is the overlap of the two item shapes,
/// each enlarged by its half of the required clearance (board
/// coordinates). The clearances are in board units.
#[derive(Clone, Debug)]
pub struct ClearanceViolation {
    /// The item whose clearance check found the violation.
    pub first_item: ItemId,
    /// The item too close to `first_item`.
    pub second_item: ItemId,
    /// The violation area.
    pub shape: TileShape,
    /// The layer of the violation.
    pub layer: i32,
    /// The required clearance.
    pub expected_clearance: f64,
    /// The clearance found (0 if the items overlap).
    pub actual_clearance: f64,
}

impl ClearanceViolation {
    /// The amount the clearance is too small, in board units.
    pub fn shortfall(&self) -> f64 {
        self.expected_clearance - self.actual_clearance
    }

    /// The centre of the violation area.
    pub fn location(&self) -> FloatPoint {
        self.shape.centre_of_gravity()
    }

    /// Java `isUnfixable`: neither item can be ripped up or rerouted.
    pub fn is_unfixable(&self, board: &BasicBoard) -> bool {
        let routable = |id: ItemId| board.item(id).is_some_and(Item::is_routable);
        !routable(self.first_item) && !routable(self.second_item)
    }

    /// Java `getCategory`.
    pub fn category(&self, board: &BasicBoard) -> ClearanceViolationCategory {
        if !self.is_unfixable(board) {
            return ClearanceViolationCategory::PotentiallyFixable;
        }
        let first = board.item(self.first_item);
        let second = board.item(self.second_item);
        let is_pin = |item: Option<&Item>| item.is_some_and(Item::is_pin);
        let is_outline_or_keepout = |item: Option<&Item>| {
            item.is_some_and(|i| {
                matches!(
                    i.kind,
                    ItemKind::BoardOutline(_) | ItemKind::ComponentOutline(_) | ItemKind::Area(_)
                )
            })
        };
        let is_route = |item: Option<&Item>| item.is_some_and(|i| i.is_trace() || i.is_via());
        if is_pin(first) && is_pin(second) {
            return ClearanceViolationCategory::PinToPin;
        }
        if (is_pin(first) && is_outline_or_keepout(second))
            || (is_pin(second) && is_outline_or_keepout(first))
        {
            return ClearanceViolationCategory::PinToOutlineOrKeepout;
        }
        if is_route(first) || is_route(second) {
            return ClearanceViolationCategory::FixedRoute;
        }
        ClearanceViolationCategory::OtherUnfixable
    }
}

/// Java `ClearanceViolation.aggregateSortedBySeverity`: the violations of
/// all `items` (a pair appears once per item), largest shortfall first.
/// Returns the violations and the smallest actual clearance among them
/// (`f64::MAX` if there is none, Java `smallestClearance`).
pub fn aggregate_sorted_by_severity(
    board: &BasicBoard,
    items: &[ItemId],
) -> (Vec<ClearanceViolation>, f64) {
    let mut result = Vec::new();
    for id in items {
        result.extend(board.clearance_violations(*id));
    }
    let smallest = result
        .iter()
        .map(|v| v.actual_clearance)
        .filter(|c| *c >= 0.0)
        .fold(f64::MAX, f64::min);
    // A stable sort like Java's `List.sort`.
    result.sort_by(|a, b| b.shortfall().total_cmp(&a.shortfall()));
    (result, smallest)
}

/// `inputs.iter().map(f)` on all cores, in input order.
pub(crate) fn parallel_map<T: Sync, R: Send>(inputs: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    if threads <= 1 || inputs.len() < 2 {
        return inputs.iter().map(f).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut results: Vec<(usize, R)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(input) = inputs.get(index) else {
                            return done;
                        };
                        done.push((index, f(input)));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("worker panicked"))
            .collect()
    });
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, r)| r).collect()
}

impl BasicBoard {
    /// The clearance violations of item `id` with other items; the first
    /// item of each violation is `id`.
    ///
    /// Port of `Item.clearanceViolations` and its `Via` override, which
    /// drops same-net violations of an escape via on its SMD layer.
    pub fn clearance_violations(&self, id: ItemId) -> Vec<ClearanceViolation> {
        let Some(item) = self.item(id) else {
            return Vec::new();
        };
        let raw = self.item_clearance_violations(item);
        match &item.kind {
            ItemKind::Via(via) if via.is_escape_via && via.escape_via_smd_layer >= 0 => raw
                .into_iter()
                .filter(|v| {
                    if v.layer != via.escape_via_smd_layer {
                        return true;
                    }
                    let other = if v.first_item == id {
                        Some(v.second_item)
                    } else if v.second_item == id {
                        Some(v.first_item)
                    } else {
                        None
                    };
                    !other
                        .and_then(|o| self.item(o))
                        .is_some_and(|o| o.shares_net(item))
                })
                .collect(),
            _ => raw,
        }
    }

    /// All clearance violations of the board, each pair of items and
    /// layer once, in item order (highest id first).
    ///
    /// Port of `DesignRulesChecker.getAllClearanceViolations`.
    ///
    /// The items are checked on all cores; the result does not depend on
    /// the number of threads.
    pub fn all_clearance_violations(&self) -> Vec<ClearanceViolation> {
        let ids: Vec<ItemId> = self.items().map(Item::id).collect();
        let per_item = parallel_map(&ids, |id| self.clearance_violations(*id));
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        for violations in per_item {
            for violation in violations {
                let (a, b) = (violation.first_item.0, violation.second_item.0);
                let key = (a.min(b), a.max(b), violation.layer);
                if seen.insert(key) {
                    result.push(violation);
                }
            }
        }
        result
    }

    /// The factor converting board units to micrometers.
    pub(crate) fn board_unit_to_um_factor(&self) -> f64 {
        Unit::scale(1.0, self.communication.unit, Unit::Um)
            / f64::from(self.communication.resolution.max(1))
    }

    fn item_clearance_violations(&self, item: &Item) -> Vec<ClearanceViolation> {
        let mut result = Vec::new();
        let tree = self.search_trees.default_tree();
        let matrix = &self.rules.clearance_matrix;
        let compensation_used = self.search_trees.is_clearance_compensation_used();
        let unit_to_um = self.board_unit_to_um_factor();
        let mut tolerance_um = self.rules.clearance_tolerance_um;
        if !tolerance_um.is_finite() || tolerance_um < 0.0 {
            tolerance_um = 0.0;
        }
        let id = item.id();
        for i in 0..self.tile_shape_count(item) {
            let Some(current_shape) = self.tile_shape(item, i) else {
                continue;
            };
            let layer = item.shape_layer(i);
            // Only the obstacles need the geometric test.
            let entries = tree.overlapping_tree_entries_with_clearance_where(
                &current_shape,
                layer,
                &[],
                item.clearance_class(),
                matrix,
                &self.items,
                |other| other.id() != id && self.is_obstacle(other, item),
            );
            for entry in entries {
                if entry.item == id {
                    continue;
                }
                let Some(other) = self.item(entry.item) else {
                    continue;
                };
                let mut is_obstacle = self.is_obstacle(other, item);
                if is_obstacle && item.is_trace() && other.is_trace() {
                    is_obstacle = !self.traces_share_tie_pin(item, other);
                }
                if is_obstacle {
                    is_obstacle = self.outline_obstacle_check(item, other, &current_shape, &entry);
                }
                if !is_obstacle {
                    continue;
                }
                // Java logs a warning for a missing shape and skips the pair.
                let Some(other_shape) = self.tile_shape(other, entry.shape_index) else {
                    continue;
                };
                let minimum_clearance = f64::from(matrix.get_value(
                    other.clearance_class(),
                    item.clearance_class(),
                    layer,
                    false,
                ));
                let (cl_comp1, cl_comp2) = if compensation_used {
                    (
                        tree.clearance_compensation_value(matrix, item.clearance_class(), layer),
                        tree.clearance_compensation_value(matrix, other.clearance_class(), layer),
                    )
                } else {
                    let c1 = round_to_i32(0.5 * minimum_clearance);
                    (c1, round_to_i32(minimum_clearance - f64::from(c1)))
                };
                let enlarged1 = if cl_comp1 > 0 {
                    current_shape.enlarge(f64::from(cl_comp1))
                } else {
                    current_shape.clone()
                };
                let enlarged2 = if cl_comp2 > 0 {
                    other_shape.enlarge(f64::from(cl_comp2))
                } else {
                    other_shape.clone()
                };
                let intersection = enlarged1.intersection(&enlarged2);
                if intersection.dimension() != 2 {
                    continue;
                }
                let actual_clearance = clearance_between_shapes(
                    &current_shape,
                    &other_shape,
                    minimum_clearance,
                    cl_comp1,
                    cl_comp2,
                );
                let shortfall_um = (minimum_clearance - actual_clearance) * unit_to_um;
                if shortfall_um > tolerance_um {
                    result.push(ClearanceViolation {
                        first_item: id,
                        second_item: other.id(),
                        shape: intersection,
                        layer,
                        expected_clearance: minimum_clearance,
                        actual_clearance,
                    });
                }
            }
        }
        result
    }

    /// True if the traces `trace` and `other` meet at an end of `trace`
    /// at a pin that shares a net with both (a tie pin), so they may
    /// overlap without sharing a net.
    fn traces_share_tie_pin(&self, trace_item: &Item, other: &Item) -> bool {
        let Some(trace) = trace_item.as_trace() else {
            return false;
        };
        let id = trace_item.id();
        let mut contacts = self.trace_normal_contacts(id, &trace.first_corner(), true);
        let mut found = contacts.contains(&other.id());
        if !found {
            contacts = self.trace_normal_contacts(id, &trace.last_corner(), true);
            found = contacts.contains(&other.id());
        }
        found
            && contacts.iter().any(|c| {
                self.item(*c)
                    .is_some_and(|c| c.is_pin() && c.shares_net(trace_item) && c.shares_net(other))
            })
    }

    /// The board outline exceptions of `Item.clearanceViolations`: traces
    /// of nets with pins on the outline, and pins whose pad lies inside the
    /// outline, are no obstacles for the outline.
    fn outline_obstacle_check(
        &self,
        item: &Item,
        other: &Item,
        current_shape: &TileShape,
        entry: &crate::board::search::TreeEntry,
    ) -> bool {
        match (&item.kind, &other.kind) {
            (ItemKind::BoardOutline(_), ItemKind::Trace(_)) => {
                self.outline_blocks_nets(other.net_numbers())
            }
            (ItemKind::Trace(_), ItemKind::BoardOutline(_)) => {
                self.outline_blocks_nets(item.net_numbers())
            }
            (ItemKind::BoardOutline(outline), ItemKind::Pin(_))
            | (ItemKind::Pin(_), ItemKind::BoardOutline(outline)) => {
                let pin_shape = if item.is_pin() {
                    Some(current_shape.clone())
                } else {
                    self.tile_shape(other, entry.shape_index)
                };
                match pin_shape {
                    Some(shape) => !(0..shape.border_line_count())
                        .all(|ci| outline.contains(&shape.corner(ci))),
                    None => true,
                }
            }
            _ => true,
        }
    }
}

/// Java `Item.calculateClearanceBetweenTwoShapes`: a binary search for the
/// largest clearance at which the two shapes, enlarged by their parts of
/// it, do not overlap.
fn clearance_between_shapes(
    shape1: &TileShape,
    shape2: &TileShape,
    minimum_clearance: f64,
    cl_comp1: i32,
    cl_comp2: i32,
) -> f64 {
    if shape1.intersection(shape2).dimension() == 2 {
        return 0.0;
    }
    let mut low = 0.0;
    let mut high = minimum_clearance;
    let sum = f64::from(cl_comp1) + f64::from(cl_comp2);
    let (factor1, factor2) = if sum > 0.0 {
        (f64::from(cl_comp1) / sum, f64::from(cl_comp2) / sum)
    } else {
        (0.5, 0.5)
    };
    for _ in 0..16 {
        let mid = (low + high) * 0.5;
        let s1 = shape1.enlarge(mid * factor1);
        let s2 = shape2.enlarge(mid * factor2);
        if s1.intersection(&s2).dimension() == 2 {
            high = mid;
        } else {
            low = mid;
        }
    }
    low
}

#[cfg(test)]
mod sync_check {
    #[test]
    fn board_is_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<crate::board::BasicBoard>();
    }
}

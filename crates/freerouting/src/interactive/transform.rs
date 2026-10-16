//! Transform of detached items with their components (I04.1): Java's
//! `board.actions.ComponentItemTransform` at `a773a70ec`.
//!
//! [`apply_transform`] turns, rotates or mirrors detached items (copies that
//! are not on the board, as [`MoveSession`] keeps them) around a pole and
//! checks that the result keeps the topology and the design rules. It
//! returns the transformed copies, or the reason it refused; the caller
//! inserts the copies. Like Java it burns no ids (the copies keep the ids of
//! the detached items) and changes the board only when it accepts: the
//! components of the items get the new pose (through
//! [`Components`](crate::board::structure::Components), so the board's undo
//! restores them), nothing else.
//!
//! The checks, in Java's order:
//! 1. The items are detached and the angle is finite (else
//!    [`TransformError::NotDetached`] / [`TransformError::NonFiniteAngle`],
//!    Java's `IllegalArgumentException`).
//! 2. A mirror needs a mirrored padstack for every via and a signal layer on
//!    the other side for every single-layer item.
//! 3. No item is user fixed; every component of an item is not position
//!    fixed and has no item left on the board (all its items are in the
//!    set).
//! 4. The detached items go into the search trees for a moment to record
//!    their normal contacts (conduction areas left out), the contacts at both
//!    ends of each trace, their clearance violations (as `(smaller id,
//!    larger id, layer)`) and the pin centres.
//! 5. The copies are transformed (the components first, as their pins read
//!    the component's pose), each connected group of copper (traces and vias)
//!    is shifted as a whole so that it ends on the transformed pin centre it
//!    touched (two groups that need different shifts are refused; copper is
//!    never distorted), and each trace keeps its corner count and, unless it
//!    was not legal before, only legal directions for the board's angle
//!    restriction.
//! 6. The copies go into the trees for a moment: the contacts of every item,
//!    the contacts at the trace ends and the set of violations must be the
//!    same as before (no new violation).
//! 7. The components get their pose back, then, if accepted, the transform
//!    as an undoable component change.
//!
//! Java keeps the contacts in `HashMap`/`HashSet`; the result does not depend
//! on their order, so sorted maps and sets stand in for them (the group walk
//! of `alignCopper` fails or shifts the same way in any visit order).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use freerouting_geometry::{FloatPoint, IntPoint, Point, Vector, jmath};

use crate::board::basic_board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::structure::{AngleRestriction, Component, ComponentId};
use crate::board::{ItemTransform, item_ops};

/// Why [`apply_transform`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    /// Mirror: a via has no mirrored padstack.
    NoMirroredViaPadstack(ItemId),
    /// Mirror: a single-layer item would land on a layer that is not a
    /// signal layer.
    MirroredLayerNotSignal(ItemId),
    /// An item is user fixed.
    UserFixed(ItemId),
    /// The position of the item's component is fixed, or the component has
    /// an item left on the board, or the component does not exist.
    ComponentNotMovable(ItemId),
    /// Two ends of one group of copper need different shifts.
    ConflictingAnchors,
    /// A trace changed its corner count, has a zero length segment, or got
    /// a direction its angle restriction forbids.
    IllegalTrace(ItemId),
    /// The transformed items touch other items than before, or end traces
    /// elsewhere.
    ContactsChanged,
    /// The transformed items violate a clearance that the original items did
    /// not.
    NewViolations,
}

/// An error of [`apply_transform`]: Java throws for the first two and
/// returns `null` for a refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformError {
    /// An item is on the board (Java `IllegalArgumentException`).
    NotDetached(ItemId),
    /// The angle is not finite (Java `IllegalArgumentException`).
    NonFiniteAngle,
    /// The transform would break the topology or the design rules.
    Rejected(Rejection),
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransformError::NotDetached(id) => write!(f, "Transform requires detached items: {id}"),
            TransformError::NonFiniteAngle => write!(f, "Rotation must be finite"),
            TransformError::Rejected(reason) => write!(f, "transform rejected: {reason:?}"),
        }
    }
}

impl std::error::Error for TransformError {}

fn rejected<T>(reason: Rejection) -> Result<T, TransformError> {
    Err(TransformError::Rejected(reason))
}

/// How the items are moved: one of Java's three branches.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Mirror,
    Turn90(i32),
    Rotate(f64),
}

impl Kind {
    fn of(angle: f64, mirror: bool) -> Kind {
        if mirror {
            Kind::Mirror
        } else if angle % 90.0 == 0.0 {
            Kind::Turn90(jmath::d2i(angle / 90.0))
        } else {
            Kind::Rotate(angle)
        }
    }

    fn item_transform(self, pole: IntPoint) -> ItemTransform {
        match self {
            Kind::Mirror => ItemTransform::ChangeSide { pole },
            Kind::Turn90(factor) => ItemTransform::Turn90 { factor, pole },
            Kind::Rotate(angle_in_degree) => ItemTransform::Rotate {
                angle_in_degree,
                pole: FloatPoint::from_int_point(pole),
            },
        }
    }

    /// Java `transformed(point, ...)`.
    fn point(self, point: IntPoint, pole: IntPoint) -> IntPoint {
        match self {
            Kind::Mirror => point.mirror_vertical(pole),
            Kind::Turn90(factor) => point.turn_90_degree(factor, pole),
            Kind::Rotate(angle) => point
                .to_float()
                .rotate(angle.to_radians(), FloatPoint::from_int_point(pole))
                .round(),
        }
    }

    fn component(self, component: &mut Component, pole: IntPoint, rotate_first: bool) {
        match self {
            Kind::Mirror => component.change_side(pole, rotate_first),
            Kind::Turn90(factor) => component.turn_90_degree(factor, pole, rotate_first),
            Kind::Rotate(angle) => component.rotate(angle, pole, rotate_first),
        }
    }
}

/// What the detached items touch and violate (Java's `contacts`,
/// `endpointContacts`, `violations`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Topology {
    contacts: BTreeMap<ItemId, BTreeSet<ItemId>>,
    endpoint_contacts: BTreeMap<(ItemId, u8), BTreeSet<ItemId>>,
    violations: BTreeSet<(i32, i32, i32)>,
}

/// Runs `f` with `items` in the board's item map and search trees (Java
/// `searchTreeManager.insert` of items that are not in the item list), and
/// takes them out again. The undo bookkeeping, the change journal and the
/// ledger do not see them.
pub(crate) fn with_in_trees<R>(
    board: &mut BasicBoard,
    items: &[Item],
    f: impl FnOnce(&BasicBoard) -> R,
) -> R {
    for item in items {
        let mut scratch = item.clone();
        scratch.on_the_board = false;
        board.items.insert_untracked(item.id(), scratch);
        board.insert_into_trees(item.id());
    }
    let result = f(board);
    // in the order of the insertion, as Java's `remove(board, items)` does
    for item in items {
        board.remove_from_trees(item.id());
        board.items.remove_untracked(&item.id());
    }
    result
}

/// Runs `f` with `items` in the board's item map only (not in the trees),
/// for the queries that look an item up by id while its tree shapes are
/// computed from the item itself: the clearance violations of a floating
/// item (Java `Item.clearanceViolations` on an item that is not in a tree).
pub(crate) fn with_in_map<R>(
    board: &mut BasicBoard,
    items: &[Item],
    f: impl FnOnce(&BasicBoard) -> R,
) -> R {
    for item in items {
        let mut scratch = item.clone();
        scratch.on_the_board = false;
        board.items.insert_untracked(item.id(), scratch);
    }
    let result = f(board);
    for item in items.iter().rev() {
        board.items.remove_untracked(&item.id());
    }
    result
}

fn topology(board: &BasicBoard, items: &[Item]) -> Topology {
    let mut contacts = BTreeMap::new();
    let mut endpoint_contacts = BTreeMap::new();
    let mut violations = BTreeSet::new();
    let visible = |ids: BTreeSet<ItemId>| -> BTreeSet<ItemId> {
        ids.into_iter()
            .filter(|id| !board.item(*id).is_some_and(Item::is_conduction_area))
            .collect()
    };
    for item in items {
        let id = item.id();
        contacts.insert(id, visible(board.normal_contacts(id)));
        if let Some(trace) = item.as_trace() {
            let ends = [trace.first_corner(), trace.last_corner()];
            for (i, end) in ends.iter().enumerate() {
                endpoint_contacts.insert(
                    (id, i as u8),
                    visible(board.trace_normal_contacts(id, end, false)),
                );
            }
        }
        for violation in board.clearance_violations(id) {
            let (a, b) = (violation.first_item.0, violation.second_item.0);
            violations.insert((a.min(b), a.max(b), violation.layer));
        }
    }
    Topology {
        contacts,
        endpoint_contacts,
        violations,
    }
}

fn is_copper(item: &Item) -> bool {
    item.is_trace() || item.is_via()
}

fn legal_direction(dx: f64, dy: f64, restriction: AngleRestriction) -> bool {
    restriction == AngleRestriction::None
        || dx == 0.0
        || dy == 0.0
        || (restriction == AngleRestriction::FortyfiveDegree && dx == dy)
}

/// Java `legalTraces`.
fn legal_traces(
    restriction: AngleRestriction,
    old: &[Item],
    candidate: &[Item],
) -> Result<(), Rejection> {
    for (old_item, item) in old.iter().zip(candidate) {
        let (Some(trace), Some(previous)) = (item.as_trace(), old_item.as_trace()) else {
            continue;
        };
        if trace.corner_count() != previous.corner_count() {
            return Err(Rejection::IllegalTrace(item.id()));
        }
        for j in 1..trace.corner_count() {
            let a = trace.polyline.corner(j - 1).to_float();
            let b = trace.polyline.corner(j).to_float();
            let (dx, dy) = ((a.x - b.x).abs(), (a.y - b.y).abs());
            if dx == 0.0 && dy == 0.0 {
                return Err(Rejection::IllegalTrace(item.id()));
            }
            let old_a = previous.polyline.corner(j - 1).to_float();
            let old_b = previous.polyline.corner(j).to_float();
            // Imported pin escapes may already be non-grid directions. Reject
            // newly introduced violations without preventing rigid moves of
            // those existing escapes.
            if !legal_direction(dx, dy, restriction)
                && legal_direction(
                    (old_a.x - old_b.x).abs(),
                    (old_a.y - old_b.y).abs(),
                    restriction,
                )
            {
                return Err(Rejection::IllegalTrace(item.id()));
            }
        }
    }
    Ok(())
}

/// Java `alignCopper`: shifts each connected group of copper so that it ends
/// on the pin it touched.
fn align_copper(
    board: &BasicBoard,
    items: &mut [Item],
    contacts: &BTreeMap<ItemId, BTreeSet<ItemId>>,
    pin_centers: &BTreeMap<ItemId, IntPoint>,
    kind: Kind,
    pole: IntPoint,
) -> Result<(), Rejection> {
    let index: BTreeMap<ItemId, usize> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (item.id(), i))
        .collect();
    let mut visited: BTreeSet<ItemId> = BTreeSet::new();
    for seed in 0..items.len() {
        if !is_copper(&items[seed]) || !visited.insert(items[seed].id()) {
            continue;
        }
        let mut group = vec![seed];
        let mut adjustment: Option<Vector> = None;
        let mut i = 0;
        while i < group.len() {
            let group_id = items[group[i]].id();
            i += 1;
            let Some(touching) = contacts.get(&group_id) else {
                continue;
            };
            for contact_id in touching {
                let Some(&at) = index.get(contact_id) else {
                    continue;
                };
                let contact = &items[at];
                if let Some(pin) = contact.as_pin() {
                    let Some(old_center) = pin_centers.get(contact_id) else {
                        continue;
                    };
                    let required = Point::Int(pin.drill.center)
                        .difference_by(&Point::Int(kind.point(*old_center, pole)));
                    if adjustment.as_ref().is_some_and(|a| *a != required) {
                        return Err(Rejection::ConflictingAnchors);
                    }
                    adjustment = Some(required);
                } else if is_copper(contact) && visited.insert(*contact_id) {
                    group.push(at);
                }
            }
        }
        if let Some(vector) = adjustment {
            for at in group {
                board.transform_detached(&mut items[at], &ItemTransform::Translate(vector.clone()));
            }
        }
    }
    Ok(())
}

/// Java `ComponentItemTransform.apply`: turns (`angle` a multiple of 90),
/// rotates or (`mirror`) flips the detached `detached` around `pole`, with
/// their components, and returns the transformed copies in the same order.
/// See the module docs for the checks.
///
/// The copies are not on the board; the caller inserts them (Java
/// `insertItem`). Nothing changes on a refusal. On acceptance the components
/// of the items have the new pose, saved for the board's undo.
pub fn apply_transform(
    board: &mut BasicBoard,
    detached: &[Item],
    angle: f64,
    mirror: bool,
    pole: IntPoint,
) -> Result<Vec<Item>, TransformError> {
    if let Some(item) = detached
        .iter()
        .find(|i| i.is_on_the_board() || board.item(i.id()).is_some())
    {
        return Err(TransformError::NotDetached(item.id()));
    }
    if !angle.is_finite() {
        return Err(TransformError::NonFiniteAngle);
    }
    let kind = Kind::of(angle, mirror);
    if mirror {
        check_mirror(board, detached)?;
    }
    if let Some(item) = detached.iter().find(|i| i.is_user_fixed()) {
        return rejected(Rejection::UserFixed(item.id()));
    }
    let mut poses: BTreeMap<i32, Component> = BTreeMap::new();
    for item in detached {
        let component_id = item.component_id();
        if component_id > 0 {
            let movable = board
                .components
                .get(ComponentId(component_id))
                .filter(|c| !c.position_fixed && board.component_items(component_id).is_empty());
            let Some(component) = movable else {
                return rejected(Rejection::ComponentNotMovable(item.id()));
            };
            poses
                .entry(component_id)
                .or_insert_with(|| component.clone());
        }
    }
    let (before, pin_centers) = with_in_trees(board, detached, |b| {
        let centers: BTreeMap<ItemId, IntPoint> = detached
            .iter()
            .filter_map(|i| i.as_pin().map(|p| (i.id(), p.drill.center)))
            .collect();
        (topology(b, detached), centers)
    });
    let outcome = check_candidates(board, detached, &poses, &before, &pin_centers, kind, pole);
    // Java's `finally`: the components get their pose back.
    for (id, original) in &poses {
        board.components.restore_pose(ComponentId(*id), original);
    }
    let candidate = outcome?;
    for id in poses.keys() {
        let id = ComponentId(*id);
        match kind {
            Kind::Mirror => board.components.change_side(id, pole),
            Kind::Turn90(factor) => board.components.turn_90_degree(id, factor, pole),
            Kind::Rotate(angle) => board.components.rotate(id, angle, pole),
        }
    }
    Ok(candidate)
}

fn check_mirror(board: &BasicBoard, detached: &[Item]) -> Result<(), TransformError> {
    let layer_count = board.layer_count() as i32;
    for item in detached {
        if let Some(via) = item.as_via()
            && board
                .library
                .get_mirrored_via_padstack(via.drill.padstack)
                .is_none()
        {
            return rejected(Rejection::NoMirroredViaPadstack(item.id()));
        }
        if item.first_layer() == item.last_layer() {
            let other = layer_count - item.first_layer() - 1;
            let signal = usize::try_from(other)
                .ok()
                .and_then(|i| board.layer_structure.layers.get(i))
                .is_some_and(|l| l.is_signal);
            if !signal {
                return rejected(Rejection::MirroredLayerNotSignal(item.id()));
            }
        }
    }
    Ok(())
}

/// The part of `apply` between the component transform and its undo: builds
/// the candidates with the components in their new pose and checks them.
fn check_candidates(
    board: &mut BasicBoard,
    detached: &[Item],
    poses: &BTreeMap<i32, Component>,
    before: &Topology,
    pin_centers: &BTreeMap<ItemId, IntPoint>,
    kind: Kind,
    pole: IntPoint,
) -> Result<Vec<Item>, TransformError> {
    let rotate_first = board.components.flip_style_rotate_first();
    for id in poses.keys() {
        if let Some(component) = board.components.get_mut(ComponentId(*id)) {
            kind.component(component, pole, rotate_first);
        }
    }
    let mut candidate: Vec<Item> = detached
        .iter()
        .map(|item| item_ops::copy_with_id(item, item.id()))
        .collect();
    let transform = kind.item_transform(pole);
    for item in &mut candidate {
        board.transform_detached(item, &transform);
    }
    align_copper(
        board,
        &mut candidate,
        &before.contacts,
        pin_centers,
        kind,
        pole,
    )
    .map_err(TransformError::Rejected)?;
    legal_traces(board.rules.trace_angle_restriction(), detached, &candidate)
        .map_err(TransformError::Rejected)?;
    let after = with_in_trees(board, &candidate, |b| topology(b, &candidate));
    if before.contacts != after.contacts || before.endpoint_contacts != after.endpoint_contacts {
        return rejected(Rejection::ContactsChanged);
    }
    if !after.violations.is_subset(&before.violations) {
        return rejected(Rejection::NewViolations);
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_follows_javas_modulo_test() {
        assert_eq!(Kind::of(90.0, false), Kind::Turn90(1));
        assert_eq!(Kind::of(-270.0, false), Kind::Turn90(-3));
        assert_eq!(Kind::of(720.0, false), Kind::Turn90(8));
        assert_eq!(Kind::of(45.0, false), Kind::Rotate(45.0));
        assert_eq!(Kind::of(0.0, true), Kind::Mirror);
        // 0 % 90 == 0: a zero turn
        assert_eq!(Kind::of(0.0, false), Kind::Turn90(0));
    }

    #[test]
    fn legal_directions_by_restriction() {
        assert!(legal_direction(3.0, 5.0, AngleRestriction::None));
        assert!(legal_direction(0.0, 5.0, AngleRestriction::NinetyDegree));
        assert!(!legal_direction(3.0, 3.0, AngleRestriction::NinetyDegree));
        assert!(legal_direction(3.0, 3.0, AngleRestriction::FortyfiveDegree));
        assert!(!legal_direction(
            3.0,
            5.0,
            AngleRestriction::FortyfiveDegree
        ));
    }
}

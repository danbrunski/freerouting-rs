//! Free-space room completion (B03): grow an incomplete expansion room into
//! the largest free room around the shape it must contain, restrained by
//! the obstacles in an autoroute search tree.
//!
//! Port of `ShapeSearchTree.completeShape`, `ShapeSearchTree45Degree.completeShape`
//! and `ShapeSearchTree90Degree.completeShape` with their `divideLargeRoom`
//! (at `a773a70ec`). The restrain steps are in [`restrain_plain`],
//! [`super::restrain45`] and [`super::restrain90`]. One entry point,
//! [`BasicBoard::complete_shape`], dispatches on the tree's [`TreeKind`]
//! like Java's virtual call.
//!
//! The 45° and 90° variants walk the tree depth-first and shrink their
//! bound after every obstacle, so the result depends on the tree's shape
//! (decision D5): the traversal is [`ShapeTree::depth_first`], Java's
//! stack order exactly (the second child is visited first). The plain
//! variant collects every leaf against the start bound first and visits
//! them sorted (`ShapeTree::overlaps`'s order).
//!
//! Work: every node popped is one unit on the tree's work meter, as
//! `WorkMeter.tick()` in the three Java loops (B03.7); the count is added
//! once per call.
//!
//! Where Java warns and returns an empty list (no contained shape, a start
//! shape of the wrong type, an empty tree) the port returns an empty list
//! at the same point. Java's debug tracing (`isCompleteShapeDebugAnchor`,
//! `traceCompleteShape*`) is not ported.
//!
//! [`ShapeTree::depth_first`]: crate::datastructures::ShapeTree::depth_first

use freerouting_geometry::{
    IntBox, IntOctagon, LineSegment, PolylineShapeOps, RegularTileShape, Side, TileShape,
    TileShapeOps,
};

use super::rooms::IncompleteRoom;
use super::{ShapeSearchTree, TreeKind, TreeObject, restrain45, restrain90};
use crate::board::basic_board::BasicBoard;
use crate::board::items::ItemId;

/// Reusable buffers of room completion: the depth-first stack and the
/// room list being built after each obstacle. Java keeps the same stack
/// per thread (`completeShapeStack`, a `ThreadLocal`); the caller keeps one
/// per search.
#[derive(Clone, Debug, Default)]
pub struct CompletionScratch {
    stack: Vec<usize>,
    spare: Vec<IncompleteRoom>,
}

impl CompletionScratch {
    pub fn new() -> Self {
        Self::default()
    }
}

/// What a completion query needs besides the room.
struct Query<'a, F> {
    board_box: IntBox,
    room: &'a IncompleteRoom,
    ignore: Option<TreeObject>,
    ignore_shape: Option<&'a TileShape>,
    /// For an item shape: its layer if the item is a trace obstacle for
    /// the query's net, else `None`.
    item_obstacle_layer: F,
}

impl BasicBoard {
    /// Completes `room` against the autoroute tree compensated for
    /// `tree_class` (Java `ShapeSearchTree.completeShape` on that tree):
    /// the largest rooms inside `room`'s shape (the board box if `None`)
    /// that overlap only items of `net` on the room's layer and contain
    /// `room.contained`, or several rooms that each intersect it.
    ///
    /// `ignore` is never an obstacle. Rooms in the tree whose intersection
    /// with a current room lies inside `ignore_shape` are ignored (the door
    /// to the room the search came from). Returns no rooms if there is no
    /// such tree.
    pub fn complete_shape(
        &self,
        tree_class: i32,
        room: &IncompleteRoom,
        net: i32,
        ignore: Option<TreeObject>,
        ignore_shape: Option<&TileShape>,
        scratch: &mut CompletionScratch,
    ) -> Vec<IncompleteRoom> {
        let Some(tree) = self.search_trees.tree_by_class(tree_class) else {
            return Vec::new();
        };
        tree.complete_shape(
            self.bounding_box,
            room,
            ignore,
            ignore_shape,
            scratch,
            |id, index| {
                let item = self.items.get(&id)?;
                self.is_trace_obstacle(item, net)
                    .then(|| item.shape_layer(index))
            },
        )
    }

    /// Inserts the complete room `id` with `shape` on `layer` into the
    /// autoroute tree compensated for `tree_class` (Java `tree.insert(room)`
    /// of a `CompleteFreeSpaceExpansionRoom`). False if there is no such
    /// tree.
    pub fn insert_room(
        &mut self,
        tree_class: i32,
        id: super::RoomId,
        shape: TileShape,
        layer: i32,
    ) -> bool {
        match self.search_trees.tree_by_class_mut(tree_class) {
            Some(tree) => {
                tree.insert_room(id, shape, layer);
                true
            }
            None => false,
        }
    }

    /// Moves a room with key `id` left in the autoroute tree of
    /// `tree_class` by an earlier connection aside, so that a new room can
    /// take the id (see [`ShapeSearchTree::rekey_room_aside`]). The new key,
    /// or `None` if no such room is in the tree.
    pub fn rekey_room_aside(
        &mut self,
        tree_class: i32,
        id: super::RoomId,
    ) -> Option<super::RoomId> {
        self.search_trees
            .tree_by_class_mut(tree_class)?
            .rekey_room_aside(id)
    }

    /// Removes room `id` from the autoroute tree compensated for
    /// `tree_class` (Java `CompleteFreeSpaceExpansionRoom.removeFromTree`).
    /// False if there is no such tree.
    pub fn remove_room(&mut self, tree_class: i32, id: super::RoomId) -> bool {
        match self.search_trees.tree_by_class_mut(tree_class) {
            Some(tree) => {
                tree.remove_room(id);
                true
            }
            None => false,
        }
    }
}

impl ShapeSearchTree {
    /// Room completion on this tree, inside `board_box`.
    /// `item_obstacle_layer(item, shape_index)` is the layer of an item's
    /// tree shape if the item is a trace obstacle for the query's net, else
    /// `None`; rooms are obstacles for every net. See
    /// [`BasicBoard::complete_shape`].
    pub fn complete_shape(
        &self,
        board_box: IntBox,
        room: &IncompleteRoom,
        ignore: Option<TreeObject>,
        ignore_shape: Option<&TileShape>,
        scratch: &mut CompletionScratch,
        item_obstacle_layer: impl Fn(ItemId, usize) -> Option<i32>,
    ) -> Vec<IncompleteRoom> {
        let query = Query {
            board_box,
            room,
            ignore,
            ignore_shape,
            item_obstacle_layer,
        };
        match self.kind {
            TreeKind::Plain => self.complete_plain(&query),
            TreeKind::FortyfiveDegree => self.complete_45(&query, scratch),
            TreeKind::NinetyDegree => self.complete_90(&query, scratch),
        }
    }

    /// The tree shape of leaf `(object, index)` if it restrains the query's
    /// room: a trace obstacle on the room's layer that is not `ignore`
    /// (Java compares the objects by identity; ids are unique).
    fn obstacle_shape<F>(
        &self,
        query: &Query<'_, F>,
        object: TreeObject,
        index: usize,
    ) -> Option<&TileShape>
    where
        F: Fn(ItemId, usize) -> Option<i32>,
    {
        if query.ignore == Some(object) {
            return None;
        }
        match object {
            TreeObject::Room(id) => {
                let room = self.rooms.get(&id)?;
                (room.layer == query.room.layer).then_some(&room.shape)
            }
            TreeObject::Item(id) => {
                let layer = (query.item_obstacle_layer)(id, index)?;
                if layer != query.room.layer {
                    return None;
                }
                self.stored_shape(id, index)
            }
        }
    }

    /// `ShapeSearchTree.completeShape` (`:683-801`).
    fn complete_plain<F>(&self, query: &Query<'_, F>) -> Vec<IncompleteRoom>
    where
        F: Fn(ItemId, usize) -> Option<i32>,
    {
        let room = query.room;
        let Some(contained) = &room.contained else {
            return Vec::new();
        };
        if self.tree.is_empty() {
            return Vec::new();
        }
        // PARITY: the start shape is the board box intersected with the
        // room shape; a start room exists only if that is 2-dimensional.
        let mut start = TileShape::IntBox(query.board_box);
        if let Some(shape) = &room.shape {
            start = start.intersection(shape);
        }
        let Some(bounds) = start.bounding_shape(self.tree.bounding_directions()) else {
            return Vec::new();
        };
        let mut result = Vec::new();
        if start.dimension() == 2 {
            result.push(IncompleteRoom::new(
                Some(start),
                room.layer,
                Some(contained.clone()),
            ));
        }
        // PARITY: the leaves are collected against the start bound and then
        // sorted in `Leaf` order (`:732`), which is `overlaps`'s order and
        // count of visited nodes. Java's `newBoundingShape` (the union of
        // the rooms so far, `:778-784`) is assigned after each obstacle but
        // never read again, because the collection is complete.
        for leaf in self.tree.overlaps(&bounds) {
            let (object, index) = self.tree.leaf(leaf);
            let Some(obstacle) = self.obstacle_shape(query, object, index) else {
                continue;
            };
            let mut new_result = Vec::with_capacity(result.len());
            for current in result {
                let current_shape = current
                    .shape
                    .as_ref()
                    .expect("completion rooms have shapes");
                let intersection = current_shape.intersection(obstacle);
                let mut changed = false;
                if intersection.dimension() == 2 {
                    // PARITY: a room in the tree is ignored when the
                    // ignore shape contains the intersection.
                    let ignored = matches!(object, TreeObject::Room(_))
                        && query
                            .ignore_shape
                            .is_some_and(|s| s.contains_shape(&intersection));
                    if !ignored {
                        changed = true;
                        new_result.extend(restrain_plain(&current, obstacle));
                    }
                }
                if !changed {
                    new_result.push(current);
                }
            }
            result = new_result;
        }
        divide_large_room(result, &query.board_box)
    }

    /// `ShapeSearchTree45Degree.completeShape` (`:113-305`).
    fn complete_45<F>(
        &self,
        query: &Query<'_, F>,
        scratch: &mut CompletionScratch,
    ) -> Vec<IncompleteRoom>
    where
        F: Fn(ItemId, usize) -> Option<i32>,
    {
        let room = query.room;
        // PARITY: the contained shape becomes its bounding octagon; none
        // gives no rooms.
        let Some(to_contain) = room.contained.as_ref().and_then(|c| c.bounding_octagon()) else {
            return Vec::new();
        };
        if self.tree.is_empty() {
            return Vec::new();
        }
        let board_octagon = query.board_box.to_int_octagon();
        let start = match &room.shape {
            None => board_octagon,
            Some(TileShape::IntOctagon(shape)) => shape.intersection_octagon(&board_octagon),
            Some(_) => return Vec::new(),
        };
        let layer = room.layer;
        let mut bound = start;
        let mut result = vec![IncompleteRoom::new(
            Some(TileShape::IntOctagon(start)),
            layer,
            Some(TileShape::IntOctagon(to_contain)),
        )];
        let mut new_result = std::mem::take(&mut scratch.spare);
        let popped = self
            .tree
            .depth_first(&mut scratch.stack, |node_bounds, leaf| {
                // PARITY: a node is descended into only if it intersects the
                // bound of the moment, which shrinks after every obstacle.
                if !regular_intersects_octagon(node_bounds, &bound) {
                    return false;
                }
                let Some((object, index)) = leaf else {
                    return true;
                };
                let Some(obstacle) = self.obstacle_shape(query, object, index) else {
                    return false;
                };
                let Some(obstacle) = obstacle.bounding_octagon() else {
                    return false;
                };
                let is_room = matches!(object, TreeObject::Room(_));
                new_result.clear();
                let mut new_bound = IntOctagon::EMPTY;
                for current in result.drain(..) {
                    let current_shape = octagon_of(&current);
                    if !current_shape.overlaps(&obstacle) {
                        new_bound = new_bound.union_octagon(&current_shape);
                        new_result.push(current);
                        continue;
                    }
                    if is_room && let Some(ignore_shape) = query.ignore_shape {
                        let intersection = current_shape.intersection_octagon(&obstacle);
                        if ignore_shape.contains_shape(&intersection) {
                            // PARITY: `:211-231`, the room is kept unless the
                            // ignore shape contains it.
                            if !ignore_shape.contains_shape(&current_shape) {
                                new_bound = new_bound.union_octagon(&current_shape);
                                new_result.push(current);
                            }
                            continue;
                        }
                    }
                    // Java unions every room of the new list after each restrain;
                    // octagon unions are exact minima and maxima, so the union
                    // of the rooms appended is the same bound.
                    let first_new = new_result.len();
                    restrain45::restrain_45(&current, &obstacle, &mut new_result);
                    for tmp in &new_result[first_new..] {
                        new_bound = new_bound.union_octagon(&octagon_of(tmp));
                    }
                }
                std::mem::swap(&mut result, &mut new_result);
                bound = new_bound;
                false
            });
        new_result.clear();
        scratch.spare = new_result;
        self.tree.meter().add(popped);
        let mut result = divide_large_room(result, &query.board_box);
        // PARITY: `divideLargeRoom`'s override (`:311-322`): every shape and
        // contained shape becomes its bounding octagon.
        for current in &mut result {
            current.shape = Some(TileShape::IntOctagon(bounding_octagon_of(
                current.shape.as_ref(),
            )));
            current.contained = Some(TileShape::IntOctagon(bounding_octagon_of(
                current.contained.as_ref(),
            )));
        }
        // PARITY: `removeIf` (`:296-298`): rooms whose contained shape
        // contains the room shape go.
        result.retain(|r| match (&r.contained, &r.shape) {
            (Some(contained), Some(shape)) => !contained.contains_shape(shape),
            _ => true,
        });
        result
    }

    /// `ShapeSearchTree90Degree.completeShape` (`:56-216`).
    fn complete_90<F>(
        &self,
        query: &Query<'_, F>,
        scratch: &mut CompletionScratch,
    ) -> Vec<IncompleteRoom>
    where
        F: Fn(ItemId, usize) -> Option<i32>,
    {
        let room = query.room;
        let Some(TileShape::IntBox(to_contain)) = room.contained else {
            return Vec::new();
        };
        if self.tree.is_empty() {
            return Vec::new();
        }
        let start = match &room.shape {
            None => query.board_box,
            Some(TileShape::IntBox(shape)) => shape.intersection_box(&query.board_box),
            Some(_) => return Vec::new(),
        };
        let layer = room.layer;
        let mut bound = start;
        let mut result = vec![IncompleteRoom::new(
            Some(TileShape::IntBox(start)),
            layer,
            Some(TileShape::IntBox(to_contain)),
        )];
        let mut new_result = std::mem::take(&mut scratch.spare);
        let popped = self
            .tree
            .depth_first(&mut scratch.stack, |node_bounds, leaf| {
                if !regular_intersects_box(node_bounds, &bound) {
                    return false;
                }
                let Some((object, index)) = leaf else {
                    return true;
                };
                let Some(obstacle) = self.obstacle_shape(query, object, index) else {
                    return false;
                };
                let obstacle = obstacle.bounding_box();
                let is_room = matches!(object, TreeObject::Room(_));
                new_result.clear();
                let mut new_bound = IntBox::EMPTY;
                for current in result.drain(..) {
                    let current_shape = box_of(&current);
                    if !current_shape.overlaps(&obstacle) {
                        new_bound = new_bound.union_box(&current_shape);
                        new_result.push(current);
                        continue;
                    }
                    if is_room && let Some(ignore_shape) = query.ignore_shape {
                        let intersection = current_shape.intersection_box(&obstacle);
                        if ignore_shape.contains_shape(&intersection) {
                            // PARITY: unlike the 45° variant the room is dropped
                            // (`:130-145`).
                            continue;
                        }
                    }
                    // As in the 45° variant, the union of the rooms appended.
                    let first_new = new_result.len();
                    restrain90::restrain_90(&current, &obstacle, &mut new_result);
                    for tmp in &new_result[first_new..] {
                        new_bound = new_bound.union_box(&box_of(tmp));
                    }
                }
                std::mem::swap(&mut result, &mut new_result);
                bound = new_bound;
                false
            });
        new_result.clear();
        scratch.spare = new_result;
        self.tree.meter().add(popped);
        // PARITY: no `divideLargeRoom` and no `removeIf` (`:196-210`).
        result
    }
}

/// The octagon shape of a room built by the 45° variant.
fn octagon_of(room: &IncompleteRoom) -> IntOctagon {
    match &room.shape {
        Some(TileShape::IntOctagon(o)) => *o,
        other => bounding_octagon_of(other.as_ref()),
    }
}

/// The box shape of a room built by the 90° variant.
fn box_of(room: &IncompleteRoom) -> IntBox {
    match &room.shape {
        Some(TileShape::IntBox(b)) => *b,
        Some(other) => other.bounding_box(),
        None => IntBox::EMPTY,
    }
}

/// Java `boundingOctagon()`, which is never null for the bounded shapes
/// room completion builds.
fn bounding_octagon_of(shape: Option<&TileShape>) -> IntOctagon {
    shape
        .and_then(|s| s.bounding_octagon())
        .unwrap_or(IntOctagon::EMPTY)
}

/// Java `node.boundingShape.intersects(octagon)`.
fn regular_intersects_octagon(node: &RegularTileShape, bound: &IntOctagon) -> bool {
    match node {
        RegularTileShape::IntOctagon(o) => o.intersects_octagon(bound),
        RegularTileShape::IntBox(b) => bound.intersects_octagon(&b.to_int_octagon()),
    }
}

/// Java `node.boundingShape.intersects(box)`.
fn regular_intersects_box(node: &RegularTileShape, bound: &IntBox) -> bool {
    match node {
        RegularTileShape::IntBox(b) => b.intersects_box(bound),
        RegularTileShape::IntOctagon(o) => o.intersects_octagon(&bound.to_int_octagon()),
    }
}

/// `ShapeSearchTree.divideLargeRoom` (`:1223-1244`): a single room larger
/// than half the board in both directions is split into sections of width
/// `0.5 * max(height, width)` of the board, so that each layer has more
/// than one room.
pub(crate) fn divide_large_room(
    rooms: Vec<IncompleteRoom>,
    board_box: &IntBox,
) -> Vec<IncompleteRoom> {
    if rooms.len() != 1 {
        return rooms;
    }
    let room = &rooms[0];
    let Some(shape) = &room.shape else {
        return rooms;
    };
    let room_box = shape.bounding_box();
    // Java int arithmetic.
    if room_box.height().wrapping_mul(2) <= board_box.height()
        || room_box.width().wrapping_mul(2) <= board_box.width()
    {
        return rooms;
    }
    let max_section_width = 0.5 * f64::from(board_box.height().max(board_box.width()));
    shape
        .divide_into_sections(max_section_width)
        .into_iter()
        .map(|section| {
            let contained = room.contained.as_ref().map(|c| section.intersection(c));
            IncompleteRoom::new(Some(section), room.layer, contained)
        })
        .collect()
}

/// `ShapeSearchTree.restrainShape` (`:809-920`): restrains `room` to a
/// shape that does not intersect the interior of `obstacle` and still
/// contains the room's contained shape, or to several pieces that each
/// contain part of it.
fn restrain_plain(room: &IncompleteRoom, obstacle: &TileShape) -> Vec<IncompleteRoom> {
    let mut result = Vec::new();
    // Java converts both to simplexes ("otherwise border lines of length 0
    // for octagons may not be handled correctly").
    let obstacle = obstacle.to_simplex();
    let Some(contained) = room
        .contained
        .as_ref()
        .map(|c| TileShape::Simplex(c.to_simplex()))
    else {
        return result;
    };
    if contained.is_empty() {
        return result;
    }
    let Some(room_shape) = room.shape.as_ref() else {
        return result;
    };
    let layer = room.layer;
    let mut cut_line = None;
    let mut cut_line_distance = -1.0;
    // PARITY: the edge line furthest from the contained shape, first one
    // on ties.
    for i in 0..obstacle.border_line_count() {
        let segment = LineSegment::from_shape(&obstacle, i);
        if room_shape.is_intersected_interior_by_segment(&segment) {
            let line = obstacle.border_line(i);
            let distance = contained.distance_to_the_left(&line);
            if distance > cut_line_distance {
                cut_line_distance = distance;
                cut_line = Some(line.opposite());
            }
        }
    }
    if let Some(cut_line) = cut_line {
        let piece = room_shape.intersection(&TileShape::get_instance_half_plane(cut_line));
        if piece.dimension() >= 2 {
            result.push(IncompleteRoom::new(Some(piece), layer, Some(contained)));
        }
        return result;
    }
    // No edge line has all of the contained shape on its right: cut along
    // one that runs through its interior.
    if contained.dimension() < 1 {
        return result;
    }
    let mut cut_line = None;
    for i in 0..obstacle.border_line_count() {
        let segment = LineSegment::from_shape(&obstacle, i);
        if room_shape.is_intersected_interior_by_segment(&segment) {
            let line = obstacle.border_line(i);
            if contained.side_of_line(&line) == Side::Collinear {
                cut_line = Some(line.opposite());
                break;
            }
        }
    }
    let Some(cut_line) = cut_line else {
        return result;
    };
    let cut_half_plane = TileShape::get_instance_half_plane(cut_line);
    let new_contained = contained.intersection(&cut_half_plane);
    let piece = room_shape.intersection(&cut_half_plane);
    if piece.dimension() >= 2 {
        result.push(IncompleteRoom::new(Some(piece), layer, Some(new_contained)));
    }
    let opposite_half_plane = TileShape::get_instance_half_plane(cut_line.opposite());
    let rest = room_shape.intersection(&opposite_half_plane);
    if rest.dimension() >= 2 {
        let rest_contained = contained.intersection(&opposite_half_plane);
        let rest_room = IncompleteRoom::new(Some(rest), layer, Some(rest_contained));
        result.extend(restrain_plain(&rest_room, &TileShape::Simplex(obstacle)));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::search::RoomId;

    /// Five rooms on layer 0, ids 1-5 for A-E, inserted A, E, B, D, C. With
    /// `MinAreaTree`'s insertion this gives
    /// `I(I(A, I(B, C)), I(E, D))` (worked out from `positionLocate`; the
    /// walk below pins it).
    fn five_leaf_tree() -> ShapeSearchTree {
        let mut tree = ShapeSearchTree::new(TreeKind::FortyfiveDegree, 0);
        let room = |x: i32| TileShape::IntBox(IntBox::new(x, 0, x + 10, 10));
        for (id, x) in [(1, 0), (5, 400), (2, 100), (4, 300), (3, 200)] {
            tree.insert_room(RoomId(id), room(x), 0);
        }
        tree
    }

    fn leaf_ids(walk: &[String]) -> Vec<String> {
        walk.iter()
            .filter(|l| l.contains(" L "))
            .map(|l| {
                let fields: Vec<&str> = l.split_whitespace().collect();
                format!("{} {}", fields[0], fields[fields.len() - 2])
            })
            .collect()
    }

    #[test]
    fn depth_first_visits_the_second_child_first() {
        let tree = five_leaf_tree();
        // Preorder (first child first): depth and room id of each leaf.
        assert_eq!(
            leaf_ids(&tree.walk_lines()),
            ["2 1", "3 2", "3 3", "2 5", "2 4"],
            "tree shape"
        );
        let mut order = Vec::new();
        let mut stack = Vec::new();
        let popped = tree.tree.depth_first(&mut stack, |_, leaf| {
            if let Some((object, _)) = leaf {
                order.push(object.room().unwrap().0);
            }
            true
        });
        // D, E (the second subtree, second child first), then C, B, A.
        assert_eq!(order, [4, 5, 3, 2, 1]);
        assert_eq!(popped, 9);
    }

    #[test]
    fn the_shrinking_bound_prunes_the_traversal() {
        let tree = five_leaf_tree();
        let board_box = IntBox::new(-1000, -1000, 1500, 1000);
        let contained = IntBox::new(330, 2, 340, 8).to_int_octagon();
        let room = IncompleteRoom::new(None, 0, Some(TileShape::IntOctagon(contained)));
        let meter = tree.tree.meter().clone();
        let before = meter.units();
        let rooms = tree.complete_shape(
            board_box,
            &room,
            None,
            None,
            &mut CompletionScratch::new(),
            |_, _| None,
        );
        // Popped: the root, I(E, D), D (restrains to x >= 310), E (to
        // x <= 400), then I(A, I(B, C)), which no longer meets the bound, so
        // A, B and C are never visited.
        assert_eq!(meter.units() - before, 5);
        assert_eq!(rooms.len(), 1);
        let Some(TileShape::IntOctagon(shape)) = &rooms[0].shape else {
            panic!("an octagon room");
        };
        assert_eq!((shape.left_x, shape.right_x), (310, 400));
        assert_eq!((shape.bottom_y, shape.top_y), (-1000, 1000));
    }

    #[test]
    fn missing_contained_shape_or_wrong_start_type_gives_no_rooms() {
        let tree = five_leaf_tree();
        let board_box = IntBox::new(-1000, -1000, 1500, 1000);
        let complete = |room: IncompleteRoom| {
            tree.complete_shape(
                board_box,
                &room,
                None,
                None,
                &mut CompletionScratch::new(),
                |_, _| None,
            )
        };
        assert!(complete(IncompleteRoom::new(None, 0, None)).is_empty());
        let contained = TileShape::IntBox(IntBox::new(330, 2, 340, 8));
        let start = TileShape::IntBox(IntBox::new(0, 0, 500, 500));
        assert!(complete(IncompleteRoom::new(Some(start), 0, Some(contained))).is_empty());
        let empty = ShapeSearchTree::new(TreeKind::FortyfiveDegree, 0);
        let room = IncompleteRoom::new(None, 0, Some(TileShape::IntBox(IntBox::new(0, 0, 1, 1))));
        let rooms = empty.complete_shape(
            board_box,
            &room,
            None,
            None,
            &mut CompletionScratch::new(),
            |_, _| None,
        );
        assert!(rooms.is_empty());
    }
}

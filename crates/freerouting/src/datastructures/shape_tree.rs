use std::sync::Arc;

use freerouting_geometry::simd::{BoundLanes, PairLanes};
use freerouting_geometry::{RegularTileShape, ShapeBoundingDirections, TileShape, TileShapeOps};

use crate::board::search::WorkMeter;

/// The scratch heap of [`ShapeTree::best_first`]: (priority, the leaf's key
/// and shape index or `None` for an inner node, node index), smallest first.
/// `None` orders before every leaf, so inner nodes come first at equal
/// priority.
pub type BestFirstHeap<K> =
    std::collections::BinaryHeap<std::cmp::Reverse<(i64, Option<(K, usize)>, usize)>>;

/// Handle of a leaf in a [`ShapeTree`]. Valid until the leaf is removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LeafId(u32);

#[derive(Clone, Debug)]
enum NodeKind<K> {
    Inner { first: usize, second: usize },
    Leaf { object: K, shape_index: usize },
}

#[derive(Clone, Debug)]
struct Node<K> {
    bounds: RegularTileShape,
    parent: Option<usize>,
    kind: NodeKind<K>,
}

/// A binary tree of bounding shapes for fast overlap queries. Each leaf stores
/// one shape of an object: the object's key and the index of the shape in it.
///
/// Port of `datastructures.ShapeTree` with the insertion strategy of its only
/// subclass `MinAreaTree`: a new leaf goes down the child whose bounding shape
/// grows least, and replaces the leaf it reaches by an inner node holding both.
/// Nodes live in an arena; [`LeafId`]s replace the Java `Leaf` references.
///
/// [`overlaps`](Self::overlaps) returns the leaves sorted by object key, then
/// shape index, as Java sorts them with `Leaf.compareTo`, so the shape of the
/// tree does not show in query results. It adds the nodes it visits to the
/// tree's [`WorkMeter`] (Java `WorkMeter.tick()` in
/// `MinAreaTree.overlapsUnlocked`); a clone shares the meter.
///
/// Beside the node arena the tree keeps two dense arrays indexed like it, for
/// the hot loop of [`overlaps`](Self::overlaps) (P05.1): for each inner node
/// the bounds of its two children side by side as [`PairLanes`] (one 64-byte
/// line), and the two children's indexes, each marked with [`LEAF_BIT`] if
/// it is a leaf. A query tests both children of a node that it enters in one
/// branch-free loop over eight lanes, which the compiler vectorizes, and
/// reads a [`Node`] only for a leaf it returns. The node's
/// [`RegularTileShape`] stays the source of truth; every write to a node's
/// bounds or children goes through [`set_bounds`](Self::set_bounds) and
/// [`set_children`](Self::set_children), which keep the arrays in step.
#[derive(Clone, Debug)]
pub struct ShapeTree<K> {
    directions: ShapeBoundingDirections,
    root: Option<usize>,
    nodes: Vec<Option<Node<K>>>,
    /// For an inner node `nodes[i]`, the bounds of its first and second
    /// child (unused for a leaf or a free slot).
    child_lanes: Vec<PairLanes>,
    /// For an inner node `nodes[i]`, its first and second child, with
    /// [`LEAF_BIT`] set for a leaf (unused for a leaf or a free slot).
    links: Vec<[u32; 2]>,
    /// True once a bound of the other kind than [`Self::directions`] makes
    /// was inserted (only tests do); queries then take the scalar path.
    mixed_kinds: bool,
    free: Vec<usize>,
    leaf_count: usize,
    meter: Arc<WorkMeter>,
}

/// The initial capacity of the traversal stack of [`ShapeTree::overlaps`].
const QUERY_STACK_CAPACITY: usize = 64;

/// Marks a child in [`ShapeTree`]'s links as a leaf.
const LEAF_BIT: u32 = 1 << 31;

/// The links of a node that has no children.
const NO_LINKS: [u32; 2] = [u32::MAX, u32::MAX];

/// The pair lanes of a node that has no children.
const NO_LANES: PairLanes = PairLanes {
    lo: [0; 8],
    hi: [0; 8],
};

/// Which lane test answers a query (P05.1).
#[derive(Clone, Copy)]
enum LaneTest {
    Octagons,
    Boxes,
}

impl<K: Ord + Copy> ShapeTree<K> {
    pub fn new(directions: ShapeBoundingDirections) -> Self {
        ShapeTree {
            directions,
            root: None,
            nodes: Vec::new(),
            child_lanes: Vec::new(),
            links: Vec::new(),
            mixed_kinds: false,
            free: Vec::new(),
            leaf_count: 0,
            meter: Arc::new(WorkMeter::new()),
        }
    }

    /// The meter the queries count their visited nodes on.
    pub fn meter(&self) -> &Arc<WorkMeter> {
        &self.meter
    }

    /// Counts the queries on `meter` from now on (a board's trees share
    /// one).
    pub fn set_meter(&mut self, meter: Arc<WorkMeter>) {
        self.meter = meter;
    }

    /// The fixed directions of the bounding shapes in this tree.
    pub fn bounding_directions(&self) -> ShapeBoundingDirections {
        self.directions
    }

    /// The number of leaves.
    pub fn size(&self) -> usize {
        self.leaf_count
    }

    fn node(&self, idx: usize) -> &Node<K> {
        self.nodes[idx].as_ref().expect("live tree node")
    }

    fn node_mut(&mut self, idx: usize) -> &mut Node<K> {
        self.nodes[idx].as_mut().expect("live tree node")
    }

    fn alloc(&mut self, node: Node<K>) -> usize {
        let inner = match node.kind {
            NodeKind::Inner { first, second } => Some((first, second)),
            NodeKind::Leaf { .. } => None,
        };
        let idx = if let Some(idx) = self.free.pop() {
            self.nodes[idx] = Some(node);
            idx
        } else {
            self.nodes.push(Some(node));
            self.child_lanes.push(NO_LANES);
            self.links.push(NO_LINKS);
            self.nodes.len() - 1
        };
        match inner {
            Some((first, second)) => self.set_children(idx, first, second),
            None => {
                self.child_lanes[idx] = NO_LANES;
                self.links[idx] = NO_LINKS;
            }
        }
        idx
    }

    /// The link of child `idx`: its index, marked if it is a leaf.
    fn link_of(&self, idx: usize) -> u32 {
        debug_assert!(
            idx < LEAF_BIT as usize,
            "tree node index {idx} overflows a link"
        );
        match self.node(idx).kind {
            NodeKind::Leaf { .. } => idx as u32 | LEAF_BIT,
            NodeKind::Inner { .. } => idx as u32,
        }
    }

    /// Sets `idx`'s bounding shape and its lanes in its parent.
    fn set_bounds(&mut self, idx: usize, bounds: RegularTileShape) {
        let node = self.node_mut(idx);
        node.bounds = bounds;
        let Some(parent) = node.parent else {
            return;
        };
        let slot = usize::from(self.links[parent][0] & !LEAF_BIT != idx as u32);
        debug_assert_eq!(self.links[parent][slot] & !LEAF_BIT, idx as u32);
        self.child_lanes[parent].set(slot, &BoundLanes::from_regular(&bounds));
    }

    /// Sets the children of the inner node `idx`, its links and the lanes
    /// of their bounds.
    fn set_children(&mut self, idx: usize, new_first: usize, new_second: usize) {
        let NodeKind::Inner { first, second } = &mut self.node_mut(idx).kind else {
            return;
        };
        *first = new_first;
        *second = new_second;
        self.links[idx] = [self.link_of(new_first), self.link_of(new_second)];
        self.child_lanes[idx] = PairLanes::new(
            &BoundLanes::from_regular(&self.node(new_first).bounds),
            &BoundLanes::from_regular(&self.node(new_second).bounds),
        );
    }

    /// The lane test that answers a query with `shape`, if every bound in
    /// the tree and `shape` have the kind this tree's directions make.
    fn lane_test(&self, shape: &RegularTileShape) -> Option<LaneTest> {
        if self.mixed_kinds {
            return None;
        }
        match (self.directions, shape) {
            (ShapeBoundingDirections::FortyfiveDegree, RegularTileShape::IntOctagon(_)) => {
                Some(LaneTest::Octagons)
            }
            (ShapeBoundingDirections::Orthogonal, RegularTileShape::IntBox(_)) => {
                Some(LaneTest::Boxes)
            }
            _ => None,
        }
    }

    fn release(&mut self, idx: usize) {
        self.nodes[idx] = None;
        self.free.push(idx);
    }

    /// The object key and shape index stored in `leaf`.
    pub fn leaf(&self, leaf: LeafId) -> (K, usize) {
        match &self.node(leaf.0 as usize).kind {
            NodeKind::Leaf {
                object,
                shape_index,
            } => (*object, *shape_index),
            NodeKind::Inner { .. } => panic!("not a leaf"),
        }
    }

    /// The bounding shape stored in `leaf`.
    pub fn leaf_bounds(&self, leaf: LeafId) -> RegularTileShape {
        self.node(leaf.0 as usize).bounds
    }

    /// Changes the object key and shape index stored in `leaf` (Java assigns
    /// `Leaf.object` and `Leaf.shapeIndexInObject` directly).
    pub fn set_leaf(&mut self, leaf: LeafId, new_object: K, new_index: usize) {
        if let NodeKind::Leaf {
            object,
            shape_index,
        } = &mut self.node_mut(leaf.0 as usize).kind
        {
            *object = new_object;
            *shape_index = new_index;
        }
    }

    /// Inserts a leaf for shape `shape_index` of `object` with bounding shape
    /// `bounds`, which the caller computes with this tree's directions.
    pub fn insert(&mut self, object: K, shape_index: usize, bounds: RegularTileShape) -> LeafId {
        if self.lane_test(&bounds).is_none() {
            self.mixed_kinds = true;
        }
        let leaf = self.alloc(Node {
            bounds,
            parent: None,
            kind: NodeKind::Leaf {
                object,
                shape_index,
            },
        });
        self.leaf_count += 1;
        let Some(root) = self.root else {
            self.root = Some(leaf);
            return LeafId(leaf as u32);
        };
        let to_replace = self.position_locate(root, &bounds);
        let replace_bounds = self.node(to_replace).bounds;
        let new_bounds = bounds.union(&replace_bounds);
        let parent = self.node(to_replace).parent;
        let inner = self.alloc(Node {
            bounds: new_bounds,
            parent,
            kind: NodeKind::Inner {
                first: to_replace,
                second: leaf,
            },
        });
        if let Some(parent) = parent {
            if let NodeKind::Inner { first, second } = self.node(parent).kind {
                if first == to_replace {
                    self.set_children(parent, inner, second);
                } else {
                    self.set_children(parent, first, inner);
                }
            }
        }
        self.node_mut(to_replace).parent = Some(inner);
        self.node_mut(leaf).parent = Some(inner);
        if self.root == Some(to_replace) {
            self.root = Some(inner);
        }
        LeafId(leaf as u32)
    }

    fn position_locate(&mut self, start: usize, bounds: &RegularTileShape) -> usize {
        let mut current = start;
        loop {
            let (first, second) = match self.node(current).kind {
                NodeKind::Leaf { .. } => return current,
                NodeKind::Inner { first, second } => (first, second),
            };
            let union = bounds.union(&self.node(current).bounds);
            self.set_bounds(current, union);
            let first_shape = self.node(first).bounds;
            let first_increase = area(&bounds.union(&first_shape)) - area(&first_shape);
            let second_shape = self.node(second).bounds;
            let second_increase = area(&bounds.union(&second_shape)) - area(&second_shape);
            current = if first_increase <= second_increase {
                first
            } else {
                second
            };
        }
    }

    /// Removes `leaf` from the tree.
    pub fn remove_leaf(&mut self, leaf: LeafId) {
        let leaf = leaf.0 as usize;
        if self.nodes.get(leaf).is_none_or(|n| n.is_none()) {
            return;
        }
        let parent = self.node(leaf).parent;
        if parent.is_none() && self.root != Some(leaf) {
            return;
        }
        self.leaf_count -= 1;
        self.release(leaf);
        let Some(parent) = parent else {
            self.root = None;
            return;
        };
        let other = match self.node(parent).kind {
            NodeKind::Inner { first, second } => {
                if second == leaf {
                    first
                } else {
                    second
                }
            }
            NodeKind::Leaf { .. } => unreachable!("parent is an inner node"),
        };
        let grand_parent = self.node(parent).parent;
        self.node_mut(other).parent = grand_parent;
        match grand_parent {
            None => self.root = Some(other),
            Some(gp) => {
                if let NodeKind::Inner { first, second } = self.node(gp).kind {
                    if second == parent {
                        self.set_children(gp, first, other);
                    } else if first == parent {
                        self.set_children(gp, other, second);
                    }
                }
            }
        }
        self.release(parent);
        // Recalculate the bounding shapes of the ancestors as long as they
        // get smaller.
        let mut to_recalculate = grand_parent;
        while let Some(idx) = to_recalculate {
            let (first, second) = match self.node(idx).kind {
                NodeKind::Inner { first, second } => (first, second),
                NodeKind::Leaf { .. } => unreachable!(),
            };
            let new_bounds = self.node(second).bounds.union(&self.node(first).bounds);
            if new_bounds.contains_regular(&self.node(idx).bounds) {
                break;
            }
            self.set_bounds(idx, new_bounds);
            to_recalculate = self.node(idx).parent;
        }
    }

    /// The leaves whose bounding shape intersects `shape`, sorted by object
    /// key and then by shape index.
    ///
    /// Every node popped off the traversal stack, leaf or inner, hit or
    /// miss, is one unit of work (Java's); in [`WorkUnit::Leaves`] every
    /// leaf returned is (P04.2). The count is added to the meter once.
    ///
    /// [`WorkUnit::Leaves`]: crate::board::search::work::WorkUnit::Leaves
    pub fn overlaps(&self, shape: &RegularTileShape) -> Vec<LeafId> {
        let mut found: Vec<(K, usize, LeafId)> = Vec::new();
        let Some(root) = self.root else {
            return Vec::new();
        };
        let query = BoundLanes::from_regular(shape);
        let visited = match self.lane_test(shape) {
            Some(LaneTest::Octagons) => self.overlaps_lanes(
                root,
                &query,
                BoundLanes::octagons_intersect,
                PairLanes::octagons_intersect,
                &mut found,
            ),
            Some(LaneTest::Boxes) => self.overlaps_lanes(
                root,
                &query,
                BoundLanes::boxes_intersect,
                PairLanes::boxes_intersect,
                &mut found,
            ),
            None => self.overlaps_scalar(root, shape, &mut found),
        };
        self.meter.add_query(visited, found.len() as u64);
        // A key and shape index identify a leaf, so the order is total.
        found.sort_unstable_by_key(|&(key, index, _)| (key, index));
        found.into_iter().map(|(_, _, leaf)| leaf).collect()
    }

    /// The leaves [`overlaps`](Self::overlaps) finds, sorted the same way,
    /// and the nodes it pops, by the scalar traversal on the node shapes
    /// (the path before P05.1, kept as the reference of the lane
    /// traversal). Does not touch the meter.
    pub fn overlaps_reference(&self, shape: &RegularTileShape) -> (Vec<LeafId>, u64) {
        let Some(root) = self.root else {
            return (Vec::new(), 0);
        };
        let mut found = Vec::new();
        let visited = self.overlaps_scalar(root, shape, &mut found);
        found.sort_unstable_by_key(|&(key, index, _)| (key, index));
        (
            found.into_iter().map(|(_, _, leaf)| leaf).collect(),
            visited,
        )
    }

    /// The traversal of [`overlaps`](Self::overlaps) on the lane arrays:
    /// the nodes [`overlaps_scalar`](Self::overlaps_scalar) pops, tested by
    /// pairs of siblings, and the leaves it finds, since `hit_root` and
    /// `hit_pair` answer as [`bounds_intersect`] does. The order differs,
    /// which the sort in `overlaps` hides; a node is popped if all its
    /// ancestors hit, in any order. Returns the nodes popped.
    #[inline(always)]
    fn overlaps_lanes(
        &self,
        root: usize,
        query: &BoundLanes,
        hit_root: impl Fn(&BoundLanes, &BoundLanes) -> bool,
        hit_pair: impl Fn(&PairLanes, &PairLanes) -> u32,
        found: &mut Vec<(K, usize, LeafId)>,
    ) -> u64 {
        if !hit_root(&BoundLanes::from_regular(&self.node(root).bounds), query) {
            return 1;
        }
        let mut push_leaf = |idx: usize| {
            if let NodeKind::Leaf {
                object,
                shape_index,
            } = self.node(idx).kind
            {
                found.push((object, shape_index, LeafId(idx as u32)));
            }
        };
        if matches!(self.node(root).kind, NodeKind::Leaf { .. }) {
            push_leaf(root);
            return 1;
        }
        let pair_query = PairLanes::splat(query);
        // Deep enough for the trees of real boards, so the stack does not
        // grow in the loop (a depth-first stack holds at most depth + 1).
        let mut stack = Vec::with_capacity(QUERY_STACK_CAPACITY);
        stack.push(root as u32);
        let mut visited = 1u64;
        while let Some(current) = stack.pop() {
            let current = current as usize;
            visited += 2;
            let mask = hit_pair(&self.child_lanes[current], &pair_query);
            if mask == 0 {
                continue;
            }
            let links = self.links[current];
            for (slot, link) in links.into_iter().enumerate() {
                if mask & (1 << slot) == 0 {
                    continue;
                }
                if link & LEAF_BIT != 0 {
                    push_leaf((link & !LEAF_BIT) as usize);
                } else {
                    stack.push(link);
                }
            }
        }
        visited
    }

    /// The traversal of [`overlaps`](Self::overlaps) on the node shapes,
    /// for a tree or query with bounds of mixed kinds. Returns the nodes
    /// popped.
    fn overlaps_scalar(
        &self,
        root: usize,
        shape: &RegularTileShape,
        found: &mut Vec<(K, usize, LeafId)>,
    ) -> u64 {
        let query = shape.to_tile_shape();
        let mut stack = vec![root];
        let mut visited = 0u64;
        while let Some(current) = stack.pop() {
            visited += 1;
            let node = self.node(current);
            if bounds_intersect(&node.bounds, &query) {
                match node.kind {
                    NodeKind::Leaf {
                        object,
                        shape_index,
                    } => found.push((object, shape_index, LeafId(current as u32))),
                    NodeKind::Inner { first, second } => {
                        stack.push(first);
                        stack.push(second);
                    }
                }
            }
        }
        visited
    }

    /// All leaves, from the leftmost to the rightmost (Java `toArray`).
    pub fn to_array(&self) -> Vec<LeafId> {
        let mut result = Vec::with_capacity(self.leaf_count);
        let Some(root) = self.root else {
            return result;
        };
        let mut current = root;
        loop {
            while let NodeKind::Inner { first, .. } = self.node(current).kind {
                current = first;
            }
            result.push(LeafId(current as u32));
            let mut parent = self.node(current).parent;
            while let Some(p) = parent {
                match self.node(p).kind {
                    NodeKind::Inner { second, .. } if second == current => {
                        current = p;
                        parent = self.node(p).parent;
                    }
                    _ => break,
                }
            }
            let Some(p) = parent else {
                break;
            };
            current = match self.node(p).kind {
                NodeKind::Inner { second, .. } => second,
                NodeKind::Leaf { .. } => unreachable!(),
            };
        }
        result
    }

    /// Visits every node in preorder: a node, then its first child's
    /// subtree, then its second child's. `visit` gets the depth (the root
    /// has depth 0), the bounding shape, and for a leaf its key and shape
    /// index.
    pub fn preorder(&self, mut visit: impl FnMut(usize, &RegularTileShape, Option<(K, usize)>)) {
        let Some(root) = self.root else {
            return;
        };
        let mut stack = vec![(root, 0usize)];
        while let Some((current, depth)) = stack.pop() {
            let node = self.node(current);
            match node.kind {
                NodeKind::Leaf {
                    object,
                    shape_index,
                } => visit(depth, &node.bounds, Some((object, shape_index))),
                NodeKind::Inner { first, second } => {
                    visit(depth, &node.bounds, None);
                    stack.push((second, depth + 1));
                    stack.push((first, depth + 1));
                }
            }
        }
    }

    /// The number of nodes from `leaf` up to the root.
    pub fn distance_to_root(&self, leaf: LeafId) -> usize {
        let mut result = 0;
        let mut current = self.node(leaf.0 as usize).parent;
        while let Some(p) = current {
            result += 1;
            current = self.node(p).parent;
        }
        result
    }

    /// Walks the tree as Java's room completion does
    /// (`ShapeSearchTree45Degree.completeShape`, B03): pop a node, call
    /// `visit` with its bounding shape and, for a leaf, its key and shape
    /// index; if `visit` returns true for an inner node, push its first
    /// child, then its second, so the second is visited first. `visit`
    /// decides with the bound of the moment, which it may shrink at a
    /// leaf. `stack` is reusable scratch (its contents are dropped first).
    ///
    /// Returns the number of nodes popped, Java's `WorkMeter.tick()` count;
    /// the caller adds it to the meter. Unlike [`overlaps`](Self::overlaps)
    /// this does not touch the meter.
    pub fn depth_first(
        &self,
        stack: &mut Vec<usize>,
        mut visit: impl FnMut(&RegularTileShape, Option<(K, usize)>) -> bool,
    ) -> u64 {
        stack.clear();
        let Some(root) = self.root else {
            return 0;
        };
        stack.push(root);
        let mut popped = 0u64;
        while let Some(current) = stack.pop() {
            popped += 1;
            let node = self.node(current);
            match node.kind {
                NodeKind::Leaf {
                    object,
                    shape_index,
                } => {
                    visit(&node.bounds, Some((object, shape_index)));
                }
                NodeKind::Inner { first, second } => {
                    if visit(&node.bounds, None) {
                        stack.push(first);
                        stack.push(second);
                    }
                }
            }
        }
        popped
    }

    /// Walks the tree best first, for room completion in the canonical
    /// order (P04.1): nodes come off a min-heap ordered by `priority` of
    /// their bounding shape, inner nodes before leaves of the same
    /// priority, leaves by priority, then key, then shape index. `visit`
    /// gets the bounding shape and, for a leaf, its key and shape index; if
    /// it returns true for an inner node, both children are pushed.
    ///
    /// If `priority` never decreases from a node to its children (a
    /// distance to a fixed shape does, since a child's bounds lie inside
    /// its parent's), the leaves come in the order (priority, key, shape
    /// index) whatever the tree's shape, and a subtree pruned because it
    /// misses a bound that only shrinks holds no leaf that would meet the
    /// bound at its turn. `heap` is reusable scratch. Returns the nodes
    /// popped; the caller adds the count to the meter.
    pub fn best_first(
        &self,
        heap: &mut BestFirstHeap<K>,
        priority: impl Fn(&RegularTileShape) -> i64,
        mut visit: impl FnMut(&RegularTileShape, Option<(K, usize)>) -> bool,
    ) -> u64 {
        heap.clear();
        let Some(root) = self.root else {
            return 0;
        };
        let entry = |idx: usize| {
            let node = self.node(idx);
            let leaf = match node.kind {
                NodeKind::Leaf {
                    object,
                    shape_index,
                } => Some((object, shape_index)),
                NodeKind::Inner { .. } => None,
            };
            std::cmp::Reverse((priority(&node.bounds), leaf, idx))
        };
        heap.push(entry(root));
        let mut popped = 0u64;
        while let Some(std::cmp::Reverse((_, leaf, current))) = heap.pop() {
            popped += 1;
            let node = self.node(current);
            match node.kind {
                NodeKind::Leaf { .. } => {
                    visit(&node.bounds, leaf);
                }
                NodeKind::Inner { first, second } => {
                    if visit(&node.bounds, None) {
                        heap.push(entry(first));
                        heap.push(entry(second));
                    }
                }
            }
        }
        popped
    }

    /// True if the tree has no leaves (Java `root == null`).
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }
}

/// `bounds.to_tile_shape().intersects_tile(query)`, with the two pairs of
/// one kind (the 45 degree and the 90 degree trees with their own queries)
/// tested directly, the same calls `intersects_tile` makes (P02.2).
#[inline]
fn bounds_intersect(bounds: &RegularTileShape, query: &TileShape) -> bool {
    match (bounds, query) {
        (RegularTileShape::IntOctagon(node), TileShape::IntOctagon(q)) => {
            q.intersects_octagon(node)
        }
        (RegularTileShape::IntBox(node), TileShape::IntBox(q)) => q.intersects_box(node),
        _ => bounds.to_tile_shape().intersects_tile(query),
    }
}

fn area(shape: &RegularTileShape) -> f64 {
    match shape {
        RegularTileShape::IntBox(b) => b.area(),
        RegularTileShape::IntOctagon(o) => o.area(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting_geometry::IntBox;

    fn bx(x1: i32, y1: i32, x2: i32, y2: i32) -> RegularTileShape {
        RegularTileShape::IntBox(IntBox::new(x1, y1, x2, y2))
    }

    #[test]
    fn overlaps_finds_leaves_sorted() {
        let mut tree = ShapeTree::new(ShapeBoundingDirections::Orthogonal);
        tree.insert(3, 0, bx(0, 0, 10, 10));
        tree.insert(1, 1, bx(5, 5, 20, 20));
        tree.insert(1, 0, bx(100, 100, 110, 110));
        tree.insert(2, 0, bx(8, 8, 9, 9));
        let found: Vec<_> = tree
            .overlaps(&bx(6, 6, 9, 9))
            .into_iter()
            .map(|l| tree.leaf(l))
            .collect();
        assert_eq!(found, vec![(1, 1), (2, 0), (3, 0)]);
        assert_eq!(tree.size(), 4);
        assert_eq!(tree.to_array().len(), 4);
    }

    #[test]
    fn removed_leaves_are_not_found() {
        let mut tree = ShapeTree::new(ShapeBoundingDirections::Orthogonal);
        let a = tree.insert(1, 0, bx(0, 0, 10, 10));
        let b = tree.insert(2, 0, bx(20, 0, 30, 10));
        let c = tree.insert(3, 0, bx(40, 0, 50, 10));
        tree.remove_leaf(b);
        assert_eq!(tree.size(), 2);
        assert!(tree.overlaps(&bx(21, 1, 29, 9)).is_empty());
        tree.remove_leaf(a);
        tree.remove_leaf(c);
        assert_eq!(tree.size(), 0);
        assert!(tree.overlaps(&bx(0, 0, 100, 100)).is_empty());
    }

    /// Port of `MinAreaTreeLeafLifecycleTest.removingSameLeafTwiceDoesNotCorruptTree`.
    /// The other tests of that class and `ShapeSearchTreeLeafLifecycleTest`
    /// check Java's read/write locking, which the borrow checker makes
    /// unnecessary.
    #[test]
    fn removing_same_leaf_twice_does_not_corrupt_tree() {
        let mut tree = ShapeTree::new(ShapeBoundingDirections::FortyfiveDegree);
        let first = tree.insert(1, 0, bx(-10, -10, 10, 10));
        tree.remove_leaf(first);
        assert_eq!(tree.size(), 0);
        tree.remove_leaf(first);
        assert_eq!(tree.size(), 0);
        let second = tree.insert(2, 0, bx(-20, -20, 20, 20));
        assert_eq!(tree.size(), 1);
        assert!(tree.overlaps(&bx(-100, -100, 100, 100)).contains(&second));
    }

    /// Random boxes and octagons on a small grid (touching and empty bounds
    /// are frequent) and near the coordinate limit.
    fn random_bounds(
        random: &mut freerouting_geometry::jmath::JavaRandom,
        octagon: bool,
    ) -> RegularTileShape {
        use freerouting_geometry::{CRIT_INT, IntOctagon};
        let range = if random.next_int(4) == 0 {
            CRIT_INT
        } else {
            40
        };
        let x = random.next_int(2 * range + 1) - range;
        let y = random.next_int(2 * range + 1) - range;
        // Sometimes empty (the upper corner below or left of the lower one).
        let w = random.next_int(range / 2) - range / 16;
        let h = random.next_int(range / 2) - range / 16;
        let b = IntBox::new(x, y, x.saturating_add(w), y.saturating_add(h));
        if random.next_int(23) == 0 {
            return if octagon {
                RegularTileShape::IntOctagon(IntOctagon::EMPTY)
            } else {
                RegularTileShape::IntBox(IntBox::EMPTY)
            };
        }
        if !octagon {
            return RegularTileShape::IntBox(b);
        }
        let mut cut = || random.next_int(range / 4 + 1);
        let (lx, by, rx, ty) = (b.ll.x, b.ll.y, b.ur.x, b.ur.y);
        let o = IntOctagon::new(
            lx,
            by,
            rx,
            ty,
            lx.saturating_sub(ty).saturating_add(cut()),
            rx.saturating_sub(by).saturating_sub(cut()),
            lx.saturating_add(by).saturating_add(cut()),
            rx.saturating_add(ty).saturating_sub(cut()),
        );
        RegularTileShape::IntOctagon(if x % 2 == 0 { o.normalize() } else { o })
    }

    /// The lane arrays hold each live inner node's children and their
    /// bounds (P05.1).
    fn assert_lanes_in_step<K: Ord + Copy>(tree: &ShapeTree<K>) {
        for (idx, node) in tree.nodes.iter().enumerate() {
            let Some(node) = node else { continue };
            let NodeKind::Inner { first, second } = node.kind else {
                continue;
            };
            for (slot, child) in [first, second].into_iter().enumerate() {
                let bounds = tree.node(child).bounds;
                assert_eq!(
                    tree.child_lanes[idx].get(slot),
                    BoundLanes::from_regular(&bounds)
                );
                assert_eq!(tree.links[idx][slot], tree.link_of(child));
                assert_eq!(tree.node(child).parent, Some(idx));
            }
        }
    }

    /// P05.1: the lane traversal finds the leaves the scalar traversal
    /// finds and pops as many nodes, on trees that grow and shrink.
    #[test]
    fn lane_queries_equal_scalar_queries() {
        let mut random = freerouting_geometry::jmath::JavaRandom::new(5);
        for (octagon, directions) in [
            (true, ShapeBoundingDirections::FortyfiveDegree),
            (false, ShapeBoundingDirections::Orthogonal),
        ] {
            let mut tree: ShapeTree<u32> = ShapeTree::new(directions);
            let mut leaves = Vec::new();
            let mut queries = 0u32;
            for step in 0..3000u32 {
                if !leaves.is_empty() && random.next_int(3) == 0 {
                    let at = random.next_int(leaves.len() as i32) as usize;
                    tree.remove_leaf(leaves.swap_remove(at));
                } else {
                    leaves.push(tree.insert(step, 0, random_bounds(&mut random, octagon)));
                }
                if step % 50 == 0 {
                    assert_lanes_in_step(&tree);
                }
                let query = random_bounds(&mut random, octagon);
                assert!(tree.lane_test(&query).is_some());
                let Some(root) = tree.root else { continue };
                let mut fast = Vec::new();
                let mut slow = Vec::new();
                let popped = match tree.lane_test(&query) {
                    Some(LaneTest::Octagons) => tree.overlaps_lanes(
                        root,
                        &BoundLanes::from_regular(&query),
                        BoundLanes::octagons_intersect,
                        PairLanes::octagons_intersect,
                        &mut fast,
                    ),
                    Some(LaneTest::Boxes) => tree.overlaps_lanes(
                        root,
                        &BoundLanes::from_regular(&query),
                        BoundLanes::boxes_intersect,
                        PairLanes::boxes_intersect,
                        &mut fast,
                    ),
                    None => unreachable!(),
                };
                assert_eq!(popped, tree.overlaps_scalar(root, &query, &mut slow));
                fast.sort_unstable_by_key(|&(key, index, _)| (key, index));
                slow.sort_unstable_by_key(|&(key, index, _)| (key, index));
                assert_eq!(fast, slow, "query {query:?}");
                queries += u32::from(!fast.is_empty());
            }
            assert!(queries > 300, "too few queries hit: {queries}");
            assert!(!tree.mixed_kinds);
        }
    }

    /// A bound of the other kind sends queries down the scalar path.
    #[test]
    fn mixed_kinds_take_the_scalar_path() {
        use freerouting_geometry::IntOctagon;
        let mut tree = ShapeTree::new(ShapeBoundingDirections::Orthogonal);
        tree.insert(1, 0, bx(0, 0, 10, 10));
        assert!(tree.lane_test(&bx(0, 0, 1, 1)).is_some());
        let oct = RegularTileShape::IntOctagon(IntOctagon::new(20, 0, 30, 10, -10, 30, 20, 40));
        assert!(tree.lane_test(&oct).is_none());
        tree.insert(2, 0, oct);
        assert!(tree.lane_test(&bx(0, 0, 1, 1)).is_none());
        let found: Vec<_> = tree
            .overlaps(&bx(5, 5, 25, 5))
            .into_iter()
            .map(|l| tree.leaf(l).0)
            .collect();
        assert_eq!(found, vec![1, 2]);
    }
}

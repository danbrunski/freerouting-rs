use freerouting_geometry::{RegularTileShape, ShapeBoundingDirections, TileShapeOps};

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
/// tree does not show in query results.
#[derive(Clone, Debug)]
pub struct ShapeTree<K> {
    directions: ShapeBoundingDirections,
    root: Option<usize>,
    nodes: Vec<Option<Node<K>>>,
    free: Vec<usize>,
    leaf_count: usize,
}

impl<K: Ord + Copy> ShapeTree<K> {
    pub fn new(directions: ShapeBoundingDirections) -> Self {
        ShapeTree {
            directions,
            root: None,
            nodes: Vec::new(),
            free: Vec::new(),
            leaf_count: 0,
        }
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
        if let Some(idx) = self.free.pop() {
            self.nodes[idx] = Some(node);
            idx
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
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
            if let NodeKind::Inner { first, second } = &mut self.node_mut(parent).kind {
                if *first == to_replace {
                    *first = inner;
                } else {
                    *second = inner;
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
            self.node_mut(current).bounds = union;
            let first_shape = self.node(first).bounds;
            let first_increase =
                area(&bounds.union(&first_shape)) - area(&first_shape);
            let second_shape = self.node(second).bounds;
            let second_increase =
                area(&bounds.union(&second_shape)) - area(&second_shape);
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
                if let NodeKind::Inner { first, second } = &mut self.node_mut(gp).kind {
                    if *second == parent {
                        *second = other;
                    } else if *first == parent {
                        *first = other;
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
            self.node_mut(idx).bounds = new_bounds;
            to_recalculate = self.node(idx).parent;
        }
    }

    /// The leaves whose bounding shape intersects `shape`, sorted by object
    /// key and then by shape index.
    pub fn overlaps(&self, shape: &RegularTileShape) -> Vec<LeafId> {
        let mut found = Vec::new();
        let Some(root) = self.root else {
            return found;
        };
        let query = shape.to_tile_shape();
        let mut stack = vec![root];
        while let Some(current) = stack.pop() {
            let node = self.node(current);
            if node.bounds.to_tile_shape().intersects_tile(&query) {
                match node.kind {
                    NodeKind::Leaf { .. } => found.push(LeafId(current as u32)),
                    NodeKind::Inner { first, second } => {
                        stack.push(first);
                        stack.push(second);
                    }
                }
            }
        }
        found.sort_by(|a, b| self.leaf(*a).cmp(&self.leaf(*b)));
        found
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
}

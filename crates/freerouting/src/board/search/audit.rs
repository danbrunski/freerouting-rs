//! P02.1: the incremental search trees against a rebuild.
//!
//! The search trees are updated on every insert and remove (B02) and for
//! the autorouter's rooms (A02). A tree whose incremental state drifts from
//! the board routes wrong, not slowly, so every caching optimization (P02)
//! is tested with this audit on. Behind the setting
//! `router.debug.audit_incremental` (off by default) the batch loop runs it
//! after each ripup and restore and at the end of each pass.
//!
//! An audit rebuilds each tree of the board (the default tree and every
//! autoroute tree) from scratch: a new tree of the same kind and clearance
//! class, the items on the board inserted in item-list order (Java's
//! `insertAllBoardItems`), then the live tree's rooms in id order. It then
//! compares *contents*, never shape: an incrementally updated tree
//! legitimately has another shape than a fresh build (D5: the insert and
//! remove history decides it), so B02's shape fingerprints cannot be
//! compared with a rebuild. Compared, per tree:
//! 1. the entries against a fresh calculation: the same items, each with
//!    the same number of tree shapes, each shape equal to the calculated
//!    one (exactly: boxes and octagons by value, simplexes line by line)
//!    or one of the deviations below, and a leaf exactly for the shapes
//!    that have a bounding shape;
//! 2. the leaves: [`ShapeSearchTree::content_digest`] of the live tree
//!    against a tree built afresh from the shapes step 1 accepted, and on a
//!    difference the sorted leaf lists, to name the first one;
//! 3. a fixed query sample: every board item's bounding box as an
//!    all-layer overlap query on both trees. Each found entry carries its
//!    layer, so one all-layer query compares the results on every layer at
//!    once. This is what catches a tree whose leaves are right but whose
//!    inner bounds no longer cover them.
//!
//! **Java's own deviations.** A Java tree updated incrementally does not
//! hold the shapes a rebuild calculates, and the port keeps them (D5), so
//! step 1 tolerates three kinds of stored trace shape, counted in
//! [`Tolerated`] (found on `connection-parity` and `benchmark`, P02.1):
//! - *reversed*: a leaf kept by a merge with a reversed trace keeps the
//!   shape calculated in the other direction (`entries.rs`); it equals the
//!   mirrored shape of the reversed trace;
//! - *rounded*: a leaf kept by `changeEntries` for a line that is equal
//!   (Java's geometric `Line.equals`) but defined by other points; its
//!   offset rounds differently, by at most one unit per octagon bound;
//! - *reduced*: the first or last shape of a trace that
//!   `reduceTraceShapeAtTiePin` cut down in an autoroute tree; it lies
//!   inside the calculated shape.
//!
//! A shape that moved further, an item missing from a tree or left in it,
//! a leaf without an entry and a query that misses a leaf are differences.
//!
//! The first difference is reported with its [`ItemId`] (or room), shape
//! index, layer and tree. The audit only reads: its queries walk the tree
//! with [`ShapeTree::depth_first`](crate::datastructures::ShapeTree::depth_first),
//! which leaves the board's work meter alone (D5), and the rebuilds count
//! on meters of their own. It never burns an item id.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt;

use freerouting_geometry::{RegularTileShape, ShapeBoundingDirections, TileShape};

use super::{ShapeContext, ShapeSearchTree, TreeObject};
use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::datastructures::UndoableObjects;

/// Where in the batch loop an audit ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditPoint {
    /// After a connection of pass `pass` ripped items or failed (and the
    /// ripup policy handled the failure).
    Ripup { pass: u32 },
    /// After the tail removal at the end of pass `pass`.
    PassEnd { pass: u32 },
    /// After a history restore swapped the board in.
    Restore,
    /// After the last-mile ripup or the fanout recovery.
    LoopRipup,
    /// After the final best-board restore and tail removal.
    Finish,
    /// A caller's own check (tests, tools).
    Manual,
}

impl fmt::Display for AuditPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuditPoint::Ripup { pass } => write!(f, "after a ripup in pass {pass}"),
            AuditPoint::PassEnd { pass } => write!(f, "at the end of pass {pass}"),
            AuditPoint::Restore => f.write_str("after a restore"),
            AuditPoint::LoopRipup => f.write_str("after a loop ripup"),
            AuditPoint::Finish => f.write_str("at the finish"),
            AuditPoint::Manual => f.write_str("on request"),
        }
    }
}

/// What differs between a live tree and its rebuild.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DifferenceKind {
    /// The live tree holds entries of an item that is not on the board.
    StaleItem,
    /// An item on the board has no entries in the live tree.
    MissingItem,
    /// The live tree stores another number of tree shapes for the item.
    ShapeCount { live: usize, rebuilt: usize },
    /// A stored tree shape differs from the calculated one (both as
    /// `Debug` text, `None` for no shape).
    ShapeDiffers { live: String, rebuilt: String },
    /// A shape has a leaf in one tree and none in the other.
    LeafPresence { live: bool },
    /// A leaf is in one tree only, or its bounds differ.
    Leaf { in_live: bool, in_rebuilt: bool },
    /// The overlap query with the bounding box of `query` found this entry
    /// in one tree only.
    Query { query: ItemId, in_live: bool },
}

/// The first difference an audit found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditDifference {
    /// The tree's key (Java `ShapeSearchTree.getKey`).
    pub tree: String,
    /// The tree's index in the board's tree list (0 is the default tree).
    pub tree_index: usize,
    /// The item or room.
    pub object: TreeObject,
    /// The shape index.
    pub shape_index: usize,
    /// The shape's layer, if known: for an item no longer on the board, the
    /// layer it had at the previous audit.
    pub layer: Option<i32>,
    pub kind: DifferenceKind,
}

impl AuditDifference {
    /// The item, if the difference is about an item.
    pub fn item(&self) -> Option<ItemId> {
        self.object.item()
    }
}

impl fmt::Display for AuditDifference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (label, id) = self.object.walk_label();
        let what = if label == 'R' { "room" } else { "item" };
        write!(
            f,
            "tree {} (#{}): {what} {id} shape {}",
            self.tree, self.tree_index, self.shape_index
        )?;
        match self.layer {
            Some(layer) => write!(f, " on layer {layer}")?,
            None => f.write_str(" on an unknown layer")?,
        }
        match &self.kind {
            DifferenceKind::StaleItem => f.write_str(": in the tree but not on the board"),
            DifferenceKind::MissingItem => f.write_str(": on the board but not in the tree"),
            DifferenceKind::ShapeCount { live, rebuilt } => {
                write!(f, ": {live} tree shapes stored, {rebuilt} calculated")
            }
            DifferenceKind::ShapeDiffers { live, rebuilt } => {
                write!(
                    f,
                    ": stored tree shape {live} differs from the calculated {rebuilt}"
                )
            }
            DifferenceKind::LeafPresence { live } => {
                let (with, without) = if *live {
                    ("the live tree", "the rebuild")
                } else {
                    ("the rebuild", "the live tree")
                };
                write!(f, ": has a leaf in {with} and none in {without}")
            }
            DifferenceKind::Leaf {
                in_live,
                in_rebuilt,
            } => match (in_live, in_rebuilt) {
                (true, false) => f.write_str(": leaf only in the live tree"),
                (false, true) => f.write_str(": leaf only in the rebuild"),
                _ => f.write_str(": leaf bounds differ"),
            },
            DifferenceKind::Query { query, in_live } => write!(
                f,
                ": the query with the bounding box of item {} finds it only in {}",
                query.0,
                if *in_live {
                    "the live tree"
                } else {
                    "the rebuild"
                }
            ),
        }
    }
}

/// Stored tree shapes that differ from a fresh calculation the way Java's
/// incremental updates make them differ (see the module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tolerated {
    /// Kept trace shapes equal to the shape of the reversed trace.
    pub reversed: u64,
    /// Kept trace shapes whose bounding octagon is within one unit of the
    /// calculated one's (lines kept by Java's geometric equality but
    /// defined by other points, which offset with another rounding).
    pub rounded: u64,
    /// First or last trace shapes inside the calculated one (tie-pin
    /// reductions).
    pub reduced: u64,
}

/// The tally of a run's audits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditReport {
    /// Audits run.
    pub checks: u64,
    /// Audits that found a difference.
    pub failed_checks: u64,
    /// The first difference of the run and where it was found.
    pub first: Option<(AuditPoint, AuditDifference)>,
    /// The tolerated shapes of the last check, over all trees.
    pub tolerated: Tolerated,
}

impl AuditReport {
    /// The summary line of the job log.
    pub fn summary_line(&self) -> String {
        format!(
            "Incremental search-tree audit: {} checks, {} with differences (last check \
             tolerated {} reversed, {} rounded and {} reduced trace shapes).",
            self.checks,
            self.failed_checks,
            self.tolerated.reversed,
            self.tolerated.rounded,
            self.tolerated.reduced
        )
    }
}

/// The audit state of one run: the tally and each item's shape layers at
/// the previous audit, to name the layer of an item that left the board
/// while its entries stayed in a tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IncrementalAudit {
    report: AuditReport,
    layers: BTreeMap<ItemId, Vec<i32>>,
}

impl IncrementalAudit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Audits every tree of `board`. Returns the first difference, which
    /// the report keeps if it is the run's first.
    pub fn check(&mut self, board: &BasicBoard, point: AuditPoint) -> Option<AuditDifference> {
        self.report.checks += 1;
        let (difference, tolerated) = audit_board_with(board, &self.layers);
        self.report.tolerated = tolerated;
        self.layers = shape_layers(board);
        if let Some(difference) = &difference {
            self.report.failed_checks += 1;
            if self.report.first.is_none() {
                self.report.first = Some((point, difference.clone()));
            }
        }
        difference
    }

    pub fn report(&self) -> &AuditReport {
        &self.report
    }
}

/// Audits every tree of `board` without layer memory (see the module
/// docs). Returns the first difference.
pub fn audit_board(board: &BasicBoard) -> Option<AuditDifference> {
    audit_board_with(board, &BTreeMap::new()).0
}

fn audit_board_with(
    board: &BasicBoard,
    known_layers: &BTreeMap<ItemId, Vec<i32>>,
) -> (Option<AuditDifference>, Tolerated) {
    let items = board.item_list();
    let ctx = board.shape_context();
    let tolerated = Cell::new(Tolerated::default());
    let difference = board
        .search_trees()
        .trees()
        .iter()
        .enumerate()
        .find_map(|(index, tree)| {
            let at = TreeAt {
                tree,
                index,
                items,
                known_layers,
                ctx: &ctx,
                tolerated: &tolerated,
            };
            at.audit()
        });
    (difference, tolerated.get())
}

/// The layer of each stored shape of the default tree's items.
fn shape_layers(board: &BasicBoard) -> BTreeMap<ItemId, Vec<i32>> {
    let items = board.item_list();
    board
        .search_trees()
        .default_tree()
        .entries
        .iter()
        .filter_map(|(id, entries)| {
            let item = items.get(id)?;
            let layers = (0..entries.shapes.len()).map(|i| item.shape_layer(i));
            Some((*id, layers.collect()))
        })
        .collect()
}

/// One live tree under audit.
struct TreeAt<'a> {
    tree: &'a ShapeSearchTree,
    index: usize,
    items: &'a UndoableObjects<ItemId, Item>,
    known_layers: &'a BTreeMap<ItemId, Vec<i32>>,
    ctx: &'a ShapeContext<'a>,
    tolerated: &'a Cell<Tolerated>,
}

impl TreeAt<'_> {
    fn audit(&self) -> Option<AuditDifference> {
        let calculated = self.calculate();
        if let Some(found) = self.compare_entries(&calculated) {
            return Some(found);
        }
        let rebuilt = self.rebuild();
        self.compare_leaves(&rebuilt)
            .or_else(|| self.compare_queries(&rebuilt))
    }

    /// The entries a fresh build of the tree would have: the tree shapes of
    /// the items on the board, calculated (the tree itself is not built).
    fn calculate(&self) -> ShapeSearchTree {
        let mut calculated =
            ShapeSearchTree::new(self.tree.kind, self.tree.compensated_clearance_class);
        for (id, item) in self.items.iter().filter(|(_, i)| i.is_on_the_board()) {
            let shapes = calculated.calculate_tree_shapes(item, self.ctx);
            let leaves = vec![None; shapes.len()];
            calculated
                .entries
                .insert(*id, super::ItemEntries { shapes, leaves });
        }
        calculated
    }

    /// A fresh tree of the same kind and class: the items on the board in
    /// item-list order with the shapes step 1 accepted (the live tree's
    /// stored ones), then the live tree's rooms in id order.
    fn rebuild(&self) -> ShapeSearchTree {
        let mut rebuilt =
            ShapeSearchTree::new(self.tree.kind, self.tree.compensated_clearance_class);
        let directions = rebuilt.tree.bounding_directions();
        for (id, _) in self.items.iter().filter(|(_, i)| i.is_on_the_board()) {
            let Some(live) = self.tree.entries.get(id) else {
                continue;
            };
            let leaves = live
                .shapes
                .iter()
                .enumerate()
                .map(|(index, shape)| {
                    let bounds = shape.as_ref()?.bounding_shape(directions)?;
                    Some(rebuilt.tree.insert(TreeObject::Item(*id), index, bounds))
                })
                .collect();
            let shapes = live.shapes.clone();
            rebuilt
                .entries
                .insert(*id, super::ItemEntries { shapes, leaves });
        }
        for (id, room) in &self.tree.rooms {
            rebuilt.insert_room(*id, room.shape.clone(), room.layer);
        }
        rebuilt
    }

    fn layer_of(&self, object: TreeObject, shape_index: usize) -> Option<i32> {
        match object {
            TreeObject::Item(id) => match self.items.get(&id) {
                Some(item) => Some(item.shape_layer(shape_index)),
                None => self
                    .known_layers
                    .get(&id)
                    .and_then(|layers| layers.get(shape_index).copied()),
            },
            TreeObject::Room(id) => self.tree.rooms.get(&id).map(|room| room.layer),
        }
    }

    fn difference(
        &self,
        object: TreeObject,
        shape_index: usize,
        kind: DifferenceKind,
    ) -> AuditDifference {
        AuditDifference {
            tree: self.tree.key(),
            tree_index: self.index,
            object,
            shape_index,
            layer: self.layer_of(object, shape_index),
            kind,
        }
    }

    /// Step 1: the same items with the same stored shapes and leaves.
    fn compare_entries(&self, rebuilt: &ShapeSearchTree) -> Option<AuditDifference> {
        let mut live = self.tree.entries.iter().peekable();
        let mut fresh = rebuilt.entries.iter().peekable();
        loop {
            let (id, kind) = match (live.peek(), fresh.peek()) {
                (None, None) => return None,
                (Some((id, _)), None) => (**id, DifferenceKind::StaleItem),
                (None, Some((id, _))) => (**id, DifferenceKind::MissingItem),
                (Some((a, _)), Some((b, _))) if a < b => (**a, DifferenceKind::StaleItem),
                (Some((a, _)), Some((b, _))) if b < a => (**b, DifferenceKind::MissingItem),
                (Some(_), Some(_)) => {
                    let (id, l) = live.next().expect("peeked");
                    let (_, r) = fresh.next().expect("peeked");
                    if let Some(found) = self.compare_item(*id, l, r) {
                        return Some(found);
                    }
                    continue;
                }
            };
            return Some(self.difference(TreeObject::Item(id), 0, kind));
        }
    }

    fn compare_item(
        &self,
        id: ItemId,
        live: &super::ItemEntries,
        rebuilt: &super::ItemEntries,
    ) -> Option<AuditDifference> {
        let object = TreeObject::Item(id);
        if live.shapes.len() != rebuilt.shapes.len() || live.leaves.len() != live.shapes.len() {
            let kind = DifferenceKind::ShapeCount {
                live: live.shapes.len(),
                rebuilt: rebuilt.shapes.len(),
            };
            return Some(self.difference(object, 0, kind));
        }
        for (index, (a, b)) in live.shapes.iter().zip(&rebuilt.shapes).enumerate() {
            let same = match (a, b) {
                (Some(a), Some(b)) => same_tile_shape(a, b),
                (None, None) => true,
                _ => false,
            };
            if !same && !self.tolerates(id, index, live.shapes.len(), a.as_ref()) {
                let kind = DifferenceKind::ShapeDiffers {
                    live: format!("{a:?}"),
                    rebuilt: format!("{b:?}"),
                };
                return Some(self.difference(object, index, kind));
            }
            let directions = self.tree.tree.bounding_directions();
            let has_live = live.leaves[index].is_some();
            let should = a
                .as_ref()
                .and_then(|s| s.bounding_shape(directions))
                .is_some();
            if has_live != should {
                let kind = DifferenceKind::LeafPresence { live: has_live };
                return Some(self.difference(object, index, kind));
            }
        }
        None
    }

    /// True if the stored shape `index` of item `id` (of `count`) differs
    /// from the calculated one in a way Java's incremental updates produce
    /// (see the module docs), and counts it.
    fn tolerates(
        &self,
        id: ItemId,
        index: usize,
        count: usize,
        stored: Option<&TileShape>,
    ) -> bool {
        let Some(item) = self.items.get(&id) else {
            return false;
        };
        let ItemKind::Trace(trace) = &item.kind else {
            return false;
        };
        let Some(stored) = stored else {
            return false;
        };
        let forward = self.tree.calculate_tree_shapes(item, self.ctx);
        let mut reversed_item = item.clone();
        if let ItemKind::Trace(t) = &mut reversed_item.kind {
            t.polyline = trace.polyline.reverse();
        }
        let reversed = self.tree.calculate_tree_shapes(&reversed_item, self.ctx);
        let mirrored = count.checked_sub(index + 1).and_then(|i| reversed.get(i));
        if let Some(Some(shape)) = mirrored
            && same_tile_shape(stored, shape)
        {
            self.count(|t| t.reversed += 1);
            return true;
        }
        let near = |shape: Option<&Option<TileShape>>| {
            shape
                .and_then(Option::as_ref)
                .is_some_and(|f| within_one_unit(stored, f))
        };
        if near(forward.get(index)) || near(mirrored) {
            self.count(|t| t.rounded += 1);
            return true;
        }
        let at_end = index == 0 || index + 1 == count;
        let inside = |shape: Option<&Option<TileShape>>| {
            shape
                .and_then(Option::as_ref)
                .is_some_and(|f| inside_bounds(stored, f))
        };
        if at_end && (inside(forward.get(index)) || inside(mirrored)) {
            self.count(|t| t.reduced += 1);
            return true;
        }
        false
    }

    fn count(&self, add: impl FnOnce(&mut Tolerated)) {
        let mut tolerated = self.tolerated.get();
        add(&mut tolerated);
        self.tolerated.set(tolerated);
    }

    /// Step 2: the content digests, and on a difference the first leaf
    /// that differs.
    fn compare_leaves(&self, rebuilt: &ShapeSearchTree) -> Option<AuditDifference> {
        if self.tree.content_digest() == rebuilt.content_digest() {
            return None;
        }
        let live = sorted_leaves(self.tree);
        let fresh = sorted_leaves(rebuilt);
        let mut i = 0;
        let mut j = 0;
        loop {
            let (entry, in_live, in_rebuilt) = match (live.get(i), fresh.get(j)) {
                (None, None) => return None,
                (Some(a), None) => (*a, true, false),
                (None, Some(b)) => (*b, false, true),
                (Some(a), Some(b)) if (a.0, a.1) < (b.0, b.1) => (*a, true, false),
                (Some(a), Some(b)) if (b.0, b.1) < (a.0, a.1) => (*b, false, true),
                (Some(a), Some(b)) => {
                    i += 1;
                    j += 1;
                    if a.2 == b.2 {
                        continue;
                    }
                    (*a, true, true)
                }
            };
            let kind = DifferenceKind::Leaf {
                in_live,
                in_rebuilt,
            };
            return Some(self.difference(entry.0, entry.1, kind));
        }
    }

    /// Step 3: every board item's bounding box as an all-layer overlap
    /// query on both trees.
    fn compare_queries(&self, rebuilt: &ShapeSearchTree) -> Option<AuditDifference> {
        let directions = self.tree.tree.bounding_directions();
        let mut stack = Vec::new();
        let mut live = Vec::new();
        let mut fresh = Vec::new();
        for (id, item) in self.items.iter().filter(|(_, i)| i.is_on_the_board()) {
            let Some(bounds) = TileShape::IntBox(item.bounding_box()).bounding_shape(directions)
            else {
                continue;
            };
            query_leaves(self.tree, &bounds, &mut stack, &mut live);
            query_leaves(rebuilt, &bounds, &mut stack, &mut fresh);
            if live == fresh {
                continue;
            }
            let first = live
                .iter()
                .zip(&fresh)
                .position(|(a, b)| a != b)
                .unwrap_or(live.len().min(fresh.len()));
            let (entry, in_live) = match (live.get(first), fresh.get(first)) {
                (Some(a), Some(b)) if a < b => (*a, true),
                (Some(_), Some(b)) => (*b, false),
                (Some(a), None) => (*a, true),
                (None, Some(b)) => (*b, false),
                (None, None) => unreachable!("the results differ"),
            };
            let kind = DifferenceKind::Query {
                query: *id,
                in_live,
            };
            return Some(self.difference(entry.0, entry.1, kind));
        }
        None
    }
}

/// The leaves of `tree` as (object, shape index, bounds), sorted.
fn sorted_leaves(tree: &ShapeSearchTree) -> Vec<(TreeObject, usize, RegularTileShape)> {
    let mut leaves: Vec<_> = tree
        .tree
        .to_array()
        .into_iter()
        .map(|leaf| {
            let (object, index) = tree.tree.leaf(leaf);
            (object, index, tree.tree.leaf_bounds(leaf))
        })
        .collect();
    leaves.sort_unstable_by_key(|(object, index, _)| (*object, *index));
    leaves
}

/// The leaves of `tree` whose bounds intersect `bounds`, sorted, found
/// without touching the work meter.
fn query_leaves(
    tree: &ShapeSearchTree,
    bounds: &RegularTileShape,
    stack: &mut Vec<usize>,
    result: &mut Vec<(TreeObject, usize)>,
) {
    result.clear();
    let query = bounds.to_tile_shape();
    tree.tree.depth_first(stack, |node, leaf| {
        let hit = node.to_tile_shape().intersects_tile(&query);
        if hit && let Some(entry) = leaf {
            result.push(entry);
        }
        hit
    });
    result.sort_unstable();
}

/// The bounding octagon of `shape`.
fn bounding_octagon(shape: &TileShape) -> Option<freerouting_geometry::IntOctagon> {
    match shape.bounding_shape(ShapeBoundingDirections::FortyfiveDegree) {
        Some(RegularTileShape::IntOctagon(o)) => Some(o),
        _ => None,
    }
}

/// True if the bounding octagons of `a` and `b` differ by at most one unit
/// in each of their eight bounds.
fn within_one_unit(a: &TileShape, b: &TileShape) -> bool {
    let (Some(a), Some(b)) = (bounding_octagon(a), bounding_octagon(b)) else {
        return false;
    };
    let fields = |o: &freerouting_geometry::IntOctagon| {
        [
            o.left_x,
            o.bottom_y,
            o.right_x,
            o.top_y,
            o.upper_left_diagonal_x,
            o.lower_right_diagonal_x,
            o.lower_left_diagonal_x,
            o.upper_right_diagonal_x,
        ]
    };
    fields(&a)
        .iter()
        .zip(fields(&b))
        .all(|(x, y)| (i64::from(*x) - i64::from(y)).abs() <= 1)
}

/// True if the bounding octagon of `inner` lies inside that of `outer`.
fn inside_bounds(inner: &TileShape, outer: &TileShape) -> bool {
    match (bounding_octagon(inner), bounding_octagon(outer)) {
        (Some(i), Some(o)) => i.is_contained_in_octagon(&o),
        _ => false,
    }
}

/// Exact equality: boxes and octagons by value, simplexes line by line
/// with the lines' defining points (`Line`'s `==` is Java's geometric
/// `equals`).
fn same_tile_shape(a: &TileShape, b: &TileShape) -> bool {
    match (a, b) {
        (TileShape::IntBox(a), TileShape::IntBox(b)) => a == b,
        (TileShape::IntOctagon(a), TileShape::IntOctagon(b)) => a == b,
        (TileShape::Simplex(a), TileShape::Simplex(b)) => {
            a.lines().len() == b.lines().len()
                && a.lines()
                    .iter()
                    .zip(b.lines())
                    .all(|(l, m)| l.a() == m.a() && l.b() == m.b())
        }
        _ => false,
    }
}

/// The seeded fault of P02.1's acceptance: a test build can skip one tree
/// removal.
#[cfg(test)]
pub(super) mod fault {
    use std::cell::Cell;

    use crate::board::items::ItemId;

    thread_local! {
        static SKIP: Cell<Option<(ItemId, usize)>> = const { Cell::new(None) };
    }

    /// The next removal of `id` from tree `tree` is skipped.
    pub fn skip_next_removal(id: ItemId, tree: usize) {
        SKIP.set(Some((id, tree)));
    }

    /// True (once) if the removal of `id` from tree `tree` is to be skipped.
    pub fn skips_removal(id: ItemId, tree: usize) -> bool {
        let hit = SKIP.get() == Some((id, tree));
        if hit {
            SKIP.set(None);
        }
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::RoutingBoard;
    use crate::io::{BoardReadResult, read_board};

    fn load(name: &str) -> RoutingBoard {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name);
        match read_board(&std::fs::read(path).unwrap()) {
            BoardReadResult::Success { board, .. } => *board,
            _ => panic!("{name} must load"),
        }
    }

    /// Issue413 with an autoroute tree beside the default tree.
    fn board_with_two_trees() -> RoutingBoard {
        let mut board = load("Issue413-test.dsn");
        board.autoroute_search_tree(1);
        assert_eq!(board.search_trees().trees().len(), 2);
        board
    }

    /// A trace the board lets us delete, and its layer.
    fn removable_trace(board: &RoutingBoard) -> (ItemId, i32) {
        board
            .items()
            .find(|item| item.as_trace().is_some() && !board.is_deletion_forbidden(item))
            .map(|item| (item.id(), item.shape_layer(0)))
            .expect("Issue413 has a removable trace")
    }

    #[test]
    fn loaded_and_edited_board_audits_clean() {
        let mut board = board_with_two_trees();
        let units = board.search_trees().work_meter().units();
        let mut audit = IncrementalAudit::new();
        assert_eq!(audit.check(&board, AuditPoint::Manual), None);
        let (trace, _) = removable_trace(&board);
        assert!(board.remove_item(trace));
        assert_eq!(audit.check(&board, AuditPoint::Manual), None);
        assert_eq!(audit.report().checks, 2);
        assert_eq!(audit.report().failed_checks, 0);
        // The audit's queries and rebuilds count nothing on the board's meter.
        assert_eq!(board.search_trees().work_meter().units(), units);
    }

    #[test]
    fn skipped_tree_removal_is_reported_with_item_and_layer() {
        let mut board = board_with_two_trees();
        let mut audit = IncrementalAudit::new();
        assert_eq!(audit.check(&board, AuditPoint::Manual), None);
        let (trace, layer) = removable_trace(&board);
        fault::skip_next_removal(trace, 1);
        assert!(board.remove_item(trace));
        let found = audit
            .check(&board, AuditPoint::Ripup { pass: 1 })
            .expect("the stale entry is found");
        assert_eq!(found.item(), Some(trace));
        assert_eq!(found.layer, Some(layer));
        assert_eq!(found.tree_index, 1);
        assert_eq!(found.kind, DifferenceKind::StaleItem);
        assert_eq!(found.tree, board.search_trees().trees()[1].key());
        let report = audit.report();
        assert_eq!(report.failed_checks, 1);
        assert_eq!(
            report.first,
            Some((AuditPoint::Ripup { pass: 1 }, found.clone()))
        );
        let text = found.to_string();
        assert!(text.contains(&format!("item {}", trace.0)), "{text}");
        assert!(text.contains(&format!("on layer {layer}")), "{text}");
    }

    #[test]
    fn orphaned_leaf_of_a_skipped_removal_is_reported() {
        let mut board = board_with_two_trees();
        let (trace, layer) = removable_trace(&board);
        // A reinsert whose removal is skipped leaves the old leaves in the
        // default tree while the entries point at the new ones: the
        // entries balance, the leaves do not (KiCadRoutingTools #806).
        fault::skip_next_removal(trace, 0);
        board.reinsert_into_trees(trace);
        let found = audit_board(&board).expect("the orphaned leaf is found");
        assert_eq!(found.item(), Some(trace));
        assert_eq!(found.layer, Some(layer));
        assert_eq!(found.tree_index, 0);
        assert!(
            matches!(
                found.kind,
                DifferenceKind::Leaf {
                    in_live: true,
                    in_rebuilt: false
                }
            ),
            "{found}"
        );
    }

    #[test]
    fn moved_trace_without_tree_update_is_reported() {
        let mut board = board_with_two_trees();
        let (trace, layer) = removable_trace(&board);
        let shift = freerouting_geometry::Vector::get_instance(5_000, 0);
        if let Some(ItemKind::Trace(t)) = board.items.get_mut(&trace).map(|i| &mut i.kind) {
            t.polyline = t.polyline.translate_by(&shift);
        }
        let found = audit_board(&board).expect("the stale shape is found");
        assert_eq!(found.item(), Some(trace));
        assert_eq!(found.layer, Some(layer));
        assert_eq!(found.tree_index, 0);
        assert!(
            matches!(found.kind, DifferenceKind::ShapeDiffers { .. }),
            "{found}"
        );
    }

    #[test]
    fn exact_shape_equality_tells_simplexes_apart() {
        use freerouting_geometry::{IntBox, IntOctagon};
        let a = TileShape::IntBox(IntBox::new(0, 0, 10, 10));
        let b = TileShape::IntBox(IntBox::new(0, 0, 10, 11));
        assert!(same_tile_shape(&a, &a.clone()));
        assert!(!same_tile_shape(&a, &b));
        let o: IntOctagon = IntBox::new(0, 0, 10, 10).to_int_octagon();
        assert!(!same_tile_shape(&a, &TileShape::IntOctagon(o)));
    }
}

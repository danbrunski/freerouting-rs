//! The search trees of a board: overlap queries over the tile shapes of
//! the items and of the autorouter's rooms.
//!
//! Port of `board.searchtree.ShapeSearchTree`, `ShapeSearchTree45Degree`,
//! `ShapeSearchTree90Degree` and `SearchTreeManager`. The leaves are keyed by
//! [`TreeObject`] (rooms first, then items, each by id descending: Java's
//! `Leaf.compareTo`). The room completion algorithms of the autorouter
//! (`completeShape`, `restrainShape`, ...) are B03's.
//!
//! The tree's *shape* is part of the parity target (decision D5): room
//! completion visits obstacles in the tree's depth-first order, and work
//! limits count the nodes visited. [`fingerprint`] hashes the shape so a
//! probe can compare it with Java's.
//!
//! **Beyond Java (N03.1, P04).** [`SearchConfig`] can untie the router from
//! the tree's shape: `router.room_order = canonical` completes rooms in an
//! order that depends only on the tree's contents ([`RoomOrder`]), and
//! `router.work_unit = leaves` counts work in leaves returned instead of
//! nodes visited ([`WorkUnit`]). The defaults, and the `legacy` profile,
//! are Java's (`tree`, `nodes`); neither key is in profile `enhanced` until
//! N03.3's measurement accepts it (through N05).
//!
//! Java stores the tree shapes and leaves of an item in the item
//! (`ItemSearchTreesInfo`); here each tree keeps them in a map keyed by
//! [`ItemId`], and the rooms in a map keyed by [`RoomId`].
//!
//! A board's trees take trace shapes from one [`ShapeMemo`] (P02.5), which
//! the board's snapshots and restores share, when they build a whole tree (a
//! restore, an autoroute tree, a compensation change): a trace's shapes are
//! computed once per offset and tree kind, not again at every build. Single
//! insertions and [`ShapeSearchTree::calculate_tree_shapes`] compute; a tree
//! made with [`ShapeSearchTree::new`] has no memo.
//!
//! **The cutouts' switches (N06).** The tree shapes of an area with holes, and of
//! the board outline's generated keepout, come from the convex split under the
//! board's `router.java_quirks` cutout switches ([`ShapeContext::split_quirks`];
//! the outline keeps the value its piece count was taken with,
//! [`BoardOutline::keepout_split`]). [`SplitQuirks::JAVA`] is Java's split. The
//! board fixes the value before it inserts an item; a change afterwards reinserts
//! the items whose pieces change ([`crate::board::BasicBoard::set_java_quirks`]).

pub mod audit;
pub mod entries;
mod entry_map;
pub mod fingerprint;
pub mod memo;
pub mod tree_object;
pub mod work;

pub mod complete_shape;
mod restrain45;
mod restrain90;
pub mod rooms;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use freerouting_geometry::{
    Circle, FloatPoint, Line, Polyline, PolylineShapeOps, Shape, ShapeBoundingDirections,
    SplitQuirks, TileShape, TileShapeOps,
};

use crate::board::items::{BoardOutline, Item, ItemId, ItemKind};
use crate::board::state::Communication;
use crate::board::structure::{AngleRestriction, Unit};
use crate::datastructures::{LeafId, ShapeTree, UndoableObjects};
use crate::library::Padstacks;
use crate::rules::{BoardRules, ClearanceMatrix};

pub use complete_shape::RoomOrder;
pub use entries::{TraceChange, TraceMerge};
pub use fingerprint::TreeFingerprint;
pub use memo::{MemoCounts, ShapeMemo};
pub use tree_object::{ObjectEntry, RoomEntry, RoomId, TreeObject};
pub use work::{LEAF_UNITS_PER_NOMINAL_MS, UNITS_PER_NOMINAL_MS, WorkLimit, WorkMeter, WorkUnit};

/// The settings key of [`RoomOrder`] (N03.1).
pub const ROOM_ORDER_KEY: &str = "router.room_order";

/// The settings key of [`WorkUnit`] (N03.1).
pub const WORK_UNIT_KEY: &str = "router.work_unit";

/// How a board's search trees order room completion and count work
/// (N03.1, from P04.1 and P04.2): `router.room_order` and
/// `router.work_unit`. The default is Java's (tree order, nodes visited);
/// the routing pipeline sets it from the settings
/// ([`SearchConfig::from_settings`]) and the job's load sequence refuses an
/// unknown value. Board copies, snapshots and restores keep it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchConfig {
    pub room_order: RoomOrder,
    pub work_unit: WorkUnit,
    /// Test hook: whole-tree builds (autoroute trees, rebuilds) insert the
    /// items in reverse item-list order, which gives trees of the same
    /// contents and another shape.
    #[doc(hidden)]
    pub reversed_builds: bool,
}

impl SearchConfig {
    /// The configuration `router.room_order` and `router.work_unit` ask
    /// for; unset or blank values are the defaults. An unknown value is an
    /// error naming the key and the valid values.
    pub fn from_settings(settings: &crate::settings::RouterSettings) -> Result<Self, String> {
        let mut config = SearchConfig::default();
        if let Some(name) = setting_value(&settings.room_order) {
            config.room_order = RoomOrder::parse(name).ok_or_else(|| {
                format!("{ROOM_ORDER_KEY}: unknown value '{name}' (expected 'tree' or 'canonical')")
            })?;
        }
        if let Some(name) = setting_value(&settings.work_unit) {
            config.work_unit = WorkUnit::parse(name).ok_or_else(|| {
                format!("{WORK_UNIT_KEY}: unknown value '{name}' (expected 'nodes' or 'leaves')")
            })?;
        }
        Ok(config)
    }

    /// True for Java's order and unit (`tree`, `nodes`), the defaults and
    /// the `legacy` profile's; the test hook is ignored.
    pub fn is_java(&self) -> bool {
        self.room_order == RoomOrder::Tree && self.work_unit == WorkUnit::Nodes
    }

    /// The log line of a routing job: `Search trees: room order <o>, work
    /// unit <u>`.
    pub fn log_line(&self) -> String {
        format!(
            "Search trees: room order {}, work unit {}",
            self.room_order.name(),
            self.work_unit.name()
        )
    }
}

/// A string setting without its surrounding blanks; unset or blank is `None`.
fn setting_value(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// The extra margin added to the drill hole clearance.
const DRILL_HOLE_CLEARANCE_MARGIN: f64 = 10.0;

/// The shape strategy of a search tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    /// `ShapeSearchTree`: shapes as calculated, bounded by octagons.
    Plain,
    /// `ShapeSearchTree45Degree`: all shapes are octagons.
    FortyfiveDegree,
    /// `ShapeSearchTree90Degree`: all shapes are boxes.
    NinetyDegree,
}

/// One tile shape of an item in a search tree.
///
/// Port of `ShapeSearchTree.TreeEntry`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TreeEntry {
    pub item: ItemId,
    pub shape_index: usize,
}

/// What the tree shape calculation needs from the board.
#[derive(Clone, Copy)]
pub struct ShapeContext<'a> {
    pub rules: &'a BoardRules,
    pub padstacks: &'a Padstacks,
    pub communication: &'a Communication,
    pub layer_count: usize,
    /// The board's switches of the cutouts' convex split (N06,
    /// [`JavaQuirks::split`](crate::settings::JavaQuirks::split)), for the shapes of
    /// areas with holes. [`SplitQuirks::JAVA`] is Java's split.
    pub split_quirks: SplitQuirks,
}

#[derive(Clone, Debug, Default)]
struct ItemEntries {
    shapes: Vec<Option<TileShape>>,
    leaves: Vec<Option<LeafId>>,
}

/// A search tree over the tile shapes of the board items, optionally with
/// the shapes enlarged for a clearance class.
///
/// Port of `board.searchtree.ShapeSearchTree` and its 45 and 90 degree
/// subclasses.
#[derive(Clone, Debug)]
pub struct ShapeSearchTree {
    kind: TreeKind,
    /// The clearance class the shapes are compensated for; 0 for none.
    compensated_clearance_class: i32,
    tree: ShapeTree<TreeObject>,
    entries: entry_map::ItemIdMap<ItemEntries>,
    rooms: BTreeMap<RoomId, RoomEntry>,
    /// The board's trace-shape memo (P02.5); `None` for a tree of its own,
    /// which computes every shape.
    memo: Option<Arc<ShapeMemo>>,
    /// The obstacle order of room completion (P04.1).
    room_order: RoomOrder,
}

impl ShapeSearchTree {
    pub fn new(kind: TreeKind, compensated_clearance_class: i32) -> Self {
        let directions = match kind {
            TreeKind::NinetyDegree => ShapeBoundingDirections::Orthogonal,
            TreeKind::Plain | TreeKind::FortyfiveDegree => ShapeBoundingDirections::FortyfiveDegree,
        };
        ShapeSearchTree {
            kind,
            compensated_clearance_class,
            tree: ShapeTree::new(directions),
            entries: entry_map::ItemIdMap::default(),
            rooms: BTreeMap::new(),
            memo: None,
            room_order: RoomOrder::Tree,
        }
    }

    /// The obstacle order of room completion (P04.1).
    pub fn room_order(&self) -> RoomOrder {
        self.room_order
    }

    /// Completes rooms in `order` from now on.
    pub fn set_room_order(&mut self, order: RoomOrder) {
        self.room_order = order;
    }

    /// An empty tree of the same kind and class, counting on the same meter
    /// and sharing the same memo.
    fn emptied(&self) -> ShapeSearchTree {
        let mut tree = ShapeSearchTree::new(self.kind, self.compensated_clearance_class);
        tree.tree.set_meter(self.tree.meter().clone());
        tree.memo = self.memo.clone();
        tree.room_order = self.room_order;
        tree
    }

    pub fn kind(&self) -> TreeKind {
        self.kind
    }

    /// The clearance class the shapes are compensated for; 0 for none.
    pub fn compensated_clearance_class(&self) -> i32 {
        self.compensated_clearance_class
    }

    /// Java `ShapeSearchTree.getKey`.
    pub fn key(&self) -> String {
        let (class_name, directions) = match self.kind {
            TreeKind::Plain => ("ShapeSearchTree", "FortyfiveDegree"),
            TreeKind::FortyfiveDegree => ("ShapeSearchTree45Degree", "FortyfiveDegree"),
            TreeKind::NinetyDegree => ("ShapeSearchTree90Degree", "Orthogonal"),
        };
        format!(
            "{}_{}_cc{}",
            class_name, directions, self.compensated_clearance_class
        )
    }

    pub fn is_clearance_compensation_used(&self) -> bool {
        self.compensated_clearance_class > 0
    }

    /// The number of leaves.
    pub fn size(&self) -> usize {
        self.tree.size()
    }

    /// The amount the shapes of class `class` are enlarged by on `layer`.
    pub fn clearance_compensation_value(
        &self,
        matrix: &ClearanceMatrix,
        class: i32,
        layer: i32,
    ) -> i32 {
        if class <= 0 {
            return 0;
        }
        let result = matrix.get_value(class, self.compensated_clearance_class, layer, false)
            - matrix.clearance_compensation_value(self.compensated_clearance_class, layer);
        result.max(0)
    }

    /// The underlying shape tree, for inspecting its shape (leaf order,
    /// depths and bounds).
    pub fn shape_tree(&self) -> &ShapeTree<TreeObject> {
        &self.tree
    }

    /// True if `id` has entries in this tree.
    pub fn contains(&self, id: ItemId) -> bool {
        self.entries.contains_key(&id)
    }

    /// Inserts the tree shapes of `item`.
    pub fn insert(&mut self, item: &Item, ctx: &ShapeContext<'_>) {
        let shapes = self.calculate_tree_shapes(item, ctx);
        self.insert_shapes(item, shapes);
    }

    /// [`insert`](Self::insert) with the trace shapes from the board's memo
    /// (P02.5): for whole-tree builds, which meet the same traces again.
    pub(crate) fn insert_memoized(&mut self, item: &Item, ctx: &ShapeContext<'_>) {
        let shapes = self.tree_shapes_of(item, ctx, self.memo.as_deref());
        self.insert_shapes(item, shapes);
    }

    fn insert_shapes(&mut self, item: &Item, shapes: Vec<Option<TileShape>>) {
        let directions = self.tree.bounding_directions();
        let mut leaves = Vec::with_capacity(shapes.len());
        for (index, shape) in shapes.iter().enumerate() {
            let leaf = shape
                .as_ref()
                .and_then(|s| s.bounding_shape(directions))
                .map(|bounds| self.tree.insert(TreeObject::Item(item.id()), index, bounds));
            leaves.push(leaf);
        }
        self.entries
            .insert(item.id(), ItemEntries { shapes, leaves });
    }

    /// Removes the entries of `id`.
    pub fn remove(&mut self, id: ItemId) {
        if let Some(entries) = self.entries.remove(&id) {
            for leaf in entries.leaves.into_iter().flatten() {
                self.tree.remove_leaf(leaf);
            }
        }
    }

    /// Inserts room `id` with its one tree shape `shape` on `layer` (Java
    /// `ShapeTree.insert` of a `CompleteFreeSpaceExpansionRoom`). The
    /// caller supplies the id. A room already in the tree is replaced.
    pub fn insert_room(&mut self, id: RoomId, shape: TileShape, layer: i32) {
        self.remove_room(id);
        let leaf = shape
            .bounding_shape(self.tree.bounding_directions())
            .map(|bounds| self.tree.insert(TreeObject::Room(id), 0, bounds));
        self.rooms.insert(id, RoomEntry { shape, layer, leaf });
    }

    /// Removes room `id` (Java `CompleteFreeSpaceExpansionRoom.removeFromTree`).
    pub fn remove_room(&mut self, id: RoomId) {
        if let Some(entry) = self.rooms.remove(&id)
            && let Some(leaf) = entry.leaf
        {
            self.tree.remove_leaf(leaf);
        }
    }

    /// Moves room `old` to the unused key below every room key in this
    /// tree, in place: its leaf stays where it is (the tree's shape is
    /// unchanged, D5) and only its key changes. Returns the new key, or
    /// `None` if there is no room `old`.
    ///
    /// For rooms a failed connection leaves in the tree (A06.5): Java's
    /// `autorouteConnection` returns before `clear()` when the search
    /// cannot be initialised, so its rooms stay in the tree, and a later
    /// connection's engine counts its room ids from 1 again. Java then has
    /// two rooms with one id; this tree keys rooms by id, so the old room
    /// moves aside before the new one is inserted.
    pub fn rekey_room_aside(&mut self, old: RoomId) -> Option<RoomId> {
        let entry = self.rooms.remove(&old)?;
        let lowest = self.rooms.keys().next().map_or(0, |k| k.0.min(0));
        let new = RoomId(lowest - 1);
        if let Some(leaf) = entry.leaf {
            self.tree.set_leaf(leaf, TreeObject::Room(new), 0);
        }
        self.rooms.insert(new, entry);
        Some(new)
    }

    /// The room `id`, if it is in this tree.
    pub fn room(&self, id: RoomId) -> Option<&RoomEntry> {
        self.rooms.get(&id)
    }

    /// The number of rooms in this tree.
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    /// The number of tree shapes of `item`.
    pub fn tree_shape_count(&self, item: &Item, ctx: &ShapeContext<'_>) -> usize {
        match (self.entries.get(&item.id()), &item.kind) {
            (Some(entries), _) => entries.shapes.len(),
            // One shape per segment (`tree_shapes_of`), without computing them.
            (None, ItemKind::Trace(trace)) => trace.tile_shape_count(),
            (None, _) => self.calculate_tree_shapes(item, ctx).len(),
        }
    }

    /// The tree shape `index` of `item`: the stored one if the item is in
    /// the tree, else calculated.
    pub fn tree_shape(
        &self,
        item: &Item,
        index: usize,
        ctx: &ShapeContext<'_>,
    ) -> Option<TileShape> {
        match (self.entries.get(&item.id()), &item.kind) {
            (Some(entries), _) => entries.shapes.get(index).cloned().flatten(),
            // Only segment `index` of the shapes `tree_shapes_of` computes.
            (None, ItemKind::Trace(trace)) => (index < trace.tile_shape_count())
                .then(|| {
                    let offset = self.trace_offset(item, trace.half_width, trace.layer, ctx);
                    self.trace_offset_shape(&trace.polyline, offset, index)
                })
                .flatten(),
            (None, _) => self
                .calculate_tree_shapes(item, ctx)
                .into_iter()
                .nth(index)
                .flatten(),
        }
    }

    /// All tree shapes of `item`.
    pub fn tree_shapes(&self, item: &Item, ctx: &ShapeContext<'_>) -> Vec<Option<TileShape>> {
        match self.entries.get(&item.id()) {
            Some(entries) => entries.shapes.clone(),
            None => self.calculate_tree_shapes(item, ctx),
        }
    }

    /// Appends the item entries whose shape overlaps `shape` on `layer` (all
    /// layers if negative) to `result`, skipping items that contain one of
    /// `ignore_nets`. Rooms are skipped; [`overlapping_object_entries`]
    /// returns them too.
    ///
    /// Port of `ShapeSearchTree.overlappingTreeEntries`.
    ///
    /// [`overlapping_object_entries`]: Self::overlapping_object_entries
    pub fn overlapping_tree_entries(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        items: &UndoableObjects<ItemId, Item>,
        result: &mut Vec<TreeEntry>,
    ) {
        self.overlapping_entries_impl(shape, layer, ignore_nets, items, |entry| {
            if let TreeObject::Item(item) = entry.object {
                result.push(TreeEntry {
                    item,
                    shape_index: entry.shape_index,
                });
            }
        });
    }

    /// Q22.11: the items with a tree shape whose bounds overlap the bounds
    /// of `shape`, on any layer, in ascending id order, without touching the
    /// work meter: the hole-to-hole check is no Java search, so a run with a
    /// hole-to-hole minimum stops its work limits at the same counts
    /// ([`crate::board::hole_spacing`]).
    pub fn items_near_unmetered(&self, shape: &TileShape) -> BTreeSet<ItemId> {
        let Some(bounds) = shape.bounding_shape(self.tree.bounding_directions()) else {
            return BTreeSet::new();
        };
        self.tree
            .overlaps_reference(&bounds)
            .0
            .into_iter()
            .filter_map(|leaf| match self.tree.leaf(leaf).0 {
                TreeObject::Item(id) => Some(id),
                TreeObject::Room(_) => None,
            })
            .collect()
    }

    /// Appends the entries of items and rooms whose shape overlaps `shape`
    /// on `layer` (all layers if negative) to `result`, in Java's leaf order
    /// (rooms first, ids descending, then shape index). Rooms are obstacles
    /// for every net, so `ignore_nets` never drops them.
    ///
    /// Port of `ShapeSearchTree.overlappingTreeEntries` on `SearchTreeObject`s.
    pub fn overlapping_object_entries(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        items: &UndoableObjects<ItemId, Item>,
        result: &mut Vec<ObjectEntry>,
    ) {
        self.overlapping_entries_impl(shape, layer, ignore_nets, items, |entry| result.push(entry));
    }

    /// The items and rooms whose shape overlaps `shape` on `layer`, sorted
    /// (Java `ShapeSearchTree.overlappingObjects`, a `TreeSet<SearchTreeObject>`).
    pub fn overlapping_objects(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        items: &UndoableObjects<ItemId, Item>,
    ) -> BTreeSet<TreeObject> {
        let mut result = BTreeSet::new();
        self.overlapping_entries_impl(shape, layer, ignore_nets, items, |entry| {
            result.insert(entry.object);
        });
        result
    }

    fn overlapping_entries_impl(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        items: &UndoableObjects<ItemId, Item>,
        mut add: impl FnMut(ObjectEntry),
    ) {
        let Some(bounds) = shape.bounding_shape(self.tree.bounding_directions()) else {
            return;
        };
        let is_45_degree = matches!(shape, TileShape::IntOctagon(_));
        for leaf in self.tree.overlaps(&bounds) {
            let (object, shape_index) = self.tree.leaf(leaf);
            let current = match object {
                TreeObject::Item(id) => {
                    let Some(item) = items.get(&id) else {
                        continue;
                    };
                    if layer >= 0 && item.shape_layer(shape_index) != layer {
                        continue;
                    }
                    if ignore_nets.iter().any(|n| !item.is_obstacle_for_net(*n)) {
                        continue;
                    }
                    let Some(current) = self.stored_shape(id, shape_index) else {
                        continue;
                    };
                    current
                }
                TreeObject::Room(id) => {
                    let Some(room) = self.rooms.get(&id) else {
                        continue;
                    };
                    if layer >= 0 && room.layer != layer {
                        continue;
                    }
                    &room.shape
                }
            };
            let add_entry = (is_45_degree && matches!(current, TileShape::IntOctagon(_)))
                || current.intersects_tile(shape);
            if add_entry {
                add(ObjectEntry {
                    object,
                    shape_index,
                });
            }
        }
    }

    /// The entries whose shapes come closer to `shape` than the clearance
    /// of `clearance_class` on `layer` requires. With clearance compensation
    /// the plain overlap query is used.
    ///
    /// Port of `ShapeSearchTree.overlappingTreeEntriesWithClearance`.
    pub fn overlapping_tree_entries_with_clearance(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
        matrix: &ClearanceMatrix,
        items: &UndoableObjects<ItemId, Item>,
    ) -> Vec<TreeEntry> {
        self.overlapping_tree_entries_with_clearance_where(
            shape,
            layer,
            ignore_nets,
            clearance_class,
            matrix,
            items,
            |_| true,
        )
    }

    /// [`overlapping_tree_entries_with_clearance`](Self::overlapping_tree_entries_with_clearance)
    /// restricted to the items for which `keep` is true. The result is the
    /// one of the unrestricted query without the other items; the
    /// restriction only saves the geometric tests of the dropped entries.
    #[allow(clippy::too_many_arguments)]
    pub fn overlapping_tree_entries_with_clearance_where(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
        matrix: &ClearanceMatrix,
        items: &UndoableObjects<ItemId, Item>,
        keep: impl Fn(&Item) -> bool,
    ) -> Vec<TreeEntry> {
        if self.is_clearance_compensation_used() {
            let mut result = Vec::new();
            self.overlapping_tree_entries(shape, layer, ignore_nets, items, &mut result);
            result.retain(|e| items.get(&e.item).is_some_and(&keep));
            return result;
        }
        self.clearance_search_entries_where(
            shape,
            layer,
            ignore_nets,
            clearance_class,
            matrix,
            items,
            keep,
            true,
        )
    }

    /// Java's five-argument `ShapeSearchTree.overlappingTreeEntriesWithClearance`: the
    /// entries whose shapes come closer to `shape` than the clearance of
    /// `clearance_class` on `layer` requires, searched with the clearance even when
    /// the tree is clearance compensated (whose shapes are then enlarged twice, as in
    /// Java). `BasicBoard.overlappingItemsWithClearance` and
    /// `overlappingObjectsWithClearance` use this form;
    /// [`overlapping_tree_entries_with_clearance`](Self::overlapping_tree_entries_with_clearance)
    /// is the four-argument form, which switches to the plain overlap in a
    /// compensated tree. Rooms are skipped (Java casts every leaf to `Item`).
    pub fn clearance_search_entries(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
        matrix: &ClearanceMatrix,
        items: &UndoableObjects<ItemId, Item>,
    ) -> Vec<TreeEntry> {
        self.clearance_search_entries_where(
            shape,
            layer,
            ignore_nets,
            clearance_class,
            matrix,
            items,
            |_| true,
            true,
        )
    }

    /// [`clearance_search_entries`](Self::clearance_search_entries) without
    /// touching the work meter (the tree's reference traversal finds the same
    /// leaves): for a counter that only observes (N06.15), so that a run with
    /// statistics on cuts its work limits off at the same point.
    pub fn clearance_search_entries_unmetered(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
        matrix: &ClearanceMatrix,
        items: &UndoableObjects<ItemId, Item>,
    ) -> Vec<TreeEntry> {
        self.clearance_search_entries_where(
            shape,
            layer,
            ignore_nets,
            clearance_class,
            matrix,
            items,
            |_| true,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn clearance_search_entries_where(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
        matrix: &ClearanceMatrix,
        items: &UndoableObjects<ItemId, Item>,
        keep: impl Fn(&Item) -> bool,
        metered: bool,
    ) -> Vec<TreeEntry> {
        let mut result = Vec::new();
        let Some(bounds) = shape.bounding_shape(self.tree.bounding_directions()) else {
            return result;
        };
        let max_clearance = (1.2 * f64::from(matrix.max_value(clearance_class, layer))) as i32;
        let offset_bounds = match bounds.to_tile_shape().offset(f64::from(max_clearance)) {
            TileShape::IntBox(b) => b.into(),
            TileShape::IntOctagon(o) => o.into(),
            TileShape::Simplex(_) => bounds,
        };
        // (clearance, entry id) sorted like the Java TreeSet of
        // EntrySortedByClearance.
        let mut sorted: Vec<(i32, usize, LeafId)> = Vec::new();
        let leaves = if metered {
            self.tree.overlaps(&offset_bounds)
        } else {
            self.tree.overlaps_reference(&offset_bounds).0
        };
        for leaf in leaves {
            let (object, shape_index) = self.tree.leaf(leaf);
            // Java casts every leaf object to `Item` here; rooms never reach
            // this query.
            let TreeObject::Item(id) = object else {
                continue;
            };
            let Some(item) = items.get(&id) else {
                continue;
            };
            if layer >= 0 && item.shape_layer(shape_index) != layer {
                continue;
            }
            if ignore_nets.iter().any(|n| !item.is_obstacle_for_net(*n)) {
                continue;
            }
            if !keep(item) {
                continue;
            }
            let clearance = matrix.get_value(clearance_class, item.clearance_class(), layer, true);
            let entry_id = sorted.len();
            sorted.push((clearance, entry_id, leaf));
        }
        sorted.sort_by_key(|(clearance, entry_id, _)| (*clearance, *entry_id));
        let mut current_half_clearance = 0;
        let mut current_offset_shape = shape.clone();
        let mut current_offset_box = current_offset_shape.bounding_box();
        for (clearance, _, leaf) in sorted {
            let half_clearance = clearance / 2;
            if half_clearance != current_half_clearance {
                current_half_clearance = half_clearance;
                current_offset_shape = shape.enlarge(f64::from(half_clearance));
                current_offset_box = current_offset_shape.bounding_box();
            }
            let (object, shape_index) = self.tree.leaf(leaf);
            let TreeObject::Item(id) = object else {
                continue;
            };
            let Some(tree_shape) = self.stored_shape(id, shape_index) else {
                continue;
            };
            let offset_shape = tree_shape.enlarge(f64::from(current_half_clearance));
            // K13.9: shapes whose boxes are apart do not intersect.
            if !current_offset_box.is_apart_from(&offset_shape.bounding_box())
                && current_offset_shape.intersects_tile(&offset_shape)
            {
                result.push(TreeEntry {
                    item: id,
                    shape_index,
                });
            }
        }
        result
    }

    fn stored_shape(&self, id: ItemId, index: usize) -> Option<&TileShape> {
        self.entries.get(&id)?.shapes.get(index)?.as_ref()
    }

    /// Java `ShapeSearchTree.calculateTreeShapes` for every item class, with
    /// the overrides of the 45 and 90 degree trees.
    pub fn calculate_tree_shapes(
        &self,
        item: &Item,
        ctx: &ShapeContext<'_>,
    ) -> Vec<Option<TileShape>> {
        self.tree_shapes_of(item, ctx, None)
    }

    fn tree_shapes_of(
        &self,
        item: &Item,
        ctx: &ShapeContext<'_>,
        memo: Option<&ShapeMemo>,
    ) -> Vec<Option<TileShape>> {
        match &item.kind {
            ItemKind::Pin(_) | ItemKind::Via(_) => self.drill_item_tree_shapes(item, ctx),
            ItemKind::Trace(trace) => {
                let offset = self.trace_offset(item, trace.half_width, trace.layer, ctx);
                let compute = || {
                    (0..trace.tile_shape_count())
                        .map(|i| self.trace_offset_shape(&trace.polyline, offset, i))
                        .collect()
                };
                match memo {
                    Some(memo) => {
                        let boxes = self.kind == TreeKind::NinetyDegree;
                        memo.trace_shapes(&trace.polyline, offset, boxes, compute)
                    }
                    None => compute(),
                }
            }
            ItemKind::Area(area) => {
                // PARITY(quirk, java_quirks.octagon_box_cutout_branch3): with
                // `cutout_hole_crosses_border`, `simplex_octagon_half_corner` and
                // `simplex_prev_division_line`, the board's split switches; Java's
                // split when they are `true`.
                let shapes = self.obstacle_area_tree_shapes(
                    item,
                    area.area().split_to_convex_with(None, ctx.split_quirks),
                    area.layer(),
                    ctx,
                );
                self.regularize(shapes)
            }
            ItemKind::BoardOutline(_) => {
                let shapes = self.board_outline_tree_shapes(item, ctx);
                self.regularize(shapes)
            }
            ItemKind::ComponentOutline(_) => Vec::new(),
        }
    }

    /// The offset of the tree shapes of a trace of `item`'s clearance class:
    /// its half width plus the compensation on its layer.
    fn trace_offset(
        &self,
        item: &Item,
        half_width: i32,
        layer: i32,
        ctx: &ShapeContext<'_>,
    ) -> i32 {
        half_width
            + self.clearance_compensation_value(
                &ctx.rules.clearance_matrix,
                item.clearance_class(),
                layer,
            )
    }

    /// The 45 degree tree replaces area and outline shapes by their bounding
    /// octagons, the 90 degree tree by their bounding boxes.
    fn regularize(&self, shapes: Vec<Option<TileShape>>) -> Vec<Option<TileShape>> {
        match self.kind {
            TreeKind::Plain => shapes,
            TreeKind::FortyfiveDegree => shapes
                .into_iter()
                .map(|s| s.and_then(|s| s.bounding_octagon().map(TileShape::IntOctagon)))
                .collect(),
            TreeKind::NinetyDegree => shapes
                .into_iter()
                .map(|s| s.map(|s| TileShape::IntBox(s.bounding_box())))
                .collect(),
        }
    }

    fn trace_offset_shape(
        &self,
        polyline: &Polyline,
        half_width: i32,
        index: usize,
    ) -> Option<TileShape> {
        match self.kind {
            TreeKind::NinetyDegree => {
                Some(TileShape::IntBox(polyline.offset_box(half_width, index)))
            }
            _ => polyline.offset_shape(half_width, index),
        }
    }

    fn drill_item_tree_shapes(
        &self,
        item: &Item,
        ctx: &ShapeContext<'_>,
    ) -> Vec<Option<TileShape>> {
        let drill = item.drill().expect("drill item");
        let padstack = ctx.padstacks.get(drill.padstack);
        let mut result = Vec::with_capacity(drill.shapes.len());
        for i in 0..drill.shapes.len() {
            // K15.19: on a layer without pad the hole is the obstacle, held as
            // the circle around all of it.
            let on_pad = drill.shape(i).is_some();
            let shape = match drill.shape(i) {
                Some(s) => Some(s.clone()),
                None => drill_hole_obstacle(item, ctx),
            };
            let Some(shape) = shape else {
                result.push(None);
                continue;
            };
            let layer = drill.shape_layer(i);
            let mut offset = self.clearance_compensation_value(
                &ctx.rules.clearance_matrix,
                item.clearance_class(),
                layer,
            );
            offset += self.drill_hole_clearance_delta(item, padstack, &shape, layer, on_pad, ctx);
            let tile = match self.kind {
                TreeKind::Plain => {
                    let tile = match ctx.rules.trace_angle_restriction() {
                        AngleRestriction::NinetyDegree => TileShape::IntBox(shape.bounding_box()),
                        AngleRestriction::FortyfiveDegree => match shape.bounding_octagon() {
                            Some(o) => TileShape::IntOctagon(o),
                            None => {
                                result.push(None);
                                continue;
                            }
                        },
                        AngleRestriction::None => shape.bounding_tile(),
                    };
                    tile.enlarge(f64::from(offset))
                }
                TreeKind::FortyfiveDegree => {
                    let Some(octagon) = shape.bounding_octagon() else {
                        result.push(None);
                        continue;
                    };
                    let tile = if octagon.is_int_box() {
                        TileShape::IntBox(shape.bounding_box())
                    } else {
                        TileShape::IntOctagon(octagon)
                    };
                    match tile.offset(f64::from(offset)).bounding_octagon() {
                        Some(o) => TileShape::IntOctagon(o),
                        None => {
                            result.push(None);
                            continue;
                        }
                    }
                }
                TreeKind::NinetyDegree => {
                    TileShape::IntBox(shape.bounding_box().offset(f64::from(offset)))
                }
            };
            result.push(Some(tile));
        }
        result
    }

    /// Java `ShapeSearchTree.drillHoleClearanceDelta`. K15.19: with the
    /// pin's real hole (a `.kicad_pcb`'s, [`DrillData::hole`](crate::board::items::DrillData::hole))
    /// the drill radius is the hole's: a slot's half width on a pad layer
    /// (the pad's own border distance stands for the rest of the slot), the
    /// circle around all of it on a layer without pad.
    #[allow(clippy::too_many_arguments)]
    fn drill_hole_clearance_delta(
        &self,
        item: &Item,
        padstack: Option<&crate::library::Padstack>,
        shape: &Shape,
        layer: i32,
        on_pad: bool,
        ctx: &ShapeContext<'_>,
    ) -> i32 {
        let hole_clearance = ctx.rules.hole_clearance();
        let Some(padstack) = padstack else {
            return 0;
        };
        if hole_clearance <= 0 {
            return 0;
        }
        let drill = item.drill().expect("drill item");
        let drill_radius = match drill.hole {
            Some(hole) if !on_pad => hole.bounding_radius(),
            _ => drill.drill_radius(padstack),
        };
        if drill_radius <= 0.0 {
            return 0;
        }
        let copper_radius = if padstack.hole_only {
            drill_radius
        } else {
            let radius = shape.border_distance(drill.center.to_float());
            if radius <= 0.0 {
                match padstack.get_shape(layer) {
                    Some(pad) => Shape::from(pad.clone()).border_distance(FloatPoint::ZERO),
                    None => drill_radius,
                }
            } else {
                radius
            }
        };
        let clearance_class = if self.compensated_clearance_class > 0 {
            self.compensated_clearance_class
        } else {
            BoardRules::default_clearance_class()
        };
        let copper_clearance = ctx.rules.clearance_matrix.get_value(
            item.clearance_class(),
            clearance_class,
            layer,
            false,
        );
        let value = (drill_radius + f64::from(hole_clearance) + DRILL_HOLE_CLEARANCE_MARGIN
            - copper_radius
            - f64::from(copper_clearance))
        .ceil();
        (value as i32).max(0)
    }

    fn obstacle_area_tree_shapes(
        &self,
        item: &Item,
        convex_shapes: Option<Vec<TileShape>>,
        layer: i32,
        ctx: &ShapeContext<'_>,
    ) -> Vec<Option<TileShape>> {
        let Some(convex_shapes) = convex_shapes else {
            return Vec::new();
        };
        let mut max_tree_shape_width = 50000.0;
        if ctx.communication.host_cad_exists() {
            max_tree_shape_width = f64::min(
                500.0 * ctx.communication.get_resolution(Unit::Mil),
                max_tree_shape_width,
            );
        }
        let offset = self.clearance_compensation_value(
            &ctx.rules.clearance_matrix,
            item.clearance_class(),
            layer,
        );
        let mut result = Vec::new();
        for shape in convex_shapes {
            let enlarged = shape.enlarge(f64::from(offset));
            result.extend(
                enlarged
                    .divide_into_sections(max_tree_shape_width)
                    .into_iter()
                    .map(Some),
            );
        }
        result
    }

    fn board_outline_tree_shapes(
        &self,
        item: &Item,
        ctx: &ShapeContext<'_>,
    ) -> Vec<Option<TileShape>> {
        let outline = item.as_board_outline().expect("board outline");
        let matrix = &ctx.rules.clearance_matrix;
        let mut result = Vec::new();
        if outline.keepout_outside_outline_generated() {
            // PARITY(quirk, java_quirks.octagon_box_cutout_branch3): the split switches
            // the outline's piece count was taken with (the board's), Java's split when
            // they are `true`.
            let Some(convex_shapes) = outline.keepout_pieces() else {
                return result;
            };
            for layer in 0..ctx.layer_count as i32 {
                let offset =
                    self.clearance_compensation_value(matrix, item.clearance_class(), layer);
                for shape in &convex_shapes {
                    result.push(Some(shape.enlarge(f64::from(offset))));
                }
            }
        } else {
            let half_width = BoardOutline::HALF_WIDTH;
            for layer in 0..ctx.layer_count as i32 {
                for shape in outline.shapes() {
                    let count = shape.border_line_count();
                    let mut previous: Line = shape.border_line(count - 1);
                    for i in 0..count {
                        let current = shape.border_line(i);
                        let next = shape.border_line((i + 1) % count);
                        let polyline = Polyline::new(&[previous, current, next]);
                        let offset = self.clearance_compensation_value(
                            matrix,
                            item.clearance_class(),
                            layer,
                        );
                        result.push(polyline.offset_shape(half_width + offset, 0));
                        previous = current;
                    }
                }
            }
        }
        result
    }
}

/// Java `ShapeSearchTree.drillHoleObstacle`: the drill hole as a circle on
/// layers without pad, if hole clearances are checked.
fn drill_hole_obstacle(item: &Item, ctx: &ShapeContext<'_>) -> Option<Shape> {
    if ctx.rules.hole_clearance() <= 0 {
        return None;
    }
    let drill = item.drill()?;
    let padstack = ctx.padstacks.get(drill.padstack)?;
    // K15.19: a `.kicad_pcb` pin's real hole, as the circle around all of it.
    let drill_radius = drill.hole.map_or_else(
        || padstack.get_drill_radius(),
        |hole| hole.bounding_radius(),
    );
    if drill_radius <= 0.0 {
        return None;
    }
    Some(Shape::Circle(Circle::new(
        drill.center,
        drill_radius.ceil() as i32,
    )))
}

/// The search trees of a board: the default tree and the trees with
/// clearance compensation the autorouter asks for.
///
/// Port of `board.searchtree.SearchTreeManager`.
///
/// Its trees share one [`WorkMeter`], the board's: every overlap query on
/// them adds the nodes it visits (B02.7).
#[derive(Debug)]
pub struct SearchTreeManager {
    /// The default tree is always the first.
    trees: Vec<ShapeSearchTree>,
    clearance_compensation_used: bool,
    meter: Arc<WorkMeter>,
    /// The trace-shape memo of the trees (P02.5), shared with the board's
    /// copies, snapshots and restores.
    memo: Arc<ShapeMemo>,
    /// Room order and work unit (P04).
    config: SearchConfig,
}

impl Default for SearchTreeManager {
    fn default() -> Self {
        Self::with_memo(Arc::new(ShapeMemo::new()))
    }
}

impl Clone for SearchTreeManager {
    /// A copy counts on a meter of its own (another board's), starting at
    /// this one's count.
    fn clone(&self) -> Self {
        let meter = Arc::new(WorkMeter::clone(&self.meter));
        let mut trees = self.trees.clone();
        for tree in &mut trees {
            tree.tree.set_meter(meter.clone());
        }
        SearchTreeManager {
            trees,
            clearance_compensation_used: self.clearance_compensation_used,
            meter,
            memo: self.memo.clone(),
            config: self.config,
        }
    }
}

impl SearchTreeManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// A manager with only the default tree, whose trees share `memo`.
    pub fn with_memo(memo: Arc<ShapeMemo>) -> Self {
        let mut manager = SearchTreeManager {
            trees: Vec::new(),
            clearance_compensation_used: false,
            meter: Arc::new(WorkMeter::new()),
            memo,
            config: SearchConfig::default(),
        };
        let tree = manager.new_tree(TreeKind::Plain, 0);
        manager.trees.push(tree);
        manager
    }

    /// Room order and work unit of the trees (P04).
    pub fn search_config(&self) -> SearchConfig {
        self.config
    }

    /// Sets the room order of every tree, now and later, and the meter's
    /// unit. Set it before a limit starts (see [`WorkMeter::set_unit`]).
    pub fn set_search_config(&mut self, config: SearchConfig) {
        self.config = config;
        self.meter.set_unit(config.work_unit);
        for tree in &mut self.trees {
            tree.room_order = config.room_order;
        }
    }

    /// The trace-shape memo of the trees (P02.5).
    pub fn shape_memo(&self) -> &Arc<ShapeMemo> {
        &self.memo
    }

    /// The board's work meter: the nodes visited by the queries on its
    /// trees (Java `WorkMeter`, B02.7).
    pub fn work_meter(&self) -> &WorkMeter {
        &self.meter
    }

    /// A new tree counting on the board's meter.
    fn new_tree(&self, kind: TreeKind, class: i32) -> ShapeSearchTree {
        let mut tree = ShapeSearchTree::new(kind, class);
        tree.tree.set_meter(self.meter.clone());
        tree.memo = Some(self.memo.clone());
        tree.room_order = self.config.room_order;
        tree
    }

    /// The tree used for the board queries.
    pub fn default_tree(&self) -> &ShapeSearchTree {
        &self.trees[0]
    }

    /// All trees, the default tree first.
    pub fn trees(&self) -> &[ShapeSearchTree] {
        &self.trees
    }

    pub fn is_clearance_compensation_used(&self) -> bool {
        self.clearance_compensation_used
    }

    /// Inserts `item` into all trees. The caller marks it as on the board.
    pub fn insert(&mut self, item: &Item, ctx: &ShapeContext<'_>) {
        for tree in &mut self.trees {
            tree.insert(item, ctx);
        }
    }

    /// [`insert`](Self::insert) with the trace shapes from the memo, for a
    /// whole-board build (a restore).
    pub(crate) fn insert_memoized(&mut self, item: &Item, ctx: &ShapeContext<'_>) {
        for tree in &mut self.trees {
            tree.insert_memoized(item, ctx);
        }
    }

    /// Removes the entries of `id` from all trees.
    pub fn remove(&mut self, id: ItemId) {
        // P02.1's seeded fault: a test build can skip one removal.
        #[cfg(test)]
        for (index, tree) in self.trees.iter_mut().enumerate() {
            if !audit::fault::skips_removal(id, index) {
                tree.remove(id);
            }
        }
        #[cfg(not(test))]
        for tree in &mut self.trees {
            tree.remove(id);
        }
    }

    /// Java `SearchTreeManager.mergeEntriesInFront` (`in_front`) or
    /// `mergeEntriesAtEnd`: the merge on every tree in list order, the
    /// default tree first. Returns the indexes of the trees where a trace
    /// had no entries, which the caller fills by reinserting `merge.to`.
    pub fn merge_entries(
        &mut self,
        merge: &TraceMerge<'_>,
        in_front: bool,
        matrix: &ClearanceMatrix,
    ) -> Vec<usize> {
        let mut missing = Vec::new();
        for (index, tree) in self.trees.iter_mut().enumerate() {
            let merged = if in_front {
                tree.merge_entries_in_front(merge, matrix)
            } else {
                tree.merge_entries_at_end(merge, matrix)
            };
            if !merged {
                missing.push(index);
            }
        }
        missing
    }

    /// Java `SearchTreeManager.changeEntries` on every tree in list order.
    /// Returns the indexes of the trees where the trace had no entries.
    pub fn change_entries(
        &mut self,
        change: &TraceChange<'_>,
        matrix: &ClearanceMatrix,
    ) -> Vec<usize> {
        let mut missing = Vec::new();
        for (index, tree) in self.trees.iter_mut().enumerate() {
            if !tree.change_entries(change, matrix) {
                missing.push(index);
            }
        }
        missing
    }

    /// Java `SearchTreeManager.reuseEntriesAfterCutout` on every tree in
    /// list order (see [`ShapeSearchTree::reuse_entries_after_cutout`]).
    /// Returns the indexes of the trees where `from` had no entries.
    pub fn reuse_entries_after_cutout(
        &mut self,
        from: ItemId,
        start: &Item,
        end: &Item,
        ctx: &ShapeContext<'_>,
    ) -> Vec<usize> {
        let mut missing = Vec::new();
        for (index, tree) in self.trees.iter_mut().enumerate() {
            if !tree.reuse_entries_after_cutout(from, start, end, ctx) {
                missing.push(index);
            }
        }
        missing
    }

    /// The tree compensated for `clearance_class`, if there is one.
    pub(crate) fn tree_by_class_mut(
        &mut self,
        clearance_class: i32,
    ) -> Option<&mut ShapeSearchTree> {
        self.trees
            .iter_mut()
            .find(|t| t.compensated_clearance_class == clearance_class)
    }

    /// The tree compensated for `clearance_class`, if there is one.
    pub fn tree_by_class(&self, clearance_class: i32) -> Option<&ShapeSearchTree> {
        self.trees
            .iter()
            .find(|t| t.compensated_clearance_class == clearance_class)
    }

    /// Removes and inserts `item` in the trees `indexes` only.
    pub(crate) fn reinsert_in(&mut self, indexes: &[usize], item: &Item, ctx: &ShapeContext<'_>) {
        for &index in indexes {
            let tree = &mut self.trees[index];
            tree.remove(item.id());
            tree.insert(item, ctx);
        }
    }

    /// Switches clearance compensation of the default tree on or off and
    /// rebuilds the trees from `items`. Returns the ids of the reinserted
    /// items.
    pub fn set_clearance_compensation_used(
        &mut self,
        value: bool,
        items: &UndoableObjects<ItemId, Item>,
        ctx: &ShapeContext<'_>,
    ) {
        if self.clearance_compensation_used == value {
            return;
        }
        self.clearance_compensation_used = value;
        let class = if value { 1 } else { 0 };
        let mut tree = self.new_tree(TreeKind::Plain, class);
        for (_, item) in items.iter().filter(|(_, i)| i.is_on_the_board()) {
            tree.insert_memoized(item, ctx);
        }
        self.trees = vec![tree];
    }

    /// Drops the trees whose class differs from the default tree and, with
    /// clearance compensation, rebuilds the default tree.
    pub fn clearance_value_changed(
        &mut self,
        items: &UndoableObjects<ItemId, Item>,
        ctx: &ShapeContext<'_>,
    ) {
        let default_class = self.trees[0].compensated_clearance_class;
        self.trees
            .retain(|t| t.compensated_clearance_class == default_class);
        if self.clearance_compensation_used {
            self.rebuild(items, ctx);
        }
    }

    /// Drops the trees compensated for class `class`, unless it is the
    /// default tree's.
    pub fn clearance_class_removed(&mut self, class: i32) {
        if class == self.trees[0].compensated_clearance_class {
            return;
        }
        self.trees
            .retain(|t| t.compensated_clearance_class != class);
    }

    /// The tree compensated for `clearance_class`, created and filled with
    /// the items if there is none. The kind of a new tree follows the trace
    /// angle restriction.
    pub fn get_autoroute_tree(
        &mut self,
        clearance_class: i32,
        items: &UndoableObjects<ItemId, Item>,
        ctx: &ShapeContext<'_>,
    ) -> &ShapeSearchTree {
        if let Some(index) = self
            .trees
            .iter()
            .position(|t| t.compensated_clearance_class == clearance_class)
        {
            return &self.trees[index];
        }
        let kind = match ctx.rules.trace_angle_restriction() {
            AngleRestriction::NinetyDegree => TreeKind::NinetyDegree,
            AngleRestriction::FortyfiveDegree => TreeKind::FortyfiveDegree,
            AngleRestriction::None => TreeKind::Plain,
        };
        let mut tree = self.new_tree(kind, clearance_class);
        if self.config.reversed_builds {
            let all: Vec<&Item> = items.iter().map(|(_, item)| item).collect();
            for item in all.into_iter().rev() {
                tree.insert_memoized(item, ctx);
            }
        } else {
            for (_, item) in items.iter() {
                tree.insert_memoized(item, ctx);
            }
        }
        self.trees.push(tree);
        self.trees.last().expect("tree just pushed")
    }

    /// Keeps only the default tree.
    pub fn reset_compensated_trees(&mut self) {
        self.trees.truncate(1);
    }

    /// Removes and reinserts all items of `items` in all trees, in
    /// item-list order (Java `removeAllBoardItems` then
    /// `insertAllBoardItems`, `SearchTreeManager.java:186-232`).
    ///
    /// Removing every leaf empties a `MinAreaTree`, so a tree without rooms
    /// is rebuilt from scratch, which gives the same shape. A tree holding
    /// rooms keeps them, as in Java: its item leaves are removed one by one.
    pub fn rebuild(&mut self, items: &UndoableObjects<ItemId, Item>, ctx: &ShapeContext<'_>) {
        for tree in &mut self.trees {
            if tree.rooms.is_empty() {
                *tree = tree.emptied();
            } else {
                for (id, _) in items.iter() {
                    tree.remove(*id);
                }
            }
            let on_board = items.iter().filter(|(_, i)| i.is_on_the_board());
            if self.config.reversed_builds {
                let all: Vec<&Item> = on_board.map(|(_, item)| item).collect();
                for item in all.into_iter().rev() {
                    tree.insert_memoized(item, ctx);
                }
            } else {
                for (_, item) in on_board {
                    tree.insert_memoized(item, ctx);
                }
            }
        }
    }
}

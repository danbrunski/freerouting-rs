//! The board facade: the item list, the search trees and the operations on
//! them.

use std::collections::BTreeSet;

use crate::util::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use freerouting_geometry::{
    Area, FloatPoint, IntBox, IntOctagon, IntPoint, Point, PolylineShape, PolylineShapeOps, Shape,
    TileShape, TileShapeOps, Vector,
};

use crate::autoroute::failure_log::FailureLog;
use crate::board::items::{
    AreaKind, BoardOutline, ComponentOutline, DrillData, Item, ItemId, ItemKind, ObstacleArea, Pin,
    PolylineTrace, StopConnectionOption, Via, base_pin_name, point_tile,
};
use crate::board::journal::{Journal, Record};
use crate::board::search::{SearchTreeManager, ShapeContext, ShapeSearchTree, TreeEntry};
use crate::board::snapshot::BoardSnapshot;
use crate::board::state::{ChangedArea, Communication};
use crate::board::structure::{ComponentId, Components, FixedState, LayerStructure};
use crate::datastructures::{IdGenerator, UndoableObjects};
use crate::drc::RoutingLedger;
use crate::library::{BoardLibrary, Packages, PadstackId, Padstacks};
use crate::rules::{BoardRules, Nets};

/// The length (in trace half widths) up to which a trace from a via to an
/// SMD pin counts as a fanout trace.
const PROTECT_FANOUT_LENGTH: f64 = 400.0;

/// The limit of passes of [`BasicBoard::normalize_traces`].
pub(crate) const MAX_NORMALIZE_ITERATIONS: usize = 2000;

/// A board: layers, rules, library, components and the items with their
/// search trees.
///
/// Port of `board.facade.BasicBoard` (together with its helpers
/// `BoardItemRepository`, `BoardConnectivityQueries` and
/// `BoardSnapshotManager`). The methods of `Item` and its subclasses that
/// need the board are methods of this type taking an [`ItemId`]. Observers
/// and the graphics update box are not ported.
///
/// `Clone` is not a Java copy: it keeps the search trees and the transient
/// state. Java's serialization copy is [`BoardSnapshot`] and
/// [`RoutingBoard::deep_copy`](crate::board::RoutingBoard::deep_copy)
/// (see [`crate::board::snapshot`]).
#[derive(Clone, Debug)]
pub struct BasicBoard {
    pub(crate) items: UndoableObjects<ItemId, Item>,
    pub components: Components,
    pub rules: BoardRules,
    pub library: BoardLibrary,
    pub layer_structure: LayerStructure,
    pub communication: Communication,
    pub(crate) bounding_box: IntBox,
    pub(crate) search_trees: SearchTreeManager,
    max_trace_half_width: i32,
    min_trace_half_width: i32,
    revision: u64,
    normalize_suppressed_nets: HashSet<i32>,
    /// The nets of pins on or outside the outline (Java
    /// `BoardOutline.edgePinNets`), computed on demand.
    edge_pin_nets: OnceLock<HashSet<i32>>,
    /// The changed area of `RoutingBoard`; `None` while not marking.
    pub(crate) changed_area: Option<ChangedArea>,
    /// Messages Java writes to its log during board operations.
    pub(crate) log: Vec<String>,
    /// The autoroute change log (`board::autoroute_log`); `None` while off.
    pub(crate) autoroute_log: Option<Vec<crate::board::AutorouteChange>>,
    /// The clearance violations the board had when it was loaded for
    /// routing (Java `preExistingClearanceViolationsCount`); the statistics
    /// count only the violations beyond these as router introduced.
    pub pre_existing_clearance_violations_count: usize,
    /// The incremental incomplete-connection counts (Java `routingLedger`,
    /// transient).
    pub(crate) ledger: RoutingLedger,
    /// The change journal while journaling (B01.7).
    pub(crate) journal: Option<Box<Journal>>,
    /// The keepouts the DSN reader built from a Specctra `wire_keepout`, in
    /// insertion order (Q09.3). They are `AreaKind::Obstacle`, as Java builds
    /// them, until `router.wire_keepout = "traces_only"` makes them
    /// [`AreaKind::TraceObstacle`] ([`Self::make_wire_keepouts_trace_only`]).
    /// Not part of a [`BoardSnapshot`].
    pub(crate) wire_keepouts: Vec<ItemId>,
    /// Where a trace end connects to a pin (N04, `router.pad_contacts`;
    /// [`crate::board::contact_rule`]). A [`BoardSnapshot`] keeps it.
    pub(crate) contact_rule: crate::board::contact_rule::ContactRule,
    /// `router.java_quirks.*` (N06): the switches of the Java bugs that shove,
    /// pull-tight, the DRC, the statistics, the readers and the interactive code
    /// read through the board ([`Self::java_quirks`]). A [`BoardSnapshot`] keeps
    /// them, so deep copies and restores carry them.
    pub(crate) java_quirks: crate::settings::JavaQuirks,
    /// The copper pours the DSN reader left out of the board (K15.11): a pour of
    /// a net with a plane on a power layer, which Java's `Structure` ignores so
    /// that the net's pads route to the plane ([`DroppedPour`]). They are no
    /// items and no obstacles; only the DRC report's KiCad counting reads them
    /// ([`crate::drc::ReportRules::Kicad`]). A [`BoardSnapshot`] keeps them.
    pub(crate) dropped_pours: Arc<Vec<DroppedPour>>,
    /// K15.11: the KiCad rules the DRC report counts with (an unconditional
    /// custom clearance, KiCad's hole clearance, the pads' drilled holes),
    /// set by the `.kicad_pcb` loaders; none for a DSN board. No routing
    /// code reads them. A [`BoardSnapshot`] keeps them.
    pub(crate) kicad_report_rules: Arc<crate::drc::KicadReportRules>,
    /// K15.11: the corners of the DSN's wires that the reader dropped as
    /// collinear (`(layer, x, y)`, board units): where two segments of the
    /// design (KiCad's) meet inside one trace of the board. Only the DRC
    /// report's KiCad track-end rule reads them. A [`BoardSnapshot`] keeps
    /// them.
    pub(crate) wire_joints: Arc<BTreeSet<(i32, i32, i32)>>,
    /// K15.22: the largest bounding radius of the pins' real holes
    /// ([`BasicBoard::set_pad_holes`]), board units; 0 without any (a DSN
    /// board). How far beyond a pin's centre a hole-to-hole query reaches
    /// ([`crate::board::hole_spacing`]). A [`BoardSnapshot`] keeps it.
    pub(crate) pad_hole_reach: f64,
}

/// A copper pour the DSN reader left out of the board (K15.11): the outline of
/// a signal-layer `plane` of a net that also has a plane on a power layer
/// (Java `Structure.insertPlanes`' "Ignoring the copper pour" rule). KiCad
/// still fills it, so a via or a trace end inside it is connected on its
/// layer as KiCad counts connections ([`BasicBoard::dropped_pours`]).
#[derive(Clone, Debug)]
pub struct DroppedPour {
    /// The net of the pour.
    pub net_number: i32,
    /// The board layer of the pour.
    pub layer: i32,
    /// The pour's outline, in board coordinates.
    pub area: freerouting_geometry::Area,
}

impl BasicBoard {
    /// A board with the outline `outline_shapes` and no other items.
    pub fn new(
        bounding_box: IntBox,
        layer_structure: LayerStructure,
        outline_shapes: Vec<PolylineShape>,
        outline_clearance_class: i32,
        rules: BoardRules,
        communication: Communication,
    ) -> Self {
        Self::new_with_java_quirks(
            bounding_box,
            layer_structure,
            outline_shapes,
            outline_clearance_class,
            rules,
            communication,
            crate::settings::JavaQuirks::JAVA,
        )
    }

    /// [`BasicBoard::new`] with the switches of the Java bugs (N06) set before the
    /// outline is inserted, so every item enters the search trees split under the
    /// board's final cutout switches (the reader's path).
    pub fn new_with_java_quirks(
        bounding_box: IntBox,
        layer_structure: LayerStructure,
        outline_shapes: Vec<PolylineShape>,
        outline_clearance_class: i32,
        rules: BoardRules,
        communication: Communication,
        java_quirks: crate::settings::JavaQuirks,
    ) -> Self {
        let layer_count = layer_structure.count();
        let mut board = BasicBoard {
            items: UndoableObjects::new(),
            components: Components::new(),
            rules,
            library: BoardLibrary::new(Padstacks::new(layer_count), Packages::new()),
            layer_structure,
            communication,
            bounding_box,
            search_trees: SearchTreeManager::new(),
            max_trace_half_width: 1000,
            min_trace_half_width: 10000,
            revision: 0,
            normalize_suppressed_nets: HashSet::default(),
            edge_pin_nets: OnceLock::new(),
            changed_area: None,
            log: Vec::new(),
            pre_existing_clearance_violations_count: 0,
            ledger: RoutingLedger::default(),
            journal: None,
            autoroute_log: None,
            wire_keepouts: Vec::new(),
            contact_rule: Default::default(),
            dropped_pours: Arc::default(),
            kicad_report_rules: Arc::default(),
            wire_joints: Arc::default(),
            pad_hole_reach: 0.0,
            java_quirks,
        };
        board.insert_outline(outline_shapes, outline_clearance_class);
        board
    }

    // ------------------------------------------------------------------
    // Basic accessors
    // ------------------------------------------------------------------

    pub fn layer_count(&self) -> usize {
        self.layer_structure.count()
    }

    /// The box surrounding the board geometry.
    pub fn bounding_box(&self) -> IntBox {
        self.bounding_box
    }

    /// How often items were inserted or removed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The largest half width of the traces on the board (at least 1000).
    pub fn max_trace_half_width(&self) -> i32 {
        self.max_trace_half_width
    }

    /// The smallest half width of the traces on the board (at most 10000).
    pub fn min_trace_half_width(&self) -> i32 {
        self.min_trace_half_width
    }

    /// The messages Java writes to its log during board operations.
    pub fn log_messages(&self) -> &[String] {
        &self.log
    }

    /// What the search trees need from the board to calculate item shapes.
    pub fn shape_context(&self) -> ShapeContext<'_> {
        ShapeContext {
            rules: &self.rules,
            padstacks: &self.library.padstacks,
            communication: &self.communication,
            layer_count: self.layer_structure.count(),
            split_quirks: self.java_quirks.split(),
        }
    }

    /// The search trees.
    pub fn search_trees(&self) -> &SearchTreeManager {
        &self.search_trees
    }

    /// The item list with its undo structure (Java `itemList`), read-only;
    /// the search-tree queries take it.
    pub fn item_list(&self) -> &UndoableObjects<ItemId, Item> {
        &self.items
    }

    /// The item with id `id`.
    pub fn item(&self, id: ItemId) -> Option<&Item> {
        self.items.get(&id)
    }

    /// The item with id `id` (Java `getItem(int)`).
    pub fn get_item(&self, id: i32) -> Option<&Item> {
        self.items.get(&ItemId(id))
    }

    /// All items, highest id first.
    pub fn items(&self) -> impl Iterator<Item = &Item> + '_ {
        self.items.iter().map(|(_, item)| item)
    }

    /// The ids of all items, highest id first.
    pub fn item_ids(&self) -> Vec<ItemId> {
        self.items.keys().copied().collect()
    }

    /// The number of items.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// The board outline.
    pub fn outline(&self) -> Option<&Item> {
        self.items().find(|item| item.as_board_outline().is_some())
    }

    fn new_item_id(&mut self) -> ItemId {
        ItemId(self.communication.id_generator.new_id())
    }

    /// Takes one id from the id generator for a temporary item Java
    /// constructs and drops (decision D5: ids are burned where Java burns
    /// them). Returns the id, for logging.
    ///
    /// Java's `Item` constructor takes `idGenerator.newId()` whenever it is
    /// given an id `<= 0` (`Item.java:86-91`). The construction sites at
    /// `cda540f` and the checklist that takes the id there:
    ///
    /// | Java site | Temporary? | Rust owner |
    /// |---|---|---|
    /// | `BasicBoard.insertTraceWithoutCleaning` (`:209`) | rejected zero-length trace | B01.2 (`insert_trace_without_cleaning`) |
    /// | `BasicBoard.checkPolylineTrace` (`:1169`) | yes, every call | B04.4 |
    /// | `ShapeTraceEntries.nextSubstituteTracePiece` (`:276`) | yes, also on check paths | O03 |
    /// | `ShapeTraceEntries.fastCutoutTrace` (`:116,131`) | no: start piece, then end piece | O03 |
    /// | `Item.copy(0)` in every item class (`PolylineTrace.java:63`, `Via.java:71`, `Pin.java:122`, area items) | id taken only for `copy(0)` | B05.2 |
    /// | `BasicBoard.makeConductive` (`:1312`) | no; id before the removal | `make_conductive` |
    #[allow(dead_code)] // B04.4, B05.2 and O03 call it.
    pub(crate) fn burn_item_id(&mut self) -> ItemId {
        self.new_item_id()
    }

    pub(crate) fn item_ref(&self, id: ItemId) -> &Item {
        self.items.get(&id).expect("item on the board")
    }

    // ------------------------------------------------------------------
    // Insertion
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Snapshots (B01.3)
    // ------------------------------------------------------------------

    /// What Java serializes of this board (see [`BoardSnapshot`]).
    pub(crate) fn snapshot(&self) -> BoardSnapshot {
        BoardSnapshot {
            items: self.items.clone(),
            components: Arc::new(self.components.clone()),
            rules: Arc::new(self.rules.clone()),
            library: Arc::new(self.library.clone()),
            layer_structure: Arc::new(self.layer_structure.clone()),
            communication: self.communication.clone(),
            bounding_box: self.bounding_box,
            max_trace_half_width: self.max_trace_half_width,
            min_trace_half_width: self.min_trace_half_width,
            pre_existing_clearance_violations_count: self.pre_existing_clearance_violations_count,
            failure_log: FailureLog::new(),
            memo: Default::default(),
            shape_memo: self.search_trees.shape_memo().clone(),
            search_config: self.search_trees.search_config(),
            contact_rule: self.contact_rule,
            java_quirks: self.java_quirks,
            dropped_pours: self.dropped_pours.clone(),
            kicad_report_rules: self.kicad_report_rules.clone(),
            wire_joints: self.wire_joints.clone(),
            pad_hole_reach: self.pad_hole_reach,
            clearance_compensation_used: self.search_trees.is_clearance_compensation_used(),
        }
    }

    /// A board built from `snapshot` as Java's `readObject` builds one: the
    /// transient state fresh, and a new [`SearchTreeManager`] with only the
    /// default tree, into which every item is inserted in item-list order
    /// (highest id first), each with its shapes in index order.
    pub(crate) fn from_snapshot(snapshot: &BoardSnapshot) -> BasicBoard {
        let mut board = BasicBoard {
            items: snapshot.items.clone(),
            components: (*snapshot.components).clone(),
            rules: (*snapshot.rules).clone(),
            library: (*snapshot.library).clone(),
            layer_structure: (*snapshot.layer_structure).clone(),
            communication: snapshot.communication.clone(),
            bounding_box: snapshot.bounding_box,
            search_trees: SearchTreeManager::with_memo(snapshot.shape_memo.clone()),
            max_trace_half_width: snapshot.max_trace_half_width,
            min_trace_half_width: snapshot.min_trace_half_width,
            revision: 0,
            normalize_suppressed_nets: HashSet::default(),
            edge_pin_nets: OnceLock::new(),
            changed_area: None,
            log: Vec::new(),
            pre_existing_clearance_violations_count: snapshot
                .pre_existing_clearance_violations_count,
            ledger: RoutingLedger::default(),
            journal: None,
            autoroute_log: None,
            wire_keepouts: Vec::new(),
            contact_rule: snapshot.contact_rule,
            java_quirks: snapshot.java_quirks,
            dropped_pours: snapshot.dropped_pours.clone(),
            kicad_report_rules: snapshot.kicad_report_rules.clone(),
            wire_joints: snapshot.wire_joints.clone(),
            pad_hole_reach: snapshot.pad_hole_reach,
        };
        board.search_trees.set_search_config(snapshot.search_config);
        board.insert_all_into_trees();
        // PARITY(quirk, java_quirks.snapshot_drops_compensation): Java readObject
        // discards the GUI compensation mode. Preserve it only for the fixed arm.
        if snapshot.clearance_compensation_used && !snapshot.java_quirks.snapshot_drops_compensation
        {
            board.set_clearance_compensation_used(true);
        }
        board
    }

    /// Inserts every item into the search trees in item-list order (Java
    /// `readObject`'s loop over `getItems()`).
    fn insert_all_into_trees(&mut self) {
        let ctx = ShapeContext {
            rules: &self.rules,
            padstacks: &self.library.padstacks,
            communication: &self.communication,
            layer_count: self.layer_structure.count(),
            split_quirks: self.java_quirks.split(),
        };
        let ids: Vec<ItemId> = self.items.keys().copied().collect();
        for id in &ids {
            if let Some(item) = self.items.get(id) {
                // Every outline in a snapshot was synced to the board's split
                // when it entered the trees (`sync_outline_split`).
                debug_assert!(
                    item.as_board_outline()
                        .is_none_or(|o| o.keepout_split() == ctx.split_quirks),
                    "outline keepout split out of sync with the board's switches"
                );
                self.search_trees.insert_memoized(item, &ctx);
            }
        }
        for id in &ids {
            // Only touch items whose flag changes: the items are shared with
            // the snapshot until they change.
            if self.items.get(id).is_some_and(|item| !item.on_the_board)
                && let Some(item) = self.items.get_mut(id)
            {
                item.on_the_board = true;
            }
        }
    }

    /// Inserts `item` into the item list and the search trees (Java
    /// `insertItem`).
    pub fn insert_item(&mut self, mut item: Item) -> ItemId {
        let class_count = self.rules.clearance_matrix.class_count() as i32;
        if item.clearance_class < 0 || item.clearance_class >= class_count {
            self.log
                .push("LayeredBoard.insert_item: clearanceClass no out of range".into());
            item.clearance_class = 0;
        }
        let id = item.id();
        let invalidates = matches!(item.kind, ItemKind::Pin(_) | ItemKind::BoardOutline(_));
        item.on_the_board = true;
        self.items.insert(id, item);
        self.insert_into_trees(id);
        self.log_autoroute(crate::board::AutorouteChange::Inserted(id));
        self.revision += 1;
        if invalidates {
            self.invalidate_edge_pin_nets();
        }
        self.note_inserted(id);
        id
    }

    // ------------------------------------------------------------------
    // Bookkeeping: the routing ledger (B01.6) and the journal (B01.7)
    // ------------------------------------------------------------------

    /// Records that item `id` was added to the item list.
    fn note_inserted(&mut self, id: ItemId) {
        let Some(item) = self.items.get(&id) else {
            return;
        };
        if item.is_connectable_kind() {
            self.ledger.note_inserted(id, &item.net_numbers);
        }
        if let Some(journal) = &mut self.journal {
            journal.record(id, Record::Inserted, &item.net_numbers, item.bounding_box());
        }
    }

    /// What the bookkeeping needs of an item that leaves the item list.
    fn removal_note(&self, item: &Item) -> RemovalNote {
        RemovalNote {
            id: item.id(),
            nets: item.net_numbers.clone(),
            connectable: item.is_connectable_kind(),
            bounds: if self.journal.is_some() {
                item.bounding_box()
            } else {
                IntBox::EMPTY
            },
        }
    }

    /// Records that an item left the item list.
    fn note_removed(&mut self, note: RemovalNote) {
        if note.connectable {
            self.ledger.note_removed(note.id, &note.nets);
        }
        if let Some(journal) = &mut self.journal {
            journal.record(note.id, Record::Removed, &note.nets, note.bounds);
        }
    }

    /// Records that item `id` changed in place: marks its nets for a
    /// recount and journals it with its current bounding box. Call it
    /// before and after a change that moves the item, so that the dirty
    /// region covers both. O03's fast cutout calls it for its pieces.
    pub(crate) fn note_changed(&mut self, id: ItemId) {
        let Some(item) = self.items.get(&id) else {
            return;
        };
        self.ledger.mark_dirty(&item.net_numbers);
        if let Some(journal) = &mut self.journal {
            journal.record(id, Record::Changed, &item.net_numbers, item.bounding_box());
        }
    }

    /// Records that the nets of item `id` changed from `old_nets`.
    fn note_nets_changed(&mut self, id: ItemId, old_nets: &[i32]) {
        let Some(item) = self.items.get(&id) else {
            return;
        };
        if item.is_connectable_kind() {
            self.ledger.note_removed(id, old_nets);
            self.ledger.note_inserted(id, &item.net_numbers);
        }
        if let Some(journal) = &mut self.journal {
            journal.record(id, Record::Changed, old_nets, item.bounding_box());
            journal.record(id, Record::Changed, &item.net_numbers, IntBox::EMPTY);
        }
    }

    /// The incomplete connections of the board, from the routing ledger
    /// (Java `routingLedger().incompleteCount()`). The first read builds the
    /// ledger; later reads recount only the nets changed since.
    pub fn ledger_incomplete_count(&self) -> usize {
        self.ledger.incomplete_count(self)
    }

    /// Per net the pins and conduction areas minus one, summed, from the
    /// routing ledger (Java `routingLedger().maximumConnections()`).
    pub fn ledger_maximum_connections(&self) -> usize {
        self.ledger.maximum_connections(self)
    }

    /// The nets with incomplete connections, from the routing ledger (Java
    /// `routingLedger().incompleteNetNumbers()`).
    pub fn ledger_incomplete_net_numbers(&self) -> BTreeSet<i32> {
        self.ledger.incomplete_net_numbers(self)
    }

    /// The routing ledger.
    pub fn routing_ledger(&self) -> &RoutingLedger {
        &self.ledger
    }

    /// Inserts the tree shapes of item `id` into the search trees.
    pub(crate) fn insert_into_trees(&mut self, id: ItemId) {
        self.sync_outline_split(id);
        let ctx = ShapeContext {
            rules: &self.rules,
            padstacks: &self.library.padstacks,
            communication: &self.communication,
            layer_count: self.layer_structure.count(),
            split_quirks: self.java_quirks.split(),
        };
        if let Some(item) = self.items.get(&id) {
            self.search_trees.insert(item, &ctx);
        }
        if self.items.get(&id).is_some_and(|item| !item.on_the_board)
            && let Some(item) = self.items.get_mut(&id)
        {
            item.on_the_board = true;
        }
    }

    /// Removes and inserts item `id` in the search trees `indexes` only (the
    /// trees a leaf-reuse operation could not update).
    pub(crate) fn reinsert_in_trees(&mut self, id: ItemId, indexes: &[usize]) {
        if indexes.is_empty() {
            return;
        }
        let ctx = ShapeContext {
            rules: &self.rules,
            padstacks: &self.library.padstacks,
            communication: &self.communication,
            layer_count: self.layer_structure.count(),
            split_quirks: self.java_quirks.split(),
        };
        if let Some(item) = self.items.get(&id) {
            self.search_trees.reinsert_in(indexes, item, &ctx);
        }
    }

    /// Removes the entries of item `id` from every search tree but leaves it
    /// marked as on the board. Test support only: the port of Java's
    /// `PolylineTraceSplitTest.testCombineAtEndRecoversMissingDefaultTreeEntries`
    /// builds this broken state with `searchTreeManager.remove(first);
    /// first.setOnTheBoard(true)`.
    #[doc(hidden)]
    pub fn drop_tree_entries_for_test(&mut self, id: ItemId) {
        self.search_trees.remove(id);
    }

    /// Removes item `id` from the search trees (Java
    /// `SearchTreeManager.remove`).
    pub(crate) fn remove_from_trees(&mut self, id: ItemId) {
        let on_board = self.items.get(&id).is_some_and(|i| i.on_the_board);
        if !on_board {
            return;
        }
        self.search_trees.remove(id);
        if let Some(item) = self.items.get_mut(&id) {
            item.on_the_board = false;
        }
    }

    /// Recomputes the tree entries of item `id` after its geometry changed.
    pub(crate) fn reinsert_into_trees(&mut self, id: ItemId) {
        self.remove_from_trees(id);
        self.insert_into_trees(id);
    }

    /// Inserts a trace without normalizing it. Returns `None` for a trace
    /// with less than 2 corners, or a zero length trace that is not at least
    /// user fixed.
    pub fn insert_trace_without_cleaning(
        &mut self,
        polyline: freerouting_geometry::Polyline,
        layer: i32,
        half_width: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        fixed_state: FixedState,
    ) -> Option<ItemId> {
        if polyline.corner_count() < 2 {
            return None;
        }
        let layer = layer.max(0).min(self.layer_count() as i32 - 1);
        let trace = PolylineTrace {
            polyline,
            layer,
            half_width,
        };
        // Java constructs the trace, taking an id, before it rejects a zero
        // length one.
        let id = self.new_item_id();
        if trace.first_corner() == trace.last_corner() && fixed_state < FixedState::UserFixed {
            return None;
        }
        let item = Item::new(
            id,
            net_numbers.to_vec(),
            clearance_class,
            0,
            fixed_state,
            ItemKind::Trace(trace),
        );
        let nets_normal = item.nets_normal();
        self.insert_item(item);
        if nets_normal {
            self.max_trace_half_width = self.max_trace_half_width.max(half_width);
            self.min_trace_half_width = self.min_trace_half_width.min(half_width);
        }
        Some(id)
    }

    /// Inserts a trace and normalizes it: overlapping traces of the same net
    /// are split and connected traces combined.
    pub fn insert_trace(
        &mut self,
        polyline: freerouting_geometry::Polyline,
        layer: i32,
        half_width: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        fixed_state: FixedState,
    ) {
        let Some(id) = self.insert_trace_without_cleaning(
            polyline,
            layer,
            half_width,
            net_numbers,
            clearance_class,
            fixed_state,
        ) else {
            return;
        };
        let clip_shape = self.changed_area_on_layer(self.item_ref(id).first_layer());
        self.normalize_trace(id, clip_shape.as_ref());
    }

    /// Inserts a trace through `points`.
    pub fn insert_trace_points(
        &mut self,
        points: &[Point],
        layer: i32,
        half_width: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        fixed_state: FixedState,
    ) {
        for point in points {
            if !point.is_contained_in(&self.bounding_box) {
                self.log
                    .push("LayeredBoard.insert_trace: input point out of range".into());
            }
        }
        let polyline = freerouting_geometry::Polyline::from_points(points);
        self.insert_trace(
            polyline,
            layer,
            half_width,
            net_numbers,
            clearance_class,
            fixed_state,
        );
    }

    /// The drill data of a via with padstack `padstack` at `center`.
    pub(crate) fn via_drill_data(&self, padstack_id: PadstackId, center: IntPoint) -> DrillData {
        let padstack = self
            .library
            .padstacks
            .get(padstack_id)
            .expect("via padstack");
        let first_layer = padstack.from_layer();
        let last_layer = padstack.to_layer();
        let shape_count = (last_layer - first_layer + 1).max(0) as usize;
        let translation = Point::Int(center).difference_by(&Point::ZERO);
        let shapes = (0..shape_count)
            .map(|i| {
                padstack
                    .get_shape(i as i32 + first_layer)
                    .map(|s| Shape::from(s.clone()).translate_by(&translation))
            })
            .collect();
        DrillData {
            center,
            padstack: padstack_id,
            first_layer,
            last_layer,
            shapes,
            hole: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_via_item(
        &mut self,
        padstack: PadstackId,
        center: IntPoint,
        net_numbers: &[i32],
        clearance_class: i32,
        fixed_state: FixedState,
        attach_allowed: bool,
        escape_smd_layer: Option<i32>,
    ) -> ItemId {
        let drill = self.via_drill_data(padstack, center);
        let id = self.new_item_id();
        let via = Via {
            attach_allowed,
            is_escape_via: escape_smd_layer.is_some(),
            escape_via_smd_layer: escape_smd_layer.unwrap_or(-1),
            drill,
        };
        let item = Item::new(
            id,
            net_numbers.to_vec(),
            clearance_class,
            0,
            fixed_state,
            ItemKind::Via(via),
        );
        self.insert_item(item)
    }

    /// Inserts a via and splits the traces of its nets at its center.
    /// `attach_allowed` tells whether the via may overlap SMD pins of its
    /// net.
    pub fn insert_via(
        &mut self,
        padstack: PadstackId,
        center: IntPoint,
        net_numbers: &[i32],
        clearance_class: i32,
        fixed_state: FixedState,
        attach_allowed: bool,
    ) -> ItemId {
        let id = self.insert_via_item(
            padstack,
            center,
            net_numbers,
            clearance_class,
            fixed_state,
            attach_allowed,
            None,
        );
        let (from, to) = self.padstack_layer_range(padstack);
        // Java stops before the last layer of the padstack.
        for layer in from..to {
            for net in net_numbers {
                self.split_traces(&Point::Int(center), layer, *net);
            }
        }
        id
    }

    /// Inserts an escape via on top of an SMD pin on `smd_layer` and splits
    /// the traces of its nets at its center on all its layers.
    pub fn insert_escape_via(
        &mut self,
        padstack: PadstackId,
        center: IntPoint,
        net_numbers: &[i32],
        clearance_class: i32,
        fixed_state: FixedState,
        smd_layer: i32,
    ) -> ItemId {
        let id = self.insert_via_item(
            padstack,
            center,
            net_numbers,
            clearance_class,
            fixed_state,
            true,
            Some(smd_layer),
        );
        let (from, to) = self.padstack_layer_range(padstack);
        for layer in from..=to {
            for net in net_numbers {
                self.split_traces(&Point::Int(center), layer, *net);
            }
        }
        id
    }

    fn padstack_layer_range(&self, padstack: PadstackId) -> (i32, i32) {
        let padstack = self.library.padstacks.get(padstack).expect("padstack");
        (padstack.from_layer(), padstack.to_layer())
    }

    /// The drill data of pin `pin_index` of component `component_id`
    /// (Java `Pin.getCenter`, `getShape`, `firstLayer`, `lastLayer`).
    pub(crate) fn pin_drill_data(&self, component_id: i32, pin_index: usize) -> Option<DrillData> {
        let component = self.components.get(ComponentId(component_id))?;
        let package = self.library.packages.get(component.package())?;
        let package_pin = package.get_pin(pin_index)?;
        let padstack = self.library.padstacks.get(package_pin.padstack_id)?;
        let on_front = component.placed_on_front();
        let (from, to) = (padstack.from_layer(), padstack.to_layer());
        let board_layer_count = padstack.board_layer_count() as i32;
        let (first_layer, last_layer) = if on_front || padstack.placed_absolute {
            (from, to)
        } else {
            (board_layer_count - to - 1, board_layer_count - from - 1)
        };
        let transform = component.placement_transform(self.components.flip_style_rotate_first());
        let shape_count = (to - from + 1).max(0) as usize;
        let mut shapes = Vec::with_capacity(shape_count);
        for i in 0..shape_count {
            let padstack_layer = if on_front || padstack.placed_absolute {
                i as i32 + first_layer
            } else {
                board_layer_count - i as i32 - first_layer - 1
            };
            shapes.push(padstack.get_shape(padstack_layer).map(|s| {
                // PARITY(quirk, java_quirks.placed_shape_convex): `true` places the
                // pad as Java does (K15.2).
                transform.shape_with(
                    &Shape::from(s.clone()),
                    package_pin.exact_relative_location(),
                    package_pin.rotation_in_degree,
                    self.java_quirks.placed_shape_convex,
                )
            }));
        }
        let location = component.location()?;
        let rounded = transform
            .point(package_pin.exact_relative_location())
            .round();
        let relative = Point::Int(rounded).difference_by(&Point::Int(location));
        let mut center = Point::Int(location).translate_by(&relative);
        if let Some(shape) = shapes.iter().flatten().next()
            && !shape.contains_inside(&center)
        {
            center = Point::Int(shape.centre_of_gravity().round());
        }
        Some(DrillData {
            center: center.expect_int(),
            padstack: package_pin.padstack_id,
            first_layer,
            last_layer,
            shapes,
            hole: None,
        })
    }

    /// Inserts pin `pin_index` of the package of component `component_id`.
    /// Returns `None` if the component, package pin or padstack is missing.
    pub fn insert_pin(
        &mut self,
        component_id: i32,
        pin_index: usize,
        net_numbers: &[i32],
        clearance_class: i32,
        fixed_state: FixedState,
    ) -> Option<ItemId> {
        let drill = self.pin_drill_data(component_id, pin_index)?;
        let id = self.new_item_id();
        let pin = Pin {
            pin_index,
            drill,
            changed_to: id,
        };
        let item = Item::new(
            id,
            net_numbers.to_vec(),
            clearance_class,
            component_id,
            fixed_state,
            ItemKind::Pin(pin),
        );
        Some(self.insert_item(item))
    }

    /// The absolute area of an area item.
    fn absolute_area(
        &self,
        component_id: i32,
        relative: &Area,
        free: impl FnOnce() -> Area,
    ) -> Area {
        if component_id > 0
            && let Some(component) = self.components.get(ComponentId(component_id))
            && component.is_placed()
        {
            // PARITY(quirk, java_quirks.placed_shape_convex): `true` places the
            // component's area as Java does (K15.2).
            return component
                .placement_transform(self.components.flip_style_rotate_first())
                .area_with(relative, self.java_quirks.placed_shape_convex);
        }
        free()
    }

    /// Inserts an area item of class `kind`.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_area(
        &mut self,
        kind: AreaKind,
        area: Area,
        layer: i32,
        translation: Vector,
        rotation_in_degree: f64,
        side_changed: bool,
        net_numbers: &[i32],
        clearance_class: i32,
        component_id: i32,
        name: Option<String>,
        fixed_state: FixedState,
    ) -> ItemId {
        let mut obstacle = ObstacleArea::new(
            kind,
            area,
            layer,
            translation,
            rotation_in_degree,
            side_changed,
            name,
        );
        let rotate_first = self.components.flip_style_rotate_first();
        let absolute = self.absolute_area(component_id, obstacle.relative_area(), || {
            let mut free = obstacle.clone();
            free.update_area(rotate_first);
            free.area().clone()
        });
        obstacle.set_area(absolute);
        let id = self.new_item_id();
        let item = Item::new(
            id,
            net_numbers.to_vec(),
            clearance_class,
            component_id,
            fixed_state,
            ItemKind::Area(obstacle),
        );
        self.insert_item(item)
    }

    /// Inserts a keepout (Java `insertObstacle(Area, int, int, FixedState)`).
    pub fn insert_obstacle(
        &mut self,
        area: Area,
        layer: i32,
        clearance_class: i32,
        fixed_state: FixedState,
    ) -> ItemId {
        self.insert_area(
            AreaKind::Obstacle,
            area,
            layer,
            Vector::ZERO,
            0.0,
            false,
            &[],
            clearance_class,
            0,
            None,
            fixed_state,
        )
    }

    /// Inserts a via keepout.
    pub fn insert_via_obstacle(
        &mut self,
        area: Area,
        layer: i32,
        clearance_class: i32,
        fixed_state: FixedState,
    ) -> ItemId {
        self.insert_area(
            AreaKind::ViaObstacle,
            area,
            layer,
            Vector::ZERO,
            0.0,
            false,
            &[],
            clearance_class,
            0,
            None,
            fixed_state,
        )
    }

    /// Inserts a placement keepout.
    pub fn insert_component_obstacle(
        &mut self,
        area: Area,
        layer: i32,
        clearance_class: i32,
        fixed_state: FixedState,
    ) -> ItemId {
        self.insert_area(
            AreaKind::ComponentObstacle,
            area,
            layer,
            Vector::ZERO,
            0.0,
            false,
            &[],
            clearance_class,
            0,
            None,
            fixed_state,
        )
    }

    /// Inserts a conduction area (copper of the nets `net_numbers`). Traces
    /// and vias of other nets may cross it unless `is_obstacle`.
    pub fn insert_conduction_area(
        &mut self,
        area: Area,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        is_obstacle: bool,
        fixed_state: FixedState,
    ) -> ItemId {
        self.insert_area(
            AreaKind::Conduction {
                is_obstacle,
                is_filled: true,
            },
            area,
            layer,
            Vector::ZERO,
            0.0,
            false,
            net_numbers,
            clearance_class,
            0,
            None,
            fixed_state,
        )
    }

    /// Inserts the copper shape of a net (Q22.4: a wire with `(attr
    /// obstacle)`): a conduction area of `net_numbers` that is an obstacle to
    /// the traces and vias of other nets and stays one whatever
    /// `router.plane_as_obstacle` says
    /// ([`RoutingBoard::change_plane_as_obstacle`](crate::board::RoutingBoard::change_plane_as_obstacle)
    /// skips it). Not Java.
    pub fn insert_fixed_copper_area(
        &mut self,
        area: Area,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        fixed_state: FixedState,
    ) -> ItemId {
        let id = self.insert_conduction_area(
            area,
            layer,
            net_numbers,
            clearance_class,
            true,
            fixed_state,
        );
        if let Some(ItemKind::Area(area)) = self.items.get_mut(&id).map(|i| &mut i.kind) {
            area.set_fixed_obstacle(true);
        }
        id
    }

    /// Inserts an outline of component `component_id`. Returns `None` for
    /// an unbounded area.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_component_outline(
        &mut self,
        area: Area,
        is_front: bool,
        translation: Vector,
        rotation_in_degree: f64,
        component_id: i32,
        is_courtyard: bool,
        is_fabrication: bool,
        is_closed: bool,
        fixed_state: FixedState,
    ) -> Option<ItemId> {
        if !area.is_bounded() {
            self.log
                .push("BasicBoard.insert_component_outline: area is not bounded".into());
            return None;
        }
        let mut outline = ComponentOutline::new(
            area,
            is_front,
            translation,
            rotation_in_degree,
            is_courtyard,
            is_fabrication,
            is_closed,
            self.layer_count(),
        );
        let rotate_first = self.components.flip_style_rotate_first();
        let absolute = self.absolute_area(component_id, outline.relative_area(), || {
            let mut free = outline.clone();
            free.update_area(rotate_first);
            free.area().clone()
        });
        outline.set_area(absolute);
        let id = self.new_item_id();
        let item = Item::new(
            id,
            Vec::new(),
            0,
            component_id,
            fixed_state,
            ItemKind::ComponentOutline(outline),
        );
        Some(self.insert_item(item))
    }

    /// Inserts the board outline.
    pub fn insert_outline(&mut self, shapes: Vec<PolylineShape>, clearance_class: i32) -> ItemId {
        let outline = BoardOutline::new(shapes, self.layer_count());
        let id = self.new_item_id();
        let item = Item::new(
            id,
            Vec::new(),
            clearance_class,
            0,
            FixedState::SystemFixed,
            ItemKind::BoardOutline(outline),
        );
        self.insert_item(item)
    }

    /// Turns the area outside the outline into a keepout, or back.
    pub fn generate_keepout_outside_outline(&mut self, value: bool) {
        let Some(id) = self.outline().map(Item::id) else {
            return;
        };
        let generated = self
            .item_ref(id)
            .as_board_outline()
            .is_some_and(BoardOutline::keepout_outside_outline_generated);
        if generated == value {
            return;
        }
        let bounding_box = self.bounding_box;
        self.note_changed(id);
        self.remove_from_trees(id);
        if let Some(ItemKind::BoardOutline(outline)) = self.items.get_mut(&id).map(|i| &mut i.kind)
        {
            outline.set_keepout_outside(value, bounding_box, self.java_quirks.split());
        }
        self.insert_into_trees(id);
        self.note_changed(id);
    }

    /// Enlarges the bounding box, if necessary, to contain all items, with a
    /// margin of 1000.
    ///
    /// DIVERGENCE (K15.1, both profiles): an item whose box is not bounded
    /// ([`IntBox::has_infinite_coordinate`]) is left out, and the returned
    /// warnings name each such item, in item order
    /// (`board::unbounded_items`). Java takes the union, its
    /// `offset(1000)` wraps and the board box is empty.
    pub fn expand_bounding_box_to_include_all_items(&mut self) -> Vec<String> {
        let mut bounds = self.bounding_box;
        let mut changed = false;
        let mut warnings = Vec::new();
        for item in self.items() {
            let item_box = item.bounding_box();
            if item_box.is_empty() {
                continue;
            }
            if item_box.has_infinite_coordinate() {
                warnings.push(crate::board::unbounded_items::warning(self, item));
                continue;
            }
            if !bounds.contains_regular(&item_box.into()) {
                bounds = bounds.union_box(&item_box);
                changed = true;
            }
        }
        if changed {
            self.bounding_box = bounds.offset(1000.0);
            self.invalidate_edge_pin_nets();
        }
        warnings
    }

    /// Turns the keepout `area_id` into a conduction area of net
    /// `net_number` (Java `makeConductive`).
    pub fn make_conductive(&mut self, area_id: ItemId, net_number: i32) -> Option<ItemId> {
        let item = self.items.get(&area_id)?.clone();
        let area = item.as_area()?.clone();
        let id = self.new_item_id();
        let mut conduction = ObstacleArea::new(
            AreaKind::Conduction {
                is_obstacle: true,
                is_filled: true,
            },
            area.relative_area().clone(),
            area.layer(),
            area.translation().clone(),
            area.rotation_in_degree(),
            area.side_changed(),
            area.name.clone(),
        );
        conduction.set_area(area.area().clone());
        let new_item = Item::new(
            id,
            vec![net_number],
            item.clearance_class(),
            item.component_id(),
            item.fixed_state(),
            ItemKind::Area(conduction),
        );
        self.remove_item(area_id);
        Some(self.insert_item(new_item))
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    /// Java `Item.isDeletionForbidden`: items of components, user fixed
    /// items and power planes.
    pub fn is_deletion_forbidden(&self, item: &Item) -> bool {
        if item.component_id() > 0 || item.is_user_fixed() {
            return true;
        }
        if let Some(area) = item.as_conduction_area() {
            return !self.layer_structure.layers[area.layer() as usize].is_signal;
        }
        false
    }

    /// Removes item `id` unless its deletion is forbidden. Returns true if
    /// it was removed.
    pub fn remove_item(&mut self, id: ItemId) -> bool {
        self.remove_item_checked(id, false)
    }

    /// Java `BoardItemRepository.removeItem(item, forMove)`: a move removes
    /// an item of a component although its deletion is forbidden
    /// (B08.5's `detach_item_for_move`).
    pub(crate) fn remove_item_checked(&mut self, id: ItemId, for_move: bool) -> bool {
        let Some(item) = self.items.get(&id) else {
            return false;
        };
        if self.is_deletion_forbidden(item) && !(for_move && item.component_id() > 0) {
            return false;
        }
        let invalidates = matches!(item.kind, ItemKind::Pin(_) | ItemKind::BoardOutline(_));
        self.log_autoroute(crate::board::AutorouteChange::Removed(id));
        self.remove_from_trees(id);
        let note = self.items.get(&id).map(|item| self.removal_note(item));
        self.items.delete(&id);
        self.revision += 1;
        if invalidates {
            self.invalidate_edge_pin_nets();
        }
        if let Some(note) = note {
            self.note_removed(note);
        }
        true
    }

    /// Removes the items `ids`. Returns false if some could not be removed
    /// because they are fixed.
    pub fn remove_items(&mut self, ids: impl IntoIterator<Item = ItemId>) -> bool {
        let mut result = true;
        for id in ids {
            let Some(item) = self.items.get(&id) else {
                continue;
            };
            if self.is_deletion_forbidden(item) || item.is_user_fixed() {
                result = false;
            } else {
                self.remove_item(id);
            }
        }
        result
    }

    /// Removes all traces and vias, without undo (Java
    /// `deleteAllTracksAndVias`).
    pub fn delete_all_traces_and_vias(&mut self) {
        let ids: Vec<ItemId> = self
            .items()
            .filter(|i| i.is_trace() || i.is_via())
            .map(Item::id)
            .collect();
        for id in ids {
            self.search_trees.remove(id);
            if let (Some(journal), Some(item)) = (&mut self.journal, self.items.get(&id)) {
                journal.record(id, Record::Removed, &item.net_numbers, item.bounding_box());
            }
            self.items.delete(&id);
        }
        self.ledger.invalidate();
    }

    // ------------------------------------------------------------------
    // Item lists
    // ------------------------------------------------------------------

    /// The connectable items of net `net_number`, highest id first.
    pub fn connectable_items(&self, net_number: i32) -> Vec<ItemId> {
        self.items()
            .filter(|i| i.is_connectable_kind() && i.contains_net(net_number))
            .map(Item::id)
            .collect()
    }

    pub fn connectable_item_count(&self, net_number: i32) -> usize {
        self.items()
            .filter(|i| i.is_connectable_kind() && i.contains_net(net_number))
            .count()
    }

    /// The items of component `component_id`.
    pub fn component_items(&self, component_id: i32) -> Vec<ItemId> {
        self.items()
            .filter(|i| i.component_id() == component_id)
            .map(Item::id)
            .collect()
    }

    /// The pins of component `component_id`.
    pub fn component_pins(&self, component_id: i32) -> Vec<ItemId> {
        self.items()
            .filter(|i| i.component_id() == component_id && i.is_pin())
            .map(Item::id)
            .collect()
    }

    /// The pin `pin_index` of component `component_id`.
    pub fn get_pin(&self, component_id: i32, pin_index: usize) -> Option<ItemId> {
        self.items()
            .find(|i| {
                i.component_id() == component_id
                    && i.as_pin().is_some_and(|p| p.pin_index == pin_index)
            })
            .map(Item::id)
    }

    pub fn conduction_areas(&self) -> Vec<ItemId> {
        self.ids_where(Item::is_conduction_area)
    }

    pub fn pins(&self) -> Vec<ItemId> {
        self.ids_where(Item::is_pin)
    }

    /// The pins on a single layer.
    pub fn smd_pins(&self) -> Vec<ItemId> {
        self.ids_where(|i| i.is_pin() && i.first_layer() == i.last_layer())
    }

    pub fn vias(&self) -> Vec<ItemId> {
        self.ids_where(Item::is_via)
    }

    pub fn traces(&self) -> Vec<ItemId> {
        self.ids_where(Item::is_trace)
    }

    fn ids_where(&self, filter: impl Fn(&Item) -> bool) -> Vec<ItemId> {
        self.items().filter(|i| filter(i)).map(Item::id).collect()
    }

    /// The summed length of all traces.
    pub fn cumulative_trace_length(&self) -> f64 {
        self.items()
            .filter_map(Item::as_trace)
            .map(PolylineTrace::length)
            .sum()
    }

    /// The conduction areas among `items` on power layers (Java
    /// `powerPlaneAreas`).
    pub fn power_plane_areas(&self, items: impl IntoIterator<Item = ItemId>) -> BTreeSet<ItemId> {
        items
            .into_iter()
            .filter(|id| {
                self.items
                    .get(id)
                    .and_then(Item::as_conduction_area)
                    .is_some_and(|area| {
                        !self.layer_structure.layers[area.layer() as usize].is_signal
                    })
            })
            .collect()
    }

    /// The items of net `net_number` that are not routed: pins, fixed
    /// traces and vias, conduction areas (Java `Net.getTerminalItems`).
    pub fn net_terminal_items(&self, net_number: i32) -> Vec<ItemId> {
        self.ids_where(|i| {
            i.is_connectable_kind() && i.contains_net(net_number) && !i.is_routable()
        })
    }

    /// The pins of net `net_number`.
    pub fn net_pins(&self, net_number: i32) -> Vec<ItemId> {
        self.ids_where(|i| i.is_pin() && i.contains_net(net_number))
    }

    /// The summed length of the traces of net `net_number`.
    pub fn net_trace_length(&self, net_number: i32) -> f64 {
        self.items()
            .filter(|i| i.contains_net(net_number))
            .filter_map(Item::as_trace)
            .map(PolylineTrace::length)
            .sum()
    }

    /// The number of vias of net `net_number`.
    pub fn net_via_count(&self, net_number: i32) -> usize {
        self.items()
            .filter(|i| i.is_via() && i.contains_net(net_number))
            .count()
    }

    /// The box around the items `ids`.
    pub fn bounding_box_of(&self, ids: &[ItemId]) -> IntBox {
        let mut result = IntBox::EMPTY;
        for id in ids {
            if let Some(item) = self.items.get(id) {
                result = result.union_box(&item.bounding_box());
            }
        }
        result
    }

    // ------------------------------------------------------------------
    // Shapes and overlap queries
    // ------------------------------------------------------------------

    /// The number of tile shapes of `item` (Java `tileShapeCount`).
    pub fn tile_shape_count(&self, item: &Item) -> usize {
        match &item.kind {
            ItemKind::Pin(pin) => pin.drill.shapes.len(),
            ItemKind::Via(via) => via.drill.shapes.len(),
            ItemKind::Trace(trace) => trace.tile_shape_count(),
            ItemKind::ComponentOutline(_) => 0,
            ItemKind::Area(_) | ItemKind::BoardOutline(_) => self
                .search_trees
                .default_tree()
                .tree_shape_count(item, &self.shape_context()),
        }
    }

    /// The tile shape `index` of `item` in the default tree (Java
    /// `getTileShape`).
    pub fn tile_shape(&self, item: &Item, index: usize) -> Option<TileShape> {
        self.search_trees
            .default_tree()
            .tree_shape(item, index, &self.shape_context())
    }

    /// The tree entries overlapping `shape` on `layer` (all layers if
    /// negative) in the default tree.
    pub fn overlapping_tree_entries(&self, shape: &TileShape, layer: i32) -> Vec<TreeEntry> {
        let mut result = Vec::new();
        self.search_trees.default_tree().overlapping_tree_entries(
            shape,
            layer,
            &[],
            &self.items,
            &mut result,
        );
        result
    }

    /// The items overlapping `shape` on `layer` (all layers if negative).
    pub fn overlapping_objects(&self, shape: &TileShape, layer: i32) -> BTreeSet<ItemId> {
        self.overlapping_tree_entries(shape, layer)
            .into_iter()
            .map(|e| e.item)
            .collect()
    }

    /// The items that come closer to `shape` on `layer` than clearance
    /// class `clearance_class` allows.
    pub fn overlapping_items_with_clearance(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
    ) -> BTreeSet<ItemId> {
        // Java goes through the five-argument form, which searches with the
        // clearance also in a compensated tree.
        self.search_trees
            .default_tree()
            .clearance_search_entries(
                shape,
                layer,
                ignore_nets,
                clearance_class,
                &self.rules.clearance_matrix,
                &self.items,
            )
            .into_iter()
            .map(|e| e.item)
            .collect()
    }

    /// [`overlapping_items_with_clearance`](Self::overlapping_items_with_clearance)
    /// without touching the work meter, for counters that only observe (N06.15).
    pub fn overlapping_items_with_clearance_unmetered(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
    ) -> BTreeSet<ItemId> {
        self.search_trees
            .default_tree()
            .clearance_search_entries_unmetered(
                shape,
                layer,
                ignore_nets,
                clearance_class,
                &self.rules.clearance_matrix,
                &self.items,
            )
            .into_iter()
            .map(|e| e.item)
            .collect()
    }

    /// The items overlapping `area` on `layer`.
    pub fn overlapping_items(&self, area: &Area, layer: i32) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        // PARITY(quirk, java_quirks.octagon_box_cutout_branch3): the board's split
        // switches (all four); Java's split when they are `true`.
        let pieces = area.split_to_convex_with(None, self.java_quirks.split());
        for tile in pieces.unwrap_or_default() {
            result.extend(self.overlapping_objects(&tile, layer));
        }
        result
    }

    /// The items whose shape on `layer` contains `location`.
    pub fn pick_items(&self, location: &Point, layer: i32) -> BTreeSet<ItemId> {
        self.overlapping_objects(&point_tile(location), layer)
    }

    /// True if `point` is inside the bounding box of the board.
    pub fn contains(&self, point: &Point) -> bool {
        point.is_contained_in(&self.bounding_box)
    }

    /// The clearance between classes `class1` and `class2` on `layer`,
    /// with the safety margin.
    pub fn clearance_value(&self, class1: i32, class2: i32, layer: i32) -> i32 {
        self.rules
            .clearance_matrix
            .get_value(class1, class2, layer, true)
    }

    // ------------------------------------------------------------------
    // Item properties that need the board
    // ------------------------------------------------------------------

    /// The name of the package pin of pin `item`.
    pub fn pin_name(&self, item: &Item) -> Option<&str> {
        let pin = item.as_pin()?;
        let component = self.components.get(ComponentId(item.component_id()))?;
        let package = self.library.packages.get(component.package())?;
        Some(package.get_pin(pin.pin_index)?.name.as_str())
    }

    /// Java `Pin.drillAllowed`: the pin is on a single layer.
    fn pin_drill_allowed(item: &Item) -> bool {
        item.first_layer() == item.last_layer()
    }

    /// Java `Pin.isSameLogicalPad`.
    fn is_same_logical_pad(&self, pin1: &Item, pin2: &Item) -> bool {
        let (Some(p1), Some(p2)) = (pin1.as_pin(), pin2.as_pin()) else {
            return false;
        };
        let Some(component) = self.components.get(ComponentId(pin1.component_id())) else {
            return false;
        };
        let Some(package) = self.library.packages.get(component.package()) else {
            return false;
        };
        if p1.pin_index >= package.pin_count() || p2.pin_index >= package.pin_count() {
            return false;
        }
        let name1 = &package.get_pin(p1.pin_index).expect("pin").name;
        let name2 = &package.get_pin(p2.pin_index).expect("pin").name;
        let base1 = base_pin_name(name1);
        let base2 = base_pin_name(name2);
        !base1.is_empty() && base1 == base2
    }

    /// The copper pours the DSN reader left out of the board (K15.11,
    /// [`DroppedPour`]), in reading order.
    pub fn dropped_pours(&self) -> &[DroppedPour] {
        &self.dropped_pours
    }

    /// Records a pour the reader leaves out of the board (K15.11).
    pub(crate) fn record_dropped_pour(&mut self, pour: DroppedPour) {
        Arc::make_mut(&mut self.dropped_pours).push(pour);
    }

    /// The KiCad rules the DRC report counts with (K15.11,
    /// [`crate::drc::KicadReportRules`]).
    pub fn kicad_report_rules(&self) -> &crate::drc::KicadReportRules {
        &self.kicad_report_rules
    }

    /// Sets the KiCad rules the DRC report counts with (K15.11); no routing
    /// code reads them.
    pub fn set_kicad_report_rules(&mut self, rules: crate::drc::KicadReportRules) {
        self.kicad_report_rules = Arc::new(rules);
    }

    /// The DSN wire corners the reader dropped as collinear (K15.11), as
    /// `(layer, x, y)` in board units.
    pub fn wire_joints(&self) -> &BTreeSet<(i32, i32, i32)> {
        &self.wire_joints
    }

    /// Records the corners of a DSN wire on `layer` that its trace dropped
    /// (K15.11).
    pub(crate) fn record_wire_joints(&mut self, layer: i32, corners: &[IntPoint]) {
        if corners.is_empty() {
            return;
        }
        let joints = Arc::make_mut(&mut self.wire_joints);
        joints.extend(corners.iter().map(|c| (layer, c.x, c.y)));
    }

    /// The keepouts built from a Specctra `wire_keepout` (Q09.3), in
    /// insertion order.
    pub fn wire_keepouts(&self) -> &[ItemId] {
        &self.wire_keepouts
    }

    /// Q09.3: makes every keepout built from a `wire_keepout` that is still
    /// an [`AreaKind::Obstacle`] a [`AreaKind::TraceObstacle`] (traces of
    /// every net are kept out, vias pass). The search trees hold the same
    /// shapes for both kinds, so nothing is reinserted. Returns the number
    /// of areas changed.
    pub fn make_wire_keepouts_trace_only(&mut self) -> usize {
        let mut changed = 0;
        for id in self.wire_keepouts.clone() {
            let Some(ItemKind::Area(area)) = self.items.get_mut(&id).map(|i| &mut i.kind) else {
                continue;
            };
            if area.kind != AreaKind::Obstacle {
                continue;
            }
            area.set_kind(AreaKind::TraceObstacle);
            self.note_changed(id);
            changed += 1;
        }
        changed
    }

    /// Java `Item.isObstacle(Item)`: true if `other` may not overlap
    /// `item`.
    pub fn is_obstacle(&self, item: &Item, other: &Item) -> bool {
        match &item.kind {
            ItemKind::Pin(_) => {
                if other.id() == item.id() {
                    return false;
                }
                if let Some(area) = other.as_area() {
                    if item.shares_net(other) {
                        return false;
                    }
                    if area.is_conduction_area() && !area.conduction_is_obstacle() {
                        return false;
                    }
                    // Q22.16: a pad keepout is no copper.
                    return other.net_count() > 0 && !area.is_pad_keepout();
                }
                if !other.shares_net(item) {
                    if other.is_pin()
                        && item.component_id() > 0
                        && item.component_id() == other.component_id()
                        && item.net_count() == 0
                        && other.net_count() == 0
                        && self.is_same_logical_pad(item, other)
                    {
                        return false;
                    }
                    return true;
                }
                if other.is_trace() || other.is_pin() {
                    return false;
                }
                !Self::pin_drill_allowed(item) || !other.is_via()
            }
            ItemKind::Via(via) => {
                if other.id() == item.id() {
                    return false;
                }
                if let Some(area) = other.as_area() {
                    // A trace-only keepout (Q09) lets vias through.
                    if matches!(
                        area.kind,
                        AreaKind::ComponentObstacle | AreaKind::TraceObstacle
                    ) {
                        return false;
                    }
                    if area.is_conduction_area()
                        && !area.conduction_is_obstacle()
                        && !self.area_policy_blocks_item(other, item)
                    {
                        return false;
                    }
                }
                if !other.shares_net(item) {
                    return true;
                }
                if other.is_trace() {
                    return false;
                }
                !via.attach_allowed || !other.is_pin() || !Self::pin_drill_allowed(other)
            }
            ItemKind::Trace(_) => {
                if other.id() == item.id() {
                    return false;
                }
                if let Some(area) = other.as_area() {
                    match area.kind {
                        AreaKind::ViaObstacle | AreaKind::ComponentObstacle => return false,
                        AreaKind::Conduction {
                            is_obstacle: false, ..
                        } if !self.area_policy_blocks_item(other, item) => return false,
                        _ => {}
                    }
                }
                !other.shares_net(item)
            }
            ItemKind::Area(area) => match area.kind {
                // Q22.16: no copper, so traces and vias of other nets only.
                AreaKind::Obstacle if area.is_pad_keepout() => {
                    !other.shares_net(item) && (other.is_trace() || other.is_via())
                }
                AreaKind::Obstacle => Self::obstacle_area_is_obstacle(item, other),
                AreaKind::Conduction { is_obstacle, .. } => {
                    (is_obstacle || self.area_policy_blocks_item(item, other))
                        && Self::obstacle_area_is_obstacle(item, other)
                }
                AreaKind::ViaObstacle => !other.shares_net(item) && other.is_via(),
                // Q09: traces of every net, never vias.
                AreaKind::TraceObstacle => !other.shares_net(item) && other.is_trace(),
                AreaKind::ComponentObstacle => {
                    other.id() != item.id()
                        && other
                            .as_area()
                            .is_some_and(|a| a.kind == AreaKind::ComponentObstacle)
                        && other.component_id() != item.component_id()
                }
            },
            ItemKind::ComponentOutline(_) => false,
            ItemKind::BoardOutline(_) => {
                if matches!(other.kind, ItemKind::BoardOutline(_) | ItemKind::Area(_)) {
                    return false;
                }
                if other.is_trace() && !self.outline_blocks_nets(other.net_numbers()) {
                    return false;
                }
                true
            }
        }
    }

    /// `ObstacleArea.isObstacle`.
    fn obstacle_area_is_obstacle(item: &Item, other: &Item) -> bool {
        if other.shares_net(item) {
            return false;
        }
        if other.is_pin() {
            return item.net_count() > 0;
        }
        if other.as_area().is_some() {
            return item.net_count() > 0 && other.net_count() > 0;
        }
        other.is_trace() || other.is_via()
    }

    /// Java `isTraceObstacle(int)`: true if `item` is an obstacle for traces
    /// of net `net_number`.
    pub fn is_trace_obstacle(&self, item: &Item, net_number: i32) -> bool {
        match &item.kind {
            ItemKind::Area(area) => match area.kind {
                AreaKind::ViaObstacle | AreaKind::ComponentObstacle => false,
                AreaKind::Conduction { is_obstacle, .. } => {
                    (is_obstacle || self.area_policy_blocks_net(item, net_number))
                        && !item.contains_net(net_number)
                }
                AreaKind::Obstacle | AreaKind::TraceObstacle => !item.contains_net(net_number),
            },
            ItemKind::BoardOutline(_) => {
                // PARITY(quirk, java_quirks.edge_pin_net_crosses_outline): `true`
                // lets the traces of an edge-pin net through the outline, as Java
                // does; `false` blocks every trace (K15.19).
                !(self.java_quirks.edge_pin_net_crosses_outline
                    && net_number > 0
                    && self.edge_pin_nets().contains(&net_number))
            }
            _ => !item.contains_net(net_number),
        }
    }

    /// Java `BoardOutline.blocksNets`.
    pub fn outline_blocks_nets(&self, net_numbers: &[i32]) -> bool {
        // PARITY(quirk, java_quirks.edge_pin_net_crosses_outline): `true` exempts
        // the traces of edge-pin nets, as Java does; `false` blocks every trace
        // (K15.19).
        if net_numbers.is_empty() || !self.java_quirks.edge_pin_net_crosses_outline {
            return true;
        }
        net_numbers
            .iter()
            .any(|n| !(*n > 0 && self.edge_pin_nets().contains(n)))
    }

    /// The nets of pins whose center or a pad corner is not inside the
    /// outline.
    pub fn edge_pin_nets(&self) -> &HashSet<i32> {
        self.edge_pin_nets.get_or_init(|| {
            let mut result = HashSet::default();
            let Some(outline) = self.outline().and_then(Item::as_board_outline) else {
                return result;
            };
            for item in self.items().filter(|i| i.is_pin()) {
                let drill = item.drill().expect("pin");
                let mut is_edge_or_outside = !outline.contains(&Point::Int(drill.center));
                if !is_edge_or_outside {
                    'layers: for shape in drill.shapes.iter().flatten() {
                        if let Shape::Polyline(PolylineShape::Tile(tile)) = shape {
                            for c in 0..tile.border_line_count() {
                                if !outline.contains(&tile.corner(c)) {
                                    is_edge_or_outside = true;
                                    break 'layers;
                                }
                            }
                        }
                    }
                }
                if is_edge_or_outside {
                    result.extend(item.net_numbers().iter().copied());
                }
            }
            result
        })
    }

    pub(crate) fn invalidate_edge_pin_nets(&mut self) {
        self.edge_pin_nets.take();
    }

    /// Java `Item.isShoveFixed`, with the trace override: traces of shove
    /// fixed net classes are shove fixed.
    pub fn is_shove_fixed(&self, item: &Item) -> bool {
        if item.fixed_state() >= FixedState::ShoveFixed {
            return true;
        }
        if !item.is_trace() {
            return false;
        }
        item.net_numbers().iter().any(|n| {
            Nets::is_normal_net_number(*n)
                && self
                    .rules
                    .net_class_of(*n)
                    .is_some_and(|class| class.is_shove_fixed())
        })
    }

    /// The net class flag `ignore_cycles_with_areas` of the first net of
    /// `item`.
    pub(crate) fn ignore_cycles_with_areas(&self, item: &Item) -> bool {
        item.net_numbers()
            .first()
            .and_then(|n| self.rules.net_class_of(*n))
            .is_some_and(|class| class.ignore_cycles_with_areas())
    }

    /// Java `Item.hasIgnoredNets`.
    pub fn has_ignored_nets(&self, item: &Item) -> bool {
        item.net_numbers().iter().any(|n| {
            self.rules
                .net_class_of(*n)
                .is_some_and(|class| class.is_ignored_by_autorouter)
        })
    }

    // ------------------------------------------------------------------
    // Contacts and connectivity
    // ------------------------------------------------------------------

    /// The items in normal contact with `id`: for drill items and trace
    /// ends the items of a common net at the same point, for conduction
    /// areas the trace ends and drill centers inside it.
    pub fn normal_contacts(&self, id: ItemId) -> BTreeSet<ItemId> {
        let Some(item) = self.items.get(&id) else {
            return BTreeSet::new();
        };
        match &item.kind {
            ItemKind::Pin(_) | ItemKind::Via(_) => self.drill_normal_contacts(item),
            ItemKind::Trace(trace) => {
                let mut result = self.trace_normal_contacts(id, &trace.first_corner(), false);
                result.extend(self.trace_normal_contacts(id, &trace.last_corner(), false));
                // N04.4: under `inside_pad` the traces ending on this one (a
                // T junction), so that both traces see the contact.
                self.add_trace_body_contacts(item, &mut result);
                result
            }
            ItemKind::Area(area) if area.is_conduction_area() => {
                self.conduction_area_normal_contacts(item)
            }
            _ => BTreeSet::new(),
        }
    }

    fn drill_normal_contacts(&self, item: &Item) -> BTreeSet<ItemId> {
        let drill = item.drill().expect("drill item");
        let center = Point::Int(drill.center);
        let mut result = BTreeSet::new();
        for other_id in self.overlapping_objects(&point_tile(&center), -1) {
            let other = self.item_ref(other_id);
            if other_id == item.id() || !other.shares_net(item) || !other.shares_layer(item) {
                continue;
            }
            if contact_at_point(other, &center) {
                result.insert(other_id);
            }
        }
        // N04: under `inside_pad` the traces ending inside a pin's pad.
        self.add_pad_trace_contacts(item, &mut result);
        result
    }

    /// The items in normal contact with trace `id` at `point`, which must
    /// be an end corner (Java `Trace.getNormalContacts(Point, boolean)`).
    pub fn trace_normal_contacts(
        &self,
        id: ItemId,
        point: &Point,
        ignore_net: bool,
    ) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let Some(item) = self.items.get(&id) else {
            return result;
        };
        let Some(trace) = item.as_trace() else {
            return result;
        };
        if *point != trace.first_corner() && *point != trace.last_corner() {
            return result;
        }
        for other_id in self.overlapping_objects(&point_tile(point), trace.layer) {
            let other = self.item_ref(other_id);
            if other_id == id
                || !other.shares_layer(item)
                || !(ignore_net || other.shares_net(item))
            {
                continue;
            }
            if contact_at_point(other, point) {
                result.insert(other_id);
            }
        }
        // N04: under `inside_pad` the pins whose pad the end's cap overlaps,
        // and (N04.4) the traces of a common net whose copper it overlaps.
        self.add_trace_end_pad_contacts(item, point, ignore_net, &mut result);
        self.add_trace_end_trace_contacts(item, point, &mut result);
        result
    }

    /// Java `ConductionArea.getNormalContacts`: the traces ending, and the
    /// pins and vias centred, in the area. Not Java: two fixed copper shapes
    /// of a net (Q22.4, [`insert_fixed_copper_area`](Self::insert_fixed_copper_area))
    /// whose areas meet are in contact too, so the pieces of one stroked
    /// shape are one copper.
    fn conduction_area_normal_contacts(&self, item: &Item) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let layer = item.first_layer();
        let is_fixed = |i: &Item| i.as_area().is_some_and(|a| a.is_fixed_obstacle());
        let fixed = is_fixed(item);
        for i in 0..self.tile_shape_count(item) {
            let Some(shape) = self.tile_shape(item, i) else {
                continue;
            };
            for other_id in self.overlapping_objects(&shape, layer) {
                let other = self.item_ref(other_id);
                if other_id == item.id() || !other.shares_net(item) || !other.shares_layer(item) {
                    continue;
                }
                let contained = match &other.kind {
                    ItemKind::Trace(t) => {
                        shape.contains(&t.first_corner()) || shape.contains(&t.last_corner())
                    }
                    ItemKind::Pin(_) | ItemKind::Via(_) => {
                        shape.contains(&Point::Int(other.drill().expect("drill").center))
                    }
                    ItemKind::Area(_) if fixed && is_fixed(other) => (0..self
                        .tile_shape_count(other))
                        .filter_map(|j| self.tile_shape(other, j))
                        .any(|t| t.intersects_tile(&shape)),
                    _ => false,
                };
                if contained {
                    result.insert(other_id);
                }
            }
        }
        result
    }

    /// The contacts at the first corner of trace `id`.
    pub fn trace_start_contacts(&self, id: ItemId) -> BTreeSet<ItemId> {
        match self.items.get(&id).and_then(Item::as_trace) {
            Some(trace) => self.trace_normal_contacts(id, &trace.first_corner(), false),
            None => BTreeSet::new(),
        }
    }

    /// The contacts at the last corner of trace `id`.
    pub fn trace_end_contacts(&self, id: ItemId) -> BTreeSet<ItemId> {
        match self.items.get(&id).and_then(Item::as_trace) {
            Some(trace) => self.trace_normal_contacts(id, &trace.last_corner(), false),
            None => BTreeSet::new(),
        }
    }

    /// The connectable items of a common net overlapping item `id` (on
    /// `layer` if given).
    pub fn all_contacts(&self, id: ItemId, layer: Option<i32>) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let Some(item) = self.items.get(&id) else {
            return result;
        };
        if !item.is_connectable_kind() {
            return result;
        }
        for i in 0..self.tile_shape_count(item) {
            let shape_layer = item.shape_layer(i);
            if layer.is_some_and(|l| l != shape_layer) {
                continue;
            }
            let Some(shape) = self.tile_shape(item, i) else {
                continue;
            };
            for other_id in self.overlapping_objects(&shape, shape_layer) {
                let other = self.item_ref(other_id);
                if other_id != id && other.is_connectable_kind() && other.shares_net(item) {
                    result.insert(other_id);
                }
            }
        }
        result
    }

    /// True if item `id` touches another connectable item of its net.
    pub fn is_connected(&self, id: ItemId) -> bool {
        !self.all_contacts(id, None).is_empty()
    }

    /// The items reachable from `id` over normal contacts, restricted to
    /// net `net_number` if positive. With `stop_at_plane` the search does
    /// not pass conduction areas that belong to no component.
    pub fn connected_set(
        &self,
        id: ItemId,
        net_number: i32,
        stop_at_plane: bool,
    ) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let Some(item) = self.items.get(&id) else {
            return result;
        };
        if net_number > 0 && !item.contains_net(net_number) {
            return result;
        }
        result.insert(id);
        self.connected_set_recu(
            id,
            &mut result,
            net_number,
            stop_at_plane,
            &mut HashMap::default(),
        );
        result
    }

    /// [`connected_set`](Self::connected_set) with the normal contacts of
    /// the visited items memoized in `contacts`, for callers that query many
    /// items of an unchanged board.
    pub(crate) fn connected_set_cached(
        &self,
        id: ItemId,
        net_number: i32,
        stop_at_plane: bool,
        contacts: &mut HashMap<ItemId, Vec<ItemId>>,
    ) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let Some(item) = self.items.get(&id) else {
            return result;
        };
        if net_number > 0 && !item.contains_net(net_number) {
            return result;
        }
        result.insert(id);
        self.connected_set_recu(id, &mut result, net_number, stop_at_plane, contacts);
        result
    }

    fn cached_normal_contacts(
        &self,
        id: ItemId,
        contacts: &mut HashMap<ItemId, Vec<ItemId>>,
    ) -> std::vec::IntoIter<ItemId> {
        contacts
            .entry(id)
            .or_insert_with(|| self.normal_contacts(id).into_iter().collect())
            .clone()
            .into_iter()
    }

    fn connected_set_recu(
        &self,
        id: ItemId,
        result: &mut BTreeSet<ItemId>,
        net_number: i32,
        stop_at_plane: bool,
        contacts: &mut HashMap<ItemId, Vec<ItemId>>,
    ) {
        // Java recurses; an explicit stack visits the items in the same
        // depth-first order.
        let mut stack: Vec<std::vec::IntoIter<ItemId>> =
            vec![self.cached_normal_contacts(id, contacts)];
        while let Some(top) = stack.last_mut() {
            let Some(contact) = top.next() else {
                stack.pop();
                continue;
            };
            let other = self.item_ref(contact);
            if stop_at_plane && other.is_conduction_area() && other.component_id() <= 0 {
                continue;
            }
            if net_number > 0 && !other.contains_net(net_number) {
                continue;
            }
            if result.insert(contact) {
                stack.push(self.cached_normal_contacts(contact, contacts));
            }
        }
    }

    /// The connectable items of the net (or the nets of `id` if
    /// `net_number <= 0`) not connected to `id`.
    pub fn unconnected_set(&self, id: ItemId, net_number: i32) -> BTreeSet<ItemId> {
        self.unconnected_set_cached(id, net_number, &mut HashMap::default())
    }

    /// [`unconnected_set`](Self::unconnected_set) with memoized contacts
    /// (see [`connected_set_cached`](Self::connected_set_cached)).
    pub(crate) fn unconnected_set_cached(
        &self,
        id: ItemId,
        net_number: i32,
        contacts: &mut HashMap<ItemId, Vec<ItemId>>,
    ) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let Some(item) = self.items.get(&id) else {
            return result;
        };
        if net_number > 0 && !item.contains_net(net_number) {
            return result;
        }
        if net_number > 0 {
            result.extend(self.connectable_items(net_number));
        } else {
            for net in item.net_numbers() {
                result.extend(self.connectable_items(*net));
            }
        }
        for connected in self.connected_set_cached(id, net_number, false, contacts) {
            result.remove(&connected);
        }
        result
    }

    /// The connected sets of net `net_number`.
    pub fn connected_sets(&self, net_number: i32) -> Vec<BTreeSet<ItemId>> {
        let mut result = Vec::new();
        if net_number <= 0 {
            return result;
        }
        let mut to_handle: BTreeSet<ItemId> =
            self.connectable_items(net_number).into_iter().collect();
        while let Some(first) = to_handle.first().copied() {
            let set = self.connected_set(first, net_number, false);
            for id in &set {
                to_handle.remove(id);
            }
            // The start item is always in its own set.
            to_handle.remove(&first);
            result.push(set);
        }
        result
    }

    /// Java `Item.isCycleRecu`.
    fn is_cycle_recu(
        &self,
        id: ItemId,
        visited: &mut BTreeSet<ItemId>,
        search_item: ItemId,
        come_from: ItemId,
        ignore_areas: bool,
    ) -> bool {
        if ignore_areas && self.item_ref(id).is_conduction_area() {
            return false;
        }
        for contact in self.normal_contacts(id) {
            if contact == come_from {
                continue;
            }
            if contact == search_item {
                return true;
            }
            if visited.insert(contact)
                && self.is_cycle_recu(contact, visited, search_item, id, ignore_areas)
            {
                return true;
            }
        }
        false
    }

    /// True if trace `id` has a contact in common at both ends (Java
    /// `Trace.isOverlap`).
    pub fn trace_is_overlap(&self, id: ItemId) -> bool {
        let start = self.trace_start_contacts(id);
        let end = self.trace_end_contacts(id);
        !start.is_disjoint(&end)
    }

    /// True if trace `id` is part of a cycle (Java `Trace.isCycle`).
    pub fn trace_is_cycle(&self, id: ItemId) -> bool {
        if self.trace_is_overlap(id) {
            return true;
        }
        let start_contacts = self.trace_start_contacts(id);
        let mut visited = start_contacts.clone();
        let ignore_areas = self.ignore_cycles_with_areas(self.item_ref(id));
        start_contacts
            .iter()
            .any(|contact| self.is_cycle_recu(*contact, &mut visited, id, id, ignore_areas))
    }

    /// Java `Item.isTail`: a trace with an open end, or a via with at most
    /// one contact or contacts on the same layers only.
    pub fn is_tail(&self, id: ItemId) -> bool {
        let Some(item) = self.items.get(&id) else {
            return false;
        };
        match &item.kind {
            ItemKind::Trace(_) => {
                self.trace_start_contacts(id).is_empty() || self.trace_end_contacts(id).is_empty()
            }
            ItemKind::Via(_) => {
                let contacts = self.normal_contacts(id);
                if contacts.len() <= 1 {
                    return true;
                }
                let mut iter = contacts.iter().map(|c| self.item_ref(*c));
                let first = iter.next().expect("contact");
                let (first_layer, last_layer) = (first.first_layer(), first.last_layer());
                iter.all(|c| c.first_layer() == first_layer && c.last_layer() == last_layer)
            }
            _ => false,
        }
    }

    /// Java `Item.isFanoutVia`.
    pub fn is_fanout_via(&self, id: ItemId, ignore_items: Option<&BTreeSet<ItemId>>) -> bool {
        for contact_id in self.normal_contacts(id) {
            let contact = self.item_ref(contact_id);
            if contact.is_pin()
                && contact.first_layer() == contact.last_layer()
                && self.normal_contacts(contact_id).len() <= 1
            {
                return true;
            }
            if let Some(trace) = contact.as_trace() {
                if ignore_items.is_some_and(|ignore| ignore.contains(&contact_id)) {
                    continue;
                }
                if trace.length() >= PROTECT_FANOUT_LENGTH * f64::from(trace.half_width) {
                    continue;
                }
                for tmp_id in self.normal_contacts(contact_id) {
                    let tmp = self.item_ref(tmp_id);
                    if tmp.is_pin()
                        && tmp.first_layer() == tmp.last_layer()
                        && self.normal_contacts(tmp_id).len() <= 1
                    {
                        return true;
                    }
                    if tmp.fixed_state() == FixedState::ShoveFixed
                        && tmp.as_trace().is_some_and(|t| t.corner_count() == 2)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// All traces and vias from item `id` up to the next fork or terminal
    /// item (Java `Item.getConnectionItems`).
    pub fn connection_items(
        &self,
        id: ItemId,
        stop_option: StopConnectionOption,
    ) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let Some(item) = self.items.get(&id) else {
            return result;
        };
        let contacts = self.normal_contacts(id);
        if item.is_routable() {
            result.insert(id);
        }
        for start_contact in contacts {
            let mut current_id = start_contact;
            let Some(mut prev_contact_point) =
                self.normal_contact_point(item, self.item_ref(current_id))
            else {
                continue;
            };
            let mut prev_contact_layer = item.first_common_layer(self.item_ref(current_id));
            if item.is_trace()
                && self
                    .trace_normal_contacts(id, &prev_contact_point, false)
                    .len()
                    != 1
            {
                continue;
            }
            loop {
                let current = self.item_ref(current_id);
                // N04.4: a trace reached in its middle (a T junction) is a
                // fork, as the split trace is in Java.
                if !current.is_routable()
                    || self.is_trace_body_contact(current, &prev_contact_point)
                {
                    break;
                }
                if current.is_via() {
                    if stop_option == StopConnectionOption::Via {
                        break;
                    }
                    if stop_option == StopConnectionOption::FanoutVia
                        && self.is_fanout_via(current_id, Some(&result))
                    {
                        break;
                    }
                }
                if !result.insert(current_id) {
                    // back at an item already collected: the connection is
                    // a cycle (Java 588320207, after the pin)
                    break;
                }
                let mut next: Option<(ItemId, Point, i32)> = None;
                let mut fork_found = false;
                for tmp_id in self.normal_contacts(current_id) {
                    let tmp = self.item_ref(tmp_id);
                    let tmp_layer = current.first_common_layer(tmp);
                    if tmp_layer < 0 {
                        continue;
                    }
                    let Some(tmp_point) = self.normal_contact_point(current, tmp) else {
                        fork_found = true;
                        break;
                    };
                    if prev_contact_layer != tmp_layer || prev_contact_point != tmp_point {
                        if next.is_some() {
                            fork_found = true;
                            break;
                        }
                        next = Some((tmp_id, tmp_point, tmp_layer));
                    }
                }
                let Some((next_id, next_point, next_layer)) = next else {
                    break;
                };
                if fork_found {
                    break;
                }
                current_id = next_id;
                prev_contact_point = next_point;
                prev_contact_layer = next_layer;
            }
        }
        result
    }

    // ------------------------------------------------------------------
    // Item changes
    // ------------------------------------------------------------------

    /// Assigns item `id` to net `net_number`, or removes its nets if
    /// `net_number <= 0` (Java `Item.assignNetNo`).
    pub fn assign_net_no(&mut self, id: ItemId, net_number: i32) {
        if !Nets::is_normal_net_number(net_number) {
            return;
        }
        if net_number > self.rules.nets.max_net_number() {
            self.log.push("Item.assign_net_no: netNumber to big".into());
            return;
        }
        self.items.save_for_undo(&id);
        let Some(item) = self.items.get_mut(&id) else {
            return;
        };
        let old_nets = item.net_numbers.clone();
        if net_number <= 0 {
            item.net_numbers.clear();
        } else if item.net_numbers.is_empty() {
            item.net_numbers.push(net_number);
        } else {
            item.net_numbers[0] = net_number;
        }
        if item.is_pin() {
            self.invalidate_edge_pin_nets();
        }
        self.note_nets_changed(id, &old_nets);
    }

    /// Changes the clearance class of item `id` and updates the trees if
    /// they are compensated (Java `Item.changeClearanceClassIndex`).
    pub fn change_clearance_class(&mut self, id: ItemId, class: i32) {
        if class < 0 || class >= self.rules.clearance_matrix.class_count() as i32 {
            self.log
                .push("Item.set_clearance_class_no: index out of range".into());
            return;
        }
        if let Some(item) = self.items.get_mut(&id) {
            item.clearance_class = class;
        }
        if self.search_trees.is_clearance_compensation_used() {
            self.reinsert_into_trees(id);
        }
        self.note_changed(id);
    }

    /// Sets the fixed state of item `id`.
    pub fn set_fixed_state(&mut self, id: ItemId, fixed_state: FixedState) {
        if let Some(item) = self.items.get_mut(&id) {
            item.fixed_state = fixed_state;
        }
        self.note_changed(id);
    }

    /// Unfixes item `id` unless it is system fixed.
    pub fn unfix(&mut self, id: ItemId) {
        if let Some(item) = self.items.get_mut(&id) {
            item.unfix();
        }
        self.note_changed(id);
    }

    /// Removes net `net_number` from item `id` (Java `removeFromNet`).
    pub fn remove_from_net(&mut self, id: ItemId, net_number: i32) -> bool {
        let Some(item) = self.items.get_mut(&id) else {
            return false;
        };
        let old_nets = item.net_numbers.clone();
        let removed = item.remove_from_net(net_number);
        if removed {
            self.note_nets_changed(id, &old_nets);
        }
        removed
    }

    /// Sets whether conduction area `id` is an obstacle for other nets.
    pub fn set_conduction_is_obstacle(&mut self, id: ItemId, value: bool) {
        if let Some(ItemKind::Area(area)) = self.items.get_mut(&id).map(|i| &mut i.kind)
            && let AreaKind::Conduction { is_filled, .. } = area.kind
        {
            area.set_kind(AreaKind::Conduction {
                is_obstacle: value,
                is_filled,
            });
            self.note_changed(id);
        }
    }

    /// Switches clearance compensation of the search trees.
    pub fn set_clearance_compensation_used(&mut self, value: bool) {
        let ctx = ShapeContext {
            rules: &self.rules,
            padstacks: &self.library.padstacks,
            communication: &self.communication,
            layer_count: self.layer_structure.count(),
            split_quirks: self.java_quirks.split(),
        };
        self.search_trees
            .set_clearance_compensation_used(value, &self.items, &ctx);
    }

    /// Removes and reinserts all items in the search trees (Java
    /// `SearchTreeManager.reinsertTreeItems`).
    pub fn reinsert_tree_items(&mut self) {
        let ctx = ShapeContext {
            rules: &self.rules,
            padstacks: &self.library.padstacks,
            communication: &self.communication,
            layer_count: self.layer_structure.count(),
            split_quirks: self.java_quirks.split(),
        };
        self.search_trees.rebuild(&self.items, &ctx);
        if self.autoroute_log.is_some() {
            // Java clears each item's derived data before the reinsertion and
            // again in `insertAllBoardItems`.
            let ids = self.item_ids();
            for id in ids.iter().chain(ids.iter()) {
                self.log_autoroute(crate::board::AutorouteChange::InfoCleared(*id));
            }
        }
    }

    // ------------------------------------------------------------------
    // Undo
    // ------------------------------------------------------------------

    /// Makes the current state restorable by [`undo`](Self::undo).
    pub fn generate_snapshot(&mut self) {
        self.items.generate_snapshot();
        self.components.generate_snapshot();
    }

    /// Drops the last snapshot.
    pub fn pop_snapshot(&mut self) -> bool {
        self.items.pop_snapshot()
    }

    /// Restores the previous snapshot. Adds the nets of the changed items
    /// to `changed_nets`. Returns false if there is nothing to undo.
    pub fn undo(&mut self, mut changed_nets: Option<&mut BTreeSet<i32>>) -> bool {
        self.components.undo();
        let journaling = self.journal.is_some();
        let mut notes = Vec::new();
        let result = self.items.undo_with(|cancelled| {
            if let Some(nets) = changed_nets.as_deref_mut() {
                nets.extend(cancelled.net_numbers().iter().copied());
            }
            // Java's `SearchTreeManager.remove` takes it off the board.
            cancelled.on_the_board = false;
            notes.push(cancelled_note(cancelled, journaling));
        });
        self.apply_undo_redo(result, notes, changed_nets)
    }

    /// Restores the state before the last undo.
    pub fn redo(&mut self, mut changed_nets: Option<&mut BTreeSet<i32>>) -> bool {
        self.components.redo();
        let journaling = self.journal.is_some();
        let mut notes = Vec::new();
        let result = self.items.redo_with(|cancelled| {
            if let Some(nets) = changed_nets.as_deref_mut() {
                nets.extend(cancelled.net_numbers().iter().copied());
            }
            cancelled.on_the_board = false;
            notes.push(cancelled_note(cancelled, journaling));
        });
        self.apply_undo_redo(result, notes, changed_nets)
    }

    fn apply_undo_redo(
        &mut self,
        result: Option<(Vec<ItemId>, Vec<ItemId>)>,
        cancelled_notes: Vec<RemovalNote>,
        mut changed_nets: Option<&mut BTreeSet<i32>>,
    ) -> bool {
        let Some((cancelled, restored)) = result else {
            return false;
        };
        // The ledger and the journal see the cancelled versions leave and
        // the restored ones arrive (Java's ledger misses both, B00 Q1).
        for note in cancelled_notes {
            self.note_removed(note);
        }
        for id in &restored {
            self.note_inserted(*id);
        }
        for id in &cancelled {
            // The cancelled version is gone from the item list; only its
            // tree entries remain.
            self.search_trees.remove(*id);
        }
        // The nets of the cancelled versions were added by the caller.
        for id in &restored {
            self.search_trees.remove(*id);
            self.insert_into_trees(*id);
            self.log_autoroute(crate::board::AutorouteChange::InfoCleared(*id));
            if let (Some(nets), Some(item)) = (changed_nets.as_deref_mut(), self.items.get(id)) {
                nets.extend(item.net_numbers().iter().copied());
            }
        }
        self.invalidate_edge_pin_nets();
        true
    }

    // ------------------------------------------------------------------
    // Changed area (RoutingBoard)
    // ------------------------------------------------------------------

    /// The changed area on `layer` while marking, for use as clip shape.
    pub(crate) fn changed_area_on_layer(&self, layer: i32) -> Option<IntOctagon> {
        self.changed_area
            .as_ref()
            .map(|area| area.get_area(layer as usize))
    }

    pub(crate) fn join_changed_area(&mut self, point: FloatPoint, layer: i32) {
        if let Some(area) = &mut self.changed_area {
            area.join(point, layer as usize);
        }
    }

    pub(crate) fn normalize_suppressed(&mut self) -> &mut HashSet<i32> {
        &mut self.normalize_suppressed_nets
    }

    /// The nets [`normalize_traces`](Self::normalize_traces) skips on this
    /// board (Java `normalizeSuppressedNetNos`, transient), ascending.
    pub fn normalize_suppressed_nets(&self) -> Vec<i32> {
        let mut nets: Vec<i32> = self.normalize_suppressed_nets.iter().copied().collect();
        nets.sort_unstable();
        nets
    }

    /// Makes [`normalize_traces`](Self::normalize_traces) skip net
    /// `net_number` on this board, as it does after an oscillating net hit
    /// its iteration limit.
    pub fn suppress_normalize(&mut self, net_number: i32) {
        self.normalize_suppressed_nets.insert(net_number);
    }

    /// True if the edge-pin-net cache of the outline is filled.
    pub fn edge_pin_nets_cached(&self) -> bool {
        self.edge_pin_nets.get().is_some()
    }

    /// The tree shape of drill item `id` (a pin or via) on board `layer` in
    /// the tree compensated for `tree_class` (Java
    /// `DrillItem.getTreeShapeOnLayer`, `DrillItem.java:241-250`). `None` if
    /// there is no such tree, the item is no drill item, or `layer` is
    /// outside its layer range.
    pub fn tree_shape_on_layer(
        &self,
        tree_class: i32,
        id: ItemId,
        layer: i32,
    ) -> Option<TileShape> {
        let tree = self.search_trees.tree_by_class(tree_class)?;
        let item = self.items.get(&id)?;
        let drill = item.drill()?;
        if layer < drill.first_layer || layer > drill.last_layer {
            return None;
        }
        let index = (layer - drill.first_layer) as usize;
        tree.tree_shape(item, index, &self.shape_context())
    }

    /// Port of `ShapeSearchTree.reduceTraceShapeAtTiePin` (`:922-965`) on
    /// the tree compensated for `tree_class`: if trace `trace` starts (or
    /// ends) at the centre of pin `pin`, the pin's tree shape on the trace
    /// layer is cut out of the trace's first (or last) tree shape. The last
    /// 2-dimensional piece that contains the compare corner (corner 1, or
    /// corner `count - 2`) is kept, else the first 2-dimensional piece, and
    /// stored with `change_item_shape`. Returns true if the shape changed.
    /// The maze search calls it on the autoroute tree, and the change stays
    /// for later connections.
    pub fn reduce_trace_shape_at_tie_pin(
        &mut self,
        tree_class: i32,
        pin: ItemId,
        trace: ItemId,
    ) -> bool {
        let Some((trace_item, polyline, layer)) = self
            .items
            .get(&trace)
            .and_then(|i| i.as_trace().map(|t| (i, t.polyline.clone(), t.layer)))
        else {
            return false;
        };
        let Some(center) = self
            .items
            .get(&pin)
            .and_then(|p| p.drill())
            .map(|d| d.center)
        else {
            return false;
        };
        let Some(pin_shape) = self.tree_shape_on_layer(tree_class, pin, layer) else {
            return false;
        };
        let center = freerouting_geometry::Point::Int(center);
        let corner_count = polyline.corner_count();
        let (shape_no, compare_corner) = if polyline.first_corner() == center {
            (0, polyline.corner_approx(1))
        } else if polyline.last_corner() == center {
            (corner_count - 2, polyline.corner_approx(corner_count - 2))
        } else {
            return false;
        };
        let Some(tree) = self.search_trees.tree_by_class(tree_class) else {
            return false;
        };
        let Some(trace_shape) = tree.tree_shape(trace_item, shape_no, &self.shape_context()) else {
            return false;
        };
        if trace_shape.intersection(&pin_shape).dimension() < 2 {
            return false;
        }
        // N06 batch G: the cutouts' `router.java_quirks.*` switches, and their
        // reachability counts (N06.4).
        let mut reach = freerouting_geometry::SplitReach::default();
        let pieces = trace_shape.cutout_counted(&pin_shape, self.java_quirks.split(), &mut reach);
        crate::run_stats::record_split_reach(&reach);
        let Some(pieces) = pieces else {
            return false;
        };
        let mut new_shape: Option<TileShape> = None;
        for piece in pieces {
            if piece.dimension() == 2
                && (new_shape.is_none() || piece.contains_float(compare_corner))
            {
                new_shape = Some(piece);
            }
        }
        let new_shape =
            new_shape.unwrap_or_else(|| TileShape::Simplex(freerouting_geometry::Simplex::empty()));
        self.search_trees
            .tree_by_class_mut(tree_class)
            .is_some_and(|tree| tree.change_item_shape(trace, shape_no, new_shape))
    }

    /// Sets how the search trees order room completion and count work
    /// (P04; the routing job sets it from `router.room_order` and
    /// `router.work_unit`).
    pub fn set_search_config(&mut self, config: crate::board::search::SearchConfig) {
        self.search_trees.set_search_config(config);
    }

    /// The search tree compensated for `clearance_class`, created and
    /// filled if there is none (Java `SearchTreeManager.getAutorouteTree`).
    pub fn autoroute_search_tree(&mut self, clearance_class: i32) -> &ShapeSearchTree {
        let ctx = ShapeContext {
            rules: &self.rules,
            padstacks: &self.library.padstacks,
            communication: &self.communication,
            layer_count: self.layer_structure.count(),
            split_quirks: self.java_quirks.split(),
        };
        self.search_trees
            .get_autoroute_tree(clearance_class, &self.items, &ctx)
    }
}

/// What the ledger and the journal need of an item that leaves the item
/// list.
struct RemovalNote {
    id: ItemId,
    nets: Vec<i32>,
    connectable: bool,
    bounds: IntBox,
}

fn cancelled_note(item: &Item, journaling: bool) -> RemovalNote {
    RemovalNote {
        id: item.id(),
        nets: item.net_numbers.clone(),
        connectable: item.is_connectable_kind(),
        bounds: if journaling {
            item.bounding_box()
        } else {
            IntBox::EMPTY
        },
    }
}

/// Java's contact test of `Trace`, `DrillItem` and `ConductionArea`
/// getNormalContacts for an item found at `point`.
fn contact_at_point(other: &Item, point: &Point) -> bool {
    match &other.kind {
        ItemKind::Trace(t) => *point == t.first_corner() || *point == t.last_corner(),
        ItemKind::Pin(_) | ItemKind::Via(_) => {
            *point == Point::Int(other.drill().expect("drill").center)
        }
        ItemKind::Area(area) if area.is_conduction_area() => area.area().contains(point),
        _ => false,
    }
}

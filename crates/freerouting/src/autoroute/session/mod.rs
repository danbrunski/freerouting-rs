//! The room graph of one connection search (A02).
//!
//! Port of the room database of `autoroute/maze/AutorouteEngine.java`
//! (`:321-700`), the expansion rooms and doors of `autoroute/expansion/*`,
//! `Sorted45DegreeRoomNeighbours`, `ItemAutorouteInfo` and the search
//! initialisation `MazeSearchEngine.getInstance`/`init` (`:147-183`,
//! `:991-1125`), at `a773a70ec`.
//!
//! **One session per connection.** A [`Session`] borrows the board and the
//! connection's [`AutorouteControl`] and owns everything Java's engine and
//! its object graph hold for one connection (D1: arenas and indexes instead
//! of objects):
//!
//! - rooms in `rooms: Vec<Room>` addressed by [`RoomId`]: incomplete free
//!   rooms, complete free rooms (which also live in the autoroute search tree
//!   under their Java id, a [`TreeRoomId`]) and obstacle rooms (an item's tree
//!   shape, created lazily through the side table, as
//!   `ItemAutorouteInfo.getExpansionRoom`);
//! - doors between two rooms in `doors: Vec<Door>` ([`DoorId`]), and doors
//!   from a complete room to a start or destination item in
//!   `target_doors: Vec<TargetDoor>` ([`TargetDoorId`]);
//! - the maze search state of the door sections in one flat
//!   `sections: Vec<SectionState>`; a door owns a range of it.
//!
//! Removed rooms stay in the arena as tombstones and are never reused, so a
//! handle stays valid for the whole session. Per-room door lists keep
//! Java's `ArrayList` semantics: append on add, and removal of the first
//! occurrence shifting the rest.
//!
//! Orders that decide results, each marked `// PARITY:` where it is
//! reproduced: the incomplete and complete room lists, the door lists, the
//! tree inserts and removals (they shape the tree, D5), and the room id
//! counter (`AutorouteEngine.generateRoomIdNo`), which every neighbour
//! calculation advances, obstacle rooms and edge-removal restarts included.
//!
//! **Exceptions are values.** Java's `completeExpansionRoom` catches every
//! exception and returns no rooms, and `autorouteConnection` catches what
//! `getInstance` throws. The port returns [`RoomError`] and maps it where
//! Java's catch is. Each variant names the Java exception it stands for.
//!
//! The neighbour calculation follows the tree kind
//! ([`select_calculation_mode`]): 45 degrees (`neighbours45`), 90 degrees
//! (`neighbours90.rs`, box rooms, A07.2) and any angle
//! (`neighbours_any.rs`, convex rooms, A07.4, with its neighbour set in a
//! [`java_tree_set::JavaTreeSet`] because its comparator is not
//! transitive).
//!
//! The modules:
//! - [`rooms`]: the room database (`AutorouteEngine.java:355-700`);
//! - [`neighbours45`]: the 45 degree neighbour calculation
//!   (`Sorted45DegreeRoomNeighbours`);
//! - `incomplete45.rs`: the new incomplete rooms of the 45 degree
//!   calculation;
//! - [`doors`]: `ExpansionDoor` and `TargetItemExpansionDoor` geometry and
//!   their sections;
//! - [`queue`]: `MazeListElement` and the maze queue;
//! - [`init`]: the search initialisation (`MazeSearchEngine.init`);
//! - [`drills`]: the drill pages and drills (`autoroute/drill/*`, A03);
//! - [`search`]: the maze search, one core generic over a sink
//!   (`MazeSearchEngine`, A04), with [`sink`] (its hooks), [`layers`]
//!   (`MazeExpansionEngine`), [`ripup`] (`MazeRipupResolver`, `Connection`)
//!   and [`shove_room`] (`MazeTraceShover`).

pub mod doors;
pub mod drills;
mod incomplete45;
pub mod init;
pub mod java_tree_set;
pub mod layers;
pub mod neighbours45;
mod neighbours90;
mod neighbours_any;
pub mod queue;
pub mod ripup;
pub mod rooms;
pub mod search;
pub mod shove_room;
pub mod sink;

use std::collections::BTreeMap;
use std::fmt;

use freerouting_geometry::{Stoppable, TileShape, TileShapeOps};

use crate::autoroute::control::AutorouteControl;
use crate::autoroute::distance::DestinationDistance;
use crate::autoroute::java_ids;
use crate::autoroute::width_policy::{self, WidthPolicy};
use crate::board::RoutingBoard;
use crate::board::basic_board::BasicBoard;
use crate::board::items::ItemId;
use crate::board::search::complete_shape::CompletionScratch;
use crate::board::search::{RoomId as TreeRoomId, ShapeSearchTree, TreeKind, WorkLimit};
use crate::board::side_table::ItemSideTable;

pub use doors::{Adjustment, SectionState};
pub use queue::{MazeListElement, MazeQueue, QueueKey};

/// A room of the session's arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RoomId(pub u32);

/// A door between two rooms of the session's arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DoorId(pub u32);

/// A door from a complete room to a start or destination item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetDoorId(pub u32);

/// Something the maze search expands (Java `ExpandableObject`). A03 adds
/// the drills and drill pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExpandableRef {
    Door(DoorId),
    TargetDoor(TargetDoorId),
    /// `ExpansionDrill` (A04: a layer change).
    Drill(drills::DrillId),
    /// `DrillPage` (A04: the page whose drills are expanded next).
    DrillPage(drills::DrillPageId),
}

/// The kind of a room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomKind {
    /// `IncompleteFreeSpaceExpansionRoom`.
    Incomplete,
    /// `CompleteFreeSpaceExpansionRoom`.
    CompleteFree,
    /// `ObstacleExpansionRoom`.
    Obstacle,
}

/// An expansion room (Java `ExpansionRoom` and its classes).
#[derive(Clone, Debug)]
pub enum Room {
    /// `IncompleteFreeSpaceExpansionRoom`: a room whose shape is not yet
    /// calculated. `shape: None` is the whole plane.
    Incomplete {
        shape: Option<TileShape>,
        contained: Option<TileShape>,
        layer: i32,
        doors: Vec<DoorId>,
    },
    /// `CompleteFreeSpaceExpansionRoom`: a free room, in the autoroute tree
    /// under `TreeRoomId(id)` once it is added to the database.
    CompleteFree {
        id: i32,
        shape: TileShape,
        layer: i32,
        doors: Vec<DoorId>,
        target_doors: Vec<TargetDoorId>,
        net_dependent: bool,
    },
    /// `ObstacleExpansionRoom`: tree shape `index` of `item`, for ripping
    /// and pushing.
    Obstacle {
        item: ItemId,
        index: usize,
        shape: TileShape,
        layer: i32,
        doors: Vec<DoorId>,
        doors_calculated: bool,
    },
}

impl Room {
    pub fn kind(&self) -> RoomKind {
        match self {
            Room::Incomplete { .. } => RoomKind::Incomplete,
            Room::CompleteFree { .. } => RoomKind::CompleteFree,
            Room::Obstacle { .. } => RoomKind::Obstacle,
        }
    }

    /// The shape (`getShape`); `None` for an incomplete room on the whole
    /// plane.
    pub fn shape(&self) -> Option<&TileShape> {
        match self {
            Room::Incomplete { shape, .. } => shape.as_ref(),
            Room::CompleteFree { shape, .. } | Room::Obstacle { shape, .. } => Some(shape),
        }
    }

    pub fn layer(&self) -> i32 {
        match self {
            Room::Incomplete { layer, .. }
            | Room::CompleteFree { layer, .. }
            | Room::Obstacle { layer, .. } => *layer,
        }
    }

    /// The doors to neighbour rooms, in Java's list order.
    pub fn doors(&self) -> &[DoorId] {
        match self {
            Room::Incomplete { doors, .. }
            | Room::CompleteFree { doors, .. }
            | Room::Obstacle { doors, .. } => doors,
        }
    }

    pub(crate) fn doors_mut(&mut self) -> &mut Vec<DoorId> {
        match self {
            Room::Incomplete { doors, .. }
            | Room::CompleteFree { doors, .. }
            | Room::Obstacle { doors, .. } => doors,
        }
    }

    /// The target doors (`getTargetDoors`); empty but for a complete free
    /// room.
    pub fn target_doors(&self) -> &[TargetDoorId] {
        match self {
            Room::CompleteFree { target_doors, .. } => target_doors,
            _ => &[],
        }
    }

    /// True for a complete room (`instanceof CompleteExpansionRoom`).
    pub fn is_complete(&self) -> bool {
        !matches!(self, Room::Incomplete { .. })
    }
}

/// A door between two rooms (Java `ExpansionDoor`).
#[derive(Clone, Debug)]
pub struct Door {
    /// `firstRoom`, `secondRoom`.
    pub rooms: [RoomId; 2],
    /// 1 or 2.
    pub dimension: i32,
    /// The range of the door's sections in the session's section array;
    /// empty until `section_segments` allocates them.
    pub(crate) sections: (u32, u32),
}

/// A door to a start or destination item (Java `TargetItemExpansionDoor`).
/// Whether it leads to a destination is read from the item's start flag
/// when asked, as Java's `isDestinationDoor` does.
#[derive(Clone, Debug)]
pub struct TargetDoor {
    pub item: ItemId,
    /// The tree shape index of the item (`treeEntryNo`).
    pub tree_entry: usize,
    /// The complete room of the door (`room`).
    pub room: Option<RoomId>,
    /// The item's tree shape intersected with the room's shape.
    pub shape: TileShape,
    /// The one maze search element of the door.
    pub section: SectionState,
}

/// The autorouter's data per item (Java `ItemAutorouteInfo`). The cached
/// connection (A04.6) and a via's drill (A03.4) join it later.
#[derive(Clone, Debug, Default)]
pub struct ItemAutorouteInfo {
    /// True if the item belongs to the start set of the search
    /// (`startInfo`).
    pub start: bool,
    /// The obstacle room of each tree shape (`expansionRoomArr`), created
    /// on demand.
    pub obstacle_rooms: Option<Vec<Option<RoomId>>>,
    /// The drill of a via (`Via.autorouteDrillInfo`), created on demand
    /// (A03.4).
    pub drill: Option<drills::DrillId>,
}

/// The neighbour calculation of a tree kind (Java
/// `SortedRoomNeighbours.CalculationMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalculationMode {
    Orthogonal,
    Degree45,
    AnyAngle,
}

/// Java `SortedRoomNeighbours.selectCalculationMode`
/// (`SortedRoomNeighbours.java:80-93`): the 90 degree tree sorts
/// orthogonally, the 45 degree tree at 45 degrees, any other tree at any
/// angle.
pub fn select_calculation_mode(kind: TreeKind) -> CalculationMode {
    match kind {
        TreeKind::NinetyDegree => CalculationMode::Orthogonal,
        TreeKind::FortyfiveDegree => CalculationMode::Degree45,
        TreeKind::Plain => CalculationMode::AnyAngle,
    }
}

/// An error of the room graph where Java throws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomError {
    /// Java's `NullPointerException`: a shape, room or item that Java
    /// dereferences is missing. `at` names the Java method.
    NullPointer { at: &'static str },
    /// Java's `ArrayIndexOutOfBoundsException` (`touchingSides` of shapes
    /// that do not touch).
    IndexOutOfBounds { at: &'static str },
}

impl RoomError {
    /// The simple name of the Java exception.
    pub fn java_exception(&self) -> &'static str {
        match self {
            RoomError::NullPointer { .. } => "NullPointerException",
            RoomError::IndexOutOfBounds { .. } => "ArrayIndexOutOfBoundsException",
        }
    }
}

impl fmt::Display for RoomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RoomError::NullPointer { at } => write!(f, "NullPointerException in {at}"),
            RoomError::IndexOutOfBounds { at } => {
                write!(f, "ArrayIndexOutOfBoundsException in {at}")
            }
        }
    }
}

impl std::error::Error for RoomError {}

/// A change of the autoroute tree made by the session, in order (for tests
/// and probes: the order shapes the tree, D5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeOp {
    Insert(i32),
    Remove(i32),
}

/// The room graph and maze queue of one connection search.
pub struct Session<'b> {
    pub(crate) board: &'b mut RoutingBoard,
    pub(crate) control: &'b AutorouteControl,
    /// The clearance class of the autoroute tree.
    pub(crate) tree_class: i32,
    pub(crate) mode: CalculationMode,
    /// The net being routed (`AutorouteEngine.netNumber`).
    pub(crate) net: i32,
    pub(crate) rooms: Vec<Room>,
    pub(crate) doors: Vec<Door>,
    pub(crate) target_doors: Vec<TargetDoor>,
    pub(crate) sections: Vec<SectionState>,
    /// Existing door section ranges replaced with a different count (N06.2).
    door_section_reallocations: u64,
    /// `incompleteExpansionRooms`, in list order.
    pub(crate) incomplete: Vec<RoomId>,
    /// `completeExpansionRooms`, in list order.
    pub(crate) complete: Vec<RoomId>,
    /// The complete free rooms by Java id, to map tree leaves back.
    pub(crate) by_java_id: BTreeMap<i32, RoomId>,
    /// `expansionRoomInstanceCount`.
    pub(crate) room_count: i32,
    pub(crate) item_info: ItemSideTable<ItemAutorouteInfo>,
    pub(crate) scratch: CompletionScratch,
    pub(crate) stop: Option<&'b dyn Stoppable>,
    pub(crate) work_limit: Option<WorkLimit>,
    /// `WORK_LIMIT_CUTOFF` lines of the work limit.
    pub(crate) cutoff_log: Vec<String>,
    pub(crate) tree_ops: Vec<TreeOp>,
    pub(crate) queue: MazeQueue,
    pub(crate) distance: DestinationDistance,
    /// The drill pages (`AutorouteEngine.drillPageArray`), built on first
    /// use (A03).
    pub(crate) drill_pages: Option<drills::DrillPageArray>,
    /// The drills of the pages and vias, never reused within a session.
    pub(crate) drills: Vec<drills::ExpansionDrill>,
    /// The width policy (Q17.1); `None` reads the control's widths.
    pub(crate) widths: Option<&'b dyn WidthPolicy>,
    /// K15.25 (`router.pour_fill_target`): the opened fill's tiles of each
    /// start or destination conduction area the search reaches only where
    /// it is filled ([`pour_target`](crate::autoroute::pour_target)); set by
    /// [`init`](Self::init), empty under Java's rule.
    pub(crate) pour_fill: BTreeMap<ItemId, Vec<TileShape>>,
    /// K15.42 (`router.pour_fill_target` with `router.pour_fill_contacts`):
    /// per start or destination pin that lies alone in an island of a
    /// judged pour, the layers of those pours, the only ones on which the
    /// search leaves or enters it ([`lone_pin_layers`](crate::autoroute::pour_target::lone_pin_layers));
    /// set by [`init`](Self::init).
    pub(crate) lone_pins: BTreeMap<ItemId, Vec<i32>>,
}

impl<'b> Session<'b> {
    /// How often a door's allocated section count changed in this session.
    pub fn door_section_reallocations(&self) -> u64 {
        self.door_section_reallocations
    }

    /// A session for the net of `control` on `board` (Java `new
    /// AutorouteEngine` with `initConnection`, and the `MazeSearchEngine`
    /// constructor's destination distance). The autoroute tree of the
    /// control's clearance class is created if there is none. The stop
    /// checks of the search test `stop` and `work_limit` (Java
    /// `isStopRequested`); either may be absent.
    pub fn new(
        board: &'b mut RoutingBoard,
        control: &'b AutorouteControl,
        stop: Option<&'b dyn Stoppable>,
        work_limit: Option<WorkLimit>,
    ) -> Session<'b> {
        let tree_class = control.trace_clearance_class_index;
        let kind = board.autoroute_search_tree(tree_class).kind();
        let distance = DestinationDistance::new(
            &control.trace_costs,
            &control.layer_active,
            control.min_normal_via_cost,
            control.min_cheap_via_cost,
        );
        Session {
            board,
            control,
            tree_class,
            mode: select_calculation_mode(kind),
            net: control.net_number,
            rooms: Vec::new(),
            doors: Vec::new(),
            target_doors: Vec::new(),
            sections: Vec::new(),
            door_section_reallocations: 0,
            incomplete: Vec::new(),
            complete: Vec::new(),
            by_java_id: BTreeMap::new(),
            room_count: 0,
            item_info: ItemSideTable::new(),
            scratch: CompletionScratch::new(),
            stop,
            work_limit,
            cutoff_log: Vec::new(),
            tree_ops: Vec::new(),
            queue: MazeQueue::new(),
            distance,
            drill_pages: None,
            drills: Vec::new(),
            widths: None,
            pour_fill: BTreeMap::new(),
            lone_pins: BTreeMap::new(),
        }
    }

    /// Gives each room the half width of `policy` (Q17.1); `None` (the
    /// default) reads the control's widths.
    pub fn set_width_policy(&mut self, policy: Option<&'b dyn WidthPolicy>) {
        self.widths = policy;
    }

    /// The width policy, if any.
    pub fn width_policy(&self) -> Option<&'b dyn WidthPolicy> {
        self.widths
    }

    /// The trace half width on layer index `layer` of a room with shape
    /// `shape` (the control's without a width policy).
    #[inline]
    pub(crate) fn shape_half_width(&self, shape: Option<&TileShape>, layer: usize) -> i32 {
        width_policy::trace_half_width(self.widths, self.control, shape, layer)
    }

    /// The compensated trace half width on layer index `layer` of a room
    /// with shape `shape` (the control's without a width policy).
    #[inline]
    pub(crate) fn shape_compensated_half_width(
        &self,
        shape: Option<&TileShape>,
        layer: usize,
    ) -> i32 {
        width_policy::compensated_half_width(self.widths, self.control, shape, layer)
    }

    /// The trace half width of room `room` on layer index `layer`.
    #[inline]
    pub(crate) fn room_half_width(&self, room: RoomId, layer: usize) -> i32 {
        match self.widths {
            None => self.control.trace_half_width[layer],
            Some(_) => self.shape_half_width(self.room(room).shape(), layer),
        }
    }

    /// The compensated trace half width of room `room` on layer index
    /// `layer`.
    #[inline]
    pub(crate) fn room_compensated_half_width(&self, room: RoomId, layer: usize) -> i32 {
        match self.widths {
            None => self.control.compensated_trace_half_width[layer],
            Some(_) => self.shape_compensated_half_width(self.room(room).shape(), layer),
        }
    }

    /// K15.42: false if `item` is a lone pin ([`Self::lone_pins`]) that the
    /// search must not leave or enter on `layer`.
    pub(crate) fn pin_layer_allowed(&self, item: ItemId, layer: i32) -> bool {
        self.lone_pins
            .get(&item)
            .is_none_or(|layers| layers.contains(&layer))
    }

    /// K15.25: the parts of `shape` (a tile of conduction area `item`
    /// within a room) where the area's opened fill reaches, or `None` when
    /// the search reaches `item` anywhere (Java's rule; [`Self::pour_fill`]).
    pub(crate) fn pour_fill_parts(
        &self,
        item: ItemId,
        shape: &TileShape,
    ) -> Option<Vec<TileShape>> {
        let tiles = self.pour_fill.get(&item)?;
        Some(crate::autoroute::pour_target::fill_parts(tiles, shape))
    }

    /// The board.
    pub fn board(&self) -> &BasicBoard {
        self.board
    }

    /// The autoroute tree of the session.
    pub fn tree(&self) -> &ShapeSearchTree {
        self.board
            .search_trees()
            .tree_by_class(self.tree_class)
            .expect("the session created its autoroute tree")
    }

    pub fn control(&self) -> &AutorouteControl {
        self.control
    }

    pub fn calculation_mode(&self) -> CalculationMode {
        self.mode
    }

    pub fn room(&self, id: RoomId) -> &Room {
        &self.rooms[id.0 as usize]
    }

    pub(crate) fn room_mut(&mut self, id: RoomId) -> &mut Room {
        &mut self.rooms[id.0 as usize]
    }

    pub fn door(&self, id: DoorId) -> &Door {
        &self.doors[id.0 as usize]
    }

    pub fn target_door(&self, id: TargetDoorId) -> &TargetDoor {
        &self.target_doors[id.0 as usize]
    }

    /// `completeExpansionRooms` in list order.
    pub fn complete_rooms(&self) -> &[RoomId] {
        &self.complete
    }

    /// `incompleteExpansionRooms` in list order.
    pub fn incomplete_rooms(&self) -> &[RoomId] {
        &self.incomplete
    }

    /// The room id counter (`expansionRoomInstanceCount`).
    pub fn room_count(&self) -> i32 {
        self.room_count
    }

    /// The tree inserts and removals made so far, in order.
    pub fn tree_ops(&self) -> &[TreeOp] {
        &self.tree_ops
    }

    /// The `WORK_LIMIT_CUTOFF` line of the work limit, if it fired.
    pub fn cutoff_log(&self) -> &[String] {
        &self.cutoff_log
    }

    /// The maze queue.
    pub fn queue(&self) -> &MazeQueue {
        &self.queue
    }

    pub fn queue_mut(&mut self) -> &mut MazeQueue {
        &mut self.queue
    }

    /// The destination distance of the search.
    pub fn destination_distance(&self) -> &DestinationDistance {
        &self.distance
    }

    /// The autoroute data of `item`, if it was created.
    pub fn item_info(&self, item: ItemId) -> Option<&ItemAutorouteInfo> {
        self.item_info.get(item)
    }

    /// The autoroute data of `item`, created if missing (Java
    /// `Item.getAutorouteInfo`).
    pub(crate) fn item_info_mut(&mut self, item: ItemId) -> &mut ItemAutorouteInfo {
        if !self.item_info.contains(item) {
            self.item_info.insert(item, ItemAutorouteInfo::default());
        }
        self.item_info
            .get_mut(item)
            .expect("the entry was just inserted")
    }

    /// Java `generateRoomIdNo`: `++expansionRoomInstanceCount`.
    pub(crate) fn generate_room_id(&mut self) -> i32 {
        // PARITY: AutorouteEngine.java:696, advanced by every neighbour
        // calculation (obstacle rooms and edge-removal restarts included).
        self.room_count = self.room_count.wrapping_add(1);
        self.room_count
    }

    /// Java `isStopRequested` (`AutorouteEngine.java:308-318`): the work
    /// limit, then the stop request.
    pub fn is_stop_requested(&mut self) -> bool {
        if let Some(limit) = self.work_limit.as_mut() {
            let meter = self.board.search_trees().work_meter();
            if limit.exceeded_logging(meter, String::new, &mut self.cutoff_log) {
                return true;
            }
        }
        self.stop.is_some_and(|s| s.is_stop_requested())
    }

    /// The Java `getId` of a room: the counter id of a complete free room,
    /// `(item << 10) | index` of an obstacle room, `31 * shape id + layer`
    /// of an incomplete room (whose missing shape Java dereferences).
    pub fn room_java_id(&self, id: RoomId) -> Result<i32, RoomError> {
        match self.room(id) {
            Room::CompleteFree { id, .. } => Ok(*id),
            Room::Obstacle { item, index, .. } => {
                Ok(java_ids::obstacle_room_id(item.0, *index as i32))
            }
            Room::Incomplete { shape, layer, .. } => match shape {
                Some(shape) => Ok(java_ids::incomplete_room_id(shape.get_id(), *layer)),
                None => Err(RoomError::NullPointer {
                    at: "IncompleteFreeSpaceExpansionRoom.getId",
                }),
            },
        }
    }

    /// The arena room of a complete free room in the tree.
    pub(crate) fn room_of_tree_id(&self, id: TreeRoomId) -> Option<RoomId> {
        self.by_java_id.get(&id.0).copied()
    }

    /// A complete free room in the arena only, in no list and not in the
    /// tree (Java `new CompleteFreeSpaceExpansionRoom(shape, layer, id)`),
    /// for probes and tests.
    pub fn detached_free_room(&mut self, shape: TileShape, layer: i32, id: i32) -> RoomId {
        self.push_room(Room::CompleteFree {
            id,
            shape,
            layer,
            doors: Vec::new(),
            target_doors: Vec::new(),
            net_dependent: false,
        })
    }

    /// Adds a room to the arena (not to a list).
    pub(crate) fn push_room(&mut self, room: Room) -> RoomId {
        let id = RoomId(self.rooms.len() as u32);
        self.rooms.push(room);
        id
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod factory_tests {
    use super::*;

    /// Port of `autoroute/expansion/SortedRoomNeighboursFactoryTest` (three
    /// tests): the neighbour calculation is chosen by the tree kind.
    #[test]
    fn selects_orthogonal_calculation_for_orthogonal_search_tree() {
        assert_eq!(
            select_calculation_mode(TreeKind::NinetyDegree),
            CalculationMode::Orthogonal
        );
    }

    #[test]
    fn selects_45_degree_calculation_for_45_degree_search_tree() {
        assert_eq!(
            select_calculation_mode(TreeKind::FortyfiveDegree),
            CalculationMode::Degree45
        );
    }

    #[test]
    fn selects_any_angle_calculation_for_other_search_trees() {
        assert_eq!(
            select_calculation_mode(TreeKind::Plain),
            CalculationMode::AnyAngle
        );
    }
}

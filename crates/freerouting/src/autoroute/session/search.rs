//! The maze search (A04.1, A04.3, A04.4).
//!
//! Port of `MazeSearchEngine` (`MazeSearchEngine.java`) at `a773a70ec`:
//! `occupyNextElement` (`:326-400`), `expandToRoomDoors` (`:402-643`),
//! `expandToTargetDoors` (`:645-721`), `expandToDoor` (`:723-777`),
//! `doorIsSmall` (`:779-807`), `expandToDoorSection` (`:809-989`),
//! `roomShapeIsThick` (`:1127-1150`), `checkNeckDownAtDestPin`
//! (`:1229-1243`) and `segmentProjection` (`:185-216`). The layer changes
//! are in [`layers`](super::layers), the ripup in [`ripup`](super::ripup),
//! the shove checks of obstacle trace rooms in
//! [`shove_room`](super::shove_room).
//!
//! **One core.** [`MazeSearch`] borrows the session and is generic over a
//! [`SearchSink`] and an [`ExtraCost`]. [`MazeSearch::step`] is Java's
//! `occupyNextElement`; [`MazeSearch::find_connection`] loops it. The batch
//! router, the probes and the GUI all step this one core
//! (`gui/interactive/ExpandTestState.java` is the model for the GUI's
//! stepping between accessor reads).
//!
//! **Determinism.** The search's `java.util.Random` is reseeded with the
//! control's ripup costs for every search (`MazeSearchEngine.java:86-87`,
//! D2). The stop checks sit where Java calls `isStopRequested`: before
//! every pop, occupied sections included (`:335`), and at the four points
//! of the initialisation. They test the session's work limit (a count of
//! search-tree nodes, D5) and its stop request; the search never resets
//! the limit.
//!
//! **Section identity.** N06.2 resolves the section again after expansion so
//! occupation reaches the current door range if expansion resized it. With no
//! reallocation, the range `(start, len)` is unchanged and the arena only appends,
//! so this is exactly the slot saved at pop time. Drill ranges and target-door
//! slots are stable; drill-page pops return before this occupation write.
//!
//! The queue keys hold the Java id of the door at the time of the push.
//! Java's `TreeSet` compares the ids at comparison time, and a drill page's
//! id changes when its drills are first calculated (its net goes from -1
//! to the session's); the two differ only for a full tie of both doubles
//! with an element of such a page, which no fixture reaches.

// The hooks compiled out (`--cfg freerouting_no_search_hooks`, A04.8) leave
// their inputs unused.
#![cfg_attr(
    freerouting_no_search_hooks,
    allow(unused_imports, unused_variables, unused_mut)
)]

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::jmath::JavaRandom;
use freerouting_geometry::jmath::min_f64;
use freerouting_geometry::{FloatLine, FloatPoint, PolylineShapeOps, TileShape, TileShapeOps};

use super::doors::TRACE_WIDTH_TOLERANCE;
use super::queue::{FanoutFilter, MazeListElement, Push, QueueKey};
use super::ripup::ConnectionCache;
use super::sink::{
    BlockReason, Blocked, ExtraCost, NoExtraCost, NoopSink, PopInfo, SearchEnd, SearchSink, Step,
    StepEnd, sink,
};
use super::{Adjustment, DoorId, ExpandableRef, Room, RoomError, RoomId, Session};
use crate::board::items::ItemId;
use crate::board::selection_filter::{ItemFilter, SelectableChoice};
use crate::board::structure::AngleRestriction;
use crate::settings::board::BoardRouterSettings;

/// Java `MazeSearchEngine.ALREADY_RIPPED_COSTS`.
pub const ALREADY_RIPPED_COSTS: i32 = 1;

/// The router settings the search reads (Java's `ctrl.settings`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchSettings {
    /// `fanout.max_escape_length_mm`.
    pub fanout_max_escape_length_mm: f64,
    /// `fanout.min_escape_length_mm`.
    pub fanout_min_escape_length_mm: f64,
    /// `getStartRipupCosts()`.
    pub start_ripup_costs: i32,
}

impl SearchSettings {
    /// The values of `settings`.
    pub fn from_router(settings: &BoardRouterSettings) -> SearchSettings {
        SearchSettings {
            fanout_max_escape_length_mm: settings.fanout_max_escape_length_mm(),
            fanout_min_escape_length_mm: settings.fanout_min_escape_length_mm(),
            start_ripup_costs: settings.start_ripup_costs(),
        }
    }
}

impl Default for SearchSettings {
    /// The defaults of `DefaultSettings` (4.5 mm, 2.5 mm, 100).
    fn default() -> Self {
        SearchSettings {
            fanout_max_escape_length_mm: 4.5,
            fanout_min_escape_length_mm: 2.5,
            start_ripup_costs: 100,
        }
    }
}

/// Java `MazeSearchEngine.Result`: the destination door and its section,
/// from where the connection is backtracked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub destination: (ExpandableRef, usize),
}

/// The slot of a section: an index of the session's flat section array, or
/// a target door's own element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SectionSlot {
    Flat(usize),
    Target(super::TargetDoorId),
}

/// The maze search of one connection (Java `MazeSearchEngine`).
pub struct MazeSearch<'s, 'b, S: SearchSink = NoopSink, C: ExtraCost = NoExtraCost> {
    pub(crate) s: &'s mut Session<'b>,
    pub(crate) sink: S,
    pub(crate) extra: C,
    pub(crate) settings: SearchSettings,
    /// `randomGenerator`, seeded with the ripup costs.
    pub(crate) random: JavaRandom,
    /// The connections of `Connection.get`, cached per item as Java caches
    /// them in `ItemAutorouteInfo` (A04.6).
    pub(crate) connections: ConnectionCache,
    /// The answers of `drill_breaks_hole_to_hole` by drill and ripped via
    /// (K13.9): the board does not change during a search, so each pair is
    /// asked once; the query is not metered.
    pub(crate) hole_to_hole: BTreeMap<(super::drills::DrillId, Option<ItemId>), bool>,
    destination: Option<(ExpandableRef, usize)>,
    current: Option<MazeListElement>,
    steps: u64,
    pops: u64,
    finished: Option<Step>,
}

impl<'s, 'b> MazeSearch<'s, 'b> {
    /// A search on `session` without hooks and extra costs.
    pub fn plain(session: &'s mut Session<'b>, settings: SearchSettings) -> Self {
        MazeSearch::new(session, NoopSink, NoExtraCost, settings)
    }
}

impl<'s, 'b, S: SearchSink, C: ExtraCost> MazeSearch<'s, 'b, S, C> {
    /// Java's `MazeSearchEngine` constructor on the session (whose control
    /// is the search's `ctrl`): the random generator is reseeded with
    /// `ctrl.ripup_costs` and the queue gets the fanout filter. Call
    /// [`init`](Self::init) next, as `getInstance` does.
    pub fn new(session: &'s mut Session<'b>, sink: S, extra: C, settings: SearchSettings) -> Self {
        let control = session.control;
        // PARITY: MazeSearchEngine.java:86-87, one generator per search,
        // seeded with the ripup costs (D2).
        let random = JavaRandom::new(i64::from(control.ripup_costs));
        let resolution = session
            .board
            .communication
            .get_resolution(crate::board::structure::Unit::Um);
        let filter = FanoutFilter::new(
            control,
            settings.fanout_max_escape_length_mm,
            settings.fanout_min_escape_length_mm,
            resolution,
        );
        session.queue.set_fanout_filter(filter);
        let mut search = MazeSearch {
            s: session,
            sink,
            extra,
            settings,
            random,
            connections: ConnectionCache::default(),
            hole_to_hole: BTreeMap::new(),
            destination: None,
            current: None,
            steps: 0,
            pops: 0,
            finished: None,
        };
        sink!(search.sink.on_start(search.s));
        search
    }

    /// Java `MazeSearchEngine.init` through the session: false where
    /// `getInstance` returns null.
    pub fn init(
        &mut self,
        start: &BTreeSet<ItemId>,
        destination: &BTreeSet<ItemId>,
    ) -> Result<bool, RoomError> {
        self.s.init(start, destination)
    }

    /// Java `findConnection`: steps until the search ends. `None` if no
    /// destination was reached.
    pub fn find_connection(&mut self) -> Result<Option<SearchResult>, RoomError> {
        while self.step()? == Step::Continue {}
        Ok(self
            .destination
            .map(|destination| SearchResult { destination }))
    }

    /// Java `occupyNextElement`: expands the next element of the queue.
    pub fn step(&mut self) -> Result<Step, RoomError> {
        if let Some(done) = self.finished {
            return Ok(done);
        }
        let step = self.steps;
        self.steps += 1;
        let (outcome, popped, skipped) = self.occupy_next_element(step)?;
        sink!({
            let section_occupied = popped.is_some_and(|(door, section)| {
                self.s
                    .section_state(door, section)
                    .is_some_and(|state| state.occupied)
            });
            let end = StepEnd {
                step,
                popped,
                section_occupied,
                skipped_occupied: skipped,
                outcome,
            };
            self.sink.on_step_end(self.s, &end)
        });
        if outcome != Step::Continue {
            self.finished = Some(outcome);
            sink!({
                let end = SearchEnd {
                    outcome,
                    destination: self.destination,
                    steps: self.steps,
                };
                self.sink.on_finish(self.s, &end)
            });
        }
        Ok(outcome)
    }

    /// The session (rooms, doors, the queue) for reading.
    pub fn session(&self) -> &Session<'b> {
        self.s
    }

    /// The number of queued elements.
    pub fn queue_len(&self) -> usize {
        self.s.queue.len()
    }

    /// The element the last step expanded.
    pub fn current(&self) -> Option<&MazeListElement> {
        self.current.as_ref()
    }

    /// The destination door and section, once reached.
    pub fn destination(&self) -> Option<(ExpandableRef, usize)> {
        self.destination
    }

    /// Steps taken so far (calls of [`step`](Self::step) that ran).
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Steps that popped an element.
    pub fn pops(&self) -> u64 {
        self.pops
    }

    /// How the search ended, once it has.
    pub fn outcome(&self) -> Option<Step> {
        self.finished
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }

    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }

    /// The sink, ending the search's borrow of the session.
    pub fn into_sink(self) -> S {
        self.sink
    }

    /// `occupyNextElement` (`:326-400`): the outcome, the popped door and
    /// section, and the occupied sections popped before it.
    #[allow(clippy::type_complexity)]
    fn occupy_next_element(
        &mut self,
        step: u64,
    ) -> Result<(Step, Option<(ExpandableRef, usize)>, u32), RoomError> {
        if self.destination.is_some() {
            return Ok((Step::Found, None, 0));
        }
        let mut skipped = 0u32;
        let (key, element, slot) = loop {
            if self.s.queue.is_empty() {
                return Ok((Step::Exhausted, None, skipped));
            }
            // PARITY: MazeSearchEngine.java:335, a stop check on every pop.
            if self.s.is_stop_requested() {
                return Ok((Step::Stopped, None, skipped));
            }
            let (key, element) = self.s.queue.pop_first().expect("the queue is not empty");
            let slot = self.s.section_slot(element.door, element.section).ok_or(
                RoomError::IndexOutOfBounds {
                    at: "MazeSearchEngine.occupyNextElement",
                },
            )?;
            if !self.s.slot_state(slot).occupied {
                break (key, element, slot);
            }
            skipped += 1;
        };
        self.pops += 1;
        {
            let state = self.s.slot_state_mut(slot);
            state.backtrack = element.backtrack;
            state.room_ripped = element.room_ripped;
            state.ripup_cost = element.ripup_cost;
            state.adjustment = element.adjustment;
        }
        sink!({
            let info = PopInfo {
                step,
                key,
                element: &element,
                skipped_occupied: skipped,
            };
            self.sink.on_pop(self.s, &info)
        });
        let popped = Some((element.door, element.section));
        self.current = Some(element.clone());
        let element = &element;
        if matches!(element.door, ExpandableRef::DrillPage(_)) {
            self.expand_to_drills_of_page(element)?;
            return Ok((Step::Continue, popped, skipped));
        }
        if let ExpandableRef::TargetDoor(door) = element.door
            && self.s.is_destination_door(door)
        {
            self.destination = Some((element.door, element.section));
            return Ok((Step::Found, popped, skipped));
        }
        let is_drill = matches!(element.door, ExpandableRef::Drill(_));
        let from_drill = matches!(element.backtrack, Some((ExpandableRef::Drill(_), _)));
        let control = self.s.control;
        if control.is_fanout && is_drill && from_drill {
            // The fanout is complete after the first drill.
            self.destination = Some((element.door, element.section));
            return Ok((Step::Found, popped, skipped));
        }
        if control.vias_allowed && is_drill && !from_drill {
            self.expand_to_other_layers(element)?;
        }
        if element.next_room.is_some() && !self.expand_to_room_doors(element)? {
            // The occupation is delayed by a ripup, or nothing was expanded.
            return Ok((Step::Continue, popped, skipped));
        }
        // N06.2 fix, both profiles: MazeSearchEngine.java:394 wrote to the old
        // range. Use the current range; absent reallocation this is `slot`.
        self.s.occupy_current_section(element.door, element.section);
        Ok((Step::Continue, popped, skipped))
    }

    /// `expandToRoomDoors` (`:402-643`): true if the popped section is to be
    /// occupied, false if its occupation is delayed.
    fn expand_to_room_doors(&mut self, element: &MazeListElement) -> Result<bool, RoomError> {
        let room = element.next_room.expect("the element has a next room");
        let control = self.s.control;
        let layer = self.s.room(room).layer();
        let layer_index = layer as usize;
        let layer_active = control.layer_active[layer_index];
        // A disabled signal layer, or a power layer without a plane of this
        // net (Java `7f56b062d`, Q01.7): a start or target item's copper
        // there (a via or through-hole pin) must not lead to a trace on it.
        if !layer_active
            && (self.s.board.layer_structure.layers[layer_index].is_signal
                || !control.own_plane_layers[layer_index])
        {
            return Ok(true);
        }
        let mut half_width = f64::from(self.s.room_compensated_half_width(room, layer_index));
        let mut door_is_small = false;
        if let ExpandableRef::Door(door) = element.door {
            let mut half_width_add = half_width + TRACE_WIDTH_TOLERANCE;
            if control.with_neckdown {
                // A neck-down at a destination pin.
                let neck_down = self.check_neckdown_at_dest_pin(room);
                if neck_down > 0.0 {
                    half_width_add = min_f64(half_width_add, neck_down);
                    half_width = half_width_add;
                }
            }
            door_is_small = self.door_is_small(door, 2.0 * half_width_add)?;
        }
        self.complete_neighbours(room)?;
        let entry_middle = element.shape_entry.a.middle_point(element.shape_entry.b);
        if control.with_neckdown
            && let ExpandableRef::TargetDoor(door) = element.door
        {
            // A neck-down at a start pin.
            let item = self.s.target_door(door).item;
            if self.s.board.item(item).is_some_and(|i| i.is_pin()) {
                // `ctrl.neckdownHalfWidth` (46bf9e2ef): the pin's own width,
                // floored by `router.min_trace_width_um` when set.
                let neck_down = f64::from(control.neckdown_half_width(self.s.board, item, layer));
                if neck_down > 0.0 {
                    half_width = min_f64(half_width, neck_down);
                }
            }
        }
        let next_room_is_thick =
            self.next_room_is_thick(element, room, half_width, door_is_small, entry_middle)?;
        if !layer_active
            && let ExpandableRef::Drill(drill) = element.door
            && self.drill_to_foreign_conduction_area(drill, layer)
        {
            // A drill to a foreign conduction area on a split plane.
            return Ok(true);
        }
        let mut something_expanded =
            self.expand_to_target_doors(element, next_room_is_thick, door_is_small, entry_middle)?;
        if !layer_active {
            return Ok(true);
        }
        let mut ripup_costs = 0;
        match self.s.room(room) {
            Room::CompleteFree { .. } | Room::Incomplete { .. } => {
                if !element.already_checked && door_is_small {
                    let mut enter_through_small_door = false;
                    if next_room_is_thick {
                        // Enter the thick room from a ripped item through a
                        // small door.
                        enter_through_small_door = self.check_leaving_ripped_item(element)?;
                    }
                    if !enter_through_small_door {
                        return Ok(something_expanded);
                    }
                }
            }
            Room::Obstacle { item, .. } => {
                let item = *item;
                if !element.already_checked {
                    match self.obstacle_room_entry(
                        element,
                        room,
                        item,
                        door_is_small,
                        next_room_is_thick,
                    )? {
                        ObstacleEntry::Enter(costs) => ripup_costs = costs,
                        ObstacleEntry::Return(occupy) => {
                            return Ok(occupy || something_expanded);
                        }
                    }
                }
            }
        }
        // PARITY: MazeSearchEngine.java:573 iterates a copy of the doors.
        let doors = self.s.room(room).doors().to_vec();
        for door in doors {
            if element.door == ExpandableRef::Door(door) {
                continue;
            }
            if self.expand_to_door(
                door,
                element,
                ripup_costs,
                next_room_is_thick,
                Adjustment::None,
            )? {
                something_expanded = true;
            }
        }
        // The drill pages intersecting the room, or the via's drill.
        if control.vias_allowed && !matches!(element.door, ExpandableRef::Drill(_)) {
            let is_free = matches!(self.s.room(room), Room::CompleteFree { .. });
            if (something_expanded || next_room_is_thick) && is_free {
                let shape = self.room_shape(room)?.clone();
                for page in self.s.overlapping_drill_pages(&shape) {
                    self.expand_to_drill_page(page, element);
                    something_expanded = true;
                }
            } else if let Room::Obstacle { item, .. } = self.s.room(room) {
                let item = *item;
                if self.s.board.item(item).is_some_and(|i| i.is_via()) {
                    let drill = self.s.via_drill(item)?.ok_or(RoomError::NullPointer {
                        at: "Via.getAutorouteDrillInfo",
                    })?;
                    self.expand_to_drill(drill, element, ripup_costs)?;
                }
            }
        }
        Ok(something_expanded)
    }

    /// The obstacle room part of `expandToRoomDoors` (`:527-570`) for an
    /// element not checked before: the ripup costs to enter, or the value
    /// to return now (`Return(true)` is Java's `return true`,
    /// `Return(false)` its `return somethingExpanded`).
    fn obstacle_room_entry(
        &mut self,
        element: &MazeListElement,
        room: RoomId,
        item: ItemId,
        door_is_small: bool,
        next_room_is_thick: bool,
    ) -> Result<ObstacleEntry, RoomError> {
        let control = self.s.control;
        let layer = self.s.room(room).layer();
        let mut ripup_costs = 0;
        let mut room_rippable = false;
        if control.ripup_allowed {
            ripup_costs = self.check_ripup(element, item, door_is_small)?;
            room_rippable = ripup_costs >= 0;
        }
        let is_trace = self.s.board.item(item).is_some_and(|i| i.is_trace());
        if ripup_costs != ALREADY_RIPPED_COSTS
            && next_room_is_thick
            && !door_is_small
            && control.max_shove_trace_recursion_depth > 0
            && is_trace
            && !self.shove_trace_room(element, room)?
        {
            let blocker = self.s.board.shove_failing_obstacle().unwrap_or(item);
            self.blocked(blocker, layer, BlockReason::ShoveFailed);
            if ripup_costs > 0 {
                // Delay the occupation by ripup to allow shoving the room
                // from another door section.
                let mut delayed = element.clone();
                delayed.expansion_value = element.expansion_value + f64::from(ripup_costs);
                delayed.sorting_value = element.sorting_value + f64::from(ripup_costs);
                delayed.room_ripped = true;
                delayed.already_checked = true;
                delayed.ripup_cost = ripup_costs;
                self.push(delayed)?;
            }
            return Ok(ObstacleEntry::Return(false));
        }
        if !room_rippable {
            self.blocked(item, layer, BlockReason::NotRippable);
            return Ok(ObstacleEntry::Return(true));
        }
        Ok(ObstacleEntry::Enter(ripup_costs))
    }

    /// The thickness test of `expandToRoomDoors` (`:467-491`).
    fn next_room_is_thick(
        &self,
        element: &MazeListElement,
        room: RoomId,
        half_width: f64,
        door_is_small: bool,
        entry_middle: FloatPoint,
    ) -> Result<bool, RoomError> {
        if let Room::Obstacle { .. } = self.s.room(room) {
            return Ok(self.room_shape_is_thick(room));
        }
        let shape = self.room_shape(room)?;
        if shape.min_width() < 2.0 * half_width {
            // To prevent problems with the opposite side.
            return Ok(false);
        }
        if !element.already_checked
            && self.s.expandable_dimension(element.door) == 1
            && !door_is_small
        {
            // Only for 1-dimensional doors is the entry on the border of
            // the room shape; small doors are checked when leaving.
            let nearest = shape.nearest_border_points_approx(entry_middle, 2);
            if nearest.len() < 2 {
                // Java warns "nearestPoints.length == 2 expected".
                return Ok(false);
            }
            return Ok(nearest[1].distance(entry_middle) > half_width + 1.0);
        }
        Ok(true)
    }

    /// The split-plane check of `expandToRoomDoors` (`:492-503`): an item
    /// of another net among the conduction areas at the drill.
    fn drill_to_foreign_conduction_area(&self, drill: super::drills::DrillId, layer: i32) -> bool {
        let location = self.s.drill(drill).location.clone();
        let filter = ItemFilter::only(SelectableChoice::Conduction);
        let board = &*self.s.board;
        let net = self.s.control.net_number;
        board
            .pick_items_filtered(&location, layer, Some(&filter))
            .into_iter()
            .any(|id| board.item(id).is_some_and(|i| !i.contains_net(net)))
    }

    /// Q13 (`router.power_layer_traces = own_plane`): false if expanding
    /// from `element` to `connection_point` on `layer` would lay a trace on
    /// a power layer outside the net's own planes (under `none`, N05: any
    /// trace there, [`hop_allowed`](crate::autoroute::power_layer_traces::hop_allowed)).
    /// True under Java's rule
    /// and on signal layers; otherwise the entry segment and the connection
    /// point must lie in one convex piece of one of the net's planes on
    /// `layer`, in the board's own geometry
    /// ([`points_in_one_piece`](crate::autoroute::power_layer_traces::points_in_one_piece)).
    /// A hop without length is no exception: the search's target shapes
    /// are enlarged by the clearance compensation, so a via just outside
    /// the plane meets it with no length, and the path's insertion then
    /// lays a trace from the via into the plane (m-fc-g474, Q13.3).
    fn trace_stays_in_own_plane(
        &self,
        layer: i32,
        element: &MazeListElement,
        connection_point: FloatPoint,
    ) -> bool {
        let Some(own) = self.s.control.own_plane_areas.as_ref() else {
            return true;
        };
        let board = &*self.s.board;
        if board.layer_structure.layers[layer as usize].is_signal {
            return true;
        }
        let points = [
            element.shape_entry.a.round().into(),
            element.shape_entry.b.round().into(),
            connection_point.round().into(),
        ];
        let areas = own.get(layer as usize).map_or(&[][..], Vec::as_slice);
        let mode = self.s.control.power_layer_traces;
        crate::autoroute::power_layer_traces::hop_allowed(mode, board, areas, &points)
    }

    /// `completeNeighbourRooms` with the hook for each completed room.
    fn complete_neighbours(&mut self, room: RoomId) -> Result<(), RoomError> {
        let before = self.s.complete.len();
        self.s.complete_neighbour_rooms(room)?;
        sink!({
            let completed: Vec<RoomId> = self.s.complete.get(before..).unwrap_or(&[]).to_vec();
            for room in completed {
                self.sink.on_room_completed(self.s, room);
            }
        });
        let _ = before;
        Ok(())
    }

    /// `expandToTargetDoors` (`:645-721`): true if a target door was
    /// expanded.
    fn expand_to_target_doors(
        &mut self,
        element: &MazeListElement,
        next_room_is_thick: bool,
        door_is_small: bool,
        entry_middle: FloatPoint,
    ) -> Result<bool, RoomError> {
        let room = element.next_room.expect("the element has a next room");
        if door_is_small {
            // Entering through the small door is only checked when coming
            // from an obstacle room.
            let from_obstacle = match element.door {
                ExpandableRef::Door(door) => self
                    .s
                    .other_complete_room(door, room)
                    .is_some_and(|r| matches!(self.s.room(r), Room::Obstacle { .. })),
                _ => false,
            };
            if !from_obstacle {
                return Ok(false);
            }
        }
        let control = self.s.control;
        let layer = self.s.room(room).layer();
        let mut result = false;
        for door in self.s.room(room).target_doors().to_vec() {
            if element.door == ExpandableRef::TargetDoor(door) || self.s.is_ignored_door(door) {
                continue;
            }
            let (item, entry) = {
                let target = self.s.target_door(door);
                (target.item, target.tree_entry)
            };
            if !self.s.pin_layer_allowed(item, layer) {
                // K15.42: a lone pin is entered only on its pour's layer.
                continue;
            }
            let target_shape = {
                let board = &*self.s.board;
                let tree = self.s.tree();
                let count = board
                    .item(item)
                    .map_or(0, |i| tree.tree_shape_count(i, &board.shape_context()));
                if entry >= count {
                    // The index is stale (the trace changed during routing).
                    continue;
                }
                board.trace_connection_shape(tree, item, entry)
            };
            let Some(target_shape) = target_shape else {
                continue;
            };
            let in_fill = self.s.pour_fill.contains_key(&item);
            let connection_point = if in_fill {
                // K15.25 (`router.pour_fill_target`): a pour is reached only
                // where its opened fill meets the room.
                let in_room = target_shape.intersection(self.room_shape(room)?);
                let parts = self.s.pour_fill_parts(item, &in_room).unwrap_or_default();
                match crate::autoroute::pour_target::nearest_point(&parts, entry_middle) {
                    Some(point) => point,
                    None => continue,
                }
            } else {
                let Some(point) = target_shape.nearest_point_approx(entry_middle) else {
                    return Err(RoomError::NullPointer {
                        at: "MazeSearchEngine.expandToTargetDoors",
                    });
                };
                point
            };
            if !self.trace_stays_in_own_plane(layer, element, connection_point) {
                // Q13 (`own_plane`): the hop would lay a trace on a power
                // layer outside the net's own planes there.
                continue;
            }
            if !next_room_is_thick {
                // Check the line from the entry to the nearest point.
                let from = entry_middle.round();
                let to = connection_point.round();
                if from != to {
                    let polyline =
                        freerouting_geometry::Polyline::from_points(&[from.into(), to.into()]);
                    let half_width = self.s.room_half_width(room, layer as usize);
                    let ok = self.s.board.check_forced_trace_polyline(
                        &polyline,
                        half_width,
                        layer,
                        &[control.net_number],
                        control.trace_clearance_class_index,
                        control.max_shove_trace_recursion_depth,
                        control.max_shove_via_recursion_depth,
                        control.max_spring_over_recursion_depth,
                    );
                    if !ok {
                        if let Some(blocker) = self.s.board.shove_failing_obstacle() {
                            self.blocked(blocker, layer, BlockReason::ForcedTraceCheckFailed);
                        }
                        continue;
                    }
                }
            }
            let entry_line = FloatLine::new(connection_point, connection_point);
            if self.expand_to_door_section(
                ExpandableRef::TargetDoor(door),
                0,
                Some(entry_line),
                element,
                0,
                Adjustment::None,
            )? {
                result = true;
            }
        }
        Ok(result)
    }

    /// `expandToDoor` (`:723-777`): true if a section of `to_door` was
    /// expanded.
    pub(crate) fn expand_to_door(
        &mut self,
        to_door: DoorId,
        element: &MazeListElement,
        add_costs: i32,
        next_room_is_thick: bool,
        adjustment: Adjustment,
    ) -> Result<bool, RoomError> {
        let room = element.next_room.expect("the element has a next room");
        let layer = self.s.room(room).layer();
        let half_width = f64::from(self.s.room_compensated_half_width(room, layer as usize));
        let mut something_expanded = false;
        // PARITY(quirk): may reallocate the door's sections (A02.5).
        let sections = self.s.section_segments(to_door, half_width)?;
        let dimension = self.s.door(to_door).dimension;
        for (i, section) in sections.iter().enumerate() {
            let occupied = self
                .s
                .door_sections(to_door)
                .get(i)
                .ok_or(RoomError::IndexOutOfBounds {
                    at: "MazeSearchEngine.expandToDoor",
                })?
                .occupied;
            if occupied {
                continue;
            }
            let entry = if next_room_is_thick {
                if dimension == 1 && sections.len() == 1 && self.both_rooms_free(to_door) {
                    // Entering the door at an acute corner of the room.
                    let middle = section.a.middle_point(section.b);
                    let shape = self.room_shape(room)?;
                    if shape.min_width() < 2.0 * half_width {
                        return Ok(false);
                    }
                    let nearest = shape.nearest_border_points_approx(middle, 2);
                    if nearest.len() < 2 || nearest[1].distance(middle) <= half_width + 1.0 {
                        return Ok(false);
                    }
                }
                Some(*section)
            } else {
                // Only doors on the opposite side of the room from the
                // entry.
                if dimension == 1 && i == 0 && sections[0].b.distance_square(sections[0].a) < 1.0 {
                    // A small door of a via or thin room.
                    continue;
                }
                match segment_projection(&element.shape_entry, section) {
                    Some(entry) => Some(entry),
                    None => continue,
                }
            };
            if self.expand_to_door_section(
                ExpandableRef::Door(to_door),
                i,
                entry,
                element,
                add_costs,
                adjustment,
            )? {
                something_expanded = true;
            }
        }
        Ok(something_expanded)
    }

    fn both_rooms_free(&self, door: DoorId) -> bool {
        let [first, second] = self.s.door(door).rooms;
        matches!(self.s.room(first), Room::CompleteFree { .. })
            && matches!(self.s.room(second), Room::CompleteFree { .. })
    }

    /// `doorIsSmall` (`:779-807`): the door is narrower than `trace_width`.
    pub(crate) fn door_is_small(&self, door: DoorId, trace_width: f64) -> Result<bool, RoomError> {
        if self.s.door(door).dimension != 1 && !self.both_rooms_free(door) {
            return Ok(false);
        }
        let shape = self.s.door_shape(door)?;
        if shape.is_empty() {
            return Ok(true);
        }
        let length = match self.s.board.rules.trace_angle_restriction() {
            AngleRestriction::NinetyDegree => shape.bounding_box().max_width(),
            AngleRestriction::FortyfiveDegree => shape
                .bounding_octagon()
                .ok_or(RoomError::NullPointer {
                    at: "MazeSearchEngine.doorIsSmall",
                })?
                .max_width(),
            AngleRestriction::None => {
                let line = shape
                    .diagonal_corner_segment()
                    .ok_or(RoomError::NullPointer {
                        at: "MazeSearchEngine.doorIsSmall",
                    })?;
                line.b.distance(line.a)
            }
        };
        Ok(length < trace_width)
    }

    /// `expandToDoorSection` (`:809-989`): queues section `section` of
    /// `door`, entered at `shape_entry` from `from`. True if an element was
    /// offered to the queue (Java returns true even when the queue drops
    /// it).
    pub(crate) fn expand_to_door_section(
        &mut self,
        door: ExpandableRef,
        section: usize,
        shape_entry: Option<FloatLine>,
        from: &MazeListElement,
        add_costs: i32,
        adjustment: Adjustment,
    ) -> Result<bool, RoomError> {
        let occupied = self
            .s
            .section_state(door, section)
            .ok_or(RoomError::IndexOutOfBounds {
                at: "MazeSearchEngine.expandToDoorSection",
            })?
            .occupied;
        let Some(shape_entry) = shape_entry.filter(|_| !occupied) else {
            return Ok(false);
        };
        let from_room = from.next_room.expect("the element has a next room");
        let next_room = self.s.expandable_other_room(door, from_room);
        let layer = self.s.room(from_room).layer();
        let layer_index = layer as usize;
        let control = self.s.control;
        let entry_middle = shape_entry.a.middle_point(shape_entry.b);
        let from_middle = from.shape_entry.a.middle_point(from.shape_entry.b);
        let mut bend_penalty = 0.0;
        if control.bend_costs[layer_index] > 0.0
            && let Some((backtrack, _)) = from.backtrack
        {
            // PARITY: MazeSearchEngine.java:875-893, the direction change
            // from the backtrack door's centre of gravity.
            let cog = self.s.expandable_shape(backtrack)?.centre_of_gravity();
            let prev_dx = from_middle.x - cog.x;
            let prev_dy = from_middle.y - cog.y;
            let next_dx = entry_middle.x - from_middle.x;
            let next_dy = entry_middle.y - from_middle.y;
            let cross = prev_dx * next_dy - prev_dy * next_dx;
            let sq_len_prev = prev_dx * prev_dx + prev_dy * prev_dy;
            let sq_len_next = next_dx * next_dx + next_dy * next_dy;
            if sq_len_prev > 0.0
                && sq_len_next > 0.0
                && cross * cross > 0.01 * sq_len_prev * sq_len_next
            {
                bend_penalty = control.bend_costs[layer_index];
            }
        }
        let costs = &control.trace_costs[layer_index];
        // PARITY: MazeSearchEngine.java:895-902, summed left to right (D6).
        let mut expansion_value = from.expansion_value
            + f64::from(add_costs)
            + bend_penalty
            + entry_middle.weighted_distance(from_middle, costs.horizontal, costs.vertical);
        if C::ENABLED {
            let shape = self.s.expandable_shape(door)?;
            expansion_value += self.extra.door_cost(layer, &shape);
        }
        let sorting_value = expansion_value + self.s.distance.calculate_point(&entry_middle, layer);
        let room_ripped = add_costs > 0 && adjustment == Adjustment::None
            || from.already_checked && from.room_ripped;
        let ripup_cost = if add_costs > 0 && adjustment == Adjustment::None {
            add_costs
        } else {
            0
        };
        let element = MazeListElement {
            door,
            section,
            backtrack: Some((from.door, from.section)),
            expansion_value,
            sorting_value,
            next_room,
            shape_entry,
            room_ripped,
            adjustment,
            already_checked: false,
            ripup_cost,
        };
        self.push(element)?;
        sink!(self.sink.on_door_expanded(self.s, door, section));
        Ok(true)
    }

    /// Offers `element` to the queue under its key.
    pub(crate) fn push(&mut self, element: MazeListElement) -> Result<Push, RoomError> {
        let key = QueueKey::new(
            element.sorting_value,
            element.expansion_value,
            self.s.expandable_java_id(element.door)?,
            element.section as i32,
        );
        let outcome = self.s.queue_element(key, element);
        sink!(self.sink.on_push(self.s, &key, outcome != Push::Added));
        Ok(outcome)
    }

    /// Reports a blocking item to the sink.
    pub(crate) fn blocked(&mut self, item: ItemId, layer: i32, reason: BlockReason) {
        sink!({
            let net = self
                .s
                .board
                .item(item)
                .and_then(|i| i.net_numbers().first().copied())
                .unwrap_or(0);
            let blocked = Blocked {
                item,
                net,
                layer,
                reason,
            };
            self.sink.on_blocked(self.s, &blocked)
        });
        let _ = (item, layer, reason);
    }

    /// `roomShapeIsThick` (`:1127-1150`): the obstacle is at least as wide as
    /// the compensated trace.
    fn room_shape_is_thick(&self, room: RoomId) -> bool {
        let Room::Obstacle { item, layer, .. } = self.s.room(room) else {
            return true;
        };
        let (item, layer) = (*item, *layer);
        let board = &*self.s.board;
        let tree = self.s.tree();
        let Some(obstacle) = board.item(item) else {
            return false;
        };
        let half_width = if let Some(trace) = obstacle.as_trace() {
            f64::from(
                trace.half_width
                    + tree.clearance_compensation_value(
                        &board.rules.clearance_matrix,
                        obstacle.clearance_class(),
                        layer,
                    ),
            )
        } else if let Some(drill) = obstacle.as_via().map(|v| &v.drill) {
            let index = (layer - drill.first_layer) as usize;
            tree.tree_shape(obstacle, index, &board.shape_context())
                .map_or(0.0, |shape| 0.5 * shape.max_width())
        } else {
            // Java warns "unexpected obstacle item".
            0.0
        };
        half_width >= f64::from(self.s.room_compensated_half_width(room, layer as usize))
    }

    /// `checkNeckDownAtDestPin` (`:1229-1243`): the neck-down half width of
    /// the first pin among the room's target doors, 0 for none.
    fn check_neckdown_at_dest_pin(&self, room: RoomId) -> f64 {
        let layer = self.s.room(room).layer();
        for &door in self.s.room(room).target_doors() {
            let item = self.s.target_door(door).item;
            if self.s.board.item(item).is_some_and(|i| i.is_pin()) {
                return f64::from(
                    self.s
                        .control
                        .neckdown_half_width(self.s.board, item, layer),
                );
            }
        }
        0.0
    }

    pub(crate) fn room_shape(&self, room: RoomId) -> Result<&TileShape, RoomError> {
        self.s.room(room).shape().ok_or(RoomError::NullPointer {
            at: "ExpansionRoom.getShape",
        })
    }
}

/// The obstacle room part of `expandToRoomDoors`.
enum ObstacleEntry {
    /// Expand the room's doors with these ripup costs.
    Enter(i32),
    /// Return now: true is Java's `return true`, false its `return
    /// somethingExpanded`.
    Return(bool),
}

/// Java `MazeSearchEngine.segmentProjection` (`:185-216`): the
/// perpendicular projection of `from` onto `to`, `None` if it is empty.
///
/// Java's identity tests (`firstProjection.a == toSegment.a`) are true
/// exactly when a projection took the end point of `to` itself rather than
/// a computed point; the conditions below are the branches of
/// `FloatLine.segmentProjection` and `segmentProjection2` that do.
pub fn segment_projection(from: &FloatLine, to: &FloatLine) -> Option<FloatLine> {
    let check = from.adjust_direction(to);
    let first = to.segment_projection(&check);
    let second = to.segment_projection_2(&check);
    match (first, second) {
        (Some(first), Some(second)) => {
            // PARITY: identity of the end points, FloatLine.java's branches.
            let first_a_is_end = to.a.scalar_product(to.b, check.a) < 0.0;
            let first_b_is_end = to.b.scalar_product(to.a, check.b) < 0.0;
            let second_a_is_end = !is_negative(check.a.scalar_product(check.b, to.a));
            let second_b_is_end = !is_negative(check.b.scalar_product(check.a, to.b));
            let a = if first_a_is_end || second_a_is_end {
                to.a
            } else if first.a.distance_square(to.a) <= second.a.distance_square(to.a) {
                first.a
            } else {
                second.a
            };
            let b = if first_b_is_end || second_b_is_end {
                to.b
            } else if first.b.distance_square(to.b) <= second.b.distance_square(to.b) {
                first.b
            } else {
                second.b
            };
            Some(FloatLine::new(a, b))
        }
        (Some(first), None) => Some(first),
        (None, second) => second,
    }
}

/// Java's `x < 0` on a double (false for NaN).
fn is_negative(x: f64) -> bool {
    x < 0.0
}

impl Session<'_> {
    /// The slot of section `section` of `object`; `None` if it has no such
    /// section (Java's array index exception).
    pub(crate) fn section_slot(
        &self,
        object: ExpandableRef,
        section: usize,
    ) -> Option<SectionSlot> {
        let (start, len) = match object {
            ExpandableRef::TargetDoor(door) => {
                return (section == 0).then_some(SectionSlot::Target(door));
            }
            ExpandableRef::Door(door) => self.door(door).sections,
            ExpandableRef::Drill(drill) => self.drill(drill).sections,
            ExpandableRef::DrillPage(page) => self.built_drill_pages()?.page(page).sections,
        };
        (section < len as usize).then(|| SectionSlot::Flat(start as usize + section))
    }

    /// Occupy a surviving section after expansion may have resized its door.
    /// A section removed by shrinking no longer has a state to occupy.
    pub(crate) fn occupy_current_section(&mut self, object: ExpandableRef, section: usize) {
        if let Some(slot) = self.section_slot(object, section) {
            self.slot_state_mut(slot).occupied = true;
        }
    }

    pub(crate) fn slot_state(&self, slot: SectionSlot) -> &super::SectionState {
        match slot {
            SectionSlot::Flat(index) => &self.sections[index],
            SectionSlot::Target(door) => &self.target_door(door).section,
        }
    }

    pub(crate) fn slot_state_mut(&mut self, slot: SectionSlot) -> &mut super::SectionState {
        match slot {
            SectionSlot::Flat(index) => &mut self.sections[index],
            SectionSlot::Target(door) => &mut self.target_doors[door.0 as usize].section,
        }
    }

    /// Java `getMazeSearchElement(section)` of an expandable object;
    /// `None` out of range.
    pub fn section_state(
        &self,
        object: ExpandableRef,
        section: usize,
    ) -> Option<&super::SectionState> {
        self.section_slot(object, section)
            .map(|slot| self.slot_state(slot))
    }

    /// Java `mazeSearchElementCount`.
    pub fn section_count(&self, object: ExpandableRef) -> usize {
        match object {
            ExpandableRef::TargetDoor(_) => 1,
            ExpandableRef::Door(door) => self.door(door).sections.1 as usize,
            ExpandableRef::Drill(drill) => self.drill(drill).sections.1 as usize,
            ExpandableRef::DrillPage(page) => self
                .built_drill_pages()
                .map_or(0, |p| p.page(page).sections.1 as usize),
        }
    }

    /// Java `getShape` of an expandable object (a door's is computed now).
    pub fn expandable_shape(&self, object: ExpandableRef) -> Result<TileShape, RoomError> {
        match object {
            ExpandableRef::Door(door) => self.door_shape(door),
            ExpandableRef::TargetDoor(door) => Ok(self.target_door(door).shape.clone()),
            ExpandableRef::Drill(drill) => Ok(self.drill(drill).shape.clone()),
            ExpandableRef::DrillPage(page) => self
                .built_drill_pages()
                .map(|p| TileShape::IntBox(p.page(page).shape))
                .ok_or(RoomError::NullPointer {
                    at: "DrillPage.getShape",
                }),
        }
    }

    /// Java `getDimension`: a door's dimension, 2 for the others.
    pub fn expandable_dimension(&self, object: ExpandableRef) -> i32 {
        match object {
            ExpandableRef::Door(door) => self.door(door).dimension,
            _ => 2,
        }
    }

    /// Java `otherRoom(CompleteExpansionRoom)`: the other complete room of
    /// a door, `None` for the other objects.
    pub fn expandable_other_room(&self, object: ExpandableRef, room: RoomId) -> Option<RoomId> {
        match object {
            ExpandableRef::Door(door) => self.other_complete_room(door, room),
            _ => None,
        }
    }
}

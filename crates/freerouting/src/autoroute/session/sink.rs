//! The hooks of the maze search (A04.1).
//!
//! The search is one core, [`MazeSearch`](super::search::MazeSearch),
//! generic over a [`SearchSink`]. Every caller steps that core: the batch
//! router with [`NoopSink`], the probes with [`ProbeSink`], diagnostics with
//! [`FailureRecord`], statistics with [`WorkCounters`] and the GUI's live
//! view with [`ProgressEvents`]. [`Tee`] combines two sinks.
//!
//! Every hook has an empty `#[inline]` default and receives the session for
//! reading, so a sink that needs a Java id or a room shape looks it up
//! itself and the core computes nothing for the sinks that do not. Every
//! call site goes through the `sink!` macro, which the custom cfg
//! `freerouting_no_search_hooks` compiles to nothing; that build exists only
//! to measure what the hooks cost (A04.8).
//!
//! A sink only observes. Nothing it does reaches the search: the search
//! takes the same steps, in the same order, with every sink.

use std::sync::mpsc::Sender;

use freerouting_geometry::{FloatPoint, TileShape};

use super::drills::DrillPageId;
use super::queue::{MazeListElement, QueueKey};
use super::{ExpandableRef, RoomId, Session};
use crate::autoroute::probe::{ProbeValue, record_line};
use crate::board::items::ItemId;

/// Calls a sink hook unless the crate is built with
/// `--cfg freerouting_no_search_hooks`.
macro_rules! sink {
    ($($call:tt)*) => {
        #[cfg(not(freerouting_no_search_hooks))]
        {
            $($call)*;
        }
    };
}
pub(crate) use sink;

/// What [`MazeSearch::step`](super::search::MazeSearch::step) did (Java
/// `occupyNextElement`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// An element was expanded (Java returned true).
    Continue,
    /// A destination was reached.
    Found,
    /// The queue holds no unoccupied element.
    Exhausted,
    /// A stop was requested or the work limit was exceeded.
    Stopped,
}

/// The element a step pops, after the occupied ones before it.
#[derive(Debug)]
pub struct PopInfo<'a> {
    /// The number of the step, from 0.
    pub step: u64,
    pub key: QueueKey,
    pub element: &'a MazeListElement,
    /// The elements of occupied sections popped before it in this step.
    pub skipped_occupied: u32,
}

/// The end of one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepEnd {
    pub step: u64,
    /// The door and section popped, if an element was popped.
    pub popped: Option<(ExpandableRef, usize)>,
    /// The popped section is occupied now (read through the door's current
    /// section array, as `getMazeSearchElement` reads it).
    pub section_occupied: bool,
    /// The elements of occupied sections popped in this step.
    pub skipped_occupied: u32,
    pub outcome: Step,
}

/// Why the search could not pass an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlockReason {
    /// The obstacle room's item may not be ripped (`checkRipup` gave -1 or
    /// ripup is off).
    NotRippable,
    /// Shoving the obstacle trace failed (`shoveTraceRoom` gave false).
    ShoveFailed,
    /// `checkForcedTracePolyline` to a target in a thin room failed.
    ForcedTraceCheckFailed,
    /// `ForcedViaInserter.checkLayer` found no via of the rule drillable.
    ViaCheckFailed,
}

/// A door, room or layer change refused because of an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blocked {
    /// The item in the way: the obstacle room's item, or the board's
    /// shove-failing obstacle of the failed check.
    pub item: ItemId,
    /// The first net of the item, 0 for none.
    pub net: i32,
    pub layer: i32,
    pub reason: BlockReason,
}

/// How the search ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchEnd {
    pub outcome: Step,
    /// The destination door and section, if one was reached.
    pub destination: Option<(ExpandableRef, usize)>,
    /// The steps taken.
    pub steps: u64,
}

/// The hooks of the maze search. Every method has an empty default.
pub trait SearchSink {
    /// The search was created (before its initialisation).
    #[inline]
    fn on_start(&mut self, _session: &Session<'_>) {}

    /// A step popped the element it expands.
    #[inline]
    fn on_pop(&mut self, _session: &Session<'_>, _info: &PopInfo<'_>) {}

    /// An element was offered to the queue; `dropped` if the queue refused
    /// it (the fanout filter or a full tie with a queued element).
    #[inline]
    fn on_push(&mut self, _session: &Session<'_>, _key: &QueueKey, _dropped: bool) {}

    /// A room was completed while completing the neighbours of a room.
    #[inline]
    fn on_room_completed(&mut self, _session: &Session<'_>, _room: RoomId) {}

    /// A door section was expanded (an element for it was queued).
    #[inline]
    fn on_door_expanded(&mut self, _session: &Session<'_>, _door: ExpandableRef, _section: usize) {}

    /// The drills of a page were read; `computed` of them were created by
    /// this read.
    #[inline]
    fn on_drill_page(&mut self, _session: &Session<'_>, _page: DrillPageId, _computed: usize) {}

    /// The search could not pass an item.
    #[inline]
    fn on_blocked(&mut self, _session: &Session<'_>, _blocked: &Blocked) {}

    /// A step ended.
    #[inline]
    fn on_step_end(&mut self, _session: &Session<'_>, _end: &StepEnd) {}

    /// The search ended.
    #[inline]
    fn on_finish(&mut self, _session: &Session<'_>, _end: &SearchEnd) {}
}

/// The sink of the batch router: no hooks.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopSink;

impl SearchSink for NoopSink {}

/// Counters of the search's work (A04.1; P01.2 may add a statistics sink
/// beside it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkCounters {
    /// Steps that popped an element.
    pub pops: u64,
    /// Elements of occupied sections popped and skipped.
    pub skipped_occupied: u64,
    /// Elements offered to the queue.
    pub pushes: u64,
    /// Elements the queue refused (fanout filter or full tie).
    pub dropped: u64,
    pub rooms_completed: u64,
    pub doors_expanded: u64,
    /// Drills created by page reads.
    pub drills_computed: u64,
    /// Work meter units at the start (the meter value, not a count).
    start_units: u64,
    /// Work meter units the search consumed, from its start to its end
    /// (B02.7's meter delta, the D5 unit).
    pub work_units: u64,
}

impl SearchSink for WorkCounters {
    fn on_start(&mut self, session: &Session<'_>) {
        self.start_units = session.board().search_trees().work_meter().units();
    }

    fn on_pop(&mut self, _session: &Session<'_>, info: &PopInfo<'_>) {
        self.pops += 1;
        self.skipped_occupied += u64::from(info.skipped_occupied);
    }

    fn on_push(&mut self, _session: &Session<'_>, _key: &QueueKey, dropped: bool) {
        self.pushes += 1;
        if dropped {
            self.dropped += 1;
        }
    }

    fn on_room_completed(&mut self, _session: &Session<'_>, _room: RoomId) {
        self.rooms_completed += 1;
    }

    fn on_door_expanded(&mut self, _session: &Session<'_>, _door: ExpandableRef, _section: usize) {
        self.doors_expanded += 1;
    }

    fn on_drill_page(&mut self, _session: &Session<'_>, _page: DrillPageId, computed: usize) {
        self.drills_computed += computed as u64;
    }

    fn on_step_end(&mut self, _session: &Session<'_>, end: &StepEnd) {
        if end.popped.is_none() {
            self.skipped_occupied += u64::from(end.skipped_occupied);
        }
    }

    fn on_finish(&mut self, session: &Session<'_>, _end: &SearchEnd) {
        let units = session.board().search_trees().work_meter().units();
        self.work_units = units.saturating_sub(self.start_units);
    }
}

/// The items that blocked the search (A04.1), for Q03 and Q05.
#[derive(Clone, Debug)]
pub struct FailureRecord {
    /// The most records kept by [`finish`](Self::finish).
    pub cap: usize,
    records: Vec<Blocked>,
}

impl Default for FailureRecord {
    fn default() -> Self {
        FailureRecord::new(FailureRecord::DEFAULT_CAP)
    }
}

impl FailureRecord {
    /// The default cap.
    pub const DEFAULT_CAP: usize = 64;

    pub fn new(cap: usize) -> FailureRecord {
        FailureRecord {
            cap,
            records: Vec::new(),
        }
    }

    /// Adds a record.
    pub fn push(&mut self, blocked: Blocked) {
        self.records.push(blocked);
    }

    /// The records so far, in the order they were found.
    pub fn raw(&self) -> &[Blocked] {
        &self.records
    }

    /// The records sorted by `(blocker net, blocker id, layer, reason)`,
    /// without duplicates, cut to the first `cap`. The order the records
    /// were found in does not matter.
    pub fn finish(&self) -> Vec<Blocked> {
        let mut records = self.records.clone();
        records.sort_by_key(|b| (b.net, b.item.0, b.layer, b.reason));
        records.dedup();
        records.truncate(self.cap);
        records
    }
}

impl SearchSink for FailureRecord {
    fn on_blocked(&mut self, _session: &Session<'_>, blocked: &Blocked) {
        self.push(*blocked);
    }
}

/// The search state sent by [`ProgressEvents`] (D7).
#[derive(Clone, Debug)]
pub struct SearchProgress {
    /// Steps that popped an element.
    pub pops: u64,
    pub queue_len: usize,
    /// The complete rooms of the session.
    pub rooms: usize,
    /// The layer of the room the popped element leads to, -1 for none.
    pub layer: i32,
    /// That room's shape.
    pub room_shape: Option<TileShape>,
}

/// Sends a [`SearchProgress`] every `every` pops over a channel (D7). The
/// cadence counts pops, never time. A closed channel is ignored.
#[derive(Debug)]
pub struct ProgressEvents {
    every: u64,
    pops: u64,
    sender: Sender<SearchProgress>,
}

impl ProgressEvents {
    /// Events every `every` pops (at least 1).
    pub fn new(every: u64, sender: Sender<SearchProgress>) -> ProgressEvents {
        ProgressEvents {
            every: every.max(1),
            pops: 0,
            sender,
        }
    }
}

impl SearchSink for ProgressEvents {
    fn on_pop(&mut self, session: &Session<'_>, info: &PopInfo<'_>) {
        self.pops += 1;
        if self.pops % self.every != 0 {
            return;
        }
        let room = info.element.next_room.map(|r| session.room(r));
        let _ = self.sender.send(SearchProgress {
            pops: self.pops,
            queue_len: session.queue().len(),
            rooms: session.complete_rooms().len(),
            layer: room.map_or(-1, |r| r.layer()),
            room_shape: room.and_then(|r| r.shape().cloned()),
        });
    }
}

/// Both sinks, `A` first.
#[derive(Clone, Debug, Default)]
pub struct Tee<A, B>(pub A, pub B);

impl<A: SearchSink, B: SearchSink> SearchSink for Tee<A, B> {
    #[inline]
    fn on_start(&mut self, s: &Session<'_>) {
        self.0.on_start(s);
        self.1.on_start(s);
    }

    #[inline]
    fn on_pop(&mut self, s: &Session<'_>, info: &PopInfo<'_>) {
        self.0.on_pop(s, info);
        self.1.on_pop(s, info);
    }

    #[inline]
    fn on_push(&mut self, s: &Session<'_>, key: &QueueKey, dropped: bool) {
        self.0.on_push(s, key, dropped);
        self.1.on_push(s, key, dropped);
    }

    #[inline]
    fn on_room_completed(&mut self, s: &Session<'_>, room: RoomId) {
        self.0.on_room_completed(s, room);
        self.1.on_room_completed(s, room);
    }

    #[inline]
    fn on_door_expanded(&mut self, s: &Session<'_>, door: ExpandableRef, section: usize) {
        self.0.on_door_expanded(s, door, section);
        self.1.on_door_expanded(s, door, section);
    }

    #[inline]
    fn on_drill_page(&mut self, s: &Session<'_>, page: DrillPageId, computed: usize) {
        self.0.on_drill_page(s, page, computed);
        self.1.on_drill_page(s, page, computed);
    }

    #[inline]
    fn on_blocked(&mut self, s: &Session<'_>, blocked: &Blocked) {
        self.0.on_blocked(s, blocked);
        self.1.on_blocked(s, blocked);
    }

    #[inline]
    fn on_step_end(&mut self, s: &Session<'_>, end: &StepEnd) {
        self.0.on_step_end(s, end);
        self.1.on_step_end(s, end);
    }

    #[inline]
    fn on_finish(&mut self, s: &Session<'_>, end: &SearchEnd) {
        self.0.on_finish(s, end);
        self.1.on_finish(s, end);
    }
}

/// The Java class name of an expandable object.
pub fn expandable_kind(object: ExpandableRef) -> &'static str {
    match object {
        ExpandableRef::Door(_) => "ExpansionDoor",
        ExpandableRef::TargetDoor(_) => "TargetItemExpansionDoor",
        ExpandableRef::Drill(_) => "ExpansionDrill",
        ExpandableRef::DrillPage(_) => "DrillPage",
    }
}

/// `[kind, id, section]` of a door and section, `[]` for none.
pub fn door_value(session: &Session<'_>, door: Option<(ExpandableRef, usize)>) -> ProbeValue {
    match door {
        None => ProbeValue::List(Vec::new()),
        Some((door, section)) => ProbeValue::List(vec![
            expandable_kind(door).into(),
            session.expandable_java_id(door).unwrap_or(0).into(),
            section.into(),
        ]),
    }
}

/// The fields of a `maze-log` step line before the call: the step number
/// and, if the step expands one, the element (Java `mazeSteps` of
/// `AutorouteProbe.java`).
pub fn maze_step_fields(
    session: &Session<'_>,
    step: u64,
    element: Option<&MazeListElement>,
) -> Vec<(&'static str, ProbeValue)> {
    let mut fields: Vec<(&'static str, ProbeValue)> = vec![("step", (step as i64).into())];
    let Some(element) = element else {
        return fields;
    };
    let room = match element.next_room {
        None => ProbeValue::List(Vec::new()),
        Some(room) => {
            let kind = match session.room(room) {
                super::Room::Obstacle { .. } => "ObstacleExpansionRoom",
                super::Room::CompleteFree { .. } => "CompleteFreeSpaceExpansionRoom",
                super::Room::Incomplete { .. } => "IncompleteFreeSpaceExpansionRoom",
            };
            ProbeValue::List(vec![
                kind.into(),
                session.room_java_id(room).unwrap_or(0).into(),
                session.room(room).layer().into(),
            ])
        }
    };
    let adjustment = match element.adjustment {
        super::Adjustment::None => "NONE",
        super::Adjustment::Right => "RIGHT",
        super::Adjustment::Left => "LEFT",
    };
    fields.extend([
        ("kind", expandable_kind(element.door).into()),
        (
            "door",
            session.expandable_java_id(element.door).unwrap_or(0).into(),
        ),
        ("section", element.section.into()),
        (
            "key",
            ProbeValue::bits_list([element.sorting_value, element.expansion_value]),
        ),
        ("room", room),
        ("back", door_value(session, element.backtrack)),
        ("checked", element.already_checked.into()),
        ("adjust", adjustment.into()),
        ("ripped", element.room_ripped.into()),
        ("ripup", element.ripup_cost.into()),
    ]);
    fields
}

/// Writes the step lines of the `maze-log` probe from the hooks (A04.8):
/// the same lines the probe writes by walking the queue between steps.
#[derive(Clone, Debug, Default)]
pub struct ProbeSink {
    start_units: u64,
    pending: Option<Vec<(&'static str, ProbeValue)>>,
    lines: Vec<String>,
}

impl ProbeSink {
    pub fn new() -> ProbeSink {
        ProbeSink::default()
    }

    /// The step lines written so far.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn into_lines(self) -> Vec<String> {
        self.lines
    }

    /// The work meter units at the search's start.
    pub fn start_units(&self) -> u64 {
        self.start_units
    }
}

impl SearchSink for ProbeSink {
    fn on_start(&mut self, session: &Session<'_>) {
        self.start_units = session.board().search_trees().work_meter().units();
    }

    fn on_pop(&mut self, session: &Session<'_>, info: &PopInfo<'_>) {
        self.pending = Some(maze_step_fields(session, info.step, Some(info.element)));
    }

    fn on_step_end(&mut self, session: &Session<'_>, end: &StepEnd) {
        let mut fields = self
            .pending
            .take()
            .unwrap_or_else(|| maze_step_fields(session, end.step, None));
        fields.push(("skipped", i64::from(end.skipped_occupied).into()));
        if end.popped.is_some() {
            fields.push(("occupied", end.section_occupied.into()));
        }
        let units = session.board().search_trees().work_meter().units();
        fields.push((
            "units",
            (units.saturating_sub(self.start_units) as i64).into(),
        ));
        self.lines.push(record_line(&fields));
    }
}

/// The extra cost of entering a door (Q05, X03). Off in parity: with
/// [`NoExtraCost`] the term is not added at all, because adding `0.0` is
/// not bit-safe for `-0.0`.
pub trait ExtraCost {
    /// Whether [`door_cost`](Self::door_cost) is added.
    const ENABLED: bool;

    /// The cost added to the expansion value of a door section on `layer`.
    fn door_cost(&mut self, layer: i32, door_shape: &TileShape) -> f64;

    /// The cost added to the expansion value of the door section entered
    /// at `entry` (the middle of its entry line, where the path crosses
    /// the door) from `from` (the middle of the entry line of the element
    /// expanded, X11.3). The search calls this; the default is
    /// [`door_cost`](Self::door_cost), so a cost that only needs the door
    /// implements that alone.
    #[inline]
    fn section_cost(
        &mut self,
        layer: i32,
        door_shape: &TileShape,
        from: FloatPoint,
        entry: FloatPoint,
    ) -> f64 {
        let _ = (from, entry);
        self.door_cost(layer, door_shape)
    }
}

/// No extra cost (the parity default).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoExtraCost;

impl ExtraCost for NoExtraCost {
    const ENABLED: bool = false;

    #[inline]
    fn door_cost(&mut self, _layer: i32, _door_shape: &TileShape) -> f64 {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_sink_has_zero_size() {
        assert_eq!(std::mem::size_of::<NoopSink>(), 0);
        assert_eq!(std::mem::size_of::<NoExtraCost>(), 0);
        assert_eq!(std::mem::size_of::<Tee<NoopSink, NoopSink>>(), 0);
    }

    fn blocked(item: i32, net: i32, layer: i32, reason: BlockReason) -> Blocked {
        Blocked {
            item: ItemId(item),
            net,
            layer,
            reason,
        }
    }

    /// The finished records are sorted by (net, item id, layer, reason),
    /// without duplicates and cut to the cap, whatever the input order.
    #[test]
    fn failure_record_is_sorted_deduplicated_and_capped_for_any_input_order() {
        let records = [
            blocked(7, 2, 0, BlockReason::ShoveFailed),
            blocked(3, 2, 1, BlockReason::NotRippable),
            blocked(3, 2, 0, BlockReason::NotRippable),
            blocked(9, 1, 0, BlockReason::ViaCheckFailed),
            blocked(3, 2, 0, BlockReason::NotRippable),
            blocked(3, 2, 0, BlockReason::ForcedTraceCheckFailed),
            blocked(12, 5, 1, BlockReason::NotRippable),
        ];
        let mut forward = FailureRecord::new(5);
        let mut backward = FailureRecord::new(5);
        for b in records {
            forward.push(b);
        }
        for b in records.iter().rev() {
            backward.push(*b);
        }
        let expected = vec![
            blocked(9, 1, 0, BlockReason::ViaCheckFailed),
            blocked(3, 2, 0, BlockReason::NotRippable),
            blocked(3, 2, 0, BlockReason::ForcedTraceCheckFailed),
            blocked(3, 2, 1, BlockReason::NotRippable),
            blocked(7, 2, 0, BlockReason::ShoveFailed),
        ];
        assert_eq!(forward.finish(), expected);
        assert_eq!(backward.finish(), expected);
        assert_eq!(forward.raw().len(), 7);
        assert_eq!(FailureRecord::default().cap, 64);
    }
}

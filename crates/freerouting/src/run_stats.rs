//! Per-stage work statistics (P01.2): what a routing run did, counted by
//! stage, with the wall time of each stage.
//!
//! The statistics are **off** unless a caller turns them on with
//! [`enable`] (`freerouting --stats json <file>` does). Off, every hook is
//! one relaxed load of a flag: connection routing takes the same
//! `NoopSink` path as without this module, and [`stage`] returns an inert
//! guard. On, the connection routers step the maze search with the A04
//! sink [`WorkCounters`](crate::autoroute::session::sink::WorkCounters)
//! instead of `NoopSink` (a sink only observes, A04.4), and the search
//! trees' work meter reports each query.
//!
//! **What is counted** ([`Counter`]), summed over every thread (the
//! optimizer's workers included; a candidate's counts are added when the
//! optimizer's merge consumes it, never for a candidate evaluated ahead and
//! dropped, so a total never depends on the thread count):
//! - connections the batch router, the fanout and the optimizer attempted,
//!   routed and failed;
//! - the maze search's queue pops, pushes, refused pushes (the drop-on-equal
//!   and fanout-filter rule), skipped occupied sections, rooms completed,
//!   doors expanded, drills computed, and its work units;
//! - every search-tree query ([`Counter::TreeQueries`], the overlap queries
//!   and room completions together) and the tree nodes they visited
//!   ([`Counter::TreeWorkUnits`], D5's unit, the work limits' meter);
//! - items ripped up by connections.
//! - maze-search work limits a budget policy extended, and the units
//!   granted (Q08's `earned-budget`; always 0 with Java's router).
//!
//! **Stages** ([`stage`]): `load`, `fanout`, one `pass` per autorouter pass
//! followed by its `statistics` (the board statistics the loop scores the
//! pass with, P03.1), one `optimizer_pass` per optimizer pass, `drc` and
//! `write`. A stage's
//! record is the counters' change between its start and its end, plus its
//! wall time. Stages do not nest and run on one thread; the counters they
//! read are process-wide.
//!
//! **Determinism.** The counts of a run are a function of the input and the
//! settings; the wall times are the only part that varies. A test routes a
//! board twice and compares the counts. The statistics change no result: a
//! sink never reaches the search.
//!
//! The JSON ([`report_json`]) has per stage the counts, `wall_seconds` and
//! the unit costs derived from them (microseconds per connection, per room
//! and per tree query; `null` where the divisor is 0), and the same for the
//! whole run.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use crate::autoroute::AttemptState;
use crate::autoroute::session::sink::WorkCounters;
use crate::drc::json::JsonValue;

/// The things that are counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Counter {
    /// Connections handed to the maze search (`route_connection` calls).
    ConnectionsAttempted,
    /// Of those, the ones routed and inserted.
    ConnectionsRouted,
    /// Of those, the ones that failed (no path, not insertable, ...).
    ConnectionsFailed,
    /// Items added to a connection's ripped set.
    RippedItems,
    /// Maze queue pops.
    SearchPops,
    /// Maze queue pushes (offered elements).
    SearchPushes,
    /// Pushes the queue refused (drop-on-equal and the fanout filter).
    SearchDropped,
    /// Elements of occupied sections popped and skipped.
    SearchSkippedOccupied,
    /// Rooms completed.
    RoomsCompleted,
    /// Doors expanded.
    DoorsExpanded,
    /// Drills created by page reads.
    DrillsComputed,
    /// Work units the maze searches consumed (the meter's change).
    SearchWorkUnits,
    /// Connections whose maze-search work limit was exceeded (N03.1).
    MazeWorkLimitHits,
    /// Fanout pins whose per-pin work limit was exceeded (N03.1).
    FanoutWorkLimitHits,
    /// Search-tree queries: overlap queries and room completions.
    TreeQueries,
    /// Leaves those queries returned (N03.1's `leaves` work unit), counted
    /// whatever unit the meter uses.
    TreeLeavesReturned,
    /// Search-tree nodes visited by those queries (D5's work unit).
    TreeWorkUnits,
    /// Existing door section storage replaced with a different count (N06.2).
    DoorSectionReallocations,
    /// Maze-search work limits a budget policy extended (Q08; 0 without one).
    MazeBudgetExtensions,
    /// The units those extensions granted.
    MazeBudgetExtraUnits,
    /// Restores requested for snapshots taken with clearance compensation on (N06.11).
    CompensatedSnapshotRestores,
    /// N06.10 reachability measurement.
    DsnViaAlias,
    /// N06.10 reachability measurement.
    DsnClassClassPairs,
    /// N06.10 reachability measurement.
    DsnShapeBbox,
    /// N06.10 reachability measurement.
    DsnUnitIgnored,
    /// N06.10 reachability measurement.
    RouteResultZeroTestChanges,
    /// N06.7: side-0 in-front-of-pad tests whose result `in_front_of_pad_typo` flips.
    ShoveInFrontOfPadTypoFlips,
    /// N06.7: side-0 in-front-of-pad tests whose result `in_front_of_pad_side0_flank` flips.
    ShoveInFrontOfPadFlankFlips,
    /// N06.7: end-corner contact tests whose result `store_trace_class_self_compare` flips.
    ShoveStoreTraceClassFlips,
    /// N06.7: end-corner entry points that `rational_perp_projection` changes.
    ShoveRationalProjectionFlips,
    /// N06.4: simplex cutout pieces where `simplex_prev_division_line` would add a
    /// line (always 0: the fix is inert, see `Simplex::cutout_from_counted`).
    SimplexPrevDivisionLine,
    /// N06.4: octagon-from-box cutouts whose branch 3 changes the upper right octagon
    /// (`octagon_box_cutout_branch3`).
    OctagonBoxCutoutBranch3,
    /// N06.5: maze-search inits that failed with completed start rooms in the tree,
    /// which `init_failure_room_leak` leaves there.
    MazeInitFailureRoomLeaks,
    /// N06.5: attempts rejected before the rip whose locate added to the ripped set,
    /// which `failed_attempt_ripped_carryover` hands to the next attempt.
    MazeRippedCarryovers,
    /// N06.5: 90-degree obstacle rooms right of the room that `ortho_zero_width_room`
    /// leaves out (the fixed box would be inserted).
    MazeOrthoZeroWidthRoomFlips,
    /// N06.5: obstacle rooms without sorted neighbours, whose edge rooms come from the
    /// full sweep of `Sorted45DegreeRoomNeighbours.java:70-73`.
    MazeEdgeSweepsWithoutNeighbours,
    /// N06.5: 45-degree edge rooms that `edge_corner_not_advanced` adds or drops.
    MazeEdgeCornerFlips,
    /// N06.5: trace stubs whose half width `connect_to_trace_swapped_width` changes.
    InsertConnectToTraceWidthFlips,
}

/// Every counter, in the order of [`Counts`].
pub const COUNTERS: [Counter; 38] = [
    Counter::ConnectionsAttempted,
    Counter::ConnectionsRouted,
    Counter::ConnectionsFailed,
    Counter::RippedItems,
    Counter::SearchPops,
    Counter::SearchPushes,
    Counter::SearchDropped,
    Counter::SearchSkippedOccupied,
    Counter::RoomsCompleted,
    Counter::DoorsExpanded,
    Counter::DrillsComputed,
    Counter::SearchWorkUnits,
    Counter::MazeWorkLimitHits,
    Counter::FanoutWorkLimitHits,
    Counter::TreeQueries,
    Counter::TreeLeavesReturned,
    Counter::TreeWorkUnits,
    Counter::DoorSectionReallocations,
    Counter::MazeBudgetExtensions,
    Counter::MazeBudgetExtraUnits,
    Counter::CompensatedSnapshotRestores,
    Counter::DsnViaAlias,
    Counter::DsnClassClassPairs,
    Counter::DsnShapeBbox,
    Counter::DsnUnitIgnored,
    Counter::RouteResultZeroTestChanges,
    Counter::ShoveInFrontOfPadTypoFlips,
    Counter::ShoveInFrontOfPadFlankFlips,
    Counter::ShoveStoreTraceClassFlips,
    Counter::ShoveRationalProjectionFlips,
    Counter::SimplexPrevDivisionLine,
    Counter::OctagonBoxCutoutBranch3,
    Counter::MazeInitFailureRoomLeaks,
    Counter::MazeRippedCarryovers,
    Counter::MazeOrthoZeroWidthRoomFlips,
    Counter::MazeEdgeSweepsWithoutNeighbours,
    Counter::MazeEdgeCornerFlips,
    Counter::InsertConnectToTraceWidthFlips,
];

impl Counter {
    /// The key in the JSON.
    pub fn name(self) -> &'static str {
        match self {
            Counter::ConnectionsAttempted => "connections_attempted",
            Counter::ConnectionsRouted => "connections_routed",
            Counter::ConnectionsFailed => "connections_failed",
            Counter::RippedItems => "ripped_items",
            Counter::SearchPops => "maze_queue_pops",
            Counter::SearchPushes => "maze_queue_pushes",
            Counter::SearchDropped => "maze_drop_on_equal_skips",
            Counter::SearchSkippedOccupied => "maze_skipped_occupied",
            Counter::RoomsCompleted => "rooms_completed",
            Counter::DoorsExpanded => "doors_expanded",
            Counter::DrillsComputed => "drills_computed",
            Counter::SearchWorkUnits => "maze_work_units",
            Counter::MazeWorkLimitHits => "maze_work_limit_hits",
            Counter::FanoutWorkLimitHits => "fanout_work_limit_hits",
            Counter::TreeQueries => "tree_queries",
            Counter::TreeLeavesReturned => "tree_leaves_returned",
            Counter::TreeWorkUnits => "tree_nodes_visited",
            Counter::DoorSectionReallocations => "door_section_reallocations",
            Counter::MazeBudgetExtensions => "maze_budget_extensions",
            Counter::MazeBudgetExtraUnits => "maze_budget_extra_units",
            Counter::CompensatedSnapshotRestores => "compensated_snapshot_restores",
            Counter::DsnViaAlias => "dsn_via_alias_reached",
            Counter::DsnClassClassPairs => "dsn_class_class_pairs_reached",
            Counter::DsnShapeBbox => "dsn_shape_bbox_reached",
            Counter::DsnUnitIgnored => "dsn_unit_ignored_reached",
            Counter::RouteResultZeroTestChanges => "route_result_zero_test_changes",
            Counter::ShoveInFrontOfPadTypoFlips => "shove_in_front_of_pad_typo_flips",
            Counter::ShoveInFrontOfPadFlankFlips => "shove_in_front_of_pad_flank_flips",
            Counter::ShoveStoreTraceClassFlips => "shove_store_trace_class_flips",
            Counter::ShoveRationalProjectionFlips => "shove_rational_projection_flips",
            Counter::SimplexPrevDivisionLine => "simplex_prev_division_line_reached",
            Counter::OctagonBoxCutoutBranch3 => "octagon_box_cutout_branch3_reached",
            Counter::MazeInitFailureRoomLeaks => "maze_init_failure_room_leaks",
            Counter::MazeRippedCarryovers => "maze_ripped_carryovers",
            Counter::MazeOrthoZeroWidthRoomFlips => "maze_ortho_zero_width_room_flips",
            Counter::MazeEdgeSweepsWithoutNeighbours => "maze_edge_sweeps_without_neighbours",
            Counter::MazeEdgeCornerFlips => "maze_edge_corner_flips",
            Counter::InsertConnectToTraceWidthFlips => "insert_connect_to_trace_width_flips",
        }
    }
}

const COUNT: usize = COUNTERS.len();

static ENABLED: AtomicBool = AtomicBool::new(false);
static CELLS: [AtomicU64; COUNT] = [const { AtomicU64::new(0) }; COUNT];
static RECORDS: Mutex<Vec<StageRecord>> = Mutex::new(Vec::new());

/// Turns the statistics on and clears the counters and the stage records.
pub fn enable() {
    reset();
    ENABLED.store(true, Ordering::Relaxed);
}

/// Turns the statistics off (the records stay until [`reset`]).
pub fn disable() {
    ENABLED.store(false, Ordering::Relaxed);
}

/// True while the statistics are on.
#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Zeroes the counters and drops the stage records.
pub fn reset() {
    for cell in &CELLS {
        cell.store(0, Ordering::Relaxed);
    }
    lock_records().clear();
}

fn lock_records() -> std::sync::MutexGuard<'static, Vec<StageRecord>> {
    RECORDS.lock().unwrap_or_else(|e| e.into_inner())
}

thread_local! {
    /// Counts held back on this thread by [`capture`].
    static CAPTURED: std::cell::RefCell<Option<Counts>> =
        const { std::cell::RefCell::new(None) };
}

fn add(counter: Counter, n: u64) {
    if n > 0 {
        let held = CAPTURED.with(|c| match c.borrow_mut().as_mut() {
            Some(counts) => {
                counts.0[counter as usize] += n;
                true
            }
            None => false,
        });
        if !held {
            CELLS[counter as usize].fetch_add(n, Ordering::Relaxed);
        }
    }
}

/// Runs `f` with this thread's counts held back from the process-wide
/// counters and returns them with `f`'s result; [`add_counts`] adds them
/// later. The optimizer evaluates candidates ahead of its ordered merge
/// (P03.2) and adds a candidate's counts only when the merge consumes it,
/// so the work of candidates evaluated past a stop and dropped is never
/// counted, and a total doesn't depend on the thread count.
pub(crate) fn capture<R>(f: impl FnOnce() -> R) -> (R, Counts) {
    if !enabled() {
        return (f(), Counts::default());
    }
    let previous = CAPTURED.with(|c| c.replace(Some(Counts::default())));
    let result = f();
    let counts = CAPTURED.with(|c| c.replace(previous)).unwrap_or_default();
    (result, counts)
}

/// Adds counts that [`capture`] held back.
pub(crate) fn add_counts(counts: &Counts) {
    for counter in COUNTERS {
        add(counter, counts.get(counter));
    }
}

/// The counters at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counts(pub [u64; COUNT]);

// `Default` is derived only for arrays of up to 32 elements.
impl Default for Counts {
    fn default() -> Self {
        Counts([0; COUNT])
    }
}

impl Counts {
    /// The counters now.
    pub fn now() -> Counts {
        let mut values = [0; COUNT];
        for (value, cell) in values.iter_mut().zip(&CELLS) {
            *value = cell.load(Ordering::Relaxed);
        }
        Counts(values)
    }

    /// One counter.
    pub fn get(&self, counter: Counter) -> u64 {
        self.0[counter as usize]
    }

    /// The change since `earlier`.
    pub fn since(&self, earlier: &Counts) -> Counts {
        let mut values = [0; COUNT];
        for (i, value) in values.iter_mut().enumerate() {
            *value = self.0[i].saturating_sub(earlier.0[i]);
        }
        Counts(values)
    }

    /// Adds `other`.
    pub fn add(&mut self, other: &Counts) {
        for (value, extra) in self.0.iter_mut().zip(other.0) {
            *value += extra;
        }
    }
}

/// One stage's record.
#[derive(Clone, Debug, PartialEq)]
pub struct StageRecord {
    /// `load`, `fanout`, `pass`, `statistics`, `optimizer_pass`, `drc` or
    /// `write`.
    pub kind: &'static str,
    /// The pass number for `pass`, `statistics` and `optimizer_pass`, else 0.
    pub index: u32,
    /// Wall time of the stage.
    pub wall_seconds: f64,
    /// What the stage counted.
    pub counts: Counts,
}

/// A running stage; it records itself when dropped (or [`finish`ed]).
///
/// [`finish`ed]: StageScope::finish
#[derive(Debug)]
pub struct StageScope {
    open: Option<(&'static str, u32, Instant, Counts)>,
}

/// Starts the stage `kind` (pass number `index`, else 0). Inert while the
/// statistics are off.
pub fn stage(kind: &'static str, index: u32) -> StageScope {
    StageScope {
        open: enabled().then(|| (kind, index, Instant::now(), Counts::now())),
    }
}

impl StageScope {
    /// Ends the stage now.
    pub fn finish(self) {}
}

impl Drop for StageScope {
    fn drop(&mut self) {
        if let Some((kind, index, started, before)) = self.open.take() {
            let record = StageRecord {
                kind,
                index,
                wall_seconds: started.elapsed().as_secs_f64(),
                counts: Counts::now().since(&before),
            };
            lock_records().push(record);
        }
    }
}

/// The stage records so far, in the order the stages ended.
pub fn records() -> Vec<StageRecord> {
    lock_records().clone()
}

/// A search-tree query that visited `units` nodes (called by the work
/// meter).
#[inline]
pub(crate) fn record_tree_query(units: u64) {
    if enabled() {
        add(Counter::TreeQueries, 1);
        add(Counter::TreeWorkUnits, units);
    }
}

/// A search-tree query that visited `nodes` nodes and returned `leaves`
/// leaves (called by the work meter, N03.1).
#[inline]
pub(crate) fn record_tree_query_counts(nodes: u64, leaves: u64) {
    if enabled() {
        add(Counter::TreeQueries, 1);
        add(Counter::TreeWorkUnits, nodes);
        add(Counter::TreeLeavesReturned, leaves);
    }
}

/// A work limit was exceeded: a connection's maze search (`fanout` false)
/// or a fanout pin's (`fanout` true).
pub(crate) fn record_work_limit_hit(fanout: bool) {
    if enabled() {
        let counter = if fanout {
            Counter::FanoutWorkLimitHits
        } else {
            Counter::MazeWorkLimitHits
        };
        add(counter, 1);
    }
}

/// A budget policy extended a maze search's work limit by `units` (Q08).
pub(crate) fn record_budget_extension(units: u64) {
    if enabled() {
        add(Counter::MazeBudgetExtensions, 1);
        add(Counter::MazeBudgetExtraUnits, units);
    }
}

/// One connection attempt: the search sink's counts, the state it ended in
/// and how many items it added to the ripped set.
pub(crate) fn record_connection(search: &WorkCounters, state: AttemptState, ripped: usize) {
    add(Counter::ConnectionsAttempted, 1);
    match state {
        AttemptState::Routed => add(Counter::ConnectionsRouted, 1),
        _ => add(Counter::ConnectionsFailed, 1),
    }
    add(Counter::RippedItems, ripped as u64);
    add(Counter::SearchPops, search.pops);
    add(Counter::SearchPushes, search.pushes);
    add(Counter::SearchDropped, search.dropped);
    add(Counter::SearchSkippedOccupied, search.skipped_occupied);
    add(Counter::RoomsCompleted, search.rooms_completed);
    add(Counter::DoorsExpanded, search.doors_expanded);
    add(Counter::DrillsComputed, search.drills_computed);
    add(Counter::SearchWorkUnits, search.work_units);
}

fn number(value: f64) -> JsonValue {
    if value.is_finite() {
        JsonValue::Number(format!("{value:.6}"))
    } else {
        JsonValue::Number("null".to_string())
    }
}

fn micros_per(wall_seconds: f64, divisor: u64) -> JsonValue {
    if divisor == 0 {
        JsonValue::Number("null".to_string())
    } else {
        number(wall_seconds * 1e6 / divisor as f64)
    }
}

fn counts_json(counts: &Counts) -> JsonValue {
    JsonValue::Object(
        COUNTERS
            .iter()
            .map(|c| {
                (
                    c.name().to_string(),
                    JsonValue::Number(counts.get(*c).to_string()),
                )
            })
            .collect(),
    )
}

fn unit_costs_json(wall_seconds: f64, counts: &Counts) -> JsonValue {
    JsonValue::Object(vec![
        (
            "us_per_connection".to_string(),
            micros_per(wall_seconds, counts.get(Counter::ConnectionsAttempted)),
        ),
        (
            "us_per_room".to_string(),
            micros_per(wall_seconds, counts.get(Counter::RoomsCompleted)),
        ),
        (
            "us_per_tree_query".to_string(),
            micros_per(wall_seconds, counts.get(Counter::TreeQueries)),
        ),
        (
            "us_per_1000_tree_nodes".to_string(),
            micros_per(wall_seconds * 1000.0, counts.get(Counter::TreeWorkUnits)),
        ),
    ])
}

fn stage_json(record: &StageRecord) -> JsonValue {
    JsonValue::Object(vec![
        ("stage".to_string(), JsonValue::string(record.kind)),
        ("index".to_string(), JsonValue::int(i64::from(record.index))),
        ("wall_seconds".to_string(), number(record.wall_seconds)),
        ("counts".to_string(), counts_json(&record.counts)),
        (
            "unit_costs".to_string(),
            unit_costs_json(record.wall_seconds, &record.counts),
        ),
    ])
}

/// The statistics of the run as JSON: `design`, the stages in order and the
/// totals. Call it after the run, with the statistics still on or off.
pub fn report_json(design: &str) -> JsonValue {
    let records = records();
    let mut total = Counts::default();
    let mut wall = 0.0;
    for record in &records {
        total.add(&record.counts);
        wall += record.wall_seconds;
    }
    JsonValue::Object(vec![
        ("design".to_string(), JsonValue::string(design)),
        (
            "stages".to_string(),
            JsonValue::Array(records.iter().map(stage_json).collect()),
        ),
        (
            "total".to_string(),
            JsonValue::Object(vec![
                ("wall_seconds".to_string(), number(wall)),
                ("counts".to_string(), counts_json(&total)),
                ("unit_costs".to_string(), unit_costs_json(wall, &total)),
            ]),
        ),
    ])
}

/// The counts of the records, without times: what two runs must agree on.
pub fn counts_only(records: &[StageRecord]) -> Vec<(&'static str, u32, Counts)> {
    records
        .iter()
        .map(|r| (r.kind, r.index, r.counts.clone()))
        .collect()
}

/// Count an N06 branch when statistics are enabled, in either switch arm.
pub(crate) fn record_n06(counter: Counter) {
    if enabled() {
        add(counter, 1);
    }
}

/// N06.4: the cutouts' reachability counts ([`freerouting_geometry::SplitReach`]) of
/// one split or cutout, in either switch arm.
pub(crate) fn record_split_reach(reach: &freerouting_geometry::SplitReach) {
    if enabled() {
        add(
            Counter::SimplexPrevDivisionLine,
            reach.simplex_prev_division_line,
        );
        add(
            Counter::OctagonBoxCutoutBranch3,
            reach.octagon_box_cutout_branch3,
        );
    }
}

pub(crate) fn record_compensated_snapshot_restore() {
    if enabled() {
        add(Counter::CompensatedSnapshotRestores, 1);
    }
}

/// N06 reachability counters, carried through optimizer statistics capture.
pub(crate) fn record_door_section_reallocation() {
    if enabled() {
        add(Counter::DoorSectionReallocations, 1);
    }
}

/// The counters are process-wide, so the library's unit tests that turn them
/// on run one at a time, here and in other modules (N06.5's `connection` tests).
#[cfg(test)]
pub(crate) static TEST_SERIAL: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::TEST_SERIAL as SERIAL;
    use super::*;

    #[test]
    fn a_stage_records_the_change_of_the_counters() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        enable();
        record_tree_query(5);
        let scope = stage("pass", 3);
        record_tree_query(7);
        record_tree_query(2);
        scope.finish();
        disable();
        let records: Vec<_> = records()
            .into_iter()
            .filter(|r| r.kind == "pass" && r.index == 3)
            .collect();
        assert_eq!(records.len(), 1);
        // Other tests of this binary may route while the flag is on, so the
        // counts are lower bounds here; the CLI test compares exact counts.
        assert!(records[0].counts.get(Counter::TreeQueries) >= 2);
        assert!(records[0].counts.get(Counter::TreeWorkUnits) >= 9);
        assert!(Counts::now().get(Counter::TreeWorkUnits) >= 14);
        reset();
    }

    #[test]
    fn n06_via_ratio_counter_observes_the_actual_zero_test() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        enable();
        let result = crate::autoroute::item_route_result::ItemRouteResult::new(
            crate::board::items::ItemId(1),
            2,
            1,
            100.0,
            200.0,
            0,
            0,
        );
        assert_eq!(result.improvement_percentage(), 0.0);
        assert_ne!(
            result.with_int_via_ratio(false).improvement_percentage(),
            0.0
        );
        result.record_via_ratio_zero_test();
        assert!(Counts::now().get(Counter::RouteResultZeroTestChanges) >= 1);
        disable();
        reset();
    }

    #[test]
    fn off_records_nothing() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        disable();
        reset();
        record_tree_query(5);
        stage("fanout", 0).finish();
        assert!(records().is_empty());
    }

    #[test]
    fn door_reallocations_are_reported_only_while_counting() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        enable();
        let (_, counts) = capture(|| {
            record_door_section_reallocation();
            record_door_section_reallocation();
        });
        assert_eq!(counts.get(Counter::DoorSectionReallocations), 2);
        disable();
        let before = Counts::now().get(Counter::DoorSectionReallocations);
        record_door_section_reallocation();
        assert_eq!(Counts::now().get(Counter::DoorSectionReallocations), before);
        reset();
    }

    #[test]
    fn the_json_names_every_counter_and_leaves_zero_divisors_null() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        enable();
        stage("load", 0).finish();
        disable();
        let text = report_json("board").to_pretty_string();
        reset();
        for counter in COUNTERS {
            assert!(text.contains(counter.name()), "{} missing", counter.name());
        }
        assert_eq!(
            micros_per(0.5, 0),
            JsonValue::Number("null".to_string()),
            "a zero divisor is null"
        );
        assert_eq!(
            micros_per(0.5, 2),
            JsonValue::Number("250000.000000".into())
        );
        assert!(text.contains("\"stage\": \"load\""));
    }
}

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
    /// Search-tree queries: overlap queries and room completions.
    TreeQueries,
    /// Search-tree nodes visited by those queries (D5's work unit).
    TreeWorkUnits,
}

/// Every counter, in the order of [`Counts`].
pub const COUNTERS: [Counter; 14] = [
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
    Counter::TreeQueries,
    Counter::TreeWorkUnits,
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
            Counter::TreeQueries => "tree_queries",
            Counter::TreeWorkUnits => "tree_nodes_visited",
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts(pub [u64; COUNT]);

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

#[cfg(test)]
mod tests {
    use super::*;

    /// The counters are process-wide, so the tests that turn them on run
    /// one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

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
    fn off_records_nothing() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        disable();
        reset();
        record_tree_query(5);
        stage("fanout", 0).finish();
        assert!(records().is_empty());
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

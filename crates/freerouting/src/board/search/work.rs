//! Work counting: the deterministic budget of Java's work limits (B02.7).
//!
//! Since `30baad7e2` Java cuts its performance-critical algorithms off after
//! a fixed amount of *work*, not wall-clock time: `WorkMeter.tick()` runs
//! once per search-tree node popped off the traversal stack (leaf or inner,
//! hit or miss) in `MinAreaTree.overlapsUnlocked` and in the three
//! `completeShape` traversals, and a `WorkLimit` of `n` nominal milliseconds
//! allows `n × 64,000` units (`datastructures/WorkLimit.java`).
//!
//! Java's meter is per thread (`WorkMeter.current()`). Here each board's
//! [`SearchTreeManager`](super::SearchTreeManager) owns one [`WorkMeter`],
//! shared by its trees; a query counts the nodes it pops in a local and adds
//! them once at the end. The two count the same units whenever a limit's
//! window runs on one thread against one board, which holds at every Java
//! limit site. A thread-local meter would leak counts between rayon tasks
//! sharing a worker thread, so it is ruled out (D2). A [`WorkLimit`] records
//! the meter value at its start and is passed to the code that checks it; in
//! debug builds it asserts that it is checked against the meter it started
//! on, Rust's form of Java's owner-thread assert.
//!
//! User stops stay on `freerouting_geometry::Stoppable`; wall time never
//! changes the work done.
//!
//! **The unit (P04.2, restored by N03.1; off by default and in `legacy`).**
//! Java's unit, nodes visited, depends on the tree's
//! shape: two trees with the same contents built in another order count
//! differently, so a limit cuts off at another point. [`WorkUnit::Leaves`]
//! counts the leaves an overlap query returns (its result set) and, in room
//! completion, the leaves that meet the room's bound at their turn in the
//! canonical order (P04.1); both depend only on the tree's contents. A meter
//! counts in one unit, set per board ([`WorkMeter::set_unit`],
//! `router.work_unit`); [`WorkUnit::Nodes`] is the default and Java's.
//! A limit of `n` nominal milliseconds allows `n` times the unit's
//! [`WorkUnit::units_per_nominal_ms`]: 64,000 nodes, or
//! [`LEAF_UNITS_PER_NOMINAL_MS`] leaves, calibrated on `benchmark` so that a
//! nominal millisecond is about the same work in both units.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

/// Work units per nominal millisecond (Java
/// `WorkLimit.UNITS_PER_NOMINAL_MILLISECOND`), in [`WorkUnit::Nodes`].
pub const UNITS_PER_NOMINAL_MS: u64 = 64_000;

/// Work units per nominal millisecond in [`WorkUnit::Leaves`] (P04.2): the
/// nodes rate times the leaves returned per node visited, measured on the
/// `benchmark` set (see `docs/DIVERGENCES.md`).
pub const LEAF_UNITS_PER_NOMINAL_MS: u64 = 3_000;

/// What a [`WorkMeter`] counts (P04.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkUnit {
    /// Search-tree nodes visited, Java's unit (D5); depends on the tree's
    /// shape.
    #[default]
    Nodes,
    /// Leaves returned by overlap queries and leaves restraining a room in
    /// completion; depends only on the tree's contents.
    Leaves,
}

impl WorkUnit {
    /// The setting value: `nodes` or `leaves`.
    pub fn name(self) -> &'static str {
        match self {
            WorkUnit::Nodes => "nodes",
            WorkUnit::Leaves => "leaves",
        }
    }

    /// The unit named `name` (`nodes` or `leaves`).
    pub fn parse(name: &str) -> Option<WorkUnit> {
        match name {
            "nodes" => Some(WorkUnit::Nodes),
            "leaves" => Some(WorkUnit::Leaves),
            _ => None,
        }
    }

    /// Units per nominal millisecond of a [`WorkLimit`].
    pub fn units_per_nominal_ms(self) -> u64 {
        match self {
            WorkUnit::Nodes => UNITS_PER_NOMINAL_MS,
            WorkUnit::Leaves => LEAF_UNITS_PER_NOMINAL_MS,
        }
    }

    fn code(self) -> u8 {
        match self {
            WorkUnit::Nodes => 0,
            WorkUnit::Leaves => 1,
        }
    }

    fn from_code(code: u8) -> WorkUnit {
        if code == 1 {
            WorkUnit::Leaves
        } else {
            WorkUnit::Nodes
        }
    }
}

static NEXT_METER_ID: AtomicU64 = AtomicU64::new(1);

/// Counts units of work, search-tree nodes visited (Java `WorkMeter`) or
/// leaves returned (P04.2, [`WorkUnit`]).
#[derive(Debug)]
pub struct WorkMeter {
    id: u64,
    units: AtomicU64,
    unit: AtomicU8,
}

impl WorkMeter {
    /// A new meter at zero, counting nodes.
    pub fn new() -> Self {
        WorkMeter {
            id: NEXT_METER_ID.fetch_add(1, Ordering::Relaxed),
            units: AtomicU64::new(0),
            unit: AtomicU8::new(WorkUnit::Nodes.code()),
        }
    }

    /// What this meter counts.
    pub fn unit(&self) -> WorkUnit {
        WorkUnit::from_code(self.unit.load(Ordering::Relaxed))
    }

    /// Counts in `unit` from now on. Set it before any limit starts: a
    /// limit's allowance is fixed when it is created.
    pub fn set_unit(&self, unit: WorkUnit) {
        self.unit.store(unit.code(), Ordering::Relaxed);
    }

    /// Records `units` units of work.
    pub fn add(&self, units: u64) {
        if RECORDING.with(Cell::get) && record(WorkRecord::Units(units)) {
            return;
        }
        if units > 0 {
            self.units.fetch_add(units, Ordering::Relaxed);
            // P01.2: one load of a flag while the statistics are off.
            crate::run_stats::record_tree_query(units);
        }
    }

    /// Records a tree query that visited `nodes` nodes and returned
    /// `leaves` leaves, in this meter's unit. The run statistics (P01.2)
    /// get both counts whatever the unit.
    pub fn add_query(&self, nodes: u64, leaves: u64) {
        if RECORDING.with(Cell::get) && record(WorkRecord::Query(nodes, leaves)) {
            return;
        }
        let units = match self.unit() {
            WorkUnit::Nodes => nodes,
            WorkUnit::Leaves => leaves,
        };
        if units > 0 {
            self.units.fetch_add(units, Ordering::Relaxed);
        }
        if nodes > 0 {
            crate::run_stats::record_tree_query_counts(nodes, leaves);
        }
    }

    /// Records one unit of work (Java `tick`).
    pub fn tick(&self) {
        self.add(1);
    }

    /// The units recorded so far.
    pub fn units(&self) -> u64 {
        self.units.load(Ordering::Relaxed)
    }
}

/// K15.47: one addition to a [`WorkMeter`] while [`recorded`] runs: an
/// overlap query's nodes and leaves ([`WorkMeter::add_query`]) or units
/// ([`WorkMeter::add`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkRecord {
    Query(u64, u64),
    Units(u64),
}

impl WorkRecord {
    /// Adds this record to `meter` as the call it records did.
    pub(crate) fn replay(self, meter: &WorkMeter) {
        match self {
            WorkRecord::Query(nodes, leaves) => meter.add_query(nodes, leaves),
            WorkRecord::Units(units) => meter.add(units),
        }
    }
}

thread_local! {
    /// True while a [`recorded`] call runs on this thread.
    static RECORDING: Cell<bool> = const { Cell::new(false) };
    /// The records of the [`recorded`] calls running on this thread,
    /// innermost last, each with its "dry" flag (the meters are not touched).
    static RECORDS: RefCell<Vec<(Vec<WorkRecord>, bool)>> = const { RefCell::new(Vec::new()) };
}

/// Appends `record` to the running [`recorded`] calls, innermost first, up
/// to and with the innermost dry one, or to all of them if none is dry.
/// True if one is dry (the meter is then not touched).
fn record(record: WorkRecord) -> bool {
    RECORDS.with(|r| {
        let mut levels = r.borrow_mut();
        for (records, dry) in levels.iter_mut().rev() {
            records.push(record);
            if *dry {
                return true;
            }
        }
        false
    })
}

/// Pops the innermost level of [`RECORDS`] when a [`recorded`] call ends,
/// also by a panic.
struct RecordingLevel;

impl Drop for RecordingLevel {
    fn drop(&mut self) {
        RECORDS.with(|r| {
            let mut levels = r.borrow_mut();
            levels.pop();
            RECORDING.with(|flag| flag.set(!levels.is_empty()));
        });
    }
}

/// K15.47: runs `f` and returns, with its result, every addition it made
/// to a [`WorkMeter`] on this thread, in order; replaying them
/// ([`WorkRecord::replay`]) adds what `f` added. With `dry` the meters are
/// not touched (and the run statistics do not see the queries). A cache of
/// a metered computation keeps the records and replays them where it
/// answers from the cache, so the work limits cut off where they did
/// (`board::area_contacts`).
pub(crate) fn recorded<R>(dry: bool, f: impl FnOnce() -> R) -> (R, Vec<WorkRecord>) {
    RECORDS.with(|r| r.borrow_mut().push((Vec::new(), dry)));
    RECORDING.with(|r| r.set(true));
    let level = RecordingLevel;
    let result = f();
    let records = RECORDS.with(|r| {
        r.borrow_mut()
            .last_mut()
            .map(|(records, _)| std::mem::take(records))
            .unwrap_or_default()
    });
    drop(level);
    (result, records)
}

impl Default for WorkMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for WorkMeter {
    /// A copy is another meter (another board's), with the same count.
    fn clone(&self) -> Self {
        WorkMeter {
            id: NEXT_METER_ID.fetch_add(1, Ordering::Relaxed),
            units: AtomicU64::new(self.units()),
            unit: AtomicU8::new(self.unit().code()),
        }
    }
}

/// Stops an algorithm after a fixed amount of work (Java `WorkLimit`).
#[derive(Clone, Debug)]
pub struct WorkLimit {
    meter_id: u64,
    start: u64,
    max_units: u64,
    nominal_ms: i64,
    site: &'static str,
    reported: bool,
}

impl WorkLimit {
    /// A limit of `ms` nominal milliseconds of work on `meter`, starting
    /// now, in the meter's unit ([`WorkUnit::units_per_nominal_ms`]).
    /// Negative values count as 0; the unit count saturates at `i64::MAX`
    /// as Java's `long` does.
    pub fn nominal_ms(meter: &WorkMeter, ms: i64) -> Self {
        Self::with_site(meter, ms, "")
    }

    /// [`nominal_ms`](Self::nominal_ms) with the snake_case name of the
    /// guarded algorithm for the cut-off line.
    pub fn with_site(meter: &WorkMeter, ms: i64, site: &'static str) -> Self {
        let ms = ms.max(0) as u64;
        let rate = meter.unit().units_per_nominal_ms();
        let max_units = if ms > i64::MAX as u64 / rate {
            i64::MAX as u64
        } else {
            ms * rate
        };
        WorkLimit {
            meter_id: meter.id,
            start: meter.units(),
            max_units,
            nominal_ms: ms as i64,
            site,
            reported: false,
        }
    }

    /// The units allowed.
    pub fn max_units(&self) -> u64 {
        self.max_units
    }

    /// The units done on `meter` since the limit was created.
    pub fn units_done(&self, meter: &WorkMeter) -> u64 {
        debug_assert_eq!(
            meter.id, self.meter_id,
            "WorkLimit checked against a meter it did not start on"
        );
        meter.units().wrapping_sub(self.start)
    }

    /// True if more work than allowed has been done since the limit was
    /// created (Java `limitExceeded`, strictly greater).
    pub fn exceeded(&self, meter: &WorkMeter) -> bool {
        self.units_done(meter) > self.max_units
    }

    /// [`exceeded`](Self::exceeded), and on the first exceeded check
    /// appends Java's cut-off line to `log`:
    /// `WORK_LIMIT_CUTOFF site=<site> nominal_ms=<n> units=<done> [context]`.
    /// `context` (space-separated `key=value` pairs) is only called then.
    pub fn exceeded_logging(
        &mut self,
        meter: &WorkMeter,
        context: impl FnOnce() -> String,
        log: &mut Vec<String>,
    ) -> bool {
        let exceeded = self.exceeded(meter);
        if exceeded && !self.reported {
            self.reported = true;
            let details = context();
            let mut line = format!(
                "WORK_LIMIT_CUTOFF site={} nominal_ms={} units={}",
                self.site,
                self.nominal_ms,
                self.units_done(meter)
            );
            if !details.is_empty() {
                line.push(' ');
                line.push_str(&details);
            }
            log.push(line);
        }
        exceeded
    }
}

#[cfg(test)]
mod recording_tests {
    use super::*;

    /// A recorded call keeps every addition in order, the meter gets them
    /// as without the recording, and replaying them adds the same again.
    #[test]
    fn a_recorded_call_keeps_its_additions_and_replays_them() {
        let meter = WorkMeter::new();
        let ((), records) = recorded(false, || {
            meter.add_query(7, 2);
            meter.add(3);
            meter.add_query(0, 0);
        });
        assert_eq!(
            records,
            vec![
                WorkRecord::Query(7, 2),
                WorkRecord::Units(3),
                WorkRecord::Query(0, 0)
            ]
        );
        assert_eq!(meter.units(), 10);
        let other = WorkMeter::new();
        other.set_unit(WorkUnit::Leaves);
        for r in &records {
            r.replay(&meter);
            r.replay(&other);
        }
        assert_eq!(meter.units(), 20);
        assert_eq!(other.units(), 5, "a replay counts in the meter's unit");
        meter.add(1);
        assert_eq!(meter.units(), 21, "no recording after the call");
    }

    /// A dry call keeps its additions and the meter does not see them; a
    /// call nested in it is dry too, and the outer levels of a dry call
    /// get nothing of it.
    #[test]
    fn a_dry_call_leaves_the_meter_and_the_outer_calls_alone() {
        let meter = WorkMeter::new();
        let ((inner, dry), outer) = recorded(false, || {
            meter.add(1);
            let dry = recorded(true, || {
                meter.add(2);
                let (_, inner) = recorded(false, || meter.add_query(4, 1));
                meter.add(8);
                inner
            });
            meter.add(16);
            dry
        });
        assert_eq!(inner, vec![WorkRecord::Query(4, 1)]);
        assert_eq!(
            dry,
            vec![
                WorkRecord::Units(2),
                WorkRecord::Query(4, 1),
                WorkRecord::Units(8)
            ]
        );
        assert_eq!(outer, vec![WorkRecord::Units(1), WorkRecord::Units(16)]);
        assert_eq!(meter.units(), 17);
    }

    /// A panic inside a recorded call ends its level.
    #[test]
    fn a_panicking_call_ends_its_level() {
        let meter = WorkMeter::new();
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            recorded(true, || -> () { panic!("inside") })
        }));
        assert!(caught.is_err());
        meter.add(5);
        assert_eq!(meter.units(), 5);
        assert!(!RECORDING.with(Cell::get));
    }
}

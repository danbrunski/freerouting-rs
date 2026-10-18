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

    /// Allows `units` more units, saturating at `i64::MAX` as the
    /// constructor does (Q08's earned budget; Java never extends a limit).
    pub fn extend(&mut self, units: u64) {
        self.max_units = self.max_units.saturating_add(units).min(i64::MAX as u64);
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

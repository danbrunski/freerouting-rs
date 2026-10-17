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

use std::sync::atomic::{AtomicU64, Ordering};

/// Work units per nominal millisecond (Java
/// `WorkLimit.UNITS_PER_NOMINAL_MILLISECOND`).
pub const UNITS_PER_NOMINAL_MS: u64 = 64_000;

static NEXT_METER_ID: AtomicU64 = AtomicU64::new(1);

/// Counts units of work, search-tree nodes visited (Java `WorkMeter`).
#[derive(Debug)]
pub struct WorkMeter {
    id: u64,
    units: AtomicU64,
}

impl WorkMeter {
    /// A new meter at zero.
    pub fn new() -> Self {
        WorkMeter {
            id: NEXT_METER_ID.fetch_add(1, Ordering::Relaxed),
            units: AtomicU64::new(0),
        }
    }

    /// Records `units` units of work.
    pub fn add(&self, units: u64) {
        if units > 0 {
            self.units.fetch_add(units, Ordering::Relaxed);
            // P01.2: one load of a flag while the statistics are off.
            crate::run_stats::record_tree_query(units);
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
    /// now. Negative values count as 0; the unit count saturates at
    /// `i64::MAX` as Java's `long` does.
    pub fn nominal_ms(meter: &WorkMeter, ms: i64) -> Self {
        Self::with_site(meter, ms, "")
    }

    /// [`nominal_ms`](Self::nominal_ms) with the snake_case name of the
    /// guarded algorithm for the cut-off line.
    pub fn with_site(meter: &WorkMeter, ms: i64, site: &'static str) -> Self {
        let ms = ms.max(0) as u64;
        let max_units = if ms > i64::MAX as u64 / UNITS_PER_NOMINAL_MS {
            i64::MAX as u64
        } else {
            ms * UNITS_PER_NOMINAL_MS
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

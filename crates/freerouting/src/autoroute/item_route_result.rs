//! The result of rerouting one item in the optimizer.
//!
//! Port of `autoroute/ItemRouteResult.java` at `cda540f`. The batch
//! optimizer (O06) reroutes an item, compares the board's incomplete count,
//! via count and trace length before and after, and ranks the candidates
//! with [`ItemRouteResult::compare`].

use std::cmp::Ordering;

use crate::board::items::ItemId;

/// The metrics of a board before and after rerouting one item (Java
/// `ItemRouteResult`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemRouteResult {
    item_id: ItemId,
    improvement_percentage: f32,
    via_count_before: i32,
    via_count_after: i32,
    trace_length_before: f64,
    trace_length_after: f64,
    incomplete_count_before: i32,
    incomplete_count_after: i32,
    improved: bool,
}

impl ItemRouteResult {
    /// An unimproved result (Java `ItemRouteResult(int itemId)`): no vias,
    /// no length, and one incomplete connection after routing where there
    /// was none before.
    pub fn unimproved(item_id: ItemId) -> Self {
        let mut result = ItemRouteResult::new(item_id, 0, 0, 0.0, 0.0, 0, 1);
        result.improved = false;
        result
    }

    /// Compares the metrics before and after routing (Java's 7-argument
    /// constructor, `ItemRouteResult.java:23-58`).
    ///
    /// The result is improved when the incomplete count fell; on a tie when
    /// the via count fell; on a tie when the trace length fell.
    pub fn new(
        item_id: ItemId,
        via_count_before: i32,
        via_count_after: i32,
        trace_length_before: f64,
        trace_length_after: f64,
        incomplete_count_before: i32,
        incomplete_count_after: i32,
    ) -> Self {
        let improved = match incomplete_count_after.cmp(&incomplete_count_before) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => match via_count_after.cmp(&via_count_before) {
                Ordering::Less => true,
                Ordering::Greater => false,
                Ordering::Equal => trace_length_after < trace_length_before,
            },
        };
        let improvement_percentage = improvement_percentage(
            via_count_before,
            via_count_after,
            trace_length_before,
            trace_length_after,
            true,
        );
        ItemRouteResult {
            item_id,
            improvement_percentage,
            via_count_before,
            via_count_after,
            trace_length_before,
            trace_length_after,
            incomplete_count_before,
            incomplete_count_after,
            improved,
        }
    }

    /// This result under `router.java_quirks.route_result_int_via_ratio` (N06):
    /// `true` keeps Java's integer via ratio, `false` recomputes
    /// [`Self::improvement_percentage`] with the real ratio.
    pub fn with_int_via_ratio(self, java_quirk: bool) -> Self {
        if java_quirk {
            return self;
        }
        ItemRouteResult {
            improvement_percentage: improvement_percentage(
                self.via_count_before,
                self.via_count_after,
                self.trace_length_before,
                self.trace_length_after,
                false,
            ),
            ..self
        }
    }

    /// Whether real division changes the optimizer's equality-to-zero test.
    /// Observes both formulas without changing this result.
    pub(crate) fn record_via_ratio_zero_test(&self) {
        if crate::run_stats::enabled() {
            let zero = |java| {
                improvement_percentage(
                    self.via_count_before,
                    self.via_count_after,
                    self.trace_length_before,
                    self.trace_length_after,
                    java,
                ) == 0.0
            };
            if zero(true) != zero(false) {
                crate::run_stats::record_n06(crate::run_stats::Counter::RouteResultZeroTestChanges);
            }
        }
    }

    /// Ranks two results, better first (Java `compareTo`, `:69-90`): fewer
    /// incomplete connections after routing, then fewer vias after, then a
    /// shorter trace length after, then the lower item id.
    ///
    /// The id is compared by its value, ascending, as Java's
    /// `Integer.compare`; [`ItemId`]'s own order is descending. A NaN length
    /// falls through to the id, as in Java.
    pub fn compare(&self, other: &ItemRouteResult) -> Ordering {
        self.incomplete_count_after
            .cmp(&other.incomplete_count_after)
            .then(self.via_count_after.cmp(&other.via_count_after))
            .then_with(|| {
                if self.trace_length_after < other.trace_length_after {
                    Ordering::Less
                } else if self.trace_length_after > other.trace_length_after {
                    Ordering::Greater
                } else {
                    self.item_id.0.cmp(&other.item_id.0)
                }
            })
    }

    /// True if this result ranks before `other` (Java `improvedOver`).
    pub fn improved_over(&self, other: &ItemRouteResult) -> bool {
        self.compare(other) == Ordering::Less
    }

    /// The rerouted item (Java `itemId`).
    pub fn item_id(&self) -> ItemId {
        self.item_id
    }

    /// True if rerouting improved the board (Java `improved`).
    pub fn improved(&self) -> bool {
        self.improved
    }

    /// `1 - (vias after / vias before + length after / length before) / 2`
    /// with the via ratio in integer division, 0 when there were no vias or
    /// no length before (Java `improvementPercentage`).
    pub fn improvement_percentage(&self) -> f32 {
        self.improvement_percentage
    }

    /// The via count after routing (Java `viaCount`).
    pub fn via_count(&self) -> i32 {
        self.via_count_after
    }

    /// The trace length after routing (Java `traceLength`).
    pub fn trace_length(&self) -> f64 {
        self.trace_length_after
    }

    /// The incomplete connections after routing (Java `incompleteCount`).
    pub fn incomplete_count(&self) -> i32 {
        self.incomplete_count_after
    }

    /// The incomplete connections before routing (Java
    /// `incompleteCountBefore`).
    pub fn incomplete_count_before(&self) -> i32 {
        self.incomplete_count_before
    }

    /// Vias before minus vias after (Java `viaCountReduced`).
    pub fn via_count_reduced(&self) -> i32 {
        self.via_count_before.wrapping_sub(self.via_count_after)
    }

    /// Trace length before minus after (Java `lengthReduced`).
    pub fn length_reduced(&self) -> f64 {
        self.trace_length_before - self.trace_length_after
    }

    /// Overrides the improved flag (Java `updateImproved`).
    pub fn update_improved(&mut self, improved: bool) {
        self.improved = improved;
    }
}

/// Java's `improvementPercentage` (`ItemRouteResult.java:59-65`).
fn improvement_percentage(
    via_count_before: i32,
    via_count_after: i32,
    trace_length_before: f64,
    trace_length_after: f64,
    java_int_via_ratio: bool,
) -> f32 {
    if via_count_before == 0 || trace_length_before == 0.0 {
        return 0.0;
    }
    // PARITY(quirk, java_quirks.route_result_int_via_ratio): Java divides the
    // via counts as `int`s inside the `double` expression
    // (`viaCountAfter / viaCountBefore`), so the via term is truncated toward
    // zero: 1 / 2 contributes 0, not 0.5. `false` divides as `double`s.
    let via_ratio = if java_int_via_ratio {
        f64::from(via_count_after.wrapping_div(via_count_before))
    } else {
        f64::from(via_count_after) / f64::from(via_count_before)
    };
    (1.0 - ((via_ratio + trace_length_after / trace_length_before) / 2.0)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(args: [i32; 7]) -> ItemRouteResult {
        ItemRouteResult::new(
            ItemId(args[0]),
            args[1],
            args[2],
            f64::from(args[3]),
            f64::from(args[4]),
            args[5],
            args[6],
        )
    }

    /// `(id, vias before, vias after, length before, length after,
    /// incomplete before, incomplete after)`, then Java's `improved()` and
    /// the raw bits of `improvementPercentage()`, from a probe run of the
    /// Java class at `cda540f` with `tools/java-reference/probe.sh`.
    const JAVA_CASES: [([i32; 7], bool, u32); 8] = [
        ([7, 3, 2, 1000, 900, 1, 1], true, 0x3f0c_cccd),
        ([7, 3, 5, 1000, 900, 1, 1], false, 0x3d4c_cccd),
        ([8, 4, 4, 1000, 1000, 2, 2], false, 0x0),
        ([9, 0, 1, 1000, 900, 1, 0], true, 0x0),
        ([10, 2, 3, 0, 900, 1, 1], false, 0x0),
        ([11, 5, 11, 400, 333, 3, 3], false, 0xbed5_1eb8),
        ([12, 3, 1, 1000, 1001, 2, 2], true, 0x3eff_be77),
        ([13, -3, 7, 100, 50, 0, 0], false, 0x3fe0_0000),
    ];

    #[test]
    fn improved_and_percentage_match_java() {
        for (args, improved, bits) in JAVA_CASES {
            let r = result(args);
            assert_eq!(r.improved(), improved, "improved {args:?}");
            assert_eq!(
                r.improvement_percentage().to_bits(),
                bits,
                "percentage {args:?}"
            );
        }
    }

    #[test]
    fn integer_division_differs_from_real_division() {
        // 2 / 3 vias: Java's int division gives 0, real division 0.667.
        let r = result([7, 3, 2, 1000, 900, 1, 1]);
        let real = (1.0 - ((2.0 / 3.0 + 0.9) / 2.0)) as f32;
        assert_eq!(r.improvement_percentage(), 0.55);
        assert_ne!(r.improvement_percentage(), real);
    }

    #[test]
    fn unimproved_matches_java() {
        let r = ItemRouteResult::unimproved(ItemId(5));
        assert!(!r.improved());
        assert_eq!(r.improvement_percentage().to_bits(), 0);
        assert_eq!(r.incomplete_count(), 1);
        assert_eq!(r.incomplete_count_before(), 0);
    }

    /// Java's `a.compareTo(b)` for every pair of these five results, row by
    /// row, from the same probe run.
    const JAVA_COMPARE: [i32; 25] = [
        0, -1, -1, -1, -1, 1, 0, -1, -1, -1, 1, 1, 0, 1, -1, 1, 1, -1, 0, -1, 1, 1, 1, 1, 0,
    ];

    #[test]
    fn compare_matches_java() {
        let results = [
            result([3, 2, 1, 10, 5, 1, 0]),
            result([4, 2, 1, 10, 5, 1, 0]),
            result([2, 2, 2, 10, 5, 1, 0]),
            result([1, 2, 1, 10, 6, 1, 0]),
            result([0, 2, 0, 10, 4, 1, 1]),
        ];
        for (i, a) in results.iter().enumerate() {
            for (j, b) in results.iter().enumerate() {
                assert_eq!(a.compare(b) as i32, JAVA_COMPARE[i * 5 + j], "{i} vs {j}");
                assert_eq!(a.improved_over(b), JAVA_COMPARE[i * 5 + j] < 0);
            }
        }
    }

    #[test]
    fn reductions_and_update() {
        let mut r = result([1, 5, 3, 100, 60, 2, 2]);
        assert_eq!(r.via_count_reduced(), 2);
        assert_eq!(r.length_reduced(), 40.0);
        assert_eq!(r.via_count(), 3);
        assert_eq!(r.trace_length(), 60.0);
        assert!(r.improved());
        r.update_improved(false);
        assert!(!r.improved());
    }
}

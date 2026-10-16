//! The legacy (V1) board score.
//!
//! Port of `BoardStatistics.calculateScore`, `getMaximumScore` and
//! `getLegacyNormalizedScore` at `a773a70ec`. The weights come from the
//! router's cost settings (Java's `RoutingCostSettings`, the `scoring`
//! section of [`RouterSettings`](crate::settings::RouterSettings)).
//!
//! Java's types are kept (D6), because they decide the last bits of the
//! score:
//! - the counts are Java `int`s; a count times a `float` weight is a
//!   `float`, and the penalties are summed left to right in `float`;
//! - the costs are a `double` (`float` length times the `double` trace
//!   cost, plus the `int` product of the via count and the via cost) cast
//!   to `float`;
//! - the score and the normalized score are `float`, and the normalized
//!   score clamps with Java's `Math.max` (a `NaN` stays `NaN`).
//!
//! Java takes the trace length in millimeters when the statistics have it,
//! else the raw length; the port's statistics always have it.

use freerouting_geometry::jmath::max_f32;

use super::BoardStatistics;
use crate::settings::fields::ScoringSettings;

/// A count as Java's `int` (the statistics hold `Integer`s).
fn java_int(count: usize) -> i32 {
    count as i32
}

/// Java `getMaximumScore`: the maximum connection count times the unrouted
/// net penalty, in `float`.
pub fn legacy_maximum_score(stats: &BoardStatistics, costs: &ScoringSettings) -> f32 {
    java_int(stats.connections.maximum_count) as f32 * costs.unrouted_net_penalty
}

/// The penalties of [`legacy_score`]: `float` products summed left to
/// right.
pub fn legacy_penalties(stats: &BoardStatistics, costs: &ScoringSettings) -> f32 {
    java_int(stats.connections.incomplete_count) as f32 * costs.unrouted_net_penalty
        + java_int(stats.clearance_violations.total_count) as f32
            * costs.clearance_violation_penalty
        + java_int(stats.bends.total_count) as f32 * costs.bend_penalty
}

/// The costs of [`legacy_score`]: computed in `double`, then cast to
/// `float`. The via term is an `int` product, which wraps as Java's does.
pub fn legacy_costs(stats: &BoardStatistics, costs: &ScoringSettings) -> f32 {
    let trace_length_for_cost = stats.traces.total_length_mm;
    let via_costs = java_int(stats.vias.total_count).wrapping_mul(costs.via_costs);
    (f64::from(trace_length_for_cost) * costs.default_preferred_direction_trace_cost
        + f64::from(via_costs)) as f32
}

/// Java `calculateScore`: the maximum score minus the penalties minus the
/// costs, in `float`. Higher is better; it can be negative.
pub fn legacy_score(stats: &BoardStatistics, costs: &ScoringSettings) -> f32 {
    let maximum_score = legacy_maximum_score(stats, costs);
    let penalties = legacy_penalties(stats, costs);
    let trace_costs = legacy_costs(stats, costs);
    maximum_score - penalties - trace_costs
}

/// Java `getLegacyNormalizedScore`: the score as a part of the maximum
/// score, clamped below at zero, times 1000. Zero when the maximum score is
/// not positive (a board without connections).
pub fn legacy_normalized_score(stats: &BoardStatistics, costs: &ScoringSettings) -> f32 {
    let maximum_score = legacy_maximum_score(stats, costs);
    if maximum_score <= 0.0 {
        return 0.0;
    }
    max_f32(0.0, legacy_score(stats, costs) / maximum_score) * 1000.0
}

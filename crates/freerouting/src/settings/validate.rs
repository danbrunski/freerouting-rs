//! Java's `RouterSettings.validateAgainstBoard(null)`, which
//! `SettingsMerger.merge` runs on the merged settings.

use super::fields::SettingsPatch;
use super::layers::{MAX_BEND_COST, MIN_BEND_COST, clamp_bend_cost, normalize_max_threads};
use super::text::java_double_to_string;

/// Validates and normalises the merged settings in place and returns the
/// warnings (also stored in `validation_warnings`, which Java replaces by a
/// new list on every call).
///
/// Clamps:
/// - `autorouter.max_passes` below 0, or above 9999 and not `i32::MAX`,
///   becomes 0 (unlimited), so `-mp 10000` means unlimited;
/// - a missing or negative `max_threads` becomes `max(1, cores - 1)`, and a
///   value above `cores` becomes `cores`;
/// - `trace_pull_tight_accuracy` below 1 becomes 500;
/// - a layer `bend_cost` is clamped to [0, 9.9] and layer trace costs below
///   0.1 become 0.1.
///
/// Unknown `ignore_net_classes` names are only checked when a board is
/// given (the API path), so this never warns about them.
pub fn validate(patch: &mut SettingsPatch, cores: usize) -> Vec<String> {
    let mut warnings = Vec::new();
    let cores_i = cores.clamp(1, i32::MAX as usize) as i32;
    let default_threads = (cores_i - 1).max(1);

    if let Some(max_passes) = patch.autorouter.max_passes
        && (max_passes < 0 || (max_passes > 9999 && max_passes != i32::MAX))
    {
        warnings.push(format!(
            "Invalid maxPasses value: {max_passes}, using default 0 (no limit)"
        ));
        patch.autorouter.max_passes = Some(0);
    }

    match patch.max_threads {
        None => patch.max_threads = Some(normalize_max_threads(None, cores)),
        Some(v) if v < 0 => {
            warnings.push(format!(
                "Invalid maxThreads value: {v}, using {default_threads}"
            ));
            patch.max_threads = Some(default_threads);
        }
        Some(v) if v > cores_i => {
            warnings.push(format!(
                "Invalid maxThreads value: {v}, capping at {cores_i}"
            ));
            patch.max_threads = Some(cores_i);
        }
        Some(_) => {}
    }

    if let Some(accuracy) = patch.trace_pull_tight_accuracy
        && accuracy < 1
    {
        warnings.push(format!(
            "Invalid tracePullTightAccuracy value: {accuracy}, using default 500"
        ));
        patch.trace_pull_tight_accuracy = Some(500);
    }

    if let Some(layers) = patch.layers.as_mut() {
        for (i, layer) in layers.iter_mut().enumerate() {
            // Not `RangeInclusive::contains`: NaN must not warn, as in Java.
            #[allow(clippy::manual_range_contains)]
            let out_of_range = |bend: f64| bend < MIN_BEND_COST || bend > MAX_BEND_COST;
            if let Some(bend) = layer.bend_cost
                && out_of_range(bend)
            {
                let clamped = clamp_bend_cost(bend);
                warnings.push(format!(
                    "Layer {i} bendCost {} out of range [{}, {}], clamped to {}",
                    java_double_to_string(bend),
                    java_double_to_string(MIN_BEND_COST),
                    java_double_to_string(MAX_BEND_COST),
                    java_double_to_string(clamped)
                ));
                layer.bend_cost = Some(clamped);
            }
            if let Some(cost) = layer.preferred_direction_trace_cost
                && cost < 0.1
            {
                warnings.push(format!(
                    "Layer {i} preferredDirectionTraceCost {} below 0.1, clamped to 0.1",
                    java_double_to_string(cost)
                ));
                layer.preferred_direction_trace_cost = Some(0.1);
            }
            if let Some(cost) = layer.undesired_direction_trace_cost
                && cost < 0.1
            {
                warnings.push(format!(
                    "Layer {i} undesiredDirectionTraceCost {} below 0.1, clamped to 0.1",
                    java_double_to_string(cost)
                ));
                layer.undesired_direction_trace_cost = Some(0.1);
            }
        }
    }

    patch.validation_warnings = Some(warnings.clone());
    warnings
}

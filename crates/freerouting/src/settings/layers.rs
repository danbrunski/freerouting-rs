//! Java's `RouterSettings` layer methods: the setters the `autoroute_settings`
//! reader uses on a [`SettingsPatch`] and the getters the router reads on the
//! resolved [`RouterSettings`].

use super::fields::{RouterSettings, SettingsPatch};
use super::types::LayerSettings;

/// `RouterSettings.MIN_BEND_COST`.
pub const MIN_BEND_COST: f64 = 0.0;
/// `RouterSettings.MAX_BEND_COST`.
pub const MAX_BEND_COST: f64 = 9.9;

/// `Math.max(MIN_BEND_COST, Math.min(MAX_BEND_COST, value))`.
pub fn clamp_bend_cost(value: f64) -> f64 {
    // `Math.max`/`Math.min` return NaN for NaN.
    if value.is_nan() {
        return value;
    }
    MIN_BEND_COST.max(MAX_BEND_COST.min(value))
}

/// Java's `Math.max(double, double)` for the clamps below: NaN wins.
fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

impl SettingsPatch {
    /// `getLayerCount()`.
    pub fn layer_count(&self) -> usize {
        self.layers.as_ref().map_or(0, Vec::len)
    }

    /// `setLayerCount(n)`: every layer routable, the other layer members
    /// unset, both cost arrays `[1.0; n]`.
    pub fn set_layer_count(&mut self, layer_count: usize) {
        if self.layers.as_ref().map(Vec::len) != Some(layer_count) {
            self.costs_initialized = Some(false);
            self.layers = Some(vec![LayerSettings::default(); layer_count]);
        }
        self.scoring.preferred_direction_trace_cost = Some(vec![1.0; layer_count]);
        self.scoring.undesired_direction_trace_cost = Some(vec![1.0; layer_count]);
        if let Some(layers) = self.layers.as_mut() {
            for layer in layers {
                *layer = LayerSettings {
                    routable: Some(true),
                    ..LayerSettings::default()
                };
            }
        }
    }

    fn layer_mut(&mut self, layer: usize) -> Option<&mut LayerSettings> {
        self.layers.as_mut().and_then(|l| l.get_mut(layer))
    }

    /// `setLayerActive`; out of range layers are ignored (Java warns).
    pub fn set_layer_active(&mut self, layer: usize, value: bool) {
        if let Some(l) = self.layer_mut(layer) {
            l.routable = Some(value);
        }
    }

    /// `setPreferredDirectionIsHorizontal`.
    pub fn set_preferred_direction_is_horizontal(&mut self, layer: usize, value: bool) {
        if let Some(l) = self.layer_mut(layer) {
            l.preferred_direction_horizontal = Some(value);
        }
    }

    /// `setBendCost`: clamped to [0, 9.9].
    pub fn set_bend_cost(&mut self, layer: usize, value: f64) {
        if let Some(l) = self.layer_mut(layer) {
            l.bend_cost = Some(clamp_bend_cost(value));
        }
    }

    /// `setPreferredDirectionTraceCosts`: at least 0.1; writes the array
    /// entry and the layer member and marks the costs initialised.
    pub fn set_preferred_direction_trace_costs(&mut self, layer: usize, value: f64) {
        self.set_trace_cost(layer, value, true);
    }

    /// `setAgainstPreferredDirectionTraceCosts`.
    pub fn set_against_preferred_direction_trace_costs(&mut self, layer: usize, value: f64) {
        self.set_trace_cost(layer, value, false);
    }

    fn set_trace_cost(&mut self, layer: usize, value: f64, preferred: bool) {
        let count = self.layer_count();
        if layer >= count {
            return;
        }
        let clamped = java_max(value, 0.1);
        let array = if preferred {
            &mut self.scoring.preferred_direction_trace_cost
        } else {
            &mut self.scoring.undesired_direction_trace_cost
        };
        if array.as_ref().map(Vec::len) != Some(count) {
            *array = Some(vec![0.0; count]);
        }
        if let Some(a) = array.as_mut() {
            a[layer] = clamped;
        }
        if let Some(l) = self.layer_mut(layer) {
            if preferred {
                l.preferred_direction_trace_cost = Some(clamped);
            } else {
                l.undesired_direction_trace_cost = Some(clamped);
            }
        }
        self.costs_initialized = Some(true);
    }

    /// `setViaCosts`: at least 1.
    pub fn set_via_costs(&mut self, value: i32) {
        self.scoring.via_costs = Some(value.max(1));
    }

    /// `setPlaneViaCosts`: at least 1.
    pub fn set_plane_via_costs(&mut self, value: i32) {
        self.scoring.plane_via_costs = Some(value.max(1));
    }

    /// `setStartRipupCosts`: at least 1.
    pub fn set_start_ripup_costs(&mut self, value: i32) {
        self.scoring.start_ripup_costs = Some(value.max(1));
    }

    /// `setViasAllowed`.
    pub fn set_vias_allowed(&mut self, value: bool) {
        self.allowed_via_types = Some(value);
    }

    /// `setRunRouter`.
    pub fn set_run_router(&mut self, value: bool) {
        self.autorouter.enabled = Some(value);
    }

    /// `setRunOptimizer`.
    pub fn set_run_optimizer(&mut self, value: bool) {
        self.optimizer.enabled = Some(value);
    }
}

/// `ExpansionCostFactor`: the cost of 1 mm of horizontal and of vertical
/// trace on a layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceCostFactor {
    pub horizontal: f64,
    pub vertical: f64,
}

impl RouterSettings {
    /// `getLayerCount()`.
    pub fn layer_count(&self) -> usize {
        self.layers.as_ref().map_or(0, Vec::len)
    }

    fn layer(&self, layer: usize) -> Option<&LayerSettings> {
        self.layers.as_ref().and_then(|l| l.get(layer))
    }

    /// `getLayerActive`: unset is routable; out of range is `false`.
    pub fn layer_active(&self, layer: usize) -> bool {
        if layer >= self.layer_count() {
            return false;
        }
        self.layer(layer).and_then(|l| l.routable).unwrap_or(true)
    }

    /// `getBendCost`: the layer's value, else the clamped default; 0 out of
    /// range.
    pub fn bend_cost(&self, layer: usize) -> f64 {
        if layer >= self.layer_count() {
            return 0.0;
        }
        match self.layer(layer).and_then(|l| l.bend_cost) {
            Some(v) => v,
            None => clamp_bend_cost(self.scoring.default_bend_cost),
        }
    }

    /// `getPreferredDirectionIsHorizontal`: unset is `layer % 2 == 1`.
    pub fn preferred_direction_is_horizontal(&self, layer: usize) -> bool {
        if layer >= self.layer_count() {
            return false;
        }
        self.layer(layer)
            .and_then(|l| l.preferred_direction_horizontal)
            .unwrap_or(layer % 2 == 1)
    }

    fn cost_from(&self, layer: usize, preferred: bool) -> f64 {
        if layer >= self.layer_count() {
            return 0.0;
        }
        let explicit = self.layer(layer).and_then(|l| {
            if preferred {
                l.preferred_direction_trace_cost
            } else {
                l.undesired_direction_trace_cost
            }
        });
        if let Some(v) = explicit {
            return v;
        }
        let array = if preferred {
            &self.scoring.preferred_direction_trace_cost
        } else {
            &self.scoring.undesired_direction_trace_cost
        };
        array
            .as_ref()
            .and_then(|a| a.get(layer))
            .copied()
            .unwrap_or(1.0)
    }

    /// `getPreferredDirectionTraceCosts`.
    pub fn preferred_direction_trace_costs(&self, layer: usize) -> f64 {
        self.cost_from(layer, true)
    }

    /// `getAgainstPreferredDirectionTraceCosts`.
    pub fn against_preferred_direction_trace_costs(&self, layer: usize) -> f64 {
        self.cost_from(layer, false)
    }

    /// `getHorizontalTraceCosts`.
    pub fn horizontal_trace_costs(&self, layer: usize) -> f64 {
        if layer >= self.layer_count() {
            return 0.0;
        }
        if self.preferred_direction_is_horizontal(layer) {
            self.preferred_direction_trace_costs(layer)
        } else {
            self.against_preferred_direction_trace_costs(layer)
        }
    }

    /// `getVerticalTraceCosts`.
    pub fn vertical_trace_costs(&self, layer: usize) -> f64 {
        if layer >= self.layer_count() {
            return 0.0;
        }
        if self.preferred_direction_is_horizontal(layer) {
            self.against_preferred_direction_trace_costs(layer)
        } else {
            self.preferred_direction_trace_costs(layer)
        }
    }

    /// `getTraceCosts`: one factor per entry of the preferred-direction
    /// array; empty when that array is unset.
    pub fn trace_costs(&self) -> Vec<TraceCostFactor> {
        let Some(array) = self.scoring.preferred_direction_trace_cost.as_ref() else {
            return Vec::new();
        };
        (0..array.len())
            .map(|i| TraceCostFactor {
                horizontal: self.horizontal_trace_costs(i),
                vertical: self.vertical_trace_costs(i),
            })
            .collect()
    }

    /// `getNeckWidthUm`: the configured width, or 0 when off.
    pub fn neck_width(&self) -> f64 {
        if self.neck_width_um > 0.0 {
            self.neck_width_um
        } else {
            0.0
        }
    }

    /// `getAutorouterMaxThreads`: the nested value (the flat one when the
    /// nested is unset), normalised to `cores`.
    pub fn autorouter_max_threads(&self, cores: usize) -> i32 {
        normalize_max_threads(Some(self.autorouter.max_threads), cores)
    }
}

/// `RouterSettings.normalizeMaxThreads`.
pub fn normalize_max_threads(value: Option<i32>, cores: usize) -> i32 {
    let cores = cores.clamp(1, i32::MAX as usize) as i32;
    let default = (cores - 1).max(1);
    match value {
        None => default,
        Some(v) if v < 0 => default,
        Some(0) => cores,
        Some(v) => v.min(cores),
    }
}

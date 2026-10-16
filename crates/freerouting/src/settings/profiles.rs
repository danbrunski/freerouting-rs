//! Settings profiles for tests and for the reference comparison.

use super::fields::SettingsPatch;

/// Java's `TestingSettings` (priority 80) as the Java fixture tests use it:
/// only `copper_to_edge_clearance_um = 0.0` is set. The optimizer stays on
/// (the default). Ports of Java fixture tests (A14) merge this profile as
/// `Source::Testing`.
///
/// Java's `TestingSettings` setters that only write when nothing else set
/// the value (`setMaxPasses`, `setJobTimeoutString`, `setEnabled`) are
/// *first write wins*; use [`TestingProfile`] for those.
pub fn java_fixture_test() -> SettingsPatch {
    SettingsPatch {
        copper_to_edge_clearance_um: Some(0.0),
        ..SettingsPatch::default()
    }
}

/// Java's `TestingSettings` with its setters.
#[derive(Clone, Debug)]
pub struct TestingProfile {
    pub patch: SettingsPatch,
}

impl Default for TestingProfile {
    fn default() -> Self {
        TestingProfile {
            patch: java_fixture_test(),
        }
    }
}

impl TestingProfile {
    /// `setMaxPasses`: applies only when nothing set it yet.
    pub fn set_max_passes(&mut self, max_passes: i32) {
        if self.patch.autorouter.max_passes.is_none() {
            self.patch.autorouter.max_passes = Some(max_passes);
        }
    }

    /// `setJobTimeoutString`: applies only when nothing set it yet.
    pub fn set_job_timeout(&mut self, timeout: &str) {
        if self.patch.job_timeout.is_none() {
            self.patch.job_timeout = Some(timeout.to_string());
        }
    }

    /// `setEnabled`: applies only when nothing set it yet.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.patch.autorouter.enabled.is_none() {
            self.patch.autorouter.enabled = Some(enabled);
        }
    }

    /// `setRouterEnabled`.
    pub fn set_router_enabled(&mut self, enabled: bool) {
        self.patch.autorouter.enabled = Some(enabled);
    }

    /// `setOptimizerEnabled`.
    pub fn set_optimizer_enabled(&mut self, enabled: bool) {
        self.patch.optimizer.enabled = Some(enabled);
    }

    /// `setFanoutEnabled`.
    pub fn set_fanout_enabled(&mut self, enabled: bool) {
        self.patch.fanout.enabled = Some(enabled);
    }

    /// `setMaxItems`.
    pub fn set_max_items(&mut self, max_items: i32) {
        self.patch.autorouter.max_items = Some(max_items);
    }

    /// `setStrictDrc`.
    pub fn set_strict_drc(&mut self, strict: bool) {
        self.patch.strict_drc = Some(strict);
    }

    /// `setNeckWidthUm`.
    pub fn set_neck_width_um(&mut self, width: f64) {
        self.patch.neck_width_um = Some(width);
    }

    /// `setCopperToEdgeClearanceUm`.
    pub fn set_copper_to_edge_clearance_um(&mut self, value: f64) {
        self.patch.copper_to_edge_clearance_um = Some(value);
    }

    /// `setHoleClearanceUm`.
    pub fn set_hole_clearance_um(&mut self, value: f64) {
        self.patch.hole_clearance_um = Some(value);
    }

    /// `setDefaultBendCost`.
    pub fn set_default_bend_cost(&mut self, value: f64) {
        self.patch.scoring.default_bend_cost = Some(value);
    }

    /// `setBendCost`: grows the layer array to `layer + 1` when needed
    /// (`setLayerCount` resets the layers it creates).
    pub fn set_bend_cost(&mut self, layer: usize, bend_cost: f64) {
        if self.patch.layer_count() <= layer {
            self.patch.set_layer_count(layer + 1);
        }
        self.patch.set_bend_cost(layer, bend_cost);
    }

    /// `setOptimizerMaxPasses`.
    pub fn set_optimizer_max_passes(&mut self, max_passes: i32) {
        self.patch.optimizer.max_passes = Some(max_passes);
    }

    /// `setOptimizerMaxItems`.
    pub fn set_optimizer_max_items(&mut self, max_items: i32) {
        self.patch.optimizer.max_items = Some(max_items);
    }

    /// `setFanoutMaxPasses`.
    pub fn set_fanout_max_passes(&mut self, max_passes: i32) {
        self.patch.fanout.max_passes = Some(max_passes);
    }

    /// `setFanoutMaxItems`.
    pub fn set_fanout_max_items(&mut self, max_items: i32) {
        self.patch.fanout.max_items = Some(max_items);
    }

    /// `setOptimizerEnablePreflightGuards`.
    pub fn set_optimizer_enable_preflight_guards(&mut self, enable: bool) {
        self.patch.optimizer.enable_preflight_guards = Some(enable);
    }

    /// `setOptimizerMaxConsecutiveFailuresPass1`.
    pub fn set_optimizer_max_consecutive_failures_pass1(&mut self, max_failures: i32) {
        self.patch.optimizer.max_consecutive_failures_pass1 = Some(max_failures);
    }
}

/// Java's CLI default: no extra source. C01's `default` scenario merges only
/// `DefaultSettings` and the DSN.
pub fn java_cli_default() -> SettingsPatch {
    SettingsPatch::default()
}

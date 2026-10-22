//! Settings applied to a loaded board (C03): the board-specific trace-cost
//! tuning, the net-class exclusions and [`BoardRouterSettings`], the
//! read-only view the router reads.
//!
//! Port of `RouterSettings.applyBoardSpecificOptimizations`,
//! `applyBoardSpecificOptimizationsIfNeeded` and `applyNetClassExclusions`
//! (Java at `cda540f`), and of the getters the router calls.
//!
//! - The tuning works on the Java-shaped [`SettingsPatch`], because the
//!   `costs_initialized` flag and the two cost arrays decide results.
//! - **One explicit layer cost suppresses the computed costs.** A `layer_rule`
//!   with `preferred_direction_trace_costs` in a DSN or rules file sets
//!   `costs_initialized`. Tuning then skips every layer without an explicit
//!   cost: it keeps the seeded 1.0/1.0, with no aspect add and no outer-layer
//!   add. That is Java's behaviour and is kept (test
//!   `explicit_layer_cost_suppresses_computed_costs`).
//! - The tuning's debug log ("changed parameters") is not ported.

use freerouting_geometry::jmath::java_round;

use super::fields::{RouterSettings, SettingsPatch};
use super::layers::TraceCostFactor;
use crate::autoroute::via_clearance::ViaClearanceMode;
use crate::board::BasicBoard;
use crate::util::equals_ignore_case;

/// `applyBoardSpecificOptimizations(board)`.
///
/// Sizes the layer array and both cost arrays to the board, makes non-signal
/// layers unroutable, fills unset layer members, and, unless the costs were
/// initialised before, computes the per-layer costs from the board's shape:
/// the cost against the preferred direction grows with the aspect ratio of
/// the bounding box, and the outer layers of a board with more than two
/// signal layers cost 0.2 per signal layer more.
pub fn apply_board_specific(patch: &mut SettingsPatch, board: &BasicBoard) {
    let bbox = board.bounding_box();
    let horizontal_width = f64::from(bbox.width());
    let vertical_width = f64::from(bbox.height());
    let layer_count = board.layer_count();

    // Additional costs against the preferred direction, one digit behind the
    // decimal point: `(10 * w) / h`, rounded, then `0.1 *`.
    let horizontal_add = 0.1 * java_round(10.0 * horizontal_width / vertical_width) as f64;
    let vertical_add = 0.1 * java_round(10.0 * vertical_width / horizontal_width) as f64;

    let layers_fit = patch
        .layers
        .as_ref()
        .is_some_and(|layers| layers.len() == layer_count);
    if !layers_fit {
        patch.costs_initialized = Some(false);
        let mut old = patch.layers.take().unwrap_or_default().into_iter();
        patch.layers = Some(
            (0..layer_count)
                .map(|_| old.next().unwrap_or_default())
                .collect(),
        );
    }
    let mut reset = false;
    for array in [
        &mut patch.scoring.preferred_direction_trace_cost,
        &mut patch.scoring.undesired_direction_trace_cost,
    ] {
        if array.as_ref().map(Vec::len) != Some(layer_count) {
            *array = Some(vec![0.0; layer_count]);
            reset = true;
        }
    }
    if reset {
        patch.costs_initialized = Some(false);
    }

    // Null defaults become 1.0 (Java writes them back).
    let default_preferred = *patch
        .scoring
        .default_preferred_direction_trace_cost
        .get_or_insert(1.0);
    let default_undesired = *patch
        .scoring
        .default_undesired_direction_trace_cost
        .get_or_insert(1.0);
    let default_bend = patch.scoring.default_bend_cost.unwrap_or(0.0);

    let mut preferred_is_horizontal = horizontal_width < vertical_width;
    let initialize = patch.costs_initialized != Some(true);

    let (Some(layers), Some(preferred), Some(undesired)) = (
        patch.layers.as_mut(),
        patch.scoring.preferred_direction_trace_cost.as_mut(),
        patch.scoring.undesired_direction_trace_cost.as_mut(),
    ) else {
        return;
    };
    for (i, layer) in layers.iter_mut().enumerate() {
        let is_signal = board.layer_structure.layers[i].is_signal;
        if is_signal {
            preferred_is_horizontal = !preferred_is_horizontal;
            if layer.routable.is_none() {
                layer.routable = Some(true);
            }
        } else {
            layer.routable = Some(false);
        }
        if layer.bend_cost.is_none() {
            layer.bend_cost = Some(default_bend);
        }
        if layer.preferred_direction_horizontal.is_none() {
            layer.preferred_direction_horizontal = Some(preferred_is_horizontal);
        }

        // An explicit cost is kept; otherwise it is computed from the
        // default, plus the aspect penalty against the preferred direction.
        if let Some(cost) = layer.preferred_direction_trace_cost {
            preferred[i] = cost;
        } else if initialize {
            preferred[i] = default_preferred;
        }
        if let Some(cost) = layer.undesired_direction_trace_cost {
            undesired[i] = cost;
        } else if initialize {
            undesired[i] = default_undesired;
            undesired[i] += if preferred_is_horizontal {
                horizontal_add
            } else {
                vertical_add
            };
        }
    }
    if initialize {
        let signal_layers = board.layer_structure.signal_layer_count();
        if signal_layers > 2 {
            let outer_add = 0.2 * signal_layers as f64;
            // Only where not explicitly configured.
            for i in [0, layer_count - 1] {
                if layers[i].preferred_direction_trace_cost.is_none() {
                    preferred[i] += outer_add;
                }
                if layers[i].undesired_direction_trace_cost.is_none() {
                    undesired[i] += outer_add;
                }
            }
        }
        patch.costs_initialized = Some(true);
    }
}

/// `applyBoardSpecificOptimizationsIfNeeded(board)`: tunes only when the
/// layer count differs or the costs were not initialised.
pub fn apply_board_specific_if_needed(patch: &mut SettingsPatch, board: &BasicBoard) {
    if patch.layer_count() != board.layer_count() || patch.costs_initialized != Some(true) {
        apply_board_specific(patch, board);
    }
}

/// `applyNetClassExclusions(board)`: flags every net class whose name
/// matches (ignoring case) a non-blank `ignore_net_classes` entry as
/// ignored by the autorouter.
///
/// Returns warnings for names that match no class. Java warns only on its
/// API path, never here (docs/DIVERGENCES.md, Settings).
pub fn apply_net_class_exclusions(patch: &SettingsPatch, board: &mut BasicBoard) -> Vec<String> {
    let mut warnings = Vec::new();
    let Some(names) = patch.autorouter.ignore_net_classes.as_ref() else {
        return warnings;
    };
    for name in names {
        if name.trim().is_empty() {
            continue;
        }
        let mut found = false;
        for index in 0..board.rules.net_classes.count() {
            let Some(id) = board.rules.net_classes.nth(index) else {
                continue;
            };
            let class = board.rules.net_classes.get_mut(id);
            if equals_ignore_case(class.name(), name) {
                class.is_ignored_by_autorouter = true;
                found = true;
            }
        }
        if !found {
            warnings.push(format!(
                "ignore_net_classes: no net class named '{name}' on the board"
            ));
        }
    }
    warnings
}

/// The trace-cost and direction values of one layer, as the router reads
/// them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerRouting {
    /// `getLayerActive`.
    pub active: bool,
    /// `getPreferredDirectionIsHorizontal`.
    pub preferred_direction_is_horizontal: bool,
    /// `getBendCost`.
    pub bend_cost: f64,
    /// `getPreferredDirectionTraceCosts`.
    pub preferred_direction_trace_costs: f64,
    /// `getAgainstPreferredDirectionTraceCosts`.
    pub against_preferred_direction_trace_costs: f64,
    /// `getHorizontalTraceCosts` and `getVerticalTraceCosts`.
    pub trace_costs: TraceCostFactor,
}

/// The router's read-only view of the final settings (C03.7): the values
/// of Java's getters, computed once per job into plain vectors, with no
/// `Option` on the hot path.
///
/// Layer indices outside `0..layer_count()` read as Java's out-of-range
/// results: inactive, bend cost 0, trace costs 0, horizontal false.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardRouterSettings {
    layers: Vec<LayerRouting>,
    trace_costs: Vec<TraceCostFactor>,
    via_costs: i32,
    plane_via_costs: i32,
    start_ripup_costs: i32,
    vias_allowed: bool,
    automatic_neckdown: bool,
    fanout_min_escape_length_mm: f64,
    fanout_max_escape_length_mm: f64,
    fallback_to_board_vias: bool,
    power_layer_traces: crate::autoroute::power_layer_traces::PowerLayerTraces,
    pour_fill_target: bool,
    trace_pull_tight_accuracy: i32,
    neck_width_um: f64,
    min_trace_width_um: f64,
    board_minimums: BoardMinimums,
    strict_drc: bool,
    via_clearance: ViaClearanceMode,
    java_quirks: super::java_quirks::JavaQuirks,
}

const OUT_OF_RANGE: LayerRouting = LayerRouting {
    active: false,
    preferred_direction_is_horizontal: false,
    bend_cost: 0.0,
    preferred_direction_trace_costs: 0.0,
    against_preferred_direction_trace_costs: 0.0,
    trace_costs: TraceCostFactor {
        horizontal: 0.0,
        vertical: 0.0,
    },
};

impl BoardRouterSettings {
    /// Reads the getters of the final `settings` once.
    pub fn from_settings(settings: &RouterSettings) -> BoardRouterSettings {
        let layers = (0..settings.layer_count())
            .map(|i| LayerRouting {
                active: settings.layer_active(i),
                preferred_direction_is_horizontal: settings.preferred_direction_is_horizontal(i),
                bend_cost: settings.bend_cost(i),
                preferred_direction_trace_costs: settings.preferred_direction_trace_costs(i),
                against_preferred_direction_trace_costs: settings
                    .against_preferred_direction_trace_costs(i),
                trace_costs: TraceCostFactor {
                    horizontal: settings.horizontal_trace_costs(i),
                    vertical: settings.vertical_trace_costs(i),
                },
            })
            .collect();
        BoardRouterSettings {
            layers,
            trace_costs: settings.trace_costs(),
            via_costs: settings.scoring.via_costs,
            plane_via_costs: settings.scoring.plane_via_costs,
            start_ripup_costs: settings.scoring.start_ripup_costs,
            vias_allowed: settings.allowed_via_types,
            // Q02.2: strict sizes turn both neck-downs off.
            automatic_neckdown: settings.automatic_neckdown
                && !crate::quality::neckdown::ledger::is_strict(settings),
            fanout_min_escape_length_mm: settings.fanout.min_escape_length_mm,
            fanout_max_escape_length_mm: settings.fanout.max_escape_length_mm,
            fallback_to_board_vias: settings.fanout.fallback_to_board_vias,
            power_layer_traces: crate::autoroute::power_layer_traces::PowerLayerTraces::of(
                settings,
            ),
            pour_fill_target: settings.pour_fill_target == Some(true),
            trace_pull_tight_accuracy: settings.trace_pull_tight_accuracy,
            neck_width_um: if crate::quality::neckdown::ledger::is_strict(settings) {
                0.0
            } else {
                settings.neck_width()
            },
            min_trace_width_um: settings
                .min_trace_width_um
                .filter(|w| *w > 0.0)
                .unwrap_or(0.0),
            board_minimums: BoardMinimums::of(settings),
            strict_drc: settings.strict_drc,
            via_clearance: ViaClearanceMode::of(settings),
            java_quirks: super::java_quirks::JavaQuirks::from_settings(settings),
        }
    }

    /// `getLayerCount()`.
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    fn layer(&self, layer: usize) -> &LayerRouting {
        self.layers.get(layer).unwrap_or(&OUT_OF_RANGE)
    }

    /// `getLayerActive`.
    pub fn layer_active(&self, layer: usize) -> bool {
        self.layer(layer).active
    }

    /// `getBendCost`: the layer value, else the clamped default.
    pub fn bend_cost(&self, layer: usize) -> f64 {
        self.layer(layer).bend_cost
    }

    /// `getPreferredDirectionIsHorizontal`.
    pub fn preferred_direction_is_horizontal(&self, layer: usize) -> bool {
        self.layer(layer).preferred_direction_is_horizontal
    }

    /// `getPreferredDirectionTraceCosts`: the explicit layer value, then
    /// the array entry, then 1.0.
    pub fn preferred_direction_trace_costs(&self, layer: usize) -> f64 {
        self.layer(layer).preferred_direction_trace_costs
    }

    /// `getAgainstPreferredDirectionTraceCosts`.
    pub fn against_preferred_direction_trace_costs(&self, layer: usize) -> f64 {
        self.layer(layer).against_preferred_direction_trace_costs
    }

    /// `getHorizontalTraceCosts`.
    pub fn horizontal_trace_costs(&self, layer: usize) -> f64 {
        self.layer(layer).trace_costs.horizontal
    }

    /// `getVerticalTraceCosts`.
    pub fn vertical_trace_costs(&self, layer: usize) -> f64 {
        self.layer(layer).trace_costs.vertical
    }

    /// `getTraceCosts`: one factor per entry of the preferred-direction
    /// array (the array length is the count).
    pub fn trace_costs(&self) -> &[TraceCostFactor] {
        &self.trace_costs
    }

    /// `getViaCosts`.
    pub fn via_costs(&self) -> i32 {
        self.via_costs
    }

    /// `getPlaneViaCosts`.
    pub fn plane_via_costs(&self) -> i32 {
        self.plane_via_costs
    }

    /// `getStartRipupCosts`.
    pub fn start_ripup_costs(&self) -> i32 {
        self.start_ripup_costs
    }

    /// `getViasAllowed`.
    pub fn vias_allowed(&self) -> bool {
        self.vias_allowed
    }

    /// `getAutomaticNeckdown`.
    pub fn automatic_neckdown(&self) -> bool {
        self.automatic_neckdown
    }

    /// `fanout.min_escape_length_mm`.
    pub fn fanout_min_escape_length_mm(&self) -> f64 {
        self.fanout_min_escape_length_mm
    }

    /// `fanout.max_escape_length_mm`.
    pub fn fanout_max_escape_length_mm(&self) -> f64 {
        self.fanout_max_escape_length_mm
    }

    /// `fanout.fallback_to_board_vias`.
    pub fn fallback_to_board_vias(&self) -> bool {
        self.fallback_to_board_vias
    }

    /// `router.power_layer_traces` (Q13): where a plane net's traces may
    /// lie on a power layer.
    pub fn power_layer_traces(&self) -> crate::autoroute::power_layer_traces::PowerLayerTraces {
        self.power_layer_traces
    }

    /// `router.pour_fill_target` (K15.25): the maze search reaches a
    /// `.kicad_pcb` signal-layer pour only where its opened fill reaches.
    pub fn pour_fill_target(&self) -> bool {
        self.pour_fill_target
    }

    /// `tracePullTightAccuracy`.
    pub fn trace_pull_tight_accuracy(&self) -> i32 {
        self.trace_pull_tight_accuracy
    }

    /// `getNeckWidthUm`: the configured width, or 0 when off.
    pub fn neck_width_um(&self) -> f64 {
        self.neck_width_um
    }

    /// `getMinTraceWidthUm` (Java `46bf9e2ef`, after the pin): the narrowest
    /// trace the router may create in micrometres, or 0 for no limit.
    pub fn min_trace_width_um(&self) -> f64 {
        self.min_trace_width_um
    }

    /// `router.board_minimums.*` (Q18): the board's minimum manufacturing
    /// rules, the hard floors of every routed trace and via.
    pub fn board_minimums(&self) -> BoardMinimums {
        self.board_minimums
    }

    /// These settings with the board minimums `minimums` (Q18), for callers
    /// that build the settings by hand.
    pub fn with_board_minimums(&self, minimums: BoardMinimums) -> BoardRouterSettings {
        BoardRouterSettings {
            board_minimums: minimums,
            ..self.clone()
        }
    }

    /// `strictDrc`.
    pub fn strict_drc(&self) -> bool {
        self.strict_drc
    }

    /// `router.via_clearance` (Q14): where a routed via's clearance class
    /// comes from.
    pub fn via_clearance(&self) -> ViaClearanceMode {
        self.via_clearance
    }

    /// `router.java_quirks.*` (N06): the Java bugs kept, one switch each;
    /// [`AutorouteControl::quirks`](crate::autoroute::control::AutorouteControl::quirks)
    /// copies it.
    pub fn java_quirks(&self) -> super::java_quirks::JavaQuirks {
        self.java_quirks
    }

    /// These settings with the switches `quirks` (N06), for callers that build the
    /// settings by hand.
    pub fn with_java_quirks(&self, quirks: super::java_quirks::JavaQuirks) -> BoardRouterSettings {
        BoardRouterSettings {
            java_quirks: quirks,
            ..self.clone()
        }
    }
}

/// The board's minimum manufacturing rules (`router.board_minimums.*`, Q18),
/// in micrometres; 0 is unknown.
///
/// They are hard floors, not preferences: every trace the router creates,
/// neck-downs included, is at least [`track_width_um`](Self::track_width_um)
/// wide, and a via type whose diameter, drill or annular ring is below
/// [`via_diameter_um`](Self::via_diameter_um),
/// [`via_drill_um`](Self::via_drill_um) or
/// [`via_annular_um`](Self::via_annular_um) is routed at the floor size
/// ([`crate::job::via_minimums`]). With the track minimum unknown and
/// [`from_classes`](Self::from_classes) on (profile `enhanced`), the
/// narrowest class width of the design is the track minimum. All unknown
/// (profile `legacy`, nothing supplied) is Java's behaviour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoardMinimums {
    /// The minimum track width (KiCad `m_TrackMinWidth`).
    pub track_width_um: f64,
    /// The minimum via diameter (KiCad `m_ViasMinSize`).
    pub via_diameter_um: f64,
    /// The minimum through-hole drill (KiCad `m_MinThroughDrill`).
    pub via_drill_um: f64,
    /// The minimum via annular ring (KiCad `m_ViasMinAnnularWidth`).
    pub via_annular_um: f64,
    /// The narrowest class width stands for an unknown track minimum.
    pub from_classes: bool,
}

impl BoardMinimums {
    /// The minimums of `settings`; a value that is not positive is unknown.
    pub fn of(settings: &RouterSettings) -> BoardMinimums {
        let m = &settings.board_minimums;
        let known = |v: Option<f64>| v.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(0.0);
        BoardMinimums {
            track_width_um: known(m.track_width_um),
            via_diameter_um: known(m.via_diameter_um),
            via_drill_um: known(m.via_drill_um),
            via_annular_um: known(m.via_annular_um),
            from_classes: m.from_classes.unwrap_or(false),
        }
    }

    /// True if no minimum is known and none is derived: Java's behaviour.
    pub fn is_off(&self) -> bool {
        self.track_width_um <= 0.0
            && self.via_diameter_um <= 0.0
            && self.via_drill_um <= 0.0
            && self.via_annular_um <= 0.0
            && !self.from_classes
    }
}

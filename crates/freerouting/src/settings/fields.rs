//! The settings field table: the one definition of every router setting.
//!
//! One `settings_table!` invocation lists each field once, with its JSON
//! name, Java field name, Gson alternates, flat alias, Java type, default,
//! unset-fallback and doc line. The macro generates
//! - [`SettingsPatch`], Java's `RouterSettings` with every member nullable,
//!   which is what every settings source produces;
//! - [`RouterSettings`], the resolved view that everything else reads;
//! - [`FIELDS`] and [`SECTIONS`], the table the setter, the merge, the JSON
//!   reader, the environment source, the comparison dump, the CLI help and
//!   the GUI settings form walk.
//!
//! The members of Java's `RouterSettings` that are not scalar fields are
//! written by hand next to the generated structs: `layers`,
//! `costs_initialized` (Java's transient `boardSpecificTraceCostsApplied`),
//! `validation_warnings` and `improvement_threshold_scaled` (not Java: the
//! command line's fraction scaling, N06.12). The two per-layer cost arrays are table
//! entries (`scoring.preferred_direction_trace_cost` and
//! `scoring.undesired_direction_trace_cost`), marked as not JSON.
//!
//! Field kinds in the table: `req` (an unset value resolves to the default),
//! `fb` (an unset value resolves to a separate fallback, Java's getter
//! fallbacks such as `getViaCosts()` = 1) and `opt` (the resolved view keeps
//! the `Option`).

use super::types::{
    BoardUpdateStrategy, FieldDesc, FieldType, Flat, ItemSelectionStrategy, LayerSettings,
    OptimizerScoringVersion, RouterScoringVersion, SectionDesc,
};

macro_rules! resolved_ty {
    (req $t:ty) => {
        $t
    };
    (fb $t:ty) => {
        $t
    };
    (opt $t:ty) => {
        Option<$t>
    };
}

macro_rules! resolve_val {
    (req, $e:expr, $d:expr, $fb:expr) => {
        match &$e {
            Some(v) => v.clone(),
            None => $d.expect("every `req` field has a default"),
        }
    };
    (fb, $e:expr, $d:expr, $fb:expr) => {
        match &$e {
            Some(v) => v.clone(),
            None => $fb,
        }
    };
    (opt, $e:expr, $d:expr, $fb:expr) => {
        $e.clone()
    };
}

macro_rules! is_optional {
    (req) => {
        false
    };
    (fb) => {
        false
    };
    (opt) => {
        true
    };
}

macro_rules! settings_table {
    (
        cores $cores:ident;
        top {
            $( ($tn:ident, $tt:ty, $tk:ident, $td:expr, $tfb:expr, $tj:literal,
                [$($ta:literal),*], $tflat:expr, $tjson:literal, $tdoc:literal) ),* $(,)?
        }
        sections {
            $( $sec:ident, $Patch:ident, $Res:ident, $secj:literal, $secdoc:literal {
                $( ($n:ident, $t:ty, $k:ident, $d:expr, $fb:expr, $j:literal,
                    [$($a:literal),*], $flat:expr, $json:literal, $doc:literal) ),* $(,)?
            } )*
        }
    ) => {
        $(
            #[doc = $secdoc]
            #[derive(Clone, Debug, Default, PartialEq)]
            pub struct $Patch {
                $( #[doc = $doc] pub $n: Option<$t>, )*
            }

            #[doc = $secdoc]
            #[derive(Clone, Debug, PartialEq)]
            pub struct $Res {
                $( #[doc = $doc] pub $n: resolved_ty!($k $t), )*
            }
        )*

        /// Java's `RouterSettings` with every member nullable: what one
        /// settings source says. `None` means "no opinion".
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct SettingsPatch {
            $( #[doc = $tdoc] pub $tn: Option<$tt>, )*
            $( #[doc = $secdoc] pub $sec: $Patch, )*
            /// Per-layer settings (Java's transient `layers`).
            pub layers: Option<Vec<LayerSettings>>,
            /// Java's transient `boardSpecificTraceCostsApplied`: the
            /// per-layer costs were initialised or set explicitly and
            /// `applyBoardSpecificOptimizations` must not overwrite them.
            pub costs_initialized: Option<bool>,
            /// Java's `validationWarnings`.
            pub validation_warnings: Option<Vec<String>>,
            /// Not Java (N06.12, a port fix): `Some(true)` when the command line read
            /// `optimizer.improvement_threshold` as a fraction and stored its percentage
            /// ([`SettingsPatch::cli_property`]), so the optimizer's own fraction
            /// heuristic (`java_quirks.opt_threshold_fraction`) must not scale it again.
            /// It travels with the threshold: a source that sets the threshold also sets
            /// this flag (`None` for every source but the command line).
            pub improvement_threshold_scaled: Option<bool>,
        }

        /// The resolved router settings: the merged and validated
        /// [`SettingsPatch`] read through Java's getter fallbacks.
        #[derive(Clone, Debug, PartialEq)]
        pub struct RouterSettings {
            $( #[doc = $tdoc] pub $tn: resolved_ty!($tk $tt), )*
            $( #[doc = $secdoc] pub $sec: $Res, )*
            /// Per-layer settings, `None` before the layer count is known.
            pub layers: Option<Vec<LayerSettings>>,
            /// Java's `boardSpecificTraceCostsApplied` (`null` is `false`).
            pub costs_initialized: bool,
            /// Warnings from validation.
            pub validation_warnings: Vec<String>,
            /// The command line already scaled `optimizer.improvement_threshold` from a
            /// fraction ([`SettingsPatch::improvement_threshold_scaled`]; `null` is
            /// `false`).
            pub improvement_threshold_scaled: bool,
        }

        /// `DefaultSettings.getSettings()` for a machine with `cores`
        /// processors.
        #[allow(unused_variables)]
        pub fn default_patch($cores: usize) -> SettingsPatch {
            SettingsPatch {
                $( $tn: $td, )*
                $( $sec: $Patch { $( $n: $d, )* }, )*
                layers: None,
                costs_initialized: None,
                validation_warnings: None,
                improvement_threshold_scaled: None,
            }
        }

        impl RouterSettings {
            /// Reads a merged patch through the fallbacks of Java's getters.
            #[allow(unused_variables)]
            pub fn from_patch(p: &SettingsPatch, $cores: usize) -> RouterSettings {
                RouterSettings {
                    $( $tn: resolve_val!($tk, p.$tn, $td, $tfb), )*
                    $( $sec: $Res {
                        $( $n: resolve_val!($k, p.$sec.$n, $d, $fb), )*
                    }, )*
                    layers: p.layers.clone(),
                    costs_initialized: p.costs_initialized.unwrap_or(false),
                    validation_warnings: p.validation_warnings.clone().unwrap_or_default(),
                    improvement_threshold_scaled: p.improvement_threshold_scaled.unwrap_or(false),
                }
            }
        }

        /// The sections (nested objects) of the settings.
        pub static SECTIONS: &[SectionDesc] = &[
            $( SectionDesc { name: stringify!($sec), java_name: $secj }, )*
        ];

        /// Every scalar settings field, top-level ones first, then the
        /// sections in order.
        #[allow(unused_variables)]
        pub static FIELDS: &[FieldDesc] = &[
            $( FieldDesc {
                path: stringify!($tn),
                section: "",
                name: stringify!($tn),
                java_name: $tj,
                alternates: &[$($ta),*],
                flat: $tflat,
                kind: <$tt as FieldType>::KIND,
                json: $tjson,
                optional: is_optional!($tk),
                doc: $tdoc,
                get: |p| p.$tn.clone().map(<$tt as FieldType>::into_value),
                set: |p, v| {
                    if let Some(x) = <$tt as FieldType>::from_value(v) {
                        p.$tn = Some(x);
                    }
                },
                clear: |p| p.$tn = None,
                default: |$cores| $td.map(<$tt as FieldType>::into_value),
            }, )*
            $( $(
                FieldDesc {
                    path: concat!(stringify!($sec), ".", stringify!($n)),
                    section: stringify!($sec),
                    name: stringify!($n),
                    java_name: $j,
                    alternates: &[$($a),*],
                    flat: $flat,
                    kind: <$t as FieldType>::KIND,
                    json: $json,
                    optional: is_optional!($k),
                    doc: $doc,
                    get: |p| p.$sec.$n.clone().map(<$t as FieldType>::into_value),
                    set: |p, v| {
                        if let Some(x) = <$t as FieldType>::from_value(v) {
                            p.$sec.$n = Some(x);
                        }
                    },
                    clear: |p| p.$sec.$n = None,
                    default: |$cores| $d.map(<$t as FieldType>::into_value),
                },
            )* )*
        ];
    };
}

/// Java's `Math.max(1, availableProcessors - 1)`.
fn default_threads(cores: usize) -> i32 {
    cores.saturating_sub(1).clamp(1, i32::MAX as usize) as i32
}

settings_table! {
    cores cores;
    top {
        (profile, String, opt, None, None, "profile", [], Flat::No, true,
            "Routing profile (N01): `legacy` (Java Freerouting's results) or `enhanced` (unset; the accepted improvements)."),
        (job_timeout, String, opt, Some("12:00:00".to_string()), None, "jobTimeoutString", [], Flat::No, true,
            "Maximum job duration as `HH:mm:ss`, `mm:ss` or `ss`; capped at 24 h."),
        (trace_pull_tight_accuracy, i32, req, Some(500), 0, "tracePullTightAccuracy",
            ["tracePullTightAccuracy"], Flat::No, true,
            "The accuracy of the pull tight algorithm."),
        (allowed_via_types, bool, req, Some(true), true, "viasAllowed", [], Flat::No, true,
            "Whether the autorouter may place vias."),
        (automatic_neckdown, bool, fb, Some(true), false, "automaticNeckdown",
            ["automaticNeckdown"], Flat::No, true,
            "Lower the trace width at static pins automatically if necessary."),
        (max_threads, i32, req, Some(default_threads(cores)), 0, "maxThreads", [], Flat::No, true,
            "Maximum worker threads (headless routing never reads it)."),
        (copper_to_edge_clearance_um, f64, req, Some(250.0), 0.0, "copperToEdgeClearanceUm", [],
            Flat::No, true, "Copper to board edge clearance in micrometers."),
        (edge_cap_skips_off_board_pins, bool, opt, None, None, "edgeCapSkipsOffBoardPins", [],
            Flat::No, true,
            "The default copper-to-edge clearance is capped at the smallest gap between a pin and the outline. unset or `false` (Java; both profiles) counts a pin that lies wholly outside the outline, which caps the clearance at 0; `true` (opt-in, K15.7) leaves such pins out and logs a warning naming each."),
        (hole_clearance_um, f64, req, Some(0.0), 0.0, "holeClearanceUm", [], Flat::No, true,
            "Hole clearance in micrometers."),
        (solder_mask_expansions, String, opt, None, None, "solderMaskExpansions", [], Flat::No, true,
            "Pads whose solder-mask aperture reaches past their clearance, as `<component>:<pin>=<front um>,<back um>` entries separated by `;`: other nets' traces and vias keep at least that far from the pad on that outer layer (Q18.7). The KiCad plugin and a `.kicad_pcb` input fill it in; unset (the default) changes nothing."),
        (clearance_tolerance_um, f64, req, Some(1.0), 0.0, "clearanceToleranceUm", [], Flat::No, true,
            "Clearance shortfalls up to this value are treated as rounding noise (micrometers)."),
        (plane_nets, Vec<String>, req, Some(Vec::new()), Vec::new(), "planeNets", [], Flat::No, true,
            "Net names to treat as power-plane nets."),
        (plane_as_obstacle, bool, req, Some(false), false, "planeAsObstacle",
            ["conduction_is_obstacle", "planeAsObstacle", "conductionIsObstacle"], Flat::No, true,
            "Conduction areas block foreign traces."),
        (power_layer_traces, String, opt, None, None, "powerLayerTraces", [], Flat::No, true,
            "Where a plane net's traces may lie on a power layer: `layer` (unset, Java: anywhere on it), `own_plane` (inside its own plane; Q13) or `none` (no trace on a power layer; vias still land in their plane; profile `enhanced`'s value, N05)."),
        (plane_obstacle_net_classes, Vec<String>, opt, None, None, "planeObstacleNetClasses", [],
            Flat::No, true,
            "Net classes whose signal-layer pours block the nets with no pin or via inside them (Q01)."),
        (plane_obstacle_block, String, opt, None, None, "planeObstacleBlock", [], Flat::No, true,
            "What those pours block: `traces_and_vias` (unset, Q01) or `traces` (foreign vias pass; Q09.4)."),
        (plane_obstacle_all_pours, bool, opt, None, None, "planeObstacleAllPours", [], Flat::No, true,
            "Every signal-layer pour, whatever its net class, blocks the nets with no pin or via inside it, as a pour of a class in `plane_obstacle_net_classes` does (K15.13); unset is off."),
        (wire_keepout, String, opt, None, None, "wireKeepout", [], Flat::No, true,
            "A Specctra `wire_keepout` is a `keepout` (unset, Java) or `traces_only` (vias pass; Q09.3)."),
        (via_clearance, String, opt, None, None, "viaClearance", [], Flat::No, true,
            "A routed via's clearance class: `via_info` (unset, Java: its via type's; profile `legacy`) or `net_class` (its net class's, KiCad's rule; Q14); profile `enhanced` uses `net_class` (N05.3)."),
        (neck_width_um, f64, req, Some(0.0), 0.0, "neckWidthUm", [], Flat::No, true,
            "Retry a failed connection with traces clamped to this width (micrometers), never below `min_trace_width_um`; 0 is off (Java's default, profile `legacy`); profile `enhanced` uses 300 (N05.3)."),
        (min_trace_width_um, f64, opt, None, None, "minTraceWidthUm", [], Flat::No, true,
            "Opt-in floor for every narrowing (micrometers); unset is off."),
        (strict_drc, bool, req, Some(false), false, "strictDrc", [], Flat::No, true,
            "Reject connections that introduce clearance violations."),
        (result_json, String, opt, None, None, "resultJsonPath", [], Flat::No, true,
            "Path of a machine-readable routing result manifest."),
        (room_order, String, opt, None, None, "roomOrder", [], Flat::No, true,
            "Room completion order (N03.1): `tree` (Java's, the default) or `canonical` (independent of the search tree's shape)."),
        (work_unit, String, opt, None, None, "workUnit", [], Flat::No, true,
            "Work limit unit (N03.1): `nodes` (Java's search-tree nodes visited, the default) or `leaves` (leaves returned)."),
        (pad_contacts, String, opt, None, None, "padContacts", [], Flat::No, true,
            "Where a trace end connects to a pin (N04): `center` (unset, Java: only at the pin's centre) or `inside_pad` (wherever the trace's round end overlaps the pin's pad on the trace's layer, KiCad's rule)."),
        (output_check, String, opt, None, None, "outputCheck", [], Flat::No, true,
            "The board-rule check before saving (Q20): `off` (unset, Java), `report` (log and count the routed copper that breaks a board rule) or `repair` (remove it, retry its connections once, remove what still breaks a rule; profile `enhanced`'s value)."),
        (pour_fill_target, bool, opt, None, None, "pourFillTarget", [], Flat::No, true,
            "On a `.kicad_pcb` input, the maze search reaches a signal-layer pour only where its fill, opened by the zone's minimum thickness, reaches (K15.25); unset or `false` is Java's rule (anywhere inside the pour's outline); unset in both profiles (opt-in, K15.25's note)."),
        (pour_fill_contacts, bool, opt, None, None, "pourFillContacts", [], Flat::No, true,
            "On a `.kicad_pcb` input, the router's connectivity (the incomplete count and the connections it routes) joins a pin, via or trace end to a signal-layer pour only where the pour's fill, opened by the zone's minimum thickness, reaches it, and each island of that fill only to its own contacts (K15.31); unset or `false` is Java's rule (anywhere inside the pour's outline); unset in both profiles (opt-in, its default needs a measurement)."),
    }
    sections {
        autorouter, AutorouterPatch, AutorouterSettings, "autorouter",
        "Execution settings for the autorouter stage." {
            (enabled, bool, req, Some(true), true, "enabled", [], Flat::Deprecated, true,
                "Whether the autorouter stage runs."),
            (algorithm, String, req, Some("freerouting-router".to_string()), String::new(), "algorithm",
                [], Flat::Deprecated, true, "Algorithm identifier."),
            (max_passes, i32, req, Some(0), 0, "maxPasses", [], Flat::Deprecated, true,
                "Maximum autorouter passes (0 means unlimited)."),
            (max_items, i32, req, Some(i32::MAX), 0, "maxItems", [], Flat::Deprecated, true,
                "Maximum items attempted in the autorouter stage."),
            (max_threads, i32, req, Some(default_threads(cores)), 0, "maxThreads", [], Flat::No, true,
                "Maximum worker threads for the autorouter stage."),
            (save_intermediate_stages, bool, req, Some(false), false, "saveIntermediateStages", [],
                Flat::Deprecated, true, "Save intermediate board snapshots between passes."),
            (ignore_net_classes, Vec<String>, req, Some(Vec::new()), Vec::new(), "ignoreNetClasses", [],
                Flat::Deprecated, true, "Net class names the autorouter skips."),
            (ordering, String, opt, None, None, "ordering", [], Flat::No, true,
                "Net ordering stage id (A10.2); unset uses the `algorithm` bundle's."),
            (connection_router, String, opt, None, None, "connectionRouter", [], Flat::No, true,
                "Connection router stage id (A10.2); unset uses the `algorithm` bundle's."),
            (ripup_policy, String, opt, None, None, "ripupPolicy", [], Flat::No, true,
                "Ripup policy stage id (A10.2); unset uses the `algorithm` bundle's."),
            (pass_controller, String, opt, None, None, "passController", [], Flat::No, true,
                "Pass controller stage id (A10.2); unset uses the `algorithm` bundle's."),
            (sweep, String, opt, None, None, "sweep", [], Flat::No, true,
                "Add-only completion sweep after the passes: `off` (unset), `fixed` or `free` (Q10)."),
            (sweep_passes, i32, opt, None, None, "sweepPasses", [], Flat::No, true,
                "Most add-only sweep passes; unset is 2 (Q10)."),
            (sweep_after_optimizer, bool, opt, None, None, "sweepAfterOptimizer", [], Flat::No, true,
                "Run the sweep after the optimizer, then optimize again (Q10.3); unset is false."),
            (output_guard, bool, opt, None, None, "outputGuard", [], Flat::No, true,
                "Never end worse than the input on completed connections or unconnected pins: fall back to the best history board that isn't, else the input (Q04.3; unset: off)."),
            (restarts_from_best, i32, opt, None, None, "restartsFromBest", [], Flat::No, true,
                "Restarts from the best board after the passes: the fanout, then the passes again from pass 1, each kept only if it beats the best board, stopping at the first that doesn't (Q23); unset, 0 or negative is off."),
        }
        fanout, FanoutPatch, FanoutSettings, "fanout",
        "Configuration of the SMD-pin fanout pre-pass." {
            (enabled, bool, fb, Some(true), false, "enabled", [], Flat::No, true,
                "Whether the fanout pre-pass runs."),
            (max_passes, i32, req, Some(20), 0, "maxPasses", [], Flat::No, true,
                "Maximum fanout passes."),
            (max_items, i32, req, Some(i32::MAX), 0, "maxItems", [], Flat::No, true,
                "Maximum items attempted in the fanout stage."),
            (max_milliseconds_per_pin, i64, req, Some(10000), 0, "maxMillisecondsPerPin", [],
                Flat::No, true,
                "Per-pin budget in nominal milliseconds (64,000 search-tree node visits each)."),
            (ripup_allowed, bool, req, Some(true), true, "ripupAllowed", ["ripupAllowed"], Flat::No, true,
                "Whether fanout may rip up traces."),
            (min_escape_length_mm, f64, req, Some(2.5), 0.0, "minEscapeLengthMm", [], Flat::No, true,
                "Minimum escape length in millimeters."),
            (max_escape_length_mm, f64, req, Some(4.5), 0.0, "maxEscapeLengthMm", [], Flat::No, true,
                "Maximum escape length in millimeters."),
            (start_via_diameter_mm, f64, req, Some(0.25), 0.0, "startViaDiameterMm", [], Flat::No, true,
                "Start via diameter in millimeters (read by nothing in Java)."),
            (end_via_diameter_mm, f64, req, Some(0.25), 0.0, "endViaDiameterMm", [], Flat::No, true,
                "End via diameter in millimeters (read by nothing in Java)."),
            (pin_sorting_order, String, req, Some("outer_first".to_string()), String::new(),
                "pinSortingOrder", [], Flat::No, true, "Order in which pins are fanned out."),
            (fallback_to_board_vias, bool, req, Some(true), true, "fallbackToBoardVias", [], Flat::No, true,
                "Fall back to the board's vias."),
            (timeout, String, opt, None, None, "timeoutString", [], Flat::No, true,
                "Fanout time limit as `HH:mm:ss`, `mm:ss` or `ss`."),
            (algorithm, String, opt, None, None, "algorithm", [], Flat::No, true,
                "Fanout stage id (A10.2); unset is `freerouting-fanout`."),
            (pin_escape, String, opt, None, None, "pinEscape", [], Flat::No, true,
                "Straight escape stubs at the board minimum width on fine-pitch QFN, QFP and SOIC pins before the fanout: `off` (unset) or `straight` (Q21)."),
        }
        optimizer, OptimizerPatch, OptimizerSettings, "optimizer",
        "Configuration of the post-route optimizer." {
            (enabled, bool, fb, Some(true), false, "enabled", [], Flat::No, true,
                "Whether the optimizer runs."),
            (algorithm, String, req, Some("freerouting-optimizer".to_string()), String::new(),
                "algorithm", [], Flat::No, true, "Algorithm identifier."),
            (max_passes, i32, req, Some(100), 0, "maxPasses", [], Flat::No, true,
                "Maximum optimizer passes."),
            (max_items, i32, req, Some(i32::MAX), 0, "maxItems", [], Flat::No, true,
                "Maximum items attempted in the optimizer stage."),
            (max_threads, i32, req, Some(default_threads(cores)), 0, "maxThreads", [], Flat::No, true,
                "Maximum worker threads for the optimizer."),
            (improvement_threshold, f32, req, Some(2.5), 0.0, "optimizationImprovementThreshold", [],
                Flat::No, true, "Stop when a pass improves the score by less than this percentage."),
            (enable_preflight_guards, bool, req, Some(true), true, "enablePreflightGuards", [],
                Flat::No, true, "Enable the optimizer's pre-flight guards."),
            (max_consecutive_failures, i32, req, Some(50), 0, "maxConsecutiveFailures", [],
                Flat::No, true, "Abort a pass after this many consecutive failures."),
            (max_consecutive_failures_pass1, i32, req, Some(12), 0, "maxConsecutiveFailuresPass1", [],
                Flat::No, true, "The same limit for pass 1."),
            (additional_ripup_cost_factor_at_start, i32, req, Some(10), 0,
                "additionalRipupCostFactorAtStart", [], Flat::No, true,
                "Extra ripup cost factor for the first passes."),
            (trace_ripup_cost_factor, f32, req, Some(0.6), 0.0, "traceRipupCostFactor", [], Flat::No, true,
                "Ripup cost factor for traces."),
            (max_autoroute_passes, i32, req, Some(6), 0, "maxAutoroutePasses", [], Flat::No, true,
                "Autoroute passes per optimizer item."),
            (board_update_strategy, BoardUpdateStrategy, req, Some(BoardUpdateStrategy::GlobalOptimal),
                BoardUpdateStrategy::GlobalOptimal, "boardUpdateStrategy", [], Flat::No, false,
                "How the optimizer applies improvements (not read from JSON)."),
            (item_selection_strategy, ItemSelectionStrategy, req,
                Some(ItemSelectionStrategy::Sequential), ItemSelectionStrategy::Sequential,
                "itemSelectionStrategy", [], Flat::No, false,
                "How the optimizer picks items (not read from JSON)."),
            (timeout, String, opt, None, None, "timeoutString", [], Flat::No, true,
                "Optimizer time limit as `HH:mm:ss`, `mm:ss` or `ss`."),
        }
        scoring, ScoringPatch, ScoringSettings, "scoring",
        "Routing costs (Java's `RoutingCostSettings`)." {
            (preferred_direction_trace_cost, Vec<f64>, opt, None, None, "preferredDirectionTraceCost",
                [], Flat::No, false, "Per-layer cost of 1 mm in the preferred direction (not JSON)."),
            (undesired_direction_trace_cost, Vec<f64>, opt, None, None, "undesiredDirectionTraceCost",
                [], Flat::No, false, "Per-layer cost of 1 mm in the undesired direction (not JSON)."),
            (default_preferred_direction_trace_cost, f64, req, Some(1.0), 1.0,
                "defaultPreferredDirectionTraceCost", [], Flat::Quiet, true,
                "Default cost of 1 mm in the preferred direction."),
            (default_undesired_direction_trace_cost, f64, req, Some(1.0), 1.0,
                "defaultUndesiredDirectionTraceCost", [], Flat::Quiet, true,
                "Default cost of 1 mm in the undesired direction."),
            (via_costs, i32, fb, Some(50), 1, "viaCosts", ["viaCosts"], Flat::Quiet, true,
                "Cost of a via on a regular net."),
            (plane_via_costs, i32, fb, Some(5), 1, "planeViaCosts", [], Flat::Quiet, true,
                "Cost of a via on a plane."),
            (start_ripup_costs, i32, fb, Some(100), 1, "startRipupCosts", ["startRipupCosts"],
                Flat::Quiet, true, "Starting ripup cost."),
            (default_bend_cost, f64, req, Some(0.0), 0.0, "defaultBendCost", [], Flat::No, true,
                "Default bend cost per direction change."),
            (unrouted_net_penalty, f32, req, Some(5_000_000.0), 0.0, "unroutedNetPenalty", [],
                Flat::No, true, "Score penalty per unrouted connection."),
            (clearance_violation_penalty, f32, req, Some(1_000_000.0), 0.0,
                "clearanceViolationPenalty", [], Flat::No, true,
                "Score penalty per clearance violation."),
            (bend_penalty, f32, req, Some(10.0), 0.0, "bendPenalty", [], Flat::No, true,
                "Score penalty per bend."),
        }
        router_scoring, RouterScoringPatch, RouterScoringSettings, "routerScoring",
        "The router score (Java's `RouterScoreSettings`)." {
            (version, RouterScoringVersion, req, Some(RouterScoringVersion::V2Continuous),
                RouterScoringVersion::V2Continuous, "version", [], Flat::No, true,
                "Router score version."),
            (unrouted_connection_weight, f32, req, Some(1000.0), 0.0, "unroutedConnectionWeight", [],
                Flat::No, true, "Weight of an unrouted connection."),
            (unrouted_free_fraction, f32, req, Some(0.5), 0.0, "unroutedFreeFraction", [],
                Flat::No, true, "Fraction of unrouted connections that cost nothing."),
            (unrouted_first_half_weight, f32, req, Some(1000.0_f32 / 3.0_f32), 0.0,
                "unroutedFirstHalfWeight", [], Flat::No, true, "Weight of the first half."),
            (unrouted_second_half_weight, f32, req, Some(2000.0_f32 / 3.0_f32), 0.0,
                "unroutedSecondHalfWeight", [], Flat::No, true, "Weight of the second half."),
            (clearance_violation_count_weight, f32, req, Some(25.0), 0.0,
                "clearanceViolationCountWeight", [], Flat::No, true,
                "Weight of the violation count."),
            (clearance_violation_depth_weight, f32, req, Some(300.0), 0.0,
                "clearanceViolationDepthWeight", [], Flat::No, true,
                "Weight of the violation depth."),
            (clearance_violation_depth_scale, f32, req, Some(1000.0), 0.0,
                "clearanceViolationDepthScale", [], Flat::No, true,
                "Scale of the violation depth in micrometers."),
        }
        optimizer_scoring, OptimizerScoringPatch, OptimizerScoringSettings, "optimizerScoring",
        "The optimizer score (Java's `OptimizerScoreSettings`)." {
            (version, OptimizerScoringVersion, req, Some(OptimizerScoringVersion::V2LowerBound),
                OptimizerScoringVersion::V2LowerBound, "version", [], Flat::No, true,
                "Optimizer score version."),
            (excess_wire_length_weight, f32, req, Some(1000.0), 0.0, "excessWireLengthWeight", [],
                Flat::No, true, "Weight of excess wire length."),
            (excess_via_weight, f32, req, Some(2000.0), 0.0, "excessViaWeight", [], Flat::No, true,
                "Weight of excess vias."),
            (excess_bend_weight, f32, req, Some(500.0), 0.0, "excessBendWeight", [], Flat::No, true,
                "Weight of excess bends."),
            (length_floor, f32, req, Some(1.0), 0.0, "lengthFloor", [], Flat::No, true,
                "Lower bound of the length."),
            (difficulty_scale_floor, f32, req, Some(1.0), 0.0, "difficultyScaleFloor", [], Flat::No, true,
                "Lower bound of the difficulty scale."),
        }
        diff_pairs, DiffPairsPatch, DiffPairsSettings, "diffPairs",
        "Differential pair detection (X08; not a Java section)." {
            (infer_from_names, bool, opt, None, None, "inferFromNames", [], Flat::No, true,
                "Pair nets by name (`_P`/`_N`, `+`/`-`, ...) when no net class makes them a pair (unset: off)."),
            (skew_um, f64, opt, None, None, "skewUm", [], Flat::No, true,
                "Intra-pair skew tolerance in micrometers (X09.4; unset: 25); the shorter track gets meander bumps."),
        }
        neck_down, NeckDownPatch, NeckDownSettings, "neckDown",
        "Neck-down limits (Q02.2; not a Java section)." {
            (strict, bool, opt, None, None, "strict", [], Flat::No, true,
                "Strict sizes: neck-down stages don't narrow; exit 3 if copper ships under its class width."),
            (path_mm, f64, opt, None, None, "pathMm", [], Flat::No, true,
                "`bounded-neck` (Q17.2): the neck distance D in mm from each pin instead of courtyard plus one pitch (unset: courtyard plus pitch)."),
            (widen_back, bool, opt, None, None, "widenBack", [], Flat::No, true,
                "Widen a neck-down retry's narrow traces back to the class width in 0.5 mm pieces where they fit, with 4-step tapers (Q17.3; unset: off, profile `legacy`); profile `enhanced` turns it on (N05.3)."),
        }
        length_matching, LengthMatchingPatch, LengthMatchingSettings, "lengthMatching",
        "Length and time matching targets (X07.2; not a Java section)." {
            (groups, Vec<String>, opt, None, None, "groups", [], Flat::No, true,
                "Matched groups `NAME=GLOB[|GLOB...][@TOL]` (TOL `<n>um`, `<n>mm` or `<n>ps`; default 100um)."),
        }
        special, SpecialPatch, SpecialSettings, "special",
        "Stage selection for the special stages (A10.2; not a Java section)." {
            (length_matching, String, opt, None, None, "lengthMatching", [], Flat::No, true,
                "Length matching stage id (X07: `krt-meander`), run last; unset runs none."),
            (diff_pairs, String, opt, None, None, "diffPairs", [], Flat::No, true,
                "Differential pair stage id (X08: `krt-diffpair`), run before the fanout; unset runs none."),
            (plane_taps, String, opt, None, None, "planeTaps", [], Flat::No, true,
                "Plane tap policy id; unset uses the `autorouter.algorithm` bundle's."),
            (neck_down, String, opt, None, None, "neckDown", [], Flat::No, true,
                "Neck-down policy id; unset uses the `autorouter.algorithm` bundle's."),
            (stages, Vec<String>, opt, None, None, "stages", [], Flat::No, true,
                "Extra board stage ids, run at each stage's phase in list order."),
            (diagnostics, Vec<String>, opt, None, None, "diagnostics", [], Flat::No, true,
                "Report-only diagnostics ids (Q06 `verdicts` and rip-stats (Q05)); they change no routes."),
        }
        quantum_stop, QuantumStopPatch, QuantumStopSettings, "quantumStop",
        "The `quantum-stop` pass controller's stop rule (Q04.1; not a Java section)." {
            (grace, i32, opt, None, None, "grace", [], Flat::No, true,
                "Restore checks that never count toward the stop (unset: 0)."),
            (checks, i32, opt, None, None, "checks", [], Flat::No, true,
                "Consecutive restore checks without a gain that stop the passes (unset: 2)."),
            (quantum_abs, f64, opt, None, None, "quantumAbs", [], Flat::No, true,
                "Smallest score gain between restore checks that counts (unset: 1.0)."),
            (quantum_pct, f64, opt, None, None, "quantumPct", [], Flat::No, true,
                "Smallest gain as a fraction of the best score, if larger than `quantum_abs` (unset: 0.01)."),
            (repeat, bool, opt, None, None, "repeat", [], Flat::No, true,
                "Stop the passes when a pass leaves the board with the state hash it started with (Q04.5; unset: off; independent of the pass controller)."),
        }
        cleanup, CleanupPatch, CleanupSettings, "cleanup",
        "The final cleanup after the last routing or optimizer step (Q16; not a Java section)." {
            (remove_stubs, bool, opt, None, None, "removeStubs", [], Flat::No, true,
                "Remove routed vias and traces that connect fewer than two items of their net, counted as KiCad counts contacts, at the end of the run (unset: off, profile `legacy`; Q16); profile `enhanced` turns it on (N05.3)."),
            (pour_min_thickness_um, f64, opt, None, None, "pourMinThicknessUm", [], Flat::No, true,
                "A DSN input's signal-layer pours: the zones' minimum fill thickness in micrometers (KiCad `min_thickness`, the smallest of the board's), the opening of the fill model the stub cleanup judges a pour by (K15.26; unset: no opening; a `.kicad_pcb` input takes each zone's own)."),
            (pour_min_thickness_nets, Vec<String>, opt, None, None, "pourMinThicknessNets", [], Flat::No, true,
                "A DSN input's signal-layer pours: per net, `<micrometers>:<net name>`, the smallest minimum fill thickness of that net's zones where it is not `pour_min_thickness_um` (K15.33; unset: every net takes `pour_min_thickness_um`)."),
            (pour_clearance_um, f64, opt, None, None, "pourClearanceUm", [], Flat::No, true,
                "A DSN input's signal-layer pours: the zones' clearance to other nets' copper in micrometers (the smallest KiCad's filler applies: the zones' clearances and the `.kicad_dru` clearance rules), the clearance of the fill model the stub cleanup judges a pour by (K15.26, K15.33; unset: the router's clearances)."),
            (pour_island_removal, bool, opt, None, None, "pourIslandRemoval", [], Flat::No, true,
                "A DSN input's signal-layer pours: KiCad's filler removes their fill islands no pad reaches (`island_removal_mode` 0, always), so the stub cleanup drops them from the fill model it judges a pour by, except for the nets of `pour_islands_kept_nets` (K15.29; unset: no island is dropped; a `.kicad_pcb` input takes each zone's own mode)."),
            (pour_islands_kept_nets, Vec<String>, opt, None, None, "pourIslandsKeptNets", [], Flat::No, true,
                "With `pour_island_removal`: the nets with a signal-layer zone whose islands KiCad keeps (`island_removal_mode` 1 or 2); their pours keep every island (K15.29)."),
        }
        transaction, TransactionPatch, TransactionSettings, "transaction",
        "The `transactional-ripup` connection router (Q11; not a Java section)." {
            (rule, String, opt, None, None, "rule", [], Flat::No, true,
                "Commit a rip-up only if every victim net re-lands (`strict`, unset) or the incomplete count falls (`count`)."),
            (depth, i32, opt, None, None, "depth", [], Flat::No, true,
                "Victims may rip once more (`1`) or not (`0`, unset); no net is ripped twice in one transaction (Q11.4)."),
            (rip_factor, i32, opt, None, None, "ripFactor", [], Flat::No, true,
                "Rip cost of a net ripped k times this run times 1 + rip_factor * k (Q11.5; unset or 0: off)."),
        }
        board_minimums, BoardMinimumsPatch, BoardMinimumsSettings, "boardMinimums",
        "The board's minimum manufacturing rules (Q18; not a Java section): hard floors no routed copper goes below. The KiCad plugin and a `.kicad_pcb` input fill them from Board Setup." {
            (track_width_um, f64, opt, None, None, "trackWidthUm", [], Flat::No, true,
                "The board's minimum track width in micrometers (KiCad `m_TrackMinWidth`): no routed trace, neck-downs included, is narrower (unset: unknown)."),
            (via_diameter_um, f64, opt, None, None, "viaDiameterUm", [], Flat::No, true,
                "The board's minimum via diameter in micrometers (KiCad `m_ViasMinSize`): a smaller via type is routed at the floor size, with a warning naming its net class (unset: unknown)."),
            (via_drill_um, f64, opt, None, None, "viaDrillUm", [], Flat::No, true,
                "The board's minimum through-hole drill in micrometers (KiCad `m_MinThroughDrill`): a via type with a smaller drill is routed at the floor size (unset: unknown)."),
            (via_annular_um, f64, opt, None, None, "viaAnnularUm", [], Flat::No, true,
                "The board's minimum via annular ring in micrometers (KiCad `m_ViasMinAnnularWidth`): a via type below the via minimums is routed at the floor size, `drill + 2 * annular` at least (unset: unknown)."),
            (clearance_um, f64, opt, None, None, "clearanceUm", [], Flat::No, true,
                "The board's minimum clearance in micrometers (KiCad `m_MinClearance`): every copper clearance below it is raised to it (unset: unknown; Q22.10)."),
            (hole_to_hole_um, f64, opt, None, None, "holeToHoleUm", [], Flat::No, true,
                "The board's minimum distance between two drill holes, edge to edge, of any net, in micrometers (KiCad `m_HoleToHoleMin`): no routed via hole comes closer to another via's or a pin's drill (unset: unknown; Q22.11)."),
            (from_classes, bool, opt, None, None, "fromClasses", [], Flat::No, true,
                "With `track_width_um` unset, the narrowest class width in the design is the minimum track width (unset: off, profile `legacy`, Java's neck-downs); profile `enhanced` turns it on (Q18)."),
        }
        input_fixups, InputFixupsPatch, InputFixupsSettings, "inputFixups",
        "The KiCad plugin's DSN fix-ups (K09) for a `.kicad_pcb` input (K14; not a Java section): the reader's tree is changed as the plugin changes KiCad's DSN export. Names only (the plugin resolves its `auto` lists); an unknown class or layer is an error, an unknown net a warning. A DSN input ignores them, with a warning. Unset (the default, both profiles) changes nothing." {
            (class_widths, Vec<String>, opt, None, None, "classWidths", [], Flat::No, true,
                "Net class track widths as `<class>=<width in micrometers>` entries: each class's width rule is set to it, as the plugin's `class_widths` writes the class's `(rule (width W))`; a class an applied `.kicad_dru` `track_width` rule sets keeps the rule's width (K14.1)."),
            (route_on_plane_layers, Vec<String>, opt, None, None, "routeOnPlaneLayers", [], Flat::No, true,
                "Plane (power) layers opened to routing: each becomes a signal layer and keeps its planes, as the plugin's `route_on_plane_layers` turns `(type power)` into `(type signal)`; a layer that is not a power layer is an error (K14.2)."),
            (drop_pour_nets, Vec<String>, opt, None, None, "dropPourNets", [], Flat::No, true,
                "Nets whose zones on the outer copper layers are left out of the planes, as the plugin's `drop_pour_nets` drops their `(plane ...)`; their inner-layer zones stay (K14.3)."),
            (fixed_pour_classes, Vec<String>, opt, None, None, "fixedPourClasses", [], Flat::No, true,
                "Net classes whose filled zones on the outer copper layers become fixed wires of their net covering the fill (the file's `filled_polygon`), their planes on that layer dropped, as the plugin's `fixed_pour_classes` in `wires` mode does; a net in `drop_pour_nets` is dropped instead; an unknown class is an error. The session never holds the wires (K14.4)."),
        }
        java_quirks, JavaQuirksPatch, JavaQuirksSettings, "javaQuirks",
        "The Java bugs kept for parity, one switch each (N06; not a Java section). Unset is `true`, Java's behaviour; profile `legacy` keeps every switch `true`, profile `enhanced` turns off the fixes N06.13 promoted." {
            (simplex_prev_division_line, bool, opt, None, None, "simplexPrevDivisionLine", [], Flat::No, true,
                "Simplex cutout keeps the previous division line null: `true` (unset) keeps this Java bug, `false` assigns it as the inventory proposed (N06.4). That fix never passes a merge test, so both values give the same pieces; profile `enhanced` uses `false` (N06.13)."),
            (octagon_box_cutout_branch3, bool, opt, None, None, "octagonBoxCutoutBranch3", [], Flat::No, true,
                "Octagon-box cutout branch 3 rebuilds the octagon untrimmed (pieces overlap): `true` (unset) keeps this Java bug, `false` trims it to the moved box (N06.4)."),
            (cutout_hole_crosses_border, bool, opt, None, None, "cutoutHoleCrossesBorder", [], Flat::No, true,
                "A simplex cutout whose first corner lies on the outer border clips later pieces with the hole edge before it (pieces vanish): `true` (unset) keeps this Java bug, `false` clips nothing there (N06.14); profile `enhanced` uses `false` (N06.13)."),
            (simplex_octagon_half_corner, bool, opt, None, None, "simplexOctagonHalfCorner", [], Flat::No, true,
                "A simplex cutout leaves a hole corner that is not an integer point undivided, as if on the outer border (pieces overlap): `true` (unset) keeps this Java bug, `false` divides it from its rounded point (N06.14); profile `enhanced` uses `false` (N06.13)."),
            (init_failure_room_leak, bool, opt, None, None, "initFailureRoomLeak", [], Flat::No, true,
                "A failed maze-search init leaves the completed start rooms as obstacles: `true` (unset) keeps this Java bug, `false` fixes it (N06.5); profile `enhanced` uses `false` (N06.13)."),
            (failed_attempt_ripped_carryover, bool, opt, None, None, "failedAttemptRippedCarryover", [], Flat::No, true,
                "A rejected attempt keeps the ripped set for the next attempt: `true` (unset) keeps this Java bug, `false` fixes it (N06.5); profile `enhanced` uses `false` (N06.13)."),
            (ortho_zero_width_room, bool, opt, None, None, "orthoZeroWidthRoom", [], Flat::No, true,
                "90-degree room right of an obstacle gets width 0 and is never made: `true` (unset) keeps this Java bug, `false` fixes it (N06.5); profile `enhanced` uses `false` (N06.13)."),
            (edge_corner_not_advanced, bool, opt, None, None, "edgeCornerNotAdvanced", [], Flat::No, true,
                "45-degree edge sweep never advances its corner, so side 7 is skipped: `true` (unset) keeps this Java bug, `false` fixes it (N06.5); profile `enhanced` uses `false` (N06.13)."),
            (connect_to_trace_swapped_width, bool, opt, None, None, "connectToTraceSwappedWidth", [], Flat::No, true,
                "Start and target trace stubs take each other's layer half width: `true` (unset) keeps this Java bug, `false` fixes it (N06.5); profile `enhanced` uses `false` (N06.13)."),
            (fanout_stale_drill_carry, bool, opt, None, None, "fanoutStaleDrillCarry", [], Flat::No, true,
                "Fanout retries reuse the first attempt's drill pages: `true` (unset) keeps this Java bug, `false` fixes it (N06.6); profile `enhanced` uses `false` (N06.13)."),
            (drill_destination_room_index, bool, opt, None, None, "drillDestinationRoomIndex", [], Flat::No, true,
                "A destination drill's room is indexed from layer 0, not its first layer: `true` (unset) keeps this Java bug, `false` fixes it (N06.6); profile `enhanced` uses `false` (N06.13)."),
            (fanout_pin_order_names, bool, opt, None, None, "fanoutPinOrderNames", [], Flat::No, true,
                "The pin orders distance_to_closest_on_net and surroundings_density fall back to pin-index order: `true` (unset) keeps this Java bug, `false` fixes it (N06.6); profile `enhanced` uses `false` (N06.13)."),
            (in_front_of_pad_typo, bool, opt, None, None, "inFrontOfPadTypo", [], Flat::No, true,
                "Side 0 of the in-front-of-pad test takes b.x+b.x for b.x+b.y: `true` (unset) keeps this Java bug, `false` fixes it (N06.7); profile `enhanced` uses `false` (N06.13)."),
            (store_trace_class_self_compare, bool, opt, None, None, "storeTraceClassSelfCompare", [], Flat::No, true,
                "A contact trace's clearance class is compared with itself: `true` (unset) keeps this Java bug, `false` fixes it (N06.7); profile `enhanced` uses `false` (N06.13)."),
            (rational_perp_projection, bool, opt, None, None, "rationalPerpProjection", [], Flat::No, true,
                "The rational perpendicular projection adds a term it should subtract and drops z: `true` (unset) keeps this Java bug, `false` fixes it (N06.7); profile `enhanced` uses `false` (N06.13)."),
            (smooth_loses_trace, bool, opt, None, None, "smoothLosesTrace", [], Flat::No, true,
                "End-corner smoothing leaves the trace removed when the smoothed insert fails: `true` (unset) keeps this Java bug, `false` fixes it (N06.8); profile `enhanced` uses `false` (N06.13)."),
            (pull_tight_reused_lines_change, bool, opt, None, None, "pullTightReusedLinesChange", [], Flat::No, true,
                "Pull-tight reports a change when every line was reused, skipping the end smoothing: `true` (unset) keeps this Java bug, `false` fixes it (N06.8); profile `enhanced` uses `false` (N06.13)."),
            (opt_first_pass_stops, bool, opt, None, None, "optFirstPassStops", [], Flat::No, true,
                "An optimizer pass without a winner stops the loop instead of one more pass at normal ripup costs: `true` (unset) keeps this Java bug, `false` fixes it (N06.9); profile `enhanced` uses `false` (N06.13)."),
            (opt_equal_key_skip, bool, opt, None, None, "optEqualKeySkip", [], Flat::No, true,
                "The optimizer cursor skips every candidate whose (x, y, layer) key equals an earlier one: `true` (unset) keeps this Java bug, `false` fixes it (N06.9); profile `enhanced` uses `false` (N06.13)."),
            (dsn_via_alias, bool, opt, None, None, "dsnViaAlias", [], Flat::No, true,
                "With no structure via, every class shares the first class's use_via list: `true` (unset) keeps this Java bug, `false` fixes it (N06.10); profile `enhanced` uses `false` (N06.13)."),
            (dsn_class_class_pairs, bool, opt, None, None, "dsnClassClassPairs", [], Flat::No, true,
                "With 3 or more classes only (first, other) class_class pairs get rules: `true` (unset) keeps this Java bug, `false` fixes it (N06.10); profile `enhanced` uses `false` (N06.13)."),
            (dsn_shape_bbox, bool, opt, None, None, "dsnShapeBbox", [], Flat::No, true,
                "The DSN shape bounding boxes: unsorted rectangles, circle diameter as radius, path widths on max x: `true` (unset) keeps this Java bug, `false` fixes it (N06.10); profile `enhanced` uses `false` (N06.13)."),
            (dsn_unit_ignored, bool, opt, None, None, "dsnUnitIgnored", [], Flat::No, true,
                "The placement resolution and the pcb-level unit are ignored: `true` (unset) keeps this Java bug, `false` fixes it (N06.10); profile `enhanced` uses `false` (N06.13)."),
            (ses_reader_scale, bool, opt, None, None, "sesReaderScale", [], Flat::No, true,
                "The SES reader divides by the board scale where it should multiply: `true` (unset) keeps this Java bug, `false` fixes it (N06.10); profile `enhanced` uses `false` (N06.13)."),
            (stats_line_ab_length, bool, opt, None, None, "statsLineAbLength", [], Flat::No, true,
                "Statistics sum the distance between each line's defining points, not the segment lengths: `true` (unset) keeps this Java bug, `false` fixes it (N06.10); profile `enhanced` uses `false` (N06.13)."),
            (route_result_int_via_ratio, bool, opt, None, None, "routeResultIntViaRatio", [], Flat::No, true,
                "The route result's via ratio is an int division: `true` (unset) keeps this Java bug, `false` fixes it (N06.10); profile `enhanced` uses `false` (N06.13)."),
            (drc_units_10000_per_mm, bool, opt, None, None, "drcUnits10000PerMm", [], Flat::No, true,
                "DRC summaries, congestion zones and hints assume 10000 board units per mm: `true` (unset) keeps this Java bug, `false` fixes it (N06.10); profile `enhanced` uses `false` (N06.13)."),
            (snapshot_drops_compensation, bool, opt, None, None, "snapshotDropsCompensation", [], Flat::No, true,
                "Restore and deep copy drop the clearance compensation: `true` (unset) keeps this Java bug, `false` fixes it (N06.11); profile `enhanced` uses `false` (N06.13)."),
            (rules_matrix_trees_not_rebuilt, bool, opt, None, None, "rulesMatrixTreesNotRebuilt", [], Flat::No, true,
                "A rules file changes the clearance matrix without rebuilding the search trees: `true` (unset) keeps this Java bug, `false` fixes it (N06.11); profile `enhanced` uses `false` (N06.13)."),
            (move_drill_tidy_region, bool, opt, None, None, "moveDrillTidyRegion", [], Flat::No, true,
                "A moved drill item never pull-tightens with a finite tidy width: `true` (unset) keeps this Java bug, `false` fixes it (N06.11); profile `enhanced` uses `false` (N06.13)."),
            (route_change_layer_no_via, bool, opt, None, None, "routeChangeLayerNoVia", [], Flat::No, true,
                "An interactive layer change succeeds without a via spanning both layers: `true` (unset) keeps this Java bug, `false` fixes it (N06.11); profile `enhanced` uses `false` (N06.13)."),
            (route_connect_inner_break, bool, opt, None, None, "routeConnectInnerBreak", [], Flat::No, true,
                "Interactive connect leaves only the inner loop, retrying from the added corner: `true` (unset) keeps this Java bug, `false` fixes it (N06.11); profile `enhanced` uses `false` (N06.13)."),
            (select_extension_ignores_filter, bool, opt, None, None, "selectExtensionIgnoresFilter", [], Flat::No, true,
                "The inspect state stays after an extension filter empties the selection: `true` (unset) keeps this Java bug, `false` fixes it (N06.11); profile `enhanced` uses `false` (N06.13)."),
            (shove_via_into_pending_piece, bool, opt, None, None, "shoveViaIntoPendingPiece", [], Flat::No, true,
                "A shove's recursion moves a via of another net onto a substitute piece that is not on the board yet, and the piece is then inserted over it (a short): `true` (unset) keeps this Java bug, `false` fails the shove when a piece about to be inserted overlaps such a via (N06.15); profile `enhanced` uses `false` (N06.13)."),
            (any_angle_room_side_index, bool, opt, None, None, "anyAngleRoomSideIndex", [], Flat::No, true,
                "Any-angle room neighbours take their touching sides on the room's own shape (an octagon's 8 border lines, degenerate ones too) while the edge removal and the new incomplete rooms index its simplex (redundant lines removed); a side past the simplex's last line never ends the gap walk, which adds rooms until memory runs out: `true` (unset) keeps this Java bug, `false` takes the sides on the simplex (N06.16); profile `enhanced` uses `false` (N06.13)."),
            (placed_shape_convex, bool, opt, None, None, "placedShapeConvex", [], Flat::No, true,
                "A placed pad, component outline or component area takes its rounded corners for a convex polygon, so short edges that round to reflex vertices cut corners off the shape or leave it unbounded: `true` (unset) keeps this Java bug, `false` takes the convex hull of the rounded corners (K15.2). Neither profile sets it; a preregistered measurement decides `enhanced`."),
            (ripup_pass_no_unset, bool, opt, None, None, "ripupPassNoUnset", [], Flat::No, true,
                "ripupPassNo is never set, so the random rip-up detour never fires: `true` (unset) keeps this Java bug, `false` sets the pass number so the detour factor fires from pass 4 on, outside every third pass (N06.12, measured by prereg/N06.12-ripup-pass.json)."),
            (in_front_of_pad_side0_flank, bool, opt, None, None, "inFrontOfPadSide0Flank", [], Flat::No, true,
                "Side 0's second flank clause takes min(x+y) where sides 2, 4 and 6 take the extreme past their diagonal (max here): `true` (unset) keeps this Java quirk, `false` takes max (an inferred fix, measured as an arm of N06.7; N06.12 records its outcome); profile `enhanced` uses `false` (N06.13)."),
            (opt_threshold_fraction, bool, opt, None, None, "optThresholdFraction", [], Flat::No, true,
                "An optimizer threshold in (0, 0.1) is read as a fraction and multiplied by 100: `true` (unset) keeps this Java heuristic (left as Java by N06.12), `false` reads it as the percentage it says. A command-line value is scaled only once either way."),
            (rules_unknown_layer_scope, bool, opt, None, None, "rulesUnknownLayerScope", [], Flat::No, true,
                "A rules layer scope naming an unknown layer applies to all layers: `true` (unset) keeps this Java bug, `false` ignores the scope with a warning (N06.12); profile `enhanced` uses `false` (N06.13)."),
            (any_angle_skip_length_squared, bool, opt, None, None, "anyAngleSkipLengthSquared", [], Flat::No, true,
                "Any-angle pull-tight compares SKIP_LENGTH with a squared distance: `true` (unset) keeps this Java bug, `false` compares its square (N06.12, an arm of prereg/N06.12-ripup-pass.json); profile `enhanced` uses `false` (N06.13)."),
            (same_net_via_pad_violation, bool, opt, None, None, "sameNetViaPadViolation", [], Flat::No, true,
                "The router's score and pre-existing count take a via in or beside a pad of its own net as a clearance violation (every `.kicad_pcb` padstack is `(attach off)`): `true` (unset) keeps this Java count, `false` leaves out such pairs unless one item is copper the router placed, an unfixed trace or via (K15.5); in neither profile (no measurement yet)."),
            (edge_pin_net_crosses_outline, bool, opt, None, None, "edgePinNetCrossesOutline", [], Flat::No, true,
                "A trace of a net with a pin on or outside the board outline is no obstacle for the outline, so it may cross it anywhere (`BoardOutline.blocksNets`): `true` (unset) keeps this Java behaviour, `false` keeps every trace inside the outline, a connection that cannot be made there staying open (K15.19); profile `enhanced` uses `false` (decided 2026-10-20: routing never breaks the board's manufacturing minimums)."),
        }
        debug, DebugPatch, DebugSettings, "debug",
        "Engine self-checks (P02.1; not a Java section)." {
            (audit_incremental, bool, opt, None, None, "auditIncremental", [], Flat::No, true,
                "Compare the search trees with a rebuild after each ripup and restore and each pass (unset: off)."),
        }
    }
}

/// The field with the canonical path `path`.
pub fn field_by_path(path: &str) -> Option<&'static FieldDesc> {
    FIELDS.iter().find(|f| f.path == path)
}

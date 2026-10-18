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
//! `costs_initialized` (Java's transient `boardSpecificTraceCostsApplied`)
//! and `validation_warnings`. The two per-layer cost arrays are table
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
        (hole_clearance_um, f64, req, Some(0.0), 0.0, "holeClearanceUm", [], Flat::No, true,
            "Hole clearance in micrometers."),
        (clearance_tolerance_um, f64, req, Some(1.0), 0.0, "clearanceToleranceUm", [], Flat::No, true,
            "Clearance shortfalls up to this value are treated as rounding noise (micrometers)."),
        (plane_nets, Vec<String>, req, Some(Vec::new()), Vec::new(), "planeNets", [], Flat::No, true,
            "Net names to treat as power-plane nets."),
        (plane_as_obstacle, bool, req, Some(false), false, "planeAsObstacle",
            ["conduction_is_obstacle", "planeAsObstacle", "conductionIsObstacle"], Flat::No, true,
            "Conduction areas block foreign traces."),
        (plane_obstacle_net_classes, Vec<String>, opt, None, None, "planeObstacleNetClasses", [],
            Flat::No, true,
            "Net classes whose signal-layer pours block the nets with no pin or via inside them (Q01)."),
        (plane_obstacle_block, String, opt, None, None, "planeObstacleBlock", [], Flat::No, true,
            "What those pours block: `traces_and_vias` (unset, Q01) or `traces` (foreign vias pass; Q09.4)."),
        (wire_keepout, String, opt, None, None, "wireKeepout", [], Flat::No, true,
            "A Specctra `wire_keepout` is a `keepout` (unset, Java) or `traces_only` (vias pass; Q09.3)."),
        (neck_width_um, f64, req, Some(0.0), 0.0, "neckWidthUm", [], Flat::No, true,
            "Retry a failed connection with traces clamped to this width (micrometers); 0 is off."),
        (min_trace_width_um, f64, opt, None, None, "minTraceWidthUm", [], Flat::No, true,
            "Opt-in floor for every narrowing (micrometers); unset is off."),
        (strict_drc, bool, req, Some(false), false, "strictDrc", [], Flat::No, true,
            "Reject connections that introduce clearance violations."),
        (result_json, String, opt, None, None, "resultJsonPath", [], Flat::No, true,
            "Path of a machine-readable routing result manifest."),
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

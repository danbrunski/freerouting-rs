//! The stage catalog (N01.4): every stage slot of the pipeline, the ids
//! registered for it, the id the routing profile starts with, and the
//! settings the slot's implementations read, with the profile's defaults.
//!
//! The CLI's `--list-stages`, the GUI's "Algorithms" section and the
//! plugin's "Advanced" group all list this, so the three can't drift from
//! the registry: [`catalog`] walks every map of a [`StageRegistry`], and
//! `tests/stage_catalog.rs` checks that every registered id appears.
//!
//! **Options.** A stage implementation does not declare settings of its
//! own; it reads the settings table. [`SLOTS`] names, per slot, the table
//! paths its built-in implementations read (the Freerouting stages, the
//! special stages' sections), so that a user sees what can be tuned below
//! each choice. A path that is no settings field is a test failure.
//!
//! **Experimental (N02.4).** A candidate that failed its registered gate stays
//! only as an opt-in, off in every profile, when N02's keep rule holds: it
//! wins on a board class one can name, it passes the hard guards and it is a
//! self-contained stage or setting. [`EXPERIMENTAL`] lists those, with the
//! class and the measured numbers as the help text; the CLI's
//! `--list-stages`, the GUI's "Algorithms" section and the plugin's listing
//! mark them. Tests (`tests/stage_catalog.rs`, `freerouting-stages`) check that
//! every entry names a real settings field or registered stage id and that no
//! profile sets or selects it.

use super::registry::{StageRegistry, StageSelection};
use crate::settings::fields::{RouterSettings, field_by_path};
use crate::settings::profile::Profile;
use crate::settings::types::FieldValue;

/// What a slot holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotArity {
    /// Exactly one id (unset uses the bundle's or the default).
    One,
    /// At most one id (unset runs none).
    Optional,
    /// A list of ids, run in list order.
    List,
}

/// One slot of the pipeline: its key, its registry map and its options.
#[derive(Clone, Copy, Debug)]
pub struct SlotDesc {
    /// The registry map and the JSON name (`connection_router`).
    pub kind: &'static str,
    /// The name shown in the interfaces.
    pub label: &'static str,
    /// The settings path that selects the id(s), without `router.`.
    pub key: &'static str,
    pub arity: SlotArity,
    /// The settings paths the slot's implementations read.
    pub options: &'static [&'static str],
}

/// Every slot, in pipeline order.
pub const SLOTS: &[SlotDesc] = &[
    SlotDesc {
        kind: "algorithm",
        label: "Autorouter bundle",
        key: "autorouter.algorithm",
        arity: SlotArity::One,
        options: &[
            "autorouter.enabled",
            "autorouter.max_passes",
            "autorouter.max_items",
            "autorouter.max_threads",
            "autorouter.ignore_net_classes",
        ],
    },
    SlotDesc {
        kind: "diff_pairs",
        label: "Differential pairs",
        key: "special.diff_pairs",
        arity: SlotArity::Optional,
        options: &["diff_pairs.infer_from_names", "diff_pairs.skew_um"],
    },
    SlotDesc {
        kind: "fanout",
        label: "Fanout",
        key: "fanout.algorithm",
        arity: SlotArity::One,
        options: &[
            "fanout.enabled",
            "fanout.max_passes",
            "fanout.max_items",
            "fanout.max_milliseconds_per_pin",
            "fanout.ripup_allowed",
            "fanout.min_escape_length_mm",
            "fanout.max_escape_length_mm",
            "fanout.pin_sorting_order",
            "fanout.fallback_to_board_vias",
            "fanout.timeout",
        ],
    },
    SlotDesc {
        kind: "ordering",
        label: "Net ordering",
        key: "autorouter.ordering",
        arity: SlotArity::One,
        options: &[],
    },
    SlotDesc {
        kind: "connection_router",
        label: "Connection router",
        key: "autorouter.connection_router",
        arity: SlotArity::One,
        options: &[
            "allowed_via_types",
            "scoring.via_costs",
            "scoring.plane_via_costs",
            "scoring.default_preferred_direction_trace_cost",
            "scoring.default_undesired_direction_trace_cost",
            "scoring.default_bend_cost",
            "trace_pull_tight_accuracy",
            "automatic_neckdown",
            "strict_drc",
            "via_clearance",
            "room_order",
            "work_unit",
            "transaction.rule",
            "transaction.depth",
            "transaction.rip_factor",
        ],
    },
    SlotDesc {
        kind: "ripup_policy",
        label: "Rip-up policy",
        key: "autorouter.ripup_policy",
        arity: SlotArity::One,
        options: &["scoring.start_ripup_costs"],
    },
    SlotDesc {
        kind: "pass_controller",
        label: "Pass controller",
        key: "autorouter.pass_controller",
        arity: SlotArity::One,
        options: &[
            "router_scoring.version",
            "router_scoring.unrouted_connection_weight",
            "router_scoring.unrouted_free_fraction",
            "router_scoring.clearance_violation_count_weight",
            "router_scoring.clearance_violation_depth_weight",
            "quantum_stop.grace",
            "quantum_stop.checks",
            "quantum_stop.quantum_abs",
            "quantum_stop.quantum_pct",
            "quantum_stop.repeat",
            "autorouter.output_guard",
        ],
    },
    SlotDesc {
        kind: "plane_taps",
        label: "Plane taps",
        key: "special.plane_taps",
        arity: SlotArity::One,
        options: &["plane_nets"],
    },
    SlotDesc {
        kind: "neck_down",
        label: "Neck-down",
        key: "special.neck_down",
        arity: SlotArity::One,
        options: &[
            "neck_width_um",
            "min_trace_width_um",
            "neck_down.strict",
            "neck_down.path_mm",
            "neck_down.widen_back",
        ],
    },
    SlotDesc {
        kind: "board_stages",
        label: "Board stages",
        key: "special.stages",
        arity: SlotArity::List,
        options: &[],
    },
    SlotDesc {
        kind: "length_matching",
        label: "Length matching",
        key: "special.length_matching",
        arity: SlotArity::Optional,
        options: &["length_matching.groups"],
    },
    SlotDesc {
        kind: "optimizer",
        label: "Optimizer",
        key: "optimizer.algorithm",
        arity: SlotArity::One,
        options: &[
            "optimizer.enabled",
            "optimizer.max_passes",
            "optimizer.max_items",
            "optimizer.max_threads",
            "optimizer.improvement_threshold",
            "optimizer.timeout",
            "optimizer_scoring.version",
            "cleanup.remove_stubs",
        ],
    },
    SlotDesc {
        kind: "diagnostics",
        label: "Diagnostics (report only)",
        key: "special.diagnostics",
        arity: SlotArity::List,
        options: &[],
    },
];

/// What an [`Experimental`] entry names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExperimentalKind {
    /// A settings path (without `router.`).
    Option,
    /// A stage id registered in the slot `slot`.
    Stage,
}

/// An opt-in option or stage that is off in every profile and listed under
/// "Experimental" (N02.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Experimental {
    pub kind: ExperimentalKind,
    /// The settings path without `router.` ([`ExperimentalKind::Option`]) or
    /// the stage id ([`ExperimentalKind::Stage`]).
    pub key: &'static str,
    /// The [`SlotDesc::kind`] the entry belongs to.
    pub slot: &'static str,
    /// The board class it wins on, or why there is none yet.
    pub class: &'static str,
    /// The help text: what it does, the class and the measured numbers.
    pub help: &'static str,
}

/// Every experimental option and stage, in listing order. The table in
/// `docs/roadmap/tasks/archive/N02-keep-rule-for-defaults.md` (N02.4) gives each
/// entry's verdict and measurement.
pub const EXPERIMENTAL: &[Experimental] = &[
    Experimental {
        kind: ExperimentalKind::Option,
        key: "via_clearance",
        slot: "connection_router",
        class: "boards whose net classes ask a larger via clearance than the default class",
        help: "`net_class` gives a routed via its net class's clearance, as KiCad checks it \
               (Q14; N05.3 rejected it as a default). Wins on boards with power net classes \
               above the default clearance: canary-pwr (the plugin run's last class-via error, \
               1 -> 0) and overlord (KiCad drc_real 249 -> 209; 35 -> 8 clearance errors in \
               Q14.3). It adds no violation and no via in a pad, and it costs connections on \
               canary-pwr: KiCad unconnected 37 -> 44 (plugin path) and 45 -> 49, equal at 33 \
               with the sweep. A new registration that counts netclass violations only is planned.",
    },
    Experimental {
        kind: ExperimentalKind::Option,
        key: "cleanup.remove_stubs",
        slot: "optimizer",
        class: "boards with fixed pour strips and dangling vias (canary-pwr, interf_u)",
        help: "Removes routed vias and traces that join fewer than two items of their net \
               (Q16). Dangling vias fall 15 -> 0, 16 -> 0 (canary-pwr) and 9 -> 0 (interf_u), \
               KiCad drc_real 1 -> 0 on canary-pwr. Kept pending Q16.4: Q16.3's gate failed on \
               two guards (canary-pwr KiCad unconnected 37 -> 38, CM5_MINIMA_3 \
               track_dangling 1 -> 1); Q16.4 counts contacts as KiCad does.",
    },
    Experimental {
        kind: ExperimentalKind::Option,
        key: "room_order",
        slot: "connection_router",
        class: "none yet",
        help: "`canonical` completes rooms in an order independent of the search tree's shape \
               (P04.1). No board class wins: over 17 boards the total unrouted fell \
               1,281 -> 1,269 and the maze work 39.1 G -> 19.6 G nodes, but 5 boards \
               regressed and GP8B gained 2 clearance violations. Kept pending N03.3, which \
               judges it by the totals rule on boards P04 did not use.",
    },
    Experimental {
        kind: ExperimentalKind::Option,
        key: "work_unit",
        slot: "connection_router",
        class: "none yet",
        help: "`leaves` counts leaves returned by tree queries instead of nodes visited \
               (P04.2), so work limits do not depend on the tree's shape. No board class \
               wins: against `canonical` alone the total unrouted rose 1,269 -> 1,279 and \
               canary-pwr regressed. Kept pending N03.3.",
    },
    Experimental {
        kind: ExperimentalKind::Stage,
        key: "krt-diffpair",
        slot: "diff_pairs",
        class: "boards with one or a few differential pairs that share a signal layer",
        help: "Routes each differential pair as a coupled pair (X08, X09). Wins on boards \
               with a single pair on a shared layer: coupled share 0.91 to 0.99 on x08-straight, \
               x08-connector, x09-two-layer, x09-skew and rp2040dev, with no new violation \
               and no via in a pad. It loses on pair-dense boards: picodvi (10 pairs) couples \
               0.04 and leaves one more single-ended connection unrouted (3 -> 4), \
               pcie-active-backplane (33 pairs) couples 0.28.",
    },
];

/// The entry for the option `path`, if it is experimental.
pub fn experimental_option(path: &str) -> Option<&'static Experimental> {
    EXPERIMENTAL
        .iter()
        .find(|e| e.kind == ExperimentalKind::Option && e.key == path)
}

/// The entry for the stage `id` of the slot `slot`, if it is experimental.
pub fn experimental_stage(slot: &str, id: &str) -> Option<&'static Experimental> {
    EXPERIMENTAL
        .iter()
        .find(|e| e.kind == ExperimentalKind::Stage && e.slot == slot && e.key == id)
}

/// One option of a slot.
#[derive(Clone, Debug, PartialEq)]
pub struct StageOption {
    /// The settings path, without `router.`.
    pub path: &'static str,
    /// The field's doc line.
    pub doc: &'static str,
    /// The value under the profile; `None` is unset.
    pub default: Option<FieldValue>,
    /// Set for an experimental option (N02.4).
    pub experimental: Option<&'static Experimental>,
}

/// One slot as listed.
#[derive(Clone, Debug, PartialEq)]
pub struct StageSlot {
    pub desc: SlotDesc,
    /// The registered ids, sorted.
    pub ids: Vec<&'static str>,
    /// The ids the profile starts with: one for [`SlotArity::One`], none or
    /// one for [`SlotArity::Optional`], the list for [`SlotArity::List`].
    pub default: Vec<String>,
    pub options: Vec<StageOption>,
    /// The registered ids that are experimental (N02.4).
    pub experimental_ids: Vec<&'static Experimental>,
}

impl PartialEq for SlotDesc {
    fn eq(&self, other: &SlotDesc) -> bool {
        self.key == other.key
    }
}

fn registered(registry: &StageRegistry, kind: &str) -> Vec<&'static str> {
    fn keys<T>(map: &std::collections::BTreeMap<&'static str, T>) -> Vec<&'static str> {
        map.keys().copied().collect()
    }
    match kind {
        "algorithm" => keys(&registry.bundles),
        "fanout" => keys(&registry.fanout),
        "ordering" => keys(&registry.ordering),
        "connection_router" => keys(&registry.connection_router),
        "ripup_policy" => keys(&registry.ripup_policy),
        "pass_controller" => keys(&registry.pass_controller),
        "plane_taps" => keys(&registry.plane_taps),
        "neck_down" => keys(&registry.neck_down),
        "board_stages" | "length_matching" | "diff_pairs" => keys(&registry.board_stages),
        "optimizer" => keys(&registry.optimizer),
        "diagnostics" => keys(&registry.diagnostics),
        other => unreachable!("no registry map for slot kind {other}"),
    }
}

fn default_ids(selection: &StageSelection, settings: &RouterSettings, kind: &str) -> Vec<String> {
    let one = |c: &super::registry::StageChoice| vec![c.id.clone()];
    let sp = &settings.special;
    match kind {
        "algorithm" => one(&selection.algorithm),
        "fanout" => one(&selection.fanout),
        "ordering" => one(&selection.ordering),
        "connection_router" => one(&selection.connection_router),
        "ripup_policy" => one(&selection.ripup_policy),
        "pass_controller" => one(&selection.pass_controller),
        "plane_taps" => one(&selection.plane_taps),
        "neck_down" => one(&selection.neck_down),
        "optimizer" => one(&selection.optimizer),
        "board_stages" => sp.stages.clone().unwrap_or_default(),
        "diagnostics" => sp.diagnostics.clone().unwrap_or_default(),
        "length_matching" => sp.length_matching.iter().cloned().collect(),
        "diff_pairs" => sp.diff_pairs.iter().cloned().collect(),
        other => unreachable!("no selection for slot kind {other}"),
    }
}

/// Every slot of `registry`, with the defaults of `profile`.
pub fn catalog(registry: &StageRegistry, profile: Profile, cores: usize) -> Vec<StageSlot> {
    let defaults = profile.defaults(cores);
    let settings = RouterSettings::from_patch(&defaults, cores);
    let selection = StageSelection::resolve(&settings, registry);
    SLOTS
        .iter()
        .map(|desc| {
            let ids = registered(registry, desc.kind);
            StageSlot {
                desc: *desc,
                default: default_ids(&selection, &settings, desc.kind),
                options: desc
                    .options
                    .iter()
                    .filter_map(|path| {
                        let field = field_by_path(path)?;
                        Some(StageOption {
                            path: field.path,
                            doc: field.doc,
                            default: (field.get)(&defaults),
                            experimental: experimental_option(field.path),
                        })
                    })
                    .collect(),
                experimental_ids: EXPERIMENTAL
                    .iter()
                    .filter(|e| e.kind == ExperimentalKind::Stage && e.slot == desc.kind)
                    .filter(|e| ids.contains(&e.key))
                    .collect(),
                ids,
            }
        })
        .collect()
}

/// Every id `registry` holds, by map, for the tests that compare listings
/// with the registry.
pub fn registered_ids(registry: &StageRegistry) -> Vec<(&'static str, Vec<&'static str>)> {
    [
        "algorithm",
        "fanout",
        "ordering",
        "connection_router",
        "ripup_policy",
        "pass_controller",
        "plane_taps",
        "neck_down",
        "board_stages",
        "optimizer",
        "diagnostics",
    ]
    .into_iter()
    .map(|kind| (kind, registered(registry, kind)))
    .collect()
}

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
            "room_order",
            "work_unit",
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

/// One option of a slot.
#[derive(Clone, Debug, PartialEq)]
pub struct StageOption {
    /// The settings path, without `router.`.
    pub path: &'static str,
    /// The field's doc line.
    pub doc: &'static str,
    /// The value under the profile; `None` is unset.
    pub default: Option<FieldValue>,
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
        .map(|desc| StageSlot {
            desc: *desc,
            ids: registered(registry, desc.kind),
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
                    })
                })
                .collect(),
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

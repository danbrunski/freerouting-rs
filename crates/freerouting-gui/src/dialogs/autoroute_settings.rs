//! The autoroute settings dialog (G07.1): Java's `WindowAutorouteParameter`.
//!
//! **One settings description.** The dialog edits the engine's
//! [`SettingsPatch`] (C02: Java's `RouterSettings` with every member
//! nullable, the thing the settings sources and the board tuning produce)
//! and never keeps a copy of its own. A control is a path of the engine's
//! field table ([`CONTROLS`]); a field without a control is listed with a
//! reason in [`CLI_ONLY`]. `tests/ui_routing.rs::every_router_setting_has_a_control`
//! walks [`FIELDS`] and fails when a field is in neither, so the CLI and the
//! GUI can't drift (KRT's lesson).
//!
//! **What the dialog starts from.** The patch of the open board as the CLI
//! would have it before routing: the defaults, the design's own
//! `autoroute_settings` and the board-specific tuning of the layer costs
//! ([`freerouting::job::prepare::prepare_job_settings`]), then what the user
//! applied earlier ([`Draft::apply_overrides`]). Apply keeps the draft in
//! the app; Start routes with it. The dialog never writes board rules.
//!
//! **Java's rules** are in [`timeout`]: integers are clamped to their range
//! ([`Draft::int_range`]), positive doubles fall back to the old value, the
//! job timeout is capped at four weeks and typed as a number and a unit.
//!
//! **Not in the window.** Java's labels `start_pass` and `speed` exist in
//! its bundle but no control uses them (the window at `cda540f` has none),
//! so the dialog has none either. The optimization stage is a control, enabled
//! now that the batch optimizer is ported ([`OPTIMIZER_PORTED`], O06).

pub mod algorithms;
pub mod timeout;

use eframe::egui;
use freerouting::settings::{FIELDS, FieldValue, RouterSettings, SettingsPatch, field_by_path};

use crate::commands::Command;
use timeout::{TimeoutUnit, decompose_timeout};

/// True since the batch optimizer (O06) is ported: the "Optimization" stage
/// checkbox is enabled. Set it to false to disable the checkbox again.
pub const OPTIMIZER_PORTED: bool = true;

/// Fields with a control in the window, with the control's name. The layer
/// rows edit the two per-layer cost arrays and the `layers` member.
pub const CONTROLS: &[(&str, &str)] = &[
    ("profile", "Profile"),
    ("fanout.enabled", "Fanout"),
    ("autorouter.enabled", "Auto-routing"),
    ("optimizer.enabled", "Optimization"),
    ("allowed_via_types", "Vias allowed"),
    ("scoring.via_costs", "Via costs"),
    ("scoring.plane_via_costs", "Power or ground plane via cost"),
    ("scoring.start_ripup_costs", "Rip-up start cost"),
    ("autorouter.max_passes", "Maximum routing passes"),
    ("max_threads", "Maximum routing threads"),
    ("job_timeout", "Job timeout"),
    (
        "scoring.preferred_direction_trace_cost",
        "Trace costs: in preferred direction",
    ),
    (
        "scoring.undesired_direction_trace_cost",
        "Trace costs: against preferred direction",
    ),
];

/// Every other field, with why it has no control. Java's window has the
/// controls above and nothing else; the "Algorithms" section (N01.4,
/// [`algorithms::algorithm_paths`]) has the stage keys and their options;
/// these are set with the CLI, a JSON settings file, the environment or a
/// rules file.
pub const CLI_ONLY: &[(&str, &str)] = &[
    (
        "copper_to_edge_clearance_um",
        "board override of the job, set with the CLI or a settings file",
    ),
    (
        "hole_clearance_um",
        "board override of the job, set with the CLI or a settings file",
    ),
    (
        "clearance_tolerance_um",
        "board override of the job, set with the CLI or a settings file",
    ),
    (
        "plane_as_obstacle",
        "board override of the job, set with the CLI or a settings file",
    ),
    (
        "power_layer_traces",
        "quality option without a Java control",
    ),
    (
        "plane_obstacle_net_classes",
        "quality option without a Java control",
    ),
    (
        "plane_obstacle_block",
        "quality option without a Java control",
    ),
    ("wire_keepout", "quality option without a Java control"),
    ("pad_contacts", "quality option without a Java control"),
    ("result_json", "a file path for the CLI's result manifest"),
    (
        "autorouter.save_intermediate_stages",
        "debugging option without a Java control",
    ),
    ("autorouter.sweep", "quality option without a Java control"),
    (
        "autorouter.sweep_passes",
        "quality option without a Java control",
    ),
    (
        "autorouter.sweep_after_optimizer",
        "quality option without a Java control",
    ),
    (
        "fanout.start_via_diameter_mm",
        "fanout tuning without a Java control",
    ),
    (
        "fanout.end_via_diameter_mm",
        "fanout tuning without a Java control",
    ),
    (
        "optimizer.enable_preflight_guards",
        "optimizer tuning without a Java control",
    ),
    (
        "optimizer.max_consecutive_failures",
        "optimizer tuning without a Java control",
    ),
    (
        "optimizer.max_consecutive_failures_pass1",
        "optimizer tuning without a Java control",
    ),
    (
        "optimizer.additional_ripup_cost_factor_at_start",
        "optimizer tuning without a Java control",
    ),
    (
        "optimizer.trace_ripup_cost_factor",
        "optimizer tuning without a Java control",
    ),
    (
        "optimizer.max_autoroute_passes",
        "optimizer tuning without a Java control",
    ),
    (
        "optimizer.board_update_strategy",
        "not read from JSON in Java either",
    ),
    (
        "optimizer.item_selection_strategy",
        "not read from JSON in Java either",
    ),
    (
        "scoring.unrouted_net_penalty",
        "score weight without a Java control",
    ),
    (
        "scoring.clearance_violation_penalty",
        "score weight without a Java control",
    ),
    (
        "scoring.bend_penalty",
        "score weight without a Java control",
    ),
    (
        "router_scoring.unrouted_first_half_weight",
        "score weight without a Java control",
    ),
    (
        "router_scoring.unrouted_second_half_weight",
        "score weight without a Java control",
    ),
    (
        "router_scoring.clearance_violation_depth_scale",
        "score weight without a Java control",
    ),
    (
        "optimizer_scoring.excess_wire_length_weight",
        "score weight without a Java control",
    ),
    (
        "optimizer_scoring.excess_via_weight",
        "score weight without a Java control",
    ),
    (
        "optimizer_scoring.excess_bend_weight",
        "score weight without a Java control",
    ),
    (
        "optimizer_scoring.length_floor",
        "score weight without a Java control",
    ),
    (
        "optimizer_scoring.difficulty_scale_floor",
        "score weight without a Java control",
    ),
    (
        "debug.audit_incremental",
        "engine self-check (P02.1), set with the CLI or a settings file",
    ),
];

/// True if the window has a control for the field `path`: one of
/// [`CONTROLS`] or a row of the "Algorithms" section.
pub fn has_control(path: &str) -> bool {
    CONTROLS.iter().any(|(p, _)| *p == path) || algorithms::algorithm_paths().contains(&path)
}

/// Every path with a control, [`CONTROLS`] first, each once.
pub fn control_paths() -> Vec<&'static str> {
    let mut paths: Vec<&'static str> = CONTROLS.iter().map(|(p, _)| *p).collect();
    for path in algorithms::algorithm_paths() {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// The fields that have neither a control nor a reason: empty when the
/// table is complete (`every_router_setting_has_a_control`).
pub fn uncovered_fields<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    paths
        .into_iter()
        .filter(|path| {
            !has_control(path)
                && !CLI_ONLY.iter().any(|(p, _)| p == path)
                && !path.starts_with(JAVA_QUIRKS_PREFIX)
        })
        .collect()
}

/// The `java_quirks.*` switches (N06) have no control: they are set with the
/// CLI, a settings file or a profile, one Java bug each.
pub const JAVA_QUIRKS_PREFIX: &str = "java_quirks.";

/// A layer of the open board, for the rows of the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerInfo {
    pub name: String,
    /// False for a power plane layer.
    pub signal: bool,
}

/// What the user is editing: the engine's settings of the open board.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub patch: SettingsPatch,
    cores: usize,
}

impl Draft {
    /// A draft of `patch`; `cores` is the machine's (the thread limit).
    pub fn new(patch: SettingsPatch, cores: usize) -> Draft {
        Draft { patch, cores }
    }

    /// The resolved settings the draft stands for (what the router reads).
    pub fn settings(&self) -> RouterSettings {
        RouterSettings::from_patch(&self.patch, self.cores)
    }

    /// The value of the field `path` in the draft.
    pub fn get(&self, path: &str) -> Option<FieldValue> {
        (field_by_path(path)?.get)(&self.patch)
    }

    /// Sets the field `path` (a value of the wrong type is ignored, as the
    /// engine's setter does).
    pub fn set(&mut self, path: &str, value: FieldValue) {
        if let Some(field) = field_by_path(path) {
            (field.set)(&mut self.patch, value);
        }
    }

    /// The range Java clamps the integer control `path` to.
    pub fn int_range(&self, path: &str) -> (i32, i32) {
        match path {
            "autorouter.max_passes" => (0, 9999),
            "max_threads" => (1, i32::try_from(self.cores.max(1)).unwrap_or(i32::MAX)),
            _ => (1, i32::MAX),
        }
    }

    /// Sets the integer control `path`, clamped to its range as Java's
    /// `normalizeIntInput` does (a value that is no number keeps the old one).
    pub fn set_int(&mut self, path: &str, input: crate::dialogs::Input<'_>) {
        let (min, max) = self.int_range(path);
        let old = match self.get(path) {
            Some(FieldValue::Int(v)) => v,
            _ => min,
        };
        let value = timeout::normalize_int_input(input, old, min, max);
        self.set(path, FieldValue::Int(value));
    }

    /// The job timeout as a number and a unit.
    pub fn timeout(&self) -> timeout::Decomposed {
        let text = match self.get("job_timeout") {
            Some(FieldValue::Str(text)) => Some(text),
            _ => None,
        };
        decompose_timeout(timeout::parse_timeout(text.as_deref()).total_seconds)
    }

    /// Sets the job timeout from a number and a unit (capped at four weeks).
    pub fn set_timeout(&mut self, value: i64, unit: TimeoutUnit) {
        self.set(
            "job_timeout",
            FieldValue::Str(timeout::build_timeout_in(value, unit)),
        );
    }

    /// The scalar control values that differ from `baseline`, as the path
    /// and Java's text of the value: what the app keeps between runs. The
    /// layer rows belong to a board and are not kept.
    pub fn overrides_against(&self, baseline: &SettingsPatch) -> Vec<(String, String)> {
        control_paths()
            .into_iter()
            .filter(|path| {
                !matches!(
                    *path,
                    "scoring.preferred_direction_trace_cost"
                        | "scoring.undesired_direction_trace_cost"
                )
            })
            .filter_map(|path| {
                let field = field_by_path(path)?;
                let now = (field.get)(&self.patch)?;
                ((field.get)(baseline).as_ref() != Some(&now))
                    .then(|| (path.to_string(), now.to_string()))
            })
            .collect()
    }

    /// The settings a job starts with: `baseline`, the user's kept
    /// `overrides`, then the front-end mode's router flags `router`. The flags
    /// win over the saved settings, as the command line does over a settings
    /// file, and a `--profile` moves the values that are still the profile's
    /// first (N01.3). The one place that merge is made, for the job
    /// ([`App::autoroute_patch`](crate::app::App)) and for the readers'
    /// `java_quirks.*` before the board is read
    /// ([`crate::session::LoadSettings`]).
    pub fn user_patch(
        baseline: SettingsPatch,
        overrides: &[(String, String)],
        router: &SettingsPatch,
        cores: usize,
    ) -> SettingsPatch {
        let mut draft = Draft::new(baseline, cores);
        draft.apply_overrides(overrides);
        if router.profile.is_some()
            && let Ok(profile) = freerouting::settings::Profile::of_patch(router)
        {
            draft.set_profile(profile);
        }
        draft.patch.apply_from(router);
        draft.patch
    }

    /// Puts kept overrides on the draft. A path or text that does not parse
    /// is ignored (a settings file written by another version). A kept
    /// profile is switched to first, so the other values land on it.
    pub fn apply_overrides(&mut self, overrides: &[(String, String)]) {
        if let Some((_, text)) = overrides.iter().find(|(path, _)| path == "profile")
            && let Ok(profile) = freerouting::settings::Profile::parse(text)
        {
            self.set_profile(profile);
        }
        for (path, text) in overrides.iter().filter(|(path, _)| path != "profile") {
            if has_control(path) {
                let _ = self.patch.set_path(path, text);
            }
        }
    }
}

/// What a frame of the window asks for.
#[derive(Debug, Default)]
pub struct DialogOut {
    pub commands: Vec<Command>,
}

fn int_control(ui: &mut egui::Ui, draft: &mut Draft, path: &str, label: &str, tip: &str) {
    let (min, max) = draft.int_range(path);
    let mut value = match draft.get(path) {
        Some(FieldValue::Int(v)) => v,
        _ => min,
    };
    ui.label(label).on_hover_text(tip);
    let response = ui
        .add(egui::DragValue::new(&mut value).range(min..=max))
        .on_hover_text(tip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, label));
    ui.end_row();
    draft.set_int(path, crate::dialogs::Input::Number(f64::from(value)));
}

fn bool_control(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    path: &str,
    label: &str,
    tip: &str,
    enabled: bool,
) {
    let mut value = matches!(draft.get(path), Some(FieldValue::Bool(true)));
    let response = ui.add_enabled(enabled, egui::Checkbox::new(&mut value, label));
    response.on_hover_text(tip);
    draft.set(path, FieldValue::Bool(value));
}

/// The rows of the layer table (Java's per-layer controls).
fn layer_rows(ui: &mut egui::Ui, draft: &mut Draft, layers: &[LayerInfo]) {
    egui::Grid::new("autoroute_layers")
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Layer")
                .on_hover_text("Lists the layers configured on the routing board.");
            ui.strong("Active")
                .on_hover_text("Enable the Autorouter to route traces on this layer.");
            ui.strong("Preferred Direction").on_hover_text(
                "Set the preferred trace direction on this layer to horizontal or vertical.",
            );
            ui.end_row();
            for (i, layer) in layers.iter().enumerate() {
                let settings = draft.settings();
                let mut active = settings.layer_active(i);
                let mut horizontal = settings.preferred_direction_is_horizontal(i);
                ui.label(&layer.name);
                let name = layer.name.as_str();
                let response = ui
                    .add_enabled(layer.signal, egui::Checkbox::without_text(&mut active))
                    .on_disabled_hover_text(crate::text::POWER_LAYER_TOOLTIP);
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Checkbox,
                        layer.signal,
                        active,
                        format!("Active: {name}"),
                    )
                });
                ui.add_enabled_ui(layer.signal, |ui| {
                    egui::ComboBox::from_id_salt(("autoroute_direction", i))
                        .selected_text(if horizontal { "Horizontal" } else { "Vertical" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut horizontal, true, "Horizontal");
                            ui.selectable_value(&mut horizontal, false, "Vertical");
                        });
                });
                ui.end_row();
                // Only a change writes, so a board's own settings stay as they are.
                if layer.signal && active != settings.layer_active(i) {
                    draft.patch.set_layer_active(i, active);
                }
                if layer.signal && horizontal != settings.preferred_direction_is_horizontal(i) {
                    draft
                        .patch
                        .set_preferred_direction_is_horizontal(i, horizontal);
                }
            }
        });
}

/// The advanced table: trace costs and the bend cost of each signal layer.
fn cost_rows(ui: &mut egui::Ui, draft: &mut Draft, layers: &[LayerInfo]) {
    egui::Grid::new("autoroute_costs").striped(true).show(ui, |ui| {
        ui.strong("Layer");
        ui.strong("In preferred direction")
            .on_hover_text("Cost of routing a trace in the preferred direction. Lower cost encourages routing on this layer in that direction.");
        ui.strong("Against preferred direction")
            .on_hover_text("Cost of routing a trace against the preferred direction. Higher cost discourages routing on this layer in the perpendicular direction.");
        ui.strong("Bend cost").on_hover_text(crate::text::BEND_COST_TOOLTIP);
        ui.end_row();
        for (i, layer) in layers.iter().enumerate().filter(|(_, l)| l.signal) {
            let settings = draft.settings();
            let mut preferred = settings.preferred_direction_trace_costs(i);
            let mut against = settings.against_preferred_direction_trace_costs(i);
            let mut bend = settings.bend_cost(i);
            ui.label(&layer.name);
            let name = layer.name.as_str();
            ui.add(egui::DragValue::new(&mut preferred).range(0.1..=1000.0).speed(0.05))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, format!("In preferred direction: {name}")));
            ui.add(egui::DragValue::new(&mut against).range(0.1..=1000.0).speed(0.05))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, format!("Against preferred direction: {name}")));
            ui.add(egui::DragValue::new(&mut bend).range(0.0..=9.9).speed(0.05))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, format!("Bend cost: {name}")));
            ui.end_row();
            // Only a change writes: setting a trace cost marks the costs as
            // initialised, which stops the board tuning.
            if preferred != settings.preferred_direction_trace_costs(i) {
                draft.patch.set_preferred_direction_trace_costs(i, preferred);
            }
            if against != settings.against_preferred_direction_trace_costs(i) {
                draft.patch.set_against_preferred_direction_trace_costs(i, against);
            }
            if bend != settings.bend_cost(i) {
                draft.patch.set_bend_cost(i, bend);
            }
        }
    });
}

/// The body of the window. Returns the commands of the buttons.
pub fn ui(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    layers: &[LayerInfo],
    stop_on_canvas_click: bool,
) -> DialogOut {
    let mut out = DialogOut::default();
    algorithms::profile_selector(ui, draft);
    algorithms::algorithms(ui, draft);
    ui.separator();
    ui.label("Layer");
    layer_rows(ui, draft, layers);
    ui.separator();
    ui.label("Routing Stages:")
        .on_hover_text("Select the routing stages to run during the job.");
    ui.horizontal(|ui| {
        bool_control(
            ui,
            draft,
            "fanout.enabled",
            "Fanout",
            crate::text::FANOUT_TOOLTIP,
            true,
        );
        bool_control(
            ui,
            draft,
            "autorouter.enabled",
            "Auto-routing",
            crate::text::AUTOROUTE_TOOLTIP,
            true,
        );
        let tip = crate::text::OPTIMIZATION_TOOLTIP;
        bool_control(
            ui,
            draft,
            "optimizer.enabled",
            "Optimization",
            tip,
            OPTIMIZER_PORTED,
        );
    });
    ui.separator();
    egui::Grid::new("autoroute_numbers").show(ui, |ui| {
        ui.label("Vias allowed:");
        bool_control(
            ui,
            draft,
            "allowed_via_types",
            "",
            crate::text::VIAS_ALLOWED_TOOLTIP,
            true,
        );
        ui.end_row();
        int_control(
            ui,
            draft,
            "scoring.via_costs",
            "Via costs:",
            crate::text::VIA_COSTS_TOOLTIP,
        );
        int_control(
            ui,
            draft,
            "scoring.plane_via_costs",
            "Power or ground plane via cost:",
            crate::text::PLANE_VIA_COSTS_TOOLTIP,
        );
        int_control(
            ui,
            draft,
            "scoring.start_ripup_costs",
            "Rip-up start cost:",
            crate::text::RIPUP_COSTS_TOOLTIP,
        );
        int_control(
            ui,
            draft,
            "autorouter.max_passes",
            "Maximum routing passes:",
            crate::text::MAX_PASSES_TOOLTIP,
        );
        int_control(
            ui,
            draft,
            "max_threads",
            "Maximum routing threads:",
            crate::text::MAX_THREADS_TOOLTIP,
        );
        ui.label("Job timeout:")
            .on_hover_text(crate::text::JOB_TIMEOUT_TOOLTIP);
        let mut shown = draft.timeout();
        ui.horizontal(|ui| {
            let mut value = shown.value;
            let changed = ui
                .add(egui::DragValue::new(&mut value).range(0..=shown.unit.max_units()))
                .changed();
            let mut unit = shown.unit;
            egui::ComboBox::from_id_salt("autoroute_timeout_unit")
                .selected_text(unit.label())
                .show_ui(ui, |ui| {
                    for candidate in TimeoutUnit::ALL {
                        ui.selectable_value(&mut unit, candidate, candidate.label());
                    }
                });
            if changed || unit != shown.unit {
                shown = timeout::Decomposed { value, unit };
                draft.set_timeout(shown.value, shown.unit);
            }
        });
        ui.end_row();
    });
    ui.separator();
    egui::CollapsingHeader::new("Advanced Autorouter Settings").show(ui, |ui| {
        ui.label("Trace costs by layer");
        cost_rows(ui, draft, layers);
    });
    ui.separator();
    // A GUI preference, not a router setting: it takes effect at once and is
    // kept between runs, so it does not wait for Apply.
    let mut stop = stop_on_canvas_click;
    if ui
        .checkbox(&mut stop, crate::text::STOP_ON_CLICK_CHECKBOX)
        .on_hover_text(crate::text::STOP_ON_CLICK_TOOLTIP)
        .changed()
    {
        out.commands.push(Command::SetStopOnCanvasClick(stop));
    }
    ui.separator();
    ui.weak("Click Apply to use the current values.");
    ui.horizontal(|ui| {
        if ui.button("Apply").clicked() {
            out.commands.push(Command::ApplyAutorouteSettings(Box::new(
                draft.patch.clone(),
            )));
        }
        if ui.button("Close").clicked() {
            out.commands.push(Command::CloseAutorouteSettings);
        }
    });
    out
}

/// The paths of the engine's field table, for the tests.
pub fn field_paths() -> Vec<&'static str> {
    FIELDS.iter().map(|f| f.path).collect()
}

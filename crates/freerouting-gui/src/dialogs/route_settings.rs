//! Settings > Routing: Java's `WindowRouteParameter`. The board-rule part
//! (G06.6) is [`static_section`]; the interactive part (G09.2) is
//! [`interactive_section`]: route mode, rule selection with the manual rules
//! (`ManualRulesPanel`), push-and-shove, drag components, via snap,
//! highlight obstacle, neckdown, the pull-tight radius and accuracy. The
//! interactive fields are the GUI's [`InteractiveSettings`], stored with its
//! settings and passed to every editor call; they are not saved with the
//! board.
//!
//! The controls and the rule each one sets, through [`RulesEdit`]:
//!
//! | Control | Edit |
//! |---|---|
//! | Snap angle | `setCurrentSnapAngle`; asks first when traces would not fit |
//! | Ignore conduction areas | `setIgnoreConduction` (`changePlaneAsObstacle(!value)`) |
//! | Restrict pin exit directions, Pad-to-turn gap | `setPinEdgeToTurnDist` |
//! | Clearance compensation | `setClearanceCompensation` |
//! | Keepout outside board outline | `OutLineKeepoutListener` |
//!
//! These are board rules: they are saved with the board (DSN and `.rules`,
//! G08), not in the GUI's settings.

use eframe::egui::{self, Id};
use freerouting::board::BasicBoard;
use freerouting::board::structure::AngleRestriction;

use super::input::{Input, parse_clearance_table_value};
use super::rules_edit::RulesEdit;
use super::{RuleView, RulesDialog, confirm_window, text_cell};
use crate::commands::Command;
use crate::text;

#[derive(Default)]
pub struct State {
    pub open: bool,
    /// The Component Movement Settings window is open.
    pub move_open: bool,
    /// The layer the manual trace width applies to (`None`: all layers).
    pub manual_layer: Option<usize>,
    /// A snap angle waiting for the answer to "change anyway?".
    pub asking: Option<AngleRestriction>,
}

/// True if some trace has an angle the new restriction does not support: a
/// corner that is not a multiple of 90 degrees (`isOrthogonal`) or of 45
/// degrees (`isMultipleOf45Degree`). `None` never asks.
pub fn free_angle_traces_found(board: &BasicBoard, target: AngleRestriction) -> bool {
    board.traces().into_iter().any(|id| {
        board
            .item(id)
            .and_then(|i| i.as_trace())
            .is_some_and(|t| match target {
                AngleRestriction::NinetyDegree => !t.polyline.is_orthogonal(),
                AngleRestriction::FortyfiveDegree => !t.polyline.is_multiple_of_45_degree(),
                AngleRestriction::None => false,
            })
    })
}

/// The confirmation text of a snap angle change (`change_snap_angle_90` and
/// `change_snap_angle_45`).
pub fn snap_question(target: AngleRestriction) -> Option<&'static str> {
    match target {
        AngleRestriction::NinetyDegree => Some(text::RP_CHANGE_SNAP_90),
        AngleRestriction::FortyfiveDegree => Some(text::RP_CHANGE_SNAP_45),
        AngleRestriction::None => None,
    }
}

/// What choosing a snap angle does (`SnapAngle90Listener` and the others): the
/// current angle is a no-op for 90 and 45 degrees, a change with traces that
/// do not fit asks first, else the edit is sent.
pub fn choose_snap_angle(
    board: &BasicBoard,
    state: &mut State,
    target: AngleRestriction,
) -> Option<Command> {
    let current = board.rules.trace_angle_restriction();
    if target != AngleRestriction::None && current == target {
        return None;
    }
    if free_angle_traces_found(board, target) {
        state.asking = Some(target);
        return None;
    }
    Some(Command::EditRules(RulesEdit::SetSnapAngle(target)))
}

/// A number the way `NumberFormat` with at most `digits` fraction digits and
/// no grouping writes it.
pub fn plain_number(value: f64, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

/// `RestrictPinExitDirectionsListener`: selecting sets the distance to the
/// smallest trace half width (`boardToUser`, then back through
/// `userToBoard`), clearing sets 0.
pub fn restrict_pin_exits_edit(view: &RuleView<'_>, restrict: bool) -> RulesEdit {
    if restrict {
        let min = f64::from(view.board.rules.min_trace_half_width());
        let user = view.units.transform.board_to_dsn(min);
        RulesEdit::SetPinEdgeToTurnDist(view.units.transform.dsn_to_board(user))
    } else {
        RulesEdit::SetPinEdgeToTurnDist(0.0)
    }
}

/// The pad-to-turn gap typed in the display unit, as a `float` like Java's
/// field value, in board units.
pub fn pad_to_turn_edit(view: &RuleView<'_>, typed: &str) -> Option<RulesEdit> {
    let value = parse_clearance_table_value(Input::Text(typed.trim()))?;
    Some(RulesEdit::SetPinEdgeToTurnDist(
        view.units.to_board(f64::from(value)),
    ))
}

/// Whether the board has its outline keepout on.
pub fn keepout_outside(board: &BasicBoard) -> bool {
    board
        .outline()
        .and_then(|o| o.as_board_outline())
        .is_some_and(|o| o.keepout_outside_outline_generated())
}

/// The board-rule controls, for the Routing Settings window.
pub fn static_section(ui: &mut egui::Ui, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    let board = view.board;
    let current = board.rules.trace_angle_restriction();
    ui.horizontal(|ui| {
        ui.label(text::RP_SNAP_ANGLE)
            .on_hover_text(text::RP_SNAP_ANGLE_TIP);
        for (angle, label, tip) in [
            (AngleRestriction::NinetyDegree, text::RP_90, text::RP_90_TIP),
            (
                AngleRestriction::FortyfiveDegree,
                text::RP_45,
                text::RP_45_TIP,
            ),
            (AngleRestriction::None, text::RP_NONE, text::RP_NONE_TIP),
        ] {
            if ui
                .radio(current == angle, label)
                .on_hover_text(tip)
                .clicked()
                && let Some(command) = choose_snap_angle(board, state, angle)
            {
                out.push(command);
            }
        }
    });
    ui.separator();
    let mut ignore = board.rules.ignore_conduction();
    if ui
        .checkbox(&mut ignore, text::RP_IGNORE_CONDUCTION)
        .on_hover_text(text::RP_IGNORE_CONDUCTION_TIP)
        .changed()
    {
        out.push(Command::EditRules(RulesEdit::SetIgnoreConduction(ignore)));
    }
    ui.separator();
    let gap = board.rules.pin_edge_to_turn_dist();
    let mut restrict = gap > 0.0;
    if ui
        .checkbox(&mut restrict, text::RP_RESTRICT_PIN_EXITS)
        .on_hover_text(text::RP_RESTRICT_PIN_EXITS_TIP)
        .changed()
    {
        out.push(Command::EditRules(restrict_pin_exits_edit(view, restrict)));
    }
    ui.horizontal(|ui| {
        ui.label(text::RP_PAD_TO_TURN)
            .on_hover_text(text::RP_PAD_TO_TURN_TIP);
        let shown = plain_number(view.units.to_display(gap), 3);
        if let Some(typed) = text_cell(ui, Id::new("route_pad_to_turn"), &shown, 70.0)
            && let Some(edit) = pad_to_turn_edit(view, &typed)
        {
            out.push(Command::EditRules(edit));
        }
        ui.label(text::unit_label(view.units.display));
    });
    ui.separator();
    let used = board.search_trees().is_clearance_compensation_used();
    let mut compensation = used;
    let tip = text::fill(
        text::RP_COMPENSATION_TIP_CURRENT,
        &[if used { text::RP_ON } else { text::RP_OFF }],
    );
    if ui
        .checkbox(&mut compensation, text::RP_COMPENSATION)
        .on_hover_text(tip)
        .changed()
    {
        out.push(Command::EditRules(RulesEdit::SetClearanceCompensation(
            compensation,
        )));
    }
    ui.separator();
    let mut keepout = keepout_outside(board);
    if ui
        .add_enabled(
            board.outline().is_some(),
            egui::Checkbox::new(&mut keepout, text::RP_KEEPOUT_OUTSIDE),
        )
        .on_hover_text(text::RP_KEEPOUT_OUTSIDE_TIP)
        .changed()
    {
        out.push(Command::EditRules(RulesEdit::SetKeepoutOutsideOutline(
            keepout,
        )));
    }
    out
}

/// The interactive controls (G09.2). Each change returns one
/// [`Command::UpdateInteractive`] with the whole settings, or a manual-rule
/// command.
pub fn interactive_section(
    ui: &mut egui::Ui,
    state: &mut State,
    view: &RuleView<'_>,
) -> Vec<Command> {
    let mut out = Vec::new();
    let Some(interactive) = view.interactive else {
        return out;
    };
    let (current, manual) = (interactive.settings, interactive.manual);
    let mut settings = *current;
    ui.horizontal(|ui| {
        ui.label(text::RP_ROUTE_MODE)
            .on_hover_text(text::RP_ROUTE_MODE_TIP);
        let dynamic = ui
            .radio(!settings.stitch_route, text::RP_DYNAMIC)
            .on_hover_text(text::RP_DYNAMIC_TIP);
        let stitching = ui
            .radio(settings.stitch_route, text::RP_STITCHING)
            .on_hover_text(text::RP_STITCHING_TIP);
        if dynamic.clicked() {
            settings.stitch_route = false;
        }
        if stitching.clicked() {
            settings.stitch_route = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label(text::RP_RULE_SELECTION)
            .on_hover_text(text::RP_RULE_SELECTION_TIP);
        let automatic = ui
            .radio(!settings.manual_rule_selection, text::RP_AUTOMATIC)
            .on_hover_text(text::RP_AUTOMATIC_TIP);
        let manual_choice = ui
            .radio(settings.manual_rule_selection, text::RP_MANUAL)
            .on_hover_text(text::RP_MANUAL_TIP);
        if automatic.clicked() {
            settings.manual_rule_selection = false;
        }
        if manual_choice.clicked() {
            settings.manual_rule_selection = true;
        }
    });
    if settings.manual_rule_selection {
        out.extend(manual_rules_panel(ui, state, view, manual));
    }
    for (value, label, tip) in [
        (
            &mut settings.push_enabled,
            text::RP_PUSH_SHOVE,
            text::RP_PUSH_SHOVE_TIP,
        ),
        (
            &mut settings.drag_components_enabled,
            text::RP_DRAG_COMPONENTS,
            text::RP_DRAG_COMPONENTS_TIP,
        ),
        (
            &mut settings.via_snap_to_smd_center,
            text::RP_VIA_SNAP,
            text::RP_VIA_SNAP_TIP,
        ),
        (
            &mut settings.highlight_routing_obstacle,
            text::RP_HIGHLIGHT_OBSTACLE,
            text::RP_HIGHLIGHT_OBSTACLE_TIP,
        ),
        (
            &mut settings.automatic_neckdown,
            text::RP_NECKDOWN,
            text::RP_NECKDOWN_TIP,
        ),
    ] {
        ui.checkbox(value, label).on_hover_text(tip);
    }
    // The pull-tight radius: the whole board (Java's slider maximum) or a
    // radius in the display unit.
    ui.horizontal(|ui| {
        ui.label(text::RP_PULL_TIGHT_REGION)
            .on_hover_text(text::RP_PULL_TIGHT_REGION_TIP);
        let mut whole = settings.pull_tight_region == i32::MAX;
        if ui
            .checkbox(&mut whole, text::RP_PULL_TIGHT_WHOLE_BOARD)
            .changed()
        {
            settings.pull_tight_region = if whole {
                i32::MAX
            } else {
                (view.units.to_board(1.0).round() as i32).max(1)
            };
        }
        if !whole {
            let shown = plain_number(
                view.units.to_display(f64::from(settings.pull_tight_region)),
                3,
            );
            if let Some(typed) = text_cell(ui, Id::new("route_pull_tight_region"), &shown, 70.0)
                && let Some(value) = parse_clearance_table_value(Input::Text(typed.trim()))
            {
                let board = view.units.to_board(f64::from(value)).round();
                if board >= 1.0 && board < f64::from(i32::MAX) {
                    settings.pull_tight_region = board as i32;
                }
            }
            ui.label(text::unit_label(view.units.display));
        }
    });
    ui.horizontal(|ui| {
        ui.label(text::RP_PULL_TIGHT_ACCURACY)
            .on_hover_text(text::RP_PULL_TIGHT_ACCURACY_TIP);
        let shown = settings.pull_tight_accuracy.to_string();
        if let Some(typed) = text_cell(ui, Id::new("route_pull_tight_accuracy"), &shown, 70.0)
            && let Ok(value) = typed.trim().parse::<i32>()
            && (1..=1_000_000).contains(&value)
        {
            settings.pull_tight_accuracy = value;
        }
    });
    if settings != *current {
        out.push(Command::UpdateInteractive(settings));
    }
    out
}

/// `ManualRulesPanel`: the via rule, the trace clearance class and the trace
/// width on one layer or on all of them.
fn manual_rules_panel(
    ui: &mut egui::Ui,
    state: &mut State,
    view: &RuleView<'_>,
    manual: &freerouting::interactive::ManualRules,
) -> Vec<Command> {
    let mut out = Vec::new();
    let rules = &view.board.rules;
    ui.group(|ui| {
        ui.label(egui::RichText::new(text::MR_TITLE).strong());
        // The via rule, by index in the board's via rules.
        let via_names: Vec<String> = (0..rules.via_rules.count())
            .filter_map(|i| rules.via_rules.nth(i))
            .map(|id| rules.via_rules.get(id).name.clone())
            .collect();
        ui.horizontal(|ui| {
            ui.label(text::MR_VIA_RULE)
                .on_hover_text(text::MR_VIA_RULE_TIP);
            let current = usize::try_from(manual.via_rule_index)
                .ok()
                .and_then(|i| via_names.get(i))
                .cloned()
                .unwrap_or_default();
            if let Some(name) =
                super::name_combo(ui, Id::new("manual_via_rule"), &current, &via_names)
                && let Some(index) = via_names.iter().position(|n| *n == name)
            {
                out.push(Command::SetManualViaRule(index as i32));
            }
        });
        let classes = view.clearance_class_names();
        ui.horizontal(|ui| {
            ui.label(text::MR_CLEARANCE_CLASS)
                .on_hover_text(text::MR_CLEARANCE_CLASS_TIP);
            let current = usize::try_from(manual.clearance_class)
                .ok()
                .and_then(|i| classes.get(i))
                .cloned()
                .unwrap_or_default();
            if let Some(name) = super::name_combo(ui, Id::new("manual_class"), &current, &classes)
                && let Some(index) = classes.iter().position(|n| *n == name)
            {
                out.push(Command::SetManualClearanceClass(index as i32));
            }
        });
        let layer_names: Vec<String> = view
            .board
            .layer_structure
            .layers
            .iter()
            .map(|l| l.name.clone())
            .collect();
        let mut choices = vec![text::MR_ALL_LAYERS.to_string()];
        choices.extend(layer_names.iter().cloned());
        ui.horizontal(|ui| {
            ui.label(text::MR_ON_LAYER)
                .on_hover_text(text::MR_ON_LAYER_TIP);
            let current = state
                .manual_layer
                .and_then(|l| layer_names.get(l))
                .cloned()
                .unwrap_or_else(|| text::MR_ALL_LAYERS.to_string());
            if let Some(name) = super::name_combo(ui, Id::new("manual_layer"), &current, &choices) {
                state.manual_layer = layer_names.iter().position(|n| *n == name);
            }
        });
        ui.horizontal(|ui| {
            ui.label(text::MR_TRACE_WIDTH)
                .on_hover_text(text::MR_TRACE_WIDTH_TIP);
            let layer = state.manual_layer.unwrap_or(0);
            let half = manual.trace_half_width(layer as i32);
            let shown = plain_number(view.units.to_display(2.0 * f64::from(half)), 3);
            if let Some(typed) = text_cell(ui, Id::new("manual_trace_width"), &shown, 70.0)
                && let Some(width) = parse_clearance_table_value(Input::Text(typed.trim()))
            {
                // `Math.round(userToBoard(0.5 * width))`
                let half = view.units.to_board(0.5 * f64::from(width)).round() as i32;
                if half > 0 {
                    let layers: Vec<usize> = match state.manual_layer {
                        Some(l) => vec![l],
                        None => (0..layer_names.len()).collect(),
                    };
                    for layer in layers {
                        out.push(Command::SetManualTraceWidth {
                            layer,
                            half_width: half,
                        });
                    }
                }
            }
            ui.label(text::unit_label(view.units.display));
        });
    });
    out
}

/// `WindowMoveParameter` (G09.6): the horizontal and vertical component
/// placement grid, in the display unit; 0 is no grid. Returns
/// [`Command::SetComponentGrid`] when a value changed.
pub fn move_settings(ui: &mut egui::Ui, view: &RuleView<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    let Some(interactive) = view.interactive else {
        return out;
    };
    let (mut horizontal, mut vertical) = interactive.component_grid;
    for (label, tip, id, value) in [
        (
            text::MV_HORIZONTAL_GRID,
            text::MV_HORIZONTAL_GRID_TIP,
            "move_grid_h",
            &mut horizontal,
        ),
        (
            text::MV_VERTICAL_GRID,
            text::MV_VERTICAL_GRID_TIP,
            "move_grid_v",
            &mut vertical,
        ),
    ] {
        ui.horizontal(|ui| {
            ui.label(label).on_hover_text(tip);
            let shown = plain_number(view.units.to_display(f64::from(*value)), 3);
            if let Some(typed) = text_cell(ui, Id::new(id), &shown, 70.0)
                && let Some(typed) = parse_clearance_table_value(Input::Text(typed.trim()))
                && typed >= 0.0
            {
                *value = view.units.to_board(f64::from(typed)).round() as i32;
            }
            ui.label(text::unit_label(view.units.display));
        });
    }
    if (horizontal, vertical) != interactive.component_grid {
        out.push(Command::SetComponentGrid {
            horizontal,
            vertical,
        });
    }
    out
}

pub fn show(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    if state.move_open {
        let mut open = true;
        egui::Window::new(text::MV_TITLE)
            .id(Id::new(("rules", RulesDialog::MoveSettings)))
            .open(&mut open)
            .default_size([420.0, 120.0])
            .show(ctx, |ui| out.extend(move_settings(ui, view)));
        state.move_open = open;
    }
    if state.open {
        let mut open = true;
        egui::Window::new(text::RP_TITLE)
            .id(Id::new(("rules", RulesDialog::RouteSettings)))
            .open(&mut open)
            .default_size([360.0, 320.0])
            .show(ctx, |ui| {
                out.extend(interactive_section(ui, state, view));
                if view.interactive.is_some() {
                    ui.separator();
                }
                out.extend(static_section(ui, state, view));
            });
        state.open = open;
    }
    if let Some(target) = state.asking
        && let Some(question) = snap_question(target)
    {
        if let Some(yes) = confirm_window(
            ctx,
            "route_snap_angle",
            text::RP_TITLE,
            question,
            text::YES,
            text::CANCEL,
        ) {
            state.asking = None;
            if yes {
                out.push(Command::EditRules(RulesEdit::SetSnapAngle(target)));
            }
        }
    }
    out
}

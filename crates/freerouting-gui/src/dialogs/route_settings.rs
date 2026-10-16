//! Settings > Routing: the board-rule part of Java's `WindowRouteParameter`
//! (G06.6). The interactive fields (route mode, push and shove, pull-tight
//! radius and accuracy, neckdown and the rest) are G09.2's and join this
//! window through [`static_section`].
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

pub fn show(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    if state.open {
        let mut open = true;
        egui::Window::new(text::RP_TITLE)
            .id(Id::new(("rules", RulesDialog::RouteSettings)))
            .open(&mut open)
            .default_size([360.0, 320.0])
            .show(ctx, |ui| out.extend(static_section(ui, state, view)));
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

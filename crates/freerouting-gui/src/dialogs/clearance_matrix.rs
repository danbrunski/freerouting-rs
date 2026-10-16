//! Rules > Clearance Matrix: Java's `WindowClearanceMatrix` (G06.3).
//!
//! A layer drop-down (all layers, inner layers when the board has more than
//! two signal layers, or one signal layer) and a table of the clearance
//! between every pair of classes on that layer. A cell edit sends
//! [`RulesEdit::SetClearance`], which sets the value both ways
//! (`applyClearanceValue`). Add Class asks for a name (`addClass`); Prune
//! walks the classes in the order of `pruneClearanceMatrix` and asks before
//! each removal.
//!
//! **The prune order decides which class survives.** For `i` from the last
//! class down to 2 and, for each `i`, `j` from the last class down to 0
//! (`j != i`): the first `j` with `is_equal(i, j)` asks
//! `confirm_remove_class_message`. On Yes the items of class `i` move to `j`
//! and class `i` is removed ([`RulesEdit::MergeClearanceClass`]); on No the
//! loop goes on with the next `i`. [`next_prune_candidate`] is that walk, one
//! candidate at a time, because the dialog cannot block on the answer.
//! `prune_candidates_equal_the_engines_prune` checks it against B08's
//! `prune_clearance_matrix`.

use eframe::egui::{self, Id};
use freerouting::geometry::jmath::round_to_i32;
use freerouting::rules::ClearanceMatrix;
use freerouting::rules::edit::ClearanceLayer;

use super::input::{Input, parse_clearance_table_value};
use super::rules_edit::RulesEdit;
use super::{RuleView, RulesDialog, confirm_window, prompt_window, text_cell};
use crate::commands::Command;
use crate::text;

/// A Prune in progress: the next class `i` to look at, and the candidate
/// that waits for the user's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prune {
    /// The next `i` of the outer loop.
    pub next_i: i32,
    /// `(i, j)` of the question on screen.
    pub asking: Option<(i32, i32)>,
}

/// A cell edit that waits for the "already assigned" confirmation.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingEdit {
    pub row: usize,
    pub column: usize,
    pub board_value: i32,
    pub message: String,
}

#[derive(Default)]
pub struct State {
    pub open: bool,
    /// The layer being edited.
    pub layer: Option<ClearanceLayer>,
    /// The name typed for Add Class, while the prompt is open.
    pub new_name: Option<String>,
    pub prune: Option<Prune>,
    pub pending: Option<PendingEdit>,
}

/// The next question of a Prune that starts at class `from_i`: the first `i`
/// (counting down to 2) that has a `j` (counting down from the last class,
/// `j != i`) with `is_equal(i, j)`, and that `j`.
pub fn next_prune_candidate(matrix: &ClearanceMatrix, from_i: i32) -> Option<(i32, i32)> {
    let mut i = from_i.min(matrix.class_count() as i32 - 1);
    while i >= 2 {
        let mut j = matrix.class_count() as i32 - 1;
        while j >= 0 {
            if i != j && matrix.is_equal(i, j) {
                return Some((i, j));
            }
            j -= 1;
        }
        i -= 1;
    }
    None
}

/// The items of the layer drop-down: all layers, inner layers (more than two
/// signal layers), then each signal layer with its board layer number.
pub fn layer_choices(view: &RuleView<'_>) -> Vec<(ClearanceLayer, String)> {
    let layers = &view.board.layer_structure;
    let mut out = vec![(ClearanceLayer::All, text::LAYER_ALL.to_string())];
    if layers.signal_layer_count() > 2 {
        out.push((ClearanceLayer::Inner, text::LAYER_INNER.to_string()));
    }
    for n in 0..layers.signal_layer_count() {
        let layer = layers.get_signal_layer(n);
        out.push((
            ClearanceLayer::Layer(layer),
            layers.layers[layer].name.clone(),
        ));
    }
    out
}

/// The text of one cell: the clearance of classes `i` and `j` on `layer` in
/// the display unit, or `-1` when it depends on the layer and the choice is
/// "all layers" or "inner" (`ClearanceTableModel.setValues`).
pub fn cell_text(view: &RuleView<'_>, i: usize, j: usize, layer: ClearanceLayer) -> String {
    let m = &view.board.rules.clearance_matrix;
    let value = |l: i32| {
        view.units
            .to_display(f64::from(m.get_value(i as i32, j as i32, l, false)))
    };
    match layer {
        ClearanceLayer::All if m.is_layer_dependent(i, j) => "-1".to_string(),
        ClearanceLayer::Inner if m.is_inner_layer_dependent(i, j) => "-1".to_string(),
        ClearanceLayer::All => text::java_float_to_string(value(0) as f32),
        ClearanceLayer::Inner => text::java_float_to_string(value(1) as f32),
        ClearanceLayer::Layer(l) => text::java_float_to_string(value(l as i32) as f32),
    }
}

/// The board value of a typed cell, or `None` when the text is not a number:
/// `(int) Math.round(userToBoard(float))`.
pub fn cell_board_value(view: &RuleView<'_>, typed: &str) -> Option<i32> {
    let value = parse_clearance_table_value(Input::Text(typed))?;
    Some(round_to_i32(view.units.to_board(f64::from(value))))
}

/// The confirmation text of a cell whose classes both have items
/// (`clearance_class_already_assigned_single` / `_pair`), or `None`.
pub fn assigned_message(view: &RuleView<'_>, row: usize, column: usize) -> Option<String> {
    let board = view.board;
    if !board.clearance_items_assigned(row as i32, column as i32) {
        return None;
    }
    let m = &board.rules.clearance_matrix;
    let name = |i: usize| m.get_name(i as i32).unwrap_or_default().to_string();
    Some(if row == column {
        text::fill(text::CM_ASSIGNED_SINGLE, &[&name(row)])
    } else {
        text::fill(text::CM_ASSIGNED_PAIR, &[&name(row), &name(column)])
    })
}

pub fn show(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    if !state.open {
        return out;
    }
    let layers = layer_choices(view);
    let layer = state
        .layer
        .filter(|l| layers.iter().any(|(c, _)| c == l))
        .unwrap_or(ClearanceLayer::All);
    state.layer = Some(layer);
    let mut open = true;
    egui::Window::new(text::CM_TITLE)
        .id(Id::new(("rules", RulesDialog::ClearanceMatrix)))
        .open(&mut open)
        .default_size([760.0, 360.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(text::CM_LAYER).on_hover_text(text::CM_LAYER_TIP);
                let shown = layers
                    .iter()
                    .find(|(c, _)| *c == layer)
                    .map_or("", |(_, n)| n.as_str());
                let combo = egui::ComboBox::from_id_salt("clearance_layer")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for (choice, name) in &layers {
                            if ui.selectable_label(*choice == layer, name).clicked() {
                                state.layer = Some(*choice);
                            }
                        }
                    });
                super::name_drop_down(ui, &combo.response, shown);
            });
            out.extend(table(ui, state, view, layer));
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button(text::CM_ADD_CLASS)
                    .on_hover_text(text::CM_ADD_CLASS_TIP)
                    .clicked()
                {
                    state.new_name = Some(String::new());
                }
                if ui
                    .button(text::CM_PRUNE)
                    .on_hover_text(text::CM_PRUNE_TIP)
                    .clicked()
                {
                    state.prune = Some(Prune {
                        next_i: view.board.rules.clearance_matrix.class_count() as i32 - 1,
                        asking: None,
                    });
                }
            });
        });
    state.open = open;
    out.extend(add_class_prompt(ctx, state, view));
    out.extend(prune_step(ctx, state, view));
    out.extend(pending_confirm(ctx, state));
    out
}

fn table(
    ui: &mut egui::Ui,
    state: &mut State,
    view: &RuleView<'_>,
    layer: ClearanceLayer,
) -> Vec<Command> {
    let mut out = Vec::new();
    let names = view.clearance_class_names();
    egui::ScrollArea::both()
        .max_height(420.0)
        .scroll_source(super::net_classes::no_drag())
        .show(ui, |ui| {
            egui::Grid::new("clearance_table")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong(text::CM_CLASS);
                    for name in &names {
                        ui.strong(name);
                    }
                    ui.end_row();
                    for (row, name) in names.iter().enumerate() {
                        ui.strong(name);
                        for column in 0..names.len() {
                            let shown = cell_text(view, row, column, layer);
                            // `isCellEditable`: not the first class's row, and not
                            // its column (the table's column 1).
                            let editable = row > 0 && column > 0;
                            if !editable {
                                ui.label(&shown);
                                continue;
                            }
                            let id = Id::new(("clearance_cell", row, column, format!("{layer:?}")));
                            if let Some(typed) = text_cell(ui, id, &shown, 64.0) {
                                let Some(board_value) = cell_board_value(view, &typed) else {
                                    continue;
                                };
                                match assigned_message(view, row, column) {
                                    Some(message) => {
                                        state.pending = Some(PendingEdit {
                                            row,
                                            column,
                                            board_value,
                                            message,
                                        });
                                    }
                                    None => {
                                        out.push(set_clearance(row, column, layer, board_value))
                                    }
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    out
}

fn set_clearance(row: usize, column: usize, layer: ClearanceLayer, board_value: i32) -> Command {
    Command::EditRules(RulesEdit::SetClearance {
        row,
        column,
        layer,
        board_value,
    })
}

/// The Yes / No question of a cell whose classes have items on the board.
fn pending_confirm(ctx: &egui::Context, state: &mut State) -> Vec<Command> {
    let Some(pending) = state.pending.clone() else {
        return Vec::new();
    };
    let layer = state.layer.unwrap_or(ClearanceLayer::All);
    match confirm_window(
        ctx,
        "clearance_assigned",
        text::CM_TITLE,
        &pending.message,
        text::YES,
        text::NO,
    ) {
        Some(yes) => {
            state.pending = None;
            if yes {
                vec![set_clearance(
                    pending.row,
                    pending.column,
                    layer,
                    pending.board_value,
                )]
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    }
}

/// Java asks again until the name is legal (`isLegalClassName`), then does
/// nothing for a name that exists (the check is case sensitive).
fn add_class_prompt(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let Some(buffer) = state.new_name.as_mut() else {
        return Vec::new();
    };
    let answer = prompt_window(
        ctx,
        "clearance_new_name",
        text::CM_TITLE,
        text::CM_NEW_NAME,
        buffer,
    );
    let Some(answer) = answer else {
        return Vec::new();
    };
    let Some(name) = answer else {
        state.new_name = None;
        return Vec::new();
    };
    let name = name.trim().to_string();
    if !super::input::is_legal_class_name(&name) {
        // Ask again.
        return Vec::new();
    }
    state.new_name = None;
    if view.clearance_class_names().contains(&name) {
        return Vec::new();
    }
    vec![Command::EditRules(RulesEdit::AddClearanceClass { name })]
}

/// Walks the prune: shows the question for the current candidate and sends
/// the merge on Yes. The candidate is looked up again on the board each
/// frame, so a changed matrix cannot leave a stale question.
fn prune_step(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let Some(prune) = state.prune.as_mut() else {
        return Vec::new();
    };
    let matrix = &view.board.rules.clearance_matrix;
    if prune.asking.is_none() {
        match next_prune_candidate(matrix, prune.next_i) {
            Some(pair) => prune.asking = Some(pair),
            None => {
                state.prune = None;
                return Vec::new();
            }
        }
    }
    let (i, j) = prune.asking.expect("a candidate");
    let name = matrix.get_name(i).unwrap_or_default().to_string();
    let question = text::fill(text::CM_CONFIRM_REMOVE, &[&name]);
    match confirm_window(
        ctx,
        "clearance_prune",
        text::CM_TITLE,
        &question,
        text::YES,
        text::NO,
    ) {
        Some(yes) => {
            prune.asking = None;
            prune.next_i = i - 1;
            if yes {
                vec![Command::EditRules(RulesEdit::MergeClearanceClass {
                    from: i,
                    to: j,
                })]
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    }
}

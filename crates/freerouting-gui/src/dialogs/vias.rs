//! The Edit Vias window: Java's `WindowEditVias` (G06.4), opened by the Edit
//! button for vias of the Via Rules window.
//!
//! Columns `NAME`, `PADSTACK`, `CLEARANCE_CLASS` and `ATTACH_SMD`; the
//! buttons Add and Remove. Add names the via `new_via_<n>` for the smallest
//! `n >= 1` not in use, with the first via padstack, the default net class's
//! via clearance class and attach SMD off. Remove refuses the last via and a
//! via that a via rule contains; the refusals are the executor's
//! ([`super::rules_edit`]) so that a test can send the edit without the
//! window. The window chooses the via to remove by a click on its name.

use eframe::egui::{self, Id};
use freerouting::rules::ViaInfoId;

use super::rules_edit::RulesEdit;
use super::{RuleView, RulesDialog, name_combo, text_cell};
use crate::commands::Command;
use crate::text;

#[derive(Default)]
pub struct State {
    pub open: bool,
    /// The row chosen for Remove (`ListSelectionModel.SINGLE_SELECTION`).
    pub selected: Option<ViaInfoId>,
}

pub fn show(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    if !state.open {
        return out;
    }
    let board = view.board;
    let infos: Vec<ViaInfoId> = board.rules.via_infos.iter().map(|(id, _)| id).collect();
    if state.selected.is_some_and(|s| !infos.contains(&s)) {
        state.selected = None;
    }
    let padstacks: Vec<String> = (0..board.library.via_padstack_count())
        .filter_map(|n| board.library.get_via_padstack(n))
        .map(|p| p.name.clone())
        .collect();
    let classes = view.clearance_class_names();
    let mut open = true;
    egui::Window::new(text::EV_TITLE)
        .id(Id::new(("rules", RulesDialog::EditVias)))
        .open(&mut open)
        .default_size([560.0, 260.0])
        .show(ctx, |ui| {
            egui::Grid::new("via_table").striped(true).show(ui, |ui| {
                for title in text::EV_COLUMNS {
                    ui.strong(title);
                }
                ui.end_row();
                for &via in &infos {
                    let info = board.rules.via_infos.get(via);
                    let id = |column: &str| Id::new(("via_cell", via, column));
                    // NAME: a click chooses the row, the field renames.
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(state.selected == Some(via), "▸")
                            .on_hover_text(info.name.as_str())
                            .clicked()
                        {
                            state.selected = Some(via);
                        }
                        if let Some(name) = text_cell(ui, id("name"), &info.name, 120.0) {
                            out.push(Command::EditRules(RulesEdit::RenameViaInfo { via, name }));
                        }
                    });
                    let padstack = board
                        .library
                        .padstacks
                        .get(info.padstack)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    if let Some(padstack) = name_combo(ui, id("padstack"), &padstack, &padstacks) {
                        out.push(Command::EditRules(RulesEdit::SetViaInfoPadstack {
                            via,
                            padstack,
                        }));
                    }
                    let class = board
                        .rules
                        .clearance_matrix
                        .get_name(info.clearance_class)
                        .unwrap_or_default()
                        .to_string();
                    if let Some(name) = name_combo(ui, id("clearance"), &class, &classes) {
                        out.push(Command::EditRules(RulesEdit::SetViaInfoClearanceClass {
                            via,
                            name,
                        }));
                    }
                    let mut attach = info.attach_smd_allowed;
                    if ui.checkbox(&mut attach, "").changed() {
                        out.push(Command::EditRules(RulesEdit::SetViaInfoAttachSmd {
                            via,
                            allowed: attach,
                        }));
                    }
                    ui.end_row();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button(text::EV_ADD)
                    .on_hover_text(text::EV_ADD_TIP)
                    .clicked()
                {
                    out.push(Command::EditRules(RulesEdit::AddViaInfo));
                }
                if ui
                    .button(text::EV_REMOVE)
                    .on_hover_text(text::EV_REMOVE_TIP)
                    .clicked()
                {
                    out.extend(remove_selected(state, &infos));
                }
            });
        });
    state.open = open;
    out
}

/// `RemoveViaListener`: with one via or fewer the message of the last via,
/// without a chosen row nothing, else the edit (which refuses a via a rule
/// contains).
fn remove_selected(state: &State, infos: &[ViaInfoId]) -> Vec<Command> {
    if infos.len() <= 1 {
        return vec![Command::Notice(text::EV_LAST_NOT_REMOVED.to_string())];
    }
    match state.selected {
        Some(via) => vec![Command::EditRules(RulesEdit::RemoveViaInfo(via))],
        None => Vec::new(),
    }
}

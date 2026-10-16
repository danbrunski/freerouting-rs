//! The Assign Net Class window: Java's `WindowAssignNetClass` (G06.2),
//! opened by the Assign button of the Net Classes window.
//!
//! One row per net with a drop-down of the net classes. A change sends
//! [`RulesEdit::SetNetClass`] (`Net.setClass`). Java sorts the rows by net
//! name, ignoring case (`Net.compareTo`); so does this window.

use eframe::egui::{self, Id};
use freerouting::board::BasicBoard;

use super::rules_edit::RulesEdit;
use super::{RuleView, RulesDialog, name_combo};
use crate::commands::Command;
use crate::text;

#[derive(Default)]
pub struct State {
    pub open: bool,
}

/// The net numbers in the order of the window: by name, ignoring case; nets
/// with the same name keep their number order.
pub fn net_order(board: &BasicBoard) -> Vec<i32> {
    let mut nets: Vec<(String, i32)> = (1..=board.rules.nets.max_net_number())
        .filter_map(|n| {
            board
                .rules
                .nets
                .get(n)
                .map(|net| (net.name.to_lowercase(), n))
        })
        .collect();
    nets.sort();
    nets.into_iter().map(|(_, n)| n).collect()
}

pub fn show(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    if !state.open {
        return out;
    }
    let board = view.board;
    let classes: Vec<(freerouting::rules::NetClassId, String)> = board
        .rules
        .net_classes
        .iter()
        .map(|(id, c)| (id, c.name().to_string()))
        .collect();
    let names: Vec<String> = classes.iter().map(|(_, n)| n.clone()).collect();
    let mut open = true;
    egui::Window::new(text::ASSIGN_TITLE)
        .id(Id::new(("rules", RulesDialog::AssignNetClass)))
        .open(&mut open)
        .default_size([420.0, 400.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("assign_net_class_table")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong(text::ASSIGN_NET_NAME)
                            .on_hover_text(text::ASSIGN_NET_NAME_TIP);
                        ui.strong(text::ASSIGN_CLASS_NAME)
                            .on_hover_text(text::ASSIGN_CLASS_NAME_TIP);
                        ui.end_row();
                        for net_number in net_order(board) {
                            let Some(net) = board.rules.nets.get(net_number) else {
                                continue;
                            };
                            ui.label(&net.name);
                            let current = board.rules.net_classes.get(net.net_class()).name();
                            let id = Id::new(("assign_class", net_number));
                            if let Some(chosen) = name_combo(ui, id, current, &names)
                                && let Some((class, _)) = classes.iter().find(|(_, n)| *n == chosen)
                            {
                                out.push(Command::EditRules(RulesEdit::SetNetClass {
                                    net: net_number,
                                    class: *class,
                                }));
                            }
                            ui.end_row();
                        }
                    });
            });
        });
    state.open = open;
    out
}

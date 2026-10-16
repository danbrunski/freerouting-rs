//! The toolbar under the menu: the overlay toggles and Delete All.

use eframe::egui;

use crate::commands::Command;
use crate::overlays::OverlayState;
use crate::text;

/// Draws the toolbar and returns what was clicked.
pub fn ui(ui: &mut egui::Ui, overlay: OverlayState, board_open: bool) -> Vec<Command> {
    let mut out = Vec::new();
    ui.horizontal(|ui| {
        ui.add_enabled_ui(board_open, |ui| {
            if ui
                .selectable_label(overlay.show_ratsnest, text::RATSNEST_TOGGLE)
                .on_hover_text(text::RATSNEST_TOOLTIP)
                .clicked()
            {
                out.push(Command::ToggleRatsnest);
            }
            if ui
                .selectable_label(overlay.show_violations, text::VIOLATIONS_TOGGLE)
                .on_hover_text(text::VIOLATIONS_TOOLTIP)
                .clicked()
            {
                out.push(Command::ToggleViolations);
            }
            ui.separator();
            if ui
                .button("Undo")
                .on_hover_text("Undo the last edit (U, Ctrl+Z)")
                .clicked()
            {
                out.push(Command::Undo);
            }
            if ui
                .button("Redo")
                .on_hover_text("Redo the last undone edit (B, Ctrl+Y)")
                .clicked()
            {
                out.push(Command::Redo);
            }
            ui.separator();
            if ui
                .button(text::DELETE_ALL_BUTTON)
                .on_hover_text(text::DELETE_ALL_TOOLTIP)
                .clicked()
            {
                out.push(Command::DeleteAll);
            }
        });
    });
    out
}

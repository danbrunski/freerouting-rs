//! The toolbar under the menu: the overlay toggles and Delete All.

use eframe::egui;

use freerouting::board::structure::Unit;

use crate::commands::Command;
use crate::overlays::OverlayState;
use crate::text;
use crate::tools::ToolMode;

/// The units of the selector, in Java's order (`BoardToolbar`).
pub const UNITS: [Unit; 4] = [Unit::Mil, Unit::Inch, Unit::Mm, Unit::Um];

/// Draws the toolbar and returns what was clicked.
pub fn ui(
    ui: &mut egui::Ui,
    overlay: OverlayState,
    board_open: bool,
    unit: Unit,
    tool: ToolMode,
    session: bool,
) -> Vec<Command> {
    let mut out = Vec::new();
    ui.horizontal(|ui| {
        ui.add_enabled_ui(board_open && !session, |ui| {
            // Java's menu states: inspect (S), route (R) and drag (D).
            for (candidate, label, tip) in [
                (ToolMode::Select, text::TOOL_SELECT, text::TOOL_SELECT_TIP),
                (ToolMode::Route, text::TOOL_ROUTE, text::TOOL_ROUTE_TIP),
                (ToolMode::Drag, text::TOOL_DRAG, text::TOOL_DRAG_TIP),
            ] {
                if ui
                    .selectable_label(tool == candidate, label)
                    .on_hover_text(tip)
                    .clicked()
                {
                    out.push(Command::SetTool(candidate));
                }
            }
            ui.separator();
        });
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
        // The unit of the status bar, the lists and the item info window
        // (Java's selector at the right end of the toolbar).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for candidate in UNITS.into_iter().rev() {
                if ui
                    .selectable_label(unit == candidate, text::unit_button(candidate))
                    .on_hover_text(text::unit_tooltip(candidate))
                    .clicked()
                {
                    out.push(Command::SetUnit(candidate));
                }
            }
            ui.label(text::UNIT);
        });
    });
    out
}

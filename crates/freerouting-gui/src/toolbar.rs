//! The toolbar under the menu: the overlay toggles and Delete All.

use eframe::egui;

use freerouting::board::structure::Unit;

use crate::chrome::{self, Tone, icon};
use crate::commands::Command;
use crate::overlays::OverlayState;
use crate::text;
use crate::tools::ToolMode;

/// The units of the selector, in Java's order (`BoardToolbar`).
pub const UNITS: [Unit; 4] = [Unit::Mil, Unit::Inch, Unit::Mm, Unit::Um];

/// The room on either side of a group's separator.
const GROUP_GAP: f32 = 14.0;

/// Draws the toolbar and returns what was clicked.
pub fn ui(
    ui: &mut egui::Ui,
    overlay: OverlayState,
    board_open: bool,
    unit: Unit,
    tool: ToolMode,
    session: bool,
    routing: bool,
) -> Vec<Command> {
    let mut out = Vec::new();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // G07: Java's autoroute button starts the autorouter; while it runs the
        // button stops it.
        if routing {
            if chrome::button(ui, icon::CANCEL, text::CANCEL_BUTTON, Tone::Stop)
                .on_hover_text(text::CANCEL_BUTTON_TOOLTIP)
                .clicked()
            {
                out.push(Command::StopRouting);
            }
        } else {
            let start = ui.add_enabled_ui(board_open && !session, |ui| {
                chrome::button(ui, icon::AUTOROUTE, text::AUTOROUTE_BUTTON, Tone::Go)
                    .on_hover_text(text::AUTOROUTE_BUTTON_TOOLTIP)
            });
            if start.inner.clicked() {
                out.push(Command::StartRouting);
            }
        }
        gap(ui);
        ui.add_enabled_ui(board_open && !session && !routing, |ui| {
            // Java's menu states: inspect (S), route (R) and drag (D).
            chrome::segmented(ui, |ui| {
                for (candidate, glyph, label, tip) in [
                    (
                        ToolMode::Select,
                        icon::INSPECT,
                        text::TOOL_SELECT,
                        text::TOOL_SELECT_TIP,
                    ),
                    (
                        ToolMode::Route,
                        icon::ROUTE,
                        text::TOOL_ROUTE,
                        text::TOOL_ROUTE_TIP,
                    ),
                    (
                        ToolMode::Drag,
                        icon::DRAG,
                        text::TOOL_DRAG,
                        text::TOOL_DRAG_TIP,
                    ),
                ] {
                    if chrome::segment(ui, Some(glyph), label, tool == candidate)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        out.push(Command::SetTool(candidate));
                    }
                }
            });
        });
        gap(ui);
        ui.add_enabled_ui(board_open && !routing, |ui| {
            if chrome::toggle(
                ui,
                icon::RATSNEST,
                text::RATSNEST_TOGGLE,
                overlay.show_ratsnest,
            )
            .on_hover_text(text::RATSNEST_TOOLTIP)
            .clicked()
            {
                out.push(Command::ToggleRatsnest);
            }
            if chrome::toggle(
                ui,
                icon::VIOLATIONS,
                text::VIOLATIONS_TOGGLE,
                overlay.show_violations,
            )
            .on_hover_text(text::VIOLATIONS_TOOLTIP)
            .clicked()
            {
                out.push(Command::ToggleViolations);
            }
            gap(ui);
            if chrome::button(ui, icon::UNDO, "Undo", Tone::Quiet)
                .on_hover_text("Undo the last edit (U, Ctrl+Z)")
                .clicked()
            {
                out.push(Command::Undo);
            }
            if chrome::button(ui, icon::REDO, "Redo", Tone::Quiet)
                .on_hover_text("Redo the last undone edit (B, Ctrl+Y)")
                .clicked()
            {
                out.push(Command::Redo);
            }
            gap(ui);
            if chrome::button(ui, icon::DELETE_ALL, text::DELETE_ALL_BUTTON, Tone::Danger)
                .on_hover_text(text::DELETE_ALL_TOOLTIP)
                .clicked()
            {
                out.push(Command::DeleteAll);
            }
        });
        // The unit of the status bar, the lists and the item info window
        // (Java's selector at the right end of the toolbar).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            chrome::segmented(ui, |ui| {
                for candidate in UNITS.into_iter().rev() {
                    if chrome::segment(ui, None, text::unit_button(candidate), unit == candidate)
                        .on_hover_text(text::unit_tooltip(candidate))
                        .clicked()
                    {
                        out.push(Command::SetUnit(candidate));
                    }
                }
            });
            ui.weak(text::UNIT);
        });
    });
    out
}

/// A thin line between two groups of buttons.
fn gap(ui: &mut egui::Ui) {
    ui.add(egui::Separator::default().vertical().spacing(GROUP_GAP));
}

//! The status bar: Java's `BoardPanelStatus` (G04.8).
//!
//! Left to right: the error and warning counters (a click opens the log), the
//! status line (an error, what is loading, the last notice, or what is under
//! the cursor), the active layer and the cursor position in the chosen unit
//! with its label.

use eframe::egui::{self, Align, Layout, RichText, Sense};
use freerouting::board::structure::Unit;

use crate::commands::{Command, WindowKind};
use crate::text;
use crate::viewport::Hover;

/// What the bar shows.
pub struct Status<'a> {
    pub errors: usize,
    pub warnings: usize,
    /// An error to show instead of the hover text.
    pub error: Option<&'a str>,
    /// A background load in progress ("Reading …").
    pub loading: Option<String>,
    pub notice: Option<&'a str>,
    pub hover: &'a Hover,
    /// The name of the active layer, if a board is open.
    pub layer: Option<&'a str>,
    pub unit: Unit,
}

/// The text of the error counter, `Errors: n`.
pub fn errors_text(n: usize) -> String {
    format!("{}: {n}", text::STATUS_ERRORS)
}

/// The text of the warning counter, `Warnings: n`.
pub fn warnings_text(n: usize) -> String {
    format!("{}: {n}", text::STATUS_WARNINGS)
}

/// The text of the active layer, `Current layer: name`.
pub fn layer_text(name: &str) -> String {
    format!("{} {name}", text::STATUS_CURRENT_LAYER)
}

/// The cursor position text, empty while the cursor is off the board.
pub fn position_text(hover: &Hover, unit: Unit) -> String {
    hover
        .cursor
        .map(|p| text::format_position(p.x, p.y, unit))
        .unwrap_or_default()
}

/// Draws the bar's content and returns what was clicked.
pub fn ui(ui: &mut egui::Ui, status: &Status<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    ui.horizontal(|ui| {
        let counter = |ui: &mut egui::Ui, label: String, alert: bool| {
            let mut text = RichText::new(label);
            if alert {
                text = text.color(ui.visuals().warn_fg_color);
            }
            ui.add(egui::Label::new(text).sense(Sense::click()))
                .on_hover_text(text::STATUS_COUNTER_TIP)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
        };
        let mut open_log = counter(ui, errors_text(status.errors), status.errors > 0);
        open_log |= counter(ui, warnings_text(status.warnings), false);
        if open_log {
            out.push(Command::ShowWindow(WindowKind::Log));
        }
        ui.separator();
        // The right-hand part first, so the line gets what is left.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(text::unit_label(status.unit));
            ui.label(RichText::new(position_text(status.hover, status.unit)).monospace());
            if let Some(layer) = status.layer {
                ui.label(layer_text(layer));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                match (status.error, &status.loading) {
                    (Some(e), _) => drop(ui.colored_label(ui.visuals().error_fg_color, e)),
                    (None, Some(loading)) => {
                        ui.spinner();
                        ui.label(loading);
                    }
                    _ => {
                        if let Some(notice) = status.notice {
                            ui.label(notice);
                            ui.separator();
                        }
                        ui.label(status.hover.description.clone().unwrap_or_default());
                    }
                }
            });
        });
    });
    out
}

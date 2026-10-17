//! The routing summary window (G07.6): Java's `WindowRoutingSummary`, shown
//! when a job ends. The figures are those of the returned board
//! (`BoardStatistics`), the score and the duration those of the run.
//!
//! The sponsor message and button are dropped, with the other telemetry and
//! sponsor dialogs (G00 question 2). "Show this summary after auto-routing
//! finishes" is kept between runs (`PersistedSettings::show_routing_summary`,
//! Java `guiSettings.showRoutingSummary`).

use eframe::egui;
use freerouting::board::structure::Unit;

use crate::commands::Command;
use crate::routing::RoutingSummaryData;
use crate::text;

/// The rows of the window as (label, value) text, in Java's order, with
/// lengths in `unit`.
pub fn rows(data: &RoutingSummaryData, unit: Unit) -> Vec<(&'static str, String)> {
    let unit_name = text::unit_label(unit);
    let in_unit = |mm: f64| Unit::scale(mm, Unit::Mm, unit);
    let incomplete = if data.incomplete == 0 {
        text::SUMMARY_ZERO_INCOMPLETES.to_string()
    } else {
        data.incomplete.to_string()
    };
    let violations = if data.violations > 0 {
        format!(
            "{} (max: {} {unit_name})",
            data.violations,
            text::format_fixed(in_unit(data.max_violation_mm), 2)
        )
    } else {
        "0".to_string()
    };
    vec![
        (text::SUMMARY_NETS_TOTAL, data.total_nets.to_string()),
        (text::SUMMARY_CONNECTIONS_INCOMPLETE, incomplete),
        (text::SUMMARY_CLEARANCE_VIOLATIONS, violations),
        (text::SUMMARY_VIAS_ADDED, data.vias.to_string()),
        (
            text::SUMMARY_TRACE_LENGTH,
            format!(
                "{} {unit_name}",
                text::format_fixed(in_unit(data.trace_length_mm), 2)
            ),
        ),
        (
            text::SUMMARY_SCORE,
            format!("{:.2} / 1000 (1000 = perfect)", data.score),
        ),
        (
            text::SUMMARY_EXECUTION_TIME,
            format!("{:.2} s", data.duration_seconds),
        ),
    ]
}

/// The window body. Returns the commands of the checkbox and the button.
pub fn ui(
    ui: &mut egui::Ui,
    data: &RoutingSummaryData,
    unit: Unit,
    show_after_routing: bool,
) -> Vec<Command> {
    let mut out = Vec::new();
    let header = if data.interrupted {
        text::SUMMARY_HEADER_INTERRUPTED
    } else {
        text::SUMMARY_HEADER_COMPLETED
    };
    ui.vertical_centered(|ui| ui.heading(header));
    ui.separator();
    egui::Grid::new("routing_summary").show(ui, |ui| {
        for (label, value) in rows(data, unit) {
            ui.strong(label);
            ui.label(value);
            ui.end_row();
        }
    });
    ui.separator();
    let mut show = show_after_routing;
    if ui
        .checkbox(&mut show, text::SUMMARY_SHOW_CHECKBOX)
        .changed()
    {
        out.push(Command::SetShowRoutingSummary(show));
    }
    if ui.button(text::SUMMARY_CLOSE).clicked() {
        out.push(Command::CloseRoutingSummary);
    }
    out
}

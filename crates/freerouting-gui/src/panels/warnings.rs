//! The warnings window: what the readers skipped, and the session import
//! summary when the import had errors.

use eframe::egui;

use crate::session::Session;

pub fn ui(ui: &mut egui::Ui, session: &Session) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        if let Some(s) = session.ses_summary.filter(|s| s.errors_encountered > 0) {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "Session import: {} wires and {} vias imported, {} errors \
                     (nets, wires or vias that could not be imported).",
                    s.wires_imported, s.vias_imported, s.errors_encountered
                ),
            );
        }
        for w in &session.warnings {
            ui.label(w);
        }
    });
}

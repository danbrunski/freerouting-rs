//! Size and counts of the open board.

use eframe::egui;

use crate::commands::Command;
use crate::scene::Kind;
use crate::session::Loaded;

pub fn ui(ui: &mut egui::Ui, b: &Loaded) -> Vec<Command> {
    let mut out = Vec::new();
    ui.heading("Board");
    let count = |k: Kind| b.scene.items.iter().filter(|i| i.kind == k).count();
    egui::Grid::new("stats").show(ui, |ui| {
        let size = b.scene.bounds;
        if !size.is_empty() {
            ui.label("Size");
            ui.label(format!("{:.1} × {:.1} mm", size.width(), size.height()));
            ui.end_row();
        }
        ui.label("Components");
        ui.label(b.scene.components.to_string());
        ui.end_row();
        ui.label("Nets");
        ui.label(b.scene.nets.len().to_string());
        ui.end_row();
        ui.label("Trace segments");
        ui.label(count(Kind::Trace).to_string());
        ui.end_row();
        ui.label("Triangles");
        ui.label(b.tess.triangle_count().to_string());
        ui.end_row();
    });
    let notes = b.session.warnings.len() + usize::from(import_errors(b) > 0);
    if notes > 0 {
        ui.add_space(4.0);
        if ui.link(format!("{notes} warnings")).clicked() {
            out.push(Command::ShowWarnings);
        }
    }
    out
}

/// Nets, wires or vias of the `.ses` that the engine could not import.
pub fn import_errors(b: &Loaded) -> usize {
    b.session.ses_summary.map_or(0, |s| s.errors_encountered)
}

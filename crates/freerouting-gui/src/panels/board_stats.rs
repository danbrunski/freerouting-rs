//! Size and counts of the open board.

use eframe::egui;

use crate::analysis::Analysis;
use crate::commands::{Command, ListKind};
use crate::scene::Kind;
use crate::session::{Loaded, Session};
use crate::text;

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

/// What the background analysis found, for the side panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Checks {
    /// `incomplete_count()`, once the ratsnest is calculated.
    pub incomplete: Option<usize>,
    /// Clearance violations, once the design rules check is done.
    pub clearance: Option<usize>,
}

impl Checks {
    pub fn of(session: &Session, analysis: &Analysis) -> Checks {
        Checks {
            incomplete: analysis.ratsnest(session).map(|r| r.incomplete_count),
            clearance: analysis.drc(session).map(|d| d.clearance.len()),
        }
    }
}

/// The incomplete connections and clearance violations of the board. The
/// counts open the matching Info lists.
pub fn checks(ui: &mut egui::Ui, checks: &Checks) -> Vec<Command> {
    let mut out = Vec::new();
    ui.heading("Checks");
    let line = |ui: &mut egui::Ui, label: &str, count: Option<usize>, list: ListKind| {
        let text = match count {
            Some(n) => format!("{label}: {n}"),
            None => format!("{label}: {}", text::DRC_RUNNING),
        };
        ui.link(text).clicked().then_some(Command::ShowList(list))
    };
    out.extend(line(
        ui,
        text::INCOMPLETE_COUNT,
        checks.incomplete,
        ListKind::Incompletes,
    ));
    out.extend(line(
        ui,
        "Clearance violations",
        checks.clearance,
        ListKind::ClearanceViolations,
    ));
    out
}

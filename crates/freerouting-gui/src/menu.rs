//! The menu bar.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use crate::commands::{Command, ListKind, WindowKind};
use crate::dialogs::RulesDialog;
use crate::session::Loaded;
use crate::text;

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn item(ui: &mut egui::Ui, out: &mut Vec<Command>, label: &str, command: Command) {
    if ui.button(label).clicked() {
        out.push(command);
        ui.close();
    }
}

/// Draws the menu bar and returns what was picked.
pub fn ui(ui: &mut egui::Ui, loaded: Option<&Loaded>, recent: &[PathBuf]) -> Vec<Command> {
    let mut out = Vec::new();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            item(ui, &mut out, "Open…  (Ctrl+O)", Command::OpenDialog);
            ui.menu_button("Open Recent", |ui| {
                if recent.is_empty() {
                    ui.weak("No recent files");
                }
                for path in recent {
                    // A file that is gone stays listed, but cannot be picked.
                    let exists = path.is_file();
                    let button = egui::Button::new(file_name(path));
                    let response = ui
                        .add_enabled(exists, button)
                        .on_hover_text(path.display().to_string());
                    if response.clicked() {
                        out.push(Command::Open(path.clone()));
                        ui.close();
                    }
                }
            });
            if ui
                .add_enabled(
                    loaded.is_some(),
                    egui::Button::new(format!("{}  (Ctrl+S)", text::SAVE_AS)),
                )
                .clicked()
            {
                out.push(Command::SaveAsDialog);
                ui.close();
            }
            if ui
                .add_enabled(loaded.is_some(), egui::Button::new("Reload"))
                .clicked()
            {
                out.push(Command::Reload);
                ui.close();
            }
            if ui
                .add_enabled(loaded.is_some(), egui::Button::new("Export DRC report…"))
                .clicked()
            {
                out.push(Command::ExportDrcDialog);
                ui.close();
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                out.push(Command::Quit);
            }
        });
        ui.menu_button("View", |ui| {
            item(ui, &mut out, "Fit board  (Home)", Command::FitBoard);
            item(ui, &mut out, "Zoom in  (Z)", Command::ZoomIn);
            item(ui, &mut out, "Zoom out  (O)", Command::ZoomOut);
            item(
                ui,
                &mut out,
                "Centre on cursor  (C)",
                Command::CenterOnCursor,
            );
            item(
                ui,
                &mut out,
                "Zoom to rectangle  (F)",
                Command::ZoomToRectangle,
            );
            item(
                ui,
                &mut out,
                "Crosshair cursor  (,)",
                Command::ToggleCrosshair,
            );
        });
        analysis_menus(ui, &mut out, loaded.is_some());
        rules_menus(ui, &mut out, loaded.is_some());
        display_menu(ui, &mut out, loaded.is_some());
        ui.menu_button(text::MENU_HELP, |ui| {
            item(
                ui,
                &mut out,
                text::MENU_ABOUT,
                Command::ShowWindow(WindowKind::About),
            );
        });
        if let Some(b) = loaded {
            ui.separator();
            ui.label(RichText::new(file_name(&b.session.paths.design)).strong());
            if let Some(session) = &b.session.paths.session {
                ui.label(format!("+ {}", file_name(session)));
            }
            // Exporters often put the full path of the source file here.
            if !b.scene.name.contains(['/', '\\']) {
                ui.weak(&b.scene.name);
            }
        }
    });
    out
}

/// The Info and Other menus of Java's board menu bar.
fn analysis_menus(ui: &mut egui::Ui, out: &mut Vec<Command>, board_open: bool) {
    ui.add_enabled_ui(board_open, |ui| {
        ui.menu_button(text::MENU_INFO, |ui| {
            for (label, kind) in [
                (text::MENU_INCOMPLETES, ListKind::Incompletes),
                (text::MENU_LENGTH_VIOLATIONS, ListKind::LengthViolations),
                (
                    text::MENU_CLEARANCE_VIOLATIONS,
                    ListKind::ClearanceViolations,
                ),
                (text::MENU_UNCONNECTED_ROUTE, ListKind::Unconnected),
                (text::MENU_ZONE_ISLANDS, ListKind::ZoneIslands),
            ] {
                item(ui, out, label, Command::ShowList(kind));
            }
            ui.separator();
            for (label, kind) in [
                (text::MENU_NETS, ListKind::Nets),
                (text::MENU_COMPONENTS, ListKind::Components),
                (text::MENU_PACKAGES, ListKind::Packages),
                (text::MENU_PADSTACKS, ListKind::Padstacks),
            ] {
                item(ui, out, label, Command::ShowList(kind));
            }
            ui.separator();
            item(
                ui,
                out,
                text::MENU_LOG,
                Command::ShowWindow(WindowKind::Log),
            );
            item(ui, out, text::MENU_RUN_DRC, Command::RunDrc);
        });
        ui.menu_button(text::MENU_OTHER, |ui| {
            if ui
                .button(text::DELETE_ALL)
                .on_hover_text(text::DELETE_ALL_TOOLTIP)
                .clicked()
            {
                out.push(Command::DeleteAll);
                ui.close();
            }
        });
    });
}

/// The Display menu of Java's board menu bar (`BoardMenuDisplay`).
fn display_menu(ui: &mut egui::Ui, out: &mut Vec<Command>, board_open: bool) {
    ui.add_enabled_ui(board_open, |ui| {
        ui.menu_button(text::MENU_DISPLAY, |ui| {
            for (label, window) in [
                (text::MENU_VISIBILITY, WindowKind::Visibility),
                (text::MENU_COLORS, WindowKind::Colors),
                (text::MENU_MISC, WindowKind::DisplayMisc),
            ] {
                item(ui, out, label, Command::ShowWindow(window));
            }
        });
    });
}

/// The Rules and Settings menus of Java's board menu bar (`BoardMenuRules`,
/// `BoardMenuParameter`): the rules dialogs of G06.
fn rules_menus(ui: &mut egui::Ui, out: &mut Vec<Command>, board_open: bool) {
    ui.add_enabled_ui(board_open, |ui| {
        ui.menu_button(text::MENU_RULES, |ui| {
            for (label, dialog) in [
                (text::MENU_CLEARANCE_MATRIX, RulesDialog::ClearanceMatrix),
                (text::MENU_VIAS, RulesDialog::ViaRules),
                (text::MENU_NET_CLASSES, RulesDialog::NetClasses),
            ] {
                item(ui, out, label, Command::ShowRulesDialog(dialog));
            }
        });
        ui.menu_button(text::MENU_PARAMETER, |ui| {
            item(
                ui,
                out,
                text::MENU_ROUTING,
                Command::ShowRulesDialog(RulesDialog::RouteSettings),
            );
            // G09.6: the placement grid of a move.
            item(
                ui,
                out,
                text::MENU_MOVE_SETTINGS,
                Command::ShowRulesDialog(RulesDialog::MoveSettings),
            );
            // G07.1: the autorouter's settings.
            item(
                ui,
                out,
                text::MENU_AUTOROUTER,
                Command::ShowAutorouteSettings,
            );
            // G05.2: the selection's "General Settings".
            item(
                ui,
                out,
                text::MENU_SELECT_SETTINGS,
                Command::ShowSelectionSettings,
            );
        });
    });
}

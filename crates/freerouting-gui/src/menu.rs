//! The menu bar.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};

use crate::chrome::{self, icon};
use crate::commands::{Command, ListKind, WindowKind};
use crate::dialogs::RulesDialog;
use crate::session::Loaded;
use crate::text;

/// The width a menu opens with, so its rows have room for their shortcuts.
const MENU_WIDTH: f32 = 200.0;
/// The height of the app's icon at the left of the bar.
const LOGO_SIDE: f32 = 18.0;

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

/// A File or View entry with an icon and a shortcut: `row` is (icon, label,
/// shortcut), a row of the View menu's table.
fn entry(
    ui: &mut egui::Ui,
    out: &mut Vec<Command>,
    row: (&str, &str, Option<&str>),
    enabled: bool,
    command: Command,
) {
    let (glyph, label, shortcut) = row;
    if chrome::menu_item(ui, Some(glyph), label, shortcut, enabled).clicked() {
        out.push(command);
        ui.close();
    }
}

/// Draws the menu bar and returns what was picked.
pub fn ui(ui: &mut egui::Ui, loaded: Option<&Loaded>, recent: &[PathBuf]) -> Vec<Command> {
    let mut out = Vec::new();
    let config = egui::containers::menu::MenuConfig::new().style(chrome::menu_style);
    egui::MenuBar::new()
        .style(chrome::bar_style)
        .config(config)
        .ui(ui, |ui| {
            if let Some(logo) = chrome::logo(ui.ctx()) {
                ui.add(egui::Image::new(&logo).fit_to_exact_size(egui::Vec2::splat(LOGO_SIDE)));
                ui.add_space(2.0);
            }
            file_menu(ui, &mut out, loaded.is_some(), recent);
            view_menu(ui, &mut out);
            analysis_menus(ui, &mut out, loaded.is_some());
            rules_menus(ui, &mut out, loaded.is_some());
            display_menu(ui, &mut out, loaded.is_some());
            ui.menu_button(text::MENU_HELP, |ui| {
                ui.set_min_width(MENU_WIDTH);
                entry(
                    ui,
                    &mut out,
                    (icon::ABOUT, text::MENU_ABOUT, None),
                    true,
                    Command::ShowWindow(WindowKind::About),
                );
            });
            if let Some(b) = loaded {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // In a narrow window the chip would grow left over the
                    // menus; it is cut off at their end instead.
                    ui.shrink_clip_rect(ui.max_rect());
                    board_chip(ui, b);
                });
            }
        });
    out
}

/// The open design (and session) at the right end of the bar.
fn board_chip(ui: &mut egui::Ui, b: &Loaded) {
    // Right to left: the last text first.
    chrome::chip(ui, |ui| {
        // Exporters often put the full path of the source file here.
        if !b.scene.name.contains(['/', '\\']) {
            ui.weak(&b.scene.name);
        }
        if let Some(session) = &b.session.paths.session {
            ui.label(format!("+ {}", file_name(session)));
        }
        ui.label(RichText::new(file_name(&b.session.paths.design)).strong());
    });
}

fn file_menu(ui: &mut egui::Ui, out: &mut Vec<Command>, board_open: bool, recent: &[PathBuf]) {
    ui.menu_button("File", |ui| {
        ui.set_min_width(MENU_WIDTH);
        let open = ui
            .ctx()
            .format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O));
        let save = ui
            .ctx()
            .format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S));
        entry(
            ui,
            out,
            (icon::OPEN, "Open…", Some(&open)),
            true,
            Command::OpenDialog,
        );
        let recent_menu = ui.menu_button(chrome::menu_title("Open Recent"), |ui| {
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
        let open = recent_menu.inner.is_some();
        chrome::paint_menu_icon(ui, &recent_menu.response, icon::RECENT, open);
        ui.separator();
        entry(
            ui,
            out,
            (icon::SAVE, text::SAVE_AS, Some(&save)),
            board_open,
            Command::SaveAsDialog,
        );
        entry(
            ui,
            out,
            (icon::RELOAD, "Reload", None),
            board_open,
            Command::Reload,
        );
        entry(
            ui,
            out,
            (icon::EXPORT, "Export DRC report…", None),
            board_open,
            Command::ExportDrcDialog,
        );
        ui.separator();
        if chrome::menu_item(ui, Some(icon::QUIT), "Quit", None, true).clicked() {
            out.push(Command::Quit);
        }
    });
}

fn view_menu(ui: &mut egui::Ui, out: &mut Vec<Command>) {
    ui.menu_button("View", |ui| {
        ui.set_min_width(MENU_WIDTH);
        for (row, command) in [
            ((icon::FIT, "Fit board", Some("Home")), Command::FitBoard),
            ((icon::ZOOM_IN, "Zoom in", Some("Z")), Command::ZoomIn),
            ((icon::ZOOM_OUT, "Zoom out", Some("O")), Command::ZoomOut),
            (
                (icon::CENTRE, "Centre on cursor", Some("C")),
                Command::CenterOnCursor,
            ),
            (
                (icon::ZOOM_REGION, "Zoom to rectangle", Some("F")),
                Command::ZoomToRectangle,
            ),
            (
                (icon::CROSSHAIR, "Crosshair cursor", Some(",")),
                Command::ToggleCrosshair,
            ),
        ] {
            entry(ui, out, row, true, command);
        }
    });
}

/// The Info and Other menus of Java's board menu bar.
fn analysis_menus(ui: &mut egui::Ui, out: &mut Vec<Command>, board_open: bool) {
    ui.add_enabled_ui(board_open, |ui| {
        ui.menu_button(text::MENU_INFO, |ui| {
            ui.set_min_width(MENU_WIDTH);
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
            ui.set_min_width(MENU_WIDTH);
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
            ui.set_min_width(MENU_WIDTH);
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
            ui.set_min_width(MENU_WIDTH);
            for (label, dialog) in [
                (text::MENU_CLEARANCE_MATRIX, RulesDialog::ClearanceMatrix),
                (text::MENU_VIAS, RulesDialog::ViaRules),
                (text::MENU_NET_CLASSES, RulesDialog::NetClasses),
            ] {
                item(ui, out, label, Command::ShowRulesDialog(dialog));
            }
        });
        ui.menu_button(text::MENU_PARAMETER, |ui| {
            ui.set_min_width(MENU_WIDTH);
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

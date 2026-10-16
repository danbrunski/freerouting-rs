//! The rules dialogs (G06): net classes, assign net class, the clearance
//! matrix, vias, via padstacks and via rules, and the board-rule part of the
//! routing settings.
//!
//! **Dialogs are thin.** A dialog reads values from the board
//! ([`RuleView`]), shows them, and returns one [`Command::EditRules`] per
//! user change. [`App::execute_rules_edit`] runs it through B08's facade
//! ([`rules_edit::apply`]) on the session board, counts the edit, and the
//! session's analysis tag `(revision, edit_generation)` makes the DRC and the
//! ratsnest run again. Rule edits don't change `BasicBoard::revision()`; the
//! edit generation is what discards old results.
//!
//! Values are shown in the display unit (G04.6) and converted with
//! [`input::RuleUnits`]. Java's texts and refusals are kept (`text.rs`); a
//! refusal goes to the status line.
//!
//! | Module | Java class |
//! |---|---|
//! | [`net_classes`] | `WindowNetClasses` |
//! | [`assign_net_class`] | `WindowAssignNetClass` |
//! | [`clearance_matrix`] | `WindowClearanceMatrix` |
//! | [`vias`] | `WindowEditVias` |
//! | [`via_rules`] | `WindowVia`, `WindowViaRule` |
//! | [`route_settings`] | `WindowRouteParameter`, the board-rule part |
//! | [`autoroute_handlers`] | the static helpers of `WindowAutorouteParameter` |
//! | [`input`] | the static parse helpers of those windows |

pub mod assign_net_class;
pub mod autoroute_handlers;
pub mod clearance_matrix;
pub mod input;
pub mod net_classes;
pub mod route_settings;
pub mod rules_edit;
pub mod via_rules;
pub mod vias;

use std::collections::BTreeSet;

use eframe::egui;
use freerouting::board::BasicBoard;
use freerouting::board::structure::Unit;

use crate::app::App;
use crate::commands::Command;
use crate::panels::log::Level;
use crate::session::Session;

pub use input::{Input, RuleUnits};
pub use rules_edit::{EditOutcome, RulesEdit};

/// A rules dialog that can be opened from the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RulesDialog {
    /// `WindowNetClasses` (Rules > Net Classes).
    NetClasses,
    /// `WindowAssignNetClass` (the Net Classes window's Assign button).
    AssignNetClass,
    /// `WindowClearanceMatrix` (Rules > Clearance Matrix).
    ClearanceMatrix,
    /// `WindowVia` (Rules > Vias): padstacks, vias and via rules.
    ViaRules,
    /// `WindowEditVias` (the Via Rules window's Edit button for vias).
    EditVias,
    /// `WindowRouteParameter` (Settings > Routing).
    RouteSettings,
}

/// What a dialog reads of the open design.
pub struct RuleView<'a> {
    pub board: &'a BasicBoard,
    pub units: RuleUnits,
}

impl<'a> RuleView<'a> {
    /// The view of a session, with lengths in `display`.
    pub fn new(session: &'a Session, display: Unit) -> Self {
        let board = session.board.basic();
        let comm = &board.communication;
        RuleView {
            board,
            units: RuleUnits {
                transform: comm.coordinate_transform,
                design: comm.unit,
                display,
            },
        }
    }

    /// The names of the clearance classes, in matrix order.
    pub fn clearance_class_names(&self) -> Vec<String> {
        let matrix = &self.board.rules.clearance_matrix;
        (0..matrix.class_count() as i32)
            .map(|i| matrix.get_name(i).unwrap_or_default().to_string())
            .collect()
    }
}

/// The state of every rules dialog: open or closed, and what the user has
/// chosen in it. The values themselves always come from the board.
#[derive(Default)]
pub struct Dialogs {
    pub net_classes: net_classes::State,
    pub assign: assign_net_class::State,
    pub clearance: clearance_matrix::State,
    pub vias: vias::State,
    pub via_rules: via_rules::State,
    pub route: route_settings::State,
}

impl Dialogs {
    pub fn open(&mut self, dialog: RulesDialog, open: bool) {
        match dialog {
            RulesDialog::NetClasses => self.net_classes.open = open,
            RulesDialog::AssignNetClass => self.assign.open = open,
            RulesDialog::ClearanceMatrix => self.clearance.open = open,
            RulesDialog::ViaRules => self.via_rules.open = open,
            RulesDialog::EditVias => self.vias.open = open,
            RulesDialog::RouteSettings => self.route.open = open,
        }
    }

    pub fn is_open(&self, dialog: RulesDialog) -> bool {
        match dialog {
            RulesDialog::NetClasses => self.net_classes.open,
            RulesDialog::AssignNetClass => self.assign.open,
            RulesDialog::ClearanceMatrix => self.clearance.open,
            RulesDialog::ViaRules => self.via_rules.open,
            RulesDialog::EditVias => self.vias.open,
            RulesDialog::RouteSettings => self.route.open,
        }
    }

    /// Closes every dialog and forgets what was chosen (a new board).
    pub fn reset(&mut self) {
        *self = Dialogs::default();
    }

    /// The nets whose airlines the Net Classes window's "Filter Incomplete
    /// Connections" leaves visible, if it was used.
    pub fn incompletes_filter(&self) -> Option<&BTreeSet<i32>> {
        self.net_classes.incompletes_filter.as_ref()
    }

    /// Draws the open dialogs and returns what the user did.
    pub fn show(&mut self, ctx: &egui::Context, view: &RuleView<'_>) -> Vec<Command> {
        let mut out = Vec::new();
        out.extend(net_classes::show(ctx, &mut self.net_classes, view));
        out.extend(assign_net_class::show(ctx, &mut self.assign, view));
        out.extend(clearance_matrix::show(ctx, &mut self.clearance, view));
        out.extend(vias::show(ctx, &mut self.vias, view));
        out.extend(via_rules::show(ctx, &mut self.via_rules, view));
        out.extend(route_settings::show(ctx, &mut self.route, view));
        out
    }
}

impl App {
    /// Runs one rules edit on the session board (`Command::EditRules`).
    ///
    /// The edit counts as a session edit when it changed something, which
    /// starts the DRC and the ratsnest again. An edit that changes items
    /// builds the scene again. No edit takes a board snapshot: the board
    /// history (`generate_snapshot`, `undo`) holds items and components only,
    /// the rules are not in it, and the items these edits change are changed in
    /// place without a saved version, so an empty undo level would only swallow
    /// a press of U. A refusal or a status text goes to the status line and the
    /// log.
    pub(crate) fn execute_rules_edit(&mut self, edit: RulesEdit) {
        let Some(b) = self.loaded.as_mut() else {
            return;
        };
        let touches_items = edit.touches_items();
        let mut outcome = EditOutcome::default();
        b.session.edit(|board| {
            outcome = rules_edit::apply(board, &edit);
            outcome.changed
        });
        if outcome.changed && touches_items {
            b.rebuild_scene();
            self.selection.retain_existing(b.session.board.basic());
            self.stats.full_rebuilds += 1;
        }
        if let Some(message) = outcome.message {
            self.log.push(Level::Info, message.clone());
            self.notice = Some(message);
        }
    }
}

// ---------------------------------------------------------------------------
// Widgets shared by the dialogs
// ---------------------------------------------------------------------------

/// A text field for a table cell. It shows `shown` until the user edits it,
/// keeps the typed text while it has focus and returns the text when the
/// focus leaves (Enter or a click elsewhere) and the text changed: the way
/// Java's tables end an edit (`terminateEditOnFocusLost`).
pub(crate) fn text_cell(
    ui: &mut egui::Ui,
    id: egui::Id,
    shown: &str,
    width: f32,
) -> Option<String> {
    let focused = ui.memory(|m| m.has_focus(id));
    let mut buffer: String = if focused {
        ui.data(|d| d.get_temp(id))
            .unwrap_or_else(|| shown.to_string())
    } else {
        shown.to_string()
    };
    let response = ui.add(
        egui::TextEdit::singleline(&mut buffer)
            .id(id)
            .desired_width(width),
    );
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buffer.clone()));
    }
    if response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        return (buffer != shown).then_some(buffer);
    }
    None
}

/// A drop-down list of names; returns the newly chosen name.
pub(crate) fn name_combo(
    ui: &mut egui::Ui,
    id: egui::Id,
    current: &str,
    names: &[String],
) -> Option<String> {
    let mut chosen = None;
    let response = egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .show_ui(ui, |ui| {
            for name in names {
                if ui.selectable_label(name == current, name).clicked() && name != current {
                    chosen = Some(name.clone());
                }
            }
        });
    name_drop_down(ui, &response.response, current);
    chosen
}

/// Makes the chosen text the accessible name of a drop-down (screen readers,
/// and the tests that find it).
pub(crate) fn name_drop_down(ui: &egui::Ui, response: &egui::Response, current: &str) {
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), current)
    });
}

/// A modal-looking question with two buttons; returns `Some(true)` for the
/// first button and `Some(false)` for the second.
pub(crate) fn confirm_window(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    question: &str,
    yes: &str,
    no: &str,
) -> Option<bool> {
    let mut answer = None;
    egui::Window::new(title)
        .id(egui::Id::new(("confirm", id)))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(question);
            ui.horizontal(|ui| {
                if ui.button(yes).clicked() {
                    answer = Some(true);
                }
                if ui.button(no).clicked() {
                    answer = Some(false);
                }
            });
        });
    answer
}

/// A window asking for one line of text with OK and Cancel; returns
/// `Some(Some(text))` for OK, `Some(None)` for Cancel. `buffer` keeps the
/// text between frames.
pub(crate) fn prompt_window(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    question: &str,
    buffer: &mut String,
) -> Option<Option<String>> {
    let mut answer = None;
    egui::Window::new(title)
        .id(egui::Id::new(("prompt", id)))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(question);
            let response = ui.add(egui::TextEdit::singleline(buffer).desired_width(220.0));
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                if ui.button(crate::text::OK).clicked() || enter {
                    answer = Some(Some(buffer.clone()));
                }
                if ui.button(crate::text::CANCEL).clicked() {
                    answer = Some(None);
                }
            });
        });
    answer
}

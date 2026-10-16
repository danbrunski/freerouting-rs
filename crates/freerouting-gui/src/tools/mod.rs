//! The tools of the canvas (G05): inspect mode with its selection
//! operations, safe edits, context menus and keepout construction.
//!
//! [`Tools`] is the state of the left button's tool and of the windows that
//! belong to it. Every action is a [`Command`] run by
//! [`App::execute_tool`]; the canvas turns pointer events into commands
//! (`viewport.rs`), so a test can drive the tools with raw events or with
//! commands.
//!
//! Java's states map like this: the default state is `InspectMenuState`
//! (nothing selected), a non-empty [`Selection`] is `InspectedItemState`,
//! and `keepout` is the construction states. History (undo and redo) is
//! refused while a keepout is under construction, as Java allows it in a
//! menu state only.

pub mod edit;
pub mod inspect;
pub mod keepout;
pub mod popup;

use std::collections::BTreeSet;

use eframe::egui;
use freerouting::drc::ClearanceViolation;
use freerouting::geometry::FloatPoint;

use crate::analysis::Drc;
use crate::app::App;
use crate::commands::{Command, KeepoutKind};
use crate::overlays::{Frame, construction, violations};
use crate::panels::item_info::InfoWindows;
use crate::panels::select_settings::{self, SelectionSettings};
use crate::scene::P;
use crate::session::SceneSync;
use crate::text;
use crate::view::Camera;

/// The state of the tools.
#[derive(Default)]
pub struct Tools {
    /// What a click may select (G05.2).
    pub select: SelectionSettings,
    /// The keepout being constructed, if any.
    pub keepout: Option<keepout::KeepoutTool>,
    /// The open context menu.
    pub popup: Option<popup::Popup>,
    /// The clearance violations of the selected items while they are shown
    /// (Java `InspectedItemState.clearanceViolations`).
    pub violations: Option<Vec<ClearanceViolation>>,
    /// The "General Settings" window is open.
    pub settings_open: bool,
    /// The item info windows.
    pub info: InfoWindows,
    /// How the last edit, undo or redo brought the drawing up to date.
    pub last_sync: SceneSync,
    /// The nets the last undo or redo changed (the ratsnest of these is
    /// what Java updates; here the background analysis runs in full).
    pub changed_nets: BTreeSet<i32>,
}

impl Tools {
    /// Forgets what belongs to a selection that has ended.
    fn drop_selection_state(&mut self) {
        self.violations = None;
    }
}

impl App {
    /// The tools' state.
    pub fn tools(&self) -> &Tools {
        &self.tools
    }

    /// The state for tests that set the selection settings directly.
    pub fn tools_mut(&mut self) -> &mut Tools {
        &mut self.tools
    }

    fn pick_layers(&self) -> (usize, Vec<bool>) {
        let levels = self.view.look.layer_levels();
        let visible = self
            .view
            .layer_visible
            .iter()
            .enumerate()
            .map(|(i, on)| *on && levels.get(i).copied().unwrap_or(0) > 0)
            .collect();
        (self.view.active_layer, visible)
    }

    /// Runs the G05 commands. The caller passes only those.
    pub(crate) fn execute_tool(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::CanvasClick { at, toggle } => self.canvas_click(at, toggle),
            Command::SelectRegion { from, to } => self.select_region(from, to),
            Command::SelectAt(at) => self.canvas_click(at, false),
            Command::ExtendSelection(kind) => self.extend_selection(kind),
            Command::ToggleSelectionViolations => self.toggle_selection_violations(),
            Command::ShowItemInfo(ids) => {
                if let Some(b) = &self.loaded
                    && !ids.is_empty()
                {
                    self.tools
                        .info
                        .open(b.session.board.basic(), &ids, self.view.unit);
                }
            }
            Command::ShowSelectionSettings => self.tools.settings_open = true,
            Command::SetSelectionLayers(layers) => self.tools.select.layers = layers,
            Command::SetSelectable(choice, on) => self.tools.select.set(choice, on),
            Command::DeleteSelection => self.edit_selection(Edit::Delete),
            Command::FixSelection => self.edit_selection(Edit::Fix),
            Command::UnfixSelection => self.edit_selection(Edit::Unfix),
            Command::OpenPopup { at } => self.open_popup(at),
            Command::ClosePopup => self.tools.popup = None,
            Command::CenterAt(at) => {
                if let Some(camera) = self.view.camera.as_mut() {
                    camera.center = at;
                    self.view.auto_fit = None;
                }
            }
            Command::ZoomInAt(at) => self.zoom_at(at, crate::viewport::KEY_ZOOM),
            Command::ZoomOutAt(at) => self.zoom_at(at, 1.0 / crate::viewport::KEY_ZOOM),
            Command::StartKeepout { kind, at } => self.start_keepout(kind, at),
            Command::Complete => self.complete_keepout(None),
            _ => {}
        }
        let _ = ctx;
    }

    fn zoom_at(&mut self, at: P, factor: f64) {
        let canvas = self.view.canvas;
        if let Some(camera) = self.view.camera.as_mut() {
            let anchor = camera.to_screen(canvas, at);
            camera.zoom_at(canvas, anchor, factor);
            self.view.auto_fit = None;
        }
    }

    /// Escape: closes the menu, else cancels the keepout, else a pending
    /// zoom rectangle, else clears the selection (Java `cancel`).
    pub(crate) fn cancel(&mut self) {
        if self.tools.popup.take().is_some() {
            return;
        }
        if self.tools.keepout.take().is_some() {
            self.notice = None;
            return;
        }
        if self.view.zoom_rect_armed {
            self.view.zoom_rect_armed = false;
            return;
        }
        if !self.selection.is_empty() {
            self.selection.clear();
            self.tools.drop_selection_state();
            self.notice = Some(text::IN_INSPECT_MENU.to_string());
        }
    }

    // --- picking and selecting ---------------------------------------------

    fn canvas_click(&mut self, at: P, toggle: bool) {
        self.tools.popup = None;
        let Some(b) = &self.loaded else { return };
        let point = inspect::board_point(b.session.board.basic(), at);
        if self.tools.keepout.is_some() {
            self.keepout_click(point);
            return;
        }
        let board = b.session.board.basic();
        let (current, visible) = self.pick_layers();
        let layers = inspect::PickLayers {
            current,
            visible: &visible,
        };
        let picked = inspect::pick(board, point.round(), &self.tools.select, &layers);
        match inspect::click(&self.selection.items, &picked, toggle) {
            inspect::ClickOutcome::Selected(items) => {
                let was_empty = self.selection.is_empty();
                self.selection.set(items);
                if was_empty {
                    self.notice = Some(text::IN_INSPECT_MODE.to_string());
                }
            }
            inspect::ClickOutcome::Ended => {
                self.selection.clear();
                self.tools.drop_selection_state();
                self.notice = Some(text::IN_INSPECT_MENU.to_string());
            }
            inspect::ClickOutcome::Unchanged => {}
        }
    }

    fn select_region(&mut self, from: P, to: P) {
        if self.tools.keepout.is_some() {
            return;
        }
        let Some(b) = &self.loaded else { return };
        let board = b.session.board.basic();
        let (a, c) = (
            inspect::board_point(board, from).round(),
            inspect::board_point(board, to).round(),
        );
        let (current, visible) = self.pick_layers();
        let layers = inspect::PickLayers {
            current,
            visible: &visible,
        };
        let found = inspect::region(board, a, c, &self.tools.select, &layers);
        if found.is_empty() {
            self.notice = Some(text::NOTHING_SELECTED.to_string());
        } else {
            let mut items = self.selection.items.clone();
            items.extend(found);
            self.selection.set(items);
            self.notice = Some(text::IN_INSPECT_MODE.to_string());
        }
    }

    fn extend_selection(&mut self, kind: crate::commands::ExtendKind) {
        let Some(b) = &self.loaded else { return };
        if self.selection.is_empty() {
            return;
        }
        let board = b.session.board.basic();
        let extended = inspect::extend(
            board,
            &self.selection.items,
            kind,
            &self.tools.select.filter(),
        );
        if extended.is_empty() {
            self.selection.clear();
            self.tools.drop_selection_state();
            self.notice = Some(text::IN_INSPECT_MENU.to_string());
        } else {
            self.selection.set(extended);
        }
    }

    fn toggle_selection_violations(&mut self) {
        if self.tools.violations.take().is_some() {
            self.notice = None;
            return;
        }
        let Some(b) = &self.loaded else { return };
        if self.selection.is_empty() {
            return;
        }
        let found = inspect::selection_violations(b.session.board.basic(), &self.selection.items);
        self.notice = Some(format!(
            "{} {}",
            found.len(),
            text::CLEARANCE_VIOLATIONS_FOUND
        ));
        self.tools.violations = Some(found);
    }

    // --- edits ---------------------------------------------------------------

    fn edit_selection(&mut self, edit: Edit) {
        let Some(b) = self.loaded.as_mut() else {
            return;
        };
        if self.selection.is_empty() {
            return;
        }
        let ids = self.selection.items.clone();
        let mut outcome = edit::EditOutcome::default();
        b.session.edit(|board| {
            outcome = match edit {
                Edit::Delete => edit::delete(board, &ids),
                Edit::Fix => edit::fix(board, &ids),
                Edit::Unfix => edit::unfix(board, &ids),
            };
            outcome.edited()
        });
        if outcome.edited() {
            self.after_board_edit();
        }
        if outcome.refused > 0 {
            self.notice = Some(text::SOME_ITEMS_FIXED.to_string());
        }
    }

    // --- context menus -------------------------------------------------------

    fn open_popup(&mut self, at: P) {
        if self.loaded.is_none() {
            return;
        }
        let screen = self
            .view
            .cursor
            .unwrap_or_else(|| self.view.canvas.center());
        self.tools.popup = Some(popup::Popup {
            at,
            screen,
            keepout_open: false,
        });
    }

    fn popup_mode(&self) -> popup::Mode {
        if let Some(tool) = &self.tools.keepout {
            popup::Mode::Construction {
                circle: tool.kind == KeepoutKind::Circle,
            }
        } else if !self.selection.is_empty() {
            popup::Mode::Selection
        } else {
            let swap = self
                .loaded
                .as_ref()
                .is_some_and(|b| b.session.board.basic().library.logical_parts.count() > 0);
            popup::Mode::Main { swap }
        }
    }

    // --- keepout construction ------------------------------------------------

    fn start_keepout(&mut self, kind: KeepoutKind, at: P) {
        self.tools.popup = None;
        let Some(b) = &self.loaded else { return };
        let point = inspect::board_point(b.session.board.basic(), at);
        if kind == KeepoutKind::Hole {
            let layer = self.view.active_layer as i32;
            match keepout::KeepoutTool::start_hole(&b.session.board, layer, point) {
                Ok(tool) => {
                    self.tools.keepout = Some(tool);
                    self.notice = Some(text::ADDING_HOLE.to_string());
                }
                Err(key) => self.notice = Some(text::hole_status(key).to_string()),
            }
            return;
        }
        self.tools.keepout = Some(keepout::KeepoutTool::start(kind, &b.session.board, point));
        self.notice = Some(
            match kind {
                KeepoutKind::Tile => text::CREATING_TILE,
                KeepoutKind::Circle => text::CREATING_CIRCLE,
                _ => text::CREATING_POLYGON,
            }
            .to_string(),
        );
    }

    fn keepout_click(&mut self, point: FloatPoint) {
        let Some(tool) = self.tools.keepout.as_mut() else {
            return;
        };
        let Some(b) = &self.loaded else { return };
        if tool.kind == KeepoutKind::Circle {
            // The click that sets the radius ends a circle.
            self.complete_keepout(Some(point));
        } else {
            tool.add_corner(&b.session.board, point);
        }
    }

    /// Completes the keepout under construction. A circle takes its radius
    /// from `radius_point`, or from the cursor if there is none.
    fn complete_keepout(&mut self, radius_point: Option<FloatPoint>) {
        let Some(mut tool) = self.tools.keepout.take() else {
            return;
        };
        let cursor = self.view.cursor;
        let camera = self.view.camera;
        let canvas = self.view.canvas;
        let layer = self.view.active_layer as i32;
        let Some(b) = self.loaded.as_mut() else {
            return;
        };
        let radius_point = radius_point.or_else(|| {
            let mm = camera.zip(cursor).map(|(c, p)| c.to_board(canvas, p))?;
            Some(inspect::board_point(b.session.board.basic(), mm))
        });
        let hole = tool.kind == KeepoutKind::Hole;
        let mut outcome = keepout::Outcome::Refused;
        b.session.edit(|board| {
            outcome = keepout::complete(&mut tool, board, layer, radius_point);
            matches!(outcome, keepout::Outcome::Inserted(_))
        });
        match outcome {
            keepout::Outcome::Inserted(_) => {
                self.after_board_edit();
                let done = if hole {
                    text::HOLE_DONE
                } else {
                    text::KEEPOUT_DONE
                };
                self.notice = Some(done.to_string());
            }
            keepout::Outcome::Refused => {
                let refused = if hole {
                    text::HOLE_FAILED
                } else {
                    text::KEEPOUT_OVERLAPS
                };
                self.notice = Some(refused.to_string());
            }
        }
    }

    // --- drawing and windows ---------------------------------------------------

    /// The windows and menus of the tools; returns what the user did.
    pub(crate) fn tools_ui(&mut self, ctx: &egui::Context) -> Vec<Command> {
        let mut out = Vec::new();
        if self.loaded.is_none() {
            return out;
        }
        self.tools.info.show(ctx);
        if self.tools.settings_open {
            let layer = self
                .loaded
                .as_ref()
                .and_then(|b| b.scene.layers.get(self.view.active_layer))
                .map(|l| l.name.clone());
            let mut open = true;
            egui::Window::new(text::SELECT_TITLE)
                .open(&mut open)
                .resizable(false)
                .show(ctx, |ui| {
                    out.extend(select_settings::ui(
                        ui,
                        &self.tools.select,
                        layer.as_deref(),
                    ));
                });
            self.tools.settings_open = open;
        }
        let mode = self.popup_mode();
        if let Some(popup) = self.tools.popup.as_mut() {
            let (commands, close) = popup::ui(ctx, popup, mode);
            out.extend(commands);
            if close {
                out.push(Command::ClosePopup);
            }
        }
        out
    }

    /// Draws what the tools show on the canvas: the clearance violations of
    /// the selection and the rubber band of a keepout. Returns the number
    /// of markers and band points drawn.
    pub(crate) fn draw_tools(&self, painter: &egui::Painter, camera: &Camera) -> usize {
        let Some(b) = &self.loaded else { return 0 };
        let frame = Frame::new(b.session.board.basic());
        let rect = self.view.canvas;
        let mut drawn = 0;
        if let Some(found) = &self.tools.violations {
            let drc = Drc {
                clearance: found.clone(),
                ..Drc::default()
            };
            drawn += violations::draw(painter, rect, camera, &frame, &drc, &self.view.look);
        }
        if let Some(tool) = &self.tools.keepout {
            let cursor = self
                .view
                .cursor
                .filter(|p| rect.contains(*p))
                .map(|p| camera.to_board(rect, p));
            let cursor = construction::cursor_point(&b.session.board, cursor);
            let band = construction::band(tool, &b.session.board, cursor);
            drawn += construction::draw(painter, rect, camera, &frame, &band, tool.kind);
        }
        drawn
    }
}

#[derive(Clone, Copy)]
enum Edit {
    Delete,
    Fix,
    Unfix,
}

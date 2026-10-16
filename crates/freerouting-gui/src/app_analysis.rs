//! The part of [`App::execute`](crate::App::execute) for selection, the
//! analysis overlays and lists, delete all with undo, and the DRC export
//! (G03).

use eframe::egui;

use crate::app::App;
use crate::commands::Command;
use crate::drc_export::ExportJob;
use crate::overlays::Frame;
use crate::panels::log::Level;
use crate::text;

/// The box of the selection grows by this fraction of its width and of its
/// height (half of it on each side) when the view is zoomed to it.
const ZOOM_MARGIN: f64 = 0.10;
/// The smallest box side zoomed to, in millimetres.
const MIN_ZOOM_SIDE: f64 = 0.5;

impl App {
    /// Runs the G03 commands. The caller passes only those.
    pub(crate) fn execute_analysis(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::Select(ids) => self.selection.set(ids),
            Command::ClearSelection => self.selection.clear(),
            Command::ZoomSelection => self.zoom_selection(),
            Command::ToggleRatsnest => self.overlay.show_ratsnest = !self.overlay.show_ratsnest,
            Command::ToggleViolations => {
                self.overlay.show_violations = !self.overlay.show_violations;
            }
            Command::RunDrc => self.analysis.force(),
            Command::ShowList(kind) => self.lists.window(kind).open = true,
            Command::DeleteAll => self.confirm_delete = self.loaded.is_some(),
            Command::ConfirmDeleteAll => {
                self.confirm_delete = false;
                self.delete_all();
            }
            Command::Undo => self.undo_redo(true),
            Command::Redo => self.undo_redo(false),
            Command::ExportDrcDialog => {
                if let Some(b) = &self.loaded {
                    self.export_dialog.show_for(&b.session.paths.design);
                }
            }
            Command::ExportDrcReport(path) => {
                if let Some(b) = &self.loaded {
                    let job = ExportJob::start(
                        ctx,
                        b.session.board.clone(),
                        &b.session.paths.design,
                        path,
                    );
                    self.export_job = Some(job);
                }
            }
            _ => {}
        }
    }

    /// Fits the view to the selection plus a margin (Java
    /// `zoomSelection`, which fits the box of the items offset by the
    /// largest trace half width).
    fn zoom_selection(&mut self) {
        let Some(b) = &self.loaded else { return };
        if self.selection.is_empty() {
            return;
        }
        let board = b.session.board.basic();
        let frame = Frame::new(board);
        let Some(mut bounds) = frame.bounds(&board.bounding_box_of(&self.selection.ids())) else {
            return;
        };
        let pad_x =
            (bounds.width() * ZOOM_MARGIN / 2.0).max((MIN_ZOOM_SIDE - bounds.width()) / 2.0);
        let pad_y =
            (bounds.height() * ZOOM_MARGIN / 2.0).max((MIN_ZOOM_SIDE - bounds.height()) / 2.0);
        bounds.min.x -= pad_x;
        bounds.max.x += pad_x;
        bounds.min.y -= pad_y;
        bounds.max.y += pad_y;
        let canvas = self.view.canvas;
        if let Some(camera) = self.view.camera.as_mut() {
            camera.zoom_to_rect(canvas, bounds.min, bounds.max);
            self.view.auto_fit = None;
        }
    }

    /// Other > Delete All Traces and Vias after the confirmation: a snapshot
    /// first, so that Undo restores the wiring.
    ///
    /// Java also unfills the conduction areas and sets "ignore conduction"
    /// (`BasicBoard.unfillConductionAreas`); that engine function is not
    /// ported yet (B08), so only the traces and vias go.
    fn delete_all(&mut self) {
        let Some(b) = self.loaded.as_mut() else {
            return;
        };
        b.session.edit(|board| {
            board.generate_snapshot();
            board.delete_all_traces_and_vias();
            true
        });
        self.after_board_edit();
    }

    /// Undo (`undo == true`) or redo of the board's last snapshot step.
    fn undo_redo(&mut self, undo: bool) {
        let Some(b) = self.loaded.as_mut() else {
            return;
        };
        let changed = b.session.edit(|board| {
            if undo {
                board.undo(None)
            } else {
                board.redo(None)
            }
        });
        if changed {
            self.after_board_edit();
        }
    }

    /// The scene is rebuilt in full after an edit here; G05.5 replaces that
    /// with patching.
    fn after_board_edit(&mut self) {
        let Some(b) = self.loaded.as_mut() else {
            return;
        };
        b.rebuild_scene();
        self.selection.retain_existing(b.session.board.basic());
        self.stats.full_rebuilds += 1;
    }

    /// The confirmation window of Delete All.
    pub(crate) fn confirm_delete_ui(&mut self, ctx: &egui::Context) -> Vec<Command> {
        let mut out = Vec::new();
        if !self.confirm_delete {
            return out;
        }
        egui::Window::new(text::DELETE_ALL)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(380.0);
                ui.label(text::DELETE_ALL_CONFIRM);
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        out.push(Command::ConfirmDeleteAll);
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_delete = false;
                    }
                });
            });
        out
    }

    /// Polls the export and shows its result in the status bar.
    pub(crate) fn poll_export(&mut self) {
        let Some(job) = &self.export_job else { return };
        let Some(result) = job.poll() else { return };
        let (level, message) = match result {
            Ok(()) => (
                Level::Info,
                format!("DRC report written to {}", job.path.display()),
            ),
            Err(e) => (Level::Error, format!("Could not write the DRC report: {e}")),
        };
        self.log.push(level, message.clone());
        self.notice = Some(message);
        self.export_job = None;
    }
}

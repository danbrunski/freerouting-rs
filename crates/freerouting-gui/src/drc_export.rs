//! File > Export DRC report (G03.8): the DRC report of the open board as
//! JSON, written by the serializer the CLI uses (`drc/json.rs`).
//!
//! The check runs on a worker thread over an `Arc` clone of the session
//! board. The file is written next to its final name and renamed, so a
//! failed export never leaves half a report.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use freerouting::board::RoutingBoard;
use freerouting::drc::DesignRulesChecker;

/// The coordinate unit of the exported report.
pub const COORDINATE_UNIT: &str = "mm";

/// The report text of `board`. `source` is the design's file name, as
/// `drc::run_drc` computes it.
pub fn report_json(board: &RoutingBoard, source: &str) -> String {
    DesignRulesChecker::new(board.basic())
        .generate_report(source, COORDINATE_UNIT)
        .to_json()
}

/// The file name of `design`, the report's `source`.
pub fn source_name(design: &Path) -> String {
    design
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Writes `text` to `path` through a temporary file in the same folder.
pub fn write_atomically(path: &Path, text: &str) -> Result<(), String> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, text).map_err(|e| format!("{}: {e}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("{}: {e}", path.display())
    })
}

/// A running export.
pub struct ExportJob {
    pub path: PathBuf,
    rx: Receiver<Result<(), String>>,
}

impl ExportJob {
    pub fn start(
        ctx: &egui::Context,
        board: Arc<RoutingBoard>,
        design: &Path,
        path: PathBuf,
    ) -> ExportJob {
        let (tx, rx) = mpsc::channel();
        let (ctx, source, target) = (ctx.clone(), source_name(design), path.clone());
        std::thread::spawn(move || {
            let result = write_atomically(&target, &report_json(&board, &source));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        ExportJob { path, rx }
    }

    /// The result, once the worker is done.
    pub fn poll(&self) -> Option<Result<(), String>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("The export stopped.".into())),
        }
    }
}

/// The Export DRC report window: a path and two buttons.
#[derive(Default)]
pub struct ExportDialog {
    pub open: bool,
    pub path: String,
}

impl ExportDialog {
    /// Opens the window with the report next to the design as the default.
    pub fn show_for(&mut self, design: &Path) {
        let stem = design
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = design.parent().map(Path::to_path_buf).unwrap_or_default();
        self.path = dir.join(format!("{stem}_drc.json")).display().to_string();
        self.open = true;
    }

    /// Draws the window; returns the path to export to when Export is
    /// clicked.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        if !self.open {
            return None;
        }
        let mut chosen = None;
        let mut open = true;
        let mut close = false;
        egui::Window::new("Export DRC report")
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label("Write the design rules check as a JSON report (coordinates in mm).");
                ui.horizontal(|ui| {
                    ui.label("File:");
                    ui.text_edit_singleline(&mut self.path);
                });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.path.trim().is_empty(), egui::Button::new("Export"))
                        .clicked()
                    {
                        chosen = Some(PathBuf::from(self.path.trim()));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        self.open = open && !close;
        chosen
    }
}

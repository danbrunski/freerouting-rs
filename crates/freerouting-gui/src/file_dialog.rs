//! A small built-in file browser for opening boards and, in save mode
//! (G08.1), for choosing where to save one.
//!
//! Native dialogs need GTK or an XDG desktop portal on Linux, and WSLg has
//! neither by default, so the app browses folders itself. Drag and drop and
//! command-line paths work too.
//!
//! Save mode is Java's `BoardExportActions.showSaveAsDialog` without the
//! formats G00 question 2 dropped: a file name field, the two filters
//! (`Specctra session file (*.ses)`, `Specctra design file (*.dsn)`) and a
//! question before an existing file is replaced.

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::files::SaveKind;
use crate::text;

/// What the dialog gave back this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picked {
    /// A file to open.
    Open(PathBuf),
    /// The file to save to (its extension is `.ses` or `.dsn`; a file that
    /// exists has been confirmed).
    Save(PathBuf),
    /// The save dialog was closed without a file.
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Open,
    Save,
}

pub struct FileDialog {
    pub open: bool,
    mode: Mode,
    dir: PathBuf,
    typed: String,
    entries: Vec<(String, PathBuf, bool)>,
    error: Option<String>,
    // Save mode.
    file_name: String,
    kind: SaveKind,
    confirm_overwrite: Option<PathBuf>,
}

const EXTENSIONS: [&str; 3] = ["dsn", "ses", "kicad_pcb"];

impl FileDialog {
    pub fn new(dir: PathBuf) -> Self {
        let mut d = FileDialog {
            open: false,
            mode: Mode::Open,
            dir: PathBuf::new(),
            typed: String::new(),
            entries: Vec::new(),
            error: None,
            file_name: String::new(),
            kind: SaveKind::Ses,
            confirm_overwrite: None,
        };
        d.go(dir);
        d
    }

    pub fn show_in(&mut self, dir: &Path) {
        self.mode = Mode::Open;
        self.go(dir.to_path_buf());
        self.open = true;
    }

    /// Shows the dialog in save mode, in the folder of `default` with its
    /// name filled in; its extension chooses the filter (session by default).
    pub fn show_save(&mut self, default: &Path) {
        self.mode = Mode::Save;
        self.kind = SaveKind::of_path(default).unwrap_or(SaveKind::Ses);
        self.file_name = default
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.confirm_overwrite = None;
        let dir = default
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| self.dir.clone(), Path::to_path_buf);
        self.go(dir);
        self.open = true;
    }

    /// True while the dialog is open for saving.
    pub fn is_saving(&self) -> bool {
        self.open && self.mode == Mode::Save
    }

    /// The path a save would write for the file name field: the name in the
    /// folder shown, with the filter's extension added when it has none.
    /// `Err` is the text for an unsupported extension.
    pub fn save_target(&self) -> Result<PathBuf, String> {
        let name = self.file_name.trim();
        if name.is_empty() {
            return Err(String::new());
        }
        let mut path = self.dir.join(name);
        if path.extension().is_none() {
            path.set_extension(self.kind.extension());
        }
        match SaveKind::of_path(&path) {
            Some(_) => Ok(path),
            None => Err(text::fill(text::SAVE_UNSUPPORTED_EXTENSION, &[name])),
        }
    }

    /// The file name field (tests).
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Types into the file name field (tests).
    pub fn set_file_name(&mut self, name: &str) {
        self.file_name = name.to_string();
    }

    fn go(&mut self, dir: PathBuf) {
        let dir = dir.canonicalize().unwrap_or(dir);
        match std::fs::read_dir(&dir) {
            Ok(read) => {
                let mut entries: Vec<(String, PathBuf, bool)> = read
                    .filter_map(Result::ok)
                    .filter_map(|e| {
                        let path = e.path();
                        let is_dir = path.is_dir();
                        let name = e.file_name().to_string_lossy().into_owned();
                        let wanted = is_dir
                            || path.extension().is_some_and(|x| {
                                EXTENSIONS.iter().any(|w| x.eq_ignore_ascii_case(w))
                            });
                        (wanted && !name.starts_with('.')).then_some((name, path, is_dir))
                    })
                    .collect();
                entries.sort_by(|a, b| {
                    b.2.cmp(&a.2)
                        .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
                });
                self.entries = entries;
                self.typed = display(&dir);
                self.dir = dir;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{}: {e}", dir.display())),
        }
    }

    /// Shows the dialog; returns what was picked this frame.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<Picked> {
        if !self.open {
            return None;
        }
        let saving = self.mode == Mode::Save;
        let mut picked: Option<Picked> = None;
        let mut go_to = None;
        let mut open = self.open;
        let title = if saving {
            text::SAVE_BOARD_TITLE
        } else {
            "Open board"
        };
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .default_size([560.0, 420.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("⬆").on_hover_text("Parent folder").clicked() {
                        if let Some(parent) = self.dir.parent() {
                            go_to = Some(parent.to_path_buf());
                        }
                    }
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.typed)
                            .desired_width(f32::INFINITY)
                            .hint_text("Folder or file path"),
                    );
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let path = PathBuf::from(self.typed.trim());
                        if path.is_file() && !saving {
                            picked = Some(Picked::Open(path));
                        } else {
                            go_to = Some(path);
                        }
                    }
                });
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.separator();
                let kind = self.kind;
                let list_height =
                    (ui.available_height() - if saving { 70.0 } else { 0.0 }).max(80.0);
                egui::ScrollArea::vertical()
                    .max_height(list_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let shown: Vec<_> = self
                            .entries
                            .iter()
                            .filter(|(_, path, is_dir)| {
                                !saving || *is_dir || SaveKind::of_path(path) == Some(kind)
                            })
                            .collect();
                        if shown.is_empty() {
                            ui.weak(if saving {
                                "No folders or files of this type here."
                            } else {
                                "No folders or .dsn/.ses/.kicad_pcb files here."
                            });
                        }
                        for (name, path, is_dir) in shown {
                            let label = if *is_dir {
                                format!("📁 {name}")
                            } else {
                                format!("📄 {name}")
                            };
                            let r = ui.selectable_label(false, label);
                            if *is_dir && r.clicked() {
                                go_to = Some(path.clone());
                            } else if !*is_dir && saving && r.clicked() {
                                self.file_name = name.clone();
                            } else if !*is_dir && !saving && (r.double_clicked() || r.clicked()) {
                                picked = Some(Picked::Open(path.clone()));
                            }
                        }
                    });
                if saving {
                    ui.separator();
                    self.save_controls(ui, &mut picked);
                }
            });
        if saving {
            self.overwrite_window(ctx, &mut picked);
        }
        if let Some(dir) = go_to {
            self.go(dir);
        }
        let closed_by_user = !open && picked.is_none();
        self.open = open && picked.is_none();
        if closed_by_user && saving {
            self.confirm_overwrite = None;
            return Some(Picked::Cancelled);
        }
        picked
    }

    /// The file name field, the filter and the Save button.
    fn save_controls(&mut self, ui: &mut egui::Ui, picked: &mut Option<Picked>) {
        let mut chosen = self.kind;
        ui.horizontal(|ui| {
            ui.label(text::FILE_NAME_LABEL);
            let edit = ui
                .add(egui::TextEdit::singleline(&mut self.file_name).desired_width(f32::INFINITY));
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.try_save(picked);
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("save-filter")
                .selected_text(self.kind.filter())
                .show_ui(ui, |ui| {
                    for kind in [SaveKind::Ses, SaveKind::Dsn] {
                        ui.selectable_value(&mut chosen, kind, kind.filter());
                    }
                });
            if ui.button(text::SAVE_BUTTON).clicked() {
                self.try_save(picked);
            }
        });
        if chosen != self.kind {
            // Choosing the other filter changes the extension of the name.
            self.kind = chosen;
            let stem = Path::new(&self.file_name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty());
            if let Some(stem) = stem {
                self.file_name = format!("{stem}.{}", chosen.extension());
            }
            self.go(self.dir.clone());
        }
    }

    fn try_save(&mut self, picked: &mut Option<Picked>) {
        match self.save_target() {
            Ok(path) if path.exists() => self.confirm_overwrite = Some(path),
            Ok(path) => *picked = Some(Picked::Save(path)),
            Err(message) if message.is_empty() => {}
            Err(message) => self.error = Some(message),
        }
    }

    /// "The file exists. Replace it?"
    fn overwrite_window(&mut self, ctx: &egui::Context, picked: &mut Option<Picked>) {
        let Some(path) = self.confirm_overwrite.clone() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        egui::Window::new(text::SAVE_BOARD_TITLE)
            .id(egui::Id::new("save-overwrite"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(text::fill(text::OVERWRITE_CONFIRM, &[&name]));
                ui.horizontal(|ui| {
                    if ui.button("Replace").clicked() {
                        *picked = Some(Picked::Save(path.clone()));
                        self.confirm_overwrite = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_overwrite = None;
                    }
                });
            });
    }
}

/// A path as the user would type it, without Windows' `\\?\` prefix.
fn display(path: &Path) -> String {
    let s = path.display().to_string();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// K07.2: the built-in file browser lists `.kicad_pcb` boards beside
    /// `.dsn` and `.ses` files, in any case, and hides other files.
    #[test]
    fn lists_kicad_boards() {
        let dir = std::env::temp_dir().join(format!("fr-file-dialog-kicad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "a.dsn",
            "b.ses",
            "c.kicad_pcb",
            "D.KICAD_PCB",
            "e.kicad_pro",
            "f.txt",
        ] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        let dialog = FileDialog::new(dir.clone());
        let names: Vec<&str> = dialog.entries.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(names, ["a.dsn", "b.ses", "c.kicad_pcb", "D.KICAD_PCB"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G08.1: the save target is the file name in the folder shown, with the
    /// extension of the filter when it has none; only `.ses` and `.dsn` pass.
    #[test]
    fn save_target_follows_the_filter() {
        let dir = std::env::temp_dir().join(format!("fr-file-dialog-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut dialog = FileDialog::new(dir.clone());
        dialog.show_save(&dir.join("board.ses"));
        assert!(dialog.is_saving());
        assert_eq!(dialog.file_name(), "board.ses");
        let canonical = dir.canonicalize().unwrap();
        assert_eq!(dialog.save_target(), Ok(canonical.join("board.ses")));
        dialog.set_file_name("other");
        assert_eq!(dialog.save_target(), Ok(canonical.join("other.ses")));
        dialog.set_file_name("Design.DSN");
        assert_eq!(dialog.save_target(), Ok(canonical.join("Design.DSN")));
        dialog.set_file_name("x.scr");
        assert!(
            dialog
                .save_target()
                .unwrap_err()
                .contains("Only .ses and .dsn")
        );
        dialog.set_file_name("  ");
        assert_eq!(dialog.save_target(), Err(String::new()));
        // A design name picks the design filter and lists no sessions.
        dialog.show_save(&dir.join("board.dsn"));
        assert_eq!(dialog.save_target(), Ok(canonical.join("board.dsn")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

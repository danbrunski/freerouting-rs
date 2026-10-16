//! A small built-in file browser for opening boards.
//!
//! Native dialogs need GTK or an XDG desktop portal on Linux, and WSLg has
//! neither by default, so the app browses folders itself. Drag and drop and
//! command-line paths work too.

use std::path::{Path, PathBuf};

use eframe::egui;

pub struct FileDialog {
    pub open: bool,
    dir: PathBuf,
    typed: String,
    entries: Vec<(String, PathBuf, bool)>,
    error: Option<String>,
}

const EXTENSIONS: [&str; 3] = ["dsn", "ses", "kicad_pcb"];

impl FileDialog {
    pub fn new(dir: PathBuf) -> Self {
        let mut d = FileDialog {
            open: false,
            dir: PathBuf::new(),
            typed: String::new(),
            entries: Vec::new(),
            error: None,
        };
        d.go(dir);
        d
    }

    pub fn show_in(&mut self, dir: &Path) {
        self.go(dir.to_path_buf());
        self.open = true;
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

    /// Shows the dialog; returns the file picked this frame.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        if !self.open {
            return None;
        }
        let mut picked = None;
        let mut go_to = None;
        let mut open = self.open;
        egui::Window::new("Open board")
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
                        if path.is_file() {
                            picked = Some(path);
                        } else {
                            go_to = Some(path);
                        }
                    }
                });
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.entries.is_empty() {
                            ui.weak("No folders or .dsn/.ses/.kicad_pcb files here.");
                        }
                        for (name, path, is_dir) in &self.entries {
                            let label = if *is_dir {
                                format!("📁 {name}")
                            } else {
                                format!("📄 {name}")
                            };
                            let r = ui.selectable_label(false, label);
                            if *is_dir && r.clicked() {
                                go_to = Some(path.clone());
                            } else if !*is_dir && (r.double_clicked() || r.clicked()) {
                                picked = Some(path.clone());
                            }
                        }
                    });
            });
        if let Some(dir) = go_to {
            self.go(dir);
        }
        self.open = open && picked.is_none();
        picked
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
}

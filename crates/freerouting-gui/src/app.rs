//! The main window: menu, layer panel, status bar and board canvas.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

use eframe::egui::{self, Color32, RichText, Sense};
use freerouting::dsn::{self, Pcb, Session};
use freerouting::io::specctra::{BoardReadResult, build_board};

use crate::file_dialog::FileDialog;
use crate::scene::{self, Item, Kind, Scene, Slot};
use crate::view::{self, Camera, Tessellation, Visibility};

/// A board that has been read and tessellated.
pub struct Loaded {
    pub pcb: Arc<Pcb>,
    pub design_path: PathBuf,
    pub session_path: Option<PathBuf>,
    pub scene: Scene,
    pub tess: Tessellation,
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(dsn::decode(&bytes).into_owned())
}

fn read_design(path: &Path) -> Result<(Pcb, Vec<String>), String> {
    let (pcb, warnings) = Pcb::parse_with_warnings(&read_text(path)?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((pcb, warnings.iter().map(ToString::to_string).collect()))
}

/// The design a session belongs to: `base_design` or the session's own name
/// with `.dsn`, next to the session.
fn find_design(session_path: &Path, session: &Session) -> Option<PathBuf> {
    let dir = session_path.parent()?;
    let mut names = Vec::new();
    if let Some(base) = &session.base_design {
        names.push(base.clone());
    }
    if let Some(stem) = session_path.file_stem() {
        names.push(stem.to_string_lossy().into_owned());
    }
    names.push(session.name.clone());
    names.iter().find_map(|n| {
        let p = dir.join(format!("{n}.dsn"));
        p.is_file().then_some(p)
    })
}

/// Reads `path`. A session is shown on top of `current` if no design file
/// for it is found next to it.
pub fn load(path: &Path, current: Option<(Arc<Pcb>, PathBuf)>) -> Result<Loaded, String> {
    let is_session = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ses"));
    let (pcb, design_path, session, session_path, mut warnings) = if is_session {
        let (session, ses_warnings) = Session::parse_with_warnings(&read_text(path)?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut warnings: Vec<String> = ses_warnings.iter().map(ToString::to_string).collect();
        let (pcb, design_path) = match find_design(path, &session) {
            Some(design) => {
                let (pcb, w) = read_design(&design)?;
                warnings.extend(w);
                (Arc::new(pcb), design)
            }
            None => current.ok_or_else(|| {
                format!(
                    "No design file found for session {}. Open the .dsn first, then the .ses.",
                    path.display()
                )
            })?,
        };
        (
            pcb,
            design_path,
            Some(session),
            Some(path.to_path_buf()),
            warnings,
        )
    } else {
        let (pcb, warnings) = read_design(path)?;
        (Arc::new(pcb), path.to_path_buf(), None, None, warnings)
    };
    let merged;
    let design = match &session {
        Some(session) => {
            merged = scene::apply_session(&pcb, session);
            &merged
        }
        None => &*pcb,
    };
    let result = build_board(design);
    let board = match &result {
        BoardReadResult::ParseError { detail, .. } => {
            return Err(format!("{}: {detail}", design_path.display()));
        }
        BoardReadResult::Success { warnings: w, .. } => {
            warnings.extend(w.iter().cloned());
            result.board()
        }
        BoardReadResult::OutlineMissing { warnings: w, .. } => {
            warnings.push("The design has no usable board outline.".into());
            warnings.extend(w.iter().cloned());
            result.board()
        }
    };
    let Some(board) = board else {
        return Err(format!(
            "{}: the design has no usable board outline.",
            design_path.display()
        ));
    };
    let mut scene = Scene::from_board(&pcb.name, board);
    scene.warnings = warnings;
    let tess = Tessellation::new(&scene);
    Ok(Loaded {
        pcb,
        design_path,
        session_path,
        scene,
        tess,
    })
}

pub struct App {
    board: Option<Loaded>,
    camera: Option<Camera>,
    layer_visible: Vec<bool>,
    kind_visible: Vec<bool>,
    show_labels: bool,
    active_layer: usize,
    dialog: FileDialog,
    loading: Option<(PathBuf, mpsc::Receiver<Result<Loaded, String>>)>,
    error: Option<String>,
    show_warnings: bool,
    fit_requested: bool,
    /// The canvas size the view was last fitted to, while the user has not
    /// panned or zoomed since. The view is fitted again when the canvas size
    /// changes: on Windows the first frames come before the window has its
    /// final size, and a fit to that size leaves the board off-screen.
    auto_fit: Option<egui::Vec2>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Vec<PathBuf>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let start_dir = open
            .first()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let mut app = App {
            board: None,
            camera: None,
            layer_visible: Vec::new(),
            kind_visible: vec![true; Kind::ALL.len()],
            show_labels: true,
            active_layer: 0,
            dialog: FileDialog::new(start_dir),
            loading: None,
            error: None,
            show_warnings: false,
            fit_requested: false,
            auto_fit: None,
        };
        // `freerouting-gui board.dsn [board.ses]`: the session is read once
        // the design is loaded.
        if let Some(design) = open.first() {
            match load(design, None).and_then(|b| match open.get(1) {
                Some(ses) => load(ses, Some((b.pcb.clone(), b.design_path.clone()))),
                None => Ok(b),
            }) {
                Ok(board) => app.set_board(board),
                Err(e) => app.error = Some(e),
            }
        }
        app
    }

    fn set_board(&mut self, board: Loaded) {
        let same_layers = self.board.as_ref().is_some_and(|b| {
            b.scene.layers.len() == board.scene.layers.len() && b.design_path == board.design_path
        });
        if !same_layers {
            self.layer_visible = vec![true; board.scene.layers.len()];
            self.active_layer = 0;
            self.camera = None;
        }
        self.board = Some(board);
        self.error = None;
    }

    fn start_load(&mut self, ctx: &egui::Context, path: PathBuf) {
        let current = self
            .board
            .as_ref()
            .map(|b| (b.pcb.clone(), b.design_path.clone()));
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let p = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(load(&p, current));
            ctx.request_repaint();
        });
        self.loading = Some((path, rx));
    }

    fn poll_load(&mut self) {
        let Some((_, rx)) = &self.loading else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.loading = None;
                match result {
                    Ok(board) => self.set_board(board),
                    Err(e) => self.error = Some(e),
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.loading = None;
                self.error = Some("Reading the file failed unexpectedly.".into());
            }
        }
    }

    fn reload(&mut self, ctx: &egui::Context) {
        if let Some(b) = &self.board {
            let design = b.design_path.clone();
            let session = b.session_path.clone();
            self.board = None;
            self.start_load(ctx, session.unwrap_or(design));
        }
    }

    fn open_dialog(&mut self) {
        let dir = self
            .board
            .as_ref()
            .and_then(|b| b.design_path.parent().map(Path::to_path_buf));
        match dir {
            Some(dir) => self.dialog.show_in(&dir),
            None => self.dialog.open = true,
        }
    }

    fn menu(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open…  (Ctrl+O)").clicked() {
                    self.open_dialog();
                    ui.close();
                }
                if ui
                    .add_enabled(self.board.is_some(), egui::Button::new("Reload"))
                    .clicked()
                {
                    self.reload(ctx);
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Fit board  (F)").clicked() {
                    self.fit_requested = true;
                    ui.close();
                }
            });
            if let Some(b) = &self.board {
                ui.separator();
                let name = |p: &Path| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                };
                ui.label(RichText::new(name(&b.design_path)).strong());
                if let Some(session) = &b.session_path {
                    ui.label(format!("+ {}", name(session)));
                }
                // Exporters often put the full path of the source file here.
                if !b.scene.name.contains(['/', '\\']) {
                    ui.weak(&b.scene.name);
                }
            }
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        let Some(b) = &self.board else {
            ui.weak("No board loaded.");
            return;
        };
        let n = b.scene.layers.len();
        ui.heading("Layers");
        ui.weak("Click a name to bring it to the front.");
        for (i, layer) in b.scene.layers.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.layer_visible[i], "");
                let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                ui.painter().rect_filled(rect, 2.0, view::layer_color(i, n));
                let mut text = RichText::new(&layer.name);
                if !layer.signal {
                    text = text.italics();
                }
                let shortcut = if i < 9 {
                    format!("  {}", i + 1)
                } else {
                    String::new()
                };
                if ui
                    .selectable_label(self.active_layer == i, text)
                    .on_hover_text(format!(
                        "{} layer{shortcut}",
                        if layer.signal { "Signal" } else { "Plane" }
                    ))
                    .clicked()
                {
                    self.active_layer = i;
                    self.layer_visible[i] = true;
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.small_button("All").clicked() {
                self.layer_visible.iter_mut().for_each(|v| *v = true);
            }
            if ui.small_button("Active only").clicked() {
                for (i, v) in self.layer_visible.iter_mut().enumerate() {
                    *v = i == self.active_layer;
                }
            }
        });
        ui.add_space(8.0);
        ui.heading("Show");
        for (i, kind) in Kind::ALL.iter().enumerate() {
            ui.checkbox(&mut self.kind_visible[i], kind.label());
        }
        ui.checkbox(&mut self.show_labels, "Reference designators");
        ui.add_space(8.0);
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
        if !b.scene.warnings.is_empty() {
            ui.add_space(4.0);
            if ui
                .link(format!("{} warnings", b.scene.warnings.len()))
                .clicked()
            {
                self.show_warnings = true;
            }
        }
    }

    fn describe(scene: &Scene, item: &Item) -> String {
        let what = match item.kind {
            Kind::Plane => "Conduction area",
            Kind::Keepout => "Keepout",
            Kind::ViaKeepout => "Via keepout",
            Kind::PlaceKeepout => "Placement keepout",
            Kind::Trace => "Trace",
            Kind::Pad => "Pad",
            Kind::Via => "Via",
            Kind::Outline => "Board outline",
            Kind::ComponentOutline => "Footprint",
        };
        let mut parts = vec![what.to_string()];
        if let Some(owner) = &item.owner {
            parts.push(owner.clone());
        }
        if let Some(net) = item.net {
            parts.push(format!("net {}", scene.nets[net]));
        }
        if let Slot::Layer(l) = item.slot {
            parts.push(scene.layers[l].name.clone());
        }
        parts.join(" · ")
    }

    fn canvas(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        let Some(b) = &self.board else {
            painter.rect_filled(rect, 0.0, view::BACKGROUND);
            let msg = match &self.loading {
                Some((path, _)) => format!("Reading {}…", path.display()),
                None => "Open a .dsn board with File > Open, or drop one on the window.".into(),
            };
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                msg,
                egui::FontId::proportional(16.0),
                Color32::GRAY,
            );
            return None;
        };
        let resized = self.auto_fit.is_some_and(|size| size != rect.size());
        if self.camera.is_none() || self.fit_requested || resized {
            self.camera = Some(Camera::fit(&b.scene, rect));
            self.fit_requested = false;
            self.auto_fit = Some(rect.size());
        }
        let cam = self.camera.as_mut().unwrap();

        if response.dragged() {
            cam.pan(response.drag_delta());
            self.auto_fit = None;
        }
        if let Some(hover) = response.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            let factor = (f64::from(scroll.y) / 200.0).exp() * f64::from(pinch);
            if factor != 1.0 {
                cam.zoom_at(rect, hover, factor);
                self.auto_fit = None;
            }
        }
        let vis = Visibility {
            layers: &self.layer_visible,
            kinds: &self.kind_visible,
            active: self.active_layer,
            labels: self.show_labels,
        };
        view::paint(
            &painter,
            rect,
            cam,
            &b.tess,
            b.scene.layers.len(),
            &b.scene.labels,
            &vis,
        );

        let pos = response.hover_pos()?;
        let p = cam.to_board(rect, pos);
        let slack = 2.0 / cam.zoom;
        let picked = b.scene.pick(
            p,
            slack,
            |i| vis.slot(i.slot) && vis.kind(i.kind),
            Some(self.active_layer),
        );
        let mut status = format!("X {:.3}  Y {:.3} mm", p.x, p.y);
        if let Some(item) = picked {
            status.push_str("    ");
            status.push_str(&Self::describe(&b.scene, item));
        }
        Some(status)
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_load();

        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.start_load(ctx, path);
        }
        if let Some(path) = self.dialog.ui(ctx) {
            self.start_load(ctx, path);
        }

        let typing = ctx.wants_keyboard_input();
        ctx.input(|i| {
            if i.modifiers.command && i.key_pressed(egui::Key::O) {
                self.dialog.open = true;
            }
            if typing {
                return;
            }
            if i.key_pressed(egui::Key::F) || i.key_pressed(egui::Key::Home) {
                self.fit_requested = true;
            }
            let digits = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ];
            for (n, key) in digits.iter().enumerate() {
                if i.key_pressed(*key) && n < self.layer_visible.len() {
                    self.active_layer = n;
                    self.layer_visible[n] = true;
                }
            }
        });

        egui::TopBottomPanel::top("menu").show(ctx, |ui| self.menu(ctx, ui));
        let status_id = egui::Id::new("status");
        let status: Option<String> = ctx
            .data(|d| d.get_temp::<Option<String>>(status_id))
            .flatten();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(e) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                } else if let Some((path, _)) = &self.loading {
                    ui.spinner();
                    ui.label(format!("Reading {}", path.display()));
                } else {
                    ui.label(status.unwrap_or_default());
                }
            });
        });
        egui::SidePanel::left("layers")
            .resizable(true)
            .default_width(210.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.side_panel(ui));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let status = self.canvas(ui);
                // The status bar is laid out before the canvas, so a new
                // hover text shows on the next frame.
                if ctx.data(|d| d.get_temp::<Option<String>>(status_id)) != Some(status.clone()) {
                    ctx.request_repaint();
                }
                ctx.data_mut(|d| d.insert_temp(status_id, status));
            });

        if self.show_warnings {
            let mut open = true;
            egui::Window::new("Warnings")
                .open(&mut open)
                .default_size([600.0, 300.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(b) = &self.board {
                            for w in &b.scene.warnings {
                                ui.label(w);
                            }
                        }
                    });
                });
            self.show_warnings = open;
        }
    }
}

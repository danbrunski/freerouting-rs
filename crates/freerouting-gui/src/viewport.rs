//! The board canvas: view state (camera, visibility, active layer), mouse
//! handling and the hover status text.
//!
//! The mouse follows Java's model (`gui.board.BoardPanel`): the left button
//! is reserved for tools, the middle button (or Space and the left button)
//! pans, the wheel zooms at the cursor, and a left-drag after `F` zooms to
//! the dragged rectangle. Keys are in [`crate::input`].

use eframe::egui::{self, Color32, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};

use freerouting::board::structure::Unit;

use crate::commands::Command;
use crate::look::{Look, snap_level};
use crate::scene::{Item, Kind, P, Scene, Slot};
use crate::session::Loaded;
use crate::stats::PaintStats;
use crate::view::{self, Camera, Labels, Orientation, Visibility};

/// Zoom factor of the `Z` and `O` keys (Java's `c_zoom_factor`).
pub const KEY_ZOOM: f64 = 2.0;

/// Which layers and kinds are shown, which layer is active, and the camera.
pub struct ViewState {
    pub camera: Option<Camera>,
    pub layer_visible: Vec<bool>,
    pub kind_visible: Vec<bool>,
    pub show_labels: bool,
    pub active_layer: usize,
    pub fit_requested: bool,
    /// The canvas size the view was last fitted to, while the user has not
    /// panned or zoomed since. The view is fitted again when the canvas size
    /// changes: on Windows the first frames come before the window has its
    /// final size, and a fit to that size leaves the board off-screen.
    pub auto_fit: Option<Vec2>,
    pub crosshair: bool,
    /// Colours, visibility levels, layer dimming and the virtual layers
    /// (G04.4, G04.5, G04.7).
    pub look: Look,
    /// Rotation and mirroring of the displayed board (G04.6).
    pub orientation: Orientation,
    /// The unit of the status bar, the lists and the item info window.
    pub unit: Unit,
    /// The next left-drag zooms to its rectangle.
    pub zoom_rect_armed: bool,
    /// The canvas as of the last frame.
    pub canvas: Rect,
    /// How the board is drawn: the GPU pipeline or the CPU fallback.
    pub renderer: view::Renderer,
    /// The cursor, while it is over the canvas.
    pub cursor: Option<Pos2>,
    /// The corners of the rectangle being dragged for a zoom.
    zoom_rect_drag: Option<(Pos2, Pos2)>,
    pan_anchor: Option<Pos2>,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            camera: None,
            layer_visible: Vec::new(),
            kind_visible: vec![true; Kind::ALL.len()],
            show_labels: true,
            active_layer: 0,
            fit_requested: false,
            auto_fit: None,
            crosshair: false,
            look: Look::default(),
            orientation: Orientation::default(),
            unit: Unit::Mm,
            zoom_rect_armed: false,
            canvas: Rect::from_min_size(Pos2::ZERO, Vec2::new(1.0, 1.0)),
            renderer: view::Renderer::default(),
            cursor: None,
            zoom_rect_drag: None,
            pan_anchor: None,
        }
    }
}

impl ViewState {
    pub fn kind_on(&self, kind: Kind) -> bool {
        Kind::ALL
            .iter()
            .position(|&k| k == kind)
            .is_some_and(|i| self.kind_visible[i])
    }

    pub fn visibility(&self) -> Visibility<'_> {
        Visibility {
            layers: &self.layer_visible,
            kinds: &self.kind_visible,
            active: self.active_layer,
            labels: self.show_labels,
            look: &self.look,
        }
    }

    /// A new board: all layers on, first layer active, fitted view.
    pub fn reset_for_board(&mut self, layers: usize) {
        self.layer_visible = vec![true; layers];
        self.look.reset_for_board(layers);
        self.active_layer = 0;
        self.camera = None;
        self.auto_fit = None;
    }

    /// Turns or mirrors the board about the view centre, so the displayed
    /// section stays (Java keeps the viewport centre when it mirrors). A view
    /// that has not been moved by the user is fitted again, because a quarter
    /// turn changes which side of the board is the long one.
    fn set_orientation(&mut self, orientation: Orientation) {
        self.orientation = orientation;
        if let Some(cam) = self.camera.as_mut() {
            cam.orientation = orientation;
            if self.auto_fit.is_some() {
                self.fit_requested = true;
            }
        }
    }

    /// The point the key commands act on: the cursor, else the canvas centre.
    fn anchor(&self) -> Pos2 {
        self.cursor.unwrap_or_else(|| self.canvas.center())
    }

    fn zoom_at_anchor(&mut self, factor: f64) {
        let anchor = self.anchor();
        let rect = self.canvas;
        if let Some(cam) = self.camera.as_mut() {
            cam.zoom_at(rect, anchor, factor);
            self.auto_fit = None;
        }
    }

    /// Applies a view command. `signal` tells which layers are signal layers.
    pub fn apply(&mut self, cmd: &Command, signal: &[bool]) {
        match cmd {
            Command::FitBoard => self.fit_requested = true,
            Command::ZoomIn => self.zoom_at_anchor(KEY_ZOOM),
            Command::ZoomOut => self.zoom_at_anchor(1.0 / KEY_ZOOM),
            Command::CenterOnCursor => {
                let (rect, anchor) = (self.canvas, self.anchor());
                if let Some(cam) = self.camera.as_mut() {
                    cam.center = cam.to_board(rect, anchor);
                    self.auto_fit = None;
                }
            }
            Command::ZoomToRectangle => self.zoom_rect_armed = true,
            Command::Cancel => self.zoom_rect_armed = false,
            Command::ToggleCrosshair => self.crosshair = !self.crosshair,
            Command::SetActiveLayer(n) if *n < self.layer_visible.len() => {
                self.active_layer = *n;
                self.layer_visible[*n] = true;
            }
            Command::StepActiveLayer(step) => {
                let next = (1..=signal.len())
                    .map(|d| self.active_layer as i64 + i64::from(*step) * d as i64)
                    .take_while(|&i| i >= 0 && (i as usize) < signal.len())
                    .find(|&i| signal[i as usize]);
                if let Some(i) = next {
                    self.apply(&Command::SetActiveLayer(i as usize), signal);
                }
            }
            Command::ToggleLayer(n) => {
                if let Some(v) = self.layer_visible.get_mut(*n) {
                    *v = !*v;
                }
            }
            Command::ShowAllLayers => self.layer_visible.iter_mut().for_each(|v| *v = true),
            Command::ActiveLayerOnly => {
                for (i, v) in self.layer_visible.iter_mut().enumerate() {
                    *v = i == self.active_layer;
                }
            }
            Command::ToggleKind(kind) => {
                if let Some(i) = Kind::ALL.iter().position(|k| k == kind) {
                    self.kind_visible[i] = !self.kind_visible[i];
                }
            }
            Command::ToggleLabels => self.show_labels = !self.show_labels,
            Command::SetCrosshair(on) => self.crosshair = *on,
            Command::SetRotation(turns) => {
                self.set_orientation(Orientation {
                    quarter_turns: turns % 4,
                    ..self.orientation
                });
            }
            Command::SetMirror(mirror) => {
                self.set_orientation(Orientation {
                    mirror: *mirror,
                    ..self.orientation
                });
            }
            Command::SetUnit(unit) => self.unit = *unit,
            Command::SetLayerLevel(layer, level) => {
                self.look.set_layer_level(*layer, snap_level(*level));
            }
            Command::SetObjectLevel(object, level) => {
                self.look.set_object_level(*object, snap_level(*level));
            }
            Command::SetDim(dim) => self.look.set_dim(snap_level(*dim)),
            Command::ResetLevels => self.look.reset_levels(),
            Command::ToggleVirtualLayer(i) if *i < crate::look::VIRTUAL_LAYERS => {
                self.look.set_virtual_on(*i, !self.look.virtual_on(*i));
            }
            Command::SetItemColor(layer, column, [r, g, b]) => {
                self.look
                    .set_item_color(*layer, *column, Color32::from_rgb(*r, *g, *b));
            }
            Command::SetOtherColor(color, [r, g, b]) => {
                self.look
                    .set_other_color(*color, Color32::from_rgb(*r, *g, *b));
            }
            Command::ResetColors => self.look.reset_colors(),
            _ => {}
        }
    }
}

/// Where the cursor is and what is under it, for the status bar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hover {
    /// The cursor on the board, in millimetres of the design file's frame.
    pub cursor: Option<P>,
    /// What is under it (`Pad · U1-3 · net GND · F.Cu`).
    pub description: Option<String>,
}

/// Where the canvas leaves its [`Hover`] for the status bar, which is laid
/// out before the canvas. The type of the stored value is `Hover` on both
/// the read and the write, or the read silently returns nothing.
fn hover_id() -> egui::Id {
    egui::Id::new("hover")
}

/// The hover of the last frame.
pub fn hover_of(ctx: &egui::Context) -> Hover {
    ctx.data(|d| d.get_temp::<Hover>(hover_id()))
        .unwrap_or_default()
}

/// Stores the hover for the next frame's status bar; true if it changed.
pub fn store_hover(ctx: &egui::Context, hover: &Hover) -> bool {
    let changed = hover_of(ctx) != *hover;
    ctx.data_mut(|d| d.insert_temp(hover_id(), hover.clone()));
    changed
}

/// What a frame of the canvas produced.
#[derive(Default)]
pub struct CanvasOut {
    /// The hover for the status bar.
    pub hover: Hover,
    pub paint: PaintStats,
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

/// Draws the canvas and handles the mouse. `message` is shown when no board
/// is open.
pub fn canvas(
    ui: &mut egui::Ui,
    view: &mut ViewState,
    loaded: Option<&Loaded>,
    message: &str,
) -> CanvasOut {
    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = response.rect;
    view.canvas = rect;
    view.cursor = response.hover_pos();
    let Some(b) = loaded else {
        painter.rect_filled(rect, 0.0, view::BACKGROUND);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            message,
            egui::FontId::proportional(16.0),
            Color32::GRAY,
        );
        return CanvasOut::default();
    };
    let resized = view.auto_fit.is_some_and(|size| size != rect.size());
    if view.camera.is_none() || view.fit_requested || resized {
        view.camera = Some(Camera::fit_oriented(&b.scene, rect, view.orientation));
        view.fit_requested = false;
        view.auto_fit = Some(rect.size());
    }
    handle_mouse(ui, &response, view, rect);
    let cam = *view.camera.as_ref().expect("camera set above");
    let paint = view::paint(
        &painter,
        rect,
        &cam,
        &b.tess,
        b.scene.layers.len(),
        &Labels::of(&b.scene),
        &view.visibility(),
        &view.renderer,
    );
    draw_overlays(&painter, view, rect);

    let mut out = CanvasOut {
        hover: Hover::default(),
        paint,
    };
    let Some(pos) = response.hover_pos() else {
        return out;
    };
    let p = cam.to_board(rect, pos);
    let vis = view.visibility();
    let picked = b.scene.pick(
        p,
        2.0 / cam.zoom,
        |i| vis.slot(i.slot) && vis.kind(i.kind),
        Some(view.active_layer),
    );
    out.hover = Hover {
        cursor: Some(p),
        description: picked.map(|item| describe(&b.scene, item)),
    };
    out
}

fn handle_mouse(ui: &egui::Ui, response: &egui::Response, view: &mut ViewState, rect: Rect) {
    let space = ui.input(|i| i.key_down(egui::Key::Space));
    let panning = response.dragged_by(PointerButton::Middle)
        || (space && response.dragged_by(PointerButton::Primary));
    if panning {
        let origin = ui.input(|i| i.pointer.press_origin());
        if let (Some(pos), Some(cam)) = (response.interact_pointer_pos(), view.camera.as_mut()) {
            let from = view.pan_anchor.or(origin).unwrap_or(pos);
            cam.pan(pos - from);
            view.pan_anchor = Some(pos);
            view.auto_fit = None;
        }
    } else {
        view.pan_anchor = None;
    }

    if view.zoom_rect_armed && !panning {
        if response.dragged_by(PointerButton::Primary) {
            let from = ui.input(|i| i.pointer.press_origin());
            let to = response.interact_pointer_pos();
            if let Some((from, to)) = from.zip(to) {
                view.zoom_rect_drag = Some((from, to));
            }
        } else if response.drag_stopped_by(PointerButton::Primary) {
            if let (Some((a, b)), Some(cam)) = (view.zoom_rect_drag.take(), view.camera.as_mut()) {
                cam.zoom_to_rect(rect, cam.to_board(rect, a), cam.to_board(rect, b));
                view.auto_fit = None;
            }
            view.zoom_rect_armed = false;
        }
    } else {
        view.zoom_rect_drag = None;
    }

    if let Some(hover) = response.hover_pos() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        let factor = (f64::from(scroll.y) / 200.0).exp() * f64::from(pinch);
        if factor != 1.0
            && let Some(cam) = view.camera.as_mut()
        {
            cam.zoom_at(rect, hover, factor);
            view.auto_fit = None;
        }
    }
}

fn draw_overlays(painter: &egui::Painter, view: &ViewState, rect: Rect) {
    let stroke = Stroke::new(1.0, Color32::from_white_alpha(140));
    if let Some((a, b)) = view.zoom_rect_drag {
        painter.rect_stroke(
            Rect::from_two_pos(a, b),
            0.0,
            stroke,
            egui::StrokeKind::Inside,
        );
    }
    if view.crosshair
        && let Some(c) = view.cursor
    {
        painter.hline(rect.x_range(), c.y, stroke);
        painter.vline(c.x, rect.y_range(), stroke);
    }
}

/// The board point under a screen point, for tests.
pub fn board_point(view: &ViewState, pos: Pos2) -> Option<P> {
    view.camera.map(|c| c.to_board(view.canvas, pos))
}

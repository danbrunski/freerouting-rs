//! The board canvas: tessellates a [`Scene`] once, then draws it every frame
//! through a pan/zoom camera.
//!
//! [`Tessellation`] is the CPU builder: it triangulates each scene item
//! ([`tessellate_item`]) and keeps the result in chunks
//! ([`crate::canvas::chunks`]). [`paint`] draws the chunks that are on screen,
//! either through the GPU pipeline of [`crate::canvas`] or, as a fallback
//! (`FREEROUTING_GUI_RENDERER=cpu`, or no wgpu render state), with egui
//! meshes ([`paint_cpu`]). Both paths use the same draw plan, so they draw
//! the same chunks in the same order.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use eframe::egui::{self, Color32, Mesh, Painter, Pos2, Rect, Shape, epaint::Vertex};
use freerouting::board::items::ItemId;
use freerouting::dsn::ast::Side;

use crate::canvas::chunks::{BuildTimes, ChunkSet, MeshBuf, PatchReport};
use crate::canvas::{self, GpuCounters};
use crate::look::{Look, OtherColor};
use crate::scene::{Geom, Item, Kind, Label, P, Scene, Slot};
use crate::stats::{FrameClock, PaintStats};

/// Segments used for a full circle.
const CIRCLE_SEGMENTS: usize = 24;

/// Cosine and sine of the angles of a disc's rim, computed once. They are the
/// values `f32::cos` and `f32::sin` give for the same angles, so the discs
/// are the ones the first version of the canvas drew.
fn rim() -> &'static [[f32; 2]; CIRCLE_SEGMENTS] {
    static RIM: std::sync::OnceLock<[[f32; 2]; CIRCLE_SEGMENTS]> = std::sync::OnceLock::new();
    RIM.get_or_init(|| {
        std::array::from_fn(|i| {
            let a = i as f32 * std::f32::consts::TAU / CIRCLE_SEGMENTS as f32;
            [a.cos(), a.sin()]
        })
    })
}

/// Appends the triangles (or, for hairlines, segments) of one item to a
/// buffer, in millimetres relative to `origin`. Indices count from the
/// buffer's vertex count at the call, so the caller shifts them to the
/// buffer's start.
struct Builder<'a> {
    out: &'a mut MeshBuf,
    first: usize,
}

impl Builder<'_> {
    fn push(&mut self, v: [f32; 2]) -> u32 {
        self.out.vertices.push(v);
        (self.out.vertices.len() - 1 - self.first) as u32
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.out.indices.extend([a, b, c]);
    }

    fn disc(&mut self, c: [f32; 2], r: f32) {
        let center = self.push(c);
        let first = center + 1;
        for [cos, sin] in rim() {
            self.push([c[0] + r * cos, c[1] + r * sin]);
        }
        let n = CIRCLE_SEGMENTS as u32;
        for i in 0..n {
            self.triangle(center, first + i, first + (i + 1) % n);
        }
    }

    fn segment(&mut self, a: [f32; 2], b: [f32; 2], r: f32) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy);
        if len == 0.0 {
            return;
        }
        let (nx, ny) = (-dy / len * r, dx / len * r);
        let i0 = self.push([a[0] + nx, a[1] + ny]);
        let i1 = self.push([b[0] + nx, b[1] + ny]);
        let i2 = self.push([b[0] - nx, b[1] - ny]);
        let i3 = self.push([a[0] - nx, a[1] - ny]);
        self.triangle(i0, i1, i2);
        self.triangle(i0, i2, i3);
    }

    fn polygon(&mut self, outer: &[[f32; 2]], holes: &[Vec<[f32; 2]>]) {
        let mut flat: Vec<f64> = Vec::new();
        let mut hole_starts = Vec::new();
        for p in outer {
            flat.extend([f64::from(p[0]), f64::from(p[1])]);
        }
        for hole in holes {
            hole_starts.push(flat.len() / 2);
            for p in hole {
                flat.extend([f64::from(p[0]), f64::from(p[1])]);
            }
        }
        let Ok(tris) = earcutr::earcut(&flat, &hole_starts, 2) else {
            return;
        };
        let base = (self.out.vertices.len() - self.first) as u32;
        for p in flat.chunks_exact(2) {
            self.out.vertices.push([p[0] as f32, p[1] as f32]);
        }
        self.out
            .indices
            .extend(tris.iter().map(|&i| base + i as u32));
    }

    /// A hairline through `points`: one segment per pair of points.
    fn line(&mut self, points: &[[f32; 2]], width: f32) {
        match points {
            [] => {}
            [only] => self
                .out
                .lines
                .push([only[0], only[1], only[0], only[1], width]),
            _ => {
                for s in points.windows(2) {
                    self.out
                        .lines
                        .push([s[0][0], s[0][1], s[1][0], s[1][1], width]);
                }
            }
        }
    }
}

/// Appends the geometry of `item` to `out`: triangles for filled shapes,
/// hairline segments for the board and footprint outlines and for
/// zero-width paths. Vertices are in millimetres relative to `origin`.
/// Indices count from the first vertex this call adds.
pub fn tessellate_item(origin: P, item: &Item, out: &mut MeshBuf) {
    let rel = |p: &P| [(p.x - origin.x) as f32, (p.y - origin.y) as f32];
    let first = out.vertices.len();
    let mut b = Builder { out, first };
    let is_line = matches!(item.kind, Kind::Outline | Kind::ComponentOutline)
        || matches!(item.geom, Geom::Path { width, .. } if width == 0.0);
    if is_line {
        let (points, width): (Vec<[f32; 2]>, f64) = match &item.geom {
            Geom::Path { points, width } => (points.iter().map(rel).collect(), *width),
            Geom::Circle { center, radius } => (
                (0..=CIRCLE_SEGMENTS * 2)
                    .map(|i| {
                        let a = i as f64 * std::f64::consts::TAU / (CIRCLE_SEGMENTS * 2) as f64;
                        rel(&P::new(
                            center.x + radius * a.cos(),
                            center.y + radius * a.sin(),
                        ))
                    })
                    .collect(),
                0.0,
            ),
            Geom::Polygon { outer, .. } => {
                let mut pts: Vec<_> = outer.iter().map(rel).collect();
                if let Some(&p) = pts.first() {
                    pts.push(p);
                }
                (pts, 0.0)
            }
        };
        b.line(&points, width as f32);
        return;
    }
    match &item.geom {
        Geom::Circle { center, radius } => b.disc(rel(center), *radius as f32),
        Geom::Path { points, width } => {
            let r = *width as f32 / 2.0;
            let pts: Vec<_> = points.iter().map(rel).collect();
            for &p in &pts {
                b.disc(p, r);
            }
            for s in pts.windows(2) {
                b.segment(s[0], s[1], r);
            }
        }
        Geom::Polygon { outer, holes } => {
            let outer: Vec<_> = outer.iter().map(rel).collect();
            let holes: Vec<Vec<_>> = holes.iter().map(|h| h.iter().map(rel).collect()).collect();
            b.polygon(&outer, &holes);
        }
    }
}

/// The scene, ready to draw: its chunks, shared with the GPU callback that
/// uploads and draws them.
pub struct Tessellation {
    chunks: Arc<Mutex<ChunkSet>>,
    counters: Arc<GpuCounters>,
    times: BuildTimes,
}

impl Tessellation {
    /// Triangulates and chunks the scene (the full build).
    pub fn new(scene: &Scene) -> Tessellation {
        let set = ChunkSet::build(scene, scene.bounds.center());
        let times = set.times;
        Tessellation {
            chunks: Arc::new(Mutex::new(set)),
            counters: Arc::new(GpuCounters::default()),
            times,
        }
    }

    fn lock(&self) -> MutexGuard<'_, ChunkSet> {
        self.chunks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads the chunks.
    pub fn with<R>(&self, f: impl FnOnce(&ChunkSet) -> R) -> R {
        f(&self.lock())
    }

    pub fn triangle_count(&self) -> usize {
        self.with(ChunkSet::triangle_count)
    }

    /// Removes the scene items of the engine items `removed` and adds
    /// `added` (flattened with [`crate::scene::Flattener`]), rebuilding only
    /// the chunks they touch. The GPU uploads just those on the next frame.
    pub fn patch(&self, removed: &[ItemId], added: &[Item]) -> PatchReport {
        self.lock().patch(removed, added)
    }

    /// The GPU-side counters of this board.
    pub fn counters(&self) -> &Arc<GpuCounters> {
        &self.counters
    }

    pub(crate) fn shared(&self) -> &Arc<Mutex<ChunkSet>> {
        &self.chunks
    }

    /// How long the grouping and the triangulation of the full build took.
    pub fn build_times(&self) -> BuildTimes {
        self.times
    }
}

/// Mirroring of the displayed board (Java `ScreenTransform`: left and right,
/// or top and bottom, are swapped; the two are exclusive in the window).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mirror {
    #[default]
    None,
    LeftRight,
    TopBottom,
}

/// How the board is turned on the screen: a rotation by quarter turns
/// (counterclockwise, as Java's `setRotation(0.5 * Math.PI)` is), then
/// mirroring. Board coordinates are never rotated; only the map from board to
/// screen is (G04.6), so picking and hovering use the inverse of this.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Orientation {
    /// 0, 1, 2 or 3 quarter turns.
    pub quarter_turns: u8,
    pub mirror: Mirror,
}

impl Orientation {
    /// `[a, b, c, d]` with `screen = zoom * (a dx + b dy, c dx + d dy)` for a
    /// board offset `(dx, dy)` (Y up) from the view centre, and screen Y down.
    /// All four are 0, 1 or -1, so the map is exact.
    pub fn basis(self) -> [i32; 4] {
        let (c, s) = [(1, 0), (0, 1), (-1, 0), (0, -1)][usize::from(self.quarter_turns % 4)];
        let fx = if self.mirror == Mirror::LeftRight {
            -1
        } else {
            1
        };
        let fy = if self.mirror == Mirror::TopBottom {
            -1
        } else {
            1
        };
        [fx * c, -fx * s, -fy * s, -fy * c]
    }

    /// True if the board's x axis runs up or down on the screen.
    pub fn swaps_axes(self) -> bool {
        self.quarter_turns % 2 == 1
    }

    /// A board offset as a screen offset, in board units (not yet scaled by
    /// the zoom).
    fn to_screen_delta(self, dx: f64, dy: f64) -> (f64, f64) {
        let [a, b, c, d] = self.basis().map(f64::from);
        (a * dx + b * dy, c * dx + d * dy)
    }

    /// A screen offset (already divided by the zoom) as a board offset.
    fn to_board_delta(self, u: f64, v: f64) -> (f64, f64) {
        // The basis is orthogonal, so the inverse is the transpose.
        let [a, b, c, d] = self.basis().map(f64::from);
        (a * u + c * v, b * u + d * v)
    }

    /// The half extent on the board of a screen box of half sizes `hw`, `hh`.
    fn board_extent(self, hw: f64, hh: f64) -> (f64, f64) {
        let [a, b, c, d] = self.basis().map(|v| f64::from(v.abs()));
        (a * hw + c * hh, b * hw + d * hh)
    }

    /// The extent on the screen of a board box of sides `w`, `h`.
    fn screen_extent(self, w: f64, h: f64) -> (f64, f64) {
        let [a, b, c, d] = self.basis().map(|v| f64::from(v.abs()));
        (a * w + b * h, c * w + d * h)
    }
}

/// Pan and zoom.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// The board point at the center of the canvas, in millimetres.
    pub center: P,
    /// Pixels per millimetre.
    pub zoom: f64,
    /// Rotation and mirroring about the centre.
    pub orientation: Orientation,
}

impl Camera {
    /// A camera looking at `center` at `zoom`, not rotated.
    pub fn new(center: P, zoom: f64) -> Camera {
        Camera {
            center,
            zoom,
            orientation: Orientation::default(),
        }
    }

    pub fn fit(scene: &Scene, rect: Rect) -> Camera {
        Camera::fit_oriented(scene, rect, Orientation::default())
    }

    /// The camera that fits the board in `rect` turned as `orientation`.
    pub fn fit_oriented(scene: &Scene, rect: Rect, orientation: Orientation) -> Camera {
        let b = if scene.bounds.is_empty() {
            crate::scene::Bounds {
                min: P::new(-10.0, -10.0),
                max: P::new(10.0, 10.0),
            }
        } else {
            scene.bounds
        };
        let (w, h) = orientation.screen_extent(b.width().max(1e-3), b.height().max(1e-3));
        let zoom = (f64::from(rect.width()) / w).min(f64::from(rect.height()) / h) * 0.92;
        Camera {
            center: b.center(),
            zoom,
            orientation,
        }
    }

    pub fn to_screen(self, rect: Rect, p: P) -> Pos2 {
        let c = rect.center();
        let (sx, sy) = self
            .orientation
            .to_screen_delta(p.x - self.center.x, p.y - self.center.y);
        Pos2::new(c.x + (sx * self.zoom) as f32, c.y + (sy * self.zoom) as f32)
    }

    pub fn to_board(self, rect: Rect, s: Pos2) -> P {
        let c = rect.center();
        let u = f64::from(s.x - c.x) / self.zoom;
        let v = f64::from(s.y - c.y) / self.zoom;
        let (dx, dy) = self.orientation.to_board_delta(u, v);
        P::new(self.center.x + dx, self.center.y + dy)
    }

    /// Zooms by `factor`, keeping the board point under `anchor` in place.
    pub fn zoom_at(&mut self, rect: Rect, anchor: Pos2, factor: f64) {
        let before = self.to_board(rect, anchor);
        self.zoom = (self.zoom * factor).clamp(1e-3, 1e6);
        let after = self.to_board(rect, anchor);
        self.center.x += before.x - after.x;
        self.center.y += before.y - after.y;
    }

    /// Moves the view by `delta` screen points (the board follows the mouse).
    pub fn pan(&mut self, delta: egui::Vec2) {
        let (dx, dy) = self.orientation.to_board_delta(
            f64::from(delta.x) / self.zoom,
            f64::from(delta.y) / self.zoom,
        );
        self.center.x -= dx;
        self.center.y -= dy;
    }

    /// Centres the view on the rectangle with corners `a` and `b` and zooms
    /// so that it just fits in `rect` (Java: `BoardPanel.zoomFrame`).
    pub fn zoom_to_rect(&mut self, rect: Rect, a: P, b: P) {
        let (w, h) = self
            .orientation
            .screen_extent((a.x - b.x).abs(), (a.y - b.y).abs());
        let (w, h) = (w.max(1e-9), h.max(1e-9));
        let zoom = (f64::from(rect.width()) / w).min(f64::from(rect.height()) / h);
        self.zoom = zoom.clamp(1e-3, 1e6);
        self.center = P::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    }
}

/// What to draw.
pub struct Visibility<'a> {
    pub layers: &'a [bool],
    pub kinds: &'a [bool],
    pub active: usize,
    /// Reference designators.
    pub labels: bool,
    /// Colours, levels, dimming and the virtual layers.
    pub look: &'a Look,
}

impl Visibility<'_> {
    pub fn kind(&self, kind: Kind) -> bool {
        self.kinds[Kind::ALL.iter().position(|&k| k == kind).unwrap()]
            && self.look.kind_visible(kind)
    }

    pub fn slot(&self, slot: Slot) -> bool {
        let checked = match slot {
            Slot::Layer(i) => self.layers.get(i).copied().unwrap_or(false),
            _ => true,
        };
        checked && self.look.slot_visible(slot)
    }
}

pub use crate::look::{BACKGROUND, layer_color};

/// Which painter draws the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderPath {
    /// The wgpu pipeline of [`crate::canvas`].
    Gpu,
    /// egui meshes, built on the CPU every frame.
    Cpu,
}

/// How the canvas is drawn this frame.
#[derive(Clone)]
pub struct Renderer {
    pub path: RenderPath,
    /// The format of the render target, while a wgpu render state exists.
    pub target_format: Option<eframe::wgpu::TextureFormat>,
    /// Skip chunks that are off screen. Always on; the environment variable
    /// `FREEROUTING_GUI_CULL=0` turns it off to measure what culling saves.
    pub cull: bool,
    pub clock: Arc<FrameClock>,
    forced_cpu: bool,
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer {
            path: RenderPath::Cpu,
            target_format: None,
            cull: std::env::var("FREEROUTING_GUI_CULL").map_or(true, |v| v != "0"),
            clock: Arc::new(FrameClock::default()),
            forced_cpu: std::env::var("FREEROUTING_GUI_RENDERER").is_ok_and(|v| v == "cpu"),
        }
    }
}

impl Renderer {
    /// Draws with the CPU painter even where a wgpu render state exists, as
    /// `FREEROUTING_GUI_RENDERER=cpu` does.
    pub fn force_cpu(&mut self, on: bool) {
        self.forced_cpu = on;
    }

    /// Picks the path for the coming frame: the GPU one when eframe gave a
    /// wgpu render state and `FREEROUTING_GUI_RENDERER` is not `cpu`.
    pub fn select(&mut self, target_format: Option<eframe::wgpu::TextureFormat>) {
        self.target_format = target_format;
        self.path = if target_format.is_some() && !self.forced_cpu {
            RenderPath::Gpu
        } else {
            RenderPath::Cpu
        };
    }
}

/// The board point range on screen, with a margin of a few points for line
/// widths.
fn view_box(camera: &Camera, rect: Rect) -> crate::scene::Bounds {
    const MARGIN_PT: f64 = 2.0;
    let hw = f64::from(rect.width()) / 2.0 / camera.zoom + MARGIN_PT / camera.zoom;
    let hh = f64::from(rect.height()) / 2.0 / camera.zoom + MARGIN_PT / camera.zoom;
    let (hw, hh) = camera.orientation.board_extent(hw, hh);
    crate::scene::Bounds {
        min: P::new(camera.center.x - hw, camera.center.y - hh),
        max: P::new(camera.center.x + hw, camera.center.y + hh),
    }
}

/// The text drawn over the board: reference designators and part numbers.
#[derive(Clone, Copy, Default)]
pub struct Labels<'a> {
    pub designators: &'a [Label],
    pub part_numbers: &'a [Label],
}

impl<'a> Labels<'a> {
    pub fn of(scene: &'a Scene) -> Labels<'a> {
        Labels {
            designators: &scene.labels,
            part_numbers: &scene.part_numbers,
        }
    }

    /// Designators only, for callers that draw no part numbers.
    pub fn designators(designators: &'a [Label]) -> Labels<'a> {
        Labels {
            designators,
            part_numbers: &[],
        }
    }
}

/// Draws the board and reports what it drew.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    tess: &Tessellation,
    layer_count: usize,
    labels: &Labels,
    vis: &Visibility,
    renderer: &Renderer,
) -> PaintStats {
    painter.rect_filled(rect, 0.0, vis.look.background());
    let view = view_box(camera, rect);
    let (plan, origin) =
        tess.with(|set| (canvas::plan(set, vis, &view, renderer.cull), set.origin()));
    let colors = canvas::color_table(vis.look, layer_count, vis.active);
    let mut stats = PaintStats {
        triangles: plan.triangles,
        draws: plan.draws.len() as u64,
        chunks_drawn: plan.chunks_drawn,
        chunks_total: plan.chunks_total,
        ..PaintStats::default()
    };
    match renderer.path {
        RenderPath::Gpu => {
            let (pending, buffer_bytes) = tess.counters().take_frame();
            stats.uploaded_bytes = pending;
            stats.gpu_buffer_bytes = buffer_bytes;
            canvas::add_board_callback(
                painter, rect, camera, tess, origin, plan, &colors, renderer,
            );
        }
        RenderPath::Cpu => {
            stats.uploaded_bytes =
                tess.with(|set| paint_cpu(painter, rect, camera, set, &plan, &colors));
            canvas::add_clock_marker(painter, rect, renderer);
        }
    }
    if vis.labels {
        paint_labels(painter, rect, camera, labels.designators);
    }
    paint_part_numbers(painter, rect, camera, labels.part_numbers, vis.look);
    stats
}

/// The fallback painter: one egui mesh per chunk draw, with the vertices
/// moved to the screen on the CPU. Hairline segments become quads of at
/// least one point, as in the shader. Returns the bytes handed to egui.
pub fn paint_cpu(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    set: &ChunkSet,
    plan: &canvas::Plan,
    colors: &[Color32],
) -> u64 {
    let c = rect.center();
    let z = camera.zoom as f32;
    let origin = set.origin();
    let ox = (origin.x - camera.center.x) as f32;
    let oy = (origin.y - camera.center.y) as f32;
    let [ba, bb, bc, bd] = camera.orientation.basis().map(|v| v as f32);
    let to_screen = |v: [f32; 2]| {
        let (dx, dy) = (v[0] + ox, v[1] + oy);
        Pos2::new(c.x + (ba * dx + bb * dy) * z, c.y + (bc * dx + bd * dy) * z)
    };
    let mut bytes = 0u64;
    for draw in &plan.draws {
        let chunk = set.chunk(draw.chunk);
        let color = colors[draw.color as usize];
        let mut mesh = Mesh::default();
        let vertex = |pos: Pos2| Vertex {
            pos,
            uv: egui::epaint::WHITE_UV,
            color,
        };
        if draw.lines {
            for &[ax, ay, bx, by, width] in &chunk.mesh.lines {
                let (a, b) = (to_screen([ax, ay]), to_screen([bx, by]));
                let d = b - a;
                let len = d.length();
                let dir = if len > 1e-6 {
                    d / len
                } else {
                    egui::vec2(1.0, 0.0)
                };
                let half = 0.5 * (width * z).max(1.0);
                let n = egui::vec2(-dir.y, dir.x) * half;
                let (p0, p1) = (a - dir * half, b + dir * half);
                let base = mesh.vertices.len() as u32;
                mesh.vertices.extend([
                    vertex(p0 - n),
                    vertex(p0 + n),
                    vertex(p1 - n),
                    vertex(p1 + n),
                ]);
                mesh.indices
                    .extend([base, base + 1, base + 2, base + 2, base + 1, base + 3]);
            }
        } else {
            mesh.indices.clone_from(&chunk.mesh.indices);
            mesh.vertices = chunk
                .mesh
                .vertices
                .iter()
                .map(|&v| vertex(to_screen(v)))
                .collect();
        }
        bytes += (mesh.vertices.len() * 20 + mesh.indices.len() * 4) as u64;
        painter.add(Shape::mesh(mesh));
    }
    bytes
}

/// Draws reference designators, back side first and dimmer. Labels smaller
/// than a few pixels are left out. The text stays upright and unmirrored
/// whatever the orientation; a label that reads along the tall side of a
/// footprint turns when a quarter turn swaps the sides.
fn paint_labels(painter: &Painter, rect: Rect, camera: &Camera, labels: &[Label]) {
    for side in [Side::Back, Side::Front] {
        let color = match side {
            Side::Front => Color32::from_rgb(232, 232, 232),
            Side::Back => Color32::from_rgb(140, 155, 190),
        };
        draw_labels(painter, rect, camera, labels, side, color);
    }
}

/// Part numbers on the fabrication layers (Java `drawComponentPartNumbers`):
/// the colour of the fabrication layer of the side, at its level, and only
/// while that layer is on.
fn paint_part_numbers(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    labels: &[Label],
    look: &Look,
) {
    for (side, index, color) in [
        (Side::Back, 5, OtherColor::FabBack),
        (Side::Front, 4, OtherColor::FabFront),
    ] {
        if look.virtual_on(index) {
            let color = look.overlay_color(color, crate::look::Object::ComponentOutlines);
            draw_labels(painter, rect, camera, labels, side, color);
        }
    }
}

fn draw_labels(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    labels: &[Label],
    side: Side,
    color: Color32,
) {
    const MIN_PX: f32 = 6.0;
    let view = rect.expand(50.0);
    for label in labels.iter().filter(|l| l.side == side) {
        let px = (label.height * camera.zoom) as f32;
        let center = camera.to_screen(rect, label.center);
        if px < MIN_PX || !view.contains(center) {
            continue;
        }
        let galley = painter.layout_no_wrap(label.text.clone(), egui::FontId::monospace(px), color);
        let size = galley.size();
        let vertical = label.vertical != camera.orientation.swaps_axes();
        let (angle, pos) = if vertical {
            // Rotated a quarter turn counterclockwise about the top-left
            // corner of the text, so the text centre lands on `center`.
            (
                -std::f32::consts::FRAC_PI_2,
                center + egui::vec2(-size.y / 2.0, size.x / 2.0),
            )
        } else {
            (0.0, center - size / 2.0)
        };
        painter.add(egui::epaint::TextShape::new(pos, galley, color).with_angle(angle));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_round_trip_and_zoom_anchor() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut cam = Camera::new(P::new(10.0, 20.0), 4.0);
        let s = cam.to_screen(rect, P::new(12.0, 21.0));
        assert_eq!(s, Pos2::new(408.0, 296.0));
        let anchor = Pos2::new(100.0, 50.0);
        let before = cam.to_board(rect, anchor);
        cam.zoom_at(rect, anchor, 2.5);
        let after = cam.to_board(rect, anchor);
        assert!((before.x - after.x).abs() < 1e-9 && (before.y - after.y).abs() < 1e-9);
    }

    /// The eight orientations map a board offset to the screen as Java's
    /// `ScreenTransform` does: a rotation of the board counterclockwise by
    /// quarter turns, then the mirroring, Y down on the screen.
    #[test]
    fn orientations_follow_java_screen_transform() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 200.0));
        let to = |turns, mirror, p: P| {
            let mut cam = Camera::new(P::new(0.0, 0.0), 10.0);
            cam.orientation = Orientation {
                quarter_turns: turns,
                mirror,
            };
            let s = cam.to_screen(rect, p);
            (s.x - 100.0, s.y - 100.0)
        };
        let east = P::new(1.0, 0.0);
        let north = P::new(0.0, 1.0);
        // No turn: east is right, north is up (smaller screen y).
        assert_eq!(to(0, Mirror::None, east), (10.0, 0.0));
        assert_eq!(to(0, Mirror::None, north), (0.0, -10.0));
        // Rotation 90: the board turns counterclockwise, east becomes north.
        assert_eq!(to(1, Mirror::None, east), (0.0, -10.0));
        assert_eq!(to(1, Mirror::None, north), (-10.0, 0.0));
        assert_eq!(to(2, Mirror::None, east), (-10.0, 0.0));
        assert_eq!(to(3, Mirror::None, east), (0.0, 10.0));
        assert_eq!(to(3, Mirror::None, north), (10.0, 0.0));
        // Mirrored left to right: east is left; top to bottom: north is down.
        assert_eq!(to(0, Mirror::LeftRight, east), (-10.0, 0.0));
        assert_eq!(to(0, Mirror::LeftRight, north), (0.0, -10.0));
        assert_eq!(to(0, Mirror::TopBottom, east), (10.0, 0.0));
        assert_eq!(to(0, Mirror::TopBottom, north), (0.0, 10.0));
        // The mirroring comes after the rotation, as in Java.
        assert_eq!(to(1, Mirror::LeftRight, east), (0.0, -10.0));
        assert_eq!(to(1, Mirror::TopBottom, east), (0.0, 10.0));
    }

    #[test]
    fn every_orientation_round_trips_and_pans_with_the_mouse() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        for turns in 0..4 {
            for mirror in [Mirror::None, Mirror::LeftRight, Mirror::TopBottom] {
                let mut cam = Camera::new(P::new(12.5, -3.0), 7.5);
                cam.orientation = Orientation {
                    quarter_turns: turns,
                    mirror,
                };
                let p = P::new(14.25, 1.5);
                let back = cam.to_board(rect, cam.to_screen(rect, p));
                assert!((back.x - p.x).abs() < 1e-4 && (back.y - p.y).abs() < 1e-4);
                // The board follows the mouse by the distance it moved.
                let before = cam.to_screen(rect, p);
                cam.pan(egui::vec2(30.0, -20.0));
                let after = cam.to_screen(rect, p);
                assert!(
                    (after.x - before.x - 30.0).abs() < 1e-3,
                    "{turns} {mirror:?}"
                );
                assert!(
                    (after.y - before.y + 20.0).abs() < 1e-3,
                    "{turns} {mirror:?}"
                );
                // Zooming keeps the point under the cursor.
                let anchor = Pos2::new(123.0, 456.0);
                let under = cam.to_board(rect, anchor);
                cam.zoom_at(rect, anchor, 3.0);
                let still = cam.to_board(rect, anchor);
                assert!((under.x - still.x).abs() < 1e-9 && (under.y - still.y).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn a_quarter_turn_swaps_the_fitted_sides() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 500.0));
        let scene = Scene {
            bounds: crate::scene::Bounds {
                min: P::new(0.0, 0.0),
                max: P::new(100.0, 10.0),
            },
            ..Scene::default()
        };
        let flat = Camera::fit(&scene, rect);
        let turned = Camera::fit_oriented(
            &scene,
            rect,
            Orientation {
                quarter_turns: 1,
                mirror: Mirror::None,
            },
        );
        // A long board lies across the canvas; turned, it stands up and
        // must be smaller to fit the height.
        assert!((flat.zoom - 1000.0 / 100.0 * 0.92).abs() < 1e-9);
        assert!((turned.zoom - 500.0 / 100.0 * 0.92).abs() < 1e-9);
        // A zoom rectangle fits the screen extent of the box.
        let mut cam = turned;
        cam.zoom_to_rect(rect, P::new(0.0, 0.0), P::new(100.0, 10.0));
        assert!((cam.zoom - 5.0).abs() < 1e-9);
    }

    /// Every fixture board under 50 KB (the larger ones are slow to build in
    /// a debug build).
    #[test]
    fn tessellates_fixture_boards() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");
        let mut boards = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let small = std::fs::metadata(&path).is_ok_and(|m| m.len() < 50_000);
            if path.extension().is_none_or(|e| e != "dsn") || !small {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let result = freerouting::io::specctra::read_board(&bytes);
            let Some(board) = result.board() else {
                continue;
            };
            let scene = Scene::from_board("", board);
            let tess = Tessellation::new(&scene);
            tess.with(|set| {
                for c in set.chunks() {
                    let m = &c.mesh;
                    assert!(
                        m.indices.iter().all(|&i| (i as usize) < m.vertices.len()),
                        "{}",
                        path.display()
                    );
                }
            });
            boards += 1;
        }
        assert!(boards > 20, "only {boards} boards read");
    }
}

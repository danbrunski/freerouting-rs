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

/// Pan and zoom.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// The board point at the center of the canvas, in millimetres.
    pub center: P,
    /// Pixels per millimetre.
    pub zoom: f64,
}

impl Camera {
    pub fn fit(scene: &Scene, rect: Rect) -> Camera {
        let b = if scene.bounds.is_empty() {
            crate::scene::Bounds {
                min: P::new(-10.0, -10.0),
                max: P::new(10.0, 10.0),
            }
        } else {
            scene.bounds
        };
        let w = b.width().max(1e-3);
        let h = b.height().max(1e-3);
        let zoom = (f64::from(rect.width()) / w).min(f64::from(rect.height()) / h) * 0.92;
        Camera {
            center: b.center(),
            zoom,
        }
    }

    pub fn to_screen(self, rect: Rect, p: P) -> Pos2 {
        let c = rect.center();
        Pos2::new(
            c.x + ((p.x - self.center.x) * self.zoom) as f32,
            c.y - ((p.y - self.center.y) * self.zoom) as f32,
        )
    }

    pub fn to_board(self, rect: Rect, s: Pos2) -> P {
        let c = rect.center();
        P::new(
            self.center.x + f64::from(s.x - c.x) / self.zoom,
            self.center.y - f64::from(s.y - c.y) / self.zoom,
        )
    }

    /// Zooms by `factor`, keeping the board point under `anchor` in place.
    pub fn zoom_at(&mut self, rect: Rect, anchor: Pos2, factor: f64) {
        let before = self.to_board(rect, anchor);
        self.zoom = (self.zoom * factor).clamp(1e-3, 1e6);
        let after = self.to_board(rect, anchor);
        self.center.x += before.x - after.x;
        self.center.y += before.y - after.y;
    }

    pub fn pan(&mut self, delta: egui::Vec2) {
        self.center.x -= f64::from(delta.x) / self.zoom;
        self.center.y += f64::from(delta.y) / self.zoom;
    }

    /// Centres the view on the rectangle with corners `a` and `b` and zooms
    /// so that it just fits in `rect` (Java: `BoardPanel.zoomFrame`).
    pub fn zoom_to_rect(&mut self, rect: Rect, a: P, b: P) {
        let w = (a.x - b.x).abs().max(1e-9);
        let h = (a.y - b.y).abs().max(1e-9);
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
}

impl Visibility<'_> {
    pub fn kind(&self, kind: Kind) -> bool {
        self.kinds[Kind::ALL.iter().position(|&k| k == kind).unwrap()]
    }

    pub fn slot(&self, slot: Slot) -> bool {
        match slot {
            Slot::Layer(i) => self.layers.get(i).copied().unwrap_or(false),
            Slot::Board | Slot::Silk(_) => true,
        }
    }
}

/// The color of copper layer `index` of `count`: red for the top, blue for
/// the bottom, as most PCB tools draw them.
pub fn layer_color(index: usize, count: usize) -> Color32 {
    const INNER: [Color32; 8] = [
        Color32::from_rgb(127, 200, 127),
        Color32::from_rgb(206, 125, 44),
        Color32::from_rgb(79, 203, 203),
        Color32::from_rgb(219, 98, 139),
        Color32::from_rgb(167, 165, 198),
        Color32::from_rgb(40, 204, 217),
        Color32::from_rgb(232, 178, 167),
        Color32::from_rgb(242, 237, 161),
    ];
    if index == 0 {
        Color32::from_rgb(200, 52, 52)
    } else if index + 1 == count {
        Color32::from_rgb(77, 127, 196)
    } else {
        INNER[(index - 1) % INNER.len()]
    }
}

pub(crate) fn kind_color(kind: Kind, slot: Slot, layer_count: usize) -> Color32 {
    let layer = match slot {
        Slot::Layer(i) => layer_color(i, layer_count),
        _ => Color32::GRAY,
    };
    match kind {
        Kind::Trace => layer,
        Kind::Pad => layer.lerp_to_gamma(Color32::WHITE, 0.15),
        Kind::Via => Color32::from_rgb(212, 196, 120),
        Kind::Plane => layer.gamma_multiply(0.35),
        Kind::Keepout => Color32::from_rgb(150, 110, 200).gamma_multiply(0.3),
        Kind::ViaKeepout => Color32::from_rgb(150, 150, 150).gamma_multiply(0.25),
        Kind::PlaceKeepout => Color32::from_rgb(120, 160, 120).gamma_multiply(0.2),
        Kind::Outline => Color32::from_rgb(222, 214, 120),
        Kind::ComponentOutline => match slot {
            Slot::Silk(Side::Back) => Color32::from_rgb(110, 125, 160),
            _ => Color32::from_rgb(190, 190, 190),
        },
    }
}

pub const BACKGROUND: Color32 = Color32::from_rgb(12, 14, 18);

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
    crate::scene::Bounds {
        min: P::new(camera.center.x - hw, camera.center.y - hh),
        max: P::new(camera.center.x + hw, camera.center.y + hh),
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
    labels: &[Label],
    vis: &Visibility,
    renderer: &Renderer,
) -> PaintStats {
    painter.rect_filled(rect, 0.0, BACKGROUND);
    let view = view_box(camera, rect);
    let (plan, origin) =
        tess.with(|set| (canvas::plan(set, vis, &view, renderer.cull), set.origin()));
    let colors = canvas::color_table(layer_count, vis.active);
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
        paint_labels(painter, rect, camera, labels);
    }
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
    let to_screen = |v: [f32; 2]| Pos2::new(c.x + (v[0] + ox) * z, c.y - (v[1] + oy) * z);
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
/// than a few pixels are left out.
fn paint_labels(painter: &Painter, rect: Rect, camera: &Camera, labels: &[Label]) {
    const MIN_PX: f32 = 6.0;
    let view = rect.expand(50.0);
    for side in [Side::Back, Side::Front] {
        let color = match side {
            Side::Front => Color32::from_rgb(232, 232, 232),
            Side::Back => Color32::from_rgb(140, 155, 190),
        };
        for label in labels.iter().filter(|l| l.side == side) {
            let px = (label.height * camera.zoom) as f32;
            let center = camera.to_screen(rect, label.center);
            if px < MIN_PX || !view.contains(center) {
                continue;
            }
            let galley =
                painter.layout_no_wrap(label.text.clone(), egui::FontId::monospace(px), color);
            let size = galley.size();
            let (angle, pos) = if label.vertical {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_round_trip_and_zoom_anchor() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut cam = Camera {
            center: P::new(10.0, 20.0),
            zoom: 4.0,
        };
        let s = cam.to_screen(rect, P::new(12.0, 21.0));
        assert_eq!(s, Pos2::new(408.0, 296.0));
        let anchor = Pos2::new(100.0, 50.0);
        let before = cam.to_board(rect, anchor);
        cam.zoom_at(rect, anchor, 2.5);
        let after = cam.to_board(rect, anchor);
        assert!((before.x - after.x).abs() < 1e-9 && (before.y - after.y).abs() < 1e-9);
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

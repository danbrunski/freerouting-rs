//! The board canvas: tessellates a [`Scene`] once, then draws it every frame
//! through a pan/zoom camera.

use eframe::egui::{self, Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, epaint::Vertex};
use freerouting::dsn::ast::Side;

use crate::scene::{Geom, Kind, P, Scene, Slot};

/// Segments used for a full circle.
const CIRCLE_SEGMENTS: usize = 24;

/// Triangles of one kind of item on one slot, in millimetres relative to
/// [`Tessellation::origin`]. `f32` is plenty once the large board offset is
/// taken out.
#[derive(Default)]
struct Batch {
    slot: Option<Slot>,
    kind: Option<Kind>,
    vertices: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Batch {
    fn push(&mut self, v: [f32; 2]) -> u32 {
        self.vertices.push(v);
        (self.vertices.len() - 1) as u32
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend([a, b, c]);
    }

    fn disc(&mut self, c: [f32; 2], r: f32) {
        let center = self.push(c);
        let first = self.vertices.len() as u32;
        for i in 0..CIRCLE_SEGMENTS {
            let a = i as f32 * std::f32::consts::TAU / CIRCLE_SEGMENTS as f32;
            self.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
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
        let base = self.vertices.len() as u32;
        for p in flat.chunks_exact(2) {
            self.vertices.push([p[0] as f32, p[1] as f32]);
        }
        self.indices.extend(tris.iter().map(|&i| base + i as u32));
    }
}

/// A line drawn with a stroke of at least one pixel: board and footprint
/// outlines, and zero-width paths.
struct Line {
    slot: Slot,
    kind: Kind,
    points: Vec<[f32; 2]>,
    width: f32,
}

/// The scene, ready to draw.
pub struct Tessellation {
    /// The point in millimetres that vertex coordinates are relative to.
    origin: P,
    batches: Vec<Batch>,
    lines: Vec<Line>,
}

impl Tessellation {
    pub fn new(scene: &Scene) -> Tessellation {
        let origin = scene.bounds.center();
        let rel = |p: &P| [(p.x - origin.x) as f32, (p.y - origin.y) as f32];
        let mut batches: Vec<Batch> = Vec::new();
        let mut lines = Vec::new();
        let mut batch_of = std::collections::HashMap::new();
        for item in &scene.items {
            let is_line = matches!(item.kind, Kind::Outline | Kind::ComponentOutline)
                || matches!(item.geom, Geom::Path { width, .. } if width == 0.0);
            if is_line {
                let (points, width) = match &item.geom {
                    Geom::Path { points, width } => (points.iter().map(rel).collect(), *width),
                    Geom::Circle { center, radius } => (
                        (0..=CIRCLE_SEGMENTS * 2)
                            .map(|i| {
                                let a =
                                    i as f64 * std::f64::consts::TAU / (CIRCLE_SEGMENTS * 2) as f64;
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
                        pts.push(pts[0]);
                        (pts, 0.0)
                    }
                };
                lines.push(Line {
                    slot: item.slot,
                    kind: item.kind,
                    points,
                    width: width as f32,
                });
                continue;
            }
            let i = *batch_of.entry((item.slot, item.kind)).or_insert_with(|| {
                batches.push(Batch {
                    slot: Some(item.slot),
                    kind: Some(item.kind),
                    ..Batch::default()
                });
                batches.len() - 1
            });
            let batch = &mut batches[i];
            match &item.geom {
                Geom::Circle { center, radius } => batch.disc(rel(center), *radius as f32),
                Geom::Path { points, width } => {
                    let r = *width as f32 / 2.0;
                    let pts: Vec<_> = points.iter().map(rel).collect();
                    for &p in &pts {
                        batch.disc(p, r);
                    }
                    for s in pts.windows(2) {
                        batch.segment(s[0], s[1], r);
                    }
                }
                Geom::Polygon { outer, holes } => {
                    let outer: Vec<_> = outer.iter().map(rel).collect();
                    let holes: Vec<Vec<_>> =
                        holes.iter().map(|h| h.iter().map(rel).collect()).collect();
                    batch.polygon(&outer, &holes);
                }
            }
        }
        Tessellation {
            origin,
            batches,
            lines,
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.batches.iter().map(|b| b.indices.len() / 3).sum()
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

    #[cfg_attr(not(test), allow(dead_code))]
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
}

/// What to draw.
pub struct Visibility<'a> {
    pub layers: &'a [bool],
    pub kinds: &'a [bool],
    pub active: usize,
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

fn kind_color(kind: Kind, slot: Slot, layer_count: usize) -> Color32 {
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

/// Draws the board.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    tess: &Tessellation,
    layer_count: usize,
    vis: &Visibility,
) {
    painter.rect_filled(rect, 0.0, BACKGROUND);

    // Bottom layer first, the active layer last, everything off the copper
    // layers on top.
    let mut order: Vec<Slot> = (0..layer_count)
        .rev()
        .filter(|&i| i != vis.active)
        .map(Slot::Layer)
        .collect();
    order.push(Slot::Layer(vis.active));
    order.extend([Slot::Silk(Side::Back), Slot::Silk(Side::Front), Slot::Board]);

    let c = rect.center();
    let z = camera.zoom as f32;
    let ox = (tess.origin.x - camera.center.x) as f32;
    let oy = (tess.origin.y - camera.center.y) as f32;
    let to_screen = |v: [f32; 2]| Pos2::new(c.x + (v[0] + ox) * z, c.y - (v[1] + oy) * z);

    for slot in order {
        if !vis.slot(slot) {
            continue;
        }
        let dim = matches!(slot, Slot::Layer(i) if i != vis.active);
        for kind in Kind::ALL {
            if !vis.kind(kind) {
                continue;
            }
            let mut color = kind_color(kind, slot, layer_count);
            if dim {
                // Opaque shapes are dimmed toward the background rather than
                // made translucent: the round joints of a trace overlap its
                // segments and would show as darker dots.
                color = if color.is_opaque() {
                    BACKGROUND.lerp_to_gamma(color, 0.6)
                } else {
                    color.gamma_multiply(0.6)
                };
            }
            for batch in tess
                .batches
                .iter()
                .filter(|b| b.slot == Some(slot) && b.kind == Some(kind))
            {
                let mut mesh = Mesh::default();
                mesh.indices.clone_from(&batch.indices);
                mesh.vertices = batch
                    .vertices
                    .iter()
                    .map(|&v| Vertex {
                        pos: to_screen(v),
                        uv: egui::epaint::WHITE_UV,
                        color,
                    })
                    .collect();
                painter.add(Shape::mesh(mesh));
            }
            for line in tess
                .lines
                .iter()
                .filter(|l| l.slot == slot && l.kind == kind)
            {
                let width = (line.width * z).max(1.0);
                let points: Vec<Pos2> = line.points.iter().map(|&v| to_screen(v)).collect();
                if points.len() == 1 {
                    painter.circle_filled(points[0], width / 2.0, color);
                } else {
                    painter.add(Shape::line(points, Stroke::new(width, color)));
                }
            }
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
            for b in &tess.batches {
                assert!(
                    b.indices.iter().all(|&i| (i as usize) < b.vertices.len()),
                    "{}",
                    path.display()
                );
            }
            boards += 1;
        }
        assert!(boards > 20, "only {boards} boards read");
    }
}

//! The selection highlight: the outline of each selected item in the
//! highlight colour, with a translucent fill for convex shapes.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape as Draw, Stroke};
use freerouting::board::BasicBoard;
use freerouting::board::items::{Item, ItemId, ItemKind};
use freerouting::geometry::{PolylineShapeOps, Shape};

use super::Frame;
use crate::scene::{Geom, P};
use crate::selection::Selection;
use crate::view::Camera;

/// The highlight colour.
pub const COLOR: Color32 = Color32::from_rgb(0, 235, 255);

/// The shapes of item `id` in millimetres, one per layer for pads and vias.
/// Empty if the item is gone.
pub fn item_geoms(board: &BasicBoard, frame: &Frame, id: ItemId) -> Vec<Geom> {
    let Some(item) = board.item(id) else {
        return Vec::new();
    };
    geoms_of(item, frame)
}

fn shape_geom(frame: &Frame, shape: &Shape) -> Option<Geom> {
    match shape {
        Shape::Circle(c) => (c.radius > 0).then(|| Geom::Circle {
            center: frame.pt(c.center.to_float()),
            radius: frame.len(f64::from(c.radius)),
        }),
        Shape::Polyline(_) => {
            let outer: Vec<P> = shape
                .corner_approx_arr()
                .into_iter()
                .map(|p| frame.pt(p))
                .collect();
            (outer.len() >= 3).then_some(Geom::Polygon {
                outer,
                holes: Vec::new(),
            })
        }
    }
}

fn geoms_of(item: &Item, frame: &Frame) -> Vec<Geom> {
    match &item.kind {
        ItemKind::Pin(pin) => pin
            .drill
            .shapes
            .iter()
            .flatten()
            .filter_map(|s| shape_geom(frame, s))
            .collect(),
        ItemKind::Via(via) => via
            .drill
            .shapes
            .iter()
            .flatten()
            .filter_map(|s| shape_geom(frame, s))
            .collect(),
        ItemKind::Trace(trace) => vec![Geom::Path {
            points: trace
                .polyline
                .corner_approx_arr()
                .into_iter()
                .map(|p| frame.pt(p))
                .collect(),
            width: frame.len(2.0 * f64::from(trace.half_width)),
        }],
        ItemKind::Area(area) => shape_geom(frame, &area.area().get_border())
            .into_iter()
            .collect(),
        ItemKind::ComponentOutline(outline) => shape_geom(frame, &outline.area().get_border())
            .into_iter()
            .collect(),
        ItemKind::BoardOutline(outline) => outline
            .shapes()
            .iter()
            .filter_map(|s| {
                let outer: Vec<P> = s
                    .corner_approx_arr()
                    .into_iter()
                    .map(|p| frame.pt(p))
                    .collect();
                (outer.len() >= 3).then_some(Geom::Polygon {
                    outer,
                    holes: Vec::new(),
                })
            })
            .collect(),
    }
}

fn is_convex(points: &[Pos2]) -> bool {
    let n = points.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b, c) = (points[i], points[(i + 1) % n], points[(i + 2) % n]);
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        if cross != 0.0 {
            if sign != 0.0 && cross.signum() != sign.signum() {
                return false;
            }
            sign = cross;
        }
    }
    true
}

/// Draws the selection and returns how many shapes it drew.
pub fn draw(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    frame: &Frame,
    board: &BasicBoard,
    selection: &Selection,
) -> usize {
    let stroke = Stroke::new(2.0, COLOR);
    let fill = Color32::from_rgba_unmultiplied(0, 235, 255, 70);
    let screen = |p: P| camera.to_screen(rect, p);
    let mut drawn = 0;
    for id in &selection.items {
        for geom in item_geoms(board, frame, *id) {
            drawn += 1;
            match geom {
                Geom::Circle { center, radius } => {
                    let r = ((radius * camera.zoom) as f32).max(2.0);
                    painter.circle(screen(center), r, fill, stroke);
                }
                Geom::Path { points, width } => {
                    let w = ((width * camera.zoom) as f32).max(2.0);
                    let line: Vec<Pos2> = points.into_iter().map(screen).collect();
                    painter.add(Draw::line(
                        line,
                        Stroke::new(w, Color32::from_rgba_unmultiplied(0, 235, 255, 150)),
                    ));
                }
                Geom::Polygon { outer, .. } => {
                    let points: Vec<Pos2> = outer.into_iter().map(screen).collect();
                    if is_convex(&points) {
                        painter.add(Draw::convex_polygon(points, fill, stroke));
                    } else {
                        painter.add(Draw::closed_line(points, stroke));
                    }
                }
            }
        }
    }
    drawn
}

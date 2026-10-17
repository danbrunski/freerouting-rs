//! What the editor shows while a session is under way (G09.5): the data of
//! the engine's [`Overlay`] as egui shapes, in the colours of the look.
//!
//! - **Route** (Java `Route.draw`): the rubber band from the last corner to
//!   the nearest target point (in the violations colour when the net's length
//!   limits are violated), the targets, the swappable pins and their
//!   incompletes, the length-matching ellipses, the layer-change marker and,
//!   with "highlight routing obstacle", the item the shove failed at.
//! - **Pin swap**: the first pin and the pins it can swap with.
//! - **Move and copy** (`MoveItemState.draw`, `CopyItemState.draw`): the
//!   floating items and the clearance violations they cause.
//!
//! The committed part of a trace is not drawn here: the route inserts it on
//! the board, and it arrives through the change journal like any other
//! edit (`tools::host`).

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape as Draw, Stroke};
use freerouting::board::items::ItemId;
use freerouting::board::{GeometryShape, ItemGeometry, RoutingBoard};
use freerouting::geometry::{FloatPoint, PolylineShapeOps};
use freerouting::interactive::Overlay;

use super::Frame;
use super::highlight::item_geoms;
use crate::look::{Look, Object, OtherColor};
use crate::scene::{Geom, P};
use crate::view::Camera;

/// The points of a length-matching ellipse.
const ELLIPSE_POINTS: usize = 72;

/// The items the overlay highlights as the obstacle of the route: the
/// failing obstacle when the setting is on, else none.
pub fn highlights(overlay: &Overlay) -> Vec<ItemId> {
    overlay
        .route
        .as_ref()
        .and_then(|r| r.failing_obstacle)
        .into_iter()
        .collect()
}

/// The shapes of an item geometry in millimetres.
pub fn geometry_geoms(frame: &Frame, geometry: &ItemGeometry) -> Vec<Geom> {
    geometry
        .shapes
        .iter()
        .filter_map(|s| shape_geom(frame, &s.shape))
        .collect()
}

fn shape_geom(frame: &Frame, shape: &GeometryShape) -> Option<Geom> {
    let polygon = |corners: Vec<FloatPoint>| {
        let outer: Vec<P> = corners.into_iter().map(|p| frame.pt(p)).collect();
        (outer.len() >= 3).then_some(Geom::Polygon {
            outer,
            holes: Vec::new(),
        })
    };
    match shape {
        GeometryShape::Polygon { outer, .. } => polygon(outer.clone()),
        GeometryShape::Polyline {
            corners,
            half_width,
        } => Some(Geom::Path {
            points: corners.iter().map(|p| frame.pt(*p)).collect(),
            width: frame.len(2.0 * f64::from(*half_width)),
        }),
        GeometryShape::Circle { center, radius } => (*radius > 0).then(|| Geom::Circle {
            center: frame.pt(center.to_float()),
            radius: frame.len(f64::from(*radius)),
        }),
        GeometryShape::Octagon(o) => polygon(o.corner_approx_arr()),
        GeometryShape::Box(b) => polygon(b.corner_approx_arr()),
    }
}

/// The points of an ellipse outline, in board units.
pub fn ellipse_points(e: &freerouting::geometry::Ellipse) -> Vec<FloatPoint> {
    let (sin, cos) = e.rotation.sin_cos();
    (0..=ELLIPSE_POINTS)
        .map(|k| {
            let t = std::f64::consts::TAU * k as f64 / ELLIPSE_POINTS as f64;
            let (x, y) = (e.bigger_radius * t.cos(), e.smaller_radius * t.sin());
            FloatPoint::new(
                e.center.x + x * cos - y * sin,
                e.center.y + x * sin + y * cos,
            )
        })
        .collect()
}

struct Pen<'a> {
    painter: &'a Painter,
    rect: Rect,
    camera: &'a Camera,
    frame: &'a Frame,
}

impl Pen<'_> {
    fn screen(&self, p: FloatPoint) -> Pos2 {
        self.camera.to_screen(self.rect, self.frame.pt(p))
    }

    fn geom(&self, geom: Geom, stroke: Stroke, fill: Color32) {
        let screen = |p: P| self.camera.to_screen(self.rect, p);
        match geom {
            Geom::Circle { center, radius } => {
                let r = ((radius * self.camera.zoom) as f32).max(2.0);
                self.painter.circle(screen(center), r, fill, stroke);
            }
            Geom::Path { points, width } => {
                let w = ((width * self.camera.zoom) as f32).max(2.0);
                let line: Vec<Pos2> = points.into_iter().map(screen).collect();
                self.painter.add(Draw::line(line, Stroke::new(w, fill)));
            }
            Geom::Polygon { outer, .. } => {
                let points: Vec<Pos2> = outer.into_iter().map(screen).collect();
                self.painter.add(Draw::closed_line(points.clone(), stroke));
                self.painter
                    .add(Draw::convex_polygon(points, fill, Stroke::NONE));
            }
        }
    }

    fn item(&self, board: &RoutingBoard, id: ItemId, stroke: Stroke, fill: Color32) -> usize {
        let geoms = item_geoms(board.basic(), self.frame, id);
        let n = geoms.len();
        for g in geoms {
            self.geom(g, stroke, fill);
        }
        n
    }
}

/// Draws the overlay of the editor's last response; returns how many shapes.
pub fn draw(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    frame: &Frame,
    board: &RoutingBoard,
    overlay: &Overlay,
    look: &Look,
) -> usize {
    let pen = Pen {
        painter,
        rect,
        camera,
        frame,
    };
    let highlight = look.other_color(OtherColor::Highlight);
    let violation = look.other_color(OtherColor::Violations);
    let incomplete = look.overlay_color(OtherColor::Incompletes, Object::Incompletes);
    let mut drawn = 0;
    if let Some(route) = &overlay.route {
        let faint =
            Color32::from_rgba_unmultiplied(highlight.r(), highlight.g(), highlight.b(), 50);
        for id in &route.targets {
            drawn += pen.item(board, *id, Stroke::new(1.0f32, highlight), faint);
        }
        for pin in &route.swap_pins {
            drawn += pen.item(board, pin.pin, Stroke::new(1.5f32, highlight), faint);
            if let Some((a, b)) = pin.incomplete {
                painter.line_segment(
                    [pen.screen(a), pen.screen(b)],
                    Stroke::new(1.0f32, incomplete),
                );
                drawn += 1;
            }
        }
        if let Some(line) = &route.incomplete {
            let color = if line.length_violation {
                violation
            } else {
                incomplete
            };
            let width = ((frame.len(line.width) * camera.zoom) as f32).max(1.0);
            painter.line_segment(
                [pen.screen(line.from), pen.screen(line.to)],
                Stroke::new(width, color),
            );
            drawn += 1;
        }
        for ellipse in &route.length_ellipses {
            let points: Vec<Pos2> = ellipse_points(ellipse)
                .into_iter()
                .map(|p| pen.screen(p))
                .collect();
            painter.add(Draw::line(points, Stroke::new(1.0f32, incomplete)));
            drawn += 1;
        }
        if let Some((centre, radius)) = route.layer_change_marker {
            let r = ((frame.len(radius) * camera.zoom) as f32).max(3.0);
            painter.circle_stroke(pen.screen(centre), r, Stroke::new(1.5f32, highlight));
            drawn += 1;
        }
        for id in highlights(overlay) {
            let fill =
                Color32::from_rgba_unmultiplied(violation.r(), violation.g(), violation.b(), 90);
            drawn += pen.item(board, id, Stroke::new(2.0f32, violation), fill);
        }
    }
    if let Some((first, swappable)) = &overlay.swap {
        drawn += pen.item(
            board,
            *first,
            Stroke::new(2.0f32, highlight),
            Color32::TRANSPARENT,
        );
        let faint =
            Color32::from_rgba_unmultiplied(highlight.r(), highlight.g(), highlight.b(), 60);
        for id in swappable {
            drawn += pen.item(board, *id, Stroke::new(1.5f32, highlight), faint);
        }
    }
    let fill = Color32::from_rgba_unmultiplied(highlight.r(), highlight.g(), highlight.b(), 90);
    for geometry in &overlay.floating {
        for g in geometry_geoms(frame, geometry) {
            pen.geom(g, Stroke::new(1.5f32, highlight), fill);
            drawn += 1;
        }
    }
    for (_, shape) in &overlay.floating_violations {
        if let Some(g) = shape_geom(frame, shape) {
            let vfill =
                Color32::from_rgba_unmultiplied(violation.r(), violation.g(), violation.b(), 110);
            pen.geom(g, Stroke::new(1.0f32, violation), vfill);
            drawn += 1;
        }
    }
    drawn
}

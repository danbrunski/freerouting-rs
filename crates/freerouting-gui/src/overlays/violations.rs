//! Clearance violation markers: each violation's shape as an outline with a
//! translucent fill, and a ring around its centre so that it can be seen
//! when the view is zoomed out (Java `ClearanceViolations` graphics).

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use freerouting::geometry::PolylineShapeOps;

use super::Frame;
use crate::analysis::Drc;
use crate::look::{Look, OtherColor};
use crate::view::Camera;

const RING_RADIUS: f32 = 7.0;

/// Draws one marker per clearance violation and returns how many.
pub fn draw(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    frame: &Frame,
    drc: &Drc,
    look: &Look,
) -> usize {
    let color = look.other_color(OtherColor::Violations);
    let outline = Stroke::new(1.0, color);
    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 90);
    for violation in &drc.clearance {
        let points: Vec<Pos2> = violation
            .shape
            .corner_approx_arr()
            .into_iter()
            .map(|p| camera.to_screen(rect, frame.pt(p)))
            .collect();
        if points.len() >= 3 {
            // Violation areas are overlaps of convex shapes, so convex.
            painter.add(Shape::convex_polygon(points, fill, outline));
        }
        let centre = camera.to_screen(rect, frame.pt(violation.location()));
        painter.circle_stroke(centre, RING_RADIUS, Stroke::new(1.5, color));
    }
    drc.clearance.len()
}

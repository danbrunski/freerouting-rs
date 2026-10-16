//! The rubber band of a keepout under construction (G05.7): the corners
//! clicked so far, joined by lines, and a line to the snapped cursor
//! (Java `CornerItemConstructionState.draw`: white at half intensity); for a
//! circle the centre and the circle through the cursor
//! (`CircleConstructionState.draw`).

use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke};
use freerouting::board::RoutingBoard;
use freerouting::geometry::{FloatPoint, IntPoint};

use super::Frame;
use crate::commands::KeepoutKind;
use crate::scene::P;
use crate::tools::keepout::KeepoutTool;
use crate::view::Camera;

/// The pixels a corner marker is wide.
const CORNER: f32 = 3.0;

/// What a frame of the rubber band is made of, for tests.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Band {
    /// The corners and the snapped cursor, in board units, in order.
    pub points: Vec<IntPoint>,
    /// A circle's radius in board units (0 for the other shapes).
    pub radius: f64,
}

/// The points of the rubber band: the corners, then the cursor snapped like
/// the next corner would be (or, for a circle, only the centre).
pub fn band(tool: &KeepoutTool, board: &RoutingBoard, cursor: Option<FloatPoint>) -> Band {
    let mut points = tool.corners().to_vec();
    let mut radius = 0.0;
    if let Some(cursor) = cursor {
        if tool.kind == KeepoutKind::Circle {
            if let Some(center) = points.first() {
                radius = center.to_float().distance(cursor);
            }
        } else {
            points.push(tool.snap(board, cursor.round()));
        }
    }
    Band { points, radius }
}

/// Draws the band; returns the number of points drawn.
pub fn draw(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    frame: &Frame,
    band: &Band,
    kind: KeepoutKind,
) -> usize {
    let color = Color32::from_white_alpha(200);
    let stroke = Stroke::new(1.5, color);
    let screen = |p: IntPoint| -> Pos2 { camera.to_screen(rect, frame.pt(p.to_float())) };
    if kind == KeepoutKind::Circle {
        let Some(center) = band.points.first() else {
            return 0;
        };
        let c = screen(*center);
        painter.circle_filled(c, CORNER, color);
        let r = (frame.len(band.radius) * camera.zoom) as f32;
        if r > 0.0 {
            painter.circle_stroke(c, r, stroke);
        }
        return 1;
    }
    let points: Vec<Pos2> = band.points.iter().map(|p| screen(*p)).collect();
    for pair in points.windows(2) {
        painter.line_segment([pair[0], pair[1]], stroke);
    }
    for p in &points {
        painter.rect_filled(
            Rect::from_center_size(*p, eframe::egui::vec2(CORNER * 2.0, CORNER * 2.0)),
            0.0,
            color,
        );
    }
    points.len()
}

/// The cursor as a board point, from the scene's millimetres.
pub fn cursor_point(board: &RoutingBoard, mm: Option<P>) -> Option<FloatPoint> {
    mm.map(|p| crate::tools::inspect::board_point(board.basic(), p))
}

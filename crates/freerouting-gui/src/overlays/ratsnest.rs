//! The ratsnest: one hairline per airline (Java `NetIncompletesGraphics`).

use eframe::egui::{Color32, Painter, Rect, Stroke};

use super::Frame;
use crate::analysis::Ratsnest;
use crate::view::Camera;

/// The colour of the airlines.
pub const COLOR: Color32 = Color32::from_rgb(255, 225, 110);

/// Draws every airline as a 1 px line and returns how many.
pub fn draw(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    frame: &Frame,
    ratsnest: &Ratsnest,
) -> usize {
    let stroke = Stroke::new(1.0, COLOR);
    for line in &ratsnest.airlines {
        let from = camera.to_screen(rect, frame.pt(line.from_corner));
        let to = camera.to_screen(rect, frame.pt(line.to_corner));
        painter.line_segment([from, to], stroke);
    }
    ratsnest.airlines.len()
}

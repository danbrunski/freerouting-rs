//! The ratsnest: one hairline per airline (Java `NetIncompletesGraphics`).

use eframe::egui::{Painter, Rect, Stroke};

use super::Frame;
use crate::analysis::Ratsnest;
use crate::look::{Look, Object, OtherColor};
use crate::view::Camera;

/// Draws every airline as a 1 px line and returns how many.
pub fn draw(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    frame: &Frame,
    ratsnest: &Ratsnest,
    look: &Look,
) -> usize {
    // The colour is the "incomplete connections" colour at the level of the
    // "incomplete connections" object slider; level 0 draws nothing.
    if look.object_level(Object::Incompletes) == 0 {
        return 0;
    }
    let stroke = Stroke::new(
        1.0f32,
        look.overlay_color(OtherColor::Incompletes, Object::Incompletes),
    );
    for line in &ratsnest.airlines {
        let from = camera.to_screen(rect, frame.pt(line.from_corner));
        let to = camera.to_screen(rect, frame.pt(line.to_corner));
        painter.line_segment([from, to], stroke);
    }
    ratsnest.airlines.len()
}

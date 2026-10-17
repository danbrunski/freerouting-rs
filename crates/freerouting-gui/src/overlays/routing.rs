//! The autorouter's overlay (G07.5): Java's `GuiRoutingJobWorker.draw`.
//! The airline of the connection being routed, in the incomplete colour,
//! `min(3 mil in board units, 300)` wide; and the optimizer's position as an
//! X of radius `10 * defaultTraceHalfWidth(layer 0)`.
//!
//! Both come from the newest `Progress` event (`airline`,
//! `optimizer_position`). [`shapes`] lists them in board millimetres so a
//! test can check them without a screen; [`draw`] paints that list.

use eframe::egui::{Painter, Rect, Stroke};
use freerouting::autoroute::events::Progress;

use super::Frame;
use crate::look::{Look, Object, OtherColor};
use crate::scene::P;
use crate::view::Camera;

/// 3 mil in micrometres.
const THREE_MIL_UM: f64 = 76.2;
/// Java's cap on the airline width, in board units.
const MAX_AIRLINE_WIDTH: f64 = 300.0;

/// One thing the overlay draws, in the millimetres of the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoutingShape {
    /// The airline of the connection being routed.
    Line { from: P, to: P, width_mm: f64 },
    /// The optimizer's position: the two diagonals of a square of this
    /// half size.
    Cross { center: P, radius_mm: f64 },
}

/// The shapes for `progress`. `half_width` is the board's default trace half
/// width on layer 0, in board units.
pub fn shapes(progress: &Progress, frame: &Frame, half_width: i32) -> Vec<RoutingShape> {
    let mut out = Vec::new();
    if let Some((from, to)) = progress.airline {
        // `Unit.scale(3, mil, board unit)` capped at 300: the board unit is
        // the design's unit divided by its resolution, so go through mm.
        let three_mil_board = THREE_MIL_UM / 1000.0 / frame.len(1.0);
        out.push(RoutingShape::Line {
            from: frame.pt(from),
            to: frame.pt(to),
            width_mm: frame.len(three_mil_board.min(MAX_AIRLINE_WIDTH)),
        });
    }
    if let Some(position) = progress.optimizer_position {
        out.push(RoutingShape::Cross {
            center: frame.pt(position),
            radius_mm: frame.len(10.0 * f64::from(half_width)),
        });
    }
    out
}

/// Paints `shapes`; returns how many.
pub fn draw(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    shapes: &[RoutingShape],
    look: &Look,
) -> usize {
    let color = look.overlay_color(OtherColor::Incompletes, Object::Incompletes);
    for shape in shapes {
        match *shape {
            RoutingShape::Line { from, to, width_mm } => {
                let width = (width_mm * camera.zoom).max(1.0) as f32;
                painter.line_segment(
                    [camera.to_screen(rect, from), camera.to_screen(rect, to)],
                    Stroke::new(width, color),
                );
            }
            RoutingShape::Cross { center, radius_mm } => {
                let r = radius_mm;
                let stroke = Stroke::new(2.0f32, color);
                for (a, b) in [
                    (
                        P::new(center.x - r, center.y - r),
                        P::new(center.x + r, center.y + r),
                    ),
                    (
                        P::new(center.x - r, center.y + r),
                        P::new(center.x + r, center.y - r),
                    ),
                ] {
                    painter.line_segment(
                        [camera.to_screen(rect, a), camera.to_screen(rect, b)],
                        stroke,
                    );
                }
            }
        }
    }
    shapes.len()
}

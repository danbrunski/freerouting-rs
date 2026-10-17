//! Overlays drawn on top of the board canvas with egui shapes: the
//! ratsnest, the clearance violation markers and the selection highlight.
//!
//! They read engine results only ([`crate::analysis`], the board, the
//! [`Selection`]) and are at most a few thousand shapes, so they stay out of
//! the GPU pipeline (G00 lanes). [`draw`] returns what it drew so tests can
//! assert it (`stats.rs`).

pub mod construction;
pub mod highlight;
pub mod ratsnest;
pub mod route;
pub mod violations;

use eframe::egui::{Painter, Rect};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::geometry::{FloatPoint, IntBox};
use freerouting::io::CoordinateTransform;

use crate::analysis::{Drc, Ratsnest};
use crate::look::Look;
use crate::scene::{Bounds, P};
use crate::selection::Selection;
use crate::stats::OverlayCounts;
use crate::view::Camera;

/// Which overlays are on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayState {
    /// The airlines of the incomplete connections (Java: `G`).
    pub show_ratsnest: bool,
    /// The clearance violation markers (Java: `V`).
    pub show_violations: bool,
}

impl Default for OverlayState {
    fn default() -> Self {
        OverlayState {
            show_ratsnest: true,
            show_violations: false,
        }
    }
}

/// Board coordinates to the millimetres the scene and the camera use: the
/// coordinates of the design file, Y up.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    transform: CoordinateTransform,
    mm_per_unit: f64,
}

impl Frame {
    pub fn new(board: &BasicBoard) -> Frame {
        let comm = &board.communication;
        Frame {
            transform: comm.coordinate_transform,
            mm_per_unit: comm.unit.micrometers() / 1000.0,
        }
    }

    /// A board point in millimetres.
    pub fn pt(&self, p: FloatPoint) -> P {
        let [x, y] = self.transform.board_to_dsn_point(p);
        P::new(x * self.mm_per_unit, y * self.mm_per_unit)
    }

    /// A board length in millimetres.
    pub fn len(&self, v: f64) -> f64 {
        self.transform.board_to_dsn(v) * self.mm_per_unit
    }

    /// A board length in the unit of the design file (Java
    /// `CoordinateTransform.boardToUser(double)`).
    pub fn user_len(&self, v: f64) -> f64 {
        self.transform.board_to_dsn(v)
    }

    /// A board point in the unit of the design file (Java's "user"
    /// coordinates).
    pub fn user(&self, p: FloatPoint) -> [f64; 2] {
        self.transform.board_to_dsn_point(p)
    }

    /// A box in millimetres; `None` for the empty box.
    pub fn bounds(&self, b: &IntBox) -> Option<Bounds> {
        if b.ll.x > b.ur.x || b.ll.y > b.ur.y {
            return None;
        }
        let mut out = Bounds::EMPTY;
        out.add(self.pt(b.ll.to_float()));
        out.add(self.pt(b.ur.to_float()));
        Some(out)
    }
}

/// What the overlays draw this frame.
pub struct Inputs<'a> {
    /// The colours and levels of the highlight, the airlines and the markers.
    pub look: &'a Look,
    pub board: &'a RoutingBoard,
    pub ratsnest: Option<&'a Ratsnest>,
    pub drc: Option<&'a Drc>,
    pub selection: &'a Selection,
    pub state: OverlayState,
}

/// Draws the overlays on `painter` for the canvas `rect`.
pub fn draw(painter: &Painter, rect: Rect, camera: &Camera, inputs: &Inputs<'_>) -> OverlayCounts {
    let frame = Frame::new(inputs.board.basic());
    let mut counts = OverlayCounts::default();
    if inputs.state.show_ratsnest
        && let Some(ratsnest) = inputs.ratsnest
    {
        counts.airlines = ratsnest::draw(painter, rect, camera, &frame, ratsnest, inputs.look);
    }
    if inputs.state.show_violations
        && let Some(drc) = inputs.drc
    {
        counts.markers = violations::draw(painter, rect, camera, &frame, drc, inputs.look);
    }
    if !inputs.selection.is_empty() {
        counts.highlighted = highlight::draw(
            painter,
            rect,
            camera,
            &frame,
            inputs.board.basic(),
            inputs.selection,
            inputs.look,
        );
    }
    counts
}

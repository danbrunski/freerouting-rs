//! Keepout construction (G05.7, I03.4): the GUI side of the engine's
//! [`Construction`] (Java's `TileConstructionState`,
//! `CircleConstructionState`, `PolygonShapeConstructionState`,
//! `HoleConstructionState` and `CornerItemConstructionState`).
//!
//! The engine owns the behaviour (I03.4): corner rounding and snapping to
//! the angle restriction, the tile's concave-corner removal, the shape
//! check, the snapshot and the insertion, and the hole's remove and
//! reinsert. The tool only remembers which kind the user chose and maps the
//! clicks; the rubber band and the notices stay here.

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::geometry::{FloatPoint, IntPoint};
use freerouting::interactive::{Construction, EditSettings, KeepoutKind as EngineKind};

use crate::commands::KeepoutKind;

/// A keepout or hole under construction.
#[derive(Clone, Debug, PartialEq)]
pub struct KeepoutTool {
    pub kind: KeepoutKind,
    construction: Construction,
}

/// What [`complete`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The keepout was inserted, or the hole added (after a snapshot).
    Inserted(ItemId),
    /// Java's `keepout_cancelled_because_of_overlaps` (`adding_hole_failed`
    /// for a hole): too few corners, an invalid shape, a zero radius, or an
    /// overlap.
    Refused,
}

impl KeepoutTool {
    /// Starts a tile, polygon or circle at `at` (the right-click location).
    /// A hole starts with [`start_hole`](Self::start_hole).
    pub fn start(kind: KeepoutKind, board: &RoutingBoard, at: FloatPoint) -> KeepoutTool {
        let engine = match kind {
            KeepoutKind::Tile | KeepoutKind::Hole => EngineKind::Tile,
            KeepoutKind::Polygon => EngineKind::Polygon,
            KeepoutKind::Circle => EngineKind::Circle,
        };
        KeepoutTool {
            kind,
            construction: Construction::keepout(board, engine, at),
        }
    }

    /// Starts a hole in the one keepout, via keepout or conduction area at
    /// `at` on `layer`; `Err` is Java's message key.
    pub fn start_hole(
        board: &RoutingBoard,
        layer: i32,
        at: FloatPoint,
    ) -> Result<KeepoutTool, &'static str> {
        let settings = EditSettings {
            layer,
            ..EditSettings::for_board(board)
        };
        Construction::hole(board, &settings, at)
            .map(|construction| KeepoutTool {
                kind: KeepoutKind::Hole,
                construction,
            })
            .map_err(|status| status.key)
    }

    /// The corners so far (for a circle, its rounded centre).
    pub fn corners(&self) -> Vec<IntPoint> {
        if self.kind == KeepoutKind::Circle {
            return vec![self.construction.circle().0.round()];
        }
        self.construction.corners().to_vec()
    }

    /// Java `CornerItemConstructionState.snap`: the line from the last
    /// corner to `point`, snapped to the board's angle restriction.
    pub fn snap(&self, board: &RoutingBoard, point: IntPoint) -> IntPoint {
        self.construction.snap(board, point)
    }

    /// Java `leftButtonClicked` of a tile, polygon or hole: rounds `at`,
    /// snaps it and adds it (a tile then drops its concave corners, a hole
    /// takes only corners inside its area).
    pub fn add_corner(&mut self, board: &RoutingBoard, at: FloatPoint) {
        if self.kind != KeepoutKind::Circle {
            self.construction.click(board, at);
        }
    }
}

/// Completes the keepout on `layer` or adds the hole (Java's `complete()` of
/// the construction states, I03.4). A circle takes its radius from
/// `radius_point`, Java's pointer position (unrounded, as Java's distance from
/// the centre to the mouse position is).
pub fn complete(
    tool: &mut KeepoutTool,
    board: &mut RoutingBoard,
    layer: i32,
    radius_point: Option<FloatPoint>,
) -> Outcome {
    let mut construction = tool.construction.clone();
    if tool.kind == KeepoutKind::Circle {
        let Some(edge) = radius_point else {
            return Outcome::Refused;
        };
        construction.pointer_moved(board, edge);
    }
    match construction.complete(board, layer).inserted {
        Some(id) => Outcome::Inserted(id),
        None => Outcome::Refused,
    }
}

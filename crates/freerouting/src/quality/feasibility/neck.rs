//! The neck ladder: how long a neck-down at the pin would make an
//! infeasible pin routable (Q03.5).
//!
//! For each radius R of [`NECK_RADII_MM`] the required width is the pin's
//! neck width within R of the pin centre and the class width outside, and
//! the widest path is searched again on the same distance field. The
//! smallest R that gives `passable` is `neck_needed`; Q02 reads it to
//! choose its neck distance.
//!
//! The pin widths are Java's `Pin.getMinWidth` and
//! `Pin.getTraceNeckdownHalfwidth` (`board/model/items/Pin.java:567-590`),
//! B05.4's [`BasicBoard::pin_min_width`] and
//! [`BasicBoard::trace_neckdown_half_width`] (`board/pin_exits.rs`), which
//! the `pin-exits` probe compares with Java.

use freerouting_geometry::FloatPoint;

use crate::board::BasicBoard;

use super::Verdict;
use super::widest_path::{Field, Requirement, ViaSpan, widest_path};
use super::{PinContext, verdict_of};

/// The neck radii tried, in mm from the pin centre.
pub const NECK_RADII_MM: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// One rung of the ladder.
#[derive(Clone, Debug, PartialEq)]
pub struct NeckRung {
    /// The neck radius in mm.
    pub radius_mm: f64,
    pub verdict: Verdict,
    /// The corridor at the throat in board units (`None`: enclosed; the
    /// maximum for a connected pin).
    pub corridor: Option<i64>,
}

/// The neck ladder of one pin.
#[derive(Clone, Debug, PartialEq)]
pub struct NeckLadder {
    /// The neck width on each routing layer (board units).
    pub width: Vec<i64>,
    /// The rungs tried, up to the first `passable` one.
    pub rungs: Vec<NeckRung>,
    /// The smallest radius in mm that gives `passable`, or `None`.
    pub needed_mm: Option<f64>,
}

/// Runs the neck ladder for the pin of `ctx` on its distance field.
pub(super) fn neck_ladder(
    board: &BasicBoard,
    ctx: &PinContext<'_>,
    field: &Field,
    vias: &[ViaSpan],
) -> NeckLadder {
    let item = ctx.item;
    let floor = 2 * i64::from(board.rules.min_trace_half_width());
    let width: Vec<i64> = ctx
        .window
        .frame
        .layers
        .iter()
        .zip(&ctx.width)
        .map(|(&layer, &w)| {
            if item.drill().and_then(|d| d.shape_on_layer(layer)).is_none() {
                return w;
            }
            let neck = 2 * i64::from(board.trace_neckdown_half_width(item.id(), layer));
            neck.max(floor).min(w)
        })
        .collect();
    let centre = item.drill().expect("pin").center.to_float();
    let mut rungs = Vec::new();
    let mut needed_mm = None;
    for radius_mm in NECK_RADII_MM {
        let req = Requirement {
            width: ctx.width.clone(),
            neck: Some((
                FloatPoint {
                    x: centre.x,
                    y: centre.y,
                },
                radius_mm * ctx.units_per_mm,
                width.clone(),
            )),
        };
        let outcome = widest_path(
            &ctx.window,
            field,
            vias,
            &req,
            &ctx.start,
            ctx.plane_targets,
        );
        let (verdict, corridor) = verdict_of(&outcome, ctx.delta);
        rungs.push(NeckRung {
            radius_mm,
            verdict,
            corridor,
        });
        if verdict == Verdict::Passable {
            needed_mm = Some(radius_mm);
            break;
        }
    }
    NeckLadder {
        width,
        rungs,
        needed_mm,
    }
}

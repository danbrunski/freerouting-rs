//! The coupled-length report (X08.5).
//!
//! Per routed pair: the **coupled length**, the length of the centreline
//! the P and N tracks were offset from; and the **share within ±10 % of
//! g**: of all the pair's copper on its layer (coupled tracks and legs,
//! sampled at 100 points per segment, weighted by segment length), the
//! part whose edge-to-edge distance to the partner net's copper is within
//! 10 % of the gap. Measured on the board as it is, so it can be taken
//! again after later stages.

use freerouting::board::BasicBoard;
use freerouting::geometry::{FloatPoint, Point};

use super::measure::polyline_distance;
use super::model::Pair;

/// Samples per segment.
pub const SAMPLES: usize = 100;

/// The share tolerance, as a fraction of the gap.
pub const GAP_TOLERANCE: f64 = 0.10;

/// One pair's coupling on the board.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coupling {
    /// The pair's copper length on the layer (P and N), board units.
    pub copper_length: f64,
    /// The part of it within ±10 % of the gap, board units.
    pub within_length: f64,
}

impl Coupling {
    /// `within_length / copper_length`, 0 without copper.
    pub fn share(&self) -> f64 {
        if self.copper_length > 0.0 {
            self.within_length / self.copper_length
        } else {
            0.0
        }
    }
}

/// The trace polylines (with half widths) of `net` on `layer`.
fn traces(board: &BasicBoard, net: i32, layer: i32) -> Vec<(Vec<FloatPoint>, f64)> {
    board
        .items()
        .filter(|i| i.contains_net(net))
        .filter_map(|i| i.as_trace())
        .filter(|t| t.layer == layer)
        .map(|t| {
            (
                t.polyline.corners().iter().map(Point::to_float).collect(),
                f64::from(t.half_width),
            )
        })
        .collect()
}

/// The coupling of `pair` on `layer` ([module docs](self)).
pub fn coupling(board: &BasicBoard, pair: &Pair, layer: i32) -> Coupling {
    let gap = f64::from(pair.gap[layer as usize]);
    let p = traces(board, pair.p, layer);
    let n = traces(board, pair.n, layer);
    let mut out = Coupling {
        copper_length: 0.0,
        within_length: 0.0,
    };
    for (mine, theirs) in [(&p, &n), (&n, &p)] {
        for (corners, hw) in mine {
            for s in corners.windows(2) {
                let len = (s[1].x - s[0].x).hypot(s[1].y - s[0].y);
                out.copper_length += len;
                let step = len / SAMPLES as f64;
                for k in 0..SAMPLES {
                    let t = (k as f64 + 0.5) / SAMPLES as f64;
                    let x = FloatPoint::new(
                        s[0].x + (s[1].x - s[0].x) * t,
                        s[0].y + (s[1].y - s[0].y) * t,
                    );
                    let edge = theirs
                        .iter()
                        .map(|(c, their_hw)| polyline_distance(x, c) - hw - their_hw)
                        .fold(f64::INFINITY, f64::min);
                    if (edge - gap).abs() <= GAP_TOLERANCE * gap {
                        out.within_length += step;
                    }
                }
            }
        }
    }
    out
}

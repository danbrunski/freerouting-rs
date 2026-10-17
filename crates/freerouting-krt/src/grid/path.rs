// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/geometry_utils.py
// (simplify_path) and py_router/single_ended_routing.py
// (_path_to_segments_vias) at 0486e799.

//! A grid path → the copper to insert (X05.2).
//!
//! The vendored search returns cells `(gx, gy, layer)` from the source to
//! the target, one 8-neighbour step or one layer change at a time.
//! [`simplify`] drops the interior points of straight runs (KRT
//! `simplify_path`), with an exact integer test where KRT compares
//! normalized float directions: a point is dropped when it is on one layer
//! with both neighbours and the two steps have a zero cross product and a
//! positive dot product. [`plan`] then cuts the simplified path into
//! [`Piece`]s in path order (KRT `_path_to_segments_vias`): one trace per run
//! of equal layer, through the cell centres in board coordinates, and a via
//! at each layer change, except where the caller says an own-net hole
//! already joins the two layers (KRT's through-hole skip).
//!
//! A run of one point (a path that starts or ends with a layer change, or a
//! one-cell path) is kept as a one-corner trace piece, so the terminal
//! connectors (`terminals`) always find the end run of each terminal; the
//! inserter skips traces with fewer than two distinct corners.

use freerouting::geometry::IntPoint;

use super::frame::Frame;

/// A cell of a grid path: `(gx, gy, layer)`, as the vendored search
/// returns it.
pub type GridPoint = (i32, i32, u8);

/// One piece of copper, in path order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    /// A trace on `layer` through `corners` (board coordinates).
    Trace {
        /// The board layer.
        layer: i32,
        /// The corners, in path order.
        corners: Vec<IntPoint>,
    },
    /// A via at `at` between two layers.
    Via {
        /// The via's centre.
        at: IntPoint,
        /// The layer the path comes from.
        from_layer: i32,
        /// The layer the path goes to.
        to_layer: i32,
    },
}

/// Why a path could not be planned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// The path has no point.
    Empty,
    /// A layer change moved in the plane, at path index `index`.
    SkewedLayerChange {
        /// The index of the point after the change.
        index: usize,
    },
}

/// True if the step `a → b` continues the step `prev → a` in the same
/// direction: zero cross product and positive dot product, in `i64`.
fn continues(prev: GridPoint, a: GridPoint, b: GridPoint) -> bool {
    let (dx1, dy1) = (i64::from(a.0 - prev.0), i64::from(a.1 - prev.1));
    let (dx2, dy2) = (i64::from(b.0 - a.0), i64::from(b.1 - a.1));
    dx1 * dy2 - dy1 * dx2 == 0 && dx1 * dx2 + dy1 * dy2 > 0
}

/// `path` without the interior points of its straight same-layer runs
/// (KRT `simplify_path`). The ends and every point next to a layer change
/// are kept.
pub fn simplify(path: &[GridPoint]) -> Vec<GridPoint> {
    if path.len() <= 2 {
        return path.to_vec();
    }
    let mut result = Vec::with_capacity(path.len());
    result.push(path[0]);
    for w in path.windows(3) {
        let (prev, current, next) = (w[0], w[1], w[2]);
        let layer_change = prev.2 != current.2 || current.2 != next.2;
        if layer_change || !continues(prev, current, next) {
            result.push(current);
        }
    }
    result.push(path[path.len() - 1]);
    result
}

/// The pieces of `path` on `frame` (module docs). `own_hole(at, from, to)`
/// is true where an own-net hole already joins layers `from` and `to` at
/// `at`, so no via is planned there.
pub fn plan(
    path: &[GridPoint],
    frame: &Frame,
    own_hole: impl Fn(IntPoint, i32, i32) -> bool,
) -> Result<Vec<Piece>, PlanError> {
    let simple = simplify(path);
    let Some(&first) = simple.first() else {
        return Err(PlanError::Empty);
    };
    let centre = |p: GridPoint| frame.centre(p.0, p.1);
    let mut pieces = Vec::new();
    let mut layer = i32::from(first.2);
    let mut corners = vec![centre(first)];
    for (index, pair) in simple.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        if a.2 == b.2 {
            corners.push(centre(b));
            continue;
        }
        if (a.0, a.1) != (b.0, b.1) {
            return Err(PlanError::SkewedLayerChange { index: index + 1 });
        }
        let at = centre(a);
        pieces.push(Piece::Trace {
            layer,
            corners: std::mem::take(&mut corners),
        });
        let to_layer = i32::from(b.2);
        if !own_hole(at, layer, to_layer) {
            pieces.push(Piece::Via {
                at,
                from_layer: layer,
                to_layer,
            });
        }
        layer = to_layer;
        corners.push(at);
    }
    pieces.push(Piece::Trace { layer, corners });
    Ok(pieces)
}

/// The number of vias in `pieces`.
pub fn via_count(pieces: &[Piece]) -> usize {
    pieces
        .iter()
        .filter(|p| matches!(p, Piece::Via { .. }))
        .count()
}

/// The traces of `pieces` with at least two distinct corners.
pub fn real_traces(pieces: &[Piece]) -> impl Iterator<Item = (i32, &[IntPoint])> {
    pieces.iter().filter_map(|p| match p {
        Piece::Trace { layer, corners } if corners.windows(2).any(|w| w[0] != w[1]) => {
            Some((*layer, corners.as_slice()))
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting::geometry::IntBox;

    fn frame() -> Frame {
        Frame::covering(1000, &IntBox::new(-50_000, -50_000, 50_000, 50_000), 2)
    }

    fn no_hole(_: IntPoint, _: i32, _: i32) -> bool {
        false
    }

    #[test]
    fn a_staircase_of_45_degree_steps_collapses_to_one_diagonal_segment() {
        let path: Vec<GridPoint> = (0..6).map(|k| (k, k, 0)).collect();
        assert_eq!(simplify(&path), vec![(0, 0, 0), (5, 5, 0)]);
        let pieces = plan(&path, &frame(), no_hole).unwrap();
        assert_eq!(
            pieces,
            vec![Piece::Trace {
                layer: 0,
                corners: vec![IntPoint::new(0, 0), IntPoint::new(5000, 5000)],
            }]
        );
        // A turn keeps its corner; a reversal is not collinear either.
        let turn = [(0, 0, 0), (1, 1, 0), (2, 2, 0), (3, 2, 0), (4, 2, 0)];
        assert_eq!(simplify(&turn), vec![(0, 0, 0), (2, 2, 0), (4, 2, 0)]);
        let back = [(0, 0, 0), (1, 0, 0), (0, 0, 0)];
        assert_eq!(simplify(&back), back.to_vec());
    }

    #[test]
    fn a_layer_change_produces_exactly_one_via_at_that_point() {
        let path = [
            (0, 0, 0),
            (1, 0, 0),
            (2, 0, 0),
            (2, 0, 1),
            (2, 1, 1),
            (2, 2, 1),
        ];
        let pieces = plan(&path, &frame(), no_hole).unwrap();
        assert_eq!(via_count(&pieces), 1);
        assert_eq!(
            pieces,
            vec![
                Piece::Trace {
                    layer: 0,
                    corners: vec![IntPoint::new(0, 0), IntPoint::new(2000, 0)],
                },
                Piece::Via {
                    at: IntPoint::new(2000, 0),
                    from_layer: 0,
                    to_layer: 1,
                },
                Piece::Trace {
                    layer: 1,
                    corners: vec![IntPoint::new(2000, 0), IntPoint::new(2000, 2000)],
                },
            ]
        );
    }

    #[test]
    fn a_path_that_returns_to_the_same_layer_twice_produces_two_vias() {
        let path = [
            (0, 0, 0),
            (1, 0, 0),
            (1, 0, 1),
            (2, 0, 1),
            (3, 0, 1),
            (3, 0, 0),
            (4, 0, 0),
        ];
        let pieces = plan(&path, &frame(), no_hole).unwrap();
        assert_eq!(via_count(&pieces), 2);
        let vias: Vec<_> = pieces
            .iter()
            .filter_map(|p| match p {
                Piece::Via {
                    at,
                    from_layer,
                    to_layer,
                } => Some((*at, *from_layer, *to_layer)),
                Piece::Trace { .. } => None,
            })
            .collect();
        assert_eq!(
            vias,
            vec![
                (IntPoint::new(1000, 0), 0, 1),
                (IntPoint::new(3000, 0), 1, 0)
            ]
        );
        assert_eq!(real_traces(&pieces).count(), 3);
    }

    #[test]
    fn an_own_hole_replaces_the_via_and_end_vias_leave_one_corner_runs() {
        let path = [(0, 0, 0), (0, 0, 1), (1, 0, 1), (1, 0, 0)];
        let at_origin = |at: IntPoint, _: i32, _: i32| at == IntPoint::new(0, 0);
        let pieces = plan(&path, &frame(), at_origin).unwrap();
        assert_eq!(via_count(&pieces), 1);
        // The first and last runs are single points, kept for the
        // connectors; only the middle run is a real trace.
        assert_eq!(pieces.len(), 4);
        assert_eq!(real_traces(&pieces).count(), 1);
        assert_eq!(
            plan(&[(0, 0, 0), (1, 0, 1)], &frame(), no_hole),
            Err(PlanError::SkewedLayerChange { index: 1 })
        );
        assert_eq!(plan(&[], &frame(), no_hole), Err(PlanError::Empty));
    }

    #[test]
    fn the_collinearity_test_is_exact_at_the_coordinate_limits() {
        let m = i32::MAX / 2;
        // Far apart points on one diagonal and one off by a single unit.
        assert!(continues((-m, -m, 0), (0, 0, 0), (m, m, 0)));
        assert!(!continues((-m, -m, 0), (0, 0, 0), (m, m - 1, 0)));
        assert!(!continues((-m, 0, 0), (0, 0, 0), (-m, 0, 0)));
    }
}

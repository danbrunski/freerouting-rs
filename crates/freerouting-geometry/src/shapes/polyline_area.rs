//! Areas whose border and hole borders consist of straight lines.

use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::float_point::FloatPoint;
use crate::point::{IntPoint, Point};
use crate::quirks::{SplitQuirks, SplitReach};
use crate::stoppable::Stoppable;
use crate::vector::Vector;

use super::{IntBox, IntOctagon, PolylineShape, PolylineShapeOps, TileShape, TileShapeOps};

struct PolylineAreaInner {
    border: PolylineShape,
    holes: Vec<PolylineShape>,
    /// One cached split per [`SplitQuirks`] value ([`SplitQuirks::cache_slot`]), so a
    /// split is never shared across switch values (N06).
    convex_pieces: [OnceLock<Vec<TileShape>>; SplitQuirks::COMBINATIONS],
}

/// An area where the outside border curve and the hole borders consist of straight
/// lines.
///
/// Port of `app.freerouting.geometry.planar.PolylineArea`. Cheap to clone (the border,
/// holes and the cached convex pieces are shared).
#[derive(Clone)]
pub struct PolylineArea {
    inner: Arc<PolylineAreaInner>,
}

impl fmt::Debug for PolylineArea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PolylineArea")
            .field("border", &self.inner.border)
            .field("holes", &self.inner.holes)
            .finish()
    }
}

impl PolylineArea {
    /// Creates an area from its border shape and its holes.
    pub fn new(border: PolylineShape, holes: Vec<PolylineShape>) -> Self {
        PolylineArea {
            inner: Arc::new(PolylineAreaInner {
                border,
                holes,
                convex_pieces: Default::default(),
            }),
        }
    }

    /// The dimension of the border shape.
    pub fn dimension(&self) -> i32 {
        self.inner.border.dimension()
    }

    /// True if the border shape is bounded.
    pub fn is_bounded(&self) -> bool {
        self.inner.border.is_bounded()
    }

    /// True if the border shape is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.border.is_empty()
    }

    /// True if the border shape is contained in `b`.
    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        self.inner.border.is_contained_in(b)
    }

    /// The outside border shape.
    pub fn get_border(&self) -> &PolylineShape {
        &self.inner.border
    }

    /// The holes.
    pub fn get_holes(&self) -> &[PolylineShape] {
        &self.inner.holes
    }

    /// The bounding box of the border shape.
    pub fn bounding_box(&self) -> IntBox {
        self.inner.border.bounding_box()
    }

    /// The bounding octagon of the border shape.
    pub fn bounding_octagon(&self) -> Option<IntOctagon> {
        self.inner.border.bounding_octagon()
    }

    /// True if `point` is contained in the border shape but in no hole.
    pub fn contains_float(&self, point: FloatPoint) -> bool {
        if !self.inner.border.contains_float(point) {
            return false;
        }
        !self.inner.holes.iter().any(|h| h.contains_float(point))
    }

    /// True if `point` is contained in the border shape but not in the interior of a
    /// hole.
    pub fn contains(&self, point: &Point) -> bool {
        if !self.inner.border.contains(point) {
            return false;
        }
        !self.inner.holes.iter().any(|h| h.contains_inside(point))
    }

    /// Approximation of the nearest point of the area to `from_point`, computed from the
    /// convex pieces. `None` if the division into convex pieces fails or is empty.
    pub fn nearest_point_approx(&self, from_point: FloatPoint) -> Option<FloatPoint> {
        let mut min_dist = f64::MAX;
        let mut result = None;
        for piece in self.split_to_convex(None)? {
            let Some(curr) = piece.nearest_point_approx(from_point) else {
                continue;
            };
            let curr_dist = curr.distance_square(from_point);
            if curr_dist < min_dist {
                min_dist = curr_dist;
                result = Some(curr);
            }
        }
        result
    }

    /// The area translated by `vector`.
    pub fn translate_by(&self, vector: &Vector) -> PolylineArea {
        if vector.is_zero() {
            return self.clone();
        }
        self.map_shapes(|s| s.translate_by(vector))
    }

    /// Approximations of the corners of the border followed by those of the holes.
    pub fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        let mut result = self.inner.border.corner_approx_arr();
        for hole in &self.inner.holes {
            result.extend(hole.corner_approx_arr());
        }
        result
    }

    /// Splits the area into convex pieces by cutting the convex hole pieces out of the
    /// convex border pieces. Not exact, because rounded line intersections are used.
    ///
    /// Returns `None` if a split fails, if a cutout fails, or if `stoppable` requests a
    /// stop. The result is cached only on success.
    pub fn split_to_convex(&self, stoppable: Option<&dyn Stoppable>) -> Option<Vec<TileShape>> {
        self.split_to_convex_with(stoppable, SplitQuirks::JAVA)
    }

    /// [`Self::split_to_convex`] with the switches of the cutouts (N06 batch G). Each
    /// switch value has its own cache.
    pub fn split_to_convex_with(
        &self,
        stoppable: Option<&dyn Stoppable>,
        quirks: SplitQuirks,
    ) -> Option<Vec<TileShape>> {
        self.split_to_convex_counted(stoppable, quirks, &mut SplitReach::default())
    }

    /// [`Self::split_to_convex_with`], adding to `reach` where the switches change a
    /// piece of the cutouts this call computes ([`SplitReach`]). A split served from
    /// the cache computes no cutout and adds nothing; a failed or stopped split adds
    /// the cutouts it made.
    pub fn split_to_convex_counted(
        &self,
        stoppable: Option<&dyn Stoppable>,
        quirks: SplitQuirks,
        reach: &mut SplitReach,
    ) -> Option<Vec<TileShape>> {
        let cache = &self.inner.convex_pieces[quirks.cache_slot()];
        if let Some(pieces) = cache.get() {
            return Some(pieces.clone());
        }
        let mut curr_pieces = self.inner.border.split_to_convex()?;
        for hole in &self.inner.holes {
            if hole.dimension() < 2 {
                // Java logs "PolylineArea. split_to_convex: dimension 2 for hole expected".
                continue;
            }
            let hole_pieces = hole.split_to_convex()?;
            for hole_piece in &hole_pieces {
                let mut new_pieces = Vec::new();
                for divide_piece in &curr_pieces {
                    if stoppable.is_some_and(|s| s.is_stop_requested()) {
                        return None;
                    }
                    for piece in divide_piece.cutout_counted(hole_piece, quirks, reach)? {
                        if piece.dimension() == 2 {
                            new_pieces.push(piece);
                        }
                    }
                }
                curr_pieces = new_pieces;
            }
        }
        Some(cache.get_or_init(|| curr_pieces).clone())
    }

    /// The area turned by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> PolylineArea {
        self.map_shapes(|s| s.turn_90_degree(factor, pole))
    }

    /// The area rotated by `angle` around `pole`; not exact.
    pub fn rotate_approx(&self, angle: f64, pole: FloatPoint) -> PolylineArea {
        self.map_shapes(|s| s.rotate_approx(angle, pole))
    }

    /// The area mirrored at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: IntPoint) -> PolylineArea {
        self.map_shapes(|s| s.mirror_vertical(pole))
    }

    /// The area mirrored at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: IntPoint) -> PolylineArea {
        self.map_shapes(|s| s.mirror_horizontal(pole))
    }

    fn map_shapes(&self, f: impl Fn(&PolylineShape) -> PolylineShape) -> PolylineArea {
        PolylineArea::new(
            f(&self.inner.border),
            self.inner.holes.iter().map(f).collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Flag(AtomicBool);

    impl Stoppable for Flag {
        fn request_stop(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
        fn is_stop_requested(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn frame() -> PolylineArea {
        PolylineArea::new(
            PolylineShape::from(IntBox::new(0, 0, 30, 30)),
            vec![PolylineShape::from(IntBox::new(10, 10, 20, 20))],
        )
    }

    #[test]
    fn contains_respects_holes() {
        let a = frame();
        assert!(a.contains(&Point::from(IntPoint::new(5, 5))));
        assert!(!a.contains(&Point::from(IntPoint::new(15, 15))));
        // On the hole border: not inside the hole.
        assert!(a.contains(&Point::from(IntPoint::new(10, 15))));
        assert!(a.contains_float(FloatPoint::new(5.0, 5.0)));
        assert!(!a.contains_float(FloatPoint::new(15.0, 15.0)));
        assert!(!a.contains_float(FloatPoint::new(40.0, 15.0)));
    }

    #[test]
    fn split_to_convex_cuts_out_holes() {
        let a = frame();
        let pieces = a.split_to_convex(None).expect("split succeeds");
        let total: f64 = pieces.iter().map(|p| p.area()).sum();
        assert_eq!(total, 800.0);
        for p in &pieces {
            assert_eq!(p.dimension(), 2);
            assert!(!p.contains_inside(&Point::from(IntPoint::new(15, 15))));
        }
        // Cached.
        assert_eq!(a.split_to_convex(None).unwrap().len(), pieces.len());
    }

    #[test]
    fn split_to_convex_can_be_stopped() {
        let a = frame();
        let flag = Flag(AtomicBool::new(false));
        flag.request_stop();
        assert!(a.split_to_convex(Some(&flag)).is_none());
        // Not cached after a stop.
        assert!(a.split_to_convex(None).is_some());
    }

    #[test]
    fn corner_approx_arr_concatenates_border_and_holes() {
        let a = frame();
        assert_eq!(a.corner_approx_arr().len(), 8);
        let t = a.translate_by(&Vector::from(crate::vector::IntVector::new(1, 2)));
        assert_eq!(t.bounding_box(), IntBox::new(1, 2, 31, 32));
        assert_eq!(t.get_holes()[0].bounding_box(), IntBox::new(11, 12, 21, 22));
    }

    #[test]
    fn nearest_point_approx_uses_convex_pieces() {
        let a = frame();
        let p = a.nearest_point_approx(FloatPoint::new(15.0, 14.0)).unwrap();
        assert!(p.distance(FloatPoint::new(15.0, 10.0)) < 1e-9);
    }
}

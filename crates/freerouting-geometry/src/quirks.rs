//! The geometry's switches for the Java bugs it keeps (N06).
//!
//! The geometry crate has no settings access, so the engine's
//! `router.java_quirks.*` switches reach it as explicit arguments:
//! [`SplitQuirks`] for the convex split of an area
//! ([`PolylineArea::split_to_convex_with`](crate::PolylineArea::split_to_convex_with),
//! [`TileShape::cutout_with`](crate::TileShape::cutout_with)) and a `bool` for the rational
//! perpendicular projection
//! ([`Point::perpendicular_projection_with`](crate::Point::perpendicular_projection_with),
//! [`TileShapeOps::nearest_point_with`](crate::TileShapeOps::nearest_point_with)).
//! `true` is Java's behaviour; the functions without `_with` keep it.
//!
//! [`SplitReach`] counts where the cutouts' switches matter (N06.4's reachability
//! counters): the `_counted` variants
//! ([`PolylineArea::split_to_convex_counted`](crate::PolylineArea::split_to_convex_counted),
//! [`TileShape::cutout_counted`](crate::TileShape::cutout_counted)) add to it, and the
//! engine reports the sums in its run statistics.

/// The switches of the convex split (N06 batch G). `true` keeps Java's bug.
///
/// - `simplex_prev_division_line`: `Simplex.cutoutFrom` assigns
///   `nextDivisionLine = prevDivisionLine` (`Simplex.java:860`), so the previous
///   division line stays `null`. Under `false` it is assigned the other way round,
///   `prevDivisionLine = nextDivisionLine`. That fix never changes a piece: the
///   assigned line is the next corner's first division line, and neither merge test
///   of that corner can pass with it (see
///   [`Simplex::cutout_from_counted`](crate::Simplex::cutout_from_counted)).
/// - `octagon_box_cutout_branch3`: branch 3 of `IntOctagon.cutoutFromBox`
///   (`IntOctagon.java:1206-1223`) moves the upper box to the upper right octagon's right
///   edge but rebuilds that octagon unchanged, so the two overlap. Under `false` the
///   octagon's top moves down to the box's bottom, as branch 2 does on the left.
/// - `cutout_hole_crosses_border` (N06.14): `Simplex.cutoutFrom` does not divide a
///   corner of the cut-out part that lies on the outer border; its stand-in division
///   line is the hole edge before it. Java still treats the stand-in as a division line
///   (`Simplex.java:731-732`, `:756-768`, `:809-824`): as the first one, which later
///   pieces are clipped with, and as a corner's last one, which turns that clipping on.
///   When the hole crosses the outer border, whole pieces vanish. Under `false` only
///   the lines of divided corners take part.
/// - `simplex_octagon_half_corner` (N06.14): `Simplex.calcDivisionLines`
///   (`Simplex.java:1042-1057`) takes every corner that is not an integer point for one
///   on the outer border and divides nothing there. An octagon whose diagonals meet at
///   a half-integer point, or an enlarged simplex, has such corners inside the outer
///   shape, and the pieces beside them overlap. Under `false` such a corner, unless it
///   lies on an outer border line, is divided at its exact point; as a line through it
///   need contain no integer point, the pieces on either side take the nearest line
///   through integer points on their own side
///   ([`Line::nearest_through_integer_points`](crate::Line::nearest_through_integer_points))
///   and overlap by a strip at most one unit wide.
///
/// Inside the domain of N06.4's invariant test (holes inside the outer shape with
/// integer corners) the two N06.14 switches change no piece.
///
/// The two N06.14 switches act only in
/// [`Simplex::cutout_from_counted`](crate::Simplex::cutout_from_counted), which
/// [`TileShape::cutout_counted`](crate::TileShape::cutout_counted) runs when either shape
/// is a simplex (a box or octagon is converted, as Java's `cutoutFrom(Simplex)`
/// overloads do). So they are reached in any-angle mode, whose tree keeps every obstacle's own
/// shape, and on a 45 degree board too: its search tree keeps the simplex shape of every trace
/// with a line that is not a multiple of 45 degrees, and the drill page or tie-pin cutout
/// with such a shape in it goes through the simplex code (N06.14's correction of
/// 2026-10-20). A 90 degree tree holds boxes only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SplitQuirks {
    pub simplex_prev_division_line: bool,
    pub octagon_box_cutout_branch3: bool,
    pub cutout_hole_crosses_border: bool,
    pub simplex_octagon_half_corner: bool,
}

impl SplitQuirks {
    /// Java's behaviour: every bug kept.
    pub const JAVA: SplitQuirks = SplitQuirks {
        simplex_prev_division_line: true,
        octagon_box_cutout_branch3: true,
        cutout_hole_crosses_border: true,
        simplex_octagon_half_corner: true,
    };

    /// Every bug fixed.
    pub const FIXED: SplitQuirks = SplitQuirks {
        simplex_prev_division_line: false,
        octagon_box_cutout_branch3: false,
        cutout_hole_crosses_border: false,
        simplex_octagon_half_corner: false,
    };

    /// The number of distinct values, the size of a piece cache that keeps one
    /// result per value.
    pub const COMBINATIONS: usize = 16;

    /// This value's slot in a piece cache of [`Self::COMBINATIONS`] entries: a
    /// cached split is never shared across flag values.
    pub fn cache_slot(self) -> usize {
        usize::from(self.simplex_prev_division_line)
            | (usize::from(self.octagon_box_cutout_branch3) << 1)
            | (usize::from(self.cutout_hole_crosses_border) << 2)
            | (usize::from(self.simplex_octagon_half_corner) << 3)
    }

    /// Every value of the four switches, in [`Self::cache_slot`] order.
    pub fn all() -> [SplitQuirks; SplitQuirks::COMBINATIONS] {
        std::array::from_fn(|slot| SplitQuirks {
            simplex_prev_division_line: slot & 1 != 0,
            octagon_box_cutout_branch3: slot & 2 != 0,
            cutout_hole_crosses_border: slot & 4 != 0,
            simplex_octagon_half_corner: slot & 8 != 0,
        })
    }
}

impl Default for SplitQuirks {
    fn default() -> Self {
        SplitQuirks::JAVA
    }
}

/// How often the cutouts reached the code of each [`SplitQuirks`] switch, in a way
/// that makes the two values of the switch give different pieces. The counts are the
/// same whatever the switches are, for the cutout that is computed: they look at
/// Java's state and at the fixed one side by side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SplitReach {
    /// Pieces of a simplex cutout for which the fixed previous division line passes a
    /// merge test, so that `false` adds a line Java leaves out. Always 0: see
    /// [`Simplex::cutout_from_counted`](crate::Simplex::cutout_from_counted).
    pub simplex_prev_division_line: u64,
    /// Octagon-from-box cutouts whose branch 3 fired with the upper right octagon
    /// reaching above the moved upper box, so that `false` trims the octagon and Java
    /// keeps the overlap.
    pub octagon_box_cutout_branch3: u64,
    /// Pieces of a simplex cutout that Java clips with the first division line although
    /// the first corner lies on the outer border and has none (`cutout_hole_crosses_border`).
    pub cutout_hole_crosses_border: u64,
    /// Corners of a simplex cutout that are not integer points and do not lie on the
    /// outer border, which Java leaves undivided (`simplex_octagon_half_corner`).
    pub simplex_octagon_half_corner: u64,
}

impl SplitReach {
    /// Adds `other`'s counts.
    pub fn add(&mut self, other: &SplitReach) {
        self.simplex_prev_division_line += other.simplex_prev_division_line;
        self.octagon_box_cutout_branch3 += other.octagon_box_cutout_branch3;
        self.cutout_hole_crosses_border += other.cutout_hole_crosses_border;
        self.simplex_octagon_half_corner += other.simplex_octagon_half_corner;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_java_and_cache_slots_are_distinct() {
        assert_eq!(SplitQuirks::default(), SplitQuirks::JAVA);
        let all = SplitQuirks::all();
        for (slot, q) in all.iter().enumerate() {
            assert_eq!(q.cache_slot(), slot);
        }
        let mut distinct = all.to_vec();
        distinct.dedup();
        assert_eq!(distinct.len(), SplitQuirks::COMBINATIONS);
        assert_eq!(all[0], SplitQuirks::FIXED);
        assert_eq!(all[SplitQuirks::COMBINATIONS - 1], SplitQuirks::JAVA);
    }
}

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SplitQuirks {
    pub simplex_prev_division_line: bool,
    pub octagon_box_cutout_branch3: bool,
}

impl SplitQuirks {
    /// Java's behaviour: every bug kept.
    pub const JAVA: SplitQuirks = SplitQuirks {
        simplex_prev_division_line: true,
        octagon_box_cutout_branch3: true,
    };

    /// Every bug fixed.
    pub const FIXED: SplitQuirks = SplitQuirks {
        simplex_prev_division_line: false,
        octagon_box_cutout_branch3: false,
    };

    /// The number of distinct values, the size of a piece cache that keeps one
    /// result per value.
    pub const COMBINATIONS: usize = 4;

    /// This value's slot in a piece cache of [`Self::COMBINATIONS`] entries: a
    /// cached split is never shared across flag values.
    pub fn cache_slot(self) -> usize {
        usize::from(self.simplex_prev_division_line)
            | (usize::from(self.octagon_box_cutout_branch3) << 1)
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
}

impl SplitReach {
    /// Adds `other`'s counts.
    pub fn add(&mut self, other: &SplitReach) {
        self.simplex_prev_division_line += other.simplex_prev_division_line;
        self.octagon_box_cutout_branch3 += other.octagon_box_cutout_branch3;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_java_and_cache_slots_are_distinct() {
        assert_eq!(SplitQuirks::default(), SplitQuirks::JAVA);
        let mut slots = Vec::new();
        for a in [false, true] {
            for b in [false, true] {
                let q = SplitQuirks {
                    simplex_prev_division_line: a,
                    octagon_box_cutout_branch3: b,
                };
                assert!(q.cache_slot() < SplitQuirks::COMBINATIONS);
                slots.push(q.cache_slot());
            }
        }
        slots.sort_unstable();
        slots.dedup();
        assert_eq!(slots.len(), SplitQuirks::COMBINATIONS);
    }
}

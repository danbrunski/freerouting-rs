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

/// The switches of the convex split (N06 batch G). `true` keeps Java's bug.
///
/// - `simplex_prev_division_line`: `Simplex.cutoutFrom` assigns
///   `nextDivisionLine = prevDivisionLine` (`Simplex.java:860`), so the previous
///   division line stays `null`. N06.4 adds the fix under `false`.
/// - `octagon_box_cutout_branch3`: branch 3 of `IntOctagon.cutoutFromBox`
///   (`IntOctagon.java:1206-1223`) rebuilds the octagon unchanged. N06.4 adds the fix
///   under `false`.
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

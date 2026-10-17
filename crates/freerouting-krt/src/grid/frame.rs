//! The grid frame (X04.2): which cells a raster covers, and the map between
//! cells and board points. All integer.
//!
//! Cell `(i, j)` has its centre at `(i·s, j·s)` board units for every frame
//! of step `s`, so two frames agree on every cell they share. A cell covers
//! the half-open square `((i − ½)·s, (i + ½)·s] × ((j − ½)·s, (j + ½)·s]`:
//! a point exactly on the boundary between two cells belongs to the one
//! with the lower index.

use freerouting::board::BasicBoard;
use freerouting::geometry::{IntBox, IntPoint};

use super::{GridTask, WINDOW_MARGIN_UM};
use freerouting::job::overrides::um_to_board_units;

/// A rectangle of cells `i0..i0 + nx` × `j0..j0 + ny` on `layers` layers,
/// at step `step` board units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The grid step in board units (positive).
    pub step: i32,
    /// The lowest cell column.
    pub i0: i32,
    /// The lowest cell row.
    pub j0: i32,
    /// The number of columns.
    pub nx: i32,
    /// The number of rows.
    pub ny: i32,
    /// The number of layers (the board's).
    pub layers: usize,
}

/// `ceil(a / b)` for `b > 0`.
fn ceil_div(a: i64, b: i64) -> i64 {
    -((-a).div_euclid(b))
}

impl Frame {
    /// The frame of the cells whose centres lie in `window` (borders
    /// included). Empty (`nx` or `ny` 0) if no centre does.
    pub fn covering(step: i32, window: &IntBox, layers: usize) -> Frame {
        assert!(step > 0, "grid step must be positive, got {step}");
        let s = i64::from(step);
        let lo = |v: i32| ceil_div(i64::from(v), s);
        let hi = |v: i32| i64::from(v).div_euclid(s);
        let (i0, i1) = (lo(window.ll.x), hi(window.ur.x));
        let (j0, j1) = (lo(window.ll.y), hi(window.ur.y));
        Frame {
            step,
            i0: i0 as i32,
            j0: j0 as i32,
            nx: (i1 - i0 + 1).max(0) as i32,
            ny: (j1 - j0 + 1).max(0) as i32,
            layers,
        }
    }

    /// The cell containing `p` (the lower index on a boundary).
    pub fn cell_of(&self, p: IntPoint) -> (i32, i32) {
        cell_of(self.step, p)
    }

    /// The centre of cell `(i, j)`.
    pub fn centre(&self, i: i32, j: i32) -> IntPoint {
        let s = i64::from(self.step);
        let x = i64::from(i) * s;
        let y = i64::from(j) * s;
        IntPoint::new(
            i32::try_from(x).expect("cell centre outside the i32 range"),
            i32::try_from(y).expect("cell centre outside the i32 range"),
        )
    }

    /// True if the frame has no cell.
    pub fn is_empty(&self) -> bool {
        self.nx <= 0 || self.ny <= 0
    }

    /// True if `(i, j)` is a cell of the frame.
    pub fn contains_cell(&self, i: i32, j: i32) -> bool {
        i >= self.i0 && i < self.i0 + self.nx && j >= self.j0 && j < self.j0 + self.ny
    }

    /// The index of `(i, j)` in a row-major `nx·ny` array, if in the frame.
    pub fn index(&self, i: i32, j: i32) -> Option<usize> {
        self.contains_cell(i, j)
            .then(|| (j - self.j0) as usize * self.nx as usize + (i - self.i0) as usize)
    }

    /// The number of cells per layer.
    pub fn cell_count(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            self.nx as usize * self.ny as usize
        }
    }

    /// The box spanned by the cell centres.
    pub fn centre_box(&self) -> IntBox {
        let ll = self.centre(self.i0, self.j0);
        let ur = self.centre(self.i0 + self.nx - 1, self.j0 + self.ny - 1);
        IntBox::from_points(ll, ur)
    }

    /// The inclusive cell range `(i_lo, i_hi, j_lo, j_hi)` of the frame's
    /// cells whose centres lie in `b` enlarged by `margin` on every side;
    /// `None` if there is none.
    pub fn cells_near(&self, b: &IntBox, margin: i32) -> Option<(i32, i32, i32, i32)> {
        if self.is_empty() || b.ll.x > b.ur.x || b.ll.y > b.ur.y {
            return None;
        }
        let s = i64::from(self.step);
        let m = i64::from(margin.max(0));
        let i_lo = ceil_div(i64::from(b.ll.x) - m, s).max(i64::from(self.i0));
        let i_hi = (i64::from(b.ur.x) + m)
            .div_euclid(s)
            .min(i64::from(self.i0 + self.nx - 1));
        let j_lo = ceil_div(i64::from(b.ll.y) - m, s).max(i64::from(self.j0));
        let j_hi = (i64::from(b.ur.y) + m)
            .div_euclid(s)
            .min(i64::from(self.j0 + self.ny - 1));
        (i_lo <= i_hi && j_lo <= j_hi).then_some((
            i_lo as i32,
            i_hi as i32,
            j_lo as i32,
            j_hi as i32,
        ))
    }
}

/// The cell of step `step` containing `p`: `ceil((2·x − s) / (2·s))` per
/// axis, so a boundary point `(i + ½)·s` maps to `i`, the lower index.
pub fn cell_of(step: i32, p: IntPoint) -> (i32, i32) {
    let s = i64::from(step);
    let axis = |v: i32| ceil_div(2 * i64::from(v) - s, 2 * s) as i32;
    (axis(p.x), axis(p.y))
}

/// The window of `task`: the bounding box of its start and destination
/// items enlarged by [`WINDOW_MARGIN_UM`], clipped to the bounding box of
/// the board outline (the board's box if it has none), at step `step`.
pub fn window_for(board: &BasicBoard, task: &GridTask, step: i32) -> Frame {
    let items = board.bounding_box_of(&task.items());
    let margin = i64::from(um_to_board_units(board, WINDOW_MARGIN_UM));
    let limit = board
        .outline()
        .map_or_else(|| board.bounding_box(), |o| o.bounding_box());
    let clamp = |v: i64, lo: i32, hi: i32| v.clamp(i64::from(lo), i64::from(hi)) as i32;
    let window = if items.ll.x > items.ur.x || items.ll.y > items.ur.y {
        // No item: an empty window.
        IntBox::new(1, 1, 0, 0)
    } else {
        IntBox::new(
            clamp(i64::from(items.ll.x) - margin, limit.ll.x, limit.ur.x),
            clamp(i64::from(items.ll.y) - margin, limit.ll.y, limit.ur.y),
            clamp(i64::from(items.ur.x) + margin, limit.ll.x, limit.ur.x),
            clamp(i64::from(items.ur.y) + margin, limit.ll.y, limit.ur.y),
        )
    };
    Frame::covering(step, &window, board.layer_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_points_map_to_the_lower_index() {
        let s = 1000;
        // (i + ½)·s is the boundary of cells i and i + 1.
        for i in -5..5 {
            let b = i * s + s / 2;
            assert_eq!(cell_of(s, IntPoint::new(b, b)), (i, i));
            assert_eq!(cell_of(s, IntPoint::new(b + 1, b + 1)), (i + 1, i + 1));
            assert_eq!(cell_of(s, IntPoint::new(i * s, i * s)), (i, i));
        }
        // An odd step: the boundary lies between two board units.
        assert_eq!(cell_of(5, IntPoint::new(2, 0)), (0, 0));
        assert_eq!(cell_of(5, IntPoint::new(3, 0)), (1, 0));
        assert_eq!(cell_of(5, IntPoint::new(-2, -3)), (0, -1));
    }

    #[test]
    fn overlapping_frames_share_centres() {
        let a = Frame::covering(1000, &IntBox::new(-3_456, 1_234, 20_000, 9_999), 2);
        let b = Frame::covering(1000, &IntBox::new(7_001, -5_000, 31_500, 6_500), 2);
        let mut shared = 0;
        for j in a.j0..a.j0 + a.ny {
            for i in a.i0..a.i0 + a.nx {
                if b.contains_cell(i, j) {
                    assert_eq!(a.centre(i, j), b.centre(i, j));
                    assert_eq!(a.cell_of(a.centre(i, j)), (i, j));
                    assert_eq!(b.cell_of(b.centre(i, j)), (i, j));
                    shared += 1;
                }
            }
        }
        // Columns 8..=20, rows 2..=6.
        assert_eq!(shared, 13 * 5);
        assert_eq!((a.i0, a.nx, a.j0, a.ny), (-3, 24, 2, 8));
    }

    #[test]
    fn cells_near_clips_to_the_frame() {
        let f = Frame::covering(100, &IntBox::new(0, 0, 1000, 1000), 1);
        assert_eq!((f.nx, f.ny), (11, 11));
        assert_eq!(
            f.cells_near(&IntBox::new(250, 250, 250, 250), 60),
            Some((2, 3, 2, 3))
        );
        assert_eq!(
            f.cells_near(&IntBox::new(-500, -500, 50, 50), 0),
            Some((0, 0, 0, 0))
        );
        assert_eq!(f.cells_near(&IntBox::new(2000, 0, 3000, 10), 10), None);
    }
}

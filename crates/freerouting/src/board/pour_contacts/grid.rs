//! K15.47: a uniform grid over the boxes of an opened fill's pieces, for
//! the point queries of the pour contacts ([`super::AreaIslands::island_at`]).
//!
//! The query is "the first piece, in piece order, whose box holds the point
//! and that passes a test", the answer of a scan over all pieces in order
//! (K15.41's `island_at` scanned every piece: on kit-dev-coldfire's route
//! about 24 000 pieces per fill and 900 000 queries, 25 s of a 216 s route).
//! Each cell lists, in ascending order, the pieces whose box meets it, and
//! the query scans the list of the point's cell. The cell of a coordinate
//! is a non-decreasing function of it (a floor of a correctly rounded
//! subtraction and division), so a box that holds a point is listed in the
//! point's cell, and a point outside the union of the boxes is in no piece:
//! the grid gives the scan's answer.

use freerouting_geometry::{FloatPoint, IntBox};

/// The grid aims at about this many cells per piece.
const CELLS_PER_PIECE: f64 = 1.0;
/// The most cells on one axis.
const MAX_AXIS: usize = 4096;
/// A grid whose lists hold more than this many entries per piece is made
/// coarser (long thin pieces cross many cells).
const MAX_ENTRIES_PER_PIECE: usize = 32;

/// The pieces' boxes binned by cell (compressed rows: `starts[c]..starts[c + 1]`
/// of `entries` lists cell `c`'s pieces in ascending order).
#[derive(Debug)]
pub(super) struct PieceGrid {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    cell_w: f64,
    cell_h: f64,
    nx: usize,
    ny: usize,
    starts: Vec<u32>,
    entries: Vec<u32>,
}

impl PieceGrid {
    /// The grid of `boxes` (the pieces' boxes, in piece order).
    pub(super) fn new(boxes: &[IntBox]) -> PieceGrid {
        let mut x0 = f64::INFINITY;
        let mut y0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for b in boxes {
            x0 = x0.min(f64::from(b.ll.x));
            y0 = y0.min(f64::from(b.ll.y));
            x1 = x1.max(f64::from(b.ur.x));
            y1 = y1.max(f64::from(b.ur.y));
        }
        let (w, h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
        let cells = (boxes.len() as f64 * CELLS_PER_PIECE).max(1.0);
        let mut nx = ((cells * w / h).sqrt().round() as usize).clamp(1, MAX_AXIS);
        let mut ny = ((cells * h / w).sqrt().round() as usize).clamp(1, MAX_AXIS);
        loop {
            let grid = PieceGrid::with_axes(boxes, [x0, y0, x1, y1], nx, ny);
            let coarse = nx == 1 && ny == 1;
            if coarse || grid.entries.len() <= MAX_ENTRIES_PER_PIECE * boxes.len().max(1) {
                return grid;
            }
            nx = nx.div_ceil(2);
            ny = ny.div_ceil(2);
        }
    }

    fn with_axes(boxes: &[IntBox], extent: [f64; 4], nx: usize, ny: usize) -> PieceGrid {
        let [x0, y0, x1, y1] = extent;
        let mut grid = PieceGrid {
            x0,
            y0,
            x1,
            y1,
            cell_w: (x1 - x0).max(1.0) / nx as f64,
            cell_h: (y1 - y0).max(1.0) / ny as f64,
            nx,
            ny,
            starts: vec![0; nx * ny + 1],
            entries: Vec::new(),
        };
        let spans: Vec<[usize; 4]> = boxes.iter().map(|b| grid.span(b)).collect();
        for [cx0, cy0, cx1, cy1] in &spans {
            for cy in *cy0..=*cy1 {
                for cx in *cx0..=*cx1 {
                    grid.starts[cy * nx + cx + 1] += 1;
                }
            }
        }
        for c in 0..nx * ny {
            grid.starts[c + 1] += grid.starts[c];
        }
        let mut fill: Vec<u32> = grid.starts[..nx * ny].to_vec();
        grid.entries = vec![0; grid.starts[nx * ny] as usize];
        for (i, [cx0, cy0, cx1, cy1]) in spans.iter().enumerate() {
            for cy in *cy0..=*cy1 {
                for cx in *cx0..=*cx1 {
                    let slot = &mut fill[cy * nx + cx];
                    grid.entries[*slot as usize] = i as u32;
                    *slot += 1;
                }
            }
        }
        grid
    }

    /// The cells a box meets: `[x first, y first, x last, y last]`.
    fn span(&self, b: &IntBox) -> [usize; 4] {
        [
            self.column(f64::from(b.ll.x)),
            self.row(f64::from(b.ll.y)),
            self.column(f64::from(b.ur.x)),
            self.row(f64::from(b.ur.y)),
        ]
    }

    /// The column of `x`, which lies in `x0..=x1`: non-decreasing in `x`.
    fn column(&self, x: f64) -> usize {
        (((x - self.x0) / self.cell_w).floor() as usize).min(self.nx - 1)
    }

    /// The row of `y`, which lies in `y0..=y1`: non-decreasing in `y`.
    fn row(&self, y: f64) -> usize {
        (((y - self.y0) / self.cell_h).floor() as usize).min(self.ny - 1)
    }

    /// The pieces, ascending, whose box may hold `point`: none when the
    /// point is outside every box's extent (or not a number).
    pub(super) fn candidates(&self, point: FloatPoint) -> &[u32] {
        let inside =
            point.x >= self.x0 && point.x <= self.x1 && point.y >= self.y0 && point.y <= self.y1;
        if !inside {
            return &[];
        }
        let c = self.row(point.y) * self.nx + self.column(point.x);
        &self.entries[self.starts[c] as usize..self.starts[c + 1] as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holds(b: &IntBox, p: FloatPoint) -> bool {
        f64::from(b.ll.x) <= p.x
            && p.x <= f64::from(b.ur.x)
            && f64::from(b.ll.y) <= p.y
            && p.y <= f64::from(b.ur.y)
    }

    /// Every box that holds a point is among the point's candidates, in
    /// ascending order, so the first that passes a test is the scan's.
    #[test]
    fn the_candidates_hold_every_box_that_holds_the_point() {
        let mut random = freerouting_geometry::jmath::JavaRandom::new(47);
        let mut boxes = Vec::new();
        for _ in 0..500 {
            let x = random.next_int(20_000) - 10_000;
            let y = random.next_int(20_000) - 10_000;
            let w = random.next_int(if boxes.len() % 7 == 0 { 15_000 } else { 400 });
            let h = random.next_int(if boxes.len() % 5 == 0 { 15_000 } else { 400 });
            boxes.push(IntBox::new(x, y, x + w, y + h));
        }
        boxes.push(IntBox::new(3, 3, 3, 3));
        let grid = PieceGrid::new(&boxes);
        let mut points: Vec<FloatPoint> = Vec::new();
        for b in &boxes {
            points.push(FloatPoint::new(f64::from(b.ll.x), f64::from(b.ll.y)));
            points.push(FloatPoint::new(f64::from(b.ur.x), f64::from(b.ur.y)));
            points.push(FloatPoint::new(f64::from(b.ur.x) + 0.5, f64::from(b.ll.y)));
        }
        for _ in 0..5000 {
            let x = f64::from(random.next_int(30_000) - 15_000) + 0.25;
            let y = f64::from(random.next_int(30_000) - 15_000);
            points.push(FloatPoint::new(x, y));
        }
        points.push(FloatPoint::new(f64::NAN, 0.0));
        for p in points {
            let scan: Vec<u32> = (0..boxes.len() as u32)
                .filter(|i| holds(&boxes[*i as usize], p))
                .collect();
            let listed: Vec<u32> = grid
                .candidates(p)
                .iter()
                .copied()
                .filter(|i| holds(&boxes[*i as usize], p))
                .collect();
            assert_eq!(listed, scan, "at {p:?}");
            assert!(grid.candidates(p).windows(2).all(|w| w[0] < w[1]));
        }
    }

    /// One box, and boxes of no extent, give a grid of one cell.
    #[test]
    fn degenerate_extents_give_one_cell() {
        let grid = PieceGrid::new(&[IntBox::new(5, 5, 5, 5)]);
        assert_eq!(grid.candidates(FloatPoint::new(5.0, 5.0)), &[0]);
        assert!(grid.candidates(FloatPoint::new(5.5, 5.0)).is_empty());
    }
}

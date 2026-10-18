//! Convex shapes defined as the intersection of half-planes.

use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::direction::IntDirection;
use crate::float_point::{FloatPoint, java_max, java_min};
use crate::jmath::d2i;
use crate::limits::CRIT_INT;
use crate::line::Line;
use crate::point::{IntPoint, Point};
use crate::quirks::{SplitQuirks, SplitReach};
use crate::side::Side;
use crate::vector::Vector;

use super::{IntBox, IntOctagon, PolylineShapeOps, TileShape, TileShapeOps};

struct SimplexInner {
    lines: Box<[Line]>,
    corners: Box<[OnceLock<Point>]>,
    float_corners: Box<[OnceLock<FloatPoint>]>,
    bounding_box: OnceLock<IntBox>,
    bounding_octagon: OnceLock<IntOctagon>,
}

/// A convex shape defined as the intersection of the half-planes on the left of
/// directed lines.
///
/// Port of `app.freerouting.geometry.planar.Simplex`. The value is immutable and cheap
/// to clone (the lines and the lazily computed corner caches are shared).
#[derive(Clone)]
pub struct Simplex {
    inner: Arc<SimplexInner>,
}

impl fmt::Debug for Simplex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Simplex")
            .field("lines", &self.inner.lines)
            .finish()
    }
}

fn sort_lines(lines: &mut [Line]) {
    // `Arrays.sort` on objects is a stable merge sort, as is `sort_by`.
    lines.sort_by(|a, b| a.compare_to(b).cmp(&0));
}

/// Lines up to this many are handled in stack buffers by the predicates
/// below (P02.2); longer ones fall back to the heap.
const SCRATCH_LINES: usize = 32;

/// The first step of [`Simplex::remove_redundant_lines`]: drops each line
/// that `fast_equals` the last line kept, in place. Returns the number kept
/// at the front.
fn dedup_lines(lines: &mut [Line]) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let mut kept = 1;
    for i in 1..lines.len() {
        if !lines[i].fast_equals(&lines[kept - 1]) {
            lines[kept] = lines[i];
            kept += 1;
        }
    }
    kept
}

/// The rest of [`Simplex::remove_redundant_lines`] on `lines` (sorted, no
/// consecutive duplicates): removes the redundant lines in place and
/// returns how many remain at the front; 0 means the simplex is empty.
fn reduce_redundant_lines(lines: &mut [Line]) -> usize {
    let mut new_length = lines.len();
    // On which side of each line the previous and the next line intersect.
    let mut stack_sides = [None; SCRATCH_LINES];
    let mut heap_sides;
    let intersection_sides: &mut [Option<Side>] = if new_length <= SCRATCH_LINES {
        &mut stack_sides[..new_length]
    } else {
        heap_sides = vec![None; new_length];
        &mut heap_sides
    };
    let mut try_again = new_length > 2;
    let mut index_of_last_removed_line = new_length as isize;
    while try_again {
        try_again = false;
        let mut prev_ind = new_length - 1;
        let mut prev_line = lines[prev_ind];
        let mut current_line = lines[0];
        let mut ind: isize = 0;
        while ind < new_length as isize {
            let u = ind as usize;
            let next_ind = if u == new_length - 1 { 0 } else { u + 1 };
            let next_line = lines[next_ind];
            let mut remove_line = false;
            let prev_dir = prev_line.int_direction();
            let next_dir = next_line.int_direction();
            let det = prev_dir.determinant(&next_dir);
            if det != 0.0 {
                let side = *intersection_sides[u].get_or_insert_with(|| {
                    current_line.side_of_intersection(&prev_line, &next_line)
                });
                if det > 0.0 {
                    // If the intersection of prev_line and next_line is not on the
                    // left of current_line, current_line is redundant.
                    remove_line = side != Side::OnTheLeft;
                } else if side == Side::OnTheLeft {
                    let current_direction = current_line.int_direction();
                    if prev_dir.determinant(&current_direction) > 0.0 {
                        // The simplex is empty.
                        new_length = 0;
                        break;
                    }
                }
            } else if prev_line.side_of_int(next_line.a) == Side::OnTheLeft {
                // Parallel half-planes without intersection.
                new_length = 0;
                break;
            }
            if remove_line {
                try_again = true;
                new_length -= 1;
                for i in u..new_length {
                    lines[i] = lines[i + 1];
                    intersection_sides[i] = intersection_sides[i + 1];
                }
                if new_length < 3 {
                    try_again = false;
                    break;
                }
                if u == 0 {
                    prev_ind = new_length - 1;
                }
                intersection_sides[prev_ind] = None;
                let reset_ind = if u >= new_length { 0 } else { u };
                intersection_sides[reset_ind] = None;
                ind -= 1;
                index_of_last_removed_line = ind;
            } else {
                prev_line = current_line;
                prev_ind = u;
            }
            current_line = next_line;
            if !try_again && ind >= index_of_last_removed_line {
                break;
            }
            ind += 1;
        }
        if new_length == 0 {
            try_again = false;
        }
    }
    if new_length == 2 && lines[0].is_parallel(&lines[1]) {
        if lines[0].direction() == lines[1].direction() {
            // One of the two remaining lines is redundant.
            if lines[1].side_of_int(lines[0].a) == Side::OnTheLeft {
                lines[0] = lines[1];
            }
            new_length -= 1;
        } else if lines[1].side_of_int(lines[0].a) == Side::OnTheLeft {
            // Opposite directions without intersection.
            new_length = 0;
        }
    }
    new_length
}

/// The division lines of one corner of a cutout (`Simplex.calcDivisionLines`).
struct Division {
    /// Java's lines: one, or two at a sharp corner. A piece on the left of a line uses
    /// it as it is.
    lines: Vec<Line>,
    /// The lines that pieces on their right use, reversed; `None` if they are `lines`.
    /// Only a corner that is not an integer point has them (the fixed arm of
    /// `simplex_octagon_half_corner`): no line through integer points need pass through
    /// it, so the pieces on either side take the nearest such line on their own side,
    /// and overlap by a strip at most one unit wide instead of leaving a gap.
    right_lines: Option<Vec<Line>>,
    /// False for a corner taken to lie on the outer border, whose line is a stand-in.
    divided: bool,
}

impl Division {
    fn undivided(line: Line) -> Division {
        Division {
            lines: vec![line],
            right_lines: None,
            divided: false,
        }
    }

    /// Line `no` as a piece on its right uses it: reversed.
    fn reversed(&self, no: usize) -> Line {
        let line = self.right_lines.as_ref().map_or(&self.lines, |r| r)[no];
        Line::new(line.b, line.a)
    }

    fn first(&self) -> Line {
        self.lines[0]
    }

    fn last(&self) -> Line {
        self.lines[self.lines.len() - 1]
    }
}

/// The first-division-line test of `Simplex.cutoutFrom` (`checkCrossFirstLine`,
/// `Simplex.java:756-768`, `:809-824`), in both arms of `cutout_hole_crosses_border`
/// (N06.14).
///
/// A corner of the cut-out part that `calc_division_lines` does not divide (one on the
/// outer border) gets the hole edge before it as a stand-in division line. Java treats
/// that stand-in as a division line twice: as the first division line, which later
/// pieces are clipped with, and as a corner's last division line, which turns the
/// crossing test on. Either way pieces of the rest vanish. The fixed arm uses only
/// lines of divided corners. Both arms are followed side by side, so that `reach` counts
/// the pieces whose clip differs whichever arm is computed.
struct CrossFirstLine {
    /// Java's `checkCrossFirstLine`.
    java: bool,
    /// The same, turned on only by the division line of a divided corner.
    fixed: bool,
    first_corner_divided: bool,
    keep_java: bool,
}

impl CrossFirstLine {
    fn new(first_corner_divided: bool, quirks: SplitQuirks) -> CrossFirstLine {
        CrossFirstLine {
            java: false,
            fixed: false,
            first_corner_divided,
            keep_java: quirks.cutout_hole_crosses_border,
        }
    }

    /// Java's `if (!checkCrossFirstLine) checkCrossFirstLine = crosses;` for a corner's
    /// division line, divided or a stand-in.
    fn turn_on(&mut self, crosses: bool, corner_divided: bool) {
        self.java |= crosses;
        self.fixed |= crosses && corner_divided;
    }

    /// Whether the piece is clipped by the first division line, given that its other
    /// line lies before the first one (`determinant < 0`).
    fn clips(&self, before_first: bool, reach: &mut SplitReach) -> bool {
        let java = self.java && before_first;
        let fixed = self.fixed && before_first && self.first_corner_divided;
        if java != fixed {
            reach.cutout_hole_crosses_border += 1;
        }
        if self.keep_java { java } else { fixed }
    }
}

/// `!Simplex::new(a ++ b sorted).remove_redundant_lines().is_empty()`, the
/// emptiness test of [`Simplex::intersection_simplex`], without building a
/// simplex: the same lines in the same order through the same steps, in a
/// stack buffer. `a` and `b` are the lines of two simplexes, `a` first.
fn lines_intersect(a: &[Line], b: &[Line]) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let n = a.len() + b.len();
    let mut stack = [a[0]; SCRATCH_LINES];
    let mut heap;
    let buffer: &mut [Line] = if n <= SCRATCH_LINES {
        &mut stack[..n]
    } else {
        heap = vec![a[0]; n];
        &mut heap
    };
    buffer[..a.len()].copy_from_slice(a);
    buffer[a.len()..].copy_from_slice(b);
    sort_lines(buffer);
    let distinct = dedup_lines(buffer);
    reduce_redundant_lines(&mut buffer[..distinct]) > 0
}

/// The lines of `o.to_simplex()` in `out`, without building it; returns
/// their number (0 for an empty octagon or simplex).
pub(crate) fn octagon_simplex_lines(o: &IntOctagon, out: &mut [Line; 8]) -> usize {
    if o.is_empty() {
        return 0;
    }
    for (i, line) in out.iter_mut().enumerate() {
        *line = o.border_line(i);
    }
    let distinct = dedup_lines(out);
    reduce_redundant_lines(&mut out[..distinct])
}

/// The lines of `b.to_simplex()` in `out`; returns their number (0 for an
/// empty box).
pub(crate) fn box_simplex_lines(b: &IntBox, out: &mut [Line; 4]) -> usize {
    if b.is_empty() {
        return 0;
    }
    *out = [
        Line::get_instance(b.ll, IntDirection::RIGHT),
        Line::get_instance(b.ur, IntDirection::UP),
        Line::get_instance(b.ur, IntDirection::LEFT),
        Line::get_instance(b.ll, IntDirection::DOWN),
    ];
    4
}

impl Simplex {
    /// Creates a simplex from the directed lines without normalizing it (Java
    /// `new Simplex(Line[])`); use [`Simplex::get_instance`] for a normalized one.
    pub fn new(lines: Vec<Line>) -> Simplex {
        let n = lines.len();
        Simplex {
            inner: Arc::new(SimplexInner {
                lines: lines.into_boxed_slice(),
                corners: (0..n).map(|_| OnceLock::new()).collect(),
                float_corners: (0..n).map(|_| OnceLock::new()).collect(),
                bounding_box: OnceLock::new(),
                bounding_octagon: OnceLock::new(),
            }),
        }
    }

    /// The empty simplex (Java `Simplex.EMPTY`).
    pub fn empty() -> Simplex {
        Simplex::new(Vec::new())
    }

    /// The intersection of the half-planes on the left of `lines`: the lines are sorted
    /// by direction and redundant lines are removed.
    pub fn get_instance(lines: &[Line]) -> Simplex {
        if lines.is_empty() {
            return Simplex::empty();
        }
        let mut current = lines.to_vec();
        sort_lines(&mut current);
        Simplex::new(current).remove_redundant_lines()
    }

    /// The border lines.
    pub fn lines(&self) -> &[Line] {
        &self.inner.lines
    }

    fn clamp_index(&self, index: usize) -> usize {
        index.min(self.inner.lines.len().wrapping_sub(1))
    }

    /// Converts this simplex to an `IntBox` or `IntOctagon`, if possible.
    pub fn simplify(&self) -> TileShape {
        if self.is_empty() {
            TileShape::Simplex(Simplex::empty())
        } else if self.is_int_box() {
            TileShape::IntBox(self.bounding_box())
        } else if self.is_int_octagon() {
            match self.to_int_octagon() {
                Some(o) => TileShape::IntOctagon(o),
                None => TileShape::Simplex(self.clone()),
            }
        } else {
            TileShape::Simplex(self.clone())
        }
    }

    /// The equivalent `IntOctagon`; `None` if not all lines are multiples of 45 degrees.
    pub fn to_int_octagon(&self) -> Option<IntOctagon> {
        if !self.is_int_octagon() {
            return None;
        }
        if self.is_empty() {
            return Some(IntOctagon::EMPTY);
        }
        let mut rx = CRIT_INT;
        let mut uy = CRIT_INT;
        let mut lrx = CRIT_INT;
        let mut urx = CRIT_INT;
        let mut lx = -CRIT_INT;
        let mut ly = -CRIT_INT;
        let mut llx = -CRIT_INT;
        let mut ulx = -CRIT_INT;
        for line in self.lines() {
            let (a, b) = (line.a, line.b);
            if a.y == b.y {
                if b.x >= a.x {
                    ly = a.y;
                }
                if b.x <= a.x {
                    uy = a.y;
                }
            }
            if a.x == b.x {
                if b.y >= a.y {
                    rx = a.x;
                }
                if b.y <= a.y {
                    lx = a.x;
                }
            }
            if a.y < b.y {
                if a.x < b.x {
                    lrx = a.x.wrapping_sub(a.y);
                } else if a.x > b.x {
                    urx = a.x.wrapping_add(a.y);
                }
            } else if a.y > b.y {
                if a.x < b.x {
                    llx = a.x.wrapping_add(a.y);
                } else if a.x > b.x {
                    ulx = a.x.wrapping_sub(a.y);
                }
            }
        }
        Some(IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx).normalize())
    }

    /// The simplex translated by `vector` (not normalized).
    ///
    /// # Panics
    /// If the translated lines are not defined by integer points.
    pub fn translate_by(&self, vector: &Vector) -> Simplex {
        if vector.is_zero_constant() {
            return self.clone();
        }
        Simplex::new(
            self.lines()
                .iter()
                .map(|l| l.translate_by(vector))
                .collect(),
        )
    }

    /// The simplex offset by `width`: to the outside if positive, else to the inside.
    pub fn offset(&self, width: f64) -> Simplex {
        if width == 0.0 {
            return self.clone();
        }
        let offset_simplex =
            Simplex::new(self.lines().iter().map(|l| l.translate(-width)).collect());
        if width < 0.0 {
            return offset_simplex.remove_redundant_lines();
        }
        offset_simplex
    }

    /// This simplex enlarged by `offset`, intersected with the enlarged bounding octagon.
    pub fn enlarge(&self, offset: f64) -> Simplex {
        if offset == 0.0 {
            return self.clone();
        }
        let offset_simplex = self.offset(offset);
        let Some(bounding_oct) = self.bounding_octagon() else {
            return Simplex::empty();
        };
        let offset_oct = bounding_oct.offset(offset);
        offset_simplex.intersection_simplex(&offset_oct.to_simplex())
    }

    /// The number of the right most corner seen from `from_point` (exact version with a
    /// `Point` argument).
    pub fn index_of_right_most_corner_exact(&self, from_point: &Point) -> usize {
        let mut right_most_corner = self.corner(0);
        let mut result = 0;
        for i in 1..self.lines().len() {
            let current_corner = self.corner(i);
            if current_corner.side_of(from_point, &right_most_corner) == Side::OnTheRight {
                right_most_corner = current_corner;
                result = i;
            }
        }
        result
    }

    /// The intersection with `b`.
    pub fn intersection_box(&self, b: &IntBox) -> Simplex {
        self.intersection_simplex(&b.to_simplex())
    }

    /// The intersection with `other`.
    pub fn intersection_octagon(&self, other: &IntOctagon) -> Simplex {
        self.intersection_simplex(&other.to_simplex())
    }

    /// The intersection with `other`: the lines of this simplex followed by those of
    /// `other`, sorted and normalized.
    pub fn intersection_simplex(&self, other: &Simplex) -> Simplex {
        if self.is_empty() || other.is_empty() {
            return Simplex::empty();
        }
        let mut new_array = Vec::with_capacity(self.lines().len() + other.lines().len());
        new_array.extend_from_slice(self.lines());
        new_array.extend_from_slice(other.lines());
        sort_lines(&mut new_array);
        Simplex::new(new_array).remove_redundant_lines()
    }

    /// True if this simplex and `other` intersect: the intersection is not
    /// empty, tested without building it.
    pub fn intersects_simplex(&self, other: &Simplex) -> bool {
        lines_intersect(self.lines(), other.lines())
    }

    /// `self.intersects_simplex(&o.to_simplex())` without building either
    /// simplex.
    pub fn intersects_octagon_simplex(&self, o: &IntOctagon) -> bool {
        let mut lines = [Line::get_instance(IntPoint::new(0, 0), IntDirection::RIGHT); 8];
        let n = octagon_simplex_lines(o, &mut lines);
        lines_intersect(self.lines(), &lines[..n])
    }

    /// `self.intersects_simplex(&b.to_simplex())` without building either
    /// simplex.
    pub fn intersects_box_simplex(&self, b: &IntBox) -> bool {
        let mut lines = [Line::get_instance(IntPoint::new(0, 0), IntDirection::RIGHT); 4];
        let n = box_simplex_lines(b, &mut lines);
        lines_intersect(self.lines(), &lines[..n])
    }

    /// This simplex without border line `no` (it may become unbounded).
    pub fn remove_border_line(&self, no: usize) -> Simplex {
        if no >= self.lines().len() {
            return self.clone();
        }
        let mut new_array = self.lines().to_vec();
        new_array.remove(no);
        Simplex::new(new_array)
    }

    /// Cuts this simplex out of `outer_simplex` and divides the rest into convex pieces
    /// along minimal distance lines from the inner corners to the outer simplex. `None`
    /// if this simplex is not 2-dimensional (Java returns `null`).
    pub fn cutout_from(&self, outer_simplex: &Simplex) -> Option<Vec<Simplex>> {
        self.cutout_from_with(outer_simplex, SplitQuirks::JAVA)
    }

    /// [`Self::cutout_from`] with the switches of the cutouts (N06 batch G).
    pub fn cutout_from_with(
        &self,
        outer_simplex: &Simplex,
        quirks: SplitQuirks,
    ) -> Option<Vec<Simplex>> {
        self.cutout_from_counted(outer_simplex, quirks, &mut SplitReach::default())
    }

    /// [`Self::cutout_from_with`], adding to `reach` the pieces where
    /// `simplex_prev_division_line` could matter.
    ///
    /// Java never assigns `prevDivisionLine` (`Simplex.java:860` assigns
    /// `nextDivisionLine = prevDivisionLine` instead), so both merges of the previous
    /// division line are dead code. The `false` path assigns it the other way round: at
    /// corner `i` the previous division line is `D[i][0]`, the first division line of
    /// corner `i` itself (the previous iteration's next division line). Neither merge
    /// test can pass with it:
    /// - the sharp-corner piece tests `det(D[i][0], prev) > 0`, which is
    ///   `det(D[i][0], D[i][0]) = 0`;
    /// - the edge piece tests `det(D[i][last], prev) > 0`. With one division line that
    ///   is again `det(D[i][0], D[i][0]) = 0`. With two, `calc_division_lines` accepts a
    ///   second projection only while `det(D[i][0], D[i][1]) >= 0` (a negative value
    ///   breaks off with "the angle would already be bigger than 180 degrees"), so
    ///   `det(D[i][1], D[i][0]) <= 0`.
    ///
    /// So the fix that the inventory names gives Java's pieces; `reach` counts the
    /// merges it makes, which stay 0, and `tests/cutout_invariant.rs` checks both values
    /// give the same pieces.
    pub fn cutout_from_counted(
        &self,
        outer_simplex: &Simplex,
        quirks: SplitQuirks,
        reach: &mut SplitReach,
    ) -> Option<Vec<Simplex>> {
        if self.dimension() < 2 {
            return None;
        }
        let inner_simplex = self.intersection_simplex(outer_simplex);
        if inner_simplex.dimension() < 2 {
            return Some(vec![outer_simplex.clone()]);
        }
        let inner_corner_count = inner_simplex.lines().len();
        let mut division_line_arr: Vec<Division> = Vec::with_capacity(inner_corner_count);
        for inner_corner_no in 0..inner_corner_count {
            match inner_simplex.calc_division_lines(inner_corner_no, outer_simplex, quirks, reach) {
                Some(division) => division_line_arr.push(division),
                None => return Some(vec![outer_simplex.clone()]),
            }
        }
        // PARITY(quirk, java_quirks.cutout_hole_crosses_border): Simplex.java:731-732,
        // 756-768, 809-824. A corner on the outer border is not divided; Java uses its
        // stand-in division line, the hole edge before it, as the first division line and
        // to turn the crossing test on, and pieces of the rest vanish (a hole crossing the
        // outer border). Under `true` the stand-ins act as in Java; under `false` only
        // divided corners' lines do ([`CrossFirstLine`], which counts `reach` in both arms).
        let mut cross_first = CrossFirstLine::new(division_line_arr[0].divided, quirks);
        // PARITY(quirk, java_quirks.simplex_prev_division_line): Java declares
        // `prevDivisionLine` but never assigns it a value (the statement at the end of the
        // loop, `Simplex.java:860`, is `nextDivisionLine = prevDivisionLine`, backwards), so
        // it stays `null` and a piece at a sharp inner corner can overlap its predecessor.
        // Under `true` it stays `None`; under `false` it is the previous corner's next
        // division line (`fixed_prev_division_line`, kept in both arms for `reach`).
        let mut fixed_prev_division_line: Option<Line> = None;
        let first_division = &division_line_arr[0];
        let first_direction = first_division.first().int_direction();
        let mut result_list = Vec::new();

        for inner_corner_no in 0..inner_corner_count {
            let prev_division_line = if quirks.simplex_prev_division_line {
                None
            } else {
                fixed_prev_division_line
            };
            let fixed_merge = |dir: IntDirection| {
                fixed_prev_division_line.is_some_and(|p| dir.determinant(&p.int_direction()) > 0.0)
            };
            let next_corner_no = (inner_corner_no + 1) % inner_corner_count;
            let next_division = &division_line_arr[next_corner_no];
            let next_division_line = next_division.first();
            let current = &division_line_arr[inner_corner_no];
            let current_division_lines = &current.lines;
            if current_division_lines.len() == 2 {
                // Two division lines are necessary (sharp corner).
                let current_direction = current_division_lines[0].int_direction();
                if fixed_merge(current_direction) {
                    reach.simplex_prev_division_line += 1;
                }
                let merge_prev_division_line = prev_division_line
                    .is_some_and(|p| current_direction.determinant(&p.int_direction()) > 0.0);
                cross_first.turn_on(
                    inner_corner_no > 0 && current_direction.determinant(&first_direction) > 0.0,
                    current.divided,
                );
                let current_dir_2 = current_division_lines[1].int_direction();
                let merge_first_division_line =
                    cross_first.clips(current_dir_2.determinant(&first_direction) < 0.0, reach);
                let mut piece_lines = vec![current.reversed(1), current_division_lines[0]];
                if let Some(p) = prev_division_line.filter(|_| merge_prev_division_line) {
                    piece_lines.push(p);
                }
                if merge_first_division_line {
                    piece_lines.push(first_division.reversed(0));
                }
                result_list.push(Simplex::new(piece_lines).intersection_simplex(outer_simplex));
            }
            let merge_next_division_line = next_division_line.b != next_division_line.a;
            let last_curr_division_line = current.last();
            let last_curr_dir = last_curr_division_line.int_direction();
            let merge_last_curr_division_line =
                last_curr_division_line.b != last_curr_division_line.a;
            if fixed_merge(last_curr_dir) {
                reach.simplex_prev_division_line += 1;
            }
            let merge_prev_division_line = prev_division_line
                .is_some_and(|p| last_curr_dir.determinant(&p.int_direction()) > 0.0);
            cross_first.turn_on(
                inner_corner_no > 0
                    && last_curr_dir.determinant(&first_direction) > 0.0
                    && last_curr_dir
                        .get_vector()
                        .scalar_product_int(first_direction.get_vector())
                        < 0.0,
                current.divided,
            );
            let next_dir = next_division_line.int_direction();
            let merge_first_division_line =
                cross_first.clips(next_dir.determinant(&first_direction) < 0.0, reach);
            let current_line = inner_simplex.lines()[inner_corner_no];
            let mut piece_lines = vec![Line::new(current_line.b, current_line.a)];
            if merge_next_division_line {
                piece_lines.push(next_division.reversed(0));
            }
            if merge_last_curr_division_line {
                piece_lines.push(last_curr_division_line);
            }
            if let Some(p) = prev_division_line.filter(|_| merge_prev_division_line) {
                piece_lines.push(p);
            }
            if merge_first_division_line {
                piece_lines.push(first_division.reversed(0));
            }
            result_list.push(Simplex::new(piece_lines).intersection_simplex(outer_simplex));
            fixed_prev_division_line = Some(next_division_line);
        }
        Some(result_list)
    }

    /// Removes lines which are redundant in the definition of the shape. Assumes that the
    /// lines are sorted.
    ///
    /// Java fails with an `ArrayIndexOutOfBoundsException` for an empty simplex; this
    /// port returns it unchanged.
    pub(crate) fn remove_redundant_lines(&self) -> Simplex {
        let src = self.lines();
        if src.is_empty() {
            return self.clone();
        }
        let mut lines: Vec<Line> = src.to_vec();
        let distinct = dedup_lines(&mut lines);
        let new_length = reduce_redundant_lines(&mut lines[..distinct]);
        if new_length == src.len() {
            return self.clone();
        }
        if new_length == 0 {
            return Simplex::empty();
        }
        lines.truncate(new_length);
        Simplex::new(lines)
    }

    /// For corner `inner_corner_no` of this inner simplex, one or two perpendicular
    /// projections onto lines of `outer_simplex`, so that the pieces after cutting out
    /// the inner simplex are convex. A corner taken to lie on the outer border is not
    /// divided ([`Division::divided`]).
    fn calc_division_lines(
        &self,
        inner_corner_no: usize,
        outer_simplex: &Simplex,
        quirks: SplitQuirks,
        reach: &mut SplitReach,
    ) -> Option<Division> {
        let lines = self.lines();
        let current_inner_line = lines[inner_corner_no];
        let prev_inner_line = if inner_corner_no != 0 {
            lines[inner_corner_no - 1]
        } else {
            lines[lines.len() - 1]
        };
        let intersection = current_inner_line.intersection_approx(&prev_inner_line);
        if intersection.x >= f64::from(i32::MAX) {
            return None;
        }
        let inner_corner = intersection.round();
        let ctolerance = 0.0001;
        let is_exact = (f64::from(inner_corner.x) - intersection.x).abs() < ctolerance
            && (f64::from(inner_corner.y) - intersection.y).abs() < ctolerance;
        let outer_lines = outer_simplex.lines();
        if !is_exact {
            // Java assumes that corners which are not exact come from the intersection
            // with the outer simplex and lie on its border: no division is necessary.
            // PARITY(quirk, java_quirks.simplex_octagon_half_corner): Simplex.java:1042-1057.
            // A hole's own corner can be inexact too: an octagon whose diagonals meet at a
            // half-integer point, as a simplex, or a corner of an offset simplex. Under
            // `true` it stays undivided, as in Java, and the pieces beside it overlap; under
            // `false` a corner on no outer line is divided at its exact point, along the
            // nearest lines through integer points. `reach` counts such corners in both
            // arms.
            let on_border = outer_lines.iter().any(|outer_line| {
                outer_line.side_of_intersection(&prev_inner_line, &current_inner_line)
                    == Side::Collinear
            });
            if !on_border {
                reach.simplex_octagon_half_corner += 1;
            }
            if on_border || quirks.simplex_octagon_half_corner {
                return Some(Division::undivided(prev_inner_line));
            }
        }
        // Only the fixed arm of `simplex_octagon_half_corner` gets here with an inexact
        // corner; Java's corners are all exact here.
        let (inner_corner_point, corner_float) = if is_exact {
            (Point::Int(inner_corner), inner_corner.to_float())
        } else {
            (
                current_inner_line.intersection(&prev_inner_line),
                intersection,
            )
        };
        let division_line = |dir: IntDirection, point_on_left: bool| {
            Line::nearest_through_integer_points(&inner_corner_point, dir, point_on_left)
                .unwrap_or_else(|| Line::from_point_and_direction(inner_corner, dir))
        };
        let mut first_projection_dir = IntDirection::NULL;
        let mut second_projection_dir = IntDirection::NULL;
        let prev_inner_dir = prev_inner_line.int_direction().opposite();
        let next_inner_dir = current_inner_line.int_direction();
        let mut outer_line_no = 0;
        let mut min_distance = f64::from(i32::MAX);
        for _ in 0..outer_lines.len() {
            let outer_line = outer_lines[outer_line_no];
            let current_projection_dir = inner_corner_point
                .perpendicular_direction(&outer_line)
                .expect_int();
            if current_projection_dir == IntDirection::NULL {
                return Some(Division::undivided(Line::new(inner_corner, inner_corner)));
            }
            let projection_visible = prev_inner_dir.determinant(&current_projection_dir) >= 0.0;
            if projection_visible {
                let mut current_distance = outer_line.signed_distance(corner_float).abs();
                let second_division_necessary =
                    current_projection_dir.determinant(&next_inner_dir) < 0.0;
                let mut current_second_projection_dir = current_projection_dir;
                if second_division_necessary {
                    let mut second_projection_visible = false;
                    let mut tmp_outer_line_no = outer_line_no;
                    while !second_projection_visible {
                        if tmp_outer_line_no == outer_lines.len() - 1 {
                            tmp_outer_line_no = 0;
                        } else {
                            tmp_outer_line_no += 1;
                        }
                        current_second_projection_dir = inner_corner_point
                            .perpendicular_direction(&outer_lines[tmp_outer_line_no])
                            .expect_int();
                        if current_second_projection_dir == IntDirection::NULL {
                            return Some(Division::undivided(Line::new(
                                inner_corner,
                                inner_corner,
                            )));
                        }
                        if current_projection_dir.determinant(&current_second_projection_dir) < 0.0
                        {
                            // The angle would already be bigger than 180 degrees.
                            current_distance = f64::from(i32::MAX);
                            break;
                        }
                        second_projection_visible =
                            current_second_projection_dir.determinant(&next_inner_dir) >= 0.0;
                    }
                    current_distance += outer_lines[tmp_outer_line_no]
                        .signed_distance(corner_float)
                        .abs();
                }
                if current_distance < min_distance {
                    min_distance = current_distance;
                    first_projection_dir = current_projection_dir;
                    second_projection_dir = current_second_projection_dir;
                }
            }
            if outer_line_no == outer_lines.len() - 1 {
                outer_line_no = 0;
            } else {
                outer_line_no += 1;
            }
        }
        if min_distance == f64::from(i32::MAX) {
            return None;
        }
        let dirs: &[IntDirection] = if first_projection_dir == second_projection_dir {
            &[first_projection_dir]
        } else {
            &[first_projection_dir, second_projection_dir]
        };
        let (lines, right_lines) = if is_exact {
            let lines = dirs
                .iter()
                .map(|&dir| Line::from_point_and_direction(inner_corner, dir))
                .collect();
            (lines, None)
        } else {
            // A piece on the left of a line takes the one with the corner on its left
            // too, and the other way round, so that the pieces overlap.
            let lines = dirs.iter().map(|&dir| division_line(dir, true)).collect();
            let right_lines = dirs.iter().map(|&dir| division_line(dir, false)).collect();
            (lines, Some(right_lines))
        };
        Some(Division {
            lines,
            right_lines,
            divided: true,
        })
    }

    fn compute_bounding_octagon(&self) -> Option<IntOctagon> {
        let mut lx = f64::from(i32::MAX);
        let mut ly = f64::from(i32::MAX);
        let mut rx = f64::from(i32::MIN);
        let mut uy = f64::from(i32::MIN);
        let mut ulx = f64::from(i32::MAX);
        let mut lrx = f64::from(i32::MIN);
        let mut llx = f64::from(i32::MAX);
        let mut urx = f64::from(i32::MIN);
        for i in 0..self.lines().len() {
            let current = self.corner_approx(i);
            lx = java_min(lx, current.x);
            ly = java_min(ly, current.y);
            rx = java_max(rx, current.x);
            uy = java_max(uy, current.y);
            let tmp = current.x - current.y;
            ulx = java_min(ulx, tmp);
            lrx = java_max(lrx, tmp);
            let tmp = current.x + current.y;
            llx = java_min(llx, tmp);
            urx = java_max(urx, tmp);
        }
        let crit = f64::from(CRIT_INT);
        if java_min(lx, ly) < -crit
            || java_max(rx, uy) > crit
            || java_min(ulx, llx) < -crit
            || java_max(lrx, urx) > crit
        {
            return None;
        }
        Some(IntOctagon::new(
            d2i(lx.floor()),
            d2i(ly.floor()),
            d2i(rx.ceil()),
            d2i(uy.ceil()),
            d2i(ulx.floor()),
            d2i(lrx.ceil()),
            d2i(llx.floor()),
            d2i(urx.ceil()),
        ))
    }
}

impl PolylineShapeOps for Simplex {
    fn border_line_count(&self) -> usize {
        self.inner.lines.len()
    }

    /// The intersection of the lines `no - 1` and `no`. Out of range numbers are clamped
    /// like in Java.
    ///
    /// # Panics
    /// If the simplex is empty.
    fn corner(&self, corner_index: usize) -> Point {
        let no = self.clamp_index(corner_index);
        let lines = self.lines();
        self.inner.corners[no]
            .get_or_init(|| {
                let prev = if no == 0 {
                    lines[lines.len() - 1]
                } else {
                    lines[no - 1]
                };
                lines[no].intersection(&prev)
            })
            .clone()
    }

    /// The border line `no`, clamped to the valid range like in Java.
    ///
    /// # Panics
    /// If the simplex is empty (Java returns `null`).
    fn border_line(&self, edge_index: usize) -> Line {
        assert!(
            !self.lines().is_empty(),
            "Simplex.borderLine: simplex is empty"
        );
        self.lines()[self.clamp_index(edge_index)]
    }

    fn corner_is_bounded(&self, corner_index: usize) -> bool {
        let no = self.clamp_index(corner_index);
        let lines = self.lines();
        if lines.len() == 1 {
            return false;
        }
        let prev_no = if no == 0 { lines.len() - 1 } else { no - 1 };
        let prev_dir = lines[prev_no].int_direction().get_vector();
        let current_direction = lines[no].int_direction().get_vector();
        prev_dir.determinant(current_direction) > 0
    }

    fn is_empty(&self) -> bool {
        self.lines().is_empty()
    }

    fn is_bounded(&self) -> bool {
        let n = self.lines().len();
        if n == 0 {
            return true;
        }
        if n < 3 {
            return false;
        }
        (0..n).all(|i| self.corner_is_bounded(i))
    }

    fn dimension(&self) -> i32 {
        let lines = self.lines();
        match lines.len() {
            0 => -1,
            1 => 2,
            2 => {
                if lines[0].overlaps(&lines[1]) {
                    1
                } else {
                    2
                }
            }
            3 => {
                if lines[0].overlaps(&lines[1])
                    || lines[0].overlaps(&lines[2])
                    || lines[1].overlaps(&lines[2])
                {
                    return 1;
                }
                let intersection = lines[1].intersection(&lines[2]);
                match lines[0].side_of(&intersection) {
                    Side::OnTheRight => 2,
                    Side::OnTheLeft => -1,
                    Side::Collinear => 0,
                }
            }
            4 => {
                let collinear02 = lines[0].overlaps(&lines[2]);
                let collinear13 = lines[1].overlaps(&lines[3]);
                if collinear02 && collinear13 {
                    0
                } else if collinear02 || collinear13 {
                    1
                } else {
                    2
                }
            }
            _ => 2,
        }
    }

    fn bounding_box(&self) -> IntBox {
        if self.lines().is_empty() {
            return IntBox::EMPTY;
        }
        *self.inner.bounding_box.get_or_init(|| {
            let mut llx = f64::from(i32::MAX);
            let mut lly = f64::from(i32::MAX);
            let mut urx = f64::from(i32::MIN);
            let mut ury = f64::from(i32::MIN);
            for i in 0..self.lines().len() {
                let current = self.corner_approx(i);
                llx = java_min(llx, current.x);
                lly = java_min(lly, current.y);
                urx = java_max(urx, current.x);
                ury = java_max(ury, current.y);
            }
            IntBox::from_points(
                IntPoint::new(d2i(llx.floor()), d2i(lly.floor())),
                IntPoint::new(d2i(urx.ceil()), d2i(ury.ceil())),
            )
        })
    }

    fn bounding_octagon(&self) -> Option<IntOctagon> {
        if let Some(o) = self.inner.bounding_octagon.get() {
            return Some(*o);
        }
        let result = self.compute_bounding_octagon()?;
        Some(*self.inner.bounding_octagon.get_or_init(|| result))
    }

    /// Approximation of the intersection of the lines `no - 1` and `no`.
    ///
    /// # Panics
    /// If the simplex is empty (Java returns `null`).
    fn corner_approx(&self, corner_index: usize) -> FloatPoint {
        assert!(
            !self.lines().is_empty(),
            "Simplex.cornerApprox: simplex is empty"
        );
        let no = self.clamp_index(corner_index);
        let lines = self.lines();
        *self.inner.float_corners[no].get_or_init(|| {
            let prev = if no == 0 {
                lines[lines.len() - 1]
            } else {
                lines[no - 1]
            };
            lines[no].intersection_approx(&prev)
        })
    }

    fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        (0..self.lines().len())
            .map(|i| self.corner_approx(i))
            .collect()
    }
}

impl TileShapeOps for Simplex {
    fn get_id(&self) -> i32 {
        self.lines().iter().fold(0i32, |acc, l| {
            31i32.wrapping_mul(acc).wrapping_add(l.get_id())
        })
    }

    fn is_int_box(&self) -> bool {
        (0..self.lines().len())
            .all(|i| self.lines()[i].is_orthogonal() && self.corner_is_bounded(i))
    }

    fn is_int_octagon(&self) -> bool {
        (0..self.lines().len())
            .all(|i| self.lines()[i].is_multiple_of_45_degree() && self.corner_is_bounded(i))
    }

    fn border_line_index(&self, line: &Line) -> Option<usize> {
        self.lines().iter().position(|l| line == l)
    }

    fn to_simplex(&self) -> Simplex {
        self.clone()
    }

    fn to_tile_shape(&self) -> TileShape {
        TileShape::Simplex(self.clone())
    }

    fn max_width(&self) -> f64 {
        if !self.is_bounded() {
            return f64::from(i32::MAX);
        }
        let mut max_distance = f64::from(i32::MIN);
        let mut max_distance_2 = f64::from(i32::MIN);
        let gravity_point = self.centre_of_gravity();
        for line in self.lines() {
            let current_distance = line.signed_distance(gravity_point).abs();
            if current_distance > max_distance {
                max_distance_2 = max_distance;
                max_distance = current_distance;
            } else if current_distance > max_distance_2 {
                max_distance_2 = current_distance;
            }
        }
        max_distance + max_distance_2
    }

    fn min_width(&self) -> f64 {
        if !self.is_bounded() {
            return f64::from(i32::MAX);
        }
        let mut min_distance = f64::from(i32::MAX);
        let mut min_distance_2 = f64::from(i32::MAX);
        let gravity_point = self.centre_of_gravity();
        for line in self.lines() {
            let current_distance = line.signed_distance(gravity_point).abs();
            if current_distance < min_distance {
                min_distance_2 = min_distance;
                min_distance = current_distance;
            } else if current_distance < min_distance_2 {
                min_distance_2 = current_distance;
            }
        }
        min_distance + min_distance_2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Simplex {
        IntBox::new(0, 0, 10, 10).to_simplex()
    }

    #[test]
    fn box_simplex_corners_and_bounds() {
        let s = square();
        assert_eq!(s.border_line_count(), 4);
        assert_eq!(s.corner(0), Point::Int(IntPoint::new(0, 0)));
        assert_eq!(s.corner(1), Point::Int(IntPoint::new(10, 0)));
        assert_eq!(s.corner(2), Point::Int(IntPoint::new(10, 10)));
        assert_eq!(s.corner(3), Point::Int(IntPoint::new(0, 10)));
        assert!(s.is_bounded());
        assert_eq!(s.dimension(), 2);
        assert_eq!(s.bounding_box(), IntBox::new(0, 0, 10, 10));
        assert!(s.is_int_box());
        assert_eq!(s.area(), 100.0);
        assert!(matches!(s.simplify(), TileShape::IntBox(_)));
    }

    #[test]
    fn get_instance_removes_redundant_lines() {
        let lines = [
            Line::from_coords(0, 0, 1, 0),
            Line::from_coords(10, 0, 10, 1),
            Line::from_coords(0, 10, -1, 10),
            Line::from_coords(0, 0, 0, -1),
            // redundant: lower line further down
            Line::from_coords(0, -5, 1, -5),
            // redundant diagonal cutting nothing
            Line::from_coords(100, 0, 99, 1),
        ];
        let s = Simplex::get_instance(&lines);
        assert_eq!(s.border_line_count(), 4);
        assert_eq!(s.bounding_box(), IntBox::new(0, 0, 10, 10));
    }

    #[test]
    fn empty_and_half_plane() {
        // Two half-planes y >= 1 and y <= 0 do not intersect.
        let lines = [
            Line::from_coords(0, 1, 1, 1),
            Line::from_coords(0, 0, -1, 0),
        ];
        assert!(Simplex::get_instance(&lines).is_empty());
        let half = Simplex::get_instance(&[Line::from_coords(0, 0, 1, 0)]);
        assert_eq!(half.dimension(), 2);
        assert!(!half.is_bounded());
        assert!(!half.corner_is_bounded(0));
    }

    #[test]
    fn triangle_and_intersection() {
        let tri = TileShape::get_instance_from_points(&[
            Point::Int(IntPoint::new(0, 0)),
            Point::Int(IntPoint::new(10, 0)),
            Point::Int(IntPoint::new(0, 10)),
        ]);
        assert!(matches!(
            tri,
            TileShape::IntOctagon(_) | TileShape::Simplex(_)
        ));
        assert_eq!(tri.area(), 50.0);
        let t = tri.to_simplex();
        let b = IntBox::new(5, 0, 20, 20);
        let is = t.intersection_box(&b);
        assert_eq!(is.dimension(), 2);
        assert_eq!(is.area(), 12.5);
        let tri2 = TileShape::get_instance_from_points(&[
            Point::Int(IntPoint::new(0, 0)),
            Point::Int(IntPoint::new(10, 0)),
            Point::Int(IntPoint::new(3, 7)),
        ]);
        let s = tri2.to_simplex();
        assert!(!s.is_int_octagon());
        assert_eq!(s.area(), 35.0);
        assert!(s.contains(&Point::Int(IntPoint::new(3, 3))));
        assert!(!s.contains(&Point::Int(IntPoint::new(9, 3))));
    }

    #[test]
    fn cutout_from_produces_pieces_covering_the_rest() {
        let outer = IntBox::new(0, 0, 100, 100).to_simplex();
        let inner = TileShape::get_instance_from_points(&[
            Point::Int(IntPoint::new(40, 40)),
            Point::Int(IntPoint::new(60, 45)),
            Point::Int(IntPoint::new(50, 60)),
        ])
        .to_simplex();
        let pieces = inner.cutout_from(&outer).unwrap();
        assert!(!pieces.is_empty());
        let total: f64 = pieces.iter().map(|p| p.area()).sum();
        assert!((total + inner.area() - outer.area()).abs() < 1.0);
        for p in &pieces {
            assert!(p.dimension() <= 2);
        }
    }

    #[test]
    fn offset_and_enlarge() {
        let s = square();
        let o = s.offset(2.0);
        assert_eq!(o.bounding_box(), IntBox::new(-2, -2, 12, 12));
        let shrunk = s.offset(-2.0);
        assert_eq!(shrunk.bounding_box(), IntBox::new(2, 2, 8, 8));
        let e = s.enlarge(2.0);
        assert!(
            e.bounding_box()
                .is_contained_in(&IntBox::new(-2, -2, 12, 12))
        );
    }
}

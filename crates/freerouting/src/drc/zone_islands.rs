//! Copper islands of filled conduction areas.
//!
//! Port of `DesignRulesChecker.getZoneIslandViolations`,
//! `drc.ZoneIslandViolation` and `ConductionArea.getDetailedFillArea`.
//!
//! Java builds the detailed fill with `java.awt.geom.Area` and splits it
//! into islands along the subpaths of the AWT path. The port describes the
//! fill as a boolean combination of polygons (circles are flattened AWT
//! Bézier ellipses) and finds the islands with a vertical slab
//! decomposition: in each slab between two consecutive vertex or
//! intersection abscissas the edges do not cross, the filled intervals of
//! a slab are trapezoids, and trapezoids of neighbouring slabs touching
//! along a segment of positive length belong to the same island.
//!
//! Not Java: [`eroded_fill_islands`] (K15.23), the fill eroded by a
//! distance, for the stub cleanup's model of KiCad's minimum-thickness
//! opening (`quality::pour_fill`). With no erosion its operands are Java's.
//! K15.35: [`opened_fill_islands`] can take a board's keepouts as KiCad's
//! zone filler does ([`KeepoutFill`]), for the cleanup's model of a DSN
//! input's pours; K15.38: and of a `.kicad_pcb`'s zones, where the board's
//! rule areas say which keepouts ban the pour; K15.40: a pad-clearance
//! circle as the pad it stands for ([`pad_clearance_fill`]) and the board
//! outline by KiCad's edge clearance ([`edge_fill_clearance`]).

use crate::util::HashMap;

use freerouting_geometry::jmath::round_to_i32;
use freerouting_geometry::{
    Area, FloatPoint, IntBox, IntPoint, PolylineShape, PolylineShapeOps, Shape, TileShape,
};

use super::clearance::parallel_map;
use crate::board::BasicBoard;
use crate::board::items::{AreaKind, Item, ItemId, ItemKind, NET_TIE_KEEPOUT_NAME, ObstacleArea};
use crate::board::structure::Unit;

/// The kind of a zone island finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ZoneIslandKind {
    /// An island with items that are not connected to the main island.
    IsolatedIslandUnconnected,
    /// An island without items, at least 1 mm² large (bounding box).
    IsolatedIslandDeadCopper,
}

impl ZoneIslandKind {
    /// The Java type string.
    pub fn type_name(self) -> &'static str {
        match self {
            ZoneIslandKind::IsolatedIslandUnconnected => "isolated_island_unconnected",
            ZoneIslandKind::IsolatedIslandDeadCopper => "isolated_island_dead_copper",
        }
    }
}

/// A trapezoid with vertical left and right sides, a piece of an island.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trapezoid {
    pub x_left: f64,
    pub x_right: f64,
    /// Lower and upper y at `x_left`.
    pub left: [f64; 2],
    /// Lower and upper y at `x_right`.
    pub right: [f64; 2],
}

impl Trapezoid {
    /// The corners counterclockwise from the lower left one.
    pub fn corners(&self) -> [FloatPoint; 4] {
        [
            FloatPoint::new(self.x_left, self.left[0]),
            FloatPoint::new(self.x_right, self.right[0]),
            FloatPoint::new(self.x_right, self.right[1]),
            FloatPoint::new(self.x_left, self.left[1]),
        ]
    }

    fn contains(&self, p: FloatPoint) -> bool {
        if p.x < self.x_left || p.x > self.x_right {
            return false;
        }
        let width = self.x_right - self.x_left;
        let t = if width > 0.0 {
            (p.x - self.x_left) / width
        } else {
            0.0
        };
        let low = self.left[0] + t * (self.right[0] - self.left[0]);
        let high = self.left[1] + t * (self.right[1] - self.left[1]);
        p.y >= low && p.y <= high
    }
}

/// A connected piece of the fill of a conduction area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Island {
    /// The island as trapezoids (board coordinates), for drawing.
    pub trapezoids: Vec<Trapezoid>,
    /// `[min x, min y, max x, max y]`.
    pub bounds: [f64; 4],
}

impl Island {
    /// True if `point` is inside the island or on its border.
    pub fn contains(&self, point: FloatPoint) -> bool {
        point.x >= self.bounds[0]
            && point.x <= self.bounds[2]
            && point.y >= self.bounds[1]
            && point.y <= self.bounds[3]
            && self.trapezoids.iter().any(|t| t.contains(point))
    }

    /// The centre of the bounding box.
    pub fn center(&self) -> FloatPoint {
        FloatPoint::new(
            (self.bounds[0] + self.bounds[2]) / 2.0,
            (self.bounds[1] + self.bounds[3]) / 2.0,
        )
    }

    /// The area of the bounding box (Java approximates the island area so).
    pub fn bounding_box_area(&self) -> f64 {
        (self.bounds[2] - self.bounds[0]) * (self.bounds[3] - self.bounds[1])
    }
}

/// An isolated or dead copper island of a conduction area.
///
/// Port of `drc.ZoneIslandViolation`. A viewer draws
/// [`island`](Self::island) on [`layer`](Self::layer).
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneIslandViolation {
    pub conduction_area: ItemId,
    pub island: Island,
    pub layer: i32,
    pub net_number: i32,
    pub kind: ZoneIslandKind,
    /// The items of the net whose centre (first corner for traces) lies on
    /// the island.
    pub items_in_island: Vec<ItemId>,
}

impl ZoneIslandViolation {
    /// Java `getCenter`: the centre of the island's bounding box.
    pub fn center(&self) -> FloatPoint {
        self.island.center()
    }

    /// Java `getAreaInBoardUnits`: the area of the island's bounding box.
    pub fn area_in_board_units(&self) -> f64 {
        self.island.bounding_box_area()
    }
}

// ---------------------------------------------------------------------------
// The fill as a boolean combination of polygons.

/// The role of an operand in the fill formula
/// `(border - holes - foreign - same) + sum_j (same_j * spoke_j)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Border,
    Hole,
    Foreign,
    /// The clearance of a same-net drill item, with its index.
    Same(usize),
    /// The spokes of the same-net drill item with the index.
    Spoke(usize),
}

struct Operand {
    role: Role,
    rings: Vec<Vec<(f64, f64)>>,
}

/// The fill state while walking up a slab.
struct FillState<'a> {
    roles: &'a [Role],
    winding: Vec<i32>,
    border: i32,
    holes: i32,
    foreign: i32,
    same: i32,
    same_inside: Vec<bool>,
    spoke_inside: Vec<bool>,
    connected_spokes: i32,
}

impl<'a> FillState<'a> {
    fn new(roles: &'a [Role], drill_count: usize) -> Self {
        FillState {
            roles,
            winding: vec![0; roles.len()],
            border: 0,
            holes: 0,
            foreign: 0,
            same: 0,
            same_inside: vec![false; drill_count],
            spoke_inside: vec![false; drill_count],
            connected_spokes: 0,
        }
    }

    fn cross(&mut self, operand: usize, dir: i32) {
        let before = self.winding[operand] != 0;
        self.winding[operand] += dir;
        let after = self.winding[operand] != 0;
        if before == after {
            return;
        }
        let delta = if after { 1 } else { -1 };
        match self.roles[operand] {
            Role::Border => self.border += delta,
            Role::Hole => self.holes += delta,
            Role::Foreign => self.foreign += delta,
            Role::Same(j) => {
                self.same += delta;
                self.update_spoke(j, true, after);
            }
            Role::Spoke(j) => self.update_spoke(j, false, after),
        }
    }

    fn update_spoke(&mut self, j: usize, is_same: bool, value: bool) {
        let before = self.same_inside[j] && self.spoke_inside[j];
        if is_same {
            self.same_inside[j] = value;
        } else {
            self.spoke_inside[j] = value;
        }
        let after = self.same_inside[j] && self.spoke_inside[j];
        if before != after {
            self.connected_spokes += if after { 1 } else { -1 };
        }
    }

    fn filled(&self) -> bool {
        (self.border > 0 && self.holes == 0 && self.foreign == 0 && self.same == 0)
            || self.connected_spokes > 0
    }
}

#[derive(Clone, Copy, Debug)]
struct Edge {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    operand: usize,
    /// +1 if the ring runs from left to right along this edge.
    dir: i32,
}

impl Edge {
    fn y_at(&self, x: f64) -> f64 {
        if x <= self.x1 {
            return self.y1;
        }
        if x >= self.x2 {
            return self.y2;
        }
        self.y1 + (self.y2 - self.y1) * (x - self.x1) / (self.x2 - self.x1)
    }
}

/// The x of the proper intersection of `a` and `b` inside both x ranges.
fn intersection_x(a: &Edge, b: &Edge) -> Option<f64> {
    let (dax, day) = (a.x2 - a.x1, a.y2 - a.y1);
    let (dbx, dby) = (b.x2 - b.x1, b.y2 - b.y1);
    let det = dax * dby - day * dbx;
    if det == 0.0 {
        return None;
    }
    let (ox, oy) = (b.x1 - a.x1, b.y1 - a.y1);
    let t = (ox * dby - oy * dbx) / det;
    let u = (ox * day - oy * dax) / det;
    if t <= 0.0 || t >= 1.0 || u <= 0.0 || u >= 1.0 {
        return None;
    }
    Some(a.x1 + t * dax)
}

/// Union-find.
struct Components {
    parent: Vec<usize>,
}

impl Components {
    fn add(&mut self) -> usize {
        self.parent.push(self.parent.len());
        self.parent.len() - 1
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra.max(rb)] = ra.min(rb);
        }
    }
}

/// The target number of edges of a grid cell of [`islands_of`].
const CELL_EDGES: usize = 256;

/// A trapezoid side on a cell border: the covered interval and the
/// trapezoid index.
type Side = (f64, f64, usize);

/// The trapezoids of one grid cell touching its left, right, bottom and top
/// border.
#[derive(Default)]
struct CellSides {
    left: Vec<Side>,
    right: Vec<Side>,
    bottom: Vec<Side>,
    top: Vec<Side>,
}

/// Unites the trapezoids of two lists of disjoint intervals sorted by their
/// lower end that overlap along a segment of positive length.
fn connect_sides(a: &[Side], b: &[Side], components: &mut Components) {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let (a0, a1, ai) = a[i];
        let (b0, b1, bi) = b[j];
        if a1.min(b1) - a0.max(b0) > 1e-9 {
            components.union(ai, bi);
        }
        if a1 < b1 {
            i += 1;
        } else {
            j += 1;
        }
    }
}

/// Adds the edges and vertex abscissas of `ring` of operand `operand`.
fn add_ring_edges(ring: &[(f64, f64)], operand: usize, edges: &mut Vec<Edge>, xs: &mut Vec<f64>) {
    let n = ring.len();
    for i in 0..n {
        let (ax, ay) = ring[i];
        let (bx, by) = ring[(i + 1) % n];
        xs.push(ax);
        if ax == bx {
            continue;
        }
        edges.push(if ax < bx {
            Edge {
                x1: ax,
                y1: ay,
                x2: bx,
                y2: by,
                operand,
                dir: 1,
            }
        } else {
            Edge {
                x1: bx,
                y1: by,
                x2: ax,
                y2: ay,
                operand,
                dir: -1,
            }
        });
    }
}

/// Clips `ring` to the rectangle `[min x, min y, max x, max y]`
/// (Sutherland–Hodgman). Inside the rectangle the winding numbers of the
/// result are those of `ring`.
fn clip_ring(ring: &[(f64, f64)], rect: [f64; 4]) -> Vec<(f64, f64)> {
    // The intersection with a vertical (`vertical`) or horizontal line at
    // `value`, computed from the lexicographically smaller end so that
    // neighbouring cells get the same point.
    fn cut(a: (f64, f64), b: (f64, f64), value: f64, vertical: bool) -> (f64, f64) {
        let (p, q) = if (a.0, a.1) <= (b.0, b.1) {
            (a, b)
        } else {
            (b, a)
        };
        if vertical {
            (value, p.1 + (q.1 - p.1) * (value - p.0) / (q.0 - p.0))
        } else {
            (p.0 + (q.0 - p.0) * (value - p.1) / (q.1 - p.1), value)
        }
    }
    let mut points = ring.to_vec();
    // (vertical line, value, keep the side with greater coordinates)
    let planes = [
        (true, rect[0], true),
        (true, rect[2], false),
        (false, rect[1], true),
        (false, rect[3], false),
    ];
    for (vertical, value, keep_greater) in planes {
        if points.is_empty() {
            break;
        }
        let inside = |p: (f64, f64)| {
            let c = if vertical { p.0 } else { p.1 };
            if keep_greater { c >= value } else { c <= value }
        };
        let mut result = Vec::with_capacity(points.len() + 4);
        let n = points.len();
        for i in 0..n {
            let current = points[i];
            let previous = points[(i + n - 1) % n];
            match (inside(previous), inside(current)) {
                (true, true) => result.push(current),
                (true, false) => result.push(cut(previous, current, value, vertical)),
                (false, true) => {
                    result.push(cut(previous, current, value, vertical));
                    result.push(current);
                }
                (false, false) => {}
            }
        }
        points = result;
    }
    points
}

/// The slab decomposition of the edges of one cell: appends the filled
/// trapezoids to `trapezoids`, unites those of neighbouring slabs and
/// returns the ones on the borders of `rect`.
fn sweep_cell(
    mut edges: Vec<Edge>,
    mut xs: Vec<f64>,
    rect: [f64; 4],
    state: &mut FillState,
    components: &mut Components,
    trapezoids: &mut Vec<Trapezoid>,
) -> CellSides {
    let mut sides = CellSides::default();
    if edges.is_empty() {
        return sides;
    }
    edges.sort_by(|a, b| a.x1.total_cmp(&b.x1));

    // The intersections of the edges.
    {
        let mut active: Vec<usize> = Vec::new();
        for i in 0..edges.len() {
            let e = edges[i];
            active.retain(|&k| edges[k].x2 > e.x1);
            let (elo, ehi) = (e.y1.min(e.y2), e.y1.max(e.y2));
            for &k in &active {
                let f = &edges[k];
                if f.y1.max(f.y2) < elo || f.y1.min(f.y2) > ehi {
                    continue;
                }
                if let Some(x) = intersection_x(&e, f) {
                    xs.push(x);
                }
            }
            active.push(i);
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup();

    // The trapezoids of the previous slab: (lower, upper y at its right
    // side, trapezoid index).
    let mut previous: Vec<Side> = Vec::new();
    let mut current: Vec<Side> = Vec::new();
    let mut next_edge = 0;
    // The active edges, in the order of the previous slab: nearly sorted
    // for the next one.
    let mut order: Vec<(f64, usize)> = Vec::new();
    for w in xs.windows(2) {
        let (xa, xb) = (w[0], w[1]);
        order.retain(|&(_, k)| edges[k].x2 > xa);
        while next_edge < edges.len() && edges[next_edge].x1 <= xa {
            if edges[next_edge].x2 > xa {
                order.push((0.0, next_edge));
            }
            next_edge += 1;
        }
        if xb - xa <= 0.0 {
            continue;
        }
        let xm = 0.5 * (xa + xb);
        for entry in order.iter_mut() {
            entry.0 = edges[entry.1].y_at(xm);
        }
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Crossing all edges of a slab brings every winding back to zero,
        // so the state is clear at the start of each slab.
        current.clear();
        let mut open: Option<usize> = None;
        for &(_, k) in &order {
            let edge = &edges[k];
            let was_filled = state.filled();
            state.cross(edge.operand, edge.dir);
            let is_filled = state.filled();
            if !was_filled && is_filled {
                open = Some(k);
            } else if was_filled
                && !is_filled
                && let Some(lower) = open.take()
            {
                let low = &edges[lower];
                let trapezoid = Trapezoid {
                    x_left: xa,
                    x_right: xb,
                    left: [low.y_at(xa), edge.y_at(xa)],
                    right: [low.y_at(xb), edge.y_at(xb)],
                };
                // Coincident edges give empty trapezoids.
                if trapezoid.left[1] <= trapezoid.left[0]
                    && trapezoid.right[1] <= trapezoid.right[0]
                {
                    continue;
                }
                let index = components.add();
                trapezoids.push(trapezoid);
                current.push((trapezoid.left[0], trapezoid.left[1], index));
                if xa == rect[0] {
                    sides
                        .left
                        .push((trapezoid.left[0], trapezoid.left[1], index));
                }
                if xb == rect[2] {
                    sides
                        .right
                        .push((trapezoid.right[0], trapezoid.right[1], index));
                }
                if trapezoid.left[0] == rect[1] && trapezoid.right[0] == rect[1] {
                    sides.bottom.push((xa, xb, index));
                }
                if trapezoid.left[1] == rect[3] && trapezoid.right[1] == rect[3] {
                    sides.top.push((xa, xb, index));
                }
            }
        }
        // Connect with the trapezoids of the previous slab.
        connect_sides(&previous, &current, components);
        // Keep the right sides for the next slab.
        for side in current.iter_mut() {
            let t = &trapezoids[side.2];
            *side = (t.right[0], t.right[1], side.2);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    sides
}

/// A polygon ring as `(x, y)` points.
type Ring = Vec<(f64, f64)>;

/// A ring with its operand index and bounding box `[min_x, min_y, max_x, max_y]`.
type RingEntry<'a> = (usize, &'a [(f64, f64)], [f64; 4]);

/// The connected components of the fill described by `operands`.
///
/// Large fills are cut into a grid of cells with about [`CELL_EDGES`]
/// edges each; the cells are decomposed separately and their trapezoids
/// united along the cell borders.
fn islands_of(operands: &[Operand], drill_count: usize) -> Vec<Island> {
    let roles: Vec<Role> = operands.iter().map(|o| o.role).collect();
    // (operand, ring, bounding box)
    let mut rings: Vec<RingEntry<'_>> = Vec::new();
    let mut bounds = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut edge_count = 0;
    for (index, operand) in operands.iter().enumerate() {
        for ring in &operand.rings {
            if ring.is_empty() {
                continue;
            }
            let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for &(x, y) in ring {
                b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            }
            bounds = [
                bounds[0].min(b[0]),
                bounds[1].min(b[1]),
                bounds[2].max(b[2]),
                bounds[3].max(b[3]),
            ];
            edge_count += ring.len();
            rings.push((index, ring, b));
        }
    }
    if rings.is_empty() {
        return Vec::new();
    }
    let n = if edge_count <= CELL_EDGES {
        1
    } else {
        (edge_count as f64 / CELL_EDGES as f64).sqrt().ceil() as usize
    };
    let grid = |lo: f64, hi: f64| -> Vec<f64> {
        (0..=n)
            .map(|i| {
                if i == n {
                    hi
                } else {
                    lo + (hi - lo) * i as f64 / n as f64
                }
            })
            .collect()
    };
    let gx = grid(bounds[0], bounds[2]);
    let gy = grid(bounds[1], bounds[3]);
    // The range of cells a coordinate interval touches.
    let cells_of = |g: &[f64], lo: f64, hi: f64| -> (usize, usize) {
        let first = g[1..n].partition_point(|&v| v < lo);
        let last = g[1..n].partition_point(|&v| v <= hi);
        (first, last)
    };
    let mut cell_rings: Vec<Vec<usize>> = vec![Vec::new(); n * n];
    for (r, (_, _, b)) in rings.iter().enumerate() {
        let (x0, x1) = cells_of(&gx, b[0], b[2]);
        let (y0, y1) = cells_of(&gy, b[1], b[3]);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                cell_rings[cy * n + cx].push(r);
            }
        }
    }

    let mut state = FillState::new(&roles, drill_count);
    let mut components = Components { parent: Vec::new() };
    let mut trapezoids: Vec<Trapezoid> = Vec::new();
    let mut sides: Vec<CellSides> = Vec::with_capacity(n * n);
    for cy in 0..n {
        for cx in 0..n {
            let rect = [gx[cx], gy[cy], gx[cx + 1], gy[cy + 1]];
            let mut edges = Vec::new();
            let mut xs = vec![rect[0], rect[2]];
            for &r in &cell_rings[cy * n + cx] {
                let (operand, ring, b) = rings[r];
                if b[0] >= rect[0] && b[1] >= rect[1] && b[2] <= rect[2] && b[3] <= rect[3] {
                    add_ring_edges(ring, operand, &mut edges, &mut xs);
                } else {
                    let clipped = clip_ring(ring, rect);
                    if clipped.len() >= 3 {
                        add_ring_edges(&clipped, operand, &mut edges, &mut xs);
                    }
                }
            }
            sides.push(sweep_cell(
                edges,
                xs,
                rect,
                &mut state,
                &mut components,
                &mut trapezoids,
            ));
        }
    }
    // Unite along the cell borders.
    for cy in 0..n {
        for cx in 0..n {
            let mut cell = std::mem::take(&mut sides[cy * n + cx].right);
            cell.sort_by(|a, b| a.0.total_cmp(&b.0));
            if cx + 1 < n {
                let other = &mut sides[cy * n + cx + 1].left;
                other.sort_by(|a, b| a.0.total_cmp(&b.0));
                connect_sides(&cell, other, &mut components);
            }
            let mut cell = std::mem::take(&mut sides[cy * n + cx].top);
            cell.sort_by(|a, b| a.0.total_cmp(&b.0));
            if cy + 1 < n {
                let other = &mut sides[(cy + 1) * n + cx].bottom;
                other.sort_by(|a, b| a.0.total_cmp(&b.0));
                connect_sides(&cell, other, &mut components);
            }
        }
    }

    let mut by_root: HashMap<usize, usize> = HashMap::default();
    let mut islands: Vec<Island> = Vec::new();
    for (index, trapezoid) in trapezoids.into_iter().enumerate() {
        let root = components.find(index);
        let island_index = *by_root.entry(root).or_insert_with(|| {
            islands.push(Island {
                trapezoids: Vec::new(),
                bounds: [f64::MAX, f64::MAX, f64::MIN, f64::MIN],
            });
            islands.len() - 1
        });
        let island = &mut islands[island_index];
        island.bounds[0] = island.bounds[0].min(trapezoid.x_left);
        island.bounds[2] = island.bounds[2].max(trapezoid.x_right);
        island.bounds[1] = island.bounds[1].min(trapezoid.left[0].min(trapezoid.right[0]));
        island.bounds[3] = island.bounds[3].max(trapezoid.left[1].max(trapezoid.right[1]));
        island.trapezoids.push(trapezoid);
    }
    // Java lists the islands in the order of the AWT path, which starts
    // with the topmost curves; here by the lower bound in y, then in x.
    islands.sort_by(|a, b| {
        a.bounds[1]
            .total_cmp(&b.bounds[1])
            .then(a.bounds[0].total_cmp(&b.bounds[0]))
    });
    islands
}

// ---------------------------------------------------------------------------
// Board shapes as rings.

/// The ring of a polyline shape (Java `cornerApprox` path).
fn polyline_ring(shape: &PolylineShape) -> Vec<(f64, f64)> {
    (0..shape.border_line_count())
        .map(|i| {
            let p = shape.corner_approx(i);
            (p.x, p.y)
        })
        .collect()
}

/// The ring of an AWT `Ellipse2D` circle: four cubic Bézier arcs,
/// flattened.
fn circle_ring(center: IntPoint, radius: f64) -> Vec<(f64, f64)> {
    const CTRL: f64 = 0.552_284_749_830_793_3;
    const STEPS: usize = 16;
    let (cx, cy) = (f64::from(center.x), f64::from(center.y));
    // Ellipse2D starts at the right and runs through the bottom (larger y).
    let quadrants = [
        [(1.0, 0.0), (1.0, CTRL), (CTRL, 1.0), (0.0, 1.0)],
        [(0.0, 1.0), (-CTRL, 1.0), (-1.0, CTRL), (-1.0, 0.0)],
        [(-1.0, 0.0), (-1.0, -CTRL), (-CTRL, -1.0), (0.0, -1.0)],
        [(0.0, -1.0), (CTRL, -1.0), (1.0, -CTRL), (1.0, 0.0)],
    ];
    let mut ring = Vec::with_capacity(4 * STEPS);
    for q in quadrants {
        for step in 0..STEPS {
            let t = step as f64 / STEPS as f64;
            let s = 1.0 - t;
            let b = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
            let x: f64 = (0..4).map(|i| b[i] * q[i].0).sum();
            let y: f64 = (0..4).map(|i| b[i] * q[i].1).sum();
            ring.push((cx + radius * x, cy + radius * y));
        }
    }
    ring
}

/// Java `getAwtAreaFromShapeInBoardUnits`.
fn shape_ring(shape: &Shape) -> Option<Vec<(f64, f64)>> {
    match shape {
        Shape::Circle(c) => Some(circle_ring(c.center, f64::from(c.radius))),
        Shape::Polyline(p) => Some(polyline_ring(p)),
    }
}

fn tile_ring(shape: &TileShape) -> Vec<(f64, f64)> {
    polyline_ring(&PolylineShape::Tile(shape.clone()))
}

/// The rings of the border and the holes of a conduction area (Java
/// `getAwtAreaInBoardUnits`), `None` if the area is empty or unbounded.
fn area_rings(area: &Area) -> Option<(Ring, Vec<Ring>)> {
    if area.is_empty() {
        return None;
    }
    match area {
        Area::Shape(Shape::Circle(c)) => {
            Some((circle_ring(c.center, f64::from(c.radius)), Vec::new()))
        }
        Area::Shape(Shape::Polyline(p)) => {
            if !p.is_bounded() {
                return None;
            }
            Some((polyline_ring(p), Vec::new()))
        }
        Area::PolylineArea(a) => {
            let border = a.get_border();
            if !border.is_bounded() {
                return None;
            }
            let holes = a.get_holes().iter().map(polyline_ring).collect();
            Some((polyline_ring(border), holes))
        }
    }
}

/// The operands of the detailed fill of conduction area `ca` (Java
/// `ConductionArea.ensureDetailedFillCache`) and the number of same-net
/// drill items.
fn fill_operands(board: &BasicBoard, ca: &Item) -> Option<(Vec<Operand>, usize)> {
    fill_operands_eroded(board, ca, 0.0, 0.0, false)
}

/// K15.35: how KiCad's zone filler treats a keepout of the board, as the
/// cleanup's model of a pour takes it ([`opened_fill_islands`] with
/// `kicad_keepouts`). Java's fill keeps every keepout's clearance class
/// from it; KiCad's filler knocks a rule area out at its outline with no
/// clearance (`ZONE_FILLER::buildCopperItemClearances`: "Keepouts use
/// outline with no clearance"), and KiCad's Specctra export (and the
/// `.kicad_pcb` reader) writes an NPTH without copper as a keepout already
/// grown by the board's hole clearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeepoutFill {
    /// Not cut: on a DSN input a solder-mask aperture or a thermal spoke's
    /// way out fitted to its pins (`ObstacleArea::is_pad_keepout`, no
    /// copper), which KiCad's filler does not see, and a `via_keepout`, a
    /// `wire_keepout` or a `place_keepout`, a rule area that lets tracks,
    /// vias or both through, which the DSN does not say bans the pour (KiCad
    /// cuts a rule area only where it bans the pour); K15.38: on a
    /// `.kicad_pcb` the keepouts of a rule area that lets the pour in,
    /// whatever their kind.
    Ignored,
    /// On a DSN input a `keepout` (a rule area that bans tracks and vias, as
    /// KiCad's default rule area bans the pour too) or an NPTH; K15.38: on a
    /// `.kicad_pcb` the keepouts of a rule area that bans the pour, whatever
    /// their kind, and an NPTH: cut at its outline.
    Outline,
    /// Kept the clearance from, like other copper: copper the export leaves
    /// out (a copper graphic or text, a net tie's shape). K15.38: on a
    /// `.kicad_pcb` every keepout the reader names and an unnamed one of no
    /// rule area ([`kicad_pcb_keepout_fill`]).
    Clearance,
    /// K15.40: a pad-clearance circle ([`pad_clearance_fill`]) of a pad or
    /// an NPTH of no net or another net than the pour's: kept exactly the
    /// default clearance it was drawn less (the keepout's class against
    /// itself, without Java's safety margin), never the floor, so that it
    /// reaches the pad's own clearance as KiCad's filler keeps it.
    Encoded,
}

/// The names of the keepouts that stand for copper
/// ([`KeepoutFill::Clearance`]): the KiCad plugin's and the `.kicad_pcb`
/// reader's copper graphics and texts (Q22.4) and net-tie shapes (Q22.19).
const COPPER_KEEPOUT_NAMES: [&str; 3] = [
    freerouting_kicad::copper_graphics::KEEPOUT_NAME,
    freerouting_kicad::copper_graphics::TEXT_KEEPOUT_NAME,
    NET_TIE_KEEPOUT_NAME,
];

/// K15.35: [`KeepoutFill`] of `item` on `layer` for the fill of pour `ca`,
/// or `None` if it is no keepout. K15.38: a board read from a `.kicad_pcb`
/// takes [`kicad_pcb_keepout_fill`]. K15.40: a pad-clearance circle, on
/// either input, takes [`pad_clearance_fill`].
fn keepout_fill(board: &BasicBoard, ca: &Item, item: &Item, layer: i32) -> Option<KeepoutFill> {
    let area = item.as_area().filter(|a| !a.is_conduction_area())?;
    let name = area.name.as_deref().unwrap_or("");
    if COPPER_KEEPOUT_NAMES.contains(&name) {
        return Some(KeepoutFill::Clearance);
    }
    if let Some(fill) = pad_clearance_fill(board, ca, item, area, layer) {
        return Some(fill);
    }
    if board.kicad_report_rules().kicad_pcb {
        return Some(kicad_pcb_keepout_fill(board, item, area, layer));
    }
    let fill = if area.kind == AreaKind::Obstacle && !area.is_pad_keepout() {
        KeepoutFill::Outline
    } else {
        KeepoutFill::Ignored
    };
    Some(fill)
}

/// K15.38: [`KeepoutFill`] of keepout `item` (`area`) on `layer` of a board
/// read from a `.kicad_pcb`. The reader writes a rule area as unnamed
/// keepouts of its outline, one per copper layer (`keepout`, `via_keepout`,
/// `wire_keepout`, `place_keepout` by its tracks, vias and footprints
/// flags), and a copper-less round hole as an unnamed keepout circle of a
/// footprint (the hole grown by the hole clearance, the circle Java's
/// `assignHoleKeepoutClearanceClass` takes for a hole); the board builder
/// names a footprint's unnamed keepouts ([`is_generated_name`]). Which rule
/// area a keepout is, and whether that area bans the pour, the board's
/// [`BoardRuleArea`](crate::drc::BoardRuleArea)s say: the area on `layer`
/// whose box is the keepout's, within [`RULE_AREA_SLACK_UM`]. Every keepout
/// the reader names keeps the clearance, as in Java's fill: the copper ones;
/// `edge_clearance` and `pad_clearance`, drawn so that the router's
/// clearance from them is KiCad's (K15.40: a pad-clearance circle is taken
/// by [`pad_clearance_fill`] first); `solder_mask_shared`, under which the
/// pads are cut back (`freerouting_kicad::pad_trim`); and the fitted
/// `solder_mask_pads`, `solder_mask` and `thermal_spoke` (which KiCad's
/// filler does not see; leaving them out moved canary-pwr's routes, so
/// K15.38 keeps Java's handling of them). So does an unnamed keepout of no
/// rule area (a cutout of the board outline).
fn kicad_pcb_keepout_fill(
    board: &BasicBoard,
    item: &Item,
    area: &ObstacleArea,
    layer: i32,
) -> KeepoutFill {
    if area
        .name
        .as_deref()
        .is_some_and(|name| !is_generated_name(name))
    {
        return KeepoutFill::Clearance;
    }
    let hole = item.component_id() > 0
        && area.kind == AreaKind::Obstacle
        && matches!(area.area(), Area::Shape(Shape::Circle(_)));
    if hole {
        return KeepoutFill::Outline;
    }
    let slack = board
        .communication
        .coordinate_transform
        .dsn_to_board(RULE_AREA_SLACK_UM);
    let b = item.bounding_box();
    let own = [b.ll.x, b.ll.y, b.ur.x, b.ur.y].map(f64::from);
    let mut matched = board
        .kicad_report_rules()
        .rule_areas
        .iter()
        .filter(|r| r.layer == layer && own.iter().zip(r.bbox).all(|(a, b)| (a - b).abs() <= slack))
        .peekable();
    if matched.peek().is_none() {
        return KeepoutFill::Clearance;
    }
    // Two areas of one box: the one that bans the pour, so the model is
    // never wider than KiCad's fill.
    if matched.any(|r| r.bans_copper_pour) {
        KeepoutFill::Outline
    } else {
        KeepoutFill::Ignored
    }
}

/// K15.40: how KiCad's filler takes the pad a pad-clearance circle stands
/// for, if `item` (`area`) is one on `layer`. The KiCad plugin's DSN fix-up
/// `edge_and_pad_keepouts` and the `.kicad_pcb` reader
/// (`freerouting_kicad::pad_clearance`) draw, round a pad whose own
/// clearance exceeds the default, a structure-level circle keepout of
/// diameter `max(size) + 2 * (local - default)` on its centre (the board
/// keeps no name for a structure keepout), so that the router's default
/// clearance from it is the pad's. KiCad's filler (KiCad 10.0.7
/// `ZONE_FILLER`) joins a pad of the zone's net to the zone (thermal or
/// solid, `buildThermalSpokes`), and knocks a pad or an NPTH of no net or
/// another net out by its own clearance (`knockoutPadClearance`; measured
/// on CM5_MINIMA_3's netless NPTHs with `(clearance 1.7)`: the fill keeps
/// hole radius + 1.7 mm + 0.5 µm). So the circle is left out round a pin of
/// `ca`'s net ([`KeepoutFill::Ignored`]; the model fills over the pin) and
/// kept the default clearance otherwise ([`KeepoutFill::Encoded`]). A circle
/// with no pin and no NPTH keepout circle on its centre is not one (`None`).
fn pad_clearance_fill(
    board: &BasicBoard,
    ca: &Item,
    item: &Item,
    area: &ObstacleArea,
    layer: i32,
) -> Option<KeepoutFill> {
    let named = area.name.as_deref().unwrap_or("");
    if item.component_id() != 0
        || area.kind != AreaKind::Obstacle
        || !(named.is_empty() || named == freerouting_kicad::pad_clearance::KEEPOUT_NAME)
    {
        return None;
    }
    let Area::Shape(Shape::Circle(circle)) = area.area() else {
        return None;
    };
    let centre = circle.center;
    let slack = round_to_i32(PAD_CENTRE_SLACK_UM * board.communication.get_resolution(Unit::Um));
    let near = |p: IntPoint| (p.x - centre.x).abs() <= slack && (p.y - centre.y).abs() <= slack;
    let probe = IntBox::new(
        centre.x - slack,
        centre.y - slack,
        centre.x + slack,
        centre.y + slack,
    );
    let mut found = None;
    for id in board.overlapping_objects(&TileShape::IntBox(probe), layer) {
        let Some(other) = board.item(id).filter(|o| o.id() != item.id()) else {
            continue;
        };
        if let Some(pin) = other.as_pin() {
            if !near(pin.drill.center) {
                continue;
            }
            if other.shares_net(ca) {
                return Some(KeepoutFill::Ignored);
            }
            found = Some(KeepoutFill::Encoded);
        } else if other.component_id() > 0
            && other.as_area().is_some_and(|a| {
                a.kind == AreaKind::Obstacle
                    && matches!(a.area(), Area::Shape(Shape::Circle(c)) if near(c.center))
            })
        {
            found = Some(KeepoutFill::Encoded);
        }
    }
    found
}

/// K15.40: how far, in µm, a pad-clearance circle's centre may lie from its
/// pad's ([`pad_clearance_fill`]): the plugin writes whole µm, the reader
/// nanometres.
const PAD_CENTRE_SLACK_UM: f64 = 1.0;

/// K15.38: true for a name the board builder gives a footprint's unnamed
/// keepout (Java `Library.generateMissingKeepoutNames`: `keepout_<n>`,
/// `via_keepout_<n>`, `place_keepout_<n>`).
fn is_generated_name(name: &str) -> bool {
    ["keepout_", "via_keepout_", "place_keepout_"]
        .iter()
        .filter_map(|prefix| name.strip_prefix(prefix))
        .any(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// K15.38: how far, in µm, a keepout's box may lie from its rule area's
/// ([`kicad_pcb_keepout_fill`]): the reader writes a footprint's area
/// relative to the footprint at nanometres, and the board places it again.
const RULE_AREA_SLACK_UM: f64 = 1.0;

/// [`fill_operands`] eroded by `erode` board units (K15.23): every foreign
/// obstacle grown by its clearance plus `erode`, a strip of `erode` cut along
/// the border and every hole ([`edge_strips`]), and the same-net drills
/// filled over (no relief, no spokes). With `erode` 0 these are
/// [`fill_operands`]' operands exactly. K15.26, K15.33: a positive
/// `clearance_floor` is every foreign obstacle's clearance, in board units
/// (the least clearance KiCad's filler applies, which a DSN does not carry:
/// a `.kicad_dru` rule replaces the net classes' and the zone's, so the
/// router's net class clearance can be above it); 0 keeps the router's.
/// K15.35: with `kicad_keepouts` the keepouts are taken as KiCad's filler
/// takes them ([`KeepoutFill`]); without, as Java's fill takes them.
fn fill_operands_eroded(
    board: &BasicBoard,
    ca: &Item,
    erode: f64,
    clearance_floor: f64,
    kicad_keepouts: bool,
) -> Option<(Vec<Operand>, usize)> {
    let eroded = erode > 0.0;
    let grow = |clearance: f64| if eroded { clearance + erode } else { clearance };
    let area = ca.as_conduction_area()?;
    let (border, holes) = area_rings(area.area())?;
    let layer = area.layer();
    let resolution_um = board.communication.get_resolution(Unit::Um);
    let mut max_clearance = 2000.0 * resolution_um;
    let matrix_max = f64::from(
        board
            .rules
            .clearance_matrix
            .max_value(ca.clearance_class(), layer),
    );
    max_clearance = max_clearance.max(matrix_max.max(clearance_floor) + 100.0 * resolution_um);
    if eroded {
        max_clearance += erode;
    }
    let spoke_width = 400.0 * resolution_um;
    let max_cl = round_to_i32(max_clearance);
    let bbox = ca.bounding_box();
    let inflated = IntBox::new(
        bbox.ll.x - max_cl,
        bbox.ll.y - max_cl,
        bbox.ur.x + max_cl,
        bbox.ur.y + max_cl,
    );

    let mut operands = Vec::new();
    if eroded {
        for strip in std::iter::once(&border)
            .chain(&holes)
            .flat_map(|ring| edge_strips(ring, erode))
        {
            operands.push(Operand {
                role: Role::Foreign,
                rings: vec![strip],
            });
        }
    }
    operands.insert(
        0,
        Operand {
            role: Role::Border,
            rings: vec![border],
        },
    );
    for hole in holes {
        operands.push(Operand {
            role: Role::Hole,
            rings: vec![hole],
        });
    }
    let mut drill_count = 0;
    for id in board.overlapping_objects(&TileShape::IntBox(inflated), layer) {
        let Some(other) = board.item(id) else {
            continue;
        };
        if id == ca.id() || !other.shares_layer(ca) {
            continue;
        }
        let same_net = other.shares_net(ca);
        if (other.is_trace() || other.is_conduction_area()) && same_net {
            continue;
        }
        let keepout = if kicad_keepouts {
            keepout_fill(board, ca, other, layer)
        } else {
            None
        };
        if keepout == Some(KeepoutFill::Ignored) {
            continue;
        }
        let clearance =
            f64::from(board.clearance_value(ca.clearance_class(), other.clearance_class(), layer));
        let clearance = if keepout == Some(KeepoutFill::Outline) {
            0.0
        } else if keepout == Some(KeepoutFill::Encoded) {
            let class = other.clearance_class();
            f64::from(
                board
                    .rules
                    .clearance_matrix
                    .get_value(class, class, layer, false),
            )
        } else if kicad_keepouts && let Some(outline) = other.as_board_outline() {
            edge_fill_clearance(board, ca, other, outline, layer)
        } else if same_net || clearance_floor <= 0.0 {
            clearance
        } else {
            clearance_floor
        };
        if let Some(drill) = other.drill() {
            let Some(shape) = drill.shape_on_layer(layer) else {
                continue;
            };
            if same_net && eroded {
                continue;
            }
            let Some(ring) = shape.enlarge(grow(clearance)).as_ref().and_then(shape_ring) else {
                continue;
            };
            if same_net {
                let center = drill.center;
                let item_box = other.bounding_box();
                let max_dim = f64::from(item_box.width().max(item_box.height()));
                let expansion = max_dim / 2.0 + clearance;
                let half = spoke_width / 2.0;
                let (cx, cy) = (f64::from(center.x), f64::from(center.y));
                let base = [
                    (cx - half, cy - expansion),
                    (cx + half, cy - expansion),
                    (cx + half, cy + expansion),
                    (cx - half, cy + expansion),
                ];
                let rotated = |angle: f64| -> Vec<(f64, f64)> {
                    let (sin, cos) = angle.sin_cos();
                    base.iter()
                        .map(|(x, y)| {
                            let (dx, dy) = (x - cx, y - cy);
                            (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos)
                        })
                        .collect()
                };
                operands.push(Operand {
                    role: Role::Same(drill_count),
                    rings: vec![ring],
                });
                operands.push(Operand {
                    role: Role::Spoke(drill_count),
                    rings: vec![
                        rotated(std::f64::consts::FRAC_PI_4),
                        rotated(-std::f64::consts::FRAC_PI_4),
                    ],
                });
                drill_count += 1;
            } else {
                operands.push(Operand {
                    role: Role::Foreign,
                    rings: vec![ring],
                });
            }
        } else if !same_net {
            let mut rings = Vec::new();
            for i in 0..board.tile_shape_count(other) {
                if other.shape_layer(i) != layer {
                    continue;
                }
                if let Some(shape) = board.tile_shape(other, i) {
                    rings.push(tile_ring(&shape.enlarge(grow(clearance))));
                }
            }
            // Separate operands: each ring is its own AWT area.
            for ring in rings {
                operands.push(Operand {
                    role: Role::Foreign,
                    rings: vec![ring],
                });
            }
        }
    }
    Some((operands, drill_count))
}

/// K15.40: the clearance, in board units, the model keeps from the board
/// outline `item` (`outline`) on `layer`, as KiCad's filler keeps a zone
/// from the board edge: the edge clearance from the Edge.Cuts line itself
/// (`knockoutGraphicClearance`, `EDGE_CLEARANCE_CONSTRAINT` with line widths
/// ignored, plus 0.5 µm), never the zone clearance (the floor). The router's
/// outline class carries `router.copper_to_edge_clearance_um`; the model
/// takes it without Java's safety margin, less the half width of the
/// outline's lines ([`BoardOutline::HALF_WIDTH`], when they are its shapes),
/// and less [`KICAD_ARC_ERROR_UM`], how much nearer KiCad's fill comes at the
/// outline's arcs (2.2 µm on CM5_MINIMA_3). A `.kicad_dru` `edge_clearance`
/// rule replaces Board Setup's value in KiCad, below it too, while the
/// router's setting only rises to a rule; so the model takes the least rule
/// (`KicadReportRules::edge_clearance_least`) when it is smaller, on every
/// layer (a rule of some layers is the bound there, Board Setup's elsewhere:
/// the model may then be wider than KiCad's fill, never narrower).
///
/// [`BoardOutline::HALF_WIDTH`]: crate::board::items::BoardOutline::HALF_WIDTH
fn edge_fill_clearance(
    board: &BasicBoard,
    ca: &Item,
    item: &Item,
    outline: &crate::board::items::BoardOutline,
    layer: i32,
) -> f64 {
    let mut edge = f64::from(board.rules.clearance_matrix.get_value(
        ca.clearance_class(),
        item.clearance_class(),
        layer,
        false,
    ));
    let least = board.kicad_report_rules().edge_clearance_least;
    if least > 0.0 {
        edge = edge.min(least);
    }
    let half_width = if outline.keepout_outside_outline_generated() {
        0
    } else {
        crate::board::items::BoardOutline::HALF_WIDTH
    };
    let slack = KICAD_ARC_ERROR_UM * board.communication.get_resolution(Unit::Um);
    (edge - f64::from(half_width) - slack).max(0.0)
}

/// K15.40: KiCad's default arc approximation error (`ARC_HIGH_DEF`,
/// 0.005 mm), the most its fill can come nearer an arc of the outline than
/// the edge clearance ([`edge_fill_clearance`]).
const KICAD_ARC_ERROR_UM: f64 = 5.0;

/// K15.23: the strips that erode a fill ring by `erode`: a rectangle of
/// half-width `erode` along each edge and, at each corner, the octagon
/// circumscribing the disc of radius `erode` (KiCad's minimum-thickness
/// opening deflates the fill by a round offset; the octagon takes at most 8 %
/// of `erode` more at a corner).
fn edge_strips(ring: &[(f64, f64)], erode: f64) -> Vec<Ring> {
    let n = ring.len();
    let mut out = Vec::with_capacity(2 * n);
    let t = erode * std::f64::consts::FRAC_PI_8.tan();
    for i in 0..n {
        let (x1, y1) = ring[i];
        let (x2, y2) = ring[(i + 1) % n];
        let length = (x2 - x1).hypot(y2 - y1);
        if length > 0.0 {
            let (nx, ny) = (-(y2 - y1) / length * erode, (x2 - x1) / length * erode);
            out.push(vec![
                (x1 + nx, y1 + ny),
                (x1 - nx, y1 - ny),
                (x2 - nx, y2 - ny),
                (x2 + nx, y2 + ny),
            ]);
        }
        out.push(vec![
            (x1 + erode, y1 - t),
            (x1 + erode, y1 + t),
            (x1 + t, y1 + erode),
            (x1 - t, y1 + erode),
            (x1 - erode, y1 + t),
            (x1 - erode, y1 - t),
            (x1 - t, y1 - erode),
            (x1 + t, y1 - erode),
        ]);
    }
    out
}

/// K15.23: the islands of the detailed fill of conduction area `ca` eroded
/// by `erode` board units ([`fill_operands_eroded`]); empty for an unfilled
/// area. Each island grown back by `erode` is a part of the fill a disc of
/// diameter `2 * erode` fits in: the fill after KiCad's minimum-thickness
/// opening, with the router's clearances.
pub fn eroded_fill_islands(board: &BasicBoard, ca: &Item, erode: f64) -> Vec<Island> {
    opened_fill_islands(board, ca, erode, 0.0, false)
}

/// K15.26: [`eroded_fill_islands`] with every foreign obstacle's clearance
/// `clearance_floor` board units ([`fill_operands_eroded`], K15.33: not the
/// larger of it and the router's); with `clearance_floor` 0 the same islands.
/// K15.35: with `kicad_keepouts` the keepouts are taken as KiCad's zone
/// filler takes them ([`KeepoutFill`]): a rule area or an NPTH cut at its
/// outline, a mask aperture left out, a copper keepout kept the clearance
/// from.
pub fn opened_fill_islands(
    board: &BasicBoard,
    ca: &Item,
    erode: f64,
    clearance_floor: f64,
    kicad_keepouts: bool,
) -> Vec<Island> {
    let filled = matches!(
        ca.as_conduction_area().map(|a| a.kind),
        Some(AreaKind::Conduction {
            is_filled: true,
            ..
        })
    );
    if !filled {
        return Vec::new();
    }
    match fill_operands_eroded(board, ca, erode, clearance_floor, kicad_keepouts) {
        Some((operands, drill_count)) => islands_of(&operands, drill_count),
        None => Vec::new(),
    }
}

/// The islands of the detailed fill of conduction area `ca`; empty for an
/// unfilled area.
pub fn fill_islands(board: &BasicBoard, ca: &Item) -> Vec<Island> {
    let filled = matches!(
        ca.as_conduction_area().map(|a| a.kind),
        Some(AreaKind::Conduction {
            is_filled: true,
            ..
        })
    );
    if !filled {
        return Vec::new();
    }
    match fill_operands(board, ca) {
        Some((operands, drill_count)) => islands_of(&operands, drill_count),
        None => Vec::new(),
    }
}

/// Port of `DesignRulesChecker.getZoneIslandViolations`.
pub(crate) fn zone_island_violations(
    board: &BasicBoard,
    contacts: &mut HashMap<ItemId, Vec<ItemId>>,
) -> Vec<ZoneIslandViolation> {
    let mut result = Vec::new();
    let resolution_mm = board.communication.get_resolution(Unit::Mm);
    let min_dead_copper_area = resolution_mm * resolution_mm;
    let areas = board.conduction_areas();
    // The fills on all cores.
    let all_islands = parallel_map(&areas, |id| {
        board
            .item(*id)
            .map(|ca| fill_islands(board, ca))
            .unwrap_or_default()
    });
    for (&ca_id, islands) in areas.iter().zip(all_islands) {
        let Some(ca) = board.item(ca_id) else {
            continue;
        };
        if islands.len() <= 1 {
            continue;
        }
        let net_number = if ca.net_count() > 0 {
            ca.net_number(0)
        } else {
            0
        };
        let layer = ca.first_layer();
        let same_net_items: Vec<(ItemId, Option<FloatPoint>)> = board
            .items()
            .filter(|i| {
                i.id() != ca_id && i.is_connectable_kind() && i.shares_net(ca) && i.shares_layer(ca)
            })
            .map(|i| {
                let center = match &i.kind {
                    ItemKind::Pin(_) | ItemKind::Via(_) => i.drill().map(|d| d.center.to_float()),
                    ItemKind::Trace(t) => Some(t.first_corner().to_float()),
                    _ => None,
                };
                (i.id(), center)
            })
            .collect();
        let island_items: Vec<Vec<ItemId>> = islands
            .iter()
            .map(|island| {
                same_net_items
                    .iter()
                    .filter(|(_, c)| c.is_some_and(|c| island.contains(c)))
                    .map(|(id, _)| *id)
                    .collect()
            })
            .collect();
        let mut primary = None;
        let mut max_items = None;
        for (i, items) in island_items.iter().enumerate() {
            if max_items.is_none_or(|m| items.len() > m) {
                max_items = Some(items.len());
                primary = Some(i);
            }
        }
        let primary_set = primary
            .filter(|p| !island_items[*p].is_empty())
            .map(|p| board.connected_set_cached(island_items[p][0], net_number, true, contacts));
        for (i, island) in islands.into_iter().enumerate() {
            let items = &island_items[i];
            if !items.is_empty() {
                if Some(i) == primary {
                    continue;
                }
                let connected = primary_set
                    .as_ref()
                    .is_some_and(|set| items.iter().any(|id| set.contains(id)));
                if !connected {
                    result.push(ZoneIslandViolation {
                        conduction_area: ca_id,
                        island,
                        layer,
                        net_number,
                        kind: ZoneIslandKind::IsolatedIslandUnconnected,
                        items_in_island: items.clone(),
                    });
                }
            } else if island.bounding_box_area() >= min_dead_copper_area {
                result.push(ZoneIslandViolation {
                    conduction_area: ca_id,
                    island,
                    layer,
                    net_number,
                    kind: ZoneIslandKind::IsolatedIslandDeadCopper,
                    items_in_island: Vec::new(),
                });
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<(f64, f64)> {
        vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    }

    #[test]
    fn a_cut_splits_the_border_into_two_islands() {
        let operands = vec![
            Operand {
                role: Role::Border,
                rings: vec![square(0.0, 0.0, 100.0, 100.0)],
            },
            Operand {
                role: Role::Foreign,
                rings: vec![square(40.0, -10.0, 60.0, 110.0)],
            },
        ];
        let islands = islands_of(&operands, 0);
        assert_eq!(islands.len(), 2);
        assert_eq!(islands[0].bounds, [0.0, 0.0, 40.0, 100.0]);
        assert_eq!(islands[1].bounds, [60.0, 0.0, 100.0, 100.0]);
        assert!(islands[0].contains(FloatPoint::new(20.0, 50.0)));
        assert!(!islands[0].contains(FloatPoint::new(50.0, 50.0)));
    }

    #[test]
    fn spokes_inside_a_clearance_form_an_island_unless_they_reach_the_fill() {
        let border = Operand {
            role: Role::Border,
            rings: vec![square(0.0, 0.0, 1000.0, 1000.0)],
        };
        // A square clearance around a pad at (500, 500); the diagonal
        // spokes end before its corners.
        let clearance = Operand {
            role: Role::Same(0),
            rings: vec![square(400.0, 400.0, 600.0, 600.0)],
        };
        let spoke = |len: f64| Operand {
            role: Role::Spoke(0),
            rings: vec![vec![
                (500.0 - len, 500.0 - len - 5.0),
                (500.0 + len + 5.0, 500.0 + len),
                (500.0 + len, 500.0 + len + 5.0),
                (500.0 - len - 5.0, 500.0 - len),
            ]],
        };
        let short = vec![border, clearance, spoke(60.0)];
        assert_eq!(islands_of(&short, 1).len(), 2);
        let long = vec![
            Operand {
                role: Role::Border,
                rings: vec![square(0.0, 0.0, 1000.0, 1000.0)],
            },
            Operand {
                role: Role::Same(0),
                rings: vec![square(400.0, 400.0, 600.0, 600.0)],
            },
            spoke(150.0),
        ];
        assert_eq!(islands_of(&long, 1).len(), 1);
    }

    #[test]
    fn a_hole_keeps_one_island() {
        let operands = vec![
            Operand {
                role: Role::Border,
                rings: vec![square(0.0, 0.0, 100.0, 100.0)],
            },
            Operand {
                role: Role::Hole,
                rings: vec![square(30.0, 30.0, 70.0, 70.0)],
            },
        ];
        let islands = islands_of(&operands, 0);
        assert_eq!(islands.len(), 1);
        assert!(!islands[0].contains(FloatPoint::new(50.0, 50.0)));
        assert!(islands[0].contains(FloatPoint::new(10.0, 50.0)));
    }
}

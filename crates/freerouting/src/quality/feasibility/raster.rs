//! The window raster: which cells are foreign copper (per clearance group),
//! own-net copper, the pin itself, and where a via may not go (Q03.1).
//!
//! Cell `(i, j)` of a [`Frame`] has its centre at `((i0 + i)·s, (j0 + j)·s)`
//! in board coordinates, so overlapping windows agree on every cell. A cell
//! belongs to a shape when its centre lies inside the shape (border
//! included). A shape that covers no cell centre at all (a thin line, a
//! tiny pad) marks every cell whose square it touches, which includes the
//! cell of its first corner, so thin items never vanish.
//!
//! An item of the routed net is own copper, except a trace narrower than
//! the class width on its layer, which is ignored: a connection at the
//! class width can't continue on it (D8: class widths are hard limits).
//!
//! Orders that decide results: items are visited in ascending [`ItemId`];
//! groups are sorted by their clearance key ascending (the trace clearance
//! first); a cell's label in a group is the lowest `ItemId` covering it.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{FloatPoint, IntBox, IntPoint, PolylineShapeOps, Shape, TileShape};

use crate::board::BasicBoard;
use crate::board::items::{AreaKind, Item, ItemId, ItemKind};
use crate::board::search::{ShapeSearchTree, TreeKind};

/// The label of a cell that no item covers.
pub const NO_LABEL: i32 = i32::MAX;

/// A fixed-size bit set over the cells of a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bits {
    words: Vec<u64>,
}

impl Bits {
    /// `len` cleared bits.
    pub fn new(len: usize) -> Self {
        Bits {
            words: vec![0; len.div_ceil(64)],
        }
    }

    pub fn get(&self, index: usize) -> bool {
        self.words[index / 64] & (1 << (index % 64)) != 0
    }

    pub fn set(&mut self, index: usize) {
        self.words[index / 64] |= 1 << (index % 64);
    }

    /// The number of set bits.
    pub fn count_ones(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

/// The cell grid of a window: `nx × ny` cells per layer, one slot per
/// routing layer, row-major (`index = (slot·ny + j)·nx + i`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The cell pitch in board units.
    pub step: i32,
    /// The global cell column of local column 0.
    pub i0: i32,
    /// The global cell row of local row 0.
    pub j0: i32,
    pub nx: usize,
    pub ny: usize,
    /// The board layer of each slot, ascending.
    pub layers: Vec<i32>,
}

impl Frame {
    /// The smallest frame whose cell centres cover `bounds`.
    pub fn covering(bounds: &IntBox, step: i32, layers: Vec<i32>) -> Frame {
        assert!(step > 0, "the raster step must be positive");
        let i0 = bounds.ll.x.div_euclid(step);
        let j0 = bounds.ll.y.div_euclid(step);
        let i1 = ceil_div(bounds.ur.x, step);
        let j1 = ceil_div(bounds.ur.y, step);
        Frame {
            step,
            i0,
            j0,
            nx: (i1 - i0 + 1) as usize,
            ny: (j1 - j0 + 1) as usize,
            layers,
        }
    }

    /// The number of cells on one layer.
    pub fn layer_cells(&self) -> usize {
        self.nx * self.ny
    }

    /// The number of cells on all layers.
    pub fn cell_count(&self) -> usize {
        self.layer_cells() * self.layers.len()
    }

    pub fn index(&self, slot: usize, i: usize, j: usize) -> usize {
        (slot * self.ny + j) * self.nx + i
    }

    /// `(slot, i, j)` of cell `index`.
    pub fn coords(&self, index: usize) -> (usize, usize, usize) {
        let per_layer = self.layer_cells();
        let slot = index / per_layer;
        let rest = index % per_layer;
        (slot, rest % self.nx, rest / self.nx)
    }

    /// The centre of local cell `(i, j)` in board units.
    pub fn centre(&self, i: usize, j: usize) -> IntPoint {
        IntPoint {
            x: (self.i0 + i as i32) * self.step,
            y: (self.j0 + j as i32) * self.step,
        }
    }

    /// The slot of board layer `layer`.
    pub fn slot_of(&self, layer: i32) -> Option<usize> {
        self.layers.iter().position(|l| *l == layer)
    }

    /// True if local cell `(i, j)` is on the border of the frame.
    pub fn is_border(&self, i: usize, j: usize) -> bool {
        i == 0 || j == 0 || i + 1 == self.nx || j + 1 == self.ny
    }

    /// The board area the cell centres span.
    pub fn bounds(&self) -> IntBox {
        IntBox {
            ll: self.centre(0, 0),
            ur: self.centre(self.nx - 1, self.ny - 1),
        }
    }
}

fn ceil_div(a: i32, b: i32) -> i32 {
    -((-a).div_euclid(b))
}

/// A convex piece of an item's shape on one layer.
#[derive(Clone, Debug)]
pub enum Piece {
    Disk {
        centre: FloatPoint,
        radius: f64,
    },
    /// A convex polygon (possibly degenerate) given by its corners.
    Polygon(Vec<FloatPoint>),
}

impl Piece {
    fn y_range(&self) -> (f64, f64) {
        match self {
            Piece::Disk { centre, radius } => (centre.y - radius, centre.y + radius),
            Piece::Polygon(corners) => corners
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(p.y), hi.max(p.y))
                }),
        }
    }

    /// The x interval of the piece on the horizontal line `y`.
    fn row_span(&self, y: f64) -> Option<(f64, f64)> {
        match self {
            Piece::Disk { centre, radius } => {
                let dy = y - centre.y;
                let h2 = radius * radius - dy * dy;
                (h2 >= 0.0).then(|| {
                    let h = h2.sqrt();
                    (centre.x - h, centre.x + h)
                })
            }
            Piece::Polygon(corners) => {
                let mut span = Span::default();
                let n = corners.len();
                for k in 0..n {
                    let (p, q) = (corners[k], corners[(k + 1) % n]);
                    if y < p.y.min(q.y) || y > p.y.max(q.y) {
                        continue;
                    }
                    if p.y == q.y {
                        span.add(p.x);
                        span.add(q.x);
                    } else {
                        span.add(p.x + (y - p.y) * (q.x - p.x) / (q.y - p.y));
                    }
                }
                span.get()
            }
        }
    }

    /// The x interval of the piece within the band `y0 ≤ y ≤ y1`.
    fn band_span(&self, y0: f64, y1: f64) -> Option<(f64, f64)> {
        match self {
            Piece::Disk { centre, .. } => {
                let nearest = centre.y.clamp(y0, y1);
                self.row_span(nearest)
            }
            Piece::Polygon(corners) => {
                let mut span = Span::default();
                for p in corners {
                    if p.y >= y0 && p.y <= y1 {
                        span.add(p.x);
                    }
                }
                for y in [y0, y1] {
                    if let Some((lo, hi)) = self.row_span(y) {
                        span.add(lo);
                        span.add(hi);
                    }
                }
                span.get()
            }
        }
    }
}

#[derive(Default)]
struct Span {
    lo: Option<f64>,
    hi: f64,
}

impl Span {
    fn add(&mut self, x: f64) {
        match self.lo {
            None => {
                self.lo = Some(x);
                self.hi = x;
            }
            Some(lo) => {
                self.lo = Some(lo.min(x));
                self.hi = self.hi.max(x);
            }
        }
    }

    fn get(&self) -> Option<(f64, f64)> {
        self.lo.map(|lo| (lo, self.hi))
    }
}

/// Calls `mark` with the layer-local index (`j·nx + i`) of every cell of
/// `frame` that `piece` covers. Returns the number of marked cells.
pub fn rasterize(frame: &Frame, piece: &Piece, mut mark: impl FnMut(usize)) -> usize {
    let s = f64::from(frame.step);
    let (ymin, ymax) = piece.y_range();
    if ymin.is_nan() || ymax.is_nan() || ymin > ymax {
        return 0;
    }
    let mut marked = 0;
    let mut covers_a_centre = false;
    let mut mark_range = |j: i64, xl: f64, xr: f64, marked: &mut usize| {
        let ilo = (xl / s).ceil() as i64 - i64::from(frame.i0);
        let ihi = (xr / s).floor() as i64 - i64::from(frame.i0);
        if j < 0 || j >= frame.ny as i64 {
            return;
        }
        let ilo = ilo.max(0);
        let ihi = ihi.min(frame.nx as i64 - 1);
        for i in ilo..=ihi {
            mark(j as usize * frame.nx + i as usize);
            *marked += 1;
        }
    };
    let row_lo = (ymin / s).ceil() as i64;
    let row_hi = (ymax / s).floor() as i64;
    for row in row_lo..=row_hi {
        if let Some((xl, xr)) = piece.row_span(row as f64 * s)
            && (xl / s).ceil() <= (xr / s).floor()
        {
            covers_a_centre = true;
            mark_range(row - i64::from(frame.j0), xl, xr, &mut marked);
        }
    }
    if covers_a_centre {
        return marked;
    }
    // A thin piece: mark every cell whose square it touches.
    let half = s / 2.0;
    let row_lo = ((ymin - half) / s).ceil() as i64;
    let row_hi = ((ymax + half) / s).floor() as i64;
    for row in row_lo..=row_hi {
        let y = row as f64 * s;
        if let Some((xl, xr)) = piece.band_span(y - half, y + half) {
            mark_range(row - i64::from(frame.j0), xl - half, xr + half, &mut marked);
        }
    }
    marked
}

/// The pieces of `item`'s shape on each layer, as `(layer, piece)`.
///
/// Pins and vias use their true pad shapes (a circle stays a circle); other
/// items use the tree shapes of an uncompensated plain search tree, which
/// are the shapes Java's clearance checks use. A drill item layer without a
/// pad shape (a bare hole) is skipped: every simplification errs toward
/// "passable".
pub fn item_pieces(board: &BasicBoard, item: &Item) -> Vec<(i32, Piece)> {
    let mut result = Vec::new();
    if let Some(drill) = item.drill() {
        for index in 0..drill.shapes.len() {
            let layer = drill.shape_layer(index);
            match drill.shape(index) {
                Some(Shape::Circle(circle)) => result.push((
                    layer,
                    Piece::Disk {
                        centre: circle.center.to_float(),
                        radius: f64::from(circle.radius),
                    },
                )),
                Some(shape) => {
                    for tile in shape.split_to_convex().unwrap_or_default() {
                        result.push((layer, polygon(&tile)));
                    }
                }
                None => {}
            }
        }
        return result;
    }
    if matches!(item.kind, ItemKind::ComponentOutline(_)) {
        return result;
    }
    let tree = ShapeSearchTree::new(TreeKind::Plain, 0);
    let shapes = tree.calculate_tree_shapes(item, &board.shape_context());
    for (index, shape) in shapes.into_iter().enumerate() {
        if let Some(tile) = shape {
            result.push((item.shape_layer(index), polygon(&tile)));
        }
    }
    result
}

fn polygon(tile: &TileShape) -> Piece {
    Piece::Polygon(tile.corner_approx_arr())
}

/// One clearance group: the items whose clearance to the routed net's
/// traces (and vias) is the same.
#[derive(Clone, Debug)]
pub struct Group {
    /// The clearance `c_k` between the routed trace and the group's items,
    /// in board units, without the safety margin.
    pub clearance: i32,
    /// The clearance of each via of the net's via rule to the group's items.
    pub via_clearances: Vec<i32>,
    /// The cells the group's items cover, on all layers.
    pub source: Bits,
    /// Per cell, the lowest item id covering it ([`NO_LABEL`] if none).
    pub labels: Vec<i32>,
}

/// What to rasterise for one pin.
#[derive(Clone, Debug)]
pub struct WindowSpec {
    pub frame: Frame,
    /// The pin to route from.
    pub pin: ItemId,
    /// The routed net.
    pub net: i32,
    /// The net class's trace clearance class.
    pub trace_clearance_class: i32,
    /// The clearance class of each via of the net's via rule.
    pub via_clearance_classes: Vec<i32>,
    /// The class trace half width on each frame slot. Own traces narrower
    /// than this are ignored: a connection at the class width can't use
    /// them.
    pub class_half_width: Vec<i32>,
}

/// The rasterised window of one pin.
#[derive(Clone, Debug)]
pub struct Window {
    pub frame: Frame,
    pub pin: ItemId,
    pub net: i32,
    /// Foreign copper and keepouts, by clearance key ascending.
    pub groups: Vec<Group>,
    /// Copper of the routed net other than the pin.
    pub own: Bits,
    /// The pin's own shapes.
    pub start: Bits,
    /// Cells where no via may be placed: pads of any net (D8) and via
    /// keepouts of other nets.
    pub via_blocked: Bits,
    /// The items that overlap the window, ascending.
    pub items: Vec<ItemId>,
}

/// How an item takes part in a window.
enum Role {
    Start,
    Own,
    Obstacle,
    Ignored,
}

fn role(board: &BasicBoard, item: &Item, spec: &WindowSpec) -> Role {
    if item.id() == spec.pin {
        Role::Start
    } else if item.contains_net(spec.net) && item.is_connectable_kind() {
        match item.as_trace() {
            Some(trace)
                if spec
                    .frame
                    .slot_of(trace.layer)
                    .is_some_and(|slot| trace.half_width < spec.class_half_width[slot]) =>
            {
                Role::Ignored
            }
            _ => Role::Own,
        }
    } else if board.is_trace_obstacle(item, spec.net) {
        Role::Obstacle
    } else {
        Role::Ignored
    }
}

fn blocks_vias(item: &Item, net: i32) -> bool {
    match &item.kind {
        ItemKind::Pin(_) => true,
        ItemKind::Area(area) => area.kind == AreaKind::ViaObstacle && !item.contains_net(net),
        _ => false,
    }
}

/// Rasterises the window `spec` describes.
pub fn build_window(board: &BasicBoard, spec: &WindowSpec) -> Window {
    let frame = spec.frame.clone();
    let cells = frame.cell_count();
    let per_layer = frame.layer_cells();
    let window_box = TileShape::IntBox(frame.bounds());
    let mut ids = BTreeSet::new();
    for &layer in &frame.layers {
        ids.extend(board.overlapping_objects(&window_box, layer));
    }
    let matrix = &board.rules.clearance_matrix;
    let mut own = Bits::new(cells);
    let mut start = Bits::new(cells);
    let mut via_blocked = Bits::new(cells);
    let mut groups: BTreeMap<Vec<i32>, Group> = BTreeMap::new();
    for &id in &ids {
        let Some(item) = board.item(id) else {
            continue;
        };
        let role = role(board, item, spec);
        let blocks = blocks_vias(item, spec.net);
        if matches!(role, Role::Ignored) && !blocks {
            continue;
        }
        for (layer, piece) in item_pieces(board, item) {
            let Some(slot) = frame.slot_of(layer) else {
                continue;
            };
            let offset = slot * per_layer;
            if blocks {
                rasterize(&frame, &piece, |c| via_blocked.set(offset + c));
            }
            match role {
                Role::Start => {
                    rasterize(&frame, &piece, |c| start.set(offset + c));
                }
                Role::Own => {
                    rasterize(&frame, &piece, |c| own.set(offset + c));
                }
                Role::Obstacle => {
                    let class = item.clearance_class();
                    let mut key =
                        vec![matrix.get_value(spec.trace_clearance_class, class, layer, false)];
                    key.extend(
                        spec.via_clearance_classes
                            .iter()
                            .map(|via| matrix.get_value(*via, class, layer, false)),
                    );
                    let group = groups.entry(key.clone()).or_insert_with(|| Group {
                        clearance: key[0],
                        via_clearances: key[1..].to_vec(),
                        source: Bits::new(cells),
                        labels: vec![NO_LABEL; cells],
                    });
                    rasterize(&frame, &piece, |c| {
                        let cell = offset + c;
                        group.source.set(cell);
                        group.labels[cell] = group.labels[cell].min(id.0);
                    });
                }
                Role::Ignored => {}
            }
        }
    }
    Window {
        frame,
        pin: spec.pin,
        net: spec.net,
        groups: groups.into_values().collect(),
        own,
        start,
        via_blocked,
        items: ids.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(step: i32) -> Frame {
        Frame::covering(
            &IntBox {
                ll: IntPoint { x: -2000, y: -2000 },
                ur: IntPoint { x: 2000, y: 2000 },
            },
            step,
            vec![0],
        )
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Piece {
        Piece::Polygon(vec![
            FloatPoint { x: x0, y: y0 },
            FloatPoint { x: x1, y: y0 },
            FloatPoint { x: x1, y: y1 },
            FloatPoint { x: x0, y: y1 },
        ])
    }

    #[test]
    fn frame_has_a_fixed_origin() {
        let f = frame(10);
        assert_eq!((f.i0, f.j0, f.nx, f.ny), (-200, -200, 401, 401));
        assert_eq!(f.centre(0, 0), IntPoint { x: -2000, y: -2000 });
        let g = Frame::covering(
            &IntBox {
                ll: IntPoint { x: -1995, y: 3 },
                ur: IntPoint { x: 7, y: 21 },
            },
            10,
            vec![0, 1],
        );
        assert_eq!((g.i0, g.j0, g.nx, g.ny), (-200, 0, 202, 4));
        let index = g.index(1, 5, 3);
        assert_eq!(g.coords(index), (1, 5, 3));
    }

    /// A 0.25 × 0.8 mm pad at a 10 µm step (board units of 1 µm), placed
    /// off the cell centres: 25 columns × 80 rows.
    #[test]
    fn pad_covers_the_expected_cell_count() {
        let f = frame(10);
        let pad = rect(-122.0, -397.0, 128.0, 403.0);
        let mut cells = Vec::new();
        assert_eq!(rasterize(&f, &pad, |c| cells.push(c)), 25 * 80);
        cells.sort_unstable();
        cells.dedup();
        assert_eq!(cells.len(), 2000);
        let disk = Piece::Disk {
            centre: FloatPoint { x: 0.0, y: 0.0 },
            radius: 15.0,
        };
        // Centres within 15 of the origin: (0,0), (±10,0), (0,±10), (±10,±10).
        assert_eq!(rasterize(&f, &disk, |_| {}), 9);
    }

    #[test]
    fn thin_shape_still_marks_a_cell() {
        let f = frame(10);
        let thin = rect(1.0, 1.0, 6.0, 8.0);
        let mut cells = Vec::new();
        rasterize(&f, &thin, |c| cells.push(c));
        let first_corner = (0 - f.j0) as usize * f.nx + (0 - f.i0) as usize;
        assert!(cells.contains(&first_corner), "{cells:?}");
        assert!(cells.len() <= 4);
        // A long zero-width line marks a connected run of cells.
        let line = Piece::Polygon(vec![
            FloatPoint { x: -500.0, y: 3.0 },
            FloatPoint { x: 500.0, y: 3.0 },
        ]);
        let mut run = Vec::new();
        rasterize(&f, &line, |c| run.push(c));
        assert_eq!(run.len(), 101);
    }

    #[test]
    fn piece_outside_the_frame_marks_nothing() {
        let f = frame(10);
        assert_eq!(
            rasterize(&f, &rect(5000.0, 5000.0, 6000.0, 6000.0), |_| {}),
            0
        );
        // Clipped at the frame border.
        assert_eq!(rasterize(&f, &rect(1995.0, -4.0, 3000.0, 4.0), |_| {}), 1);
    }

    /// A pin of net A between a pad of the default class (east) and a pad
    /// of a class with a larger clearance (west), with a via keepout to the
    /// north.
    const GROUPS_DSN: &str = r#"(pcb groups.dsn
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal))
    (layer B.Cu (type signal))
    (boundary (path pcb 0  0 0  20000 0  20000 20000  0 20000  0 0))
    (via VIA600)
    (rule (width 200) (clearance 200))
    (via_keepout "" (polygon F.Cu 0  9000 12000  11000 12000  11000 13000  9000 13000))
  )
  (placement
    (component PAD
      (place P 10000 10000 front 0)
      (place Q 12000 10000 front 0)
      (place R 8000 10000 front 0)
      (place JA 10000 4000 front 0)
    )
  )
  (library
    (image PAD (pin SQ 1 0 0))
    (padstack SQ (shape (rect F.Cu -300 -300 300 300)) (attach off))
    (padstack VIA600 (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
    (net A (pins P-1 JA-1))
    (net B (pins Q-1))
    (net C (pins R-1))
    (class kicad_default B (rule (width 200) (clearance 200)))
    (class WIDE A (circuit (use_via VIA600)) (rule (width 1000) (clearance 300)))
    (class FAR C (rule (width 200) (clearance 600)))
  )
)
"#;

    fn groups_window() -> (crate::board::RoutingBoard, Window) {
        let board = crate::drc::load_design(GROUPS_DSN.as_bytes()).expect("board");
        let pin = board
            .pins()
            .into_iter()
            .find(|id| {
                let item = board.item(*id).expect("pin");
                item.drill().expect("drill").center
                    == IntPoint {
                        x: 100000,
                        y: 100000,
                    }
            })
            .expect("pin P");
        let item = board.item(pin).expect("pin");
        let net = item.net_number(0);
        let class = board.rules.net_class_of(net).expect("class");
        let frame = Frame::covering(
            &IntBox {
                ll: IntPoint { x: 60000, y: 60000 },
                ur: IntPoint {
                    x: 140000,
                    y: 140000,
                },
            },
            100,
            vec![0, 1],
        );
        let spec = WindowSpec {
            class_half_width: vec![class.trace_half_width(0), class.trace_half_width(1)],
            frame,
            pin,
            net,
            trace_clearance_class: class.trace_clearance_class(),
            via_clearance_classes: vec![class.trace_clearance_class()],
        };
        let window = build_window(&board, &spec);
        (board, window)
    }

    #[test]
    fn items_with_different_clearance_classes_land_in_two_groups() {
        let (board, window) = groups_window();
        // The outline is a third group; the two pads differ from each other.
        let pad_groups: Vec<&Group> = window
            .groups
            .iter()
            .filter(|g| {
                g.labels
                    .iter()
                    .any(|l| *l != NO_LABEL && board.item(ItemId(*l)).is_some_and(|i| i.is_pin()))
            })
            .collect();
        assert_eq!(
            pad_groups.len(),
            2,
            "{:?}",
            window
                .groups
                .iter()
                .map(|g| g.clearance)
                .collect::<Vec<_>>()
        );
        assert!(pad_groups[0].clearance < pad_groups[1].clearance);
        assert!(
            window
                .groups
                .windows(2)
                .all(|w| w[0].clearance <= w[1].clearance)
        );
        // Each 0.6 mm square pad covers 61 × 61 cell centres at 0.01 mm, on F.Cu only.
        for group in &pad_groups {
            let f_cu = (0..window.frame.layer_cells())
                .filter(|c| group.source.get(*c))
                .count();
            assert_eq!(f_cu, 61 * 61);
        }
        assert_eq!(window.start.count_ones(), 61 * 61);
        assert_eq!(window.own.count_ones(), 0);
    }

    #[test]
    fn via_keepout_blocks_vias_only() {
        let (_, window) = groups_window();
        let frame = &window.frame;
        // A cell inside the via keepout: (10.0, 12.5) mm.
        let cell = frame.index(0, 400, 650);
        assert_eq!(
            frame.centre(400, 650),
            IntPoint {
                x: 100000,
                y: 125000
            }
        );
        assert!(window.via_blocked.get(cell));
        assert!(window.groups.iter().all(|g| !g.source.get(cell)));
        // Pads block vias too (D8), the pin's own included.
        assert!(window.via_blocked.get(frame.index(0, 400, 400)));
        assert!(window.via_blocked.get(frame.index(0, 600, 400)));
        // Not on the layer below.
        assert!(!window.via_blocked.get(frame.index(1, 400, 650)));
    }
}

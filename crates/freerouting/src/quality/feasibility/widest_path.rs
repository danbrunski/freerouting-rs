//! The margin field and the Kruskal widest path with via edges (Q03.3).
//!
//! `D(p) = min_k(dist_k(p) − c_k)` over the clearance groups, where
//! `dist_k` is the distance from the cell centre to the nearest source cell
//! centre of group `k`. A trace of width `w` centred at `p` keeps its
//! clearances when `2·D(p) ≥ w`, so `margin(p) = ⌊2·D(p) − req(p)⌋`.
//! Cells are activated in descending margin under a disjoint-set union; the
//! first activation that joins a component holding the pin to one holding a
//! target is the throat, and the corridor is `C = margin + req` there.
//!
//! Orders that decide results: cells are sorted by `(margin desc, index
//! asc)`, where the index order is `(layer slot, j, i)`; neighbours are
//! visited E, NE, N, NW, W, SW, S, SE, then the via layers ascending; ties
//! between groups at one cell keep the group with the lower key.

use freerouting_geometry::FloatPoint;

use super::edt::{INF, NO_SOURCE, squared_edt_by};
use super::raster::{Bits, Frame, NO_LABEL, Window};

/// The margin of a cell that no obstacle constrains.
pub const FREE_MARGIN: i64 = i64::MAX / 4;

/// The 8 same-layer neighbour offsets, in visiting order: E, NE, N, NW, W,
/// SW, S, SE (`j` grows northward).
const NEIGHBOURS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// One via of the net's via rule, as seen by the window.
#[derive(Clone, Debug)]
pub struct ViaSpan {
    /// The first and last frame slot the via reaches.
    pub first_slot: usize,
    pub last_slot: usize,
    /// The via pad radius on each slot of the span (board units), indexed
    /// by `slot - first_slot`.
    pub radius: Vec<f64>,
}

/// The distance fields of one window.
#[derive(Clone, Debug)]
pub struct Field {
    /// `D(p)` in board units; `f64::INFINITY` with no obstacle in the window.
    pub clearance: Vec<f64>,
    /// The item of the group that sets `D(p)` ([`NO_LABEL`] if none).
    pub label: Vec<i32>,
    /// The nearest source cell (global cell index) of that group.
    pub nearest: Vec<u32>,
    /// Per via of the rule, the layer cells `(j·nx + i)` where it is legal.
    pub via_ok: Vec<Bits>,
}

/// Computes the distance fields of `window`.
pub fn field(window: &Window, vias: &[ViaSpan]) -> Field {
    let frame = &window.frame;
    let cells = frame.cell_count();
    let per_layer = frame.layer_cells();
    let step = f64::from(frame.step);
    let mut clearance = vec![f64::INFINITY; cells];
    let mut label = vec![NO_LABEL; cells];
    let mut nearest = vec![NO_SOURCE; cells];
    let mut via_room: Vec<Vec<f64>> = vias.iter().map(|_| vec![f64::INFINITY; cells]).collect();
    for group in &window.groups {
        for slot in 0..frame.layers.len() {
            let offset = slot * per_layer;
            let (d2, source) = squared_edt_by(frame.nx, frame.ny, |c| group.source.get(offset + c));
            for c in 0..per_layer {
                if d2[c] == INF {
                    continue;
                }
                let cell = offset + c;
                let dist = (d2[c] as f64).sqrt() * step;
                let value = dist - f64::from(group.clearance);
                if value < clearance[cell] {
                    clearance[cell] = value;
                    let source_cell = offset + source[c] as usize;
                    label[cell] = group.labels[source_cell];
                    nearest[cell] = source_cell as u32;
                }
                for (v, room) in via_room.iter_mut().enumerate() {
                    let value = dist - f64::from(group.via_clearances[v]);
                    if value < room[cell] {
                        room[cell] = value;
                    }
                }
            }
        }
    }
    let via_ok = vias
        .iter()
        .zip(&via_room)
        .map(|(via, room)| {
            let mut ok = Bits::new(per_layer);
            for c in 0..per_layer {
                let legal = (via.first_slot..=via.last_slot).all(|slot| {
                    let cell = slot * per_layer + c;
                    room[cell] >= via.radius[slot - via.first_slot] && !window.via_blocked.get(cell)
                });
                if legal {
                    ok.set(c);
                }
            }
            ok
        })
        .collect();
    Field {
        clearance,
        label,
        nearest,
        via_ok,
    }
}

/// The required trace width per cell.
#[derive(Clone, Debug)]
pub struct Requirement {
    /// The class width on each slot (board units).
    pub width: Vec<i64>,
    /// A narrower width within a radius of a point: `(centre, radius,
    /// width per slot)` (Q03.5).
    pub neck: Option<(FloatPoint, f64, Vec<i64>)>,
}

impl Requirement {
    /// The width required at `cell`.
    pub fn at(&self, frame: &Frame, cell: usize) -> i64 {
        let (slot, i, j) = frame.coords(cell);
        if let Some((centre, radius, width)) = &self.neck {
            let p = frame.centre(i, j);
            let (dx, dy) = (f64::from(p.x) - centre.x, f64::from(p.y) - centre.y);
            if dx * dx + dy * dy <= radius * radius {
                return width[slot];
            }
        }
        self.width[slot]
    }
}

/// How the search ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathOutcome {
    /// The pin touches own copper or a target before any cell is added.
    Connected,
    /// The pin joined a target when the throat cell was added.
    Throat {
        cell: usize,
        /// `margin(throat)`.
        margin: i64,
        /// `C = margin + req` at the throat (board units).
        corridor: i64,
        /// `req(throat)`.
        required: i64,
    },
    /// No path at any width: no target in the window at all (every cell,
    /// obstacle or not, is eventually added, so a pin behind copper gets a
    /// negative corridor instead).
    Enclosed,
}

/// `margin(p)` for every cell (own cells [`i64::MAX`]).
pub fn margins(window: &Window, field: &Field, req: &Requirement) -> Vec<i64> {
    (0..window.frame.cell_count())
        .map(|cell| {
            if window.own.get(cell) {
                return i64::MAX;
            }
            let d = field.clearance[cell];
            if d.is_infinite() {
                return FREE_MARGIN;
            }
            let m = (2.0 * d - req.at(&window.frame, cell) as f64).floor();
            m.clamp(-(FREE_MARGIN as f64), FREE_MARGIN as f64) as i64
        })
        .collect()
}

const START: u8 = 1;
const TARGET: u8 = 2;

struct Forest {
    parent: Vec<u32>,
    size: Vec<u32>,
    flags: Vec<u8>,
}

impl Forest {
    fn new(n: usize) -> Self {
        Forest {
            parent: (0..n as u32).collect(),
            size: vec![1; n],
            flags: vec![0; n],
        }
    }

    /// The root of `x`, with path halving.
    fn find(&mut self, mut x: u32) -> u32 {
        while self.parent[x as usize] != x {
            let grand = self.parent[self.parent[x as usize] as usize];
            self.parent[x as usize] = grand;
            x = grand;
        }
        x
    }

    /// Joins the sets of `a` and `b` (union by size). Returns the root's
    /// flags.
    fn union(&mut self, a: u32, b: u32) -> u8 {
        let (mut ra, mut rb) = (self.find(a), self.find(b));
        if ra == rb {
            return self.flags[ra as usize];
        }
        if self.size[ra as usize] < self.size[rb as usize] {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent[rb as usize] = ra;
        self.size[ra as usize] += self.size[rb as usize];
        self.flags[ra as usize] |= self.flags[rb as usize];
        self.flags[ra as usize]
    }
}

/// The Kruskal search state.
struct Search<'a> {
    window: &'a Window,
    field: &'a Field,
    vias: &'a [ViaSpan],
    start: &'a Bits,
    plane_targets: bool,
    active: Bits,
    forest: Forest,
}

impl Search<'_> {
    /// Activates `cell`; true if its set now holds the pin and a target.
    fn activate(&mut self, cell: usize) -> bool {
        let frame = &self.window.frame;
        let (slot, i, j) = frame.coords(cell);
        let layer_cell = j * frame.nx + i;
        let mut flags = 0;
        if self.start.get(cell) {
            flags |= START;
        }
        let via_here = |v: usize, via: &ViaSpan| {
            (via.first_slot..=via.last_slot).contains(&slot) && self.field.via_ok[v].get(layer_cell)
        };
        let plane_via = self.plane_targets
            && self
                .vias
                .iter()
                .enumerate()
                .any(|(v, via)| via_here(v, via));
        if self.window.own.get(cell) || frame.is_border(i, j) || plane_via {
            flags |= TARGET;
        }
        self.active.set(cell);
        self.forest.flags[cell] = flags;
        let mut root_flags = flags;
        for (di, dj) in NEIGHBOURS {
            let (ni, nj) = (i as i64 + i64::from(di), j as i64 + i64::from(dj));
            if ni < 0 || nj < 0 || ni >= frame.nx as i64 || nj >= frame.ny as i64 {
                continue;
            }
            let other = frame.index(slot, ni as usize, nj as usize);
            if self.active.get(other) {
                root_flags = self.forest.union(cell as u32, other as u32);
            }
        }
        for (v, via) in self.vias.iter().enumerate() {
            if !via_here(v, via) {
                continue;
            }
            for other_slot in via.first_slot..=via.last_slot {
                if other_slot == slot {
                    continue;
                }
                let other = frame.index(other_slot, i, j);
                if self.active.get(other) {
                    root_flags = self.forest.union(cell as u32, other as u32);
                }
            }
        }
        root_flags == START | TARGET
    }
}

/// Runs the widest-path search on `window` with the fields `field`, from
/// the cells `start` (the pin's shapes, [`Window::start`], or only its
/// centre cells).
///
/// `plane_targets`: the net has a plane, so a legal via site is a target.
pub fn widest_path(
    window: &Window,
    field: &Field,
    vias: &[ViaSpan],
    req: &Requirement,
    start: &Bits,
    plane_targets: bool,
) -> PathOutcome {
    let frame = &window.frame;
    let cells = frame.cell_count();
    let margin = margins(window, field, req);
    let mut search = Search {
        window,
        field,
        vias,
        start,
        plane_targets,
        active: Bits::new(cells),
        forest: Forest::new(cells),
    };
    let mut connected = false;
    for cell in 0..cells {
        if start.get(cell) || window.own.get(cell) {
            connected |= search.activate(cell);
        }
    }
    if connected {
        return PathOutcome::Connected;
    }
    let mut order: Vec<(i64, u32)> = (0..cells)
        .filter(|&c| !search.active.get(c))
        .map(|c| (-margin[c], c as u32))
        .collect();
    order.sort_unstable();
    for (_, cell) in order {
        let cell = cell as usize;
        if search.activate(cell) {
            let required = req.at(frame, cell);
            return PathOutcome::Throat {
                cell,
                margin: margin[cell],
                corridor: margin[cell].saturating_add(required),
                required,
            };
        }
    }
    PathOutcome::Enclosed
}

#[cfg(test)]
mod tests {
    use super::super::raster::{Group, Piece, rasterize};
    use super::*;
    use freerouting_geometry::{IntBox, IntPoint};

    /// A window of `2·half` board units square at step 10, on `layers`
    /// slots, with the pin a disk of radius 100 at the origin on slot 0.
    fn window(half: i32, layers: usize) -> Window {
        let frame = Frame::covering(
            &IntBox {
                ll: IntPoint { x: -half, y: -half },
                ur: IntPoint { x: half, y: half },
            },
            10,
            (0..layers as i32).collect(),
        );
        let cells = frame.cell_count();
        let mut start = Bits::new(cells);
        let pin = Piece::Disk {
            centre: FloatPoint { x: 0.0, y: 0.0 },
            radius: 100.0,
        };
        rasterize(&frame, &pin, |c| start.set(c));
        Window {
            groups: Vec::new(),
            own: Bits::new(cells),
            start,
            via_blocked: Bits::new(cells),
            items: Vec::new(),
            pin: crate::board::items::ItemId(1),
            net: 1,
            frame,
        }
    }

    fn add_obstacle(window: &mut Window, slot: usize, piece: &Piece, clearance: i32, id: i32) {
        let cells = window.frame.cell_count();
        if window.groups.iter().all(|g| g.clearance != clearance) {
            window.groups.push(Group {
                clearance,
                via_clearances: vec![clearance],
                source: Bits::new(cells),
                labels: vec![NO_LABEL; cells],
            });
            window.groups.sort_by_key(|g| g.clearance);
        }
        let offset = slot * window.frame.layer_cells();
        let group = window
            .groups
            .iter_mut()
            .find(|g| g.clearance == clearance)
            .expect("group");
        rasterize(&window.frame, piece, |c| {
            group.source.set(offset + c);
            group.labels[offset + c] = group.labels[offset + c].min(id);
        });
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Piece {
        Piece::Polygon(vec![
            FloatPoint { x: x0, y: y0 },
            FloatPoint { x: x1, y: y0 },
            FloatPoint { x: x1, y: y1 },
            FloatPoint { x: x0, y: y1 },
        ])
    }

    fn requirement(width: i64, layers: usize) -> Requirement {
        Requirement {
            width: vec![width; layers],
            neck: None,
        }
    }

    const DELTA: f64 = 10.0 * std::f64::consts::SQRT_2;

    /// Board units are µm here: a corridor 1000 wide (y from -500 to 500)
    /// between two long foreign pads, clearance 100, open to the east.
    /// The pin sits in the corridor, the west end is closed.
    fn corridor_window() -> Window {
        let mut w = window(3000, 1);
        add_obstacle(&mut w, 0, &rect(-3000.0, 500.0, 3000.0, 3000.0), 100, 7);
        add_obstacle(&mut w, 0, &rect(-3000.0, -3000.0, 3000.0, -500.0), 100, 8);
        add_obstacle(&mut w, 0, &rect(-3000.0, -500.0, -2000.0, 500.0), 100, 9);
        w
    }

    #[test]
    fn corridor_width_is_found_within_delta() {
        let w = corridor_window();
        let f = field(&w, &[]);
        let outcome = widest_path(&w, &f, &[], &requirement(500, 1), &w.start, false);
        let PathOutcome::Throat { cell, corridor, .. } = outcome else {
            panic!("expected a throat, got {outcome:?}");
        };
        // 1.0 − 2c in mm: 1000 − 200.
        let expected = 1000.0 - 2.0 * 100.0;
        assert!(
            (corridor as f64 - expected).abs() <= DELTA,
            "C = {corridor}"
        );
        // The throat lies in the corridor.
        let (_, i, j) = w.frame.coords(cell);
        let p = w.frame.centre(i, j);
        assert!(p.y.abs() < 500 && p.x > -2000, "throat at {p:?}");
        let throat_label = f.label[cell];
        assert!(throat_label == 7 || throat_label == 8, "{throat_label}");
    }

    fn ring(w: &mut Window, slot: usize) {
        // A square ring from 600 to 900 around the pin.
        add_obstacle(w, slot, &rect(-900.0, 600.0, 900.0, 900.0), 100, 20);
        add_obstacle(w, slot, &rect(-900.0, -900.0, 900.0, -600.0), 100, 21);
        add_obstacle(w, slot, &rect(-900.0, -600.0, -600.0, 600.0), 100, 22);
        add_obstacle(w, slot, &rect(600.0, -600.0, 900.0, 600.0), 100, 23);
    }

    #[test]
    fn enclosed_pin_is_infeasible_at_every_width() {
        let mut w = window(2000, 2);
        ring(&mut w, 0);
        let f = field(&w, &[]);
        for width in [1, 100, 1000] {
            let outcome = widest_path(&w, &f, &[], &requirement(width, 2), &w.start, false);
            let (verdict, corridor) = super::super::verdict_of(&outcome, DELTA);
            assert_eq!(verdict, super::super::Verdict::Infeasible, "{outcome:?}");
            // The best path crosses the ring's clearance zone: C = 2·(0 − c).
            assert_eq!(corridor, Some(-200));
        }
    }

    #[test]
    fn via_inside_the_ring_opens_the_layer_below() {
        let mut w = window(2000, 2);
        ring(&mut w, 0);
        let vias = [ViaSpan {
            first_slot: 0,
            last_slot: 1,
            radius: vec![50.0, 50.0],
        }];
        let f = field(&w, &vias);
        // A via fits between the pin edge and the ring (but not on the pin).
        let outcome = widest_path(&w, &f, &vias, &requirement(200, 2), &w.start, false);
        let PathOutcome::Throat { corridor, cell, .. } = outcome else {
            panic!("expected a throat, got {outcome:?}");
        };
        assert!(corridor >= 200, "C = {corridor}");
        // The pin joins a target on the open layer, or through the via
        // site inside the ring.
        let (slot, _, _) = w.frame.coords(cell);
        assert!(slot <= 1);
        // Pads block vias: with the ring's inside marked via-blocked the
        // pin is enclosed again.
        let mut blocked = w.clone();
        let inside = rect(-600.0, -600.0, 600.0, 600.0);
        rasterize(&w.frame, &inside, |c| blocked.via_blocked.set(c));
        let f = field(&blocked, &vias);
        let outcome = widest_path(
            &blocked,
            &f,
            &vias,
            &requirement(200, 2),
            &blocked.start,
            false,
        );
        let (verdict, _) = super::super::verdict_of(&outcome, DELTA);
        assert_eq!(verdict, super::super::Verdict::Infeasible, "{outcome:?}");
    }

    #[test]
    fn own_copper_touching_the_pin_is_connected() {
        let mut w = window(1000, 1);
        let wire = rect(0.0, -20.0, 800.0, 20.0);
        let mut own = w.own.clone();
        rasterize(&w.frame, &wire, |c| own.set(c));
        w.own = own;
        let f = field(&w, &[]);
        assert_eq!(
            widest_path(&w, &f, &[], &requirement(100, 1), &w.start, false),
            PathOutcome::Connected
        );
    }

    #[test]
    fn neck_requirement_applies_inside_the_radius() {
        let req = Requirement {
            width: vec![1000],
            neck: Some((FloatPoint { x: 0.0, y: 0.0 }, 50.0, vec![200])),
        };
        let w = window(1000, 1);
        let centre = w.frame.index(0, 100, 100);
        assert_eq!(w.frame.centre(100, 100), IntPoint { x: 0, y: 0 });
        assert_eq!(req.at(&w.frame, centre), 200);
        assert_eq!(req.at(&w.frame, w.frame.index(0, 110, 100)), 1000);
    }
}

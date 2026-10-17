// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/qfn_fanout/layout.py
// (analyze_qfn_layout, analyze_pad, single_line_reason) at 0486e799.

//! The part frame and the sides of an edge-row part (X10.1).
//!
//! [`PartFrame`] maps board points to the package frame of a component
//! (undoing its placement, rotation and the back-side mirror) and back, in
//! integer board units. It applies the same [`PlacementTransform`] the
//! board used to place the pins, so `to_board` of a package pin's relative
//! location is the pin's centre, and `to_part` of the centre gives the
//! relative location back: exactly for rotations by multiples of 90°, within
//! one board unit (rounding) for any other angle.
//!
//! [`analyze`] classifies a component's pins by side in that frame, as KRT's
//! `analyze_qfn_layout` and `analyze_pad` do in the footprint's local frame,
//! so the result does not depend on the placement:
//! - the box of the pin centres (`to_part` of each board centre); fewer than
//!   four pins, or a box narrower than a pad across (KRT
//!   `single_line_reason`: a single row), is no edge-row part;
//! - the edge tolerance is a tenth of the box's smaller side; a pin within
//!   it (strictly) of an edge is on that side, the nearest edge first; on a
//!   tie the edge the pad's long axis points out of (not in KRT, which puts
//!   a SOIC's end pads on the top and bottom rows), then KRT's order top,
//!   bottom, left, right, which in the DSN's y-up frame is N, S, W, E;
//! - the pitch is the smallest gap above 0.1 mm between the distinct
//!   along-edge positions of one side's pins (KRT's `p > 0.1` filter).
//!
//! **Which parts qualify** (the X10 decision): four non-empty sides (QFN,
//! QFP) or exactly two opposite ones (SOIC, SOP) with at least
//! [`MIN_SIDE_PINS`] pins on them (KRT's floor for a perimeter package, so
//! SOT-23-5 and similar small parts go to Freerouting), a pitch of at most
//! 0.65 mm (plus KRT's 1 µm `_DIST_TOL`, as in X06's region rule), and no
//! interior pin except exposed pads: a pin off every side must have a pad
//! whose smaller side is at least two pitches, or lie inside such a pad
//! (thermal vias of an exposed pad). A BGA, whose interior balls are as small
//! as its edge balls, does not qualify; KRT's `center` pads are kept out of
//! the surface fan the same way.
//!
//! Pad extents in the part frame are the box of the four mapped corners of
//! the pad's board bounding box: exact at multiples of 90°, larger (so
//! stubs run a little further) at other angles.
//!
//! Determinism (D2): integer arithmetic on the mapped centres; pins sorted
//! by pin number (numeric names by value, then by name, then by item id).

use std::collections::BTreeMap;

use freerouting::board::BasicBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::ComponentId;
use freerouting::geometry::{FloatPoint, IntBox, IntPoint, PlacementTransform};
use freerouting::job::overrides::um_to_board_units;

/// The largest pitch of a qualifying part (µm, the X10 decision).
pub const MAX_PITCH_UM: f64 = 650.0;
/// KRT `_DIST_TOL` (1e-3 mm), added to [`MAX_PITCH_UM`].
pub const PITCH_TOLERANCE_UM: f64 = 1.0;
/// KRT's smallest pitch gap (`p > 0.1` mm in `analyze_qfn_layout`).
pub const MIN_PITCH_GAP_UM: f64 = 100.0;
/// The fewest side pins of a qualifying part (KRT's "a QFN, QFP or
/// two-row IC with at least 8 perimeter pads", `qfn_fanout/README.md`).
pub const MIN_SIDE_PINS: usize = 8;
/// KRT `single_line_reason`'s floor on the pad size (1e-3 mm).
pub const SINGLE_LINE_FLOOR_UM: f64 = 1.0;

/// A side of the package frame, in the X10 order W, S, E, N.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// The lowest x.
    W,
    /// The lowest y.
    S,
    /// The highest x.
    E,
    /// The highest y.
    N,
}

impl Side {
    /// The sides in stub order.
    pub const ALL: [Side; 4] = [Side::W, Side::S, Side::E, Side::N];

    /// The index in [`Side::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// The outward unit direction in the part frame.
    pub fn outward(self) -> (i64, i64) {
        match self {
            Side::W => (-1, 0),
            Side::S => (0, -1),
            Side::E => (1, 0),
            Side::N => (0, 1),
        }
    }

    /// The side's name.
    pub fn name(self) -> &'static str {
        match self {
            Side::W => "W",
            Side::S => "S",
            Side::E => "E",
            Side::N => "N",
        }
    }
}

/// The map between board points and the package frame of one component.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartFrame {
    transform: PlacementTransform,
    location: FloatPoint,
    rotation: f64,
    mirrored: bool,
    rotate_first: bool,
}

impl PartFrame {
    /// The frame of a component at `location`, turned by `rotation`
    /// degrees, mirrored when on the back, with the board's flip style.
    pub fn new(location: FloatPoint, rotation: f64, mirrored: bool, rotate_first: bool) -> Self {
        PartFrame {
            transform: PlacementTransform::new(location, rotation, mirrored, rotate_first),
            location,
            rotation,
            mirrored,
            rotate_first,
        }
    }

    /// The frame of `component` on `board`; `None` if it is not placed.
    pub fn of_component(board: &BasicBoard, component: ComponentId) -> Option<Self> {
        let c = board.components.get(component)?;
        let location = c.exact_location()?;
        Some(PartFrame::new(
            location,
            c.rotation_in_degree(),
            !c.placed_on_front(),
            board.components.flip_style_rotate_first(),
        ))
    }

    /// True if the component is on the back (the frame mirrors).
    pub fn mirrored(&self) -> bool {
        self.mirrored
    }

    /// The board point of the part-frame point `p` (the placement's
    /// transform, rounded as the board rounds pin centres).
    pub fn to_board(&self, p: IntPoint) -> IntPoint {
        self.transform.point(p.to_float()).round()
    }

    /// The part-frame point of the board point `q` (the inverse
    /// transform, rounded).
    pub fn to_part(&self, q: IntPoint) -> IntPoint {
        let mut v = FloatPoint::new(
            f64::from(q.x) - self.location.x,
            f64::from(q.y) - self.location.y,
        );
        if self.mirrored && self.rotate_first {
            v = FloatPoint::new(-v.x, v.y);
        }
        v = PlacementTransform::rotate(v, -self.rotation);
        if self.mirrored && !self.rotate_first {
            v = FloatPoint::new(-v.x, v.y);
        }
        v.round()
    }

    /// The part-frame box around the board box `b`: the box of its four
    /// mapped corners.
    pub fn box_to_part(&self, b: &IntBox) -> IntBox {
        let corners = [
            IntPoint::new(b.ll.x, b.ll.y),
            IntPoint::new(b.ur.x, b.ll.y),
            IntPoint::new(b.ll.x, b.ur.y),
            IntPoint::new(b.ur.x, b.ur.y),
        ]
        .map(|p| self.to_part(p));
        corners[1..]
            .iter()
            .fold(corners[0].surrounding_box(), |acc, p| {
                acc.union_box(&p.surrounding_box())
            })
    }
}

/// One pin of an analysed part, in the part frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartPin {
    /// The pin item.
    pub id: ItemId,
    /// Its package pin name (the pin number).
    pub name: String,
    /// Its centre in the part frame.
    pub centre: IntPoint,
    /// Its pad box in the part frame (all layers of the pin).
    pub pad: IntBox,
    /// Its side, `None` for an interior pin.
    pub side: Option<Side>,
}

/// The layout of a qualifying edge-row part ([`analyze`]).
#[derive(Clone, Debug, PartialEq)]
pub struct PartLayout {
    /// The component.
    pub component: ComponentId,
    /// Its name.
    pub name: String,
    /// The part frame.
    pub frame: PartFrame,
    /// The pins, by pin number.
    pub pins: Vec<PartPin>,
    /// The pin ids per side in [`Side::ALL`] order, each by pin number.
    pub sides: [Vec<ItemId>; 4],
    /// The pitch, board units.
    pub pitch: i64,
    /// The box of every pad in the part frame (the pin field).
    pub field: IntBox,
    /// The box of the package outline in the part frame, if it has one.
    pub outline: Option<IntBox>,
}

impl PartLayout {
    /// The pin `id`, if it is one of the part's.
    pub fn pin(&self, id: ItemId) -> Option<&PartPin> {
        self.pins.iter().find(|p| p.id == id)
    }

    /// The box the stubs escape: the pin field united with the outline.
    pub fn escape_box(&self) -> IntBox {
        match &self.outline {
            Some(o) => self.field.union_box(o),
            None => self.field,
        }
    }
}

/// The sides of `component` (X10.1's interface): the pin ids per side in
/// W, S, E, N order, each by pin number; `None` unless the part qualifies
/// (module docs).
pub fn sides(board: &BasicBoard, component: ComponentId) -> Option<[Vec<ItemId>; 4]> {
    analyze(board, component).map(|layout| layout.sides)
}

/// The sort key of a pin number: numeric names by value first.
fn pin_number_key(name: &str, id: ItemId) -> (bool, u64, String, ItemId) {
    match name.parse::<u64>() {
        Ok(n) => (false, n, name.to_string(), id),
        Err(_) => (true, 0, name.to_string(), id),
    }
}

/// The pins of `component` in the part frame, by pin number, without
/// sides.
fn part_pins(board: &BasicBoard, component: ComponentId, frame: &PartFrame) -> Vec<PartPin> {
    let package = board
        .components
        .get(component)
        .and_then(|c| board.library.packages.get(c.package()));
    let mut pins: Vec<PartPin> = board
        .items()
        .filter(|i| i.is_pin() && i.component_id() == component.0)
        .filter_map(|item: &Item| {
            let pin = item.as_pin()?;
            let drill = item.drill()?;
            let name = package
                .and_then(|p| p.get_pin(pin.pin_index))
                .map_or_else(|| pin.pin_index.to_string(), |p| p.name.clone());
            Some(PartPin {
                id: item.id(),
                name,
                centre: frame.to_part(drill.center),
                pad: frame.box_to_part(&drill.bounding_box()),
                side: None,
            })
        })
        .collect();
    pins.sort_by_cached_key(|p| pin_number_key(&p.name, p.id));
    pins
}

/// The side of a pin at `c` with pad box `pad` in the centre box `b`
/// whose smaller side is `narrow` (KRT `analyze_pad`): the nearest edge if
/// it is closer than a tenth of `narrow` (`10·d < narrow`). Among equally
/// near edges, the one the pad's long axis points out of (a SOIC's corner
/// pads lie on both a row and a column end), then KRT's order top, bottom,
/// left, right, which is N, S, W, E here.
fn side_of(c: IntPoint, pad: &IntBox, b: &IntBox, narrow: i64) -> Option<Side> {
    let (x, y) = (i64::from(c.x), i64::from(c.y));
    let candidates = [
        (Side::N, i64::from(b.ur.y) - y),
        (Side::S, y - i64::from(b.ll.y)),
        (Side::W, x - i64::from(b.ll.x)),
        (Side::E, i64::from(b.ur.x) - x),
    ];
    let min = candidates.iter().map(|(_, d)| *d).min()?;
    if min.saturating_mul(10) >= narrow {
        return None;
    }
    let nearest: Vec<Side> = candidates
        .iter()
        .filter(|(_, d)| *d == min)
        .map(|(s, _)| *s)
        .collect();
    let along_x = |s: &Side| matches!(s, Side::W | Side::E);
    let long_axis = match pad.width().cmp(&pad.height()) {
        std::cmp::Ordering::Greater => nearest.iter().find(|s| along_x(s)),
        std::cmp::Ordering::Less => nearest.iter().find(|s| !along_x(s)),
        std::cmp::Ordering::Equal => None,
    };
    long_axis.or(nearest.first()).copied()
}

/// The smallest gap above `min_gap` between the distinct along-edge
/// positions of each side's pins; `None` if no side has one.
fn pitch_of(pins: &[PartPin], min_gap: i64) -> Option<i64> {
    let mut along: BTreeMap<Side, Vec<i64>> = BTreeMap::new();
    for p in pins {
        if let Some(side) = p.side {
            let v = match side {
                Side::W | Side::E => p.centre.y,
                Side::S | Side::N => p.centre.x,
            };
            along.entry(side).or_default().push(i64::from(v));
        }
    }
    along
        .values_mut()
        .filter_map(|v| {
            v.sort_unstable();
            v.dedup();
            v.windows(2)
                .map(|w| w[1] - w[0])
                .filter(|g| *g > min_gap)
                .min()
        })
        .min()
}

/// The smaller side of a box, as `i64`.
fn smaller_side(b: &IntBox) -> i64 {
    i64::from(b.width()).min(i64::from(b.height()))
}

/// True if every interior pin is an exposed pad (smaller side at least two
/// pitches) or lies inside one.
fn interior_pins_are_exposed(pins: &[PartPin], pitch: i64) -> bool {
    let exposed: Vec<&IntBox> = pins
        .iter()
        .filter(|p| p.side.is_none() && smaller_side(&p.pad) >= 2 * pitch)
        .map(|p| &p.pad)
        .collect();
    pins.iter().filter(|p| p.side.is_none()).all(|p| {
        smaller_side(&p.pad) >= 2 * pitch || exposed.iter().any(|e| e.contains_inside_int(p.centre))
    })
}

/// The layout of `component` if it is a qualifying edge-row part (module
/// docs), else `None`.
pub fn analyze(board: &BasicBoard, component: ComponentId) -> Option<PartLayout> {
    let frame = PartFrame::of_component(board, component)?;
    let mut pins = part_pins(board, component, &frame);
    if pins.len() < 4 {
        return None;
    }
    let centres = pins[1..]
        .iter()
        .fold(pins[0].centre.surrounding_box(), |acc, p| {
            acc.union_box(&p.centre.surrounding_box())
        });
    let narrow = smaller_side(&centres);
    let pad_min = pins.iter().map(|p| smaller_side(&p.pad)).min()?;
    let floor = i64::from(um_to_board_units(board, SINGLE_LINE_FLOOR_UM));
    if narrow < pad_min.max(floor) {
        return None;
    }
    for p in &mut pins {
        p.side = side_of(p.centre, &p.pad, &centres, narrow);
    }
    let mut sides: [Vec<ItemId>; 4] = Default::default();
    for p in &pins {
        if let Some(side) = p.side {
            sides[side.index()].push(p.id);
        }
    }
    let present = [0, 1, 2, 3].map(|i| !sides[i].is_empty());
    let four = present.iter().all(|b| *b);
    let two = present == [true, false, true, false] || present == [false, true, false, true];
    if (!four && !two) || sides.iter().map(Vec::len).sum::<usize>() < MIN_SIDE_PINS {
        return None;
    }
    let pitch = pitch_of(&pins, i64::from(um_to_board_units(board, MIN_PITCH_GAP_UM)))?;
    let max_pitch = i64::from(um_to_board_units(board, MAX_PITCH_UM + PITCH_TOLERANCE_UM));
    if pitch > max_pitch || !interior_pins_are_exposed(&pins, pitch) {
        return None;
    }
    let field = pins[1..]
        .iter()
        .fold(pins[0].pad, |acc, p| acc.union_box(&p.pad));
    let c = board.components.get(component)?;
    let outline = board
        .library
        .packages
        .get(c.package())
        .and_then(|p| p.outline.as_ref())
        .and_then(|shapes| {
            shapes
                .iter()
                .flatten()
                .map(|s| s.bounding_box())
                .reduce(|a, b| a.union_box(&b))
        });
    Some(PartLayout {
        component,
        name: c.name.clone(),
        frame,
        pins,
        sides,
        pitch,
        field,
        outline,
    })
}

//! Dog-bone vias of a ball array (X12.3).
//!
//! Written from the X12 checklist's design. KiCadRoutingTools'
//! `py_router/bga_fanout` (at `0486e799`) gave the idea of a via in the
//! diagonal gap between four balls (its `dogbone` method, #128) and of
//! sizing the via to the pitch (#158); no KRT code is ported here. KRT
//! falls back to a via in the pad where no gap via fits; D8 forbids that,
//! so such a ball is reported instead ([`DogBone::NoViaFits`]).
//!
//! **The site.** Every ball's via sits at the centre of the square between
//! it and three neighbours, half a pitch outward through its side and half
//! a pitch to the side `σ` across: toward the top for a W or E ball in the
//! upper half of the rows (the middle row counts as upper), else the
//! bottom; toward the left for an N or S ball in the left half of the
//! columns (the middle column counts as left), else the right. With the
//! pinwheel sides of [`super::array::BallArray::side`] every via of ring
//! `k` lies on the square half a pitch outside the ring, one per position,
//! so no two vias of an array share a site, and the outer ring's vias lie
//! outside the pin field.
//!
//! **The padstack** (the X12 decision): the smallest via of the net's via
//! rule (its class's rule, else the board's first, as `NetRules::of_net`)
//! whose copper plus clearance fits the square: on the pad layer, the
//! distance from the site to each populated corner ball's pad is at least
//! the via's radius plus their clearance, plus one board unit for
//! rounding. Vias are compared by their largest radius, ties in rule
//! order; a via must span the pad layer and a layer the ball escapes to
//! (a signal layer, or the plane of a plane ball).
//!
//! Determinism (D2): the order of the via rule; no hashing.

use freerouting::board::BasicBoard;
use freerouting::board::items::Item;
use freerouting::geometry::{ConvexShape, FloatPoint, IntPoint, PolylineShapeOps};
use freerouting::library::PadstackId;
use freerouting::rules::ViaInfoId;

use super::array::BallArray;
use crate::fanout::frame::Side;

/// One via of a net's via rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViaOption {
    /// The via info.
    pub info: ViaInfoId,
    /// Its padstack.
    pub padstack: PadstackId,
    /// Its clearance class.
    pub clearance_class: i32,
    /// Whether it may attach to an SMD pad.
    pub attach_smd_allowed: bool,
    /// The first board layer of the padstack.
    pub from_layer: i32,
    /// The last board layer of the padstack.
    pub to_layer: i32,
    /// The largest distance from the centre to the copper per board layer,
    /// rounded up (0 off the span).
    pub radius: Vec<i32>,
}

impl ViaOption {
    /// True if the padstack spans `layer`.
    pub fn spans(&self, layer: i32) -> bool {
        self.from_layer <= layer && layer <= self.to_layer
    }

    /// The radius on `layer` (0 off the span).
    pub fn radius_on(&self, layer: i32) -> i32 {
        usize::try_from(layer)
            .ok()
            .and_then(|l| self.radius.get(l).copied())
            .unwrap_or(0)
    }

    /// The largest radius over its layers.
    pub fn max_radius(&self) -> i32 {
        self.radius.iter().copied().max().unwrap_or(0)
    }
}

/// The vias of net `net`'s via rule, smallest first (module docs).
pub fn via_options(board: &BasicBoard, net: i32) -> Vec<ViaOption> {
    let rules = &board.rules;
    let class = rules
        .nets
        .get(net)
        .map(|n| rules.net_classes.get(n.net_class()));
    let Some(rule) = class
        .and_then(|c| c.via_rule())
        .or_else(|| rules.via_rules.nth(0))
    else {
        return Vec::new();
    };
    let mut out: Vec<ViaOption> = rules
        .via_rules
        .get(rule)
        .vias()
        .iter()
        .filter_map(|&id| via_option(board, id))
        .collect();
    out.sort_by_key(ViaOption::max_radius);
    out
}

fn via_option(board: &BasicBoard, id: ViaInfoId) -> Option<ViaOption> {
    let info = board.rules.via_infos.get(id);
    let padstack = board.library.padstacks.get(info.padstack)?;
    let (from_layer, to_layer) = (padstack.from_layer(), padstack.to_layer());
    let radius = (0..board.layer_count() as i32)
        .map(|l| {
            if l < from_layer || l > to_layer {
                return 0;
            }
            padstack.get_shape(l).map_or(0, bounding_radius)
        })
        .collect();
    Some(ViaOption {
        info: id,
        padstack: info.padstack,
        clearance_class: info.clearance_class,
        attach_smd_allowed: info.attach_smd_allowed,
        from_layer,
        to_layer,
        radius,
    })
}

/// The largest distance from the origin to `shape`, rounded up (as X05's
/// raster measures a via).
fn bounding_radius(shape: &ConvexShape) -> i32 {
    let origin = FloatPoint::new(0.0, 0.0);
    let r = match shape {
        ConvexShape::Circle(c) => f64::from(c.radius) + origin.distance(c.center.to_float()),
        ConvexShape::Tile(t) => t
            .corner_approx_arr()
            .iter()
            .map(|p| origin.distance(*p))
            .fold(0.0, f64::max),
    };
    r.ceil() as i32
}

/// The lateral side `σ` (±1) of the via of `(row, col)` (module docs).
pub fn sigma(array: &BallArray, row: u16, col: u16) -> i64 {
    match array.side(row, col) {
        Side::W | Side::E => {
            if 2 * u32::from(row) < u32::from(array.rows) {
                1
            } else {
                -1
            }
        }
        Side::N | Side::S => {
            if 2 * u32::from(col) < u32::from(array.cols) {
                -1
            } else {
                1
            }
        }
    }
}

/// The lateral unit vector of `side` (along the side's edge).
pub fn lateral_unit(side: Side) -> (i64, i64) {
    match side {
        Side::W | Side::E => (0, 1),
        Side::S | Side::N => (1, 0),
    }
}

/// The part-frame site of the via of `(row, col)` (module docs).
pub fn site(array: &BallArray, row: u16, col: u16) -> IntPoint {
    let side = array.side(row, col);
    let c = array.centre(i64::from(row), i64::from(col));
    let half = array.pitch / 2;
    let (ux, uy) = side.outward();
    let (lx, ly) = lateral_unit(side);
    let s = sigma(array, row, col);
    let x = i64::from(c.x) + (ux + s * lx) * half;
    let y = i64::from(c.y) + (uy + s * ly) * half;
    IntPoint::new(x as i32, y as i32)
}

/// The lattice positions of the square around the site of `(row, col)`:
/// the ball and its three neighbours there (some may be off the array).
pub fn square(array: &BallArray, row: u16, col: u16) -> [(i64, i64); 4] {
    let c = array.centre(i64::from(row), i64::from(col));
    let s = site(array, row, col);
    let dc = i64::from((s.x - c.x).signum());
    let dr = -i64::from((s.y - c.y).signum());
    let (r, k) = (i64::from(row), i64::from(col));
    [(r, k), (r + dr, k), (r, k + dc), (r + dr, k + dc)]
}

/// The pins at the populated positions of `square`.
fn square_pins<'a>(board: &'a BasicBoard, array: &BallArray, row: u16, col: u16) -> Vec<&'a Item> {
    square(array, row, col)
        .iter()
        .filter_map(|&(r, c)| {
            let (r, c) = (u16::try_from(r).ok()?, u16::try_from(c).ok()?);
            array.ball_of.get(&(r, c))
        })
        .filter_map(|id| board.item(*id))
        .collect()
}

/// True if `via` at the site of `(row, col)` keeps its clearance from the
/// pads of the square on the pad layer (module docs).
pub fn fits(board: &BasicBoard, array: &BallArray, row: u16, col: u16, via: &ViaOption) -> bool {
    let layer = array.pad_layer;
    let at = array.frame.to_board(site(array, row, col)).to_float();
    let r = f64::from(via.radius_on(layer));
    square_pins(board, array, row, col).iter().all(|pin| {
        let Some(shape) = pin.drill().and_then(|d| d.shape_on_layer(layer)) else {
            return true;
        };
        let c = f64::from(board.clearance_value(via.clearance_class, pin.clearance_class(), layer));
        shape.distance(at) >= r + c + 1.0
    })
}

/// The dog-bone of one ball.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DogBone {
    /// The via and its board site.
    Via {
        /// The chosen via.
        via: ViaOption,
        /// Its centre on the board.
        at: IntPoint,
    },
    /// No via of the rule fits the square.
    NoViaFits,
}

/// The dog-bone of `(row, col)` for net `net`: the smallest via of
/// `options` that `usable` accepts and that fits (module docs).
pub fn choose(
    board: &BasicBoard,
    array: &BallArray,
    row: u16,
    col: u16,
    options: &[ViaOption],
    usable: impl Fn(&ViaOption) -> bool,
) -> DogBone {
    options
        .iter()
        .filter(|v| v.spans(array.pad_layer) && usable(v))
        .find(|v| fits(board, array, row, col, v))
        .map_or(DogBone::NoViaFits, |via| DogBone::Via {
            via: via.clone(),
            at: array.frame.to_board(site(array, row, col)),
        })
}

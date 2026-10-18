//! The net rules the escape planners read: a net's trace class, half-widths,
//! routing layers and via, and the check that a via's copper leaves every
//! pad alone.
//!
//! These were the rules of the `krt-grid` connection router's obstacle raster
//! (X04, X05); that router was judged by N02.4 and removed (it won on no
//! board class), and the escape stubs of [`super::qfn`] and
//! [`super::bga`] kept the part they use.

use freerouting::board::BasicBoard;
use freerouting::board::items::Item;
use freerouting::geometry::{ConvexShape, FloatPoint, PolylineShapeOps, TileShape};
use freerouting::rules::ViaInfoId;

/// What an escape needs from the net's rules.
#[derive(Clone, Debug, PartialEq)]
pub struct NetRules {
    /// The net's trace clearance class.
    pub trace_class: i32,
    /// The net's trace half-width per board layer.
    pub half_width: Vec<i32>,
    /// Whether the net class routes on each board layer.
    pub active: Vec<bool>,
    /// The via the path would use, if the net has a via rule with a via.
    pub via: Option<ViaChoice>,
}

/// The via of a connection: [`NetRules::via`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViaChoice {
    /// The via info in the board's rules.
    pub info: ViaInfoId,
    /// Its clearance class.
    pub clearance_class: i32,
    /// The first board layer of its padstack.
    pub from_layer: i32,
    /// The last board layer of its padstack.
    pub to_layer: i32,
    /// The largest distance from the via's centre to its pad, per board
    /// layer, rounded up (0 off the padstack's span).
    pub radius: Vec<i32>,
}

impl NetRules {
    /// The rules of net `net` (Java `AutorouteControl`'s fallback for a
    /// missing net: clearance class 1 and the board's first via rule).
    pub fn of_net(board: &BasicBoard, net: i32) -> NetRules {
        let rules = &board.rules;
        let layers = board.layer_count();
        let class = rules
            .nets
            .get(net)
            .map(|n| rules.net_classes.get(n.net_class()));
        let trace_class = class.map_or(1, |c| c.trace_clearance_class());
        let width_net = if net > 0 { net } else { 1 };
        let half_width = (0..layers as i32)
            .map(|l| rules.trace_half_width(width_net, l))
            .collect();
        let active = (0..layers as i32)
            .map(|l| class.is_none_or(|c| c.is_active_routing_layer(l)))
            .collect();
        let via_rule = class
            .and_then(|c| c.via_rule())
            .or_else(|| rules.via_rules.nth(0));
        let via = via_rule.and_then(|id| choose_via(board, rules.via_rules.get(id).vias()));
        NetRules {
            trace_class,
            half_width,
            active,
            via,
        }
    }

    /// The half-width on `layer` (0 off the board's layers).
    pub fn half_width_on(&self, layer: i32) -> i32 {
        usize::try_from(layer)
            .ok()
            .and_then(|l| self.half_width.get(l).copied())
            .unwrap_or(0)
    }
}

/// The first via of `vias` whose padstack spans every board layer, else
/// the first via with a padstack.
fn choose_via(board: &BasicBoard, vias: &[ViaInfoId]) -> Option<ViaChoice> {
    let last = board.layer_count() as i32 - 1;
    let choices: Vec<ViaChoice> = vias
        .iter()
        .filter_map(|&id| via_choice(board, id))
        .collect();
    let through = choices
        .iter()
        .position(|v| v.from_layer <= 0 && v.to_layer >= last);
    let index = through.unwrap_or(0);
    choices.into_iter().nth(index)
}

fn via_choice(board: &BasicBoard, id: ViaInfoId) -> Option<ViaChoice> {
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
    Some(ViaChoice {
        info: id,
        clearance_class: info.clearance_class,
        from_layer,
        to_layer,
        radius,
    })
}

/// The largest distance from the origin to `shape`, rounded up.
fn bounding_radius(shape: &ConvexShape) -> i32 {
    let r = match shape {
        ConvexShape::Circle(c) => {
            f64::from(c.radius) + FloatPoint::new(0.0, 0.0).distance(c.center.to_float())
        }
        ConvexShape::Tile(t) => t
            .corner_approx_arr()
            .iter()
            .map(|p| FloatPoint::new(0.0, 0.0).distance(*p))
            .fold(0.0, f64::max),
    };
    r.ceil() as i32
}

/// True if the copper of via `via` overlaps a pin's pad on any layer.
pub fn via_in_pad(board: &BasicBoard, via: &Item) -> bool {
    let Some(drill) = via.drill() else {
        return false;
    };
    (drill.first_layer..=drill.last_layer).any(|layer| {
        let Some(shape) = drill.shape_on_layer(layer) else {
            return false;
        };
        let probe = TileShape::IntBox(shape.bounding_box());
        board
            .overlapping_tree_entries(&probe, layer)
            .iter()
            .filter_map(|e| board.item(e.item))
            .filter(|item| item.is_pin())
            .filter_map(Item::drill)
            .any(|pin| {
                pin.shape_on_layer(layer)
                    .is_some_and(|pad| pad.intersects(shape))
            })
    })
}

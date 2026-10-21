//! Q22.16: a footprint's solder-mask aperture as a keepout that keeps other
//! nets out and lets the pads in it be reached. Not Java.
//!
//! KiCad's DRC (`solder_mask_bridge`) flags copper of two nets in one mask
//! aperture. The KiCad plugin and the `.kicad_pcb` reader write a footprint's
//! own F.Mask / B.Mask shape as a `keepout` named [`PAD_KEEPOUT_NAME`] on
//! its side's copper. [`insert_pad_keepouts`] inserts it once the components
//! are on the board, after the pins it overlaps on that layer (their shapes
//! on the layer meet the keepout):
//!
//! - **pins of one net**: a keepout that carries that net, so the net routes
//!   through it as if it were not there and a pad wholly inside its own
//!   aperture stays reachable, while every other net keeps out. It keeps the
//!   name, and is an obstacle for traces and vias only
//!   (`ObstacleArea::is_pad_keepout`): it is no copper, so a pin of another
//!   net beside the aperture or the next pad's aperture is no violation;
//! - **pins of two or more nets**: the keepout without the pins (each pin's
//!   bounding tile on the layer, grown by the keepout's clearance to the pin,
//!   cut out of the keepout's convex pieces), a keepout for every net: no net
//!   can cross the aperture, and a pin that reaches out of it is entered from
//!   the part outside (an edge connector's fingers). The router ends a trace
//!   at the pin's centre (Java `Pin.getTraceConnectionShape`), so a trace runs
//!   along its own pin into the aperture, where KiCad still sees a bridge;
//! - **no pin with a net**: a keepout for every net, as Java reads it.
//!
//! The name is otherwise ignored, as Java ignores it; a DSN without it reads
//! as Java reads it. The DSN writer writes a keepout that carries a net back
//! under the name, so it reads back the same.

use std::collections::BTreeSet;

use freerouting_geometry::{Area, PolylineShapeOps, Shape, TileShape};

use super::ReadContext;
use super::shape::{DsnArea, transform_area_to_board};
use super::structure::{insert_keepout, insert_keepout_on_layer};
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::rules::{BoardRules, ItemClass};

/// The name of a footprint's mask-aperture keepout (module docs).
pub use crate::board::items::PAD_KEEPOUT_NAME;

/// True if `area` is a footprint's mask-aperture keepout.
pub(crate) fn is_pad_keepout(area: &DsnArea) -> bool {
    area.name.as_deref() == Some(PAD_KEEPOUT_NAME)
}

/// Inserts the keepouts [`super::structure::read_structure`] put aside (module
/// docs), in file order. `false` if one cannot be inserted.
pub(crate) fn insert_pad_keepouts(ctx: &mut ReadContext) -> bool {
    let areas = std::mem::take(&mut ctx.pad_keepouts);
    if ctx.board.is_none() {
        return true;
    }
    areas.iter().all(|area| insert_pad_keepout(ctx, area))
}

fn insert_pad_keepout(ctx: &mut ReadContext, area: &DsnArea) -> bool {
    let layer = area.layer().clone();
    let transform = ctx.coordinate_transform;
    let pieces = transform_area_to_board(&area.shapes, &transform)
        .filter(|a| a.dimension() >= 2 && layer.no >= 0 && !layer.is_signal_pseudo_layer())
        .and_then(|a| a.split_to_convex());
    let Some(pieces) = pieces else {
        // Java's own handling (and its log lines) of the shape and layer.
        return insert_keepout(ctx, area, 0, false, &[]);
    };
    let class = area_class(ctx.board(), area);
    let board = ctx.board();
    let pins = pins_in(board, &pieces, layer.no);
    let nets: BTreeSet<i32> = pins
        .iter()
        .filter_map(|id| board.item(*id))
        .flat_map(|item| item.net_numbers().iter().copied())
        .filter(|n| *n > 0)
        .collect();
    match nets.len() {
        0 => insert_keepout(ctx, area, 0, false, &[]),
        1 => {
            let net: Vec<i32> = nets.into_iter().collect();
            insert_keepout(ctx, area, 0, false, &net)
        }
        count => {
            let rest = without_pins(board, pieces, &pins, layer.no, class);
            let shapes = rest.len();
            for piece in rest {
                let piece = Area::Shape(Shape::from(piece));
                insert_keepout_on_layer(ctx.board(), &piece, layer.no, area, 0, &[]);
            }
            ctx.log(format!(
                "keepout '{PAD_KEEPOUT_NAME}' on {}: the pads of {count} nets cut out \
                 ({shapes} pieces)",
                layer.name
            ));
            true
        }
    }
}

/// The pins whose shape on `layer` meets one of `pieces`, in id order.
fn pins_in(board: &RoutingBoard, pieces: &[TileShape], layer: i32) -> BTreeSet<ItemId> {
    let mut pins = BTreeSet::new();
    for piece in pieces {
        let shape = Shape::from(piece.clone());
        for id in board.overlapping_objects(piece, layer) {
            let Some(item) = board.item(id).filter(|i| i.is_pin()) else {
                continue;
            };
            if item
                .drill()
                .and_then(|d| d.shape_on_layer(layer))
                .is_some_and(|s| s.intersects(&shape))
            {
                pins.insert(id);
            }
        }
    }
    pins
}

/// `pieces` with each pin of `pins` cut out: its bounding tile on `layer`
/// grown by the clearance between the keepout's class `class` and the pin's. A piece
/// a cut cannot divide stays whole; pieces below two dimensions go.
fn without_pins(
    board: &RoutingBoard,
    mut pieces: Vec<TileShape>,
    pins: &BTreeSet<ItemId>,
    layer: i32,
    class: i32,
) -> Vec<TileShape> {
    for id in pins {
        let Some(item) = board.item(*id) else {
            continue;
        };
        let Some(shape) = item.drill().and_then(|d| d.shape_on_layer(layer)) else {
            continue;
        };
        let clearance =
            board
                .rules
                .clearance_matrix
                .get_value(class, item.clearance_class(), layer, false);
        let cut = shape.bounding_tile().enlarge(f64::from(clearance));
        pieces = pieces
            .into_iter()
            .flat_map(|p| {
                if p.intersects_tile(&cut) {
                    p.cutout(&cut).unwrap_or_else(|| vec![p])
                } else {
                    vec![p]
                }
            })
            .filter(|p| p.dimension() >= 2)
            .collect();
    }
    pieces
}

/// The clearance class `insert_keepout_on_layer` gives the keepout.
fn area_class(board: &mut RoutingBoard, area: &DsnArea) -> i32 {
    match &area.clearance_class {
        None => {
            let default = board.rules.default_net_class();
            board
                .rules
                .net_classes
                .get(default)
                .default_item_clearance_classes
                .get(ItemClass::Area)
        }
        Some(name) => {
            let no = board.rules.clearance_matrix.get_no(name);
            if no < 0 {
                BoardRules::clearance_class_none()
            } else {
                no
            }
        }
    }
}

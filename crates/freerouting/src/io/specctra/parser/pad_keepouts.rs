//! Q22.16, Q22.18: a solder-mask aperture as a keepout fitted to the pins
//! under it. Not Java.
//!
//! KiCad's DRC (`solder_mask_bridge`) flags copper of two nets in one mask
//! aperture. The KiCad plugin and the `.kicad_pcb` reader write each mask
//! shape KiCad's export leaves out as a `keepout` on its side's copper, with
//! `(clearance_class null)` (Q22.18: the aperture is already grown by the
//! board's solder-mask-to-copper clearance, so nothing more is kept): a
//! board-level F.Mask / B.Mask shape named `solder_mask` (Q18.7), a
//! footprint's own named `solder_mask_pads`, or, when the footprint's shape
//! lies over pads of two or more nets, `solder_mask_shared` (they cut those
//! pads back to their part outside the aperture, with the pin's anchor at its
//! centre; `crate::board::items::MASK_KEEPOUT_NAMES`).
//! [`insert_pad_keepouts`] inserts them once the components are on the
//! board, after the pins they overlap on that layer (their shapes on the
//! layer meet the keepout). Q22.18: a mask keepout with no pin under it
//! (the 1 µm strips that grow an aperture) takes the pins of the mask
//! keepouts it meets on that layer, and their `solder_mask_shared` name:
//!
//! - **pins of one net**: a keepout that carries that net, so the net routes
//!   through it as if it were not there and a pad wholly inside its own
//!   aperture stays reachable, while every other net keeps out;
//! - **pins of two or more nets**, or any `solder_mask_shared` keepout: a
//!   keepout that carries no net, so every net keeps out, the pins' own too
//!   (KiCad flags a track of a pad's own net inside an aperture shared with
//!   another net). A pin entered only from inside it stays unrouted;
//! - **no pin with a net**: a keepout for every net, as Java reads it.
//!
//! The first two keep their name and are obstacles for traces and vias only
//! (`ObstacleArea::is_pad_keepout`): they are no copper, so a pin under one,
//! or a pin of another net beside it, is no violation. The name is
//! otherwise ignored, as Java ignores it; a DSN without these names reads as
//! Java reads it. The DSN writer writes such a keepout back under its name,
//! so it reads back the same.

use std::collections::BTreeSet;

use freerouting_geometry::{IntBox, PolylineShapeOps, Shape, TileShape};

use super::ReadContext;
use super::shape::{DsnArea, transform_area_to_board};
use super::structure::insert_keepout;
use crate::board::RoutingBoard;
use crate::board::items::ItemId;

use crate::board::items::{MASK_KEEPOUT_NAMES, SHARED_PAD_KEEPOUT_NAME};

/// True if `area` is a mask-aperture keepout (module docs).
pub(crate) fn is_pad_keepout(area: &DsnArea) -> bool {
    area.name
        .as_deref()
        .is_some_and(|n| MASK_KEEPOUT_NAMES.contains(&n))
}

/// Inserts the keepouts [`super::structure::read_structure`] put aside (module
/// docs), in file order. `false` if one cannot be inserted.
pub(crate) fn insert_pad_keepouts(ctx: &mut ReadContext) -> bool {
    let areas = std::mem::take(&mut ctx.pad_keepouts);
    if ctx.board.is_none() {
        return true;
    }
    let fits: Vec<Option<Fit>> = areas.iter().map(|area| fit(ctx, area)).collect();
    let groups = aperture_groups(&fits);
    areas
        .iter()
        .zip(&fits)
        .enumerate()
        .all(|(i, (area, f))| match f {
            // Java's own handling (and its log lines) of the shape and layer.
            None => insert_keepout(ctx, area, 0, false, None),
            Some(f) => insert_fitted(ctx, area, &f.layer_name, &groups[i]),
        })
}

/// A mask keepout's convex pieces on its layer, and what lies under them.
struct Fit {
    layer: i32,
    layer_name: String,
    pieces: Vec<TileShape>,
    boxes: Vec<IntBox>,
    shared: bool,
    nets: BTreeSet<i32>,
}

/// What a keepout is fitted to: the nets of the pins under it, and whether
/// it is `solder_mask_shared`.
#[derive(Clone, Default)]
struct Group {
    shared: bool,
    nets: BTreeSet<i32>,
}

fn fit(ctx: &ReadContext, area: &DsnArea) -> Option<Fit> {
    let layer = area.layer();
    let transform = ctx.coordinate_transform;
    let pieces = transform_area_to_board(&area.shapes, &transform)
        .filter(|a| a.dimension() >= 2 && layer.no >= 0 && !layer.is_signal_pseudo_layer())
        .and_then(|a| a.split_to_convex())?;
    let board = ctx.board.as_ref()?;
    let nets = pins_in(board, &pieces, layer.no)
        .iter()
        .filter_map(|id| board.item(*id))
        .flat_map(|item| item.net_numbers().iter().copied())
        .filter(|n| *n > 0)
        .collect();
    Some(Fit {
        layer: layer.no,
        layer_name: layer.name.clone(),
        boxes: pieces.iter().map(|p| p.bounding_box()).collect(),
        pieces,
        shared: area.name.as_deref() == Some(SHARED_PAD_KEEPOUT_NAME),
        nets,
    })
}

/// Q22.18: the 1 µm strips that grow an aperture
/// (`freerouting_kicad::edge_keepouts`) have no pin under them, so a mask
/// keepout with no pin under it, and not shared, is fitted to the mask
/// keepouts it meets on its layer that have a pin under them or are shared
/// (its aperture's body; a strip between two apertures of different nets
/// keeps both out). A keepout with pins keeps its own fit, so apertures that
/// touch keep theirs. `groups[i]` is keepout `i`'s.
fn aperture_groups(fits: &[Option<Fit>]) -> Vec<Group> {
    let own = |f: &Fit| Group {
        shared: f.shared,
        nets: f.nets.clone(),
    };
    let fitted = |f: &Fit| f.shared || !f.nets.is_empty();
    fits.iter()
        .map(|f| match f {
            None => Group::default(),
            Some(f) if fitted(f) => own(f),
            Some(f) => {
                let mut g = own(f);
                for o in fits.iter().flatten().filter(|o| fitted(o) && meet(f, o)) {
                    g.shared |= o.shared;
                    g.nets.extend(o.nets.iter().copied());
                }
                g
            }
        })
        .collect()
}

/// True if two mask keepouts on one layer share a point.
fn meet(a: &Fit, b: &Fit) -> bool {
    a.layer == b.layer
        && a.pieces.iter().zip(&a.boxes).any(|(pa, ba)| {
            b.pieces
                .iter()
                .zip(&b.boxes)
                .any(|(pb, bb)| ba.intersects_box(bb) && pa.intersects(&Shape::from(pb.clone())))
        })
}

fn insert_fitted(ctx: &mut ReadContext, area: &DsnArea, layer: &str, group: &Group) -> bool {
    match group.nets.len() {
        _ if group.shared => insert_keepout(ctx, area, 0, false, Some(&[])),
        0 => insert_keepout(ctx, area, 0, false, None),
        1 => {
            let net: Vec<i32> = group.nets.iter().copied().collect();
            insert_keepout(ctx, area, 0, false, Some(&net))
        }
        count => {
            ctx.log(format!(
                "keepout '{}' on {layer}: over the pads of {count} nets, it keeps every net out",
                area.name.as_deref().unwrap_or_default(),
            ));
            insert_keepout(ctx, area, 0, false, Some(&[]))
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

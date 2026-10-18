//! Q16.4: the contacts of the stub cleanup, counted as KiCad counts them.
//!
//! Java's normal contacts (`BasicBoard::normal_contacts`) join a via and a
//! trace only where an end corner of the trace is the via's centre, and two
//! traces only at a common end corner. KiCad joins any two items of a net
//! whose copper meets on a common layer: a via sitting on a trace's body
//! (the plugin's fixed pour strips cross vias mid-segment), a trace ending
//! on another trace's body, a via off the centre of a pad. Q16.3 measured
//! the cleanup on Java's contacts and it removed such a via, which KiCad
//! counted as a connection (canary-pwr, KiCad unconnected 37 -> 38).
//!
//! So the cleanup reads its contacts here, independently of the board's
//! contact rule (`router.pad_contacts`):
//! - **the copper of an item** ([`pieces`]) is, per layer, a trace's
//!   segment shapes (`Polyline::offset_shape` at its half width), a via's
//!   or pin's pad shape on each of its layers, and a conduction area's
//!   tile shapes (a plane is its DSN outline: the cleanup cannot know the
//!   zone fill KiCad pours inside it);
//! - **two items touch** ([`touches`]) when they share a net and a piece of
//!   one intersects a piece of the other on the same layer, by the board's
//!   exact shape intersection (`Shape::intersects`, borders included, in
//!   integer and rational arithmetic, no tolerance);
//! - **a trace end has a contact** ([`end_has_contact`]) when the cap of the
//!   end, the disc of half the trace's width around it, intersects a piece
//!   of another item of its net on its layer (N04's end-cap test; an end at
//!   a rational point tests the point alone). The cap is a little more
//!   generous than KiCad's point test, so the cleanup keeps a trace KiCad
//!   might still call dangling, never the reverse;
//! - **a via's contact layers** ([`via_contact_layers`]) are the layers on
//!   which its pad touches another item of its net. KiCad's `via_dangling`
//!   is a via connected on fewer than two layers.
//!
//! Java's contacts are a subset of these (an end corner at a via's centre,
//! a common end corner, an end inside a plane all make copper meet), so a
//! stub here is a stub for Java's connectivity as well, and the cleanup's
//! guard (its contacts connected through one another, Q16.5) keeps every
//! connection that is complete for the router or for KiCad.

use std::collections::BTreeSet;

use freerouting_geometry::{Circle, Point, Shape, TileShape};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};

/// One piece of an item's copper: its layer and its exact shape.
type Piece = (i32, Shape);

/// The copper of `item` (see the module docs); empty for items that are no
/// copper (obstacle areas, outlines).
pub fn pieces(board: &BasicBoard, item: &Item) -> Vec<Piece> {
    match &item.kind {
        ItemKind::Trace(trace) => (0..trace.tile_shape_count())
            .filter_map(|i| trace.polyline.offset_shape(trace.half_width, i))
            .map(|shape| (trace.layer, Shape::from(shape)))
            .collect(),
        ItemKind::Pin(_) | ItemKind::Via(_) => {
            let drill = item.drill().expect("a drill item");
            (drill.first_layer..=drill.last_layer)
                .filter_map(|layer| Some((layer, drill.shape_on_layer(layer)?.clone())))
                .collect()
        }
        ItemKind::Area(area) if area.is_conduction_area() => (0..board.tile_shape_count(item))
            .filter_map(|i| board.tile_shape(item, i))
            .map(|shape| (area.layer(), Shape::from(shape)))
            .collect(),
        _ => Vec::new(),
    }
}

/// The items of `of`'s net, other than `of`, whose copper overlaps
/// `piece` on its layer, in id order.
fn touching_piece(board: &BasicBoard, of: &Item, piece: &Piece) -> BTreeSet<ItemId> {
    let (layer, shape) = piece;
    let region = TileShape::IntBox(shape.bounding_box());
    board
        .overlapping_objects(&region, *layer)
        .into_iter()
        .filter(|id| *id != of.id())
        .filter(|id| {
            let other = board.item_ref(*id);
            other.shares_net(of)
                && pieces(board, other)
                    .iter()
                    .any(|(l, s)| l == layer && s.intersects(shape))
        })
        .collect()
}

/// The items of a common net whose copper overlaps `id`'s on a common
/// layer, in id order.
pub fn overlap_contacts(board: &BasicBoard, id: ItemId) -> BTreeSet<ItemId> {
    let Some(item) = board.item(id) else {
        return BTreeSet::new();
    };
    pieces(board, item)
        .iter()
        .flat_map(|piece| touching_piece(board, item, piece))
        .collect()
}

/// True if `a` and `b` share a net and their copper meets on a common layer.
pub fn touches(board: &BasicBoard, a: &Item, b: &Item) -> bool {
    if !a.shares_net(b) {
        return false;
    }
    let pb = pieces(board, b);
    pieces(board, a)
        .iter()
        .any(|(la, sa)| pb.iter().any(|(lb, sb)| la == lb && sa.intersects(sb)))
}

/// True if the cap of trace `trace_item`'s end `end` meets the copper of
/// another item of its net on its layer.
pub fn end_has_contact(board: &BasicBoard, trace_item: &Item, end: &Point) -> bool {
    let Some(trace) = trace_item.as_trace() else {
        return false;
    };
    let cap = match end {
        Point::Int(centre) => Some(Circle::new(*centre, trace.half_width)),
        _ => None,
    };
    let region = match &cap {
        Some(circle) => circle.bounding_box(),
        None => TileShape::get_instance_point(end),
    };
    board
        .overlapping_objects(&TileShape::IntBox(region), trace.layer)
        .into_iter()
        .filter(|id| *id != trace_item.id())
        .any(|id| {
            let other = board.item_ref(id);
            other.shares_net(trace_item)
                && pieces(board, other).iter().any(|(l, s)| {
                    *l == trace.layer
                        && match &cap {
                            Some(circle) => s.intersects_circle(circle),
                            None => s.contains(end),
                        }
                })
        })
}

/// The layers on which via `via`'s pad meets the copper of another item of
/// its net.
pub fn via_contact_layers(board: &BasicBoard, via: &Item) -> BTreeSet<i32> {
    pieces(board, via)
        .iter()
        .filter(|piece| !touching_piece(board, via, piece).is_empty())
        .map(|(layer, _)| *layer)
        .collect()
}

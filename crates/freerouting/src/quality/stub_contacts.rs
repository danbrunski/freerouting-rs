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
//!   tile shapes (a plane is its DSN outline). The `_with` variants take
//!   [`PourFills`]: on a `.kicad_pcb` a signal-layer pour is then the
//!   router's fill model of it, opened by the zone's minimum thickness as
//!   KiCad's filler opens it (K15.23, [`crate::quality::pour_fill`]), and
//!   on a DSN input every signal-layer pour is its fill model, opened as
//!   `router.cleanup.pour_*` give (K15.26); the cleanup uses them, the
//!   output check and the KiCad report the outline;
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
use crate::quality::pour_fill::{Piece, PourFills};

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

/// The copper of `item` as the cleanup judges it: [`pieces`], except that
/// with `fills` a pour of a `.kicad_pcb` is its opened fill model (K15.23,
/// [`PourFills::pieces`]).
fn with_pieces<R>(
    board: &BasicBoard,
    item: &Item,
    fills: Option<&PourFills>,
    f: impl FnOnce(&[Piece]) -> R,
) -> R {
    if let Some(fills) = fills
        && item.is_conduction_area()
        && let Some(opened) = fills.pieces(board, item)
    {
        return f(&opened);
    }
    f(&pieces(board, item))
}

/// The items of `of`'s net, other than `of`, whose copper overlaps
/// `piece` on its layer, in id order.
fn touching_piece(
    board: &BasicBoard,
    of: &Item,
    piece: &Piece,
    fills: Option<&PourFills>,
) -> BTreeSet<ItemId> {
    let (layer, shape) = piece;
    let region = TileShape::IntBox(shape.bounding_box());
    board
        .overlapping_objects(&region, *layer)
        .into_iter()
        .filter(|id| *id != of.id())
        .filter(|id| {
            let other = board.item_ref(*id);
            other.shares_net(of)
                && with_pieces(board, other, fills, |p| {
                    p.iter().any(|(l, s)| l == layer && s.intersects(shape))
                })
        })
        .collect()
}

/// The items of a common net whose copper overlaps `id`'s on a common
/// layer, in id order.
pub fn overlap_contacts(board: &BasicBoard, id: ItemId) -> BTreeSet<ItemId> {
    overlap_contacts_with(board, id, None)
}

/// [`overlap_contacts`], a pour's copper taken from `fills` (K15.23).
pub fn overlap_contacts_with(
    board: &BasicBoard,
    id: ItemId,
    fills: Option<&PourFills>,
) -> BTreeSet<ItemId> {
    let Some(item) = board.item(id) else {
        return BTreeSet::new();
    };
    with_pieces(board, item, fills, |own| {
        own.iter()
            .flat_map(|piece| touching_piece(board, item, piece, fills))
            .collect()
    })
}

/// True if `a` and `b` share a net and their copper meets on a common layer.
pub fn touches(board: &BasicBoard, a: &Item, b: &Item) -> bool {
    touches_with(board, a, b, None)
}

/// [`touches`], a pour's copper taken from `fills` (K15.23).
pub fn touches_with(board: &BasicBoard, a: &Item, b: &Item, fills: Option<&PourFills>) -> bool {
    if !a.shares_net(b) {
        return false;
    }
    with_pieces(board, b, fills, |pb| {
        with_pieces(board, a, fills, |pa| {
            pa.iter()
                .any(|(la, sa)| pb.iter().any(|(lb, sb)| la == lb && sa.intersects(sb)))
        })
    })
}

/// True if the cap of trace `trace_item`'s end `end` meets the copper of
/// another item of its net on its layer.
pub fn end_has_contact(board: &BasicBoard, trace_item: &Item, end: &Point) -> bool {
    end_has_contact_with(board, trace_item, end, None)
}

/// [`end_has_contact`], a pour's copper taken from `fills` (K15.23).
pub fn end_has_contact_with(
    board: &BasicBoard,
    trace_item: &Item,
    end: &Point,
    fills: Option<&PourFills>,
) -> bool {
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
                && with_pieces(board, other, fills, |p| {
                    p.iter().any(|(l, s)| {
                        *l == trace.layer
                            && match &cap {
                                Some(circle) => s.intersects_circle(circle),
                                None => s.contains(end),
                            }
                    })
                })
        })
}

/// The layers on which via `via`'s pad meets the copper of another item of
/// its net.
pub fn via_contact_layers(board: &BasicBoard, via: &Item) -> BTreeSet<i32> {
    via_contact_layers_with(board, via, None)
}

/// [`via_contact_layers`], a pour's copper taken from `fills` (K15.23).
pub fn via_contact_layers_with(
    board: &BasicBoard,
    via: &Item,
    fills: Option<&PourFills>,
) -> BTreeSet<i32> {
    pieces(board, via)
        .iter()
        .filter(|piece| !touching_piece(board, via, piece, fills).is_empty())
        .map(|(layer, _)| *layer)
        .collect()
}

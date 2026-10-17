//! Terminal cells (X04.5): the cells a connection starts from and may end
//! on.
//!
//! Written from KiCadRoutingTools' `add_source_target_cell` and
//! `add_allowed_rect` (`rust_router/src/obstacle_map.rs` at `0486e799`), as
//! an idea. A terminal cell is a frame cell whose centre lies inside the
//! copper of one of the items, on that item's layer (pads with their exact
//! shape, traces and areas with their tile shapes), and inside the board
//! outline. Clearance to neighbouring pads does not remove a terminal cell:
//! [`mark`] adds the cells to the map's source/target set, which overrides
//! the trace blocking there (KRT's rule), and the exact DRC of X05 judges the
//! first move out of the pad. An item with no cell at the frame's step is
//! reported in [`Terminals::empty`]; X05 then retries at a finer step.
//!
//! **Connectors (X05.3).** A grid path ends on a terminal cell, inside the
//! copper of one of the items ([`end_owner`]: pins and vias before traces
//! before areas, then the lowest id). The end is then joined to the item's
//! exact connection point ([`Anchor`]). This is the X05 checklist's design,
//! written from the idea of KiCadRoutingTools' `_merge_terminal_to_exact`
//! (`py_router/single_ended_routing.py` at `0486e799`), not from its code:
//! - a pin or via: a straight segment from the path end to the drill centre
//!   if [`BasicBoard::check_polyline_trace`] passes it; else the end vertex
//!   moved onto the nearest point inside the pad on that layer (the end
//!   itself when it is inside, which a terminal cell always is); else the
//!   attempt fails;
//! - a trace: a straight segment to the projection of the end onto the
//!   trace when that is an integer point on it; else the projection
//!   connector of `RoutingBoard::connect_to_trace_object` on the trace as it
//!   was before the insert, made after the path's own trace is on the board;
//! - a conduction area: nothing, the end lies in it.

use std::collections::BTreeSet;

use freerouting::board::BasicBoard;
use freerouting::board::items::{Item, ItemId, ItemKind};
use freerouting::geometry::{IntBox, IntPoint, Point, Polyline, PolylineShapeOps, TileShapeOps};
use krt_grid_router::GridObstacleMap;

use super::GridTask;
use super::frame::Frame;

/// The terminal cells of a set of items.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Terminals {
    /// The cells `[i, j, layer]`, sorted by `(layer, j, i)`, without repeats.
    pub cells: Vec<[i32; 3]>,
    /// The items with no cell at this step, ascending.
    pub empty: Vec<ItemId>,
}

/// The cells of `items` on `frame`: [`collect`]'s cells.
pub fn cells(board: &BasicBoard, frame: &Frame, items: &BTreeSet<ItemId>) -> Vec<[i32; 3]> {
    collect(board, frame, items).cells
}

/// The terminal cells of `items` on `frame`, and the items that have none.
pub fn collect(board: &BasicBoard, frame: &Frame, items: &BTreeSet<ItemId>) -> Terminals {
    let outline = board.outline().and_then(Item::as_board_outline);
    let mut cells = Vec::new();
    let mut empty = Vec::new();
    for &id in items {
        let Some(item) = board.item(id) else {
            empty.push(id);
            continue;
        };
        let before = cells.len();
        for index in 0..board.tile_shape_count(item) {
            let layer = item.shape_layer(index);
            if layer < 0 || layer as usize >= frame.layers {
                continue;
            }
            let inside = |p: &Point| -> bool {
                match item.drill() {
                    Some(drill) => drill.shape(index).is_some_and(|s| s.contains(p)),
                    None => board.tile_shape(item, index).is_some_and(|s| s.contains(p)),
                }
            };
            let Some(bounds) = shape_box(board, item, index) else {
                continue;
            };
            let Some((i_lo, i_hi, j_lo, j_hi)) = frame.cells_near(&bounds, 0) else {
                continue;
            };
            for j in j_lo..=j_hi {
                for i in i_lo..=i_hi {
                    let p = Point::from(frame.centre(i, j));
                    if inside(&p) && outline.is_none_or(|o| o.contains(&p)) {
                        cells.push([i, j, layer]);
                    }
                }
            }
        }
        if cells.len() == before {
            empty.push(id);
        }
    }
    cells.sort_by_key(|c| (c[2], c[1], c[0]));
    cells.dedup();
    Terminals { cells, empty }
}

/// The bounding box of tile shape `index` of `item` (the exact pad shape's
/// for drill items).
fn shape_box(board: &BasicBoard, item: &Item, index: usize) -> Option<IntBox> {
    match item.drill() {
        Some(drill) => drill.shape(index).map(|s| s.bounding_box()),
        None => board.tile_shape(item, index).map(|s| s.bounding_box()),
    }
}

/// The source and target terminals of `task` on `frame`.
pub fn for_task(board: &BasicBoard, frame: &Frame, task: &GridTask) -> (Terminals, Terminals) {
    (
        collect(board, frame, &task.start),
        collect(board, frame, &task.dest),
    )
}

/// Adds `cells` to the map's source/target set (KRT's override of the
/// blocking at the terminals).
pub fn mark(map: &mut GridObstacleMap, cells: &[[i32; 3]]) {
    for &[i, j, layer] in cells {
        if layer >= 0 {
            map.add_source_target_cell(i, j, layer as usize);
        }
    }
}

/// True if the copper of `item` on `layer` contains `p` (pads with their
/// exact shape, traces and areas with their tile shapes), the test of
/// [`collect`].
pub fn copper_contains(board: &BasicBoard, item: &Item, layer: i32, p: &Point) -> bool {
    (0..board.tile_shape_count(item)).any(|index| {
        item.shape_layer(index) == layer
            && match item.drill() {
                Some(drill) => drill.shape(index).is_some_and(|s| s.contains(p)),
                None => board.tile_shape(item, index).is_some_and(|s| s.contains(p)),
            }
    })
}

/// Where a path end joins its item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// A pin or via: its drill centre.
    Centre(IntPoint),
    /// A trace: the projection of the end onto it.
    Trace(ItemId),
    /// A conduction area (or any other copper): the end itself.
    Inside,
}

/// The item of `items` whose copper on `layer` contains `at`, and its
/// anchor: pins and vias first, then traces, then the rest, each by
/// ascending id. `None` if no item contains `at`.
pub fn end_owner(
    board: &BasicBoard,
    items: &BTreeSet<ItemId>,
    at: IntPoint,
    layer: i32,
) -> Option<(ItemId, Anchor)> {
    let p = Point::from(at);
    items
        .iter()
        .filter_map(|&id| board.item(id).map(|item| (id, item)))
        .filter(|(_, item)| copper_contains(board, item, layer, &p))
        .map(|(id, item)| {
            let (rank, anchor) = match (&item.kind, item.drill()) {
                (ItemKind::Pin(_) | ItemKind::Via(_), Some(drill)) => {
                    (0, Anchor::Centre(drill.center))
                }
                (ItemKind::Trace(_), _) => (1, Anchor::Trace(id)),
                _ => (2, Anchor::Inside),
            };
            (rank, id.0, id, anchor)
        })
        .min_by_key(|&(rank, raw, _, _)| (rank, raw))
        .map(|(_, _, id, anchor)| (id, anchor))
}

/// How a path end is joined to its anchor ([`connector`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connector {
    /// Nothing to add: the end is the anchor, or lies in an area.
    None,
    /// A straight segment from the end to this point (appended as a
    /// corner).
    Segment(IntPoint),
    /// The end vertex moved onto this point inside the pad.
    Moved(IntPoint),
    /// The projection connector onto this trace, made after insertion.
    ToTrace(ItemId),
}

/// The trace a connector belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceSpec {
    /// The board layer.
    pub layer: i32,
    /// The half-width.
    pub half_width: i32,
    /// The net.
    pub net: i32,
    /// The trace clearance class.
    pub clearance_class: i32,
}

/// The connector of the path end `end` on `owner` with `anchor` (module
/// docs). `None` if neither the straight segment nor a point inside the pad
/// works.
pub fn connector(
    board: &mut BasicBoard,
    owner: ItemId,
    anchor: Anchor,
    end: IntPoint,
    spec: TraceSpec,
) -> Option<Connector> {
    let centre = match anchor {
        Anchor::Inside => return Some(Connector::None),
        Anchor::Trace(trace) => return Some(trace_connector(board, trace, end)),
        Anchor::Centre(centre) => centre,
    };
    if centre == end {
        return Some(Connector::None);
    }
    let segment = Polyline::from_corners(&Point::from(end), &Point::from(centre));
    if board.check_polyline_trace(
        &segment,
        spec.layer,
        spec.half_width,
        &[spec.net],
        spec.clearance_class,
    ) {
        return Some(Connector::Segment(centre));
    }
    let pad = board.item(owner)?.drill()?.shape_on_layer(spec.layer)?;
    if pad.contains(&Point::from(end)) {
        return Some(Connector::Moved(end));
    }
    let nearest = pad.nearest_point_approx(end.to_float())?.round();
    pad.contains(&Point::from(nearest))
        .then_some(Connector::Moved(nearest))
}

/// The connector onto trace `trace` from `end`: a segment to the
/// projection of `end` onto the trace when that point is an integer point
/// on the trace's polyline (the board's normalization then splits the trace
/// there), else the board's projection connector after insertion.
fn trace_connector(board: &BasicBoard, trace: ItemId, end: IntPoint) -> Connector {
    let Some(polyline) = board
        .item(trace)
        .and_then(Item::as_trace)
        .map(|t| &t.polyline)
    else {
        return Connector::ToTrace(trace);
    };
    let from = Point::from(end);
    if polyline.contains(&from) {
        return Connector::None;
    }
    let projection = polyline
        .projection_line(&from)
        .map(|segment| segment.to_polyline())
        .filter(|connection| connection.corner_count() == 2)
        .map(|connection| connection.corner(1));
    match projection {
        Some(Point::Int(p)) if polyline.contains(&Point::Int(p)) => Connector::Segment(p),
        _ => Connector::ToTrace(trace),
    }
}

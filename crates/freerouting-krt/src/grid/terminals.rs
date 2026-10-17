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

use std::collections::BTreeSet;

use freerouting::board::BasicBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::geometry::{IntBox, Point, PolylineShapeOps, TileShapeOps};
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

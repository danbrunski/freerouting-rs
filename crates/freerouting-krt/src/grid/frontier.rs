//! The failure record of the `krt-grid` stage (X05.5): the items behind a
//! failed grid search's frontier.
//!
//! For each frontier cell `(gx, gy, layer)` of the vendored search
//! (`route_with_frontier`, at most 10,000 cells, sorted), the items whose
//! shape on that layer is closer to the cell centre than the raster's trace
//! inflation `c + hw + h` (the test that blocked the cell, [`super::raster`]) are
//! counted once per cell, per `(net, item, layer)`. The list is sorted by
//! `(net, item id, layer)` ascending and cut to [`MAX_BLOCKING_ITEMS`]. Cells
//! blocked only statically (outside the outline, the frame's ring) name no
//! item. Written from KiCadRoutingTools' frontier idea
//! (`_identify_blocking_obstacles`, `py_router/single_ended_routing.py` at
//! `0486e799`), not its code.

use std::collections::{BTreeMap, BTreeSet};

use freerouting::autoroute::stages::{BlockingItem, MAX_BLOCKING_ITEMS};
use freerouting::board::BasicBoard;
use freerouting::board::items::ItemId;
use freerouting::geometry::{IntBox, TileShape, TileShapeOps};

use super::half_diagonal;
use super::raster::{Raster, trace_keepout};

/// The largest clearance of class `class` to any class on `layer`.
fn max_clearance(board: &BasicBoard, class: i32, layer: i32) -> i32 {
    let n = board.rules.clearance_matrix.class_count() as i32;
    (0..n)
        .map(|k| board.clearance_value(class, k, layer))
        .max()
        .unwrap_or(0)
}

/// The frontier's blocking items (module docs) for net `net`.
pub fn blocking_items(
    board: &BasicBoard,
    net: i32,
    raster: &Raster,
    frontier: &[(i32, i32, u8)],
) -> Vec<BlockingItem> {
    let frame = &raster.frame;
    let rules = &raster.rules;
    let h = half_diagonal(frame.step);
    let mut counts: BTreeMap<(i32, i32, i32), u32> = BTreeMap::new();
    for &(gx, gy, layer) in frontier {
        let layer = i32::from(layer);
        let centre = frame.centre(gx, gy);
        let margin =
            max_clearance(board, rules.trace_class, layer) + rules.half_width_on(layer) + h;
        let probe = TileShape::IntBox(
            IntBox::new(centre.x, centre.y, centre.x, centre.y).offset(f64::from(margin)),
        );
        let mut entries = board.overlapping_tree_entries(&probe, layer);
        entries.sort_by_key(|e| (e.item, e.shape_index));
        entries.dedup();
        let mut seen = BTreeSet::new();
        for entry in entries {
            let Some(item) = board.item(entry.item) else {
                continue;
            };
            if item.shape_layer(entry.shape_index) != layer
                || !board.is_trace_obstacle(item, net)
                || seen.contains(&entry.item.0)
            {
                continue;
            }
            let Some(shape) = board.tile_shape(item, entry.shape_index) else {
                continue;
            };
            let limit = f64::from(trace_keepout(board, rules, item, layer) + h);
            if shape.distance(centre.to_float()) < limit {
                seen.insert(entry.item.0);
                let item_net = item.net_numbers().first().copied().unwrap_or(0);
                *counts.entry((item_net, entry.item.0, layer)).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .take(MAX_BLOCKING_ITEMS)
        .map(|((net, item, layer), cells)| BlockingItem {
            net,
            item: ItemId(item),
            layer,
            cells,
        })
        .collect()
}

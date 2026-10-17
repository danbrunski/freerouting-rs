//! Board items → the conservative obstacle raster of one connection (X04.3
//! traces, keepouts and outline; X04.4 the via map).
//!
//! Written from KiCadRoutingTools' `build_base_obstacle_map`,
//! `add_rule_area_keepout_obstacles` and `add_board_edge_obstacles`
//! (`py_router/obstacle_map.py` at `0486e799`), as an idea: the inflation
//! here is an exact distance test on the board's shapes, not KRT's capsule
//! rasterizer.
//!
//! **Trace cells.** For each layer `L`, the default tree's entries
//! overlapping the frame (enlarged by the largest inflation) are taken in
//! ascending `(ItemId, shape index)`. An entry whose item is a trace
//! obstacle for the routed net ([`BasicBoard::is_trace_obstacle`], which
//! applies Q01's area policy) blocks every frame cell whose centre is closer
//! to the tile shape than `c + hw(L) + h`: `c` the clearance between the
//! net's trace clearance class and the item's class on `L`
//! (`add_safety_margin = true`, as the router uses), `hw(L)` the net's
//! half-width on `L`, `h` = [`half_diagonal`](super::half_diagonal).
//! Own-net items are not obstacles. Layers the net class doesn't route on
//! are blocked whole.
//!
//! **Via cells.** The via is the first via info of the net's via rule whose
//! padstack spans every board layer (the through via on a two-layer board),
//! else the rule's first. `r_via(L)` is the largest distance from the via's
//! centre to its pad on `L`. A cell is via-blocked if, on a layer of the via's
//! span, it is closer than `c_via + r_via(L) + h` to an item that is a via
//! obstacle ([`is_via_obstacle`]), or closer than `r_via(L) + h` to any pad of
//! any net. The second rule keeps the via's copper out of every pad (D8; the
//! gate's `via_in_pad_count` counts any overlap), and is KRT's extra margin
//! around pads, widened from half a step to `h`.
//!
//! **Keepouts and outline.** `Obstacle` areas block traces and vias,
//! `ViaObstacle` areas vias only, `ComponentObstacle` areas nothing. Cells
//! whose centre is outside the board outline are blocked statically on every
//! layer and for vias; the outline lines themselves are trace and via
//! obstacles like any item (not for traces of edge-pin nets, as Java). A ring
//! of static cells one cell outside the frame keeps the search inside it,
//! since the vendored search has no bounds of its own.
//!
//! **Order.** Cells are collected in per-layer bitmaps and handed to the
//! vendored map once each, in `(layer, j, i)` order (vias in `(j, i)`). The
//! map's refcounts make the result independent of that order; it is fixed
//! anyway. Not covered: hole-to-hole clearance (`BoardRules::hole_clearance`)
//! is left to the exact DRC of X05.

use freerouting::board::BasicBoard;
use freerouting::board::items::{AreaKind, BoardOutline, Item, ItemKind};
use freerouting::geometry::{
    ConvexShape, FloatPoint, Point, PolylineShapeOps, TileShape, TileShapeOps,
};
use freerouting::rules::ViaInfoId;
use krt_grid_router::GridObstacleMap;

use super::frame::Frame;
use super::{GridTask, half_diagonal};

/// What the raster needs from the net's rules.
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

/// Java's via obstacle rule for net `net`: true if a via of `net` may not
/// come within clearance of `item`. Own-net copper is not an obstacle (own
/// pads are handled by the pad rule of the via map).
pub fn is_via_obstacle(board: &BasicBoard, item: &Item, net: i32) -> bool {
    match &item.kind {
        ItemKind::Area(area) => match area.kind {
            AreaKind::ComponentObstacle => false,
            AreaKind::Obstacle | AreaKind::ViaObstacle => !item.contains_net(net),
            AreaKind::Conduction { .. } => board.is_trace_obstacle(item, net),
        },
        ItemKind::BoardOutline(_) => true,
        ItemKind::ComponentOutline(_) => false,
        _ => !item.contains_net(net),
    }
}

/// The trace inflation `c + hw(L)` of `item` on `layer` for `rules` (the
/// raster adds `h`).
pub fn trace_keepout(board: &BasicBoard, rules: &NetRules, item: &Item, layer: i32) -> i32 {
    board.clearance_value(rules.trace_class, item.clearance_class(), layer)
        + rules.half_width_on(layer)
}

/// The via inflation `c_via + r_via(L)` of `item` on `layer` (the raster
/// adds `h`).
pub fn via_keepout(board: &BasicBoard, via: &ViaChoice, item: &Item, layer: i32) -> i32 {
    board.clearance_value(via.clearance_class, item.clearance_class(), layer)
        + via.radius[layer as usize]
}

/// Counts of one build.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RasterStats {
    /// Trace-blocked cells per layer (item stamps only).
    pub blocked: Vec<usize>,
    /// Via-blocked cells (item stamps only).
    pub via_blocked: usize,
    /// Frame cells outside the board outline (per layer).
    pub outside: usize,
    /// Tile shapes stamped.
    pub shapes: usize,
    /// Point-to-shape distance evaluations: the build's work.
    pub distance_tests: u64,
}

/// A built raster: the map plus what X05 needs to read it.
pub struct Raster {
    /// The frame the map covers.
    pub frame: Frame,
    /// The vendored obstacle map.
    pub map: GridObstacleMap,
    /// The rules it was built for.
    pub rules: NetRules,
    /// The build's counts.
    pub stats: RasterStats,
}

/// The obstacle map of `task` on `frame` (the checklist's
/// `grid::raster::build`).
pub fn build(board: &BasicBoard, task: &GridTask, frame: &Frame) -> GridObstacleMap {
    build_raster(board, task, frame).map
}

/// A per-layer cell bitmap over a frame.
struct CellSet {
    bits: Vec<bool>,
    count: usize,
}

impl CellSet {
    fn new(frame: &Frame) -> CellSet {
        CellSet {
            bits: vec![false; frame.cell_count()],
            count: 0,
        }
    }

    fn set(&mut self, index: usize) {
        if !self.bits[index] {
            self.bits[index] = true;
            self.count += 1;
        }
    }
}

/// Marks in `set` the cells of `frame` whose centre is closer to `shape`
/// than `inflate`. Returns the distance evaluations.
fn stamp(frame: &Frame, shape: &TileShape, inflate: i32, set: &mut CellSet) -> u64 {
    let Some((i_lo, i_hi, j_lo, j_hi)) = frame.cells_near(&shape.bounding_box(), inflate) else {
        return 0;
    };
    let limit = f64::from(inflate);
    let mut tests = 0;
    for j in j_lo..=j_hi {
        for i in i_lo..=i_hi {
            let index = frame.index(i, j).expect("cells_near stays in the frame");
            if set.bits[index] {
                continue;
            }
            tests += 1;
            if shape.distance(frame.centre(i, j).to_float()) < limit {
                set.set(index);
            }
        }
    }
    tests
}

/// The largest clearance of class `class` to any class on `layer`, with
/// the safety margin.
fn max_clearance(board: &BasicBoard, class: i32, layer: i32) -> i32 {
    let n = board.rules.clearance_matrix.class_count() as i32;
    (0..n)
        .map(|k| board.clearance_value(class, k, layer))
        .max()
        .unwrap_or(0)
}

/// Builds the raster of `task` on `frame`.
pub fn build_raster(board: &BasicBoard, task: &GridTask, frame: &Frame) -> Raster {
    let rules = NetRules::of_net(board, task.net);
    let layers = frame.layers;
    let h = half_diagonal(frame.step);
    let mut map = GridObstacleMap::new(layers);
    let mut stats = RasterStats {
        blocked: vec![0; layers],
        ..RasterStats::default()
    };
    if frame.is_empty() {
        return Raster {
            frame: *frame,
            map,
            rules,
            stats,
        };
    }
    let mut via_set = CellSet::new(frame);
    let mut trace_sets: Vec<CellSet> = (0..layers).map(|_| CellSet::new(frame)).collect();
    for (layer, trace_set) in trace_sets.iter_mut().enumerate() {
        let l = layer as i32;
        let via_on_layer = rules
            .via
            .as_ref()
            .filter(|v| l >= v.from_layer && l <= v.to_layer);
        let trace_margin = max_clearance(board, rules.trace_class, l) + rules.half_width_on(l);
        let via_margin = via_on_layer.map_or(0, |v| {
            max_clearance(board, v.clearance_class, l) + v.radius[layer]
        });
        let margin = trace_margin.max(via_margin) + h;
        let query = frame.centre_box().offset(f64::from(margin));
        let mut entries = board.overlapping_tree_entries(&TileShape::IntBox(query), l);
        entries.sort_by_key(|e| (e.item, e.shape_index));
        entries.dedup();
        for entry in entries {
            let Some(item) = board.item(entry.item) else {
                continue;
            };
            if item.shape_layer(entry.shape_index) != l {
                continue;
            }
            let Some(shape) = board.tile_shape(item, entry.shape_index) else {
                continue;
            };
            let mut stamped = false;
            if rules.active[layer] && board.is_trace_obstacle(item, task.net) {
                let inflate = trace_keepout(board, &rules, item, l) + h;
                stats.distance_tests += stamp(frame, &shape, inflate, trace_set);
                stamped = true;
            }
            if let Some(via) = via_on_layer {
                let inflate = if is_via_obstacle(board, item, task.net) {
                    Some(via_keepout(board, via, item, l) + h)
                } else if item.is_pin() {
                    Some(via.radius[layer] + h)
                } else {
                    None
                };
                if let Some(inflate) = inflate {
                    stats.distance_tests += stamp(frame, &shape, inflate, &mut via_set);
                    stamped = true;
                }
            }
            stats.shapes += usize::from(stamped);
        }
    }
    if rules.via.is_none() {
        // No via to place: every cell is via-blocked.
        via_set.bits.iter_mut().for_each(|b| *b = true);
        via_set.count = via_set.bits.len();
    }
    emit_dynamic(frame, &trace_sets, &via_set, &mut map);
    for (layer, set) in trace_sets.iter().enumerate() {
        stats.blocked[layer] = set.count;
    }
    stats.via_blocked = via_set.count;
    stats.outside = emit_static(board, frame, &rules, &mut map);
    Raster {
        frame: *frame,
        map,
        rules,
        stats,
    }
}

/// Hands the item stamps to the map in `(layer, j, i)` order.
fn emit_dynamic(frame: &Frame, traces: &[CellSet], vias: &CellSet, map: &mut GridObstacleMap) {
    let mut cells = Vec::new();
    for (layer, set) in traces.iter().enumerate() {
        for_each_set(frame, set, |i, j| cells.push([i, j, layer as i32]));
    }
    map.add_blocked_cells_batch(&cells);
    let mut via_cells = Vec::with_capacity(vias.count);
    for_each_set(frame, vias, |i, j| via_cells.push([i, j]));
    map.add_blocked_vias_batch(&via_cells);
}

fn for_each_set(frame: &Frame, set: &CellSet, mut f: impl FnMut(i32, i32)) {
    for j in 0..frame.ny {
        for i in 0..frame.nx {
            if set.bits[(j * frame.nx + i) as usize] {
                f(frame.i0 + i, frame.j0 + j);
            }
        }
    }
}

/// The static cells: outside the outline, inactive layers, and the ring
/// around the frame. Returns the number of frame cells outside the outline.
fn emit_static(
    board: &BasicBoard,
    frame: &Frame,
    rules: &NetRules,
    map: &mut GridObstacleMap,
) -> usize {
    let outside = board
        .outline()
        .and_then(Item::as_board_outline)
        .map_or_else(Vec::new, |o| outside_cells(o, frame));
    let mut cells: Vec<[i32; 3]> = Vec::new();
    for layer in 0..frame.layers {
        if rules.active[layer] {
            cells.extend(outside.iter().map(|&[i, j]| [i, j, layer as i32]));
        }
    }
    map.add_static_blocked_cells_batch(&cells);
    map.add_static_blocked_vias_batch(&outside);
    // Inactive layers and the ring, as column spans.
    let (i_lo, i_hi) = (frame.i0 - 1, frame.i0 + frame.nx);
    let (j_lo, j_hi) = (frame.j0 - 1, frame.j0 + frame.ny);
    let mut spans: Vec<[i32; 4]> = Vec::new();
    for layer in 0..frame.layers as i32 {
        if rules.active[layer as usize] {
            spans.push([i_lo, j_lo, j_hi, layer]);
            for i in frame.i0..frame.i0 + frame.nx {
                spans.push([i, j_lo, j_lo, layer]);
                spans.push([i, j_hi, j_hi, layer]);
            }
            spans.push([i_hi, j_lo, j_hi, layer]);
        } else {
            for i in i_lo..=i_hi {
                spans.push([i, j_lo, j_hi, layer]);
            }
        }
    }
    spans.sort_by_key(|s| (s[3], s[0], s[1]));
    map.add_static_blocked_cell_spans_batch(&spans);
    let mut ring: Vec<[i32; 3]> = vec![[i_lo, j_lo, j_hi]];
    for i in frame.i0..frame.i0 + frame.nx {
        ring.push([i, j_lo, j_lo]);
        ring.push([i, j_hi, j_hi]);
    }
    ring.push([i_hi, j_lo, j_hi]);
    map.add_static_blocked_via_spans_batch(&ring);
    outside.len()
}

/// The side of a block of cells, for [`outside_cells`].
const OUTLINE_BLOCK: i32 = 16;

/// The cells of `frame` whose centre is not in `outline`
/// ([`BoardOutline::contains`], border inside), in `(j, i)` order.
///
/// Exactly the per-cell test, faster: a block of `OUTLINE_BLOCK²` cells that
/// no outline edge's bounding box (widened by 2 units for the float corners)
/// touches lies wholly on one side of the outline, so one centre decides it.
pub fn outside_cells(outline: &BoardOutline, frame: &Frame) -> Vec<[i32; 2]> {
    let mut edges: Vec<[f64; 4]> = Vec::new();
    for shape in outline.shapes() {
        let corners = shape.corner_approx_arr();
        for (k, a) in corners.iter().enumerate() {
            let b = corners[(k + 1) % corners.len()];
            edges.push([
                a.x.min(b.x) - 2.0,
                a.y.min(b.y) - 2.0,
                a.x.max(b.x) + 2.0,
                a.y.max(b.y) + 2.0,
            ]);
        }
    }
    let mut outside = vec![false; frame.cell_count()];
    let inside = |i: i32, j: i32| outline.contains(&Point::from(frame.centre(i, j)));
    for bj in (0..frame.ny).step_by(OUTLINE_BLOCK as usize) {
        for bi in (0..frame.nx).step_by(OUTLINE_BLOCK as usize) {
            let (i_lo, j_lo) = (frame.i0 + bi, frame.j0 + bj);
            let i_hi = (i_lo + OUTLINE_BLOCK).min(frame.i0 + frame.nx) - 1;
            let j_hi = (j_lo + OUTLINE_BLOCK).min(frame.j0 + frame.ny) - 1;
            let lo = frame.centre(i_lo, j_lo).to_float();
            let hi = frame.centre(i_hi, j_hi).to_float();
            let crossed = edges
                .iter()
                .any(|e| e[0] <= hi.x && e[2] >= lo.x && e[1] <= hi.y && e[3] >= lo.y);
            let uniform = (!crossed).then(|| !inside(i_lo, j_lo));
            for j in j_lo..=j_hi {
                for i in i_lo..=i_hi {
                    let out = uniform.unwrap_or_else(|| !inside(i, j));
                    if out {
                        outside[frame.index(i, j).expect("block in frame")] = true;
                    }
                }
            }
        }
    }
    let mut result = Vec::new();
    for j in 0..frame.ny {
        for i in 0..frame.nx {
            if outside[(j * frame.nx + i) as usize] {
                result.push([frame.i0 + i, frame.j0 + j]);
            }
        }
    }
    result
}

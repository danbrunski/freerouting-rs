//! Infeasible-connection diagnostic (Q03): can a trace of its net class's
//! width reach a pin at all?
//!
//! For each candidate pin, a window around it is rasterised
//! ([`raster`]), an exact distance transform gives the widest trace that
//! fits at each cell ([`edt`], [`widest_path::field`]), and a Kruskal
//! union-find in descending margin finds the width at which the pin first
//! joins the rest of its net or leaves the window ([`widest_path`]). That
//! width is the exact bottleneck of the raster. Infeasible pins also get a
//! neck ladder ([`neck`]). The result is a report value ([`report`]);
//! nothing in routing reads it.
//!
//! Credit: the design is KiCadRoutingTools' widest-path instrument
//! (`py_placer/placement/reachability.py`, `slack_field` and `widest_path`,
//! at `0486e799`); this module is written from the idea, not from its code.
//!
//! Every simplification errs toward "passable", so `infeasible` is a proof
//! up to the raster tolerance `δ = s·√2`:
//! - clearances from the matrix without the safety margin;
//! - pads and vias use their true pad shapes (a round pad is a disk, not
//!   Java's bounding octagon); a drill layer without a pad shape is
//!   ignored;
//! - a via is legal where its pad keeps its clearance to foreign copper and
//!   keepouts on every routing layer of its span and its centre is on no
//!   pad (D8) and no via keepout; drill-to-drill rules and plane layers are
//!   ignored;
//! - moves are 8-connected; a net with a plane counts every legal via site
//!   as a target.
//!
//! Targets are own-net copper other than the pin (own traces narrower than
//! the class width don't count) and every cell on the window border: any real route either ends on own copper in the window or
//! leaves it. A pin that reaches neither is walled in.
//!
//! Candidates are the pins of nets with at least two connectable items
//! whose class width on one of the pin's layers exceeds the pin's minimum
//! width there (Java's neck-down condition), or every such pin with
//! [`FeasibilityOptions::all_pins`].

pub mod edt;
pub mod neck;
pub mod raster;
pub mod report;
pub mod widest_path;

use freerouting_geometry::{IntBox, IntPoint};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::structure::{ComponentId, Unit};
use crate::rules::{NetClass, Nets};

pub use neck::{NeckLadder, NeckRung};
pub use report::{FeasibilityReport, NearItem, PinFeasibility, Throat};

use raster::{Bits, Frame, Window, WindowSpec, build_window};
use widest_path::{PathOutcome, Requirement, ViaSpan, field, widest_path};

/// The default raster step in µm.
pub const DEFAULT_STEP_UM: f64 = 10.0;

/// The smallest window margin around the pin, in mm.
pub const MIN_WINDOW_MM: f64 = 3.0;

/// Where a trace must end on the pin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartModel {
    /// Anywhere inside the pin's shapes (the default): "no trace of width
    /// W can end on the pin at all". Java's router ends most traces at the
    /// pin centre but not all: on the reference routes 36 of 118 trace ends
    /// inside the pins this check flags lie off the centre (Q03.7).
    PinShape,
    /// At the pin centre: the trace keeps its width from the centre
    /// outward and the pin's other cells are ordinary cells. This is
    /// canary-09's model ("a foreign pad closer to the pin centre than
    /// W/2 + c"); it is not a proof against Java, which can end a trace
    /// near the pad edge.
    PinCentre,
}

impl StartModel {
    pub fn name(self) -> &'static str {
        match self {
            StartModel::PinCentre => "centre",
            StartModel::PinShape => "shape",
        }
    }
}

/// The options of [`check_board`].
#[derive(Clone, Debug, PartialEq)]
pub struct FeasibilityOptions {
    /// The raster step in µm.
    pub step_um: f64,
    /// Where a trace must end on the pin.
    pub start: StartModel,
    /// Check every connectable pin, not only the neck-down candidates.
    pub all_pins: bool,
    /// Run the neck ladder on infeasible pins.
    pub neck_ladder: bool,
    /// Check the pins on all cores (the result is the same).
    pub parallel: bool,
}

impl Default for FeasibilityOptions {
    fn default() -> Self {
        FeasibilityOptions {
            step_um: DEFAULT_STEP_UM,
            start: StartModel::PinShape,
            all_pins: false,
            neck_ladder: true,
            parallel: true,
        }
    }
}

/// The verdict for one pin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// `C + δ < W`: no trace of the class width reaches the pin.
    Infeasible,
    /// `C < W ≤ C + δ`: within the raster tolerance.
    Marginal,
    /// `C ≥ W`.
    Passable,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Infeasible => "infeasible",
            Verdict::Marginal => "marginal",
            Verdict::Passable => "passable",
        }
    }
}

/// The verdict and corridor of a search outcome (`None`: enclosed).
pub(crate) fn verdict_of(outcome: &PathOutcome, delta: f64) -> (Verdict, Option<i64>) {
    match *outcome {
        PathOutcome::Connected => (Verdict::Passable, Some(i64::MAX)),
        PathOutcome::Enclosed => (Verdict::Infeasible, None),
        PathOutcome::Throat {
            margin, corridor, ..
        } => {
            let verdict = if margin as f64 + delta < 0.0 {
                Verdict::Infeasible
            } else if margin < 0 {
                Verdict::Marginal
            } else {
                Verdict::Passable
            };
            (verdict, Some(corridor))
        }
    }
}

/// Everything the check of one pin needs.
pub(crate) struct PinContext<'a> {
    pub item: &'a Item,
    pub net: i32,
    pub window: Window,
    /// The cells a route starts from.
    pub start: Bits,
    /// The class width on each frame slot (board units).
    pub width: Vec<i64>,
    pub units_per_mm: f64,
    pub delta: f64,
    pub plane_targets: bool,
}

/// The routing layers of `class`: the active signal layers.
fn routing_layers(board: &BasicBoard, class: &NetClass) -> Vec<i32> {
    (0..board.layer_count() as i32)
        .filter(|&l| {
            board.layer_structure.layers[l as usize].is_signal && class.is_active_routing_layer(l)
        })
        .collect()
}

/// The first normal net of `item`.
fn pin_net(item: &Item) -> Option<i32> {
    item.net_numbers()
        .iter()
        .copied()
        .find(|n| Nets::is_normal_net_number(*n))
}

/// The pins to check, ascending.
pub fn candidate_pins(board: &BasicBoard, all_pins: bool) -> Vec<ItemId> {
    let mut result = Vec::new();
    for id in board.pins() {
        let Some(item) = board.item(id) else {
            continue;
        };
        let Some(net) = pin_net(item) else {
            continue;
        };
        if board.connectable_item_count(net) < 2 {
            continue;
        }
        let Some(class) = board.rules.net_class_of(net) else {
            continue;
        };
        let layers = routing_layers(board, class);
        let on_layers: Vec<i32> = layers
            .into_iter()
            .filter(|l| item.drill().and_then(|d| d.shape_on_layer(*l)).is_some())
            .collect();
        if on_layers.is_empty() {
            continue;
        }
        let needs_neck = on_layers.iter().any(|&l| {
            f64::from(2 * class.trace_half_width(l)) > neck::pin_min_width(board, item, l)
        });
        if all_pins || needs_neck {
            result.push(id);
        }
    }
    result.sort();
    result
}

/// Board units per mm.
pub fn units_per_mm(board: &BasicBoard) -> f64 {
    board.communication.get_resolution(Unit::Mm)
}

/// Checks every candidate pin of `board`.
pub fn check_board(board: &BasicBoard, opts: &FeasibilityOptions) -> FeasibilityReport {
    let upm = units_per_mm(board);
    let step = ((opts.step_um * upm / 1000.0).round() as i32).max(1);
    let candidates = candidate_pins(board, opts.all_pins);
    let check = |id: &ItemId| check_pin(board, *id, step, opts);
    let pins: Vec<PinFeasibility> = if opts.parallel {
        crate::drc::parallel_map(&candidates, check)
    } else {
        candidates.iter().map(check).collect()
    }
    .into_iter()
    .flatten()
    .collect();
    FeasibilityReport {
        options: opts.clone(),
        step,
        units_per_mm: upm,
        candidates: candidates.len(),
        pins,
    }
}

/// The via spans of `class`'s via rule on `frame_layers`, with their
/// clearance classes.
fn via_spans(
    board: &BasicBoard,
    class: &NetClass,
    frame_layers: &[i32],
) -> (Vec<ViaSpan>, Vec<i32>) {
    let mut spans = Vec::new();
    let mut classes = Vec::new();
    let Some(rule) = class.via_rule().or_else(|| board.rules.default_via_rule()) else {
        return (spans, classes);
    };
    for &via in board.rules.via_rules.get(rule).vias() {
        let info = board.rules.via_infos.get(via);
        let Some(padstack) = board.library.padstacks.get(info.padstack) else {
            continue;
        };
        let slots: Vec<usize> = frame_layers
            .iter()
            .enumerate()
            .filter(|(_, l)| (padstack.from_layer()..=padstack.to_layer()).contains(*l))
            .map(|(s, _)| s)
            .collect();
        let (Some(&first_slot), Some(&last_slot)) = (slots.first(), slots.last()) else {
            continue;
        };
        if first_slot == last_slot {
            continue;
        }
        let origin = freerouting_geometry::FloatPoint { x: 0.0, y: 0.0 };
        let radius = (first_slot..=last_slot)
            .map(|s| {
                padstack
                    .get_shape(frame_layers[s])
                    .map_or(0.0, |shape| shape.to_shape().border_distance(origin))
            })
            .collect();
        spans.push(ViaSpan {
            first_slot,
            last_slot,
            radius,
        });
        classes.push(info.clearance_class);
    }
    (spans, classes)
}

/// The start cells of the pin of `window`: its shapes, or the cell nearest
/// its centre on each layer where it has a shape.
fn start_cells(window: &Window, item: &Item, model: StartModel) -> Bits {
    if model == StartModel::PinShape {
        return window.start.clone();
    }
    let frame = &window.frame;
    let mut result = Bits::new(frame.cell_count());
    let Some(drill) = item.drill() else {
        return result;
    };
    let step = f64::from(frame.step);
    let i = (f64::from(drill.center.x) / step).round() as i64 - i64::from(frame.i0);
    let j = (f64::from(drill.center.y) / step).round() as i64 - i64::from(frame.j0);
    if i < 0 || j < 0 || i >= frame.nx as i64 || j >= frame.ny as i64 {
        return result;
    }
    for (slot, layer) in frame.layers.iter().enumerate() {
        if drill.shape_on_layer(*layer).is_some() {
            result.set(frame.index(slot, i as usize, j as usize));
        }
    }
    result
}

/// Checks one pin; `None` if it has no routable net or layer.
pub fn check_pin(
    board: &BasicBoard,
    id: ItemId,
    step: i32,
    opts: &FeasibilityOptions,
) -> Option<PinFeasibility> {
    let item = board.item(id)?;
    let net = pin_net(item)?;
    let class = board.rules.net_class_of(net)?;
    let layers = routing_layers(board, class);
    if layers.is_empty() {
        return None;
    }
    let upm = units_per_mm(board);
    let width: Vec<i64> = layers
        .iter()
        .map(|&l| 2 * i64::from(class.trace_half_width(l)))
        .collect();
    let matrix = &board.rules.clearance_matrix;
    let c_max = layers
        .iter()
        .flat_map(|&l| {
            (0..matrix.class_count() as i32)
                .map(move |j| matrix.get_value(class.trace_clearance_class(), j, l, false))
        })
        .max()
        .unwrap_or(0);
    let w_max = width.iter().copied().max().unwrap_or(0) as f64;
    let reach = (MIN_WINDOW_MM * upm).max(4.0 * (w_max / 2.0 + f64::from(c_max)));
    let reach = reach.ceil() as i32;
    let pin_box = item.bounding_box();
    let bounds = IntBox {
        ll: IntPoint {
            x: pin_box.ll.x - reach,
            y: pin_box.ll.y - reach,
        },
        ur: IntPoint {
            x: pin_box.ur.x + reach,
            y: pin_box.ur.y + reach,
        },
    };
    let layers_half_width = layers.iter().map(|&l| class.trace_half_width(l)).collect();
    let frame = Frame::covering(&bounds, step, layers);
    let (vias, via_classes) = via_spans(board, class, &frame.layers);
    let spec = WindowSpec {
        frame,
        pin: id,
        net,
        trace_clearance_class: class.trace_clearance_class(),
        via_clearance_classes: via_classes,
        class_half_width: layers_half_width,
    };
    let window = build_window(board, &spec);
    let start = start_cells(&window, item, opts.start);
    if start.count_ones() == 0 {
        return None;
    }
    let ctx = PinContext {
        item,
        net,
        window,
        start,
        width,
        units_per_mm: upm,
        delta: f64::from(step) * std::f64::consts::SQRT_2,
        plane_targets: board
            .rules
            .nets
            .get(net)
            .is_some_and(|n| n.contains_plane()),
    };
    let field = field(&ctx.window, &vias);
    let req = Requirement {
        width: ctx.width.clone(),
        neck: None,
    };
    let outcome = widest_path(
        &ctx.window,
        &field,
        &vias,
        &req,
        &ctx.start,
        ctx.plane_targets,
    );
    let (verdict, _) = verdict_of(&outcome, ctx.delta);
    let ladder = (opts.neck_ladder && verdict == Verdict::Infeasible)
        .then(|| neck::neck_ladder(board, &ctx, &field, &vias));
    Some(report::pin_report(board, &ctx, &field, &outcome, ladder))
}

/// The component and pin name of a pin, as `("U1", "23")`.
pub(crate) fn component_and_pin(board: &BasicBoard, item: &Item) -> Option<(String, String)> {
    let component = board.components.get(ComponentId(item.component_id()))?;
    let pin = board.pin_name(item)?;
    Some((component.name.clone(), pin.to_string()))
}

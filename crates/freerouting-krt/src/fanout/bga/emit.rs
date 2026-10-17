//! Emission of the BGA escapes through the board API (X12.4).
//!
//! Written from the X12 checklist's design; KiCadRoutingTools'
//! `py_router/bga_fanout` (at `0486e799`) gave the ideas of escaping rings
//! outside in and dropping plane balls to their plane with their own via
//! (#424); no KRT code is ported here.
//!
//! **Which balls.** Every populated position of every array
//! ([`analyze_array`]), components by name, rings outside in, each ring
//! clockwise from its top-left ([`BallArray::escape_order`]). A ball is
//! skipped when it has other than one net, an ignored net, is the only
//! terminal of its net or already has copper attached (as X10's pins).
//!
//! **What a ball gets.** A *plane ball* (its net has a conduction area on
//! another layer) gets its dog-bone only: a trace on the pad layer from the
//! ball's centre to its via ([`super::vias`]), and the via, which must span
//! a layer of the net's plane (D8: its own via, never in a pad). A *signal
//! ball* gets its dog-bone and an escape on the layer and slot
//! [`super::channels::assign`] gives it: from the via a 45° jog to its slot
//! of the band, then straight out to past the array (the escape box's edge
//! plus clearance plus half-width, and at least one and a half pitches past
//! the outer ring's vias). A signal ball without a fitting via or without a
//! free slot gets nothing and is reported.
//!
//! **Widths.** On every layer the half-width is the net's class
//! half-width there, narrowed to the ball's neck-down half-width (B05: Java
//! `neckdownHalfWidth`) and never below the board's minimum (D8). The
//! band capacity of a layer uses the widest of these over the array, the
//! largest clearance there between any two of the classes in the band
//! (the escaping nets' trace classes and their vias' classes), and the
//! largest via.
//!
//! **Insertion.** Each ball's dog-bone, via and escape go in one B09
//! transaction ([`RoutingBoard::try_change`], [`CheckPolicy::NewItemsOnly`]);
//! the transaction also refuses a via in a pad, a via that a nearby pin's
//! check reports (X06's finding) and copper that does not reach its ball. A
//! rejected ball is reported ([`Skip::Rejected`]) and skipped.
//!
//! Determinism (D2): integer geometry in the part frame, ordered loops,
//! nothing reads a clock or iterates a hash.

use std::collections::BTreeSet;

use freerouting::autoroute::control::min_trace_half_width;
use freerouting::autoroute::events::CancelToken;
use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::{ComponentId, FixedState};
use freerouting::board::transaction::{CheckPolicy, TransactionResult};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::geometry::{IntPoint, Point, Polyline, TileShape};
use freerouting::settings::board::BoardRouterSettings;

use super::array::{BallArray, Rejection, arrays_of};
use super::channels::{Band, Demand, EscapeLayer, Slot, assign, capacity, slot_offset};
use super::vias::{DogBone, ViaOption, choose, lateral_unit, sigma, via_options};
use crate::fanout::frame::Side;
use crate::grid::raster::NetRules;
use crate::grid::router::via_in_pad;

/// Why a ball got nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Skip {
    /// The ball has no net or more than one.
    NetCount,
    /// Its net is ignored by the autorouter.
    IgnoredNet,
    /// It is the only terminal of its net.
    Unconnected,
    /// Copper is already attached to it.
    AlreadyConnected,
    /// No via of its rule fits its square, or none spans a layer it can
    /// escape to: not escapable by vias.
    NoViaFits,
    /// No layer has a free slot in its band.
    NoChannel,
    /// The transaction refused it.
    Rejected,
}

/// The result for one ball.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BallResult {
    /// A signal ball: dog-bone, via and an escape on `layer` in `slot`.
    Escaped {
        /// The board layer of the escape.
        layer: i32,
        /// Its slot in the band.
        slot: u32,
    },
    /// A plane ball: dog-bone and via.
    PlaneVia,
    /// Nothing.
    Skipped(Skip),
}

/// One ball's outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BallOutcome {
    /// The component.
    pub component: ComponentId,
    /// The ball.
    pub pin: ItemId,
    /// Its row and column.
    pub position: (u16, u16),
    /// Its ring.
    pub ring: u16,
    /// Its side.
    pub side: Side,
    /// The result.
    pub result: BallResult,
    /// The inserted items: the dog-bone trace, the via, then the escape.
    pub items: Vec<ItemId>,
}

/// What [`escape`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EscapeReport {
    /// The arrays, by component name.
    pub parts: Vec<ComponentId>,
    /// The escape layers of each array, in its own order.
    pub layers: Vec<(ComponentId, Vec<EscapeLayer>)>,
    /// The components with at least [`REPORTED_PINS`] pins that are not
    /// arrays, by name, and why.
    pub rejected: Vec<(ComponentId, Rejection)>,
    /// Every ball of the arrays, in escape order.
    pub outcomes: Vec<BallOutcome>,
}

/// Components with fewer pins are not listed as rejected.
pub const REPORTED_PINS: usize = 16;

impl EscapeReport {
    /// The signal escapes inserted.
    pub fn escapes(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o.result, BallResult::Escaped { .. }))
            .count()
    }

    /// The plane vias inserted.
    pub fn plane_vias(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| o.result == BallResult::PlaneVia)
            .count()
    }

    /// The arrays where the planner placed copper on at least one ball.
    pub fn escaped_parts(&self) -> Vec<ComponentId> {
        self.parts
            .iter()
            .copied()
            .filter(|&p| {
                self.outcomes
                    .iter()
                    .any(|o| o.component == p && !o.items.is_empty())
            })
            .collect()
    }

    /// The escapes of `component` per board layer, by layer.
    pub fn escapes_per_layer(&self, component: ComponentId) -> Vec<(i32, usize)> {
        let mut out: std::collections::BTreeMap<i32, usize> = Default::default();
        for o in self.outcomes.iter().filter(|o| o.component == component) {
            if let BallResult::Escaped { layer, .. } = o.result {
                *out.entry(layer).or_default() += 1;
            }
        }
        out.into_iter().collect()
    }

    /// One log line per array, one per rejected component, one summary.
    pub fn log(&self, board: &BasicBoard) -> Vec<String> {
        let name = |c: ComponentId| board.components.get(c).map_or("?", |c| c.name.as_str());
        let mut out = Vec::new();
        for &part in &self.parts {
            let mine: Vec<&BallOutcome> = self
                .outcomes
                .iter()
                .filter(|o| o.component == part)
                .collect();
            let count = |s: Skip| {
                mine.iter()
                    .filter(|o| o.result == BallResult::Skipped(s))
                    .count()
            };
            out.push(format!(
                "krt-bga-escape {}: {} balls, {} escapes, {} plane vias, {} no via fits, {} no channel, {} rejected; skipped: {} net count, {} ignored, {} alone, {} connected",
                name(part),
                mine.len(),
                mine.iter()
                    .filter(|o| matches!(o.result, BallResult::Escaped { .. }))
                    .count(),
                mine.iter().filter(|o| o.result == BallResult::PlaneVia).count(),
                count(Skip::NoViaFits),
                count(Skip::NoChannel),
                count(Skip::Rejected),
                count(Skip::NetCount),
                count(Skip::IgnoredNet),
                count(Skip::Unconnected),
                count(Skip::AlreadyConnected),
            ));
        }
        for &(c, why) in &self.rejected {
            out.push(format!(
                "krt-bga-escape {}: not an array ({})",
                name(c),
                why.name()
            ));
        }
        out.push(format!(
            "krt-bga-escape: {} arrays, {} escapes, {} plane vias",
            self.parts.len(),
            self.escapes(),
            self.plane_vias()
        ));
        out
    }
}

/// The arrays of `board` by component name, and the rejected components
/// with at least [`REPORTED_PINS`] pins.
pub fn arrays(board: &BasicBoard) -> (Vec<BallArray>, Vec<(ComponentId, Rejection)>) {
    let mut found = Vec::new();
    let mut rejected = Vec::new();
    for (id, result) in arrays_of(board) {
        match result {
            Ok(array) => found.push(array),
            Err(why) => {
                let pins = board
                    .items()
                    .filter(|i| i.is_pin() && i.component_id() == id.0)
                    .count();
                if pins >= REPORTED_PINS {
                    rejected.push((id, why));
                }
            }
        }
    }
    (found, rejected)
}

/// Escapes every array of `board` (module docs). A router stop requested
/// on `cancel` ends the run between arrays.
pub fn escape(
    board: &mut RoutingBoard,
    settings: &BoardRouterSettings,
    cancel: &CancelToken,
) -> EscapeReport {
    let (found, rejected) = arrays(board);
    let mut report = EscapeReport {
        parts: found.iter().map(|a| a.component).collect(),
        rejected,
        ..EscapeReport::default()
    };
    let min_half_width = min_trace_half_width(board, settings).max(1);
    for array in &found {
        if cancel.is_router_stop_requested() {
            break;
        }
        let plan = plan_array(board, array, min_half_width);
        report.layers.push((array.component, plan.layers.clone()));
        for ball in &plan.balls {
            let outcome = insert_ball(board, array, &plan, ball);
            report.outcomes.push(outcome);
        }
    }
    report
}

/// One ball's plan.
#[derive(Clone, Debug)]
struct BallPlan {
    position: (u16, u16),
    pin: ItemId,
    ring: u16,
    side: Side,
    skip: Option<Skip>,
    net: i32,
    class: i32,
    plane: bool,
    via: Option<(ViaOption, IntPoint)>,
    /// The half-width per index of the escape layers.
    half_width: Vec<i32>,
    /// The pad-layer half-width (the dog-bone).
    pad_half_width: i32,
    slot: Option<Slot>,
}

/// An array's plan.
struct ArrayPlan {
    layers: Vec<EscapeLayer>,
    /// Per escape layer: trace pitch (`w + c`), largest half-width and
    /// clearance.
    spacing: Vec<(i64, i64, i64)>,
    balls: Vec<BallPlan>,
}

/// The signal layers in the part's own order: ascending on the front,
/// descending on the back, so the pad layer comes first either way.
fn part_layers(board: &BasicBoard, array: &BallArray) -> Vec<i32> {
    let mut layers: Vec<i32> = (0..board.layer_count() as i32)
        .filter(|&l| board.layer_structure.layers[l as usize].is_signal)
        .collect();
    if array.frame.mirrored() {
        layers.reverse();
    }
    layers
}

/// The layers other than `layer` with a conduction area of net `net`.
fn plane_layers(board: &BasicBoard, net: i32, layer: i32) -> BTreeSet<i32> {
    board
        .conduction_areas()
        .into_iter()
        .filter_map(|id| board.item(id))
        .filter(|a| a.contains_net(net) && a.first_layer() != layer)
        .map(Item::first_layer)
        .collect()
}

/// Why `item` gets nothing, if it doesn't (X10's rules).
fn skip_reason(board: &BasicBoard, item: &Item) -> Option<Skip> {
    if item.net_count() != 1 {
        return Some(Skip::NetCount);
    }
    if board.has_ignored_nets(item) {
        return Some(Skip::IgnoredNet);
    }
    let net = item.net_number(0);
    if !board
        .net_terminal_items(net)
        .iter()
        .any(|&id| id != item.id())
    {
        return Some(Skip::Unconnected);
    }
    if board.connected_set(item.id(), net, false).len() > 1 {
        return Some(Skip::AlreadyConnected);
    }
    None
}

/// The plan of one ball before the channel assignment.
fn plan_ball(
    board: &BasicBoard,
    array: &BallArray,
    layers: &[i32],
    position: (u16, u16),
    min_half_width: i32,
) -> BallPlan {
    let (row, col) = position;
    let pin = array.ball_of[&position];
    let mut plan = BallPlan {
        position,
        pin,
        ring: array.ring(row, col),
        side: array.side(row, col),
        skip: None,
        net: 0,
        class: 1,
        plane: false,
        via: None,
        half_width: vec![0; layers.len()],
        pad_half_width: 0,
        slot: None,
    };
    let Some(item) = board.item(pin) else {
        plan.skip = Some(Skip::NetCount);
        return plan;
    };
    plan.skip = skip_reason(board, item);
    if plan.skip.is_some() {
        return plan;
    }
    let net = item.net_number(0);
    let rules = NetRules::of_net(board, net);
    plan.net = net;
    plan.class = rules.trace_class;
    let neck = board
        .trace_neckdown_half_width(pin, array.pad_layer)
        .max(min_half_width);
    let narrow = |layer: i32| rules.half_width_on(layer).min(neck).max(min_half_width);
    plan.half_width = layers.iter().map(|&l| narrow(l)).collect();
    plan.pad_half_width = narrow(array.pad_layer);
    let planes = plane_layers(board, net, array.pad_layer);
    plan.plane = !planes.is_empty();
    let active = |l: i32| {
        usize::try_from(l)
            .ok()
            .and_then(|i| rules.active.get(i).copied())
            .unwrap_or(false)
    };
    let options = via_options(board, net);
    let usable = |v: &ViaOption| {
        if plan.plane {
            planes.iter().any(|&l| v.spans(l))
        } else {
            layers
                .iter()
                .any(|&l| l != array.pad_layer && v.spans(l) && active(l))
        }
    };
    match choose(board, array, row, col, &options, usable) {
        DogBone::Via { via, at } => plan.via = Some((via, at)),
        DogBone::NoViaFits => plan.skip = Some(Skip::NoViaFits),
    }
    plan
}

/// The band of a ball: its side and the row or column it runs along.
fn band_of(side: Side, (row, col): (u16, u16)) -> Band {
    let line = match side {
        Side::W | Side::E => row,
        Side::S | Side::N => col,
    };
    Band { side, line }
}

/// The plan of `array`: every ball, the escape layers with their
/// capacities and the channel assignment (module docs).
fn plan_array(board: &BasicBoard, array: &BallArray, min_half_width: i32) -> ArrayPlan {
    let layers = part_layers(board, array);
    let mut balls: Vec<BallPlan> = array
        .escape_order()
        .into_iter()
        .map(|p| plan_ball(board, array, &layers, p, min_half_width))
        .collect();
    let with_via: Vec<&BallPlan> = balls
        .iter()
        .filter(|b| b.skip.is_none() && b.via.is_some())
        .collect();
    let mut classes: BTreeSet<i32> = BTreeSet::new();
    for b in &with_via {
        if let Some((v, _)) = &b.via {
            classes.insert(v.clearance_class);
        }
        if !b.plane {
            classes.insert(b.class);
        }
    }
    let mut escape_layers = Vec::new();
    let mut spacing = Vec::new();
    for (i, &layer) in layers.iter().enumerate() {
        let obstacle = with_via
            .iter()
            .filter_map(|b| b.via.as_ref())
            .map(|(v, _)| 2 * i64::from(v.radius_on(layer)))
            .max()
            .unwrap_or(0);
        let h = with_via
            .iter()
            .filter(|b| !b.plane)
            .map(|b| i64::from(b.half_width[i]))
            .max()
            .unwrap_or(0);
        let c = classes
            .iter()
            .flat_map(|&a| classes.iter().map(move |&b| (a, b)))
            .map(|(a, b)| i64::from(board.clearance_value(a, b, layer)))
            .max()
            .unwrap_or(0);
        escape_layers.push(EscapeLayer {
            layer,
            capacity: capacity(array.pitch, obstacle, 2 * h, c),
            pad_layer: layer == array.pad_layer,
        });
        spacing.push((2 * h + c, h, c));
    }
    let signal: Vec<usize> = (0..balls.len())
        .filter(|&i| balls[i].skip.is_none() && !balls[i].plane)
        .collect();
    let demands: Vec<Demand> = signal
        .iter()
        .map(|&i| {
            let b = &balls[i];
            let rules = NetRules::of_net(board, b.net);
            let via = b.via.as_ref().map(|(v, _)| v);
            let allowed = layers
                .iter()
                .enumerate()
                .filter(|&(_, &l)| {
                    via.is_some_and(|v| v.spans(l))
                        && usize::try_from(l)
                            .ok()
                            .and_then(|x| rules.active.get(x).copied())
                            .unwrap_or(false)
                })
                .fold(0u64, |m, (k, _)| if k < 64 { m | (1 << k) } else { m });
            Demand {
                ring: b.ring,
                band: band_of(b.side, b.position),
                allowed,
            }
        })
        .collect();
    let bands = 2 * (u32::from(array.rows) + u32::from(array.cols));
    for (&i, slot) in signal.iter().zip(assign(&escape_layers, bands, &demands)) {
        balls[i].slot = slot;
        if slot.is_none() {
            balls[i].skip = Some(Skip::NoChannel);
        }
    }
    ArrayPlan {
        layers: escape_layers,
        spacing,
        balls,
    }
}

/// The along-`u` coordinate of `p`.
fn along(side: Side, p: IntPoint) -> i64 {
    let (ux, uy) = side.outward();
    i64::from(p.x) * ux + i64::from(p.y) * uy
}

/// The lateral coordinate of `p` on `side`.
fn lateral(side: Side, p: IntPoint) -> i64 {
    let (lx, ly) = lateral_unit(side);
    i64::from(p.x) * lx + i64::from(p.y) * ly
}

/// The part-frame point at `t` along `u` and `v` across.
fn part_point(side: Side, t: i64, v: i64) -> IntPoint {
    let c = |x: i64| x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let (ux, uy) = side.outward();
    let (lx, ly) = lateral_unit(side);
    IntPoint::new(c(t * ux + v * lx), c(t * uy + v * ly))
}

/// The escape box's side along `u`.
fn edge_of(array: &BallArray, side: Side) -> i64 {
    let b = array.escape_box();
    match side {
        Side::W => -i64::from(b.ll.x),
        Side::S => -i64::from(b.ll.y),
        Side::E => i64::from(b.ur.x),
        Side::N => i64::from(b.ur.y),
    }
}

/// The along-`u` coordinate of the outer ring's balls on `side`.
fn outer_line(array: &BallArray, side: Side) -> i64 {
    let (last_row, last_col) = (i64::from(array.rows) - 1, i64::from(array.cols) - 1);
    let p = match side {
        Side::W | Side::N => array.centre(0, 0),
        Side::E => array.centre(0, last_col),
        Side::S => array.centre(last_row, 0),
    };
    along(side, p)
}

/// The escape of a signal ball as part-frame points: via, jog end, end.
fn escape_points(
    array: &BallArray,
    plan: &ArrayPlan,
    ball: &BallPlan,
    slot: Slot,
) -> Vec<IntPoint> {
    let (row, col) = ball.position;
    let side = ball.side;
    let centre = array.centre(i64::from(row), i64::from(col));
    let (t_b, v_b) = (along(side, centre), lateral(side, centre));
    let half = array.pitch / 2;
    let s = sigma(array, row, col);
    let (step, h, c) = plan.spacing[slot.layer];
    let n = plan.layers[slot.layer].capacity;
    let off = slot_offset(slot.slot, n, step, s);
    let t_v = t_b + half;
    let t_j = t_v + (half - s * off);
    let t_end =
        (edge_of(array, side) + c + h).max(outer_line(array, side) + array.pitch + 2 * half);
    let mut points = vec![
        part_point(side, t_v, v_b + s * half),
        part_point(side, t_j, v_b + off),
    ];
    if t_end > t_j {
        points.push(part_point(side, t_end, v_b + off));
    }
    points
}

/// Why a ball was refused inside the transaction.
#[derive(Debug)]
enum Refusal {
    Insert,
    ViaInPad,
    PinSide,
    NotConnected,
}

/// What one ball's transaction inserts, in board coordinates.
struct BallCopper {
    net: i32,
    class: i32,
    pin: ItemId,
    pad_layer: i32,
    bone: (IntPoint, IntPoint, i32),
    via: (ViaOption, IntPoint),
    escape: Option<(Vec<IntPoint>, i32, i32)>,
}

/// Inserts `ball`'s copper in one transaction (module docs).
fn insert_ball(
    board: &mut RoutingBoard,
    array: &BallArray,
    plan: &ArrayPlan,
    ball: &BallPlan,
) -> BallOutcome {
    let mut outcome = BallOutcome {
        component: array.component,
        pin: ball.pin,
        position: ball.position,
        ring: ball.ring,
        side: ball.side,
        result: BallResult::Skipped(ball.skip.unwrap_or(Skip::Rejected)),
        items: Vec::new(),
    };
    if ball.skip.is_some() {
        return outcome;
    }
    let Some((via, at)) = ball.via.clone() else {
        outcome.result = BallResult::Skipped(Skip::NoViaFits);
        return outcome;
    };
    let start = board
        .item(ball.pin)
        .and_then(Item::drill)
        .map_or(at, |d| d.center);
    let escape = match (ball.plane, ball.slot) {
        (true, _) => None,
        (false, Some(slot)) => {
            let points: Vec<IntPoint> = escape_points(array, plan, ball, slot)
                .into_iter()
                .map(|p| array.frame.to_board(p))
                .collect();
            let points = std::iter::once(at)
                .chain(points.into_iter().skip(1))
                .collect();
            Some((
                points,
                plan.layers[slot.layer].layer,
                ball.half_width[slot.layer],
            ))
        }
        (false, None) => {
            outcome.result = BallResult::Skipped(Skip::NoChannel);
            return outcome;
        }
    };
    let copper = BallCopper {
        net: ball.net,
        class: ball.class,
        pin: ball.pin,
        pad_layer: array.pad_layer,
        bone: (start, at, ball.pad_half_width),
        via: (via, at),
        escape,
    };
    let result = board.try_change(|b| insert_copper(b, &copper), CheckPolicy::NewItemsOnly);
    match result {
        TransactionResult::Committed(items) => {
            outcome.items = items;
            outcome.result = match (ball.plane, ball.slot) {
                (false, Some(slot)) => BallResult::Escaped {
                    layer: plan.layers[slot.layer].layer,
                    slot: slot.slot,
                },
                _ => BallResult::PlaneVia,
            };
        }
        TransactionResult::Reverted { .. } | TransactionResult::Aborted(_) => {
            drop_redo(board);
            outcome.result = BallResult::Skipped(Skip::Rejected);
        }
    }
    outcome
}

/// Inserts one trace; `None` if the board refuses it.
fn insert_trace(
    b: &mut RoutingBoard,
    points: &[IntPoint],
    layer: i32,
    half_width: i32,
    copper: &BallCopper,
) -> Option<ItemId> {
    let mut corners = points.to_vec();
    corners.dedup();
    if corners.len() < 2 {
        return None;
    }
    let points: Vec<Point> = corners.into_iter().map(Point::from).collect();
    b.insert_trace_without_cleaning(
        Polyline::from_points(&points),
        layer,
        half_width,
        &[copper.net],
        copper.class,
        FixedState::Unfixed,
    )
}

/// The body of one ball's transaction: dog-bone, via, escape.
fn insert_copper(b: &mut RoutingBoard, copper: &BallCopper) -> Result<Vec<ItemId>, Refusal> {
    let mut items = Vec::new();
    let (from, to, hw) = copper.bone;
    if from != to {
        items.push(
            insert_trace(b, &[from, to], copper.pad_layer, hw, copper).ok_or(Refusal::Insert)?,
        );
    }
    let (via, at) = &copper.via;
    let info = b.rules.via_infos.get(via.info).clone();
    let id = b.insert_via(
        info.padstack,
        *at,
        &[copper.net],
        info.clearance_class,
        FixedState::Unfixed,
        info.attach_smd_allowed,
    );
    if b.item(id).is_some_and(|v| via_in_pad(b, v)) {
        return Err(Refusal::ViaInPad);
    }
    if !pin_side_violations(b, id).is_empty() {
        return Err(Refusal::PinSide);
    }
    items.push(id);
    if let Some((points, layer, hw)) = &copper.escape {
        items.push(insert_trace(b, points, *layer, *hw, copper).ok_or(Refusal::Insert)?);
    }
    let first = *items.first().ok_or(Refusal::Insert)?;
    let set = b.connected_set(first, copper.net, false);
    if !set.contains(&copper.pin) || !items.iter().all(|i| set.contains(i)) {
        return Err(Refusal::NotConnected);
    }
    Ok(items)
}

/// The clearance violations of the new via `via` that only a nearby pin's
/// check reports (X05's `pin_side_violations`, for one via).
fn pin_side_violations(board: &BasicBoard, via: ItemId) -> Vec<(ItemId, ItemId, i32)> {
    let mut out = Vec::new();
    let Some(drill) = board.item(via).and_then(Item::drill) else {
        return out;
    };
    for layer in drill.first_layer..=drill.last_layer {
        let reach = f64::from(board.rules.clearance_matrix.max_value_on_layer(layer));
        let probe = TileShape::IntBox(drill.bounding_box().offset(reach));
        let pins: BTreeSet<ItemId> = board
            .overlapping_tree_entries(&probe, layer)
            .iter()
            .filter(|e| board.item(e.item).is_some_and(Item::is_pin))
            .map(|e| e.item)
            .collect();
        for pin in pins {
            out.extend(
                board
                    .clearance_violations(pin)
                    .into_iter()
                    .filter(|v| v.first_item == via || v.second_item == via)
                    .map(|v| (v.first_item, v.second_item, v.layer)),
            );
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Drops the redo data a reverted transaction leaves (as X05's inserter).
fn drop_redo(board: &mut RoutingBoard) {
    board.generate_snapshot();
    board.pop_snapshot();
}

//! Escape stubs for QFN, QFP, SOIC and SOP parts (X10.2).
//!
//! Written from the X10 checklist's design. KiCadRoutingTools'
//! `py_router/qfn_fanout` (at `0486e799`) gave the idea of a straight
//! escape along the pad's lead in the part's own frame, and its
//! `_apply_neckdown_widths` the taper widths; no KRT code is ported here
//! (the side classification is, in [`super::frame`]).
//!
//! **Which pins.** Every pin of a qualifying part ([`analyze`]) on a side,
//! components by name, sides W, S, E, N, pins by pin number. A pin is
//! skipped when it spans layers ([`Skip::NotSmd`]), has other than one net,
//! has an ignored net, is the only terminal of its net
//! ([`Skip::Unconnected`]) or already has copper attached
//! ([`Skip::AlreadyConnected`]).
//!
//! **The stub**, in the part frame along the side's outward direction `u`,
//! from the pin centre on the pin's layer. With `edge` the escape box's
//! side (the pin field united with the package outline), `c` the largest
//! clearance of the net's class on the layer and `w` the class half-width:
//! - narrow half-width `n = min(w, neck-down half-width)`, never below the
//!   board's minimum (B05: Java `neckdownHalfWidth`, D8): every caller passes
//!   [`min_trace_half_width`], which is the larger of `router.min_trace_width_um`
//!   and Q18's board minimum (`router.board_minimums.track_width_um`, or the
//!   narrowest class width with `board_minimums.from_classes`). A stub that
//!   keeps no clearance at that width is rejected by its transaction and the
//!   pin gets none (Q21, `tests/pin_escape.rs`);
//! - a pin whose class is wider than `n`, or whose net has a conduction
//!   area on another layer (a plane-net pin), needs room `r`: `w`, or the
//!   radius of its class's via. Its stub runs narrow to
//!   `S = edge + c + r`, pushed past any same-side neighbour stub it would
//!   otherwise face (`|lateral gap| < r + c + n_neighbour`) to that stub's
//!   reach plus `c + r`; then
//!   - a plane-net pin gets its own via at `S` (D8: never in a pad), or
//!   - a wide pin widens over [`TAPER_LENGTH_UM`] in [`TAPER_STEPS`] steps
//!     at half-widths `w + (n − w)·k/5`, `k = 4..1` (Q02.5's taper, which is
//!     not built yet; replace this one with it when it lands), then runs one
//!     more step at `w`;
//! - every other pin runs narrow to `edge + c + w` (the X10 decision).
//!
//! **Insertion.** Each stub goes in one B09 transaction
//! ([`RoutingBoard::try_change`], [`CheckPolicy::NewItemsOnly`]); the
//! transaction also refuses a via in a pad, a via that a nearby pin's check
//! reports (X06's finding: the via's own check misses a same-net pad), and
//! a stub that does not touch its pin. A rejected stub is retried narrow
//! only, ending `edge + o`, with the overshoot `o = c + w` halved each time
//! while it is at least `n`, then exactly at `edge`; if every try is
//! rejected the pin is skipped ([`Skip::Rejected`]).
//!
//! Determinism (D2): integer geometry in the part frame, ordered loops;
//! nothing reads a clock or iterates a hash.

use std::collections::BTreeSet;

use freerouting::autoroute::control::min_trace_half_width;
use freerouting::autoroute::events::CancelToken;
use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::{ComponentId, FixedState};
use freerouting::board::transaction::{CheckPolicy, TransactionResult};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::geometry::{IntBox, IntPoint, Point, Polyline, TileShape};
use freerouting::job::overrides::um_to_board_units;
use freerouting::settings::board::BoardRouterSettings;

use super::frame::{PartLayout, PartPin, Side, analyze};
use super::rules::{NetRules, ViaChoice, via_in_pad};

/// The length of the taper from the narrow to the class width (µm, KRT
/// `_apply_neckdown_widths`).
pub const TAPER_LENGTH_UM: f64 = 500.0;
/// The taper's steps (`k = 4..1` of `w + (n − w)·k/5`).
pub const TAPER_STEPS: i64 = 4;
/// The divisor of the taper widths.
const TAPER_DIVISOR: i64 = 5;

/// What a stub is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StubKind {
    /// One narrow trace.
    Narrow,
    /// Narrow, a taper, then the class width.
    Widened,
    /// Narrow, then a via to the net's plane.
    Via,
}

/// Why a pin got no stub.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Skip {
    /// The pin spans more than one layer.
    NotSmd,
    /// The pin has no net or more than one.
    NetCount,
    /// The pin's net is ignored by the autorouter.
    IgnoredNet,
    /// The pin is the only terminal of its net.
    Unconnected,
    /// Copper is already attached to the pin.
    AlreadyConnected,
    /// Every try was rejected.
    Rejected,
}

/// The result for one pin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinResult {
    /// A stub was inserted: its kind, and which try was kept (0 for the
    /// planned stub; a pin that needs room has its narrow retry at 1).
    Stub {
        /// What was inserted.
        kind: StubKind,
        /// The try that was kept, from 0.
        attempt: u32,
    },
    /// No stub.
    Skipped(Skip),
}

/// One pin's outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinOutcome {
    /// The component.
    pub component: ComponentId,
    /// The pin.
    pub pin: ItemId,
    /// Its side.
    pub side: Side,
    /// The result.
    pub result: PinResult,
    /// The inserted items, in insertion order (traces, then the via).
    pub items: Vec<ItemId>,
}

/// What [`escape`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EscapeReport {
    /// The qualifying parts, by name.
    pub parts: Vec<ComponentId>,
    /// Every side pin of those parts, in stub order.
    pub outcomes: Vec<PinOutcome>,
}

impl EscapeReport {
    /// The number of stubs inserted.
    pub fn stubs(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o.result, PinResult::Stub { .. }))
            .count()
    }

    /// One log line per part and one summary line.
    pub fn log(&self, board: &BasicBoard) -> Vec<String> {
        let mut out = Vec::new();
        for &part in &self.parts {
            let name = board.components.get(part).map_or("?", |c| c.name.as_str());
            let mine: Vec<&PinOutcome> = self
                .outcomes
                .iter()
                .filter(|o| o.component == part)
                .collect();
            let count = |k: StubKind| {
                mine.iter()
                    .filter(|o| matches!(o.result, PinResult::Stub { kind, .. } if kind == k))
                    .count()
            };
            let rejected = mine
                .iter()
                .filter(|o| o.result == PinResult::Skipped(Skip::Rejected))
                .count();
            out.push(format!(
                "krt-qfn-escape {name}: {} side pins, {} narrow, {} widened, {} via stubs, {} rejected",
                mine.len(),
                count(StubKind::Narrow),
                count(StubKind::Widened),
                count(StubKind::Via),
                rejected
            ));
        }
        out.push(format!(
            "krt-qfn-escape: {} parts, {} stubs",
            self.parts.len(),
            self.stubs()
        ));
        out
    }
}

/// The qualifying parts of `board`, by component name.
pub fn qualifying_parts(board: &BasicBoard) -> Vec<PartLayout> {
    let mut ids: Vec<(String, ComponentId)> = board
        .components
        .iter()
        .map(|c| (c.name.clone(), c.id))
        .collect();
    ids.sort();
    ids.into_iter()
        .filter_map(|(_, id)| analyze(board, id))
        .collect()
}

/// Inserts the stubs of every qualifying part of `board` (module docs).
/// A router stop requested on `cancel` ends the run between parts.
pub fn escape(
    board: &mut RoutingBoard,
    settings: &BoardRouterSettings,
    cancel: &CancelToken,
) -> EscapeReport {
    let parts = qualifying_parts(board);
    let mut report = EscapeReport {
        parts: parts.iter().map(|p| p.component).collect(),
        outcomes: Vec::new(),
    };
    let min_half_width = min_trace_half_width(board, settings).max(1);
    for part in &parts {
        if cancel.is_router_stop_requested() {
            break;
        }
        for side in Side::ALL {
            let plans = plan_side(board, part, side, min_half_width);
            for plan in plans {
                let outcome = insert_pin(board, part, side, plan);
                report.outcomes.push(outcome);
            }
        }
    }
    report
}

/// A pin's rules and position, or why it is skipped.
#[derive(Clone, Debug)]
struct PinPlan {
    pin: ItemId,
    skip: Option<Skip>,
    net: i32,
    layer: i32,
    class: i32,
    start: IntPoint,
    t0: i64,
    lateral: i64,
    narrow: i32,
    wide: i32,
    clearance: i64,
    via: Option<ViaChoice>,
    /// Where the narrow part ends for a pin that needs room (`S`).
    room_at: Option<i64>,
}

impl PinPlan {
    /// The room past the narrow part: the via radius or the class
    /// half-width; 0 for a narrow stub.
    fn room(&self) -> i64 {
        if let Some(via) = &self.via {
            let r = usize::try_from(self.layer)
                .ok()
                .and_then(|l| via.radius.get(l).copied())
                .unwrap_or(0);
            return i64::from(r.max(self.narrow));
        }
        if self.wide > self.narrow {
            i64::from(self.wide)
        } else {
            0
        }
    }

    /// The end of the planned narrow stub (`edge + c + w`).
    fn narrow_end(&self, edge: i64) -> i64 {
        edge + self.clearance + i64::from(self.wide)
    }
}

/// The along-`u` coordinate of `p`.
fn along(side: Side, p: IntPoint) -> i64 {
    let (ux, uy) = side.outward();
    i64::from(p.x) * ux + i64::from(p.y) * uy
}

/// The lateral coordinate of `p` on `side`.
fn lateral(side: Side, p: IntPoint) -> i64 {
    match side {
        Side::W | Side::E => i64::from(p.y),
        Side::S | Side::N => i64::from(p.x),
    }
}

/// The part-frame point at `t` along `u` and `lat` across.
fn part_point(side: Side, t: i64, lat: i64) -> IntPoint {
    let c = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    match side {
        Side::W => IntPoint::new(c(-t), c(lat)),
        Side::S => IntPoint::new(c(lat), c(-t)),
        Side::E => IntPoint::new(c(t), c(lat)),
        Side::N => IntPoint::new(c(lat), c(t)),
    }
}

/// The escape box's side along `u`.
fn edge_of(b: &IntBox, side: Side) -> i64 {
    match side {
        Side::W => -i64::from(b.ll.x),
        Side::S => -i64::from(b.ll.y),
        Side::E => i64::from(b.ur.x),
        Side::N => i64::from(b.ur.y),
    }
}

/// The largest clearance of class `class` on `layer`.
fn max_clearance(board: &BasicBoard, class: i32, layer: i32) -> i64 {
    let n = board.rules.clearance_matrix.class_count() as i32;
    (0..n)
        .map(|k| i64::from(board.clearance_value(class, k, layer)))
        .max()
        .unwrap_or(0)
}

/// True if net `net` has a conduction area on a layer other than `layer`.
fn has_plane_off(board: &BasicBoard, net: i32, layer: i32) -> bool {
    board.conduction_areas().into_iter().any(|id| {
        board
            .item(id)
            .is_some_and(|a| a.contains_net(net) && a.first_layer() != layer)
    })
}

/// The plan of one pin before the side's room pass.
fn plan_pin(board: &BasicBoard, pin: &PartPin, side: Side, min_half_width: i32) -> PinPlan {
    let item = board.item(pin.id);
    let mut plan = PinPlan {
        pin: pin.id,
        skip: None,
        net: 0,
        layer: 0,
        class: 1,
        start: item
            .and_then(Item::drill)
            .map_or(IntPoint::new(0, 0), |d| d.center),
        t0: along(side, pin.centre),
        lateral: lateral(side, pin.centre),
        narrow: 0,
        wide: 0,
        clearance: 0,
        via: None,
        room_at: None,
    };
    let Some(item) = item else {
        plan.skip = Some(Skip::NotSmd);
        return plan;
    };
    plan.skip = skip_reason(board, item);
    if plan.skip.is_some() {
        return plan;
    }
    let net = item.net_number(0);
    let layer = item.first_layer();
    let rules = NetRules::of_net(board, net);
    plan.net = net;
    plan.layer = layer;
    plan.class = rules.trace_class;
    plan.wide = rules.half_width_on(layer).max(min_half_width);
    let neck = board
        .trace_neckdown_half_width(pin.id, layer)
        .max(min_half_width);
    plan.narrow = plan.wide.min(neck);
    plan.clearance = max_clearance(board, rules.trace_class, layer);
    if has_plane_off(board, net, layer) {
        plan.via = rules
            .via
            .filter(|v| v.from_layer <= layer && v.to_layer >= layer);
    }
    plan
}

/// Why `item` gets no stub, if it doesn't.
fn skip_reason(board: &BasicBoard, item: &Item) -> Option<Skip> {
    if item.first_layer() != item.last_layer() {
        return Some(Skip::NotSmd);
    }
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

/// The plans of one side's pins, in pin-number order, with the room
/// positions of the pins that need room (module docs).
fn plan_side(
    board: &BasicBoard,
    part: &PartLayout,
    side: Side,
    min_half_width: i32,
) -> Vec<PinPlan> {
    let edge = edge_of(&part.escape_box(), side);
    let mut plans: Vec<PinPlan> = part.sides[side.index()]
        .iter()
        .filter_map(|id| part.pin(*id))
        .map(|pin| plan_pin(board, pin, side, min_half_width))
        .collect();
    // The narrow stubs of the side: lateral position, reach along `u`
    // (end plus half-width), clearance and half-width.
    let narrow: Vec<(i64, i64, i64, i64)> = plans
        .iter()
        .filter(|p| p.skip.is_none() && p.room() == 0)
        .map(|p| {
            let n = i64::from(p.narrow);
            (p.lateral, p.narrow_end(edge) + n, p.clearance, n)
        })
        .collect();
    for plan in plans.iter_mut().filter(|p| p.skip.is_none()) {
        let r = plan.room();
        if r == 0 {
            continue;
        }
        let mut at = edge + plan.clearance + r;
        for &(lat, reach, cl, n) in &narrow {
            let c = plan.clearance.max(cl);
            if (plan.lateral - lat).abs() < r + c + n {
                at = at.max(reach + c + r);
            }
        }
        plan.room_at = Some(at);
    }
    plans
}

/// One try: the traces as `(from, to, half-width)` in the part frame
/// along `u`, and the via position.
struct Try {
    kind: StubKind,
    traces: Vec<(i64, i64, i32)>,
    via: Option<i64>,
}

/// The tries of `plan`, the planned stub first (module docs).
fn tries(board: &BasicBoard, plan: &PinPlan, edge: i64) -> Vec<Try> {
    let mut out = Vec::new();
    let (t0, n, w) = (plan.t0, plan.narrow, plan.wide);
    if let Some(at) = plan.room_at {
        if plan.via.is_some() {
            out.push(Try {
                kind: StubKind::Via,
                traces: vec![(t0, at, n)],
                via: Some(at),
            });
        } else {
            let length = i64::from(um_to_board_units(board, TAPER_LENGTH_UM));
            let step = (length / TAPER_STEPS).max(1);
            let mut traces = vec![(t0, at, n)];
            for j in 0..TAPER_STEPS {
                let k = TAPER_STEPS - j;
                let hw = i64::from(w) - (i64::from(w) - i64::from(n)) * k / TAPER_DIVISOR;
                let hw = i32::try_from(hw).unwrap_or(w);
                traces.push((at + j * step, at + (j + 1) * step, hw));
            }
            let end = at + TAPER_STEPS * step;
            traces.push((end, end + step, w));
            out.push(Try {
                kind: StubKind::Widened,
                traces,
                via: None,
            });
        }
    }
    let mut o = plan.clearance + i64::from(w);
    out.push(narrow_try(t0, edge + o, n));
    loop {
        o /= 2;
        if o < i64::from(n) {
            break;
        }
        out.push(narrow_try(t0, edge + o, n));
    }
    if edge > t0 {
        out.push(narrow_try(t0, edge, n));
    }
    out
}

fn narrow_try(t0: i64, end: i64, n: i32) -> Try {
    Try {
        kind: StubKind::Narrow,
        traces: vec![(t0, end, n)],
        via: None,
    }
}

/// Why a try was refused inside the transaction.
#[derive(Debug)]
enum Refusal {
    Insert,
    ViaInPad,
    PinSide,
    NotConnected,
}

/// Inserts the first try of `plan` the transaction keeps.
fn insert_pin(
    board: &mut RoutingBoard,
    part: &PartLayout,
    side: Side,
    plan: PinPlan,
) -> PinOutcome {
    let mut outcome = PinOutcome {
        component: part.component,
        pin: plan.pin,
        side,
        result: PinResult::Skipped(plan.skip.unwrap_or(Skip::Rejected)),
        items: Vec::new(),
    };
    if plan.skip.is_some() {
        return outcome;
    }
    let edge = edge_of(&part.escape_box(), side);
    for (attempt, t) in tries(board, &plan, edge).into_iter().enumerate() {
        let points = |from: i64, to: i64| -> (IntPoint, IntPoint) {
            let a = if from == plan.t0 {
                plan.start
            } else {
                part.frame.to_board(part_point(side, from, plan.lateral))
            };
            let b = part.frame.to_board(part_point(side, to, plan.lateral));
            (a, b)
        };
        let traces: Vec<(IntPoint, IntPoint, i32)> = t
            .traces
            .iter()
            .map(|&(f, to, hw)| {
                let (a, b) = points(f, to);
                (a, b, hw)
            })
            .collect();
        let via = t
            .via
            .map(|at| part.frame.to_board(part_point(side, at, plan.lateral)));
        let result = board.try_change(
            |b| insert_try(b, &plan, &traces, via),
            CheckPolicy::NewItemsOnly,
        );
        match result {
            TransactionResult::Committed(items) => {
                outcome.result = PinResult::Stub {
                    kind: t.kind,
                    attempt: attempt as u32,
                };
                outcome.items = items;
                return outcome;
            }
            TransactionResult::Reverted { .. } | TransactionResult::Aborted(_) => {
                drop_redo(board);
            }
        }
    }
    outcome.result = PinResult::Skipped(Skip::Rejected);
    outcome
}

/// The body of one try's transaction.
fn insert_try(
    b: &mut RoutingBoard,
    plan: &PinPlan,
    traces: &[(IntPoint, IntPoint, i32)],
    via: Option<IntPoint>,
) -> Result<Vec<ItemId>, Refusal> {
    let mut items = Vec::new();
    for &(from, to, hw) in traces {
        if from == to {
            continue;
        }
        let polyline = Polyline::from_points(&[Point::from(from), Point::from(to)]);
        let id = b
            .insert_trace_without_cleaning(
                polyline,
                plan.layer,
                hw,
                &[plan.net],
                plan.class,
                FixedState::Unfixed,
            )
            .ok_or(Refusal::Insert)?;
        items.push(id);
    }
    if let (Some(at), Some(choice)) = (via, &plan.via) {
        let info = b.rules.via_infos.get(choice.info).clone();
        let id = b.insert_via(
            info.padstack,
            at,
            &[plan.net],
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
    }
    let first = *items.first().ok_or(Refusal::Insert)?;
    if !b.connected_set(first, plan.net, false).contains(&plan.pin) {
        return Err(Refusal::NotConnected);
    }
    Ok(items)
}

/// The clearance violations of the new via `via` that only a nearby pin's
/// check reports (X05's `pin_side_violations`, for one via: a same-net
/// via with attach off must keep its clearance from the pads of its net).
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

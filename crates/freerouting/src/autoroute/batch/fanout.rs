//! The fanout stage (A12.2, A12.3): `freerouting-fanout`.
//!
//! Port of `pipeline/BatchFanout.java` at `a773a70ec`: `fanoutBoard`
//! (`:93-172`) is [`FreeroutingFanout::run`] and [`FanoutRun::run`], and
//! `fanoutPass` (`:175-480`) is [`FanoutRun::pass`]. One pin is
//! [`fanout_pin`](crate::autoroute::fanout_pin::fanout_pin).
//!
//! **One pass, in Java's order**, per component and pin in A10.6's order
//! ([`fanout_components`]):
//! - stop when `fanout.max_items` pins were fanned out (when positive);
//! - the pin's [`WorkLimit`] of `max_milliseconds_per_pin * (pass + 1)`
//!   nominal ms on the board's meter, created before the skips;
//! - skip a pin with an ignored net; read the pin's unconnected set (Java's
//!   `targetCount`, a log value whose contact queries still count work
//!   against the pin's limit); skip a pin whose net class has no vias
//!   unless `fanout.fallback_to_board_vias` holds and the board's first via
//!   rule has vias; skip a pin that failed in its component's current
//!   generation ([`retry_fanout`]);
//! - the id watermark, `start_marking_changed_area`, then
//!   [`fanout_pin`] with the ripup cost `start_ripup_costs * (pass + 1)`, or
//!   -1 when `fanout.ripup_allowed` is off;
//! - on `Routed`, [`enforce_strict_drc`] **always** (A11.3), whatever the
//!   strict-DRC setting: a rejected escape becomes `Failed`;
//! - `Routed` counts, adds a fanned-out item and bumps the component's
//!   generation; `Failed` and `InsertError` count, add a fanned-out item and
//!   record the pin's generation; `AlreadyConnected` counts;
//! - after each pin, the fanout deadline ends the pass (and the run, timed
//!   out) and a router stop ends the pass.
//!
//! **The passes** ([`FanoutRun::run`]): at most `fanout.max_passes`, each
//! started unless the deadline passed or `max_items` was reached, stopped
//! by [`FanoutPassTracker`] after a pass that routed nothing, after
//! [`FANOUT_STAGNATION_PASS_LIMIT`](super::fanout_rules::FANOUT_STAGNATION_PASS_LIMIT)
//! identical outcomes, on a timeout or when the board's [`state_hash`]
//! did not change. The summary's escape counts come from
//! [`BoardStatistics`].
//!
//! **Generations live in `BTreeMap`s.** Java's `failedPinGeneration` and
//! `componentGeneration` are `HashMap`s read by key only, so sorted maps
//! change nothing.
//!
//! **Time.** `fanout.timeout` is read once at the start of the run as an
//! [`Instant`] deadline that may only end work (D2); the per-pin budget is
//! work. A [`Progress`] goes out after each pin and at the end of each pass
//! while someone listens, with the pass's counters (`failed` is the not
//! routed plus the insert errors); Java's throttling to one a second is
//! not ported.
//!
//! Not ported: Java's trace and info logging, the `alreadyConnectedPinCount`
//! of the constructor (a log value) and the pass durations.
//!
//! [`state_hash`]: crate::board::BasicBoard::state_hash

use std::collections::BTreeMap;
use std::time::Instant;

use super::fanout_rules::{
    FanoutComponent, FanoutPassTracker, FanoutStop, PinSortingOrder, fanout_components,
    retry_fanout,
};
use super::strict_drc::enforce_strict_drc;
use crate::autoroute::events::{Phase, Progress};
use crate::autoroute::fanout_pin::fanout_pin;
use crate::autoroute::stages::{FanoutStage, FanoutSummary, StageCtx};
use crate::autoroute::{AttemptResult, AttemptState};
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::search::WorkLimit;
use crate::datastructures::IdGenerator;
use crate::scoring::BoardStatistics;

/// The site name of the per-pin limit in a `WORK_LIMIT_CUTOFF` line.
pub const FANOUT_PIN_SITE: &str = "fanout_pin";

/// Why a pin of a pass was not fanned out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinSkip {
    /// The pin has an ignored net.
    IgnoredNet,
    /// Its net class has no vias and the fallback is off or unavailable.
    NoVias,
    /// It failed in its component's current generation.
    NoRetry,
}

impl PinSkip {
    /// The probe's name of the skip.
    pub fn name(self) -> &'static str {
        match self {
            PinSkip::IgnoredNet => "ignored_net",
            PinSkip::NoVias => "no_vias",
            PinSkip::NoRetry => "no_retry",
        }
    }
}

/// What a fanned-out pin gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinOutcome {
    /// The result after the strict DRC.
    pub result: AttemptResult,
    /// True if the strict DRC turned a `Routed` into a `Failed`.
    pub strict_rejected: bool,
    /// `maxGeneratedId` before the pin.
    pub max_id_before: i32,
}

/// The counters of one pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FanoutPassReport {
    /// The pass number, from 0 (Java's `passNo`).
    pub pass: u32,
    /// Pins routed.
    pub routed: i32,
    /// Pins that failed.
    pub not_routed: i32,
    /// Pins whose insertion failed.
    pub insert_errors: i32,
    /// Pins that were already connected.
    pub already_connected: i32,
    /// Vias added in the pass (never negative).
    pub extra_vias: usize,
    /// True if the deadline or a router stop ended the pass early.
    pub interrupted: bool,
}

/// Sees every pin of a fanout pass (the `fanout` probe, diagnostics). The
/// default methods do nothing.
pub trait FanoutObserver {
    /// Before pass `pass` (from 0).
    fn on_pass(&mut self, _board: &RoutingBoard, _pass: u32) {}
    /// Before the pins of `component` are visited.
    fn on_component(&mut self, _board: &RoutingBoard, _component: &FanoutComponent) {}
    /// Before the pin's limit is created.
    fn before_pin(&mut self, _board: &RoutingBoard, _component: i32, _pin: ItemId) {}
    /// The pin was skipped.
    fn skipped(&mut self, _board: &RoutingBoard, _component: i32, _pin: ItemId, _why: PinSkip) {}
    /// The pin was fanned out (after the strict DRC).
    fn after_pin(
        &mut self,
        _board: &RoutingBoard,
        _component: i32,
        _pin: ItemId,
        _outcome: &PinOutcome,
    ) {
    }
    /// After the pass.
    fn after_pass(&mut self, _board: &RoutingBoard, _report: &FanoutPassReport) {}
}

/// A [`FanoutObserver`] that sees nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoFanoutObserver;

impl FanoutObserver for NoFanoutObserver {}

/// The state of one fanout run (Java's `BatchFanout` instance).
#[derive(Clone, Debug)]
pub struct FanoutRun {
    components: Vec<FanoutComponent>,
    total_smd_pins: usize,
    /// Pin id to the component generation when it failed.
    failed_generation: BTreeMap<ItemId, i32>,
    /// Component id to its successful escapes.
    component_generation: BTreeMap<i32, i32>,
    /// Pins fanned out so far (Java `totalItemsFanouted`).
    total_items_fanouted: i32,
    deadline: Option<Instant>,
    timed_out: bool,
}

impl FanoutRun {
    /// Java's constructor: the components of `board` in fanout order, the
    /// deadline of `fanout.timeout` from now.
    pub fn new(board: &RoutingBoard, ctx: &StageCtx) -> FanoutRun {
        let order = PinSortingOrder::from_name(&ctx.settings.fanout.pin_sorting_order);
        let components = fanout_components(board, order);
        let total_smd_pins = components.iter().map(FanoutComponent::smd_pin_count).sum();
        let deadline = ctx
            .settings
            .deadlines()
            .fanout
            .and_then(|d| Instant::now().checked_add(d));
        FanoutRun {
            components,
            total_smd_pins,
            failed_generation: BTreeMap::new(),
            component_generation: BTreeMap::new(),
            total_items_fanouted: 0,
            deadline,
            timed_out: false,
        }
    }

    /// The components in fanout order.
    pub fn components(&self) -> &[FanoutComponent] {
        &self.components
    }

    /// Pins fanned out so far.
    pub fn total_items_fanouted(&self) -> i32 {
        self.total_items_fanouted
    }

    /// True if the deadline stopped the run.
    pub fn timed_out(&self) -> bool {
        self.timed_out
    }

    fn deadline_passed(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }

    fn max_items_reached(&self, ctx: &StageCtx) -> bool {
        let max_items = ctx.settings.fanout.max_items;
        max_items > 0 && self.total_items_fanouted >= max_items
    }

    /// Java `fanoutBoard`'s pass loop: the passes, then the summary.
    pub fn run(
        &mut self,
        board: &mut RoutingBoard,
        ctx: &mut StageCtx,
        observer: &mut dyn FanoutObserver,
    ) -> FanoutSummary {
        let max_passes = u32::try_from(ctx.settings.fanout.max_passes).unwrap_or(0);
        let mut tracker = FanoutPassTracker::new(board.state_hash());
        let mut completed = 0;
        for pass in 0..max_passes {
            let before = tracker.before_pass(
                self.deadline_passed(),
                self.total_items_fanouted,
                ctx.settings.fanout.max_items,
            );
            if let Some(stop) = before {
                if stop == FanoutStop::Deadline {
                    self.timed_out = true;
                }
                break;
            }
            let report = self.pass(board, pass, ctx, observer);
            completed += 1;
            let vias = board.vias().len();
            let hash = board.state_hash();
            if tracker
                .after_pass(report.routed, vias, self.timed_out, hash)
                .is_some()
            {
                break;
            }
        }
        let stats = BoardStatistics::new(board);
        FanoutSummary {
            completed_passes: completed,
            escaped: stats.fanout.escaped_count,
            total: stats.fanout.total_smd_pins,
            timed_out: self.timed_out,
        }
    }

    /// Java `fanoutPass(passNo)`; the report's `routed` is its result.
    pub fn pass(
        &mut self,
        board: &mut RoutingBoard,
        pass: u32,
        ctx: &mut StageCtx,
        observer: &mut dyn FanoutObserver,
    ) -> FanoutPassReport {
        let mut report = FanoutPassReport {
            pass,
            ..FanoutPassReport::default()
        };
        let mut pins_to_go = self.total_smd_pins;
        let vias_before = board.vias().len();
        let factor = i64::from(pass) + 1;
        let ripup_cost = ctx
            .board_settings
            .start_ripup_costs()
            .wrapping_mul(factor as i32);
        let effective_ripup = if ctx.settings.fanout.ripup_allowed {
            ripup_cost
        } else {
            -1
        };
        // PARITY: :237, `double maxMilliseconds = base * (passNo + 1)`, then
        // `(long)`.
        let budget_ms =
            (ctx.settings.fanout.max_milliseconds_per_pin as f64 * factor as f64) as i64;
        observer.on_pass(board, pass);
        self.progress(board, ctx, &report, pins_to_go, true);
        let components = self.components.clone();
        'components: for component in &components {
            observer.on_component(board, component);
            for pin in &component.pins {
                if self.max_items_reached(ctx) {
                    break 'components;
                }
                observer.before_pin(board, component.component, pin.pin);
                let limit = WorkLimit::with_site(
                    board.search_trees().work_meter(),
                    budget_ms,
                    FANOUT_PIN_SITE,
                );
                let skip = self.skip_reason(board, component.component, pin.pin, ctx);
                if let Some(why) = skip {
                    pins_to_go = pins_to_go.saturating_sub(1);
                    observer.skipped(board, component.component, pin.pin, why);
                    continue;
                }
                let outcome = fanout_one(board, pin.pin, effective_ripup, &limit, ctx);
                self.count(&mut report, component.component, pin.pin, &outcome.result);
                observer.after_pin(board, component.component, pin.pin, &outcome);
                pins_to_go = pins_to_go.saturating_sub(1);
                report.extra_vias = board.vias().len().saturating_sub(vias_before);
                self.progress(board, ctx, &report, pins_to_go, false);
                if self.deadline_passed() {
                    self.timed_out = true;
                    report.interrupted = true;
                    break 'components;
                }
                if ctx.cancel.is_router_stop_requested() {
                    report.interrupted = true;
                    break 'components;
                }
            }
        }
        report.extra_vias = board.vias().len().saturating_sub(vias_before);
        self.progress(board, ctx, &report, pins_to_go, true);
        observer.after_pass(board, &report);
        report
    }

    /// The skips of `:255-319` that follow the limit's creation; the
    /// ignored-net check first, then the unconnected set is read (its
    /// contact queries count against the limit), the via check and the
    /// retry rule.
    fn skip_reason(
        &self,
        board: &RoutingBoard,
        component: i32,
        pin: ItemId,
        ctx: &StageCtx,
    ) -> Option<PinSkip> {
        let item = board.item(pin)?;
        if board.has_ignored_nets(item) {
            return Some(PinSkip::IgnoredNet);
        }
        let net = item.net_number(0);
        // PARITY: :264, `targetCount`, read for the log only.
        let _ = board.unconnected_set(pin, net).len();
        if let Some(rule_net) = board.rules.nets.get(net) {
            let class = board.rules.net_classes.get(rule_net.net_class());
            let class_vias = class
                .via_rule()
                .is_some_and(|r| board.rules.via_rules.get(r).via_count() > 0);
            let board_vias = board
                .rules
                .via_rules
                .nth(0)
                .is_some_and(|r| board.rules.via_rules.get(r).via_count() > 0);
            let fallback = ctx.settings.fanout.fallback_to_board_vias && board_vias;
            if !class_vias && !fallback {
                return Some(PinSkip::NoVias);
            }
        }
        let generation = self.generation(component);
        if !retry_fanout(self.failed_generation.get(&pin).copied(), generation) {
            return Some(PinSkip::NoRetry);
        }
        None
    }

    fn generation(&self, component: i32) -> i32 {
        self.component_generation
            .get(&component)
            .copied()
            .unwrap_or(0)
    }

    /// The counting of `:358-450`.
    fn count(
        &mut self,
        report: &mut FanoutPassReport,
        component: i32,
        pin: ItemId,
        result: &AttemptResult,
    ) {
        match result.state {
            AttemptState::Routed => {
                report.routed += 1;
                self.total_items_fanouted += 1;
                *self.component_generation.entry(component).or_insert(0) += 1;
            }
            AttemptState::AlreadyConnected => report.already_connected += 1,
            AttemptState::Failed | AttemptState::InsertError => {
                if result.state == AttemptState::Failed {
                    report.not_routed += 1;
                } else {
                    report.insert_errors += 1;
                }
                self.total_items_fanouted += 1;
                let generation = self.generation(component);
                self.failed_generation.insert(pin, generation);
            }
            _ => {}
        }
    }

    /// A [`Progress`] of the pass, while someone listens.
    fn progress(
        &self,
        board: &RoutingBoard,
        ctx: &StageCtx,
        report: &FanoutPassReport,
        pins_to_go: usize,
        statistics: bool,
    ) {
        if !ctx.events.is_listening() {
            return;
        }
        let mut progress = Progress {
            phase: Phase::Fanout,
            pass: i32::try_from(report.pass + 1).unwrap_or(i32::MAX),
            items_to_go: pins_to_go,
            routed: report.routed.max(0) as usize,
            skipped: report.already_connected.max(0) as usize,
            failed: (report.not_routed + report.insert_errors).max(0) as usize,
            extra_vias: report.extra_vias,
            ..Progress::default()
        };
        if statistics {
            let stats = BoardStatistics::new(board);
            progress.incomplete = stats.connections.incomplete_count;
            progress.violations = stats.clearance_violations.total_count;
            progress.score = stats.router_score(Some(ctx.settings));
        }
        ctx.events.progress(progress);
    }
}

/// One pin: the watermark, the changed area, [`fanout_pin`] and the strict
/// DRC (`:325-356`).
fn fanout_one(
    board: &mut RoutingBoard,
    pin: ItemId,
    ripup_cost: i32,
    limit: &WorkLimit,
    ctx: &StageCtx,
) -> PinOutcome {
    let net = board.item(pin).map_or(0, |item| item.net_number(0));
    let max_id_before = board.communication.id_generator.max_generated_id();
    board.start_marking_changed_area();
    let mut result = fanout_pin(board, pin, ripup_cost, limit, ctx);
    let mut strict_rejected = false;
    if result.state == AttemptState::Routed
        && let Some(rejection) = enforce_strict_drc(board, net, max_id_before)
    {
        result = rejection;
        strict_rejected = true;
    }
    PinOutcome {
        result,
        strict_rejected,
        max_id_before,
    }
}

/// Java's fanout stage (`freerouting-fanout`): `BatchFanout.fanoutBoard`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreeroutingFanout;

impl FreeroutingFanout {
    /// [`FanoutStage::run`] with an observer.
    pub fn run_observed(
        &mut self,
        board: &mut RoutingBoard,
        ctx: &mut StageCtx,
        observer: &mut dyn FanoutObserver,
    ) -> FanoutSummary {
        let mut run = FanoutRun::new(board, ctx);
        run.run(board, ctx, observer)
    }
}

impl FanoutStage for FreeroutingFanout {
    fn id(&self) -> &'static str {
        "freerouting-fanout"
    }

    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> FanoutSummary {
        self.run_observed(board, ctx, &mut NoFanoutObserver)
    }
}

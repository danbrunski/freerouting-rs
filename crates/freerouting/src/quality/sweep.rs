//! Q10: the add-only completion sweep (`router.autorouter.sweep`).
//!
//! On `canary-pwr` most connections left incomplete after pass 1 are
//! routable on the final board with rip-up off (Q06.5: 83 of 109), and
//! Java's best result there comes from a second run on the frozen pass-1
//! wiring. The sweep does that inside one job: after the batch loop's finish
//! (the best-board restore and the final tail removal, A13), it runs Java's
//! pass again on the board the loop would output, with rip-up off, so it can
//! only add copper. Altium Situs' "Completion" pass is the same idea.
//!
//! **One sweep pass** is [`PassRunner`]'s pass with the job's own stages:
//! the same ordering stage (reset first, as a restore resets it), the same
//! connection router and ripup schedule, the same work budgets. Two wrappers
//! make it add-only:
//! - [`ConnectionParams::ripup_allowed`] is false for every connection, so
//!   the maze search never rips;
//! - the ripup policy's repeated-failure ripup ([`RipupPolicy::after_failure`],
//!   which removes a failing net's wiring) does nothing.
//!
//! Sweep passes are numbered from 1, as in Java's second run: pass 1 routes
//! in the ordering's `select` order and pass 2 reorders, the strict check and
//! the maze budget are those of the pass number. Pull-tight and the shover
//! still move existing wiring unless it is frozen.
//!
//! **The controller** ([`SweepController`]) repeats sweep passes until one
//! routes nothing, the board is complete, or `sweep_passes` (default
//! [`DEFAULT_SWEEP_PASSES`]) is reached. Each pass is kept only if the board
//! after it has no more incomplete connections and no more clearance
//! violations than before; otherwise, and when a pass routed nothing, the
//! board before the pass comes back (a `Clone`, so the search tree keeps its
//! shape and the work counts after it are those of no pass at all). Q04's
//! quantum rule can replace the cap later.
//!
//! **Arms** ([`SweepMode`]):
//! - `fixed`: every trace and via that is not user-fixed is marked
//!   user-fixed for the sweep and given its state back afterwards
//!   ([`Frozen`]), which reproduces Java's two-run flow (`dsn_export
//!   --fix-existing`, or a session loaded with the design, whose wires come
//!   in fixed); a piece that a sweep connection split off a frozen trace
//!   takes the state its vanished parent had when that is unambiguous, and
//!   is unfixed otherwise;
//! - `free`: nothing is frozen, so pull-tight and the shover inside the
//!   sweep may move earlier wiring.
//!
//! **Where it runs.** With `sweep_after_optimizer` off (the default), after
//! the batch loop's finish ([`after_loop`], called by `run_batch_loop`).
//! With it on and the optimizer enabled, after a first optimizer run, and
//! the pipeline's optimizer then runs again ([`after_optimizer`], called by
//! `run_pipeline`; Q10.3). With the optimizer off the setting changes
//! nothing.
//!
//! **Stops.** The sweep does not run after a requested stop (the user's, a
//! timeout, `max_items`): the loop's [`StopReason::StopRequested`] or a full
//! stop. The batch loop itself requests a router stop when it ends on its
//! pass limit or stagnation, so the sweep routes with its own
//! [`CancelToken`]; a full stop of the job (the deadline, a cancel) reaches
//! it before every connection, and a full stop the sweep requests
//! (`max_items`) goes back to the job's token. As for the optimizer, a
//! router-only stop of the user is not seen once the loop has ended.
//!
//! Off by default; with `router.autorouter.sweep` unset or `off` nothing
//! here runs and every result is Java's. Not consulted: the AfterPass board
//! stages (they run after the loop's passes only).

use std::collections::{BTreeMap, BTreeSet};

use crate::autoroute::batch::batch_loop::{LoopOutcome, LoopStages, PassExecutor};
use crate::autoroute::batch::pass::{PassOutcome, PassStages};
use crate::autoroute::batch::snapshot::BoardFigures;
use crate::autoroute::events::{CancelToken, Phase};
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, ConnectionRouter, Optimizer, RipupPolicy, RouteTask,
    StageCtx, StopReason, UnknownStage,
};
use crate::board::RoutingBoard;
use crate::board::items::{ItemId, StopConnectionOption};
use crate::board::search::audit::AuditPoint;
use crate::board::structure::FixedState;
use crate::job::deadline::{StopLevel, Watcher};
use crate::job::{LogLevel, LogLine};
use crate::settings::RouterSettings;

/// The settings key of the mode, as the CLI and the JSON file name it.
pub const SETTINGS_KEY: &str = "router.autorouter.sweep";

/// The settings key of the pass cap.
pub const PASSES_KEY: &str = "router.autorouter.sweep_passes";

/// The most sweep passes when `sweep_passes` is unset.
pub const DEFAULT_SWEEP_PASSES: u32 = 2;

/// The mode values, in the order the error message names them.
pub const MODES: [&str; 3] = ["off", "fixed", "free"];

/// What the sweep does with the wiring it starts from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SweepMode {
    /// No sweep (Java).
    #[default]
    Off,
    /// The earlier wiring is user-fixed during the sweep.
    Fixed,
    /// The earlier wiring stays as it is, so the sweep may move it.
    Free,
}

impl SweepMode {
    /// The mode of a setting value; unset is [`SweepMode::Off`]. Fails on
    /// any other value, naming it.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim) {
            None | Some("") | Some("off") => Ok(SweepMode::Off),
            Some("fixed") => Ok(SweepMode::Fixed),
            Some("free") => Ok(SweepMode::Free),
            Some(other) => Err(format!(
                "{SETTINGS_KEY}: unknown value '{other}' (expected 'off', 'fixed' or 'free')"
            )),
        }
    }

    /// The setting value of the mode.
    pub fn as_str(self) -> &'static str {
        match self {
            SweepMode::Off => "off",
            SweepMode::Fixed => "fixed",
            SweepMode::Free => "free",
        }
    }
}

/// The three sweep settings, resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SweepConfig {
    /// `router.autorouter.sweep`.
    pub mode: SweepMode,
    /// `router.autorouter.sweep_passes`, unset [`DEFAULT_SWEEP_PASSES`].
    pub passes: u32,
    /// `router.autorouter.sweep_after_optimizer`, unset false.
    pub after_optimizer: bool,
}

impl SweepConfig {
    /// The configuration of `settings`; fails on an unknown mode or a
    /// negative pass cap, naming the key.
    pub fn from_settings(settings: &RouterSettings) -> Result<Self, String> {
        let mode = SweepMode::parse(settings.autorouter.sweep.as_deref())?;
        let passes = match settings.autorouter.sweep_passes {
            None => DEFAULT_SWEEP_PASSES,
            Some(n) => u32::try_from(n)
                .map_err(|_| format!("{PASSES_KEY}: {n} is negative (expected 0 or more)"))?,
        };
        Ok(SweepConfig {
            mode,
            passes,
            after_optimizer: settings.autorouter.sweep_after_optimizer == Some(true),
        })
    }

    /// The configuration of `settings`, off if they are invalid
    /// ([`check_settings`] refuses those before routing).
    pub fn of(settings: &RouterSettings) -> Self {
        SweepConfig::from_settings(settings).unwrap_or(SweepConfig {
            mode: SweepMode::Off,
            passes: 0,
            after_optimizer: false,
        })
    }

    /// True if a sweep may route.
    pub fn is_on(&self) -> bool {
        self.mode != SweepMode::Off && self.passes > 0
    }

    /// True if the sweep runs after the batch loop.
    pub fn runs_after_loop(&self, settings: &RouterSettings) -> bool {
        self.is_on() && !(self.after_optimizer && settings.run_optimizer())
    }

    /// True if the sweep runs between two optimizer runs (Q10.3).
    pub fn runs_after_optimizer(&self, settings: &RouterSettings) -> bool {
        self.is_on() && self.after_optimizer && settings.run_optimizer()
    }
}

/// Refuses an unknown `router.autorouter.sweep` value or a negative
/// `sweep_passes`, as an unknown stage id is refused (`run_pipeline` calls
/// it before anything changes).
pub fn check_settings(settings: &RouterSettings) -> Result<(), UnknownStage> {
    SweepMode::parse(settings.autorouter.sweep.as_deref()).map_err(|_| UnknownStage {
        key: SETTINGS_KEY,
        id: settings.autorouter.sweep.clone().unwrap_or_default(),
        known: MODES.to_vec(),
    })?;
    match settings.autorouter.sweep_passes {
        Some(n) if n < 0 => Err(UnknownStage {
            key: PASSES_KEY,
            id: n.to_string(),
            known: vec!["0 or more passes"],
        }),
        _ => Ok(()),
    }
}

/// Why the sweep stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepStop {
    /// The board had no incomplete connection (before or after a pass).
    Complete,
    /// The ordering found nothing to route.
    NothingToRoute,
    /// A pass routed no connection; the board before it came back.
    RoutedNothing,
    /// A pass left more incomplete connections or clearance violations than
    /// before; the board before it came back.
    Regressed,
    /// `sweep_passes` passes ran.
    PassLimit,
    /// A full stop was requested.
    StopRequested,
}

/// One sweep pass.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepPassRecord {
    /// The sweep pass number, from 1.
    pub pass: u32,
    /// Connections routed.
    pub routed: usize,
    /// Connections that failed.
    pub failed: usize,
    /// Incomplete connections before the pass.
    pub incomplete_before: usize,
    /// Incomplete connections after the pass (before a roll-back).
    pub incomplete_after: usize,
    /// Clearance violations after the pass (before a roll-back).
    pub violations_after: usize,
    /// True if the board after the pass was kept.
    pub kept: bool,
}

/// What the sweep did.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepOutcome {
    /// The mode it ran in.
    pub mode: SweepMode,
    /// Its passes, in order.
    pub passes: Vec<SweepPassRecord>,
    /// Why it stopped.
    pub stop: SweepStop,
    /// The board's figures before the sweep.
    pub before: BoardFigures,
    /// The board's figures after it.
    pub after: BoardFigures,
    /// Traces and vias the `fixed` arm froze.
    pub frozen: usize,
    /// Split pieces of frozen wiring whose state was not known and that
    /// were unfixed (see [`Frozen::thaw`]).
    pub unknown_pieces: usize,
    /// The job log lines, in order.
    pub log: Vec<String>,
}

/// Decides the sweep passes: Q10's stop rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SweepController {
    max_passes: u32,
    pass: u32,
}

/// What the controller says after a sweep pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepStep {
    /// Keep the pass and run the next one.
    Continue,
    /// Keep the pass (or not, see [`SweepStep::Rollback`]) and stop.
    Stop(SweepStop),
    /// Bring back the board before the pass and stop.
    Rollback(SweepStop),
}

impl SweepController {
    /// A controller for at most `max_passes` passes.
    pub fn new(max_passes: u32) -> Self {
        SweepController {
            max_passes,
            pass: 0,
        }
    }

    /// The number of the next pass, or why no pass runs.
    pub fn before_pass(
        &mut self,
        incomplete: usize,
        stop_requested: bool,
    ) -> Result<u32, SweepStop> {
        if stop_requested {
            return Err(SweepStop::StopRequested);
        }
        if incomplete == 0 {
            return Err(SweepStop::Complete);
        }
        if self.pass >= self.max_passes {
            return Err(SweepStop::PassLimit);
        }
        self.pass += 1;
        Ok(self.pass)
    }

    /// The step after a pass that went from `before` to `after` and routed
    /// `routed` connections.
    pub fn after_pass(
        &self,
        outcome: &PassOutcome,
        before: &BoardFigures,
        after: &BoardFigures,
    ) -> SweepStep {
        if outcome.nothing_to_route {
            return SweepStep::Stop(SweepStop::NothingToRoute);
        }
        if outcome.routed == 0 {
            return SweepStep::Rollback(SweepStop::RoutedNothing);
        }
        if after.incomplete > before.incomplete || after.violations > before.violations {
            return SweepStep::Rollback(SweepStop::Regressed);
        }
        if after.incomplete == 0 {
            return SweepStep::Stop(SweepStop::Complete);
        }
        SweepStep::Continue
    }
}

/// The connection router of a sweep: the job's, with rip-up off.
struct AddOnlyRouter<'a> {
    inner: &'a mut dyn ConnectionRouter,
    job: &'a CancelToken,
}

impl ConnectionRouter for AddOnlyRouter<'_> {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        // A full stop of the job (deadline, cancel) only stops work.
        if self.job.is_stop_requested() {
            ctx.cancel.stop_all();
        }
        let params = ConnectionParams {
            ripup_allowed: false,
            ..p.clone()
        };
        self.inner.route(board, task, &params, ctx)
    }
}

/// The ripup policy of a sweep: the job's costs and strict check, and no
/// repeated-failure ripup.
struct AddOnlyRipup<'a> {
    inner: &'a mut dyn RipupPolicy,
}

impl RipupPolicy for AddOnlyRipup<'_> {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn ripup_cost(&self, phase: Phase, pass: u32, start_ripup_costs: i32) -> i32 {
        self.inner.ripup_cost(phase, pass, start_ripup_costs)
    }

    fn strict_check(&self, pass: u32, strict_drc: bool) -> bool {
        self.inner.strict_check(pass, strict_drc)
    }

    fn after_failure(&mut self, _board: &mut RoutingBoard, _task: &RouteTask, _failures: u32) {}
}

/// A trace or via the `fixed` arm froze, or found user-fixed already.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FrozenItem {
    /// Its state before the sweep.
    state: FixedState,
    /// Its nets.
    nets: Vec<i32>,
    /// Its first layer.
    layer: i32,
    /// True for a trace.
    trace: bool,
}

/// The wiring the `fixed` arm froze, to give it its state back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frozen {
    /// Every trace and via before the sweep that is not system fixed, by
    /// id, with its state then.
    items: BTreeMap<ItemId, FrozenItem>,
    /// How many of them were frozen (were below user-fixed).
    frozen: usize,
}

impl Frozen {
    /// Marks every trace and via below user-fixed as user-fixed, in id
    /// order, and remembers every state it found.
    pub fn freeze(board: &mut RoutingBoard) -> Frozen {
        let mut frozen = Frozen::default();
        for item in board.items() {
            if !(item.is_trace() || item.is_via()) || item.fixed_state() == FixedState::SystemFixed
            {
                continue;
            }
            frozen.items.insert(
                item.id(),
                FrozenItem {
                    state: item.fixed_state(),
                    nets: sorted_nets(item.net_numbers()),
                    layer: item.first_layer(),
                    trace: item.is_trace(),
                },
            );
        }
        let ids: Vec<ItemId> = frozen
            .items
            .iter()
            .filter(|(_, f)| f.state < FixedState::UserFixed)
            .map(|(id, _)| *id)
            .collect();
        frozen.frozen = ids.len();
        for id in ids {
            board.set_fixed_state(id, FixedState::UserFixed);
        }
        frozen
    }

    /// How many traces and vias were frozen.
    pub fn frozen_count(&self) -> usize {
        self.frozen
    }

    /// Gives every frozen item that is still on the board its state back,
    /// in id order. A user-fixed trace or via the board did not hold before
    /// the sweep is a piece a sweep connection split off (or combined with)
    /// frozen wiring: it takes the state of the vanished items on its nets
    /// and layer when they all had one state, else it is unfixed. Returns
    /// how many pieces were unfixed with no known state.
    pub fn thaw(&self, board: &mut RoutingBoard) -> usize {
        let present: BTreeSet<ItemId> = board.items().map(|item| item.id()).collect();
        let vanished: Vec<&FrozenItem> = self
            .items
            .iter()
            .filter(|(id, _)| !present.contains(id))
            .map(|(_, f)| f)
            .collect();
        let mut changes: Vec<(ItemId, FixedState)> = Vec::new();
        let mut unknown = 0;
        for item in board.items() {
            if !(item.is_trace() || item.is_via()) || item.fixed_state() != FixedState::UserFixed {
                continue;
            }
            match self.items.get(&item.id()) {
                Some(f) if f.state < FixedState::UserFixed => changes.push((item.id(), f.state)),
                Some(_) => {}
                None => {
                    let nets = sorted_nets(item.net_numbers());
                    let states: BTreeSet<FixedState> = vanished
                        .iter()
                        .filter(|f| {
                            f.trace == item.is_trace()
                                && f.nets == nets
                                && f.layer == item.first_layer()
                        })
                        .map(|f| f.state)
                        .collect();
                    let state = match states.iter().next() {
                        Some(state) if states.len() == 1 => *state,
                        _ => {
                            unknown += 1;
                            FixedState::Unfixed
                        }
                    };
                    if state < FixedState::UserFixed {
                        changes.push((item.id(), state));
                    }
                }
            }
        }
        for (id, state) in changes {
            board.set_fixed_state(id, state);
        }
        unknown
    }
}

fn sorted_nets(nets: &[i32]) -> Vec<i32> {
    let mut nets = nets.to_vec();
    nets.sort_unstable();
    nets
}

/// Runs the sweep on `board` with the job's stages (see the module docs).
/// `final_tails` repeats the loop's final `remove_tails(NONE)` after a
/// sweep that kept a pass.
pub fn run_sweep(
    board: &mut RoutingBoard,
    stages: &mut PassStages<'_>,
    executor: &mut dyn PassExecutor,
    ctx: &StageCtx,
    config: SweepConfig,
    final_tails: bool,
) -> SweepOutcome {
    let settings = ctx.settings;
    let before = executor.figures(board, settings);
    let mut outcome = SweepOutcome {
        mode: config.mode,
        passes: Vec::new(),
        stop: SweepStop::Complete,
        before: before.clone(),
        after: before.clone(),
        frozen: 0,
        unknown_pieces: 0,
        log: Vec::new(),
    };
    let local = CancelToken::new();
    let mut local_ctx = StageCtx {
        settings,
        board_settings: ctx.board_settings,
        cancel: &local,
        events: ctx.events,
    };
    let mut controller = SweepController::new(config.passes);
    let mut figures = before;
    let mut frozen = None;
    let mut kept_any = false;
    loop {
        let stop_requested = ctx.cancel.is_stop_requested() || local.is_stop_requested();
        let pass = match controller.before_pass(figures.incomplete, stop_requested) {
            Ok(pass) => pass,
            Err(stop) => {
                outcome.stop = stop;
                break;
            }
        };
        if pass == 1 {
            if config.mode == SweepMode::Fixed {
                let f = Frozen::freeze(board);
                outcome.frozen = f.frozen_count();
                frozen = Some(f);
            }
            // As after a restore: the ordering's state belongs to the loop.
            stages.ordering.reset();
        }
        let saved = board.clone();
        let scope = crate::run_stats::stage("sweep", pass);
        let result = {
            let mut router = AddOnlyRouter {
                inner: &mut *stages.router,
                job: ctx.cancel,
            };
            let mut ripup = AddOnlyRipup {
                inner: &mut *stages.ripup,
            };
            let mut sweep_stages = PassStages {
                ordering: &mut *stages.ordering,
                router: &mut router,
                ripup: &mut ripup,
            };
            executor.run_pass(board, pass, &mut sweep_stages, &mut local_ctx)
        };
        scope.finish();
        outcome.log.extend(result.log.iter().cloned());
        let after = executor.figures(board, settings);
        let step = controller.after_pass(&result, &figures, &after);
        let kept = !matches!(step, SweepStep::Rollback(_));
        if !result.nothing_to_route {
            outcome.passes.push(SweepPassRecord {
                pass,
                routed: result.routed,
                failed: result.failed,
                incomplete_before: figures.incomplete,
                incomplete_after: after.incomplete,
                violations_after: after.violations,
                kept,
            });
            outcome.log.push(format!(
                "Add-only sweep pass #{pass} ({}): routed {}, failed {}; incomplete {} -> {}, \
                 violations {} -> {}{}.",
                config.mode.as_str(),
                result.routed,
                result.failed,
                figures.incomplete,
                after.incomplete,
                figures.violations,
                after.violations,
                if kept {
                    ""
                } else {
                    "; the board before it is kept"
                }
            ));
        }
        if kept {
            kept_any |= !result.nothing_to_route;
            figures = after;
        } else {
            swap_in(board, saved, ctx);
            executor.audit(board, AuditPoint::Restore);
        }
        match step {
            SweepStep::Continue => {}
            SweepStep::Stop(stop) | SweepStep::Rollback(stop) => {
                outcome.stop = stop;
                break;
            }
        }
    }
    if let Some(f) = &frozen {
        outcome.unknown_pieces = f.thaw(board);
    }
    if final_tails && kept_any {
        executor.remove_tails(board, StopConnectionOption::None);
        executor.audit(board, AuditPoint::Finish);
    }
    // `max_items` in the sweep is a full stop of the job.
    if local.is_stop_requested() {
        ctx.cancel.stop_all();
    }
    if frozen.is_some() || (final_tails && kept_any) {
        figures = executor.figures(board, settings);
    }
    outcome.log.push(format!(
        "Add-only sweep ({}) {}: {} pass(es), incomplete {} -> {}, violations {} -> {}{}.",
        config.mode.as_str(),
        stop_text(outcome.stop),
        outcome.passes.len(),
        outcome.before.incomplete,
        figures.incomplete,
        outcome.before.violations,
        figures.violations,
        if config.mode == SweepMode::Fixed {
            format!(
                "; {} item(s) frozen, {} split piece(s) unfixed",
                outcome.frozen, outcome.unknown_pieces
            )
        } else {
            String::new()
        }
    ));
    outcome.after = figures;
    outcome
}

fn stop_text(stop: SweepStop) -> &'static str {
    match stop {
        SweepStop::Complete => "completed the board",
        SweepStop::NothingToRoute => "found nothing to route",
        SweepStop::RoutedNothing => "stopped: a pass routed nothing",
        SweepStop::Regressed => "stopped: a pass made the board worse",
        SweepStop::PassLimit => "reached its pass limit",
        SweepStop::StopRequested => "stopped on request",
    }
}

/// Swaps `saved` in and announces it, as the batch loop's restore does.
fn swap_in(board: &mut RoutingBoard, saved: RoutingBoard, ctx: &StageCtx) {
    let listening = ctx.events.is_listening();
    *board = saved;
    if listening {
        ctx.events.replaced(board.snapshot());
        board.set_journaling(true);
    }
}

/// True if the loop that produced `outcome` leaves the sweep something to
/// do: the router ran and ended without a requested stop.
fn loop_allows_sweep(outcome: &LoopOutcome, ctx: &StageCtx, timed_out: bool) -> bool {
    outcome.router_enabled
        && !timed_out
        && !ctx.cancel.is_stop_requested()
        && outcome.stop != Some(StopReason::StopRequested)
}

/// The sweep after the batch loop's finish, when the settings ask for it
/// there (called by `run_batch_loop`); `None` when it does not run.
pub fn after_loop(
    board: &mut RoutingBoard,
    stages: &mut LoopStages<'_>,
    executor: &mut dyn PassExecutor,
    ctx: &StageCtx,
    outcome: &LoopOutcome,
    timed_out: bool,
) -> Option<SweepOutcome> {
    let config = SweepConfig::of(ctx.settings);
    if !config.runs_after_loop(ctx.settings) || !loop_allows_sweep(outcome, ctx, timed_out) {
        return None;
    }
    let mut pass_stages = PassStages {
        ordering: &mut *stages.ordering,
        router: &mut *stages.router,
        ripup: &mut *stages.ripup,
    };
    Some(run_sweep(
        board,
        &mut pass_stages,
        executor,
        ctx,
        config,
        outcome.final_tails,
    ))
}

/// Q10.3: a first optimizer run, then the sweep, when the settings ask for
/// it (called by `run_pipeline` after the batch loop; the pipeline's own
/// optimizer run follows). The optimizer's lines and the sweep's go into
/// `outcome.log`, and the sweep into `outcome.sweep`. The optimizer's time
/// limit is armed as for the pipeline's run.
pub fn after_optimizer(
    board: &mut RoutingBoard,
    stages: &mut LoopStages<'_>,
    executor: &mut dyn PassExecutor,
    optimizer: &mut dyn Optimizer,
    ctx: &mut StageCtx,
    watcher: &Watcher,
    outcome: &mut LoopOutcome,
) {
    let config = SweepConfig::of(ctx.settings);
    if !config.runs_after_optimizer(ctx.settings)
        || !loop_allows_sweep(outcome, ctx, watcher.fired())
    {
        return;
    }
    let deadline = ctx
        .settings
        .deadlines()
        .optimizer
        .map(|limit| watcher.arm(limit, StopLevel::All));
    let summary = optimizer.run(board, ctx);
    if let Some(id) = deadline {
        watcher.disarm(id);
    }
    outcome.log.extend(summary.log.iter().cloned());
    if ctx.cancel.is_stop_requested() {
        return;
    }
    let mut pass_stages = PassStages {
        ordering: &mut *stages.ordering,
        router: &mut *stages.router,
        ripup: &mut *stages.ripup,
    };
    let sweep = run_sweep(
        board,
        &mut pass_stages,
        executor,
        ctx,
        config,
        outcome.final_tails,
    );
    outcome.log.extend(sweep.log.iter().map(|text| LogLine {
        level: LogLevel::Info,
        text: text.clone(),
    }));
    outcome.sweep = Some(sweep);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn figures(incomplete: usize, violations: usize) -> BoardFigures {
        BoardFigures {
            incomplete,
            violations,
            ..BoardFigures::default()
        }
    }

    fn pass(routed: usize) -> PassOutcome {
        PassOutcome {
            routed,
            ..PassOutcome::default()
        }
    }

    #[test]
    fn modes_parse_and_unknown_values_are_refused() {
        assert_eq!(SweepMode::parse(None), Ok(SweepMode::Off));
        assert_eq!(SweepMode::parse(Some("")), Ok(SweepMode::Off));
        assert_eq!(SweepMode::parse(Some("off")), Ok(SweepMode::Off));
        assert_eq!(SweepMode::parse(Some(" fixed ")), Ok(SweepMode::Fixed));
        assert_eq!(SweepMode::parse(Some("free")), Ok(SweepMode::Free));
        let error = SweepMode::parse(Some("on")).expect_err("refused");
        assert!(
            error.contains(SETTINGS_KEY) && error.contains("'on'"),
            "{error}"
        );
        for mode in [SweepMode::Off, SweepMode::Fixed, SweepMode::Free] {
            assert_eq!(SweepMode::parse(Some(mode.as_str())), Ok(mode));
            assert!(MODES.contains(&mode.as_str()));
        }
    }

    #[test]
    fn the_controller_stops_at_its_cap_and_on_a_complete_board() {
        let mut c = SweepController::new(2);
        assert_eq!(c.before_pass(3, false), Ok(1));
        assert_eq!(c.before_pass(3, false), Ok(2));
        assert_eq!(c.before_pass(3, false), Err(SweepStop::PassLimit));
        let mut c = SweepController::new(2);
        assert_eq!(c.before_pass(0, false), Err(SweepStop::Complete));
        assert_eq!(c.before_pass(3, true), Err(SweepStop::StopRequested));
        assert_eq!(
            SweepController::new(0).before_pass(3, false),
            Err(SweepStop::PassLimit)
        );
    }

    #[test]
    fn a_pass_is_kept_only_if_it_routed_and_made_nothing_worse() {
        let c = SweepController::new(2);
        let before = figures(5, 1);
        assert_eq!(
            c.after_pass(&pass(2), &before, &figures(3, 1)),
            SweepStep::Continue
        );
        assert_eq!(
            c.after_pass(&pass(5), &before, &figures(0, 1)),
            SweepStep::Stop(SweepStop::Complete)
        );
        assert_eq!(
            c.after_pass(&pass(0), &before, &figures(5, 1)),
            SweepStep::Rollback(SweepStop::RoutedNothing)
        );
        assert_eq!(
            c.after_pass(&pass(1), &before, &figures(4, 2)),
            SweepStep::Rollback(SweepStop::Regressed)
        );
        assert_eq!(
            c.after_pass(&pass(1), &before, &figures(6, 1)),
            SweepStep::Rollback(SweepStop::Regressed)
        );
        let empty = PassOutcome {
            nothing_to_route: true,
            ..PassOutcome::default()
        };
        assert_eq!(
            c.after_pass(&empty, &before, &before),
            SweepStep::Stop(SweepStop::NothingToRoute)
        );
    }
}

//! Q12: the pass-1 portfolio and squeaky-wheel restarts
//! (`router.autorouter.portfolio`, `router.autorouter.restarts`).
//!
//! Pass 1 is the best pass on `canary-pwr`, and its result moves with the
//! order the connections are routed in (KRT #1230: about ±10 % open nets
//! from placebo perturbations). The portfolio spends the work of later
//! passes on several whole pass-1 solutions instead and keeps the best, as
//! TopoR and Cadence's US6957408B1 do.
//!
//! **The stage** ([`run_routing`], one call in `run_pipeline` in place of
//! the batch loop):
//! 1. the batch loop's start on the job's board with the job's stages: the
//!    routable-layer check, the fanout and its board stages
//!    ([`start_batch_loop`]);
//! 2. `K = portfolio` arms ([`arms::portfolio_arms`]: the configured
//!    ordering, Q07's `conflict-rounds` and `inside-out`, seeded shuffles
//!    1, 2, …). Arm i continues the loop ([`continue_batch_loop`]: its
//!    passes, the finish, Q10's sweep if on, the after-routing board
//!    stages) on its own clone of the post-fanout board, with stages built
//!    afresh from the registry and its own ordering;
//! 3. the best arm by (incomplete connections, higher router score, lower
//!    arm index) is kept;
//! 4. `R = restarts` squeaky-wheel generations, one at a time: pass 1 in
//!    the current best's pass-1 order with that run's failed nets first,
//!    then the nets it ripped ([`arms::squeaky_order`]), on a clone of the
//!    post-fanout board; the best of all is kept, by the same key. The
//!    generations stop early once the best board is complete, or when the
//!    best is still the arm the last generation started from (the next one
//!    would route the same order on the same board again);
//! 5. the job's board becomes the best arm's (`Replaced` sent when someone
//!    listens); the pipeline's optimizer then runs once on it.
//!
//! With `restarts` set and `portfolio` unset, the portfolio is arm 0 alone.
//! With both unset or 0, [`run_routing`] is [`run_batch_loop`] and nothing
//! here runs.
//!
//! **Arm 0 is the configured ordering**, so the kept board is never worse
//! than the job without the portfolio by the engine's own key. With
//! `portfolio = 1` and no restarts the SES is the job's without it: arm 0
//! routes the same board (a `Clone`, so the search trees keep their shape)
//! with the same stages.
//!
//! **Determinism.** Each arm is a function of the post-fanout board and the
//! arm: its own stages, its own [`PassRunner`], its own cancel token (an
//! arm's router stop at its pass limit stops no other arm; the job's stops
//! reach every arm before each connection), and no events. Arms run on up
//! to `autorouter.max_threads` scoped threads at once, in waves of arm
//! indexes; the results are merged in arm order, so the kept arm does not
//! depend on the thread count or on which arm finished first. After the
//! portfolio the job's cancel token takes the kept arm's stop state.
//!
//! **Memory and time guard** (Q12.3). Each running arm holds a board clone
//! and its sweep's and restores' copies; the guard estimates a clone from
//! the post-fanout board ([`clone_estimate_bytes`]) and runs at most as many
//! arms at once as fit, with the post-fanout board and the best board so
//! far, under `portfolio_memory_mb` (unset [`DEFAULT_MEMORY_MB`]). It never
//! drops an arm: when not even one fits, arms run one at a time and the job
//! log says so. Time is bounded by the arms' work budgets (each arm runs the
//! job's passes) and by the job's deadline and cancel, which only stop work.
//!
//! **Order spread** (Q12.4). The outcome ([`PortfolioOutcome`]) and the job
//! log name every arm's incomplete count, score and violations, and the
//! minimum, median and maximum incomplete count over the K portfolio arms.
//!
//! Not consulted with the portfolio on: the job's report-only diagnostics
//! (`router.special.diagnostics`) see the input and the final board but no
//! arm's connections; the P02.1 audit covers no arm; the pass executor a
//! test hands `run_pipeline_with` routes no arm (arms use [`PassRunner`]);
//! progress events and board deltas are not sent while the arms route.
//!
//! Off by default and in no profile.

pub mod arms;

use std::cell::RefCell;
use std::rc::Rc;

use crate::autoroute::batch::batch_loop::{
    LoopOutcome, LoopStages, LoopStart, PassExecutor, continue_batch_loop, run_batch_loop,
    start_batch_loop,
};
use crate::autoroute::batch::pass::PassRunner;
use crate::autoroute::batch::report::NoRoutableLayer;
use crate::autoroute::batch::snapshot::BoardFigures;
use crate::autoroute::events::{CancelToken, NullEvents};
use crate::autoroute::stages::{StageCtx, StageRegistry, StageSelection, UnknownStage};
use crate::board::RoutingBoard;
use crate::settings::RouterSettings;
use crate::settings::board::BoardRouterSettings;
use arms::{Arm, PassOneRecord, RecordingOrdering, RecordingRouter, portfolio_arms, squeaky_order};

/// The settings key of K.
pub const PORTFOLIO_KEY: &str = "router.autorouter.portfolio";

/// The settings key of R.
pub const RESTARTS_KEY: &str = "router.autorouter.restarts";

/// The settings key of the memory limit.
pub const MEMORY_KEY: &str = "router.autorouter.portfolio_memory_mb";

/// The memory limit when `portfolio_memory_mb` is unset, in MiB.
pub const DEFAULT_MEMORY_MB: u32 = 2048;

/// Estimated bytes per board item in a clone. Q12.3's calibration on
/// `canary-pwr` (K = 4, `-mp 1`, release, Linux): a second concurrent arm
/// raised the peak resident memory from 107 MiB to 171 MiB, about 64 MiB per
/// running arm; 8 KiB per item estimates 24 MiB per clone there, 72 MiB per
/// running arm.
pub const BYTES_PER_ITEM: u64 = 8192;

/// Estimated fixed bytes of a clone besides its items.
pub const BYTES_PER_BOARD: u64 = 8 << 20;

/// Board copies a running arm holds at its peak, in clones: its board,
/// the sweep's or a restore's copy, and the history's snapshots.
pub const CLONES_PER_ARM: u64 = 3;

/// The portfolio settings, resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortfolioConfig {
    /// K, the portfolio arms (0: off).
    pub arms: usize,
    /// R, the squeaky-wheel generations.
    pub restarts: usize,
    /// The memory limit in MiB.
    pub memory_mb: u32,
}

impl PortfolioConfig {
    /// The configuration of `settings`; fails on a negative value, naming
    /// the key.
    pub fn from_settings(settings: &RouterSettings) -> Result<Self, UnknownStage> {
        let count = |key: &'static str, value: Option<i32>| match value {
            None => Ok(0),
            Some(n) => usize::try_from(n).map_err(|_| UnknownStage {
                key,
                id: n.to_string(),
                known: vec!["0 or more"],
            }),
        };
        let arms = count(PORTFOLIO_KEY, settings.autorouter.portfolio)?;
        let restarts = count(RESTARTS_KEY, settings.autorouter.restarts)?;
        let memory_mb = match settings.autorouter.portfolio_memory_mb {
            None => DEFAULT_MEMORY_MB,
            Some(n) if n > 0 => n.unsigned_abs(),
            Some(n) => {
                return Err(UnknownStage {
                    key: MEMORY_KEY,
                    id: n.to_string(),
                    known: vec!["1 or more MiB"],
                });
            }
        };
        Ok(PortfolioConfig {
            // Restarts without a portfolio run arm 0 alone first.
            arms: if restarts > 0 { arms.max(1) } else { arms },
            restarts,
            memory_mb,
        })
    }

    /// True if the portfolio replaces the batch loop.
    pub fn is_on(&self) -> bool {
        self.arms > 0
    }
}

/// Refuses a negative `portfolio` or `restarts` or a non-positive memory
/// limit before routing (`run_pipeline` calls it).
pub fn check_settings(settings: &RouterSettings) -> Result<(), UnknownStage> {
    PortfolioConfig::from_settings(settings).map(|_| ())
}

/// One arm's result as the outcome reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct ArmSummary {
    /// The arm index (restarts follow the K portfolio arms).
    pub index: usize,
    /// The arm's label.
    pub label: String,
    /// The figures of the board the arm left.
    pub figures: BoardFigures,
}

/// The incomplete counts over the K portfolio arms (Q12.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spread {
    /// The fewest incomplete connections.
    pub min: usize,
    /// The median (the mean of the two middle values for even K).
    pub median: f64,
    /// The most incomplete connections.
    pub max: usize,
}

impl Spread {
    /// The spread of `counts`; `None` if empty.
    pub fn of(counts: &[usize]) -> Option<Spread> {
        let mut sorted = counts.to_vec();
        sorted.sort_unstable();
        let n = sorted.len();
        let (&min, &max) = (sorted.first()?, sorted.last()?);
        let median = if n % 2 == 1 {
            sorted[n / 2] as f64
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) as f64 / 2.0
        };
        Some(Spread { min, median, max })
    }
}

/// What the portfolio did.
#[derive(Clone, Debug, PartialEq)]
pub struct PortfolioOutcome {
    /// Every arm, the K portfolio arms then the restarts, in index order.
    pub arms: Vec<ArmSummary>,
    /// The index of the kept arm.
    pub winner: usize,
    /// The arms that ran at once.
    pub concurrency: usize,
    /// The guard's estimate of one clone, in bytes.
    pub clone_estimate: u64,
    /// The incomplete counts over the K portfolio arms.
    pub spread: Option<Spread>,
}

/// The guard's estimate of one clone of `board`, in bytes.
pub fn clone_estimate_bytes(board: &RoutingBoard) -> u64 {
    let items = u64::try_from(board.items().count()).unwrap_or(u64::MAX);
    BYTES_PER_BOARD.saturating_add(items.saturating_mul(BYTES_PER_ITEM))
}

/// The arms that may run at once: at most `threads`, and as many as fit
/// under `limit_mb` beside two clones (the post-fanout board and the best
/// so far); at least 1.
pub fn concurrency(clone_bytes: u64, limit_mb: u32, threads: usize) -> usize {
    let limit = u64::from(limit_mb) << 20;
    let per_arm = clone_bytes.saturating_mul(CLONES_PER_ARM).max(1);
    let free = limit.saturating_sub(clone_bytes.saturating_mul(2));
    let fit = usize::try_from(free / per_arm).unwrap_or(usize::MAX);
    fit.min(threads).max(1)
}

/// What `run_pipeline` hands the portfolio besides the loop's arguments.
pub struct PortfolioEnv<'a> {
    /// The registry the job's stages came from.
    pub registry: &'a StageRegistry,
    /// The job's stage selection.
    pub selection: &'a StageSelection,
}

/// The batch loop, or the portfolio in its place when it is on (module
/// docs). `timed_out` is shared with the arms' threads.
pub fn run_routing(
    board: &mut RoutingBoard,
    stages: &mut LoopStages<'_>,
    executor: &mut dyn PassExecutor,
    ctx: &mut StageCtx,
    timed_out: &(dyn Fn() -> bool + Sync),
    env: &PortfolioEnv<'_>,
) -> Result<LoopOutcome, NoRoutableLayer> {
    let config = PortfolioConfig::from_settings(ctx.settings).unwrap_or(PortfolioConfig {
        arms: 0,
        restarts: 0,
        memory_mb: DEFAULT_MEMORY_MB,
    });
    if !config.is_on() {
        return run_batch_loop(board, stages, executor, ctx, timed_out);
    }
    let start = start_batch_loop(board, stages, ctx)?;
    let settings = ctx.settings;
    let routes = settings.run_router() && settings.autorouter.max_passes >= 0;
    if !routes || ctx.cancel.is_router_stop_requested() {
        return Ok(continue_batch_loop(
            start, board, stages, executor, ctx, timed_out,
        ));
    }
    Ok(Portfolio {
        config,
        env,
        settings,
        board_settings: ctx.board_settings,
        job: ctx.cancel,
        timed_out,
    }
    .run(board, &start, ctx))
}

/// One arm's run.
struct ArmResult {
    index: usize,
    label: String,
    board: RoutingBoard,
    outcome: LoopOutcome,
    figures: BoardFigures,
    record: PassOneRecord,
    stop_all: bool,
    stop_router: bool,
    router_log: Vec<String>,
}

impl ArmResult {
    fn summary(&self) -> ArmSummary {
        ArmSummary {
            index: self.index,
            label: self.label.clone(),
            figures: self.figures.clone(),
        }
    }
}

/// True if an arm with figures `a` beats an earlier arm (lower index) with
/// figures `b`: fewer incomplete connections, then a higher router score. A
/// tie keeps the earlier arm.
pub fn beats(a: &BoardFigures, b: &BoardFigures) -> bool {
    a.incomplete < b.incomplete
        || (a.incomplete == b.incomplete && a.score.total_cmp(&b.score).is_gt())
}

struct Portfolio<'a> {
    config: PortfolioConfig,
    env: &'a PortfolioEnv<'a>,
    settings: &'a RouterSettings,
    board_settings: &'a BoardRouterSettings,
    job: &'a CancelToken,
    timed_out: &'a (dyn Fn() -> bool + Sync),
}

impl Portfolio<'_> {
    fn run(&self, board: &mut RoutingBoard, start: &LoopStart, ctx: &StageCtx) -> LoopOutcome {
        let clone_bytes = clone_estimate_bytes(board);
        let threads = usize::try_from(self.settings.autorouter.max_threads)
            .unwrap_or(1)
            .clamp(1, crate::parallel::threads().max(1));
        let at_once = concurrency(clone_bytes, self.config.memory_mb, threads)
            .min(self.config.arms)
            .max(1);
        let mut log = Vec::new();
        if clone_bytes.saturating_mul(CLONES_PER_ARM + 2) > u64::from(self.config.memory_mb) << 20 {
            log.push(format!(
                "Portfolio memory guard: about {} MiB per arm exceeds {MEMORY_KEY} = {} MiB; \
                 the arms run one at a time.",
                (clone_bytes * CLONES_PER_ARM) >> 20,
                self.config.memory_mb
            ));
        }
        let arms = portfolio_arms(self.config.arms);
        let mut summaries = Vec::new();
        let mut best: Option<ArmResult> = None;
        for wave in arms.chunks(at_once).enumerate().map(|(w, chunk)| {
            chunk
                .iter()
                .enumerate()
                .map(|(i, arm)| (w * at_once + i, arm.clone()))
                .collect::<Vec<_>>()
        }) {
            // A wave of one routes on this thread with the job's budget.
            let _budget = (wave.len() > 1).then(|| crate::parallel::ThreadBudget::set(wave.len()));
            let results = crate::parallel::map(&wave, |(index, arm)| {
                self.run_arm(*index, arm, board, start)
            });
            for result in results {
                self.merge(result, &mut best, &mut summaries, &mut log);
            }
        }
        let spread = Spread::of(
            &summaries
                .iter()
                .map(|s: &ArmSummary| s.figures.incomplete)
                .collect::<Vec<_>>(),
        );
        let mut restarted_from = None;
        for generation in 1..=self.config.restarts {
            let Some(current) = &best else { break };
            // A generation from the arm the last one started from would
            // route the same order again: same board, same result.
            if current.figures.incomplete == 0
                || self.job.is_stop_requested()
                || restarted_from == Some(current.index)
            {
                break;
            }
            restarted_from = Some(current.index);
            let arm = Arm::Restart {
                generation: u32::try_from(generation).unwrap_or(u32::MAX),
                order: squeaky_order(&current.record),
            };
            let index = self.config.arms + generation - 1;
            let r = &current.record;
            log.push(format!(
                "Portfolio restart {generation}: the order of arm {}, its {} failed net{}{} \
                 first, then its {} other ripped net{}{}.",
                current.index,
                r.failed.len(),
                if r.failed.len() == 1 { "" } else { "s" },
                net_names(board, &r.failed),
                r.victims.difference(&r.failed).count(),
                if r.victims.difference(&r.failed).count() == 1 {
                    ""
                } else {
                    "s"
                },
                net_names(board, &r.victims.difference(&r.failed).copied().collect())
            ));
            let result = self.run_arm(index, &arm, board, start);
            self.merge(result, &mut best, &mut summaries, &mut log);
        }
        let Some(winner) = best else {
            return start.for_arm().outcome().clone();
        };
        let outcome = PortfolioOutcome {
            arms: summaries,
            winner: winner.index,
            concurrency: at_once,
            clone_estimate: clone_bytes,
            spread,
        };
        self.install(board, winner, outcome, log, ctx)
    }

    /// Keeps `result` if it beats the best so far; notes it either way.
    fn merge(
        &self,
        result: ArmResult,
        best: &mut Option<ArmResult>,
        summaries: &mut Vec<ArmSummary>,
        log: &mut Vec<String>,
    ) {
        let f = &result.figures;
        log.push(format!(
            "Portfolio arm {} ({}): {} incomplete, score {}, {} clearance violation{}.",
            result.index,
            result.label,
            f.incomplete,
            crate::job::log::decimal_format_0_00(f64::from(f.score)),
            f.violations,
            if f.violations == 1 { "" } else { "s" }
        ));
        summaries.push(result.summary());
        if best
            .as_ref()
            .is_none_or(|b| beats(&result.figures, &b.figures))
        {
            *best = Some(result);
        }
    }

    /// Arm `index` on a clone of `base`.
    fn run_arm(
        &self,
        index: usize,
        arm: &Arm,
        base: &RoutingBoard,
        start: &LoopStart,
    ) -> ArmResult {
        let mut board = base.clone();
        board.set_journaling(false);
        let local = CancelToken::new();
        let events = NullEvents;
        let mut ctx = StageCtx {
            settings: self.settings,
            board_settings: self.board_settings,
            cancel: &local,
            events: &events,
        };
        // The arms run no report-only diagnostics (module docs).
        let selection = StageSelection {
            diagnostics: Vec::new(),
            ..self.env.selection.clone()
        };
        let mut pipeline = match self.env.registry.build(&selection, self.settings) {
            Ok(pipeline) => pipeline,
            // The job built the same selection already, so this does not
            // happen; the arm then leaves the post-fanout board.
            Err(_) => return self.unrouted_arm(index, arm, board, start),
        };
        let record = Rc::new(RefCell::new(PassOneRecord::default()));
        let configured = std::mem::replace(
            &mut pipeline.ordering,
            Box::new(crate::autoroute::batch::ordering::FreeroutingOrdering::new()),
        );
        let mut ordering = RecordingOrdering {
            inner: arm.ordering(configured),
            record: Rc::clone(&record),
        };
        let mut runner = PassRunner::for_job(self.settings, self.board_settings);
        let outcome = {
            let mut router = RecordingRouter::new(
                pipeline.connection_router.as_mut(),
                self.job,
                Rc::clone(&record),
                &board,
            );
            let mut stages = LoopStages {
                fanout: pipeline.fanout.as_mut(),
                ordering: &mut ordering,
                router: &mut router,
                ripup: pipeline.ripup_policy.as_mut(),
                controller: pipeline.pass_controller.as_mut(),
                board_stages: &mut pipeline.board_stages,
            };
            continue_batch_loop(
                start.for_arm(),
                &mut board,
                &mut stages,
                &mut runner,
                &mut ctx,
                self.timed_out,
            )
        };
        let figures = outcome
            .final_figures
            .clone()
            .unwrap_or_else(|| BoardFigures::of(&board, self.settings));
        let record = record.borrow().clone();
        ArmResult {
            index,
            label: arm.label(),
            board,
            outcome,
            figures,
            record,
            stop_all: local.is_stop_requested(),
            stop_router: local.is_router_stop_requested(),
            router_log: pipeline.connection_router.log_lines(),
        }
    }

    fn unrouted_arm(
        &self,
        index: usize,
        arm: &Arm,
        board: RoutingBoard,
        start: &LoopStart,
    ) -> ArmResult {
        ArmResult {
            index,
            label: arm.label(),
            figures: BoardFigures::of(&board, self.settings),
            board,
            outcome: start.for_arm().outcome().clone(),
            record: PassOneRecord::default(),
            stop_all: false,
            stop_router: false,
            router_log: Vec::new(),
        }
    }

    /// Makes the winner the job's board and outcome.
    fn install(
        &self,
        board: &mut RoutingBoard,
        winner: ArmResult,
        portfolio: PortfolioOutcome,
        mut log: Vec<String>,
        ctx: &StageCtx,
    ) -> LoopOutcome {
        *board = winner.board;
        if ctx.events.is_listening() {
            ctx.events.replaced(board.snapshot());
        }
        if winner.stop_all {
            self.job.stop_all();
        } else if winner.stop_router {
            self.job.stop_router();
        }
        let mut outcome = winner.outcome;
        log.extend(winner.router_log);
        log.push(portfolio_line(
            &portfolio,
            self.config.arms,
            self.config.memory_mb,
        ));
        for text in log {
            outcome.log.push(crate::job::LogLine {
                level: crate::job::LogLevel::Info,
                text,
            });
        }
        outcome.final_figures = Some(winner.figures);
        outcome.portfolio = Some(portfolio);
        outcome
    }
}

/// The first [`NAMED_HOT_NETS`] names of `nets`, as ` (A, B, …)`.
fn net_names(board: &RoutingBoard, nets: &std::collections::BTreeSet<i32>) -> String {
    if nets.is_empty() {
        return String::new();
    }
    let mut names: Vec<String> = nets
        .iter()
        .take(NAMED_HOT_NETS)
        .map(|net| {
            board
                .rules
                .nets
                .get(*net)
                .map_or_else(|| format!("#{net}"), |n| n.name.clone())
        })
        .collect();
    if nets.len() > NAMED_HOT_NETS {
        names.push("…".to_string());
    }
    format!(" ({})", names.join(", "))
}

/// The hot nets a restart's log line names.
const NAMED_HOT_NETS: usize = 10;

fn portfolio_line(p: &PortfolioOutcome, arms: usize, memory_mb: u32) -> String {
    let label = p
        .arms
        .iter()
        .find(|a| a.index == p.winner)
        .map_or("", |a| a.label.as_str());
    let spread = p.spread.map_or(String::new(), |s| {
        format!(
            " Incomplete over the portfolio arms: min {}, median {}, max {}.",
            s.min, s.median, s.max
        )
    });
    let restarts = p.arms.len().saturating_sub(arms);
    format!(
        "Portfolio: {arms} arm{} and {restarts} restart{}, {} at a time (about {} MiB per \
         clone, limit {memory_mb} MiB); kept arm {} ({label}).{spread}",
        if arms == 1 { "" } else { "s" },
        if restarts == 1 { "" } else { "s" },
        p.concurrency,
        p.clone_estimate >> 20,
        p.winner,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn figures(incomplete: usize, score: f32) -> BoardFigures {
        BoardFigures {
            score,
            incomplete,
            violations: 0,
        }
    }

    /// The kept arm of `arms` merged in index order.
    fn kept(arms: &[BoardFigures]) -> usize {
        let mut best = 0;
        for (i, f) in arms.iter().enumerate().skip(1) {
            if beats(f, &arms[best]) {
                best = i;
            }
        }
        best
    }

    #[test]
    fn the_key_is_incomplete_then_score_then_index() {
        // Arm 2 beats arm 0 on incomplete connections.
        assert_eq!(
            kept(&[figures(5, 900.0), figures(6, 950.0), figures(4, 800.0)]),
            2
        );
        // Equal incomplete: the higher score.
        assert_eq!(
            kept(&[figures(4, 900.0), figures(4, 901.0), figures(4, 800.0)]),
            1
        );
        // A full tie keeps the lower index.
        assert_eq!(
            kept(&[figures(4, 900.0), figures(4, 900.0), figures(4, 900.0)]),
            0
        );
    }

    #[test]
    fn the_spread_has_min_median_and_max() {
        let s = Spread::of(&[7, 3, 5, 9]).unwrap();
        assert_eq!((s.min, s.median, s.max), (3, 6.0, 9));
        let s = Spread::of(&[4, 1, 2]).unwrap();
        assert_eq!((s.min, s.median, s.max), (1, 2.0, 4));
        assert!(Spread::of(&[]).is_none());
    }

    #[test]
    fn the_guard_lowers_concurrency_and_never_below_one() {
        let mib = 1u64 << 20;
        // 100 MiB clones: 2 clones kept, 300 MiB per arm.
        assert_eq!(concurrency(100 * mib, 2048, 8), 6);
        assert_eq!(concurrency(100 * mib, 2048, 2), 2);
        assert_eq!(concurrency(100 * mib, 500, 8), 1);
        assert_eq!(concurrency(100 * mib, 100, 8), 1);
        assert_eq!(concurrency(0, 1, 4), 4);
    }
}

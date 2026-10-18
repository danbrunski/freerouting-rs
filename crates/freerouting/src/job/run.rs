//! The job flow of the CLI and the GUI's front-end mode (C07.4).
//!
//! Port of `CliRunner.initializeCli` (after the input is read),
//! `RoutingJobSchedulerActionThread.threadAction` and `CliRunner.
//! writeCliOutputIfAvailable` at `a773a70ec`, with the fixes of the C series:
//! outputs are written atomically and never with 0 bytes ([`outputs`]), and
//! wall-clock limits only stop work ([`deadline`]).
//!
//! # The flow of [`run_job`]
//!
//! 0. Tell the observer the load begins ([`JobEvent`]).
//! 1. Remove stale temporary files next to the outputs, and delete an
//!    existing primary output (Java does, so a failed run cannot leave a
//!    stale file behind).
//! 2. [`prepare_job`] (C03.6). An error ends the job as `Terminated`.
//! 3. Run [`run_pipeline`] (A13) on the prepared board with the job's cancel
//!    token and the job limit (`job_timeout`, capped at 24 h), which the
//!    pipeline hands to a `Watcher`: a limit only requests a full stop (D2).
//!    A progress sink listens to the router's events and times the passes.
//!    "All layers are disabled" is a `Terminated` job, as
//!    `AutorouteBatchLoop`'s `IllegalArgumentException`.
//! 4. The log: the loop's own pass lines are replaced by Java's
//!    (`Auto-routing pass #N on board '<hash>' was completed in X.XX seconds
//!    with score S`), and a run with the router enabled ends with the stage
//!    summary that `compare.py` parses.
//! 5. [`assign_terminal_state`]: `Completed`, `TimedOut` or `Cancelled`.
//! 6. For `Completed` and `TimedOut` only, write the outputs from the final
//!    board (after the best-board restore and the optimizer), the primary one
//!    first.
//!
//! The exit code is Java's `computeCliExitCode` ([`JobOutcome::exit_code`]):
//! 0 when the job is `Completed` or `TimedOut` and at least one output with
//! more than 0 bytes was written, else 1.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::deadline::{RunFacts, assign_terminal_state, terminate_on_error};
use super::log::{self, TIMEOUT_WARNING};
use super::outputs::{
    BeforeRename, OutputSource, OutputSpec, remove_stale_temp_files, serialize, write_atomic,
};
use super::prepare::{JobInput, PreparedJob, prepare_job};
use super::{LogLevel, LogLine};
use crate::autoroute::events::{
    CancelToken, ItemGeometry, Phase, Progress, RouterEvents, RunSummary,
};
use crate::autoroute::pipeline::{
    PipelineError, PipelineJob, PipelineOutcome, router_enabled, run_pipeline,
};
use crate::autoroute::stages::StageRegistry;
use crate::board::items::ItemId;
use crate::board::{BoardSnapshot, RoutingBoard};
use crate::scoring::BoardStatistics;
use crate::settings::{ResolveContext, RouterSettings, SettingsPatch};

/// `RoutingJobState`, the states a job passes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Running,
    Stopping,
    Completed,
    TimedOut,
    Cancelled,
    Terminated,
}

impl JobState {
    /// Java's enum name (`COMPLETED`, `TIMED_OUT`, ...).
    pub fn name(self) -> &'static str {
        match self {
            JobState::Running => "RUNNING",
            JobState::Stopping => "STOPPING",
            JobState::Completed => "COMPLETED",
            JobState::TimedOut => "TIMED_OUT",
            JobState::Cancelled => "CANCELLED",
            JobState::Terminated => "TERMINATED",
        }
    }

    /// True for the four states a finished job ends in.
    pub fn is_terminal(self) -> bool {
        !matches!(self, JobState::Running | JobState::Stopping)
    }
}

/// What a running job tells its observer (C08.3). The observer is called on
/// the job's thread, at work boundaries only, so the sequence of events does
/// not depend on time.
#[derive(Debug)]
pub enum JobEvent<'a> {
    /// The load sequence begins.
    Load,
    /// The router's counters (fanout, routing passes, optimizer).
    Progress(&'a Progress),
    /// The outputs are about to be written; the run's summary if the router
    /// ran.
    Write(Option<&'a RunSummary>),
    /// Log lines that have been added to the job's log since the last call.
    Log(&'a [LogLine]),
}

/// Receives the [`JobEvent`]s of a job.
pub type JobObserver = Arc<dyn for<'a> Fn(JobEvent<'a>) + Send + Sync>;

/// What a job runs with.
#[derive(Clone)]
pub struct JobContext {
    /// The machine facts of the settings merge (the core count).
    pub resolve: ResolveContext,
    /// The two-level stop shared with the stages (A08).
    pub cancel: Arc<CancelToken>,
    /// Set by [`cancel_by_user`](Self::cancel_by_user); a user cancel beats
    /// an expired job limit.
    pub cancelled_by_user: Arc<AtomicBool>,
    /// A hook between the temporary write and the rename of every output
    /// (tests stop the process there).
    pub before_rename: Option<BeforeRename>,
    /// Live events, for a progress display (C08.3). The job's log is still
    /// returned in [`JobOutcome::log`] whole.
    pub observer: Option<JobObserver>,
    /// Compute the board statistics of the loaded and the final board for
    /// [`JobOutcome::report`] (the result manifest, C08.4). They take about
    /// as long as a design rules check, so they are off by default.
    pub collect_report: bool,
    /// The stage registry the pipeline builds from. `None` is
    /// [`StageRegistry::builtin`] (Freerouting's own stages); a binary that
    /// also wants the KRT stages passes the registry `freerouting-stages`
    /// builds (N05.1). Registering a stage selects nothing: a job runs only
    /// the stages its settings name.
    pub registry: Option<Arc<StageRegistry>>,
}

impl JobContext {
    /// A context with a fresh token and no hook.
    pub fn new(resolve: ResolveContext) -> JobContext {
        JobContext {
            resolve,
            cancel: Arc::new(CancelToken::new()),
            cancelled_by_user: Arc::new(AtomicBool::new(false)),
            before_rename: None,
            observer: None,
            collect_report: false,
            registry: None,
        }
    }

    fn notify(&self, event: JobEvent<'_>) {
        if let Some(observer) = &self.observer {
            observer(event);
        }
    }

    /// Cancels the job as a user does: a full stop that is not a timeout.
    pub fn cancel_by_user(&self) {
        self.cancelled_by_user.store(true, Ordering::Release);
        self.cancel.stop_all();
    }
}

/// What a finished job leaves.
#[derive(Debug)]
pub struct JobOutcome {
    pub state: JobState,
    /// The files written and their sizes, in writing order.
    pub written: Vec<(PathBuf, u64)>,
    /// Java's log lines of the job, in order.
    pub log: Vec<LogLine>,
    /// The facts the result manifest is made of (C08.4).
    pub report: JobReport,
}

/// What a finished job knows for the result manifest (C08.4,
/// `RoutingResultManifest.fromJob`). The statistics are computed only when
/// [`JobContext::collect_report`] is set.
#[derive(Clone, Debug, Default)]
pub struct JobReport {
    /// The merged `result_json` setting (Java's `resultJsonPath`), known
    /// once the load sequence has run.
    pub result_json: Option<String>,
    /// The final settings with Java's nulls (the manifest's
    /// `settings_snapshot`).
    pub settings: Option<SettingsPatch>,
    /// The final settings as the router reads them.
    pub router_settings: Option<RouterSettings>,
    /// The board as loaded, before the fanout.
    pub loaded: Option<BoardStatistics>,
    /// The board that was written (or would have been).
    pub finished: Option<BoardStatistics>,
    /// What the router run reports, if it ran.
    pub summary: Option<RunSummary>,
    /// Routing passes completed (the pass log lines).
    pub passes_completed: usize,
    /// Passes the fanout completed, if it ran.
    pub fanout_passes: Option<u32>,
    /// Passes the optimizer completed, if it completed any.
    pub optimizer_passes: Option<u32>,
    /// Seconds from the start of the pipeline to the end of the fanout.
    pub fanout_seconds: Option<f64>,
    /// Seconds of the routing passes.
    pub router_seconds: Option<f64>,
    /// Seconds of the whole job.
    pub total_seconds: f64,
}

impl JobReport {
    /// Q02.2: the number of reported wide nets with copper under their class
    /// width, when `router.neck_down.strict` is on (`None` when it is off or
    /// the `widths` report did not run). The CLI exits 3 when it is above 0.
    pub fn strict_sizes_violations(&self) -> Option<usize> {
        let settings = self.router_settings.as_ref()?;
        let widths = self.summary.as_ref()?.widths.as_ref().map(|w| &w.widths);
        crate::quality::neckdown::ledger::strict_violation(settings, widths)
            .then(|| widths.map_or(0, |w| w.nets.iter().filter(|n| n.under > 0.0).count()))
    }
}

impl JobOutcome {
    /// `CliRunner.computeCliExitCode`, with "written" meaning more than 0
    /// bytes.
    pub fn exit_code(&self) -> i32 {
        let finished = matches!(self.state, JobState::Completed | JobState::TimedOut);
        let written = self.written.iter().any(|(_, size)| *size > 0);
        if finished && written { 0 } else { 1 }
    }
}

fn push(log: &mut Vec<LogLine>, level: LogLevel, text: impl Into<String>) {
    log.push(LogLine {
        level,
        text: text.into(),
    });
}

/// The router's event sink for a job (C07.8). It keeps the time of the last
/// progress event of every pass (and of the fanout) so the pass lines can say
/// how long a pass took; the board changes and snapshots are dropped, and the
/// final board is handed back to the caller.
///
/// It listens, so the board's change journal runs while the router works
/// (B01.7); that changes no result.
struct ProgressSink {
    start: Instant,
    fanout_end: Cell<Option<Instant>>,
    /// `(pass, time of its last progress event)`, in pass order.
    pass_ends: RefCell<Vec<(i32, Instant)>>,
    /// The job's observer, which sees every progress event as it happens.
    observer: Option<JobObserver>,
    /// True once an optimizer event went by.
    optimize_seen: Cell<bool>,
}

impl ProgressSink {
    fn new(observer: Option<JobObserver>) -> ProgressSink {
        ProgressSink {
            start: Instant::now(),
            fanout_end: Cell::new(None),
            pass_ends: RefCell::new(Vec::new()),
            observer,
            optimize_seen: Cell::new(false),
        }
    }

    /// Seconds from the start to the last fanout event, if the fanout ran.
    fn fanout_seconds(&self) -> Option<f64> {
        self.fanout_end
            .get()
            .map(|end| end.saturating_duration_since(self.start).as_secs_f64())
    }

    /// Seconds from the end of the fanout (or the start) to the last
    /// routing event, if a pass ran.
    fn router_seconds(&self) -> Option<f64> {
        let last = self.pass_ends.borrow().last().map(|(_, at)| *at)?;
        let begin = self.fanout_end.get().unwrap_or(self.start);
        Some(last.saturating_duration_since(begin).as_secs_f64())
    }

    /// The seconds pass `pass` took: from the end of the previous pass (or of
    /// the fanout, or the start of the run) to its last progress event.
    fn pass_seconds(&self, pass: i32) -> f64 {
        let ends = self.pass_ends.borrow();
        let Some(index) = ends.iter().position(|(p, _)| *p == pass) else {
            return 0.0;
        };
        let begin = if index > 0 {
            ends[index - 1].1
        } else {
            self.fanout_end.get().unwrap_or(self.start)
        };
        ends[index].1.saturating_duration_since(begin).as_secs_f64()
    }
}

impl RouterEvents for ProgressSink {
    fn is_listening(&self) -> bool {
        true
    }

    fn progress(&self, progress: Progress) -> bool {
        let now = Instant::now();
        match progress.phase {
            Phase::Fanout => self.fanout_end.set(Some(now)),
            Phase::Autoroute => {
                let mut ends = self.pass_ends.borrow_mut();
                match ends.last_mut() {
                    Some(last) if last.0 == progress.pass => last.1 = now,
                    _ => ends.push((progress.pass, now)),
                }
            }
            Phase::Optimize => self.optimize_seen.set(true),
        }
        if let Some(observer) = &self.observer {
            observer(JobEvent::Progress(&progress));
        }
        true
    }

    fn delta(&self, _removed: Vec<ItemId>, _added: Vec<ItemGeometry>) {}

    fn replaced(&self, _snapshot: BoardSnapshot) {}

    fn finished(&self, board: RoutingBoard, _summary: RunSummary) -> Option<RoutingBoard> {
        Some(board)
    }
}

/// The loop's log with Java's pass lines (`was completed in X.XX seconds`) in
/// place of its own, and the pipeline's other lines as they are.
fn pipeline_lines(outcome: &PipelineOutcome, sink: &ProgressSink) -> Vec<LogLine> {
    let mut passes = outcome.passes.iter();
    outcome
        .log
        .iter()
        .map(|line| {
            if line.text.starts_with("Auto-routing pass #")
                && let Some(record) = passes.next()
            {
                return LogLine {
                    level: line.level,
                    text: log::pass_line(
                        record.pass,
                        &format!("{:032x}", record.hash),
                        sink.pass_seconds(i32::try_from(record.pass).unwrap_or(i32::MAX)),
                        record.score,
                        to_u32(record.incomplete),
                        to_u32(record.violations),
                    ),
                };
            }
            line.clone()
        })
        .collect()
}

/// The stage summary of `RoutingJobSchedulerActionThread` (`isTimedOut` and
/// the status words of the stage listener).
fn stage_summary(
    outcome: &PipelineOutcome,
    ctx: &JobContext,
    started: Instant,
    job_deadline: Option<Duration>,
) -> String {
    let summary = &outcome.summary;
    let clock_expired = job_deadline.is_some_and(|d| started.elapsed() >= d);
    let status = if clock_expired && ctx.cancel.is_stop_requested() {
        "completed with timeout"
    } else if ctx.cancel.is_stop_requested() {
        if ctx.cancelled_by_user.load(Ordering::Acquire) {
            "cancelled"
        } else {
            "interrupted"
        }
    } else {
        "completed"
    };
    log::stage_summary_line(
        status,
        to_u32(summary.initial_unrouted),
        started.elapsed().as_secs_f64(),
        summary.score,
        to_u32(summary.incomplete),
        to_u32(summary.violations),
    )
}

/// The lines every run starts with after the load: the effective
/// `start_ripup_costs` and where it came from (D8: Dan's `freerouting.json`
/// sets it), and the warning for an `ignore_net_classes` list whose parts
/// joined with `,` are the name of a net class on the board (the comma
/// split of Java's `-inc Power,Default` is a mistake when KiCad's class is
/// called `Power,Default`; `-inc 'Power\,Default'` keeps the comma).
fn settings_lines(prepared: &PreparedJob) -> Vec<LogLine> {
    let mut lines = Vec::new();
    let source = prepared
        .provenance
        .get("scoring.start_ripup_costs")
        .map_or("defaults", |s| s.label());
    lines.push(LogLine {
        level: LogLevel::Info,
        text: format!(
            "Effective start_ripup_costs: {} (source: {source})",
            prepared.settings.scoring.start_ripup_costs
        ),
    });
    let names = prepared.settings.autorouter.ignore_net_classes.clone();
    if names.len() >= 2 {
        let joined = names.join(",");
        let classes = &prepared.board.rules.net_classes;
        let matches = (0..classes.count())
            .filter_map(|index| classes.nth(index))
            .any(|id| classes.get(id).name().eq_ignore_ascii_case(&joined));
        if matches {
            lines.push(LogLine {
                level: LogLevel::Warn,
                text: format!(
                    "ignore_net_classes names {} separately, but the board has a net class \
                     named '{joined}'; write it as '{}' to keep the comma",
                    names
                        .iter()
                        .map(|n| format!("'{n}'"))
                        .collect::<Vec<_>>()
                        .join(" and "),
                    joined.replace(',', "\\,")
                ),
            });
        }
    }
    lines
}

fn to_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Sends the log lines added since `sent` to the observer.
fn flush_log(ctx: &JobContext, log: &[LogLine], sent: &mut usize) {
    if *sent < log.len() {
        ctx.notify(JobEvent::Log(&log[*sent..]));
        *sent = log.len();
    }
}

/// Runs a job: prepare, route ([`run_pipeline`]) and write the outputs.
/// The first of `outputs` is the primary one.
///
/// A full stop that is requested before the router starts (during the load)
/// ends the job as `Cancelled` with nothing written (C08.1, exit code 130).
pub fn run_job(input: JobInput, outputs: &[OutputSpec], ctx: &JobContext) -> JobOutcome {
    let mut log = Vec::new();
    let mut sent = 0;
    let mut report = JobReport::default();
    let started = Instant::now();
    ctx.notify(JobEvent::Load);
    // 1. Stale temporary files, and `CliRunner`'s delete of the old output.
    for output in outputs {
        remove_stale_temp_files(&output.path);
    }
    if let Some(primary) = outputs.first()
        && primary.path.exists()
        && let Err(e) = std::fs::remove_file(&primary.path)
    {
        push(
            &mut log,
            LogLevel::Warn,
            format!("Couldn't delete the file '{}': {e}", primary.path.display()),
        );
    }

    let design_name = input.design_name.clone();
    let kicad_source = input.kicad_source.clone();

    // 2. The load sequence.
    let load_scope = crate::run_stats::stage("load", 0);
    let prepared = match prepare_job(&input, &ctx.resolve) {
        Ok(prepared) => {
            load_scope.finish();
            prepared
        }
        Err(e) => {
            push(&mut log, LogLevel::Error, e.to_string());
            flush_log(ctx, &log, &mut sent);
            report.total_seconds = started.elapsed().as_secs_f64();
            return JobOutcome {
                state: terminate_on_error(JobState::Running),
                written: Vec::new(),
                log,
                report,
            };
        }
    };
    // P03: the analyses (statistics, DRC, airlines) of this job run on at
    // most `autorouter.max_threads` threads; no result depends on it.
    let _threads = crate::parallel::ThreadBudget::set(
        usize::try_from(prepared.settings.autorouter_max_threads(ctx.resolve.cores)).unwrap_or(1),
    );
    log.extend(prepared.log.iter().cloned());
    log.extend(settings_lines(&prepared));
    report.result_json = prepared.settings.result_json.clone();
    report.settings = Some(prepared.patch.clone());
    report.router_settings = Some(prepared.settings.clone());
    if ctx.collect_report {
        report.loaded = Some(BoardStatistics::new(&prepared.board));
    }
    if ctx.cancel.is_stop_requested() {
        push(
            &mut log,
            LogLevel::Warn,
            format!("Job '{design_name}' was stopped while it was loading; nothing is written."),
        );
        push(
            &mut log,
            LogLevel::Info,
            format!(
                "Job '{design_name}' finished with state: {} (elapsed: {}).",
                JobState::Cancelled.name(),
                log::format_duration(started.elapsed().as_secs_f64())
            ),
        );
        flush_log(ctx, &log, &mut sent);
        report.total_seconds = started.elapsed().as_secs_f64();
        return JobOutcome {
            state: JobState::Cancelled,
            written: Vec::new(),
            log,
            report,
        };
    }
    flush_log(ctx, &log, &mut sent);

    // 3. The pipeline. The job limit goes to the pipeline's watcher, which
    // only requests a full stop.
    let job_deadline = prepared.settings.deadlines().job;
    let sink = ProgressSink::new(ctx.observer.clone());
    let PreparedJob {
        board,
        settings,
        board_settings,
        ..
    } = prepared;
    let job = PipelineJob {
        settings: &settings,
        board_settings: &board_settings,
        cancel: Arc::clone(&ctx.cancel),
        events: &sink,
        deadline: job_deadline,
        registry: ctx.registry.as_deref(),
    };
    let mut state = JobState::Running;
    let mut final_board = None;
    let mut summary = None;
    match run_pipeline(board, &job) {
        Ok(outcome) => {
            log.extend(pipeline_lines(&outcome, &sink));
            if router_enabled(&settings) && outcome.loop_ran {
                push(
                    &mut log,
                    LogLevel::Info,
                    stage_summary(&outcome, ctx, started, job_deadline),
                );
            }
            // An optimizer that completed no pass (the unported one) has not
            // run as far as a listener or the manifest is concerned.
            if let Some(optimizer) = outcome.optimizer.as_ref().filter(|o| o.passes > 0)
                && !sink.optimize_seen.get()
                && let Some(observer) = &ctx.observer
            {
                // The optimizer sends no events yet (O06): one at its end.
                observer(JobEvent::Progress(&Progress {
                    phase: Phase::Optimize,
                    pass: i32::try_from(optimizer.passes).unwrap_or(i32::MAX),
                    incomplete: outcome.summary.incomplete,
                    violations: outcome.summary.violations,
                    score: outcome.summary.score,
                    ..Progress::default()
                }));
            }
            report.passes_completed = outcome.passes.len();
            report.fanout_passes = outcome.fanout.as_ref().map(|f| f.completed_passes);
            report.optimizer_passes = outcome
                .optimizer
                .as_ref()
                .map(|o| o.passes)
                .filter(|passes| *passes > 0);
            report.fanout_seconds = sink.fanout_seconds();
            report.router_seconds = sink.router_seconds();
            summary = Some(outcome.summary.clone());
            final_board = outcome.board;
        }
        Err(failure) => {
            let message = failure.error.to_string();
            if matches!(failure.error, PipelineError::NoRoutableLayer(_)) {
                push(&mut log, LogLevel::Warn, message.clone());
            }
            push(
                &mut log,
                LogLevel::Error,
                format!("Job '{design_name}' terminated due to an error: {message}"),
            );
            state = terminate_on_error(state);
        }
    }
    if final_board.is_none() && state == JobState::Running {
        push(
            &mut log,
            LogLevel::Error,
            format!("Job '{design_name}' terminated due to an error: no board came back"),
        );
        state = terminate_on_error(state);
    }
    flush_log(ctx, &log, &mut sent);

    // 4. The terminal state.
    let stop_requested = ctx.cancel.is_stop_requested();
    let facts = RunFacts {
        cancelled_by_user: ctx.cancelled_by_user.load(Ordering::Acquire),
        clock_expired: job_deadline.is_some_and(|d| started.elapsed() >= d),
        stop_requested,
    };
    state = assign_terminal_state(state, facts);
    if state == JobState::TimedOut {
        push(&mut log, LogLevel::Warn, TIMEOUT_WARNING);
    }

    // 6. The outputs.
    let mut written = Vec::new();
    if matches!(state, JobState::Completed | JobState::TimedOut)
        && let Some(final_board) = &final_board
    {
        if crate::run_stats::enabled() {
            // P01.2: the statistics time one full clearance check of the
            // board that is about to be written.
            let scope = crate::run_stats::stage("drc", 0);
            let _ = crate::drc::DesignRulesChecker::new(final_board).all_clearance_violations();
            scope.finish();
        }
        let write_scope = crate::run_stats::stage("write", 0);
        ctx.notify(JobEvent::Write(summary.as_ref()));
        let source = OutputSource {
            board: final_board,
            design_name: &design_name,
            settings: &settings,
            kicad_source: kicad_source.as_ref(),
        };
        write_outputs(outputs, &source, ctx, &mut written, &mut log);
        write_scope.finish();
    }
    if ctx.collect_report
        && let Some(final_board) = &final_board
    {
        report.finished = Some(BoardStatistics::new(final_board));
    }
    report.summary = summary;
    push(
        &mut log,
        LogLevel::Info,
        format!(
            "Job '{design_name}' finished with state: {} (elapsed: {}).",
            state.name(),
            log::format_duration(started.elapsed().as_secs_f64())
        ),
    );
    flush_log(ctx, &log, &mut sent);
    report.total_seconds = started.elapsed().as_secs_f64();
    JobOutcome {
        state,
        written,
        log,
        report,
    }
}

/// `writeCliOutputIfAvailable`: the primary output, then the others. A
/// failure is logged and does not stop the rest.
fn write_outputs(
    outputs: &[OutputSpec],
    source: &OutputSource<'_>,
    ctx: &JobContext,
    written: &mut Vec<(PathBuf, u64)>,
    log: &mut Vec<LogLine>,
) {
    for (index, output) in outputs.iter().enumerate() {
        let additional = index > 0;
        if !additional {
            push(
                log,
                LogLevel::Info,
                log::saving_line(&output.path, output.format.name()),
            );
        }
        let result = write_atomic(
            &output.path,
            output.format,
            additional,
            ctx.before_rename,
            |writer| serialize(output.format, source, writer),
        );
        match result {
            Ok(size) => {
                push(
                    log,
                    LogLevel::Info,
                    log::saved_line(&output.path, size, additional),
                );
                written.push((output.path.clone(), size));
            }
            Err(e) => push(log, LogLevel::Error, e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(state: JobState, sizes: &[u64]) -> JobOutcome {
        JobOutcome {
            state,
            written: sizes.iter().map(|s| (PathBuf::from("x"), *s)).collect(),
            log: Vec::new(),
            report: JobReport::default(),
        }
    }

    /// `CliRunner.computeCliExitCode`, with "written" meaning more than 0
    /// bytes.
    #[test]
    fn exit_code_follows_the_state_and_the_written_files() {
        use JobState::*;
        assert_eq!(outcome(Completed, &[10]).exit_code(), 0);
        assert_eq!(outcome(TimedOut, &[10]).exit_code(), 0);
        assert_eq!(outcome(Completed, &[0, 5]).exit_code(), 0);
        assert_eq!(outcome(Completed, &[]).exit_code(), 1);
        assert_eq!(outcome(Completed, &[0]).exit_code(), 1);
        assert_eq!(outcome(TimedOut, &[]).exit_code(), 1);
        assert_eq!(outcome(Cancelled, &[10]).exit_code(), 1);
        assert_eq!(outcome(Terminated, &[10]).exit_code(), 1);
    }

    #[test]
    fn states_have_javas_names() {
        assert_eq!(JobState::TimedOut.name(), "TIMED_OUT");
        assert!(JobState::Cancelled.is_terminal());
        assert!(!JobState::Stopping.is_terminal());
    }
}

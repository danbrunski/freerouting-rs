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
//! 1. Remove stale temporary files next to the outputs, and delete an
//!    existing primary output (Java does, so a failed run cannot leave a
//!    stale file behind).
//! 2. [`prepare_job`] (C03.6). An error ends the job as `Terminated`.
//! 3. Arm the job limit (`job_timeout`, capped at 24 h) on a [`Watcher`].
//! 4. Run the pipeline. **Until C07.8 the pipeline is a stub** that leaves
//!    the board as it is (it logs that, and the stage summary a run with the
//!    router enabled prints). "All layers are disabled" is a `Terminated`
//!    job, as `AutorouteBatchLoop`'s `IllegalArgumentException`.
//! 5. [`assign_terminal_state`]: `Completed`, `TimedOut` or `Cancelled`.
//! 6. For `Completed` and `TimedOut` only, write the outputs from the final
//!    board, the primary one first.
//!
//! The exit code is Java's `computeCliExitCode` ([`JobOutcome::exit_code`]):
//! 0 when the job is `Completed` or `TimedOut` and at least one output with
//! more than 0 bytes was written, else 1.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::deadline::{RunFacts, StopLevel, Watcher, assign_terminal_state, terminate_on_error};
use super::log::{self, TIMEOUT_WARNING};
use super::outputs::{
    BeforeRename, OutputSource, OutputSpec, remove_stale_temp_files, serialize, write_atomic,
};
use super::prepare::{JobInput, PreparedJob, prepare_job};
use super::{LogLevel, LogLine};
use crate::autoroute::events::CancelToken;
use crate::scoring::BoardStatistics;
use crate::settings::ResolveContext;

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
}

impl JobContext {
    /// A context with a fresh token and no hook.
    pub fn new(resolve: ResolveContext) -> JobContext {
        JobContext {
            resolve,
            cancel: Arc::new(CancelToken::new()),
            cancelled_by_user: Arc::new(AtomicBool::new(false)),
            before_rename: None,
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

/// The stub pipeline of C07: it leaves the board as it is. With the router
/// enabled it logs a warning and the stage summary of a stage that ran no
/// pass (`started with N unrouted ... final score`). C07.8 replaces it by A13's
/// `run_pipeline`, which takes the same prepared job, the work bounds, the
/// cancel token and a progress sink.
///
/// Returns `Err` for what Java throws (`AutorouteBatchLoop`: all layers
/// disabled), which ends the job as `Terminated`.
fn run_pipeline_stub(
    prepared: &PreparedJob,
    ctx: &JobContext,
    started: Instant,
    job_deadline: Option<Duration>,
    log: &mut Vec<LogLine>,
) -> Result<(), String> {
    let settings = &prepared.settings;
    let router_enabled = settings.run_router() && settings.autorouter.max_passes >= 0;
    if !router_enabled {
        return Ok(());
    }
    let board = &prepared.board;
    let any_routable = (0..prepared.board_settings.layer_count()).any(|layer| {
        prepared.board_settings.layer_active(layer)
            && board
                .layer_structure
                .layers
                .get(layer)
                .is_some_and(|l| l.is_signal)
    });
    if !any_routable {
        push(
            log,
            LogLevel::Warn,
            "Cannot start autorouter: all layers are disabled.",
        );
        return Err("Cannot start autorouter: all layers are disabled.".to_string());
    }
    push(
        log,
        LogLevel::Warn,
        "routing is not implemented yet (C07.8); the board is written as loaded",
    );
    let stats = BoardStatistics::new(board);
    let unrouted = to_u32(stats.connections.incomplete_count);
    let violations = to_u32(stats.clearance_violations.total_count);
    let score = stats.router_score(Some(settings));
    // `isTimedOut` and the status words of the stage listener.
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
    push(
        log,
        LogLevel::Info,
        log::stage_summary_line(
            status,
            unrouted,
            started.elapsed().as_secs_f64(),
            score,
            unrouted,
            violations,
        ),
    );
    Ok(())
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

/// Runs a job: prepare, route (a stub until C07.8) and write the outputs.
/// The first of `outputs` is the primary one.
pub fn run_job(input: JobInput, outputs: &[OutputSpec], ctx: &JobContext) -> JobOutcome {
    let mut log = Vec::new();
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

    // 2. The load sequence.
    let prepared = match prepare_job(&input, &ctx.resolve) {
        Ok(prepared) => prepared,
        Err(e) => {
            push(&mut log, LogLevel::Error, e.to_string());
            return JobOutcome {
                state: terminate_on_error(JobState::Running),
                written: Vec::new(),
                log,
            };
        }
    };
    log.extend(prepared.log.iter().cloned());
    log.extend(settings_lines(&prepared));

    // 3. The job limit.
    let started = Instant::now();
    let job_deadline = prepared.settings.deadlines().job;
    let watcher = Watcher::new(Arc::clone(&ctx.cancel));
    if let Some(limit) = job_deadline {
        watcher.arm(limit, StopLevel::All);
    }

    // 4. The pipeline.
    let mut state = JobState::Running;
    if let Err(message) = run_pipeline_stub(&prepared, ctx, started, job_deadline, &mut log) {
        push(
            &mut log,
            LogLevel::Error,
            format!("Job '{design_name}' terminated due to an error: {message}"),
        );
        state = terminate_on_error(state);
    }

    // 5. The terminal state.
    let _ = watcher.finish();
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
    if matches!(state, JobState::Completed | JobState::TimedOut) {
        let source = OutputSource {
            board: &prepared.board,
            design_name: &design_name,
            settings: &prepared.settings,
        };
        write_outputs(outputs, &source, ctx, &mut written, &mut log);
    }
    push(
        &mut log,
        LogLevel::Info,
        format!(
            "Job '{design_name}' finished with state: {} (elapsed: {}).",
            state.name(),
            log::format_duration(started.elapsed().as_secs_f64())
        ),
    );
    JobOutcome {
        state,
        written,
        log,
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

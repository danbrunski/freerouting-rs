//! Stopping a run (C08.5): `stop` and `stop-router` on standard input, the
//! interrupt policy, and the end of input.
//!
//! Java's `StoppableThreadTest` is ported against `CancelToken` in
//! `crates/freerouting/tests/java_ports.rs` (A08.5); here the two levels are
//! driven by the stdin commands.
//!
//! Signals (Ctrl-C, SIGTERM) go through the `ctrlc` crate (K02, the operator
//! approved the `NEW DEP`): `sigint_and_sigterm_are_a_stop` and
//! `two_signals_abort` send real signals to the binary (Unix only; Windows
//! uses the same handler for Ctrl-C and Ctrl-Break). The in-process tests
//! inject the interrupts through the test hook `FREEROUTING_TEST_INTERRUPT`
//! (`stop::TestHook`), which calls the same `Interrupts::interrupt` the
//! handler does.

mod common;

use std::cell::Cell;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;

use common::*;
use freerouting::autoroute::events::{
    CancelToken, ItemGeometry, Phase, Progress, RouterEvents, RunSummary,
};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{
    DEFAULT_OPTIMIZER_ALGORITHM, Optimizer, OptimizerSummary, StageCtx, StageRegistry,
};
use freerouting::board::items::ItemId;
use freerouting::board::{BoardSnapshot, RoutingBoard};
use freerouting::drc::json::JsonValue;
use freerouting::io::specctra::read_session;
use freerouting::io::{BoardReadResult, read_board};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::settings::{ResolveContext, RouterSettings};
use freerouting_cli::stop::{Command as StopCommand, read_commands};

const SMALL: &str = "Issue026-J2_reference.dsn";
const BIG: &str = "Issue508-DAC2020_bm01.dsn";

/// The binary with piped stdin and stderr, in a fresh directory holding
/// `fixture`.
fn spawn(
    name: &str,
    fixture: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> (std::process::Child, std::path::PathBuf) {
    let dir = scratch(name);
    copy_fixtures(&dir, &[fixture]);
    let mut command = Command::new(env!("CARGO_BIN_EXE_freerouting-rs"));
    command
        .args(["-de", fixture, "-do", "out.ses"])
        .args(args)
        .current_dir(&dir)
        .env("XDG_CONFIG_HOME", dir.join("no-config"))
        .env_remove("FREEROUTING_TEST_ABORT_BEFORE_RENAME")
        .env_remove("FREEROUTING_TEST_INTERRUPT")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    (command.spawn().expect("the binary runs"), dir)
}

fn names_in(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn stdin_eof_is_not_a_stop() {
    let (mut child, dir) = spawn("stop-eof", SMALL, &["-mp", "2"], &[]);
    drop(child.stdin.take()); // EOF at once
    let output = child.wait_with_output().unwrap();
    let log = stderr(&output);
    assert_eq!(code(&output), 0, "{log}");
    assert!(log.contains("finished with state: COMPLETED"), "{log}");
    // The router ran as usual: both passes.
    let passes = log
        .lines()
        .filter(|l| l.starts_with("Auto-routing pass #"))
        .count();
    assert_eq!(passes, 2, "{log}");
    assert!(dir.join("out.ses").metadata().unwrap().len() > 0);
}

/// `stop` after the first `route` progress event: the run ends early, the
/// board is written, and the exit code is 0.
#[test]
fn stop_line_keeps_the_board() {
    let (mut child, dir) = spawn("stop-line", BIG, &["-mp", "50", "--progress", "json"], &[]);
    let mut stdin = child.stdin.take().unwrap();
    let reader = BufReader::new(child.stderr.take().unwrap());
    let mut lines = Vec::new();
    let mut sent = false;
    for line in reader.lines() {
        let line = line.unwrap();
        if !sent && line.contains("\"phase\":\"route\"") {
            writeln!(stdin, "stop").unwrap();
            stdin.flush().unwrap();
            sent = true;
        }
        lines.push(line);
    }
    assert!(sent, "no route event: {lines:#?}");
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0), "{lines:#?}");

    let done = JsonValue::parse(lines.last().unwrap()).unwrap();
    assert_eq!(string_of(member(&done, "event")).as_deref(), Some("done"));
    assert_eq!(
        string_of(member(&done, "state")).as_deref(),
        Some("completed")
    );
    // The run ends early: unstopped it takes 21 passes (about a minute).
    let passes = lines
        .iter()
        .filter(|l| l.contains("Auto-routing pass #"))
        .count();
    assert!(passes <= 1, "{passes} passes");

    // The SES imports into the board it was routed from.
    let ses = std::fs::read_to_string(dir.join("out.ses")).unwrap();
    let bytes = std::fs::read(dir.join(BIG)).unwrap();
    let BoardReadResult::Success { mut board, .. } = read_board(&bytes) else {
        panic!("{BIG} does not load");
    };
    let summary = read_session(&mut board, &ses).expect("the SES is a session file");
    assert_eq!(summary.errors_encountered, 0);
    assert_eq!(names_in(&dir), [BIG, "out.ses"]);
}

// ---- stop-router lets the optimizer run ---------------------------------

thread_local! {
    static OPTIMIZER_RAN: Cell<bool> = const { Cell::new(false) };
}

/// An optimizer that only records that it ran.
struct ProbeOptimizer;

impl Optimizer for ProbeOptimizer {
    fn id(&self) -> &'static str {
        DEFAULT_OPTIMIZER_ALGORITHM
    }
    fn run(&mut self, _: &mut RoutingBoard, _: &mut StageCtx) -> OptimizerSummary {
        OPTIMIZER_RAN.with(|ran| ran.set(true));
        let mut summary = OptimizerSummary::default();
        summary.passes = 1;
        summary
    }
}

/// A listening sink that feeds one stdin line to the token at the first
/// routing event, as the reader thread does.
struct StdinAtFirstRouteEvent {
    token: Arc<CancelToken>,
    line: Option<&'static str>,
    fired: Cell<bool>,
}

impl RouterEvents for StdinAtFirstRouteEvent {
    fn is_listening(&self) -> bool {
        true
    }
    fn progress(&self, progress: Progress) -> bool {
        if let Some(line) = self.line
            && progress.phase == Phase::Autoroute
            && !self.fired.replace(true)
        {
            read_commands(Cursor::new(format!("{line}\n")), &self.token, |_| {});
        }
        true
    }
    fn delta(&self, _: Vec<ItemId>, _: Vec<ItemGeometry>) {}
    fn replaced(&self, _: BoardSnapshot) {}
    fn finished(&self, board: RoutingBoard, _: RunSummary) -> Option<RoutingBoard> {
        Some(board)
    }
}

/// Runs the pipeline on `SMALL` with the probe optimizer, feeding `line` to
/// the token at the first routing event. Returns `(passes, optimizer ran)`.
fn pipeline_with_stdin_line(line: Option<&'static str>) -> (usize, bool) {
    OPTIMIZER_RAN.with(|ran| ran.set(false));
    let bytes = std::fs::read(repo("fixtures").join(SMALL)).unwrap();
    let prepared = prepare_job(
        &JobInput::new(bytes, JobInput::design_name_of(SMALL)),
        &ResolveContext { cores: 4 },
    )
    .expect("the board prepares");
    let settings: &RouterSettings = &prepared.settings;
    assert!(settings.run_optimizer());
    let mut registry = StageRegistry::builtin();
    registry
        .optimizer
        .insert(DEFAULT_OPTIMIZER_ALGORITHM, |_| Box::new(ProbeOptimizer));
    let token = Arc::new(CancelToken::new());
    let sink = StdinAtFirstRouteEvent {
        token: Arc::clone(&token),
        line,
        fired: Cell::new(false),
    };
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: token,
        events: &sink,
        deadline: None,
        registry: Some(&registry),
    };
    let outcome = run_pipeline(prepared.board, &job).expect("the pipeline runs");
    (outcome.passes.len(), OPTIMIZER_RAN.with(Cell::get))
}

#[test]
fn stop_router_lets_the_optimizer_run() {
    let (passes, ran) = pipeline_with_stdin_line(None);
    assert_eq!(passes, 2, "J2 needs two passes");
    assert!(ran, "the optimizer runs after a normal router");

    // `stop-router`: the router ends early, the optimizer still runs.
    let (passes, ran) = pipeline_with_stdin_line(Some("stop-router"));
    assert!(passes < 2, "the router stopped: {passes} passes");
    assert!(ran, "the optimizer ran after stop-router");

    // `stop`: everything ends, the optimizer is skipped.
    let (_, ran) = pipeline_with_stdin_line(Some("stop"));
    assert!(!ran, "stop ends the optimizer too");
}

// ---- interrupts ------------------------------------------------------------

#[test]
fn second_interrupt_aborts() {
    for spec in ["route:2", "load:2", "load:1"] {
        let (mut child, dir) = spawn(
            &format!("stop-abort-{}", spec.replace(':', "-")),
            SMALL,
            &["-mp", "50"],
            &[("FREEROUTING_TEST_INTERRUPT", spec)],
        );
        drop(child.stdin.take());
        let output = child.wait_with_output().unwrap();
        let log = stderr(&output);
        assert_eq!(code(&output), 130, "{spec}: {log}");
        // No output and no temporary file.
        assert_eq!(names_in(&dir), [SMALL], "{spec}: {log}");
        assert!(log.contains("CANCELLED"), "{spec}: {log}");
    }
}

#[test]
fn one_interrupt_is_a_stop() {
    let (mut child, dir) = spawn(
        "stop-one-interrupt",
        BIG,
        &["-mp", "50"],
        &[("FREEROUTING_TEST_INTERRUPT", "route:1")],
    );
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    let log = stderr(&output);
    assert_eq!(code(&output), 0, "{log}");
    assert!(dir.join("out.ses").metadata().unwrap().len() > 0);
}

#[test]
fn a_manifest_of_an_aborted_run_says_so() {
    let (mut child, dir) = spawn(
        "stop-abort-manifest",
        SMALL,
        &["-mp", "50", "--result-json", "r.json"],
        &[("FREEROUTING_TEST_INTERRUPT", "load:2")],
    );
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(code(&output), 130, "{}", stderr(&output));
    let manifest = JsonValue::parse(&std::fs::read_to_string(dir.join("r.json")).unwrap()).unwrap();
    assert_eq!(
        string_of(member(&manifest, "final_state")).as_deref(),
        Some("CANCELLED")
    );
    assert_eq!(
        member(&manifest, "output_written"),
        Some(&JsonValue::Bool(false))
    );
    assert_eq!(
        member(&manifest, "exit_code"),
        Some(&JsonValue::Number("130".to_string()))
    );
    // Silence the unused import of the enum under the stop module.
    let _ = StopCommand::Stop;
}

// ---- real signals (C08.5 remainder, K02) -----------------------------------

/// Runs the binary on `BIG` with `--progress json` and sends each of `signals`
/// (as `kill -SIG`, back to back) when the first `route` event arrives.
/// Returns the stderr lines, the exit status and the directory.
#[cfg(unix)]
fn run_with_signals(
    name: &str,
    signals: &[&str],
) -> (Vec<String>, std::process::ExitStatus, std::path::PathBuf) {
    let (mut child, dir) = spawn(name, BIG, &["-mp", "50", "--progress", "json"], &[]);
    let _stdin = child.stdin.take(); // stays open: EOF is not a stop either way
    let reader = BufReader::new(child.stderr.take().unwrap());
    let mut lines = Vec::new();
    let mut sent = false;
    for line in reader.lines() {
        let line = line.unwrap();
        if !sent && line.contains("\"phase\":\"route\"") {
            for signal in signals {
                let status = Command::new("kill")
                    .arg(format!("-{signal}"))
                    .arg(child.id().to_string())
                    .status()
                    .expect("kill runs");
                assert!(status.success(), "kill -{signal}");
            }
            sent = true;
        }
        lines.push(line);
    }
    assert!(sent, "no route event: {lines:#?}");
    let status = child.wait().unwrap();
    (lines, status, dir)
}

/// The first Ctrl-C or SIGTERM is `stop`: the best board is written, exit 0.
#[cfg(unix)]
#[test]
fn sigint_and_sigterm_are_a_stop() {
    for signal in ["INT", "TERM"] {
        let (lines, status, dir) = run_with_signals(&format!("stop-sig-{signal}"), &[signal]);
        assert_eq!(status.code(), Some(0), "SIG{signal}: {lines:#?}");
        let done = JsonValue::parse(lines.last().unwrap()).unwrap();
        assert_eq!(
            string_of(member(&done, "state")).as_deref(),
            Some("completed"),
            "SIG{signal}"
        );
        let passes = lines
            .iter()
            .filter(|l| l.contains("Auto-routing pass #"))
            .count();
        assert!(passes <= 1, "SIG{signal}: {passes} passes");
        let ses = std::fs::read_to_string(dir.join("out.ses")).unwrap();
        let bytes = std::fs::read(dir.join(BIG)).unwrap();
        let BoardReadResult::Success { mut board, .. } = read_board(&bytes) else {
            panic!("{BIG} does not load");
        };
        let summary = read_session(&mut board, &ses).expect("the SES is a session file");
        assert_eq!(summary.errors_encountered, 0);
        assert_eq!(names_in(&dir), [BIG, "out.ses"], "SIG{signal}");
    }
}

/// The second signal aborts: exit 130, no output file, no temporary file.
#[cfg(unix)]
#[test]
fn two_signals_abort() {
    let (lines, status, dir) = run_with_signals("stop-sig-two", &["INT", "INT"]);
    assert_eq!(status.code(), Some(130), "{lines:#?}");
    assert_eq!(names_in(&dir), [BIG], "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("CANCELLED")), "{lines:#?}");
}

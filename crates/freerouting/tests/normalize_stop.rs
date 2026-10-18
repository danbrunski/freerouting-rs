//! A stop request ends a trace normalization early
//! (`BasicBoard::normalize_traces_until`).
//!
//! The insertion after each route normalizes the whole net, and a net whose
//! split and combine oscillate runs all 2000 passes of Java's iteration
//! limit: on `Issue690-kit-dev-coldfire-xilinx_5213` the GND net (289
//! traces) took about 280 s inside one fanout pin, with no stop check, so
//! `timeout 110 freerouting -de ...` returned after 433 s. The stop is
//! checked before every pass after the first and before every trace of
//! those passes; the first pass always runs to its end. Without a stop
//! request the result is `normalize_traces`'s.
//!
//! The board is `tests/data/normalize-stale-trace.dsn` with its wire inserted
//! again (as `normalize_stale_trace.rs` does): its normalization changes
//! something in the first pass, so a second pass runs.
//!
//! `stop_ends_an_oscillating_normalization_in_the_fanout` is the end-to-end
//! check on coldfire (release, `--ignored`).

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use freerouting::autoroute::events::{
    CancelToken, ItemGeometry, Phase, Progress, RouterEvents, RunSummary,
};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::board::{BoardSnapshot, RoutingBoard};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{IntPoint, Point, Polyline, Stoppable};
use freerouting::io::specctra::{BoardReadResult, read_board_from_str};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::settings::ResolveContext;

const DSN: &str = include_str!("data/normalize-stale-trace.dsn");

/// The wire of the DSN in board units (as in `normalize_stale_trace.rs`).
const WIRE: [(i32, i32); 6] = [
    (1246000, -650750),
    (1246000, -650749),
    (1230000, -650750),
    (1293500, -650750),
    (1278000, -650750),
    (1278000, -650749),
];

/// The board without its wiring, with the wire inserted unnormalized.
fn board_with_wire() -> RoutingBoard {
    let wiring = DSN.find("  (wiring").expect("the DSN has a wiring section");
    let text = format!("{})\n", &DSN[..wiring]);
    let mut board = match read_board_from_str(&text) {
        BoardReadResult::Success { board, .. } => *board,
        BoardReadResult::OutlineMissing {
            board: Some(board), ..
        } => *board,
        _ => panic!("normalize-stale-trace.dsn must load"),
    };
    let corners: Vec<Point> = WIRE
        .iter()
        .map(|&(x, y)| Point::Int(IntPoint::new(x, y)))
        .collect();
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&corners),
            0,
            2500,
            &[1],
            1,
            FixedState::Unfixed,
        )
        .expect("the wire is inserted");
    board
}

/// The traces of `board` as `(id, layer, corners)`, in item order.
fn traces(board: &RoutingBoard) -> Vec<String> {
    board
        .items()
        .filter_map(|item| {
            let trace = item.as_trace()?;
            let corners: Vec<String> = (0..trace.polyline.corner_count())
                .map(|i| format!("{:?}", trace.polyline.corner(i)))
                .collect();
            Some(format!(
                "{} {} {}",
                item.id().0,
                trace.layer,
                corners.join(" ")
            ))
        })
        .collect()
}

/// A stop request that answers yes from its `stop_at`-th check on (never
/// for `u32::MAX`) and counts the checks.
struct StopAt {
    stop_at: u32,
    checks: Cell<u32>,
}

impl StopAt {
    fn new(stop_at: u32) -> StopAt {
        StopAt {
            stop_at,
            checks: Cell::new(0),
        }
    }
}

impl Stoppable for StopAt {
    fn request_stop(&self) {}

    fn is_stop_requested(&self) -> bool {
        let n = self.checks.get() + 1;
        self.checks.set(n);
        n >= self.stop_at
    }
}

#[test]
fn an_unanswered_stop_check_changes_nothing() {
    let mut plain = board_with_wire();
    let plain_changed = plain.normalize_traces(1);
    let mut checked = board_with_wire();
    let never = StopAt::new(u32::MAX);
    let checked_changed = checked.normalize_traces_until(1, Some(&never));
    assert!(plain_changed, "the wire's normalization changes something");
    assert_eq!(checked_changed, plain_changed);
    assert_eq!(traces(&checked), traces(&plain));
    assert_eq!(
        checked.communication.id_generator.max_generated_id(),
        plain.communication.id_generator.max_generated_id()
    );
    // The later passes asked: before the pass and before each trace.
    assert!(never.checks.get() >= 2, "{} checks", never.checks.get());
}

#[test]
fn a_stop_ends_the_normalization_after_its_first_pass() {
    let mut board = board_with_wire();
    let stop = StopAt::new(1);
    assert!(board.normalize_traces_until(1, Some(&stop)));
    // Asked once, before the second pass, which did not run.
    assert_eq!(stop.checks.get(), 1);
    // A stop is not the iteration limit: the net is not suppressed.
    assert!(board.normalize_suppressed_nets().is_empty());
}

#[test]
fn a_stop_ends_a_later_pass_at_its_next_trace() {
    let never = StopAt::new(u32::MAX);
    board_with_wire().normalize_traces_until(1, Some(&never));
    let all = never.checks.get();
    for stop_at in 2..=all {
        let stop = StopAt::new(stop_at);
        board_with_wire().normalize_traces_until(1, Some(&stop));
        assert_eq!(stop.checks.get(), stop_at, "stop at check {stop_at}");
    }
}

// ---- end to end: coldfire's fanout ----------------------------------------

const COLDFIRE: &str = "Issue690-kit-dev-coldfire-xilinx_5213.dsn";

/// Calls `on_stall` at the fanout's progress event of pass 2 with at least
/// 155 pins routed: the last event before the pin whose insertion
/// normalizes the GND net for minutes.
struct AtFanoutStall {
    on_stall: Box<dyn Fn()>,
    fired: Cell<bool>,
}

impl RouterEvents for AtFanoutStall {
    fn is_listening(&self) -> bool {
        true
    }
    fn progress(&self, p: Progress) -> bool {
        if p.phase == Phase::Fanout && p.pass == 2 && p.routed >= 155 && !self.fired.replace(true) {
            (self.on_stall)();
        }
        true
    }
    fn delta(&self, _: Vec<ItemId>, _: Vec<ItemGeometry>) {}
    fn replaced(&self, _: BoardSnapshot) {}
    fn finished(&self, board: RoutingBoard, _: RunSummary) -> Option<RoutingBoard> {
        Some(board)
    }
}

/// A full stop 3 s into the GND net's oscillating normalization (it lasts
/// about 280 s; before the fix the job returned only after it) ends the job
/// within 10 s. Release only: the fanout reaches the stall after about 19 s.
#[test]
#[ignore = "release; about 25 s: cargo test --release --test normalize_stop -- --ignored"]
fn stop_ends_an_oscillating_normalization_in_the_fanout() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes = std::fs::read(root.join("fixtures").join(COLDFIRE)).unwrap();
    let prepared = prepare_job(
        &JobInput::new(bytes, JobInput::design_name_of(COLDFIRE)),
        &ResolveContext { cores: 4 },
    )
    .expect("coldfire prepares");
    let token = Arc::new(CancelToken::new());
    let stalled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stalled);
    let sink = AtFanoutStall {
        on_stall: Box::new(move || flag.store(true, Ordering::Release)),
        fired: Cell::new(false),
    };
    let stopper = {
        let token = Arc::clone(&token);
        let stalled = Arc::clone(&stalled);
        std::thread::spawn(move || {
            while !stalled.load(Ordering::Acquire) && !token.is_stop_requested() {
                std::thread::sleep(Duration::from_millis(20));
            }
            std::thread::sleep(Duration::from_secs(3));
            let at = Instant::now();
            token.stop_all();
            at
        })
    };
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::clone(&token),
        events: &sink,
        deadline: None,
        registry: None,
    };
    let outcome = run_pipeline(prepared.board, &job).expect("the pipeline runs");
    let returned = Instant::now();
    // Unblocks the stopper if the job ended before the stall.
    token.stop_all();
    let stopped_at = stopper.join().unwrap();
    assert!(
        stalled.load(Ordering::Acquire),
        "the fanout reached the stall"
    );
    let latency = returned.saturating_duration_since(stopped_at);
    assert!(
        latency < Duration::from_secs(10),
        "stop latency {latency:?}"
    );
    assert!(
        outcome.passes.is_empty(),
        "the stop ended the job in the fanout"
    );
}

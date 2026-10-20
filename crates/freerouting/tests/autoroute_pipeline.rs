//! The batch loop and `run_pipeline` (A13): snapshots in the history
//! (A13.1), the loop's restores with a fake pass runner (A13.2), the
//! router-enabled rule, the optimizer gate and `reported_equals_saved`
//! (A13.4), the ported Java pipeline tests (A13.5) and the per-pass score
//! lines against Java's route references (A13.7).
//!
//! Java tests ported here (they belong in `tests/java_ports.rs`, which
//! carries another session's uncommitted hunks):
//! `fixtures.MaxPassesSettingRoutingTest`, `fixtures.MaxItemsRoutingTest`
//! (strengthened: exactly 20 items, the full stop and no optimizer),
//! `fixtures.HeadlessCompleteRoutingTest` (all three),
//! `fixtures.RoutingPipelineComparisonTest` (as a determinism test: Java
//! compares the GUI and headless workers, which are one pipeline here),
//! `fixtures.Issue684MemoryLeakRoutingTest.boardHistorySizeCapIsPositiveAndBounded`
//! and the engine half of `fixtures.GuiFinalBoardTest`
//! (`reported_equals_saved`). Not ported:
//! `Issue684MemoryLeakRoutingTest.routingCompletesWithoutOutOfMemoryError`
//! (a smoke run of a 50-item job on a large board; the checklist ports test
//! 1 only), and `RoutingPipelineComparisonTest`'s optimizer bounds (the
//! optimizer is O06's).

mod autoroute_common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use autoroute_common::{Loaded, repo_root};
use freerouting::autoroute::batch::batch_loop::PassExecutor;
use freerouting::autoroute::batch::connection::FreeroutingConnectionRouter;
use freerouting::autoroute::batch::last_mile::{MAX_ITEMS, plan_last_mile, rip_blockers};
use freerouting::autoroute::batch::ordering::FreeroutingOrdering;
use freerouting::autoroute::batch::pass::{PassOutcome, PassRunner, PassStages};
use freerouting::autoroute::batch::report::{corridor, is_removable_blocker};
use freerouting::autoroute::batch::ripup_policy::FreeroutingRipupPolicy;
use freerouting::autoroute::batch::snapshot::{BoardFigures, ScoredBoard, SnapshotHistory};
use freerouting::autoroute::events::{CancelToken, EventSink, NullEvents, RouterEvent};
use freerouting::autoroute::history::MAX_HISTORY_SIZE;
use freerouting::autoroute::pipeline::{
    PipelineJob, PipelineOutcome, router_enabled, run_pipeline, run_pipeline_with,
};
use freerouting::autoroute::probe::{ProbeValue, ProbeWriter};
use freerouting::autoroute::stages::{
    Optimizer, OptimizerSummary, StageCtx, StageRegistry, StopReason,
};
use freerouting::autoroute::{AttemptState, events::RouterEvents};
use freerouting::board::RoutingBoard;
use freerouting::board::items::{ItemId, StopConnectionOption};
use freerouting::board::structure::FixedState;
use freerouting::drc::DesignRulesChecker;
use freerouting::geometry::{Point, Polyline, TileShape};
use freerouting::io::read_session;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::board::BoardRouterSettings;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, RouterSettings, SettingsPatch, Source};

const J2: &str = "fixtures/Issue026-J2_reference.dsn";
const BM01: &str = "fixtures/Issue508-DAC2020_bm01.dsn";
const BM08: &str = "fixtures/Issue508-DAC2020_bm08.dsn";
const CONGFLOCK: &str = "fixtures/PCBench-retroreflectors-CONGFLOCK.dsn";
const TWO_PIN: &str = "fixtures/scoring-perfect-two-pin.dsn";

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Loads `dsn` through the job's load sequence with `extra` at
/// `source`'s priority.
fn load_with(dsn: &str, source: Source, mut extra: SettingsPatch) -> Loaded {
    // Java's references: profile `legacy` unless the test picks one.
    if extra.profile.is_none() {
        extra.profile = Some("legacy".to_string());
    }
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("board");
    let mut input = JobInput::new(bytes, JobInput::design_name_of(file_name));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(source, extra)];
    let prepared =
        prepare_job(&input, &ResolveContext { cores: 8 }).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    Loaded {
        board: prepared.board,
        settings: prepared.settings,
        router: prepared.board_settings,
    }
}

/// Java `RoutingFixtureTest.getRoutingJob` with `TestingSettings` (its
/// `setMaxPasses(100)` and `setJobTimeoutString("00:01:00")`, first write
/// wins).
fn load_testing(dsn: &str, mut profile: TestingProfile) -> Loaded {
    profile.set_job_timeout("00:01:00");
    profile.set_max_passes(100);
    load_with(dsn, Source::Testing, profile.patch)
}

/// J01.3's `route` mode: the defaults, the DSN's settings and the adjacent
/// rules, then the harness overrides at priority 80 (the optimizer off and
/// `max_passes`).
fn load_route_mode(dsn: &str, max_passes: i32) -> Loaded {
    let mut patch = SettingsPatch::default();
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(max_passes);
    load_with(dsn, Source::Testing, patch)
}

fn job<'a>(
    loaded: &'a Loaded,
    cancel: &Arc<CancelToken>,
    events: &'a dyn RouterEvents,
) -> PipelineJob<'a> {
    job_of(&loaded.settings, &loaded.router, cancel, events)
}

fn job_of<'a>(
    settings: &'a RouterSettings,
    board_settings: &'a BoardRouterSettings,
    cancel: &Arc<CancelToken>,
    events: &'a dyn RouterEvents,
) -> PipelineJob<'a> {
    PipelineJob {
        settings,
        board_settings,
        cancel: Arc::clone(cancel),
        events,
        deadline: None,
        registry: None,
    }
}

/// `run_pipeline` with no listener; the board comes back in the outcome.
fn route(loaded: Loaded) -> (PipelineOutcome, RoutingBoard, Arc<CancelToken>) {
    let cancel = Arc::new(CancelToken::new());
    let Loaded {
        board,
        settings,
        router,
    } = loaded;
    let mut outcome = run_pipeline(board, &job_of(&settings, &router, &cancel, &NullEvents))
        .unwrap_or_else(|f| panic!("{}", f.error));
    let board = outcome.board.take().expect("no listener took the board");
    (outcome, board, cancel)
}

fn items_text(board: &RoutingBoard) -> Vec<String> {
    board.items().map(|item| format!("{item:?}")).collect()
}

// ---------------------------------------------------------------------------
// A13.1: snapshots for the history
// ---------------------------------------------------------------------------

/// The Freerouting pass stages, cloneable so two continuations start from
/// the same stage state.
#[derive(Clone)]
struct Stages {
    runner: PassRunner,
    ordering: FreeroutingOrdering,
    ripup: FreeroutingRipupPolicy,
}

impl Stages {
    fn new(loaded: &Loaded) -> Stages {
        Stages {
            runner: PassRunner::for_job(&loaded.settings, &loaded.router),
            ordering: FreeroutingOrdering::new(),
            ripup: FreeroutingRipupPolicy::from_settings(&loaded.settings),
        }
    }

    fn pass(
        &mut self,
        board: &mut RoutingBoard,
        pass: u32,
        settings: &RouterSettings,
        board_settings: &BoardRouterSettings,
    ) -> PassOutcome {
        let cancel = CancelToken::new();
        let mut ctx = StageCtx {
            settings,
            board_settings,
            cancel: &cancel,
            events: &NullEvents,
        };
        let mut router = FreeroutingConnectionRouter::from_settings(settings);
        let mut stages = PassStages {
            ordering: &mut self.ordering,
            router: &mut router,
            ripup: &mut self.ripup,
        };
        self.runner.run_pass(board, pass, &mut stages, &mut ctx)
    }
}

/// Adds `board` to `history` as the batch loop does.
fn add(history: &mut SnapshotHistory, board: &RoutingBoard, settings: &RouterSettings) {
    let score = BoardFigures::of(board, settings).score;
    history.add(&ScoredBoard {
        board,
        hash: board.state_hash(),
        score,
    });
}

/// Routes `dsn` for 3 passes, snapshots, routes pass 4 twice from the
/// snapshot; returns pass 4's outcome after checking that the two results
/// are equal item for item.
fn continue_from_snapshot(loaded: &mut Loaded) -> PassOutcome {
    let mut stages = Stages::new(loaded);
    for pass in 1..=3 {
        stages.pass(&mut loaded.board, pass, &loaded.settings, &loaded.router);
    }
    let mut history = SnapshotHistory::new();
    add(&mut history, &loaded.board, &loaded.settings);
    let mut results = Vec::new();
    for _ in 0..2 {
        let mut board = history.restore_best().expect("one board");
        let mut continuation = stages.clone();
        let outcome = continuation.pass(&mut board, 4, &loaded.settings, &loaded.router);
        results.push((items_text(&board), board.state_hash(), outcome));
    }
    assert_eq!(results[0].0, results[1].0, "items after pass 4");
    assert_eq!(results[0].1, results[1].1, "state hash after pass 4");
    assert_eq!(results[0].2, results[1].2, "pass 4 outcome");
    results.swap_remove(0).2
}

/// Route J2 for 3 passes, snapshot, route pass 4 twice from the snapshot:
/// the two results are equal item for item (A13.1). J2 is complete after
/// pass 1, so bm01 (fanout off) is also continued, where pass 4 routes,
/// fails and rips (25 routed, 2 failed, 18 ripped when this was written).
#[test]
fn restored_board_continues_identically() {
    let mut j2 = load_testing(J2, TestingProfile::default());
    continue_from_snapshot(&mut j2);
    let mut profile = TestingProfile::default();
    profile.set_fanout_enabled(false);
    let mut bm01 = load_testing(BM01, profile);
    let pass_4 = continue_from_snapshot(&mut bm01);
    assert!(pass_4.routed + pass_4.failed > 0, "bm01's pass 4 works");
}

/// A restore brings back the failure log and the id generator of the board
/// it was taken from, and its items with their ids (A13.1).
#[test]
fn restore_rolls_back_failure_log_and_ids() {
    let mut loaded = load_testing(J2, TestingProfile::default());
    let mut history = SnapshotHistory::new();
    add(&mut history, &loaded.board, &loaded.settings);
    let before_log = loaded.board.failure_log().clone();
    let before_ids = loaded.board.item_ids();
    let mut before_board = loaded.board.clone();
    let mut stages = Stages::new(&loaded);
    let outcome = stages.pass(&mut loaded.board, 1, &loaded.settings, &loaded.router);
    assert!(outcome.routed > 0, "pass 1 routes something");
    let pin = before_ids[0];
    loaded.board.failure_log_mut().record_failure(
        pin,
        1,
        1,
        AttemptState::Failed,
        Some("after the snapshot"),
    );
    assert_ne!(loaded.board.item_ids(), before_ids);
    let mut restored = history.restore_best().expect("one board");
    assert_eq!(restored.failure_log(), &before_log);
    assert_eq!(restored.item_ids(), before_ids);
    assert_eq!(items_text(&restored), items_text(&before_board));
    // The next id is the one the board would have used at snapshot time.
    let next = |b: &mut RoutingBoard| {
        let pins: Vec<_> = b
            .items()
            .filter(|i| i.is_pin() && i.net_count() > 0)
            .take(2)
            .map(|i| (i.drill().expect("drill").center, i.net_numbers()[0]))
            .collect();
        let polyline = Polyline::from_points(&[Point::Int(pins[0].0), Point::Int(pins[1].0)]);
        b.insert_trace_without_cleaning(polyline, 0, 100, &[pins[0].1], 0, FixedState::Unfixed)
    };
    assert_eq!(next(&mut restored), next(&mut before_board));
}

// ---------------------------------------------------------------------------
// A13.2: the loop with a fake pass runner
// ---------------------------------------------------------------------------

/// A pass runner that inserts one trace per pass (so every board state has
/// its own hash) and gives each state a scripted score; the incomplete and
/// violation counts are the board's own.
struct FakeRunner {
    script: fn(u32) -> f32,
    scores: Rc<RefCell<BTreeMap<u128, f32>>>,
    hashes: Rc<RefCell<Vec<u128>>>,
}

impl PassExecutor for FakeRunner {
    fn run_pass(
        &mut self,
        board: &mut RoutingBoard,
        pass: u32,
        _: &mut PassStages<'_>,
        _: &mut StageCtx,
    ) -> PassOutcome {
        let pins: Vec<_> = board
            .items()
            .filter(|i| i.is_pin())
            .map(|i| (i.drill().expect("drill").center, i.net_numbers()[0]))
            .collect();
        let polyline = Polyline::from_points(&[Point::Int(pins[0].0), Point::Int(pins[1].0)]);
        board.insert_trace_without_cleaning(polyline, 0, 100, &[pins[0].1], 0, FixedState::Unfixed);
        let hash = board.state_hash();
        self.scores.borrow_mut().insert(hash, (self.script)(pass));
        self.hashes.borrow_mut().push(hash);
        PassOutcome {
            continue_routing: true,
            routed: 1,
            ..PassOutcome::default()
        }
    }

    fn remove_tails(&mut self, _: &mut RoutingBoard, _: StopConnectionOption) -> bool {
        false
    }

    fn figures(&self, board: &RoutingBoard, settings: &RouterSettings) -> BoardFigures {
        let real = BoardFigures::of(board, settings);
        let score = self
            .scores
            .borrow()
            .get(&board.state_hash())
            .copied()
            .unwrap_or(0.0);
        BoardFigures { score, ..real }
    }
}

/// A10.3's restore case: 10 per pass up to pass 7, then 50.
fn restore_script(pass: u32) -> f32 {
    if pass < 8 { 10.0 * pass as f32 } else { 50.0 }
}

fn fake_loaded(max_passes: i32) -> Loaded {
    let mut profile = TestingProfile::default();
    profile.set_max_passes(max_passes);
    profile.set_fanout_enabled(false);
    profile.set_optimizer_enabled(false);
    load_testing(TWO_PIN, profile)
}

/// What a listener saw: the boards of every `Replaced`, the `Finished`
/// board and summary.
struct Seen {
    replaced: Vec<u128>,
    finished: Option<(RoutingBoard, freerouting::autoroute::events::RunSummary)>,
}

fn listen(receiver: Receiver<RouterEvent>) -> std::thread::JoinHandle<Seen> {
    std::thread::spawn(move || {
        let mut seen = Seen {
            replaced: Vec::new(),
            finished: None,
        };
        // The listener applies every `Replaced`: it rebuilds its board from
        // the snapshot.
        let mut applied: Option<RoutingBoard> = None;
        for event in receiver {
            match event {
                RouterEvent::Replaced { snapshot } => {
                    let board = snapshot.restore();
                    seen.replaced.push(board.state_hash());
                    applied = Some(board);
                }
                RouterEvent::Finished { board, summary } => {
                    seen.finished = Some((*board, summary));
                }
                RouterEvent::Progress(_) | RouterEvent::Delta { .. } => {}
            }
        }
        let _ = applied;
        seen
    })
}

/// The batch loop on A10.3's restore case swaps in the history's best board
/// at pass 8 (the board stored before pass 8, after pass 7's 70), announces
/// it, and the final best-board restore brings it back again after pass 9
/// (A13.2), and the board `Finished` hands over is the last one a
/// `Replaced` announced, with the summary's figures (A13.4,
/// `reported_equals_saved`, Java `GuiFinalBoardTest`).
#[test]
fn reported_equals_saved() {
    let loaded = fake_loaded(9);
    let scores = Rc::new(RefCell::new(BTreeMap::new()));
    let hashes = Rc::new(RefCell::new(Vec::new()));
    let cancel = Arc::new(CancelToken::new());
    let (sink, receiver) = EventSink::channel(1024);
    let listener = listen(receiver);
    let mut factory = |_: &RouterSettings, _: &_| {
        Box::new(FakeRunner {
            script: restore_script,
            scores: Rc::clone(&scores),
            hashes: Rc::clone(&hashes),
        }) as Box<dyn PassExecutor>
    };
    let board = loaded.board.clone();
    let outcome = run_pipeline_with(board, &job(&loaded, &cancel, &sink), &mut factory)
        .unwrap_or_else(|f| panic!("{}", f.error));
    drop(sink);
    let seen = listener.join().expect("listener");
    assert!(outcome.board.is_none(), "the listener took the board");
    assert_eq!(outcome.restores, 1, "one restore during the passes");
    assert!(outcome.restored_best, "the final best-board restore");
    assert_eq!(outcome.stop, Some(StopReason::MaxPasses));
    let after_pass_7 = hashes.borrow()[6];
    assert_eq!(seen.replaced, vec![after_pass_7, after_pass_7]);
    // The pass-8 line reports the restored board.
    let pass_8 = &outcome.passes[7];
    assert_eq!(
        (pass_8.pass, pass_8.hash, pass_8.score),
        (8, after_pass_7, 70.0)
    );
    let (board, summary) = seen.finished.expect("finished");
    assert_eq!(board.state_hash(), *seen.replaced.last().unwrap());
    let real = BoardStatistics::new(&board);
    assert_eq!(summary.incomplete, real.connections.incomplete_count);
    assert_eq!(summary.violations, real.clearance_violations.total_count);
    assert_eq!(summary.score, 70.0);
    assert_eq!(summary.passes, 9);
}

// ---------------------------------------------------------------------------
// A13.4: the router-enabled rule and the optimizer gate
// ---------------------------------------------------------------------------

fn with_max_passes(max_passes: i32, enabled: bool) -> RouterSettings {
    let mut settings = load_testing(TWO_PIN, TestingProfile::default()).settings;
    settings.autorouter.max_passes = max_passes;
    settings.autorouter.enabled = enabled;
    settings
}

/// `autorouter.enabled && (max_passes == null || max_passes >= 0)`; the
/// port's `max_passes` is never null, its unset default is 0.
#[test]
fn router_enabled_rule() {
    assert!(router_enabled(&with_max_passes(0, true)));
    assert!(router_enabled(&with_max_passes(2, true)));
    assert!(!router_enabled(&with_max_passes(-1, true)));
    assert!(!router_enabled(&with_max_passes(2, false)));
    let defaults = autoroute_common::load(TWO_PIN).settings;
    assert_eq!(defaults.autorouter.max_passes, 0, "the unset value");
    assert!(router_enabled(&defaults));
}

/// A disabled router with the fanout on is a fanout-only run: the loop runs
/// with `max_passes = 0` and finds the router disabled, so no pass runs.
#[test]
fn disabled_router_with_fanout_runs_the_fanout_alone() {
    let mut profile = TestingProfile::default();
    profile.set_router_enabled(false);
    profile.set_fanout_enabled(true);
    let loaded = load_testing("fixtures/Issue508-SMD-routing-issue-demo.dsn", profile);
    let traces_before = loaded.board.items().filter(|i| i.is_trace()).count();
    let (outcome, board, _) = route(loaded);
    assert!(outcome.loop_ran);
    assert!(outcome.fanout.is_some(), "the fanout ran");
    assert!(outcome.passes.is_empty(), "no routing pass");
    assert_eq!(outcome.summary.passes, 0);
    assert!(board.items().filter(|i| i.is_trace()).count() >= traces_before);
}

/// Java's quirk: `max_passes = -1` with the router enabled is "disabled"
/// for the pipeline. The fanout-only run must keep the router disabled.
#[test]
fn negative_max_passes_with_fanout_does_not_route() {
    let mut profile = TestingProfile::default();
    profile.set_fanout_enabled(true);
    let mut loaded = load_testing(J2, profile);
    loaded.settings.autorouter.max_passes = -1;
    let (outcome, _, _) = route(loaded);
    assert!(outcome.loop_ran);
    assert!(
        outcome.passes.is_empty(),
        "fanout must not reenable routing"
    );
    // Without the fanout nothing runs.
    let mut profile = TestingProfile::default();
    profile.set_fanout_enabled(false);
    let mut loaded = load_testing(J2, profile);
    loaded.settings.autorouter.max_passes = -1;
    let (outcome, _, _) = route(loaded);
    assert!(!outcome.loop_ran);
    assert!(outcome.passes.is_empty());
}

thread_local! {
    static OPTIMIZER_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A fake optimizer that counts its runs (on the calling thread).
struct CountingOptimizer;

impl Optimizer for CountingOptimizer {
    fn id(&self) -> &'static str {
        "freerouting-optimizer"
    }
    fn run(&mut self, _: &mut RoutingBoard, _: &mut StageCtx) -> OptimizerSummary {
        OPTIMIZER_RUNS.with(|c| c.set(c.get() + 1));
        OptimizerSummary::default()
    }
}

fn counting_registry() -> StageRegistry {
    let mut registry = StageRegistry::builtin();
    registry
        .optimizer
        .insert("freerouting-optimizer", |_| Box::new(CountingOptimizer));
    registry
}

/// Runs J2 (fanout off) with the counting optimizer; returns whether the
/// optimizer ran and the outcome.
fn optimizer_ran(
    edit: impl FnOnce(&mut TestingProfile),
    before: impl FnOnce(&CancelToken),
    deadline: Option<Duration>,
) -> (bool, PipelineOutcome) {
    let mut profile = TestingProfile::default();
    profile.set_fanout_enabled(false);
    profile.set_optimizer_enabled(true);
    edit(&mut profile);
    let loaded = load_testing(J2, profile);
    let registry = counting_registry();
    let cancel = Arc::new(CancelToken::new());
    before(&cancel);
    let mut pipeline_job = job(&loaded, &cancel, &NullEvents);
    pipeline_job.registry = Some(&registry);
    pipeline_job.deadline = deadline;
    OPTIMIZER_RUNS.with(|c| c.set(0));
    let outcome =
        run_pipeline(loaded.board.clone(), &pipeline_job).unwrap_or_else(|f| panic!("{}", f.error));
    (OPTIMIZER_RUNS.with(|c| c.get()) == 1, outcome)
}

/// The optimizer runs after a finished run and after a router-only stop
/// (max passes, a router stop request), and not after a full stop
/// (`max_items`, a timeout, a cancel).
#[test]
fn optimizer_gate_under_each_stop_kind() {
    let (ran, outcome) = optimizer_ran(|_| {}, |_| {}, None);
    assert!(ran, "finished run");
    assert_eq!(outcome.stop, Some(StopReason::Completed));

    let (ran, outcome) = optimizer_ran(|p| p.set_max_passes(1), |_| {}, None);
    assert!(ran, "max passes is a router-only stop");
    assert_eq!(outcome.stop, Some(StopReason::MaxPasses));

    let (ran, outcome) = optimizer_ran(|_| {}, |c| c.stop_router(), None);
    assert!(ran, "a router stop leaves the optimizer");
    assert!(!outcome.loop_ran, "no routing after a router stop");

    let (ran, _) = optimizer_ran(|p| p.set_max_items(3), |_| {}, None);
    assert!(!ran, "max_items is a full stop");

    let (ran, outcome) = optimizer_ran(|_| {}, |_| {}, Some(Duration::ZERO));
    assert!(!ran, "a timeout is a full stop");
    assert!(outcome.timed_out);
    assert_eq!(
        outcome.summary.status,
        freerouting::autoroute::events::CompletionStatus::CompletedWithTimeout
    );

    let (ran, _) = optimizer_ran(|_| {}, |c| c.stop_all(), None);
    assert!(!ran, "a cancel is a full stop");
}

// ---------------------------------------------------------------------------
// A13.5: ported Java pipeline tests
// ---------------------------------------------------------------------------

/// Java `MaxPassesSettingRoutingTest.issue522MaxPassesSettingIsRespected`.
#[test]
fn max_passes_setting_is_respected() {
    let mut profile = TestingProfile::default();
    profile.set_max_passes(2);
    let (outcome, _, _) = route(load_testing(J2, profile));
    assert_eq!(outcome.summary.passes, 2, "the job's current pass");
}

/// A pass runner that counts the items the real runner routed.
struct CountingRunner {
    inner: PassRunner,
    routed: Rc<RefCell<i64>>,
}

impl PassExecutor for CountingRunner {
    fn run_pass(
        &mut self,
        board: &mut RoutingBoard,
        pass: u32,
        stages: &mut PassStages<'_>,
        ctx: &mut StageCtx,
    ) -> PassOutcome {
        let outcome = self.inner.run_pass(board, pass, stages, ctx);
        *self.routed.borrow_mut() = self.inner.total_items_routed();
        outcome
    }

    fn remove_tails(&mut self, board: &mut RoutingBoard, option: StopConnectionOption) -> bool {
        self.inner.remove_tails(board, option)
    }
}

/// Java `MaxItemsRoutingTest.maxItemsLimit`, strengthened: J2 with
/// `max_items = 20` routes exactly 20 items, requests the full stop and
/// skips the optimizer.
#[test]
fn max_items_limit() {
    let mut profile = TestingProfile::default();
    profile.set_max_items(20);
    profile.set_max_passes(100);
    profile.set_optimizer_enabled(true);
    let loaded = load_testing(J2, profile);
    let registry = counting_registry();
    let cancel = Arc::new(CancelToken::new());
    let mut pipeline_job = job(&loaded, &cancel, &NullEvents);
    pipeline_job.registry = Some(&registry);
    let routed = Rc::new(RefCell::new(0));
    let mut factory = |s: &RouterSettings, b: &_| {
        Box::new(CountingRunner {
            inner: PassRunner::for_job(s, b),
            routed: Rc::clone(&routed),
        }) as Box<dyn PassExecutor>
    };
    OPTIMIZER_RUNS.with(|c| c.set(0));
    let outcome = run_pipeline_with(loaded.board.clone(), &pipeline_job, &mut factory)
        .unwrap_or_else(|f| panic!("{}", f.error));
    assert_eq!(*routed.borrow(), 20, "items routed over all passes");
    assert!(cancel.is_stop_requested(), "the full stop");
    assert_eq!(OPTIMIZER_RUNS.with(|c| c.get()), 0, "no optimizer");
    assert!(
        outcome
            .log
            .iter()
            .any(|l| l.text == "Max items limit reached (20). Stopping auto-router.")
    );
}

/// Java `HeadlessCompleteRoutingTest`: bounded bm01 runs and the empty board
/// reach a terminal state with a board.
#[test]
fn headless_routing_completes() {
    for (passes, items) in [(3, 50), (2, 20)] {
        let mut profile = TestingProfile::default();
        profile.set_max_passes(passes);
        profile.set_max_items(items);
        let (outcome, board, _) = route(load_testing(BM01, profile));
        assert!(outcome.summary.passes >= 1, "bm01 {passes}/{items}");
        assert!(board.items().count() > 0);
    }
    let started = Instant::now();
    let mut profile = TestingProfile::default();
    profile.set_max_passes(1);
    let (outcome, board, _) = route(load_testing("fixtures/empty_board.dsn", profile));
    assert_eq!(outcome.summary.incomplete, 0);
    assert!(outcome.passes.len() <= 1);
    assert_eq!(board.items().filter(|i| i.is_trace()).count(), 0);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the empty board exits at once"
    );
}

/// Java `RoutingPipelineComparisonTest` as a determinism test: two runs of
/// bm01 with one pass and two items give identical statistics.
#[test]
fn pipeline_runs_are_deterministic() {
    let run = || {
        let mut profile = TestingProfile::default();
        profile.set_max_passes(1);
        profile.set_max_items(2);
        let (outcome, board, _) = route(load_testing(BM01, profile));
        let stats = BoardStatistics::new(&board);
        (
            stats.connections.incomplete_count,
            stats.items.via_count,
            stats.clearance_violations.total_count,
            board.state_hash(),
            outcome.summary.clone(),
        )
    };
    assert_eq!(run(), run());
}

/// Java `Issue684MemoryLeakRoutingTest.boardHistorySizeCapIsPositiveAndBounded`.
#[test]
fn board_history_size_cap_is_bounded() {
    assert_eq!(MAX_HISTORY_SIZE, 30);
    assert_eq!(SnapshotHistory::new().max_size(), 30);
}

// ---------------------------------------------------------------------------
// A13.7: the pass log against Java
// ---------------------------------------------------------------------------

/// The `passes` of `fixtures/reference/route/<board>.json`: `(pass, score
/// text)`.
fn java_passes(board: &str) -> Vec<(u32, String)> {
    let path = repo_root().join(format!("fixtures/reference/route/{board}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{board}: {e}"));
    let start = text.find("\"passes\"").expect("passes");
    let end = start + text[start..].find(']').expect("end of passes");
    text[start..end]
        .split('{')
        .skip(1)
        .map(|entry| {
            let field = |key: &str| {
                let at = entry.find(&format!("\"{key}\"")).expect(key) + key.len() + 3;
                entry[at..]
                    .trim_start()
                    .trim_start_matches('"')
                    .split(['"', ',', '}'])
                    .next()
                    .unwrap()
                    .trim()
                    .to_string()
            };
            (field("pass").parse().expect("pass"), field("score"))
        })
        .collect()
}

/// The per-pass score lines of J2 and bm08, bounded as in
/// `route-bounds.tsv` (100 passes: both stop on their own), equal Java's
/// route references pass by pass (A13.7), and the routed boards' SES files
/// equal Java's byte for byte (fanout, passes and the final tail removal).
/// CONGFLOCK (also 100 passes, a natural stop at pass 30) runs the whole
/// loop: three history restores, the fanout recovery, two last-mile ripups,
/// the global stagnation stop and the final best-board restore.
#[test]
fn pass_scores_match_java() {
    for (dsn, board) in [
        (J2, "Issue026-J2_reference"),
        (BM08, "Issue508-DAC2020_bm08"),
        (CONGFLOCK, "PCBench-retroreflectors-CONGFLOCK"),
    ] {
        let (outcome, routed, _) = route(load_route_mode(dsn, 100));
        let rust: Vec<(u32, String)> = outcome
            .passes
            .iter()
            .map(|p| (p.pass, p.score_text()))
            .collect();
        assert_eq!(rust, java_passes(board), "{board}");
        // Beyond the pass lines: the routed board writes Java's SES.
        let design = Path::new(dsn).file_stem().and_then(|n| n.to_str()).unwrap();
        let ses = write_ses(&routed, design, SesStyle::Java);
        let java_ses = repo_root().join(format!("fixtures/reference/route/{board}.ses"));
        let java_ses = std::fs::read_to_string(java_ses).expect("the Java SES");
        assert!(ses == java_ses, "{board}: the SES differs from Java's");
    }
}

// ---------------------------------------------------------------------------
// A13.3: the last-mile blocker ripup against Java
// ---------------------------------------------------------------------------

/// The board of the `last-mile` probe: the load, Java's routed reference
/// SES imported and its traces and vias unfixed (the import makes them user
/// fixed, which the ripup never removes).
fn last_mile_board(dsn: &str) -> RoutingBoard {
    let mut board = autoroute_common::load(dsn).board;
    let ses = repo_root().join(format!(
        "fixtures/reference/route/{}.ses",
        autoroute_common::board_name(dsn)
    ));
    let text = std::fs::read_to_string(&ses).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    read_session(&mut board, &text).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let route_items: Vec<_> = board
        .items()
        .filter(|i| i.is_trace() || i.is_via())
        .map(|i| i.id())
        .collect();
    for id in route_items {
        board.unfix(id);
    }
    board
}

/// The `last-mile` probe's lines (`AutorouteProbe.lastMile`): the airlines
/// with their corridors, overlapping items and added blockers, written by a
/// replica of the loop; then the blockers, and the real `rip_blockers` on a
/// second load with whether it left the replica's item ids. Also checks
/// that the replica's blockers are [`plan_last_mile`]'s.
fn last_mile_lines(dsn: &str) -> Vec<String> {
    let mut board = last_mile_board(dsn);
    let mut w = ProbeWriter::new();
    w.header("last-mile", &autoroute_common::board_name(dsn), "a773a70ec");
    let airlines = DesignRulesChecker::new(&board).all_airlines();
    w.record(&[
        ("stage", "airlines".into()),
        ("count", ProbeValue::Int(airlines.len() as i64)),
        ("maxItems", ProbeValue::Int(MAX_ITEMS as i64)),
    ]);
    let mut blockers: Vec<ItemId> = Vec::new();
    for (i, airline) in airlines.iter().enumerate() {
        let corridor = corridor(airline.from_corner, airline.to_corner);
        let mut overlapping = Vec::new();
        let mut added = Vec::new();
        for id in board.overlapping_objects(&TileShape::IntBox(corridor), -1) {
            overlapping.push(i64::from(id.0));
            if blockers.len() >= MAX_ITEMS {
                break;
            }
            let item = board.item(id).expect("an item");
            if is_removable_blocker(&board, item, airline.net_number) && !blockers.contains(&id) {
                blockers.push(id);
                added.push(i64::from(id.0));
            }
        }
        w.record(&[
            ("airline", ProbeValue::Int(i as i64)),
            ("net", ProbeValue::Int(i64::from(airline.net_number))),
            ("from", ProbeValue::Int(i64::from(airline.from_item.0))),
            ("to", ProbeValue::Int(i64::from(airline.to_item.0))),
            ("fromX", ProbeValue::Bits(airline.from_corner.x)),
            ("fromY", ProbeValue::Bits(airline.from_corner.y)),
            ("toX", ProbeValue::Bits(airline.to_corner.x)),
            ("toY", ProbeValue::Bits(airline.to_corner.y)),
            (
                "corridor",
                ProbeValue::ints(
                    [corridor.ll.x, corridor.ll.y, corridor.ur.x, corridor.ur.y].map(i64::from),
                ),
            ),
            ("overlapping", ProbeValue::ints(overlapping)),
            ("added", ProbeValue::ints(added)),
        ]);
        if blockers.len() >= MAX_ITEMS {
            break;
        }
    }
    w.record(&[
        ("stage", "blockers".into()),
        (
            "ids",
            ProbeValue::ints(blockers.iter().map(|id| i64::from(id.0))),
        ),
    ]);
    assert_eq!(
        plan_last_mile(&board).blockers,
        blockers,
        "{dsn}: plan_last_mile"
    );
    assert!(blockers.len() <= MAX_ITEMS);
    board.remove_items(blockers.iter().copied());
    let mut real = last_mile_board(dsn);
    let removed = rip_blockers(&mut real);
    w.record(&[
        ("stage", "real".into()),
        ("removed", ProbeValue::Int(removed as i64)),
        (
            "replica_matches_real",
            ProbeValue::Bool(board.item_ids() == real.item_ids()),
        ),
    ]);
    w.into_lines()
}

/// The last-mile ripup on CONGFLOCK and bm01 after Java's routed reference
/// SES: the airline order, the corridors, the overlapping items and the
/// removed ids in order equal Java's (A13.3).
#[test]
fn last_mile_matches_java() {
    let fixtures = autoroute_common::kind_fixtures("last-mile");
    assert_eq!(fixtures.len(), 2);
    for dsn in &fixtures {
        let lines = last_mile_lines(dsn);
        autoroute_common::compare_probe("last-mile", dsn, &lines).unwrap_or_else(|e| panic!("{e}"));
    }
}

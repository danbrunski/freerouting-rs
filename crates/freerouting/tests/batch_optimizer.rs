//! The batch optimizer (O06): candidate order, one candidate, the parallel
//! pass with its ordered merge, the pass loop, the stage and its events.
//!
//! Java tests ported here (they need a board and run the optimizer):
//! `fixtures.BatchOptimizerDeterminismTest` (both),
//! `fixtures.BatchOptimizerStrategyTest` (all five variants),
//! `fixtures.MaxFanoutOptimizerSettingsTest.optimizerMaxPassesAndMaxItems`,
//! `fixtures.BatchOptimizerPreflightGuardsTest.runBatchLoopSetsManifestSnapshotsAndZeroPassesWhenBypassed`,
//! the two board cases of `autoroute.pipeline.BatchOptimizerTraceMetricTest` and
//! `fixtures.Issue952FanoutIgnoredNetClassTest` cases 5 and 6 (the optimizer's
//! candidate filter, which A12 left to O06). The board-free gate tests are in
//! `tests/java_ports.rs` (`autoroute_pipeline_batch_optimizer_gates`). Not ported:
//! `MaxFanoutOptimizerSettingsTest.fanoutMaxPassesAndMaxItems` (the fanout half, A12's).
//!
//! **Routed boards.** Java's `SesReader` imports a session's wires and vias user fixed, and
//! a user-fixed item is never an optimizer candidate, so Java's own tests on
//! `bm08-routed.ses` (`BatchOptimizerStrategyTest`, Issue952 cases 5 and 6) optimize nothing.
//! The tests here unfix what the session imported ([`routed`]) unless they port such a test
//! as it is; the ported Java test then runs both ways.

mod autoroute_common;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{
    CancelToken, CompletionStatus, ItemGeometry, NullEvents, Progress, RouterEvents, RunSummary,
};
use freerouting::autoroute::item_route_result::ItemRouteResult;
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{StageCtx, StageRegistry, StageSelection};
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::board::{BoardSnapshot, RoutingBoard};
use freerouting::geometry::{IntPoint, Point, Polyline};
use freerouting::io::read_session;
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::optimize::batch::candidates::{Candidate, prepare_candidates, sorted_candidates};
use freerouting::optimize::batch::item::{ItemPassParams, opt_route_item};
use freerouting::optimize::batch::worker::{
    PassSetup, candidate_start_board, evaluate_chunk, evaluate_pass,
};
use freerouting::optimize::batch::{
    OPTIMIZER_ID, OptimizerOptions, OptimizerRun, run_batch_optimizer,
};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::board::BoardRouterSettings;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{
    ItemSelectionStrategy, JavaQuirks, ResolveContext, RouterSettings, Source,
};

const BM01: &str = "fixtures/Issue508-DAC2020_bm01.dsn";
const BM05: &str = "fixtures/Issue508-DAC2020_bm05.dsn";
const BM08: &str = "fixtures/Issue508-DAC2020_bm08.dsn";
const BM08_SES: &str = "fixtures/Issue508-DAC2020_bm08-routed.ses";
const J2: &str = "fixtures/Issue026-J2_reference.dsn";

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

struct Loaded {
    board: RoutingBoard,
    settings: RouterSettings,
    router: BoardRouterSettings,
}

/// Loads `dsn` through the job's load sequence with `profile` at the testing priority.
fn load(dsn: &str, profile: TestingProfile) -> Loaded {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("board");
    let mut input = JobInput::new(bytes, JobInput::design_name_of(file_name));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    let prepared =
        prepare_job(&input, &ResolveContext { cores: 8 }).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    Loaded {
        board: prepared.board,
        settings: prepared.settings,
        router: prepared.board_settings,
    }
}

/// Imports the session `ses`; with `unfix`, every wire and via is unfixed as the Java
/// probes do (`item.unfix()`, which keeps system-fixed items fixed).
fn import(board: &mut RoutingBoard, ses: &str, unfix: bool) {
    let text = std::fs::read_to_string(repo_root().join(ses)).unwrap_or_else(|e| panic!("{e}"));
    read_session(board, &text).unwrap_or_else(|e| panic!("{ses}: {e}"));
    if unfix {
        for id in board.item_ids() {
            if board.item(id).is_some_and(|i| i.is_trace() || i.is_via()) {
                board.unfix(id);
            }
        }
    }
}

/// The session of `dsn`'s Java route reference.
fn reference_ses(dsn: &str) -> String {
    let name = Path::new(dsn).file_stem().unwrap().to_str().unwrap();
    format!("fixtures/reference/route/{name}.ses")
}

/// `dsn` with `ses` imported and unfixed, optimizer settings from `profile`.
fn routed(dsn: &str, ses: &str, profile: TestingProfile) -> Loaded {
    let mut loaded = load(dsn, profile);
    import(&mut loaded.board, ses, true);
    loaded
}

fn optimizer_profile(max_passes: i32, max_items: i32) -> TestingProfile {
    let mut profile = TestingProfile::default();
    profile.set_optimizer_max_passes(max_passes);
    profile.set_optimizer_max_items(max_items);
    profile
}

fn run_with(
    loaded: &mut Loaded,
    options: &OptimizerOptions,
    events: &dyn RouterEvents,
) -> OptimizerRun {
    let cancel = CancelToken::new();
    let ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events,
    };
    run_batch_optimizer(&mut loaded.board, &ctx, options)
}

fn run(loaded: &mut Loaded, threads: usize) -> OptimizerRun {
    let options = OptimizerOptions {
        threads: Some(threads),
        ..OptimizerOptions::default()
    };
    run_with(loaded, &options, &NullEvents)
}

/// What must be equal between two runs of the same input.
fn fingerprint(run: &OptimizerRun, board: &RoutingBoard) -> String {
    let passes: Vec<String> = run
        .pass_log
        .iter()
        .map(|p| {
            format!(
                "pass {} candidates {:?} results {:?} work {:?} winner {:?} scores {:08x} {:08x} \
                 {:?} early {} hash {:032x}",
                p.pass,
                p.candidates,
                p.results,
                p.work,
                p.winner,
                p.score_before.to_bits(),
                p.score_after.to_bits(),
                p.rejection,
                p.early_stop,
                p.state_hash
            )
        })
        .collect();
    format!(
        "{}\npasses {} items {} restored {} final {:032x}",
        passes.join("\n"),
        run.passes,
        run.items,
        run.restored_best,
        board.state_hash()
    )
}

// ---------------------------------------------------------------------------
// O06.2: the candidate order
// ---------------------------------------------------------------------------

/// bm08 routed, plus two vias and one trace with the same key at `p`.
struct EqualKeys {
    loaded: Loaded,
    p: IntPoint,
    high: ItemId,
    low: ItemId,
    trace: ItemId,
}

impl EqualKeys {
    /// The candidates whose key is `p` on layer 0, in order.
    fn at_p(&self, candidates: &[Candidate]) -> Vec<ItemId> {
        let p = self.p;
        candidates
            .iter()
            .filter(|c| c.key.x == f64::from(p.x) && c.key.y == f64::from(p.y) && c.key.layer == 0)
            .map(|c| c.id)
            .collect()
    }
}

fn equal_keys() -> EqualKeys {
    let mut loaded = routed(BM08, BM08_SES, TestingProfile::default());
    let board = &mut loaded.board;
    let rules = &board.rules;
    let rule = rules.via_rules.nth(0).expect("a via rule");
    let info = rules.via_rules.get(rule).vias()[0];
    let padstack = rules.via_infos.get(info).padstack;
    let nets: Vec<i32> = board.rules.nets.iter().map(|n| n.net_number).collect();
    let (net_a, net_b) = (nets[0], nets[1]);
    // A point no item uses, outside the routed area.
    let bb = board.bounding_box();
    let p = IntPoint::new(bb.ur.x - 1000, bb.ur.y - 1000);
    let q = IntPoint::new(p.x - 5000, p.y - 5000);
    let low = board.insert_via(padstack, p, &[net_a], 0, FixedState::Unfixed, true);
    let high = board.insert_via(padstack, p, &[net_a], 0, FixedState::Unfixed, true);
    // A trace of another net whose larger end corner is `p` on layer 0 (the vias' first
    // layer), so its key equals theirs.
    let polyline = Polyline::from_corners(&Point::Int(q), &Point::Int(p));
    let before: BTreeSet<ItemId> = board.item_ids().into_iter().collect();
    board.insert_trace(polyline, 0, 100, &[net_b], 0, FixedState::Unfixed);
    let trace = board
        .item_ids()
        .into_iter()
        .find(|id| !before.contains(id))
        .expect("the trace");
    assert!(high.0 > low.0);
    EqualKeys {
        loaded,
        p,
        high,
        low,
        trace,
    }
}

/// Two vias and one trace with the same key: only the via that comes first in the item
/// list (the higher id) is a candidate; the other via and the trace are skipped for the pass.
#[test]
fn equal_key_skips_rest_of_pass() {
    let setup = equal_keys();
    let (high, low, trace) = (setup.high, setup.low, setup.trace);
    let candidates = sorted_candidates(&setup.loaded.board);
    assert_eq!(
        setup.at_p(&candidates),
        vec![high],
        "only the higher-id via has that key's slot"
    );
    assert!(!candidates.iter().any(|c| c.id == low || c.id == trace));
    // The keys strictly increase, as Java's `next()` requires.
    for pair in candidates.windows(2) {
        let (a, b) = (pair[0].key, pair[1].key);
        assert!((a.x, a.y, a.layer) < (b.x, b.y, b.layer), "{a:?} {b:?}");
    }
}

/// N06.9 `router.java_quirks.opt_equal_key_skip = false`: every sorted candidate stays, in
/// the same (key, vias first, item-list order) order, so the three items at `p` come up as
/// the higher-id via, the lower-id via, then the trace; Java's list (`true`, the test above)
/// is this list with each key's first candidate only.
#[test]
fn equal_key_switch_keeps_every_candidate() {
    let mut setup = equal_keys();
    let java = sorted_candidates(&setup.loaded.board);
    let quirks = JavaQuirks::JAVA.with("opt_equal_key_skip", false).unwrap();
    setup.loaded.board.set_java_quirks(quirks);
    let fixed = sorted_candidates(&setup.loaded.board);
    assert_eq!(setup.at_p(&fixed), vec![setup.high, setup.low, setup.trace]);
    // Non-decreasing keys, and Java's list is the first of each key.
    let mut first_of_each: Vec<Candidate> = Vec::new();
    for pair in fixed.windows(2) {
        let (a, b) = (pair[0].key, pair[1].key);
        assert!((a.x, a.y, a.layer) <= (b.x, b.y, b.layer), "{a:?} {b:?}");
    }
    for candidate in &fixed {
        let same = first_of_each.last().is_some_and(|prev| {
            (prev.key.x, prev.key.y, prev.key.layer)
                == (candidate.key.x, candidate.key.y, candidate.key.layer)
        });
        if !same {
            first_of_each.push(*candidate);
        }
    }
    assert_eq!(first_of_each, java);
    assert!(
        fixed.len() >= java.len() + 2,
        "{} {}",
        fixed.len(),
        java.len()
    );
    // The pass's candidate list follows the switch.
    let ids = prepare_candidates(
        &setup.loaded.board,
        ItemSelectionStrategy::Sequential,
        &mut BTreeMap::new(),
    );
    assert_eq!(ids, fixed.iter().map(|c| c.id).collect::<Vec<_>>());
}

// ---------------------------------------------------------------------------
// O06.3: one candidate (BatchOptimizerTraceMetricTest, board cases)
// ---------------------------------------------------------------------------

/// J2 routed by the autorouter with one pass, the fanout off (Java's `routedFixtureJob`).
fn routed_j2() -> Loaded {
    let mut profile = TestingProfile::default();
    profile.set_max_passes(1);
    profile.set_max_items(i32::MAX);
    profile.set_fanout_enabled(false);
    profile.set_optimizer_enabled(false);
    profile.patch.optimizer.max_threads = Some(1);
    profile.patch.optimizer.max_autoroute_passes = Some(1);
    let mut loaded = load(J2, profile);
    let cancel = Arc::new(CancelToken::new());
    let job = PipelineJob {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel,
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(loaded.board.clone(), &job).unwrap_or_else(|f| panic!("{}", f.error));
    loaded.board = outcome.board.take().expect("the board");
    loaded
}

fn weighted_length(board: &RoutingBoard) -> f64 {
    let stats =
        BoardStatistics::with_options(board, freerouting::board::structure::Unit::Mm, false, false);
    f64::from(stats.traces.total_weighted_length)
}

fn lowest_unfixed_trace(board: &RoutingBoard) -> ItemId {
    board
        .items()
        .filter(|i| i.is_trace() && !i.is_user_fixed())
        .map(|i| i.id())
        .min_by_key(|id| id.0)
        .expect("an unfixed trace")
}

fn reroute(loaded: &mut Loaded, item: ItemId, baseline: f64) -> ItemRouteResult {
    let cancel = CancelToken::new();
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &NullEvents,
    };
    let params = ItemPassParams {
        baseline_length: baseline,
        with_preferred_directions: false,
        use_increased_ripup_costs: false,
    };
    let settings = loaded.settings.clone();
    opt_route_item(&mut loaded.board, item, &params, &settings, &mut ctx)
}

#[test]
fn worker_uses_the_weighted_trace_metric_from_its_board_snapshot() {
    let mut loaded = routed_j2();
    let trace = lowest_unfixed_trace(&loaded.board);
    let before = weighted_length(&loaded.board);
    assert!(before > 0.0);
    let result = reroute(&mut loaded, trace, before);
    let reconstructed = result.length_reduced() + result.trace_length();
    assert!(
        (reconstructed - before).abs() < 0.001,
        "{reconstructed} {before}"
    );
    let after = weighted_length(&loaded.board);
    assert!(
        (after - result.trace_length()).abs() < 0.001,
        "{after} {result:?}"
    );
}

#[test]
fn zero_baseline_fallback_derives_from_board_snapshot() {
    let mut loaded = routed_j2();
    let trace = lowest_unfixed_trace(&loaded.board);
    let before = weighted_length(&loaded.board);
    let result = reroute(&mut loaded, trace, 0.0);
    let reconstructed = result.length_reduced() + result.trace_length();
    assert!(
        (reconstructed - before).abs() < 0.001,
        "{reconstructed} {before}"
    );
}

// ---------------------------------------------------------------------------
// O06.4: parallel evaluation with an ordered merge
// ---------------------------------------------------------------------------

fn thread_independence(dsn: &str, ses: &str, profile: impl Fn() -> TestingProfile) {
    let mut reference: Option<String> = None;
    for threads in [1, 2, 4, 8] {
        let mut loaded = routed(dsn, ses, profile());
        let run = run(&mut loaded, threads);
        assert!(run.bypass.is_none(), "{dsn}: {:?}", run.bypass);
        let print = fingerprint(&run, &loaded.board);
        eprintln!(
            "{dsn} threads {threads}: {} passes, {} items, winners {:?}",
            run.passes,
            run.items,
            run.pass_log
                .iter()
                .map(|p| p.winner.map(|w| w.item_id().0))
                .collect::<Vec<_>>()
        );
        match &reference {
            None => reference = Some(print),
            Some(r) => assert_eq!(r, &print, "{dsn}: threads {threads} differ from 1"),
        }
    }
}

#[test]
fn optimizer_is_thread_count_independent() {
    thread_independence(BM08, BM08_SES, || optimizer_profile(3, 200));
    thread_independence(J2, &reference_ses(J2), || optimizer_profile(2, 100));
}

/// bm05 from its Java route reference (14 connections unrouted, so the guards are off).
/// Each candidate reroutes for several seconds; run with `--ignored` in release.
#[test]
#[ignore = "long: about 3 minutes per thread count in release"]
fn optimizer_is_thread_count_independent_bm05() {
    thread_independence(BM05, &reference_ses(BM05), || {
        let mut profile = optimizer_profile(1, 8);
        profile.set_optimizer_enable_preflight_guards(false);
        profile
    });
}

#[test]
fn chunk_size_does_not_change_results() {
    let mut reference: Option<String> = None;
    for chunk in [1, 8, 64] {
        let mut loaded = routed(BM08, BM08_SES, optimizer_profile(3, 200));
        let options = OptimizerOptions {
            threads: Some(4),
            chunk_size: Some(chunk),
            ..OptimizerOptions::default()
        };
        let run = run_with(&mut loaded, &options, &NullEvents);
        let print = fingerprint(&run, &loaded.board);
        match &reference {
            None => reference = Some(print),
            Some(r) => assert_eq!(r, &print, "chunk {chunk} differs from chunk 1"),
        }
    }
}

#[test]
fn candidate_copy_equals_deep_copy() {
    let loaded = routed(BM08, BM08_SES, TestingProfile::default());
    let snapshot: BoardSnapshot = loaded.board.snapshot();
    let candidate = candidate_start_board(&snapshot);
    let deep = loaded.board.deep_copy();
    assert_eq!(candidate.state_hash(), deep.state_hash());
    assert_eq!(
        candidate.search_trees().default_tree().fingerprint(),
        deep.search_trees().default_tree().fingerprint()
    );
    assert_eq!(candidate.search_trees().work_meter().units(), 0);
}

fn setup<'a>(loaded: &'a Loaded, cancel: &'a CancelToken, threads: usize) -> PassSetup<'a> {
    PassSetup {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel,
        params: ItemPassParams {
            baseline_length: weighted_length(&loaded.board),
            with_preferred_directions: true,
            use_increased_ripup_costs: true,
        },
        threads,
        chunk_size: 8,
        max_consecutive_failures: 12,
    }
}

#[test]
fn candidates_leave_the_baseline_untouched() {
    let loaded = routed(BM08, BM08_SES, TestingProfile::default());
    let hash = loaded.board.state_hash();
    let tree = loaded.board.search_trees().default_tree().fingerprint();
    let ids: Vec<ItemId> = sorted_candidates(&loaded.board)
        .iter()
        .map(|c| c.id)
        .collect();
    let cancel = CancelToken::new();
    let mut results = BTreeMap::new();
    let mut total = 0;
    let pass = evaluate_pass(
        &loaded.board,
        &ids,
        &setup(&loaded, &cancel, 4),
        &mut results,
        &mut total,
    );
    assert!(!pass.consumed.is_empty());
    assert_eq!(loaded.board.state_hash(), hash);
    assert_eq!(
        loaded.board.search_trees().default_tree().fingerprint(),
        tree
    );
}

/// Every candidate counts its work on its own board: the same visits whether it runs alone,
/// after nine other candidates on the same thread, or among them on eight threads. A work
/// limit started on the candidate's board therefore fires at the same visit in all three.
#[test]
fn work_limit_is_per_candidate_not_per_thread() {
    let loaded = routed(BM08, BM08_SES, TestingProfile::default());
    let ids: Vec<ItemId> = sorted_candidates(&loaded.board)
        .iter()
        .take(10)
        .map(|c| c.id)
        .collect();
    assert_eq!(ids.len(), 10);
    let snapshot = loaded.board.snapshot();
    let cancel = CancelToken::new();
    let one = setup(&loaded, &cancel, 1);
    let alone: Vec<(ItemRouteResult, u64)> = ids
        .iter()
        .map(|id| {
            let o = evaluate_chunk(&snapshot, &[*id], &one).remove(0);
            (o.result, o.work)
        })
        .collect();
    let sequential: Vec<(ItemRouteResult, u64)> = evaluate_chunk(&snapshot, &ids, &one)
        .into_iter()
        .map(|o| (o.result, o.work))
        .collect();
    let eight = setup(&loaded, &cancel, 8);
    let parallel: Vec<(ItemRouteResult, u64)> = evaluate_chunk(&snapshot, &ids, &eight)
        .into_iter()
        .map(|o| (o.result, o.work))
        .collect();
    assert!(alone.iter().all(|(_, work)| *work > 0), "{alone:?}");
    assert_eq!(alone, sequential, "after nine others on one thread");
    assert_eq!(alone, parallel, "on eight threads");
}

// ---------------------------------------------------------------------------
// O06.5: the pass loop
// ---------------------------------------------------------------------------

/// bm01 routed by the autorouter (one pass, two items), as `BatchOptimizerDeterminismTest`.
fn routed_bm01() -> Loaded {
    let mut profile = TestingProfile::default();
    profile.set_max_passes(1);
    profile.set_max_items(2);
    profile.set_optimizer_enabled(false);
    profile.set_optimizer_enable_preflight_guards(false);
    profile.set_job_timeout("00:01:00");
    profile.patch.autorouter.max_threads = Some(1);
    let mut loaded = load(BM01, profile);
    let cancel = Arc::new(CancelToken::new());
    let job = PipelineJob {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel,
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(loaded.board.clone(), &job).unwrap_or_else(|f| panic!("{}", f.error));
    loaded.board = outcome.board.take().expect("the board");
    loaded.settings.optimizer.max_passes = 1;
    loaded.settings.optimizer.max_items = 6;
    loaded
}

#[test]
fn cross_thread_optimization_produces_identical_results() {
    let routed = routed_bm01();
    let stats_of = |board: &RoutingBoard| {
        let s = BoardStatistics::new(board);
        (
            s.connections.incomplete_count,
            s.items.via_count,
            s.traces.total_length,
            s.clearance_violations.total_count,
        )
    };
    let mut runs = Vec::new();
    for threads in [1, 4] {
        let mut loaded = Loaded {
            board: routed.board.deep_copy(),
            settings: routed.settings.clone(),
            router: routed.router.clone(),
        };
        let run = run(&mut loaded, threads);
        eprintln!(
            "bm01 threads {threads}: score {} -> {}, passes {}, accepted {:?}",
            run.score_before,
            run.score_after,
            run.passes,
            run.pass_log.iter().map(|p| p.rejection).collect::<Vec<_>>()
        );
        runs.push((
            loaded.board.state_hash(),
            stats_of(&loaded.board),
            fingerprint(&run, &loaded.board),
        ));
    }
    assert_eq!(runs[0], runs[1]);
}

#[test]
fn initial_board_state_preserved_when_optimization_does_not_improve() {
    let mut loaded = routed_bm01();
    loaded.settings.optimizer.max_passes = 0;
    let before = BoardStatistics::new(&loaded.board);
    let hash = loaded.board.state_hash();
    let score = before.legacy_normalized_score(&loaded.settings.scoring);
    let run = run(&mut loaded, 1);
    assert_eq!(run.passes, 0);
    let after = BoardStatistics::new(&loaded.board);
    assert_eq!(loaded.board.state_hash(), hash);
    assert!((after.legacy_normalized_score(&loaded.settings.scoring) - score).abs() < 0.001);
    assert_eq!(
        before.connections.incomplete_count,
        after.connections.incomplete_count
    );
    assert_eq!(before.items.via_count, after.items.via_count);
}

/// `BatchOptimizerStrategyTest.optimizerDoesNotDegradeFullyRoutedBoard`, the five variants,
/// on bm08 with the session unfixed (Java's board keeps it fixed, see the module docs).
#[test]
fn optimizer_does_not_degrade_fully_routed_board() {
    let variants = [
        (ItemSelectionStrategy::Sequential, 1),
        (ItemSelectionStrategy::Sequential, 4),
        (ItemSelectionStrategy::Sequential, 2),
        (ItemSelectionStrategy::Prioritized, 1),
        (ItemSelectionStrategy::Prioritized, 4),
    ];
    for (strategy, threads) in variants {
        let mut profile = optimizer_profile(2, 50);
        profile.set_max_passes(1);
        profile.patch.optimizer.item_selection_strategy = Some(strategy);
        let mut loaded = routed(BM08, BM08_SES, profile);
        let before = BoardStatistics::new(&loaded.board);
        let score_before = before.legacy_normalized_score(&loaded.settings.scoring);
        let run = run(&mut loaded, threads);
        let after = BoardStatistics::new(&loaded.board);
        let label = format!("{strategy:?} x {threads}");
        assert!(run.passes > 0, "{label}");
        assert_eq!(after.connections.incomplete_count, 0, "{label}");
        assert_eq!(after.clearance_violations.total_count, 0, "{label}");
        let score_after = after.legacy_normalized_score(&loaded.settings.scoring);
        assert!(
            score_after >= score_before - 0.001,
            "{label}: {score_before} -> {score_after}"
        );
    }
}

/// The same test as Java runs it, on the session as imported (user fixed): no candidates,
/// and the board comes back unchanged.
#[test]
fn optimizer_on_a_fixed_session_changes_nothing() {
    let mut loaded = load(BM08, optimizer_profile(2, 50));
    import(&mut loaded.board, BM08_SES, false);
    let hash = loaded.board.state_hash();
    let run = run(&mut loaded, 4);
    assert!(run.pass_log.iter().all(|p| p.candidates.is_empty()));
    assert_eq!(loaded.board.state_hash(), hash);
}

/// `MaxFanoutOptimizerSettingsTest.optimizerMaxPassesAndMaxItems`: the job with one router
/// pass, then the optimizer with one pass and two items.
#[test]
fn optimizer_max_passes_and_max_items() {
    let mut profile = TestingProfile::default();
    profile.set_enabled(true);
    profile.set_max_passes(1);
    profile.set_fanout_enabled(false);
    profile.set_optimizer_enabled(true);
    profile.set_optimizer_max_passes(1);
    profile.set_optimizer_max_items(2);
    profile.set_job_timeout("00:02:00");
    let loaded = load(J2, profile);
    let cancel = Arc::new(CancelToken::new());
    let job = PipelineJob {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel,
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let outcome =
        run_pipeline(loaded.board.clone(), &job).unwrap_or_else(|f| panic!("{}", f.error));
    assert!(outcome.board.is_some());
    let optimizer = outcome.optimizer.expect("the optimizer ran");
    assert!(optimizer.passes <= 1);
    assert!(optimizer.items <= 2, "{optimizer:?}");
}

/// `runBatchLoopSetsManifestSnapshotsAndZeroPassesWhenBypassed`: with the excess weights at 0
/// the optimizer score is 1000, so guard 3 skips the stage and no pass runs.
#[test]
fn run_batch_loop_sets_zero_passes_when_bypassed() {
    let mut loaded = load(BM08, TestingProfile::default());
    import(&mut loaded.board, BM08_SES, false);
    let scoring = &mut loaded.settings.optimizer_scoring;
    scoring.excess_wire_length_weight = 0.0;
    scoring.excess_via_weight = 0.0;
    scoring.excess_bend_weight = 0.0;
    let run = run(&mut loaded, 1);
    assert!(run.bypass.is_some());
    assert_eq!(run.passes, 0);
    assert!(
        run.log[0].text.starts_with("Skipping optimization stage: "),
        "{:?}",
        run.log
    );
}

/// A pass whose candidates all fail (no autoroute pass per candidate, so every rerouted
/// item stays unrouted) clears the increased ripup costs inside the pass, so the loop sees
/// a 0 % improvement below the threshold and stops after one pass (the kept quirk).
#[test]
fn first_pass_without_winner_stops() {
    let mut profile = optimizer_profile(10, 200);
    profile.patch.optimizer.max_autoroute_passes = Some(0);
    let mut loaded = routed(BM08, BM08_SES, profile);
    let hash = loaded.board.state_hash();
    let run = run(&mut loaded, 2);
    assert_eq!(run.passes, 1, "{:?}", run.log);
    let pass = &run.pass_log[0];
    assert!(pass.winner.is_none());
    assert!(!pass.results.is_empty());
    assert!(pass.results.iter().all(|r| !r.improved()));
    // The pass was gated as not improved and the incumbent came back.
    assert!(pass.rejection.is_some());
    let deep = {
        let mut copy = routed(BM08, BM08_SES, TestingProfile::default()).board;
        copy = copy.deep_copy();
        copy.state_hash()
    };
    assert_eq!(loaded.board.state_hash(), deep);
    let _ = hash;
}

/// N06.9 `router.java_quirks.opt_first_pass_stops`: the pass of
/// `first_pass_without_winner_stops` (no candidate improves). `true` stops after it; `false`
/// goes on for one pass at normal ripup costs, which finds no winner either and stops on the
/// threshold.
#[test]
fn first_pass_switch_both_behaviours() {
    let profile = || {
        let mut profile = optimizer_profile(10, 400);
        profile.patch.optimizer.max_autoroute_passes = Some(0);
        profile
    };
    let mut java = routed(BM08, BM08_SES, profile());
    assert!(java.router.java_quirks().opt_first_pass_stops);
    let java_run = run(&mut java, 2);
    assert_eq!(java_run.passes, 1, "{:?}", java_run.log);

    let mut fixed = routed(BM08, BM08_SES, profile());
    let quirks = JavaQuirks::JAVA
        .with("opt_first_pass_stops", false)
        .unwrap();
    fixed.router = fixed.router.with_java_quirks(quirks);
    let fixed_run = run(&mut fixed, 2);
    assert_eq!(fixed_run.passes, 2, "{:?}", fixed_run.log);
    // The first pass is Java's, bit for bit.
    assert_eq!(fixed_run.pass_log[0], java_run.pass_log[0]);
    let second = &fixed_run.pass_log[1];
    assert!(second.winner.is_none());
    assert!(!second.results.is_empty());
    assert!(second.results.iter().all(|r| !r.improved()));
    let stops: Vec<&str> = fixed_run
        .log
        .iter()
        .map(|l| l.text.as_str())
        .filter(|t| t.starts_with("Stopping optimizer because the improvement"))
        .collect();
    assert_eq!(stops.len(), 1, "{:?}", fixed_run.log);
    // Both runs end on the incumbent, the routed board.
    assert_eq!(fixed.board.state_hash(), java.board.state_hash());
}

/// N06.9's correction of the `opt_first_pass_stops` notes: an improved result has an
/// `improvement_percentage` of exactly 0 when the board had no vias (or no length) before,
/// not because of the integer via ratio. With vias, the integer ratio gives 0 only when the
/// two ratios sum to exactly 2 (here: a connection routed with the old length).
#[test]
fn zero_improvement_needs_a_board_without_vias() {
    let lengths = [1000.0, 999.0, 900.0, 500.0, 1001.0, 1500.0, 2500.0];
    let mut zero_with_vias = Vec::new();
    for vias_before in 0..=6 {
        for vias_after in 0..=13 {
            for &length_after in &lengths {
                for (incomplete_before, incomplete_after) in [(1, 1), (1, 0)] {
                    let r = ItemRouteResult::new(
                        ItemId(1),
                        vias_before,
                        vias_after,
                        1000.0,
                        length_after,
                        incomplete_before,
                        incomplete_after,
                    );
                    if !r.improved() || r.improvement_percentage() != 0.0 {
                        continue;
                    }
                    if vias_before != 0 {
                        zero_with_vias.push((vias_before, vias_after, length_after));
                    }
                }
            }
        }
    }
    // The coincidences: via ratio 1 (truncated) and the same length; nothing else.
    assert!(!zero_with_vias.is_empty());
    for (before, after, length) in &zero_with_vias {
        assert_eq!(*length, 1000.0, "{before} {after}");
        assert_eq!(after / before, 1, "{before} {after}");
    }
    // Without vias every improved result is 0 %.
    let r = ItemRouteResult::new(ItemId(1), 0, 0, 1000.0, 900.0, 1, 1);
    assert!(r.improved());
    assert_eq!(r.improvement_percentage(), 0.0);
    // With vias, fewer vias or a shorter length is never 0 %.
    for (before, after, length) in [
        (3, 2, 1000.0),
        (2, 1, 1001.0),
        (4, 4, 999.0),
        (1, 0, 1500.0),
    ] {
        let r = ItemRouteResult::new(ItemId(1), before, after, 1000.0, length, 1, 1);
        assert!(r.improved(), "{before} {after} {length}");
        assert_ne!(r.improvement_percentage(), 0.0, "{before} {after} {length}");
    }
}

// ---------------------------------------------------------------------------
// O06.6: the stage and its events
// ---------------------------------------------------------------------------

#[test]
fn registry_resolves_the_optimizer() {
    let settings = load(BM08, TestingProfile::default()).settings;
    let registry = StageRegistry::builtin();
    let selection = StageSelection::resolve(&settings, &registry);
    assert_eq!(selection.optimizer.id, OPTIMIZER_ID);
    let pipeline = registry.build(&selection, &settings).expect("the pipeline");
    assert_eq!(pipeline.optimizer.id(), "freerouting-optimizer");
}

#[derive(Default)]
struct Recorder {
    events: RefCell<Vec<String>>,
}

impl RouterEvents for Recorder {
    fn is_listening(&self) -> bool {
        true
    }
    fn progress(&self, p: Progress) -> bool {
        self.events.borrow_mut().push(format!(
            "progress {:?} pass {} end {} position {}",
            p.phase,
            p.pass,
            p.pass_end,
            p.optimizer_position.is_some()
        ));
        true
    }
    fn delta(&self, _: Vec<ItemId>, _: Vec<ItemGeometry>) {
        self.events.borrow_mut().push("delta".to_string());
    }
    fn replaced(&self, _: BoardSnapshot) {
        self.events.borrow_mut().push("replaced".to_string());
    }
    fn finished(&self, board: RoutingBoard, _: RunSummary) -> Option<RoutingBoard> {
        Some(board)
    }
}

/// Per pass: progress at the start, the swap and a progress after an adopted winner, a
/// progress closing the pass, and a swap and a progress when the gate restores the
/// incumbent.
#[test]
fn optimizer_event_order() {
    let mut loaded = routed(BM08, BM08_SES, optimizer_profile(3, 200));
    let recorder = Recorder::default();
    let quiet = run(&mut routed(BM08, BM08_SES, optimizer_profile(3, 200)), 2);
    let run = run_with(
        &mut loaded,
        &OptimizerOptions {
            threads: Some(2),
            ..OptimizerOptions::default()
        },
        &recorder,
    );
    // Listening changes nothing.
    assert_eq!(quiet.pass_log, run.pass_log);
    let mut expected = Vec::new();
    for pass in &run.pass_log {
        let n = pass.pass;
        expected.push(format!(
            "progress Optimize pass {n} end false position {}",
            n > 1
        ));
        if pass.winner.is_some() {
            expected.push("replaced".to_string());
            expected.push(format!(
                "progress Optimize pass {n} end false position true"
            ));
        }
        expected.push(format!(
            "progress Optimize pass {n} end true position {}",
            !pass.results.is_empty() || n > 1
        ));
        if pass.rejection.is_some() {
            expected.push("replaced".to_string());
            expected.push(format!(
                "progress Optimize pass {n} end false position true"
            ));
        }
    }
    if run.restored_best {
        expected.push("replaced".to_string());
        expected.push(format!(
            "progress Optimize pass {} end false position true",
            run.passes
        ));
    }
    assert!(run.pass_log.iter().any(|p| p.winner.is_some()));
    assert_eq!(*recorder.events.borrow(), expected);
}

/// `optimizer.timeout` is armed when the stage starts and stops it (C07.8): a zero limit
/// fires before the first pass, and the job reports the timeout.
#[test]
fn optimizer_timeout_is_a_full_stop() {
    let mut profile = optimizer_profile(3, 200);
    profile.set_enabled(false);
    profile.set_fanout_enabled(false);
    profile.patch.optimizer.timeout = Some("00:00:00".to_string());
    let mut loaded = routed(BM08, BM08_SES, profile);
    loaded.settings.optimizer.enabled = true;
    let cancel = Arc::new(CancelToken::new());
    let job = PipelineJob {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: Arc::clone(&cancel),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let outcome =
        run_pipeline(loaded.board.clone(), &job).unwrap_or_else(|f| panic!("{}", f.error));
    let optimizer = outcome.optimizer.expect("the optimizer ran");
    assert_eq!(optimizer.passes, 0, "{optimizer:?}");
    assert!(outcome.timed_out);
    assert_eq!(
        outcome.summary.status,
        CompletionStatus::CompletedWithTimeout
    );
    assert!(cancel.is_stop_requested());
}

// ---------------------------------------------------------------------------
// Issue952FanoutIgnoredNetClassTest, cases 5 and 6
// ---------------------------------------------------------------------------

fn item_counts(board: &RoutingBoard) -> (usize, usize) {
    (board.traces().len(), board.vias().len())
}

/// `optimizerSkipsCandidatesFromIgnoredNetClass`, on the unfixed session (with Java's fixed
/// one there is nothing to filter): with net class 0 ignored, every trace has an ignored net
/// and the optimizer finds no candidate.
#[test]
fn issue952_optimizer_skips_candidates_from_ignored_net_class() {
    let mut loaded = routed(BM08, BM08_SES, optimizer_profile(1, i32::MAX));
    let board = &mut loaded.board;
    let first = board.traces()[0];
    assert!(!board.has_ignored_nets(board.item(first).unwrap()));
    let class = board.rules.net_classes.nth(0).unwrap();
    board
        .rules
        .net_classes
        .get_mut(class)
        .is_ignored_by_autorouter = true;
    assert!(board.has_ignored_nets(board.item(first).unwrap()));
    let counts = item_counts(board);
    let hash = board.state_hash();
    let run = run(&mut loaded, 4);
    assert!(
        run.pass_log.iter().all(|p| p.candidates.is_empty()),
        "{run:?}"
    );
    assert_eq!(item_counts(&loaded.board), counts);
    assert_eq!(loaded.board.state_hash(), hash);
}

/// `mixedNetConnectionWithIgnoredNetIsProtectedFromOptimizerRipup`: a via of an active and an
/// ignored net on an active trace's first corner is never ripped up.
#[test]
fn issue952_mixed_net_connection_with_ignored_net_is_protected_from_optimizer_ripup() {
    let mut loaded = routed(BM08, BM08_SES, optimizer_profile(1, i32::MAX));
    let board = &mut loaded.board;
    let class = board.rules.get_new_named_net_class("IgnoredClass");
    board
        .rules
        .net_classes
        .get_mut(class)
        .is_ignored_by_autorouter = true;
    let ignored_net = board.rules.nets.add("IGNORED_MIXED", 1, false, class);
    let trace = board.traces()[0];
    let (active_net, corner) = {
        let item = board.item(trace).unwrap();
        assert!(!board.has_ignored_nets(item));
        let corner = item.as_trace().unwrap().first_corner();
        (item.net_number(0), corner)
    };
    let Point::Int(corner) = corner else {
        panic!("a rational corner");
    };
    let rule = board.rules.via_rules.nth(0).expect("a via rule");
    let info = board.rules.via_rules.get(rule).vias()[0];
    let padstack = board.rules.via_infos.get(info).padstack;
    let via = board.insert_via(
        padstack,
        corner,
        &[active_net, ignored_net],
        0,
        FixedState::Unfixed,
        true,
    );
    assert!(board.has_ignored_nets(board.item(via).unwrap()));
    let vias = board.vias().len();
    let _ = run(&mut loaded, 4);
    assert!(
        loaded.board.item(via).is_some(),
        "the mixed-net via stays on the board"
    );
    assert_eq!(loaded.board.vias().len(), vias);
}

/// P03.2: the workers run up to a window of candidates ahead of the ordered merge. A pass
/// that the failure limit ends early, with candidates already running past the stop, consumes
/// the same candidates with the same results and work as one candidate at a time on one
/// thread, and adopts the same winner.
#[test]
fn lookahead_past_an_early_stop_changes_nothing() {
    let mut stopped_early = false;
    for loaded in [
        routed(BM08, BM08_SES, TestingProfile::default()),
        routed(J2, &reference_ses(J2), TestingProfile::default()),
    ] {
        let ids: Vec<ItemId> = sorted_candidates(&loaded.board)
            .iter()
            .map(|c| c.id)
            .collect();
        let run = |threads: usize, chunk_size: usize, failures: i32| {
            let cancel = CancelToken::new();
            let pass_setup = PassSetup {
                chunk_size,
                max_consecutive_failures: failures,
                ..setup(&loaded, &cancel, threads)
            };
            let (mut results, mut total) = (BTreeMap::new(), 0);
            let pass = evaluate_pass(&loaded.board, &ids, &pass_setup, &mut results, &mut total);
            let winner = pass
                .winner
                .as_ref()
                .map(|(result, board)| (*result, board.state_hash()));
            (
                pass.consumed,
                pass.consumed_work,
                pass.early_stop,
                winner,
                results,
                total,
            )
        };
        for failures in [1, 12] {
            let reference = run(1, 1, failures);
            stopped_early |= reference.2 && reference.0.len() < ids.len();
            for (threads, chunk) in [(8, 8), (8, 64), (3, 5), (2, 1)] {
                assert_eq!(
                    run(threads, chunk, failures),
                    reference,
                    "{threads} threads, chunk {chunk}, failure limit {failures}"
                );
            }
        }
    }
    assert!(stopped_early, "no pass ended early at the failure limit");
}

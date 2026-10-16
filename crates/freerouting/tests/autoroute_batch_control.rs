//! The D9 stage registry and the batch pipeline's pure decision logic
//! (A10), against Java at `a773a70ec`.
//!
//! - Ported Java tests, grouped by package:
//!   - `autoroute.pipeline`: `RoutingPipelineAlgorithmNormalizationTest`
//!     (all three), `FanoutRetryTest` (its one test),
//!     `FanoutSliceAndLastMileTest` (its one test). Not ported from the
//!     package: `AutoroutePipelineCollaboratorTest` (routes boards, A13),
//!     `BatchOptimizer*Test` (O06), `NamedAlgorithmSealingTest` (Java class
//!     hierarchy only; the port's stages are traits).
//!   - `autoroute`: `RoutableLayersSafetyCheckTest` (its one test). Not
//!     ported here: `BatchAutorouterDebugTest`, `StrictDrcEnforcementTest`
//!     (route boards, A11), `BoardHistoryTest` (A08).
//! - The registry and selection (A10.2), the pass controller over synthetic
//!   score sequences (A10.3), the ripup policy (A10.5) and the fanout stop
//!   rules (A10.6).
//! - Differential checks against `tools/java-reference/probes/batch/BatchProbe.java`,
//!   whose output is committed under `tests/data/batch_probe/<kind>/`
//!   (kinds `items`, `fanout-order`, `unrouted-report`; regenerate with the
//!   command in that file's docs). The boards load as the DRC loads them
//!   (`drc::load_design`), which gives Java's item ids (B01.1).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use freerouting::autoroute::batch::control::FreeroutingPassController;
use freerouting::autoroute::batch::fanout_rules::{
    FanoutPassTracker, FanoutStop, PinSortingOrder, fanout_components, retry_fanout,
};
use freerouting::autoroute::batch::ordering::{FreeroutingOrdering, select_items};
use freerouting::autoroute::batch::report::{
    NoRoutableLayer, check_routable_layers, corridor, is_removable_blocker, unrouted_report,
};
use freerouting::autoroute::batch::ripup_policy::FreeroutingRipupPolicy;
use freerouting::autoroute::stages::{
    ActionOutcome, BoardScore, NetOrdering, PassController, PassDecision, PassReport, PassStart,
    Phase, RestoredBoard, RipupPolicy, RouteTask, StageRegistry, StageSelection, StopReason,
};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::drc::json::JsonValue;
use freerouting::drc::load_design;
use freerouting::geometry::FloatPoint;
use freerouting::io::read_session;
use freerouting::settings::board::{BoardRouterSettings, apply_board_specific};
use freerouting::settings::{
    ResolveContext, RouterSettings, SettingsPatch, Source, default_patch, resolve,
};

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

const CORES: usize = 4;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn load(name: &str) -> RoutingBoard {
    let path = fixtures().join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    load_design(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn settings_with(edit: impl FnOnce(&mut SettingsPatch)) -> RouterSettings {
    let ctx = ResolveContext { cores: CORES };
    let mut patch = default_patch(CORES);
    edit(&mut patch);
    resolve(&[(Source::Defaults, patch)], &ctx).settings
}

fn defaults() -> RouterSettings {
    settings_with(|_| {})
}

/// The probe lines of `kind` on `board` (header first).
fn probe_lines(kind: &str, board: &str) -> Vec<JsonValue> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/batch_probe")
        .join(kind)
        .join(format!("{board}.jsonl"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let lines: Vec<JsonValue> = text
        .lines()
        .map(|line| JsonValue::parse(line).expect("probe line is JSON"))
        .collect();
    assert_eq!(text_of(&lines[0], "javaCommit"), "a773a70ec", "probe pin");
    assert!(
        lines.iter().all(|l| member(l, "exception").is_none()),
        "{kind}/{board}: the Java probe threw"
    );
    lines
}

fn member<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match value {
        JsonValue::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn text_of(value: &JsonValue, key: &str) -> String {
    match member(value, key) {
        Some(JsonValue::String(s)) => s.clone(),
        Some(JsonValue::Number(s)) => s.clone(),
        other => panic!("{key}: {other:?}"),
    }
}

fn int_of(value: &JsonValue, key: &str) -> i64 {
    text_of(value, key).parse().expect("integer")
}

fn ints_of(value: &JsonValue, key: &str) -> Vec<i64> {
    match member(value, key) {
        Some(JsonValue::Array(items)) => items
            .iter()
            .map(|item| match item {
                JsonValue::Number(n) => n.parse().expect("integer"),
                other => panic!("{key}: {other:?}"),
            })
            .collect(),
        other => panic!("{key}: {other:?}"),
    }
}

fn bits_of(value: &JsonValue, key: &str) -> Vec<String> {
    match member(value, key) {
        Some(JsonValue::Array(items)) => items
            .iter()
            .map(|item| match item {
                JsonValue::String(s) => s.clone(),
                other => panic!("{key}: {other:?}"),
            })
            .collect(),
        other => panic!("{key}: {other:?}"),
    }
}

fn bits(value: f64) -> String {
    format!("0x{:016x}", value.to_bits())
}

fn ids(tasks: &[RouteTask]) -> Vec<i64> {
    tasks.iter().map(|t| i64::from(t.item.0)).collect()
}

// ---------------------------------------------------------------------
// Ported Java tests: autoroute.pipeline
// ---------------------------------------------------------------------

/// `RoutingPipelineAlgorithmNormalizationTest.unknownAlgorithmNormalizesToCurrent`.
#[test]
fn unknown_algorithm_normalizes_to_current() {
    let settings = settings_with(|p| {
        p.autorouter.algorithm = Some("custom-unsupported-router".into());
    });
    let selection = StageSelection::from_settings(&settings);
    assert_eq!(selection.algorithm.id, "freerouting-router");
    assert_eq!(
        selection.warnings,
        vec![
            "The algorithm 'custom-unsupported-router' is not supported. The default algorithm \
             'freerouting-router' will be used instead."
                .to_string()
        ]
    );
}

/// `RoutingPipelineAlgorithmNormalizationTest.legacyAlgorithmTokenNormalizesToCurrent`.
#[test]
fn legacy_algorithm_token_normalizes_to_current() {
    let settings =
        settings_with(|p| p.autorouter.algorithm = Some("freerouting-router-v19".into()));
    let selection = StageSelection::from_settings(&settings);
    assert_eq!(selection.algorithm.id, "freerouting-router");
    assert_eq!(selection.warnings.len(), 1);
}

/// `RoutingPipelineAlgorithmNormalizationTest.canonicalAlgorithmRemainsUnchanged`.
#[test]
fn canonical_algorithm_remains_unchanged() {
    let selection = StageSelection::from_settings(&defaults());
    assert_eq!(selection.algorithm.id, "freerouting-router");
    assert!(selection.warnings.is_empty());
}

/// `FanoutRetryTest.retriesUntilTheComponentEscapesSomethingElse`.
#[test]
fn retries_until_the_component_escapes_something_else() {
    assert!(retry_fanout(None, 0));
    assert!(!retry_fanout(Some(0), 0));
    assert!(retry_fanout(Some(0), 1));
}

/// `FanoutSliceAndLastMileTest.airlineCorridorCoversBothEndsAndAMargin`.
#[test]
fn airline_corridor_covers_both_ends_and_a_margin() {
    let corridor = corridor(FloatPoint::new(0.0, 0.0), FloatPoint::new(400.0, 100.0));
    assert!(corridor.ll.x < 0);
    assert!(corridor.ur.x > 400);
    assert!(corridor.ll.y < 0);
    assert!(corridor.ur.y > 100);
    assert_eq!(corridor.ur.x - 400, 100);
}

// ---------------------------------------------------------------------
// Ported Java tests: autoroute
// ---------------------------------------------------------------------

/// `RoutableLayersSafetyCheckTest.testRoutingFailsWhenAllLayersDisabledCurrent`;
/// Java's `IllegalArgumentException` is [`NoRoutableLayer`].
#[test]
fn routing_fails_when_all_layers_disabled() {
    let board = load("Issue508-DAC2020_bm01.dsn");
    let ctx = ResolveContext { cores: CORES };
    let mut patch = default_patch(CORES);
    apply_board_specific(&mut patch, &board);
    let enabled = BoardRouterSettings::from_settings(
        &resolve(&[(Source::Defaults, patch.clone())], &ctx).settings,
    );
    assert_eq!(check_routable_layers(&board, &enabled), Ok(()));
    for layer in patch.layers.as_mut().expect("board layers") {
        layer.routable = Some(false);
    }
    let disabled =
        BoardRouterSettings::from_settings(&resolve(&[(Source::Defaults, patch)], &ctx).settings);
    let error = check_routable_layers(&board, &disabled).unwrap_err();
    assert_eq!(error, NoRoutableLayer);
    assert_eq!(
        error.to_string(),
        "Cannot start autorouter: all layers are disabled."
    );
}

// ---------------------------------------------------------------------
// A10.2: registry and selection
// ---------------------------------------------------------------------

#[test]
fn default_selection_builds_the_freerouting_bundle() {
    let settings = defaults();
    let selection = StageSelection::from_settings(&settings);
    let pipeline = StageRegistry::builtin()
        .build(&selection, &settings)
        .expect("defaults build");
    assert_eq!(pipeline.ordering.id(), "freerouting-ordering");
    assert_eq!(pipeline.ripup_policy.id(), "freerouting-ripup");
    assert_eq!(pipeline.pass_controller.id(), "freerouting-passes");
    assert_eq!(pipeline.connection_router.id(), "freerouting-connection");
    assert_eq!(pipeline.plane_taps.id(), "freerouting-plane-taps");
    assert_eq!(pipeline.neck_down.id(), "freerouting-neck-down");
    assert_eq!(pipeline.fanout.id(), "freerouting-fanout");
    assert_eq!(pipeline.optimizer.id(), "freerouting-optimizer");
    assert!(pipeline.board_stages.is_empty());
    let entries = selection.entries();
    let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        [
            "router.autorouter.algorithm",
            "router.fanout.algorithm",
            "router.autorouter.ordering",
            "router.autorouter.connection_router",
            "router.autorouter.ripup_policy",
            "router.autorouter.pass_controller",
            "router.special.plane_taps",
            "router.special.neck_down",
            "router.optimizer.algorithm",
        ]
    );
    assert!(selection.log_line().starts_with(
        "Routing stages: router.autorouter.algorithm=freerouting-router, \
         router.fanout.algorithm=freerouting-fanout,"
    ));
}

#[test]
fn unknown_optimizer_algorithm_warns_and_defaults() {
    let settings = settings_with(|p| p.optimizer.algorithm = Some("my-optimizer".into()));
    let selection = StageSelection::from_settings(&settings);
    assert_eq!(selection.optimizer.id, "freerouting-optimizer");
    assert_eq!(
        selection.warnings,
        vec![
            "The algorithm 'my-optimizer' is not supported by the batch autorouter. The default \
             algorithm 'freerouting-optimizer' will be used instead."
                .to_string()
        ]
    );
}

#[test]
fn unknown_per_stage_id_is_an_error_naming_the_key_and_the_known_ids() {
    let settings = settings_with(|p| p.autorouter.ordering = Some("nope".into()));
    let selection = StageSelection::from_settings(&settings);
    let error = StageRegistry::builtin()
        .build(&selection, &settings)
        .unwrap_err();
    assert_eq!(error.key, "router.autorouter.ordering");
    assert_eq!(error.id, "nope");
    assert_eq!(error.known, ["freerouting-ordering"]);
    assert_eq!(
        error.to_string(),
        "unknown stage id 'nope' in router.autorouter.ordering; known ids: freerouting-ordering"
    );

    let settings = settings_with(|p| {
        p.special.stages = Some(vec!["freerouting-unrouted-report".into(), "typo".into()]);
    });
    let error = StageRegistry::builtin()
        .build(&StageSelection::from_settings(&settings), &settings)
        .unwrap_err();
    assert_eq!(error.key, "router.special.stages");
    assert_eq!(error.id, "typo");
    assert_eq!(error.known, ["freerouting-unrouted-report"]);
}

#[test]
fn listed_board_stages_are_built_in_order() {
    let settings = settings_with(|p| {
        p.special.stages = Some(vec!["freerouting-unrouted-report".into()]);
    });
    let selection = StageSelection::from_settings(&settings);
    let pipeline = StageRegistry::builtin()
        .build(&selection, &settings)
        .unwrap();
    let stage_ids: Vec<&str> = pipeline.board_stages.iter().map(|s| s.id()).collect();
    assert_eq!(stage_ids, ["freerouting-unrouted-report"]);
    assert_eq!(
        selection.entries().last().unwrap(),
        &(
            "router.special.stages".to_string(),
            "freerouting-unrouted-report".to_string()
        )
    );
}

/// A test-only ordering: Freerouting's selection, reversed.
struct ReverseOrdering;

impl NetOrdering for ReverseOrdering {
    fn id(&self) -> &'static str {
        "reverse-ordering"
    }
    fn select(&mut self, board: &RoutingBoard) -> Vec<RouteTask> {
        let mut tasks = select_items(board);
        tasks.reverse();
        tasks
    }
    fn order(&mut self, tasks: Vec<RouteTask>, _: &RoutingBoard, _: u32) -> Vec<RouteTask> {
        tasks
    }
    fn on_pass_end(&mut self, _: &PassReport) {}
    fn reset(&mut self) {}
}

#[test]
fn registered_test_ordering_changes_the_task_order_and_is_named() {
    let mut registry = StageRegistry::builtin();
    registry
        .ordering
        .insert("reverse-ordering", |_| Box::new(ReverseOrdering));
    let settings = settings_with(|p| p.autorouter.ordering = Some("reverse-ordering".into()));
    let selection = StageSelection::resolve(&settings, &registry);
    let mut pipeline = registry.build(&selection, &settings).expect("registered");
    assert_eq!(pipeline.ordering.id(), "reverse-ordering");
    assert!(selection.entries().contains(&(
        "router.autorouter.ordering".into(),
        "reverse-ordering".into()
    )));
    assert!(!selection.ordering.from_default);

    let board = load("Issue026-J2_reference.dsn");
    let default_order = FreeroutingOrdering::new().select(&board);
    let custom = pipeline.ordering.select(&board);
    assert!(default_order.len() > 1);
    let mut reversed = default_order.clone();
    reversed.reverse();
    assert_eq!(custom, reversed);
    assert_ne!(custom, default_order);
}

// ---------------------------------------------------------------------
// A10.3: the pass controller
// ---------------------------------------------------------------------

/// A synthetic run: what the driver would do with scripted scores.
struct Script {
    /// `(score, incomplete)` after pass `n`.
    pass: fn(u32) -> (f32, usize),
    /// The board's score before pass 1.
    initial: f32,
    fanout: bool,
    max_passes: i32,
    /// Items each last-mile ripup removes.
    last_mile_removes: usize,
    /// The history gives no board on a restore.
    history_exhausted: bool,
}

impl Default for Script {
    fn default() -> Self {
        Script {
            pass: |_| (100.0, 5),
            initial: 0.0,
            fanout: false,
            max_passes: 0,
            last_mile_removes: 0,
            history_exhausted: false,
        }
    }
}

/// Runs `script` and returns every decision as `(pass, decision)`, the
/// last being the stop.
fn simulate(script: &Script) -> Vec<(u32, PassDecision)> {
    let mut ctl = FreeroutingPassController::new(script.max_passes, script.fanout, true);
    let mut history: Vec<f32> = Vec::new();
    let mut board_score = script.initial;
    let mut log = Vec::new();
    for pass in 1..=200 {
        match ctl.before_pass(pass, false) {
            PassStart::Stop(reason) => {
                log.push((pass, PassDecision::Stop(reason)));
                return log;
            }
            PassStart::Run { stop_router } => assert!(!stop_router),
        }
        // `bh.add(router.board)` before the pass (AutorouteBatchLoop.java:344).
        history.push(board_score);
        let (score, incomplete) = (script.pass)(pass);
        let max = history.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let report = PassReport {
            pass,
            continue_routing: true,
            score,
            incomplete,
            history_size: history.len(),
            history_max_score: max,
            ..PassReport::default()
        };
        board_score = score;
        let mut decision = ctl.after_pass(&report);
        loop {
            log.push((pass, decision));
            let outcome = match decision {
                PassDecision::Continue => break,
                PassDecision::Stop(_) => return log,
                PassDecision::Restore { .. } => {
                    if script.history_exhausted {
                        ActionOutcome::Restored(None)
                    } else {
                        board_score = max;
                        ActionOutcome::Restored(Some(RestoredBoard {
                            rank: Some(1),
                            board: BoardScore {
                                score: max,
                                incomplete,
                            },
                        }))
                    }
                }
                PassDecision::FanoutRecovery => {
                    ActionOutcome::FanoutRecovered(BoardScore { score, incomplete })
                }
                PassDecision::LastMileRipup => ActionOutcome::LastMile {
                    removed: script.last_mile_removes,
                    board: BoardScore { score, incomplete },
                },
            };
            decision = ctl.after_action(&outcome);
        }
    }
    panic!("no stop in 200 passes");
}

fn count(log: &[(u32, PassDecision)], what: PassDecision) -> usize {
    log.iter().filter(|(_, d)| *d == what).count()
}

fn last(log: &[(u32, PassDecision)]) -> (u32, PassDecision) {
    *log.last().unwrap()
}

/// A flat score: pass 8 records the global best (`:564-567`, `lastBestScore`
/// starts at `-inf`), passes 9-18 count no improvement (`:489`), and the
/// pass-local counter reaches 10 at pass 18 (`:538`), the same pass the
/// global tracker's `18 - 8 >= 10` would stop (`:568`). Equal scores never
/// restore (`:365`, strict `>`), and the two last-mile ripups that remove
/// nothing do not reset the counter (`:521-528`).
#[test]
fn flat_score_stops_at_pass_18_without_restoring() {
    let log = simulate(&Script::default());
    assert_eq!(last(&log), (18, PassDecision::Stop(StopReason::Stagnation)));
    assert_eq!(
        count(
            &log,
            PassDecision::Restore {
                max_restore_count: 3
            }
        ),
        0
    );
    // `0 < 5 <= 8` incompletes and the counter at 3 (pass 11) and 4 (12).
    let ripups: Vec<u32> = log
        .iter()
        .filter(|(_, d)| *d == PassDecision::LastMileRipup)
        .map(|(p, _)| *p)
        .collect();
    assert_eq!(ripups, [11, 12]);
}

/// With last-mile ripups that remove items, the pass-local counter resets
/// (`:523-528`) but the global best does not move, so the global tracker
/// stops at pass 18 (`:568`), its earliest stop. Last-mile fires at most
/// twice (`LAST_MILE_MAX_ATTEMPTS`, `:519`).
#[test]
fn global_tracker_stops_a_flat_score_at_pass_18() {
    let log = simulate(&Script {
        last_mile_removes: 3,
        ..Script::default()
    });
    assert_eq!(
        last(&log),
        (18, PassDecision::Stop(StopReason::GlobalStagnation))
    );
    let ripups: Vec<u32> = log
        .iter()
        .filter(|(_, d)| *d == PassDecision::LastMileRipup)
        .map(|(p, _)| *p)
        .collect();
    // Counter 3 at pass 11, reset; 3 again at pass 14; then no attempt left.
    assert_eq!(ripups, [11, 14]);
}

/// Last-mile needs `0 < incomplete <= 8` (`:517-518`).
#[test]
fn last_mile_needs_few_incompletes() {
    let log = simulate(&Script {
        pass: |_| (100.0, 9),
        ..Script::default()
    });
    assert_eq!(count(&log, PassDecision::LastMileRipup), 0);
    assert_eq!(last(&log), (18, PassDecision::Stop(StopReason::Stagnation)));
}

/// A score that drops at pass 8: the history holds the boards before
/// passes 1-8 (8 boards, `:357`), pass 8 is a check pass (`8 % 4 == 0`,
/// `:359`) and the best stored score 70 beats 50 (`:365`), so the board is
/// restored with 3 tries (`MAXIMUM_TRIES_ON_THE_SAME_BOARD`); the counter is
/// reset and `lastBestScore` becomes the restored score (`:386-390`).
#[test]
fn score_drop_at_pass_8_restores() {
    let log = simulate(&Script {
        pass: |p| {
            if p < 8 {
                (10.0 * p as f32, 5)
            } else {
                (50.0, 5)
            }
        },
        ..Script::default()
    });
    assert_eq!(
        log[7],
        (
            8,
            PassDecision::Restore {
                max_restore_count: 3
            }
        )
    );
    assert_eq!(log[8], (8, PassDecision::Continue));
    // No restore before pass 8: the history is shorter than 8 boards.
    assert!(log[..7].iter().all(|(_, d)| *d == PassDecision::Continue));
}

/// A restore that finds no board stops the router (`:367-374`).
#[test]
fn exhausted_history_stops() {
    let log = simulate(&Script {
        pass: |p| {
            if p < 8 {
                (10.0 * p as f32, 5)
            } else {
                (50.0, 5)
            }
        },
        history_exhausted: true,
        ..Script::default()
    });
    assert_eq!(
        last(&log),
        (8, PassDecision::Stop(StopReason::HistoryExhausted))
    );
}

/// A restored board ranked past `BOARD_RANK_LIMIT` (30) stops (`:376-381`).
#[test]
fn restored_board_past_the_rank_limit_stops() {
    let mut ctl = FreeroutingPassController::new(0, false, true);
    let report = PassReport {
        pass: 8,
        continue_routing: true,
        score: 1.0,
        incomplete: 3,
        history_size: 8,
        history_max_score: 2.0,
        ..PassReport::default()
    };
    assert_eq!(
        ctl.after_pass(&report),
        PassDecision::Restore {
            max_restore_count: 3
        }
    );
    let restored = RestoredBoard {
        rank: Some(31),
        board: BoardScore {
            score: 2.0,
            incomplete: 3,
        },
    };
    assert_eq!(
        ctl.after_action(&ActionOutcome::Restored(Some(restored))),
        PassDecision::Stop(StopReason::RankLimit)
    );
}

/// The one-time fanout recovery: counter 3 at pass 11 with incompletes
/// (`:495-498`); it resets the counter (`:504`), so the next stagnation
/// round goes to last-mile instead (`fanoutRecoveryApplied`).
#[test]
fn fanout_recovery_fires_once() {
    let log = simulate(&Script {
        fanout: true,
        ..Script::default()
    });
    let recoveries: Vec<u32> = log
        .iter()
        .filter(|(_, d)| *d == PassDecision::FanoutRecovery)
        .map(|(p, _)| *p)
        .collect();
    assert_eq!(recoveries, [11]);
    // Pass 14: the counter is 3 again, last-mile runs; pass 18 is the
    // global stop (18 - 8 >= 10) before the counter (7) reaches 10.
    assert_eq!(count(&log, PassDecision::LastMileRipup), 2);
    assert_eq!(
        last(&log),
        (18, PassDecision::Stop(StopReason::GlobalStagnation))
    );
}

/// `max_passes = 2` stops before pass 3 (`:331-336`).
#[test]
fn max_passes_stops_before_the_next_pass() {
    let log = simulate(&Script {
        max_passes: 2,
        ..Script::default()
    });
    assert_eq!(last(&log), (3, PassDecision::Stop(StopReason::MaxPasses)));
    assert!(StopReason::MaxPasses.requests_router_stop());
}

/// A timed-out job requests a router stop and still runs the pass
/// (`:314-316`); the restore check then runs on any pass (`:357-359`).
#[test]
fn timed_out_pass_requests_a_stop_and_checks_restore() {
    let mut ctl = FreeroutingPassController::new(0, false, true);
    assert_eq!(
        ctl.before_pass(3, true),
        PassStart::Run { stop_router: true }
    );
    let report = PassReport {
        pass: 3,
        continue_routing: true,
        score: 1.0,
        incomplete: 3,
        router_stop_requested: true,
        timed_out: true,
        history_size: 3,
        history_max_score: 2.0,
        ..PassReport::default()
    };
    assert_eq!(
        ctl.after_pass(&report),
        PassDecision::Restore {
            max_restore_count: 3
        }
    );
    let restored = RestoredBoard {
        rank: Some(1),
        board: BoardScore {
            score: 2.0,
            incomplete: 2,
        },
    };
    assert_eq!(
        ctl.after_action(&ActionOutcome::Restored(Some(restored))),
        PassDecision::Stop(StopReason::StopRequested)
    );
}

/// A pass with nothing to route ends the loop without a router stop
/// (`:602-605`), so the final tail removal runs (`:635-639`); a complete
/// board with a positive score resets the counter (`:591-599`).
#[test]
fn completed_pass_stops_without_a_router_stop() {
    let mut ctl = FreeroutingPassController::new(0, false, true);
    let report = PassReport {
        pass: 2,
        continue_routing: false,
        score: 900.0,
        incomplete: 0,
        history_size: 2,
        history_max_score: 10.0,
        ..PassReport::default()
    };
    assert_eq!(
        ctl.after_pass(&report),
        PassDecision::Stop(StopReason::Completed)
    );
    assert!(!StopReason::Completed.requests_router_stop());
    assert_eq!(ctl.no_improvement_passes(), 0);
    let finish = ctl.finish(900.0, 10.0, false);
    assert!(!finish.restore_best);
    assert!(finish.remove_tails);
}

/// The final best-board restore (`:610-612`, strict `>`) and the final
/// tail removal only when the router ran and was not stopped (`:635-639`).
#[test]
fn finish_restores_the_best_board_and_removes_tails() {
    let mut ctl = FreeroutingPassController::new(0, false, true);
    let finish = ctl.finish(5.0, 7.0, true);
    assert!(finish.restore_best);
    assert!(!finish.remove_tails);
    assert!(!ctl.finish(7.0, 7.0, false).restore_best);
    let mut not_run = FreeroutingPassController::new(-1, false, false);
    assert!(!not_run.finish(7.0, 7.0, false).remove_tails);
    let settings = settings_with(|p| p.autorouter.enabled = Some(false));
    let mut from_settings = FreeroutingPassController::from_settings(&settings);
    assert!(!from_settings.finish(7.0, 7.0, false).remove_tails);
}

/// The threshold is added in `f32`: `1000.0 + 0.5` and `1000.25` compare
/// as Java's floats do.
#[test]
fn improvement_threshold_is_added_in_f32() {
    let mut ctl = FreeroutingPassController::new(0, false, true);
    let mut report = PassReport {
        pass: 8,
        continue_routing: true,
        score: 1000.0,
        incomplete: 5,
        history_size: 8,
        history_max_score: 1000.0,
        ..PassReport::default()
    };
    assert_eq!(ctl.after_pass(&report), PassDecision::Continue);
    report.pass = 9;
    report.score = 1000.5;
    report.history_max_score = 1000.5;
    // 1000.5 > 1000.0 + 0.5 is false.
    assert_eq!(ctl.after_pass(&report), PassDecision::Continue);
    assert_eq!(ctl.no_improvement_passes(), 1);
    report.pass = 10;
    report.score = 1000.5625;
    report.history_max_score = 1000.5625;
    assert_eq!(ctl.after_pass(&report), PassDecision::Continue);
    assert_eq!(ctl.no_improvement_passes(), 0);
}

// ---------------------------------------------------------------------
// A10.4: the ordering
// ---------------------------------------------------------------------

/// `Collections.shuffle(0..39, new Random(2 * 1000003 + 40))`, as Java at
/// `a773a70ec` prints it:
/// `probe.sh PIN tools/java-reference/probes/batch/BatchProbe.java shuffle 2000046 40`.
#[test]
fn pass_shuffle_matches_java() {
    let java = [
        17, 25, 14, 9, 30, 19, 18, 36, 0, 10, 34, 21, 12, 27, 8, 7, 29, 23, 26, 38, 6, 3, 37, 13,
        4, 16, 2, 28, 20, 11, 31, 5, 24, 39, 35, 33, 22, 32, 15, 1,
    ];
    let mut list: Vec<i32> = (0..40).collect();
    let seed = 2 * freerouting::autoroute::batch::ordering::SHUFFLE_PASS_FACTOR + 40;
    assert_eq!(seed, 2_000_046);
    freerouting::geometry::jmath::JavaRandom::new(seed).shuffle(&mut list);
    assert_eq!(list, java);
}

/// The `items` probe: `getAutorouteItems`, then `reorderSingleThreadItems`
/// in four states. Rust's incomplete nets of the loaded board must be the
/// ones Java used.
fn check_items(board_name: &str) {
    let lines = probe_lines("items", board_name);
    let board = load(&format!("{board_name}.dsn"));
    let selected = select_items(&board);
    assert_eq!(
        ids(&selected),
        ints_of(&lines[1], "items"),
        "{board_name}: select"
    );
    let plane = int_of(&lines[1], "plane") as usize;
    assert_eq!(
        selected.iter().filter(|t| t.is_plane).count(),
        plane,
        "{board_name}: plane list"
    );
    assert!(selected[..plane].iter().all(|t| t.is_plane));
    let mut drc = freerouting::drc::DesignRulesChecker::new(&board);
    drc.calculate_all_incompletes();
    let incomplete_nets = drc.incomplete_net_numbers();
    for line in &lines[2..] {
        let stage = text_of(line, "stage");
        let nets: BTreeSet<i32> = ints_of(line, "nets").iter().map(|n| *n as i32).collect();
        if stage == "pass2-persistent" {
            assert_eq!(nets, incomplete_nets, "{board_name}: incomplete nets");
        }
        let mut ordering = FreeroutingOrdering::new();
        ordering.on_pass_end(&PassReport {
            incomplete_nets: nets,
            ..PassReport::default()
        });
        ordering.set_stagnation_count(int_of(line, "stagnation") as u32);
        let pass = int_of(line, "pass") as u32;
        let order = ordering.order(selected.clone(), &board, pass);
        assert_eq!(ids(&order), ints_of(line, "items"), "{board_name}: {stage}");
    }
}

#[test]
fn items_probe_j2() {
    check_items("Issue026-J2_reference");
}

#[test]
fn items_probe_dac2020_bm01() {
    check_items("Issue508-DAC2020_bm01");
}

#[test]
fn items_probe_z10_plane_board() {
    check_items("Issue269-z10_module");
}

/// The anti-oscillation state: the stagnation count grows while the
/// incomplete count does not drop, and `reset` clears it (`:233-239`,
/// `:319-323`).
#[test]
fn ordering_state_counts_stagnation_and_resets() {
    let mut ordering = FreeroutingOrdering::new();
    let report = |incomplete: usize, nets: &[i32]| PassReport {
        incomplete,
        incomplete_nets: nets.iter().copied().collect(),
        ..PassReport::default()
    };
    ordering.on_pass_end(&report(5, &[1, 2]));
    assert_eq!(ordering.stagnation_count(), 0, "no previous count");
    ordering.on_pass_end(&report(5, &[1, 2]));
    assert_eq!(ordering.stagnation_count(), 1);
    ordering.on_pass_end(&report(6, &[3]));
    assert_eq!(ordering.stagnation_count(), 2);
    assert_eq!(ordering.previous_incomplete_nets(), &BTreeSet::from([3]));
    ordering.on_pass_end(&report(4, &[3]));
    assert_eq!(ordering.stagnation_count(), 0);
    ordering.on_pass_end(&report(4, &[3]));
    ordering.reset();
    assert_eq!(ordering, FreeroutingOrdering::new());
}

// ---------------------------------------------------------------------
// A10.5: the ripup policy
// ---------------------------------------------------------------------

#[test]
fn ripup_cost_schedule() {
    let policy = FreeroutingRipupPolicy::new(true);
    assert_eq!(policy.ripup_cost(Phase::Autoroute, 1, 100), 100);
    assert_eq!(policy.ripup_cost(Phase::Autoroute, 7, 100), 700);
    // The fanout counts passes from 0.
    assert_eq!(policy.ripup_cost(Phase::Fanout, 0, 100), 100);
    assert_eq!(policy.ripup_cost(Phase::Fanout, 2, 100), 300);
    // Java's int product wraps.
    assert_eq!(policy.ripup_cost(Phase::Autoroute, 2, i32::MAX), -2);
    let no_fanout_ripup = FreeroutingRipupPolicy::new(false);
    assert_eq!(no_fanout_ripup.ripup_cost(Phase::Fanout, 0, 100), -1);
    assert_eq!(no_fanout_ripup.ripup_cost(Phase::Autoroute, 3, 100), 300);
    let from_defaults = FreeroutingRipupPolicy::from_settings(&defaults());
    assert_eq!(from_defaults, FreeroutingRipupPolicy::new(true));
}

#[test]
fn strict_check_from_pass_3() {
    let policy = FreeroutingRipupPolicy::new(true);
    assert!(!policy.strict_check(1, false));
    assert!(!policy.strict_check(2, false));
    assert!(policy.strict_check(3, false));
    assert!(policy.strict_check(9, false));
    assert!(policy.strict_check(1, true));
}

/// J2 with Java's routed session. `SesReader` imports wires user-fixed
/// (`SesReader.java:324, 405`), so every trace and via whose id is not a
/// multiple of 5 is unfixed (Java `Item.unfix`); the rest stay fixed.
fn routed_j2() -> RoutingBoard {
    let mut board = load("Issue026-J2_reference.dsn");
    let ses = std::fs::read_to_string(fixtures().join("reference/route/Issue026-J2_reference.ses"))
        .expect("J2 session");
    read_session(&mut board, &ses).expect("session imports");
    let wires: Vec<ItemId> = board
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.id().0 % 5 != 0)
        .map(|i| i.id())
        .collect();
    for id in wires {
        board.unfix(id);
    }
    board
}

fn traces_and_vias(board: &RoutingBoard, net: i32) -> Vec<ItemId> {
    board
        .connectable_items(net)
        .into_iter()
        .filter(|id| board.item(*id).is_some_and(|i| i.is_trace() || i.is_via()))
        .collect()
}

/// `after_failure` on J2 with Java's session: the second failure of an
/// item rips every unfixed single-net trace and via of its net, in
/// `connectable_items` order, and keeps the DSN's user-fixed wires; the
/// first failure rips nothing.
#[test]
fn after_failure_rips_the_net_on_the_second_failure() {
    let mut board = routed_j2();
    let unfixed = |board: &RoutingBoard, net: i32| -> Vec<ItemId> {
        traces_and_vias(board, net)
            .into_iter()
            .filter(|id| {
                let item = board.item(*id).unwrap();
                !item.is_user_fixed() && item.net_count() == 1
            })
            .collect()
    };
    let net = (1..=board.rules.nets.max_net_number())
        .max_by_key(|net| (unfixed(&board, *net).len(), -net))
        .unwrap();
    let rip = unfixed(&board, net);
    let all = traces_and_vias(&board, net);
    let kept: Vec<ItemId> = all.iter().copied().filter(|id| !rip.contains(id)).collect();
    assert!(rip.len() > 3, "net {net} has session wires");
    assert!(!kept.is_empty(), "net {net} keeps fixed wires");
    let pin = board
        .connectable_items(net)
        .into_iter()
        .find(|id| board.item(*id).is_some_and(|i| i.is_pin()))
        .expect("a pin on the net");
    let task = RouteTask {
        item: pin,
        net,
        is_plane: false,
    };
    let count_before = board.item_count();
    let mut policy = FreeroutingRipupPolicy::new(true);
    policy.after_failure(&mut board, &task, 1);
    assert_eq!(board.item_count(), count_before, "one failure rips nothing");
    assert_eq!(
        FreeroutingRipupPolicy::net_items_to_rip(&board, net, 2),
        rip,
        "connectable_items order"
    );
    policy.after_failure(&mut board, &task, 2);
    assert_eq!(traces_and_vias(&board, net), kept);
    assert_eq!(board.item_count(), count_before - rip.len());
    for id in &rip {
        assert!(board.item(*id).is_none(), "{id:?} removed");
    }
}

/// DSN wires of `(type route)` are user-fixed (Java `Wiring.calcFixed`), so
/// `after_failure` leaves them.
#[test]
fn after_failure_keeps_user_fixed_wires() {
    let mut board = load("Issue957-smoke-ldo-led.dsn");
    let net = board
        .rules
        .nets
        .get_by_name("+5V", 1)
        .expect("+5V")
        .net_number;
    let before = traces_and_vias(&board, net);
    assert_eq!(before.len(), 3, "the fixture's +5V wires");
    assert!(
        before
            .iter()
            .all(|id| board.item(*id).unwrap().is_user_fixed())
    );
    let task = RouteTask {
        item: before[0],
        net,
        is_plane: false,
    };
    FreeroutingRipupPolicy::new(true).after_failure(&mut board, &task, 5);
    assert_eq!(traces_and_vias(&board, net), before);
}

// ---------------------------------------------------------------------
// A10.6: fanout ordering and stop rules
// ---------------------------------------------------------------------

/// The `fanout-order` probe: Java's `sortedComponents` for each pin
/// sorting order, with every value the comparators read, bit for bit.
#[test]
fn fanout_order_probe_dev_board() {
    let lines = probe_lines("fanout-order", "Issue558-dev-board");
    let board = load("Issue558-dev-board.dsn");
    let mut checked = 0;
    for name in [
        "outer_first",
        "inner_first",
        "distanceToClosestOnNet",
        "surroundingsDensity",
        "distance_to_closest_on_net",
    ] {
        let java: Vec<&JsonValue> = lines[1..]
            .iter()
            .filter(|l| text_of(l, "stage") == name)
            .collect();
        let rust = fanout_components(&board, PinSortingOrder::from_name(name));
        assert_eq!(rust.len(), java.len(), "{name}: component count");
        for (r, j) in rust.iter().zip(&java) {
            let at = format!("{name}, component {}", r.component);
            assert_eq!(i64::from(r.component), int_of(j, "component"), "{at}");
            assert_eq!(r.smd_pin_count() as i64, int_of(j, "smdPins"), "{at}");
            assert_eq!(bits(r.gravity_center.x), text_of(j, "cx"), "{at}: cx");
            assert_eq!(bits(r.gravity_center.y), text_of(j, "cy"), "{at}: cy");
            let pins: Vec<i64> = r.pins.iter().map(|p| i64::from(p.pin.0)).collect();
            assert_eq!(pins, ints_of(j, "pins"), "{at}: pin order");
            let dist: Vec<String> = r.pins.iter().map(|p| bits(p.distance_to_center)).collect();
            assert_eq!(dist, bits_of(j, "dist"), "{at}: distance to centre");
            let closest: Vec<String> = r
                .pins
                .iter()
                .map(|p| bits(p.distance_to_closest_on_net))
                .collect();
            assert_eq!(closest, bits_of(j, "closest"), "{at}: closest on net");
            let density: Vec<i64> = r
                .pins
                .iter()
                .map(|p| i64::from(p.surroundings_density))
                .collect();
            assert_eq!(density, ints_of(j, "density"), "{at}: density");
            checked += 1;
        }
    }
    assert!(checked > 5, "the probe has components");
}

/// The undocumented spellings fall back to pin-index order (A00 open
/// question 7, Java's string comparisons).
#[test]
fn pin_sorting_order_names() {
    assert_eq!(
        PinSortingOrder::from_name("outer_first"),
        PinSortingOrder::OuterFirst
    );
    assert_eq!(
        PinSortingOrder::from_name("surroundings_density"),
        PinSortingOrder::PinIndex
    );
    assert_eq!(
        PinSortingOrder::from_name("bogus"),
        PinSortingOrder::PinIndex
    );
}

/// The fanout pass loop's stop rules (`BatchFanout.java:118-165`), as a
/// table: `(routed, vias, timed out, hash)` per pass and the expected stop.
#[test]
fn fanout_pass_loop_stop_rules() {
    type Pass = (i32, usize, bool, u128);
    type Case<'a> = (&'a str, &'a [Pass], Option<(usize, FanoutStop)>);
    let cases: &[Case] = &[
        (
            "a pass routes 0 pins",
            &[(5, 3, false, 2), (0, 3, false, 3)],
            Some((1, FanoutStop::NoPinRouted)),
        ),
        // The first outcome sets the state; three identical repeats stop.
        (
            "identical outcome 3 times",
            &[
                (4, 2, false, 2),
                (4, 2, false, 3),
                (4, 2, false, 4),
                (4, 2, false, 5),
            ],
            Some((3, FanoutStop::NoProgress)),
        ),
        (
            "a different outcome resets the count",
            &[
                (4, 2, false, 2),
                (4, 2, false, 3),
                (4, 3, false, 4),
                (4, 3, false, 5),
                (4, 3, false, 6),
            ],
            None,
        ),
        (
            "an unchanged board hash",
            &[(4, 2, false, 2), (5, 2, false, 2)],
            Some((1, FanoutStop::BoardUnchanged)),
        ),
        (
            "the first pass leaves the start hash",
            &[(4, 2, false, 1)],
            Some((0, FanoutStop::BoardUnchanged)),
        ),
        (
            "the deadline after a pass",
            &[(4, 2, true, 2)],
            Some((0, FanoutStop::Deadline)),
        ),
    ];
    for (name, passes, want) in cases {
        let mut tracker = FanoutPassTracker::new(1);
        let mut got = None;
        for (i, &(routed, vias, timed_out, hash)) in passes.iter().enumerate() {
            if let Some(stop) = tracker.after_pass(routed, vias, timed_out, hash) {
                got = Some((i, stop));
                break;
            }
        }
        assert_eq!(got, *want, "{name}");
    }
    // The routed count is shifted into the high word: 1 routed with 0 vias
    // differs from 0 routed with 1 << 32 vias' low word.
    let mut tracker = FanoutPassTracker::new(0);
    assert_eq!(tracker.after_pass(1, 0, false, 1), None);
    assert_eq!(tracker.after_pass(1, 0, false, 2), None);
    let tracker = FanoutPassTracker::new(0);
    assert_eq!(tracker.before_pass(true, 0, 10), Some(FanoutStop::Deadline));
    assert_eq!(
        tracker.before_pass(false, 10, 10),
        Some(FanoutStop::MaxItems)
    );
    assert_eq!(tracker.before_pass(false, 9, 10), None);
    assert_eq!(
        tracker.before_pass(false, 1000, 0),
        None,
        "max_items 0 is off"
    );
}

// ---------------------------------------------------------------------
// A10.7: reports and small checks
// ---------------------------------------------------------------------

/// `AutorouteUnroutedReport.build` on the loaded board, against Java's text.
#[test]
fn unrouted_report_matches_java_on_congflock() {
    let lines = probe_lines("unrouted-report", "PCBench-retroreflectors-CONGFLOCK");
    let board = load("PCBench-retroreflectors-CONGFLOCK.dsn");
    assert_eq!(unrouted_report(&board), text_of(&lines[1], "text"));
}

#[test]
fn unrouted_report_of_a_complete_board() {
    let board = load("empty_board.dsn");
    assert_eq!(unrouted_report(&board), "  (no unrouted connections found)");
}

/// `isRemovableBlocker`: traces and vias with a net, not fixed, not on the
/// airline's net.
#[test]
fn removable_blockers() {
    let board = routed_j2();
    let trace = board
        .items()
        .find(|i| i.is_trace() && !i.is_user_fixed())
        .unwrap();
    let own = trace.net_number(0);
    let other = if own == 1 { 2 } else { 1 };
    assert!(is_removable_blocker(&board, trace, other));
    assert!(!is_removable_blocker(&board, trace, own), "own net");
    let via = board
        .items()
        .find(|i| i.is_via() && !i.is_user_fixed())
        .unwrap();
    assert!(is_removable_blocker(&board, via, -5));
    let pin = board.items().find(|i| i.is_pin()).unwrap();
    assert!(
        !is_removable_blocker(&board, pin, other),
        "not a trace or via"
    );
    let fixed = load("Issue957-smoke-ldo-led.dsn");
    let fixed_trace = fixed.items().find(|i| i.is_trace()).unwrap();
    assert!(!is_removable_blocker(&fixed, fixed_trace, -5), "user fixed");
}

/// The corridor's margin is at least 1 and its int arithmetic is Java's.
#[test]
fn corridor_margins() {
    let point = corridor(FloatPoint::new(10.4, 10.6), FloatPoint::new(10.4, 10.6));
    assert_eq!(
        (point.ll.x, point.ll.y, point.ur.x, point.ur.y),
        (9, 10, 11, 12)
    );
    // (int) Math.round(-2.5) = -2.
    let tie = corridor(FloatPoint::new(-2.5, 0.0), FloatPoint::new(5.5, 0.0));
    assert_eq!((tie.ll.x, tie.ur.x), (-4, 8));
}

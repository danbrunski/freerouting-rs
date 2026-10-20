//! Q20.2–Q20.4: the output check before saving (`quality::output_check`,
//! `router.output_check = off | report | repair`).
//!
//! Fixtures in `tests/data/quality/`:
//! - `q20-output-check.dsn`: net A from S1 (5, 15 mm) to S2 (35, 15 mm), net D
//!   from S4 (5, 5 mm) to S5 (35, 5 mm), both easy; a 0.4 mm via padstack in
//!   the library that no class uses.
//! - `q18-neck-floor.dsn` (Q18): net PWR2 fits into U3-2 only below 0.2 mm.
//!
//! The test hook is a board stage ([`Breach`], after the optimizer) that
//! replaces net A's wiring with a 0.1 mm trace and adds a lone 0.4 mm via of
//! net D: two breaches under the minimums 0.2 mm track and 0.5 mm via.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, PipelineOutcome, run_pipeline};
use freerouting::autoroute::stages::{
    BoardStage, StageCtx, StagePhase, StageRegistry, StageReport,
};
use freerouting::board::RoutingBoard;
use freerouting::board::structure::FixedState;
use freerouting::drc::BoardRule;
use freerouting::geometry::{IntPoint, Point};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::output_check::{Mode, OutputCheck};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};

const CHECK: &str = "crates/freerouting/tests/data/quality/q20-output-check.dsn";
const NECK: &str = "crates/freerouting/tests/data/quality/q18-neck-floor.dsn";
const SMALL_VIA: &str = "Via[0-1]_400:300_um";

/// `dsn` prepared under `profile` with `mode` and `patch`.
fn prepare(dsn: &str, profile: &str, mode: Option<&str>, patch: SettingsPatch) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    let mut patch = patch;
    patch.profile = Some(profile.to_string());
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    patch.output_check = mode.map(str::to_string);
    // Profile `enhanced` removes dangling stubs (N05.3) before the check, which
    // would take the hook's lone 0.4 mm via away; these tests need it to reach
    // the check, as Q20's input to it.
    patch.cleanup.remove_stubs = Some(false);
    input.extra_sources = vec![(Source::Testing, patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn minimums() -> SettingsPatch {
    let mut p = SettingsPatch::default();
    p.board_minimums.track_width_um = Some(200.0);
    p.board_minimums.via_diameter_um = Some(500.0);
    p
}

fn with_hook(mut patch: SettingsPatch) -> SettingsPatch {
    patch.special.stages = Some(vec![HOOK.to_string()]);
    patch
}

const HOOK: &str = "q20-breach";

/// The test hook (see the module docs).
struct Breach;

impl BoardStage for Breach {
    fn id(&self) -> &'static str {
        HOOK
    }

    fn phase(&self) -> StagePhase {
        StagePhase::AfterOptimizer
    }

    fn run(&mut self, board: &mut RoutingBoard, _ctx: &mut StageCtx) -> StageReport {
        let net = |name: &str| {
            board
                .rules
                .nets
                .get_by_name(name, 1)
                .expect("net")
                .net_number
        };
        let (a, d) = (net("A"), net("D"));
        let wiring: Vec<_> = board
            .items()
            .filter(|i| (i.is_trace() || i.is_via()) && i.contains_net(a))
            .map(|i| i.id())
            .collect();
        board.remove_items(wiring);
        let class = freerouting::rules::BoardRules::default_clearance_class();
        board.insert_trace_points(
            &[
                Point::Int(IntPoint::new(50000, 150000)),
                Point::Int(IntPoint::new(350000, 150000)),
            ],
            0,
            500,
            &[a],
            class,
            FixedState::Unfixed,
        );
        let via = board
            .library
            .padstacks
            .get_by_name(SMALL_VIA)
            .expect("via")
            .id;
        board.insert_via(
            via,
            IntPoint::new(200000, 250000),
            &[d],
            class,
            FixedState::Unfixed,
            false,
        );
        StageReport {
            log: vec!["q20 hook: a 0.1 mm trace and a 0.4 mm via".into()],
        }
    }
}

const MASK_HOOK: &str = "q20-mask-breach";

/// Q20.1 mask hook: net A's wiring becomes a 0.2 mm trace S1, down to
/// (5, 5.95 mm), across to x = 35 mm and up to S2. It passes 0.55 mm above
/// S4's pad (net D), inside a 0.8 mm mask aperture, and breaks no other rule.
struct MaskBreach;

impl BoardStage for MaskBreach {
    fn id(&self) -> &'static str {
        MASK_HOOK
    }

    fn phase(&self) -> StagePhase {
        StagePhase::AfterOptimizer
    }

    fn run(&mut self, board: &mut RoutingBoard, _ctx: &mut StageCtx) -> StageReport {
        let a = board
            .rules
            .nets
            .get_by_name("A", 1)
            .expect("net")
            .net_number;
        let wiring: Vec<_> = board
            .items()
            .filter(|i| (i.is_trace() || i.is_via()) && i.contains_net(a))
            .map(|i| i.id())
            .collect();
        board.remove_items(wiring);
        let class = freerouting::rules::BoardRules::default_clearance_class();
        let points = [
            (50000, 150000),
            (50000, 59500),
            (350000, 59500),
            (350000, 150000),
        ]
        .map(|(x, y)| Point::Int(IntPoint::new(x, y)));
        board.insert_trace_points(&points, 0, 1000, &[a], class, FixedState::Unfixed);
        StageReport {
            log: vec!["q20 mask hook: net A passes S4's mask aperture".into()],
        }
    }
}

fn registry() -> StageRegistry {
    let mut registry = StageRegistry::builtin();
    registry.board_stages.insert(HOOK, |_| Box::new(Breach));
    registry
        .board_stages
        .insert(MASK_HOOK, |_| Box::new(MaskBreach));
    registry
}

/// Routes `prepared` with `registry`; the final board and the outcome.
fn route(prepared: PreparedJob, registry: &StageRegistry) -> (RoutingBoard, PipelineOutcome) {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: Some(registry),
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    let board = outcome.board.take().expect("no listener took the board");
    (board, outcome)
}

fn ses(board: &RoutingBoard) -> String {
    write_ses(board, "q20", SesStyle::Java)
}

fn check(outcome: &PipelineOutcome) -> &OutputCheck {
    outcome.output_check.as_ref().expect("the check ran")
}

fn narrowest(board: &RoutingBoard) -> i32 {
    board
        .items()
        .filter_map(|i| i.as_trace())
        .map(|t| 2 * t.half_width)
        .min()
        .unwrap_or(i32::MAX)
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

#[test]
fn modes_parse_and_unknown_values_are_refused() {
    assert_eq!(Mode::parse(None), Ok(Mode::Off));
    assert_eq!(Mode::parse(Some("off")), Ok(Mode::Off));
    assert_eq!(Mode::parse(Some("report")), Ok(Mode::Report));
    assert_eq!(Mode::parse(Some("repair")), Ok(Mode::Repair));
    assert!(Mode::parse(Some("fix")).unwrap_err().contains("'fix'"));
    let prepared = prepare(CHECK, "enhanced", Some("fix"), SettingsPatch::default());
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let error = run_pipeline(prepared.board, &job)
        .expect_err("refused")
        .error;
    assert!(error.to_string().contains("router.output_check"), "{error}");
}

#[test]
fn the_profiles_set_repair_and_off() {
    let enhanced = prepare(CHECK, "enhanced", None, SettingsPatch::default());
    assert_eq!(enhanced.settings.output_check.as_deref(), Some("repair"));
    let legacy = prepare(CHECK, "legacy", None, SettingsPatch::default());
    assert_eq!(Mode::of(&legacy.settings), Mode::Off);
}

#[test]
fn repair_removes_the_breaching_trace_and_via_and_retries_them() {
    let registry = registry();
    let prepared = prepare(CHECK, "enhanced", Some("repair"), with_hook(minimums()));
    let (board, outcome) = route(prepared, &registry);
    let c = check(&outcome);
    assert!(c.found >= 2, "{c:?}");
    assert!(c.by_rule.contains_key(&BoardRule::TrackWidth), "{c:?}");
    assert!(c.by_rule.contains_key(&BoardRule::ViaDiameter), "{c:?}");
    assert_eq!(c.remaining, 0);
    assert!(c.removed >= 2);
    assert_eq!(c.retried, 1, "net A's connection: {c:?}");
    assert_eq!(c.repaired, 1);
    assert_eq!(c.left_open, 0);
    let text = ses(&board);
    assert!(!text.contains(SMALL_VIA), "the 0.4 mm via is saved");
    assert!(narrowest(&board) >= 2000, "a trace under 0.2 mm is saved");
    assert_eq!(incomplete(&board), 0, "net A routed again");
    assert!(
        c.log.iter().any(|l| l.contains("track_width 1")),
        "{:?}",
        c.log
    );
}

#[test]
fn report_counts_and_changes_nothing() {
    let registry = registry();
    let prepared = prepare(CHECK, "enhanced", Some("report"), with_hook(minimums()));
    let (board, outcome) = route(prepared, &registry);
    let c = check(&outcome);
    assert!(c.found >= 2);
    assert_eq!(c.removed, 0);
    assert_eq!(c.remaining, c.found);
    assert!(ses(&board).contains(SMALL_VIA));
    assert_eq!(narrowest(&board), 1000);
    // `off` writes the same session as `report`.
    let prepared = prepare(CHECK, "enhanced", Some("off"), with_hook(minimums()));
    let (off, outcome) = route(prepared, &registry);
    assert!(outcome.output_check.is_none());
    assert_eq!(ses(&off), ses(&board));
}

#[test]
fn a_connection_that_fits_only_below_the_rules_is_left_unrouted() {
    // PWR2 into U3-2 needs a 0.12 mm neck; the board minimum is 0.2 mm. The
    // hook-less run leaves it open; a 0.12 mm trace put in by a stage is
    // removed, retried, and left open.
    struct Neck;
    impl BoardStage for Neck {
        fn id(&self) -> &'static str {
            "q20-neck"
        }
        fn phase(&self) -> StagePhase {
            StagePhase::AfterOptimizer
        }
        fn run(&mut self, board: &mut RoutingBoard, _ctx: &mut StageCtx) -> StageReport {
            let net = board.rules.nets.get_by_name("PWR2", 1).unwrap().net_number;
            let class = freerouting::rules::BoardRules::default_clearance_class();
            board.insert_trace_points(
                &[
                    Point::Int(IntPoint::new(30000, 150000)),
                    Point::Int(IntPoint::new(270000, 150000)),
                ],
                0,
                599,
                &[net],
                class,
                FixedState::Unfixed,
            );
            StageReport::default()
        }
    }
    let mut registry = StageRegistry::builtin();
    registry.board_stages.insert("q20-neck", |_| Box::new(Neck));
    let mut patch = SettingsPatch::default();
    patch.board_minimums.track_width_um = Some(200.0);
    patch.special.stages = Some(vec!["q20-neck".to_string()]);
    let prepared = prepare(NECK, "enhanced", Some("repair"), patch);
    let (board, outcome) = route(prepared, &registry);
    let c = check(&outcome);
    assert!(c.by_rule.contains_key(&BoardRule::TrackWidth), "{c:?}");
    assert!(c.retried >= 1, "{c:?}");
    assert_eq!(c.repaired, 0);
    assert!(c.left_open >= 1);
    assert_eq!(c.remaining, 0);
    assert!(narrowest(&board) >= 2000);
}

#[test]
fn two_runs_give_the_same_session() {
    let registry = registry();
    let run = || {
        let prepared = prepare(CHECK, "enhanced", Some("repair"), with_hook(minimums()));
        let (board, outcome) = route(prepared, &registry);
        let log: Vec<String> = outcome.log.iter().map(|l| l.text.clone()).collect();
        (ses(&board), check(&outcome).clone(), log)
    };
    let (a, ca, la) = run();
    let (b, cb, lb) = run();
    assert_eq!(a, b);
    // The run log carries no time, so equal runs log equal lines.
    assert!(
        la.iter().any(|l| l.starts_with("Output check: removed")),
        "{la:?}"
    );
    assert_eq!(la, lb);
    // Equality leaves out the times.
    assert_eq!(ca, cb);
    assert_eq!(
        (ca.found, ca.removed, ca.retried, ca.repaired, ca.left_open),
        (cb.found, cb.removed, cb.retried, cb.repaired, cb.left_open)
    );
}

#[test]
fn a_clean_board_gives_the_session_of_off() {
    let registry = StageRegistry::builtin();
    let (repaired, outcome) = route(
        prepare(CHECK, "enhanced", Some("repair"), minimums()),
        &registry,
    );
    let c = check(&outcome);
    assert_eq!(c.found, 0, "{c:?}");
    assert!(!c.changed());
    let (off, _) = route(
        prepare(CHECK, "enhanced", Some("off"), minimums()),
        &registry,
    );
    assert_eq!(ses(&repaired), ses(&off));
    assert_eq!(incomplete(&repaired), 0);
}

const INPUT: &str = "crates/freerouting/tests/data/quality/q20-input-violations.dsn";

/// Q20.4: the input's breaches are logged once at load and kept.
#[test]
fn input_violations_are_reported_once_and_kept() {
    let prepared = prepare(INPUT, "enhanced", Some("repair"), minimums());
    let input_ses = ses(&prepared.board);
    let (board, outcome) = route(prepared, &StageRegistry::builtin());
    let c = check(&outcome);
    assert_eq!(c.input.get(&BoardRule::TrackWidth), Some(&1), "{c:?}");
    assert!(
        c.input.get(&BoardRule::Clearance).is_some_and(|n| *n >= 1),
        "{c:?}"
    );
    let load: Vec<&str> = outcome
        .log
        .iter()
        .map(|l| l.text.as_str())
        .filter(|t| t.contains("the board already has"))
        .collect();
    assert_eq!(load.len(), 1, "{:?}", outcome.log);
    assert!(
        load[0].contains("track_width 1") && load[0].contains("clearance"),
        "{load:?}"
    );
    // Nothing the router made breaks a rule, and the input wiring stays.
    assert_eq!(c.found, 0, "{c:?}");
    assert_eq!(c.remaining, 0);
    assert_eq!(c.removed, 0);
    assert_eq!(narrowest(&board), 1000, "the 0.1 mm input track is kept");
    let routed = ses(&board);
    for wire in ["5000 25000", "15000 25250"] {
        let scaled: Vec<String> = wire
            .split(' ')
            .map(|v| (v.parse::<i32>().unwrap() * 10).to_string())
            .collect();
        let needle = scaled.join(" ");
        assert_eq!(
            input_ses.contains(&needle),
            routed.contains(&needle),
            "input wire at {wire} changed"
        );
    }
    assert_eq!(incomplete(&board), 0, "A and D routed");
}

/// Q20.4: a repair removes only routed copper of a breach with input copper.
#[test]
fn the_repair_never_removes_input_copper() {
    // The hook's 0.1 mm trace of net A crosses nothing of the input; the
    // input's own breaches stay as they are.
    let registry = registry();
    let prepared = prepare(INPUT, "enhanced", Some("repair"), with_hook(minimums()));
    let input_ids: Vec<_> = prepared
        .board
        .items()
        .filter(|i| i.is_trace() || i.is_via())
        .map(|i| i.id())
        .collect();
    let (board, outcome) = route(prepared, &registry);
    let c = check(&outcome);
    assert!(c.removed >= 2, "{c:?}");
    for id in input_ids {
        assert!(board.item(id).is_some(), "input item {id:?} removed");
    }
    assert_eq!(narrowest(&board), 1000);
}

/// Q20.1 mask rule: a pad's own aperture (`router.solder_mask_expansions`)
/// is checked on routed copper; a repair removes the trace and the retry
/// routes net A outside it.
#[test]
fn copper_inside_a_pads_mask_aperture_is_found_and_repaired() {
    let registry = registry();
    let patch = |mode: &str| {
        let mut p = SettingsPatch::default();
        p.special.stages = Some(vec![MASK_HOOK.to_string()]);
        p.solder_mask_expansions = Some("S4:1=800,0".to_string());
        (Some(mode.to_string()), p)
    };
    let (mode, p) = patch("report");
    let (board, outcome) = route(prepare(CHECK, "enhanced", mode.as_deref(), p), &registry);
    let c = check(&outcome);
    assert_eq!(c.by_rule.get(&BoardRule::MaskBridge), Some(&1), "{c:?}");
    assert_eq!(c.removed, 0);
    assert_eq!(c.remaining, c.found);
    assert!(narrowest(&board) == 2000);

    let (mode, p) = patch("repair");
    let (board, outcome) = route(prepare(CHECK, "enhanced", mode.as_deref(), p), &registry);
    let c = check(&outcome);
    assert_eq!(c.by_rule.get(&BoardRule::MaskBridge), Some(&1), "{c:?}");
    assert!(c.removed >= 1, "{c:?}");
    assert_eq!(c.retried, 1, "net A's connection: {c:?}");
    assert_eq!(c.repaired, 1, "{c:?}");
    assert_eq!(c.remaining, 0, "{c:?}");
    assert_eq!(
        incomplete(&board),
        0,
        "net A routed again outside the aperture"
    );
}

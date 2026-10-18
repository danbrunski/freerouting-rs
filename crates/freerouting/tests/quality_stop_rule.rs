//! Q04: the `quantum-stop` pass controller (`quality::stop_rule`).
//!
//! Q04.1: table-driven runs of the controller over synthetic score
//! sequences, against Java's controller alone (`FreeroutingPassController`,
//! A10.3). Q04.2: the batch loop with a scripted pass runner, the default
//! controller against `quantum-stop`: where the rule fires after the last
//! score improvement of the default run, the final best-board restore
//! leaves both on the same board and the sessions are equal.

mod autoroute_common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::batch::batch_loop::PassExecutor;
use freerouting::autoroute::batch::control::FreeroutingPassController;
use freerouting::autoroute::batch::pass::{PassOutcome, PassStages};
use freerouting::autoroute::batch::snapshot::BoardFigures;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, PipelineOutcome, run_pipeline_with};
use freerouting::autoroute::stages::{
    ActionOutcome, BoardScore, PassController, PassDecision, PassReport, PassStart, RestoredBoard,
    StageCtx, StageRegistry, StopReason,
};
use freerouting::board::RoutingBoard;
use freerouting::board::items::StopConnectionOption;
use freerouting::board::structure::FixedState;
use freerouting::geometry::{Point, Polyline};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::stop_rule::{
    CONTROLLER_ID, Check, QuantumStop, StopRule, check_settings,
};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, RouterSettings, Source};

// ---------------------------------------------------------------------------
// Q04.1: the controller over synthetic score sequences
// ---------------------------------------------------------------------------

/// A synthetic run: the score and incomplete count after each pass, and
/// the pass after which nothing is left to route (Java `autoroutePass`
/// returning false), if any.
#[derive(Clone, Copy)]
struct Script {
    pass: fn(u32) -> (f32, usize),
    max_passes: i32,
    done_at: Option<u32>,
}

impl Script {
    fn new(pass: fn(u32) -> (f32, usize)) -> Script {
        Script {
            pass,
            max_passes: 40,
            done_at: None,
        }
    }
}

/// Runs `script` against `ctl` as the batch loop would (a history of the
/// board before every pass, Java's `getMaxScore` starting at 0, a restore
/// bringing back the best stored score) and returns the decisions as
/// `(pass, decision)`, the last being the stop.
fn simulate(ctl: &mut dyn PassController, script: &Script) -> Vec<(u32, PassDecision)> {
    let mut history: Vec<f32> = Vec::new();
    let mut board_score = 0.0f32;
    let mut log = Vec::new();
    for pass in 1..=200 {
        match ctl.before_pass(pass, false) {
            PassStart::Stop(reason) => {
                log.push((pass, PassDecision::Stop(reason)));
                return log;
            }
            PassStart::Run { stop_router } => assert!(!stop_router),
        }
        history.push(board_score);
        let (score, incomplete) = (script.pass)(pass);
        let max = history.iter().copied().fold(0.0f32, f32::max);
        let report = PassReport {
            pass,
            continue_routing: script.done_at != Some(pass),
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
                    board_score = max;
                    ActionOutcome::Restored(Some(RestoredBoard {
                        rank: Some(1),
                        board: BoardScore {
                            score: max,
                            incomplete,
                        },
                    }))
                }
                PassDecision::FanoutRecovery => {
                    ActionOutcome::FanoutRecovered(BoardScore { score, incomplete })
                }
                PassDecision::LastMileRipup => ActionOutcome::LastMile {
                    removed: 0,
                    board: BoardScore { score, incomplete },
                },
            };
            decision = ctl.after_action(&outcome);
        }
    }
    panic!("no stop in 200 passes");
}

fn java(script: &Script) -> Vec<(u32, PassDecision)> {
    let mut ctl = FreeroutingPassController::new(script.max_passes, false, true);
    simulate(&mut ctl, script)
}

fn quantum(rule: StopRule, script: &Script) -> (Vec<(u32, PassDecision)>, Vec<Check>) {
    let inner = FreeroutingPassController::new(script.max_passes, false, true);
    let mut ctl = QuantumStop::new(inner, rule);
    let log = simulate(&mut ctl, script);
    (log, ctl.checks().to_vec())
}

fn last(log: &[(u32, PassDecision)]) -> (u32, PassDecision) {
    *log.last().expect("a decision")
}

fn flat(_: u32) -> (f32, usize) {
    (100.0, 5)
}

fn rising(pass: u32) -> (f32, usize) {
    (10.0 * pass as f32, 5)
}

/// 1000, then +9 at every multiple of 4: at the checks from pass 12 the
/// gain is 9, just below the default quantum (1 % of about 1030, 10.27 to
/// 10.36).
fn plus_9_per_check(pass: u32) -> (f32, usize) {
    (1000.0 + 9.0 * (pass / 4) as f32, 5)
}

/// As [`plus_9_per_check`] with +11: above the quantum at every check.
fn plus_11_per_check(pass: u32) -> (f32, usize) {
    (1000.0 + 11.0 * (pass / 4) as f32, 5)
}

fn rule(grace: u32, checks: u32, quantum_abs: f32, quantum_pct: f32) -> StopRule {
    StopRule {
        grace,
        checks,
        quantum_abs,
        quantum_pct,
    }
}

const QUANTUM: PassDecision = PassDecision::Stop(StopReason::Quantum);

/// A table row: its name, the rule, the scores, and the stop of
/// `quantum-stop`, `None` meaning every decision equals Java's.
type Row = (&'static str, StopRule, Script, Option<(u32, PassDecision)>);

/// One setting made invalid, and the key the refusal names.
type BadValue = (&'static str, fn(&mut RouterSettings));

#[test]
fn the_rule_over_synthetic_score_sequences() {
    let default = StopRule::default();
    let rows: [Row; 9] = [
        // A flat score: no gain at pass 8 (against pass 1) nor at pass 12.
        ("flat", default, Script::new(flat), Some((12, QUANTUM))),
        // A gain above the quantum at every check never stops early.
        ("rising", default, Script::new(rising), None),
        (
            "+11 per check",
            default,
            Script::new(plus_11_per_check),
            None,
        ),
        // A rise just below the quantum counts as no gain: pass 8 gains 18
        // over pass 1, passes 12 and 16 gain 9 each.
        (
            "+9 per check",
            default,
            Script::new(plus_9_per_check),
            Some((16, QUANTUM)),
        ),
        // A gain equal to the quantum counts; just above it does not.
        (
            "abs 9, gain 9",
            rule(0, 2, 9.0, 0.0),
            Script::new(plus_9_per_check),
            None,
        ),
        (
            "abs 9.001, gain 9",
            rule(0, 2, 9.001, 0.0),
            Script::new(plus_9_per_check),
            Some((16, QUANTUM)),
        ),
        // Grace: the check at pass 8 does not count.
        (
            "flat, grace 1",
            rule(1, 2, 1.0, 0.01),
            Script::new(flat),
            Some((16, QUANTUM)),
        ),
        (
            "flat, 1 check",
            rule(0, 1, 1.0, 0.01),
            Script::new(flat),
            Some((8, QUANTUM)),
        ),
        // An inner stop wins: nothing left to route at pass 12, where the
        // rule fires too.
        (
            "done at 12",
            default,
            Script {
                done_at: Some(12),
                ..Script::new(flat)
            },
            Some((12, PassDecision::Stop(StopReason::Completed))),
        ),
    ];
    for (name, rule, script, want) in rows {
        let java_log = java(&script);
        let (log, _) = quantum(rule, &script);
        match want {
            None => assert_eq!(log, java_log, "{name}: every decision is Java's"),
            Some(stop) => {
                assert_eq!(last(&log), stop, "{name}");
                // Up to the stop, every decision is Java's.
                let before = log.len() - 1;
                assert_eq!(log[..before], java_log[..before], "{name}");
            }
        }
    }
}

/// The Java controller alone stops a flat score at pass 18; with the
/// defaults the rule stops it at the second check, pass 12, in place of
/// Java's last-mile ripup there. The checks record the gains.
#[test]
fn a_flat_score_stops_at_the_second_check_not_at_pass_18() {
    let script = Script::new(flat);
    assert_eq!(
        last(&java(&script)),
        (18, PassDecision::Stop(StopReason::Stagnation))
    );
    let java_at_12: Vec<PassDecision> = java(&script)
        .into_iter()
        .filter(|(p, _)| *p == 12)
        .map(|(_, d)| d)
        .collect();
    assert_eq!(java_at_12[0], PassDecision::LastMileRipup);
    let (log, checks) = quantum(StopRule::default(), &script);
    assert_eq!(last(&log), (12, QUANTUM));
    let check = |pass, misses| Check {
        pass,
        best: 100.0,
        gain: 0.0,
        quantum: 1.0,
        counted: true,
        misses,
    };
    assert_eq!(checks, [check(8, 1), check(12, 2)]);
}

/// A restore decided at a firing check becomes the stop; one decided at a
/// check that does not fire is applied as Java's.
#[test]
fn a_restore_at_the_firing_check_becomes_the_stop() {
    // Pass 1 is the best, every later pass worse: Java restores at 8 and
    // 12; the rule misses at 8 (the restore goes ahead) and fires at 12.
    let script = Script::new(|p| if p == 1 { (100.0, 5) } else { (50.0, 9) });
    let java_log = java(&script);
    let restore = PassDecision::Restore {
        max_restore_count: 3,
    };
    assert!(java_log.contains(&(8, restore)) && java_log.contains(&(12, restore)));
    let (log, _) = quantum(StopRule::default(), &script);
    assert!(log.contains(&(8, restore)));
    assert_eq!(last(&log), (12, QUANTUM));
}

/// The rule's state survives nothing but counters: the inner controller's
/// view (its global best) is Java's.
#[test]
fn the_inner_controller_sees_every_pass() {
    let script = Script::new(rising);
    let inner = FreeroutingPassController::new(script.max_passes, false, true);
    let mut ctl = QuantumStop::new(inner, StopRule::default());
    simulate(&mut ctl, &script);
    let mut java_ctl = FreeroutingPassController::new(script.max_passes, false, true);
    simulate(&mut java_ctl, &script);
    assert_eq!(ctl.inner().global_best(), java_ctl.global_best());
    assert_eq!(ctl.id(), CONTROLLER_ID);
}

#[test]
fn the_rule_reads_its_settings_and_refuses_bad_values() {
    let base = prepare(None).settings;
    let mut settings = base.clone();
    assert_eq!(StopRule::from_settings(&settings), Ok(StopRule::default()));
    assert_eq!(
        StopRule::default(),
        rule(0, 2, 1.0, 0.01),
        "the preregistered defaults (Q04.json)"
    );
    settings.quantum_stop.grace = Some(1);
    settings.quantum_stop.checks = Some(3);
    settings.quantum_stop.quantum_abs = Some(2.5);
    settings.quantum_stop.quantum_pct = Some(0.02);
    assert_eq!(
        StopRule::from_settings(&settings),
        Ok(rule(1, 3, 2.5, 0.02))
    );
    let bad_values: [BadValue; 4] = [
        ("router.quantum_stop.checks", |s: &mut RouterSettings| {
            s.quantum_stop.checks = Some(0)
        }),
        ("router.quantum_stop.grace", |s: &mut RouterSettings| {
            s.quantum_stop.grace = Some(-1)
        }),
        (
            "router.quantum_stop.quantum_abs",
            |s: &mut RouterSettings| s.quantum_stop.quantum_abs = Some(-1.0),
        ),
        (
            "router.quantum_stop.quantum_pct",
            |s: &mut RouterSettings| s.quantum_stop.quantum_pct = Some(f64::NAN),
        ),
    ];
    for (key, bad) in bad_values {
        let mut s = base.clone();
        bad(&mut s);
        let error = check_settings(&s).expect_err(key);
        assert_eq!(error.key, key);
    }
}

#[test]
fn quantum_stop_is_registered_and_selected_by_the_pass_controller_key() {
    let registry = StageRegistry::builtin();
    assert!(registry.pass_controller.contains_key(CONTROLLER_ID));
    let settings = prepare(None).settings;
    let built = registry.pass_controller[CONTROLLER_ID](&settings);
    assert_eq!(built.id(), "quantum-stop");
}

// ---------------------------------------------------------------------------
// Q04.2: the batch loop, the default controller against quantum-stop
// ---------------------------------------------------------------------------

const TWO_PIN: &str = "fixtures/scoring-perfect-two-pin.dsn";

fn prepare(controller: Option<&str>) -> PreparedJob {
    let path = repo_root().join(TWO_PIN);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{TWO_PIN}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_fanout_enabled(false);
    profile.set_max_passes(60);
    profile.patch.autorouter.pass_controller = controller.map(str::to_string);
    let mut input = JobInput::new(bytes, JobInput::design_name_of(TWO_PIN));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{TWO_PIN}: {e}"))
}

/// A pass runner that adds a trace each pass (so every pass leaves a new
/// board and session) and scores the board it leaves with `script(pass)`.
struct ScriptedRunner {
    script: fn(u32) -> f32,
    scores: Rc<RefCell<BTreeMap<u128, f32>>>,
}

impl PassExecutor for ScriptedRunner {
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
        // A bend that moves with the pass, so every pass's board has its
        // own session.
        let (a, b) = (pins[0].0, pins[1].0);
        let bend = freerouting::geometry::IntPoint::new((a.x + b.x) / 2, a.y + 100 * pass as i32);
        let polyline = Polyline::from_points(&[Point::Int(a), Point::Int(bend), Point::Int(b)]);
        board.insert_trace_without_cleaning(polyline, 0, 100, &[pins[0].1], 0, FixedState::Unfixed);
        self.scores
            .borrow_mut()
            .insert(board.state_hash(), (self.script)(pass));
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

/// One arm: the outcome and the session of the board it saves.
struct Arm {
    outcome: PipelineOutcome,
    ses: String,
}

fn run_arm(controller: Option<&str>, script: fn(u32) -> f32) -> Arm {
    let prepared = prepare(controller);
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let scores = Rc::new(RefCell::new(BTreeMap::new()));
    let mut factory = |_: &RouterSettings, _: &_| {
        Box::new(ScriptedRunner {
            script,
            scores: Rc::clone(&scores),
        }) as Box<dyn PassExecutor>
    };
    let mut outcome = run_pipeline_with(prepared.board.clone(), &job, &mut factory)
        .unwrap_or_else(|f| panic!("{}", f.error));
    let board = outcome.board.take().expect("board");
    Arm {
        outcome,
        ses: write_ses(&board, "two-pin", SesStyle::Java),
    }
}

/// The pass of the last score improvement: the last pass whose logged
/// score beat every earlier one.
fn last_improvement(outcome: &PipelineOutcome) -> u32 {
    let mut best = f32::NEG_INFINITY;
    let mut at = 0;
    for record in &outcome.passes {
        if record.score > best {
            best = record.score;
            at = record.pass;
        }
    }
    at
}

fn stop_pass(arm: &Arm) -> u32 {
    arm.outcome.passes.last().map_or(0, |r| r.pass)
}

/// Pass 1 is the best and every later pass is worse (canary-pwr's shape):
/// Java restores the pass-1 board at the checks and stops at pass 18; the
/// rule stops at pass 12, after the last improvement (pass 1), and the
/// final best-board restore leaves the same board: the sessions are equal.
#[test]
fn a_stop_after_the_last_improvement_saves_the_same_session() {
    let script: fn(u32) -> f32 = |p| if p == 1 { 100.0 } else { 50.0 + (p % 3) as f32 };
    let default = run_arm(None, script);
    let stop = run_arm(Some(CONTROLLER_ID), script);
    assert_eq!(last_improvement(&default.outcome), 1);
    assert!(matches!(
        default.outcome.stop,
        Some(StopReason::Stagnation | StopReason::GlobalStagnation)
    ));
    assert_eq!(stop_pass(&default), 18);
    assert_eq!(stop.outcome.stop, Some(StopReason::Quantum));
    assert_eq!(stop_pass(&stop), 12);
    assert!(stop.outcome.restored_best && default.outcome.restored_best);
    assert_eq!(stop.ses, default.ses, "the same saved board");
    assert!(
        stop.outcome
            .log
            .iter()
            .any(|l| l.text.ends_with("Stopping the auto-router (quantum-stop).")),
        "the stop is logged"
    );
}

/// A late improvement (pass 14, after the rule's stop at 12): the default
/// run keeps it and the sessions differ. Such boards go to Q04.4's
/// measurement.
#[test]
fn a_stop_before_the_last_improvement_changes_the_result() {
    let script: fn(u32) -> f32 = |p| match p {
        1 => 100.0,
        14 => 200.0,
        _ => 50.0,
    };
    let default = run_arm(None, script);
    let stop = run_arm(Some(CONTROLLER_ID), script);
    assert_eq!(last_improvement(&default.outcome), 14);
    assert_eq!(stop_pass(&stop), 12);
    assert_ne!(stop.ses, default.ses);
}

/// Without the selection the default run is Java's controller.
#[test]
fn the_default_pass_controller_is_java_s() {
    let prepared = prepare(None);
    let registry = StageRegistry::builtin();
    let selection =
        freerouting::autoroute::stages::StageSelection::resolve(&prepared.settings, &registry);
    let pipeline = registry
        .build(&selection, &prepared.settings)
        .expect("builds");
    assert_eq!(pipeline.pass_controller.id(), "freerouting-passes");
}

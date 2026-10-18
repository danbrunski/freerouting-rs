//! Q10: the add-only completion sweep (`quality::sweep`,
//! `router.autorouter.sweep`).
//!
//! The fixture is Q06's `q06-sole-wall.dsn` (`tests/data/quality/`): one
//! layer; the only route from A1 to A2 crosses the routed wire of net B,
//! whose pads sit 0.2 mm from the outline. Pass 1 routes A by ripping B's
//! wire, and B is then incomplete but routable round A's new trace (Q06's
//! `found` verdict). With `-mp 1` nothing routes B again; the sweep does.

mod autoroute_common;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, PipelineOutcome, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::structure::FixedState;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const SOLE_WALL: &str = "crates/freerouting/tests/data/quality/q06-sole-wall.dsn";

/// The sweep settings of a run.
#[derive(Clone, Copy, Default)]
struct Sweep {
    mode: Option<&'static str>,
    passes: Option<i32>,
    after_optimizer: Option<bool>,
}

const FREE: Sweep = Sweep {
    mode: Some("free"),
    passes: None,
    after_optimizer: None,
};

const FIXED: Sweep = Sweep {
    mode: Some("fixed"),
    passes: None,
    after_optimizer: None,
};

fn prepare(dsn: &str, max_passes: i32, optimizer: bool, sweep: Sweep) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(optimizer);
    profile.set_max_passes(max_passes);
    profile.patch.autorouter.sweep = sweep.mode.map(str::to_string);
    profile.patch.autorouter.sweep_passes = sweep.passes;
    profile.patch.autorouter.sweep_after_optimizer = sweep.after_optimizer;
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn run(prepared: PreparedJob) -> Result<PipelineOutcome, String> {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    run_pipeline(prepared.board, &job).map_err(|f| f.error.to_string())
}

/// The routed board and the job log's text.
fn route(dsn: &str, max_passes: i32, optimizer: bool, sweep: Sweep) -> (RoutingBoard, Vec<String>) {
    let mut outcome = run(prepare(dsn, max_passes, optimizer, sweep)).expect("the job runs");
    let board = outcome.board.take().expect("no listener took the board");
    (board, outcome.log.iter().map(|l| l.text.clone()).collect())
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

fn ses(board: &RoutingBoard) -> String {
    write_ses(board, "q06-sole-wall", SesStyle::Java)
}

fn sweep_lines(log: &[String]) -> Vec<&String> {
    log.iter()
        .filter(|l| l.starts_with("Add-only sweep"))
        .collect()
}

/// The traces and vias of `board` with their geometry and fixed state, as
/// text, sorted.
fn wiring(board: &RoutingBoard) -> BTreeSet<String> {
    board
        .items()
        .filter(|item| item.is_trace() || item.is_via())
        .map(|item| format!("{:?} {:?}", item.kind, item.fixed_state()))
        .collect()
}

fn fixed_wiring(board: &RoutingBoard) -> usize {
    board
        .items()
        .filter(|item| (item.is_trace() || item.is_via()) && item.is_user_fixed())
        .count()
}

#[test]
fn sole_wall_ends_complete_with_the_sweep_and_incomplete_without() {
    let (off, off_log) = route(SOLE_WALL, 1, false, Sweep::default());
    assert_eq!(
        incomplete(&off),
        1,
        "pass 1 rips B and never routes it again"
    );
    assert!(sweep_lines(&off_log).is_empty());
    for sweep in [FREE, FIXED] {
        let mode = sweep.mode.unwrap_or_default();
        let (on, log) = route(SOLE_WALL, 1, false, sweep);
        assert_eq!(incomplete(&on), 0, "{mode}: the sweep routes B round A");
        let lines = sweep_lines(&log);
        assert_eq!(lines.len(), 2, "{mode}: {lines:?}");
        assert!(
            lines[0].starts_with(&format!(
                "Add-only sweep pass #1 ({mode}): routed 1, failed 0; incomplete 1 -> 0"
            )),
            "{}",
            lines[0]
        );
        assert!(lines[1].contains("completed the board"), "{}", lines[1]);
        // Add-only: every trace and via of the pass-1 board is still there.
        let before = wiring(&off);
        let after = wiring(&on);
        assert!(
            before.is_subset(&after),
            "{mode}: the sweep removed or moved wiring"
        );
        assert!(after.len() > before.len());
    }
}

#[test]
fn the_fixed_arm_gives_the_wiring_its_state_back() {
    let input = prepare(SOLE_WALL, 1, false, Sweep::default());
    let fixed_in_input = fixed_wiring(&input.board);
    let (free, _) = route(SOLE_WALL, 1, false, FREE);
    let (fixed, log) = route(SOLE_WALL, 1, false, FIXED);
    assert_eq!(fixed_wiring(&fixed), fixed_in_input);
    // Both arms complete the board, by different routes: frozen wiring is
    // an obstacle the shover may not move, so the `fixed` arm goes round
    // the other side.
    assert_eq!(incomplete(&fixed), 0);
    assert_eq!(incomplete(&free), 0);
    assert_ne!(wiring(&fixed), wiring(&free));
    let last = sweep_lines(&log).pop().expect("a sweep line").clone();
    assert!(
        last.contains("item(s) frozen, 0 split piece(s) unfixed"),
        "{last}"
    );
    assert!(!last.contains("; 0 item(s) frozen"), "{last}");
}

#[test]
fn off_is_the_unchanged_engine() {
    let (unset, _) = route(SOLE_WALL, 1, false, Sweep::default());
    let reference = ses(&unset);
    for sweep in [
        Sweep {
            mode: Some("off"),
            passes: Some(5),
            after_optimizer: Some(true),
        },
        Sweep {
            mode: Some("free"),
            passes: Some(0),
            after_optimizer: None,
        },
    ] {
        let (board, log) = route(SOLE_WALL, 1, false, sweep);
        assert_eq!(ses(&board), reference);
        assert!(sweep_lines(&log).is_empty());
    }
    // With the optimizer on as well.
    let (unset, _) = route(SOLE_WALL, 1, true, Sweep::default());
    let off = Sweep {
        mode: Some("off"),
        passes: None,
        after_optimizer: Some(true),
    };
    let (board, _) = route(SOLE_WALL, 1, true, off);
    assert_eq!(ses(&board), ses(&unset));
}

#[test]
fn after_the_optimizer_the_sweep_runs_between_two_optimizer_runs() {
    let after = Sweep {
        mode: Some("free"),
        passes: None,
        after_optimizer: Some(true),
    };
    let (board, log) = route(SOLE_WALL, 1, true, after);
    assert_eq!(incomplete(&board), 0);
    let sweep_at = log
        .iter()
        .position(|l| l.starts_with("Add-only sweep pass #1"))
        .expect("the sweep ran");
    let optimizer_lines: Vec<usize> = log
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("ptimiz"))
        .map(|(i, _)| i)
        .collect();
    assert!(
        optimizer_lines.iter().any(|&i| i < sweep_at)
            && optimizer_lines.iter().any(|&i| i > sweep_at),
        "optimizer lines {optimizer_lines:?}, sweep at {sweep_at}: {log:#?}"
    );
    // With the optimizer off, `sweep_after_optimizer` sweeps after the loop.
    let (no_opt, _) = route(SOLE_WALL, 1, false, after);
    let (loop_sweep, _) = route(SOLE_WALL, 1, false, FREE);
    assert_eq!(ses(&no_opt), ses(&loop_sweep));
}

#[test]
fn the_sweep_is_deterministic() {
    for sweep in [FREE, FIXED] {
        let (a, log_a) = route(SOLE_WALL, 1, true, sweep);
        let (b, log_b) = route(SOLE_WALL, 1, true, sweep);
        assert_eq!(ses(&a), ses(&b));
        assert_eq!(log_a, log_b);
    }
}

#[test]
fn unknown_sweep_settings_are_refused_before_routing() {
    let bad = Sweep {
        mode: Some("on"),
        passes: None,
        after_optimizer: None,
    };
    let error = run(prepare(SOLE_WALL, 1, false, bad)).expect_err("refused");
    assert!(
        error.contains("router.autorouter.sweep") && error.contains("'on'"),
        "{error}"
    );
    let negative = Sweep {
        mode: Some("free"),
        passes: Some(-1),
        after_optimizer: None,
    };
    let error = run(prepare(SOLE_WALL, 1, false, negative)).expect_err("refused");
    assert!(error.contains("router.autorouter.sweep_passes"), "{error}");
}

#[test]
fn the_sweep_keeps_fixed_states_of_the_input() {
    // The input's own fixed wiring stays fixed through a `fixed` sweep.
    let input = prepare(SOLE_WALL, 1, false, Sweep::default());
    let states: Vec<FixedState> = input
        .board
        .items()
        .filter(|item| item.is_trace() || item.is_via())
        .map(|item| item.fixed_state())
        .collect();
    let (fixed, _) = route(SOLE_WALL, 1, false, FIXED);
    let user_fixed = states
        .iter()
        .filter(|s| **s >= FixedState::UserFixed)
        .count();
    assert_eq!(fixed_wiring(&fixed), user_fixed);
}

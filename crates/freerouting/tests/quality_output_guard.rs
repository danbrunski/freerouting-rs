//! Q04.3: the two-axis output guard (`quality::output_guard`,
//! `router.autorouter.output_guard`).
//!
//! The fixture `tests/data/quality/q04-prerouted.dsn` has one layer, nets B
//! and C routed as walls across the board (their pads 0.2 mm from the
//! outline), and net A with A1-A2 routed and A3, A4 and A5 beyond the
//! walls. Joining A's pins means crossing both walls; with `-mp 1` the
//! router rips B and C and nothing routes them again: the routed board
//! loses 2 connections and gains 3, and its unconnected pins go from 3 (A3,
//! A4, A5) to 4 (B1, B2, C1, C2).

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::batch::batch_loop::PassExecutor;
use freerouting::autoroute::batch::pass::{PassOutcome, PassStages};
use freerouting::autoroute::batch::snapshot::{ScoredBoard, SnapshotHistory};
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{
    PipelineJob, PipelineOutcome, run_pipeline, run_pipeline_with,
};
use freerouting::autoroute::stages::StageCtx;
use freerouting::board::RoutingBoard;
use freerouting::board::items::{ItemId, StopConnectionOption};
use freerouting::board::structure::FixedState;
use freerouting::geometry::{Point, Polyline};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::output_guard::{Axes, GuardChoice, OutputGuard, PinGroups};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, RouterSettings, Source};

const PREROUTED: &str = "crates/freerouting/tests/data/quality/q04-prerouted.dsn";

fn prepare(dsn: &str, max_passes: i32, guard: Option<bool>) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_fanout_enabled(false);
    profile.set_max_passes(max_passes);
    profile.patch.autorouter.output_guard = guard;
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn job(prepared: &PreparedJob) -> PipelineJob<'_> {
    PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    }
}

/// Routes `prepared` with the real router.
fn route(prepared: PreparedJob) -> PipelineOutcome {
    let board = prepared.board.clone();
    run_pipeline(board, &job(&prepared)).unwrap_or_else(|f| panic!("{}", f.error))
}

fn log_text(outcome: &PipelineOutcome) -> Vec<String> {
    outcome.log.iter().map(|l| l.text.clone()).collect()
}

fn axes(input: &RoutingBoard, board: &RoutingBoard) -> Axes {
    Axes::between(&PinGroups::of(input), &PinGroups::of(board))
}

/// The pin of component `name`.
fn pin(board: &RoutingBoard, name: &str) -> (ItemId, Point, i32) {
    let component = board
        .components
        .get_by_name(name)
        .unwrap_or_else(|| panic!("no component {name}"))
        .id
        .0;
    let item = board
        .items()
        .find(|i| i.is_pin() && i.component_id() == component)
        .unwrap_or_else(|| panic!("no pin of {name}"));
    (
        item.id(),
        Point::Int(item.drill().expect("drill").center),
        item.net_numbers()[0],
    )
}

/// A straight trace from the pin of `from` to the pin of `to`.
fn join(board: &mut RoutingBoard, from: &str, to: &str) {
    let (_, a, net) = pin(board, from);
    let (_, b, _) = pin(board, to);
    board.insert_trace_without_cleaning(
        Polyline::from_points(&[a, b]),
        0,
        125,
        &[net],
        0,
        FixedState::Unfixed,
    );
}

/// Removes the traces of net `net`.
fn rip(board: &mut RoutingBoard, net: i32) {
    let ids: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_trace() && i.contains_net(net))
        .map(|i| i.id())
        .collect();
    board.remove_items(ids);
}

fn net_of(board: &RoutingBoard, component: &str) -> i32 {
    pin(board, component).2
}

#[test]
fn the_input_has_one_connection_of_a_and_the_two_walls() {
    let prepared = prepare(PREROUTED, 1, None);
    let groups = PinGroups::of(&prepared.board);
    // A: A1-A2 (1); B and C: 1 each.
    assert_eq!(groups.completed(), 3);
    assert_eq!(groups.unconnected_pins(), 3, "A3, A4 and A5");
}

/// The scripted run: the edit a router makes that rips B's and C's walls
/// and joins A3, A4 and A5, by hand, so the axes are known exactly.
fn rip_walls_and_join_a(board: &mut RoutingBoard) {
    let b = net_of(board, "B1");
    let c = net_of(board, "C1");
    rip(board, b);
    rip(board, c);
    join(board, "A2", "A3");
    join(board, "A3", "A4");
    join(board, "A4", "A5");
}

#[test]
fn the_axes_count_lost_and_gained_connections_and_unconnected_pins() {
    let input = prepare(PREROUTED, 1, None).board;
    let mut routed = input.clone();
    rip_walls_and_join_a(&mut routed);
    let routed_axes = axes(&input, &routed);
    assert_eq!(
        routed_axes,
        Axes {
            lost: 2,
            gained: 3,
            unconnected_input: 3,
            unconnected: 4,
        }
    );
    assert!(
        routed_axes.worse(),
        "more unconnected pins: worse on that axis"
    );
    assert_eq!(
        routed_axes.text(),
        "completed connections lost 2, gained 3; unconnected pins 3 -> 4"
    );
    // Neither axis outvotes the other: fewer connections is worse too.
    let mut lost_only = input.clone();
    rip(&mut lost_only, net_of(&input, "B1"));
    join(&mut lost_only, "A2", "A3");
    join(&mut lost_only, "A3", "A4");
    join(&mut lost_only, "A4", "A5");
    let lost_one = axes(&input, &lost_only);
    assert_eq!((lost_one.lost, lost_one.gained), (1, 3));
    assert_eq!((lost_one.unconnected_input, lost_one.unconnected), (3, 2));
    assert!(!lost_one.worse());
    let mut fewer = input.clone();
    rip(&mut fewer, net_of(&input, "A1"));
    let fewer = axes(&input, &fewer);
    assert_eq!((fewer.lost, fewer.gained), (1, 0));
    assert!(fewer.worse());
    // The input against itself.
    assert!(!axes(&input, &input).worse());
}

/// With an empty history the guard falls back to the input board.
#[test]
fn with_no_history_board_the_guard_saves_the_input() {
    let input = prepare(PREROUTED, 1, None).board;
    let guard = OutputGuard::new(&input);
    let mut routed = input.clone();
    rip_walls_and_join_a(&mut routed);
    let (outcome, saved) = guard.check(&routed, &SnapshotHistory::new());
    assert_eq!(outcome.choice, GuardChoice::Input);
    assert!(outcome.fired());
    let saved = saved.expect("a replacement");
    assert_eq!(saved.state_hash(), input.state_hash());
    assert!(!outcome.saved.worse());
    assert_eq!(outcome.log.len(), 2);
    assert!(outcome.log[0].contains("lost 2, gained 3; unconnected pins 3 -> 4"));
    assert!(outcome.log[1].contains("saving the input board"));
}

/// A routed board worse on neither axis is kept, with no replacement.
#[test]
fn a_board_worse_on_neither_axis_is_kept() {
    let input = prepare(PREROUTED, 1, None).board;
    let guard = OutputGuard::new(&input);
    let mut routed = input.clone();
    join(&mut routed, "A2", "A3");
    let (outcome, saved) = guard.check(&routed, &SnapshotHistory::new());
    assert_eq!(outcome.choice, GuardChoice::Routed);
    assert!(saved.is_none());
    assert_eq!(outcome.routed, outcome.saved);
    assert_eq!((outcome.routed.lost, outcome.routed.gained), (0, 1));
}

/// The guard takes the best history board that is worse on neither axis,
/// skipping better-scored ones that are.
#[test]
fn the_guard_takes_the_best_history_board_worse_on_neither_axis() {
    let input = prepare(PREROUTED, 1, None).board;
    let guard = OutputGuard::new(&input);
    let mut routed = input.clone();
    rip_walls_and_join_a(&mut routed);
    // History, best first: the routed board itself (skipped), a board
    // with only C ripped (worse), one with A3 joined (not worse), the input.
    let mut c_ripped = input.clone();
    rip(&mut c_ripped, net_of(&input, "C1"));
    let mut a3_joined = input.clone();
    join(&mut a3_joined, "A2", "A3");
    let mut history = SnapshotHistory::new();
    for (board, score) in [
        (&input, 1.0),
        (&a3_joined, 2.0),
        (&c_ripped, 3.0),
        (&routed, 4.0),
    ] {
        history.add(&ScoredBoard {
            board,
            hash: board.state_hash(),
            score,
        });
    }
    let (outcome, saved) = guard.check(&routed, &history);
    assert_eq!(
        outcome.choice,
        GuardChoice::History {
            rank: 3,
            score: 2.0
        }
    );
    assert_eq!(
        saved.expect("a replacement").state_hash(),
        a3_joined.state_hash()
    );
    assert_eq!((outcome.saved.lost, outcome.saved.gained), (0, 1));
    // The history is unchanged (no restore count moved, order kept).
    assert_eq!(history.scores(), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(history.restore_counts(), [0, 0, 0, 0]);
}

// ---------------------------------------------------------------------------
// In the batch loop
// ---------------------------------------------------------------------------

/// A pass runner whose pass 1 is [`rip_walls_and_join_a`], with the
/// boards' real figures.
struct ScriptedRun;

impl PassExecutor for ScriptedRun {
    fn run_pass(
        &mut self,
        board: &mut RoutingBoard,
        pass: u32,
        _: &mut PassStages<'_>,
        _: &mut StageCtx,
    ) -> PassOutcome {
        if pass == 1 {
            rip_walls_and_join_a(board);
        }
        PassOutcome {
            continue_routing: true,
            routed: 3,
            ..PassOutcome::default()
        }
    }

    fn remove_tails(&mut self, _: &mut RoutingBoard, _: StopConnectionOption) -> bool {
        false
    }
}

fn scripted(guard: Option<bool>) -> (PreparedJob, PipelineOutcome) {
    let prepared = prepare(PREROUTED, 1, guard);
    let mut factory = |_: &RouterSettings, _: &_| Box::new(ScriptedRun) as Box<dyn PassExecutor>;
    let outcome = run_pipeline_with(prepared.board.clone(), &job(&prepared), &mut factory)
        .unwrap_or_else(|f| panic!("{}", f.error));
    (prepared, outcome)
}

/// The scripted run scores better than its input (2 incomplete
/// connections against 3), so Java's finish keeps it; the guard sees more
/// unconnected pins and saves the history's board before pass 1, which is
/// the input, and logs both axes.
#[test]
fn in_the_loop_the_guard_saves_a_board_not_worse_than_the_input() {
    let (prepared, mut off) = scripted(None);
    let off_board = off.board.take().expect("board");
    assert!(off.output_guard.is_none());
    assert!(!off.restored_best, "Java keeps the better-scored board");
    assert!(axes(&prepared.board, &off_board).worse());

    let (prepared, mut on) = scripted(Some(true));
    let on_board = on.board.take().expect("board");
    let guard = on.output_guard.clone().expect("the guard ran");
    assert_eq!(
        guard.choice,
        GuardChoice::History {
            rank: 1,
            score: guard_score(&guard)
        }
    );
    assert_eq!(
        guard.routed,
        Axes {
            lost: 2,
            gained: 3,
            unconnected_input: 3,
            unconnected: 4
        }
    );
    let saved = axes(&prepared.board, &on_board);
    assert!(!saved.worse(), "{saved:?}");
    assert_eq!(on_board.state_hash(), prepared.board.state_hash());
    assert_eq!(on.summary.incomplete, 3, "the summary is the saved board's");
    let log = log_text(&on);
    assert!(
        log.iter().any(|l| l
            == "Output guard: the routed board against the input: completed connections lost 2, \
                gained 3; unconnected pins 3 -> 4."),
        "{log:#?}"
    );
    assert!(
        log.iter()
            .any(|l| l.starts_with("Output guard: the routed board is worse"))
    );
}

fn guard_score(guard: &freerouting::quality::output_guard::GuardOutcome) -> f32 {
    match guard.choice {
        GuardChoice::History { score, .. } => score,
        _ => f32::NAN,
    }
}

/// The real router, `-mp 1`: pass 1 rips B's and C's walls to join A3, A4
/// and A5 and nothing routes them again. The routed board scores better
/// than the input (2 incomplete connections against 3), so Java's finish
/// keeps it; the guard logs both axes and saves the history's board before
/// pass 1, the input.
#[test]
fn the_batch_run_rips_the_walls_and_the_guard_saves_the_input_wiring() {
    let mut off = route(prepare(PREROUTED, 1, None));
    let off_board = off.board.take().expect("board");
    let prepared = prepare(PREROUTED, 1, Some(true));
    let input = prepared.board.clone();
    assert_eq!(
        axes(&input, &off_board),
        Axes {
            lost: 2,
            gained: 3,
            unconnected_input: 3,
            unconnected: 4
        },
        "without the guard"
    );
    assert!(!off.restored_best);
    let mut on = route(prepared);
    let board = on.board.take().expect("board");
    let guard = on.output_guard.clone().expect("the guard ran");
    assert_eq!(guard.routed, axes(&input, &off_board));
    assert!(
        matches!(guard.choice, GuardChoice::History { rank: 1, .. }),
        "{guard:#?}"
    );
    assert!(!axes(&input, &board).worse());
    assert_eq!(
        board.state_hash(),
        input.state_hash(),
        "the board before pass 1"
    );
    let log = log_text(&on);
    assert!(
        log.iter().any(|l| l
            == "Output guard: the routed board against the input: completed connections lost 2, \
                gained 3; unconnected pins 3 -> 4."),
        "{log:#?}"
    );
}

/// With more passes the router reroutes the walls; the guard keeps the
/// routed board. Whatever the router does, the guarded run never ends
/// worse than the input on either axis.
#[test]
fn with_more_passes_the_guarded_run_is_never_worse_than_the_input() {
    for max_passes in [2, 3, 5] {
        let prepared = prepare(PREROUTED, max_passes, Some(true));
        let input = prepared.board.clone();
        let mut outcome = route(prepared);
        let board = outcome.board.take().expect("board");
        let guard = outcome.output_guard.clone().expect("the guard ran");
        let saved = axes(&input, &board);
        assert!(!saved.worse(), "-mp {max_passes}: {saved:?}\n{guard:#?}");
    }
}

/// On a board with no input wiring the guard never fires, and the session
/// is the unguarded one.
#[test]
fn without_input_wiring_the_guard_never_fires() {
    for dsn in [
        "fixtures/scoring-perfect-two-pin.dsn",
        "fixtures/Issue433-my-board.dsn",
    ] {
        let mut off = route(prepare(dsn, 3, None));
        let mut on = route(prepare(dsn, 3, Some(true)));
        let guard = on.output_guard.clone().expect("the guard ran");
        assert!(!guard.fired(), "{dsn}: {guard:#?}");
        let off_board = off.board.take().expect("board");
        let on_board = on.board.take().expect("board");
        assert_eq!(
            write_ses(&off_board, "b", SesStyle::Java),
            write_ses(&on_board, "b", SesStyle::Java),
            "{dsn}"
        );
    }
}

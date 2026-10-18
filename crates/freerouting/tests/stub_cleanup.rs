//! Q16.2: `router.cleanup.remove_stubs`, the final stub cleanup
//! ([`freerouting::quality::stubs`]).
//!
//! The fixture `tests/data/quality/q16-stubs.dsn` is a synthetic three-layer
//! board (F.Cu, the power layer In1.Cu with a GND plane in its lower half,
//! B.Cu) whose wiring is the copper the router could leave, one net per case:
//! - `FAILED`: a failed connection's start, pin A1 to a via, a B.Cu trace to
//!   a second via and an F.Cu trace that ends in the open short of B1;
//! - `CHAIN`: three F.Cu traces from pin A2 (widths 250, 300 and 350, so
//!   nothing combines them) ending in the open; only the last is a tail
//!   before the cleanup;
//! - `DONE`: a completed connection A3 to B3 through two vias;
//! - `FIXED`: a `type fix` via with a `type protect` trace ending in the
//!   open, and an unfixed via alone;
//! - `ONE_LAYER`: a complete connection A5 to B5 on F.Cu whose two traces
//!   meet at a via they use on F.Cu only;
//! - `GND`: pins G1 and G2 each joined by an F.Cu trace to a via into the
//!   GND plane, and a lone via in the plane.
//!
//! The cleanup never removes the copper of the board as loaded
//! ([`input_copper`]). The tests of the rules call [`remove_stubs`] with an
//! empty keep set, so the fixture's wiring stands for copper the router
//! inserted; the pipeline tests show the input's copper kept.

mod autoroute_common;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, PipelineOutcome, run_pipeline};
use freerouting::autoroute::stages::StageRegistry;
use freerouting::autoroute::stages::catalog::catalog;
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::stubs::{StubCleanup, input_copper, is_removable_stub, remove_stubs};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::profile::legacy_pin;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{Profile, ResolveContext, Source};

const STUBS: &str = "crates/freerouting/tests/data/quality/q16-stubs.dsn";

/// The job's load sequence on the fixture with `legacy` pinned, the router,
/// the fanout and the optimizer off, and `extra` (`path=value` pairs).
fn prepare(extra: &[(&str, &str)]) -> PreparedJob {
    let path = repo_root().join(STUBS);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{STUBS}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_router_enabled(false);
    profile.set_fanout_enabled(false);
    profile.set_optimizer_enabled(false);
    for (key, value) in extra {
        profile
            .patch
            .set_path(key, value)
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    let mut input = JobInput::new(bytes, JobInput::design_name_of(STUBS));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![
        (Source::Testing, legacy_pin()),
        (Source::Testing, profile.patch),
    ];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{STUBS}: {e}"))
}

fn board() -> RoutingBoard {
    prepare(&[]).board
}

fn net_number(board: &BasicBoard, name: &str) -> i32 {
    (1..=board.rules.nets.max_net_number())
        .find(|n| board.rules.nets.get(*n).is_some_and(|net| net.name == name))
        .unwrap_or_else(|| panic!("no net {name}"))
}

/// The traces and vias of net `name`, by id.
fn copper(board: &BasicBoard, name: &str) -> BTreeSet<ItemId> {
    let net = net_number(board, name);
    board
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.net_numbers() == [net])
        .map(|i| i.id())
        .collect()
}

/// `(vias, traces)` of net `name`.
fn counts(board: &BasicBoard, name: &str) -> (usize, usize) {
    let ids = copper(board, name);
    let vias = ids
        .iter()
        .filter(|id| board.item(**id).is_some_and(|i| i.is_via()))
        .count();
    (vias, ids.len() - vias)
}

fn incomplete(board: &RoutingBoard) -> usize {
    BoardStatistics::new(board).connections.incomplete_count
}

fn cleaned() -> (RoutingBoard, StubCleanup) {
    let mut board = board();
    let summary = remove_stubs(&mut board, &BTreeSet::new());
    (board, summary)
}

#[test]
fn the_fixture_loads_with_its_stubs() {
    let board = board();
    assert_eq!(counts(&board, "FAILED"), (2, 3));
    assert_eq!(counts(&board, "CHAIN"), (0, 3));
    assert_eq!(counts(&board, "DONE"), (2, 3));
    assert_eq!(counts(&board, "FIXED"), (2, 1));
    assert_eq!(counts(&board, "ONE_LAYER"), (1, 2));
    assert_eq!(counts(&board, "GND"), (3, 2));
    // Before the cleanup only the open end of the chain is a stub.
    let chain: Vec<bool> = copper(&board, "CHAIN")
        .into_iter()
        .map(|id| is_removable_stub(&board, id, &BTreeSet::new()))
        .collect();
    assert_eq!(chain.iter().filter(|s| **s).count(), 1, "{chain:?}");
    // FAILED, CHAIN and FIXED (whose wiring joins neither pin) are
    // incomplete; every other net is complete.
    assert_eq!(incomplete(&board), 3);
}

#[test]
fn a_via_pair_on_a_failed_connection_is_removed() {
    let (board, _) = cleaned();
    assert_eq!(counts(&board, "FAILED"), (0, 0));
}

#[test]
fn a_three_stub_chain_goes_in_one_call() {
    let (board, summary) = cleaned();
    assert_eq!(counts(&board, "CHAIN"), (0, 0));
    // Each link is judged when its turn comes, so the links that became
    // stubs go in the same call; the last round removes nothing.
    assert!(summary.rounds >= 2, "{summary:?}");
    // A second call finds nothing left.
    let mut board = board;
    let again = remove_stubs(&mut board, &BTreeSet::new());
    assert_eq!((again.removed(), again.rounds), (0, 1), "{again:?}");
}

#[test]
fn a_completed_connection_is_untouched() {
    let before = copper(&board(), "DONE");
    let (board, _) = cleaned();
    assert_eq!(copper(&board, "DONE"), before);
}

#[test]
fn a_fixed_via_is_kept() {
    let (board, _) = cleaned();
    // The unfixed lone via goes; the fixed via and the protected trace stay.
    assert_eq!(counts(&board, "FIXED"), (1, 1));
    for id in copper(&board, "FIXED") {
        let item = board.item(id).expect("an item");
        assert!(item.fixed_state() >= FixedState::UserFixed, "{id:?}");
    }
}

#[test]
fn a_via_onto_its_own_plane_is_kept() {
    let before = board();
    let lone: Vec<ItemId> = copper(&before, "GND")
        .into_iter()
        .filter(|id| before.normal_contacts(*id).len() == 1)
        .filter(|id| before.item(*id).is_some_and(|i| i.is_via()))
        .collect();
    assert_eq!(lone.len(), 1, "the via whose only contact is the plane");
    let (board, _) = cleaned();
    // The two vias that join a trace to the plane stay with their traces;
    // the via that touches only the plane connects one item and goes.
    assert_eq!(counts(&board, "GND"), (2, 2));
    assert!(board.item(lone[0]).is_none());
}

#[test]
fn a_via_used_on_one_layer_goes_and_its_traces_stay_joined() {
    let (board, _) = cleaned();
    assert_eq!(counts(&board, "ONE_LAYER"), (0, 2));
}

#[test]
fn the_cleanup_counts_what_it_removed_and_completes_nothing_less() {
    let before = board();
    let incomplete_before = incomplete(&before);
    let (board, summary) = cleaned();
    // Vias: FAILED 2, FIXED 1, ONE_LAYER 1, GND 1; traces: FAILED 3, CHAIN 3.
    assert_eq!((summary.vias, summary.traces), (5, 6), "{summary:?}");
    assert_eq!(incomplete(&board), incomplete_before);
    // The same board gives the same result.
    let (again, again_summary) = cleaned();
    assert_eq!(again_summary, summary);
    let ids = |b: &RoutingBoard| b.items().map(|i| i.id()).collect::<Vec<_>>();
    assert_eq!(ids(&again), ids(&board));
}

fn run(prepared: PreparedJob) -> PipelineOutcome {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{STUBS}: {}", f.error))
}

#[test]
fn an_unfixed_input_via_pair_that_dangles_is_kept() {
    // The pipeline keeps every trace and via of the board as loaded: the
    // fixture's wiring is all input, so nothing goes, FAILED's unfixed
    // dangling via pair included.
    let prepared = prepare(&[("cleanup.remove_stubs", "true")]);
    let before: Vec<ItemId> = prepared.board.items().map(|i| i.id()).collect();
    let mut outcome = run(prepared);
    let summary = outcome.stubs.expect("the cleanup ran");
    assert_eq!(summary.removed(), 0, "{summary:?}");
    assert!(
        outcome
            .log
            .iter()
            .any(|l| l.text == summary.log_line() && l.text.contains("removed 0 dangling via")),
        "{:?}",
        outcome.log
    );
    let board = outcome.board.take().expect("the board");
    assert_eq!(counts(&board, "FAILED"), (2, 3));
    assert_eq!(board.items().map(|i| i.id()).collect::<Vec<_>>(), before);
}

#[test]
fn only_copper_outside_the_keep_set_goes() {
    // FAILED's copper stands for what the router inserted; the rest is the
    // input's, stubs included.
    let mut board = board();
    let failed = copper(&board, "FAILED");
    let keep: BTreeSet<ItemId> = input_copper(&board).difference(&failed).copied().collect();
    let summary = remove_stubs(&mut board, &keep);
    assert_eq!((summary.vias, summary.traces), (2, 3), "{summary:?}");
    assert_eq!(counts(&board, "FAILED"), (0, 0));
    assert_eq!(counts(&board, "CHAIN"), (0, 3));
    assert_eq!(counts(&board, "FIXED"), (2, 1));
    assert_eq!(counts(&board, "ONE_LAYER"), (1, 2));
    assert_eq!(counts(&board, "GND"), (3, 2));
}

#[test]
fn with_the_setting_unset_or_off_the_pipeline_removes_nothing() {
    for extra in [&[][..], &[("cleanup.remove_stubs", "false")][..]] {
        let prepared = prepare(extra);
        let before: Vec<ItemId> = prepared.board.items().map(|i| i.id()).collect();
        let mut outcome = run(prepared);
        assert_eq!(outcome.stubs, None);
        assert!(!outcome.log.iter().any(|l| l.text.contains("Stub cleanup")));
        let board = outcome.board.take().expect("the board");
        let after: Vec<ItemId> = board.items().map(|i| i.id()).collect();
        assert_eq!(after, before, "{extra:?}");
    }
}

#[test]
fn the_setting_is_listed_with_the_optimizer_and_unset_in_both_profiles() {
    let registry = StageRegistry::builtin();
    for profile in [Profile::Legacy, Profile::Enhanced] {
        let slots = catalog(&registry, profile, 8);
        let option = slots
            .iter()
            .filter(|s| s.desc.kind == "optimizer")
            .flat_map(|s| s.options.iter())
            .find(|o| o.path == "cleanup.remove_stubs")
            .expect("the option is listed");
        assert_eq!(option.default, None, "{profile:?}");
    }
}

//! P02.4: a snapshot restored again clones the board its second restore
//! built (`board::snapshot`, "Repeated restores"). These tests pin that
//! board equal to a fresh build in everything a restore resets or keeps
//! (B01.3): items and undo structure (`state_hash`), the default tree's
//! shape and entries, the work meter's count, the fresh transient state,
//! the incremental audit (P02.1), and what a route on it produces.

mod autoroute_common;
mod board_probe;

use std::sync::Arc;

use autoroute_common::load;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::search::audit::audit_board;
use freerouting::geometry::{Area, Shape, TileShape};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::settings::RouterSettings;
use freerouting::settings::board::BoardRouterSettings;

fn j2_with_session() -> RoutingBoard {
    let row = board_probe::fixture("Issue026-J2_reference");
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row).expect("J2 has a session");
    board
}

/// Everything of a restored board a test can see: the state hash, the
/// default tree's walk and content digest, the tree count, the meter, the
/// revision and the transient flags.
fn restored_state(board: &RoutingBoard) -> (u128, Vec<String>, u64, usize, u64, u64, [bool; 3]) {
    let trees = board.search_trees();
    let tree = trees.default_tree();
    (
        board.state_hash(),
        tree.walk_lines(),
        tree.content_digest(),
        trees.trees().len(),
        trees.work_meter().units(),
        board.revision(),
        [
            trees.is_clearance_compensation_used(),
            board.edge_pin_nets_cached(),
            board.routing_ledger().is_built(),
        ],
    )
}

#[test]
fn repeated_restores_equal_a_rebuild() {
    let board = j2_with_session();
    let snapshot = board.snapshot();
    let rebuilt = restored_state(&snapshot.rebuild());
    assert!(!rebuilt.1.is_empty());
    assert!(!snapshot.is_memoized());

    let first = snapshot.restore();
    assert!(!snapshot.is_memoized(), "the first restore only builds");
    let second = snapshot.restore();
    assert!(snapshot.is_memoized(), "the second restore fills the memo");
    // A clone of the snapshot shares the memo.
    let third = snapshot.clone().restore();
    for (n, restored) in [first, second, third].iter().enumerate() {
        assert_eq!(restored_state(restored), rebuilt, "restore {}", n + 1);
        assert_eq!(audit_board(restored.basic()), None, "restore {}", n + 1);
    }
}

#[test]
fn memoized_restore_counts_from_the_snapshot_meter() {
    // A fresh build and a memoized clone start at the same meter count, and
    // the same queries count the same units on both.
    let board = j2_with_session();
    let snapshot = board.snapshot();
    let _ = (snapshot.restore(), snapshot.restore());
    let memoized = snapshot.restore();
    let rebuilt = snapshot.rebuild();
    let units = |b: &RoutingBoard| b.search_trees().work_meter().units();
    assert_eq!(units(&memoized), units(&rebuilt));
    let query = |b: &RoutingBoard| {
        let bounds = b.bounding_box();
        let area = Area::from(Shape::from(TileShape::IntBox(bounds)));
        let found = b.overlapping_items(&area, -1);
        (found.len(), units(b))
    };
    assert_eq!(query(&memoized), query(&rebuilt));
}

/// Routes `board` with `settings` (the pipeline's whole run) and returns the
/// session text, the pass records, the run summary and the optimizer's.
fn route(board: RoutingBoard, settings: &RouterSettings, router: &BoardRouterSettings) -> String {
    let job = PipelineJob {
        settings,
        board_settings: router,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome = run_pipeline(board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    let board = outcome.board.take().expect("no listener took the board");
    let optimizer = outcome.optimizer.as_ref().expect("the optimizer ran");
    // Three candidates restore one pass snapshot three times.
    assert!(optimizer.items >= 3, "{optimizer:?}");
    format!(
        "{}\n{:?}\n{:?}\n{:?}",
        write_ses(&board, "board", SesStyle::Java),
        outcome.passes,
        outcome.summary,
        optimizer
    )
}

#[test]
fn memoized_restore_routes_like_a_rebuild() {
    // The optimizer on, so the run restores its own snapshots again.
    let mut loaded = load("fixtures/Issue433-my-board.dsn");
    loaded.settings.optimizer.enabled = true;
    loaded.settings.autorouter.max_passes = 3;
    let snapshot = loaded.board.snapshot();
    let _ = (snapshot.restore(), snapshot.restore());
    assert!(snapshot.is_memoized());
    let memoized = route(snapshot.restore(), &loaded.settings, &loaded.router);
    let rebuilt = route(snapshot.rebuild(), &loaded.settings, &loaded.router);
    assert_eq!(memoized, rebuilt);
}

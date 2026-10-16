//! Ports of `datastructures.WorkLimitTest` (Java at `a773a70ec`, B02.7), and
//! the parallel-DRC count of the board's work meter.
//!
//! Java's meter is per thread; the port's belongs to a board's search trees
//! (`board::search::work`). So `workOfOtherThreadsDoesNotCount` becomes "work
//! on another board doesn't count" and `checkingFromAnotherThreadFailsLoudly`
//! becomes "checking against another board's meter fails in debug builds".
//! All 11 tests are ported.

use freerouting::board::items::ItemId;
use freerouting::board::search::{UNITS_PER_NOMINAL_MS, WorkLimit, WorkMeter};
use freerouting::datastructures::ShapeTree;
use freerouting::geometry::{IntBox, ShapeBoundingDirections, TileShape};

fn do_work(meter: &WorkMeter, units: u64) {
    meter.add(units);
}

#[test]
fn limit_is_exceeded_only_after_its_work_is_done() {
    let meter = WorkMeter::new();
    let limit = WorkLimit::nominal_ms(&meter, 2);
    do_work(&meter, 2 * UNITS_PER_NOMINAL_MS);
    assert!(!limit.exceeded(&meter));
    do_work(&meter, 1);
    assert!(limit.exceeded(&meter));
}

#[test]
fn elapsed_time_alone_never_exceeds_the_limit() {
    let meter = WorkMeter::new();
    let limit = WorkLimit::nominal_ms(&meter, 1);
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(!limit.exceeded(&meter));
}

#[test]
fn work_done_before_the_limit_was_created_does_not_count() {
    let meter = WorkMeter::new();
    do_work(&meter, 5 * UNITS_PER_NOMINAL_MS);
    let limit = WorkLimit::nominal_ms(&meter, 1);
    assert!(!limit.exceeded(&meter));
}

#[test]
fn work_on_another_board_does_not_count() {
    let meter = WorkMeter::new();
    let other = WorkMeter::new();
    let limit = WorkLimit::nominal_ms(&meter, 1);
    std::thread::scope(|scope| {
        scope.spawn(|| do_work(&other, 10 * UNITS_PER_NOMINAL_MS));
    });
    assert!(!limit.exceeded(&meter));
}

#[test]
fn zero_limit_is_exceeded_by_the_first_unit_of_work() {
    let meter = WorkMeter::new();
    let limit = WorkLimit::nominal_ms(&meter, 0);
    assert!(!limit.exceeded(&meter));
    do_work(&meter, 1);
    assert!(limit.exceeded(&meter));
}

#[test]
fn negative_limit_behaves_like_zero() {
    let meter = WorkMeter::new();
    let limit = WorkLimit::nominal_ms(&meter, -5);
    assert!(!limit.exceeded(&meter));
    do_work(&meter, 1);
    assert!(limit.exceeded(&meter));
}

#[test]
fn largest_exact_limit_and_the_next_one_do_not_overflow() {
    let meter = WorkMeter::new();
    let largest_exact = (i64::MAX as u64 / UNITS_PER_NOMINAL_MS) as i64;
    let exact = WorkLimit::nominal_ms(&meter, largest_exact);
    let saturated = WorkLimit::nominal_ms(&meter, largest_exact + 1);
    do_work(&meter, 1000);
    assert!(!exact.exceeded(&meter));
    assert!(!saturated.exceeded(&meter));
    assert_eq!(saturated.max_units(), i64::MAX as u64);
}

#[test]
#[cfg(debug_assertions)]
fn checking_against_another_boards_meter_fails_in_debug_builds() {
    let meter = WorkMeter::new();
    let other = WorkMeter::new();
    let limit = WorkLimit::nominal_ms(&meter, 1);
    let result = std::panic::catch_unwind(|| limit.exceeded(&other));
    assert!(result.is_err(), "a check against another meter must fail");
}

#[test]
fn huge_limit_saturates_instead_of_overflowing() {
    let meter = WorkMeter::new();
    let limit = WorkLimit::nominal_ms(&meter, i64::MAX);
    do_work(&meter, 1000);
    assert!(!limit.exceeded(&meter));
}

#[test]
fn cut_off_logs_one_machine_readable_line() {
    let meter = WorkMeter::new();
    let mut limit = WorkLimit::with_site(&meter, 0, "maze_search");
    do_work(&meter, 3);
    let mut log = Vec::new();
    assert!(limit.exceeded_logging(&meter, || "net=12 item=345".to_string(), &mut log));
    assert!(limit.exceeded_logging(&meter, || "net=12 item=345".to_string(), &mut log));
    assert_eq!(
        log,
        vec!["WORK_LIMIT_CUTOFF site=maze_search nominal_ms=0 units=3 net=12 item=345"]
    );
}

#[test]
fn tree_queries_count_the_nodes_they_visit() {
    let mut tree = ShapeTree::new(ShapeBoundingDirections::FortyfiveDegree);
    let bounds = |b: IntBox| {
        TileShape::IntBox(b)
            .bounding_shape(ShapeBoundingDirections::FortyfiveDegree)
            .expect("bounded")
    };
    tree.insert(1, 0, bounds(IntBox::new(0, 0, 10, 10)));
    tree.insert(2, 0, bounds(IntBox::new(100, 100, 110, 110)));
    let before = tree.meter().units();
    tree.overlaps(&bounds(IntBox::new(0, 0, 5, 5)));
    // The root and both leaves: the root's bounding shape overlaps the query.
    assert_eq!(tree.meter().units() - before, 3);
}

#[test]
fn meter_counts_every_query_once_under_parallel_drc() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/Issue026-J2_reference.dsn"),
    )
    .expect("fixture");
    let board = freerouting::drc::load_design(&bytes).expect("board");
    let meter = board.search_trees().work_meter();
    let start = meter.units();
    let parallel = board.all_clearance_violations();
    let parallel_units = meter.units() - start;
    let ids: Vec<ItemId> = board.items().map(|i| i.id()).collect();
    let start = meter.units();
    let mut serial = 0;
    for id in ids {
        serial += board.clearance_violations(id).len();
    }
    let serial_units = meter.units() - start;
    assert!(parallel_units > 0);
    assert_eq!(parallel_units, serial_units);
    assert!(serial >= parallel.len());
}

#[test]
fn restore_starts_a_new_meter_at_zero() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/Issue026-J2_reference.dsn"),
    )
    .expect("fixture");
    let board = freerouting::drc::load_design(&bytes).expect("board");
    let _ = board.all_clearance_violations();
    assert!(board.search_trees().work_meter().units() > 0);
    let copy = board.deep_copy();
    assert_eq!(copy.search_trees().work_meter().units(), 0);
    // The copy counts on its own meter.
    let before = board.search_trees().work_meter().units();
    let _ = copy.all_clearance_violations();
    assert_eq!(board.search_trees().work_meter().units(), before);
    assert!(copy.search_trees().work_meter().units() > 0);
}

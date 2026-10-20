//! The work counts of the microbenchmarks (P01.3) are pinned.
//!
//! A change that claims to keep results identical must not move them; a
//! change that moves them on purpose (a different search tree, a changed
//! search) updates the numbers here and says why in its done note. Only the
//! times of the kernels may change between builds.

use std::path::Path;

use freerouting_dev::microbench::{KERNELS, load, run_kernels};

type Expected = [(&'static str, &'static [(&'static str, u64)])];

fn root(board: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(board)
}

fn check(board: &str, expected: &Expected) {
    let loaded = load(&root(board)).unwrap();
    // Two runs per kernel: `run_kernels` fails if the second differs.
    let reports = run_kernels(&loaded, 2, &[]).unwrap();
    let names: Vec<&str> = reports.iter().map(|r| r.kernel).collect();
    assert_eq!(names, KERNELS);
    for (report, (kernel, work)) in reports.iter().zip(expected) {
        assert_eq!(report.kernel, *kernel);
        assert_eq!(report.work, *work, "{board}: kernel {kernel}");
    }
}

#[test]
fn work_counts_of_issue433() {
    check(
        "Issue433-my-board.dsn",
        &[
            (
                "overlap",
                &[
                    ("queries", 20000),
                    ("tree_nodes_visited", 266126),
                    ("entries_found", 3789),
                ],
            ),
            (
                "complete_shape",
                &[
                    ("queries", 4000),
                    ("tree_nodes_visited", 204288),
                    ("rooms_found", 5508),
                ],
            ),
            (
                "maze",
                &[
                    // Q18.2: profile `enhanced`'s `board_minimums.from_classes`
                    // floors the pin neck-down at the narrowest class width, and
                    // one connection that only fit necked below it stays open (7
                    // routed, 6076 pushes, 3008 doors, 90636 work units and
                    // 174040 tree nodes before).
                    ("connections", 7),
                    ("routed", 6),
                    ("queue_pops", 952),
                    ("queue_pushes", 6080),
                    ("rooms_completed", 241),
                    ("doors_expanded", 3010),
                    // N06.13: profile `enhanced`'s Java-bug fixes add 148 tree
                    // nodes to the maze search (90488 and 173255 before); the
                    // routes and every other count are unchanged. Its second
                    // round adds 637 more (173403 before): the shove's
                    // pending-piece check (`shove_via_into_pending_piece =
                    // false`) queries the tree outside the work meter, so
                    // `maze_work_units` and the routes stay.
                    ("maze_work_units", 91206),
                    ("tree_nodes_visited", 173457),
                ],
            ),
            (
                "pull_tight",
                // Q18.2: on the maze kernel's board (one connection fewer):
                // 11, 49, 9279, 16 and 47 before; the DRC 46 items and 56920
                // tree nodes.
                &[
                    ("traces_before", 7),
                    ("corners_before", 39),
                    ("tree_nodes_visited", 8966),
                    ("traces_after", 11),
                    ("corners_after", 33),
                ],
            ),
            (
                "drc",
                &[
                    ("checks", 20),
                    ("items", 42),
                    ("tree_nodes_visited", 51840),
                    ("violations", 0),
                ],
            ),
        ],
    );
}

#[test]
fn work_counts_of_issue413() {
    check(
        "Issue413-test.dsn",
        &[
            (
                "overlap",
                &[
                    ("queries", 20000),
                    ("tree_nodes_visited", 507178),
                    ("entries_found", 2359),
                ],
            ),
            (
                "complete_shape",
                &[
                    ("queries", 4000),
                    ("tree_nodes_visited", 305582),
                    ("rooms_found", 5203),
                ],
            ),
            (
                "maze",
                &[
                    ("connections", 1),
                    ("routed", 1),
                    ("queue_pops", 224),
                    ("queue_pushes", 29917),
                    ("rooms_completed", 32),
                    ("doors_expanded", 396),
                    ("maze_work_units", 23261),
                    ("tree_nodes_visited", 40600),
                ],
            ),
            (
                "pull_tight",
                &[
                    ("traces_before", 12),
                    ("corners_before", 34),
                    ("tree_nodes_visited", 4210),
                    ("traces_after", 13),
                    ("corners_after", 36),
                ],
            ),
            (
                "drc",
                &[
                    ("checks", 20),
                    ("items", 38),
                    ("tree_nodes_visited", 68880),
                    ("violations", 0),
                ],
            ),
        ],
    );
}

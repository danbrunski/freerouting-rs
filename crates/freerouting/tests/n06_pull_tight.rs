//! N06.8, batch P (pull-tight): both behaviours of the two pull-tight switches and
//! the reachability counters, in their own process so that no other test resets the
//! process-wide counters (`run_stats`).
//!
//! - `smooth_loses_trace` (`TraceTightener.smoothenEndCornersAtTrace1`,
//!   `TraceTightener.java:431-441` at `a773a70ec`): the smoothing removes the trace
//!   before an insert that may fail;
//! - `pull_tight_reused_lines_change` (`PolylineTrace.pullTight`,
//!   `PolylineTrace.java:837-839`): a result whose lines `change` finds all reused
//!   still reports a change;
//! - the counter of `PolylineTrace.change`'s strict prefix or suffix identity case
//!   (`PolylineTrace.java:956-979`), which has no switch.
//!
//! `true` is Java's behaviour in each switch; the counters count in either arm.
mod board_probe;

use std::sync::Mutex;

use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::board::{ChangeOutcome, LineReuse, RoutingBoard};
use freerouting::geometry::{IntPoint, Point, Polyline};
use freerouting::optimize::{SmoothedReplace, TrackedPolyline};
use freerouting::run_stats::{self, Counter};
use freerouting::settings::JavaQuirks;

/// The counters are process-wide: the tests that read them run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn p(x: i32, y: i32) -> Point {
    Point::from(IntPoint::new(x, y))
}

/// The two-pin board with `quirks`, and its net.
fn board(quirks: JavaQuirks) -> (RoutingBoard, i32) {
    let mut board = board_probe::load(&board_probe::fixture("scoring-perfect-two-pin"));
    board.set_java_quirks(quirks);
    let net = board.items().find(|i| i.is_pin()).unwrap().net_numbers()[0];
    (board, net)
}

fn insert(board: &mut RoutingBoard, points: &[Point], net: i32, fixed: FixedState) -> ItemId {
    board
        .insert_trace_without_cleaning(Polyline::from_points(points), 0, 100, &[net], 0, fixed)
        .expect("a trace")
}

fn on_board(board: &RoutingBoard, id: ItemId) -> bool {
    board.item(id).is_some_and(|i| i.is_on_the_board())
}

fn polyline_of(board: &RoutingBoard, id: ItemId) -> Polyline {
    board.item(id).unwrap().as_trace().unwrap().polyline.clone()
}

/// The corners of `polyline` (`Polyline` has no `PartialEq`).
fn corners(polyline: &Polyline) -> Vec<Point> {
    (0..polyline.corner_count())
        .map(|i| polyline.corner(i))
        .collect()
}

fn corners_of(board: &RoutingBoard, id: ItemId) -> Vec<Point> {
    corners(&polyline_of(board, id))
}

/// Runs `f` with the statistics on and returns its result and `counter`'s change.
fn counted<R>(counter: Counter, f: impl FnOnce() -> R) -> (R, u64) {
    run_stats::enable();
    let result = f();
    let n = run_stats::Counts::now().get(counter);
    run_stats::disable();
    run_stats::reset();
    (result, n)
}

const TRACE: [(i32, i32); 3] = [(30000, 30000), (34000, 30000), (36000, 32000)];

fn trace_points() -> Vec<Point> {
    TRACE.iter().map(|&(x, y)| p(x, y)).collect()
}

/// `smooth_loses_trace`: a smoothed polyline whose insert fails (zero length and
/// not user fixed, or fewer than two corners). Java removes the trace first and
/// loses it; `false` keeps it unchanged. A polyline that inserts replaces the
/// trace in both arms, and each failing insert counts once in either arm.
#[test]
fn smooth_loses_trace_switch_both_behaviours() {
    let _guard = serial();
    // Closed: the first corner is the last one, so the length counts as 0.
    let closed = Polyline::from_points(&[
        p(30000, 30000),
        p(34000, 30000),
        p(34000, 34000),
        p(30000, 30000),
    ]);
    assert!(closed.corner_count() >= 2);
    assert_eq!(closed.first_corner(), closed.last_corner());
    // Two parallel lines and a third: Java's constructor drops the parallel one
    // and, with fewer than three lines left, returns an empty polyline.
    let short = Polyline::new(&closed.lines()[..2]);
    assert!(short.corner_count() < 2);
    for (failing, name) in [(&closed, "zero length"), (&short, "fewer than two corners")] {
        for java in [true, false] {
            let quirks = JavaQuirks::JAVA.with("smooth_loses_trace", java).unwrap();
            let (mut board, net) = board(quirks);
            let id = insert(&mut board, &trace_points(), net, FixedState::Unfixed);
            let before = polyline_of(&board, id);
            let traces = board.traces().len();
            let (replaced, n) = counted(Counter::PullTightSmoothLostTraces, || {
                board.replace_smoothed_trace(id, failing, 100)
            });
            assert_eq!(n, 1, "{name}, java={java}: one failing insert");
            if java {
                assert_eq!(replaced, SmoothedReplace::Lost, "{name}");
                assert!(!on_board(&board, id), "{name}: Java loses the trace");
                assert_eq!(board.traces().len(), traces - 1, "{name}");
            } else {
                assert_eq!(replaced, SmoothedReplace::Kept, "{name}");
                assert!(on_board(&board, id), "{name}: the fix keeps the trace");
                assert_eq!(
                    corners_of(&board, id),
                    corners(&before),
                    "{name}: unchanged"
                );
                assert_eq!(board.traces().len(), traces, "{name}");
            }
        }
    }
    for java in [true, false] {
        let quirks = JavaQuirks::JAVA.with("smooth_loses_trace", java).unwrap();
        // A polyline that inserts replaces the trace in both arms, uncounted.
        let (mut board, net) = board(quirks);
        let id = insert(&mut board, &trace_points(), net, FixedState::Unfixed);
        let smoothed = Polyline::from_points(&[p(30000, 30000), p(33000, 30000), p(36000, 32000)]);
        let (replaced, n) = counted(Counter::PullTightSmoothLostTraces, || {
            board.replace_smoothed_trace(id, &smoothed, 100)
        });
        assert_eq!(n, 0, "java={java}");
        let SmoothedReplace::Inserted(new_id) = replaced else {
            panic!("java={java}: {replaced:?}")
        };
        assert!(
            !on_board(&board, id) && on_board(&board, new_id),
            "java={java}"
        );
        assert_eq!(
            corners_of(&board, new_id),
            corners(&smoothed),
            "java={java}"
        );
        // A user-fixed trace of length 0 inserts (Java's rule), so nothing fails.
        let id = insert(&mut board, &trace_points(), net, FixedState::UserFixed);
        let (replaced, n) = counted(Counter::PullTightSmoothLostTraces, || {
            board.replace_smoothed_trace(id, &closed, 100)
        });
        assert_eq!(n, 0, "java={java}: user fixed");
        assert!(
            matches!(replaced, SmoothedReplace::Inserted(_)),
            "java={java}: {replaced:?}"
        );
    }
}

/// `pull_tight_reused_lines_change`: a pull-tight result whose lines are all the
/// old trace's (so `change_trace` returns early) reports a change under Java and
/// none under `false`, which lets pull-tight go on to the pin exits and the end
/// smoothing. A changed result reports a change in both arms. Each ignored result
/// counts once in either arm.
#[test]
fn pull_tight_reused_lines_switch_both_behaviours() {
    let _guard = serial();
    for java in [true, false] {
        let quirks = JavaQuirks::JAVA
            .with("pull_tight_reused_lines_change", java)
            .unwrap();
        let (mut board, net) = board(quirks);
        let id = insert(&mut board, &trace_points(), net, FixedState::Unfixed);
        let old = polyline_of(&board, id);
        let old_lines = old.lines().len();
        let reused = TrackedPolyline::of_trace(old.clone());
        let (reported, n) = counted(Counter::PullTightReusedLinesFlips, || {
            board.apply_pull_tight_result(id, reused, old_lines)
        });
        assert_eq!(reported, java, "java={java}: every line reused");
        assert_eq!(n, 1, "java={java}");
        assert_eq!(
            corners_of(&board, id),
            corners(&old),
            "java={java}: unchanged"
        );

        let new = Polyline::from_points(&[p(30000, 30000), p(33000, 30000), p(36000, 32000)]);
        let changed = TrackedPolyline::all_new(new.clone());
        let (reported, n) = counted(Counter::PullTightReusedLinesFlips, || {
            board.apply_pull_tight_result(id, changed, old_lines)
        });
        assert!(reported, "java={java}: a changed result");
        assert_eq!(n, 0, "java={java}");
        assert_eq!(
            corners_of(&board, id),
            corners(&new),
            "java={java}: changed"
        );
    }
}

/// The reviewed list's identity case: `change` ignores a new polyline that is a
/// strict prefix or suffix of the old one by line identity (or the reverse),
/// because both of Java's scans stop at the shorter polyline. The port does the
/// same and counts it; an equal polyline or a real change is not counted.
#[test]
fn prefix_suffix_identity_case_is_ignored_and_counted() {
    let _guard = serial();
    let (mut board, net) = board(JavaQuirks::JAVA);
    let points = [
        p(30000, 30000),
        p(34000, 30000),
        p(36000, 32000),
        p(36000, 36000),
    ];
    let id = insert(&mut board, &points, net, FixedState::Unfixed);
    let old = polyline_of(&board, id);
    let lines = old.lines().to_vec();
    assert_eq!(lines.len(), 5);
    let prefix = Polyline::new(&lines[..4]);
    let suffix = Polyline::new(&lines[1..]);
    assert_eq!(prefix.lines().len(), 4);
    assert_eq!(suffix.lines().len(), 4);
    let cases = [
        (
            "strict prefix",
            prefix,
            LineReuse {
                prefix: 4,
                suffix: 0,
            },
            ChangeOutcome::Unchanged,
            1,
        ),
        (
            "strict suffix",
            suffix,
            LineReuse {
                prefix: 0,
                suffix: 4,
            },
            ChangeOutcome::Unchanged,
            1,
        ),
        (
            "equal",
            old.clone(),
            LineReuse {
                prefix: 5,
                suffix: 5,
            },
            ChangeOutcome::Unchanged,
            0,
        ),
    ];
    for (name, polyline, reuse, want, count) in cases {
        let (outcome, n) = counted(Counter::TraceChangePrefixSuffixIgnored, || {
            board.change_trace(id, polyline, reuse)
        });
        assert_eq!(outcome, want, "{name}");
        assert_eq!(n, count, "{name}");
        assert_eq!(
            corners_of(&board, id),
            corners(&old),
            "{name}: the trace keeps its lines"
        );
    }
    let new = Polyline::from_points(&[p(30000, 30000), p(33000, 30000), p(36000, 36000)]);
    let (outcome, n) = counted(Counter::TraceChangePrefixSuffixIgnored, || {
        board.change_trace(id, new, LineReuse::NONE)
    });
    assert_eq!(outcome, ChangeOutcome::Changed);
    assert_eq!(n, 0);
}

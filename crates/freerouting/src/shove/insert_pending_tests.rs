//! N06.15 `shove_via_into_pending_piece`: both behaviours of the re-query that
//! runs before a substitute piece goes on the board, and its counters.
//!
//! A unit-level test: a board where a shove moves a via onto a pending piece was
//! not found small. The event needs a cascade of several pushes (N03.2 traced it on
//! GP8B at recursion depths 16 to 19) with a via moved back onto an ancestor's
//! piece. Synthetic boards with dense trace bundles, bends and vias (118,000
//! random forced inserts), 400 forced inserts on each of 80 routed probe boards and
//! 3,000 on each of four dense ones, the `shove-insert` probe stages on 30 probe
//! boards, and three-pass `legacy` routes of 15 A14 boards never reached it; only
//! GP8B's routes do, which is too large for a unit test. So the test drives the
//! re-query on the board state it sees at that point: a piece's shapes over a via
//! of another net.

use freerouting_geometry::{IntBox, IntPoint, Point, Polyline, TileShape};

use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::structure::FixedState;
use crate::io::specctra::{BoardReadResult, read_board_from_str};
use crate::run_stats::{self, Counter, Counts};
use crate::settings::JavaQuirks;
use crate::shove::trace_check::TraceShover;

const ID: &str = "shove_via_into_pending_piece";

/// Two signal layers, the via `Via[0-1]_800:400_um` (radius 4000), trace half
/// width 1250 and clearance 2000 (board units, um * 10); nets `UNUSED` and `SINGLE`.
fn board() -> RoutingBoard {
    let BoardReadResult::Success { board, .. } =
        read_board_from_str(include_str!("../../../../fixtures/scoring-empty-net.dsn"))
    else {
        panic!("fixture")
    };
    *board
}

fn net(board: &RoutingBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .expect(name)
        .net_number
}

fn class(board: &RoutingBoard, net: i32) -> i32 {
    let class = board.rules.nets.get(net).expect("net").net_class();
    board.rules.net_classes.get(class).trace_clearance_class()
}

fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> TileShape {
    TileShape::IntBox(IntBox::new(x0, y0, x1, y1))
}

/// The re-query of a piece of net `SINGLE` with `shapes` on layer 0, under the
/// switch value `java`: its answer, the counters it recorded and the work units it
/// put on the board's meter.
fn query(
    board: &mut RoutingBoard,
    shapes: &[TileShape],
    java: bool,
) -> (Option<ItemId>, Counts, u64) {
    board.set_java_quirks(JavaQuirks::JAVA.with(ID, java).unwrap());
    let single = net(board, "SINGLE");
    let class = class(board, single);
    let before = board.search_trees().work_meter().units();
    let (via, counts) = run_stats::capture(|| {
        TraceShover::new(board).foreign_via_in_pending_piece(shapes, 0, &[single], class)
    });
    let units = board.search_trees().work_meter().units() - before;
    (via, counts, units)
}

#[test]
fn pending_piece_switch_both_behaviours_and_counters() {
    let _guard = run_stats::TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    run_stats::enable();
    let mut board = board();
    let padstack = board.library.via_padstack_ids()[0];
    let unused = net(&board, "UNUSED");
    let single = net(&board, "SINGLE");
    let class = class(&board, single);
    let foreign_via = board.insert_via(
        padstack,
        IntPoint::new(50000, 50000),
        &[unused],
        class,
        FixedState::Unfixed,
        false,
    );
    let own_via = board.insert_via(
        padstack,
        IntPoint::new(20000, 20000),
        &[single],
        class,
        FixedState::Unfixed,
        false,
    );
    let corners = [
        Point::Int(IntPoint::new(30000, 80000)),
        Point::Int(IntPoint::new(70000, 80000)),
    ];
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&corners),
            0,
            1250,
            &[unused],
            class,
            FixedState::Unfixed,
        )
        .expect("foreign trace");
    assert_ne!(foreign_via, own_via);

    // A piece over the foreign via (the via's copper reaches y 46000..54000).
    let over_via = rect(30000, 48750, 70000, 51250);
    // 1000 above the via's copper: inside the clearance (2000), no copper overlap.
    let near_via = rect(30000, 55000, 70000, 57500);
    // Over the own-net via only, and far from everything.
    let over_own_via = rect(15000, 18750, 25000, 21250);
    let far = rect(30000, 62000, 70000, 64500);
    // Over the foreign trace's copper (y 78750..81250), and 1000 above it.
    let over_trace = rect(30000, 79000, 70000, 81500);
    let near_trace = rect(30000, 82250, 70000, 84750);

    for java in [true, false] {
        let fixed = |via: ItemId| if java { None } else { Some(via) };
        // Java inserts the piece over the via; the fix fails the shove at it.
        let (via, counts, _) = query(&mut board, &[far.clone(), over_via.clone()], java);
        assert_eq!(via, fixed(foreign_via), "java {java}");
        assert_eq!(
            counts.get(Counter::ShoveViaIntoPendingPieces),
            1,
            "java {java}"
        );
        assert_eq!(counts.get(Counter::ShovePendingPieceForeignTraces), 0);
        // The re-query uses the piece's clearance, as the insert's own query does.
        let (via, counts, _) = query(&mut board, std::slice::from_ref(&near_via), java);
        assert_eq!(via, fixed(foreign_via), "java {java}");
        assert_eq!(counts.get(Counter::ShoveViaIntoPendingPieces), 1);
        // A via of the piece's own net, or none near: nothing to fail at.
        for shapes in [vec![over_own_via.clone()], vec![far.clone()]] {
            let (via, counts, _) = query(&mut board, &shapes, java);
            assert_eq!(via, None, "java {java}");
            assert_eq!(counts.get(Counter::ShoveViaIntoPendingPieces), 0);
        }
        // A trace of another net is only counted, and only where the copper
        // overlaps: a piece placed at the clearance from it is the normal case.
        let (via, counts, _) = query(&mut board, std::slice::from_ref(&over_trace), java);
        assert_eq!(via, None, "java {java}");
        assert_eq!(counts.get(Counter::ShovePendingPieceForeignTraces), 1);
        let (_, counts, _) = query(&mut board, std::slice::from_ref(&near_trace), java);
        assert_eq!(counts.get(Counter::ShovePendingPieceForeignTraces), 0);
    }
    run_stats::disable();
}

/// Java's arm only observes: with statistics on its queries leave the work meter
/// alone, so a work limit cuts off where it would without them; with statistics
/// off it does not query at all. The fix queries on the meter, as the board's
/// other queries do.
#[test]
fn pending_piece_counting_leaves_the_work_meter_alone() {
    let _guard = run_stats::TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut board = board();
    let padstack = board.library.via_padstack_ids()[0];
    let unused = net(&board, "UNUSED");
    let class = class(&board, unused);
    board.insert_via(
        padstack,
        IntPoint::new(50000, 50000),
        &[unused],
        class,
        FixedState::Unfixed,
        false,
    );
    let shapes = [rect(30000, 48750, 70000, 51250)];
    for stats in [true, false] {
        if stats {
            run_stats::enable();
        } else {
            run_stats::disable();
        }
        let (via, _, units) = query(&mut board, &shapes, true);
        assert_eq!((via, units), (None, 0), "stats {stats}");
        let shover = TraceShover::new(&mut board);
        assert_eq!(shover.checks_pending_piece(), stats, "stats {stats}");
        let (via, _, units) = query(&mut board, &shapes, false);
        assert!(via.is_some() && units > 0, "stats {stats}: {units}");
        let shover = TraceShover::new(&mut board);
        assert!(shover.checks_pending_piece());
    }
    run_stats::disable();
}

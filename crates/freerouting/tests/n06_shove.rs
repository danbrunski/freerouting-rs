//! N06.7, batch S (shove): both behaviours of the four shove switches and their
//! reachability counters, in their own process so that no other test resets the
//! process-wide counters (`run_stats`).
//!
//! - `in_front_of_pad_typo` and `in_front_of_pad_side0_flank`
//!   (`ForcedPadRouter.inFrontOfPad`, side 0; Java at `a773a70ec`, lines 78 and 86);
//! - `store_trace_class_self_compare` (`ShapeTraceEntries.storeTrace`, line 385);
//! - `rational_perp_projection` (`RationalPoint.perpendicularProjection`, through
//!   `storeTrace`'s end-corner projection).
//!
//! `true` is Java's behaviour in each switch; the counters count, in either arm, the
//! tests whose result the switch's other value would flip.
use std::sync::Mutex;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::geometry::shapes::{IntBox, IntOctagon, TileShape};
use freerouting::geometry::{IntPoint, Line, Polyline};
use freerouting::io::specctra::{BoardReadResult, read_board_from_str};
use freerouting::run_stats::{self, Counter};
use freerouting::settings::JavaQuirks;
use freerouting::shove::ShapeTraceEntries;
use freerouting::shove::drill_check::in_front_of_pad_with;

/// The counters are process-wide: the tests that read them run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn quirks(id: &str, java: bool) -> JavaQuirks {
    JavaQuirks::JAVA.with(id, java).unwrap()
}

/// The octagon of the box (-100, -100)-(100, 100): ul = ll = -200, lr = ur = 200.
fn pad() -> TileShape {
    TileShape::IntOctagon(IntOctagon::new(-100, -100, 100, 100, -200, 200, -200, 200))
}

fn line(a: (i32, i32), b: (i32, i32)) -> Line {
    Line::new(IntPoint { x: a.0, y: a.1 }, IntPoint { x: b.0, y: b.1 })
}

/// `in_front_of_pad_side0_flank`: right of the pad (min x 150 >= right 100), one end
/// past the upper right diagonal (x + y = 300 >= 200), the other not (50). Java's
/// `min(x + y)` says not in front; `false` takes `max`, as sides 2, 4 and 6 test the
/// extreme that reaches past their diagonal.
#[test]
fn side0_flank_switch_both_behaviours() {
    let l = line((150, -100), (300, 0));
    let java = JavaQuirks::JAVA;
    let fixed = quirks("in_front_of_pad_side0_flank", false);
    assert!(!in_front_of_pad_with(&l, &pad(), 0, 0, true, &java));
    assert!(in_front_of_pad_with(&l, &pad(), 0, 0, true, &fixed));
    // The flank clause runs only with sides, and only on side 0.
    assert!(!in_front_of_pad_with(&l, &pad(), 0, 0, false, &fixed));
    for side in 1..8 {
        assert_eq!(
            in_front_of_pad_with(&l, &pad(), side, 0, true, &java),
            in_front_of_pad_with(&l, &pad(), side, 0, true, &fixed),
            "side {side}"
        );
    }
    // Both ends past the diagonal: `min` and `max` agree.
    let both = line((250, -40), (300, 0));
    assert!(in_front_of_pad_with(&both, &pad(), 0, 0, true, &java));
    assert!(in_front_of_pad_with(&both, &pad(), 0, 0, true, &fixed));
}

/// Each side-0 counter counts the tests its switch flips, in either arm, and the
/// returned value stays the configured one.
#[test]
fn in_front_of_pad_counters_count_flips_in_both_arms() {
    let _guard = serial();
    // Flipped by the typo only (see `in_front_of_pad_keeps_java_typo`).
    let typo_line = line((300, -100), (150, -140));
    // Flipped by the flank only.
    let flank_line = line((150, -100), (300, 0));
    // Flipped by neither.
    let neither = line((0, 300), (50, 400));
    for java in [true, false] {
        run_stats::enable();
        let q = quirks("in_front_of_pad_typo", java)
            .with("in_front_of_pad_side0_flank", java)
            .unwrap();
        assert_eq!(
            in_front_of_pad_with(&typo_line, &pad(), 0, 0, false, &q),
            java
        );
        assert_eq!(
            in_front_of_pad_with(&flank_line, &pad(), 0, 0, true, &q),
            !java
        );
        assert!(in_front_of_pad_with(&neither, &pad(), 0, 0, true, &q));
        // Other sides never count.
        in_front_of_pad_with(&typo_line, &pad(), 4, 0, true, &q);
        in_front_of_pad_with(&flank_line, &pad(), 6, 0, true, &q);
        let counts = run_stats::Counts::now();
        assert_eq!(counts.get(Counter::ShoveInFrontOfPadTypoFlips), 1, "{java}");
        assert_eq!(
            counts.get(Counter::ShoveInFrontOfPadFlankFlips),
            1,
            "{java}"
        );
        run_stats::disable();
    }
    // Statistics off: nothing is counted.
    run_stats::reset();
    in_front_of_pad_with(&typo_line, &pad(), 0, 0, true, &JavaQuirks::JAVA);
    assert_eq!(
        run_stats::Counts::now().get(Counter::ShoveInFrontOfPadTypoFlips),
        0
    );
}

/// One-layer board with the net PERFECT and a second clearance class.
fn board() -> (RoutingBoard, i32, i32, i32) {
    let BoardReadResult::Success { board, .. } = read_board_from_str(include_str!(
        "../../../fixtures/scoring-perfect-two-pin.dsn"
    )) else {
        panic!("fixture")
    };
    let mut board = *board;
    let net = board
        .items()
        .find(|i| i.is_pin())
        .expect("pin")
        .net_numbers()[0];
    let class = board
        .items()
        .find(|i| i.is_pin())
        .unwrap()
        .clearance_class();
    assert!(board.add_clearance_class("n06_other"));
    let other = board.rules.clearance_matrix.class_count() as i32 - 1;
    assert_ne!(class, other);
    (board, net, class, other)
}

fn insert(board: &mut RoutingBoard, lines: &[Line], net: i32, class: i32) -> ItemId {
    let polyline = Polyline::new(lines);
    board
        .insert_trace_without_cleaning(polyline, 0, 100, &[net], class, FixedState::Unfixed)
        .expect("trace")
}

fn store(board: &RoutingBoard, shape: TileShape, trace: ItemId) -> (bool, ShapeTraceEntries) {
    // A foreign net (none), so that the end-corner test runs.
    let mut entries = ShapeTraceEntries::new(shape, 0, &[], 1, None);
    let stored = entries.store_items(board, [trace], false, false);
    (stored, entries)
}

fn rect(ll: (i32, i32), ur: (i32, i32)) -> TileShape {
    TileShape::IntBox(IntBox::new(ll.0, ll.1, ur.0, ur.1))
}

/// `store_trace_class_self_compare`: two same-net traces of different classes meet
/// at (5000, 5000), inside the shape. Java compares the contact's class with itself,
/// so the contact may be shoved; `false` compares it with the stored trace's class
/// and stops at the contact.
#[test]
fn store_trace_class_switch_both_behaviours() {
    let _guard = serial();
    for (same_class, expect_java, expect_fixed) in [(true, true, true), (false, true, false)] {
        let (mut board, net, class, other) = board();
        let a = insert(
            &mut board,
            &[
                line((4000, 0), (4000, 1)),
                line((4000, 5000), (5000, 5000)),
                line((5000, 0), (5000, 1)),
            ],
            net,
            class,
        );
        let b = insert(
            &mut board,
            &[
                line((5000, 0), (5000, 1)),
                line((5000, 5000), (6000, 5000)),
                line((6000, 0), (6000, 1)),
            ],
            net,
            if same_class { class } else { other },
        );
        assert_eq!(
            board.trace_end_contacts(a).into_iter().collect::<Vec<_>>(),
            [b]
        );
        let shape = rect((4800, 4800), (5200, 5200));
        for (java, expect) in [(true, expect_java), (false, expect_fixed)] {
            board.set_java_quirks(quirks("store_trace_class_self_compare", java));
            run_stats::enable();
            let (stored, entries) = store(&board, shape.clone(), a);
            let flips = run_stats::Counts::now().get(Counter::ShoveStoreTraceClassFlips);
            run_stats::disable();
            assert_eq!(stored, expect, "same class {same_class}, java {java}");
            if !expect {
                assert_eq!(entries.found_obstacle(), Some(b));
            }
            assert_eq!(flips, u64::from(!same_class), "same class {same_class}");
        }
    }
}

/// `rational_perp_projection`: the counter counts an end-corner entry point the
/// switch changes, in either arm (the entry points themselves are pinned by
/// `shove::trace_entries::tests::rational_end_corner_projection_switch_both_behaviours`).
/// An integer end corner projects the same under both values and is never counted.
#[test]
fn rational_projection_counter_counts_in_both_arms() {
    let _guard = serial();
    for (end_x, b_line, expect) in [
        (5001, line((5000, 5001), (5002, 5000)), 1),
        (5000, line((4998, 5001), (5000, 5000)), 0),
    ] {
        let (mut board, net, class, _) = board();
        // A ends at (end_x, 4500 + (end_x - 4000) / 2): rational for 5001, (5000, 5000)
        // for 5000; B of the same net starts there.
        let a = insert(
            &mut board,
            &[
                line((3000, 0), (3000, 1)),
                line((4000, 4500), (6000, 5500)),
                line((end_x, 0), (end_x, 1)),
            ],
            net,
            class,
        );
        let b = insert(
            &mut board,
            &[
                line((end_x, 0), (end_x, 1)),
                b_line,
                line((6000, 0), (6000, 1)),
            ],
            net,
            class,
        );
        assert_eq!(
            board.trace_end_contacts(a).into_iter().collect::<Vec<_>>(),
            [b]
        );
        for java in [true, false] {
            board.set_java_quirks(quirks("rational_perp_projection", java));
            run_stats::enable();
            let (stored, _) = store(&board, rect((4800, 4800), (5200, 5200)), a);
            let flips = run_stats::Counts::now().get(Counter::ShoveRationalProjectionFlips);
            run_stats::disable();
            assert!(stored);
            assert_eq!(flips, expect, "end x {end_x}, java {java}");
        }
    }
}

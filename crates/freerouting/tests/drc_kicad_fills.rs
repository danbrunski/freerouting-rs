//! K15.16: the KiCad-counted report's split nets and isolated islands, as
//! KiCad's zone fill joins items.
//!
//! The boards are in `tests/data/kicad/pcu-k1516/`, hand-written by
//! `make_boards.py` there on the pcu board's 4-layer stack (`F.Cu`, `In1.Cu`
//! and `In2.Cu` power, `B.Cu`; `../power-control-unit` at `76163c7`), each
//! twice: `<name>-unfilled.kicad_pcb` as written (zones without
//! `filled_polygon`, so the report counts with the router's fill model) and
//! `<name>.kicad_pcb` as `kicad-cli pcb drc --severity-all --format json
//! --refill-zones --save-board` (KiCad 10.0.7) saved it, with KiCad's fills.
//! The KiCad counts are that run's:
//!
//! - `a-pour-joins-two-pads`: N1's pads J1 and J2 on F.Cu, joined only by
//!   N1's F.Cu pour; N1 also has an In2.Cu plane, so the DSN reader drops the
//!   F.Cu pour (K15.11). KiCad: 0 unconnected; 1 `isolated_copper`, the
//!   In2.Cu plane, which no pad reaches and which KiCad keeps (its zone's
//!   only island).
//! - `b-pour-cut-in-two`: N2's trace from J3 to J4 cuts N1's F.Cu pour in
//!   two, J1 on one side and J2 on the other. KiCad: 1 unconnected (two
//!   pieces of the zone).
//! - `c-island-mode-{0,1,2-keep,2-remove}`: N2's trace cuts off a part of
//!   N1's pour that no N1 item reaches, the zone's `island_removal_mode` 0,
//!   1, and 2 with `island_area_min` 10 and 200 mm² (the part is about 90
//!   mm²). KiCad: 0 unconnected; `isolated_copper` 0, 1, 1 and 0.
//! - `d2-two-isolated-parts`: N2's trace cuts N1's pour in two, neither part
//!   reaching an N1 item (N1's pads are outside the zone), mode 0. KiCad: 0
//!   unconnected, 2 `isolated_copper`: every island of the zone is isolated,
//!   and KiCad keeps them all.
//! - `d3-one-connected-two-isolated`: two traces cut N1's pour in three, N1's
//!   pads in the first part, mode 0. KiCad: 0 unconnected, no
//!   `isolated_copper`.

use std::path::PathBuf;

use freerouting::drc::{DesignRulesChecker, DrcReport, DrcRunOptions, ReportRules, run_drc};

const DEAD: &str = "isolated_island_dead_copper";

fn case(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/kicad/pcu-k1516")
        .join(format!("{name}.kicad_pcb"))
}

fn drc(name: &str) -> (freerouting::board::RoutingBoard, DrcReport) {
    let run = run_drc(&DrcRunOptions::new(case(name))).expect("the case loads");
    (run.board, run.report)
}

fn of_type(report: &DrcReport, kind: &str) -> usize {
    report
        .violations
        .iter()
        .filter(|v| v.violation_type == kind)
        .count()
}

fn unconnected(report: &DrcReport, kind: &str) -> usize {
    report
        .unconnected_items
        .iter()
        .filter(|v| v.violation_type == kind)
        .count()
}

/// `(unconnected, isolated islands)` of the KiCad-counted report of `name`
/// with KiCad's fill and with the fill model, after checking which one the
/// report counted with.
fn both(name: &str) -> [(usize, usize); 2] {
    let (filled, filled_report) = drc(name);
    assert!(
        filled.kicad_report_rules().fills_current(&filled),
        "{name}: KiCad's fill counts"
    );
    let unfilled_name = format!("{name}-unfilled");
    let (unfilled, unfilled_report) = drc(&unfilled_name);
    assert!(
        !unfilled.kicad_report_rules().fills_current(&unfilled),
        "{name}: no fill in the file, the fill model counts"
    );
    [filled_report, unfilled_report].map(|r| {
        assert_eq!(
            unconnected(&r, "isolated_island_unconnected"),
            0,
            "{name}: a KiCad-counted report has no unconnected islands"
        );
        (r.unconnected_items.len(), of_type(&r, DEAD))
    })
}

fn java(name: &str) -> DrcReport {
    let (board, _) = drc(name);
    let mut checker = DesignRulesChecker::new(&board);
    checker.set_report_rules(ReportRules::Java);
    checker.generate_report("case", "mm")
}

#[test]
fn two_pads_joined_only_by_a_pour_are_connected() {
    // KiCad: 0 unconnected, 1 isolated_copper (the In2.Cu plane)
    assert_eq!(both("a-pour-joins-two-pads"), [(0, 1), (0, 1)]);
    // Java's connectivity has no dropped pour: the pads are two groups
    let java = java("a-pour-joins-two-pads");
    assert_eq!(unconnected(&java, "unconnectedItems"), 1);
}

#[test]
fn a_pour_cut_in_two_is_one_unconnected() {
    // KiCad: 1 unconnected, the two pieces of the zone
    assert_eq!(both("b-pour-cut-in-two"), [(1, 0), (1, 0)]);
    let (_, report) = drc("b-pour-cut-in-two");
    let split = &report.unconnected_items[0];
    assert_eq!(split.violation_type, "unconnectedItems");
    assert!(
        split
            .description
            .starts_with("Unconnected items: Pin [N1] and Pin [N1]"),
        "{}",
        split.description
    );
    // Java joins both pads through the area's outline and reports the second
    // piece of its fill as an island with an item, and two dead islands
    let java = java("b-pour-cut-in-two");
    assert_eq!(unconnected(&java, "unconnectedItems"), 0);
    assert_eq!(unconnected(&java, "isolated_island_unconnected"), 1);
    assert_eq!(of_type(&java, DEAD), 2);
}

#[test]
fn an_isolated_island_is_reported_as_its_zone_keeps_it() {
    // KiCad: isolated_copper 0 (mode 0), 1 (mode 1), 1 and 0 (mode 2 under
    // and over the island's area); never unconnected
    assert_eq!(both("c-island-mode-0"), [(0, 0), (0, 0)]);
    assert_eq!(both("c-island-mode-1"), [(0, 1), (0, 1)]);
    assert_eq!(both("c-island-mode-2-keep"), [(0, 1), (0, 1)]);
    assert_eq!(both("c-island-mode-2-remove"), [(0, 0), (0, 0)]);
    // Java reports the cut-off part and the spokes' gap around each pad
    assert_eq!(of_type(&java("c-island-mode-0"), DEAD), 2);
}

#[test]
fn a_zone_whose_islands_are_all_isolated_keeps_them() {
    // KiCad: 2 isolated_copper under mode 0, as no island reaches a pad
    assert_eq!(both("d2-two-isolated-parts"), [(0, 2), (0, 2)]);
    // and none once one island reaches the pads
    assert_eq!(both("d3-one-connected-two-isolated"), [(0, 0), (0, 0)]);
}

#[test]
fn the_files_fill_counts_only_while_the_board_is_unchanged() {
    let (mut board, report) = drc("b-pour-cut-in-two");
    assert_eq!(report.unconnected_items.len(), 1);
    // remove N2's trace: the file's fill (cut in two) is stale, and the fill
    // model has N1's pour in one piece
    let n2 = board
        .rules
        .nets
        .get_by_name("N2", 1)
        .expect("N2")
        .net_number;
    let trace = board
        .items()
        .find(|i| i.is_trace() && i.contains_net(n2))
        .map(|i| i.id())
        .expect("N2's trace");
    // input copper is user fixed
    board.unfix(trace);
    assert!(board.remove_item(trace));
    assert!(!board.kicad_report_rules().fills_current(&board));
    let after = DesignRulesChecker::new(&board).generate_report("case", "mm");
    // N1 is one piece; N2's pads J3 and J4 are apart now
    let split: Vec<&str> = after
        .unconnected_items
        .iter()
        .map(|u| u.description.as_str())
        .collect();
    assert_eq!(split.len(), 1, "{split:?}");
    assert!(
        split[0].starts_with("Unconnected items: Pin [N2] and Pin [N2]"),
        "{split:?}"
    );
}

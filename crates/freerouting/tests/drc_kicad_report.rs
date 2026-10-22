//! K15.5: the DRC report counts what KiCad counts, and the score's same-net
//! via switch.
//!
//! One synthetic board (`BOARD`) holds the three cases of the pcu board's
//! report:
//! - a via of the input (`(type route)`, user fixed) inside SMD pad C1-1 of its
//!   own net GND, off the pad's centre, joining the pad to the GND plane on
//!   `Gnd.Cu`;
//! - a via the router placed (no type, unfixed) inside pad C2-1 of GND, also
//!   joined to the plane;
//! - on net SIG, a trace T1 from pad C3-1's centre to the middle of the trace
//!   between C1-2 and C2-2 (a T junction), and a stub S from pad C2-2's centre
//!   to a free end.
//!
//! Java's report (`ReportRules::Java`) has both vias as `holeClearance` with
//! their pads and as `via_dangling` (each has one contact, the plane), and T1
//! and S as `track_dangling`. KiCad's (`ReportRules::Kicad`, the default)
//! keeps only the placed via's pair (D8: the router never puts a via in a
//! pad), no dangling via, and S alone. The search's obstacle test is Java's
//! for both vias.

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::drc::{DesignRulesChecker, DrcReport, ReportRules, UnconnectedKind};
use freerouting::io::specctra::{BoardReadResult, read_board_from_str_with};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::JavaQuirks;

const BOARD: &str = r#"(pcb k15-5-drc.dsn
  (parser
    (string_quote ")
    (space_in_quoted_tokens on)
    (host_cad "KiCad's Pcbnew")
    (host_version "synthetic: K15.5 same-net vias in pads, a T junction and a stub")
  )
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer Gnd.Cu (type power) (property (index 1)))
    (layer Power.Cu (type power) (property (index 2)))
    (layer B.Cu (type signal) (property (index 3)))
    (boundary
      (path pcb 0  0 0  20000 0  20000 -10000  0 -10000  0 0)
    )
    (plane GND (polygon Gnd.Cu 0  0 0  20000 0  20000 -10000  0 -10000))
    (via "Via[0-3]_600:300_um")
    (rule
      (width 200)
      (clearance 200)
    )
  )
  (placement
    (component "C_0603"
      (place C1 4000 -3000 front 0)
      (place C2 14000 -3000 front 180)
      (place C3 8800 -7000 front 0)
    )
  )
  (library
    (image "C_0603"
      (pin RectPad 1 -800 0)
      (pin RectPad 2 800 0)
    )
    (padstack RectPad
      (shape (rect F.Cu -400 -450 400 450))
      (attach off)
    )
    (padstack "Via[0-3]_600:300_um"
      (shape (circle F.Cu 600))
      (shape (circle Gnd.Cu 600))
      (shape (circle Power.Cu 600))
      (shape (circle B.Cu 600))
      (attach off)
    )
  )
  (network
    (net GND (pins C1-1 C2-1))
    (net SIG (pins C1-2 C2-2 C3-1))
    (net NC (pins C3-2))
    (class kicad_default "" GND NC SIG
      (circuit (use_via "Via[0-3]_600:300_um"))
      (rule (width 200) (clearance 200))
    )
  )
  (wiring
    (via "Via[0-3]_600:300_um" 3300 -3000 (net GND) (type route))
    (via "Via[0-3]_600:300_um" 14900 -3000 (net GND))
    (wire (path F.Cu 200  4800 -3000  13200 -3000) (net SIG) (type route))
    (wire (path F.Cu 200  8000 -7000  8000 -3000) (net SIG) (type route))
    (wire (path F.Cu 200  13200 -3000  13200 -1500) (net SIG) (type route))
  )
)
"#;

fn board(quirks: JavaQuirks) -> RoutingBoard {
    match read_board_from_str_with(BOARD, quirks) {
        BoardReadResult::Success { board, .. } => *board,
        _ => panic!("the K15.5 board reads"),
    }
}

/// The ids of the via centred at `x` (board units: 10 per um).
fn via_at(board: &RoutingBoard, x: i32) -> ItemId {
    board
        .items()
        .find(|i| i.as_via().is_some_and(|v| v.drill.center.x == x * 10))
        .map(|i| i.id())
        .expect("the via")
}

/// The id of the trace whose first corner is at `(x, y)` um.
fn trace_from(board: &RoutingBoard, x: i32, y: i32) -> ItemId {
    board
        .items()
        .find(|i| {
            i.as_trace().is_some_and(|t| {
                let c = t.first_corner().to_float();
                (c.x, c.y) == (f64::from(x * 10), f64::from(y * 10))
            })
        })
        .map(|i| i.id())
        .expect("the trace")
}

fn report(board: &RoutingBoard, rules: ReportRules) -> DrcReport {
    let mut checker = DesignRulesChecker::new(board);
    checker.set_check_zone_islands(false);
    checker.set_report_rules(rules);
    checker.generate_report("k15-5-drc.dsn", "mm")
}

/// The item ids of the report's violations of type `t`, each sorted.
fn ids(report: &DrcReport, t: &str) -> Vec<Vec<i32>> {
    let mut result: Vec<Vec<i32>> = report
        .violations
        .iter()
        .filter(|v| v.violation_type == t)
        .map(|v| {
            let mut ids: Vec<i32> = v.items.iter().map(|i| i.uuid.parse().unwrap()).collect();
            ids.sort_unstable();
            ids
        })
        .collect();
    result.sort();
    result
}

fn pin_of_pad(board: &RoutingBoard, via: ItemId) -> i32 {
    let v = board.item(via).unwrap();
    board
        .items()
        .find(|i| {
            i.is_pin() && i.shares_net(v) && {
                let c = i.drill().unwrap().center;
                (c.x - v.drill().unwrap().center.x).abs() <= 1000
            }
        })
        .map(|i| i.id().0)
        .expect("the via's pad")
}

#[test]
fn the_fixture_has_an_input_via_and_a_routed_via_in_pads_of_their_net() {
    let board = board(JavaQuirks::JAVA);
    let input = via_at(&board, 3300);
    let routed = via_at(&board, 14900);
    assert_eq!(
        board.item(input).unwrap().fixed_state(),
        FixedState::UserFixed
    );
    assert_eq!(
        board.item(routed).unwrap().fixed_state(),
        FixedState::Unfixed
    );
    // D8: the search keeps vias out of pads of their own net, input or not.
    for via in [input, routed] {
        let pin = board.get_item(pin_of_pad(&board, via)).unwrap();
        assert!(board.is_obstacle(board.item(via).unwrap(), pin));
    }
}

#[test]
fn java_report_counts_same_net_pairs_and_centre_contacts() {
    let board = board(JavaQuirks::JAVA);
    let report = report(&board, ReportRules::Java);
    let (input, routed) = (via_at(&board, 3300), via_at(&board, 14900));
    let mut pairs = vec![
        vec![pin_of_pad(&board, input), input.0],
        vec![pin_of_pad(&board, routed), routed.0],
    ];
    pairs.iter_mut().for_each(|p| p.sort_unstable());
    pairs.sort();
    assert_eq!(ids(&report, "holeClearance"), pairs);
    assert!(ids(&report, "clearance").is_empty());
    assert_eq!(
        ids(&report, "via_dangling"),
        vec![vec![input.0], vec![routed.0]]
    );
    let (t1, stub) = (
        trace_from(&board, 8000, -7000),
        trace_from(&board, 13200, -3000),
    );
    let mut dangling = vec![vec![t1.0], vec![stub.0]];
    dangling.sort();
    assert_eq!(ids(&report, "track_dangling"), dangling);
}

#[test]
fn kicad_report_counts_different_nets_and_copper_overlap() {
    let board = board(JavaQuirks::JAVA);
    let report = report(&board, ReportRules::Kicad);
    let routed = via_at(&board, 14900);
    let mut pair = vec![pin_of_pad(&board, routed), routed.0];
    pair.sort_unstable();
    // The input's via is left out; the via the run placed stays (D8), as
    // KiCad's `clearance` (K15.11).
    assert_eq!(ids(&report, "clearance"), vec![pair]);
    assert!(ids(&report, "holeClearance").is_empty());
    // Each via meets its pad on F.Cu and the plane on Gnd.Cu.
    assert!(ids(&report, "via_dangling").is_empty());
    // T1 ends on C3-1 and on the SIG trace's body; the stub has a free end.
    let stub = trace_from(&board, 13200, -3000);
    assert_eq!(ids(&report, "track_dangling"), vec![vec![stub.0]]);
    // K15.16: Java's contacts split GND (the vias meet their pads off the
    // pads' centres) and SIG (T1 ends on the middle of a trace); KiCad joins
    // items whose copper touches, so it has no split net. The scores are the
    // same under both rules.
    let java = self::report(&board, ReportRules::Java);
    let nets: Vec<&str> = java
        .unconnected_items
        .iter()
        .map(|u| {
            let d = u.description.as_str();
            &d[d.find('[').unwrap() + 1..d.find(']').unwrap()]
        })
        .collect();
    assert_eq!(nets, ["GND", "SIG"]);
    assert!(report.unconnected_items.is_empty());
    assert_eq!(report.quality_score, java.quality_score);
    assert_eq!(report.optimizer_score, java.optimizer_score);
}

#[test]
fn the_default_report_is_kicads_and_the_board_findings_stay_javas() {
    let board = board(JavaQuirks::JAVA);
    let mut checker = DesignRulesChecker::new(&board);
    assert_eq!(checker.report_rules(), ReportRules::Kicad);
    let findings = checker.check();
    assert_eq!(findings.clearance_violations.len(), 2);
    let kinds = |kind| {
        findings
            .unconnected_items
            .iter()
            .filter(|u| u.kind == kind)
            .count()
    };
    assert_eq!(kinds(UnconnectedKind::ViaDangling), 2);
    assert_eq!(kinds(UnconnectedKind::TrackDangling), 2);
}

#[test]
fn score_switch_leaves_out_only_the_inputs_same_net_pairs() {
    let java = board(JavaQuirks::JAVA);
    let fixed = board(
        JavaQuirks::JAVA
            .with("same_net_via_pad_violation", false)
            .unwrap(),
    );
    let routed = via_at(&fixed, 14900);
    let java_list = DesignRulesChecker::new(&java).all_clearance_violations();
    let fixed_list = DesignRulesChecker::new(&fixed).all_clearance_violations();
    assert_eq!(java_list.len(), 2);
    assert_eq!(fixed_list.len(), 1);
    let v = &fixed_list[0];
    assert!(v.first_item == routed || v.second_item == routed);
    assert_eq!(
        BoardStatistics::new(&java).clearance_violations.total_count,
        2
    );
    assert_eq!(
        BoardStatistics::new(&fixed)
            .clearance_violations
            .total_count,
        1
    );
    // The report's own filter does not depend on the switch.
    assert_eq!(
        ids(&report(&java, ReportRules::Kicad), "holeClearance"),
        ids(&report(&fixed, ReportRules::Kicad), "holeClearance")
    );
}

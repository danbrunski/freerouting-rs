//! K15.11: the DRC report's last differences from KiCad on the pcu board,
//! one small board per case.
//!
//! The boards are in `tests/data/kicad/pcu-k155/` (cut from the
//! power-control-unit board, `../power-control-unit` at `76163c7`,
//! `hardware/rev2.0/Power_Control_Unit_24V.kicad_pcb`, by the orchestrator
//! branch of 2026-10-21; each zone's outline is the cut's box, its fill
//! dropped; the outline is the box grown by 1 mm) and
//! `tests/data/kicad/pcu-k1511/`:
//!
//! - `a-via-in-pad`: U12 (HTSSOP-20, at 81.35 94.1) with the 8 BUS_GND
//!   thermal vias of its exposed pad and the In1.Cu BUS_GND plane. KiCad
//!   10.0.7: no `clearance`, `hole_clearance` or `via_dangling`.
//! - `b-pour-to-plane`: 6 V_BUS_24V vias in the F.Cu pour `EFUSE_VIN_1` and on
//!   the In2.Cu V_BUS_24V plane, no footprint. KiCad 10.0.7: no
//!   `via_dangling`.
//! - `c-stub`: R98 (0805 at 85.0125 61.54) with the two short F.Cu tracks of
//!   Net-(D11-A) whose ends lie inside its pad 2 but not at its centre, and
//!   one added track from the pad's centre to (84.1, 62.6), a stub. KiCad:
//!   1 `track_dangling`, the stub.
//! - `d-via-via`: U26 with the vias of Net-(U26-SS) and Net-(U26-RT), 0.1673
//!   mm apart as circles, 0.1249 mm as Java's octagons, under the Default
//!   class's 0.15 mm. KiCad: no `clearance`.
//! - `f-rule-clearance`: `d-via-via` with the Default class clearance raised
//!   to 0.2 mm and the pcu board's unconditional `.kicad_dru` rule
//!   `clearance (min 0.127mm)`. KiCad: no `clearance`; without the rule, 4
//!   (`clearance 0.2000 mm; actual 0.1673 mm`, once per copper layer).
//! - `e-npth-pads`: J13 (USB-C receptacle) of the pcu board, verbatim, with
//!   its two NPTH pegs, and the pcu board's copper and hole rules (`hole_clearance
//!   (min 0.254mm)`). KiCad: 4 `hole_clearance`, pads A1-B12 and A12-B1
//!   (USB_GND) at 0.1733 mm and A4-B9 and A9-B4 (Net-(U24-OUT)) at 0.2063 mm.
//! - `g-track-ends`: on N1, two collinear 1 mm segments between pads P1 and
//!   P2 (one trace on the board) and a 0.2 mm diagonal from their junction to
//!   the first one's copper edge, joined there to a 0.2 mm segment from pad
//!   P3 (the pcu board's Net-(U11-ACUV) at 118.4 50.6); on N2, a 0.5 mm
//!   segment between pads P4 and P5 and a 0.3 mm segment of 0.125 mm whose
//!   both ends lie on its copper and nowhere else (FE_HGATE's 0.3 mm segment).
//!   KiCad: 1 `track_dangling`, the 0.125 mm segment.
//!
//! The KiCad counts are `kicad-cli pcb drc --severity-all --format json
//! --refill-zones` (KiCad 10.0.7) on each board. Each test also pins Java's
//! report (`ReportRules::Java`), which is what `-drc` reported before K15.5,
//! and names what `-drc` reported before K15.11.

use std::collections::BTreeMap;
use std::path::PathBuf;

use freerouting::board::RoutingBoard;
use freerouting::drc::{DesignRulesChecker, DrcReport, DrcRunOptions, ReportRules, run_drc};
use freerouting::io::kicad::{ReadOptions, read_kicad_input};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::settings::ResolveContext;

fn case(dir: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/kicad")
        .join(dir)
        .join(format!("{name}.kicad_pcb"))
}

/// The board of case `name` as `-drc` loads it, and its `-drc` report.
fn drc(dir: &str, name: &str) -> (RoutingBoard, DrcReport) {
    let run = run_drc(&DrcRunOptions::new(case(dir, name))).expect("the case loads");
    (run.board, run.report)
}

fn counts(report: &DrcReport) -> BTreeMap<String, usize> {
    let mut result = BTreeMap::new();
    for violation in &report.violations {
        *result.entry(violation.violation_type.clone()).or_default() += 1;
    }
    result
}

fn java_counts(board: &RoutingBoard) -> BTreeMap<String, usize> {
    let mut checker = DesignRulesChecker::new(board);
    checker.set_report_rules(ReportRules::Java);
    checker.set_check_zone_islands(false);
    counts(&checker.generate_report("case", "mm"))
}

fn count(counts: &BTreeMap<String, usize>, kind: &str) -> usize {
    counts.get(kind).copied().unwrap_or(0)
}

fn of_type<'a>(report: &'a DrcReport, kind: &str) -> Vec<&'a str> {
    report
        .violations
        .iter()
        .filter(|v| v.violation_type == kind)
        .map(|v| v.description.as_str())
        .collect()
}

#[test]
fn a_via_in_a_dropped_pour_and_its_plane_is_connected() {
    let (board, report) = drc("pcu-k155", "b-pour-to-plane");
    // Java's reader leaves the F.Cu pour out: its net has a power-layer plane.
    assert_eq!(board.dropped_pours().len(), 1);
    assert_eq!(board.dropped_pours()[0].layer, 0);
    // Java: one contact each, the plane. Before K15.11 `-drc` counted the
    // same 6 (the vias met copper on In2.Cu only).
    assert_eq!(count(&java_counts(&board), "via_dangling"), 6);
    // KiCad 10.0.7: none; the pour is copper of V_BUS_24V on F.Cu.
    assert_eq!(count(&counts(&report), "via_dangling"), 0, "{report:?}");
}

#[test]
fn dropped_pours_and_the_report_rules_survive_a_deep_copy() {
    let (board, _) = drc("pcu-k155", "b-pour-to-plane");
    let copy = board.deep_copy();
    assert_eq!(copy.dropped_pours().len(), 1);
    let (board, _) = drc("pcu-k1511", "e-npth-pads");
    let holes = &board.kicad_report_rules().holes;
    assert_eq!(holes.len(), 2, "J13's two NPTH pegs");
    assert!(holes.iter().all(|h| !h.plated && h.reference == "J13"));
    assert_eq!(
        board.deep_copy().kicad_report_rules(),
        board.kicad_report_rules()
    );
}

#[test]
fn a_pair_that_fails_as_octagons_passes_as_copper() {
    let (board, report) = drc("pcu-k155", "d-via-via");
    // Java's octagons put the vias 0.1249 mm apart, under the 0.15 mm, on
    // each of the 4 layers; `-drc` before K15.11 reported the same 4.
    assert_eq!(count(&java_counts(&board), "holeClearance"), 4);
    let kicad = counts(&report);
    // KiCad 10.0.7: no clearance finding; the circles are 0.1673 mm apart.
    assert_eq!(count(&kicad, "clearance"), 0, "{kicad:?}");
    assert_eq!(count(&kicad, "holeClearance"), 0, "{kicad:?}");
}

#[test]
fn a_true_violation_is_kept_once_per_layer_with_its_own_distance() {
    // `f-rule-clearance` without its `.kicad_dru`: the Default class's 0.2 mm.
    let src = case("pcu-k1511", "f-rule-clearance");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("k1511-f-no-rule");
    std::fs::create_dir_all(&dir).unwrap();
    let pcb = dir.join("f-no-rule.kicad_pcb");
    std::fs::copy(&src, &pcb).unwrap();
    std::fs::copy(
        src.with_extension("kicad_pro"),
        pcb.with_extension("kicad_pro"),
    )
    .unwrap();
    let run = run_drc(&DrcRunOptions::new(&pcb)).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(run.board.kicad_report_rules().clearance_um, None);
    assert_eq!(count(&java_counts(&run.board), "holeClearance"), 4);
    // KiCad 10.0.7: 4 `clearance` (one per copper layer), "clearance 0.2000
    // mm; actual 0.1673 mm".
    let clearance = of_type(&run.report, "clearance");
    assert_eq!(clearance.len(), 4, "{clearance:?}");
    for description in clearance {
        assert!(
            description.contains("expected: 0.2000 mm, actual: 0.1673 mm"),
            "{description}"
        );
    }
    assert!(of_type(&run.report, "holeClearance").is_empty());
}

#[test]
fn an_unconditional_rule_under_the_class_clearance_is_what_the_report_counts() {
    let (board, report) = drc("pcu-k1511", "f-rule-clearance");
    assert_eq!(board.kicad_report_rules().clearance_um, Some(127.0));
    // Java (and `-drc` before K15.11): the class's 0.2 mm, on 4 layers.
    assert_eq!(count(&java_counts(&board), "holeClearance"), 4);
    // KiCad 10.0.7: the rule's 0.127 mm replaces the class's 0.2 mm; the
    // vias are 0.1673 mm apart.
    assert_eq!(count(&counts(&report), "clearance"), 0, "{report:?}");

    // The routing job keeps the larger value: the class's 0.2 mm in the
    // matrix, the rule as the board-wide floor (K15.3); only the report takes
    // the rule's.
    let options = ReadOptions {
        apply_design_rules: true,
        ..ReadOptions::default()
    };
    let read = read_kicad_input(&case("pcu-k1511", "f-rule-clearance"), options).unwrap();
    let mut input = JobInput::new(Vec::new(), "f-rule-clearance");
    input.set_kicad(read);
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
    assert_eq!(prepared.patch.board_minimums.clearance_um, Some(127.0));
    let via = prepared.board.items().find(|i| i.is_via()).unwrap();
    let class = via.clearance_class();
    let matrix = prepared
        .board
        .rules
        .clearance_matrix
        .get_value(class, class, 0, false);
    assert_eq!(matrix, 2000, "0.2 mm in 0.1 um units");
    assert_eq!(
        prepared.board.kicad_report_rules().clearance_um,
        Some(127.0)
    );
    let mut checker = DesignRulesChecker::new(&prepared.board);
    let job_report = checker.generate_report("f-rule-clearance", "mm");
    assert_eq!(count(&counts(&job_report), "clearance"), 0);
}

#[test]
fn a_pad_of_another_net_near_an_npth_is_a_hole_clearance() {
    let (board, report) = drc("pcu-k1511", "e-npth-pads");
    // Java and `-drc` before K15.11: no copper-to-hole check for pads.
    let java = java_counts(&board);
    assert_eq!(count(&java, "hole_clearance"), 0, "{java:?}");
    // KiCad 10.0.7: the 4 pads beside J13's NPTH pegs.
    let mut holes = of_type(&report, "hole_clearance");
    holes.sort_unstable();
    assert_eq!(
        holes,
        [
            "Hole clearance violation between Pin [/USB PD Source Circuit/USB_GND] and NPTH hole of \
             J13 (expected: 0.2540 mm, actual: 0.1733 mm)",
            "Hole clearance violation between Pin [/USB PD Source Circuit/USB_GND] and NPTH hole of \
             J13 (expected: 0.2540 mm, actual: 0.1733 mm)",
            "Hole clearance violation between Pin [Net-(U24-OUT)] and NPTH hole of J13 (expected: \
             0.2540 mm, actual: 0.2063 mm)",
            "Hole clearance violation between Pin [Net-(U24-OUT)] and NPTH hole of J13 (expected: \
             0.2540 mm, actual: 0.2063 mm)",
        ]
    );
    assert_eq!(count(&counts(&report), "clearance"), 0, "{report:?}");

    // A routing job's DRC report counts the same.
    let read = read_kicad_input(&case("pcu-k1511", "e-npth-pads"), ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), "e-npth-pads");
    input.set_kicad(read);
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
    let mut checker = DesignRulesChecker::new(&prepared.board);
    let job_report = checker.generate_report("e-npth-pads", "mm");
    assert_eq!(count(&counts(&job_report), "hole_clearance"), 4);
}

#[test]
fn a_short_segment_on_one_trace_dangles_and_a_junction_on_a_joined_trace_does_not() {
    let (board, report) = drc("pcu-k1511", "g-track-ends");
    // `-drc` before K15.11 (an end point on copper of its net): none.
    // KiCad 10.0.7: the 0.125 mm segment of N2 only.
    let dangling: Vec<_> = report
        .violations
        .iter()
        .filter(|v| v.violation_type == "track_dangling")
        .collect();
    assert_eq!(dangling.len(), 1, "{dangling:?}");
    assert!(
        dangling[0].items[0]
            .description
            .contains("[N2] on F.Cu, length 0.1250 mm"),
        "{:?}",
        dangling[0].items
    );
    // The two 1 mm segments of N1 are one trace on the board.
    let n1 = board.rules.nets.get_by_name("N1", 1).unwrap().net_number;
    let wide: Vec<_> = board
        .items()
        .filter_map(|i| i.as_trace().filter(|_| i.contains_net(n1)))
        .map(|t| (t.half_width, t.corner_count()))
        .filter(|(half_width, _)| *half_width == 5000)
        .collect();
    assert_eq!(wide, [(5000, 2)], "one 2 mm trace of two corners");
}

#[test]
fn same_net_vias_in_a_pad_are_no_finding() {
    let (board, report) = drc("pcu-k155", "a-via-in-pad");
    let java = java_counts(&board);
    // Java: 8 via-pad pairs and one via pair 0.1 mm apart, the vias as tails.
    assert_eq!(count(&java, "holeClearance"), 9, "{java:?}");
    assert_eq!(count(&java, "via_dangling"), 8, "{java:?}");
    let kicad = counts(&report);
    for kind in [
        "clearance",
        "holeClearance",
        "hole_clearance",
        "via_dangling",
    ] {
        assert_eq!(count(&kicad, kind), 0, "{kind}: {kicad:?}");
    }
}

#[test]
fn a_stub_that_represents_a_split_group_is_reported() {
    let (board, report) = drc("pcu-k155", "c-stub");
    // Java: 1 of the three traces (their ends are off the pad's centre or
    // free; the other two represent groups of the split net, which Java
    // leaves out). `-drc` before K15.11 reported none: the stub represents a
    // group too.
    assert_eq!(count(&java_counts(&board), "track_dangling"), 1);
    // KiCad 10.0.7: the stub.
    let dangling: Vec<_> = report
        .violations
        .iter()
        .filter(|v| v.violation_type == "track_dangling")
        .collect();
    assert_eq!(dangling.len(), 1);
    assert!(
        dangling[0].items[0]
            .description
            .contains("[Net-(D11-A)] on F.Cu, length 1.1024 mm"),
        "{:?}",
        dangling[0].items
    );
}

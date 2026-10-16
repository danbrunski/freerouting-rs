//! The infeasible-connection diagnostic (Q03) on the synthetic fixtures in
//! `crates/freerouting/tests/data/quality/` and on `canary-pwr`. They live
//! there, not in `fixtures/`, because every DSN under `fixtures/` needs a Java
//! load reference (`tests/reference_load.rs`).

use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::drc::load_design;
use freerouting::quality::feasibility::report::sha256_hex;
use freerouting::quality::feasibility::{
    FeasibilityOptions, FeasibilityReport, PinFeasibility, StartModel, Verdict, check_board,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load(relative: &str) -> (Vec<u8>, RoutingBoard) {
    let bytes = std::fs::read(repo_root().join(relative)).expect("fixture");
    let board = load_design(&bytes).expect("board loads");
    (bytes, board)
}

fn pin<'a>(report: &'a FeasibilityReport, component: &str, pin: &str) -> &'a PinFeasibility {
    report
        .pins
        .iter()
        .find(|p| p.component == component && p.pin == pin)
        .unwrap_or_else(|| panic!("{component}-{pin} is not in the report"))
}

fn mm(report: &FeasibilityReport, value: i64) -> f64 {
    value as f64 / report.units_per_mm
}

/// The raster tolerance δ = s·√2 in mm.
fn delta_mm(report: &FeasibilityReport) -> f64 {
    mm(report, i64::from(report.step)) * std::f64::consts::SQRT_2
}

const QFN: &str = "crates/freerouting/tests/data/quality/q03-qfn-0p5.dsn";
const SOIC: &str = "crates/freerouting/tests/data/quality/q03-soic-boundary.dsn";
const CANARY: &str = "fixtures/canary-pwr/canary-pwr-2026-10-15-1747-unpoured.dsn";

/// Q03.4: the JSON report of the QFN fixture equals the committed golden
/// file. `FEASIBILITY_UPDATE_GOLDEN=1` rewrites it.
#[test]
fn qfn_fixture_json_matches_golden() {
    let (bytes, board) = load(QFN);
    let report = check_board(&board, &FeasibilityOptions::default());
    let json = report.to_json(&sha256_hex(&bytes)).to_pretty_string() + "\n";
    let golden = repo_root().join("crates/freerouting/tests/data/q03-qfn-0p5.feasibility.json");
    if std::env::var_os("FEASIBILITY_UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &json).expect("write golden");
    }
    let expected = std::fs::read_to_string(&golden).expect("golden file");
    assert_eq!(
        json, expected,
        "rerun with FEASIBILITY_UPDATE_GOLDEN=1 if the change is intended"
    );
    assert!(json.contains("\"schema\": \"freerouting-feasibility/1\""));
}

/// Q03.6: a 0.5 mm pitch QFN row (canary-pwr U1's 0.25 × 0.7 mm pads) and
/// a 1.5 mm net. At the pin centre the neighbour pad edges are 0.375 mm
/// away, so `C = 2·(0.375 − c)`.
#[test]
fn qfn_pin_is_infeasible_with_the_hand_value() {
    let (_, board) = load(QFN);
    let report = check_board(&board, &FeasibilityOptions::default());
    assert_eq!(report.candidates, 1);
    let p = pin(&report, "U1", "5");
    assert_eq!(p.verdict, Verdict::Infeasible);
    assert_eq!(mm(&report, p.width), 1.5);
    let c = board.rules.clearance_matrix.get_value(
        board
            .rules
            .net_class_of(p.net_number)
            .unwrap()
            .trace_clearance_class(),
        board.item(p.near[0].id).unwrap().clearance_class(),
        0,
        false,
    );
    let hand = 2.0 * (0.375 - mm(&report, i64::from(c)));
    let corridor = mm(&report, p.corridor.expect("corridor"));
    assert!(
        (corridor - hand).abs() <= delta_mm(&report),
        "C = {corridor}, hand {hand}"
    );
    let near: Vec<&str> = p.near.iter().filter_map(|n| n.pin.as_deref()).collect();
    assert_eq!(near.len(), 2, "{:?}", p.near);
    assert!(near.contains(&"4") && near.contains(&"6"), "{near:?}");
    let line = p.text_line(report.units_per_mm).expect("text line");
    assert!(
        line.starts_with("cannot route PWR to U1-5 at width 1.500 mm: widest corridor"),
        "{line}"
    );
}

/// Q03.6: the SOIC boundary case (canary-pwr U7's 1.27 mm pitch and 0.6 mm
/// pads): the 1.5 mm net fails by 0.06 mm of width, the 1.40 mm net
/// passes. Hand value `2·(0.97 − 0.25) = 1.44` mm.
#[test]
fn soic_boundary_case() {
    let (_, board) = load(SOIC);
    let report = check_board(&board, &FeasibilityOptions::default());
    let wide = pin(&report, "U7", "3");
    assert_eq!(wide.verdict, Verdict::Infeasible);
    let corridor = mm(&report, wide.corridor.expect("corridor"));
    assert!((corridor - 1.44).abs() <= 0.02, "C = {corridor}");
    let narrower = pin(&report, "U7", "5");
    assert_eq!(mm(&report, narrower.width), 1.4);
    assert_eq!(narrower.verdict, Verdict::Passable);
    assert!(narrower.neck.is_none());
}

/// Q03.5: the QFN pin becomes routable with a neck of at most its pad
/// length plus 0.5 mm.
#[test]
fn qfn_neck_ladder() {
    let (_, board) = load(QFN);
    let report = check_board(&board, &FeasibilityOptions::default());
    let neck = pin(&report, "U1", "5").neck.as_ref().expect("neck ladder");
    let needed = neck.needed_mm.expect("a neck helps");
    assert!(needed <= 0.7 + 0.5, "neck needed {needed} mm");
    // The neck is Java's getTraceNeckdownHalfwidth: (int)(0.5·2500 − 1).
    assert_eq!(neck.width[0], 2 * 1249);
    assert_eq!(neck.rungs.last().unwrap().verdict, Verdict::Passable);
}

/// A pin of a 1 mm net inside a keepout ring on both layers.
const ENCLOSED_DSN: &str = r#"(pcb enclosed.dsn
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal))
    (layer B.Cu (type signal))
    (boundary (path pcb 0  0 0  20000 0  20000 20000  0 20000  0 0))
    (via VIA600)
    (rule (width 200) (clearance 200))
    (keepout "" (polygon F.Cu 0  7000 7000  13000 7000  13000 7500  7000 7500))
    (keepout "" (polygon F.Cu 0  7000 12500  13000 12500  13000 13000  7000 13000))
    (keepout "" (polygon F.Cu 0  7000 7000  7500 7000  7500 13000  7000 13000))
    (keepout "" (polygon F.Cu 0  12500 7000  13000 7000  13000 13000  12500 13000))
    (keepout "" (polygon B.Cu 0  7000 7000  13000 7000  13000 7500  7000 7500))
    (keepout "" (polygon B.Cu 0  7000 12500  13000 12500  13000 13000  7000 13000))
    (keepout "" (polygon B.Cu 0  7000 7000  7500 7000  7500 13000  7000 13000))
    (keepout "" (polygon B.Cu 0  12500 7000  13000 7000  13000 13000  12500 13000))
  )
  (placement
    (component PAD
      (place P 10000 10000 front 0)
      (place J 3000 3000 front 0)
    )
  )
  (library
    (image PAD (pin SQ 1 0 0))
    (padstack SQ (shape (rect F.Cu -300 -300 300 300)) (attach off))
    (padstack VIA600 (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
    (net A (pins P-1 J-1))
    (class WIDE A (circuit (use_via VIA600)) (rule (width 1000) (clearance 200)))
  )
)
"#;

/// Q03.5: no neck helps an enclosed pin; Q03.3 on a board: the pin is
/// infeasible.
#[test]
fn enclosed_pin_has_no_neck() {
    let board = load_design(ENCLOSED_DSN.as_bytes()).expect("board");
    let report = check_board(&board, &FeasibilityOptions::default());
    let p = pin(&report, "P", "1");
    assert_eq!(p.verdict, Verdict::Infeasible);
    assert!(p.corridor.expect("corridor") < 0);
    let neck = p.neck.as_ref().expect("neck ladder");
    assert_eq!(neck.needed_mm, None);
    assert_eq!(neck.rungs.len(), 5);
    assert!(
        p.text_line(report.units_per_mm)
            .unwrap()
            .contains("no neck up to 4 mm helps")
    );
}

/// Q03.8: the per-pin checks on all cores and sequentially give equal
/// reports, and two runs give byte-identical JSON.
#[test]
fn parallel_and_sequential_reports_are_equal() {
    for fixture in [QFN, SOIC] {
        let (bytes, board) = load(fixture);
        let options = FeasibilityOptions {
            all_pins: true,
            ..FeasibilityOptions::default()
        };
        let parallel = check_board(&board, &options);
        let sequential = check_board(
            &board,
            &FeasibilityOptions {
                parallel: false,
                ..options.clone()
            },
        );
        assert_eq!(parallel.pins, sequential.pins, "{fixture}");
        let again = check_board(&board, &options);
        let sha = sha256_hex(&bytes);
        assert_eq!(
            parallel.to_json(&sha).to_pretty_string(),
            again.to_json(&sha).to_pretty_string()
        );
    }
}

/// canary-09's 17 pins that no 1.5 mm trace can reach (canary-pwr, the
/// 17:47 export; Power_High and Switch_Node are 1.5 mm, clearance 0.25 mm).
const CANARY_INFEASIBLE: [(&str, &str); 17] = [
    ("U1", "22"),
    ("U1", "24"),
    ("U1", "30"),
    ("U1", "31"),
    ("U1", "34"),
    ("U2", "1"),
    ("U2", "8"),
    ("U2", "10"),
    ("U2", "12"),
    ("U3", "8"),
    ("U3", "10"),
    ("U7", "3"),
    ("U7", "10"),
    ("Q2", "3"),
    ("Q3", "1"),
    ("Q3", "2"),
    ("Q3", "3"),
];

/// Q03.6 and Q03.8 on canary-pwr. With the centre start model (canary-09's
/// model) the 17 pins are infeasible and U7 pin 3 matches the hand value
/// `2·(0.97 − 0.25) = 1.44` mm; with the default model (a trace may end
/// anywhere on the pin) Q3 pins 1 and 2 are passable, because a 1.5 mm trace
/// fits at their outer ends next to pin 2 of the same net, and U7 pin 3 is
/// set by the rounded pad corners at the pad end:
/// `2·(√(0.15² + 1.12²) − 0.15 − 0.25) = 1.460` mm. The three Default-class
/// pins are never infeasible. The parallel and sequential runs agree. About a
/// minute and a half in the dev profile.
#[test]
#[ignore = "about 90 s in the dev profile; run with --ignored (seconds in release)"]
fn canary_pwr_infeasible_pins() {
    let (bytes, board) = load(CANARY);
    let centre = check_board(
        &board,
        &FeasibilityOptions {
            start: StartModel::PinCentre,
            ..FeasibilityOptions::default()
        },
    );
    let shape = check_board(&board, &FeasibilityOptions::default());
    for (component, number) in CANARY_INFEASIBLE {
        let p = pin(&centre, component, number);
        assert_eq!(
            p.verdict,
            Verdict::Infeasible,
            "centre: {component}-{number}: {p:?}"
        );
        let p = pin(&shape, component, number);
        let expected = if component == "Q3" && number != "3" {
            Verdict::Passable
        } else {
            Verdict::Infeasible
        };
        assert_eq!(p.verdict, expected, "shape: {component}-{number}: {p:?}");
    }
    for report in [&centre, &shape] {
        for (component, number) in [("U1", "21"), ("U3", "9"), ("Q7", "3")] {
            let flagged = report.pins.iter().any(|p| {
                p.component == component && p.pin == number && p.verdict == Verdict::Infeasible
            });
            assert!(
                !flagged,
                "{component}-{number} is in the Default class here"
            );
        }
    }
    let u7 = |report: &FeasibilityReport| mm(report, pin(report, "U7", "3").corridor.unwrap());
    assert!(
        (u7(&centre) - 1.44).abs() <= 0.02,
        "U7-3 centre: C = {}",
        u7(&centre)
    );
    assert!(
        (u7(&shape) - 1.460).abs() <= 0.02,
        "U7-3 shape: C = {}",
        u7(&shape)
    );
    let sequential = check_board(
        &board,
        &FeasibilityOptions {
            parallel: false,
            ..FeasibilityOptions::default()
        },
    );
    let sha = sha256_hex(&bytes);
    assert_eq!(
        shape.to_json(&sha).to_pretty_string(),
        sequential.to_json(&sha).to_pretty_string()
    );
}

//! `freerouting-dev failed-vs-human` (F03.2) on a synthetic QFN board: a
//! 1.5 mm net the Q03 diagnostic calls infeasible that the "human" drew
//! anyway, an ordinary net the human routed, and a net the human left open.

use std::path::{Path, PathBuf};

use freerouting_dev::failed_vs_human::{FailedConnection, Report, analyze};
use freerouting_dev::ses_diff::parse_session;
use freerouting_dev::{load_board, read_text};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn report(feasibility: bool) -> Report {
    let unrouted = load_board(&data("fvh-qfn.dsn")).unwrap();
    let human = load_board(&data("fvh-qfn-human.dsn")).unwrap();
    let session = parse_session(&read_text(&data("fvh-empty.ses")).unwrap()).unwrap();
    let options = freerouting::quality::feasibility::FeasibilityOptions::default();
    analyze(&unrouted, &session, &human, feasibility.then_some(&options))
}

fn net<'a>(report: &'a Report, name: &str) -> &'a FailedConnection {
    report
        .connections
        .iter()
        .find(|c| c.net == name)
        .unwrap_or_else(|| panic!("no unrouted connection on {name}: {}", report.to_text()))
}

#[test]
fn every_unrouted_connection_has_a_human_counterpart_or_no_human_route() {
    let r = report(true);
    assert_eq!(r.connections.len(), 3, "{}", r.to_text());
    for c in &r.connections {
        assert!(
            c.tags.contains(&"router gap") ^ c.tags.contains(&"no human route"),
            "exactly one of the two: {c:?}"
        );
        assert_eq!(c.human.routed, c.tags.contains(&"router gap"));
    }
}

#[test]
fn both_tags_appear_and_infeasible_comes_from_q03() {
    let r = report(true);
    let pwr = net(&r, "PWR");
    assert_eq!(pwr.tags, ["infeasible", "router gap"]);
    assert_eq!(pwr.infeasible_pins.len(), 1);
    assert_eq!(
        pwr.infeasible_pins[0].0,
        ("U1".to_string(), "5".to_string())
    );
    assert_eq!(pwr.infeasible_pins[0].1, 1.5);
    // The human's copper: one 1.5 mm track on F.Cu, no via.
    assert_eq!((pwr.human.tracks, pwr.human.vias), (1, 0));
    assert_eq!(pwr.human.layers, ["F.Cu"]);
    assert!((pwr.human.min_width_mm - 1.5).abs() < 1e-9);
    assert!(
        (pwr.human.length_mm - 6.0).abs() < 0.01,
        "{}",
        pwr.human.length_mm
    );
    let gnd = net(&r, "GND");
    assert_eq!(gnd.tags, ["router gap"]);
    assert_eq!(gnd.human.tracks, 1);
    assert!((gnd.human.min_width_mm - 0.2).abs() < 1e-9);
    let n1 = net(&r, "N1");
    assert_eq!(n1.tags, ["no human route"]);
    assert_eq!(n1.human.tracks, 0);
    let counts = r.counts();
    assert_eq!(counts["infeasible"], 1);
    assert_eq!(counts["router gap"], 2);
    assert_eq!(counts["no human route"], 1);
}

#[test]
fn without_the_diagnostic_there_is_no_infeasible_tag() {
    let r = report(false);
    assert!(!r.counts().contains_key("infeasible"));
    assert_eq!(net(&r, "PWR").tags, ["router gap"]);
}

#[test]
fn headroom_is_the_gap_to_foreign_copper_minus_clearance() {
    let r = report(true);
    let pwr = net(&r, "PWR");
    // The 1.5 mm track runs down x = 10 mm from the centre of pad 5: its
    // axis is 0.375 mm from the edge of the neighbouring pads 4 and 6, so the
    // gap is 0.375 - 0.75 = -0.375 mm, and the Power_High class asks for
    // 0.25 mm of clearance: headroom -0.625 mm.
    let h = pwr.human.headroom_mm.expect("headroom");
    assert!((h + 0.625).abs() < 0.005, "headroom {h}");
    // The 0.2 mm GND track starts on the exposed pad's centre, 1.825 mm
    // above the PWR track's end: 1.825 - 0.75 - 0.1 - 0.25 = 0.725 mm.
    let g = net(&r, "GND").human.headroom_mm.expect("headroom");
    assert!((g - 0.725).abs() < 0.005, "headroom {g}");
    assert!(
        r.to_text().contains("net PWR: J1-1 -> U1-5"),
        "{}",
        r.to_text()
    );
}

#[test]
fn text_and_json_reports_name_every_connection() {
    let r = report(true);
    let text = r.to_text();
    assert!(
        text.contains("3 unrouted connection(s): router gap 2, infeasible 1, no human route 1")
    );
    assert!(text.contains("infeasible: no 1.500 mm trace reaches U1-5 (Q03)"));
    let json = r.to_json().to_pretty_string();
    for needle in [
        "\"unrouted_connections\": 3",
        "\"router gap\"",
        "\"no human route\"",
        "\"infeasible\"",
    ] {
        assert!(json.contains(needle), "{needle} in {json}");
    }
}

#[test]
fn the_command_prints_the_report_and_writes_json() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fvh.json");
    let _ = std::fs::remove_file(&out);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_freerouting-dev"))
        .arg("failed-vs-human")
        .arg(data("fvh-qfn.dsn"))
        .arg(data("fvh-empty.ses"))
        .arg(data("fvh-qfn-human.dsn"))
        .arg("--json")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("router gap 2, infeasible 1"));
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("\"connections\"")
    );
    let usage = std::process::Command::new(env!("CARGO_BIN_EXE_freerouting-dev"))
        .args(["failed-vs-human", "only-one-argument"])
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
}

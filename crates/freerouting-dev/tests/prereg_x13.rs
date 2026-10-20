//! Pins X13's registration (`docs/roadmap/prereg/X13.json`, X13.1): the head-to-head rerun of
//! freerouting-rs at main a25c82f (`fr-legacy`, `fr-enhanced-sweep`, `fr-plugin`) against
//! KiCadRoutingTools 0486e799 in its strict mode (`krt-strict`), every board graded by KiCad
//! 10.0.7's DRC under its original rules. A report, not a keep rule.
//!
//! Registered 2026-10-20 before any route of any arm. Rows are kept to what routes quickly
//! (decided 2026-10-20: a measurement takes a few minutes in total): pic_programmer,
//! sonde_xilinx, interf_u (`fr-legacy` only, the check against X01's Java arm) and canary-pwr
//! at canary 2a7e070b. A later change goes in the file's `amendments` with the reason and
//! updates the hash here in the same change; amendment 1 is expected to replace
//! sonde_xilinx's source hash with its prepared board's hash before the first engine run.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/X13.json";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

#[test]
fn the_x13_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "X13-head-to-head-rerun");
    assert_eq!(prereg.names.base, "fr-enhanced-sweep");
    assert_eq!(prereg.names.cand, "krt-strict");
    let boards: Vec<&str> = prereg.boards.keys().map(String::as_str).collect();
    assert_eq!(
        boards,
        [
            "canary-pwr-2a7e070b",
            "interf_u",
            "pic_programmer",
            "sonde_xilinx"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some("tools/bench/sets/x13-h2h.txt"));
    assert!(repo().join("tools/bench/sets/x13-h2h.txt").is_file());
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..5],
        [
            "via_in_pad",
            "below_minimum",
            "inner_tracks",
            "drc_real",
            "kicad_unconnected"
        ]
    );
}

#[test]
fn every_arm_names_its_rows_and_the_registered_commits() {
    let json = Json::parse(&text()).unwrap();
    let arms = json.get("arms").unwrap();
    let rows = |arm: &str| -> Vec<String> {
        match arms.get(arm).and_then(|a| a.get("rows")) {
            Some(Json::Array(list)) => list
                .iter()
                .map(|r| r.as_str().unwrap().to_string())
                .collect(),
            other => panic!("arm {arm} has no rows: {other:?}"),
        }
    };
    assert_eq!(
        rows("fr-enhanced-sweep"),
        ["pic_programmer", "sonde_xilinx"]
    );
    assert_eq!(
        rows("fr-legacy"),
        ["pic_programmer", "interf_u", "sonde_xilinx"]
    );
    assert_eq!(rows("fr-plugin"), ["canary-pwr-2a7e070b"]);
    assert_eq!(
        rows("krt-strict"),
        ["pic_programmer", "sonde_xilinx", "canary-pwr-2a7e070b"]
    );
    let command = |arm: &str| {
        arms.get(arm)
            .and_then(|a| a.get("command"))
            .and_then(Json::as_str)
            .unwrap()
            .to_string()
    };
    assert!(command("krt-strict").contains("--escalation off"));
    assert!(command("krt-strict").contains("--no-power-tap-neckdown"));
    assert!(command("fr-legacy").contains("--router.profile=legacy"));
    assert!(command("fr-enhanced-sweep").contains("--router.autorouter.sweep=free"));
    let engine = json.get("engine").and_then(Json::as_str).unwrap();
    assert!(engine.contains("a25c82f24c90af5edfc75601ed96e00605ab47b9"));
    assert!(engine.contains("0486e799144050d4e826f71bb31acdc7275af2a2"));
}

#[test]
fn the_x13_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "10733a3bad3b6ace886e6bbe0a0baa31e0f2a870c563fab42a275f02ebe18049",
        "docs/roadmap/prereg/X13.json changed: record the change in its amendments"
    );
}

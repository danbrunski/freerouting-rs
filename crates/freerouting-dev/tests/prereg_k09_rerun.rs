//! Pins K09.9's rerun registration (`docs/roadmap/prereg/K09.9-rerun.json`):
//! whether the plugin's `enhanced` defaults add `-mp 1`, Q10's sweep or the
//! rest of canary's CLI flow on top of today's engine `enhanced`
//! (`power_layer_traces = none`, `neck_width_um = 300`,
//! `via_clearance = net_class`), judged by N02's totals rule on canary-pwr and
//! K09's three KiCad demos through the plugin's own headless path.
//!
//! Registered 2026-10-18 before any routing number as
//! f9899ad1c3db7669c4e53563b0bdedb3d6e47e3daaddd9b43871c258b2893dac. A later
//! change goes in the file's `amendments` with the reason and updates the hash
//! here in the same change. Amendment 1 (after the runs, editorial: a name
//! removed from the procedure prose), now
//! 4132300c2a1e60e1b1e7be9507113b141f08b3d3279d100bef4bb2ce53e0b36b. `K09.json` (the first round) stays as registered.
//!
//! The judged boards are outside the repository (the canary repo and KiCad's
//! demos), so their hashes are checked by the run; the report row's DSN is in
//! the repository and is checked here.
use std::path::{Path, PathBuf};

use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::gate_totals::Accept;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/K09.9-rerun.json";
const DSN45: &str = "crates/freerouting/tests/data/canary-pwr-n05/plugin-b1994423.dsn";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

#[test]
fn the_rerun_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "K09.9-rerun-plugin-enhanced-defaults");
    assert_eq!(prereg.names.base, "plugin-default");
    assert_eq!(prereg.names.cand, "+mp1+sweep");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "+cli-flags",
            "+cli-flow",
            "+mp1",
            "+mp1+sweep",
            "+sweep",
            "plugin-default"
        ]
    );
    assert_eq!(prereg.boards.len(), 5);
    let totals = prereg.totals.as_ref().expect("a totals block");
    assert_eq!(totals.accept, Accept::Improve);
    assert_eq!(totals.signal, ["kicad_unconnected"]);
    let set_noise: f64 = totals.noise.values().map(|b| b * b).sum::<f64>().sqrt();
    assert!((set_noise - 17.14).abs() < 0.01, "{set_noise}");
    let roles: Vec<(&str, Role)> = prereg
        .specs
        .iter()
        .map(|s| (s.name.as_str(), s.role))
        .collect();
    assert_eq!(
        &roles[..5],
        [
            ("via_in_pad", Role::Hard),
            ("below_minimum", Role::Hard),
            ("power_layer_traces", Role::Hard),
            ("clearance", Role::Guard),
            ("kicad_unconnected", Role::Score),
        ]
    );
    assert_eq!(prereg.amendments, 1);
}

/// The baseline is today's plugin default (no option beyond the profile), and
/// each candidate's options are exactly what its name says.
#[test]
fn the_arms_options_are_the_registered_ones() {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let json = Json::parse(&text).unwrap();
    let keys = |arm: &str| -> Vec<String> {
        let Some(Json::Object(o)) = json.path(&format!("arms.{arm}.options")) else {
            panic!("arms.{arm}.options")
        };
        o.keys().cloned().collect()
    };
    assert_eq!(keys("plugin-default"), ["profile", "schema"]);
    assert_eq!(keys("+mp1"), ["max_passes", "profile", "schema"]);
    assert_eq!(
        keys("+cli-flow"),
        [
            "drop_pour_nets",
            "fixed_pour_classes",
            "max_passes",
            "profile",
            "schema",
            "start_ripup_costs",
            "sweep",
            "sweep_passes"
        ]
    );
}

#[test]
fn the_rerun_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "4132300c2a1e60e1b1e7be9507113b141f08b3d3279d100bef4bb2ce53e0b36b"
    );
}

#[test]
fn the_report_rows_dsn_hashes_as_registered() {
    let bytes = std::fs::read(repo().join(DSN45)).unwrap();
    assert_eq!(
        registration()
            .boards
            .get("canary-pwr-dsn45")
            .map(String::as_str),
        Some(freerouting::quality::feasibility::report::sha256_hex(&bytes).as_str())
    );
}

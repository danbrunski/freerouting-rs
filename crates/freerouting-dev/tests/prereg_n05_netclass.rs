//! Pins N05.3's third via-clearance registration
//! (`docs/roadmap/prereg/N05-via-clearance-netclass.json`):
//! `router.via_clearance = net_class` under profile `enhanced`, judged by N02's
//! totals rule with `accept: no-regress`, whose clearance clause counts only
//! KiCad's net-class-rule violations (`netclass_via_clearance_errors`,
//! `tools/kicad-oracle/oracle_lib.py`). `N05-via-clearance.json` failed on a
//! metric that also counted KiCad's board-minimum clearance, which the DSN
//! does not carry.
//!
//! Registered 2026-10-18 before any routing number as
//! 08ede5c03d4c9233b629fcb87fecf270ef0d247929bff17846ed59d50a955604. A later change goes in the file's `amendments` with the reason and
//! updates the hash here in the same change.
//!
//! The KiCad demo and corpus rows are outside the repository (named by path,
//! with their hashes in the registration); the canary row's input is checked
//! here.
use std::path::{Path, PathBuf};

use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N05-via-clearance-netclass.json";
const SET: &str = "tools/bench/sets/n05-via-clearance-netclass.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

#[test]
fn the_netclass_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N05.3-via-clearance-netclass");
    assert_eq!(prereg.names.base, "enhanced");
    assert_eq!(prereg.names.cand, "enhanced+netclass");
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 6);
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
            ("netclass_via_clearance_errors", Role::Guard),
            ("kicad_unconnected", Role::Score),
        ]
    );
    assert!(
        prereg
            .specs
            .iter()
            .all(|s| s.name != "class_via_clearance_errors"),
        "the board-minimum metric of N05-via-clearance.json is not judged here"
    );
}

/// Every row's input is in the set file; the repository inputs hash as
/// registered, and no row reuses a board N05-via-clearance.json routed except
/// canary-pwr's plugin path.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let json = Json::parse(&text).unwrap();
    let Some(Json::Object(rows)) = json.path("boards.rows") else {
        panic!("boards.rows")
    };
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let old =
        std::fs::read_to_string(repo().join("tools/bench/sets/n05-via-clearance.txt")).unwrap();
    for (row, spec) in rows {
        let input = spec.get("input").and_then(Json::as_str).expect("input");
        assert!(set.contains(input), "{input} missing from {SET}");
        if row != "canary-plugin45" {
            assert!(
                !old.contains(input),
                "{row} was routed by N05-via-clearance.json"
            );
        }
        if input.starts_with('/') || input.starts_with("target/") {
            continue;
        }
        let bytes = std::fs::read(repo().join(input)).unwrap_or_else(|e| panic!("{input}: {e}"));
        let want = json
            .path("boards.files")
            .and_then(|f| f.get(row))
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("boards.files.{row}"));
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            want,
            "{row}"
        );
    }
}

#[test]
fn the_netclass_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "08ede5c03d4c9233b629fcb87fecf270ef0d247929bff17846ed59d50a955604"
    );
}

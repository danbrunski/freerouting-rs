//! Pins N05's preregistrations (N05.3, the promotions into profile
//! `enhanced`), both judged by N02's totals rule with `accept: no-regress`
//! (correctness fixes):
//!
//! - `docs/roadmap/prereg/N05-power-layer-traces.json`:
//!   `router.power_layer_traces = none` against `own_plane` (Q13's rule);
//! - `docs/roadmap/prereg/N05-via-clearance.json`:
//!   `router.via_clearance = net_class` with the power-layer value the first
//!   one chooses (Q14.3 judged again).
//!
//! Both registered 2026-10-17 before any number, as
//! a5ca60498269945f8b33a2c5f0b3fa163d11ea4e3b714746c372c578a530d33d and
//! 75794c38fa84df963629434b1151e48d169a3144b5a7370dfb2f2eb23dc892dd. A later
//! change goes in the file's `amendments` with the reason and updates the
//! hash here in the same commit. `N05-power-layer-traces.json` was amended
//! once (2026-10-18, before any graded number): the plugin run's own session
//! came from the Windows build, so its calibration is replaced.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const PLT: &str = "docs/roadmap/prereg/N05-power-layer-traces.json";
const PLT_SET: &str = "tools/bench/sets/n05-power-layer-traces.txt";
const VIA: &str = "docs/roadmap/prereg/N05-via-clearance.json";
const VIA_SET: &str = "tools/bench/sets/n05-via-clearance.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration(file: &str) -> Prereg {
    let text = std::fs::read_to_string(repo().join(file)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("{file} is invalid: {e:?}"))
}

/// Every row's input is in the set file and, when it is in the repository,
/// hashes as registered (the corpus rows are skipped while missing).
fn check_inputs(file: &str, set: &str) {
    let text = std::fs::read_to_string(repo().join(file)).unwrap();
    let json = Json::parse(&text).unwrap();
    let Some(Json::Object(rows)) = json.path("boards.rows") else {
        panic!("{file}: boards.rows")
    };
    let files = json.path("boards.files").expect("boards.files");
    let set_text = std::fs::read_to_string(repo().join(set)).unwrap();
    for (row, spec) in rows {
        let path = spec.get("input").and_then(Json::as_str).expect("input");
        assert!(set_text.contains(path), "{file}: {path} missing from {set}");
        let sha = files
            .get(row)
            .and_then(Json::as_str)
            .expect("a hash per row");
        let Ok(bytes) = std::fs::read(repo().join(path)) else {
            assert!(path.starts_with("target/corpus/"), "{path} is missing");
            continue;
        };
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            sha,
            "{file}: {row}"
        );
    }
}

#[test]
fn the_power_layer_traces_registration_is_valid() {
    let prereg = registration(PLT);
    assert_eq!(prereg.id, "N05.3-power-layer-traces-none");
    assert_eq!(prereg.names.base, "own_plane");
    assert_eq!(prereg.names.cand, "none");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["layer", "none", "own_plane"]);
    assert_eq!(prereg.set.as_deref(), Some(PLT_SET));
    assert_eq!(prereg.boards.len(), 8);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..8],
        [
            "via_in_pad",
            "below_minimum",
            "outside_own_plane",
            "foreign_power_traces",
            "power_layer_traces",
            "drc_real",
            "inner_tracks",
            "kicad_unconnected"
        ]
    );
    check_inputs(PLT, PLT_SET);
}

#[test]
fn the_via_clearance_registration_is_valid() {
    let prereg = registration(VIA);
    assert_eq!(prereg.id, "N05.3-via-clearance-net-class");
    assert_eq!(prereg.names.base, "plt");
    assert_eq!(prereg.names.cand, "plt+netclass");
    assert_eq!(prereg.set.as_deref(), Some(VIA_SET));
    assert_eq!(prereg.boards.len(), 5);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..3],
        ["via_in_pad", "below_minimum", "class_via_clearance_errors"]
    );
    check_inputs(VIA, VIA_SET);
}

#[test]
fn the_n05_registrations_are_pinned_by_their_hashes() {
    assert_eq!(
        registration(PLT).sha256,
        "2013aaacf692c82b5c3ff6ef2bc24c7c61d71314e456d427fda9c94716b8ea23"
    );
    assert_eq!(
        registration(VIA).sha256,
        "75794c38fa84df963629434b1151e48d169a3144b5a7370dfb2f2eb23dc892dd"
    );
}

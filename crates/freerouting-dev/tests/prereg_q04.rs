//! Pins Q04's preregistration (`docs/roadmap/prereg/Q04.json`, Q04.4):
//! whether `router.autorouter.pass_controller = quantum-stop` with its
//! registered defaults cuts the routing wall time by at least 20 % on the
//! boards where Q04.2 finds an earlier stop, judged by N02's totals rule
//! with its speed clause, and the guards unrouted, vias and clearance
//! violations not rising on any board. Registered 2026-10-18 as
//! 8b587d03a724cc9f6e03c7f32beb3e28e9134982196cc39256d7fc49807e3679,
//! before any Q04.4 number; a later change goes in its `amendments` with
//! the reason and updates the hash below in the same change. Amendment 1
//! (2026-10-18, Q04.5, before any Q04.4 number): the `quantum-stop` arm
//! also sets `router.quantum_stop.repeat=true`; the hash below is the
//! amended file's.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::gate_totals::Accept;

const FILE: &str = "docs/roadmap/prereg/Q04.json";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("Q04 registration is invalid: {e:?}"))
}

#[test]
fn the_q04_registration_is_valid_and_names_its_arms_boards_and_rule() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q04.4-quantum-stop");
    assert_eq!(prereg.names.base, "default");
    assert_eq!(prereg.names.cand, "quantum-stop");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["default", "quantum-stop"]);
    // The 16 benchmark boards and the five in-repository canary-pwr variants.
    assert_eq!(prereg.boards.len(), 21);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        metrics,
        [
            "via_in_pad",
            "unrouted",
            "vias",
            "clearance_violations",
            "wall_seconds"
        ]
    );
    let totals = prereg.totals.expect("a totals block");
    assert_eq!(totals.accept, Accept::Improve);
    assert_eq!(totals.signal, ["unrouted"]);
    assert_eq!(totals.noise.get("canary-pwr"), Some(&10.0));
    assert_eq!(totals.min_boards, 3);
    assert!(totals.speed.is_some(), "the speed clause on wall_seconds");
    assert_eq!(prereg.amendments, 1, "Q04.5's repeat stop");
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    assert!(text.contains("--router.quantum_stop.repeat=true"));
}

#[test]
fn the_q04_registration_states_the_defaults_the_engine_uses() {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    for (key, value) in [
        ("router.quantum_stop.grace", "0"),
        ("router.quantum_stop.checks", "2"),
        ("router.quantum_stop.quantum_abs", "1.0"),
        ("router.quantum_stop.quantum_pct", "0.01"),
    ] {
        assert!(
            text.contains(&format!("\"{key}\": {value}")),
            "{key} = {value}"
        );
    }
}

#[test]
fn the_q04_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "291de123ff9857d10a64ddfdf3bc09abe37130c9510d94949c56eb74400ba0e7"
    );
}

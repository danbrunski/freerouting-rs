//! Pins Q16's preregistration (`docs/roadmap/prereg/Q16.json`, Q16.3):
//! whether `router.cleanup.remove_stubs = true` leaves no dangling via or
//! trace on canary-pwr (the plugin path and Dan's plugin45 handoff DSN) and
//! the K09 demos, judged by N02's totals rule with `accept: no-regress` and
//! the hard guards unconnected not rising on any board, no new DRC error
//! and determinism. Registered 2026-10-18 as
//! 83d71ce12b22a3aba7f3fd6bc33339d86e0911afbbfd5b200c6c173181438c82,
//! before any routing number; a later change goes in its `amendments` with
//! the reason and updates the hash below in the same change. Amended once,
//! before any Q16.3 number, to
//! d3d20814b910f432e799b2262218f8dbe4b700e408a6b2323787b7bdd0a24493: N05
//! gave `enhanced` `power_layer_traces = none` after the registration, so
//! both arms run under it.
//!
//! The boards are outside the repository (Dan's canary repo, KiCad's demos
//! and the handoff directory), so their hashes are checked by the run, not
//! here.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::gate_totals::Accept;

const FILE: &str = "docs/roadmap/prereg/Q16.json";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("Q16 registration is invalid: {e:?}"))
}

#[test]
fn the_q16_registration_is_valid_and_names_its_arms_boards_and_rule() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q16.3-remove-stubs");
    assert_eq!(prereg.names.base, "plugin45");
    assert_eq!(prereg.names.cand, "plugin45+cleanup");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["plugin45", "plugin45+cleanup"]);
    assert_eq!(prereg.boards.len(), 5);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..6],
        [
            "via_in_pad",
            "below_minimum",
            "via_dangling",
            "track_dangling",
            "drc_real",
            "kicad_unconnected"
        ]
    );
    let totals = prereg.totals.expect("a totals block");
    assert_eq!(totals.accept, Accept::NoRegress);
    assert_eq!(totals.signal, ["kicad_unconnected"]);
    assert_eq!(totals.noise.get("canary-pwr"), Some(&10.0));
    assert_eq!(totals.min_boards, 3);
    assert_eq!(totals.speed, None);
    assert_eq!(prereg.amendments, 1);
}

#[test]
fn the_q16_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "d3d20814b910f432e799b2262218f8dbe4b700e408a6b2323787b7bdd0a24493"
    );
}

//! Pins Q16.4's preregistration (`docs/roadmap/prereg/Q16-4.json`): whether
//! `router.cleanup.remove_stubs = true`, with the stub test counting
//! contacts as KiCad does, leaves no dangling via or track visible from the
//! DSN on canary-pwr (the plugin path) and four KiCad demos Q16.3 did not
//! use, judged by N02's totals rule with `accept: no-regress` and the hard
//! guards unconnected not rising on any board, no new DRC error and
//! determinism. Registered 2026-10-18 as
//! 45a2679297b0ce5cc0684f98d9d90c14e965b2fdf8955d3dc04679964cebbf2d,
//! before any routing number; a later change goes in its `amendments` with
//! the reason and updates the hash below in the same change. Amendment 1
//! (2026-10-18, before any route) moved the engine to main b93bcd0, whose
//! `enhanced` adds `via_clearance = net_class`.
//!
//! The boards are outside the repository (the owner's canary repo and KiCad's
//! demos), so their hashes are checked by the run, not here.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::gate_totals::Accept;

const FILE: &str = "docs/roadmap/prereg/Q16-4.json";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("Q16.4 registration is invalid: {e:?}"))
}

#[test]
fn the_q16_4_registration_is_valid_and_names_its_arms_boards_and_rule() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q16.4-remove-stubs-kicad-contacts");
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
            "via_dangling_dsn",
            "track_dangling_dsn",
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
fn the_q16_4_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "c0674fb312912cbd6d006404c2c0c0fa247281392533b66d8a908e261adcec9e"
    );
}

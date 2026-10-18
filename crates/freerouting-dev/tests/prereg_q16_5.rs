//! Pins Q16.5's preregistration (`docs/roadmap/prereg/Q16-5.json`): whether
//! `router.cleanup.remove_stubs = true`, with Q16.4's contacts and Q16.5's
//! guard (the removed item's contacts connected through one another, not
//! every two touching), leaves no dangling via or track visible from the DSN
//! on canary-pwr at canary d6199b48 (the plugin path) and four KiCad demos
//! Q16.3 and Q16.4 did not use, all under the plugin's default `enhanced`
//! options, so each board keeps its own Board Setup minimum. Judged by N02's
//! totals rule with `accept: no-regress` and Q16.4's hard guards (unconnected
//! not rising on any board, no new DRC error with `solder_mask_bridge` taken
//! as the smaller count of two gradings, via_in_pad and below_minimum 0, no
//! dangling copper visible from the DSN, determinism). Registered
//! 2026-10-18, before any routing number; a later change goes in its
//! `amendments` with the reason and updates the hash below in the same
//! change.
//!
//! The boards are outside the repository (the owner's canary repo and KiCad's
//! demos), so their hashes are checked by the run, not here.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::gate_totals::Accept;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q16-5.json";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("Q16.5 registration is invalid: {e:?}"))
}

#[test]
fn the_q16_5_registration_is_valid_and_names_its_arms_boards_and_rule() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q16.5-remove-stubs-connected-contacts");
    assert_eq!(prereg.names.base, "enhanced-default");
    assert_eq!(prereg.names.cand, "enhanced-default+cleanup");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["enhanced-default", "enhanced-default+cleanup"]);
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
    assert!(totals.noise.is_empty(), "every band is N02's default");
    assert_eq!(totals.min_boards, 3);
    assert_eq!(totals.speed, None);
    assert_eq!(prereg.amendments, 0);
}

#[test]
fn the_q16_5_registration_uses_the_plugin_default_options_and_no_q16_board() {
    let prereg = registration();
    for (name, arm) in &prereg.arms {
        let Some(Json::Object(options)) = arm.get("options") else {
            panic!("{name} has no options");
        };
        let keys: Vec<&str> = options.keys().map(String::as_str).collect();
        assert_eq!(keys, ["profile", "schema"], "{name}: the plugin's defaults");
        assert_eq!(options["profile"].as_str(), Some("enhanced"));
        assert_eq!(options["schema"].as_f64(), Some(4.0));
    }
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    for used in [
        "interf_u",
        "CM5_MINIMA_3",
        "pic_programmer",
        "kit-dev-coldfire-xilinx_5213",
        "StickHub",
        "RoyalBlue54L-Feather",
        "multichannel_mixer",
    ] {
        assert!(
            !text.contains(used),
            "{used} was a board of Q16.1, Q16.3 or Q16.4"
        );
    }
}

#[test]
fn the_q16_5_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "0408cd5a07c87033cee3758b54e1acccbf77cac94eba3a72c0404e5051423a6b"
    );
}

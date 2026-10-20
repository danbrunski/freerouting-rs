//! Pins N06.15's registration (`docs/roadmap/prereg/N06.15.json`): the shove fix
//! `router.java_quirks.shove_via_into_pending_piece`, against Java's behaviour under
//! profile `legacy` and under profile `enhanced`, judged by N02's totals rule with
//! `accept: no-regress` after the reachability counter.
//!
//! Registered 2026-10-18 before any routing number of the fix as a switch (N03.2's
//! prototype numbers on GP8B came first; the file names them). A later change goes
//! in the file's `amendments` with the reason and updates the hash here in the same
//! change; the one amendment so far rewrote `pairs` into the array form the gate
//! reads, with the same two pairs.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::{PairRole, Prereg};
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.15.json";
const SET: &str = "tools/bench/sets/n06-pending-piece.txt";
const SWITCH: &str = "shove_via_into_pending_piece";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn json() -> Json {
    Json::parse(&std::fs::read_to_string(repo().join(FILE)).unwrap()).unwrap()
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

#[test]
fn the_pending_piece_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.15-shove-via-into-pending-piece");
    assert_eq!(prereg.names.base, "legacy_baseline");
    assert_eq!(prereg.names.cand, "legacy_fix");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "enhanced_baseline",
            "enhanced_fix",
            "legacy_baseline",
            "legacy_fix"
        ]
    );
    // Two judged pairs, one per profile (`gate --prereg --all-pairs`).
    let pairs: Vec<(&str, &str, PairRole)> = prereg
        .pairs
        .iter()
        .map(|p| (p.arms.base.as_str(), p.arms.cand.as_str(), p.role))
        .collect();
    assert_eq!(
        pairs,
        [
            ("legacy_baseline", "legacy_fix", PairRole::Listed),
            ("enhanced_baseline", "enhanced_fix", PairRole::Listed)
        ]
    );
    assert!(prereg.pairs.iter().all(|p| p.rows.is_none()));
    assert_eq!(prereg.set.as_deref(), Some(SET));
    // The DSN rows plus the plugin row (amendment 2: `canary-pwr-plugin`, hashed as
    // its exported DSN, routed outside n06_run.py).
    assert_eq!(prereg.boards.len(), 8);
    assert!(
        prereg
            .boards
            .contains_key("fixtures/Issue066-Project_GP8B.dsn")
    );
    assert!(prereg.totals_registered);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    for name in [
        "unrouted",
        "clearance_violations",
        "kicad_unconnected",
        "kicad_shorting_items",
        "kicad_clearance",
        "session_differences",
        "shove_via_into_pending_pieces",
        "shove_pending_piece_foreign_traces",
    ] {
        assert!(metrics.contains(&name), "{name}");
    }
    // n06_run.py runs the arms in file order; each arm's own baseline comes
    // before it, and the registration's baseline first.
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let arms = &text[text.find("\"arms\": {").expect("arms")..];
    let first = arms[9..].trim_start().split('"').nth(1);
    assert_eq!(first, Some("legacy_baseline"));
    let order: Vec<usize> = [
        "legacy_baseline",
        "legacy_fix",
        "enhanced_baseline",
        "enhanced_fix",
    ]
    .iter()
    .map(|arm| arms.find(&format!("\"{arm}\": {{")).expect(arm))
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
}

/// Each arm names its profile and gives the switch explicitly, `false` exactly in
/// the fix arms, and is compared with the baseline of its own profile.
#[test]
fn each_arm_sets_its_profile_and_the_switch() {
    let json = json();
    let expect = [
        ("legacy_baseline", "legacy", true, "legacy_baseline"),
        ("legacy_fix", "legacy", false, "legacy_baseline"),
        ("enhanced_baseline", "enhanced", true, "enhanced_baseline"),
        ("enhanced_fix", "enhanced", false, "enhanced_baseline"),
    ];
    for (arm, profile, java, baseline) in expect {
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        assert_eq!(command.matches("--router.java_quirks.").count(), 1, "{arm}");
        assert_eq!(command.matches("--router.profile=").count(), 1, "{arm}");
        assert!(
            command.contains(&format!("--router.profile={profile} ")),
            "{arm}"
        );
        assert!(
            command.ends_with(&format!("--router.java_quirks.{SWITCH}={java}")),
            "{arm}: {command}"
        );
        for bound in [
            "--router.autorouter.max_passes=3",
            "--router.optimizer.enabled=false",
            "--stats json",
        ] {
            assert!(command.contains(bound), "{arm}: {bound}");
        }
        assert_eq!(
            spec.get("baseline").and_then(Json::as_str),
            Some(baseline),
            "{arm}"
        );
        let Some(Json::Object(settings)) = spec.get("settings") else {
            panic!("{arm}: settings")
        };
        let key = format!("router.java_quirks.{SWITCH}");
        assert_eq!(settings.iter().any(|(k, _)| *k == key), !java, "{arm}");
        let enhanced = settings.iter().any(|(k, _)| k == "router.profile");
        assert_eq!(enhanced, profile == "enhanced", "{arm}");
    }
}

/// Every row is in the set file and hashes as registered.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let json = json();
    let Some(Json::Object(all)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    // The rows routed outside n06_run.py (amendment 2) are the plugin row only; its
    // input is the plugin's export, which is not in the repository.
    let Some(Json::Array(outside)) = json.path("boards.outside_runner") else {
        panic!("boards.outside_runner")
    };
    let outside: Vec<&str> = outside.iter().filter_map(Json::as_str).collect();
    assert_eq!(outside, ["canary-pwr-plugin"]);
    let files: Vec<_> = all
        .iter()
        .filter(|(row, _)| !outside.contains(&row.as_str()))
        .collect();
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let rows: Vec<&str> = set
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(rows.len(), files.len());
    for &(input, want) in &files {
        assert!(rows.contains(&input.as_str()), "{input} missing from {SET}");
        let bytes = std::fs::read(repo().join(input)).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            want.as_str().unwrap(),
            "{input}"
        );
    }
    let options = json
        .path("plugin_canary.options")
        .and_then(Json::as_str)
        .expect("options");
    let bytes = std::fs::read(repo().join(options)).unwrap();
    assert_eq!(
        Some(freerouting::quality::feasibility::report::sha256_hex(&bytes).as_str()),
        json.path("plugin_canary.options_sha256")
            .and_then(Json::as_str)
    );
}

#[test]
fn the_pending_piece_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "04376de54af271cdb1e5bb58a69de0bc627be3a9036df2f935c82f5b9b8efe6e"
    );
}

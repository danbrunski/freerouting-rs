//! Pins N06.13's confirmation registration (`docs/roadmap/prereg/N06.13-promotion.json`):
//! profile `enhanced` at the parent of the promotion commit against `enhanced` with the 30
//! `router.java_quirks` switches N06.13 promoted (the promotion commit), judged by N02's
//! totals rule with `accept: no-regress` and option (b) hard guards, every route under an
//! 8000000 kB address-space cap.
//!
//! Registered 2026-10-19 before any route of either arm. A later change goes in the file's
//! `amendments` with the reason and updates the hash here in the same change.
use std::path::{Path, PathBuf};

use freerouting::settings::profile::ENHANCED;
use freerouting_dev::gate_prereg::{PairRole, Prereg};
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.13-promotion.json";
const SET: &str = "tools/bench/sets/n06-13-promotion.txt";
const PLUGIN_ROW: &str = "canary-pwr-plugin";
const BASE_BIN: &str =
    "/home/thebo/code/freerouting-rs/target/orchestrator/n06-13-base/release/freerouting";
const CAND_BIN: &str =
    "/home/thebo/code/freerouting-rs/target/orchestrator/n06-13-cand/release/freerouting";

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

/// The switches `enhanced`'s table turns off, sorted.
fn promoted() -> Vec<&'static str> {
    let mut ids: Vec<&str> = ENHANCED
        .iter()
        .filter_map(|d| d.path.strip_prefix("java_quirks."))
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn the_promotion_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.13-promotion");
    assert_eq!(prereg.names.base, "enhanced_main");
    assert_eq!(prereg.names.cand, "enhanced_promoted");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["enhanced_main", "enhanced_promoted"]);
    let pairs: Vec<(&str, &str, PairRole)> = prereg
        .pairs
        .iter()
        .map(|p| (p.arms.base.as_str(), p.arms.cand.as_str(), p.role))
        .collect();
    assert_eq!(
        pairs,
        [("enhanced_main", "enhanced_promoted", PairRole::Listed)]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 22, "21 DSN rows and the plugin row");
    assert!(prereg.totals_registered);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    for name in [
        "via_in_pad",
        "below_minimum",
        "clearance_violations",
        "drc_real",
        "kicad_shorting_items",
        "unrouted",
        "kicad_unconnected",
        "session_differences",
        "wall_seconds",
        "maze_ortho_zero_width_room_flips",
        "fanout_stale_drill_carries",
        "route_result_zero_test_changes",
        "any_angle_skip_length_flips",
    ] {
        assert!(metrics.contains(&name), "{name}");
    }
    let json = json();
    assert_eq!(
        json.path("totals.accept").and_then(Json::as_str),
        Some("no-regress")
    );
    let accept = json.path("accept").expect("accept");
    let Json::Array(clauses) = accept else {
        panic!("accept")
    };
    assert!(
        clauses
            .iter()
            .filter_map(Json::as_str)
            .any(|c| c.contains("option (b), no new breach versus the baseline arm")),
        "the hard guards are option (b)"
    );
}

/// Both arms run the default profile with no router flag; they differ in the binary
/// they call and, through it, in ENHANCED's java_quirks entries, which the candidate's
/// `settings` list exactly.
#[test]
fn the_arms_differ_only_in_the_promoted_switches() {
    let json = json();
    for (arm, binary) in [("enhanced_main", BASE_BIN), ("enhanced_promoted", CAND_BIN)] {
        let command = json
            .path(&format!("arms.{arm}.command"))
            .and_then(Json::as_str)
            .expect(arm);
        assert!(command.starts_with(&format!("{binary} ")), "{arm}");
        assert!(!command.contains("--router."), "{arm}: no router flag");
        assert!(!command.contains("--profile"), "{arm}: the default profile");
        for flag in [
            "--gui.enabled=false",
            "--stats json",
            "--result-json={RESULT}",
        ] {
            assert!(command.contains(flag), "{arm}: {flag}");
        }
    }
    let base = json
        .path("arms.enhanced_main.command")
        .and_then(Json::as_str)
        .unwrap();
    let cand = json
        .path("arms.enhanced_promoted.command")
        .and_then(Json::as_str)
        .unwrap();
    assert_eq!(base.replace(BASE_BIN, ""), cand.replace(CAND_BIN, ""));
    let Some(Json::Object(none)) = json.path("arms.enhanced_main.settings") else {
        panic!("baseline settings")
    };
    assert!(none.is_empty());
    let Some(Json::Object(settings)) = json.path("arms.enhanced_promoted.settings") else {
        panic!("candidate settings")
    };
    let mut keys: Vec<&str> = settings
        .iter()
        .map(|(k, v)| {
            assert!(matches!(v, Json::Bool(false)), "{k} is false");
            k.strip_prefix("router.java_quirks.").expect("a switch")
        })
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        promoted(),
        "the candidate's settings are ENHANCED's switches"
    );
    assert_eq!(keys.len(), 30);
}

/// Every DSN row is in the set file and hashes as registered; the plugin row is hashed
/// as its source board and routed outside the runner; the any-angle J2 and rpi_splitter
/// variants are not rows.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let json = json();
    let Some(Json::Object(files)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    let Some(Json::Array(outside)) = json.path("boards.outside_runner") else {
        panic!("boards.outside_runner")
    };
    let outside: Vec<&str> = outside.iter().filter_map(Json::as_str).collect();
    assert_eq!(outside, [PLUGIN_ROW]);
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let rows: Vec<&str> = set
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(rows.len(), 21);
    assert_eq!(rows.len() + 1, files.len());
    for row in &rows {
        assert!(!row.ends_with("-any.dsn") || row.contains("scoring-mixed-layer-any"));
    }
    for (input, want) in files {
        if input == PLUGIN_ROW {
            assert_eq!(
                want.as_str(),
                json.path("plugin_canary.source_sha256")
                    .and_then(Json::as_str),
                "the plugin row is hashed as its source board"
            );
            continue;
        }
        assert!(rows.contains(&input.as_str()), "{input} missing from {SET}");
        let bytes = std::fs::read(repo().join(input)).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            want.as_str().unwrap(),
            "{input}"
        );
    }
    assert_eq!(
        json.path("totals.noise.canary-pwr-plugin")
            .and_then(Json::as_f64),
        Some(10.0)
    );
}

/// The orchestrator's command builds both binaries and routes under the memory cap.
#[test]
fn every_route_runs_under_the_memory_cap() {
    let json = json();
    let command = json
        .path("orchestrator_command")
        .and_then(Json::as_str)
        .expect("orchestrator_command");
    for part in [
        "CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-13-base",
        "CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-13-cand",
        "N06_RUN_ULIMIT_KB=8000000 python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.13-promotion.json",
        "--out /home/thebo/code/freerouting-rs/target/orchestrator/n06-13",
    ] {
        assert!(command.contains(part), "{part}");
    }
    let route = json
        .path("plugin_canary.route")
        .and_then(Json::as_str)
        .expect("plugin_canary.route");
    assert!(route.contains("ulimit -v 8000000"));
}

#[test]
fn the_promotion_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "cd57a3af1e9836f075d8bb5059076202844186ba56ffdafea66cdcd4bd6b5d24"
    );
}

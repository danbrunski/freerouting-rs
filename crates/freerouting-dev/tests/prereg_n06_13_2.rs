//! Pins the confirmation registration of N06.13's second promotion round
//! (`docs/roadmap/prereg/N06.13-promotion-2.json`): profile `enhanced` at the parent of the
//! round's commit against `enhanced` with the five second-round `router.java_quirks`
//! switches `false` (the round's commit), both arms `--router.profile=enhanced`, judged by
//! N02's totals rule with `accept: no-regress`, option (b) hard guards and N06.16's abort
//! rule for a baseline that aborts at the memory cap, every route under an 8000000 kB
//! address-space cap.
//!
//! Registered 2026-10-20 before any route of either arm. A later change goes in the file's
//! `amendments` with the reason and updates the hash here in the same change.
use std::path::{Path, PathBuf};

use freerouting::settings::profile::ENHANCED;
use freerouting_dev::gate_prereg::{PairRole, Prereg};
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.13-promotion-2.json";
const SET: &str = "tools/bench/sets/n06-13-promotion-2.txt";
const ROUND_1_SET: &str = "tools/bench/sets/n06-13-promotion.txt";
const PLUGIN_ROW: &str = "canary-pwr-plugin";
const BASE_BIN: &str =
    "/home/thebo/code/freerouting-rs/target/orchestrator/n06-13b-base/release/freerouting";
const CAND_BIN: &str =
    "/home/thebo/code/freerouting-rs/target/orchestrator/n06-13b-cand/release/freerouting";
const ANY_ROWS: [&str; 2] = [
    "crates/freerouting/tests/data/autoroute-variants/Issue026-J2_reference-any.dsn",
    "crates/freerouting/tests/data/autoroute-variants/Issue143-rpi_splitter-any.dsn",
];

/// The switches the second round turned off, in inventory order.
const SECOND_ROUND: [&str; 5] = [
    "simplex_prev_division_line",
    "opt_first_pass_stops",
    "opt_equal_key_skip",
    "shove_via_into_pending_piece",
    "any_angle_room_side_index",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn json() -> Json {
    Json::parse(&text()).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

fn set_rows(set: &str) -> Vec<String> {
    std::fs::read_to_string(repo().join(set))
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

#[test]
fn the_second_round_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.13-promotion-2");
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
    assert_eq!(prereg.boards.len(), 24, "23 DSN rows and the plugin row");
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
        "aborted",
        "peak_rss_kb",
        "optimizer_passes",
        "simplex_prev_division_line_reached",
        "shove_via_into_pending_pieces",
        "any_angle_room_side_mismatches",
    ] {
        assert!(metrics.contains(&name), "{name}");
    }
    let json = json();
    assert_eq!(
        json.path("totals.accept").and_then(Json::as_str),
        Some("no-regress")
    );
    let Some(Json::Array(clauses)) = json.path("accept") else {
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

/// `n06_run.py` needs the baseline first. Both arms give `--router.profile=enhanced` and no
/// other router flag; they differ in the binary they call and, through it, in ENHANCED's
/// second-round entries, which the candidate's `settings` list exactly.
#[test]
fn the_arms_differ_only_in_the_second_round_switches() {
    let json = json();
    let text = text();
    let at = |arm: &str| text.find(&format!("\"{arm}\": {{")).expect(arm);
    assert!(at("enhanced_main") < at("enhanced_promoted"));
    for (arm, binary) in [("enhanced_main", BASE_BIN), ("enhanced_promoted", CAND_BIN)] {
        let command = json
            .path(&format!("arms.{arm}.command"))
            .and_then(Json::as_str)
            .expect(arm);
        assert!(command.starts_with(&format!("{binary} ")), "{arm}");
        assert_eq!(command.matches("--router.").count(), 1, "{arm}");
        assert!(command.contains(" --router.profile=enhanced "), "{arm}");
        for flag in [
            "{DSN}",
            "{SES}",
            "--gui.enabled=false",
            "--stats json",
            "--stats-out {STATS}",
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
    let mut want = SECOND_ROUND.to_vec();
    want.sort_unstable();
    assert_eq!(keys, want);
    for id in SECOND_ROUND {
        let path = format!("java_quirks.{id}");
        assert!(
            ENHANCED
                .iter()
                .any(|d| d.path == path && d.value == "false"),
            "{id} is false in ENHANCED"
        );
    }
}

/// The rows are round 1's 21 DSN rows in its order, then the two any-angle rows; every
/// DSN row hashes as registered; the plugin row is hashed as its source board and routed
/// outside the runner.
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
    let rows = set_rows(SET);
    let mut want = set_rows(ROUND_1_SET);
    want.extend(ANY_ROWS.iter().map(|r| r.to_string()));
    assert_eq!(rows, want);
    assert_eq!(rows.len(), 23);
    assert_eq!(rows.len() + 1, files.len());
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
        assert!(rows.contains(input), "{input} missing from {SET}");
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

/// Only the baseline may abort at the cap, under N06.16's rule; the orchestrator's command
/// builds both binaries and routes under the cap.
#[test]
fn the_abort_rule_and_the_memory_cap() {
    let json = json();
    let Some(Json::Array(arms)) = json.path("abort_rule.arms") else {
        panic!("abort_rule.arms")
    };
    let arms: Vec<&str> = arms.iter().filter_map(Json::as_str).collect();
    assert_eq!(arms, ["enhanced_main"]);
    let rule = json
        .path("abort_rule.rule")
        .and_then(Json::as_str)
        .expect("abort_rule.rule");
    for part in [
        "via_in_pad_raw 0",
        "clearance_violations 0",
        "memory allocation of",
    ] {
        assert!(rule.contains(part), "{part}");
    }
    let command = json
        .path("orchestrator_command")
        .and_then(Json::as_str)
        .expect("orchestrator_command");
    for part in [
        "git worktree add --detach target/orchestrator/n06-13b-base-wt c8b91b7d75f40879ab8415562fc7f3c4361114fb",
        "git worktree add --detach target/orchestrator/n06-13b-cand-wt 0bf562b81f8d271d1951b7d763da96219578646a",
        "CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-13b-base",
        "CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-13b-cand",
        "N06_RUN_ULIMIT_KB=8000000 python3 tools/bench/n06_run.py /home/thebo/code/freerouting-rs/docs/roadmap/prereg/N06.13-promotion-2.json",
        "--out /home/thebo/code/freerouting-rs/target/orchestrator/n06-13b",
    ] {
        assert!(command.contains(part), "{part}");
    }
    let route = json
        .path("plugin_canary.route")
        .and_then(Json::as_str)
        .expect("plugin_canary.route");
    assert!(route.contains("ulimit -v 8000000"));
    assert!(route.contains("--router.profile=enhanced"));
}

#[test]
fn the_second_round_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "773ec7a061a67f130fd5c6d4f113b066da68d1f289ecff8ae5b01d062b6eb921"
    );
}

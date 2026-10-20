//! Pins N06.16's registration (`docs/roadmap/prereg/N06.16.json`): the any-angle room
//! sides, `router.java_quirks.any_angle_room_side_index`, Java's indexes (`true`) against
//! the room's simplex (`false`), under profile `enhanced`, judged by N02's totals rule with
//! `accept: no-regress` and the registration's abort rule (a baseline that aborts at the
//! memory cap counts as the fix's win if the fix completes without a hard-guard breach), and
//! run by `tools/bench/n06_run.py`.
//!
//! Registered 2026-10-20 before any number under it. A later change goes in the file's
//! `amendments` with the reason and updates the hash here in the same change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.16.json";
const SET: &str = "tools/bench/sets/n06-16.txt";
const KEY: &str = "any_angle_room_side_index";
const VARIANTS: &str = "crates/freerouting/tests/data/autoroute-variants";

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

#[test]
fn the_n06_16_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.16-any-angle-room-sides");
    assert_eq!(prereg.names.base, "baseline");
    assert_eq!(prereg.names.cand, "fix");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["baseline", "fix"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 8);
    assert!(prereg.totals_registered);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    for name in [
        "via_in_pad",
        "below_minimum",
        "clearance_violations",
        "unrouted",
        "aborted",
        "session_differences",
        "any_angle_room_side_mismatches",
    ] {
        assert!(metrics.contains(&name), "{name}");
    }
}

/// `n06_run.py` needs the baseline first; each command names the binary, the profile it
/// would be promoted into and the one key.
#[test]
fn each_arm_sets_only_the_room_side_switch_under_enhanced() {
    let json = json();
    let text = text();
    let at = |arm: &str| text.find(&format!("\"{arm}\": {{")).expect(arm);
    assert!(
        at("baseline") < at("fix"),
        "n06_run.py needs the baseline first"
    );
    for (arm, value) in [("baseline", "true"), ("fix", "false")] {
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        assert!(command.starts_with('/'), "{arm}: an absolute binary path");
        for placeholder in ["{DSN}", "{SES}", "{STATS}", "{RESULT}"] {
            assert!(command.contains(placeholder), "{arm}: {placeholder}");
        }
        assert!(command.contains(" --router.profile=enhanced "), "{arm}");
        assert_eq!(command.matches("--router.java_quirks.").count(), 1);
        assert!(
            command.ends_with(&format!("--router.java_quirks.{KEY}={value}")),
            "{arm}: {command}"
        );
        let Some(Json::Object(settings)) = spec.get("settings") else {
            panic!("{arm}: settings")
        };
        assert_eq!(settings.len(), usize::from(value == "false"), "{arm}");
    }
}

/// Every any-angle variant is a row, and the identity rows are named in the pairs rule.
#[test]
fn every_any_angle_variant_is_a_row() {
    let json = json();
    let Some(Json::Object(files)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    let mut variants: Vec<String> = std::fs::read_dir(repo().join(VARIANTS))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with("-any.dsn"))
        .map(|n| format!("{VARIANTS}/{n}"))
        .collect();
    variants.sort();
    assert_eq!(variants.len(), 3);
    for row in &variants {
        assert!(files.contains_key(row), "{row}");
    }
    let rule = json
        .path("pairs_rule")
        .and_then(Json::as_str)
        .expect("pairs_rule");
    for row in files.keys().filter(|r| !variants.contains(r)) {
        // The 90 degree variants are named together ("the two -90 variants").
        let named =
            rule.contains(row.as_str()) || (row.ends_with("-90.dsn") && rule.contains("-90"));
        assert!(named, "{row}");
    }
    assert!(rule.contains("any_angle_room_side_mismatches 0"), "{rule}");
}

/// The abort rule: only the baseline may abort, at the registered cap, and the runner
/// knows it.
#[test]
fn the_abort_rule_is_registered() {
    let json = json();
    let Some(Json::Array(arms)) = json.path("abort_rule.arms") else {
        panic!("abort_rule.arms")
    };
    let arms: Vec<&str> = arms.iter().filter_map(Json::as_str).collect();
    assert_eq!(arms, ["baseline"]);
    let rule = json
        .path("abort_rule.rule")
        .and_then(Json::as_str)
        .expect("rule");
    for needle in [
        "ulimit -v 8000000",
        "SIGABRT",
        "memory allocation of",
        "not gated",
    ] {
        assert!(rule.contains(needle), "{needle}");
    }
    let runner = std::fs::read_to_string(repo().join("tools/bench/n06_run.py")).unwrap();
    assert!(runner.contains("abort_rule") && runner.contains("memory allocation of"));
    let command = json
        .path("orchestrator_command")
        .and_then(Json::as_str)
        .unwrap();
    assert!(command.contains("N06_RUN_ULIMIT_KB=8000000"));
}

/// Every board is in the set file, and every input hashes as registered.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let json = json();
    let Some(Json::Object(files)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let mut listed: Vec<&str> = set
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .collect();
    let mut registered: Vec<&str> = files.keys().map(String::as_str).collect();
    listed.sort_unstable();
    registered.sort_unstable();
    assert_eq!(listed, registered, "{SET} and boards.files");
    for (input, want) in files {
        let bytes = std::fs::read(repo().join(input)).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            want.as_str().expect("hash"),
            "{input}"
        );
    }
}

#[test]
fn the_n06_16_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "9dad8e7a0c826d08d33b9667325456af6a201205ead4a2b1803c6b4498a08753"
    );
}

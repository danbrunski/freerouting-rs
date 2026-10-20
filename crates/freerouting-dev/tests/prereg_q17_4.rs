//! Pins Q17.4's registration (`docs/roadmap/prereg/Q17.4.json`): Q17.3's widen-back after
//! enhanced's necked retry at 300 um, and Q17.2's bounded-neck with the widen-back, each against
//! profile `enhanced` as it is at main a25c82f, judged by N02's totals rule with
//! `accept: no-regress` plus Q17's width clause.
//!
//! Registered 2026-10-20 before any number under it. The hard guards are "no new breach versus
//! baseline" (decided 2026-10-18), and the run is kept short (decided 2026-10-20: three arms,
//! canary-pwr's two rows plus four rows that route in well under a minute). A later change goes
//! in the file's `amendments` with the reason and updates the hash here in the same change.
//!
//! The demo and corpus rows are outside the repository; their hashes are checked by the run.
//! The canary rows' DSNs are checked here.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q17.4.json";
const SET: &str = "tools/bench/sets/q17-4.txt";

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
fn the_q17_4_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q17.4-neck-in-the-pin-field");
    assert_eq!(prereg.names.base, "enhanced");
    assert_eq!(prereg.names.cand, "bounded-neck-widen-back");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["bounded-neck-widen-back", "enhanced", "widen-back"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 6);
    assert!(prereg.totals_registered);
    assert_eq!(prereg.amendments, 0);
    let json = json();
    assert_eq!(
        json.path("totals.accept").and_then(Json::as_str),
        Some("no-regress")
    );
    assert_eq!(
        json.path("totals.noise")
            .and_then(|n| n.get("canary-pwr"))
            .and_then(Json::as_f64),
        Some(10.0)
    );
    let Some(Json::Array(metrics)) = json.get("metrics") else {
        panic!("metrics")
    };
    let role = |name: &str| {
        metrics
            .iter()
            .find(|m| m.get("name").and_then(Json::as_str) == Some(name))
            .and_then(|m| m.get("role"))
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("metric {name}"))
            .to_string()
    };
    for hard in [
        "via_in_pad",
        "below_minimum",
        "clearance_errors",
        "power_layer_traces",
    ] {
        assert_eq!(role(hard), "hard", "{hard}");
        assert_eq!(role(&format!("{hard}_raw")), "report", "{hard}_raw");
    }
    for guard in ["drc_real", "inner_tracks"] {
        assert_eq!(role(guard), "guard", "{guard}");
    }
    assert_eq!(role("kicad_unconnected"), "score");
    for report in ["under_board_setup_mm", "wall_seconds", "aborted"] {
        assert_eq!(role(report), "report", "{report}");
    }
    let definitions = json
        .get("metric_definitions")
        .and_then(|d| d.get("via_in_pad, below_minimum, clearance_errors, power_layer_traces"))
        .and_then(Json::as_str)
        .expect("the hard guards' definition");
    assert!(definitions.contains("no new breach versus baseline"));
}

/// Each arm routes under the explicit profile and adds exactly its neck-down flags.
#[test]
fn each_arm_adds_only_its_neck_down_flags() {
    let json = json();
    let expect: [(&str, &[(&str, &str)]); 3] = [
        ("enhanced", &[]),
        ("widen-back", &[("router.neck_down.widen_back", "true")]),
        (
            "bounded-neck-widen-back",
            &[
                ("router.special.neck_down", "bounded-neck"),
                ("router.neck_down.widen_back", "true"),
            ],
        ),
    ];
    for (arm, flags) in expect {
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        let base = "{ROUTE} -de {DSN} -do {SES} --gui.enabled=false --router.profile=enhanced \
                    {ROW_FLAGS}";
        let added: String = flags
            .iter()
            .map(|(key, value)| format!(" --{key}={value}"))
            .collect();
        assert_eq!(command, format!("{base}{added}"), "{arm}");
        let Some(Json::Object(settings)) = spec.get("settings") else {
            panic!("{arm}: settings")
        };
        assert_eq!(settings.len(), flags.len(), "{arm}");
        for (key, value) in flags {
            assert_eq!(
                settings.get(*key).and_then(Json::as_str),
                Some(*value),
                "{arm}: {key}"
            );
        }
    }
    let Some(Json::Array(pairs)) = json.get("pairs") else {
        panic!("pairs")
    };
    assert_eq!(pairs.len(), 2, "each candidate against enhanced");
}

/// The canary DSN rows hash as registered and are in the set file; every other row is
/// registered as outside the repository and named in the set file.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let json = json();
    let Some(Json::Object(rows)) = json.path("boards.rows") else {
        panic!("boards.rows")
    };
    let Some(Json::Array(outside)) = json.path("boards.outside_repository") else {
        panic!("boards.outside_repository")
    };
    let outside: Vec<&str> = outside.iter().filter_map(Json::as_str).collect();
    assert_eq!(
        outside,
        [
            "interf_u",
            "pic_programmer",
            "flight-computer-core-board",
            "cm4-edge-ai-carrier"
        ]
    );
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let mut in_repo = 0;
    for (row, spec) in rows {
        let want = json
            .path("boards.files")
            .and_then(|f| f.get(row))
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("boards.files.{row}"));
        if outside.contains(&row.as_str()) {
            assert!(
                set.lines().any(|l| l.starts_with('#') && l.contains(row)),
                "{row} is named in {SET}"
            );
            continue;
        }
        in_repo += 1;
        let input = spec.get("input").and_then(Json::as_str).expect("input");
        assert!(
            set.lines().any(|l| l.trim() == input),
            "{input} missing from {SET}"
        );
        let bytes = std::fs::read(repo().join(input)).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            want,
            "{row}"
        );
        assert_eq!(
            spec.get("board").and_then(Json::as_str),
            Some("canary-pwr"),
            "{row}"
        );
    }
    assert_eq!(in_repo, 2, "the two canary rows");
}

/// The run limit is registered with its decision, the cut arm is named, and the numbers seen
/// before registration are listed.
#[test]
fn the_run_limit_and_the_numbers_seen_before_are_registered() {
    let json = json();
    let limits = json
        .get("run_limits")
        .and_then(Json::as_str)
        .expect("run_limits");
    assert!(limits.contains("decided 2026-10-20: measurement runs stay short"));
    assert!(limits.contains("ulimit -v 8000000"));
    let Some(Json::Array(cut)) = json.get("not_registered") else {
        panic!("not_registered")
    };
    assert!(
        cut.iter()
            .filter_map(Json::as_str)
            .any(|c| c.starts_with("the arm bounded-neck without the widen-back"))
    );
    let seen = json
        .get("seen_before_registration")
        .and_then(Json::as_str)
        .expect("seen_before_registration");
    for needle in ["N05.3", "140.9", "309.9", "32.6", "Q17.1", "Q17.2", "Q10.4"] {
        assert!(seen.contains(needle), "{needle}");
    }
}

#[test]
fn the_q17_4_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "b3d4ad492fb09fccb150bf142448f40ed21d041c049d35b086c1aa566a6bc2d9"
    );
}

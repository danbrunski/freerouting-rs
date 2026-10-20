//! Pins N06.14's second registration (`docs/roadmap/prereg/N06.14b.json`): the simplex
//! cutouts' two Java-bug fixes, `router.java_quirks.cutout_hole_crosses_border` and
//! `simplex_octagon_half_corner`, each alone and both, against Java's cutouts, under profile
//! `enhanced` with the 2026-10-20 round-2 candidates fixed in every arm, judged by N02's
//! totals rule with `accept: no-regress`, and run by `tools/bench/n06_run.py`.
//!
//! Registered 2026-10-20 before any number under it, after `N06.14.json`'s run showed its
//! reach claim ("any-angle mode only") wrong: canary-pwr, a 45 degree board, reaches both
//! switches through its any-angle wiring (`tests/n06_cutout_reach_45.rs` in the engine
//! crate). A new file rather than an amendment, because `N06.14.json` has numbers. A later
//! change goes in the file's `amendments` with the reason and updates the hash here in the
//! same change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.14b.json";
const SET: &str = "tools/bench/sets/n06-g2b.txt";
const PLUGIN_ROW: &str = "canary-pwr-plugin";
/// The keys every arm sets, with the value every arm gives them (`None`: the arm's own).
const KEYS: [(&str, Option<&str>); 8] = [
    ("octagon_box_cutout_branch3", Some("true")),
    ("cutout_hole_crosses_border", None),
    ("simplex_octagon_half_corner", None),
    ("simplex_prev_division_line", Some("false")),
    ("any_angle_room_side_index", Some("false")),
    ("opt_first_pass_stops", Some("false")),
    ("opt_equal_key_skip", Some("false")),
    ("shove_via_into_pending_piece", Some("false")),
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

#[test]
fn the_n06_14b_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.14b-simplex-cutouts");
    assert_eq!(prereg.names.base, "baseline");
    assert_eq!(prereg.names.cand, "all");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "all",
            "baseline",
            "cutout_hole_crosses_border",
            "simplex_octagon_half_corner"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 8, "7 DSN rows and the plugin row");
    assert!(prereg.totals_registered);
    assert_eq!(prereg.amendments, 0);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    for name in [
        "via_in_pad",
        "below_minimum",
        "clearance_violations",
        "drc_real",
        "kicad_unconnected",
        "unrouted",
        "session_differences",
        "cutout_hole_crosses_border_reached",
        "simplex_octagon_half_corner_reached",
        "any_angle_room_side_mismatches",
    ] {
        assert!(metrics.contains(&name), "{name}");
    }
    let json = json();
    assert_eq!(
        json.path("totals.accept").and_then(Json::as_str),
        Some("no-regress")
    );
    for row in ["fixtures/canary-pwr/canary-pwr-9511d9a.dsn", PLUGIN_ROW] {
        assert_eq!(
            json.path("totals.noise")
                .and_then(|n| n.get(row))
                .and_then(Json::as_f64),
            Some(10.0),
            "{row}"
        );
    }
}

/// `n06_run.py` reads the arms in file order and needs the baseline first; each command
/// has its placeholders, gives the profile and sets the eight keys: the two N06.14 keys
/// per arm, the other six to the same value in every arm.
#[test]
fn each_arm_sets_its_two_switches_and_the_same_six_others() {
    let json = json();
    let text = text();
    let at = |arm: &str| text.find(&format!("\"{arm}\": {{")).expect(arm);
    let expect = [
        ("baseline", "true", "true"),
        ("cutout_hole_crosses_border", "false", "true"),
        ("simplex_octagon_half_corner", "true", "false"),
        ("all", "false", "false"),
    ];
    for (arm, border, half) in expect {
        assert!(
            at("baseline") <= at(arm),
            "n06_run.py needs the baseline first"
        );
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        assert!(command.starts_with('/'), "{arm}: an absolute binary path");
        assert!(
            command.contains(" --gui.enabled=false --router.profile=enhanced "),
            "{arm}: the explicit profile"
        );
        for placeholder in ["{DSN}", "{SES}", "{STATS}", "{RESULT}"] {
            assert!(command.contains(placeholder), "{arm}: {placeholder}");
        }
        assert_eq!(command.matches("--router.java_quirks.").count(), KEYS.len());
        let mut off = 0;
        for (id, common) in KEYS {
            let value = match (id, common) {
                (_, Some(v)) => v,
                ("cutout_hole_crosses_border", None) => border,
                _ => half,
            };
            off += usize::from(value == "false");
            assert!(
                command.contains(&format!("--router.java_quirks.{id}={value}")),
                "{arm}: {command}"
            );
        }
        let Some(Json::Object(settings)) = spec.get("settings") else {
            panic!("{arm}: settings")
        };
        assert_eq!(settings.len(), off, "{arm}: settings lists the false keys");
        assert!(settings.values().all(|v| *v == Json::Bool(false)), "{arm}");
    }
}

/// The rows: canary-pwr's DSN and plugin rows are measured, J2 is the only identity row,
/// and the consistency check names the two rows whose stored areas reach a switch.
#[test]
fn the_rows_and_the_consistency_check_follow_the_corrected_reach_claim() {
    let json = json();
    let rule = json
        .path("pairs_rule")
        .and_then(Json::as_str)
        .expect("pairs_rule");
    assert!(rule.contains("Every row is a measured row"), "{rule}");
    assert!(
        rule.contains("Identity row: fixtures/Issue026-J2_reference.dsn"),
        "{rule}"
    );
    assert!(rule.contains("No other row is an identity row"), "{rule}");
    for row in [
        "fixtures/Issue756-tomu-fpga8.dsn",
        "fixtures/Issue756-tomu-fpga9.dsn",
    ] {
        assert!(rule.contains(row), "the consistency check excludes {row}");
    }
    let reach = json.path("reach").and_then(Json::as_str).expect("reach");
    for needle in [
        "ShapeSearchTree45Degree",
        "Polyline.offsetShapes",
        "isIntOctagon",
        "DrillPage.getDrills",
        "reduceTraceShapeAtTiePin",
        "tests/n06_cutout_reach_45.rs",
    ] {
        assert!(reach.contains(needle), "{needle}");
    }
    let seen = json
        .path("seen_before_registration")
        .and_then(Json::as_str)
        .expect("seen_before_registration");
    for needle in [
        "122",
        "129",
        "39",
        "44",
        "109",
        "4686",
        "1552",
        "196 -> 208",
    ] {
        assert!(seen.contains(needle), "amendment 2's numbers: {needle}");
    }
    assert_eq!(json.path("plugin_canary.required"), Some(&Json::Bool(true)));
    assert_eq!(
        json.path("plugin_canary.export.dsn_sha256"),
        json.path("boards.files").and_then(|f| f.get(PLUGIN_ROW)),
        "the plugin row is hashed as its export"
    );
}

/// Every DSN board is in the set file, and every input hashes as registered; the plugin
/// row is routed outside the runner, its export is not in the repository.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let json = json();
    let Some(Json::Object(all)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    let Some(Json::Array(outside)) = json.path("boards.outside_runner") else {
        panic!("boards.outside_runner")
    };
    let outside: Vec<&str> = outside.iter().filter_map(Json::as_str).collect();
    assert_eq!(outside, [PLUGIN_ROW]);
    let files: std::collections::BTreeMap<_, _> = all
        .iter()
        .filter(|(row, _)| !outside.contains(&row.as_str()))
        .collect();
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let mut listed: Vec<&str> = set
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect();
    let mut registered: Vec<&str> = files.keys().map(|k| k.as_str()).collect();
    registered.sort_unstable();
    listed.sort_unstable();
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
fn the_n06_14b_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "9417fb2c253a4e54d04406599ee7b2b8742ef64aae1ce4c0c2115fa1c6ef6b6b"
    );
}

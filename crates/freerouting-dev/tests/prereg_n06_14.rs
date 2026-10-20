//! Pins N06.14's registration (`docs/roadmap/prereg/N06.14.json`): the simplex cutouts'
//! two Java-bug fixes outside N06.4's domain, `router.java_quirks.cutout_hole_crosses_border`
//! and `simplex_octagon_half_corner`, each alone and both, against Java's cutouts, judged
//! by N02's totals rule with `accept: no-regress`, and run by `tools/bench/n06_run.py`.
//!
//! Registered 2026-10-18 before any routing number. A later change goes in the file's
//! `amendments` with the reason and updates the hash here in the same change.
//!
//! Amendment 1 (2026-10-18, before any N06.14 route): every `Area` split of the board
//! follows the cutout switches, so N06.4's accepted `octagon_box_cutout_branch3` is
//! re-measured at the wider reach by a fifth arm of its own name, on two more rows
//! (`Issue756-tomu-fpga8`, `-fpga9`), and every arm sets that key explicitly.
//!
//! Amendment 2 (2026-10-20, before any N06.14 route): main 08062e2 under an explicit
//! `--router.profile=enhanced` (30 promoted fixes since N06.13), every arm sets
//! `any_angle_room_side_index=false` (N06.16; decided 2026-10-20) and
//! `simplex_prev_division_line` (`false` only in the branch-3 arm), and the plugin row
//! `canary-pwr-plugin` joins `boards.files` as a row routed outside `n06_run.py`.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.14.json";
const SET: &str = "tools/bench/sets/n06-g2.txt";
const IDS: [&str; 2] = ["cutout_hole_crosses_border", "simplex_octagon_half_corner"];
/// The keys every arm sets (amendment 1 adds N06.4's branch-3 switch, amendment 2
/// N06.4's simplex switch and N06.16's room-side switch).
const KEYS: [&str; 5] = [
    "octagon_box_cutout_branch3",
    "cutout_hole_crosses_border",
    "simplex_octagon_half_corner",
    "simplex_prev_division_line",
    "any_angle_room_side_index",
];
/// Amendment 2: `false` in every arm.
const COMMON_OFF: &str = "any_angle_room_side_index";
/// Amendment 2: the row routed outside `n06_run.py`, hashed as the plugin's export.
const PLUGIN_ROW: &str = "canary-pwr-plugin";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

#[test]
fn the_n06_14_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.14-simplex-cutouts");
    assert_eq!(prereg.names.base, "baseline");
    assert_eq!(prereg.names.cand, "all");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "all",
            "baseline",
            "cutout_hole_crosses_border",
            "octagon_box_cutout_branch3",
            "simplex_octagon_half_corner"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 8, "7 DSN rows and the plugin row");
    assert!(prereg.totals_registered);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    for name in [
        "via_in_pad",
        "below_minimum",
        "clearance_violations",
        "kicad_unconnected",
        "unrouted",
        "session_differences",
        "cutout_hole_crosses_border_reached",
        "simplex_octagon_half_corner_reached",
        "octagon_box_cutout_branch3_reached",
        "simplex_prev_division_line_reached",
        "any_angle_room_side_mismatches",
    ] {
        assert!(metrics.contains(&name), "{name}");
    }
}

/// `n06_run.py` reads the arms in file order and needs the baseline first; each command
/// has its placeholders, gives the profile and sets the five keys, only its own and
/// `any_angle_room_side_index` to `false`.
#[test]
fn each_arm_sets_only_its_switches_and_runs_under_n06_run() {
    let json = Json::parse(&text()).unwrap();
    let text = text();
    let at = |arm: &str| text.find(&format!("\"{arm}\": {{")).expect(arm);
    for arm in [
        "cutout_hole_crosses_border",
        "simplex_octagon_half_corner",
        "all",
        "octagon_box_cutout_branch3",
    ] {
        assert!(
            at("baseline") < at(arm),
            "n06_run.py needs the baseline first"
        );
    }
    let expect = [
        ("baseline", &[][..]),
        (
            "cutout_hole_crosses_border",
            &["cutout_hole_crosses_border"][..],
        ),
        (
            "simplex_octagon_half_corner",
            &["simplex_octagon_half_corner"][..],
        ),
        ("all", &IDS[..]),
        (
            "octagon_box_cutout_branch3",
            &["octagon_box_cutout_branch3", "simplex_prev_division_line"][..],
        ),
    ];
    for (arm, off) in expect {
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        assert!(command.starts_with('/'), "{arm}: an absolute binary path");
        assert!(
            command.contains(" --gui.enabled=false --router.profile=enhanced "),
            "{arm}: amendment 2's explicit profile"
        );
        for placeholder in ["{DSN}", "{SES}", "{STATS}", "{RESULT}"] {
            assert!(command.contains(placeholder), "{arm}: {placeholder}");
        }
        assert_eq!(command.matches("--router.java_quirks.").count(), KEYS.len());
        for id in KEYS {
            let value = if off.contains(&id) || id == COMMON_OFF {
                "false"
            } else {
                "true"
            };
            assert!(
                command.contains(&format!("--router.java_quirks.{id}={value}")),
                "{arm}: {command}"
            );
        }
        let Some(Json::Object(settings)) = spec.get("settings") else {
            panic!("{arm}: settings")
        };
        assert_eq!(settings.len(), off.len() + 1, "{arm}");
        assert_eq!(
            settings.get(&format!("router.java_quirks.{COMMON_OFF}")),
            Some(&Json::Bool(false)),
            "{arm}"
        );
    }
}

/// The 45 degree rows are the identity checks the registration names, and the plugin
/// canary is required.
#[test]
fn the_45_degree_rows_and_the_plugin_canary_are_registered() {
    let json = Json::parse(&text()).unwrap();
    let rule = json
        .path("pairs_rule")
        .and_then(Json::as_str)
        .expect("pairs_rule");
    for row in [
        "fixtures/Issue026-J2_reference.dsn",
        "fixtures/canary-pwr/canary-pwr-9511d9a.dsn",
        "fixtures/Issue756-tomu-fpga8.dsn",
        "fixtures/Issue756-tomu-fpga9.dsn",
    ] {
        assert!(
            !row.contains("tomu") || rule.contains(row),
            "amendment 1's rows are identity checks: {row}"
        );
        assert!(
            json.path("boards.files").and_then(|f| f.get(row)).is_some(),
            "{row}"
        );
    }
    assert!(rule.contains("session_differences 0"), "{rule}");
    assert_eq!(json.path("plugin_canary.required"), Some(&Json::Bool(true)));
}

/// Every DSN board is in the set file, and every input hashes as registered; the plugin
/// row (amendment 2) is routed outside the runner, its export is not in the repository.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let json = Json::parse(&text()).unwrap();
    let Some(Json::Object(all)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    let Some(Json::Array(outside)) = json.path("boards.outside_runner") else {
        panic!("boards.outside_runner")
    };
    let outside: Vec<&str> = outside.iter().filter_map(Json::as_str).collect();
    assert_eq!(outside, [PLUGIN_ROW]);
    assert_eq!(
        json.path("plugin_canary.export.dsn_sha256"),
        all.get(PLUGIN_ROW),
        "the plugin row is hashed as its export"
    );
    let files: std::collections::BTreeMap<_, _> = all
        .iter()
        .filter(|(row, _)| !outside.contains(&row.as_str()))
        .collect();
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let listed: Vec<&str> = set
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect();
    let mut registered: Vec<&str> = files.keys().map(|k| k.as_str()).collect();
    let mut listed = listed;
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
fn the_n06_14_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "987fbb62c5bac23da88584445f41aee20ccb151cd5b7e818272d66f682f8f431"
    );
}

/// Amendment 1: recorded, the branch-3 arm compared with the baseline and reported
/// as its own pair.
#[test]
fn amendment_1_re_measures_the_branch_3_switch_at_the_wider_reach() {
    let json = Json::parse(&text()).unwrap();
    let Some(Json::Array(amendments)) = json.path("amendments") else {
        panic!("amendments")
    };
    assert_eq!(amendments.len(), 2);
    assert_eq!(
        json.path("arms.octagon_box_cutout_branch3.baseline")
            .and_then(Json::as_str),
        Some("baseline")
    );
    let Some(Json::Array(reported)) = json.path("pairs.reported") else {
        panic!("pairs.reported")
    };
    let arms: Vec<Vec<&str>> = reported
        .iter()
        .map(|p| match p.get("arms") {
            Some(Json::Array(a)) => a.iter().filter_map(Json::as_str).collect(),
            _ => panic!("arms"),
        })
        .collect();
    assert!(arms.contains(&vec!["baseline", "octagon_box_cutout_branch3"]));
    let rule = json.path("pairs_rule").and_then(Json::as_str).unwrap();
    assert!(rule.contains("--arms baseline,octagon_box_cutout_branch3"));
}

/// Amendment 2: recorded, the plugin row has the registered band, and the switch
/// counters of N06.4's simplex switch and N06.16 are reported.
#[test]
fn amendment_2_bounds_the_any_angle_rows_and_names_the_profile() {
    let json = Json::parse(&text()).unwrap();
    let Some(Json::Array(amendments)) = json.path("amendments") else {
        panic!("amendments")
    };
    let changes = amendments[1]
        .get("changes")
        .and_then(Json::as_str)
        .expect("changes");
    for needle in [
        "any_angle_room_side_index=false",
        "--router.profile=enhanced",
        "main 08062e2",
        "8000000",
    ] {
        assert!(changes.contains(needle), "{needle}");
    }
    assert_eq!(
        json.path("totals.noise")
            .and_then(|n| n.get(PLUGIN_ROW))
            .and_then(Json::as_f64),
        Some(10.0)
    );
}

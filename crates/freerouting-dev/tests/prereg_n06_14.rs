//! Pins N06.14's registration (`docs/roadmap/prereg/N06.14.json`): the simplex cutouts'
//! two Java-bug fixes outside N06.4's domain, `router.java_quirks.cutout_hole_crosses_border`
//! and `simplex_octagon_half_corner`, each alone and both, against Java's cutouts, judged
//! by N02's totals rule with `accept: no-regress`, and run by `tools/bench/n06_run.py`.
//!
//! Registered 2026-10-18 before any routing number. A later change goes in the file's
//! `amendments` with the reason and updates the hash here in the same change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.14.json";
const SET: &str = "tools/bench/sets/n06-g2.txt";
const IDS: [&str; 2] = ["cutout_hole_crosses_border", "simplex_octagon_half_corner"];

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
            "simplex_octagon_half_corner"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 5);
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
    ] {
        assert!(metrics.contains(&name), "{name}");
    }
}

/// `n06_run.py` reads the arms in file order and needs the baseline first; each command
/// has its placeholders and sets both switches, only its own to `false`.
#[test]
fn each_arm_sets_only_its_switches_and_runs_under_n06_run() {
    let json = Json::parse(&text()).unwrap();
    let text = text();
    let at = |arm: &str| text.find(&format!("\"{arm}\": {{")).expect(arm);
    for arm in [
        "cutout_hole_crosses_border",
        "simplex_octagon_half_corner",
        "all",
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
    ];
    for (arm, off) in expect {
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        assert!(command.starts_with('/'), "{arm}: an absolute binary path");
        for placeholder in ["{DSN}", "{SES}", "{STATS}", "{RESULT}"] {
            assert!(command.contains(placeholder), "{arm}: {placeholder}");
        }
        assert_eq!(command.matches("--router.java_quirks.").count(), IDS.len());
        for id in IDS {
            let value = if off.contains(&id) { "false" } else { "true" };
            assert!(
                command.contains(&format!("--router.java_quirks.{id}={value}")),
                "{arm}: {command}"
            );
        }
        let Some(Json::Object(settings)) = spec.get("settings") else {
            panic!("{arm}: settings")
        };
        assert_eq!(settings.len(), off.len(), "{arm}");
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
    ] {
        assert!(
            json.path("boards.files").and_then(|f| f.get(row)).is_some(),
            "{row}"
        );
    }
    assert!(rule.contains("session_differences 0"), "{rule}");
    assert_eq!(json.path("plugin_canary.required"), Some(&Json::Bool(true)));
}

/// Every board is in the set file, and every input hashes as registered.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let json = Json::parse(&text()).unwrap();
    let Some(Json::Object(files)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let listed: Vec<&str> = set
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect();
    let registered: Vec<&str> = files.keys().map(String::as_str).collect();
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
        "1b945407a39a72fd9c27f061c73f842b28e6cb1529dfc5408173721f38680c99"
    );
}

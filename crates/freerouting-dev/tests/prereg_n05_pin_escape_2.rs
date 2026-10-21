//! Pins N05.3's pin-escape re-registration (`docs/roadmap/prereg/N05-pin-escape-2.json`):
//! `router.fanout.pin_escape = straight` against off under profile `enhanced`, on
//! N05-pin-escape's seven rows plus the three smallest eligible KiCad projects it did not use,
//! with the absolute guard (zero routing-caused manufacturing breaches, below-minimum copper
//! and vias in pads in each arm) and N02's totals rule with `accept: no-regress` and a lower
//! total (its `keep_rule`). Registered 2026-10-20 before any route or grade of either arm; it
//! runs after Q22.8. A later change goes in its `amendments` with the reason and updates the
//! hash below in the same change.
//!
//! The boards live under the main checkout's `target/` (not in git). A row's file is looked up
//! under the repository root and its ancestors (a worktree under `target/orchestrator/` finds the
//! main checkout's copies); a missing file is skipped, a present one must hash as registered.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N05-pin-escape-2.json";
const SET: &str = "tools/bench/sets/n05-pin-escape-2.txt";
/// The registration this one re-registers: its seven rows are kept with the same files.
const FIRST: &str = "docs/roadmap/prereg/N05-pin-escape.json";
const BASE: &str = "enhanced";
const CAND: &str = "enhanced+pin_escape";
/// The rows in set order: key, pad-to-pad connections, timeout (s), whether N05-pin-escape
/// had it.
const ROWS: [(&str, f64, f64, bool); 10] = [
    ("mydevboard-rp2040dev", 130.0, 120.0, true),
    ("canary-pwr", 309.0, 600.0, true),
    ("vfo-controller-catvfoctrl", 113.0, 120.0, true),
    ("flight-computer-core-board", 98.0, 120.0, true),
    (
        "ch32v003-usb-devboard-ch32v003-usb-devboard",
        74.0,
        120.0,
        true,
    ),
    (
        "solarmeshtasticnode-meshtasticnode-bq25185",
        102.0,
        120.0,
        true,
    ),
    ("cc1101-module-cc1101-module", 43.0, 120.0, true),
    ("vna-usb-pd", 73.0, 120.0, false),
    ("picodvi-picodvi", 102.0, 120.0, false),
    (
        "openmuscle-flexgrid-om-flexgrid-rigid-pcb",
        157.0,
        120.0,
        false,
    ),
];
/// The routing-caused KiCad types of the manufacturing guard, as Q22.json defines them
/// (routing-quality.md, order of importance item 1).
const MANUFACTURING: [&str; 10] = [
    "clearance",
    "hole_clearance",
    "hole_to_hole",
    "copper_edge_clearance",
    "track_width",
    "via_diameter",
    "annular_width",
    "drill_out_of_range",
    "solder_mask_bridge",
    "shorting_items",
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

/// The bytes of `path` (relative to a checkout root) under the repository root or the nearest
/// ancestor that has it.
fn find(path: &str) -> Option<Vec<u8>> {
    let root = repo().canonicalize().ok()?;
    root.ancestors()
        .find_map(|dir| std::fs::read(dir.join(path)).ok())
}

fn object<'a>(json: &'a Json, path: &str) -> &'a BTreeMap<String, Json> {
    match json.path(path) {
        Some(Json::Object(o)) => o,
        other => panic!("{path}: {other:?}"),
    }
}

fn string<'a>(json: &'a Json, path: &str) -> &'a str {
    json.path(path)
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("{path}"))
}

fn number(json: &Json, path: &str) -> f64 {
    match json.path(path) {
        Some(Json::Number(n)) => *n,
        other => panic!("{path}: {other:?}"),
    }
}

/// The set file's rows: (path, comment).
fn set_rows() -> Vec<(String, String)> {
    std::fs::read_to_string(repo().join(SET))
        .unwrap()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (path, comment) = l.split_once('#').unwrap_or((l, ""));
            (path.trim().to_string(), comment.trim().to_string())
        })
        .collect()
}

#[test]
fn the_registration_is_valid_and_names_its_arms_and_metrics() {
    let prereg = registration();
    assert_eq!(prereg.id, "N05.3-pin-escape-2");
    assert_eq!(prereg.names.base, BASE);
    assert_eq!(prereg.names.cand, CAND);
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, [BASE, CAND]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), ROWS.len());
    assert!(prereg.totals_registered);
    assert_eq!(prereg.amendments, 1);
    let roles: Vec<(&str, Role)> = prereg
        .specs
        .iter()
        .map(|s| (s.name.as_str(), s.role))
        .collect();
    assert_eq!(
        &roles[..4],
        [
            ("via_in_pad", Role::Hard),
            ("below_minimum", Role::Hard),
            ("manufacturing_breaches", Role::Hard),
            ("kicad_unconnected", Role::Score),
        ]
    );
    assert!(
        prereg.specs[4..]
            .iter()
            .all(|s| s.role == Role::Report && !s.compared)
    );
    // The verdict pair guards the candidate; the reported pair puts enhanced under the same
    // hard metrics, because the guard is absolute in each arm.
    let pairs: Vec<(&str, &str)> = prereg
        .pairs
        .iter()
        .map(|p| (p.arms.base.as_str(), p.arms.cand.as_str()))
        .collect();
    assert_eq!(pairs, [(BASE, CAND), (CAND, BASE)]);
}

/// The guards are absolute in each arm and count Q22.json's ten manufacturing types on the
/// copper basis; nothing is subtracted from the baseline.
#[test]
fn the_guards_are_absolute_in_each_arm() {
    let json = json();
    let types = MANUFACTURING.join(", ");
    let breaches = string(&json, "metric_definitions.manufacturing_breaches");
    assert!(breaches.contains(&types), "{breaches}");
    assert!(breaches.contains("routing_caused_basis = copper"));
    assert!(breaches.contains("absolute in each arm"));
    let q22 = std::fs::read_to_string(repo().join("docs/roadmap/prereg/Q22.json")).unwrap();
    let q22 = Json::parse(&q22).unwrap();
    assert!(string(&q22, "metric_definitions.manufacturing_breaches").contains(&types));
    let below = string(&json, "metric_definitions.below_minimum");
    assert!(below.contains("copper_key") && below.contains("input_copper_keys"));
    assert!(below.contains("absolute in each arm"));
    let guards = string(&json, "guards");
    assert!(guards.contains(&types), "{guards}");
    assert!(guards.contains("in EACH arm, on every row"));
    assert!(guards.contains("replaces N05-pin-escape's 'arm minus baseline, floored at 0'"));
    for name in ["via_in_pad", "below_minimum", "manufacturing_breaches"] {
        let def = string(&json, &format!("metric_definitions.{name}"));
        assert!(!def.contains("enhanced arm's raw count"), "{name}: {def}");
    }
    let keep = string(&json, "keep_rule");
    assert!(keep.contains("the guards hold in both arms on every row"));
    assert!(keep.contains("change < 0"));
}

/// The set lists exactly the registered inputs in the registered order, each with its
/// connections and timeout in the comment; each noise band is N05's rule; every input present
/// hashes as registered, and so do the rules files each grade reads; N05-pin-escape's seven
/// rows keep its files and counts.
#[test]
fn the_rows_match_the_set_with_their_bands_and_hashes() {
    let json = json();
    let first = Json::parse(&std::fs::read_to_string(repo().join(FIRST)).unwrap()).unwrap();
    let rows = object(&json, "boards.rows");
    let set = set_rows();
    assert_eq!(set.len(), ROWS.len());
    assert_eq!(rows.len(), ROWS.len());
    for ((path, comment), (row, connections, limit, kept)) in set.iter().zip(ROWS) {
        let spec = rows
            .get(row)
            .unwrap_or_else(|| panic!("{row} is not registered"));
        assert_eq!(
            spec.get("input").and_then(Json::as_str),
            Some(path.as_str())
        );
        assert!(
            path.starts_with("target/orchestrator/n05pe2/boards/"),
            "{path}"
        );
        assert!(
            comment.starts_with(&format!("{row}: {connections} connections, ")),
            "{row}: {comment}"
        );
        assert!(
            comment.ends_with(&format!(", {limit} s")),
            "{row}: {comment}"
        );
        assert_eq!(
            number(&json, &format!("boards.rows.{row}.connections")),
            connections
        );
        assert_eq!(
            number(&json, &format!("boards.rows.{row}.timeout_s")),
            limit
        );
        assert_eq!(spec.get("rules").and_then(Json::as_str), Some("project"));
        assert_eq!(spec.get("in_n05_pin_escape"), Some(&Json::Bool(kept)));
        let version = number(&json, &format!("boards.rows.{row}.kicad_pcb_version"));
        assert!(
            version >= 20210228.0,
            "{row}: {version} is older than KiCad 5.99"
        );
        let band = number(&json, &format!("totals.noise.{row}"));
        let want = if row == "canary-pwr" {
            10.0
        } else {
            (connections * 0.05).ceil().max(2.0)
        };
        assert_eq!(band, want, "{row}");
        let sha = string(&json, &format!("boards.files.{row}"));
        if kept {
            assert_eq!(string(&first, &format!("boards.files.{row}")), sha, "{row}");
            assert_eq!(
                number(&first, &format!("boards.rows.{row}.connections")),
                connections
            );
            assert_eq!(number(&first, &format!("totals.noise.{row}")), band);
        }
        let rules = object(&json, &format!("boards.rules_files.{row}"));
        assert!(rules.contains_key("kicad_pro"), "{row}");
        let Some(bytes) = find(path) else {
            continue;
        };
        assert_eq!(sha256_hex(&bytes), sha, "{row}");
        let dir = Path::new(path.as_str()).parent().unwrap();
        let stem = Path::new(path.as_str())
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        for (ext, want) in rules {
            let file = dir.join(format!("{stem}.{ext}"));
            let bytes = find(file.to_str().unwrap()).unwrap_or_else(|| panic!("{file:?}"));
            assert_eq!(Some(sha256_hex(&bytes).as_str()), want.as_str(), "{file:?}");
        }
    }
    let files = object(&json, "boards.files");
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        rows.keys().collect::<Vec<_>>()
    );
    let noise = object(&json, "totals.noise");
    assert_eq!(
        noise.keys().collect::<Vec<_>>(),
        rows.keys().collect::<Vec<_>>()
    );
    let set_noise = noise
        .values()
        .map(|b| match b {
            Json::Number(n) => n * n,
            other => panic!("{other:?}"),
        })
        .sum::<f64>()
        .sqrt();
    assert!(string(&json, "noise_why").contains(&format!("= {set_noise:.2}")));
}

/// The two arms differ by the plugin's checkbox and nothing else: the same options but
/// `pin_escape`, the same command but the arm's name, one registered setting that is a settings
/// field, and N02's totals rule with `accept: no-regress`.
#[test]
fn the_arms_differ_by_the_pin_escape_only() {
    let json = json();
    assert_eq!(json.path("rule").and_then(Json::as_str), Some("totals"));
    assert_eq!(
        json.path("totals.accept").and_then(Json::as_str),
        Some("no-regress")
    );
    let options = |arm: &str| -> BTreeMap<String, Json> {
        object(&json, &format!("arms.{arm}.options")).clone()
    };
    let off = options(BASE);
    let mut straight = options(CAND);
    assert_eq!(straight.remove("pin_escape"), Some(Json::Bool(true)));
    assert_eq!(off, straight);
    assert_eq!(off.get("profile").and_then(Json::as_str), Some("enhanced"));
    assert_eq!(off.get("schema"), Some(&Json::Number(5.0)));
    assert!(!off.contains_key("pin_escape"));
    assert!(object(&json, &format!("arms.{BASE}.settings")).is_empty());
    let settings = object(&json, &format!("arms.{CAND}.settings"));
    let keys: Vec<&str> = settings.keys().map(String::as_str).collect();
    assert_eq!(keys, ["router.fanout.pin_escape"]);
    assert_eq!(
        settings["router.fanout.pin_escape"].as_str(),
        Some("straight")
    );
    assert!(freerouting::settings::fields::field_by_path("fanout.pin_escape").is_some());
    let base = string(&json, &format!("arms.{BASE}.command"));
    let cand = string(&json, &format!("arms.{CAND}.command"));
    assert_ne!(base, cand);
    assert_eq!(cand.replace(CAND, BASE), base);
    for part in [
        "ulimit -v 8000000; timeout -k 10 LIMIT bash tools/kicad-oracle/grade.sh",
        "--export plugin OUT/enhanced.options.json BOARD",
        "-- ENGINE -de {DSN} -do {SES} --gui.enabled=false -mp 1 \
         --router.scoring.start_ripup_costs=1500",
    ] {
        assert!(base.contains(part), "{part} not in {base}");
    }
    assert!(!base.contains("--keep-tracks"));
    assert_eq!(
        string(&json, &format!("arms.{BASE}.engine")),
        string(&json, &format!("arms.{CAND}.engine"))
    );
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "741b55107260b88545bbbec14c0712dfaec1b5cb314f018889e63d455fb144da"
    );
}

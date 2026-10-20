//! Pins N03.3's preregistration (`docs/roadmap/prereg/N03.json`): under profile
//! `enhanced`, `router.room_order = canonical` with `router.work_unit = leaves`
//! against Java's tree order and nodes-visited unit, judged by N02's totals rule
//! with `accept: improve` and its speed clause (`tree_nodes_visited` down at least
//! 20 % with the signal within noise), on boards P04 did not use: PCBWorld D3-C,
//! canary-pwr's DSN and four KiCad demos routed with their tracks kept.
//!
//! Registered 2026-10-20 before any number, within that day's rule (decided
//! 2026-10-20: measurement runs stay a few minutes): every row but canary-pwr
//! routes in well under a minute, two arms. A later change goes in the file's
//! `amendments` with the reason and updates the hash here in the same change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::gate_totals::Accept;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N03.json";
const SET: &str = "tools/bench/sets/n03-room-order.txt";

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
fn the_n03_registration_is_valid_and_names_its_arms_rows_and_rule() {
    let prereg = registration();
    assert_eq!(prereg.id, "N03.3-canonical-room-order");
    assert_eq!(prereg.names.base, "tree+nodes");
    assert_eq!(prereg.names.cand, "canonical+leaves");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["canonical+leaves", "tree+nodes"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 15, "10 D3-C rows, canary-pwr, 4 demos");
    let totals = prereg.totals.expect("a totals block");
    assert_eq!(totals.accept, Accept::Improve);
    assert_eq!(totals.signal, ["kicad_unconnected", "unrouted"]);
    assert_eq!(totals.min_boards, 3);
    assert!(
        totals.speed.is_some(),
        "the speed clause on tree_nodes_visited"
    );
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..5],
        [
            "via_in_pad",
            "clearance_violations",
            "drc_real",
            "kicad_unconnected",
            "unrouted"
        ]
    );
    assert!(metrics.contains(&"tree_nodes_visited"));
    assert_eq!(prereg.amendments, 0);
}

/// The arms differ only in the two keys, each a setting, and both commands are the
/// same apart from them.
#[test]
fn the_arms_differ_only_in_room_order_and_work_unit() {
    let json = Json::parse(&text()).unwrap();
    let command = |arm: &str| {
        json.path(&format!("arms.{arm}.command"))
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("arms.{arm}.command"))
            .to_string()
    };
    assert_eq!(
        command("tree+nodes")
            .replace("=tree ", "=canonical ")
            .replace("=nodes", "=leaves"),
        command("canonical+leaves")
    );
    for arm in ["tree+nodes", "canonical+leaves"] {
        let Some(Json::Object(settings)) = json.path(&format!("arms.{arm}.settings")) else {
            panic!("{arm}: settings")
        };
        for (key, value) in settings {
            let value = value.as_str().expect("a string value");
            assert!(command(arm).contains(&format!("--{key}={value}")), "{arm}");
            let path = key.strip_prefix("router.").expect("a router key");
            assert!(
                freerouting::settings::fields::field_by_path(path).is_some(),
                "{arm}: {key} is not a setting"
            );
        }
    }
}

/// The set lists every row's input once; the inputs present on this machine hash
/// as registered (the D3-C corpus, KiCad's demos and canary may be missing).
#[test]
fn the_registered_inputs_hash_as_registered() {
    let json = Json::parse(&text()).unwrap();
    let Some(Json::Object(rows)) = json.path("boards.rows") else {
        panic!("boards.rows")
    };
    let files = json.path("boards.files").expect("boards.files");
    let set: Vec<String> = std::fs::read_to_string(repo().join(SET))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(set.len(), rows.len(), "{SET} lists every row once");
    for (row, spec) in rows {
        let input = spec.get("input").and_then(Json::as_str).expect("input");
        assert!(set.iter().any(|l| l == input), "{input} missing from {SET}");
        let want = files
            .get(row)
            .and_then(Json::as_str)
            .expect("a hash per row");
        let full = if input.starts_with('/') {
            PathBuf::from(input)
        } else {
            repo().join(input)
        };
        let Ok(bytes) = std::fs::read(&full) else {
            assert!(
                ["/usr/share/kicad/demos/", "target/corpus-d3c/"]
                    .iter()
                    .any(|p| input.starts_with(p)),
                "{input} is missing"
            );
            continue;
        };
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            want,
            "{row}"
        );
    }
}

#[test]
fn the_n03_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "f60fab8344a4e00f63f5576373a9f1e66872b4bf41473b0cbf8277cf939be416"
    );
}

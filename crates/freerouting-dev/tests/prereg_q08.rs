//! Pins Q08's preregistration (`docs/roadmap/prereg/Q08.json`, Q08.3):
//! whether `router.autorouter.connection_router = earned-budget` (Q08.1, E =
//! 4) lowers the unrouted total over the benchmark boards whose default run
//! hits a maze work limit, plus canary-pwr, judged by N02's totals rule with
//! `accept: improve`, the guards vias and clearance violations per row, no
//! via in a pad, and a median wall time at most 25 % above the default's.
//!
//! Registered 2026-10-18 as
//! 066c043306e327033f743a5a800c76b0c65eeb3f742d651b8bfa4f3ac455ec91, before
//! any registered number. Amendment 1 (2026-10-18) discloses spot runs of
//! both arms on five benchmark boards that preceded it (no effect seen) and
//! changes nothing else. Amendment 2 (decided 2026-10-20: measurement runs stay a
//! few minutes) keeps the 8 quick benchmark boards and `canary-pwr-pours1`
//! (`tools/bench/sets/quick-benchmark.txt`), drops the quiet-machine condition and
//! names the determinism boards. Amendment 3 (2026-10-20, after the default
//! arm's run, before any `earned-budget` run) narrows `files` to the eligible
//! rows: the default arm counted no maze work-limit hit on any benchmark row,
//! so only `canary-pwr-pours1` stays. The hash below is the amended file's;
//! any later change goes in its `amendments` with the reason and updates the
//! hash below in the same change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::gate_totals::Accept;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q08.json";

/// The candidate's router id and its E, as the engine had them when Q08.3
/// measured it (`quality::budget::{ROUTER_ID, DEFAULT_MAX_EXTENSIONS}`). The
/// router failed the keep rule and was removed (decided by Q08.3's keep rule,
/// 2026-10-20); this file stays as the record of the registration.
const REMOVED_ROUTER_ID: &str = "earned-budget";
const REMOVED_MAX_EXTENSIONS: u32 = 4;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("Q08 registration is invalid: {e:?}"))
}

#[test]
fn the_q08_registration_is_valid_and_names_its_arms_boards_and_rule() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q08.3-earned-budget");
    assert_eq!(prereg.names.base, "default");
    assert_eq!(prereg.names.cand, "earned-budget");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["default", "earned-budget"]);
    // Amendment 3: no benchmark row is eligible; canary-pwr-pours1 stays.
    assert_eq!(prereg.boards.len(), 1);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..4],
        ["via_in_pad", "vias", "clearance_violations", "unrouted"]
    );
    let totals = prereg.totals.expect("a totals block");
    assert_eq!(totals.accept, Accept::Improve);
    assert_eq!(totals.signal, ["unrouted"]);
    assert_eq!(totals.noise.get("canary-pwr"), Some(&10.0));
    assert_eq!(totals.min_boards, 3);
    assert_eq!(totals.speed, None);
    assert_eq!(prereg.amendments, 3);
}

/// The candidate's command is the default's plus the router id, and the
/// registered E is the one the engine had.
#[test]
fn the_arms_differ_only_in_the_connection_router() {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let json = Json::parse(&text).unwrap();
    let command = |arm: &str| {
        json.path(&format!("arms.{arm}.command"))
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("arms.{arm}.command"))
            .to_string()
    };
    assert_eq!(
        command("earned-budget"),
        format!(
            "{} --router.autorouter.connection_router={}",
            command("default"),
            REMOVED_ROUTER_ID
        )
    );
    let e = json.path("defaults.E").and_then(Json::as_str).expect("E");
    assert!(e.starts_with(&format!("{REMOVED_MAX_EXTENSIONS} ")));
}

/// Every registered board is in the repository and hashes as registered.
#[test]
fn the_registered_boards_hash_as_registered() {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let json = Json::parse(&text).unwrap();
    let Some(Json::Object(files)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    for (path, want) in files {
        let bytes = std::fs::read(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        let got = freerouting::quality::feasibility::report::sha256_hex(&bytes);
        assert_eq!(Some(got.as_str()), want.as_str(), "{path}");
    }
}

#[test]
fn the_q08_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "a53e55772fb2a14ed4637f91cedc7c499158913e4a376b2998d42103ebf25bb2"
    );
}

/// Amendment 2: every row is in the quick set, and the reason is the rule's.
/// Amendment 3 keeps a subset of those rows.
#[test]
fn amendment_2_keeps_only_the_quick_rows() {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let json = Json::parse(&text).unwrap();
    let Some(Json::Array(amendments)) = json.path("amendments") else {
        panic!("amendments")
    };
    let reason = amendments[1].get("reason").and_then(Json::as_str).unwrap();
    assert!(reason.starts_with("decided 2026-10-20: measurement runs stay a few minutes"));
    let set = std::fs::read_to_string(repo().join("tools/bench/sets/quick-benchmark.txt")).unwrap();
    let listed: Vec<&str> = set
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    let Some(Json::Object(files)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    let rows: Vec<&str> = files.keys().map(String::as_str).collect();
    assert!(rows.iter().all(|row| listed.contains(row)), "{rows:?}");
}

/// Amendment 3: `files` is the eligible rows only, quoting every quick row's
/// default-arm count, and the canary-pwr row stays whatever its count.
#[test]
fn amendment_3_keeps_the_eligible_rows() {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let json = Json::parse(&text).unwrap();
    let Some(Json::Array(amendments)) = json.path("amendments") else {
        panic!("amendments")
    };
    let changes = amendments[2].get("changes").and_then(Json::as_str).unwrap();
    let set = std::fs::read_to_string(repo().join("tools/bench/sets/quick-benchmark.txt")).unwrap();
    for row in set
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
    {
        let name = Path::new(row).file_stem().unwrap().to_str().unwrap();
        assert!(changes.contains(&format!("{name} 0")), "{name}");
    }
    let Some(Json::Object(files)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    let rows: Vec<&str> = files.keys().map(String::as_str).collect();
    assert_eq!(rows, ["fixtures/canary-pwr/canary-pwr-pours1.dsn"]);
}

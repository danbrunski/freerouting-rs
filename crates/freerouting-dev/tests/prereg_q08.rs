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
//! changes nothing else; the hash below is the amended file's. The amendment it
//! announces (narrowing `files` to the eligible rows from the default arm's
//! counts) and any later change go in its `amendments` with the reason and
//! update the hash below in the same change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::gate_totals::Accept;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q08.json";

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
    // The 16 benchmark boards and the 5 in-repository canary-pwr rows,
    // before the eligibility amendment.
    assert_eq!(prereg.boards.len(), 21);
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
    assert_eq!(prereg.amendments, 1);
}

/// The candidate's command is the default's plus the router id, and the
/// registered E is the engine's.
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
            freerouting::quality::budget::ROUTER_ID
        )
    );
    let e = json.path("defaults.E").and_then(Json::as_str).expect("E");
    assert!(e.starts_with(&format!(
        "{} ",
        freerouting::quality::budget::DEFAULT_MAX_EXTENSIONS
    )));
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
        "faa4188e1f510e58e2d29963ba312a62e7815a420e4464ef986a7416d044be0a"
    );
}

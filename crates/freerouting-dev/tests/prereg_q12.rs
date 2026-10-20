//! Pins Q12's preregistration (`docs/roadmap/prereg/Q12.json`, Q12.5): the
//! pass-1 portfolio (`router.autorouter.portfolio = 4`), with two
//! squeaky-wheel restarts, and composed with Q11's transactional pass 1 and
//! Q10's sweep, against the default passes (`mpdef`, the work reference) and
//! pass 1 alone (`mp1`), judged by N02's totals rule. Registered 2026-10-18
//! before any measured number (the spot checks of Q12.1-Q12.3's done notes
//! run the portfolio arm alone, for crashes and determinism) as
//! 342635f960aba7a644f97cceb009c9274dd2be62b62fd80b1bc4a7f065b2e9ab. A
//! later change (the `eligible` list, Q11.6's rule for the composed arm)
//! goes in its `amendments` with the reason and updates the hash below in
//! the same change.
//!
//! Amendment 1 (decided 2026-10-20: measurement runs stay a few minutes, before any
//! registered run): the 15 quick rows of `tools/bench/sets/q10-sweep-quick.txt`, the arms
//! `mpdef`, `portfolio4` and `mp1` (Q11.json's `mp1` command, so one run serves both), one
//! KiCad grade per distinct session.
//!
//! Amendment 2 (2026-10-20, before any registered run): the engine (main 3330f36, the build of
//! Q11.json's amendment 2), the runner and the metric sources made precise, hard guards "no new
//! breach versus baseline" (one records file per pair).
//!
//! Amendment 3 (2026-10-20, after `mp1`'s runs and before any candidate run): `eligible`, the
//! same 11 rows as Q11.json's; the hash below is the amended file's.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q12.json";
const SET: &str = "tools/bench/sets/q10-sweep-quick.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("Q12 registration is invalid: {e:?}"))
}

#[test]
fn the_q12_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q12.5-pass1-portfolio");
    assert_eq!(prereg.names.base, "mpdef");
    assert_eq!(prereg.names.cand, "portfolio4");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["mp1", "mpdef", "portfolio4"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 15);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..6],
        [
            "via_in_pad",
            "below_minimum",
            "drc_real",
            "pour_lost_pins",
            "kicad_unconnected",
            "unrouted"
        ]
    );
    // Every registered input is in the set file; those in the repository
    // hash as registered (the corpus rows are skipped while missing).
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    for (path, sha) in &prereg.boards {
        assert!(set.contains(path.as_str()), "{path} missing from the set");
        let Ok(bytes) = std::fs::read(repo().join(path)) else {
            assert!(path.starts_with("target/corpus/"), "{path} is missing");
            continue;
        };
        assert_eq!(
            &freerouting::quality::feasibility::report::sha256_hex(&bytes),
            sha,
            "{path}"
        );
    }
}

/// The totals block is N02's, and every arm command names the settings the
/// arm registers.
#[test]
fn the_q12_registration_uses_the_totals_rule_and_its_settings_exist() {
    let json = Json::parse(&text()).unwrap();
    assert_eq!(json.path("rule").and_then(Json::as_str), Some("totals"));
    assert_eq!(
        json.path("totals.accept").and_then(Json::as_str),
        Some("improve")
    );
    let Some(Json::Object(arms)) = json.path("arms") else {
        panic!("arms")
    };
    for (name, arm) in arms {
        let command = arm.get("command").and_then(Json::as_str).expect("command");
        let Some(Json::Object(settings)) = arm.get("settings") else {
            panic!("{name}: settings")
        };
        for (key, value) in settings {
            let value = value.as_str().expect("a string value");
            assert!(
                command.contains(&format!("--{key}={value}")),
                "{name}: {key}={value} not in {command}"
            );
            let path = key.strip_prefix("router.").expect("a router key");
            assert!(
                freerouting::settings::fields::field_by_path(path).is_some(),
                "{name}: {key} is not a setting"
            );
        }
    }
}

#[test]
fn the_q12_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "840f008bc17a5e1b3e1d1414cdc6cfe3f87a3e9e59490ed8d792ae81d871764d"
    );
}

/// Amendment 1: `mp1` is Q11.json's `mp1`, so its runs serve both registrations.
#[test]
fn amendment_1_shares_mp1_with_q11() {
    let json = Json::parse(&text()).unwrap();
    let q11 = std::fs::read_to_string(repo().join("docs/roadmap/prereg/Q11.json")).unwrap();
    let q11 = Json::parse(&q11).unwrap();
    let command = |j: &Json| {
        j.path("arms.mp1.command")
            .and_then(Json::as_str)
            .unwrap()
            .to_string()
    };
    assert_eq!(command(&json), command(&q11));
    let Some(Json::Array(amendments)) = json.path("amendments") else {
        panic!("amendments")
    };
    let reason = amendments[0].get("reason").and_then(Json::as_str).unwrap();
    assert!(reason.starts_with("decided 2026-10-20: measurement runs stay a few minutes"));
}

/// Amendment 2 is Q11.json's amendment 2 (one build and one grading for the shared runs), plus
/// the portfolio's work sum.
#[test]
fn amendment_2_matches_q11s() {
    let amendment = |file: &str| {
        let text = std::fs::read_to_string(repo().join(file)).unwrap();
        let json = Json::parse(&text).unwrap();
        let Some(Json::Array(amendments)) = json.path("amendments") else {
            panic!("amendments")
        };
        amendments[1]
            .get("changes")
            .and_then(Json::as_str)
            .unwrap()
            .to_string()
    };
    let q11 = amendment("docs/roadmap/prereg/Q11.json");
    let q12 = amendment(FILE);
    assert!(q12.starts_with(&q11));
    assert!(q12.contains("every portfolio arm"));
}

/// Amendment 3: the eligible rows are Q11.json's (one `mp1` run decides both).
#[test]
fn amendment_3_shares_eligible_with_q11() {
    let eligible = |file: &str| {
        let text = std::fs::read_to_string(repo().join(file)).unwrap();
        let json = Json::parse(&text).unwrap();
        let Some(Json::Array(rows)) = json.path("eligible") else {
            panic!("eligible")
        };
        rows.iter()
            .map(|r| r.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let q12 = eligible(FILE);
    assert_eq!(q12.len(), 11);
    assert_eq!(q12, eligible("docs/roadmap/prereg/Q11.json"));
}

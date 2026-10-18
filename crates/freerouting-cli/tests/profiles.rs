//! N01.3 and N01.4 on the built binary: `--profile legacy|enhanced`,
//! `--router.profile=`, `--list-stages[=json]` and `--dump-settings[=json]`.

mod common;

use common::*;
use freerouting::autoroute::stages::StageRegistry;
use freerouting::autoroute::stages::catalog::registered_ids;
use freerouting::drc::json::JsonValue;

const FIXTURE: &str = "Issue026-J2_reference.dsn";

fn route(name: &str, extra: &[&str]) -> (std::process::Output, std::path::PathBuf) {
    let dir = scratch(name);
    copy_fixtures(&dir, &[FIXTURE]);
    let mut args = vec!["-de", FIXTURE, "-do", "out.ses", "-mp", "3"];
    args.extend_from_slice(extra);
    (run_in(&dir, &args), dir)
}

fn array(value: Option<&JsonValue>) -> &[JsonValue] {
    match value {
        Some(JsonValue::Array(items)) => items,
        _ => &[],
    }
}

/// Both profiles route; `enhanced` (the default, no flag) equals `legacy`
/// on this board, which its differences so far (N05.3) do not touch:
/// `power_layer_traces = none` acts only on power layers, and this board has
/// none; `neck_width_um = 300` acts only on a class wider than 300 um, and
/// its one class is 250 um wide.
#[test]
fn both_profiles_route_and_enhanced_is_the_default() {
    let (legacy, dir_legacy) = route("profile-legacy", &["--profile", "legacy"]);
    let (enhanced, dir_enhanced) = route("profile-enhanced", &["--router.profile=enhanced"]);
    let (default, dir_default) = route("profile-default", &[]);
    for out in [&legacy, &enhanced, &default] {
        assert_eq!(code(out), 0, "{}", stderr(out));
    }
    let ses = |dir: &std::path::Path| std::fs::read(dir.join("out.ses")).unwrap();
    assert!(!ses(&dir_legacy).is_empty());
    assert!(
        ses(&dir_enhanced) == ses(&dir_default),
        "enhanced is the default"
    );
    assert!(
        ses(&dir_legacy) == ses(&dir_enhanced),
        "enhanced equals legacy on a board its differences do not touch"
    );
}

#[test]
fn an_unknown_profile_is_a_usage_error() {
    let message = "router.profile: unknown profile 'best' (valid profiles: legacy, enhanced)";
    let (out, _) = route("profile-unknown", &["--profile", "best"]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert!(stderr(&out).contains(message), "{}", stderr(&out));
    // Through the settings path the job refuses it before routing.
    let (out, dir) = route("profile-unknown-path", &["--router.profile=best"]);
    assert_ne!(code(&out), 0);
    assert!(stderr(&out).contains(message), "{}", stderr(&out));
    assert!(!dir.join("out.ses").exists());
}

#[test]
fn list_stages_json_lists_every_registered_id() {
    let dir = scratch("list-stages-json");
    let out = run_in(&dir, &["--list-stages=json", "--profile", "legacy"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let json = JsonValue::parse(&stdout(&out)).expect("JSON");
    assert_eq!(
        string_of(member(&json, "profile")).as_deref(),
        Some("legacy")
    );
    let profiles: Vec<String> = array(member(&json, "profiles"))
        .iter()
        .filter_map(|p| string_of(member(p, "name")))
        .collect();
    assert_eq!(profiles, ["legacy", "enhanced"]);
    let mut listed = std::collections::BTreeSet::new();
    for slot in array(member(&json, "slots")) {
        assert!(string_of(member(slot, "key")).is_some_and(|k| k.starts_with("router.")));
        for id in array(member(slot, "ids")) {
            if let JsonValue::String(id) = id {
                listed.insert(id.clone());
            }
        }
    }
    for (kind, ids) in registered_ids(&StageRegistry::builtin()) {
        for id in ids {
            assert!(
                listed.contains(id),
                "{kind}: {id} is not in --list-stages=json"
            );
        }
    }
}

#[test]
fn list_stages_table_lists_every_registered_id() {
    let dir = scratch("list-stages-table");
    let out = run_in(&dir, &["--list-stages"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("Routing profile: enhanced"), "{text}");
    for (kind, ids) in registered_ids(&StageRegistry::builtin()) {
        for id in ids {
            assert!(text.contains(id), "{kind}: {id} is not in --list-stages");
        }
    }
    assert!(text.contains("router.autorouter.connection_router"));
    assert!(text.contains("router.fanout.max_passes = 20"));
}

#[test]
fn dump_settings_shows_the_profile_and_the_sources() {
    let dir = scratch("dump-settings");
    copy_fixtures(&dir, &[FIXTURE]);
    let out = run_in(
        &dir,
        &[
            "--dump-settings",
            "--profile",
            "legacy",
            "--router.scoring.via_costs=42",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("Routing profile: legacy"), "{text}");
    let via = text
        .lines()
        .find(|l| l.contains(" scoring.via_costs "))
        .expect("the via cost line");
    assert!(via.starts_with('*') && via.contains("= 42  (CLI)"), "{via}");

    // JSON, with the design loaded as for a run.
    let out = run_in(&dir, &["-de", FIXTURE, "--dump-settings=json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let json = JsonValue::parse(&stdout(&out)).expect("JSON");
    assert_eq!(
        string_of(member(&json, "profile")).as_deref(),
        Some("enhanced")
    );
    let rows = array(member(&json, "settings"));
    let row = |path: &str| {
        rows.iter()
            .find(|r| string_of(member(r, "path")).as_deref() == Some(path))
            .unwrap_or_else(|| panic!("{path} is dumped"))
    };
    assert_eq!(
        member(row("autorouter.max_passes"), "from_profile"),
        Some(&JsonValue::Bool(false))
    );
    assert_eq!(
        string_of(member(row("autorouter.max_passes"), "source")).as_deref(),
        Some("defaults")
    );
    assert_eq!(rows.len(), freerouting::settings::FIELDS.len());

    let out = run_in(&dir, &["--dump-settings=xml"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn help_and_capabilities_document_the_profile() {
    let dir = scratch("profile-help");
    let help = stdout(&run_in(&dir, &["--help"]));
    for text in [
        "--profile <legacy|enhanced>",
        "--list-stages",
        "--dump-settings",
        "ROUTING PROFILES",
    ] {
        assert!(help.contains(text), "{text} is not in the help");
    }
    let caps = stdout(&run_in(&dir, &["--capabilities"]));
    for flag in ["\"--profile\"", "\"--list-stages\"", "\"--dump-settings\""] {
        assert!(caps.contains(flag), "{flag} is not in --capabilities");
    }
}

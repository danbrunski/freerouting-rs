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
/// its one class is 250 um wide. N06.13's Java-bug fixes do not change its
/// session either (N06's batches routed it identically under every switch).
#[test]
fn both_profiles_route_and_enhanced_is_the_default() {
    let (legacy, dir_legacy) = route("profile-legacy", &["--profile", "legacy"]);
    let (enhanced, dir_enhanced) = route("profile-enhanced", &["--router.profile=enhanced"]);
    let (default, dir_default) = route("profile-default", &[]);
    let (unfloored, dir_unfloored) = route(
        "profile-enhanced-unfloored",
        &[
            "--router.profile=enhanced",
            "--router.board_minimums.from_classes=false",
        ],
    );
    for out in [&legacy, &enhanced, &default, &unfloored] {
        assert_eq!(code(out), 0, "{}", stderr(out));
    }
    let ses = |dir: &std::path::Path| std::fs::read(dir.join("out.ses")).unwrap();
    assert!(!ses(&dir_legacy).is_empty());
    assert!(
        ses(&dir_enhanced) == ses(&dir_default),
        "enhanced is the default"
    );
    // Q18's class floor (`board_minimums.from_classes`) touches J2's pin
    // neck-downs; without it, `enhanced` routes J2 as `legacy` does.
    assert!(
        ses(&dir_legacy) == ses(&dir_unfloored),
        "enhanced equals legacy on a board its other differences do not touch"
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
fn list_stages_marks_the_experimental_options_and_stages() {
    let dir = scratch("list-stages-experimental");
    let out = run_in(&dir, &["--list-stages=json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let json = JsonValue::parse(&stdout(&out)).expect("JSON");
    let listed: Vec<String> = array(member(&json, "experimental"))
        .iter()
        .filter_map(|e| string_of(member(e, "key")))
        .collect();
    for key in ["router.room_order", "router.work_unit", "krt-diffpair"] {
        assert!(listed.iter().any(|k| k == key), "{key} in {listed:?}");
    }
    // The option is marked inside its slot too.
    let mut marked = Vec::new();
    for slot in array(member(&json, "slots")) {
        for option in array(member(slot, "options")) {
            if matches!(member(option, "experimental"), Some(JsonValue::Bool(true))) {
                marked.extend(string_of(member(option, "key")));
            }
        }
    }
    marked.sort();
    assert_eq!(marked, ["router.room_order", "router.work_unit"]);
    let out = run_in(&dir, &["--list-stages"]);
    let text = stdout(&out);
    assert!(
        text.contains("router.room_order = (unset)  [experimental]"),
        "{text}"
    );
    assert!(
        text.contains("\nExperimental (off in every profile"),
        "{text}"
    );
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

/// N06.13: with no flag the job runs `enhanced`, whose table turns off the
/// 37 promoted `router.java_quirks` switches; `legacy` leaves every switch
/// unset (Java), and an explicit `true` beats the profile.
#[test]
fn dump_settings_shows_the_promoted_java_quirks() {
    let dir = scratch("dump-settings-quirks");
    copy_fixtures(&dir, &[FIXTURE]);
    let quirks = |extra: &[&str]| {
        let mut args = vec!["-de", FIXTURE, "--dump-settings=json"];
        args.extend_from_slice(extra);
        let out = run_in(&dir, &args);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        let json = JsonValue::parse(&stdout(&out)).expect("JSON");
        array(member(&json, "settings"))
            .iter()
            .filter_map(|r| {
                let path = string_of(member(r, "path"))?;
                let id = path.strip_prefix("java_quirks.")?.to_string();
                let from_profile = member(r, "from_profile") == Some(&JsonValue::Bool(true));
                Some((id, member(r, "value").cloned(), from_profile))
            })
            .collect::<Vec<_>>()
    };
    let promoted: Vec<&str> = freerouting::settings::profile::ENHANCED
        .iter()
        .filter_map(|d| d.path.strip_prefix("java_quirks."))
        .collect();
    assert_eq!(promoted.len(), 37);
    let default = quirks(&[]);
    assert_eq!(default.len(), freerouting::settings::JavaQuirks::IDS.len());
    for (id, value, from_profile) in &default {
        let on = promoted.contains(&id.as_str());
        assert_eq!(*from_profile, on, "{id}");
        if on {
            assert_eq!(value.as_ref(), Some(&JsonValue::Bool(false)), "{id}");
        }
    }
    let legacy = quirks(&["--profile", "legacy"]);
    assert!(legacy.iter().all(|(_, _, p)| !p), "{legacy:?}");
    assert_eq!(legacy.len(), default.len());
    let explicit = quirks(&["--router.java_quirks.ses_reader_scale=true"]);
    let ses = explicit
        .iter()
        .find(|(id, _, _)| id == "ses_reader_scale")
        .expect("the switch is dumped");
    assert_eq!(ses.1, Some(JsonValue::Bool(true)));
    assert!(!ses.2, "the CLI beats the profile");
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

#[test]
fn help_names_every_experimental_entry() {
    use freerouting::autoroute::stages::catalog::EXPERIMENTAL;
    let help = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/help.txt"))
        .expect("help.txt");
    for e in EXPERIMENTAL {
        assert!(help.contains(e.key), "{} is not in help.txt", e.key);
    }
}

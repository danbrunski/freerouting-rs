//! C02.8: validation, test profiles and the comparison with Java's
//! `settings` dumps (C01.1).
//!
//! The comparison reads `fixtures/reference/settings/*.json` (and
//! `cases/settings/` for the synthetic boards) for the scenarios `default`,
//! `fixture`, `cli` and `owner-config`, and compares the static part of the
//! settings tree: everything except `layers`, the cost arrays and
//! `costs_initialized`, which C03 compares after the board tuning. A board
//! with an adjacent rules file gets the rules file's `autoroute_settings` as
//! `RoutingJobScheduler` and `RulesReader.read` apply them (C04.3): the
//! rules source at priority 40 in the second merge, then the file's scopes
//! read against the board's layers on top of the merged result. The rest of
//! the rules file (the board part) is compared by `job_prepare_reference.rs`.

// Patches are built field by field, as the Java tests build `RouterSettings`.
#![allow(clippy::field_reassign_with_default)]

use std::path::{Path, PathBuf};

use freerouting::drc::json::JsonValue;
use freerouting::dsn::ast::Pcb;
use freerouting::dsn::ast::RulesFile;
use freerouting::io::specctra::rules_reader::autoroute_patch;
use freerouting::settings::autoroute_scope::{patch_from_dsn, patch_from_rules};
use freerouting::settings::json_file::load_config_file;
use freerouting::settings::profiles::{TestingProfile, java_cli_default, java_fixture_test};
use freerouting::settings::validate::validate;
use freerouting::settings::{
    ResolveContext, Resolved, SettingsPatch, Source, defaults, dump, parse_cli_arguments, resolve,
};

mod common;
use common::diff;

const CORES: usize = 8;

fn ctx() -> ResolveContext {
    ResolveContext { cores: CORES }
}

// ---- validation --------------------------------------------------------

fn validated(patch: &mut SettingsPatch) -> Vec<String> {
    validate(patch, CORES)
}

#[test]
fn max_passes_clamps() {
    for (given, want, warns) in [
        (-1, 0, true),
        (10000, 0, true),
        (9999, 9999, false),
        (0, 0, false),
        (i32::MAX, i32::MAX, false),
        (i32::MIN, 0, true),
    ] {
        let mut p = SettingsPatch::default();
        p.autorouter.max_passes = Some(given);
        let warnings = validated(&mut p);
        assert_eq!(p.autorouter.max_passes, Some(want), "{given}");
        if warns {
            assert_eq!(
                warnings,
                vec![format!(
                    "Invalid maxPasses value: {given}, using default 0 (no limit)"
                )]
            );
        } else {
            assert!(warnings.is_empty(), "{given}: {warnings:?}");
        }
    }
    // `-mp 10000` means unlimited.
    let cli = parse_cli_arguments(&["-mp".to_string(), "10000".to_string()]).patch;
    let resolved = resolve(
        &[(Source::Defaults, defaults(&ctx())), (Source::Cli, cli)],
        &ctx(),
    );
    assert_eq!(resolved.settings.work_bounds().autorouter.max_passes, 0);
    assert!(resolved.warnings.iter().any(|w| w.contains("10000")));
}

#[test]
fn max_threads_clamps() {
    let case = |given: Option<i32>| {
        let mut p = SettingsPatch::default();
        p.max_threads = given;
        let warnings = validated(&mut p);
        (p.max_threads, warnings)
    };
    // A missing value becomes cores - 1, silently.
    assert_eq!(case(None), (Some(7), vec![]));
    assert_eq!(
        case(Some(-2)),
        (
            Some(7),
            vec!["Invalid maxThreads value: -2, using 7".to_string()]
        )
    );
    assert_eq!(
        case(Some(9)),
        (
            Some(8),
            vec!["Invalid maxThreads value: 9, capping at 8".to_string()]
        )
    );
    assert_eq!(case(Some(8)), (Some(8), vec![]));
    assert_eq!(
        case(Some(0)),
        (Some(0), vec![]),
        "0 stays; it means all cores later"
    );
    assert_eq!(case(Some(3)), (Some(3), vec![]));
    // A one-core machine uses one thread.
    let mut p = SettingsPatch::default();
    assert!(validate(&mut p, 1).is_empty());
    assert_eq!(p.max_threads, Some(1));
}

#[test]
fn trace_pull_tight_accuracy_clamps() {
    let mut p = SettingsPatch::default();
    p.trace_pull_tight_accuracy = Some(0);
    assert_eq!(
        validated(&mut p),
        vec!["Invalid tracePullTightAccuracy value: 0, using default 500".to_string()]
    );
    assert_eq!(p.trace_pull_tight_accuracy, Some(500));
    p.trace_pull_tight_accuracy = Some(1);
    assert!(validated(&mut p).is_empty());
}

#[test]
fn layer_costs_clamp() {
    let mut p = SettingsPatch::default();
    p.set_layer_count(3);
    {
        let layers = p.layers.as_mut().unwrap();
        layers[0].bend_cost = Some(12.5);
        layers[1].bend_cost = Some(-1.0);
        layers[1].preferred_direction_trace_cost = Some(0.05);
        layers[2].undesired_direction_trace_cost = Some(0.0);
        layers[2].bend_cost = Some(9.9); // in range
    }
    let warnings = validated(&mut p);
    assert_eq!(
        warnings,
        vec![
            "Layer 0 bendCost 12.5 out of range [0.0, 9.9], clamped to 9.9",
            "Layer 1 bendCost -1.0 out of range [0.0, 9.9], clamped to 0.0",
            "Layer 1 preferredDirectionTraceCost 0.05 below 0.1, clamped to 0.1",
            "Layer 2 undesiredDirectionTraceCost 0.0 below 0.1, clamped to 0.1",
        ]
    );
    let layers = p.layers.as_ref().unwrap();
    assert_eq!(layers[0].bend_cost, Some(9.9));
    assert_eq!(layers[1].bend_cost, Some(0.0));
    assert_eq!(layers[1].preferred_direction_trace_cost, Some(0.1));
    assert_eq!(layers[2].undesired_direction_trace_cost, Some(0.1));
    assert_eq!(layers[2].bend_cost, Some(9.9));
    // The warnings are stored, and replaced (not appended) by the next run.
    assert_eq!(p.validation_warnings.as_ref(), Some(&warnings));
    assert!(validated(&mut p).is_empty());
    assert_eq!(p.validation_warnings, Some(Vec::new()));
}

#[test]
fn unknown_ignore_net_classes_never_warn_without_a_board() {
    let mut p = SettingsPatch::default();
    p.autorouter.ignore_net_classes = Some(vec!["NO_SUCH_CLASS".to_string()]);
    assert!(validated(&mut p).is_empty());
}

#[test]
fn merge_runs_validation_and_returns_its_warnings() {
    let mut p = defaults(&ctx());
    p.autorouter.max_passes = Some(-5);
    let resolved = resolve(&[(Source::Cli, p)], &ctx());
    assert_eq!(
        resolved.warnings,
        vec!["Invalid maxPasses value: -5, using default 0 (no limit)".to_string()]
    );
    assert_eq!(resolved.settings.validation_warnings, resolved.warnings);
}

// ---- profiles ----------------------------------------------------------

#[test]
fn java_fixture_profile() {
    let resolved = resolve(
        &[
            (Source::Defaults, defaults(&ctx())),
            (Source::Testing, java_fixture_test()),
        ],
        &ctx(),
    );
    assert_eq!(resolved.settings.copper_to_edge_clearance_um, 0.0);
    assert!(resolved.settings.run_optimizer(), "the optimizer stays on");
    assert!(resolved.settings.run_router());
    assert_eq!(
        resolved.provenance.get("copper_to_edge_clearance_um"),
        Some(&Source::Testing)
    );
    // Everything else is the default.
    let only_default = resolve(&[(Source::Defaults, defaults(&ctx()))], &ctx());
    let mut expected = only_default.patch.clone();
    expected.copper_to_edge_clearance_um = Some(0.0);
    assert_eq!(resolved.patch, expected);
    // The profile is priority 80: it beats the CLI.
    let mut cli = SettingsPatch::default();
    cli.copper_to_edge_clearance_um = Some(100.0);
    let beaten = resolve(
        &[
            (Source::Defaults, defaults(&ctx())),
            (Source::Cli, cli),
            (Source::Testing, java_fixture_test()),
        ],
        &ctx(),
    );
    assert_eq!(beaten.settings.copper_to_edge_clearance_um, 0.0);
}

#[test]
fn java_cli_default_is_the_empty_profile() {
    assert_eq!(java_cli_default(), SettingsPatch::default());
}

#[test]
fn testing_setters_that_write_only_once() {
    let mut t = TestingProfile::default();
    t.set_max_passes(100);
    t.set_max_passes(5);
    t.set_job_timeout("00:00:45");
    t.set_job_timeout("01:00:00");
    t.set_enabled(false);
    t.set_enabled(true);
    assert_eq!(t.patch.autorouter.max_passes, Some(100));
    assert_eq!(t.patch.job_timeout.as_deref(), Some("00:00:45"));
    assert_eq!(t.patch.autorouter.enabled, Some(false));
    // The others overwrite.
    t.set_router_enabled(true);
    assert_eq!(t.patch.autorouter.enabled, Some(true));
    t.set_bend_cost(2, 1.5);
    assert_eq!(t.patch.layer_count(), 3);
    assert_eq!(t.patch.layers.as_ref().unwrap()[2].bend_cost, Some(1.5));
}

// ---- the dump ----------------------------------------------------------

#[test]
fn dump_has_the_gson_keys_and_normalised_threads() {
    let resolved = resolve(&[(Source::Defaults, defaults(&ctx()))], &ctx());
    let text = dump(&resolved).to_pretty_string();
    let tree = JsonValue::parse(&text).unwrap();
    let JsonValue::Object(top) = &tree else {
        panic!("object")
    };
    let get = |members: &[(String, JsonValue)], key: &str| {
        members
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        get(top, "max_threads"),
        Some(JsonValue::string("<cores-1>"))
    );
    assert_eq!(
        get(top, "copper_to_edge_clearance_um"),
        Some(JsonValue::Number("250.0".into()))
    );
    assert_eq!(get(top, "job_timeout"), Some(JsonValue::string("12:00:00")));
    assert_eq!(
        get(top, "validation_warnings"),
        Some(JsonValue::Array(vec![]))
    );
    assert_eq!(get(top, "layers"), None);
    let Some(JsonValue::Object(scoring)) = get(top, "scoring") else {
        panic!("scoring")
    };
    assert_eq!(
        get(&scoring, "unrouted_net_penalty"),
        Some(JsonValue::Number("5000000.0".into()))
    );
    assert_eq!(get(&scoring, "preferred_direction_trace_cost"), None);
    assert_eq!(
        get(&scoring, "via_costs"),
        Some(JsonValue::Number("50".into()))
    );
    let Some(JsonValue::Object(router_scoring)) = get(top, "router_scoring") else {
        panic!()
    };
    assert_eq!(
        get(&router_scoring, "unrouted_first_half_weight"),
        Some(JsonValue::Number("333.33334".into()))
    );
    assert_eq!(
        get(&router_scoring, "version"),
        Some(JsonValue::string("V2_CONTINUOUS"))
    );
    let Some(JsonValue::Object(optimizer)) = get(top, "optimizer") else {
        panic!()
    };
    assert_eq!(
        get(&optimizer, "board_update_strategy"),
        Some(JsonValue::string("GLOBAL_OPTIMAL"))
    );
    assert_eq!(
        get(&optimizer, "max_threads"),
        Some(JsonValue::string("<cores-1>"))
    );
    assert_eq!(
        get(&optimizer, "improvement_threshold"),
        Some(JsonValue::Number("2.5".into()))
    );
    // A thread count equal to the core count is "<cores>".
    let mut p = defaults(&ctx());
    p.autorouter.max_threads = Some(8);
    p.optimizer.max_threads = Some(3);
    let tree = freerouting::settings::dump::dump_patch(&p, CORES);
    let JsonValue::Object(top) = tree else {
        panic!()
    };
    let Some(JsonValue::Object(autorouter)) = get(&top, "autorouter") else {
        panic!()
    };
    assert_eq!(
        get(&autorouter, "max_threads"),
        Some(JsonValue::string("<cores>"))
    );
    let Some(JsonValue::Object(optimizer)) = get(&top, "optimizer") else {
        panic!()
    };
    assert_eq!(
        get(&optimizer, "max_threads"),
        Some(JsonValue::Number("3".into()))
    );
}

// ---- comparison with Java's dumps --------------------------------------

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

/// Drops the `null` members of an object tree: C01's dumps write unset
/// fields as explicit `null`s, the Rust dump leaves them out.
fn without_nulls(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(members) => JsonValue::Object(
            members
                .iter()
                .filter(|(_, v)| !matches!(v, JsonValue::Null))
                .map(|(k, v)| (k.clone(), without_nulls(v)))
                .collect(),
        ),
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(without_nulls).collect()),
        other => other.clone(),
    }
}

/// Removes the members C03 compares after the tuning.
fn static_part(java_settings: &JsonValue) -> JsonValue {
    let JsonValue::Object(top) = without_nulls(java_settings) else {
        return java_settings.clone();
    };
    JsonValue::Object(
        top.into_iter()
            .filter(|(k, _)| k != "layers")
            .map(|(k, v)| match (k.as_str(), v) {
                ("scoring", JsonValue::Object(s)) => (
                    k,
                    JsonValue::Object(
                        s.into_iter()
                            .filter(|(n, _)| {
                                n != "preferred_direction_trace_cost"
                                    && n != "undesired_direction_trace_cost"
                            })
                            .collect(),
                    ),
                ),
                (_, v) => (k, v),
            })
            .collect(),
    )
}

fn object_member<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match value {
        JsonValue::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// `RoutingJobScheduler`'s two merges for one scenario, then `RulesReader`'s
/// `autoroute_settings` on top when the board has a rules file (`rules` and
/// the board's layer names).
fn scheduler_merge(
    proto: Vec<(Source, SettingsPatch)>,
    dsn: &SettingsPatch,
    rules: Option<(&RulesFile, &[String])>,
) -> Resolved {
    let mut first = proto;
    first.push((Source::Dsn, dsn.clone()));
    let job = resolve(&first, &ctx());
    let mut second = first;
    if let Some((rules, _)) = rules {
        second.push((Source::RulesFile, patch_from_rules(rules)));
    }
    second.push((Source::Api, job.patch));
    let mut resolved = resolve(&second, &ctx());
    if let Some((rules, layer_names)) = rules
        && let Some(parsed) = autoroute_patch(rules, layer_names)
    {
        resolved.patch.apply_from(&parsed);
    }
    resolved
}

fn scenario_patches(scenario: &str, base: &str, inc_class: &str) -> Vec<(Source, SettingsPatch)> {
    let mut sources = vec![(Source::Defaults, defaults(&ctx()))];
    match scenario {
        "default" => {}
        "fixture" => sources.push((Source::Testing, java_fixture_test())),
        "cli" => {
            let args: Vec<String> = [
                "-de",
                &format!("{base}.dsn"),
                "-do",
                &format!("{base}.ses"),
                "-mp",
                "7",
                "-mt",
                "3",
                "-oit",
                "0.035",
                "-inc",
                inc_class,
                "--router.via_costs=77",
                "--router.scoring.plane_via_costs=7",
                "--router.optimizer.enabled=false",
                "--router.default_undesired_direction_trace_cost=1.5",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            sources.push((Source::Cli, parse_cli_arguments(&args).patch));
        }
        "owner-config" => {
            let owner = repo("tools/java-reference/cases/owner-freerouting.json");
            sources.push((
                Source::ConfigFile,
                load_config_file(&owner, true).unwrap().patch,
            ));
        }
        other => panic!("unknown scenario {other}"),
    }
    sources
}

/// Boards whose dumps are known to differ, with the reason. Empty: none yet.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

/// The scenarios with an adjacent rules file in Java's dumps that this test
/// reads: four fixtures and `explicit-cost-rules`, four scenarios each. (The
/// fifth rules fixture, `Issue742-tastexx-pcb/tastexx-pcb`, is in a
/// sub-directory that `job_prepare_reference.rs` walks and this test does
/// not; the adjacent file is named after the DSN, so
/// `Issue191-processor.Z80/processor.rules` is not one of them.)
const SCENARIOS_WITH_RULES: usize = 20;

fn adjacent_rules(scenario: &JsonValue) -> Option<String> {
    match object_member(scenario, "adjacentRules") {
        Some(JsonValue::String(name)) => Some(name.clone()),
        _ => None,
    }
}

#[test]
fn settings_match_java_dumps() {
    let mut files: Vec<(String, PathBuf, PathBuf)> = Vec::new();
    // `SETTINGS_REFERENCE_DIR` points at another dump root, for a local run of
    // `run.sh <pin> settings` with `REFERENCE_OUT_DIR` set.
    let root = std::env::var("SETTINGS_REFERENCE_DIR")
        .map_or_else(|_| repo("fixtures/reference"), PathBuf::from);
    for (dump_dir, dsn_dir) in [
        ("settings", "fixtures"),
        ("cases/settings", "tools/java-reference/cases"),
    ] {
        let dir = root.join(dump_dir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                files.push((
                    stem.clone(),
                    path,
                    repo(dsn_dir).join(format!("{stem}.dsn")),
                ));
            }
        }
    }
    if files.is_empty() {
        eprintln!(
            "settings_match_java_dumps: no fixtures/reference/settings yet (C01.1); nothing compared"
        );
        return;
    }
    files.sort();
    let mut compared = 0;
    let mut with_rules = 0;
    let mut failures = Vec::new();
    for (name, dump_path, dsn_path) in &files {
        let text = std::fs::read_to_string(dump_path).unwrap();
        let java = JsonValue::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let scenarios = object_member(&java, "scenarios").expect("scenarios");
        let Ok(bytes) = std::fs::read(dsn_path) else {
            failures.push(format!("{name}: DSN {} not found", dsn_path.display()));
            continue;
        };
        let pcb = match Pcb::parse(&freerouting::dsn::decode(&bytes)) {
            Ok(pcb) => pcb,
            Err(_) => continue, // Java rejects it too: its dump holds a failure object
        };
        let dsn = patch_from_dsn(&pcb);
        let layer_names: Vec<String> = pcb
            .structure
            .as_ref()
            .map(|s| s.layers.iter().map(|l| l.name.clone()).collect())
            .unwrap_or_default();
        for scenario in ["default", "fixture", "cli", "owner-config"] {
            let Some(expected) = object_member(scenarios, scenario) else {
                continue;
            };
            if object_member(expected, "settings").is_none() {
                continue; // a load failure in Java
            }
            let rules = adjacent_rules(expected).map(|file| {
                let path = dsn_path.with_file_name(file);
                let bytes = std::fs::read(&path).unwrap();
                let text = freerouting::dsn::decode(&bytes);
                RulesFile::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            });
            if rules.is_some() {
                with_rules += 1;
            }
            let inc_class = match object_member(expected, "incClass") {
                Some(JsonValue::String(s)) => s.clone(),
                _ => "POWER".to_string(),
            };
            let base = name.rsplit('/').next().unwrap().to_string();
            let resolved = scheduler_merge(
                scenario_patches(scenario, &base, &inc_class),
                &dsn,
                rules.as_ref().map(|r| (r, layer_names.as_slice())),
            );
            let rust = JsonValue::parse(&dump(&resolved).to_pretty_string()).unwrap();
            let java_static = static_part(object_member(expected, "settings").unwrap());
            let mut differences = Vec::new();
            diff("", &java_static, &rust, &mut differences);
            let key = format!("{name}/{scenario}");
            let known = EXPECTED_MISMATCHES.iter().any(|(k, _)| *k == key);
            if differences.is_empty() == known {
                failures.push(format!(
                    "{key}: {} (expected mismatch: {known})",
                    differences.join("; ")
                ));
            }
            compared += 1;
        }
    }
    eprintln!("compared {compared} board scenarios, {with_rules} with a rules file");
    assert_eq!(
        with_rules, SCENARIOS_WITH_RULES,
        "scenarios with a rules file"
    );
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The comparison machinery itself, on a hand-written Java-shaped tree.
#[test]
fn comparison_tree_diff_finds_differences() {
    let java = JsonValue::parse(
        r#"{"a": 1, "b": {"c": 2.5, "d": [1, 2]}, "layers": [], "scoring": {"via_costs": 5, "preferred_direction_trace_cost": [1.0]}}"#,
    )
    .unwrap();
    let rust =
        JsonValue::parse(r#"{"a": 1, "b": {"c": 2.50, "d": [1, 2]}, "scoring": {"via_costs": 5}}"#)
            .unwrap();
    let mut out = Vec::new();
    diff("", &static_part(&java), &rust, &mut out);
    assert!(out.is_empty(), "{out:?}");
    let rust = JsonValue::parse(
        r#"{"a": 1.0, "b": {"c": 2.5, "d": [1]}, "e": 1, "scoring": {"via_costs": 6}}"#,
    )
    .unwrap();
    diff("", &static_part(&java), &rust, &mut out);
    assert_eq!(out.len(), 4, "{out:?}");
    assert!(
        out.iter().any(|d| d.starts_with("a:")),
        "an int is not a float: {out:?}"
    );
}

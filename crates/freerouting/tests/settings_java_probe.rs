//! Differential check of the settings merge against Java 25 at the pin
//! (`cda540f`): `data/settings/java_merge_probe.tsv` holds the merged
//! `RouterSettings` of fixed source combinations as plain-Gson JSON,
//! written by `data/settings/MergeProbe.java.txt` (compile it as
//! `MergeProbe.java` against the pin's classes and run it as
//! `MergeProbe <fixtures dir> crates/freerouting/tests/data/settings`).
//! The expected values are Java's, not reasoned.
//!
//! The probe records its core count (8 on the machine that wrote it); the
//! Rust side resolves with the same count, and thread counts are
//! normalised like C01's dumps.

#![allow(clippy::field_reassign_with_default)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::diff;
use freerouting::drc::json::JsonValue;
use freerouting::dsn::ast::{Pcb, RulesFile};
use freerouting::settings::autoroute_scope::{patch_from_dsn, patch_from_rules};
use freerouting::settings::env::parse_environment;
use freerouting::settings::json_file::load_config_file;
use freerouting::settings::{
    LayerSettings, ResolveContext, Resolved, SettingsPatch, Source, defaults, dump,
    parse_cli_arguments, resolve,
};

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn data(name: &str) -> PathBuf {
    tests_dir().join("data/settings").join(name)
}

fn cli_args(base: &str) -> Vec<String> {
    [
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
        "POWER",
        "--router.via_costs=77",
        "--router.scoring.plane_via_costs=7",
        "--router.optimizer.enabled=false",
        "--router.default_undesired_direction_trace_cost=1.5",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn pcb(name: &str) -> SettingsPatch {
    let bytes = std::fs::read(fixtures().join(name)).unwrap();
    patch_from_dsn(&Pcb::parse(&freerouting::dsn::decode(&bytes)).unwrap())
}

fn rules(name: &str) -> SettingsPatch {
    let bytes = std::fs::read(fixtures().join(name)).unwrap();
    patch_from_rules(&RulesFile::parse(&freerouting::dsn::decode(&bytes)).unwrap())
}

fn json_file(name: &str) -> SettingsPatch {
    load_config_file(&data(name), true).unwrap().patch
}

fn cli(base: &str) -> SettingsPatch {
    parse_cli_arguments(&cli_args(base)).patch
}

fn extra_cli() -> SettingsPatch {
    let args: Vec<String> = [
        "-mp",
        "10000",
        "-mt",
        "99",
        "--router.layers.bend_cost=12.5,-1",
        "--router.layers.preferred_direction_trace_cost=0.05,3",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    parse_cli_arguments(&args).patch
}

fn env_source() -> SettingsPatch {
    parse_environment([
        ("FREEROUTING__ROUTER__MAX_PASSES", "17"),
        ("FREEROUTING__ROUTER__OPTIMIZER__MAX_THREADS", "2"),
        (
            "FREEROUTING__ROUTER__SCORING__PREFERRED_DIRECTION_TRACE_COST",
            "1.5,2.0",
        ),
        ("FREEROUTING__ROUTER__START_RIPUP_COSTS", "1500"),
        ("FREEROUTING__ROUTER__VIAS_ALLOWED", "false"),
        (
            "FREEROUTING__ROUTER__AUTOROUTER__IGNORE_NET_CLASSES",
            "a, b",
        ),
        (
            "freerouting__router__optimizer__board_update_strategy",
            "global_optimal",
        ),
        ("FREEROUTING__ROUTER__LAYERS__ROUTABLE", "false,true"),
        (
            "FREEROUTING__ROUTER__FANOUT__MAX_MILLISECONDS_PER_PIN",
            "123",
        ),
        ("FREEROUTING__ROUTER__NECK_WIDTH_UM", "12.5d"),
        ("FREEROUTING__ROUTER__BOGUS", "1"),
    ])
    .patch
}

fn sources_of(name: &str, ctx: &ResolveContext) -> Vec<(Source, SettingsPatch)> {
    let defaults = (Source::Defaults, defaults(ctx));
    let dsn187 = || pcb("Issue187-processor.Z80.dsn");
    match name {
        "defaults" => vec![defaults],
        "cli-c01" => vec![defaults, (Source::Cli, cli("Issue187-processor.Z80"))],
        "env" => vec![defaults, (Source::Environment, env_source())],
        "json-every" => vec![
            defaults,
            (Source::ConfigFile, json_file("every_section.json")),
        ],
        "json-owner" => vec![
            defaults,
            (Source::ConfigFile, json_file("owner_freerouting.json")),
        ],
        "json-cli-validation" => vec![
            defaults,
            (Source::ConfigFile, json_file("every_section.json")),
            (Source::Cli, extra_cli()),
        ],
        "dsn-Issue187-processor.Z80.dsn" => vec![defaults, (Source::Dsn, dsn187())],
        "dsn-Issue103-Board-Routed.dsn" => {
            vec![defaults, (Source::Dsn, pcb("Issue103-Board-Routed.dsn"))]
        }
        "dsn-Issue191-processor.Z80/processor.Z80.dsn" => vec![
            defaults,
            (Source::Dsn, pcb("Issue191-processor.Z80/processor.Z80.dsn")),
        ],
        "dsn-Issue413-test.dsn" => vec![defaults, (Source::Dsn, pcb("Issue413-test.dsn"))],
        "dsn-Issue066-Project_GP8B.dsn" => {
            vec![defaults, (Source::Dsn, pcb("Issue066-Project_GP8B.dsn"))]
        }
        "dsn-cli" => vec![defaults, (Source::Dsn, dsn187()), (Source::Cli, cli("x"))],
        "dsn-every-json" => vec![
            defaults,
            (Source::Dsn, dsn187()),
            (Source::ConfigFile, json_file("every_section.json")),
        ],
        "api-second" => {
            let first = vec![
                (Source::Defaults, self::defaults(ctx)),
                (Source::Cli, cli("x")),
                (Source::Dsn, pcb("Issue103-Board-Routed.dsn")),
            ];
            let job = resolve(&first, ctx).patch;
            let mut second = first;
            second.push((Source::Api, job));
            second
        }
        "rules-processor" => vec![
            defaults,
            (
                Source::RulesFile,
                rules("Issue191-processor.Z80/processor.rules"),
            ),
        ],
        "rules-hw48na" => vec![
            defaults,
            (Source::RulesFile, rules("Issue029-hw48na_valid.rules")),
        ],
        "rules-over-cli" => vec![
            defaults,
            (Source::Cli, cli("x")),
            (
                Source::RulesFile,
                rules("Issue191-processor.Z80/processor.rules"),
            ),
        ],
        other => panic!("no Rust sources for probe scenario {other}"),
    }
}

fn number(v: f64) -> JsonValue {
    JsonValue::Number(freerouting::settings::text::java_double_to_string(v))
}

/// Everything the probe writes, built from a resolved merge.
fn rust_tree(resolved: &Resolved) -> JsonValue {
    let JsonValue::Object(mut members) = dump(resolved) else {
        panic!("object")
    };
    let patch = &resolved.patch;
    let layer_json = |l: &LayerSettings| {
        let mut m = Vec::new();
        if let Some(v) = l.routable {
            m.push(("routable".to_string(), JsonValue::Bool(v)));
        }
        if let Some(v) = l.preferred_direction_horizontal {
            m.push((
                "preferred_direction_horizontal".to_string(),
                JsonValue::Bool(v),
            ));
        }
        for (name, v) in [
            ("bend_cost", l.bend_cost),
            (
                "preferred_direction_trace_cost",
                l.preferred_direction_trace_cost,
            ),
            (
                "undesired_direction_trace_cost",
                l.undesired_direction_trace_cost,
            ),
        ] {
            if let Some(v) = v {
                m.push((name.to_string(), number(v)));
            }
        }
        JsonValue::Object(m)
    };
    members.push((
        "layers".to_string(),
        JsonValue::Array(patch.layers.iter().flatten().map(layer_json).collect()),
    ));
    for member in members.iter_mut() {
        if member.0 == "scoring"
            && let JsonValue::Object(scoring) = &mut member.1
        {
            for (name, array) in [
                (
                    "preferred_direction_trace_cost",
                    &patch.scoring.preferred_direction_trace_cost,
                ),
                (
                    "undesired_direction_trace_cost",
                    &patch.scoring.undesired_direction_trace_cost,
                ),
            ] {
                if let Some(values) = array {
                    scoring.push((
                        name.to_string(),
                        JsonValue::Array(values.iter().map(|v| number(*v)).collect()),
                    ));
                }
            }
        }
    }
    if let Some(b) = patch.costs_initialized {
        members.push(("costs_initialized".to_string(), JsonValue::Bool(b)));
    }
    let s = &resolved.settings;
    members.push(("run_router".to_string(), JsonValue::Bool(s.run_router())));
    members.push((
        "run_optimizer".to_string(),
        JsonValue::Bool(s.run_optimizer()),
    ));
    members.push((
        "via_costs_getter".to_string(),
        JsonValue::int(i64::from(s.scoring.via_costs)),
    ));
    JsonValue::Object(members)
}

/// Java's thread counts are plain numbers in the probe; C01's dumps write
/// the normalised form.
fn normalise_threads(value: &mut JsonValue, cores: i64) {
    match value {
        JsonValue::Object(members) => {
            for (name, v) in members {
                if name == "max_threads"
                    && let JsonValue::Number(n) = v
                    && let Ok(n) = n.parse::<i64>()
                {
                    if n == (cores - 1).max(1) {
                        *v = JsonValue::string("<cores-1>");
                    } else if n == cores {
                        *v = JsonValue::string("<cores>");
                    }
                } else {
                    normalise_threads(v, cores);
                }
            }
        }
        JsonValue::Array(items) => items.iter_mut().for_each(|v| normalise_threads(v, cores)),
        _ => {}
    }
}

/// Scenarios whose Java result differs from ours, with the reason.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[(
    "json-every",
    "layers[1] is a JSON null in the file: Java keeps a null array element, we read an empty layer \
     (DIVERGENCES.md, settings); the merged layers of the other scenarios match",
)];

#[test]
fn merge_matches_java() {
    let text = std::fs::read_to_string(data("java_merge_probe.tsv")).unwrap();
    let mut cores = None;
    let mut java: BTreeMap<String, JsonValue> = BTreeMap::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.splitn(3, '\t').collect();
        match cols[0] {
            "CORES" => cores = Some(cols[1].parse::<usize>().unwrap()),
            "S" => {
                java.insert(cols[1].to_string(), JsonValue::parse(cols[2]).unwrap());
            }
            other => panic!("unknown line {other}"),
        }
    }
    let cores = cores.expect("CORES line");
    let ctx = ResolveContext { cores };
    assert!(java.len() >= 15, "{} scenarios", java.len());
    let mut failures = Vec::new();
    for (name, mut expected) in java {
        normalise_threads(&mut expected, cores as i64);
        let resolved = resolve(&sources_of(&name, &ctx), &ctx);
        let rust = JsonValue::parse(&rust_tree(&resolved).to_pretty_string()).unwrap();
        let mut differences = Vec::new();
        diff("", &expected, &rust, &mut differences);
        let known = EXPECTED_MISMATCHES.iter().any(|(k, _)| *k == name);
        if differences.is_empty() == known {
            failures.push(format!("{name}: {}", differences.join("; ")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

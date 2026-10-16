//! C03.6: `prepare_job` against Java's `settings` dumps (C01.1).
//!
//! For every fixture and synthetic case without an adjacent rules file, and
//! for each of C01's four scenarios (`default`, `fixture`, `cli`,
//! `owner-config`), this runs `prepare_job` and compares with the dump:
//! - every field of the final settings (including `layers` and both cost
//!   arrays), the `effective` block (read through [`BoardRouterSettings`],
//!   which is C03.7's comparison), the clearance matrix, the outline's
//!   class, `minimumPinGap` (C03.2) as Java's text, the hole clearance, the
//!   tolerance, the plane nets and the net-class flags;
//! - `postLoadViolations` against Rust's full clearance check after
//!   `prepare_job`, the end-to-end check of the overrides.
//!
//! The boards with an adjacent rules file (`Issue029-hw48na`, both `Issue107`
//! boards, `Issue593-BBD_Mars-64`, `Issue742-tastexx-pcb` and the synthetic
//! `explicit-cost-rules`) are compared too (C04.4): `prepare_job` finds the
//! rules file next to the DSN, runs the second merge and applies the file.
//! The board block holds the via infos in order, the via rules, the snap
//! angle and the padstack names (C04.1, C04.2).

use std::path::{Path, PathBuf};

use freerouting::board::BasicBoard;
use freerouting::board::structure::AngleRestriction;
use freerouting::drc::DesignRulesChecker;
use freerouting::drc::json::JsonValue;
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::rules::ItemClass;
use freerouting::settings::json_file::load_config_file;
use freerouting::settings::profiles::java_fixture_test;
use freerouting::settings::text::java_double_to_string;
use freerouting::settings::{ResolveContext, Source, dump::dump_patch, parse_cli_arguments};

mod common;
use common::diff;

const CORES: usize = 8;

fn ctx() -> ResolveContext {
    ResolveContext { cores: CORES }
}

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

// ---- the Rust side of a dump -------------------------------------------

fn obj(members: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        members
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

fn num(value: f64) -> JsonValue {
    JsonValue::Number(java_double_to_string(value))
}

fn int(value: i64) -> JsonValue {
    JsonValue::int(value)
}

fn doubles(values: &[f64]) -> JsonValue {
    JsonValue::Array(values.iter().map(|v| num(*v)).collect())
}

fn class_name(board: &BasicBoard, index: i32) -> String {
    let matrix = &board.rules.clearance_matrix;
    if index < 0 || index as usize >= matrix.class_count() {
        format!("#{index}")
    } else {
        matrix.get_name(index).unwrap().to_string()
    }
}

/// The settings tree: `dump_patch` plus `layers` and the two cost arrays.
fn settings_json(prepared: &PreparedJob) -> JsonValue {
    let JsonValue::Object(mut members) = dump_patch(&prepared.patch, CORES) else {
        panic!("object")
    };
    let layers = prepared.patch.layers.as_ref().expect("layers");
    members.push((
        "layers".into(),
        JsonValue::Array(
            layers
                .iter()
                .map(|l| {
                    let mut m = Vec::new();
                    if let Some(v) = l.routable {
                        m.push(("routable", JsonValue::Bool(v)));
                    }
                    if let Some(v) = l.preferred_direction_horizontal {
                        m.push(("preferred_direction_horizontal", JsonValue::Bool(v)));
                    }
                    if let Some(v) = l.bend_cost {
                        m.push(("bend_cost", num(v)));
                    }
                    if let Some(v) = l.preferred_direction_trace_cost {
                        m.push(("preferred_direction_trace_cost", num(v)));
                    }
                    if let Some(v) = l.undesired_direction_trace_cost {
                        m.push(("undesired_direction_trace_cost", num(v)));
                    }
                    obj(m)
                })
                .collect(),
        ),
    ));
    for (name, member) in members.iter_mut() {
        if name == "scoring"
            && let JsonValue::Object(scoring) = member
        {
            for (key, array) in [
                (
                    "preferred_direction_trace_cost",
                    &prepared.patch.scoring.preferred_direction_trace_cost,
                ),
                (
                    "undesired_direction_trace_cost",
                    &prepared.patch.scoring.undesired_direction_trace_cost,
                ),
            ] {
                if let Some(values) = array {
                    scoring.push((key.to_string(), doubles(values)));
                }
            }
        }
    }
    JsonValue::Object(members)
}

/// The `effective` block, read through the router's view (C03.7).
fn effective_json(prepared: &PreparedJob) -> JsonValue {
    let view = &prepared.board_settings;
    let board = &prepared.board;
    let layers = (0..board.layer_count())
        .map(|i| {
            obj(vec![
                (
                    "name",
                    JsonValue::string(board.layer_structure.layers[i].name.clone()),
                ),
                (
                    "isSignal",
                    JsonValue::Bool(board.layer_structure.layers[i].is_signal),
                ),
                ("active", JsonValue::Bool(view.layer_active(i))),
                (
                    "preferredHorizontal",
                    JsonValue::Bool(view.preferred_direction_is_horizontal(i)),
                ),
                ("bendCost", num(view.bend_cost(i))),
                (
                    "preferredCost",
                    num(view.preferred_direction_trace_costs(i)),
                ),
                (
                    "againstCost",
                    num(view.against_preferred_direction_trace_costs(i)),
                ),
                ("horizontalCost", num(view.horizontal_trace_costs(i))),
                ("verticalCost", num(view.vertical_trace_costs(i))),
            ])
        })
        .collect();
    obj(vec![
        ("layers", JsonValue::Array(layers)),
        (
            "boardSpecificTraceCostsApplied",
            JsonValue::Bool(prepared.settings.costs_initialized),
        ),
        ("viaCosts", int(view.via_costs().into())),
        ("planeViaCosts", int(view.plane_via_costs().into())),
        ("startRipupCosts", int(view.start_ripup_costs().into())),
        ("runRouter", JsonValue::Bool(prepared.settings.run_router())),
        (
            "runOptimizer",
            JsonValue::Bool(prepared.settings.run_optimizer()),
        ),
        (
            "runFanout",
            JsonValue::Bool(prepared.settings.fanout.enabled),
        ),
        (
            "validationWarnings",
            JsonValue::Array(
                prepared
                    .settings
                    .validation_warnings
                    .iter()
                    .map(|w| JsonValue::string(w.clone()))
                    .collect(),
            ),
        ),
    ])
}

fn board_json(board: &BasicBoard) -> JsonValue {
    let rules = &board.rules;
    let matrix = &rules.clearance_matrix;
    let names = (0..matrix.class_count())
        .map(|i| JsonValue::string(matrix.get_name(i as i32).unwrap()))
        .collect();
    let values = (0..matrix.layer_count())
        .map(|layer| {
            JsonValue::Array(
                (0..matrix.class_count())
                    .map(|i| {
                        JsonValue::Array(
                            (0..matrix.class_count())
                                .map(|j| {
                                    int(matrix
                                        .get_value(i as i32, j as i32, layer as i32, false)
                                        .into())
                                })
                                .collect(),
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    let outline = board.outline().expect("outline");
    let planes = rules
        .nets
        .iter()
        .filter(|n| n.contains_plane())
        .map(|n| JsonValue::string(n.name.clone()))
        .collect();
    let classes = (0..rules.net_classes.count())
        .map(|i| {
            let class = rules.net_classes.get(rules.net_classes.nth(i).unwrap());
            let mut m = vec![("name", JsonValue::string(class.name()))];
            if let Some(rule) = class.via_rule() {
                m.push((
                    "viaRule",
                    JsonValue::string(rules.via_rules.get(rule).name.clone()),
                ));
            }
            m.push((
                "traceClearanceClass",
                JsonValue::string(class_name(board, class.trace_clearance_class())),
            ));
            let layer_count = board.layer_count() as i32;
            m.push((
                "traceHalfWidth",
                JsonValue::Array(
                    (0..layer_count)
                        .map(|l| int(class.trace_half_width(l).into()))
                        .collect(),
                ),
            ));
            m.push((
                "activeRoutingLayer",
                JsonValue::Array(
                    (0..layer_count)
                        .map(|l| JsonValue::Bool(class.is_active_routing_layer(l)))
                        .collect(),
                ),
            ));
            m.push((
                "isIgnoredByAutorouter",
                JsonValue::Bool(class.is_ignored_by_autorouter),
            ));
            m.push(("shoveFixed", JsonValue::Bool(class.is_shove_fixed())));
            m.push(("pullTight", JsonValue::Bool(class.pull_tight())));
            m.push((
                "ignoreCyclesWithAreas",
                JsonValue::Bool(class.ignore_cycles_with_areas()),
            ));
            m.push(("minimumTraceLength", num(class.minimum_trace_length())));
            m.push(("maximumTraceLength", num(class.maximum_trace_length())));
            let dic = [
                ("NONE", ItemClass::None),
                ("TRACE", ItemClass::Trace),
                ("VIA", ItemClass::Via),
                ("PIN", ItemClass::Pin),
                ("SMD", ItemClass::Smd),
                ("AREA", ItemClass::Area),
            ]
            .iter()
            .map(|(name, ic)| {
                (
                    *name,
                    JsonValue::string(class_name(
                        board,
                        class.default_item_clearance_classes.get(*ic),
                    )),
                )
            })
            .collect();
            m.push(("defaultItemClearanceClasses", obj(dic)));
            obj(m)
        })
        .collect();
    obj(vec![
        (
            "clearanceMatrix",
            obj(vec![
                ("classNames", JsonValue::Array(names)),
                ("valuesByLayerRowCol", JsonValue::Array(values)),
            ]),
        ),
        (
            "outlineClearanceClass",
            JsonValue::string(class_name(board, outline.clearance_class())),
        ),
        (
            "minimumPinGap",
            JsonValue::string(java_double_to_string(board.minimum_pin_gap())),
        ),
        ("holeClearance", int(rules.hole_clearance().into())),
        ("clearanceToleranceUm", num(rules.clearance_tolerance_um)),
        (
            "ignoreConduction",
            JsonValue::Bool(rules.ignore_conduction()),
        ),
        ("planeNets", JsonValue::Array(planes)),
        ("netClasses", JsonValue::Array(classes)),
        ("viaInfos", via_infos_json(board)),
        ("viaRules", via_rules_json(board)),
        (
            "snapAngle",
            JsonValue::string(match rules.trace_angle_restriction() {
                AngleRestriction::None => "NONE",
                AngleRestriction::NinetyDegree => "NINETY_DEGREE",
                AngleRestriction::FortyfiveDegree => "FORTYFIVE_DEGREE",
            }),
        ),
        // Java's library of a design without a library scope has no padstack
        // list at all (`null`, dropped by `without_nulls`); the port's is empty.
        (
            "padstacks",
            if board.library.padstacks.count() == 0 {
                JsonValue::Null
            } else {
                JsonValue::Array(
                    board
                        .library
                        .padstacks
                        .iter()
                        .map(|p| JsonValue::string(p.name.clone()))
                        .collect(),
                )
            },
        ),
        (
            "viaPadstacks",
            JsonValue::Array(
                board
                    .library
                    .via_padstack_ids()
                    .iter()
                    .map(|id| {
                        JsonValue::string(board.library.padstacks.get(*id).unwrap().name.clone())
                    })
                    .collect(),
            ),
        ),
    ])
}

fn via_infos_json(board: &BasicBoard) -> JsonValue {
    JsonValue::Array(
        board
            .rules
            .via_infos
            .iter()
            .map(|(_, info)| {
                obj(vec![
                    ("name", JsonValue::string(info.name.clone())),
                    (
                        "padstack",
                        JsonValue::string(
                            board
                                .library
                                .padstacks
                                .get(info.padstack)
                                .unwrap()
                                .name
                                .clone(),
                        ),
                    ),
                    (
                        "clearanceClass",
                        JsonValue::string(class_name(board, info.clearance_class)),
                    ),
                    ("attachSmd", JsonValue::Bool(info.attach_smd_allowed)),
                ])
            })
            .collect(),
    )
}

fn via_rules_json(board: &BasicBoard) -> JsonValue {
    JsonValue::Array(
        board
            .rules
            .via_rules
            .iter()
            .map(|(_, rule)| {
                obj(vec![
                    ("name", JsonValue::string(rule.name.clone())),
                    (
                        "vias",
                        JsonValue::Array(
                            rule.vias()
                                .iter()
                                .map(|v| {
                                    JsonValue::string(board.rules.via_infos.get(*v).name.clone())
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    )
}

fn rust_scenario(prepared: &PreparedJob) -> JsonValue {
    let violations = DesignRulesChecker::new(&prepared.board)
        .all_clearance_violations()
        .len();
    obj(vec![
        ("settings", settings_json(prepared)),
        ("effective", effective_json(prepared)),
        ("board", without_nulls(&board_json(&prepared.board))),
        ("postLoadViolations", int(violations as i64)),
    ])
}

// ---- the Java side -----------------------------------------------------

fn member<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match value {
        JsonValue::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

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

/// The members of Java's board block that the Rust side builds.
const BOARD_KEYS: &[&str] = &[
    "clearanceMatrix",
    "outlineClearanceClass",
    "minimumPinGap",
    "holeClearance",
    "clearanceToleranceUm",
    "ignoreConduction",
    "planeNets",
    "netClasses",
    "viaInfos",
    "viaRules",
    "snapAngle",
    "padstacks",
    "viaPadstacks",
];

fn java_scenario(expected: &JsonValue) -> JsonValue {
    let board = member(expected, "board").expect("board");
    let JsonValue::Object(board_members) = board else {
        panic!("board object")
    };
    let board = JsonValue::Object(
        board_members
            .iter()
            .filter(|(k, _)| BOARD_KEYS.contains(&k.as_str()))
            // A design without padstacks is `null` or `[]` in Java, depending
            // on whether it had a library scope; the port's list is empty.
            .filter(|(k, v)| {
                !(k == "padstacks" && matches!(v, JsonValue::Array(items) if items.is_empty()))
            })
            .cloned()
            .collect(),
    );
    obj(vec![
        ("settings", member(expected, "settings").unwrap().clone()),
        ("effective", member(expected, "effective").unwrap().clone()),
        ("board", board),
        (
            "postLoadViolations",
            member(expected, "postLoadViolations").unwrap().clone(),
        ),
    ])
}

// ---- the scenarios -----------------------------------------------------

fn cli_arguments(base: &str, inc_class: &str) -> Vec<String> {
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
        inc_class,
        "--router.via_costs=77",
        "--router.scoring.plane_via_costs=7",
        "--router.optimizer.enabled=false",
        "--router.default_undesired_direction_trace_cost=1.5",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn job_input(
    bytes: Vec<u8>,
    base: &str,
    dsn_dir: &Path,
    scenario: &str,
    inc_class: &str,
) -> JobInput {
    let mut input = JobInput::new(bytes, base);
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = Some(dsn_dir.to_path_buf());
    match scenario {
        "default" => {}
        "fixture" => input
            .extra_sources
            .push((Source::Testing, java_fixture_test())),
        "cli" => input.cli = Some(parse_cli_arguments(&cli_arguments(base, inc_class)).patch),
        "owner-config" => {
            input.config_file = Some(
                load_config_file(
                    &repo("tools/java-reference/cases/owner-freerouting.json"),
                    true,
                )
                .unwrap()
                .patch,
            );
        }
        other => panic!("unknown scenario {other}"),
    }
    input
}

/// Boards and scenarios whose dumps are known to differ, with the reason.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[
    (
        "Issue684-Autorouter_PCB1_2026-5-8/default",
        QUOTED_EMPTY_NAME,
    ),
    (
        "Issue684-Autorouter_PCB1_2026-5-8/fixture",
        QUOTED_EMPTY_NAME,
    ),
    ("Issue684-Autorouter_PCB1_2026-5-8/cli", QUOTED_EMPTY_NAME),
    (
        "Issue684-Autorouter_PCB1_2026-5-8/owner-config",
        QUOTED_EMPTY_NAME,
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15/default",
        QUOTED_EMPTY_NAME,
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15/fixture",
        QUOTED_EMPTY_NAME,
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15/cli",
        QUOTED_EMPTY_NAME,
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15/owner-config",
        QUOTED_EMPTY_NAME,
    ),
];

/// The two boards name a net class `''` (an empty name in single quotes).
/// Java's scanner keeps the quote characters, so the class is called `''`;
/// the port's DSN reader accepts `'` as a quote (docs/DIVERGENCES.md,
/// freerouting-dsn) and reads the empty name. The class name appears in the
/// matrix names, the class's own fields and, in the `cli` scenario, in the
/// ignore flag: `-inc ''` is a blank name to the port and a real name to
/// Java. Nothing else differs, `postLoadViolations` included.
const QUOTED_EMPTY_NAME: &str =
    "a net class named '' (quoted empty name): Java keeps the quotes in the name";

struct Case {
    name: String,
    dump: PathBuf,
    dsn: PathBuf,
}

fn collect(dir: &Path, relative: &str, dsn_root: &Path, out: &mut Vec<Case>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            let sub = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            collect(&path, &sub, dsn_root, out);
        } else if let Some(stem) = name.strip_suffix(".json") {
            let case_name = if relative.is_empty() {
                stem.to_string()
            } else {
                format!("{relative}/{stem}")
            };
            out.push(Case {
                dsn: dsn_root.join(format!("{case_name}.dsn")),
                name: case_name,
                dump: path,
            });
        }
    }
}

fn cases() -> Vec<Case> {
    let root = std::env::var("SETTINGS_REFERENCE_DIR")
        .map_or_else(|_| repo("fixtures/reference"), PathBuf::from);
    let mut result = Vec::new();
    collect(&root.join("settings"), "", &repo("fixtures"), &mut result);
    collect(
        &root.join("cases/settings"),
        "",
        &repo("tools/java-reference/cases"),
        &mut result,
    );
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

/// What one dump contributed.
#[derive(Default)]
struct CaseResult {
    compared: usize,
    /// How many of those had an adjacent rules file in Java.
    with_rules: usize,
    failures: Vec<String>,
}

fn run_case(case: &Case) -> CaseResult {
    let mut result = CaseResult::default();
    let text = std::fs::read_to_string(&case.dump).unwrap();
    let java = JsonValue::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", case.name));
    let Some(scenarios) = member(&java, "scenarios") else {
        result.failures.push(format!(
            "{}: the dump holds no scenarios: {}",
            case.name,
            java.to_pretty_string()
                .chars()
                .take(200)
                .collect::<String>()
        ));
        return result;
    };
    let bytes = std::fs::read(&case.dsn).unwrap_or_else(|e| panic!("{}: {e}", case.dsn.display()));
    let base = case.name.rsplit('/').next().unwrap().to_string();
    let dsn_dir = case.dsn.parent().unwrap();
    for scenario in ["default", "fixture", "cli", "owner-config"] {
        let Some(expected) = member(scenarios, scenario) else {
            continue;
        };
        let key = format!("{}/{scenario}", case.name);
        if member(expected, "settings").is_none() {
            // Java failed to load the board; so must the port.
            let input = job_input(bytes.clone(), &base, dsn_dir, scenario, "POWER");
            if prepare_job(&input, &ctx()).is_ok() {
                result
                    .failures
                    .push(format!("{key}: Java failed to load, the port did not"));
            }
            continue;
        }
        if !matches!(
            member(expected, "adjacentRules"),
            None | Some(JsonValue::Null)
        ) {
            result.with_rules += 1;
        }
        let inc_class = match member(expected, "incClass") {
            Some(JsonValue::String(s)) => s.clone(),
            _ => "POWER".to_string(),
        };
        let input = job_input(bytes.clone(), &base, dsn_dir, scenario, &inc_class);
        let prepared = match prepare_job(&input, &ctx()) {
            Ok(p) => p,
            Err(e) => {
                result
                    .failures
                    .push(format!("{key}: prepare_job failed: {e}"));
                continue;
            }
        };
        let rust = rust_scenario(&prepared);
        let java_tree = without_nulls(&java_scenario(expected));
        let mut differences = Vec::new();
        diff("", &java_tree, &rust, &mut differences);
        let known = EXPECTED_MISMATCHES.iter().any(|(k, _)| *k == key);
        if differences.is_empty() == known {
            let shown: Vec<_> = differences.iter().take(12).cloned().collect();
            result.failures.push(format!(
                "{key}: {} differences (expected mismatch: {known}): {}",
                differences.len(),
                shown.join("; ")
            ));
        }
        result.compared += 1;
    }
    result
}

#[test]
fn prepare_job_matches_java_dumps() {
    let cases = cases();
    if cases.is_empty() {
        eprintln!("prepare_job_matches_java_dumps: no settings dumps (C01.1); nothing compared");
        return;
    }
    // The cases are independent; workers take them in turn and the results
    // are merged in case order, so the outcome does not depend on the
    // thread count.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<CaseResult>>> =
        cases.iter().map(|_| std::sync::Mutex::new(None)).collect();
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(case) = cases.get(i) else { break };
                    *results[i].lock().unwrap() = Some(run_case(case));
                }
            });
        }
    });
    let mut compared = 0;
    let mut with_rules = 0;
    let mut failures = Vec::new();
    for slot in results {
        let r = slot.into_inner().unwrap().expect("every case ran");
        compared += r.compared;
        with_rules += r.with_rules;
        failures.extend(r.failures);
    }
    eprintln!("compared {compared} board scenarios, {with_rules} with a rules file");
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // Six boards (five fixtures and `explicit-cost-rules`) in four scenarios.
    assert_eq!(with_rules, 24, "the scenarios with an adjacent rules file");
}

/// `prepare_job` against `load_design` on the largest fixture, release
/// builds only (`cargo test --release -- --ignored --nocapture timing`).
#[test]
#[ignore = "timing; run in release with --nocapture"]
fn timing_prepare_job_against_load_design() {
    let mut largest: Option<(u64, PathBuf)> = None;
    for entry in std::fs::read_dir(repo("fixtures")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) == Some("dsn") {
            let size = std::fs::metadata(&path).unwrap().len();
            if largest.as_ref().is_none_or(|(s, _)| size > *s) {
                largest = Some((size, path));
            }
        }
    }
    let (size, path) = largest.unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let time = |f: &dyn Fn()| {
        let mut runs: Vec<f64> = (0..5)
            .map(|_| {
                let start = std::time::Instant::now();
                f();
                start.elapsed().as_secs_f64()
            })
            .collect();
        runs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        runs[2]
    };
    let load = time(&|| {
        freerouting::drc::load_design(&bytes).unwrap();
    });
    let mut input = JobInput::new(bytes.clone(), "largest");
    input.rules = RulesChoice::None;
    let prepare = time(&|| {
        prepare_job(&input, &ctx()).unwrap();
    });
    eprintln!(
        "{} ({size} bytes): load_design {load:.3} s, prepare_job {prepare:.3} s (median of 5)",
        path.display()
    );
}

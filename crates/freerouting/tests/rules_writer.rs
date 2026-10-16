//! C04.6: writing a board's rules as a `.rules` file.
//!
//! Ports `RulesRoundTripTest.rulesRoundTrip` and
//! `rulesRoundTripWithAutorouteSettings`. The comparison with Java is
//! `rules_match_javas_dumps`: for every board with a dump in
//! `fixtures/reference/rules` (C01.5), the tree of Java's text (with its
//! writer's `snapAngle` and `viaRule` spelled `snap_angle` and `via_rule`,
//! which its own reader skips, so that the port's reader accepts them) equals
//! the tree of `rules_from_board` on the board `prepare_job` builds with the
//! `default` scenario, item by item.

use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::drc::load_design;
use freerouting::dsn::ast::RulesFile;
use freerouting::io::specctra::{read_rules, rules_from_board};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::settings::{ResolveContext, Source, resolve};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn board(file: &str) -> RoutingBoard {
    load_design(&std::fs::read(repo(&format!("fixtures/{file}"))).unwrap()).unwrap()
}

fn ctx() -> ResolveContext {
    ResolveContext { cores: 8 }
}

// ---- RulesRoundTripTest ---------------------------------------------------

#[test]
fn rules_round_trip() {
    let board = board("Issue029-hw48na.dsn");
    let text = rules_from_board(&board, None, "Issue029-hw48na").to_rules();
    assert!(
        text.contains("(rules PCB"),
        "Rules file must start with (rules PCB ...)"
    );
    assert!(
        text.contains("(rule"),
        "Rules file must contain at least one (rule ...) scope"
    );
    // Without settings there is no `autoroute_settings` scope.
    assert!(!text.contains("(autoroute_settings"));
    let (_, result) = read_rules(board, &text, "Issue029-hw48na");
    let outcome = result.expect("RulesReader.read must succeed on valid input");
    assert!(outcome.autoroute.is_none());
    // The header name matches: no warning about it.
    assert!(
        !outcome
            .log
            .iter()
            .any(|l| l.text.contains("designName not matching")),
        "{:?}",
        outcome.log
    );
}

#[test]
fn rules_round_trip_with_autoroute_settings() {
    let board = board("Issue029-hw48na.dsn");
    let mut patch = freerouting::settings::SettingsPatch::default();
    patch.set_layer_count(board.layer_count());
    patch.set_via_costs(75);
    patch.set_plane_via_costs(8);
    patch.set_start_ripup_costs(120);
    patch.set_layer_active(0, true);
    patch.set_layer_active(1, true);
    patch.set_preferred_direction_is_horizontal(0, true);
    patch.set_preferred_direction_is_horizontal(1, false);
    patch.set_preferred_direction_trace_costs(0, 1.2);
    patch.set_against_preferred_direction_trace_costs(0, 2.8);
    patch.set_preferred_direction_trace_costs(1, 1.0);
    patch.set_against_preferred_direction_trace_costs(1, 3.1);
    let settings = resolve(
        &[(
            Source::Custom {
                name: "test".into(),
                priority: 0,
            },
            patch,
        )],
        &ctx(),
    )
    .settings;

    let text = rules_from_board(&board, Some(&settings), "Issue029-hw48na").to_rules();
    assert!(
        text.contains("(autoroute_settings"),
        "Rules file must contain autoroute_settings scope"
    );
    assert!(
        text.contains("(layer_rule"),
        "Rules file must contain layer_rule scopes"
    );

    // Read back the rules and apply the settings to a target: the values are
    // the exact doubles, `2.8` and not `2.799999952316284`.
    let (_, result) = read_rules(board, &text, "Issue029-hw48na");
    let parsed = result.unwrap().autoroute.expect("autoroute_settings");
    let target = resolve(&[(Source::RulesFile, parsed)], &ctx()).settings;
    assert_eq!(target.scoring.via_costs, 75);
    assert_eq!(target.scoring.plane_via_costs, 8);
    assert_eq!(target.scoring.start_ripup_costs, 120);
    assert!(target.preferred_direction_is_horizontal(0));
    assert!(!target.preferred_direction_is_horizontal(1));
    assert_eq!(target.preferred_direction_trace_costs(0), 1.2);
    assert_eq!(target.against_preferred_direction_trace_costs(0), 2.8);
    assert_eq!(target.preferred_direction_trace_costs(1), 1.0);
    assert_eq!(target.against_preferred_direction_trace_costs(1), 3.1);
}

// ---- against Java ----------------------------------------------------------

/// The design name of a `.rules` dump: its file name without the extension.
fn rules_files(dir: &Path, relative: &str, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let rel = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        if path.is_dir() {
            rules_files(&path, &rel, out);
        } else if let Some(stem) = rel.strip_suffix(".rules") {
            out.push((stem.to_string(), path));
        }
    }
}

/// Runs `f` on each dump on all cores; the results come back in dump order,
/// so the outcome does not depend on the thread count.
fn parallel<T: Send>(dumps: &[(String, PathBuf)], f: impl Fn(&str, &Path) -> T + Sync) -> Vec<T> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<T>>> =
        dumps.iter().map(|_| std::sync::Mutex::new(None)).collect();
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((stem, dump)) = dumps.get(i) else {
                        break;
                    };
                    *slots[i].lock().unwrap() = Some(f(stem, dump));
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|s| s.into_inner().unwrap().expect("every dump ran"))
        .collect()
}

/// Java's text as the port's reader takes it.
fn java_tree(text: &str) -> Result<RulesFile, String> {
    let fixed = text
        .replace("(snapAngle", "(snap_angle")
        .replace("(viaRule", "(via_rule");
    RulesFile::parse(&fixed).map_err(|e| format!("Java's text does not parse: {e}"))
}

/// The first difference of two trees, item by item.
fn first_difference(java: &RulesFile, rust: &RulesFile) -> Option<String> {
    if java.design != rust.design {
        return Some(format!("design {:?} / {:?}", java.design, rust.design));
    }
    for (i, (j, r)) in java.items.iter().zip(&rust.items).enumerate() {
        if j != r {
            // Show the text around the first difference of the two debug forms.
            let (jd, rd) = (format!("{j:?}"), format!("{r:?}"));
            let at = jd
                .bytes()
                .zip(rd.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(jd.len().min(rd.len()));
            let around = |s: &str| {
                let from = (0..=at.saturating_sub(120))
                    .rev()
                    .find(|i| s.is_char_boundary(*i))
                    .unwrap_or(0);
                let to = (at + 120).min(s.len());
                let to = (to..=s.len())
                    .find(|i| s.is_char_boundary(*i))
                    .unwrap_or(s.len());
                s[from..to].to_string()
            };
            return Some(format!(
                "item {i}:\n  java ...{}\n  rust ...{}",
                around(&jd),
                around(&rd)
            ));
        }
    }
    (java.items.len() != rust.items.len()).then(|| {
        format!(
            "{} items in Java, {} in Rust",
            java.items.len(),
            rust.items.len()
        )
    })
}

/// Boards whose dump differs, with the reason.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[
    ("Issue110-Паяльная станция", RAW_NAME),
    ("Issue632-MiniAutoPilot/Mini Auto Pilot", RAW_NAME),
    (
        "Issue179-Autorouter_PCB1_2023-3-24",
        "a clearance class with an empty name: Java writes `(via \"via0-\" via0 )`, which \
         the port's parser rejects (Java's reader skips that via type); the writer's tree \
         holds the via with the empty class",
    ),
    ("Issue684-Autorouter_PCB1_2026-5-8", QUOTED_EMPTY_NAME),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
        QUOTED_EMPTY_NAME,
    ),
];

/// Java writes the design name of the header raw, so a name with blanks reads
/// back as its first word. The tree holds the whole name (the tree writer
/// quotes it).
const RAW_NAME: &str = "a design name with blanks: Java writes it unquoted";

/// The two boards name a net class `''` (an empty name in single quotes); Java
/// keeps the quotes in the name and the port's DSN reader reads an empty name
/// (docs/DIVERGENCES.md, freerouting-dsn; the same two boards are expected
/// mismatches in `job_prepare_reference.rs`). The via type `via0-''` is
/// `via0-` in the port.
const QUOTED_EMPTY_NAME: &str =
    "a net class named '' (quoted empty name): Java keeps the quotes in the name";

#[test]
fn rules_match_javas_dumps() {
    let mut dumps = Vec::new();
    rules_files(&repo("fixtures/reference/rules"), "", &mut dumps);
    assert!(dumps.len() > 100, "{} dumps", dumps.len());
    let results = parallel(&dumps, |stem, dump| -> Option<Result<(), String>> {
        let dsn = repo(&format!("fixtures/{stem}.dsn"));
        let Ok(bytes) = std::fs::read(&dsn) else {
            return Some(Err(format!("{stem}: no DSN file")));
        };
        let name = stem.rsplit('/').next().unwrap();
        let mut input = JobInput::new(bytes, name);
        input.rules = RulesChoice::Adjacent;
        input.dsn_dir = dsn.parent().map(Path::to_path_buf);
        let job = match prepare_job(&input, &ctx()) {
            Ok(job) => job,
            Err(e) => return Some(Err(format!("{stem}: prepare_job failed: {e}"))),
        };
        let rust = rules_from_board(&job.board, Some(&job.settings), name);
        let difference = match java_tree(&freerouting::dsn::decode(&std::fs::read(dump).unwrap())) {
            Ok(java) => first_difference(&java, &rust),
            Err(e) => Some(e),
        };
        let known = EXPECTED_MISMATCHES.iter().any(|(k, _)| *k == stem);
        if difference.is_some() != known {
            return Some(Err(format!(
                "{stem} (expected mismatch: {known}): {}",
                difference.unwrap_or_default()
            )));
        }
        Some(Ok(()))
    });
    let failures: Vec<String> = results
        .into_iter()
        .flatten()
        .filter_map(Result::err)
        .collect();
    eprintln!("compared {} rules files", dumps.len());
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        dumps.len(),
        failures.join("\n")
    );
}

// ---- the round trip on every fixture ------------------------------------------
//
// Java's format cannot carry a whole board back: the default rule has the
// clearance of layer 0 and one value per named class, reading it starts with
// `set_default_value`, which overwrites every cell, and a type that is one
// name is skipped. Applying the rules to a board that was loaded from the same
// DSN also appends its net classes again (`appendNetClass`) and moves its via
// types and via rules to the end. So the property is stated on what the file
// expresses:
// - the via types, in order, with their padstack, clearance class and attach flag;
// - the via rules, by name, with their via type names (the last rule of a name);
// - each net class, by name (the last one of that name): trace clearance
//   class, via rule, trace half widths, routing layers, lengths, flags, nets;
// - the snap angle and the pin edge to turn distance;
// - the clearance matrix: the original's class names, in order (reading may
//   append more), and the default clearance (class `default` against itself)
//   on every layer equal to the original's layer 0 value. The clearance of a
//   named class does not come back: the file has `(clearance v (type name))`,
//   and Java's reader skips a type without an underscore and splits `a_b` into
//   the pair `a` and `b`;
// - the resolved settings: the costs, the run flags, the via flag and the four
//   values of each layer (costs compared as `float`, which is what the file holds).

use freerouting::board::BasicBoard;

#[derive(Debug, PartialEq)]
struct ClassView {
    name: String,
    trace_clearance_class: String,
    via_rule: Option<String>,
    half_widths: Vec<i32>,
    active_layers: Vec<bool>,
    min_length: u64,
    max_length: u64,
    pull_tight: bool,
    shove_fixed: bool,
    nets: Vec<String>,
}

fn class_views(board: &BasicBoard) -> Vec<ClassView> {
    let rules = &board.rules;
    let layers = board.layer_count() as i32;
    rules
        .net_classes
        .iter()
        .map(|(id, class)| ClassView {
            name: class.name().to_string(),
            trace_clearance_class: rules
                .clearance_matrix
                .get_name(class.trace_clearance_class())
                .unwrap_or_default()
                .to_string(),
            via_rule: class
                .via_rule()
                .map(|r| rules.via_rules.get(r).name.clone()),
            // Reading a `use_layer` zeroes the widths of the other layers.
            half_widths: (0..layers)
                .map(|l| {
                    if class.is_active_routing_layer(l) {
                        class.trace_half_width(l)
                    } else {
                        0
                    }
                })
                .collect(),
            active_layers: (0..layers)
                .map(|l| class.is_active_routing_layer(l))
                .collect(),
            min_length: class.minimum_trace_length().to_bits(),
            max_length: class.maximum_trace_length().to_bits(),
            pull_tight: class.pull_tight(),
            shove_fixed: class.is_shove_fixed(),
            nets: (1..=rules.nets.max_net_number())
                .filter_map(|n| rules.nets.get(n))
                .filter(|net| net.net_class() == id)
                .map(|net| net.name.clone())
                .collect(),
        })
        .collect()
}

/// The last view of each class name, in order of name.
fn last_by_name(views: Vec<ClassView>) -> std::collections::BTreeMap<String, ClassView> {
    views.into_iter().map(|v| (v.name.clone(), v)).collect()
}

fn via_info_views(board: &BasicBoard) -> Vec<(String, String, String, bool)> {
    board
        .rules
        .via_infos
        .iter()
        .map(|(_, info)| {
            (
                info.name.clone(),
                board
                    .library
                    .padstacks
                    .get(info.padstack)
                    .unwrap()
                    .name
                    .clone(),
                board
                    .rules
                    .clearance_matrix
                    .get_name(info.clearance_class)
                    .unwrap_or_default()
                    .to_string(),
                info.attach_smd_allowed,
            )
        })
        .collect()
}

fn via_rule_views(board: &BasicBoard) -> std::collections::BTreeMap<String, Vec<String>> {
    board
        .rules
        .via_rules
        .iter()
        .map(|(_, rule)| {
            (
                rule.name.clone(),
                rule.vias()
                    .iter()
                    .map(|v| board.rules.via_infos.get(*v).name.clone())
                    .collect(),
            )
        })
        .collect()
}

/// The first difference between a board and the same board's rules applied to a fresh load,
/// or `None`.
fn round_trip_difference(original: &BasicBoard, applied: &BasicBoard) -> Option<String> {
    let (vo, va) = (via_info_views(original), via_info_views(applied));
    if vo != va {
        let at = vo
            .iter()
            .zip(&va)
            .position(|(a, b)| a != b)
            .unwrap_or(vo.len().min(va.len()));
        return Some(format!(
            "via infos ({} / {}), first difference at {at}: {:?} / {:?}",
            vo.len(),
            va.len(),
            vo.get(at),
            va.get(at)
        ));
    }
    if via_rule_views(original) != via_rule_views(applied) {
        return Some("via rules".into());
    }
    let (o, a) = (
        last_by_name(class_views(original)),
        last_by_name(class_views(applied)),
    );
    for (name, view) in &o {
        match a.get(name) {
            Some(other) if other == view => {}
            Some(other) => return Some(format!("net class {name:?}:\n  {view:?}\n  {other:?}")),
            None => return Some(format!("net class {name:?} missing")),
        }
    }
    let (ro, ra) = (&original.rules, &applied.rules);
    if ro.trace_angle_restriction() != ra.trace_angle_restriction() {
        return Some("snap angle".into());
    }
    if ro.pin_edge_to_turn_dist().to_bits() != ra.pin_edge_to_turn_dist().to_bits() {
        return Some("pin edge to turn distance".into());
    }
    let (mo, ma) = (&ro.clearance_matrix, &ra.clearance_matrix);
    let count = mo.class_count();
    // Reading appends classes: a type such as `MIN_EXTERN_188A` is taken as the
    // pair `MIN` and `EXTERN_188A` (Java's `split("_", 2)`).
    if ma.class_count() < count {
        return Some("clearance class count".into());
    }
    for i in 0..count as i32 {
        if mo.get_name(i) != ma.get_name(i) {
            return Some(format!("clearance class {i} name"));
        }
    }
    // The default clearance of every layer is the original's layer 0 value.
    let default = mo.get_value(1, 1, 0, false);
    for layer in 0..ma.layer_count() as i32 {
        if ma.get_value(1, 1, layer, false) != default {
            return Some(format!("default clearance on layer {layer}"));
        }
    }
    None
}

fn same_float(a: f64, b: f64) -> bool {
    (a as f32) == (b as f32)
}

#[test]
fn rules_round_trip_on_every_fixture() {
    let mut dumps = Vec::new();
    rules_files(&repo("fixtures/reference/rules"), "", &mut dumps);
    let results = parallel(&dumps, |stem, _| round_trip(stem));
    let compared = results.iter().flatten().count();
    let failures: Vec<String> = results.into_iter().flatten().flatten().collect();
    finish_round_trip(compared, failures);
}

/// The failures of one board's round trip; `None` if the board is skipped.
fn round_trip(stem: &str) -> Option<Vec<String>> {
    let mut failures = Vec::new();
    {
        let dsn = repo(&format!("fixtures/{stem}.dsn"));
        let Ok(bytes) = std::fs::read(&dsn) else {
            return None;
        };
        let name = stem.rsplit('/').next().unwrap();
        let mut input = JobInput::new(bytes, name);
        input.dsn_dir = dsn.parent().map(Path::to_path_buf);
        let Ok(job) = prepare_job(&input, &ctx()) else {
            return None;
        };
        let rules = rules_from_board(&job.board, Some(&job.settings), name);
        let text = rules.to_rules();

        let mut fresh_input = input.clone();
        fresh_input.rules = RulesChoice::None;
        let fresh = prepare_job(&fresh_input, &ctx()).unwrap();
        let (applied, result) = read_rules(fresh.board, &text, name);
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(e) => {
                failures.push(format!("{stem}: the written rules do not read: {e}"));
                return Some(failures);
            }
        };
        if let Some(difference) = round_trip_difference(&job.board, &applied) {
            failures.push(format!("{stem}: {difference}"));
        }
        // The settings.
        let mut patch = fresh.patch.clone();
        patch.apply_from(&outcome.autoroute.expect("autoroute_settings"));
        let resolved = freerouting::settings::RouterSettings::from_patch(&patch, 8);
        let original = &job.settings;
        let mut layer_difference = None;
        for i in 0..job.board.layer_count() {
            let same = resolved.layer_active(i) == original.layer_active(i)
                && resolved.preferred_direction_is_horizontal(i)
                    == original.preferred_direction_is_horizontal(i)
                && same_float(
                    resolved.preferred_direction_trace_costs(i),
                    original.preferred_direction_trace_costs(i),
                )
                && same_float(
                    resolved.against_preferred_direction_trace_costs(i),
                    original.against_preferred_direction_trace_costs(i),
                );
            if !same {
                layer_difference = Some(i);
            }
        }
        if resolved.scoring.via_costs != original.scoring.via_costs
            || resolved.scoring.plane_via_costs != original.scoring.plane_via_costs
            || resolved.scoring.start_ripup_costs != original.scoring.start_ripup_costs
            || resolved.run_router() != original.run_router()
            || resolved.run_optimizer() != original.run_optimizer()
            || resolved.allowed_via_types != original.allowed_via_types
            || layer_difference.is_some()
        {
            failures.push(format!("{stem}: settings (layer {layer_difference:?})"));
        }
    }
    Some(failures)
}

fn finish_round_trip(compared: usize, failures: Vec<String>) {
    eprintln!("round trip on {compared} boards");
    assert!(compared > 100, "{compared} boards");
    let known: Vec<&str> = ROUND_TRIP_MISMATCHES.iter().map(|(k, _)| *k).collect();
    let unexpected: Vec<&String> = failures
        .iter()
        .filter(|f| !known.iter().any(|k| f.starts_with(&format!("{k}:"))))
        .collect();
    assert!(
        unexpected.is_empty(),
        "{} boards differ:\n{}",
        unexpected.len(),
        unexpected
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    for (stem, _) in ROUND_TRIP_MISMATCHES {
        assert!(
            failures.iter().any(|f| f.starts_with(&format!("{stem}:"))),
            "listed mismatch {stem} no longer occurs; remove it"
        );
    }
}

/// Boards whose round trip differs, with the reason.
const ROUND_TRIP_MISMATCHES: &[(&str, &str)] = &[
    ("Issue029-hw48na", PAIR_RULES),
    ("Issue593-BBD_Mars-64", PAIR_RULES),
    ("Issue742-tastexx-pcb/tastexx-pcb", PAIR_RULES),
    ("Issue179-Autorouter_PCB1_2023-3-24", EMPTY_CLASS_NAME),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
        EMPTY_CLASS_NAME,
    ),
];

/// The adjacent rules file creates clearance classes with pair rules
/// (`(clearance v (type "default"-"1A EXTERNAL 1oz"))`). The written file has
/// the single-name types Java writes, which its reader (and the port's) skips,
/// so the classes are not created, the class order differs, and a via type
/// that names a class falls back to `default`.
const PAIR_RULES: &str = "classes created by pair rules in the adjacent rules file";

/// A clearance class with an empty name (`''` in the DSN). Java writes its
/// clearance as `(clearance v (type ))`, which reads as a clearance without a
/// type: the default clearance of every layer.
const EMPTY_CLASS_NAME: &str =
    "a clearance class with an empty name overrides the default clearance";

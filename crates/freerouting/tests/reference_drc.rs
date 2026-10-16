//! Runs the design rules check on every fixture, as loaded (`load`) and
//! with the routed session of `fixtures/reference/route` imported
//! (`route`), and compares the report with what Java's DRC command line
//! (`-de board.dsn [board.ses] -drc report.json`) produced for the same
//! input, as digested in `tests/data/java_drc_digest.tsv` (see
//! `tests/data/java_drc_digest.py` for the format).
//!
//! Compared exactly: the clearance and hole clearance violations and the
//! dangling vias (descriptions, item ids and positions, in report order),
//! the unconnected item groups (as sets: Java picks a group's
//! representatives in hash order) and the quality and optimizer scores.
//! Dangling traces are compared up to that representative choice: a
//! trace that is a group's representative is not reported as dangling.
//! The copper island findings are not compared (the port decomposes the
//! fill differently, see docs/DIVERGENCES.md) and not computed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use freerouting::drc::json::JsonValue;
use freerouting::drc::{
    DesignRulesChecker, DrcReport, DrcRunOptions, DrcViolation, load_design, run_drc,
};
use freerouting::io::read_session;

const ISSUE721: &str = "Issue721-Autorouter_CE2632_HarryMu_2026-6-15.dsn";
const ISSUE721_WHY: &str = "Java keeps the single quotes of the net names in the class \
    net lists, so the nets stay in the default class (0.41 mil clearance); the Rust DSN \
    reader accepts ' as a quote and gives the items their class's 0.4 mil \
    (docs/DIVERGENCES.md, freerouting-dsn): same violations, other expected clearances";
const ISSUE676: &str = "Issue676-ch32v-tx118s.dsn";
const ISSUE676_WHY: &str = "Java's SesReader fails on all 48 wires with a \
    ClassCastException (RationalVector to IntVector) while normalizing the traces of this \
    1 000 000-per-mm board and imports none; the port imports them";
const ISSUE289: &str = "Issue289-Autorouter_PCB_FHT-VGA_2024-03-25.dsn";
const ISSUE289_WHY: &str = "Java's DRC command line imports the session while the \
    post-load DRC still runs in a background thread; the race leaves net +5V in the \
    outline's edge-pin-net cache, which hides 6 trace-to-outline violations. Run without \
    the race (post-load processing finished first), Java reports the 12 the port reports";

/// Differences from the Java reports that are expected, as
/// `(mode, fixture, type, reason)`.
const EXPECTED_MISMATCHES: &[(&str, &str, &str, &str)] = &[
    (
        "load",
        "Issue179-Autorouter_PCB1_2023-3-24.dsn",
        "scores",
        "Java's BoardStatistics throws a NullPointerException in the Delaunay \
         triangulation of the incomplete connections, so Java writes no scores; the \
         port computes them (docs/DIVERGENCES.md)",
    ),
    ("load", ISSUE721, "holeClearance", ISSUE721_WHY),
    ("load", ISSUE721, "scores", ISSUE721_WHY),
    ("route", ISSUE721, "holeClearance", ISSUE721_WHY),
    ("route", ISSUE721, "scores", ISSUE721_WHY),
    ("route", ISSUE289, "clearance", ISSUE289_WHY),
    ("route", ISSUE289, "scores", ISSUE289_WHY),
    ("route", ISSUE676, "holeClearance", ISSUE676_WHY),
    ("route", ISSUE676, "via_dangling", ISSUE676_WHY),
    ("route", ISSUE676, "unconnectedItems", ISSUE676_WHY),
    ("route", ISSUE676, "track_dangling", ISSUE676_WHY),
    ("route", ISSUE676, "scores", ISSUE676_WHY),
];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// FNV-1a, 64 bits.
fn fnv(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// A number as the report writes it, in the digest's notation (Python's
/// `repr` of the parsed value, which Rust's `Debug` matches).
fn number(value: Option<f64>) -> String {
    match value.map(JsonValue::two_decimals) {
        Some(JsonValue::Number(text)) => format!("{:?}", text.parse::<f64>().unwrap()),
        _ => "null".into(),
    }
}

fn line(v: &DrcViolation) -> String {
    let mut parts = vec![v.description.clone()];
    for item in &v.items {
        parts.push(format!(
            "{}@{},{}",
            item.uuid,
            number(Some(item.pos.x)),
            number(Some(item.pos.y))
        ));
    }
    parts.join("\t") + "\n"
}

/// One digest line: `(count, value)` per type.
type Digest = BTreeMap<String, (String, String)>;

fn ids(v: &DrcViolation) -> Vec<String> {
    let mut ids: Vec<String> = v.items.iter().map(|i| i.uuid.clone()).collect();
    ids.sort_by_key(|id| id.parse::<i64>().unwrap_or(0));
    ids
}

fn digest(report: &DrcReport) -> Digest {
    let mut result = Digest::new();
    for t in ["clearance", "holeClearance", "via_dangling"] {
        let vs: Vec<&DrcViolation> = report
            .violations
            .iter()
            .filter(|v| v.violation_type == t)
            .collect();
        let text: String = vs.iter().map(|v| line(v)).collect();
        result.insert(t.into(), (vs.len().to_string(), fnv(&text)));
    }
    let mut groups: Vec<String> = report
        .unconnected_items
        .iter()
        .filter(|v| v.violation_type == "unconnectedItems")
        .map(|v| ids(v).join(","))
        .collect();
    groups.sort();
    result.insert(
        "unconnectedItems".into(),
        (groups.len().to_string(), fnv(&groups.join("\n"))),
    );
    let mut dangling: Vec<String> = report
        .violations
        .iter()
        .filter(|v| v.violation_type == "track_dangling")
        .map(|v| v.items[0].uuid.clone())
        .collect();
    dangling.sort_by_key(|id| id.parse::<i64>().unwrap_or(0));
    result.insert(
        "track_dangling".into(),
        (dangling.len().to_string(), dangling.join(",")),
    );
    result.insert(
        "scores".into(),
        (number(report.quality_score), number(report.optimizer_score)),
    );
    result
}

/// The Java digests of `mode`, by fixture.
fn java_digests(mode: &str) -> BTreeMap<String, Digest> {
    let text = include_str!("data/java_drc_digest.tsv");
    let mut result: BTreeMap<String, Digest> = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields[0] != mode {
            continue;
        }
        result.entry(fields[1].to_string()).or_default().insert(
            fields[2].to_string(),
            (
                fields[3].to_string(),
                fields.get(4).unwrap_or(&"").to_string(),
            ),
        );
    }
    result
}

/// The report of the Rust DRC on `fixture`, with the routed session for
/// `route`.
fn rust_report(mode: &str, fixture: &str) -> DrcReport {
    let path = fixtures_dir().join(fixture);
    let mut board =
        load_design(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{fixture}: {e}"));
    if mode == "route" {
        let ses = fixtures_dir()
            .join("reference/route")
            .join(fixture)
            .with_extension("ses");
        let bytes = std::fs::read(&ses).unwrap();
        read_session(&mut board, &String::from_utf8_lossy(&bytes))
            .unwrap_or_else(|e| panic!("{fixture}: {e}"));
    }
    let mut checker = DesignRulesChecker::new(&board);
    checker.set_check_zone_islands(false);
    let source = path.file_name().unwrap().to_string_lossy().into_owned();
    checker.generate_report(&source, "mm")
}

/// The ids of all items of the unconnected groups of `report`.
fn group_members(report: &DrcReport) -> BTreeSet<String> {
    report
        .unconnected_items
        .iter()
        .filter(|v| v.violation_type == "unconnectedItems")
        .flat_map(|v| v.items.iter().map(|i| i.uuid.clone()))
        .collect()
}

/// The differences of one fixture, as `(type, detail)`.
fn compare(java: &Digest, report: &DrcReport) -> Vec<(String, String)> {
    let rust = digest(report);
    let mut result = Vec::new();
    for (key, (java_count, java_value)) in java {
        let (rust_count, rust_value) = &rust[key];
        if key == "track_dangling" {
            // Java and Rust may pick different tails as group
            // representatives; those are not dangling in that report.
            let split = |s: &str| -> BTreeSet<String> {
                s.split(',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            };
            let (j, r) = (split(java_value), split(rust_value));
            let members = group_members(report);
            let unexplained: Vec<&String> = j
                .symmetric_difference(&r)
                .filter(|id| !members.contains(*id))
                .collect();
            if !unexplained.is_empty() {
                result.push((
                    key.clone(),
                    format!("java {java_count}, rust {rust_count}, differing {unexplained:?}"),
                ));
            }
        } else if (java_count, java_value) != (rust_count, rust_value) {
            result.push((
                key.clone(),
                format!("java {java_count} {java_value}, rust {rust_count} {rust_value}"),
            ));
        }
    }
    result
}

fn compare_mode(mode: &str) {
    let java = java_digests(mode);
    assert!(!java.is_empty());
    let fixtures: Vec<(&String, &Digest)> = java.iter().collect();
    // The fixtures are independent: check them on all cores.
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut differences: Vec<(usize, String, String, String)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut found = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some((fixture, digest)) = fixtures.get(index) else {
                            return found;
                        };
                        let report = rust_report(mode, fixture);
                        for (key, detail) in compare(digest, &report) {
                            found.push((index, fixture.to_string(), key, detail));
                        }
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    differences.sort();

    let mut failures = Vec::new();
    let mut seen = Vec::new();
    for (_, fixture, key, detail) in &differences {
        let expected = EXPECTED_MISMATCHES
            .iter()
            .find(|(m, f, k, _)| *m == mode && f == fixture && k == key);
        match expected {
            Some((_, _, _, why)) => {
                println!("expected difference: {mode} {fixture} {key}: {detail} ({why})");
                seen.push((fixture.clone(), key.clone()));
            }
            None => failures.push(format!("{fixture} {key}: {detail}")),
        }
    }
    println!(
        "{mode}: {} fixtures, {} with differences",
        fixtures.len(),
        differences
            .iter()
            .map(|d| &d.1)
            .collect::<BTreeSet<_>>()
            .len()
    );
    for (m, fixture, key, _) in EXPECTED_MISMATCHES {
        if *m == mode {
            assert!(
                seen.iter().any(|(f, k)| f == fixture && k == key),
                "listed difference {mode} {fixture} {key} no longer occurs; remove it"
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences from the Java DRC ({mode}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn drc_of_loaded_boards_matches_java() {
    compare_mode("load");
}

#[test]
fn drc_of_routed_boards_matches_java() {
    compare_mode("route");
}

// ---- C04.5: rules files in the DRC runner -------------------------------------

/// The Java digests of the DRC runs with a rules file
/// (`data/java_drc_rules_digest.tsv`), by rules file name.
fn java_rules_digests() -> BTreeMap<String, Digest> {
    let text = include_str!("data/java_drc_rules_digest.tsv");
    let mut result: BTreeMap<String, Digest> = BTreeMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let fields: Vec<&str> = line.split('\t').collect();
        result.entry(fields[1].to_string()).or_default().insert(
            fields[2].to_string(),
            (
                fields[3].to_string(),
                fields.get(4).unwrap_or(&"").to_string(),
            ),
        );
    }
    result
}

/// The log lines of C01.6's `clirun` row about the rules file, without the
/// level: the ones from `RulesReader` and `Network`. Left out are Java's
/// `DsnFile.read_string_scope` lines, which its scanner prints while it
/// recovers from a `(via_rule 1A EXTERNAL 1oz)` with unquoted spaces; the
/// port's parser skips the extra words with a warning of its own
/// (docs/DIVERGENCES.md, "Rules files").
fn java_rules_log_lines(row: &str) -> Vec<String> {
    let text = std::fs::read_to_string(fixtures_dir().join(format!("reference/clirun/{row}.json")))
        .unwrap();
    let JsonValue::Object(members) = JsonValue::parse(&text).unwrap() else {
        panic!("clirun row is an object")
    };
    let Some((_, JsonValue::Array(lines))) = members.iter().find(|(k, _)| k == "logLines") else {
        panic!("no logLines")
    };
    lines
        .iter()
        .filter_map(|l| match l {
            JsonValue::String(s) => s.strip_prefix("WARN   "),
            _ => None,
        })
        .filter(|l| l.starts_with("RulesReader.") || l.starts_with("Network.insert_net_class"))
        .map(String::from)
        .collect()
}

/// Rows 07 and 08 of C01.6's `clirun` dump: `-de Issue029-hw48na.dsn -dr
/// <valid or invalid>.rules -drc out.json`. The violations of the report
/// (per type: count, and a hash of the violations in report order) and the
/// scores equal Java's, which a rules file changes a lot (21 clearance
/// violations without it, 2 with it).
#[test]
fn drc_with_a_rules_file_matches_java() {
    let java = java_rules_digests();
    assert_eq!(java.len(), 2);
    for (rules, row) in [
        ("Issue029-hw48na_valid.rules", "07-drc-valid-rules"),
        ("Issue029-hw48na_invalid.rules", "08-drc-invalid-rules"),
    ] {
        let mut options = DrcRunOptions::new(fixtures_dir().join("Issue029-hw48na.dsn"));
        options.rules_file = Some(fixtures_dir().join(rules));
        let run = run_drc(&options).unwrap();
        let differences = compare(&java[rules], &run.report);
        assert!(differences.is_empty(), "{rules}: {differences:?}");

        // The log lines match Java's text, in order.
        let rules_path = options.rules_file.as_ref().unwrap().display().to_string();
        assert!(
            run.messages
                .contains(&format!("Loading RULES file for DRC: {rules_path}")),
            "{:?}",
            run.messages
        );
        assert!(
            run.messages
                .contains(&"RULES file loaded for DRC successfully".to_string())
        );
        let rust: Vec<&String> = run
            .messages
            .iter()
            .filter(|l| l.starts_with("RulesReader.") || l.starts_with("Network.insert_net_class"))
            .collect();
        assert_eq!(
            rust,
            java_rules_log_lines(row).iter().collect::<Vec<_>>(),
            "{rules}"
        );
    }
}

#[test]
fn a_missing_rules_file_is_a_warning() {
    let mut options = DrcRunOptions::new(fixtures_dir().join("Issue029-hw48na.dsn"));
    options.rules_file = Some(PathBuf::from("no/such/file.rules"));
    let run = run_drc(&options).unwrap();
    assert!(
        run.messages
            .contains(&"RULES file for DRC not found: no/such/file.rules".to_string()),
        "{:?}",
        run.messages
    );
    // The report is the one without rules.
    let java = java_digests("load");
    let differences = compare(&java["Issue029-hw48na.dsn"], &run.report);
    assert!(differences.is_empty(), "{differences:?}");
}

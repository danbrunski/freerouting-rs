//! Builds a board from every fixture DSN and compares it with what Java's
//! `DsnReader` and `BoardStatistics` produced for the same file
//! (`fixtures/reference/load/<path>.json`, see `tools/java-reference/`).
//!
//! **Profile (N01.2).** This comparison reads no router settings: the board
//! load and the DRC take none, so `router.profile` cannot reach it and it is
//! `legacy` by construction. A board-level behaviour that a profile switches
//! (N04's contacts inside pads) must be pinned to `legacy` here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use freerouting::io::{BoardReadResult, read_board};
use freerouting::scoring::BoardStatistics;

/// Differences from the Java reference that are expected, as
/// `(fixture path, field, reason)`. A field of `"*"` covers the whole
/// fixture.
const ISSUE721_QUOTES: &str = "the network classes list their nets as 'GND', 'VCC', ...; \
     Java's raw string reader keeps the single quotes, so no net joins its class and the \
     items keep the default clearance (0.41 mil); the Rust DSN reader accepts ' as a \
     quote (docs/DIVERGENCES.md, freerouting-dsn), so the items get their class's \
     0.4 mil and the violation depths differ (the counts match)";

const EXPECTED_MISMATCHES: &[(&str, &str, &str)] = &[
    (
        "Issue179-Autorouter_PCB1_2023-3-24.dsn",
        "result",
        "Java loads the board, then BoardStatistics throws a NullPointerException \
         (\"edgeLines\") in the Delaunay triangulation of the incomplete-connection \
         count (drc.NetIncompletes), so the reference records an exception; the Rust \
         triangulation skips the corners outside its bounding triangle and computes \
         the statistics (docs/DIVERGENCES.md)",
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15.dsn",
        "clearance_violations.total_violation_um",
        ISSUE721_QUOTES,
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15.dsn",
        "clearance_violations.min_violation_um",
        ISSUE721_QUOTES,
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15.dsn",
        "clearance_violations.max_violation_um",
        ISSUE721_QUOTES,
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15.dsn",
        "clearance_violations.avg_violation_um",
        ISSUE721_QUOTES,
    ),
];

// ---------------------------------------------------------------------------
// A minimal JSON reader, enough for the reference files.

#[derive(Clone, Debug)]
enum Json {
    Null,
    Bool,
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(map) => map.get(key),
            _ => None,
        }
    }

    fn path(&self, path: &str) -> Option<&Json> {
        path.split('.').try_fold(self, |json, key| json.get(key))
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl JsonParser<'_> {
    fn parse(text: &str) -> Json {
        let mut parser = JsonParser {
            bytes: text.as_bytes(),
            pos: 0,
        };
        let value = parser.value();
        parser.skip_ws();
        assert_eq!(parser.pos, parser.bytes.len(), "trailing characters");
        value
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn eat(&mut self, c: u8) {
        self.skip_ws();
        assert_eq!(
            self.bytes[self.pos], c,
            "expected '{}' at {}",
            c as char, self.pos
        );
        self.pos += 1;
    }

    fn peek(&mut self) -> u8 {
        self.skip_ws();
        self.bytes[self.pos]
    }

    fn value(&mut self) -> Json {
        match self.peek() {
            b'{' => {
                self.eat(b'{');
                let mut map = BTreeMap::new();
                if self.peek() == b'}' {
                    self.pos += 1;
                    return Json::Object(map);
                }
                loop {
                    let key = self.string();
                    self.eat(b':');
                    map.insert(key, self.value());
                    if self.peek() == b',' {
                        self.pos += 1;
                    } else {
                        self.eat(b'}');
                        return Json::Object(map);
                    }
                }
            }
            b'[' => {
                self.eat(b'[');
                let mut items = Vec::new();
                if self.peek() == b']' {
                    self.pos += 1;
                    return Json::Array(items);
                }
                loop {
                    items.push(self.value());
                    if self.peek() == b',' {
                        self.pos += 1;
                    } else {
                        self.eat(b']');
                        return Json::Array(items);
                    }
                }
            }
            b'"' => Json::String(self.string()),
            b't' => self.literal("true", Json::Bool),
            b'f' => self.literal("false", Json::Bool),
            b'n' => self.literal("null", Json::Null),
            _ => {
                let start = self.pos;
                while self.pos < self.bytes.len()
                    && matches!(
                        self.bytes[self.pos],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.pos += 1;
                }
                let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
                Json::Number(text.parse().unwrap_or_else(|_| panic!("bad number {text}")))
            }
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Json {
        assert!(self.bytes[self.pos..].starts_with(word.as_bytes()));
        self.pos += word.len();
        value
    }

    fn hex4(&mut self) -> u32 {
        let text = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4]).unwrap();
        self.pos += 4;
        u32::from_str_radix(text, 16).unwrap()
    }

    fn string(&mut self) -> String {
        self.eat(b'"');
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.bytes[self.pos];
            self.pos += 1;
            match c {
                b'"' => return String::from_utf8(out).unwrap(),
                b'\\' => {
                    let e = self.bytes[self.pos];
                    self.pos += 1;
                    let ch = match e {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'u' => {
                            let mut code = self.hex4();
                            if (0xD800..0xDC00).contains(&code)
                                && self.bytes[self.pos..].starts_with(b"\\u")
                            {
                                self.pos += 2;
                                let low = self.hex4();
                                code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                            }
                            char::from_u32(code).unwrap_or('\u{FFFD}')
                        }
                        other => other as char,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
    }
}

// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        // `reference/` holds the Java harness's outputs. `kicad/` holds the K
        // series' KiCad exports, which K07 compares with Java itself.
        if path == fixtures_dir().join("reference") || path == fixtures_dir().join("kicad") {
            continue;
        }
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dsn"))
        {
            out.push(path);
        }
    }
}

/// The values compared for one fixture, as `(field, value)` pairs.
type Fields = Vec<(&'static str, String)>;

/// Integer fields compared exactly.
const COUNT_FIELDS: &[&str] = &[
    "layers.total_count",
    "layers.signal_count",
    "items.total_count",
    "items.trace_count",
    "items.via_count",
    "items.conduction_area_count",
    "items.drill_item_count",
    "items.pin_count",
    "items.component_count",
    "items.other_count",
    "components.total_count",
    "pads.total_count",
    "nets.total_count",
    "nets.class_count",
    "traces.total_count",
    "traces.total_segment_count",
    "vias.total_count",
    "vias.through_hole_count",
    "vias.blind_count",
    "vias.buried_count",
    "bends.total_count",
    "bends.90_degree_count",
    "bends.45_degree_count",
    "bends.other_angle_count",
    "difficulty.pin_count",
    "difficulty.signal_layer_count",
    "difficulty.complexity_c",
    "bounds.min_via_count",
    "bounds.min_bend_count",
    "fanout.total_smd_pins",
    "fanout.pins_to_escape",
    "fanout.escaped_count",
    "connections.maximum_count",
    "connections.incomplete_count",
    "clearance_violations.total_count",
    "clearance_violations.pre_existing_count",
    "clearance_violations.unfixable_count",
    "clearance_violations.router_introduced_count",
];

/// Floating point fields, written by Java with two decimals.
const FLOAT_FIELDS: &[&str] = &[
    "board.bounding_box.x",
    "board.bounding_box.y",
    "board.bounding_box.width",
    "board.bounding_box.height",
    "board.size.x",
    "board.size.y",
    "board.size.width",
    "board.size.height",
    "board.area_cm2",
    "traces.total_length",
    "traces.total_length_mm",
    "traces.total_weighted_length",
    "traces.average_length",
    "traces.total_vertical_length",
    "traces.total_horizontal_length",
    "traces.total_angled_length",
    "difficulty.difficulty_d",
    "difficulty.board_area_cm2",
    "bounds.min_trace_length_mm",
    "clearance_violations.total_violation_um",
    "clearance_violations.min_violation_um",
    "clearance_violations.max_violation_um",
    "clearance_violations.avg_violation_um",
];

fn rust_count(stats: &BoardStatistics, field: &str) -> i64 {
    let value = match field {
        "layers.total_count" => stats.layers.total_count,
        "layers.signal_count" => stats.layers.signal_count,
        "items.total_count" => stats.items.total_count,
        "items.trace_count" => stats.items.trace_count,
        "items.via_count" => stats.items.via_count,
        "items.conduction_area_count" => stats.items.conduction_area_count,
        "items.drill_item_count" => stats.items.drill_item_count,
        "items.pin_count" => stats.items.pin_count,
        "items.component_count" => stats.items.component_outline_count,
        "items.other_count" => stats.items.other_count,
        "components.total_count" => stats.components.total_count,
        "pads.total_count" => stats.pads.total_count,
        "nets.total_count" => return i64::from(stats.nets.total_count),
        "nets.class_count" => stats.nets.class_count,
        "traces.total_count" => stats.traces.total_count,
        "traces.total_segment_count" => stats.traces.total_segment_count,
        "vias.total_count" => stats.vias.total_count,
        "vias.through_hole_count" => stats.vias.through_hole_count,
        "vias.blind_count" => stats.vias.blind_count,
        "vias.buried_count" => stats.vias.buried_count,
        "bends.total_count" => stats.bends.total_count,
        "bends.90_degree_count" => stats.bends.ninety_degree_count,
        "bends.45_degree_count" => stats.bends.forty_five_degree_count,
        "bends.other_angle_count" => stats.bends.other_angle_count,
        "difficulty.pin_count" => stats.difficulty.pin_count,
        "difficulty.signal_layer_count" => stats.difficulty.signal_layer_count,
        "difficulty.complexity_c" => stats.difficulty.complexity_c,
        "bounds.min_via_count" => return i64::from(stats.bounds.min_via_count),
        "bounds.min_bend_count" => return i64::from(stats.bounds.min_bend_count),
        "fanout.total_smd_pins" => stats.fanout.total_smd_pins,
        "fanout.pins_to_escape" => stats.fanout.pins_to_escape,
        "fanout.escaped_count" => stats.fanout.escaped_count,
        "connections.maximum_count" => stats.connections.maximum_count,
        "connections.incomplete_count" => stats.connections.incomplete_count,
        "clearance_violations.total_count" => stats.clearance_violations.total_count,
        "clearance_violations.pre_existing_count" => stats.clearance_violations.pre_existing_count,
        "clearance_violations.unfixable_count" => stats.clearance_violations.unfixable_count,
        "clearance_violations.router_introduced_count" => {
            stats.clearance_violations.router_introduced_count
        }
        _ => unreachable!("{field}"),
    };
    value as i64
}

fn rust_float(stats: &BoardStatistics, field: &str) -> f32 {
    match field {
        "board.bounding_box.x" => stats.board.bounding_box.x,
        "board.bounding_box.y" => stats.board.bounding_box.y,
        "board.bounding_box.width" => stats.board.bounding_box.width,
        "board.bounding_box.height" => stats.board.bounding_box.height,
        "board.size.x" => stats.board.size.x,
        "board.size.y" => stats.board.size.y,
        "board.size.width" => stats.board.size.width,
        "board.size.height" => stats.board.size.height,
        "board.area_cm2" => stats.board.area_cm2,
        "traces.total_length" => stats.traces.total_length,
        "traces.total_length_mm" => stats.traces.total_length_mm,
        "traces.total_weighted_length" => stats.traces.total_weighted_length,
        "traces.average_length" => stats.traces.average_length,
        "traces.total_vertical_length" => stats.traces.total_vertical_length,
        "traces.total_horizontal_length" => stats.traces.total_horizontal_length,
        "traces.total_angled_length" => stats.traces.total_angled_length,
        "difficulty.difficulty_d" => stats.difficulty.difficulty_d,
        "difficulty.board_area_cm2" => stats.difficulty.board_area_cm2,
        "bounds.min_trace_length_mm" => stats.bounds.min_trace_length_mm,
        "clearance_violations.total_violation_um" => {
            stats.clearance_violations.total_violation_um as f32
        }
        "clearance_violations.min_violation_um" => {
            stats.clearance_violations.min_violation_um as f32
        }
        "clearance_violations.max_violation_um" => {
            stats.clearance_violations.max_violation_um as f32
        }
        "clearance_violations.avg_violation_um" => {
            stats.clearance_violations.avg_violation_um as f32
        }
        _ => unreachable!("{field}"),
    }
}

/// Java prints floats with `%.2f`; accept the rounding of that plus a
/// relative error from summing in a different order.
fn floats_match(java: f64, rust: f32) -> bool {
    let rust = f64::from(rust);
    let tolerance = 0.006 + java.abs() * 1e-5;
    (java - rust).abs() <= tolerance
}

fn expected(fixture: &str, field: &str) -> Option<&'static str> {
    EXPECTED_MISMATCHES
        .iter()
        .find(|(f, fld, _)| *f == fixture && (*fld == "*" || *fld == field))
        .map(|(_, _, why)| *why)
}

#[derive(Default)]
struct Comparison {
    fixtures: usize,
    outcome_matches: usize,
    fully_matching: usize,
    field_matches: BTreeMap<&'static str, (usize, usize)>,
    failures: Vec<String>,
    expected_seen: Vec<String>,
}

fn compare_fixture(path: &Path, comparison: &mut Comparison) {
    let relative = path.strip_prefix(fixtures_dir()).unwrap();
    let shown = relative.to_string_lossy().replace('\\', "/");
    let reference_path = fixtures_dir()
        .join("reference/load")
        .join(relative)
        .with_extension("json");
    let reference = JsonParser::parse(
        &std::fs::read_to_string(&reference_path)
            .unwrap_or_else(|e| panic!("{}: {e}", reference_path.display())),
    );
    comparison.fixtures += 1;
    let mut mismatches: Fields = Vec::new();

    let bytes = std::fs::read(path).unwrap();
    let result = read_board(&bytes);
    let rust_outcome = match &result {
        BoardReadResult::Success { .. } => "success",
        BoardReadResult::OutlineMissing { .. } => "outline_missing",
        BoardReadResult::ParseError { .. } => "parse_error",
    };
    let java_outcome = reference.get("result").and_then(Json::as_str).unwrap();
    if rust_outcome == java_outcome {
        comparison.outcome_matches += 1;
    } else {
        mismatches.push((
            "result",
            format!("java {java_outcome}, rust {rust_outcome}"),
        ));
    }

    if let (BoardReadResult::ParseError { location, .. }, Some(java_location)) =
        (&result, reference.path("location").and_then(Json::as_str))
        && location != java_location
    {
        mismatches.push(("location", format!("java {java_location}, rust {location}")));
    }

    let warnings = match &result {
        BoardReadResult::Success { warnings, .. }
        | BoardReadResult::OutlineMissing { warnings, .. } => warnings.clone(),
        BoardReadResult::ParseError { .. } => Vec::new(),
    };
    if let Some(Json::Array(java_warnings)) = reference.get("warnings") {
        let java_warnings: Vec<&str> = java_warnings.iter().filter_map(Json::as_str).collect();
        if java_warnings != warnings.iter().map(String::as_str).collect::<Vec<_>>() {
            mismatches.push((
                "warnings",
                format!(
                    "java {} warnings, rust {} (first rust: {:?})",
                    java_warnings.len(),
                    warnings.len(),
                    warnings.first()
                ),
            ));
        }
    }

    if let (Some(board), Some(java_stats)) = (result.board(), reference.get("statistics")) {
        let stats = BoardStatistics::new(board);
        let java_host = java_stats
            .get("host")
            .and_then(Json::as_str)
            .unwrap_or("null");
        if java_host != stats.host {
            mismatches.push(("host", format!("java {java_host:?}, rust {:?}", stats.host)));
        }
        for &field in COUNT_FIELDS {
            let Some(java) = java_stats.path(field).and_then(Json::as_f64) else {
                continue;
            };
            let rust = rust_count(&stats, field);
            let entry = comparison.field_matches.entry(field).or_default();
            entry.1 += 1;
            if java as i64 == rust {
                entry.0 += 1;
            } else {
                mismatches.push((field, format!("java {java}, rust {rust}")));
            }
        }
        for &field in FLOAT_FIELDS {
            let Some(java) = java_stats.path(field).and_then(Json::as_f64) else {
                continue;
            };
            let rust = rust_float(&stats, field);
            let entry = comparison.field_matches.entry(field).or_default();
            entry.1 += 1;
            if floats_match(java, rust) {
                entry.0 += 1;
            } else {
                mismatches.push((field, format!("java {java:.2}, rust {rust:.4}")));
            }
        }
    }

    let mut unexpected = 0;
    for (field, detail) in mismatches {
        let message = format!("{shown}: {field}: {detail}");
        if let Some(why) = expected(&shown, field) {
            comparison
                .expected_seen
                .push(format!("{message} (expected: {why})"));
        } else {
            unexpected += 1;
            comparison.failures.push(message);
        }
    }
    if unexpected == 0 {
        comparison.fully_matching += 1;
    }
}

#[test]
fn boards_match_java_reference() {
    let mut paths = Vec::new();
    collect(&fixtures_dir(), &mut paths);
    assert!(!paths.is_empty());
    // The fixtures are independent: compare them on all cores, then merge
    // the results in fixture order.
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut per_fixture: Vec<(usize, Comparison)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = paths.get(index) else {
                            return done;
                        };
                        let mut comparison = Comparison::default();
                        compare_fixture(path, &mut comparison);
                        done.push((index, comparison));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect()
    });
    per_fixture.sort_by_key(|(index, _)| *index);
    let mut comparison = Comparison::default();
    for (_, part) in per_fixture {
        comparison.fixtures += part.fixtures;
        comparison.outcome_matches += part.outcome_matches;
        comparison.fully_matching += part.fully_matching;
        for (field, (matching, total)) in part.field_matches {
            let entry = comparison.field_matches.entry(field).or_default();
            entry.0 += matching;
            entry.1 += total;
        }
        comparison.failures.extend(part.failures);
        comparison.expected_seen.extend(part.expected_seen);
    }

    println!(
        "{} fixtures: {} same outcome, {} without unexpected differences",
        comparison.fixtures, comparison.outcome_matches, comparison.fully_matching
    );
    for (field, (matching, total)) in &comparison.field_matches {
        println!("  {field}: {matching}/{total}");
    }
    for seen in &comparison.expected_seen {
        println!("expected difference: {seen}");
    }
    for (fixture, field, _) in EXPECTED_MISMATCHES {
        assert!(
            comparison
                .expected_seen
                .iter()
                .any(|seen| seen.starts_with(&format!(
                    "{fixture}: {}",
                    if *field == "*" { "" } else { field }
                ))),
            "listed difference {fixture} {field} no longer occurs; remove it from EXPECTED_MISMATCHES"
        );
    }
    assert!(
        comparison.failures.is_empty(),
        "{} differences from the Java reference:\n{}",
        comparison.failures.len(),
        comparison.failures.join("\n")
    );
}

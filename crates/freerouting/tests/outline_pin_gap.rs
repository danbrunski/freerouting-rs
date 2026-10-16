//! C03.2: `BoardOutline::minimum_pin_gap` (Java `BoardOutline.minimumPinGap`).
//!
//! The comparison with C01.1's `minimumPinGap` runs on every board of the
//! `settings` dumps and compares Java's `Double.toString` text with the
//! port's, so equality is bit for bit.

use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::board::state::Communication;
use freerouting::board::structure::{Layer, LayerStructure};
use freerouting::drc::json::JsonValue;
use freerouting::geometry::{Circle, IntBox, IntPoint, PolylineShape, Shape, TileShape};
use freerouting::io::{BoardReadResult, read_board};
use freerouting::rules::{BoardRules, ClearanceMatrix};
use freerouting::settings::text::java_double_to_string;

fn bare_board() -> RoutingBoard {
    let layers = LayerStructure::new(vec![Layer::new("Top", true), Layer::new("Bottom", true)]);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    let outline = IntBox::new(0, 0, 1_000, 500);
    RoutingBoard::new(
        outline,
        layers,
        vec![PolylineShape::Tile(TileShape::from(outline))],
        1,
        rules,
        Communication::default(),
    )
}

fn gap_of(shapes: &[Shape]) -> f64 {
    let board = bare_board();
    board
        .outline()
        .and_then(|i| i.as_board_outline())
        .unwrap()
        .minimum_pin_gap(shapes.iter())
}

fn boxed(ll: (i32, i32), ur: (i32, i32)) -> Shape {
    Shape::from(IntBox::new(ll.0, ll.1, ur.0, ur.1))
}

#[test]
fn a_board_without_pins_has_an_infinite_gap() {
    let board = bare_board();
    assert_eq!(board.minimum_pin_gap(), f64::INFINITY);
    assert_eq!(java_double_to_string(board.minimum_pin_gap()), "Infinity");
    assert_eq!(gap_of(&[]), f64::INFINITY);
}

#[test]
fn the_gap_is_the_smallest_corner_distance_to_the_outline_lines() {
    // The pad corner (10, 40) is 10 from the left edge; (60, 40) is 40 from the bottom.
    assert_eq!(gap_of(&[boxed((10, 40), (60, 100))]), 10.0);
    // The smallest over all shapes.
    assert_eq!(
        gap_of(&[boxed((100, 100), (200, 200)), boxed((10, 40), (60, 100))]),
        10.0
    );
    assert_eq!(
        gap_of(&[boxed((100, 100), (200, 200)), boxed((300, 7), (400, 100))]),
        7.0
    );
}

#[test]
fn a_pad_that_touches_or_leaves_the_outline_has_gap_zero() {
    // A corner on the outline line is "inside" the closed outline at distance 0.
    assert_eq!(gap_of(&[boxed((0, 40), (60, 100))]), 0.0);
    // A corner outside the outline is not inside it: distance 0.
    assert_eq!(gap_of(&[boxed((-50, 40), (60, 100))]), 0.0);
    // It returns early at 0, whatever follows.
    assert_eq!(
        gap_of(&[boxed((-50, 40), (60, 100)), boxed((100, 100), (200, 200))]),
        0.0
    );
}

#[test]
fn a_round_pad_is_measured_from_its_bounding_box() {
    let circle = Shape::from(Circle::new(IntPoint::new(50, 250), 20));
    // The bounding box corners are (30, 230), (70, 230), (70, 270), (30, 270): 30 from the left edge.
    assert_eq!(gap_of(&[circle]), 30.0);
}

#[test]
fn empty_shapes_are_skipped() {
    let empty = Shape::from(TileShape::from(IntBox::EMPTY));
    assert!(empty.is_empty());
    assert_eq!(gap_of(std::slice::from_ref(&empty)), f64::INFINITY);
}

// ---- the comparison with C01.1's dumps ---------------------------------

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn member<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match value {
        JsonValue::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn dumps(dir: &Path, relative: &str, dsn_root: &Path, out: &mut Vec<(String, PathBuf, PathBuf)>) {
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
            dumps(&path, &sub, dsn_root, out);
        } else if let Some(stem) = name.strip_suffix(".json") {
            let case = if relative.is_empty() {
                stem.to_string()
            } else {
                format!("{relative}/{stem}")
            };
            out.push((case.clone(), path, dsn_root.join(format!("{case}.dsn"))));
        }
    }
}

#[test]
fn minimum_pin_gap_matches_java_on_every_board() {
    let root = std::env::var("SETTINGS_REFERENCE_DIR")
        .map_or_else(|_| repo("fixtures/reference"), PathBuf::from);
    let mut files = Vec::new();
    dumps(&root.join("settings"), "", &repo("fixtures"), &mut files);
    dumps(
        &root.join("cases/settings"),
        "",
        &repo("tools/java-reference/cases"),
        &mut files,
    );
    files.sort();
    let mut compared = 0;
    let mut finite = 0;
    let mut invalid = Vec::new();
    let mut failures = Vec::new();
    for (name, dump, dsn) in &files {
        let java = JsonValue::parse(&std::fs::read_to_string(dump).unwrap()).unwrap();
        let Some(scenarios) = member(&java, "scenarios") else {
            invalid.push(name.clone());
            continue;
        };
        // The gap does not depend on the scenario; the default one is enough.
        let Some(board_dump) = member(scenarios, "default").and_then(|s| member(s, "board")) else {
            continue; // Java could not load the board
        };
        let Some(JsonValue::String(expected)) = member(board_dump, "minimumPinGap") else {
            continue;
        };
        let bytes = std::fs::read(dsn).unwrap_or_else(|e| panic!("{}: {e}", dsn.display()));
        let board = match read_board(&bytes) {
            BoardReadResult::Success { board, .. } => *board,
            BoardReadResult::OutlineMissing {
                board: Some(board), ..
            } => *board,
            other => {
                failures.push(format!(
                    "{name}: the port cannot load it: {:?}",
                    other.board().is_some()
                ));
                continue;
            }
        };
        let gap = board.minimum_pin_gap();
        let text = java_double_to_string(gap);
        compared += 1;
        if gap.is_finite() {
            finite += 1;
        }
        if &text != expected {
            failures.push(format!("{name}: Java {expected} / Rust {text}"));
        }
    }
    eprintln!("minimumPinGap: {compared} boards compared, {finite} with a finite gap");
    assert!(invalid.is_empty(), "dumps without scenarios: {invalid:?}");
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

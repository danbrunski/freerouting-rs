//! The 90 degree and any-angle room graphs and locator (A07), against
//! `tools/java-reference/probes/autoroute/AutorouteProbe.java`.
//!
//! - `treemap_matches_java` (A07.3): [`JavaTreeSet`] makes the comparisons of
//!   `java.util.TreeMap.put`, so a comparator that is not transitive gives
//!   Java's contents and order. 200 sequences of 50 points from
//!   `new Random(13)` go into a set whose comparator treats distances within
//!   1.0 as equal (the shape of `SortedRoomNeighbour.compareTo`); the
//!   in-order sequence must equal Java's for every sequence, and a
//!   `BTreeSet` with the same comparator must differ on at least one (so the
//!   test can fail).
//! - `variants_match_java` (A07.6): the `start-rooms`, `drills`,
//!   `maze-log`, `locate` and `insert` probes, both ripup variants each, on
//!   the six boards of `tools/java-reference/probes/autoroute/variants.txt`
//!   (J2, `Issue143-rpi_splitter` and `scoring-mixed-layer` with
//!   `(snap_angle ninety_degree)` and `(snap_angle none)`), written by the
//!   same line writers as the 45 degree probes. `VARIANTS_KIND=<kind>` and
//!   `VARIANTS_BOARD=<board name>` run a subset (for debugging).

mod autoroute_common;
#[path = "autoroute_maze/mod.rs"]
mod autoroute_maze;
#[path = "support/drills_lines.rs"]
mod drills_lines;
#[path = "support/insert_lines.rs"]
mod insert_lines;
#[path = "support/locate_lines.rs"]
mod locate_lines;
#[path = "support/start_rooms_lines.rs"]
mod start_rooms_lines;

use std::cmp::Ordering;
use std::collections::BTreeSet;

use autoroute_common::{board_name, compare_probe, repo_root, writer};
use freerouting::autoroute::probe::ProbeValue;
use freerouting::autoroute::session::java_tree_set::JavaTreeSet;
use freerouting::geometry::jmath::JavaRandom;

const TREEMAP_SEQUENCES: usize = 200;
const TREEMAP_POINTS: usize = 50;
const TREEMAP_RANGE: i32 = 32;

/// A point of the treemap probe: its index and its distance from 0.
#[derive(Clone, Copy, Debug)]
struct TreePoint {
    index: usize,
    x: i32,
    y: i32,
    dist: f64,
}

/// `AutorouteProbe.treeCompare`: distances within 1.0 fall through to the
/// y difference, then the x difference. Not transitive.
fn tree_compare(a: &TreePoint, b: &TreePoint) -> Ordering {
    let mut delta = a.dist - b.dist;
    if delta.abs() <= 1.0 {
        delta = f64::from(a.y - b.y);
    }
    let res = if delta > 0.0 {
        Ordering::Greater
    } else if delta < 0.0 {
        Ordering::Less
    } else {
        Ordering::Equal
    };
    res.then(a.x.cmp(&b.x))
}

/// `tree_compare` as an `Ord`, for the `BTreeSet` the test must tell
/// apart.
#[derive(Clone, Copy, Debug)]
struct ByTreeCompare(TreePoint);

impl PartialEq for ByTreeCompare {
    fn eq(&self, other: &Self) -> bool {
        tree_compare(&self.0, &other.0) == Ordering::Equal
    }
}

impl Eq for ByTreeCompare {}

impl PartialOrd for ByTreeCompare {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ByTreeCompare {
    fn cmp(&self, other: &Self) -> Ordering {
        tree_compare(&self.0, &other.0)
    }
}

#[test]
fn treemap_matches_java() {
    let mut out = writer("treemap", "treemap");
    let mut random = JavaRandom::new(13);
    let mut btree_differs = 0;
    for seq in 0..TREEMAP_SEQUENCES {
        let mut set = JavaTreeSet::new(tree_compare);
        let mut btree = BTreeSet::new();
        let mut coords = Vec::new();
        let mut added = Vec::new();
        for index in 0..TREEMAP_POINTS {
            let x = random.next_int(TREEMAP_RANGE);
            let y = random.next_int(TREEMAP_RANGE);
            coords.extend([i64::from(x), i64::from(y)]);
            let dist = (f64::from(x) * f64::from(x) + f64::from(y) * f64::from(y)).sqrt();
            let point = TreePoint { index, x, y, dist };
            added.push(set.insert(point));
            btree.insert(ByTreeCompare(point));
        }
        let order: Vec<i64> = set.iter().map(|p| p.index as i64).collect();
        let btree_order: Vec<i64> = btree.iter().map(|p| p.0.index as i64).collect();
        if btree_order != order {
            btree_differs += 1;
        }
        out.record(&[
            ("stage", "treemap".into()),
            ("seq", (seq as i64).into()),
            ("points", ProbeValue::ints(coords)),
            ("added", ProbeValue::bools(added)),
            ("order", ProbeValue::ints(order)),
        ]);
    }
    if let Err(report) = compare_probe("treemap", "-", out.lines()) {
        panic!("{report}");
    }
    assert!(
        btree_differs > 0,
        "a BTreeSet with the same comparator gives Java's order on every sequence; \
         the test cannot tell the emulation from a plain sorted set"
    );
    eprintln!(
        "treemap: a BTreeSet differs from Java on {btree_differs} of {TREEMAP_SEQUENCES} sequences"
    );
}

/// The probe kinds compared on the variant boards, with their line writers.
/// The line writer of a probe kind.
type Lines = fn(&str) -> Vec<String>;

const KINDS: &[(&str, Lines)] = &[
    ("start-rooms", start_rooms_lines::start_rooms_lines),
    ("drills", drills_lines::drills_lines),
    ("maze-log", maze_plain_lines),
    ("locate", locate_lines::locate_lines),
    ("insert", insert_lines::insert_lines),
];

/// `(kind, board, reason)` of the comparisons that differ from Java. Empty:
/// every kind is required on every board.
const EXPECTED_MISMATCHES: &[(&str, &str, &str)] = &[];

fn maze_plain_lines(dsn: &str) -> Vec<String> {
    autoroute_maze::maze_lines(dsn, autoroute_maze::Mode::Plain)
}

/// The DSN paths of `variants.txt`.
fn variant_boards() -> Vec<String> {
    let path = repo_root().join("tools/java-reference/probes/autoroute/variants.txt");
    std::fs::read_to_string(&path)
        .expect("variants.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn variants_match_java() {
    let only_kind = std::env::var("VARIANTS_KIND").ok();
    let only_board = std::env::var("VARIANTS_BOARD").ok();
    let boards = variant_boards();
    assert_eq!(boards.len(), 6, "six variant boards");
    let mut failures = Vec::new();
    let mut compared = 0;
    for (kind, lines) in KINDS {
        if only_kind.as_ref().is_some_and(|k| k != kind) {
            continue;
        }
        for dsn in &boards {
            let board = board_name(dsn);
            if only_board
                .as_ref()
                .is_some_and(|b| !board.ends_with(b.as_str()))
            {
                continue;
            }
            compared += 1;
            let result = compare_probe(kind, dsn, &lines(dsn));
            let listed = EXPECTED_MISMATCHES
                .iter()
                .any(|(k, b, _)| k == kind && *b == board);
            match result {
                Err(report) if !listed => failures.push(report),
                Ok(()) if listed => failures.push(format!(
                    "{kind}/{board}: listed in EXPECTED_MISMATCHES but matches Java; remove it"
                )),
                _ => {}
            }
        }
    }
    eprintln!("variants: {compared} comparisons");
    assert!(compared > 0, "no comparison selected");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

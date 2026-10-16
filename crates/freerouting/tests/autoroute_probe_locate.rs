//! The `locate` probe (A05.5): the located path of every sampled
//! connection, line for line against
//! `tools/java-reference/probes/autoroute/AutorouteProbe.java`.
//!
//! Per sampled connection, ripup off and on, on one loaded board:
//! `MazeSearchEngine.getInstance`, `findConnection`, then
//! `FoundConnectionLocator.getInstance` with a `TreeSet` of ripped items and
//! a `LinkedHashMap` of costs, as `AutorouteEngine.autorouteConnection`
//! calls it. Printed: the backtrack array (door kind and id, section, next
//! room), the result items (corners as integer points, layer), the ripped
//! item ids in set order with their costs in map order, the start and
//! target items and layers, then the autoroute tree's walk hash after
//! `clear()`. A failed search prints one `{"result":"none"}` line.
//!
//! - `locate_matches_java`: the fixture ladder (to `Issue026-J2_reference`
//!   and the pre-routed boards after it), both variants.
//! - `locate_routed_matches_java`: `locate` through pre-routed traces, as
//!   `maze-routed` walks them (the board's traces and vias unfixed, nets cut
//!   one by one), so the ripped items and their costs of A05.1 are
//!   compared: the ladder's ripup variant enters no obstacle room.

mod autoroute_common;
#[path = "support/locate_lines.rs"]
mod locate_lines;

use autoroute_common::{board_name, compare_probe, kind_fixtures};

/// Boards of the ladder whose `locate` output differs from Java's, with
/// the reason. Empty: every board is required.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

/// Boards of the `locate-routed` row whose output differs from Java's,
/// with the reason. Empty: every board is required.
const ROUTED_EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

use locate_lines::*;

fn compare_kind(kind: &str, expected: &[(&str, &str)], lines: impl Fn(&str) -> Vec<String>) {
    let mut failures = Vec::new();
    let mut boards = 0;
    for dsn in kind_fixtures(kind) {
        let board = board_name(&dsn);
        let result = compare_probe(kind, &dsn, &lines(&dsn));
        boards += 1;
        let listed = expected.iter().any(|(b, _)| *b == board);
        match result {
            Err(report) if !listed => failures.push(report),
            Ok(()) if listed => failures.push(format!(
                "{board}: listed as an expected mismatch but matches Java; remove it"
            )),
            _ => {}
        }
    }
    assert!(boards > 0, "no {kind} fixture");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn locate_matches_java() {
    compare_kind("locate", EXPECTED_MISMATCHES, locate_lines);
}

#[test]
fn locate_routed_matches_java() {
    compare_kind("locate-routed", ROUTED_EXPECTED_MISMATCHES, routed_lines);
}

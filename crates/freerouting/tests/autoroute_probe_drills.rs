//! The `drills` probe (A03.5): the drill pages and drills of a search, line
//! for line against `tools/java-reference/probes/autoroute/AutorouteProbe.java`.
//!
//! `drills_match_java`: per sampled connection, ripup off, ripup on, and
//! ripup off with `attach_smd` forced on (variant `smd`: no fixture's via
//! rule allows attaching to SMD pins, so this is the only comparison of the
//! own-net SMD pin rules), on one loaded board, after `MazeSearchEngine.getInstance` (so the start rooms
//! exist, as in a real search): `DrillPage.getDrills` with `attach_smd =
//! ctrl.attach_smd_allowed` on every page of the page array in row-major
//! order, each page with its Java id, bounds and drills (Java id, location,
//! shape, layers, the room id per layer); then the room counter and the
//! autoroute tree's walk hash after `clear()`. The drills of earlier pages
//! complete rooms that later pages find, so the page order is part of the
//! comparison. Required on the boards of the `drills` row of `fixtures.tsv`:
//! `scoring-mixed-layer`, `Issue508-SMD-routing-issue-demo` (attach rules),
//! `Issue143-rpi_splitter`, `Issue143-rpi_splitter_mod`,
//! `Issue508-DAC2020_bm08` and `Issue026-J2_reference`.

mod autoroute_common;
#[path = "support/drills_lines.rs"]
mod drills_lines;

use autoroute_common::{board_name, compare_probe, kind_fixtures};

/// Boards of the `drills` row whose drills differ from Java's, with the
/// reason. Empty: every board is required.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

use drills_lines::*;

#[test]
fn drills_match_java() {
    let mut failures = Vec::new();
    for dsn in kind_fixtures("drills") {
        let board = board_name(&dsn);
        let result = compare_probe("drills", &dsn, &drills_lines(&dsn));
        let expected = EXPECTED_MISMATCHES.iter().any(|(b, _)| *b == board);
        match result {
            Err(report) if !expected => failures.push(report),
            Ok(()) if expected => failures.push(format!(
                "{board}: listed in EXPECTED_MISMATCHES but matches Java; remove it"
            )),
            _ => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

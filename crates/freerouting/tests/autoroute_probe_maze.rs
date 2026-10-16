//! The `maze-log` probe (A04.8): every step of the maze search, line for
//! line against `tools/java-reference/probes/autoroute/AutorouteProbe.java`.
//!
//! Per sampled connection, ripup off and on, on one loaded board:
//! `MazeSearchEngine.getInstance`, then `occupyNextElement` one call at a
//! time (at most 200,000 calls). Before each call the queue is walked from
//! its first element; the element the call will expand (the first whose
//! section is unoccupied) is printed with its door kind and Java id,
//! section, key bits, next room, backtrack door, flags, the occupied
//! elements popped before it, whether its section is occupied after the
//! call, and the work meter units since the search was created. At the
//! end: the destination, the room counter, the queue size, the units; then
//! the autoroute tree's walk hash after `clear()`. No work limit is passed,
//! so no cut-off fires; the units check the D5 counting step by step.
//!
//! `maze_log_matches_java` compares the whole fixture ladder (to
//! `Issue026-J2_reference` as A04.8 requires, and the pre-routed boards
//! `Issue413-test`, `Issue676-ch32v-tx118s` and `Issue957-smoke-ldo-led`
//! that A04.7 needs). The log is written twice per board: once by walking
//! the queue between steps of a search with `NoopSink` (as the Java probe
//! does), once from the hooks of `Tee<ProbeSink, WorkCounters>`; both must
//! be equal before either is compared with Java.

mod autoroute_common;
mod autoroute_maze;

use autoroute_common::{board_name, compare_probe, kind_fixtures};
use autoroute_maze::{Mode, maze_lines, routed_lines};

/// Boards of the ladder whose maze log differs from Java's, with the
/// reason. Empty: every board is required.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

/// Boards of the `maze-routed` row whose log differs from Java's, with the
/// reason. Empty: every board is required.
const ROUTED_EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

#[test]
fn maze_log_matches_java() {
    let mut failures = Vec::new();
    for dsn in kind_fixtures("maze-log") {
        let board = board_name(&dsn);
        let plain = maze_lines(&dsn, Mode::Plain);
        let hooked = maze_lines(&dsn, Mode::Hooked);
        if let Some(i) = plain.iter().zip(&hooked).position(|(a, b)| a != b) {
            failures.push(format!(
                "{board}: the hooked log differs from the plain one at line {}\n  plain:  {}\n  hooked: {}",
                i + 1,
                plain[i],
                hooked[i]
            ));
            continue;
        }
        assert_eq!(plain.len(), hooked.len(), "{board}: line counts");
        let result = compare_probe("maze-log", &dsn, &plain);
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

/// `maze-routed`: the search through pre-routed traces (ripup, A04.6, and
/// the shove checks of obstacle trace rooms, A04.7), on the boards of the
/// `maze-routed` row of `fixtures.tsv`.
#[test]
fn maze_routed_matches_java() {
    let mut failures = Vec::new();
    for dsn in kind_fixtures("maze-routed") {
        let board = board_name(&dsn);
        let plain = routed_lines(&dsn, Mode::Plain);
        let hooked = routed_lines(&dsn, Mode::Hooked);
        if plain != hooked {
            failures.push(format!(
                "{board}: the hooked log differs from the plain one"
            ));
            continue;
        }
        let result = compare_probe("maze-routed", &dsn, &plain);
        let expected = ROUTED_EXPECTED_MISMATCHES.iter().any(|(b, _)| *b == board);
        match result {
            Err(report) if !expected => failures.push(report),
            Ok(()) if expected => failures.push(format!(
                "{board}: listed in ROUTED_EXPECTED_MISMATCHES but matches Java; remove it"
            )),
            _ => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

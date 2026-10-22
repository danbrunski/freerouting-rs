//! K15.1: bug 1 of `docs/development/pcu-rev2-board-report.md`. C65's F.Fab
//! circle, at y = 105.242039 mm, places to an unbounded simplex; before K15.1
//! its box emptied the board's bounding box and every maze search failed
//! ("the maze search algorithm could not be created"). Now the outline is
//! left out of the board box with a warning naming it, and the boards route
//! in both profiles.
//!
//! The boards: the report's two repros, read from `docs/development/pcu-rev2/`;
//! `far-nm` (the repro plus four capacitors on nets 75 to 95 mm from C65,
//! `tools/kicad-reader/k15_far_fixture.py`); and KiCad 10.0.7's own DSN export
//! of the repro and of `far-nm`. The board files are in
//! `crates/freerouting/tests/data/k15/` (its README names the sources).

mod common;

use common::*;

const K15: &str = "crates/freerouting/tests/data/k15";
const REPORT: &str = "docs/development/pcu-rev2";
const WARNING: &str = "warning: The fabrication outline of component C65 at \
                       (57251.006, -105242.039) um has an unbounded shape after placement; \
                       it is left out of the board's bounding box.";

/// Routes `source` (a repository path) under `profile`; returns the log.
fn route(source: &str, profile: &str) -> String {
    let name = source.rsplit('/').next().unwrap();
    let dir = scratch(&format!("k15-{profile}-{name}"));
    std::fs::copy(repo(source), dir.join(name)).unwrap();
    let output = if name.ends_with(".dsn") {
        "out.ses"
    } else {
        "out.kicad_pcb"
    };
    let out = run_in(&dir, &["-de", name, "-do", output, "--profile", profile]);
    let log = stderr(&out) + &stdout(&out);
    assert_eq!(code(&out), 0, "{log}");
    log
}

/// Checks that the log routed all `nets` nets, with no failed maze search.
fn assert_routed(log: &str, nets: usize) {
    assert_eq!(log.matches("could not be created").count(), 0, "{log}");
    let summary = log
        .lines()
        .find(|l| l.starts_with("Auto-routing stage completed"))
        .unwrap_or_else(|| panic!("no stage summary: {log}"));
    assert!(
        summary.contains(&format!("started with {nets} unrouted nets")),
        "{summary}"
    );
    assert!(
        summary.contains("(0 unrouted and 0 violations)"),
        "{summary}"
    );
}

#[test]
fn the_c65_repro_routes_in_both_profiles() {
    for profile in ["legacy", "enhanced"] {
        let log = route(&format!("{REPORT}/c65-nm-position.kicad_pcb"), profile);
        assert_routed(&log, 2);
        assert_eq!(log.matches(WARNING).count(), 1, "{profile}: {log}");
    }
}

#[test]
fn the_snapped_repro_routes_as_before() {
    for profile in ["legacy", "enhanced"] {
        let log = route(&format!("{REPORT}/c65-snapped.kicad_pcb"), profile);
        assert_routed(&log, 2);
        assert!(!log.contains("unbounded shape"), "{profile}: {log}");
    }
}

#[test]
fn nets_far_from_c65_route() {
    for profile in ["legacy", "enhanced"] {
        let log = route(&format!("{K15}/far-nm.kicad_pcb"), profile);
        assert_routed(&log, 6);
        assert_eq!(log.matches(WARNING).count(), 1, "{profile}: {log}");
    }
}

#[test]
fn the_kicad_dsn_exports_route_the_same_way() {
    for profile in ["legacy", "enhanced"] {
        for (name, nets) in [
            ("c65-nm-position.kicad10.dsn", 2),
            ("far-nm.kicad10.dsn", 6),
        ] {
            let log = route(&format!("{K15}/{name}"), profile);
            assert_routed(&log, nets);
            assert_eq!(log.matches(WARNING).count(), 1, "{profile} {name}: {log}");
        }
        let log = route(&format!("{K15}/c65-snapped.kicad10.dsn"), profile);
        assert_routed(&log, 2);
        assert!(!log.contains("unbounded shape"), "{profile}: {log}");
    }
}

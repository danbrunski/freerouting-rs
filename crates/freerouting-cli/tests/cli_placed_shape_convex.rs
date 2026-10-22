//! K15.2: `--router.java_quirks.placed_shape_convex=false` on bug 1's repro
//! (`docs/development/pcu-rev2-board-report.md`). C65's F.Fab circle, at
//! y = 105.242039 mm, places to the convex hull of its rounded corners, so no
//! item's box is unbounded: the board routes without K15.1's warning. Unset, the
//! outline is Java's unbounded simplex and K15.1 leaves it out of the board box
//! with the warning (`cli_unbounded_outline.rs`, unchanged).

mod common;

use common::*;

const K15: &str = "crates/freerouting/tests/data/k15";
const REPORT: &str = "docs/development/pcu-rev2";
const SWITCH: &str = "--router.java_quirks.placed_shape_convex=false";

/// Routes `source` (a repository path) under `profile` with the switch `false`;
/// returns the log.
fn route_convex(source: &str, profile: &str) -> String {
    let name = source.rsplit('/').next().unwrap();
    let dir = scratch(&format!("k152-{profile}-{name}"));
    std::fs::copy(repo(source), dir.join(name)).unwrap();
    let output = if name.ends_with(".dsn") {
        "out.ses"
    } else {
        "out.kicad_pcb"
    };
    let out = run_in(
        &dir,
        &["-de", name, "-do", output, "--profile", profile, SWITCH],
    );
    let log = stderr(&out) + &stdout(&out);
    assert_eq!(code(&out), 0, "{log}");
    log
}

/// Checks that the log routed all `nets` nets, with no failed maze search and
/// no unbounded item.
fn assert_routed_without_warning(log: &str, nets: usize) {
    assert_eq!(log.matches("could not be created").count(), 0, "{log}");
    assert!(!log.contains("unbounded shape"), "{log}");
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
fn the_c65_repro_routes_without_the_warning_with_the_switch_off() {
    for profile in ["legacy", "enhanced"] {
        let log = route_convex(&format!("{REPORT}/c65-nm-position.kicad_pcb"), profile);
        assert_routed_without_warning(&log, 2);
    }
}

#[test]
fn the_kicad_dsn_exports_route_without_the_warning_with_the_switch_off() {
    for profile in ["legacy", "enhanced"] {
        for (name, nets) in [
            ("c65-nm-position.kicad10.dsn", 2),
            ("far-nm.kicad10.dsn", 6),
            ("c65-snapped.kicad10.dsn", 2),
        ] {
            let log = route_convex(&format!("{K15}/{name}"), profile);
            assert_routed_without_warning(&log, nets);
        }
    }
}

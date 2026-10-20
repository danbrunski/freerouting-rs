//! N06.4 (batch G) and N06.14: the cutouts' reachability counters reach the run
//! statistics.
//!
//! Its own test binary, because the run statistics are process-wide: no other test
//! resets them under it, and the one test that reads them runs its boards one after
//! the other. One autoroute pass of a small board computes drill pages, whose convex
//! split (`autoroute/session/drills.rs`) is where `octagon_box_cutout_branch3` and
//! `simplex_prev_division_line` are reached in a batch run.
//!
//! N06.14's two switches live in `Simplex::cutout_from`, which a drill page reaches only
//! with a simplex tree shape in it. In any-angle mode (the plain tree) every obstacle keeps
//! its simplex, so a pass reaches them hundreds of times. The 90 degree tree keeps boxes
//! only and never does. The 45 degree tree keeps octagons, except for traces with a line
//! off 45 degrees, whose tree shapes stay simplexes: a 45 degree board with such wiring
//! reaches both switches (canary-pwr, `tests/n06_cutout_reach_45.rs`). The 45 degree
//! board below, which has no wiring, reaches neither.
mod autoroute_common;

use autoroute_common::{Loaded, load};
use freerouting::autoroute::batch::connection::FreeroutingConnectionRouter;
use freerouting::autoroute::batch::ordering::FreeroutingOrdering;
use freerouting::autoroute::batch::pass::{PassRunner, PassStages};
use freerouting::autoroute::batch::ripup_policy::FreeroutingRipupPolicy;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::stages::StageCtx;
use freerouting::run_stats::{self, Counter, Counts};
use freerouting::settings::JavaQuirks;

const MIN_FR_TEST: &str = "fixtures/Issue269-min_fr_test/min_fr_test.dsn";

/// A07's any-angle variant of `Issue143-rpi_splitter` (`(snap_angle none)`).
const RPI_SPLITTER_ANY: &str =
    "crates/freerouting/tests/data/autoroute-variants/Issue143-rpi_splitter-any.dsn";

/// Routes pass 1 of `loaded` with `quirks` on its board and its control; returns the
/// counts of the pass.
fn pass_one_counts(mut loaded: Loaded, quirks: JavaQuirks) -> Counts {
    loaded.board.set_java_quirks(quirks);
    loaded.router = loaded.router.with_java_quirks(quirks);
    let mut runner = PassRunner::for_job(&loaded.settings, &loaded.router);
    let mut ordering = FreeroutingOrdering::new();
    let mut router = FreeroutingConnectionRouter::freerouting();
    let mut ripup = FreeroutingRipupPolicy::from_settings(&loaded.settings);
    let cancel = CancelToken::new();
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &events,
    };
    let mut stages = PassStages {
        ordering: &mut ordering,
        router: &mut router,
        ripup: &mut ripup,
    };
    run_stats::reset();
    let before = Counts::now();
    let outcome = runner.run_pass(&mut loaded.board, 1, &mut stages, &mut ctx);
    assert!(outcome.routed > 0, "{outcome:?}");
    Counts::now().since(&before)
}

#[test]
fn drill_page_splits_report_both_counters_in_both_arms() {
    run_stats::enable();
    let arms = [
        JavaQuirks::JAVA,
        JavaQuirks::JAVA
            .with("simplex_prev_division_line", false)
            .and_then(|q| q.with("octagon_box_cutout_branch3", false))
            .unwrap(),
        JavaQuirks::JAVA
            .with("cutout_hole_crosses_border", false)
            .and_then(|q| q.with("simplex_octagon_half_corner", false))
            .unwrap(),
    ];
    let mut branch3 = Vec::new();
    let mut n06_14 = Vec::new();
    for quirks in arms {
        let counts = pass_one_counts(load(MIN_FR_TEST), quirks);
        assert!(
            counts.get(Counter::DrillsComputed) > 0,
            "the pass reads drill pages"
        );
        // The simplex fix is inert: no piece of any arm passes its merge test.
        assert_eq!(counts.get(Counter::SimplexPrevDivisionLine), 0);
        branch3.push(counts.get(Counter::OctagonBoxCutoutBranch3));
        n06_14.push((
            counts.get(Counter::CutoutHoleCrossesBorder),
            counts.get(Counter::SimplexOctagonHalfCorner),
        ));
    }
    println!(
        "cutout_hole_crosses_border and simplex_octagon_half_corner reached, Java, N06.4 \
         fixed, N06.14 fixed: {n06_14:?}"
    );
    // A 45 degree board without wiring: no simplex tree shape, so no simplex cutout.
    assert!(n06_14.iter().all(|&c| c == (0, 0)), "{n06_14:?}");
    // An any-angle board: its plain tree keeps the simplex obstacles.
    let mut any_angle = Vec::new();
    for quirks in [arms[0], arms[2]] {
        let counts = pass_one_counts(load(RPI_SPLITTER_ANY), quirks);
        any_angle.push((
            counts.get(Counter::CutoutHoleCrossesBorder),
            counts.get(Counter::SimplexOctagonHalfCorner),
        ));
    }
    run_stats::disable();
    println!("{RPI_SPLITTER_ANY}, Java and N06.14 fixed: {any_angle:?}");
    assert!(
        any_angle[0].0 > 0 && any_angle[0].1 > 0,
        "Java reaches both: {any_angle:?}"
    );
    assert!(
        any_angle[1].1 > 0,
        "the half-corner count is taken in the fixed arm too: {any_angle:?}"
    );
    assert_eq!(
        Counter::CutoutHoleCrossesBorder.name(),
        "cutout_hole_crosses_border_reached"
    );
    assert_eq!(
        Counter::SimplexOctagonHalfCorner.name(),
        "simplex_octagon_half_corner_reached"
    );
    println!("octagon_box_cutout_branch3_reached, java and fixed: {branch3:?}");
    assert!(
        branch3[0] > 0,
        "an octagon obstacle in a drill page fires branch 3 under Java: {branch3:?}"
    );
    assert!(
        branch3[1] > 0 && branch3[2] > 0,
        "the count is taken in the fixed arms too: {branch3:?}"
    );
    assert_eq!(
        Counter::OctagonBoxCutoutBranch3.name(),
        "octagon_box_cutout_branch3_reached"
    );
    assert_eq!(
        Counter::SimplexPrevDivisionLine.name(),
        "simplex_prev_division_line_reached"
    );
}

/// The splits of the board's stored areas: the search trees' shapes of obstacle and
/// conduction areas (`board/search/mod.rs`) and of the board outline's generated
/// keepout. Until the N06 follow-up of 2026-10-18 they kept Java's code under every
/// switch value (hence the name, which the registrations' commands use); since then
/// they follow the board's switches like the drill pages. Counts, per corpus board, how
/// often those splits reach the four switches' branches (N06.4's two and N06.14's two),
/// with Java's code; a board where they do is one where the switches change these
/// splits. Read-only (loads every fixture, never routes); run by the orchestrator with
/// an absolute path in `N06_CUTOUT_COUNTERS_OUT`. `N06_CUTOUT_DSNS`, a list of
/// absolute DSN paths separated by `:`, replaces the fixture walk (a plugin-exported
/// board, say).
#[test]
#[ignore = "N06.4 corpus reachability report; set N06_CUTOUT_COUNTERS_OUT to an absolute path"]
fn unwired_area_splits_corpus_inventory() {
    use freerouting::drc::json::JsonValue as J;
    use freerouting_geometry::{Area, PolylineArea, SplitQuirks, SplitReach};
    use std::path::{Path, PathBuf};
    fn collect(dir: &Path, paths: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, paths);
            } else if path.extension().is_some_and(|s| s == "dsn") {
                paths.push(path);
            }
        }
    }
    /// A fresh copy, so that no cached split hides the cutouts.
    fn reach_of(area: &Area, reach: &mut SplitReach, splits: &mut i64) {
        if let Area::PolylineArea(a) = area {
            if a.get_holes().is_empty() {
                return;
            }
            *splits += 1;
            let fresh = PolylineArea::new(a.get_border().clone(), a.get_holes().to_vec());
            let _ = fresh.split_to_convex_counted(None, SplitQuirks::JAVA, reach);
        }
    }
    let output = std::env::var("N06_CUTOUT_COUNTERS_OUT").expect("report path required");
    assert!(
        Path::new(&output).is_absolute(),
        "cargo runs tests in the crate dir"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    match std::env::var("N06_CUTOUT_DSNS") {
        Ok(list) => files.extend(list.split(':').map(PathBuf::from)),
        Err(_) => collect(&root.join("fixtures"), &mut files),
    }
    files.sort();
    let mut rows = Vec::new();
    for path in files {
        let board = freerouting::drc::load_design(&std::fs::read(&path).unwrap());
        let name = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_str()
            .unwrap()
            .to_string();
        let mut row = vec![
            ("board".into(), J::string(&name)),
            ("loaded".into(), J::Bool(board.is_ok())),
        ];
        if let Ok(board) = board {
            for (site, outline) in [("area", false), ("outline_keepout", true)] {
                let (mut reach, mut splits) = (SplitReach::default(), 0);
                for item in board.items() {
                    if outline {
                        if let Some(keepout) =
                            item.as_board_outline().and_then(|o| o.keepout_area())
                        {
                            reach_of(keepout, &mut reach, &mut splits);
                        }
                    } else if let Some(area) = item.as_area() {
                        reach_of(area.area(), &mut reach, &mut splits);
                    }
                }
                row.push((format!("{site}_splits_with_holes"), J::int(splits)));
                row.push((
                    format!("{site}_simplex_prev_division_line_reached"),
                    J::int(reach.simplex_prev_division_line as i64),
                ));
                row.push((
                    format!("{site}_octagon_box_cutout_branch3_reached"),
                    J::int(reach.octagon_box_cutout_branch3 as i64),
                ));
                row.push((
                    format!("{site}_cutout_hole_crosses_border_reached"),
                    J::int(reach.cutout_hole_crosses_border as i64),
                ));
                row.push((
                    format!("{site}_simplex_octagon_half_corner_reached"),
                    J::int(reach.simplex_octagon_half_corner as i64),
                ));
            }
        }
        rows.push(J::Object(row));
    }
    assert!(!rows.is_empty());
    std::fs::write(output, J::Array(rows).to_pretty_string()).unwrap();
}

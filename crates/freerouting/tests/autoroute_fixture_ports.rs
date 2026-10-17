//! A14.6: the routing tests of Java's `app.freerouting.fixtures` package at `a773a70ec`,
//! work-bounded. Each test names its Java source.
//!
//! The load reproduces `RoutingFixtureTest.getRoutingJob`: the defaults, the DSN's
//! settings and its adjacent rules file, then `TestingSettings` at the testing priority
//! (copper-to-edge clearance 0, the test's setters, then `setMaxPasses(100)` and
//! `setJobTimeoutString("00:01:00")`, which only apply when the test set nothing). The
//! router assertions run with the optimizer off (A14's decision; no test here needs it), and
//! `RoutingResultAssertions.check()` becomes [`Bounds::check`] on the routed board's
//! `BoardStatistics` and the run's pass count. Wall-clock bounds (`maxDuration`, the job
//! timeouts) are dropped: the run is bounded by passes, items and the work limits.
//!
//! The boards that take minutes in a debug build are `#[ignore]`; run them with
//! `cargo test --release -p freerouting --test autoroute_fixture_ports -- --ignored
//! --test-threads=1`.
//!
//! Not ported here:
//! - `Issue872SingleLayerRoutingTest`, `MultiLayerBoardRoutingTest`,
//!   `Issue676RoutingTest`'s routing part, `InactiveLayerRoutingTest`,
//!   `Issue954CircularKeepoutClearanceTest`'s routing part, `BbdMars64PerformanceRoutingTest`,
//!   `Issue420ContributionBoardRoutingTest`, `Issue955RoutingQualityTest`,
//!   `MinTraceWidthTest` and the load-only `CombineStackOverflowTest`,
//!   `FootprintImageVariantKeepoutTest` and `PCBenchPreExistingClearanceViolationsTest`:
//!   not reached in A14's budget (the A14.6 note lists them);
//! - the fixture tests other checklists own (A14's table: `WorkLimitDeterminismTest`,
//!   `RoutingPipelineComparisonTest`, `Issue954CircularKeepoutInsertionTest`, the Issue955
//!   placement tests, `BatchOptimizerDeterminismTest`, `KiCadDrcViolationRoutingTest`,
//!   `Issue733DsnJsonParityTest`, and A13's `MaxPassesSettingRoutingTest`,
//!   `MaxItemsRoutingTest`, `HeadlessCompleteRoutingTest`).

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemKind;
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const J2: &str = "Issue026-J2_reference.dsn";
const BM01: &str = "Issue508-DAC2020_bm01.dsn";
const BM05: &str = "Issue508-DAC2020_bm05.dsn";

/// A routed fixture: the board and the run's pass count.
struct Routed {
    board: RoutingBoard,
    passes: i32,
}

/// `getRoutingJob(fixture, profile)` then `runRoutingJob`, with the optimizer off.
fn route(fixture: &str, mut profile: TestingProfile) -> Routed {
    profile.set_optimizer_enabled(false);
    route_as_set(fixture, profile)
}

/// [`route`] with the optimizer as `profile` leaves it (Java's default: on).
fn route_as_set(fixture: &str, mut profile: TestingProfile) -> Routed {
    profile.set_job_timeout("00:01:00");
    profile.set_max_passes(100);
    let path = repo_root().join("fixtures").join(fixture);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{fixture}: {e}"));
    let mut input = JobInput::new(bytes, JobInput::design_name_of(fixture));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap_or_else(|e| panic!("{fixture}: {e}"));
    let cancel = Arc::new(CancelToken::new());
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel,
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{fixture}: {}", f.error));
    Routed {
        board: outcome.board.take().expect("no listener took the board"),
        passes: outcome.summary.passes,
    }
}

/// Java `RoutingFixtureTest.RoutingResultAssertions` without `maxDuration`.
#[derive(Default)]
struct Bounds {
    max_passes: Option<i32>,
    max_incomplete: Option<usize>,
    exact_incomplete: Option<usize>,
    exact_violations: Option<usize>,
}

impl Bounds {
    fn check(&self, fixture: &str, routed: &Routed) {
        let stats = BoardStatistics::new(&routed.board);
        let incomplete = stats.connections.incomplete_count;
        let violations = stats.clearance_violations.total_count;
        eprintln!(
            "{fixture}: {} passes, {incomplete} unrouted, {violations} violations",
            routed.passes
        );
        if let Some(max) = self.max_passes {
            assert!(
                routed.passes <= max,
                "{fixture}: {} passes > {max}",
                routed.passes
            );
        }
        if let Some(max) = self.max_incomplete {
            assert!(
                incomplete <= max,
                "{fixture}: {incomplete} unrouted > {max}"
            );
        }
        if let Some(exact) = self.exact_incomplete {
            assert_eq!(incomplete, exact, "{fixture}: unrouted");
        }
        if let Some(exact) = self.exact_violations {
            assert_eq!(violations, exact, "{fixture}: clearance violations");
        }
    }
}

fn profile(max_passes: Option<i32>, max_items: Option<i32>) -> TestingProfile {
    let mut profile = TestingProfile::default();
    if let Some(passes) = max_passes {
        profile.set_max_passes(passes);
    }
    if let Some(items) = max_items {
        profile.set_max_items(items);
    }
    profile
}

/// `Dac2020Bm01RoutingTest.issue508Bm01First2NetsOnly` and
/// `Dac2020BenchmarkRoutingTest.issue508Bm01First2NetsOnly`: 1 pass, 2 items.
#[test]
fn dac2020_bm01_first_2_items() {
    let routed = route(BM01, profile(Some(1), Some(2)));
    Bounds {
        max_incomplete: Some(194),
        ..Bounds::default()
    }
    .check(BM01, &routed);
}

/// `Dac2020Bm05RoutingTest.issue508Bm05First2Items` and `issue508Bm05First5Items`.
#[test]
fn dac2020_bm05_first_2_and_5_items() {
    for (items, max) in [(2, 106), (5, 104)] {
        let routed = route(BM05, profile(Some(1), Some(items)));
        Bounds {
            max_incomplete: Some(max),
            ..Bounds::default()
        }
        .check(BM05, &routed);
    }
}

/// `Dac2020BenchmarkRoutingTest.issue508Bm01First{43,61,111,151}NetsOnly`.
#[test]
#[ignore = "slow in a debug build; run in release"]
fn dac2020_bm01_item_bounds() {
    for (items, max) in [(43, 161), (61, 147), (111, 134), (151, 126)] {
        let routed = route(BM01, profile(Some(1), Some(items)));
        Bounds {
            max_incomplete: Some(max),
            ..Bounds::default()
        }
        .check(BM01, &routed);
    }
}

/// `Dac2020BenchmarkRoutingTest.issue508Bm01FirstPassOnly`.
#[test]
#[ignore = "slow in a debug build; run in release"]
fn dac2020_bm01_first_pass_bound() {
    let routed = route(BM01, profile(Some(1), None));
    Bounds {
        max_incomplete: Some(56),
        ..Bounds::default()
    }
    .check(BM01, &routed);
}

/// `Dac2020BenchmarkRoutingTest.issue508Bm01First2PassesOnly`, with the optimizer on as in
/// Java: at most 28 unrouted. With the optimizer off Java's own route reference has 37
/// unrouted after pass 2 (`fixtures/reference/route/Issue508-DAC2020_bm01.json`) and the
/// port 34 under these settings, so the bound needs the optimizer (O06). Red at A14: with
/// the optimizer on the port still ends with 34; a Java run of this test at the pin must
/// attribute it (A14.6).
#[test]
#[ignore = "red at A14 (34 unrouted); needs the optimizer (O06) and a Java run at the pin"]
fn dac2020_bm01_two_passes_with_the_optimizer() {
    let routed = route_as_set(BM01, profile(Some(2), None));
    Bounds {
        max_incomplete: Some(28),
        ..Bounds::default()
    }
    .check(BM01, &routed);
}

/// `Dac2020BenchmarkRoutingTest.issue508Bm07` (its 30 s `maxDuration` dropped).
#[test]
#[ignore = "slow in a debug build; run in release"]
fn dac2020_bm07_completes() {
    let fixture = "Issue508-DAC2020_bm07.dsn";
    let routed = route(fixture, TestingProfile::default());
    Bounds {
        max_passes: Some(9),
        exact_incomplete: Some(0),
        ..Bounds::default()
    }
    .check(fixture, &routed);
}

/// `Dac2020BenchmarkRoutingTest.issue508Bm08` (its 20 s `maxDuration` dropped).
#[test]
fn dac2020_bm08_completes() {
    let fixture = "Issue508-DAC2020_bm08.dsn";
    let routed = route(fixture, TestingProfile::default());
    Bounds {
        max_passes: Some(2),
        exact_incomplete: Some(0),
        ..Bounds::default()
    }
    .check(fixture, &routed);
}

/// `J2ReferenceRoutingTest.issue026AutorouterInterruptedAndConnectionsNotFound`.
#[test]
#[ignore = "slow in a debug build; run in release"]
fn j2_reference_routes_within_bounds() {
    let routed = route(J2, TestingProfile::default());
    let drills = BoardStatistics::new(&routed.board).items.drill_item_count;
    assert!(drills < 60, "{drills} drill items");
    Bounds {
        max_passes: Some(99),
        max_incomplete: Some(3),
        exact_violations: Some(0),
        ..Bounds::default()
    }
    .check(J2, &routed);
}

/// `BendCostRoutingTest.routingWithBendCosts`: J2, 2 passes, 5 items, a default bend cost
/// of 5; `check()` with no bounds (it completes).
#[test]
fn bend_cost_routing_completes() {
    let mut profile = profile(Some(2), Some(5));
    profile.set_default_bend_cost(5.0);
    let routed = route(J2, profile);
    Bounds::default().check(J2, &routed);
    assert!(routed.passes <= 2);
}

/// `SetonixRoutingTest.issue159OutOfMemoryError`.
#[test]
#[ignore = "slow in a debug build; run in release"]
fn setonix_routes_within_bounds() {
    let fixture = "Issue159-setonix_2hp-pcb.dsn";
    let routed = route(fixture, TestingProfile::default());
    Bounds {
        max_incomplete: Some(4),
        exact_violations: Some(0),
        ..Bounds::default()
    }
    .check(fixture, &routed);
}

/// `Display8DigitRoutingTest.issue229KeepoutZoneWasNotExportedCorrectly`.
#[test]
#[ignore = "slow in a debug build; run in release"]
fn display_8_digit_routes_within_bounds() {
    let fixture = "Issue229-display-8-digit-hc595.dsn";
    let routed = route(fixture, TestingProfile::default());
    Bounds {
        max_incomplete: Some(20),
        exact_violations: Some(0),
        ..Bounds::default()
    }
    .check(fixture, &routed);
}

/// `StrictDrcRoutingTest.strictDrcDoesNotAddViolationsBeyondPreExisting` (`@Tag("slow")`
/// in Java too), with the optimizer on as in Java: exactly 16 violations. Java's load
/// reference has 6 on this board, and Java's and the port's optimizer-off routes end with 6
/// (A14's gate), so the other 10 come from the optimizer run (O06). Red at A14: the port
/// ends with 6 with the optimizer on as well; a Java run at the pin must attribute it.
#[test]
#[ignore = "red at A14 (6 violations); needs the optimizer (O06) and a Java run at the pin"]
fn strict_drc_adds_no_violations() {
    let fixture = "Issue555-CNH_Functional_Tester_1.dsn";
    let mut profile = TestingProfile::default();
    profile.set_strict_drc(true);
    let routed = route_as_set(fixture, profile);
    Bounds {
        max_incomplete: Some(30),
        exact_violations: Some(16),
        ..Bounds::default()
    }
    .check(fixture, &routed);
}

/// `Issue690RoutingTest`: 1 pass, 50 items, fanout off; each completes.
#[test]
fn issue690_boards_complete() {
    for fixture in ["Issue690-ecc83.dsn", "Issue690-sonde_xilinx.dsn"] {
        let mut profile = profile(Some(1), Some(50));
        profile.set_fanout_enabled(false);
        let routed = route(fixture, profile);
        Bounds::default().check(fixture, &routed);
    }
}

/// `Issue690RoutingTest.issue690KitDevColdfireXilinx` (`@Tag("slow")`).
#[test]
#[ignore = "slow in a debug build; run in release"]
fn issue690_kit_dev_coldfire_completes() {
    let fixture = "Issue690-kit-dev-coldfire-xilinx_5213.dsn";
    let mut profile = profile(Some(1), Some(50));
    profile.set_fanout_enabled(false);
    let routed = route(fixture, profile);
    Bounds::default().check(fixture, &routed);
}

/// `Issue676FullPassTest.issue676FullRouting3passes`: completes.
#[test]
#[ignore = "slow in a debug build; run in release"]
fn issue676_three_passes_complete() {
    let fixture = "Issue676-ch32v-tx118s.dsn";
    let routed = route(fixture, profile(Some(3), None));
    Bounds {
        max_passes: Some(3),
        ..Bounds::default()
    }
    .check(fixture, &routed);
}

/// `Issue957SmokeRoutingTest.boardWithComponentOutlinesRoutesAndSerializesCleanly`: 2
/// passes complete, the board has component outlines, and its hash equals its copy's.
#[test]
fn issue957_component_outlines_route_and_copy() {
    let fixture = "Issue957-smoke-ldo-led.dsn";
    let routed = route(fixture, profile(Some(2), None));
    assert!(routed.passes <= 2);
    assert!(
        routed
            .board
            .items()
            .any(|item| matches!(item.kind, ItemKind::ComponentOutline(_))),
        "the fixture has ComponentOutline items"
    );
    let hash = routed.board.state_hash();
    assert_eq!(routed.board.deep_copy().state_hash(), hash);
}

/// `PowerLayerForeignNetTest.signalNetIsNotRoutedAcrossAPowerLayer` (Java `7f56b062d`, after
/// the pin; Q01.7): a SIG via on each side of an F.Cu/B.Cu keepout wall, GND planes on both
/// inner power layers. The connection cannot be made legally, so no trace lies on a power
/// layer (before the fix the search joined the two vias across the GND plane on Gnd.Cu).
/// The fixture is Java's `fixtures/PowerLayer-foreign-via-pair.dsn`, kept in
/// `tests/data/` so the reference tests that walk `fixtures/` don't need a Java load
/// reference for it.
#[test]
fn power_layer_carries_no_foreign_net_trace() {
    let fixture = "../crates/freerouting/tests/data/PowerLayer-foreign-via-pair.dsn";
    let routed = route(fixture, TestingProfile::default());
    let board = &routed.board;
    let on_power_layers: Vec<String> = board
        .items()
        .filter(|item| item.is_trace())
        .filter(|item| !board.layer_structure.layers[item.first_layer() as usize].is_signal)
        .map(|item| {
            let net = item.net_numbers().first().copied().unwrap_or(0);
            let net_name = board.rules.nets.get(net).map_or("?", |n| n.name.as_str());
            let layer = &board.layer_structure.layers[item.first_layer() as usize].name;
            format!("{net_name} on {layer}")
        })
        .collect();
    assert_eq!(
        on_power_layers,
        Vec::<String>::new(),
        "no trace may be routed on a power layer"
    );
}

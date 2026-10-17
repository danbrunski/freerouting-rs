//! The fanout stage (A12): `fanout_pin` (one pin) and `FreeroutingFanout`
//! (the passes).
//!
//! `fanout_matches_java` (A12.6) is rung 2 of the parity ladder for the
//! fanout: one fanout pass on every board of the `fanout` row of
//! `tools/java-reference/probes/autoroute/fixtures.tsv` gives the same lines
//! as `AutorouteProbe.java` kind `fanout`, pin by pin, work units included
//! (see `tests/support/fanout_lines.rs`); `fanout_cutoff_matches_java` does
//! the same with a 1 ms per-pin budget, where the work limit fires.
//!
//! Java tests ported here (they belong in `tests/java_ports.rs`, which
//! carries another session's uncommitted hunks): `fixtures.PlaneFanoutViaTest`
//! (with the fanout), `fixtures.SmdPinFanoutRoutingTest`,
//! `fixtures.Issue952FanoutIgnoredNetClassTest` cases 1-4,
//! `fixtures.Dac2020Bm11FanoutTraceTest` and
//! `fixtures.WorkLimitDeterminismTest`. A job here is A13's `run_pipeline`
//! (the fanout stage, the batch loop with its restores, the optimizer when
//! on). `Issue952FanoutIgnoredNetClassTest` cases 5 and 6 (the
//! optimizer's candidate filter) are in `tests/batch_optimizer.rs` (O06).
//! Not ported: `Dac2020Bm11FanoutTraceTest.dac2020Bm11FanoutEscapeRate` (the same run and
//! bound as `dac2020Bm11FanoutTrace`, plus a print).

mod autoroute_common;
#[path = "support/fanout_lines.rs"]
mod fanout_lines;
#[path = "support/insert_lines.rs"]
mod insert_lines;

use std::collections::BTreeSet;
use std::path::Path;

use autoroute_common::{Loaded, repo_root};
use freerouting::autoroute::batch::fanout::FreeroutingFanout;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{FanoutStage, FanoutSummary, StageCtx};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const MIN_FR_TEST: &str = "fixtures/Issue269-min_fr_test/min_fr_test.dsn";
const DEV_BOARD: &str = "fixtures/Issue558-dev-board.dsn";

// ---------------------------------------------------------------------------
// The harness: Java's RoutingFixtureTest with TestingSettings
// ---------------------------------------------------------------------------

/// Java `RoutingFixtureTest.getRoutingJob`: the defaults, the DSN's settings
/// and `profile` as `TestingSettings` (with its `setMaxPasses(100)` and
/// `setJobTimeoutString("00:01:00")`, first write wins), through the job's
/// load sequence.
fn load_testing(dsn: &str, mut profile: TestingProfile) -> Loaded {
    profile.set_job_timeout("00:01:00");
    profile.set_max_passes(100);
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("board");
    let mut input = JobInput::new(bytes, JobInput::design_name_of(file_name));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    let prepared =
        prepare_job(&input, &ResolveContext { cores: 8 }).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    Loaded {
        board: prepared.board,
        settings: prepared.settings,
        router: prepared.board_settings,
    }
}

/// What the job did.
#[derive(Debug, Default)]
struct JobRun {
    fanout: Option<FanoutSummary>,
    /// The job's current pass (Java `job.getCurrentPass()`).
    passes: u32,
}

/// The job: A13's `run_pipeline` on the loaded board (the fanout stage when
/// it is on, then the batch loop while the autorouter is on, then the
/// optimizer when it is on), which hands the board back.
fn run_job(loaded: &mut Loaded) -> JobRun {
    let cancel = std::sync::Arc::new(CancelToken::new());
    let job = PipelineJob {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel,
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(loaded.board.clone(), &job).unwrap_or_else(|f| panic!("{}", f.error));
    loaded.board = outcome.board.take().expect("no listener took the board");
    JobRun {
        fanout: outcome.fanout,
        passes: u32::try_from(outcome.summary.passes).unwrap_or(0),
    }
}

fn net_number(board: &RoutingBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .expect("the net")
        .net_number
}

fn smd_pins_of(board: &RoutingBoard, net: i32) -> Vec<ItemId> {
    board
        .items()
        .filter(|i| i.is_pin() && i.contains_net(net) && i.first_layer() == i.last_layer())
        .map(|i| i.id())
        .collect()
}

/// Items reached from `start` through normal contacts, not through a
/// conduction area (Java `copperReachableWithoutPlane`).
fn reachable_without_plane(board: &RoutingBoard, start: ItemId) -> BTreeSet<ItemId> {
    let mut reached = BTreeSet::new();
    let mut queue = vec![start];
    while let Some(id) = queue.pop() {
        let Some(item) = board.item(id) else {
            continue;
        };
        if item.is_conduction_area() || !reached.insert(id) {
            continue;
        }
        queue.extend(board.normal_contacts(id));
    }
    reached
}

fn incomplete(board: &RoutingBoard) -> usize {
    BoardStatistics::new(board).connections.incomplete_count
}

fn net_items(board: &RoutingBoard, net: i32) -> (usize, usize) {
    let traces = board
        .items()
        .filter(|i| i.is_trace() && i.contains_net(net))
        .count();
    let vias = board
        .items()
        .filter(|i| i.is_via() && i.contains_net(net))
        .count();
    (traces, vias)
}

// ---------------------------------------------------------------------------
// A12.6: the fanout probe
// ---------------------------------------------------------------------------

/// Boards of the `fanout` and `fanout-cutoff` rows whose output differs from
/// Java's, with the reason. Empty: every board is required.
const FANOUT_EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

fn compare_kind(kind: &str) {
    let mut failures = Vec::new();
    let mut boards = 0;
    // `FANOUT_PROBE_BOARD=<board name>` runs one board (for debugging).
    let only = std::env::var("FANOUT_PROBE_BOARD").ok();
    for dsn in autoroute_common::kind_fixtures(kind) {
        let board = autoroute_common::board_name(&dsn);
        if only.as_ref().is_some_and(|b| *b != board) {
            continue;
        }
        boards += 1;
        let lines = fanout_lines::fanout_lines(kind, &dsn);
        let result = autoroute_common::compare_probe(kind, &dsn, &lines);
        let listed = FANOUT_EXPECTED_MISMATCHES.iter().any(|(b, _)| *b == board);
        match result {
            Err(report) if !listed => failures.push(report),
            Ok(()) if listed => failures.push(format!(
                "{board}: listed as an expected mismatch but matches Java; remove it"
            )),
            _ => {}
        }
    }
    assert!(boards > 0 || only.is_some(), "no {kind} fixture");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn fanout_matches_java() {
    compare_kind("fanout");
}

#[test]
fn fanout_cutoff_matches_java() {
    compare_kind("fanout-cutoff");
}

// ---------------------------------------------------------------------------
// A12.4: D8 plane vias (PlaneFanoutViaTest)
// ---------------------------------------------------------------------------

/// `fixtures.PlaneFanoutViaTest.eachPlaneNetPadGetsItsOwnViaToTheInnerPlane`,
/// with the fanout before the passes and the optimizer off.
#[test]
fn plane_fanout_via() {
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    let mut loaded = load_testing(MIN_FR_TEST, profile);
    let run = run_job(&mut loaded);
    let fanout = run.fanout.expect("the fanout ran");
    assert!(fanout.completed_passes > 0, "{fanout:?}");
    let board = &loaded.board;
    let gnd = net_number(board, "GND");
    let pads = smd_pins_of(board, gnd);
    assert_eq!(pads.len(), 2, "the fixture has two GND SMD pads");
    for &pad in &pads {
        let connected = board.connected_set(pad, gnd, false);
        assert!(
            connected
                .iter()
                .any(|id| board.item(*id).is_some_and(|i| i.is_conduction_area())),
            "{pad:?} must be connected to the GND plane"
        );
        let reached = reachable_without_plane(board, pad);
        for &other in &pads {
            assert!(
                other == pad || !reached.contains(&other),
                "{pad:?} must not be chained to {other:?} by traces"
            );
        }
    }
    let (_, gnd_vias) = net_items(board, gnd);
    assert_eq!(gnd_vias, pads.len(), "every GND pad needs its own via");
    for via in board.items().filter(|i| i.is_via()) {
        for pin in board
            .items()
            .filter(|i| i.is_pin() && i.first_layer() == i.last_layer())
        {
            let pad = board.tile_shape(pin, 0).expect("pad shape");
            for k in 0..board.tile_shape_count(via) {
                let layer = via.drill().expect("drill").shape_layer(k);
                if layer != pin.first_layer() {
                    continue;
                }
                let shape = board.tile_shape(via, k).expect("via shape");
                assert!(
                    !shape.intersects_tile(&pad),
                    "via {:?} sits on pad {:?}",
                    via.id(),
                    pin.id()
                );
            }
        }
    }
    let plane_layer_traces = board
        .items()
        .filter_map(|i| i.as_trace())
        .filter(|t| !board.layer_structure.layers[t.layer as usize].is_signal)
        .count();
    assert_eq!(plane_layer_traces, 0, "no trace on the plane layer");
}

// ---------------------------------------------------------------------------
// A12.5: SmdPinFanoutRoutingTest
// ---------------------------------------------------------------------------

/// A job of `SmdPinFanoutRoutingTest` with `max_passes` (and the optimizer
/// off): returns the incomplete connections after it.
fn smd_fanout_job(dsn: &str, max_passes: i32) -> (usize, JobRun) {
    let mut profile = TestingProfile::default();
    profile.set_max_passes(max_passes);
    profile.set_optimizer_enabled(false);
    let mut loaded = load_testing(dsn, profile);
    let run = run_job(&mut loaded);
    (incomplete(&loaded.board), run)
}

#[test]
fn smd_pin_fanout_issue558_dev_board() {
    let (left, run) = smd_fanout_job(DEV_BOARD, 10);
    eprintln!("Issue558-dev-board: {left} incomplete after {run:?}");
    assert_eq!(left, 0);
}

#[test]
fn smd_pin_fanout_fanout_only_mode() {
    let mut profile = TestingProfile::default();
    profile.set_enabled(false);
    profile.set_optimizer_enabled(false);
    profile.set_fanout_enabled(true);
    let mut loaded = load_testing(DEV_BOARD, profile);
    let run = run_job(&mut loaded);
    assert_eq!(run.passes, 0);
    let vias = loaded.board.vias().len();
    eprintln!(
        "Issue558-dev-board fanout only: {vias} vias, {:?}",
        run.fanout
    );
    assert!(vias > 0);
}

#[test]
fn smd_pin_fanout_issue508_bm06() {
    let (left, run) = smd_fanout_job("fixtures/Issue508-DAC2020_bm06.dsn", 10);
    eprintln!("Issue508-DAC2020_bm06: {left} incomplete after {run:?}");
    assert!(left <= 8, "{left}");
}

/// Slow in Java too (`@Tag("slow")`).
#[test]
#[ignore = "slow, as in Java (@Tag(\"slow\"))"]
fn smd_pin_fanout_issue508_bm10() {
    let (left, run) = smd_fanout_job("fixtures/Issue508-DAC2020_bm10.dsn", 10);
    eprintln!("Issue508-DAC2020_bm10: {left} incomplete after {run:?}");
    assert_eq!(left, 0);
}

#[test]
fn smd_pin_fanout_smd_routing_issue_demo() {
    let (left, run) = smd_fanout_job("fixtures/Issue508-SMD-routing-issue-demo.dsn", 10);
    eprintln!("Issue508-SMD-routing-issue-demo: {left} incomplete after {run:?}");
    assert!(left <= 2, "{left}");
}

// ---------------------------------------------------------------------------
// A12.5: Issue952FanoutIgnoredNetClassTest, cases 1-4
// ---------------------------------------------------------------------------

/// The Issue558 job with `ignore` as `autorouter.ignore_net_classes`.
fn ignored_class_job(router: bool, ignore: &[&str]) -> Loaded {
    let mut profile = TestingProfile::default();
    profile.set_enabled(router);
    if router {
        profile.set_max_passes(1);
    }
    profile.set_optimizer_enabled(false);
    profile.set_fanout_enabled(true);
    profile.patch.autorouter.ignore_net_classes =
        Some(ignore.iter().map(|s| s.to_string()).collect());
    let mut loaded = load_testing(DEV_BOARD, profile);
    run_job(&mut loaded);
    loaded
}

/// Nets `+3V3` and `+5V` (every subnet) have no traces and no vias.
fn assert_power_nets_untouched(board: &RoutingBoard) {
    for name in ["+3V3", "+5V"] {
        let nets = board.rules.nets.get_all_by_name(name);
        assert!(!nets.is_empty(), "{name}");
        for net in nets {
            assert_eq!(
                net_items(board, net.net_number),
                (0, 0),
                "ignored net {name} must get no traces or vias"
            );
        }
    }
}

#[test]
fn issue952_fanout_only_stage_skips_ignored_net_class() {
    let loaded = ignored_class_job(false, &["Power,Default"]);
    assert_power_nets_untouched(&loaded.board);
    assert!(
        !loaded.board.vias().is_empty(),
        "other nets get fanout vias"
    );
}

#[test]
fn issue952_full_autoroute_with_fanout_skips_ignored_net_class() {
    let loaded = ignored_class_job(true, &["Power,Default"]);
    assert_power_nets_untouched(&loaded.board);
    assert!(
        loaded.board.items().any(|i| i.is_trace()),
        "other nets get traces"
    );
}

#[test]
fn issue952_all_net_classes_ignored_gives_no_vias_and_traces() {
    let loaded = ignored_class_job(false, &["default", "kicad_default", "Power,Default"]);
    assert!(loaded.board.vias().is_empty());
    assert!(!loaded.board.items().any(|i| i.is_trace()));
}

/// Case 4 (`fullPipelineWithOptimizerSkipsIgnoredNetClass`) without the
/// optimizer, which O06 ports.
#[test]
fn issue952_full_pipeline_skips_ignored_net_class() {
    let loaded = ignored_class_job(true, &["Power,Default"]);
    assert_power_nets_untouched(&loaded.board);
    assert!(loaded.board.items().any(|i| i.is_trace()));
}

// ---------------------------------------------------------------------------
// A12.5: Dac2020Bm11FanoutTraceTest
// ---------------------------------------------------------------------------

#[test]
fn dac2020_bm11_fanout_trace() {
    let mut profile = TestingProfile::default();
    profile.set_max_passes(100);
    profile.set_max_items(0);
    profile.set_fanout_enabled(true);
    profile.set_router_enabled(false);
    profile.set_optimizer_enabled(false);
    let mut loaded = load_testing("fixtures/Issue730-DAC2020_bm11.dsn", profile);
    assert!(!loaded.settings.run_router());
    assert!(!loaded.settings.run_optimizer());
    assert!(loaded.settings.fanout.enabled);
    let run = run_job(&mut loaded);
    let stats = BoardStatistics::new(&loaded.board);
    eprintln!(
        "bm11: escaped {} of {}, {:?}",
        stats.fanout.escaped_count, stats.fanout.total_smd_pins, run.fanout
    );
    assert!(
        stats.fanout.escaped_count >= 154,
        "{}",
        stats.fanout.escaped_count
    );
}

// ---------------------------------------------------------------------------
// A12.6: WorkLimitDeterminismTest
// ---------------------------------------------------------------------------

/// `fixtures.WorkLimitDeterminismTest` on `Issue508-DAC2020_bm01`: one fanout
/// pass and the first autorouter item with the optimizer off.
mod work_limit_determinism {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    const BOARD: &str = "fixtures/Issue508-DAC2020_bm01.dsn";

    /// Java `fanOut(maxNominalMillisecondsPerPin)`: the board's state hash.
    fn fan_out(max_ms_per_pin: i64) -> u128 {
        let mut profile = TestingProfile::default();
        profile.set_optimizer_enabled(false);
        profile.set_max_items(1);
        profile.set_fanout_max_passes(1);
        // `job.routerSettings.setMaxPasses(1)` after the load.
        profile.patch.autorouter.max_passes = Some(1);
        profile.patch.fanout.max_milliseconds_per_pin = Some(max_ms_per_pin);
        let mut loaded = load_testing(BOARD, profile);
        run_job(&mut loaded);
        loaded.board.state_hash()
    }

    /// Keeps one thread per core busy until dropped (Java `CpuLoad`).
    struct CpuLoad {
        running: Arc<AtomicBool>,
        threads: Vec<std::thread::JoinHandle<u64>>,
    }

    impl CpuLoad {
        fn on_all_cores() -> CpuLoad {
            let running = Arc::new(AtomicBool::new(true));
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
            let threads = (0..cores)
                .map(|_| {
                    let running = Arc::clone(&running);
                    std::thread::spawn(move || {
                        let mut value: u64 = 0;
                        while running.load(Ordering::Relaxed) {
                            value = value.wrapping_mul(31).wrapping_add(7);
                        }
                        value
                    })
                })
                .collect();
            CpuLoad { running, threads }
        }
    }

    impl Drop for CpuLoad {
        fn drop(&mut self) {
            self.running.store(false, Ordering::Relaxed);
            for thread in self.threads.drain(..) {
                let _ = thread.join();
            }
        }
    }

    #[test]
    fn cut_off_fanout_is_the_same_when_busy() {
        let idle = fan_out(1);
        let busy = {
            let _load = CpuLoad::on_all_cores();
            fan_out(1)
        };
        assert_eq!(idle, busy);
    }

    #[test]
    fn tiny_budget_really_cuts_off() {
        assert_ne!(fan_out(1), fan_out(10_000));
    }
}

// ---------------------------------------------------------------------------
// A12.6: the input of tools/determinism-check.sh
// ---------------------------------------------------------------------------

/// Writes, for each board of the `fanout` row, the probe lines of a full
/// fanout-only run (all `fanout.max_passes`) and the board's state hash to
/// `$FANOUT_DUMP_OUT/<board>.jsonl`. Run by `tools/determinism-check.sh`
/// (see the A12.6 note of `docs/roadmap/tasks/A12-fanout.md`).
#[test]
#[ignore = "writes files; run by tools/determinism-check.sh"]
fn dump_fanout_only_runs() {
    let Ok(out) = std::env::var("FANOUT_DUMP_OUT") else {
        panic!("set FANOUT_DUMP_OUT");
    };
    let out = Path::new(&out);
    std::fs::create_dir_all(out).expect("output directory");
    for dsn in autoroute_common::kind_fixtures("fanout") {
        let board = autoroute_common::board_name(&dsn).replace('/', "_");
        let mut loaded = autoroute_common::load(&dsn);
        let mut lines = Vec::new();
        fanout_lines::run_fanout_lines(&mut loaded, &mut lines);
        lines.push(format!("{:032x}", loaded.board.state_hash()));
        std::fs::write(out.join(format!("{board}.jsonl")), lines.join("\n") + "\n")
            .expect("written");
    }
}

// ---------------------------------------------------------------------------
// A12.5: footprint frames (KRT report)
// ---------------------------------------------------------------------------

/// One fanout pass on Issue558-dev-board with U2 as placed, rotated 90
/// degrees and mirrored to the back equals Java pin by pin on each copy,
/// and so do U2's escape counts (kind `fanout-frames`, last line). Whether
/// the counts agree across copies is printed, not required: a difference is
/// a finding for the Q series.
#[test]
fn footprint_frames_match_java() {
    compare_kind("fanout-frames");
    for dsn in autoroute_common::kind_fixtures("fanout-frames") {
        let board = autoroute_common::board_name(&dsn);
        let mut loaded = fanout_lines::load_for_probe(&dsn, None);
        let mut lines = Vec::new();
        fanout_lines::run_fanout_lines(&mut loaded, &mut lines);
        let (pins, escaped) = fanout_lines::escapes_of(&loaded.board, "U2");
        let (java, _) = autoroute_common::java_output("fanout-frames", &board).expect("java");
        eprintln!(
            "{board}: U2 escaped {escaped} of {pins}; Java {}",
            java[java.len() - 1]
        );
    }
}

// ---------------------------------------------------------------------------
// A12.4: a same-net island beside two plane pins (KRT report)
// ---------------------------------------------------------------------------

const PLANE_ISLAND: &str = "crates/freerouting/tests/data/plane-island/min_fr_test-island.dsn";

/// The `plane-island` lines: the fanout stage, then per plane-net SMD pin
/// the conduction areas of its connected set, then the incomplete count.
fn plane_island_lines(loaded: &mut Loaded) -> Vec<String> {
    use freerouting::autoroute::probe::{ProbeValue, record_line};
    let cancel = CancelToken::new();
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &events,
    };
    FreeroutingFanout.run(&mut loaded.board, &mut ctx);
    let board = &loaded.board;
    let mut out =
        autoroute_common::writer("plane-island", &autoroute_common::board_name(PLANE_ISLAND))
            .into_lines();
    out.push(record_line(&[
        ("stage", "pin-planes".into()),
        (
            "boardHash",
            format!("{:016x}", fanout_lines::route_hash(board)).into(),
        ),
    ]));
    for id in board.pins() {
        let Some(pin) = board.item(id) else { continue };
        if pin.first_layer() != pin.last_layer() || pin.net_count() != 1 {
            continue;
        }
        let net = pin.net_number(0);
        if !board
            .rules
            .nets
            .get(net)
            .is_some_and(|n| n.contains_plane())
        {
            continue;
        }
        let mut areas: Vec<i32> = board
            .connected_set(id, net, false)
            .into_iter()
            .filter(|a| board.item(*a).is_some_and(|i| i.is_conduction_area()))
            .map(|a| a.0)
            .collect();
        areas.sort_unstable();
        out.push(record_line(&[
            ("pinPlanes", id.0.into()),
            ("net", net.into()),
            ("areas", ProbeValue::ints(areas.into_iter().map(i64::from))),
        ]));
    }
    out.push(record_line(&[(
        "incomplete",
        (incomplete(board) as i64).into(),
    )]));
    out
}

/// Java connects both GND pins through the island: each pin's fanout via
/// lands on the island (area 3, the left `plane GND` polygon), not on the
/// main plane, and the router then treats the pins as connected to a plane.
/// The incomplete count still reports the island cut off from the main
/// plane, and equals the DRC's. The rule that a tap must reach the main
/// plane is Q01's (and X02.2's report).
#[test]
fn plane_island() {
    let mut loaded = autoroute_common::load(PLANE_ISLAND);
    let lines = plane_island_lines(&mut loaded);
    if let Err(report) = autoroute_common::compare_probe("plane-island", PLANE_ISLAND, &lines) {
        panic!("{report}");
    }
    let board = &loaded.board;
    // The two GND planes: the island (left, under the pads) is narrower
    // than the main plane.
    let areas: Vec<(i32, i64)> = board
        .items()
        .filter(|i| i.is_conduction_area())
        .map(|i| {
            let b = i.bounding_box();
            (i.id().0, i64::from(b.ur.x) - i64::from(b.ll.x))
        })
        .collect();
    eprintln!("conduction areas (id, width): {areas:?}");
    let island = areas.iter().max_by_key(|(_, w)| *w).map(|(id, _)| *id);
    assert_eq!(areas.len(), 2);
    assert_eq!(
        island,
        Some(3),
        "area 3 is the wider left polygon, the island"
    );
    let drc = freerouting::drc::DesignRulesChecker::new(board).incomplete_count();
    assert_eq!(
        incomplete(board),
        drc,
        "the incomplete count equals the DRC's"
    );
    let gnd = net_number(board, "GND");
    let gnd_drc = freerouting::drc::DesignRulesChecker::new(board).incomplete_count_of(gnd);
    assert_eq!(gnd_drc, 1, "the island is cut off from the main plane");
}

// ---------------------------------------------------------------------------
// A12.1 and A12.3: the pin's control and the registry
// ---------------------------------------------------------------------------

/// DIVERGENCES (Fanout): a net class without a via rule on a board with
/// via rules passes `BatchFanout`'s via check on the fallback; Java's
/// `AutorouteControl` then throws, the port fails the pin.
#[test]
fn a_control_error_fails_the_pin() {
    use freerouting::autoroute::AttemptState;
    use freerouting::autoroute::fanout_pin::fanout_pin;
    use freerouting::board::search::WorkLimit;
    let mut loaded = autoroute_common::load(MIN_FR_TEST);
    let gnd = net_number(&loaded.board, "GND");
    let class = loaded.board.rules.nets.get(gnd).expect("GND").net_class();
    loaded
        .board
        .rules
        .net_classes
        .get_mut(class)
        .set_via_rule(None);
    let pin = smd_pins_of(&loaded.board, gnd)[0];
    let cancel = CancelToken::new();
    let events = NullEvents;
    let ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &events,
    };
    let board = &mut loaded.board;
    let limit = WorkLimit::nominal_ms(board.search_trees().work_meter(), 10_000);
    let before = board.state_hash();
    let result = fanout_pin(board, pin, 100, &limit, &ctx);
    assert_eq!(result.state, AttemptState::Failed, "{result}");
    assert!(!result.details.is_empty());
    assert_eq!(board.state_hash(), before, "nothing was routed");
}

#[test]
fn the_registry_builds_the_fanout_stage() {
    use freerouting::autoroute::stages::{StageRegistry, StageSelection};
    let mut loaded = autoroute_common::load(MIN_FR_TEST);
    let selection = StageSelection::from_settings(&loaded.settings);
    let mut pipeline = StageRegistry::builtin()
        .build(&selection, &loaded.settings)
        .expect("built");
    assert_eq!(pipeline.fanout.id(), "freerouting-fanout");
    let cancel = CancelToken::new();
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &events,
    };
    let summary = pipeline.fanout.run(&mut loaded.board, &mut ctx);
    assert_eq!(
        (summary.escaped, summary.total, summary.timed_out),
        (4, 4, false),
        "{summary:?}"
    );
    assert!(summary.completed_passes >= 1);
}

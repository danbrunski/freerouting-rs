//! X06: the region classes (X06.2), the raster-share probe (X04.6's
//! question) and X06's findings in `krt-grid`. X06.1's `krt-astar` bundle
//! and its tests were removed after X06.3's registered run rejected it.
//!
//! Boards: X05's synthetic fixtures in `tests/data/`, and
//! `x06-regions.dsn` (a 32-pin 0.5 mm pitch QFN U1, a 32-pin 0.8 mm pitch
//! QFP U2 with 0.45 mm pads, a 3-pin 2.54 mm header J1 and two 0603-size
//! resistors R1 and R2; nets Q = U1-1 to J1-1, L = U2-1 to J1-3, O = R1-1 to
//! R2-1). Not under `fixtures/`: every DSN there needs a Java reference.
//!
//! The ignored test is a release run over a board set (the raster share).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use freerouting::autoroute::batch::connection::{FreeroutingPlaneTaps, maze_work_budget};
use freerouting::autoroute::batch::ordering::select_items;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{
    ConnectionParams, Destinations, PlaneTapPolicy, RouteTask, StageCtx, StageRegistry,
};
use freerouting::board::items::Item;
use freerouting::board::structure::ComponentId;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};
use freerouting_krt::grid::frame::window_for;
use freerouting_krt::grid::raster::Raster;
use freerouting_krt::grid::regions::{self, Region};
use freerouting_krt::grid::router::{
    GridRun, HEURISTIC_WEIGHT, KrtGrid, MAX_FRAME_CELLS, Rejection, StepResult, TURN_COST,
    VIA_PROXIMITY_COST, via_cost_units,
};
use freerouting_krt::grid::{
    DEFAULT_GRID_STEP_UM, FINER_GRID_STEPS_UM, GridTask, raster, step_in_board_units, terminals,
};
use krt_grid_router::{GridRouter, TrackMarginArg};

const OPEN: &str = "crates/freerouting-krt/tests/data/x05-open-two-pin.dsn";
const FINE: &str = "crates/freerouting-krt/tests/data/x05-fine-pitch.dsn";
const REGIONS: &str = "crates/freerouting-krt/tests/data/x06-regions.dsn";
const OWN_PAD_VIA: &str = "crates/freerouting-krt/tests/data/x06-own-pad-via.dsn";
/// The board the 17 Q03 pins come from (Q03.6).
const CANARY_Q03: &str = "fixtures/canary-pwr/canary-pwr-2026-10-15-1747-unpoured.dsn";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `dsn` with `extra` (`path=value`) on the
/// testing source; with `testing`, no optimizer and at most that many
/// passes.
fn prepare(dsn: &str, testing: Option<u32>, extra: &[(&str, &str)]) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = default_profile();
    if let Some(passes) = testing {
        profile.set_optimizer_enabled(false);
        profile.set_max_passes(passes as i32);
    }
    for (key, value) in extra {
        profile
            .patch
            .set_path(key, value)
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    if testing.is_some() || !extra.is_empty() {
        input.extra_sources = vec![(Source::Testing, profile.patch)];
    }
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn route_prepared(prepared: PreparedJob, registry: Option<&StageRegistry>) -> RoutingBoard {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry,
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    outcome.board.take().expect("the routed board")
}

fn stem(dsn: &str) -> String {
    Path::new(dsn)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string()
}

fn net_number(board: &BasicBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("net {name}"))
        .net_number
}

/// `component-pin` of pin `item`.
fn pin_label(board: &BasicBoard, item: &Item) -> String {
    let component = board
        .components
        .get(ComponentId(item.component_id()))
        .map_or("?", |c| c.name.as_str());
    format!("{component}-{}", board.pin_name(item).unwrap_or("?"))
}

/// The class of every pin, by `component-pin`.
fn regions_by_label(board: &BasicBoard) -> BTreeMap<String, Region> {
    regions::pin_regions(board)
        .into_iter()
        .map(|(id, r)| (pin_label(board, board.item(id).unwrap()), r))
        .collect()
}

/// What the gates count on a routed board against its input.
// ---------------------------------------------------------------------------
// X06.2: region classes
// ---------------------------------------------------------------------------

#[test]
fn the_synthetic_qfn_is_a_pin_field_and_the_resistors_are_open() {
    let board = prepare(REGIONS, Some(1), &[]).board;
    let by_label = regions_by_label(&board);
    for (label, region) in &by_label {
        let expected = if label.starts_with("U1-") || label.starts_with("U2-") {
            Region::PinField
        } else {
            Region::Open
        };
        assert_eq!(*region, expected, "{label}");
    }
    assert_eq!(by_label.len(), 32 + 32 + 3 + 2 * 2);

    let classes = regions::classify(&board);
    let by_net: BTreeMap<i32, Region> = classes.iter().map(|(k, r)| (k.net, *r)).collect();
    assert_eq!(classes.len(), 3);
    assert_eq!(by_net[&net_number(&board, "Q")], Region::PinField);
    // U2: 0.8 mm pitch, 0.45 mm pads, 32 pins: the large-component rule.
    assert_eq!(by_net[&net_number(&board, "L")], Region::PinField);
    assert_eq!(by_net[&net_number(&board, "O")], Region::Open);

    // Before routing every connection is incomplete; after, none.
    let connections = regions::connections(&board);
    let before = regions::tally(&connections, &board);
    assert_eq!(before[&Region::PinField].total, 2);
    assert_eq!(before[&Region::PinField].incomplete, 2);
    assert_eq!(before[&Region::Open].incomplete, 1);
    let routed = route_prepared(prepare(REGIONS, Some(3), &[]), None);
    let after = regions::tally(&connections, &routed);
    assert!(after.values().all(|t| t.incomplete == 0 && t.unknown == 0));
}

#[test]
fn an_open_two_pin_connection_is_open() {
    let board = prepare(OPEN, Some(1), &[]).board;
    let classes = regions::classify(&board);
    assert_eq!(classes.len(), 1);
    let (key, region) = classes.iter().next().unwrap();
    assert_eq!(key.net, net_number(&board, "A"));
    assert_eq!(*region, Region::Open);
    // The 0.4 mm pitch row of X05.5 is a pin field; its far pad is not.
    let fine = prepare(FINE, Some(1), &[]).board;
    let by_label = regions_by_label(&fine);
    assert!((1..=5).all(|p| by_label[&format!("U1-{p}")] == Region::PinField));
    assert_eq!(by_label["P2-1"], Region::Open);
    assert_eq!(
        regions::classify(&fine).values().collect::<Vec<_>>(),
        vec![&Region::PinField]
    );
}

/// The 17 pins of Q03.6 on the board they come from. The rule as the
/// checklist states it puts 11 in `pin-field`. Six meet none of its
/// conditions (an operator decision, X06.2's partial note), by the DSN's
/// pad geometry:
/// - U2 (VQFN-HR, 14 pins): pin 1 (0.7 x 1.0 mm, 0.677 mm from pin 2),
///   pins 8 and 12 (0.4 x 1.0 mm, 1.085 mm from pins 7 and 13), pin 10
///   (2.4 x 0.4 mm, 1.275 mm from pins 9 and 11);
/// - U7 (SOIC-14, 1.27 mm pitch, 0.6 mm pads): pins 3 and 10.
///
/// Q03 flags them for the 1.5 mm Power_High width, not for their pitch.
#[test]
fn the_q03_pins_of_canary_pwr_by_the_rule() {
    let board = prepare(CANARY_Q03, Some(1), &[]).board;
    let by_label = regions_by_label(&board);
    let q03 = [
        "U1-22", "U1-24", "U1-30", "U1-31", "U1-34", "U2-1", "U2-8", "U2-10", "U2-12", "U3-8",
        "U3-10", "U7-3", "U7-10", "Q2-3", "Q3-1", "Q3-2", "Q3-3",
    ];
    let open: Vec<&str> = q03
        .iter()
        .copied()
        .filter(|l| by_label[*l] != Region::PinField)
        .collect();
    assert_eq!(
        open,
        vec!["U2-1", "U2-8", "U2-10", "U2-12", "U7-3", "U7-10"]
    );
    let fields = by_label
        .values()
        .filter(|r| **r == Region::PinField)
        .count();
    println!(
        "canary-pwr: {} pins, {fields} pin-field; Q03 pins not pin-field: {open:?}",
        by_label.len()
    );
}

// ---------------------------------------------------------------------------
// Release runs over a board set
// ---------------------------------------------------------------------------

fn set_boards() -> Vec<String> {
    let set = std::env::var("X06_SET").expect("X06_SET");
    let text = std::fs::read_to_string(repo_root().join(&set)).unwrap();
    let skip: Vec<String> = std::env::var("X06_SKIP")
        .map(|v| v.split(',').map(str::to_string).collect())
        .unwrap_or_default();
    text.lines()
        .map(|l| l.split('#').next().unwrap().trim().to_string())
        .filter(|l| !l.is_empty())
        .filter(|l| !skip.iter().any(|s| l.contains(s.as_str())))
        .collect()
}

/// The start set and the first destination set of `task`, as `krt-grid`
/// takes them (A11.1); `None` when there is nothing to route.
fn route_sets(board: &RoutingBoard, task: &RouteTask) -> Option<GridTask> {
    let unconnected = board.unconnected_set(task.item, task.net);
    if unconnected.is_empty() {
        return None;
    }
    let connected = board.connected_set(task.item, task.net, false);
    let plane = board
        .rules
        .nets
        .get(task.net)
        .is_some_and(|n| n.contains_plane());
    let (start, dest) = if plane {
        let Destinations { primary, .. } =
            FreeroutingPlaneTaps.destinations(board, task, &unconnected);
        (connected, primary)
    } else {
        (unconnected, connected)
    };
    Some(GridTask {
        net: task.net,
        start,
        dest,
    })
}

/// X04.6's question on the input boards of `X06_SET`: for each pass-1
/// connection, the steps `krt-grid` would try (0.1 mm, then finer while no
/// path is found), each timed in two parts: the window, raster and
/// terminal cells, and the grid search. Nothing is inserted, so every
/// connection sees the input board. Prints per board the summed times and
/// the raster's share. Report only (the clock decides nothing).
#[test]
#[ignore = "a release run over a board set; set X06_SET"]
fn raster_share_on_a_set() {
    println!("board\tconnections\tattempts\traster_ms\tsearch_ms\traster_share");
    let (mut all_raster, mut all_search) = (0u128, 0u128);
    for dsn in set_boards() {
        let prepared = prepare(&dsn, Some(1), &[]);
        let board = prepared.board;
        let (mut raster_ns, mut search_ns, mut attempts, mut connections) = (0u128, 0u128, 0, 0);
        for task in select_items(&board) {
            let Some(grid_task) = route_sets(&board, &task) else {
                continue;
            };
            connections += 1;
            let mut budget = maze_work_budget(1).min(u64::from(u32::MAX)) as u32;
            let steps = std::iter::once(DEFAULT_GRID_STEP_UM).chain(FINER_GRID_STEPS_UM);
            for step_um in steps {
                let started = Instant::now();
                let step = step_in_board_units(&board, step_um);
                let frame = window_for(&board, &grid_task, step);
                if frame.cell_count() > MAX_FRAME_CELLS {
                    continue;
                }
                let mut r = raster::build_raster(&board, &grid_task, &frame);
                let (sources, targets) = terminals::for_task(&board, &frame, &grid_task);
                terminals::mark(&mut r.map, &sources.cells);
                terminals::mark(&mut r.map, &targets.cells);
                raster_ns += started.elapsed().as_nanos();
                attempts += 1;
                if sources.cells.is_empty() || targets.cells.is_empty() {
                    continue;
                }
                let cells = |c: &[[i32; 3]]| -> Vec<(i32, i32, u8)> {
                    c.iter().map(|&[i, j, l]| (i, j, l as u8)).collect()
                };
                let started = Instant::now();
                let router = GridRouter::new(
                    via_cost_units(step_um),
                    HEURISTIC_WEIGHT,
                    Some(TURN_COST),
                    Some(VIA_PROXIMITY_COST),
                    0,
                    0,
                    None,
                    None,
                    None,
                    0,
                    0,
                    0,
                    0,
                    0,
                );
                let (found, iterations, _) = router.route_with_frontier(
                    &r.map,
                    cells(&sources.cells),
                    cells(&targets.cells),
                    budget,
                    false,
                    0,
                    None,
                    None,
                    2,
                    TrackMarginArg::Scalar(0.0),
                    0,
                    0.0,
                    0.0,
                    0,
                    0,
                );
                search_ns += started.elapsed().as_nanos();
                budget = budget.saturating_sub(iterations);
                if found.is_some() || budget == 0 {
                    break;
                }
            }
        }
        all_raster += raster_ns;
        all_search += search_ns;
        let share = raster_ns as f64 / (raster_ns + search_ns).max(1) as f64;
        println!(
            "{}\t{connections}\t{attempts}\t{:.1}\t{:.1}\t{share:.3}",
            stem(&dsn),
            raster_ns as f64 / 1e6,
            search_ns as f64 / 1e6
        );
    }
    let share = all_raster as f64 / (all_raster + all_search).max(1) as f64;
    println!(
        "all\t\t\t{:.1}\t{:.1}\t{share:.3}",
        all_raster as f64 / 1e6,
        all_search as f64 / 1e6
    );
}

// ---------------------------------------------------------------------------
// X06's finding in krt-grid: a via beside an SMD pad of its own net
// ---------------------------------------------------------------------------

/// Routes the first connection of `dsn` with `router` (krt-grid's params as
/// in X05's tests) and returns the run and the board.
fn grid_run(dsn: &str, router: &mut KrtGrid) -> (GridRun, RoutingBoard) {
    let mut prepared = prepare(dsn, Some(1), &[]);
    let task = select_items(&prepared.board)[0];
    let cancel = CancelToken::new();
    let ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    let p = ConnectionParams {
        pass: 1,
        ripup_allowed: false,
        ripup_cost: 0,
        via_cost: 50,
        trace_costs: Vec::new(),
        remove_unconnected_vias: false,
        work_budget: maze_work_budget(1),
        pull_tight_accuracy: 0,
        strict_check: false,
    };
    let run = router.route_logged(&mut prepared.board, &task, &p, &ctx);
    (run, prepared.board)
}

/// With `(attach off)` a via keeps its clearance from the SMD pads of its
/// own net (Java's `Via` obstacle rule). The raster now blocks those via
/// cells, so the via stands at the one free column, 0.6 mm past P1's edge.
#[test]
fn a_via_keeps_its_clearance_from_an_smd_pad_of_its_own_net() {
    let (run, board) = grid_run(OWN_PAD_VIA, &mut KrtGrid::default());
    for a in &run.attempts {
        println!("{}", a.line());
    }
    assert_eq!(
        run.attempts[0].result,
        StepResult::Routed { traces: 2, vias: 1 }
    );
    assert!(board.all_clearance_violations().is_empty());
    let via = board.items().find(|i| i.is_via()).unwrap();
    assert_eq!(via.drill().unwrap().center.x, 111_000);
}

/// The raster bug forced back (the via cells beside P1 unblocked): the
/// path's via is within clearance of P1, which only P1's side of the DRC
/// reports, and the gate rejects it at every step.
#[test]
fn the_gate_rejects_a_via_only_the_pads_side_of_the_drc_sees() {
    fn unblock_beside_p1(raster: &mut Raster) {
        let frame = raster.frame;
        let cells: Vec<[i32; 2]> = (frame.i0..frame.i0 + frame.nx)
            .flat_map(|i| (frame.j0..frame.j0 + frame.ny).map(move |j| [i, j]))
            .filter(|&[i, j]| {
                let (x, y) = (i * frame.step, j * frame.step);
                (108_500..=109_900).contains(&x) && (95_000..=105_000).contains(&y)
            })
            .collect();
        raster.map.remove_blocked_vias_batch(&cells);
    }
    let mut router = KrtGrid {
        raster_hook: Some(unblock_beside_p1),
        ..KrtGrid::default()
    };
    let (run, board) = grid_run(OWN_PAD_VIA, &mut router);
    let p1 = board
        .items()
        .find(|i| i.is_pin() && pin_label(&board, i) == "P1-1")
        .unwrap()
        .id();
    assert_eq!(run.attempts.len(), 3);
    for a in &run.attempts {
        println!("{}", a.line());
        match &a.result {
            StepResult::Rejected(Rejection::Clearance { count, first }) => {
                assert_eq!((*count, first.0, first.2), (1, p1, 0), "{}", a.line());
            }
            other => panic!("expected the pin-side rejection, got {other:?}"),
        }
    }
    assert!(board.items().all(|i| !i.is_via() && !i.is_trace()));
}

/// The testing source's starting patch: empty, so every run starts from
/// the default settings. Not `TestingProfile::default()`, which carries
/// Java's fixture-test settings (`java_fixture_test`: copper-to-edge
/// clearance 0) and biased the pre-P04 measurements.
fn default_profile() -> TestingProfile {
    TestingProfile {
        patch: SettingsPatch::default(),
    }
}

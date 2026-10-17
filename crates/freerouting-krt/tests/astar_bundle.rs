//! X06: the `krt-astar` bundle (grid A* path, then Freerouting's pull-tight
//! and optimizer) and the region classes that measure it.
//!
//! Boards: X05's synthetic fixtures in `tests/data/`, and
//! `x06-regions.dsn` (a 32-pin 0.5 mm pitch QFN U1, a 32-pin 0.8 mm pitch
//! QFP U2 with 0.45 mm pads, a 3-pin 2.54 mm header J1 and two 0603-size
//! resistors R1 and R2; nets Q = U1-1 to J1-1, L = U2-1 to J1-3, O = R1-1 to
//! R2-1). Not under `fixtures/`: every DSN there needs a Java reference.
//!
//! The ignored tests are release runs over a board set (X06.1's gates,
//! X06.3's pre-P04 dry run).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use freerouting::autoroute::batch::connection::{FreeroutingPlaneTaps, maze_work_budget};
use freerouting::autoroute::batch::ordering::select_items;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{
    ConnectionParams, DEFAULT_ROUTER_ALGORITHM, Destinations, PlaneTapPolicy, RouteTask, StageCtx,
    StageRegistry, StageSelection,
};
use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::ComponentId;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::datastructures::IdGenerator;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};
use freerouting_krt::grid::bundle::{KRT_ASTAR_BUNDLE, KRT_ASTAR_ID};
use freerouting_krt::grid::frame::window_for;
use freerouting_krt::grid::raster::Raster;
use freerouting_krt::grid::regions::{self, Region};
use freerouting_krt::grid::router::{
    GridRun, HEURISTIC_WEIGHT, KRT_GRID_ID, KrtGrid, MAX_FRAME_CELLS, Rejection, StepResult,
    TURN_COST, VIA_PROXIMITY_COST, via_cost_units, via_in_pad,
};
use freerouting_krt::grid::{
    DEFAULT_GRID_STEP_UM, FINER_GRID_STEPS_UM, GridTask, raster, step_in_board_units, terminals,
};
use krt_grid_router::{GridRouter, TrackMarginArg};

const OPEN: &str = "crates/freerouting-krt/tests/data/x05-open-two-pin.dsn";
const FINE: &str = "crates/freerouting-krt/tests/data/x05-fine-pitch.dsn";
const WALL: &str = "crates/freerouting-krt/tests/data/x05-wall.dsn";
const REGIONS: &str = "crates/freerouting-krt/tests/data/x06-regions.dsn";
const DETOUR: &str = "crates/freerouting-krt/tests/data/x06-detour.dsn";
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
    let mut profile = TestingProfile::default();
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

fn krt_registry() -> StageRegistry {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    registry
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

fn ses(board: &RoutingBoard, dsn: &str) -> String {
    write_ses(board, &stem(dsn), SesStyle::Java)
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Gates {
    /// Clearance violations on the input board.
    drc_input: usize,
    /// Clearance violations on the routed board.
    drc_routed: usize,
    /// Of them, violations with an item the run created.
    drc_new: usize,
    /// Vias the run created whose copper overlaps a pin's pad.
    via_in_pad: usize,
    /// Vias on the routed board.
    vias: usize,
    /// Trace length on the routed board, in board units.
    length: i64,
    /// Airlines left on the routed board.
    airlines: usize,
}

fn gates(input: &BasicBoard, routed: &BasicBoard) -> Gates {
    let max_input_id = input.communication.id_generator.max_generated_id();
    let violations = routed.all_clearance_violations();
    let new = |id: ItemId| id.0 > max_input_id;
    let mut checker = freerouting::drc::DesignRulesChecker::new(routed);
    Gates {
        drc_input: input.all_clearance_violations().len(),
        drc_routed: violations.len(),
        drc_new: violations
            .iter()
            .filter(|v| new(v.first_item) || new(v.second_item))
            .count(),
        via_in_pad: routed
            .items()
            .filter(|i| i.is_via() && new(i.id()) && via_in_pad(routed, i))
            .count(),
        vias: routed.items().filter(|i| i.is_via()).count(),
        length: routed
            .items()
            .filter_map(Item::as_trace)
            .map(|t| t.length().round() as i64)
            .sum(),
        airlines: checker.all_airlines().len(),
    }
}

// ---------------------------------------------------------------------------
// X06.1: the bundle
// ---------------------------------------------------------------------------

#[test]
fn the_registry_selects_the_krt_astar_bundle() {
    let registry = krt_registry();
    let prepared = prepare(OPEN, Some(1), &[("autorouter.algorithm", KRT_ASTAR_ID)]);
    let selection = StageSelection::resolve(&prepared.settings, &registry);
    assert!(selection.warnings.is_empty(), "{:?}", selection.warnings);
    assert_eq!(selection.algorithm.id, KRT_ASTAR_ID);
    assert_eq!(selection.connection_router.id, KRT_ASTAR_ID);
    assert!(selection.connection_router.from_default);
    // Every other stage is Freerouting's.
    let freerouting = StageSelection::resolve(&prepare(OPEN, Some(1), &[]).settings, &registry);
    for (a, b) in [
        (&selection.ordering, &freerouting.ordering),
        (&selection.ripup_policy, &freerouting.ripup_policy),
        (&selection.pass_controller, &freerouting.pass_controller),
        (&selection.plane_taps, &freerouting.plane_taps),
        (&selection.neck_down, &freerouting.neck_down),
        (&selection.optimizer, &freerouting.optimizer),
        (&selection.fanout, &freerouting.fanout),
    ] {
        assert_eq!(a.id, b.id);
    }
    assert_eq!(KRT_ASTAR_BUNDLE.ordering, "freerouting-ordering");
    let pipeline = registry.build(&selection, &prepared.settings).unwrap();
    assert_eq!(pipeline.connection_router.id(), KRT_ASTAR_ID);

    // Without this crate's registration: Java's normalisation and warning.
    let builtin = StageRegistry::builtin();
    let selection = StageSelection::resolve(&prepared.settings, &builtin);
    assert_eq!(selection.algorithm.id, DEFAULT_ROUTER_ALGORITHM);
    assert_eq!(selection.connection_router.id, "freerouting-connection");
    assert_eq!(
        selection.warnings,
        vec![format!(
            "The algorithm '{KRT_ASTAR_ID}' is not supported. The default algorithm \
             '{DEFAULT_ROUTER_ALGORITHM}' will be used instead."
        )]
    );
}

#[test]
fn registering_the_bundle_changes_no_session_while_it_is_unselected() {
    let registry = krt_registry();
    for dsn in [OPEN, FINE, REGIONS] {
        let builtin = route_prepared(prepare(dsn, Some(3), &[]), None);
        let krt = route_prepared(prepare(dsn, Some(3), &[]), Some(&registry));
        assert_eq!(ses(&builtin, dsn), ses(&krt, dsn), "{dsn}");
    }
}

/// The bundle routes the synthetic boards completely and cleanly, and its
/// pull-tight leaves no more trace length than `krt-grid` alone.
#[test]
fn the_bundle_routes_the_fixtures_and_pulls_the_grid_path_tight() {
    let registry = krt_registry();
    for dsn in [OPEN, FINE, REGIONS, DETOUR] {
        let grid = prepare(
            dsn,
            Some(3),
            &[("autorouter.connection_router", KRT_GRID_ID)],
        );
        let input = grid.board.clone();
        let grid = route_prepared(grid, Some(&registry));
        let astar = route_prepared(
            prepare(dsn, Some(3), &[("autorouter.algorithm", KRT_ASTAR_ID)]),
            Some(&registry),
        );
        let (g, a) = (gates(&input, &grid), gates(&input, &astar));
        println!("{dsn}: krt-grid {g:?}; krt-astar {a:?}");
        assert_eq!(a.airlines, 0, "{dsn}");
        assert_eq!((a.drc_new, a.via_in_pad), (0, 0), "{dsn}");
        assert_eq!(a.drc_routed, a.drc_input, "{dsn}");
        assert!(a.length <= g.length, "{dsn}: {} > {}", a.length, g.length);
    }
}

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

fn extra_settings() -> Vec<(String, String)> {
    std::env::var("X06_EXTRA")
        .ok()
        .map(|v| {
            v.split(',')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Every board of `X06_SET` routed with each algorithm of `X06_ARMS`
/// (default `krt-astar`; `freerouting-router` is the default bundle), with
/// default settings plus `X06_EXTRA`. Prints one row per board and arm:
/// the gates and the region tally. Asserts the X06.1 gates for
/// `krt-astar`: no clearance violation with a new item, no new via in a
/// pad. With `X06_SES_OUT`, writes `<dir>/<stem>.<arm>.ses`.
#[test]
#[ignore = "a release run over a board set; set X06_SET"]
fn route_a_set() {
    let registry = krt_registry();
    let arms: Vec<String> = std::env::var("X06_ARMS")
        .unwrap_or_else(|_| KRT_ASTAR_ID.to_string())
        .split(',')
        .map(str::to_string)
        .collect();
    let out = std::env::var("X06_SES_OUT").ok().map(PathBuf::from);
    if let Some(dir) = &out {
        std::fs::create_dir_all(dir).unwrap();
    }
    println!(
        "board\tarm\tdrc_input\tdrc_routed\tdrc_new\tvia_in_pad\tvias\tlength\tairlines\t\
         pin_field_total\tpin_field_incomplete\tpin_field_unknown\topen_total\topen_incomplete\t\
         open_unknown\tseconds"
    );
    for dsn in set_boards() {
        for arm in &arms {
            let mut extra = extra_settings();
            extra.push(("autorouter.algorithm".into(), arm.clone()));
            let extra: Vec<(&str, &str)> = extra
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let prepared = prepare(&dsn, None, &extra);
            let input = prepared.board.clone();
            let connections = regions::connections(&input);
            let started = Instant::now();
            let routed = route_prepared(prepared, Some(&registry));
            let seconds = started.elapsed().as_secs_f64();
            let g = gates(&input, &routed);
            let t = regions::tally(&connections, &routed);
            let (p, o) = (t[&Region::PinField], t[&Region::Open]);
            println!(
                "{}\t{arm}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{seconds:.1}",
                stem(&dsn),
                g.drc_input,
                g.drc_routed,
                g.drc_new,
                g.via_in_pad,
                g.vias,
                g.length,
                g.airlines,
                p.total,
                p.incomplete,
                p.unknown,
                o.total,
                o.incomplete,
                o.unknown
            );
            if let Some(dir) = &out {
                let path = dir.join(format!("{}.{arm}.ses", stem(&dsn)));
                std::fs::write(path, ses(&routed, &dsn)).unwrap();
            }
            if arm == KRT_ASTAR_ID {
                assert_eq!((g.drc_new, g.via_in_pad), (0, 0), "{dsn}");
            }
        }
    }
}

/// One board for an external grader (`tools/kicad-oracle/grade.sh`'s
/// ENGINE_COMMAND, X06.3's registered run): routes `X06_DSN` with
/// `router.autorouter.algorithm = X06_ALGO` (default `krt-astar`) and
/// default settings plus `X06_EXTRA`, writes the session to `X06_SES`, and
/// prints the gates and the region tally as `route_a_set` does.
#[test]
#[ignore = "an engine command for a grader; set X06_DSN and X06_SES"]
fn route_one_dsn() {
    let dsn = std::env::var("X06_DSN").expect("X06_DSN");
    let out = std::env::var("X06_SES").expect("X06_SES");
    let algo = std::env::var("X06_ALGO").unwrap_or_else(|_| KRT_ASTAR_ID.to_string());
    let mut extra = extra_settings();
    extra.push(("autorouter.algorithm".into(), algo.clone()));
    let extra: Vec<(&str, &str)> = extra
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let prepared = prepare(&dsn, None, &extra);
    let input = prepared.board.clone();
    let connections = regions::connections(&input);
    let routed = route_prepared(prepared, Some(&krt_registry()));
    std::fs::write(&out, ses(&routed, &dsn)).unwrap();
    let g = gates(&input, &routed);
    let t = regions::tally(&connections, &routed);
    println!("{} {algo}: {g:?} {t:?}", stem(&dsn));
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

/// Time split of the `krt-astar` connection router in a whole run.
#[derive(Clone, Copy, Debug, Default)]
struct StageTimes {
    connections: usize,
    attempts: usize,
    stage_ns: u128,
    raster_ns: u128,
    emulation_ns: u128,
}

thread_local! {
    static STAGE_TIMES: std::cell::Cell<StageTimes> = const {
        std::cell::Cell::new(StageTimes {
            connections: 0,
            attempts: 0,
            stage_ns: 0,
            raster_ns: 0,
            emulation_ns: 0,
        })
    };
}

const TIMED_ID: &str = "x06-timed-krt-astar";

/// `krt-astar` with a stopwatch (report only): before each connection the
/// window, raster and terminal cells of every step are built once on the
/// current board and timed; after the connection the times of the steps
/// it attempted are added up against the connection's own time.
struct TimedAstar(freerouting_krt::grid::bundle::KrtAstar);

impl freerouting::autoroute::stages::ConnectionRouter for TimedAstar {
    fn id(&self) -> &'static str {
        TIMED_ID
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> freerouting::autoroute::stages::ConnectionOutcome {
        let mut raster_ns = BTreeMap::new();
        if let Some(grid_task) = route_sets(board, task) {
            let steps = std::iter::once(DEFAULT_GRID_STEP_UM).chain(FINER_GRID_STEPS_UM);
            for step_um in steps {
                let started = Instant::now();
                let frame = window_for(board, &grid_task, step_in_board_units(board, step_um));
                if frame.cell_count() <= MAX_FRAME_CELLS {
                    let mut r = raster::build_raster(board, &grid_task, &frame);
                    let (sources, targets) = terminals::for_task(board, &frame, &grid_task);
                    terminals::mark(&mut r.map, &sources.cells);
                    terminals::mark(&mut r.map, &targets.cells);
                }
                raster_ns.insert(step_um, started.elapsed().as_nanos());
            }
        }
        let started = Instant::now();
        let run = self.0.route_logged(board, task, p, ctx);
        let stage_ns = started.elapsed().as_nanos();
        STAGE_TIMES.with(|t| {
            let mut v = t.get();
            v.connections += 1;
            v.attempts += run.attempts.len();
            v.stage_ns += stage_ns;
            v.emulation_ns += raster_ns.values().sum::<u128>();
            v.raster_ns += run
                .attempts
                .iter()
                .map(|a| raster_ns.get(&a.step_um).copied().unwrap_or(0))
                .sum::<u128>();
            t.set(v);
        });
        run.outcome
    }
}

/// X04.6's question in whole `krt-astar` runs (default settings) of the
/// boards of `X06_SET`: the raster's share of the connection router's
/// time, and the router's share of the run. The raster times are a second
/// build of the same windows on the same board state (module docs of
/// `TimedAstar`); report only.
#[test]
#[ignore = "a release run over a board set; set X06_SET"]
fn stage_time_split_on_a_set() {
    let mut registry = krt_registry();
    registry.connection_router.insert(TIMED_ID, |s| {
        Box::new(TimedAstar(
            freerouting_krt::grid::bundle::KrtAstar::from_settings(s),
        ))
    });
    println!(
        "board\tconnections\tattempts\trun_s\tstage_s\traster_s\traster_share_of_stage\tstage_share_of_run"
    );
    for dsn in set_boards() {
        STAGE_TIMES.with(|t| t.set(StageTimes::default()));
        let prepared = prepare(
            &dsn,
            None,
            &[
                ("autorouter.algorithm", KRT_ASTAR_ID),
                ("autorouter.connection_router", TIMED_ID),
            ],
        );
        let started = Instant::now();
        route_prepared(prepared, Some(&registry));
        let run_ns = started.elapsed().as_nanos();
        let t = STAGE_TIMES.with(|t| t.get());
        // The second raster builds ran outside the stage's stopwatch but
        // inside the run's.
        let run_ns = run_ns.saturating_sub(t.emulation_ns);
        println!(
            "{}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}\t{:.3}\t{:.3}",
            stem(&dsn),
            t.connections,
            t.attempts,
            run_ns as f64 / 1e9,
            t.stage_ns as f64 / 1e9,
            t.raster_ns as f64 / 1e9,
            t.raster_ns as f64 / t.stage_ns.max(1) as f64,
            t.stage_ns as f64 / run_ns.max(1) as f64
        );
    }
}

#[test]
fn the_wall_board_stays_clean_under_the_bundle() {
    // No path exists; the bundle leaves the board as krt-grid does.
    let registry = krt_registry();
    let prepared = prepare(WALL, Some(2), &[("autorouter.algorithm", KRT_ASTAR_ID)]);
    let input = prepared.board.clone();
    let routed = route_prepared(prepared, Some(&registry));
    let g = gates(&input, &routed);
    assert_eq!((g.drc_new, g.via_in_pad), (0, 0));
    assert_eq!(g.airlines, gates(&input, &input).airlines);
    assert_eq!(g.length, gates(&input, &input).length);
}

/// Routes the first connection of `dsn` with `router` as the pass loop
/// does (the changed area marked first, the pass's pull-tight accuracy and
/// trace costs); returns the outcome's work, the trace length and the
/// clearance violations after it.
fn route_one(dsn: &str, astar: bool) -> (u64, f64, usize) {
    use freerouting::autoroute::AttemptState;
    use freerouting_krt::grid::bundle::KrtAstar;
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
        ripup_allowed: true,
        ripup_cost: 0,
        via_cost: 50,
        trace_costs: prepared.board_settings.trace_costs().to_vec(),
        remove_unconnected_vias: false,
        work_budget: maze_work_budget(1),
        pull_tight_accuracy: prepared.board_settings.trace_pull_tight_accuracy(),
        strict_check: false,
    };
    let board = &mut prepared.board;
    board.start_marking_changed_area();
    let run = if astar {
        KrtAstar::new(KrtGrid::default()).route_logged(board, &task, &p, &ctx)
    } else {
        KrtGrid::default().route_logged(board, &task, &p, &ctx)
    };
    assert_eq!(run.outcome.state, AttemptState::Routed, "{dsn}");
    // The bundle's pull-tight consumes the changed area; krt-grid leaves it.
    assert_eq!(board.changed_area().is_none(), astar);
    let length = board
        .items()
        .filter_map(Item::as_trace)
        .map(|t| t.length())
        .sum();
    (
        run.outcome.work.units,
        length,
        board.all_clearance_violations().len(),
    )
}

/// Around the end of a keepout the conservative raster keeps the path
/// farther out than the clearance needs; the bundle's pull-tight takes the
/// slack back.
#[test]
fn the_bundle_pulls_the_grid_path_tight_after_the_connection() {
    let (grid_work, grid_length, grid_drc) = route_one(DETOUR, false);
    let (astar_work, astar_length, astar_drc) = route_one(DETOUR, true);
    println!(
        "krt-grid {grid_length:.0} ({grid_work} units), krt-astar {astar_length:.0} ({astar_work} units)"
    );
    assert_eq!((grid_drc, astar_drc), (0, 0));
    assert!(astar_length < grid_length - 1000.0);
    // The outcome's work includes the pull-tight's units.
    assert!(astar_work > grid_work);
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

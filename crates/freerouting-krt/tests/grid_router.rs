//! X05: the `krt-grid` connection router on synthetic boards, called
//! directly and through the whole pipeline. The boards are in `tests/data/`,
//! not `fixtures/krt/` as the checklist planned: every DSN under `fixtures/`
//! needs a Java reference (`reference_load.rs`), as Q01 and X02 found.
//!
//! - `x05-open-two-pin.dsn`: two SMD pads of net A on F.Cu with nothing
//!   between them; P1's centre is half a 0.1 mm step off the grid in x and
//!   y (X05.3);
//! - `x05-wall.dsn`: the same pads cut off by a wall of net W on both
//!   layers, from board edge to board edge (X05.4, X05.5's frontier);
//! - `x05-fine-pitch.dsn`: pin 3 of a 0.4 mm pitch row of five 0.25 x
//!   0.8 mm pads, its centre half a 0.1 mm step off the grid, to an
//!   on-grid pad (X05.5).
//!
//! The ignored tests are release runs over a board set (X05.4, X05.6).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use freerouting::autoroute::AttemptState;
use freerouting::autoroute::batch::connection::maze_work_budget;
use freerouting::autoroute::batch::ordering::select_items;
use freerouting::autoroute::events::{CancelToken, NullEvents, RunSummary};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, Diagnostics, RouteTask, StageCtx, StageRegistry,
    StageSelection,
};
use freerouting::board::items::{Item, ItemId};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::geometry::IntPoint;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};
use freerouting_krt::grid::raster::Raster;
use freerouting_krt::grid::router::{
    GridRun, KRT_GRID_ID, KrtGrid, Rejection, StepResult, via_in_pad,
};

const OPEN: &str = "crates/freerouting-krt/tests/data/x05-open-two-pin.dsn";
const WALL: &str = "crates/freerouting-krt/tests/data/x05-wall.dsn";
const FINE: &str = "crates/freerouting-krt/tests/data/x05-fine-pitch.dsn";
const PROBE_ID: &str = "x05-probe";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `dsn` (relative to the repository root) with
/// `extra` (`path=value`) on the testing source; with `testing`, the test
/// profile (no optimizer, at most `passes` passes).
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

/// The registry with this crate's stages and the test probe.
fn krt_registry() -> StageRegistry {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    registry
        .diagnostics
        .insert(PROBE_ID, |_| Box::new(Probe::default()));
    registry
}

/// Routes the prepared job with `registry` (`None`: the built-in one).
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
    let name = Path::new(dsn).file_stem().unwrap().to_string_lossy();
    write_ses(board, &name, SesStyle::Java)
}

/// Pass-1 parameters: only the work budget matters to `krt-grid`.
fn params() -> ConnectionParams {
    ConnectionParams {
        pass: 1,
        ripup_allowed: false,
        ripup_cost: 0,
        via_cost: 50,
        trace_costs: Vec::new(),
        remove_unconnected_vias: false,
        work_budget: maze_work_budget(1),
        pull_tight_accuracy: 0,
        strict_check: false,
    }
}

/// The first item of net `net` in the pass-1 order.
fn first_task(prepared: &PreparedJob, net: &str) -> RouteTask {
    let net = net_number(&prepared.board, net);
    select_items(&prepared.board)
        .into_iter()
        .find(|t| t.net == net)
        .expect("an item of the net")
}

/// Routes `task` with `router`; also returns the work units the call put
/// on the board's meter.
fn route_task(prepared: &mut PreparedJob, router: &mut KrtGrid, task: RouteTask) -> (GridRun, u64) {
    let cancel = CancelToken::new();
    let ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    let before = prepared.board.search_trees().work_meter().units();
    let run = router.route_logged(&mut prepared.board, &task, &params(), &ctx);
    let used = prepared.board.search_trees().work_meter().units() - before;
    (run, used)
}

/// Routes the first item of net A with `router`.
fn route_first(prepared: &mut PreparedJob, router: &mut KrtGrid) -> (RouteTask, GridRun) {
    let task = first_task(prepared, "A");
    let (run, _) = route_task(prepared, router, task);
    (task, run)
}

fn new_items(board: &BasicBoard, first_new_id: i32) -> Vec<&Item> {
    board.items().filter(|i| i.id().0 >= first_new_id).collect()
}

fn net_number(board: &BasicBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("net {name}"))
        .net_number
}

/// The pins of `net`, by drill centre.
fn pin_centres(board: &BasicBoard, net: i32) -> BTreeSet<(i32, i32)> {
    board
        .items()
        .filter(|i| i.is_pin() && i.contains_net(net))
        .filter_map(|i| i.drill().map(|d| (d.center.x, d.center.y)))
        .collect()
}

fn clearance_items(board: &BasicBoard, items: &[&Item]) -> usize {
    items
        .iter()
        .map(|i| board.clearance_violations(i.id()).len())
        .sum()
}

// ---------------------------------------------------------------------------
// X05.1: the stage and its selection
// ---------------------------------------------------------------------------

#[test]
fn the_registry_selects_krt_grid_and_reports_an_unknown_id() {
    let registry = krt_registry();
    let selected = prepare(
        OPEN,
        Some(1),
        &[("autorouter.connection_router", KRT_GRID_ID)],
    );
    let selection = StageSelection::resolve(&selected.settings, &registry);
    let pipeline = registry
        .build(&selection, &selected.settings)
        .expect("krt-grid is registered");
    assert_eq!(pipeline.connection_router.id(), KRT_GRID_ID);

    // A10: an unknown per-stage id is an error naming the key and the ids.
    let unknown = prepare(
        OPEN,
        Some(1),
        &[("autorouter.connection_router", "krt-nope")],
    );
    let selection = StageSelection::resolve(&unknown.settings, &registry);
    let Err(error) = registry.build(&selection, &unknown.settings) else {
        panic!("an unknown id is an error");
    };
    assert_eq!(error.key, "router.autorouter.connection_router");
    assert_eq!(error.id, "krt-nope");
    assert!(error.known.contains(&KRT_GRID_ID));
    assert!(error.known.contains(&"freerouting-connection"));

    // Without this crate's registration krt-grid is unknown too.
    let builtin = StageRegistry::builtin();
    let selection = StageSelection::resolve(&selected.settings, &builtin);
    assert!(builtin.build(&selection, &selected.settings).is_err());
}

#[test]
fn registering_krt_grid_changes_no_session_while_it_is_unselected() {
    let registry = krt_registry();
    for dsn in [OPEN, WALL, FINE] {
        let builtin = route_prepared(prepare(dsn, Some(3), &[]), None);
        let krt = route_prepared(prepare(dsn, Some(3), &[]), Some(&registry));
        assert_eq!(ses(&builtin, dsn), ses(&krt, dsn), "{dsn}");
    }
}

/// The stage-off sessions of every board in the set file `X05_SET` (at most
/// `X05_LIMIT` boards), routed with the built-in registry and with this
/// crate's, default settings plus `X05_EXTRA` (`path=value,...`): equal
/// byte for byte. Writes both to `X05_SES_DIR/<stem>.{builtin,krt}.ses` for
/// `freerouting-dev ses-diff`.
#[test]
#[ignore = "a release run over a board set; set X05_SET and X05_SES_DIR"]
fn default_off_sessions_on_a_set() {
    let dir = PathBuf::from(std::env::var("X05_SES_DIR").expect("X05_SES_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let extra = extra_settings();
    let extra: Vec<(&str, &str)> = extra
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let registry = krt_registry();
    for dsn in set_boards() {
        let started = Instant::now();
        let builtin = route_prepared(prepare(&dsn, None, &extra), None);
        let krt = route_prepared(prepare(&dsn, None, &extra), Some(&registry));
        let (a, b) = (ses(&builtin, &dsn), ses(&krt, &dsn));
        let stem = Path::new(&dsn)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        std::fs::write(dir.join(format!("{stem}.builtin.ses")), &a).unwrap();
        std::fs::write(dir.join(format!("{stem}.krt.ses")), &b).unwrap();
        println!(
            "{dsn}: {} ({:.1} s)",
            if a == b { "identical" } else { "DIFFERENT" },
            started.elapsed().as_secs_f64()
        );
        assert_eq!(a, b, "{dsn}");
    }
}

fn set_boards() -> Vec<String> {
    let set = std::env::var("X05_SET").expect("X05_SET");
    let limit: usize = std::env::var("X05_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let text = std::fs::read_to_string(repo_root().join(&set)).unwrap();
    text.lines()
        .map(|l| l.split('#').next().unwrap().trim().to_string())
        .filter(|l| !l.is_empty())
        .take(limit)
        .collect()
}

fn extra_settings() -> Vec<(String, String)> {
    std::env::var("X05_EXTRA")
        .ok()
        .map(|v| {
            v.split(',')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// X05.3: exact terminal connectors
// ---------------------------------------------------------------------------

#[test]
fn both_ends_of_the_open_two_pin_trace_lie_on_the_pin_centres() {
    let mut prepared = prepare(OPEN, Some(1), &[]);
    let (task, run) = route_first(&mut prepared, &mut KrtGrid::default());
    let board = &prepared.board;
    for a in &run.attempts {
        println!("{}", a.line());
    }
    assert_eq!(
        run.outcome.state,
        AttemptState::Routed,
        "{:?}",
        run.attempts
    );
    assert_eq!(run.attempts.len(), 1);
    assert_eq!(run.attempts[0].step_um, 100);
    let new = new_items(board, run.outcome.first_new_id);
    let traces: Vec<_> = new.iter().filter_map(|i| i.as_trace()).collect();
    assert!(!traces.is_empty());
    assert!(
        new.iter().all(|i| !i.is_via()),
        "an open board needs no via"
    );
    // The trace ends: exactly the two pin centres (connection points).
    let mut ends: Vec<(i32, i32)> = traces
        .iter()
        .flat_map(|t| [t.first_corner(), t.last_corner()])
        .filter_map(|p| p.as_int().map(|p| (p.x, p.y)))
        .collect();
    let centres = pin_centres(board, task.net);
    ends.retain(|e| centres.contains(e));
    assert_eq!(ends.iter().copied().collect::<BTreeSet<_>>(), centres);
    // P1 at (10.05, 15.05) mm is half a step off the grid: its connector is
    // one straight segment from the centre to the path's first cell centre,
    // which lies inside P1's pad.
    let p1 = IntPoint::new(100_500, 150_500);
    let connector = traces
        .iter()
        .find_map(|t| {
            let corners: Vec<IntPoint> = (0..t.corner_count())
                .filter_map(|k| t.polyline.corner(k).as_int())
                .collect();
            if corners.first() == Some(&p1) {
                Some(corners[1])
            } else if corners.last() == Some(&p1) {
                Some(corners[corners.len() - 2])
            } else {
                None
            }
        })
        .expect("a trace ends on P1's centre");
    assert_eq!((connector.x % 1000, connector.y % 1000), (0, 0));
    let pad = board
        .items()
        .find(|i| i.drill().is_some_and(|d| d.center == p1))
        .and_then(|i| i.drill())
        .and_then(|d| d.shape_on_layer(0))
        .expect("P1's pad");
    assert!(pad.contains(&freerouting::geometry::Point::from(connector)));
    println!("P1 connector: {p1:?} -> {connector:?}");
    // The DRC is clean and the net is connected.
    assert_eq!(clearance_items(board, &new), 0);
    assert!(board.unconnected_set(task.item, task.net).is_empty());
}

// ---------------------------------------------------------------------------
// X05.4: the transactional insert and the DRC gate
// ---------------------------------------------------------------------------

/// Clears the wall's trace cells on F.Cu along the row of P1's centre (a
/// forced raster bug): the search then runs straight through the wall.
fn clear_the_row_through_the_wall(raster: &mut Raster) {
    let frame = raster.frame;
    let row = freerouting_krt::grid::frame::cell_of(frame.step, IntPoint::new(0, 150_000)).1;
    let cells: Vec<[i32; 3]> = (frame.i0..frame.i0 + frame.nx)
        .filter(|&i| raster.map.is_blocked(i, row, 0))
        .map(|i| [i, row, 0])
        .collect();
    raster.map.remove_blocked_cells_batch(&cells);
}

#[test]
fn the_wall_blocks_every_step_and_the_frontier_names_the_wall() {
    let mut prepared = prepare(WALL, Some(1), &[]);
    let before = prepared.board.state_hash();
    let (_, run) = route_first(&mut prepared, &mut KrtGrid::default());
    for a in &run.attempts {
        println!("{}", a.line());
    }
    assert_eq!(run.outcome.state, AttemptState::Failed);
    assert_eq!(
        run.attempts.iter().map(|a| a.step_um).collect::<Vec<_>>(),
        vec![100, 50, 25]
    );
    assert!(run.attempts.iter().all(|a| a.result == StepResult::NoPath));
    let w = net_number(&prepared.board, "W");
    let failure = run.outcome.failure.as_ref().expect("a failure record");
    assert!(!failure.blocking.is_empty());
    // Sorted by (net, item id, layer), and both wall traces are in it.
    let keys: Vec<_> = failure
        .blocking
        .iter()
        .map(|b| (b.net, b.item.0, b.layer))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    let wall_layers: BTreeSet<i32> = failure
        .blocking
        .iter()
        .filter(|b| b.net == w)
        .filter(|b| prepared.board.item(b.item).is_some_and(Item::is_trace))
        .map(|b| b.layer)
        .collect();
    assert_eq!(wall_layers, BTreeSet::from([0, 1]));
    assert_eq!(prepared.board.state_hash(), before);
}

#[test]
fn a_forced_raster_bug_is_rejected_by_the_drc_gate_and_the_board_is_unchanged() {
    let mut prepared = prepare(WALL, Some(1), &[]);
    let before = prepared.board.state_hash();
    let items_before: Vec<String> = prepared.board.items().map(|i| format!("{i:?}")).collect();
    let mut router = KrtGrid {
        raster_hook: Some(clear_the_row_through_the_wall),
        ..KrtGrid::default()
    };
    let (task, run) = route_first(&mut prepared, &mut router);
    for a in &run.attempts {
        println!("{}", a.line());
    }
    assert_eq!(run.outcome.state, AttemptState::Failed);
    assert_eq!(run.attempts.len(), 3);
    let w = net_number(&prepared.board, "W");
    for attempt in &run.attempts {
        let StepResult::Rejected(Rejection::Clearance { count, first }) = &attempt.result else {
            panic!("the DRC gate rejects the path: {attempt:?}");
        };
        assert!(*count > 0);
        // The violation is between a new trace of A and the wall of W.
        let nets: Vec<i32> = [first.0, first.1]
            .iter()
            .filter_map(|id| prepared.board.item(*id).map(|i| i.net_numbers()[0]))
            .collect();
        assert!(nets.contains(&w), "{first:?}");
    }
    assert_eq!(prepared.board.state_hash(), before);
    let items_after: Vec<String> = prepared.board.items().map(|i| format!("{i:?}")).collect();
    assert_eq!(items_after, items_before);
    assert!(
        !prepared
            .board
            .unconnected_set(task.item, task.net)
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// X05.5: retries, failure record, work units
// ---------------------------------------------------------------------------

#[test]
fn the_fine_pitch_pin_fails_at_0_1_mm_and_a_finer_step_decides() {
    let mut prepared = prepare(FINE, Some(1), &[]);
    let task = first_task(&prepared, "A");
    let (run, used) = route_task(&mut prepared, &mut KrtGrid::default(), task);
    let board = &prepared.board;
    for a in &run.attempts {
        println!("{}", a.line());
        for b in a.blocking.iter().take(8) {
            println!("    blocking {b:?}");
        }
    }
    let first = &run.attempts[0];
    assert_eq!(first.step_um, 100);
    assert!(
        matches!(first.result, StepResult::NoPath),
        "0.1 mm fails: {first:?}"
    );
    // The 0.1 mm frontier names the neighbouring pads of the row.
    let pins: BTreeSet<ItemId> = first
        .blocking
        .iter()
        .filter(|b| board.item(b.item).is_some_and(Item::is_pin))
        .map(|b| b.item)
        .collect();
    assert!(pins.len() >= 2, "{:?}", first.blocking);
    let last = run.attempts.last().unwrap();
    assert!(last.step_um < 100);
    match &last.result {
        StepResult::Routed { .. } => {
            assert_eq!(run.outcome.state, AttemptState::Routed);
            let new = new_items(board, run.outcome.first_new_id);
            assert_eq!(clearance_items(board, &new), 0);
            assert!(board.unconnected_set(task.item, task.net).is_empty());
        }
        StepResult::NoPath => {
            assert!(
                last.blocking
                    .iter()
                    .any(|b| board.item(b.item).is_some_and(Item::is_pin))
            );
        }
        other => panic!("a finer step routes or fails with a frontier: {other:?}"),
    }
    // Work: every grid iteration is a unit on the board's meter.
    let iterations: u64 = run.attempts.iter().map(|a| u64::from(a.iterations)).sum();
    assert_eq!(run.outcome.work.units, used);
    println!("work units {used}, grid iterations {iterations}");
    assert!(used >= iterations, "{used} < {iterations}");
}

#[test]
fn the_work_of_each_connection_sums_into_the_meter() {
    let mut prepared = prepare(WALL, Some(1), &[]);
    let tasks = select_items(&prepared.board);
    let meter_before = prepared.board.search_trees().work_meter().units();
    let cancel = CancelToken::new();
    let ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    let mut router = KrtGrid::default();
    let mut total = 0;
    for task in &tasks {
        let run = router.route_logged(&mut prepared.board, task, &params(), &ctx);
        total += run.outcome.work.units;
    }
    assert!(tasks.len() >= 2);
    let used = prepared.board.search_trees().work_meter().units() - meter_before;
    assert_eq!(total, used);
}

// ---------------------------------------------------------------------------
// The pipeline with krt-grid selected
// ---------------------------------------------------------------------------

#[test]
fn the_pipeline_routes_the_fixtures_with_krt_grid_cleanly() {
    let registry = krt_registry();
    for dsn in [OPEN, FINE] {
        let prepared = prepare(
            dsn,
            Some(3),
            &[("autorouter.connection_router", KRT_GRID_ID)],
        );
        let input = prepared.board.clone();
        let board = route_prepared(prepared, Some(&registry));
        let a = net_number(&board, "A");
        let pins: Vec<&Item> = board
            .items()
            .filter(|i| i.is_pin() && i.contains_net(a))
            .collect();
        assert!(board.unconnected_set(pins[0].id(), a).is_empty(), "{dsn}");
        let all: Vec<&Item> = board.items().collect();
        let input_items: Vec<&Item> = input.items().collect();
        assert_eq!(
            clearance_items(&board, &all),
            clearance_items(&input, &input_items),
            "{dsn}"
        );
        assert!(
            board
                .items()
                .filter(|i| i.is_via())
                .all(|v| !via_in_pad(&board, v))
        );
    }
}

// ---------------------------------------------------------------------------
// X05.4 and X05.6 on a board set: the connection-level probe
// ---------------------------------------------------------------------------

/// One routed connection as the probe saw it.
#[derive(Clone, Debug)]
struct ConnRecord {
    router: String,
    state: AttemptState,
    nanos: u128,
    drc_items: usize,
    vias_in_pads: usize,
    details: String,
}

thread_local! {
    static RECORDS: RefCell<Vec<ConnRecord>> = const { RefCell::new(Vec::new()) };
}

/// A report-only diagnostic: times each connection and, for a routed one,
/// counts the clearance violations and vias in pads of its new items.
#[derive(Default)]
struct Probe {
    started: Option<Instant>,
}

impl Diagnostics for Probe {
    fn id(&self) -> &'static str {
        PROBE_ID
    }

    fn before_connection(
        &mut self,
        _: &RoutingBoard,
        _: &RouteTask,
        _: &ConnectionParams,
        _: &StageCtx,
    ) {
        self.started = Some(Instant::now());
    }

    fn after_connection(
        &mut self,
        board: &RoutingBoard,
        _: &RouteTask,
        _: &ConnectionParams,
        outcome: &ConnectionOutcome,
        ctx: &StageCtx,
    ) {
        let nanos = self.started.take().map_or(0, |t| t.elapsed().as_nanos());
        let (mut drc_items, mut vias_in_pads) = (0, 0);
        if outcome.state == AttemptState::Routed {
            let new = new_items(board, outcome.first_new_id);
            drc_items = clearance_items(board, &new);
            vias_in_pads = new
                .iter()
                .filter(|i| i.is_via() && via_in_pad(board, i))
                .count();
        }
        let router = ctx
            .settings
            .autorouter
            .connection_router
            .clone()
            .unwrap_or_else(|| "freerouting-connection".to_string());
        RECORDS.with(|r| {
            r.borrow_mut().push(ConnRecord {
                router,
                state: outcome.state,
                nanos,
                drc_items,
                vias_in_pads,
                details: outcome.details.clone(),
            })
        });
    }

    fn finish(&mut self, _: &RoutingBoard, _: &StageCtx, _: &mut RunSummary) {}
}

fn median_ms(mut nanos: Vec<u128>) -> f64 {
    if nanos.is_empty() {
        return 0.0;
    }
    nanos.sort_unstable();
    nanos[nanos.len() / 2] as f64 / 1e6
}

/// Pass 1 (no optimizer) of every board in `X05_SET` with the Freerouting
/// connection router and with `krt-grid`: per board the connections
/// attempted and routed, the median wall time per connection, and for
/// `krt-grid` the clearance violations and vias in pads of the new items of
/// each routed connection, which must be 0.
#[test]
#[ignore = "a release run over a board set; set X05_SET"]
fn connection_level_comparison_on_a_set() {
    let registry = krt_registry();
    println!("board | router | attempted | routed | median ms | drc | via-in-pad");
    for dsn in set_boards() {
        for router in ["freerouting-connection", KRT_GRID_ID] {
            RECORDS.with(|r| r.borrow_mut().clear());
            let extra = [
                ("autorouter.connection_router", router),
                ("special.diagnostics", PROBE_ID),
            ];
            route_prepared(prepare(&dsn, Some(1), &extra), Some(&registry));
            let records = RECORDS.with(|r| r.borrow().clone());
            assert!(records.iter().all(|r| r.router == router));
            let attempted = records
                .iter()
                .filter(|r| {
                    matches!(
                        r.state,
                        AttemptState::Routed | AttemptState::Failed | AttemptState::InsertError
                    )
                })
                .count();
            let routed = records
                .iter()
                .filter(|r| r.state == AttemptState::Routed)
                .count();
            let drc: usize = records.iter().map(|r| r.drc_items).sum();
            let vip: usize = records.iter().map(|r| r.vias_in_pads).sum();
            let median = median_ms(records.iter().map(|r| r.nanos).collect());
            let stem = Path::new(&dsn)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .to_string();
            println!("{stem} | {router} | {attempted} | {routed} | {median:.3} | {drc} | {vip}");
            if std::env::var_os("X05_VERBOSE").is_some() {
                for r in records.iter().filter(|r| r.state == AttemptState::Failed) {
                    println!("    failed: {}", r.details);
                }
            }
            if router == KRT_GRID_ID {
                assert_eq!((drc, vip), (0, 0), "{dsn}");
            }
        }
    }
}

/// The sessions of every board in `X05_SET` routed with `krt-grid` (default
/// settings, `X05_EXTRA` on top), written to `X05_SES_OUT/<stem>.ses`, for
/// `tools/determinism-check.sh`.
#[test]
#[ignore = "for tools/determinism-check.sh; set X05_SET and X05_SES_OUT"]
fn write_krt_grid_sessions() {
    let out = PathBuf::from(std::env::var("X05_SES_OUT").expect("X05_SES_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let mut extra = extra_settings();
    extra.push(("autorouter.connection_router".into(), KRT_GRID_ID.into()));
    let extra: Vec<(&str, &str)> = extra
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let registry = krt_registry();
    for dsn in set_boards() {
        let board = route_prepared(prepare(&dsn, None, &extra), Some(&registry));
        let stem = Path::new(&dsn)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        std::fs::write(out.join(format!("{stem}.ses")), ses(&board, &dsn)).unwrap();
    }
}

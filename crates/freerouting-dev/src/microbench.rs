//! `freerouting-dev microbench` (P01.3): the expected hot kernels, each with
//! a fixed amount of work.
//!
//! A kernel is run `runs` times (9 by default) on one board. Each run does
//! exactly the same work, and the harness checks it: the **work counts**
//! (search-tree nodes visited, queries, results, queue pops, ...) must be
//! identical in every run, and only the time may change. The report gives
//! the minimum time over the runs, its spread, and the work counts. A change
//! that claims to keep results identical must not move the work counts
//! (`tests/microbench.rs` pins them on two fixtures); the time is what an
//! optimization moves. This is KiCadRoutingTools' `microbench.py` rule: the
//! printed work must be identical across binaries.
//!
//! The kernels:
//! - `overlap`: seeded overlap queries on the board's default search tree
//!   ([`OVERLAP_QUERIES`] boxes of the size a trace segment has, all layers
//!   of the board);
//! - `complete_shape`: [`COMPLETE_QUERIES`] room completions on the
//!   autoroute tree of the smallest trace clearance class (45° or 90° as the
//!   board's rule says), seeded as the `board-complete-shape` probe draws
//!   them;
//! - `maze`: [`CONNECTIONS`] connections routed one after the other by the
//!   maze search (`route_connection`, ripup on, the pass-1 work limit);
//! - `pull_tight`: the whole-board pull-tight (`opt_changed_area`, accuracy
//!   500) of the board after [`LAID_CONNECTIONS`] connections were laid
//!   (untimed);
//! - `drc`: [`DRC_REPEATS`] full clearance checks of that board.
//!
//! The setup of a kernel (loading, choosing the connections, cloning the
//! board, laying the traces for the last two) is not timed. The queries are
//! drawn with `JavaRandom`, so they are the same on every machine.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

use freerouting::autoroute::connection::{Ripped, RouteLimits, route_connection};
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::path::RipupCosts;
use freerouting::autoroute::session::search::SearchSettings;
use freerouting::autoroute::session::sink::WorkCounters;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::search::WorkLimit;
use freerouting::board::search::complete_shape::CompletionScratch;
use freerouting::board::search::rooms::IncompleteRoom;
use freerouting::drc::DesignRulesChecker;
use freerouting::drc::json::JsonValue;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{IntBox, TileShape};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::optimize::PullTightParams;
use freerouting::settings::board::BoardRouterSettings;
use freerouting::settings::{ResolveContext, RouterSettings};

/// Overlap queries per run of the `overlap` kernel.
pub const OVERLAP_QUERIES: usize = 20_000;
/// Room completions per run of the `complete_shape` kernel.
pub const COMPLETE_QUERIES: usize = 4_000;
/// Connections routed by `maze`.
pub const CONNECTIONS: usize = 8;
/// Connections laid, untimed, before `pull_tight` and `drc`, so the board
/// has traces to tighten and check.
pub const LAID_CONNECTIONS: usize = 40;
/// The seed of the query draws.
pub const SEED: i64 = 0x5eed_0001;
/// The maze search's work limit at ripup pass 1, in nominal milliseconds
/// (`AutorouteConnectionRouter.java:83-89`).
const MAZE_LIMIT_MS: i64 = 100_000;
/// Clearance checks per run of the `drc` kernel (one check takes about a
/// millisecond on a board of this size).
pub const DRC_REPEATS: usize = 20;
/// The pull-tight accuracy of the autorouter (`opt_changed_area`).
const PULL_TIGHT_ACCURACY: i32 = 500;

/// The kernel names, in report order.
pub const KERNELS: [&str; 5] = ["overlap", "complete_shape", "maze", "pull_tight", "drc"];

/// One kernel's result.
#[derive(Clone, Debug, PartialEq)]
pub struct KernelReport {
    pub kernel: &'static str,
    /// The work counts, identical in every run.
    pub work: Vec<(&'static str, u64)>,
    /// The fastest run.
    pub min: Duration,
    /// The slowest run.
    pub max: Duration,
    pub runs: usize,
}

impl KernelReport {
    /// (max - min) / min, the spread of the runs.
    pub fn spread(&self) -> f64 {
        let min = self.min.as_secs_f64();
        if min == 0.0 {
            0.0
        } else {
            (self.max.as_secs_f64() - min) / min
        }
    }

    /// One work count by name.
    pub fn count(&self, name: &str) -> Option<u64> {
        self.work.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }
}

/// A board loaded as a job loads it.
pub struct Loaded {
    pub board: RoutingBoard,
    pub settings: RouterSettings,
    pub router: BoardRouterSettings,
}

/// Loads `dsn` as `prepare_job` does (the default settings and the DSN's).
pub fn load(dsn: &Path) -> Result<Loaded, String> {
    let bytes = std::fs::read(dsn).map_err(|e| format!("{}: {e}", dsn.display()))?;
    let file_name = dsn.file_name().and_then(|n| n.to_str()).unwrap_or("board");
    let mut input = JobInput::new(bytes, JobInput::design_name_of(file_name));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = dsn.parent().map(Path::to_path_buf);
    let prepared = prepare_job(&input, &ResolveContext { cores: 8 })
        .map_err(|e| format!("{}: {e}", dsn.display()))?;
    Ok(Loaded {
        board: prepared.board,
        settings: prepared.settings,
        router: prepared.board_settings,
    })
}

/// Runs `body` `runs` times. `body` does its untimed setup, times the work
/// itself and returns the duration with the work counts; the counts must be
/// the same every time.
fn measure(
    kernel: &'static str,
    runs: usize,
    mut body: impl FnMut() -> (Duration, Vec<(&'static str, u64)>),
) -> Result<KernelReport, String> {
    let mut work: Option<Vec<(&'static str, u64)>> = None;
    let (mut min, mut max) = (Duration::MAX, Duration::ZERO);
    for run in 0..runs.max(1) {
        let (elapsed, counts) = body();
        match &work {
            None => work = Some(counts),
            Some(first) if *first != counts => {
                return Err(format!(
                    "{kernel}: the work changed between runs (run 1 {first:?}, run {} {counts:?})",
                    run + 1
                ));
            }
            Some(_) => {}
        }
        min = min.min(elapsed);
        max = max.max(elapsed);
    }
    Ok(KernelReport {
        kernel,
        work: work.unwrap_or_default(),
        min,
        max,
        runs: runs.max(1),
    })
}

/// A square of half-size `h` around `(x, y)` as an octagon (Java
/// `new IntBox(x - h, y - h, x + h, y + h)`).
fn square(x: i32, y: i32, h: i32) -> TileShape {
    let b = IntBox::new(
        x.wrapping_sub(h),
        y.wrapping_sub(h),
        x.wrapping_add(h),
        y.wrapping_add(h),
    );
    TileShape::IntOctagon(b.to_int_octagon())
}

/// The seeded queries: a centre inside the board box, a half-size and a
/// layer.
struct Draw {
    random: JavaRandom,
    bounds: IntBox,
    layers: i32,
    max_net: i32,
}

struct Sample {
    x: i32,
    y: i32,
    h: i32,
    layer: i32,
    net: i32,
    bounded: bool,
}

impl Draw {
    fn new(board: &RoutingBoard) -> Draw {
        Draw {
            random: JavaRandom::new(SEED),
            bounds: board.bounding_box(),
            layers: i32::try_from(board.layer_count())
                .unwrap_or(i32::MAX)
                .max(1),
            max_net: board.rules.nets.max_net_number(),
        }
    }

    fn next(&mut self) -> Sample {
        let width = self.bounds.ur.x - self.bounds.ll.x;
        let height = self.bounds.ur.y - self.bounds.ll.y;
        let layer = self.random.next_int(self.layers);
        let x = self.bounds.ll.x + self.random.next_int(width + 1);
        let y = self.bounds.ll.y + self.random.next_int(height + 1);
        let h = 1 + self.random.next_int((width.min(height) / 50).max(1));
        let net = self.random.next_int(self.max_net + 1);
        let bounded = self.random.next_boolean();
        Sample {
            x,
            y,
            h,
            layer,
            net,
            bounded,
        }
    }
}

// The timed bodies of the kernels are separate functions that are never
// inlined, so `tools/bench/callgrind.sh` can count the instructions of the
// kernel alone (`--toggle-collect=*microbench::timed_<kernel>*`).

/// The loop of the `overlap` kernel: the entries found.
#[inline(never)]
fn timed_overlap(board: &RoutingBoard, queries: &[(TileShape, i32)]) -> u64 {
    let mut entries = 0u64;
    for (shape, layer) in queries {
        entries += board.overlapping_tree_entries(shape, *layer).len() as u64;
    }
    entries
}

/// The loop of the `complete_shape` kernel: the rooms found.
#[inline(never)]
fn timed_complete_shape(
    board: &RoutingBoard,
    class: i32,
    rooms: &[(IncompleteRoom, i32)],
    scratch: &mut CompletionScratch,
) -> u64 {
    let mut found = 0u64;
    for (room, net) in rooms {
        found += board
            .complete_shape(class, room, *net, None, None, scratch)
            .len() as u64;
    }
    found
}

/// One connection of the `maze` kernel.
#[inline(never)]
fn timed_maze(
    board: &mut RoutingBoard,
    c: &Connection,
    control: &AutorouteControl,
    limits: RouteLimits<'_>,
    ripped: &mut BTreeSet<ItemId>,
    costs: &mut RipupCosts,
) -> (freerouting::autoroute::AttemptResult, WorkCounters) {
    route_connection(
        board,
        &c.start,
        &c.dest,
        control,
        limits,
        Ripped {
            items: ripped,
            costs: Some(costs),
        },
        WorkCounters::default(),
    )
}

/// The `pull_tight` kernel.
#[inline(never)]
fn timed_pull_tight(board: &mut RoutingBoard) {
    board.opt_changed_area(&PullTightParams::new(PULL_TIGHT_ACCURACY));
}

/// The `drc` kernel: the violations found by all the checks.
#[inline(never)]
fn timed_drc(board: &RoutingBoard) -> u64 {
    let mut violations = 0;
    for _ in 0..DRC_REPEATS {
        violations += DesignRulesChecker::new(board)
            .all_clearance_violations()
            .len() as u64;
    }
    violations
}

fn units_of(board: &RoutingBoard) -> u64 {
    board.search_trees().work_meter().units()
}

fn overlap(loaded: &Loaded, runs: usize) -> Result<KernelReport, String> {
    let board = &loaded.board;
    let mut draw = Draw::new(board);
    let queries: Vec<(TileShape, i32)> = (0..OVERLAP_QUERIES)
        .map(|_| {
            let s = draw.next();
            (square(s.x, s.y, s.h), s.layer)
        })
        .collect();
    measure("overlap", runs, || {
        let before = units_of(board);
        let mut entries = 0u64;
        let started = Instant::now();
        entries += timed_overlap(board, &queries);
        let elapsed = started.elapsed();
        (
            elapsed,
            vec![
                ("queries", queries.len() as u64),
                ("tree_nodes_visited", units_of(board) - before),
                ("entries_found", entries),
            ],
        )
    })
}

/// The smallest non-zero trace clearance class of the net classes, else 0
/// (the class the probe completes rooms on).
fn tree_class(board: &RoutingBoard) -> i32 {
    let classes = &board.rules.net_classes;
    (0..classes.count())
        .filter_map(|i| classes.nth(i))
        .map(|id| classes.get(id).trace_clearance_class())
        .filter(|&c| c > 0)
        .min()
        .unwrap_or(0)
}

fn complete_shape(loaded: &Loaded, runs: usize) -> Result<KernelReport, String> {
    let mut board = loaded.board.clone();
    let class = tree_class(&board);
    // Creates and fills the tree; part of the setup.
    board.autoroute_search_tree(class);
    let mut draw = Draw::new(&board);
    let rooms: Vec<(IncompleteRoom, i32)> = (0..COMPLETE_QUERIES)
        .map(|_| {
            let s = draw.next();
            let start = s.bounded.then(|| square(s.x, s.y, s.h.wrapping_mul(20)));
            (
                IncompleteRoom::new(start, s.layer, Some(square(s.x, s.y, s.h))),
                s.net,
            )
        })
        .collect();
    let mut scratch = CompletionScratch::new();
    measure("complete_shape", runs, || {
        let before = units_of(&board);
        let mut found = 0u64;
        let started = Instant::now();
        found += timed_complete_shape(&board, class, &rooms, &mut scratch);
        let elapsed = started.elapsed();
        (
            elapsed,
            vec![
                ("queries", rooms.len() as u64),
                ("tree_nodes_visited", units_of(&board) - before),
                ("rooms_found", found),
            ],
        )
    })
}

/// One connection to route: its net and the route sets.
struct Connection {
    net: i32,
    start: BTreeSet<ItemId>,
    dest: BTreeSet<ItemId>,
}

/// The first `limit` connections of nets without a plane, in net
/// order (the first connectable item, highest id first, that has an
/// unconnected set and no ignored nets).
fn connections(board: &RoutingBoard, limit: usize) -> Vec<Connection> {
    let mut result = Vec::new();
    for net in 1..=board.rules.nets.max_net_number() {
        if result.len() >= limit {
            break;
        }
        if board
            .rules
            .nets
            .get(net)
            .is_some_and(|n| n.contains_plane())
        {
            continue;
        }
        let chosen = board.connectable_items(net).into_iter().find(|id| {
            board
                .item(*id)
                .is_some_and(|item| !board.has_ignored_nets(item))
                && !board.unconnected_set(*id, net).is_empty()
        });
        if let Some(item) = chosen {
            result.push(Connection {
                net,
                start: board.unconnected_set(item, net),
                dest: board.connected_set(item, net, false),
            });
        }
    }
    result
}

/// Counts of one routing of the connections.
#[derive(Default)]
struct Routed {
    routed: u64,
    failed: u64,
    search: WorkCounters,
}

/// Routes `connections` on `board` in order, as the batch router's first
/// pass does (ripup on); returns the time spent and the counts.
fn route_all(
    loaded: &Loaded,
    board: &mut RoutingBoard,
    connections: &[Connection],
) -> (Duration, Routed) {
    let mut total = Routed::default();
    let mut elapsed = Duration::ZERO;
    for c in connections {
        let Ok(mut control) = AutorouteControl::for_net(
            board,
            c.net,
            &loaded.router,
            loaded.router.via_costs(),
            loaded.router.trace_costs(),
        ) else {
            total.failed += 1;
            continue;
        };
        control.ripup_allowed = true;
        control.ripup_costs = loaded.router.start_ripup_costs();
        control.remove_unconnected_vias = !loaded.settings.fanout.enabled;
        let mut ripped: BTreeSet<ItemId> = BTreeSet::new();
        let mut costs = RipupCosts::new();
        let meter = board.search_trees().work_meter();
        let limit = WorkLimit::with_site(meter, MAZE_LIMIT_MS, "maze_search");
        let limits = RouteLimits {
            settings: SearchSettings::from_router(&loaded.router),
            stop: None,
            work_limit: Some(limit),
        };
        let started = Instant::now();
        let (result, sink) = timed_maze(board, c, &control, limits, &mut ripped, &mut costs);
        elapsed += started.elapsed();
        if result.state == freerouting::autoroute::AttemptState::Routed {
            total.routed += 1;
        } else {
            total.failed += 1;
        }
        let s = &mut total.search;
        s.pops += sink.pops;
        s.pushes += sink.pushes;
        s.dropped += sink.dropped;
        s.rooms_completed += sink.rooms_completed;
        s.doors_expanded += sink.doors_expanded;
        s.work_units += sink.work_units;
    }
    (elapsed, total)
}

fn maze(loaded: &Loaded, runs: usize) -> Result<KernelReport, String> {
    let conns = connections(&loaded.board, CONNECTIONS);
    measure("maze", runs, || {
        let mut board = loaded.board.clone();
        let before = units_of(&board);
        let (elapsed, total) = route_all(loaded, &mut board, &conns);
        let s = &total.search;
        (
            elapsed,
            vec![
                ("connections", conns.len() as u64),
                ("routed", total.routed),
                ("queue_pops", s.pops),
                ("queue_pushes", s.pushes),
                ("rooms_completed", s.rooms_completed),
                ("doors_expanded", s.doors_expanded),
                ("maze_work_units", s.work_units),
                ("tree_nodes_visited", units_of(&board) - before),
            ],
        )
    })
}

/// The traces and corners of the board.
fn trace_figures(board: &RoutingBoard) -> (u64, u64) {
    let mut traces = 0;
    let mut corners = 0;
    for item in board.items() {
        if let Some(trace) = item.as_trace() {
            traces += 1;
            corners += trace.polyline.corner_count() as u64;
        }
    }
    (traces, corners)
}

fn pull_tight(loaded: &Loaded, runs: usize) -> Result<KernelReport, String> {
    let conns = connections(&loaded.board, LAID_CONNECTIONS);
    // The board with the traces laid, once.
    let mut laid = loaded.board.clone();
    route_all(loaded, &mut laid, &conns);
    let (traces_before, corners_before) = trace_figures(&laid);
    measure("pull_tight", runs, || {
        let mut board = laid.clone();
        let before = units_of(&board);
        board.start_marking_changed_area();
        board.mark_all_changed_area();
        let started = Instant::now();
        timed_pull_tight(&mut board);
        let elapsed = started.elapsed();
        let (traces, corners) = trace_figures(&board);
        (
            elapsed,
            vec![
                ("traces_before", traces_before),
                ("corners_before", corners_before),
                ("tree_nodes_visited", units_of(&board) - before),
                ("traces_after", traces),
                ("corners_after", corners),
            ],
        )
    })
}

fn drc(loaded: &Loaded, runs: usize) -> Result<KernelReport, String> {
    let conns = connections(&loaded.board, LAID_CONNECTIONS);
    let mut laid = loaded.board.clone();
    route_all(loaded, &mut laid, &conns);
    let items = laid.items().count() as u64;
    measure("drc", runs, || {
        let before = units_of(&laid);
        let started = Instant::now();
        let violations = timed_drc(&laid);
        let elapsed = started.elapsed();
        (
            elapsed,
            vec![
                ("checks", DRC_REPEATS as u64),
                ("items", items),
                ("tree_nodes_visited", units_of(&laid) - before),
                ("violations", violations),
            ],
        )
    })
}

/// Runs the kernels named in `only` (all of them when empty) on `loaded`.
pub fn run_kernels(
    loaded: &Loaded,
    runs: usize,
    only: &[String],
) -> Result<Vec<KernelReport>, String> {
    if let Some(unknown) = only.iter().find(|k| !KERNELS.contains(&k.as_str())) {
        return Err(format!(
            "unknown kernel '{unknown}' (use {})",
            KERNELS.join(", ")
        ));
    }
    let wanted = |name: &str| only.is_empty() || only.iter().any(|k| k == name);
    let mut reports = Vec::new();
    if wanted("overlap") {
        reports.push(overlap(loaded, runs)?);
    }
    if wanted("complete_shape") {
        reports.push(complete_shape(loaded, runs)?);
    }
    if wanted("maze") {
        reports.push(maze(loaded, runs)?);
    }
    if wanted("pull_tight") {
        reports.push(pull_tight(loaded, runs)?);
    }
    if wanted("drc") {
        reports.push(drc(loaded, runs)?);
    }
    Ok(reports)
}

/// The 1-minute load average, if the system reports it.
pub fn load_average() -> Option<f64> {
    std::fs::read_to_string("/proc/loadavg")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// The text table of a run.
pub fn to_text(board: &str, reports: &[KernelReport], load: Option<f64>) -> String {
    let mut out = format!(
        "microbench {board}  (1-minute load average {})\n",
        load.map_or("unknown".to_string(), |l| format!("{l:.2}"))
    );
    for r in reports {
        out.push_str(&format!(
            "{:<15} min {:>10.3} ms  spread {:>5.1} %  runs {}\n",
            r.kernel,
            r.min.as_secs_f64() * 1e3,
            r.spread() * 100.0,
            r.runs
        ));
        for (name, value) in &r.work {
            out.push_str(&format!("    {name:<20} {value}\n"));
        }
    }
    out
}

/// The JSON of a run.
pub fn to_json(board: &str, reports: &[KernelReport], load: Option<f64>) -> JsonValue {
    let number = |v: f64| JsonValue::Number(format!("{v:.6}"));
    JsonValue::Object(vec![
        ("board".to_string(), JsonValue::string(board)),
        (
            "load_average".to_string(),
            load.map_or(JsonValue::Null, number),
        ),
        (
            "kernels".to_string(),
            JsonValue::Array(
                reports
                    .iter()
                    .map(|r| {
                        JsonValue::Object(vec![
                            ("kernel".to_string(), JsonValue::string(r.kernel)),
                            ("min_ms".to_string(), number(r.min.as_secs_f64() * 1e3)),
                            ("max_ms".to_string(), number(r.max.as_secs_f64() * 1e3)),
                            ("spread".to_string(), number(r.spread())),
                            ("runs".to_string(), JsonValue::int(r.runs as i64)),
                            (
                                "work".to_string(),
                                JsonValue::Object(
                                    r.work
                                        .iter()
                                        .map(|(n, v)| {
                                            ((*n).to_string(), JsonValue::Number(v.to_string()))
                                        })
                                        .collect(),
                                ),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_that_changes_between_runs_is_an_error() {
        let mut calls = 0u64;
        let result = measure("k", 3, || {
            calls += 1;
            (Duration::from_millis(1), vec![("x", calls)])
        });
        assert!(result.is_err_and(|e| e.contains("the work changed between runs")));
    }

    #[test]
    fn the_minimum_and_the_spread_come_from_the_runs() {
        let mut times = [30u64, 10, 20].into_iter();
        let report = measure("k", 3, || {
            let ms = times.next().unwrap();
            (Duration::from_millis(ms), vec![("x", 7)])
        })
        .unwrap();
        assert_eq!(report.min, Duration::from_millis(10));
        assert_eq!(report.count("x"), Some(7));
        assert!((report.spread() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn an_unknown_kernel_is_refused() {
        let loaded = load(Path::new("../../fixtures/Issue413-test.dsn")).unwrap();
        let error = run_kernels(&loaded, 1, &["nope".to_string()]).unwrap_err();
        assert!(error.contains("unknown kernel 'nope'"), "{error}");
    }
}

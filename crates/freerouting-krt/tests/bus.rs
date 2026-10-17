//! X11: bus detection, the `krt-bus` middle-out ordering and the
//! `krt-bus-attraction` cost.
//!
//! Board (synthetic, in `tests/data/`, written by X11; not under
//! `fixtures/`, where every DSN needs a Java reference):
//! - `x11-bus8.dsn`: nets D0–D7 from pins 1–8 of a 10-pin 0.5 mm pitch
//!   connector J1 to a 6-pin J2 (D0–D5) and a 2-pin J3 (D6, D7) placed end
//!   to end 32 mm to the right and 5 mm lower, past a keepout on both
//!   layers; J1 pins 9 and 10 (nets X1, X2) go to U2, 7 mm away. Width and
//!   clearance 0.25 mm, so one pitch is 0.5 mm.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{NetOrdering, StageRegistry, StageSelection};
use freerouting::board::items::ItemId;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::datastructures::IdGenerator;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_krt::bus::{
    AttractionParams, BUS_ATTRACTION_ID, BUS_BUNDLE_ID, BUS_ORDERING_ID, BusParams,
    KrtBusAttractionRouter, KrtBusOrdering, detect, routing_order, spacing,
};
use freerouting_krt::grid::router::via_in_pad;

const BUS8: &str = "crates/freerouting-krt/tests/data/x11-bus8.dsn";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `bytes`, with `extra` (`path=value`) on the
/// testing source.
fn prepare_bytes(bytes: Vec<u8>, dsn: &str, extra: &[(&str, &str)]) -> PreparedJob {
    let path = repo_root().join(dsn);
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    if !extra.is_empty() {
        // An empty patch, not `TestingProfile::default()` (Java's fixture
        // test settings): every arm runs the defaults plus its keys.
        let mut patch = SettingsPatch::default();
        for (key, value) in extra {
            patch
                .set_path(key, value)
                .unwrap_or_else(|e| panic!("{key}: {e:?}"));
        }
        input.extra_sources = vec![(Source::Testing, patch)];
    }
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn prepare(dsn: &str, extra: &[(&str, &str)]) -> PreparedJob {
    let bytes = std::fs::read(repo_root().join(dsn)).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    prepare_bytes(bytes, dsn, extra)
}

fn net_name(board: &BasicBoard, net: i32) -> String {
    board.rules.nets.get(net).expect("net").name.clone()
}

fn names(board: &BasicBoard, nets: &[i32]) -> Vec<String> {
    nets.iter().map(|n| net_name(board, *n)).collect()
}

fn krt_registry() -> StageRegistry {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    registry
}

// ---------------------------------------------------------------------
// X11.1: detection
// ---------------------------------------------------------------------

#[test]
fn the_bus_of_eight_is_one_group_and_the_unrelated_nets_stay_out() {
    let prepared = prepare(BUS8, &[]);
    let board: &BasicBoard = &prepared.board;
    let params = BusParams::for_board(board);
    assert_eq!(params.radius, 50_000, "5 mm at 10 units per µm");
    let groups = detect(board, &params);
    assert_eq!(groups.len(), 1, "{groups:?}");
    // Physical order: along J1's pins, bottom (D7, y 10.75 mm) to top.
    assert_eq!(
        names(board, &groups[0].nets()),
        ["D7", "D6", "D5", "D4", "D3", "D2", "D1", "D0"]
    );
    // Every member is oriented to the seed D0: all start on one side.
    let j1 = board.components.get_by_name("J1").expect("J1").id.0;
    let on_j1 = |id| board.item(id).expect("pin").component_id() == j1;
    let seed_on_j1 = on_j1(groups[0].members[7].start_pin);
    for m in &groups[0].members {
        assert_eq!(on_j1(m.start_pin), seed_on_j1, "{}", net_name(board, m.net));
        assert_ne!(on_j1(m.end_pin), seed_on_j1, "{}", net_name(board, m.net));
    }
}

#[test]
fn moving_j3_ten_mm_away_takes_its_two_nets_out_and_splits_nothing_else() {
    let text = std::fs::read_to_string(repo_root().join(BUS8)).unwrap();
    let moved = text.replace(
        "(place J3 36000 6000 front 0)",
        "(place J3 46000 6000 front 0)",
    );
    assert_ne!(moved, text);
    let prepared = prepare_bytes(moved.into_bytes(), BUS8, &[]);
    let board: &BasicBoard = &prepared.board;
    let groups = detect(board, &BusParams::for_board(board));
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(
        names(board, &groups[0].nets()),
        ["D5", "D4", "D3", "D2", "D1", "D0"]
    );
}

// ---------------------------------------------------------------------
// X11.2: middle-out ordering
// ---------------------------------------------------------------------

/// The nets of `tasks` in order, consecutive repeats merged.
fn task_nets(
    board: &BasicBoard,
    tasks: &[freerouting::autoroute::stages::RouteTask],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tasks {
        let name = net_name(board, t.net);
        if out.last() != Some(&name) {
            out.push(name);
        }
    }
    out
}

#[test]
fn the_pass_1_order_is_middle_out_then_the_other_nets() {
    let prepared = prepare(BUS8, &[]);
    let board: &RoutingBoard = &prepared.board;
    let mut ordering = KrtBusOrdering::new();
    let tasks = ordering.select(board);
    let groups = ordering.groups(board);
    // Hand-derived: physical D7 D6 D5 D4 D3 D2 D1 D0 (indices 0..8), the
    // middle n / 2 = 4 (D3), then 3, 5, 2, 6, 1, 7, 0.
    assert_eq!(
        names(board, &routing_order(&groups[0])),
        ["D3", "D4", "D2", "D5", "D1", "D6", "D0", "D7"]
    );
    let nets = task_nets(board, &tasks);
    assert_eq!(
        nets,
        [
            "D3", "D4", "D2", "D5", "D1", "D6", "D0", "D7", "X2", "X1", "X2", "X1"
        ],
        "the bus as one block, then Freerouting's order (highest item id first: U2's pins, then J1's)"
    );
    // Every task of Freerouting's list is there once.
    let mut ours: Vec<ItemId> = tasks.iter().map(|t| t.item).collect();
    let mut theirs: Vec<ItemId> = freerouting::autoroute::batch::ordering::select_items(board)
        .iter()
        .map(|t| t.item)
        .collect();
    ours.sort();
    theirs.sort();
    assert_eq!(ours, theirs);
    // Later passes are Freerouting's.
    let mut freerouting = freerouting::autoroute::batch::ordering::FreeroutingOrdering::new();
    assert_eq!(
        ordering.order(tasks.clone(), board, 2),
        freerouting.order(tasks, board, 2)
    );
}

#[test]
fn the_stages_are_selected_by_their_keys_and_off_by_default() {
    let registry = krt_registry();
    let prepared = prepare(BUS8, &[("autorouter.algorithm", BUS_BUNDLE_ID)]);
    let selection = StageSelection::resolve(&prepared.settings, &registry);
    assert!(selection.warnings.is_empty(), "{:?}", selection.warnings);
    assert_eq!(selection.ordering.id, BUS_ORDERING_ID);
    assert_eq!(selection.connection_router.id, BUS_ATTRACTION_ID);
    let pipeline = registry
        .build(&selection, &prepared.settings)
        .expect("built");
    assert_eq!(pipeline.ordering.id(), BUS_ORDERING_ID);
    assert_eq!(pipeline.connection_router.id(), BUS_ATTRACTION_ID);
    let prepared = prepare(BUS8, &[("autorouter.ordering", BUS_ORDERING_ID)]);
    let selection = StageSelection::resolve(&prepared.settings, &registry);
    assert_eq!(selection.ordering.id, BUS_ORDERING_ID);
    assert_eq!(selection.connection_router.id, "freerouting-connection");
    let default = prepare(BUS8, &[]);
    let selection = StageSelection::resolve(&default.settings, &registry);
    assert_eq!(selection.ordering.id, "freerouting-ordering");
    assert_eq!(selection.connection_router.id, "freerouting-connection");
}

// ---------------------------------------------------------------------
// X11.3: attraction
// ---------------------------------------------------------------------

fn route_prepared(prepared: PreparedJob, registry: &StageRegistry) -> RoutingBoard {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: Some(registry),
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    outcome.board.take().expect("the routed board")
}

/// What X11.3 and X11.4 count on a routed board.
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(dead_code)] // read through Debug
struct BusGates {
    unrouted: usize,
    drc_new: usize,
    via_in_pad: usize,
    vias: usize,
    /// Vias on bus nets.
    bus_vias: usize,
    /// Trace length on bus nets, mm.
    bus_length_mm: f64,
    /// The bus spacing (mean over all groups), µm; 0 without pairs.
    spacing_um: f64,
}

fn bus_gates(input: &BasicBoard, routed: &BasicBoard) -> BusGates {
    let max_input_id = input.communication.id_generator.max_generated_id();
    let new = |id: ItemId| id.0 > max_input_id;
    let groups = detect(input, &BusParams::for_board(input));
    let bus_nets: Vec<i32> = groups.iter().flat_map(|g| g.nets()).collect();
    let on_bus = |nets: &[i32]| matches!(nets, [n] if bus_nets.contains(n));
    let per_um = f64::from(freerouting::job::overrides::um_to_board_units(
        input, 1000.0,
    )) / 1000.0;
    let (mut total, mut common) = (0.0, 0.0);
    for g in &groups {
        let s = spacing(routed, g, 10.0 * per_um);
        let length: f64 = s.pairs.iter().map(|p| p.common).sum();
        total += s.mean * length;
        common += length;
    }
    BusGates {
        unrouted: freerouting::drc::DesignRulesChecker::new(routed)
            .all_airlines()
            .len(),
        drc_new: routed
            .all_clearance_violations()
            .iter()
            .filter(|v| new(v.first_item) || new(v.second_item))
            .count(),
        via_in_pad: routed
            .items()
            .filter(|i| i.is_via() && new(i.id()) && via_in_pad(routed, i))
            .count(),
        vias: routed.items().filter(|i| i.is_via()).count(),
        bus_vias: routed
            .items()
            .filter(|i| i.is_via() && on_bus(i.net_numbers()))
            .count(),
        bus_length_mm: routed
            .items()
            .filter(|i| on_bus(i.net_numbers()))
            .filter_map(|i| i.as_trace())
            .map(|t| t.length())
            .sum::<f64>()
            / (1000.0 * per_um),
        spacing_um: if common > 0.0 {
            total / common / per_um
        } else {
            0.0
        },
    }
}

/// The three arms on `x11-bus8.dsn` with the default settings: the gates
/// and the bus spacing (X11.3's measure, same-layer centreline distance
/// over the common length, sampled every 10 µm).
fn bus8_arms() -> Vec<(&'static str, BusGates)> {
    let registry = krt_registry();
    let mut out = Vec::new();
    for (arm, extra) in [
        ("freerouting", vec![]),
        (
            "krt-bus-order",
            vec![("autorouter.ordering", BUS_ORDERING_ID)],
        ),
        ("krt-bus", vec![("autorouter.algorithm", BUS_BUNDLE_ID)]),
    ] {
        let prepared = prepare(BUS8, &extra);
        let input = prepared.board.clone();
        let routed = route_prepared(prepared, &registry);
        let g = bus_gates(&input, &routed);
        println!("{arm}: {g:?}");
        out.push((arm, g));
    }
    out
}

/// X11.3's numbers on `x11-bus8.dsn` (pitch 500 µm), pinned: the
/// attraction brings the bus closer than Freerouting and the order alone,
/// but not to within 10 % of one pitch (X11.3 is partial); the order alone
/// leaves one connection unrouted on this board.
#[test]
#[ignore = "routes three arms (about 65 s in release); run with --release"]
fn the_attraction_pulls_the_bus_closer() {
    let arms = bus8_arms();
    for (arm, g) in &arms {
        assert_eq!(g.drc_new, 0, "{arm}");
        assert_eq!(g.via_in_pad, 0, "{arm}");
    }
    let unrouted: Vec<usize> = arms.iter().map(|(_, g)| g.unrouted).collect();
    assert_eq!(unrouted, [0, 1, 0]);
    let spacing: Vec<f64> = arms.iter().map(|(_, g)| g.spacing_um).collect();
    for (got, want) in spacing.iter().zip([1833.8, 2054.1, 689.0]) {
        assert!((got - want).abs() < 1.0, "{spacing:?}");
    }
    assert_eq!(arms[2].1.bus_vias, 10);
    assert_eq!(arms[0].1.bus_vias, 14);
}

#[test]
#[ignore = "routes twice (about 45 s in release); run with --release"]
fn two_attracted_runs_write_the_same_session() {
    let registry = krt_registry();
    let run = || {
        let prepared = prepare(BUS8, &[("autorouter.algorithm", BUS_BUNDLE_ID)]);
        let routed = route_prepared(prepared, &registry);
        write_ses(&routed, "x11-bus8", SesStyle::Java)
    };
    let (a, b) = (run(), run());
    assert!(a.contains("(wire"), "the session holds the routes");
    assert_eq!(a, b);
}

/// A connection cost that applies to every connection and costs nothing,
/// or declines every connection.
struct ZeroCost {
    applies: bool,
}

impl freerouting::autoroute::batch::connection_cost::ConnectionCost for ZeroCost {
    fn id(&self) -> &'static str {
        "zero"
    }

    fn begin(
        &mut self,
        _: &RoutingBoard,
        _: &freerouting::autoroute::stages::RouteTask,
        _: &freerouting::autoroute::stages::ConnectionParams,
        _: &freerouting::autoroute::control::AutorouteControl,
    ) -> bool {
        self.applies
    }

    fn section_cost(
        &mut self,
        _: i32,
        _: &freerouting::geometry::TileShape,
        _: freerouting::geometry::FloatPoint,
        _: freerouting::geometry::FloatPoint,
    ) -> f64 {
        0.0
    }
}

/// The engine's cost hook changes nothing by itself: Freerouting's router
/// with a cost that declines, or that applies and adds 0 (every expansion
/// value is positive, so adding 0.0 is exact), writes Freerouting's
/// session.
#[test]
fn a_zero_or_declining_connection_cost_routes_as_freerouting() {
    use freerouting::autoroute::batch::connection::FreeroutingConnectionRouter;
    let mut registry = krt_registry();
    registry.connection_router.insert("x11-zero", |s| {
        Box::new(
            FreeroutingConnectionRouter::from_settings(s)
                .with_cost(Box::new(ZeroCost { applies: true })),
        )
    });
    registry.connection_router.insert("x11-declining", |s| {
        Box::new(
            FreeroutingConnectionRouter::from_settings(s)
                .with_cost(Box::new(ZeroCost { applies: false })),
        )
    });
    let dsn = "fixtures/Issue433-my-board.dsn";
    let ses = |router: Option<&str>| {
        let extra: Vec<(&str, &str)> = router
            .map(|r| vec![("autorouter.connection_router", r)])
            .unwrap_or_default();
        let routed = route_prepared(prepare(dsn, &extra), &registry);
        write_ses(&routed, "Issue433-my-board", SesStyle::Java)
    };
    let freerouting = ses(None);
    assert!(freerouting.contains("(wire"));
    assert_eq!(ses(Some("freerouting-connection")), freerouting);
    assert_eq!(ses(Some("x11-declining")), freerouting);
    assert_eq!(ses(Some("x11-zero")), freerouting);
}

// ---------------------------------------------------------------------
// The X11.4 harness
// ---------------------------------------------------------------------

fn stem(dsn: &str) -> String {
    Path::new(dsn)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string()
}

fn set_boards(var: &str) -> Vec<String> {
    let set = std::env::var(var).unwrap_or_else(|_| panic!("{var}"));
    let text = std::fs::read_to_string(repo_root().join(&set)).unwrap();
    text.lines()
        .map(|l| l.split('#').next().unwrap().trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// X11.4's board eligibility on `X11_SET`: the bus groups of each board
/// (load only).
#[test]
#[ignore = "a release run over a board set; set X11_SET"]
fn bus_groups_on_a_set() {
    for dsn in set_boards("X11_SET") {
        let Ok(prepared) = std::panic::catch_unwind(|| prepare(&dsn, &[])) else {
            println!("{}: not loaded", stem(&dsn));
            continue;
        };
        let board: &BasicBoard = &prepared.board;
        let groups = detect(board, &BusParams::for_board(board));
        let sizes: Vec<String> = groups
            .iter()
            .map(|g| {
                format!(
                    "{} ({})",
                    g.members.len(),
                    names(board, &g.nets()).join(" ")
                )
            })
            .collect();
        println!(
            "{}: {} groups: {}",
            stem(&dsn),
            groups.len(),
            sizes.join("; ")
        );
    }
}

/// The X11.4 arms on `X11_SET` (or `X11_DSN`), default settings plus the
/// arm's key: `freerouting` (none), `krt-bus-order`
/// (`router.autorouter.ordering`) and `krt-bus` (`router.autorouter.algorithm`),
/// as `X11_ARMS` selects (default all three); the session of each run goes
/// to `X11_SES_DIR/<stem>.<arm>.ses` when set.
#[test]
#[ignore = "a release run over a board set; set X11_SET or X11_DSN"]
fn bus_arms_on_a_set() {
    let boards = match std::env::var("X11_DSN") {
        Ok(dsn) => vec![dsn],
        Err(_) => set_boards("X11_SET"),
    };
    let arms = std::env::var("X11_ARMS")
        .unwrap_or_else(|_| "freerouting,krt-bus-order,krt-bus".to_string());
    let registry = krt_registry();
    for dsn in boards {
        for arm in arms.split(',') {
            let extra: Vec<(&str, &str)> = match arm {
                "freerouting" => vec![],
                "krt-bus-order" => vec![("autorouter.ordering", BUS_ORDERING_ID)],
                "krt-bus" => vec![("autorouter.algorithm", BUS_BUNDLE_ID)],
                other => panic!("unknown arm {other}"),
            };
            let started = Instant::now();
            let prepared = prepare(&dsn, &extra);
            let input = prepared.board.clone();
            let routed = route_prepared(prepared, &registry);
            if let Ok(dir) = std::env::var("X11_SES_DIR") {
                std::fs::create_dir_all(&dir).unwrap();
                let path = PathBuf::from(dir).join(format!("{}.{arm}.ses", stem(&dsn)));
                std::fs::write(path, write_ses(&routed, &stem(&dsn), SesStyle::Java)).unwrap();
            }
            let g = bus_gates(&input, &routed);
            println!(
                "{} {arm}: {g:?} ({:.1} s)",
                stem(&dsn),
                started.elapsed().as_secs_f64()
            );
        }
    }
}

/// Registered but unselected, the krt stages leave sessions as the
/// built-in registry writes them, on `X11_SET` (default settings).
#[test]
#[ignore = "a release run over a board set; set X11_SET"]
fn default_off_sessions_on_a_set() {
    let registry = krt_registry();
    let builtin = StageRegistry::builtin();
    for dsn in set_boards("X11_SET") {
        let a = route_prepared(prepare(&dsn, &[]), &builtin);
        let b = route_prepared(prepare(&dsn, &[]), &registry);
        let (a, b) = (
            write_ses(&a, &stem(&dsn), SesStyle::Java),
            write_ses(&b, &stem(&dsn), SesStyle::Java),
        );
        println!("{dsn}: {}", if a == b { "identical" } else { "DIFFERENT" });
        assert_eq!(a, b, "{dsn}");
    }
}

/// An SVG of `board`'s traces (F.Cu red, B.Cu blue), vias and pins.
fn svg(board: &BasicBoard) -> String {
    let mut out = String::from(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 -240000 500000 240000' width='1500' height='720'>\n<rect x='0' y='-240000' width='500000' height='240000' fill='white'/>\n",
    );
    for item in board.items() {
        if let Some(t) = item.as_trace() {
            let colour = if t.layer == 0 { "red" } else { "blue" };
            let pts: Vec<String> = t
                .polyline
                .corner_approx_arr()
                .iter()
                .map(|p| format!("{:.0},{:.0}", p.x, -p.y))
                .collect();
            out += &format!(
                "<polyline points='{}' stroke='{colour}' stroke-width='{}' fill='none' opacity='0.6'/>\n",
                pts.join(" "),
                2 * t.half_width
            );
        } else if let Some(d) = item.drill() {
            let r = if item.is_via() { 3000 } else { 1250 };
            out += &format!(
                "<circle cx='{}' cy='{}' r='{r}' fill='{}'/>\n",
                d.center.x,
                -d.center.y,
                if item.is_via() { "green" } else { "black" }
            );
        }
    }
    out + "</svg>\n"
}

static TRIAL: std::sync::OnceLock<AttractionParams> = std::sync::OnceLock::new();

/// The three arms plus the attraction with `X11_WEIGHT`, `X11_CAP_UM` and
/// `X11_SAME_LAYER` on `x11-bus8.dsn` (or `X11_DSN`), with pictures in
/// `X11_SVG_DIR` when set.
#[test]
#[ignore = "a parameter trial; set X11_WEIGHT"]
fn attraction_trial() {
    let env = |k: &str, d: f64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let params = AttractionParams {
        weight: env("X11_WEIGHT", 1.0),
        cap_um: env("X11_CAP_UM", 5000.0),
        same_layer: std::env::var("X11_SAME_LAYER").is_ok(),
        last_pass: env("X11_LAST_PASS", 1.0) as u32,
        two_sided: std::env::var("X11_ONE_SIDED").is_err(),
    };
    TRIAL.set(params).unwrap();
    let mut registry = krt_registry();
    registry.connection_router.insert("x11-trial", |s| {
        Box::new(KrtBusAttractionRouter::with_params(
            s,
            *TRIAL.get().unwrap(),
        ))
    });
    let dsn = std::env::var("X11_DSN").unwrap_or_else(|_| BUS8.to_string());
    let arms = std::env::var("X11_ARMS").unwrap_or_else(|_| "trial".to_string());
    for arm in arms.split(',') {
        let extra: Vec<(&str, &str)> = match arm {
            "freerouting" => vec![],
            "krt-bus-order" => vec![("autorouter.ordering", BUS_ORDERING_ID)],
            "attraction-only" => vec![("autorouter.connection_router", "x11-trial")],
            _ => vec![
                ("autorouter.ordering", BUS_ORDERING_ID),
                ("autorouter.connection_router", "x11-trial"),
            ],
        };
        let started = Instant::now();
        let more = std::env::var("X11_EXTRA").unwrap_or_default();
        let mut extra = extra;
        extra.extend(more.split(',').filter_map(|kv| kv.split_once('=')));
        let prepared = prepare(&dsn, &extra);
        let input = prepared.board.clone();
        let job = PipelineJob {
            settings: &prepared.settings,
            board_settings: &prepared.board_settings,
            cancel: Arc::new(CancelToken::new()),
            events: &NullEvents,
            deadline: None,
            registry: Some(&registry),
        };
        let mut outcome =
            run_pipeline(prepared.board.clone(), &job).unwrap_or_else(|f| panic!("{}", f.error));
        println!(
            "  stop {:?} restores {} best {} passes {:?}",
            outcome.stop, outcome.restores, outcome.restored_best, outcome.passes
        );
        let routed = outcome.board.take().expect("board");
        println!(
            "{arm} {params:?}: {:?} ({:.1} s)",
            bus_gates(&input, &routed),
            started.elapsed().as_secs_f64()
        );
        if let Ok(dir) = std::env::var("X11_SVG_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(PathBuf::from(dir).join(format!("{arm}.svg")), svg(&routed)).unwrap();
        }
    }
}

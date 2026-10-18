//! N03.1 (P04.1 and P04.2 restored): with `router.room_order = canonical`
//! and `router.work_unit = leaves` the router's results and its work counts
//! depend only on the search trees' contents, not on their shape. The
//! defaults and the `legacy` profile stay Java's (`tree`, `nodes`).
//!
//! Two boards are loaded from the same design; on one, every whole-tree
//! build (the autoroute trees, rebuilds) inserts the items in reverse
//! item-list order (`SearchConfig::reversed_builds`, a test hook), which
//! gives trees with the same leaves and another shape. Java's order and
//! unit (`tree`, `nodes`) tell the two apart; the canonical order and the
//! leaf unit do not.

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::{Loaded, load};
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::search::complete_shape::CompletionScratch;
use freerouting::board::search::rooms::IncompleteRoom;
use freerouting::board::search::{RoomOrder, SearchConfig, WorkUnit};
use freerouting::board::structure::AngleRestriction;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{IntBox, RegularTileShape, TileShape};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};

const MY_BOARD: &str = "fixtures/Issue433-my-board.dsn";
/// 30 passes with history restores (A13.2).
const CONGFLOCK: &str = "fixtures/PCBench-retroreflectors-CONGFLOCK.dsn";
const SEED: i64 = 20261017;
const QUERIES: usize = 300;

fn config(order: RoomOrder, unit: WorkUnit, reversed_builds: bool) -> SearchConfig {
    SearchConfig {
        room_order: order,
        work_unit: unit,
        reversed_builds,
    }
}

/// The smallest non-zero trace clearance class of the net classes, else 0.
fn tree_class(board: &RoutingBoard) -> i32 {
    let classes = &board.rules.net_classes;
    (0..classes.count())
        .filter_map(|i| classes.nth(i))
        .map(|id| classes.get(id).trace_clearance_class())
        .filter(|&c| c > 0)
        .min()
        .unwrap_or(0)
}

/// What a fixed query sample gives on the autoroute tree: per query the
/// work units and the result (overlap leaves as keys, completion rooms as
/// text), and the tree's shape fingerprint.
#[derive(Debug, PartialEq)]
struct Sample {
    units: Vec<u64>,
    results: Vec<String>,
    shape: String,
}

fn sample(dsn: &str, angle: AngleRestriction, config: SearchConfig) -> Sample {
    let Loaded { mut board, .. } = load(dsn);
    board.rules.set_trace_angle_restriction(angle);
    board.set_search_config(config);
    let class = tree_class(&board);
    let shape = format!("{:?}", board.autoroute_search_tree(class).fingerprint());
    let bounds = board.bounding_box();
    let (width, height) = (bounds.ur.x - bounds.ll.x, bounds.ur.y - bounds.ll.y);
    let layers = board.layer_count() as i32;
    let max_net = board.rules.nets.max_net_number();
    let mut random = JavaRandom::new(SEED);
    let mut scratch = CompletionScratch::new();
    let mut units = Vec::new();
    let mut results = Vec::new();
    for _ in 0..QUERIES {
        let layer = random.next_int(layers);
        let x = bounds.ll.x + random.next_int(width + 1);
        let y = bounds.ll.y + random.next_int(height + 1);
        let h = 1 + random.next_int((width.min(height) / 50).max(1));
        let net = random.next_int(max_net + 1);
        let square = |r: i32| IntBox::new(x - r, y - r, x + r, y + r);
        let shape_of = |b: IntBox| match angle {
            AngleRestriction::NinetyDegree => TileShape::IntBox(b),
            _ => TileShape::IntOctagon(b.to_int_octagon()),
        };
        // An overlap query of a box around the point.
        let before = board.search_trees().work_meter().units();
        let tree = board
            .search_trees()
            .tree_by_class(class)
            .expect("the autoroute tree");
        let query = match angle {
            AngleRestriction::NinetyDegree => RegularTileShape::IntBox(square(20 * h)),
            _ => RegularTileShape::IntOctagon(square(20 * h).to_int_octagon()),
        };
        let leaves: Vec<_> = tree
            .shape_tree()
            .overlaps(&query)
            .into_iter()
            .map(|leaf| tree.shape_tree().leaf(leaf))
            .collect();
        units.push(board.search_trees().work_meter().units() - before);
        results.push(format!("{leaves:?}"));
        // A room completion around it, unbounded or in a larger square.
        let start = random.next_boolean().then(|| shape_of(square(20 * h)));
        let room = IncompleteRoom::new(start, layer, Some(shape_of(square(h))));
        let before = board.search_trees().work_meter().units();
        let rooms = board.complete_shape(class, &room, net, None, None, &mut scratch);
        units.push(board.search_trees().work_meter().units() - before);
        results.push(format!("{rooms:?}"));
    }
    Sample {
        units,
        results,
        shape,
    }
}

/// P04.2: the same contents in trees of two shapes give the same work
/// counts in leaves, and different ones in nodes (Java's unit). With the
/// canonical order the completions are equal too; in tree order they
/// differ.
#[test]
fn leaf_counts_and_canonical_rooms_do_not_depend_on_tree_shape() {
    for angle in [
        AngleRestriction::FortyfiveDegree,
        AngleRestriction::NinetyDegree,
    ] {
        let canonical = |reversed| {
            sample(
                MY_BOARD,
                angle,
                config(RoomOrder::Canonical, WorkUnit::Leaves, reversed),
            )
        };
        let (a, b) = (canonical(false), canonical(true));
        assert_ne!(a.shape, b.shape, "{angle:?}: the two builds have one shape");
        assert_eq!(a.results, b.results, "{angle:?}: canonical results");
        assert_eq!(a.units, b.units, "{angle:?}: leaf counts");
        assert!(a.units.iter().sum::<u64>() > 0);

        let java = |reversed| {
            sample(
                MY_BOARD,
                angle,
                config(RoomOrder::Tree, WorkUnit::Nodes, reversed),
            )
        };
        let (c, d) = (java(false), java(true));
        assert_ne!(c.units, d.units, "{angle:?}: node counts see the shape");
        // Overlap results are sorted in every mode (the even entries).
        let overlaps = |s: &Sample| s.results.iter().step_by(2).cloned().collect::<Vec<_>>();
        assert_eq!(overlaps(&c), overlaps(&a), "{angle:?}: overlap results");
        assert_eq!(overlaps(&d), overlaps(&a), "{angle:?}: overlap results");
        // The leaf counts of overlap queries are their result sizes.
        assert_eq!(a.units[0] as usize, a.results[0].matches("(Item(").count());
    }
}

/// Routes `dsn` with `config` (J01.3's route mode: optimizer off, 100
/// passes) and returns the session and the pass records.
fn route(dsn: &str, config: SearchConfig) -> (String, String) {
    let mut loaded = load(dsn);
    loaded.settings.optimizer.enabled = false;
    loaded.settings.autorouter.max_passes = 100;
    loaded.settings.room_order = Some(config.room_order.name().to_string());
    loaded.settings.work_unit = Some(config.work_unit.name().to_string());
    loaded.board.set_search_config(config);
    let job = PipelineJob {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(loaded.board, &job).unwrap_or_else(|f| panic!("{dsn}: {}", f.error));
    // Java's order and unit log nothing, as Java does; any other logs.
    assert_eq!(
        outcome.log.iter().any(|l| l.text == config.log_line()),
        !config.is_java(),
        "the job logs a search configuration that is not Java's"
    );
    let board = outcome.board.take().expect("no listener took the board");
    (
        write_ses(&board, "board", SesStyle::Java),
        format!("{:?}", outcome.passes),
    )
}

/// P04.1: with the canonical order the routes of two builds of one board
/// whose trees differ in shape are identical, in either unit (no work
/// limit fires on these boards).
#[test]
fn canonical_routes_do_not_depend_on_tree_shape() {
    for dsn in [MY_BOARD, CONGFLOCK] {
        for unit in [WorkUnit::Leaves, WorkUnit::Nodes] {
            let a = route(dsn, config(RoomOrder::Canonical, unit, false));
            let b = route(dsn, config(RoomOrder::Canonical, unit, true));
            assert_eq!(a.0, b.0, "{dsn} {unit:?}: sessions");
            assert_eq!(a.1, b.1, "{dsn} {unit:?}: pass records");
        }
        let a = route(dsn, config(RoomOrder::Tree, WorkUnit::Nodes, false));
        let b = route(dsn, config(RoomOrder::Tree, WorkUnit::Nodes, true));
        // Java's order sees the shape (on my-board; CONGFLOCK happens to
        // route the same).
        if dsn == MY_BOARD {
            assert_ne!(a.0, b.0, "{dsn}: tree order routes depend on the shape");
        }
    }
}

/// The routing job reads the two keys and refuses an unknown value.
#[test]
fn the_job_sets_the_configuration_and_refuses_unknown_values() {
    let mut loaded = load(MY_BOARD);
    loaded.settings.room_order = Some("sideways".to_string());
    let job = PipelineJob {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let failure = run_pipeline(loaded.board, &job).expect_err("an unknown room order");
    assert!(failure.error.to_string().contains("router.room_order"));
    let mut settings = load(MY_BOARD).settings;
    assert_eq!(
        SearchConfig::from_settings(&settings),
        Ok(SearchConfig::default())
    );
    settings.work_unit = Some("leaves".to_string());
    settings.room_order = Some("canonical".to_string());
    assert_eq!(
        SearchConfig::from_settings(&settings),
        Ok(config(RoomOrder::Canonical, WorkUnit::Leaves, false))
    );
}

/// N03.1: the defaults and `legacy` are Java's order and unit, also beside
/// an `enhanced` that would change both; the two keys are connection-router
/// options in the stage catalog (the CLI's `--list-stages`, the GUI's
/// Algorithms section and the plugin's Advanced group).
#[test]
fn legacy_pins_tree_order_and_nodes() {
    use freerouting::autoroute::stages::StageRegistry;
    use freerouting::autoroute::stages::catalog::catalog;
    use freerouting::settings::RouterSettings;
    use freerouting::settings::profile::{Difference, Profile, with_enhanced_differences};

    const CORES: usize = 4;
    let of = |profile: Profile| {
        let settings = RouterSettings::from_patch(&profile.defaults(CORES), CORES);
        SearchConfig::from_settings(&settings).expect("valid defaults")
    };
    assert_eq!(of(Profile::Legacy), SearchConfig::default());
    assert!(of(Profile::Legacy).is_java());
    // Not in `enhanced` until N03.3's measurement accepts it (N05).
    assert_eq!(of(Profile::Enhanced), SearchConfig::default());
    const DUMMY: &[Difference] = &[
        Difference {
            path: "room_order",
            value: "canonical",
            measurement: "test-only dummy (N03.1)",
        },
        Difference {
            path: "work_unit",
            value: "leaves",
            measurement: "test-only dummy (N03.1)",
        },
    ];
    with_enhanced_differences(DUMMY, || {
        assert_eq!(
            of(Profile::Enhanced),
            config(RoomOrder::Canonical, WorkUnit::Leaves, false)
        );
        assert_eq!(of(Profile::Legacy), SearchConfig::default());
    });

    let listing = catalog(&StageRegistry::builtin(), Profile::Legacy, CORES);
    let router = listing
        .iter()
        .find(|s| s.desc.kind == "connection_router")
        .expect("the connection router slot");
    for key in ["room_order", "work_unit"] {
        let option = router
            .options
            .iter()
            .find(|o| o.path == key)
            .unwrap_or_else(|| panic!("{key} is listed"));
        assert_eq!(option.default, None, "{key}: unset is Java's");
    }
}

/// N03.1: the job's load sequence refuses an unknown value, naming the key
/// and the valid values, before any routing; blank is the default.
#[test]
fn prepare_job_refuses_unknown_search_values() {
    use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
    use freerouting::settings::profiles::TestingProfile;
    use freerouting::settings::{ResolveContext, Source};

    let dsn = std::fs::read(autoroute_common::repo_root().join(MY_BOARD)).expect("fixture");
    let prepare = |key: &str, value: &str| {
        let mut profile = TestingProfile::default();
        profile.patch.set_path(key, value).expect("a settings path");
        let mut input = JobInput::new(dsn.clone(), "my-board");
        input.rules = RulesChoice::None;
        input.extra_sources = vec![(Source::Testing, profile.patch)];
        prepare_job(&input, &ResolveContext { cores: 1 })
    };
    for (key, value, valid) in [
        ("room_order", "sideways", "'tree' or 'canonical'"),
        ("work_unit", "seconds", "'nodes' or 'leaves'"),
    ] {
        let Err(error) = prepare(key, value) else {
            panic!("{key} = {value} fails the job");
        };
        let text = error.to_string();
        assert!(text.contains(&format!("router.{key}")), "{text}");
        assert!(text.contains(valid), "{text}");
    }
    for (key, value) in [("room_order", " canonical "), ("work_unit", "leaves")] {
        assert!(prepare(key, value).is_ok(), "{key} = {value}");
    }
    let mut settings = load(MY_BOARD).settings;
    settings.room_order = Some(" ".to_string());
    assert_eq!(
        SearchConfig::from_settings(&settings),
        Ok(SearchConfig::default())
    );
}

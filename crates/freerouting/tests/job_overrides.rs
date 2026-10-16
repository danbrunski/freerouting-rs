//! C03.2-C03.5: `BoardOutline::minimum_pin_gap` as used by the edge
//! override, and the five board overrides of `job::overrides`.
//!
//! Ports of the Java tests `CopperToEdgeClearanceCapTest` (both tests),
//! `DevBoardClearanceRoutingTest` (the clearance assertions),
//! `HoleKeepoutClearanceTest.npthKeepoutsGetHoleEdgeClearanceClass` and
//! `PowerPlaneValidationTest.{testPlaneNetsOverride, testPlaneAsObstacleOverride}`.
//! Not ported: the routing assertions of `DevBoardClearanceRoutingTest` (the
//! vias; A13), and the rest of `PowerPlaneValidationTest` (power-plane
//! validation and the heuristic plane detection are not C03).
//!
//! The Java fixture tests run the whole scheduler with limited stages; here
//! `prepare_job` stands in, because the overrides run in the load sequence.

use std::path::Path;

use freerouting::board::items::AreaKind;
use freerouting::board::state::Communication;
use freerouting::board::structure::{FixedState, Layer, LayerStructure, Unit};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::geometry::{Area, IntBox, PolylineShape, Shape, TileShape};
use freerouting::job::LogLevel;
use freerouting::job::overrides::{
    OverrideState, apply_all, apply_clearance_tolerance, apply_edge_clearance,
    apply_hole_clearance, apply_plane_as_obstacle, apply_plane_nets, um_to_board_units,
};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::rules::{BoardRules, ClearanceMatrix, ItemClass};
use freerouting::settings::{
    ResolveContext, RouterSettings, SettingsPatch, Source, default_patch, parse_cli_arguments,
};

const CORES: usize = 8;

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn prepare(name: &str, testing: SettingsPatch) -> PreparedJob {
    let base = Path::new(name)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut input = JobInput::new(fixture(name), base);
    input.rules = RulesChoice::None;
    input.extra_sources.push((Source::Testing, testing));
    prepare_job(&input, &ResolveContext { cores: CORES }).expect("the board loads")
}

fn edge_patch(um: f64) -> SettingsPatch {
    SettingsPatch {
        copper_to_edge_clearance_um: Some(um),
        ..SettingsPatch::default()
    }
}

fn settings() -> RouterSettings {
    RouterSettings::from_patch(&default_patch(CORES), CORES)
}

/// A bare board: two signal layers, clearance 10, no pins, `outline_class`.
fn bare_board(layers: Vec<Layer>, outline_class: i32) -> RoutingBoard {
    let layers = LayerStructure::new(layers);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    let outline = IntBox::new(0, 0, 2_000_000, 2_000_000);
    RoutingBoard::new(
        outline,
        layers,
        vec![PolylineShape::Tile(TileShape::from(outline))],
        outline_class,
        rules,
        Communication::default(),
    )
}

fn two_layers() -> Vec<Layer> {
    vec![Layer::new("Top", true), Layer::new("Bottom", true)]
}

/// The stored tree shapes of every item in every search tree, item by item
/// (B02's fingerprint stands in once it exists).
fn tree_fingerprint(board: &BasicBoard) -> String {
    use std::fmt::Write;
    let ctx = board.shape_context();
    let mut out = String::new();
    for id in board.item_ids() {
        let item = board.item(id).unwrap();
        for (t, tree) in board.search_trees().trees().iter().enumerate() {
            let _ = write!(out, "{}/{t}:{:?};", id.0, tree.tree_shapes(item, &ctx));
        }
    }
    out
}

fn edge_values(board: &BasicBoard, edge: i32) -> Vec<i32> {
    let matrix = &board.rules.clearance_matrix;
    (0..matrix.layer_count())
        .map(|layer| matrix.get_value(edge, edge, layer as i32, false))
        .collect()
}

// ---- C03.2 and C03.3: the copper-to-edge override ----------------------

const CONGFLOCK: &str = "PCBench-retroreflectors-CONGFLOCK.dsn";

#[test]
fn default_edge_clearance_is_capped_by_the_pin_to_outline_gap() {
    // CopperToEdgeClearanceCapTest.defaultEdgeClearanceIsCappedByPinToOutlineGap
    let job = prepare(CONGFLOCK, edge_patch(250.0));
    let board = &job.board;
    let edge = board.rules.clearance_matrix.get_no("board_edge");
    assert!(edge >= 0, "board_edge class must exist");
    let default_units = um_to_board_units(board, 250.0);
    let gap = board.minimum_pin_gap();
    eprintln!("CONGFLOCK: pin gap {gap}, default {default_units} board units");
    assert!(
        gap > 0.0 && gap < f64::from(default_units),
        "the fixture must have a pin closer to the edge than 250 um, gap={gap}"
    );
    for applied in edge_values(board, edge) {
        assert!(
            (f64::from(applied) - gap).abs() <= 1.0,
            "edge clearance {applied} must equal the smallest pin-to-outline gap {gap}"
        );
    }
    // The outline moved into the class.
    assert_eq!(board.outline().unwrap().clearance_class(), edge);
}

#[test]
fn explicit_edge_clearance_is_not_capped() {
    // CopperToEdgeClearanceCapTest.explicitEdgeClearanceIsNotCapped
    let job = prepare(CONGFLOCK, edge_patch(400.0));
    let edge = job.board.rules.clearance_matrix.get_no("board_edge");
    assert!(edge >= 0);
    let expected = um_to_board_units(&job.board, 400.0);
    assert!(f64::from(expected) > job.board.minimum_pin_gap());
    for applied in edge_values(&job.board, edge) {
        assert_eq!(applied, expected);
    }
}

#[test]
fn the_edge_default_is_detected_by_value_so_an_explicit_250_is_capped_too() {
    // A kept quirk: `|v - 250| < 1e-9` cannot tell an explicit 250 from the default.
    let explicit = prepare(CONGFLOCK, edge_patch(250.0));
    let by_default = prepare(CONGFLOCK, SettingsPatch::default());
    let edge = |j: &PreparedJob| {
        let e = j.board.rules.clearance_matrix.get_no("board_edge");
        edge_values(&j.board, e)
    };
    assert_eq!(edge(&explicit), edge(&by_default));
    // 250.0 + 1e-10 is still "the default"; 250.001 is not.
    let near = prepare(CONGFLOCK, edge_patch(250.0 + 1e-10));
    assert_eq!(edge(&near), edge(&by_default));
    let units = um_to_board_units(&near.board, 250.001);
    let beyond = prepare(CONGFLOCK, edge_patch(250.001));
    assert!(edge(&beyond).iter().all(|v| *v == units));
}

#[test]
fn dev_board_edge_clearance_650() {
    // DevBoardClearanceRoutingTest, the clearance assertions.
    let job = prepare("Issue558-dev-board.dsn", edge_patch(650.0));
    let board = &job.board;
    let edge = board.rules.clearance_matrix.get_no("board_edge");
    assert!(
        edge >= 0,
        "Expected board_edge clearance class to be created."
    );
    assert_eq!(
        board.outline().unwrap().clearance_class(),
        edge,
        "Board outline should be assigned to the board_edge clearance class."
    );
    let expected = um_to_board_units(board, 650.0);
    let values = edge_values(board, edge);
    assert_eq!(values.len(), board.rules.clearance_matrix.layer_count());
    assert!(
        values.iter().all(|v| *v == expected),
        "{values:?} != {expected}"
    );
}

#[test]
fn board_units_follow_javas_order_and_rounding() {
    let mut board = bare_board(two_layers(), 1);
    // Mil, resolution 1: 250 um = 9.84 mil.
    assert_eq!(um_to_board_units(&board, 250.0), 10);
    assert_eq!(um_to_board_units(&board, 254.0), 10);
    // Micrometers, resolution 1: Math.round rounds halves up, not to even.
    board.communication.unit = Unit::Um;
    board.communication.resolution = 1;
    assert_eq!(um_to_board_units(&board, 0.5), 1);
    assert_eq!(um_to_board_units(&board, 1.5), 2);
    assert_eq!(um_to_board_units(&board, 2.5), 3);
    // um * resolution first, then the scale.
    board.communication.resolution = 10;
    assert_eq!(um_to_board_units(&board, 250.0), 2500);
    board.communication.unit = Unit::Mm;
    board.communication.resolution = 0; // max(1, resolution)
    assert_eq!(
        um_to_board_units(&board, 250.0),
        0,
        "0.25 mm at resolution 1"
    );
    board.communication.resolution = 1000;
    assert_eq!(um_to_board_units(&board, 250.0), 250);
}

#[test]
fn explicit_outline_class_wins_over_the_default_value() {
    // Outline class 0 is not the default AREA class (1): the default value
    // leaves it alone; a different value applies.
    let mut board = bare_board(two_layers(), 0);
    let mut state = OverrideState::default();
    let lines = apply_edge_clearance(&mut board, &settings(), &mut state);
    assert!(lines.is_empty());
    assert!(!state.edge_applied);
    assert_eq!(board.rules.clearance_matrix.get_no("board_edge"), -1);
    assert_eq!(board.outline().unwrap().clearance_class(), 0);

    let mut s = settings();
    s.copper_to_edge_clearance_um = 300.0;
    let lines = apply_edge_clearance(&mut board, &s, &mut state);
    assert!(state.edge_applied);
    let edge = board.rules.clearance_matrix.get_no("board_edge");
    assert_eq!(board.outline().unwrap().clearance_class(), edge);
    assert!(lines.iter().any(|l| l.level == LogLevel::Debug
        && l.text == "Applied copper-to-edge clearance override: 300.0 um (12 board units)."));
    assert_eq!(edge_values(&board, edge), vec![12, 12]);

    // Once the override has applied, the default value applies again (the
    // outline's class is board_edge now, which is not the fallback class).
    // No pins: no cap. 250 um = 9.84 mil = 10.
    apply_edge_clearance(&mut board, &settings(), &mut state);
    assert_eq!(edge_values(&board, edge), vec![10, 10]);
}

#[test]
fn a_negative_edge_clearance_is_a_warning_and_changes_nothing() {
    let mut board = bare_board(two_layers(), 1);
    let before = tree_fingerprint(&board);
    let mut s = settings();
    s.copper_to_edge_clearance_um = -1.0;
    let mut state = OverrideState::default();
    let lines = apply_edge_clearance(&mut board, &s, &mut state);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].level, LogLevel::Warn);
    assert_eq!(
        lines[0].text,
        "Ignoring router.copper_to_edge_clearance_um because it is negative: -1.0"
    );
    assert!(!state.edge_applied);
    assert_eq!(board.rules.clearance_matrix.get_no("board_edge"), -1);
    assert_eq!(tree_fingerprint(&board), before);
}

#[test]
fn the_default_edge_clearance_without_a_cap_applies_the_converted_value() {
    let mut board = bare_board(two_layers(), 1);
    assert!(
        board.minimum_pin_gap().is_infinite(),
        "a board without pins"
    );
    let mut state = OverrideState::default();
    apply_edge_clearance(&mut board, &settings(), &mut state);
    let edge = board.rules.clearance_matrix.get_no("board_edge");
    assert_eq!(edge_values(&board, edge), vec![10, 10]);
    // Every class row and column: (edge, c) and (c, edge) for c >= 1.
    let matrix = &board.rules.clearance_matrix;
    for layer in 0..2 {
        assert_eq!(matrix.get_value(edge, 1, layer, false), 10);
        assert_eq!(matrix.get_value(1, edge, layer, false), 10);
        assert_eq!(matrix.get_value(0, edge, layer, false), 0, "class 0 stays");
    }
}

// ---- C03.4: the hole-clearance override --------------------------------

const ISSUE230: &str = "Issue230-CNH_Functional_Tester_1.dsn";

fn hole_keepouts(board: &BasicBoard) -> Vec<(i32, i32)> {
    board
        .items()
        .filter(|item| {
            item.component_id() > 0
                && item.as_area().is_some_and(|a| {
                    a.kind == AreaKind::Obstacle
                        && matches!(a.area(), Area::Shape(Shape::Circle(_)))
                })
        })
        .map(|item| (item.id().0, item.clearance_class()))
        .collect()
}

#[test]
fn npth_keepouts_get_hole_edge_clearance_class() {
    // HoleKeepoutClearanceTest.npthKeepoutsGetHoleEdgeClearanceClass, with
    // the `java_fixture_test()` profile plus hole_clearance_um = 250.
    let mut testing = freerouting::settings::profiles::java_fixture_test();
    testing.hole_clearance_um = Some(250.0);
    let job = prepare(ISSUE230, testing);
    let board = &job.board;
    let matrix = &board.rules.clearance_matrix;
    let hole_edge = matrix.get_no("hole_edge");
    assert!(hole_edge > 0, "hole_edge clearance class must exist");
    let keepouts = hole_keepouts(board);
    assert!(
        !keepouts.is_empty(),
        "fixture must contain NPTH keepout circles"
    );
    for (id, class) in &keepouts {
        assert_eq!(
            *class, hole_edge,
            "keepout {id} must use the hole_edge class"
        );
    }
    let expected = um_to_board_units(board, 250.0);
    for layer in 0..matrix.layer_count() {
        assert!(matrix.get_value(hole_edge, 1, layer as i32, false) >= expected);
    }
    assert_eq!(board.rules.hole_clearance(), expected);
    eprintln!(
        "Issue230: {} circular package keepouts in hole_edge, {expected} board units",
        keepouts.len()
    );
}

#[test]
fn hole_edge_is_the_larger_of_the_hole_clearance_and_the_area_clearance() {
    let testing = SettingsPatch {
        hole_clearance_um: Some(250.0),
        ..SettingsPatch::default()
    };
    let job = prepare(ISSUE230, testing);
    let board = &job.board;
    let matrix = &board.rules.clearance_matrix;
    let hole_edge = matrix.get_no("hole_edge");
    let units = um_to_board_units(board, 250.0);
    let rules = &board.rules;
    let default = rules.default_net_class_id().unwrap();
    let area_class = rules
        .net_classes
        .get(default)
        .default_item_clearance_classes
        .get(ItemClass::Area);
    let mut raised = 0;
    for layer in 0..matrix.layer_count() as i32 {
        for class in 1..matrix.class_count() as i32 {
            let want = units.max(matrix.get_value(area_class, class, layer, false));
            // set_value rounds odd values up to even
            let want = want + want % 2;
            assert_eq!(matrix.get_value(hole_edge, class, layer, false), want);
            assert_eq!(matrix.get_value(class, hole_edge, layer, false), want);
            if want > units {
                raised += 1;
            }
        }
    }
    eprintln!("hole_edge entries above the hole clearance: {raised}");
}

#[test]
fn a_hole_clearance_of_zero_changes_nothing() {
    let bytes = fixture(ISSUE230);
    let mut board = freerouting::drc::load_design(&bytes).unwrap();
    let before = tree_fingerprint(&board);
    let classes = board.rules.clearance_matrix.class_count();
    let (lines, reclassified) = apply_hole_clearance(&mut board, &settings());
    assert!(lines.is_empty());
    assert_eq!(reclassified, 0);
    assert_eq!(board.rules.hole_clearance(), 0);
    assert_eq!(board.rules.clearance_matrix.class_count(), classes);
    assert_eq!(board.rules.clearance_matrix.get_no("hole_edge"), -1);
    assert_eq!(tree_fingerprint(&board), before, "the trees are untouched");
}

/// True if the stored tree shapes of every item equal the shapes calculated
/// from the current rules.
fn trees_are_current(board: &BasicBoard) -> bool {
    let ctx = board.shape_context();
    board.item_ids().into_iter().all(|id| {
        let item = board.item(id).unwrap();
        board.search_trees().trees().iter().all(|tree| {
            format!("{:?}", tree.tree_shapes(item, &ctx))
                == format!("{:?}", tree.calculate_tree_shapes(item, &ctx))
        })
    })
}

#[test]
fn a_hole_clearance_reinserts_the_tree_items() {
    // A large hole clearance inflates the drilled items; without the
    // reinsertion the stored shapes would be stale.
    let bytes = fixture(ISSUE230);
    let mut board = freerouting::drc::load_design(&bytes).unwrap();
    assert!(trees_are_current(&board));
    let before = tree_fingerprint(&board);
    // The detector works: changing the rule alone leaves the trees stale.
    let mut stale = board.clone();
    let big = um_to_board_units(&stale, 5000.0);
    stale.rules.set_hole_clearance(big);
    assert!(!trees_are_current(&stale));
    let mut s = settings();
    s.hole_clearance_um = 5000.0;
    apply_hole_clearance(&mut board, &s);
    assert_ne!(
        tree_fingerprint(&board),
        before,
        "the tree shapes were rebuilt"
    );
    assert!(trees_are_current(&board));
}

#[test]
fn the_hole_override_counts_the_reclassified_keepouts() {
    let bytes = fixture(ISSUE230);
    let mut board = freerouting::drc::load_design(&bytes).unwrap();
    let mut s = settings();
    s.hole_clearance_um = 250.0;
    let (lines, reclassified) = apply_hole_clearance(&mut board, &s);
    let keepouts = hole_keepouts(&board);
    assert_eq!(
        reclassified,
        keepouts.len(),
        "none were in hole_edge before"
    );
    assert!(reclassified > 0);
    eprintln!("Issue230: {reclassified} hole keepouts reclassified");
    assert!(trees_are_current(&board));
    let info = lines.last().unwrap();
    assert_eq!(info.level, LogLevel::Info);
    assert_eq!(
        info.text,
        format!(
            "Applied drill-hole clearance override: 250.0 um ({} board units), \
             {reclassified} hole keepouts reclassified.",
            um_to_board_units(&board, 250.0)
        )
    );
    // Applying it again reclassifies nothing and the value is unchanged.
    let after = tree_fingerprint(&board);
    let (lines, again) = apply_hole_clearance(&mut board, &s);
    assert_eq!(again, 0);
    assert_eq!(tree_fingerprint(&board), after);
    assert!(
        lines[0].text.ends_with("board units)."),
        "{}",
        lines[0].text
    );
}

#[test]
fn a_negative_hole_clearance_is_a_warning_and_changes_nothing() {
    let mut board = bare_board(two_layers(), 1);
    let mut s = settings();
    s.hole_clearance_um = -5.0;
    let (lines, n) = apply_hole_clearance(&mut board, &s);
    assert_eq!(n, 0);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].level, LogLevel::Warn);
    assert_eq!(
        lines[0].text,
        "Ignoring router.hole_clearance_um because it is negative: -5.0"
    );
    assert_eq!(board.rules.hole_clearance(), 0);
}

#[test]
fn a_hole_clearance_without_keepouts_still_sets_the_rule() {
    let mut board = bare_board(two_layers(), 1);
    let mut s = settings();
    s.hole_clearance_um = 254.0; // 10 mil
    let (lines, n) = apply_hole_clearance(&mut board, &s);
    assert_eq!(n, 0);
    assert_eq!(board.rules.hole_clearance(), 10);
    assert_eq!(
        board.rules.clearance_matrix.get_no("hole_edge"),
        -1,
        "no keepouts, no class"
    );
    assert_eq!(
        lines.last().unwrap().text,
        "Applied drill-hole clearance override: 254.0 um (10 board units)."
    );
}

// ---- C03.5: plane nets, plane as obstacle, tolerance -------------------

#[test]
fn plane_nets_override() {
    // PowerPlaneValidationTest.testPlaneNetsOverride
    let mut board = bare_board(two_layers(), 1);
    let vcc = board.rules.add_net("VCC", 1, false);
    assert!(!board.rules.nets.get(vcc).unwrap().contains_plane());
    let mut s = settings();
    s.plane_nets = vec!["VCC".to_string()];
    let mut state = OverrideState::default();
    apply_all(&mut board, &s, &mut state);
    assert!(
        board.rules.nets.get(vcc).unwrap().contains_plane(),
        "Net specified in planeNets should have containsPlane=true."
    );
}

#[test]
fn plane_nets_match_trimmed_names_ignoring_case_on_all_subnets() {
    let mut board = bare_board(two_layers(), 1);
    let a = board.rules.add_net("Gnd", 1, false);
    let b = board.rules.add_net("GND", 2, false);
    let other = board.rules.add_net("VCC", 1, false);
    let already = board.rules.add_net("AGND", 1, true);
    let mut s = settings();
    s.plane_nets = vec![
        "  gnd ".to_string(),
        "   ".to_string(),
        String::new(),
        "agnd".to_string(),
        "NOPE".to_string(),
    ];
    let lines = apply_plane_nets(&mut board, &s);
    let plane = |n| board.rules.nets.get(n).unwrap().contains_plane();
    assert!(plane(a) && plane(b) && plane(already));
    assert!(!plane(other));
    // The already-plane net logs nothing.
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0].text,
        "Configured net 'Gnd' as a power plane net via router.plane_nets."
    );
    assert!(lines.iter().all(|l| l.level == LogLevel::Info));
}

#[test]
fn plane_as_obstacle_override() {
    // PowerPlaneValidationTest.testPlaneAsObstacleOverride
    let mut board = bare_board(two_layers(), 1);
    let gnd = board.rules.add_net("GND", 1, true);
    let area = TileShape::from(IntBox::new(0, 0, 1_000_000, 1_000_000));
    let id = board.insert_conduction_area(
        Area::Shape(Shape::Polyline(PolylineShape::Tile(area))),
        0,
        &[gnd],
        0,
        false,
        FixedState::Unfixed,
    );
    let mut s = settings();
    s.plane_as_obstacle = true;
    let mut state = OverrideState::default();
    apply_all(&mut board, &s, &mut state);
    assert!(
        !board.rules.ignore_conduction(),
        "planeAsObstacle=true should set rules.ignoreConduction=false."
    );
    let item = board.item(id).unwrap();
    assert!(
        item.as_conduction_area().unwrap().conduction_is_obstacle(),
        "ConductionArea should have isObstacle=true."
    );
    // And back.
    s.plane_as_obstacle = false;
    let lines = apply_plane_as_obstacle(&mut board, &s);
    assert!(board.rules.ignore_conduction());
    assert!(
        !board
            .item(id)
            .unwrap()
            .as_conduction_area()
            .unwrap()
            .conduction_is_obstacle()
    );
    assert_eq!(lines[0].text, "Applied plane_as_obstacle override: false");
}

#[test]
fn clearance_tolerance_override() {
    let mut board = bare_board(two_layers(), 1);
    assert_eq!(board.rules.clearance_tolerance_um, 1.0);
    let mut s = settings();
    s.clearance_tolerance_um = 2.5;
    let lines = apply_clearance_tolerance(&mut board, &s);
    assert_eq!(board.rules.clearance_tolerance_um, 2.5);
    assert_eq!(lines[0].text, "Applied clearance tolerance: 2.5 um.");
    s.clearance_tolerance_um = 0.0;
    apply_clearance_tolerance(&mut board, &s);
    assert_eq!(board.rules.clearance_tolerance_um, 0.0, "0 is valid");
    for bad in [-0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        s.clearance_tolerance_um = bad;
        board.rules.clearance_tolerance_um = 3.0;
        let lines = apply_clearance_tolerance(&mut board, &s);
        assert_eq!(board.rules.clearance_tolerance_um, 3.0, "{bad} is ignored");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].level, LogLevel::Warn);
        assert!(lines[0].text.starts_with(
            "Ignoring router.clearance_tolerance_um because it is invalid \
             (must be finite and >= 0): "
        ));
    }
}

// ---- C03.5: net-class exclusions through prepare_job -------------------

#[test]
fn ignore_net_classes_is_honoured_headless() {
    // Dan uses `-inc` to keep the router off hand-laid power nets. The patch
    // gives the class name as one element, the way a config file does.
    let mut cli = SettingsPatch::default();
    cli.autorouter.ignore_net_classes = Some(vec!["Power,Default".to_string()]);
    let mut input = JobInput::new(fixture("Issue558-dev-board.dsn"), "Issue558-dev-board");
    input.rules = RulesChoice::None;
    input.cli = Some(cli);
    let job = prepare_job(&input, &ResolveContext { cores: CORES }).unwrap();
    let board = &job.board;

    let class_of = |name: &str| board.rules.net_classes.get_by_name(name).expect(name);
    assert!(
        board
            .rules
            .net_classes
            .get(class_of("Power,Default"))
            .is_ignored_by_autorouter
    );
    // The DSN's `kicad_default` class is the board's `default` class.
    assert!(
        board
            .rules
            .net_classes
            .get_by_name("kicad_default")
            .is_none()
    );
    assert!(
        !board
            .rules
            .net_classes
            .get(class_of("default"))
            .is_ignored_by_autorouter
    );

    let pin_of_class = |class: &str| {
        board
            .items()
            .filter(|i| i.is_pin() && !i.net_numbers().is_empty())
            .find(|i| {
                board
                    .rules
                    .nets
                    .get(i.net_number(0))
                    .is_some_and(|n| board.rules.net_classes.get(n.net_class()).name() == class)
            })
            .unwrap_or_else(|| panic!("no pin of class {class}"))
    };
    assert!(board.has_ignored_nets(pin_of_class("Power,Default")));
    assert!(!board.has_ignored_nets(pin_of_class("default")));
}

#[test]
fn the_inc_flag_splits_at_the_comma_and_flags_the_default_class() {
    // The real command line: `-inc Power,Default` is two names, "Power" and
    // "Default", so it flags the class `default` (ignoring case), not the
    // class named "Power,Default". C01's `cli` scenario shows the same.
    let args: Vec<String> = ["-inc", "Power,Default"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut input = JobInput::new(fixture("Issue558-dev-board.dsn"), "Issue558-dev-board");
    input.rules = RulesChoice::None;
    input.cli = Some(parse_cli_arguments(&args).patch);
    let job = prepare_job(&input, &ResolveContext { cores: CORES }).unwrap();
    let classes = &job.board.rules.net_classes;
    let flags: Vec<(String, bool)> = (0..classes.count())
        .map(|i| {
            let c = classes.get(classes.nth(i).unwrap());
            (c.name().to_string(), c.is_ignored_by_autorouter)
        })
        .collect();
    assert_eq!(
        flags,
        vec![
            ("default".to_string(), true),
            ("Power,Default".to_string(), false)
        ]
    );
    // "Power" matches no class: a warning in the log (Java warns only on its
    // API path).
    assert!(
        job.log
            .iter()
            .any(|l| l.text == "ignore_net_classes: no net class named 'Power' on the board")
    );
}

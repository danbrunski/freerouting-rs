//! Q18.2: the board's minimum manufacturing rules are hard floors
//! (`router.board_minimums.*`, `settings::board::BoardMinimums`).
//!
//! Fixtures in `tests/data/quality/`:
//! - `q18-neck-floor.dsn`: net PWR2 (0.2 mm, the only class width) runs from
//!   the big pad J3 to U3-2, a 0.12 mm pad between two pads of other nets at
//!   0.35 mm pitch. A 0.2 mm trace with 0.2 mm clearance does not fit there,
//!   so Java's pin neck-down (`automatic_neckdown`) narrows the trace to the
//!   pad's width: 0.1198 mm, under the board's 0.2 mm.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::control::{
    AutorouteControl, board_min_trace_half_width, min_trace_half_width, narrowest_class_half_width,
};
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};

const NECK: &str = "crates/freerouting/tests/data/quality/q18-neck-floor.dsn";

/// `dsn` prepared with `patch` at the testing priority over `profile`.
fn prepare(dsn: &str, profile: &str, patch: SettingsPatch) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    let mut patch = patch;
    patch.profile = Some(profile.to_string());
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    input.extra_sources = vec![(Source::Testing, patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn route(prepared: PreparedJob) -> RoutingBoard {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    outcome.board.take().expect("no listener took the board")
}

/// The widths of the board's traces in board units, ascending.
fn widths(board: &RoutingBoard) -> Vec<i32> {
    let mut w: Vec<i32> = board
        .items()
        .filter_map(|i| i.as_trace())
        .map(|t| 2 * t.half_width)
        .collect();
    w.sort_unstable();
    w
}

fn minimums(track_um: Option<f64>, from_classes: Option<bool>) -> SettingsPatch {
    let mut p = SettingsPatch::default();
    p.board_minimums.track_width_um = track_um;
    p.board_minimums.from_classes = from_classes;
    p
}

fn net_number(board: &RoutingBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .expect("the net")
        .net_number
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

#[test]
fn java_pin_neck_down_goes_under_the_class_width() {
    // The width source Q18.1 found: unchanged in `legacy` with nothing known.
    let board = route(prepare(NECK, "legacy", SettingsPatch::default()));
    assert_eq!(incomplete(&board), 0);
    assert_eq!(widths(&board).first(), Some(&1198), "{:?}", widths(&board));
    // Java's `router.min_trace_width_um` floor alone lets it through at 0.15 mm.
    let user = SettingsPatch {
        min_trace_width_um: Some(100.0),
        ..SettingsPatch::default()
    };
    let board = route(prepare(NECK, "legacy", user));
    assert_eq!(widths(&board).first(), Some(&1198));
}

#[test]
fn the_board_minimum_floors_the_pin_neck_down() {
    for (track, floor, open) in [(150.0, 1500, 0), (200.0, 2000, 1)] {
        let mut patch = minimums(Some(track), None);
        // The measurement commands' user floor below the board minimum.
        patch.min_trace_width_um = Some(100.0);
        let board = route(prepare(NECK, "legacy", patch));
        let w = widths(&board);
        assert!(w.iter().all(|w| *w >= floor), "{track} um: {w:?}");
        // At 0.15 mm the neck still fits; at 0.2 mm it does not, and the
        // connection stays open (correctness before completion).
        assert_eq!(incomplete(&board), open, "{track} um");
    }
}

#[test]
fn a_board_minimum_under_the_neck_changes_nothing() {
    use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
    let java = route(prepare(NECK, "legacy", SettingsPatch::default()));
    let floored = route(prepare(NECK, "legacy", minimums(Some(100.0), None)));
    assert_eq!(
        write_ses(&java, NECK, SesStyle::Java),
        write_ses(&floored, NECK, SesStyle::Java)
    );
}

#[test]
fn from_classes_floors_at_the_narrowest_class_width() {
    let board = route(prepare(NECK, "legacy", minimums(None, Some(true))));
    assert!(
        widths(&board).iter().all(|w| *w >= 2000),
        "{:?}",
        widths(&board)
    );
    // Profile `enhanced` turns it on.
    let board = route(prepare(NECK, "enhanced", SettingsPatch::default()));
    assert!(
        widths(&board).iter().all(|w| *w >= 2000),
        "{:?}",
        widths(&board)
    );
    // A known minimum replaces the class-derived one.
    let board = route(prepare(NECK, "enhanced", minimums(Some(100.0), None)));
    assert_eq!(widths(&board).first(), Some(&1198));
}

#[test]
fn the_necked_retry_stays_at_the_board_minimum() {
    // Without the pin neck-down the first attempt at 0.2 mm fails, and
    // Java's necked retry (`router.neck_width_um`) routes PWR2 at 0.1 mm ...
    let mut retry = SettingsPatch {
        neck_width_um: Some(100.0),
        automatic_neckdown: Some(false),
        ..SettingsPatch::default()
    };
    let board = route(prepare(NECK, "legacy", retry.clone()));
    assert_eq!(incomplete(&board), 0);
    assert_eq!(widths(&board).first(), Some(&1000), "{:?}", widths(&board));
    // ... and at the board minimum when that is wider.
    retry.board_minimums.track_width_um = Some(150.0);
    let board = route(prepare(NECK, "legacy", retry));
    assert_eq!(incomplete(&board), 0);
    assert_eq!(widths(&board).first(), Some(&1500), "{:?}", widths(&board));
}

#[test]
fn the_control_floor_is_the_larger_of_the_user_floor_and_the_board_minimum() {
    let prepared = prepare(NECK, "legacy", SettingsPatch::default());
    let board = &prepared.board;
    assert_eq!(narrowest_class_half_width(board), 1000);
    assert_eq!(min_trace_half_width(board, &prepared.board_settings), 0);
    let mut patch = minimums(Some(200.0), None);
    patch.min_trace_width_um = Some(150.0);
    let p = prepare(NECK, "legacy", patch);
    assert_eq!(
        board_min_trace_half_width(&p.board, &p.board_settings),
        1000
    );
    assert_eq!(min_trace_half_width(&p.board, &p.board_settings), 1000);
    let mut patch = minimums(Some(100.0), None);
    patch.min_trace_width_um = Some(150.0);
    let p = prepare(NECK, "legacy", patch);
    assert_eq!(min_trace_half_width(&p.board, &p.board_settings), 750);
    let p = prepare(NECK, "legacy", minimums(None, Some(true)));
    assert_eq!(min_trace_half_width(&p.board, &p.board_settings), 1000);
    // A class narrower than a known minimum is raised to it.
    let p = prepare(NECK, "legacy", minimums(Some(300.0), None));
    let net = net_number(&p.board, "PWR2");
    let control = AutorouteControl::for_net_default(&p.board, net, &p.board_settings).unwrap();
    assert!(control.trace_half_width.iter().all(|w| *w == 1500));
}

/// The padstack name, diameter and drill (board units) of the via types of
/// `prepared`'s PWR2 via rule.
fn vias(prepared: &PreparedJob) -> Vec<(String, i32, i32)> {
    let board = &prepared.board;
    let net = net_number(board, "PWR2");
    let control = AutorouteControl::for_net_default(board, net, &prepared.board_settings).unwrap();
    control
        .via_rule_vias(&board.rules)
        .iter()
        .map(|id| {
            let padstack = board
                .library
                .padstacks
                .get(board.rules.via_infos.get(*id).padstack)
                .unwrap();
            let b = padstack.get_shape(0).unwrap().to_shape().bounding_box();
            let drill = (2.0 * padstack.get_drill_radius()).round() as i32;
            (padstack.name.clone(), b.width(), drill)
        })
        .collect()
}

fn via_minimums(diameter: Option<f64>, drill: Option<f64>, annular: Option<f64>) -> SettingsPatch {
    let mut patch = SettingsPatch::default();
    patch.board_minimums.via_diameter_um = diameter;
    patch.board_minimums.via_drill_um = drill;
    patch.board_minimums.via_annular_um = annular;
    patch
}

#[test]
fn vias_under_the_board_minimums_are_made_at_the_floor() {
    // VIA600 is 600 um across; its name is not KiCad's, so its drill is
    // estimated at 45 % of the pad: 270 um.
    let p = prepare(NECK, "legacy", SettingsPatch::default());
    assert_eq!(vias(&p), [("VIA600".to_string(), 6000, 2700)]);
    // At the floor already: unchanged, no warning.
    let p = prepare(
        NECK,
        "legacy",
        via_minimums(Some(600.0), Some(270.0), Some(165.0)),
    );
    assert_eq!(vias(&p), [("VIA600".to_string(), 6000, 2700)]);
    assert!(!p.log.iter().any(|l| l.text.contains("board minimums")));
    for (diameter, drill, annular, expected) in [
        (Some(601.0), None, None, ("Via[0-1]_601:270_um", 6010, 2700)),
        (None, Some(300.0), None, ("Via[0-1]_600:300_um", 6000, 3000)),
        // The drill grows, and the annular ring grows the pad with it.
        (
            Some(500.0),
            Some(300.0),
            Some(200.0),
            ("Via[0-1]_700:300_um", 7000, 3000),
        ),
        (None, None, Some(200.0), ("Via[0-1]_670:270_um", 6700, 2700)),
    ] {
        let p = prepare(NECK, "legacy", via_minimums(diameter, drill, annular));
        assert_eq!(
            vias(&p),
            [(expected.0.to_string(), expected.1, expected.2)],
            "{diameter:?} {drill:?} {annular:?}"
        );
        // One warning per net class of the via type, naming the class.
        let warnings: Vec<&str> = p
            .log
            .iter()
            .filter(|l| l.text.contains("below the board minimums"))
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].starts_with("Net class 'default': via 'VIA600' (600 um"));
        assert!(warnings[1].starts_with("Net class 'Narrow_Pin': via 'VIA600-Narrow_Pin'"));
        assert!(warnings[0].contains(expected.0), "{}", warnings[0]);
    }
}

#[test]
fn routed_vias_meet_the_board_minimums() {
    // q14-hv-via: net HV must leave a cage of fixed wires through a via
    // (`tests/via_clearance_net_class.rs`). The default class's 0.6 mm via
    // is made at the 0.7 mm floor; every via the router places is at least
    // that, and the session's library carries the new padstack.
    use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
    const HV: &str = "crates/freerouting/tests/data/quality/q14-hv-via.dsn";
    let board = route(prepare(HV, "legacy", via_minimums(Some(700.0), None, None)));
    let vias: Vec<_> = board.items().filter_map(|i| i.as_via()).collect();
    assert!(!vias.is_empty());
    for via in &vias {
        let padstack = board.library.padstacks.get(via.drill.padstack).unwrap();
        let b = padstack.get_shape(0).unwrap().to_shape().bounding_box();
        assert!(b.width() >= 7000, "{} {}", padstack.name, b.width());
    }
    let ses = write_ses(&board, HV, SesStyle::Java);
    assert!(ses.contains("Via[0-1]_700:300_um"), "{ses}");
}

#[test]
fn a_kicad_pcb_input_passes_its_board_setup_minimums() {
    use freerouting::io::kicad::{ReadOptions, read_kicad_input};
    let pcb = repo_root().join("fixtures/kicad/Issue180-Test/Test.kicad_pcb");
    let read = read_kicad_input(&pcb, ReadOptions::default()).unwrap();
    let m = read.extras.board_minimums;
    assert_eq!(
        (
            m.track_width,
            m.via_diameter,
            m.through_drill,
            m.copper_edge_clearance
        ),
        (Some(254_000), Some(381_000), Some(254_000), Some(254_000))
    );
    let mut input = JobInput::new(Vec::new(), "Test");
    input.set_kicad(read);
    let mut cli = SettingsPatch::default();
    cli.board_minimums.via_drill_um = Some(300.0);
    input.cli = Some(cli);
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
    let s = &prepared.settings;
    assert_eq!(s.board_minimums.track_width_um, Some(254.0));
    assert_eq!(s.board_minimums.via_diameter_um, Some(381.0));
    // The command line wins over the project file.
    assert_eq!(s.board_minimums.via_drill_um, Some(300.0));
    assert_eq!(s.copper_to_edge_clearance_um, 254.0);
    assert_eq!(s.board_minimums.via_annular_um, Some(254.0));
    assert_eq!(s.hole_clearance_um, 381.0);
}

#[test]
fn kicad_board_minimums_default_to_kicads_and_zero_is_none() {
    use freerouting::io::kicad::reader::project::{BoardMinimums, Project};
    let none = Project::parse(r#"{"board": {"design_settings": {"rules": {}}}}"#).unwrap();
    assert_eq!(none.minimums, BoardMinimums::default());
    assert_eq!(Project::default().minimums, BoardMinimums::default());
    assert_eq!(
        BoardMinimums::default(),
        BoardMinimums {
            track_width: Some(200_000),
            via_diameter: Some(500_000),
            through_drill: Some(300_000),
            via_annular: Some(100_000),
            copper_edge_clearance: Some(500_000),
            hole_clearance: Some(250_000),
            clearance: None,
            hole_to_hole: Some(250_000),
        }
    );
    let zero = Project::parse(
        r#"{"board": {"design_settings": {"rules": {"min_track_width": 0.0,
            "min_via_diameter": 0.0, "min_through_hole_diameter": 0.0,
            "min_via_annular_width": 0.0, "min_copper_edge_clearance": 0.0,
            "min_hole_clearance": 0.0, "min_hole_to_hole": 0.0}}}}"#,
    )
    .unwrap();
    assert_eq!(zero.minimums, BoardMinimums::NONE);
    assert!(freerouting::job::prepare::board_minimums_patch(&zero.minimums).is_none());
}

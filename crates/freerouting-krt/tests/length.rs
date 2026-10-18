//! X07: length and time matching with the `krt-meander` stage on synthetic
//! boards. The boards are in `tests/data/`, not `fixtures/krt/` as the
//! checklist planned: every DSN under `fixtures/` needs a Java reference
//! (`reference_load.rs`), as X02 and Q01 found.
//!
//! - `x07-measure.dsn`: net M, 20 mm on F.Cu, a through via, then 15 mm and
//!   15·√2 mm on B.Cu to a through-hole pin; net N, 40 mm on F.Cu without
//!   vias, in a class with a 100 mm minimum (X07.1);
//! - `x07-group.dsn`: nets A, B, C of 50, 60 and 70 mm, 20 mm apart (X07.4);
//! - `x07-tight.dsn`: net A, 50 mm with a 60 mm class minimum, between
//!   parallel K and L traces 0.7 mm away, so no bump fits (X07.4);
//! - `x07-time.dsn`: net T1, 70 mm with two vias (F.Cu, B.Cu, F.Cu), and
//!   net T2, 60 mm on F.Cu (X07.5).
//!
//! The stackups are written here as the KiCad reader (K05.1) returns them:
//! these boards are DSN files, which carry none.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{StageCtx, StageRegistry};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::drc::DesignRulesChecker;
use freerouting::geometry::TileShapeOps;
use freerouting::io::kicad::reader::layers::StackupLayer;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};
use freerouting_krt::length::{
    KrtMeander, LengthModel, MEANDER_ID, MatchReport, Stackup, Status, Units, measure_net,
    timing::Timing,
};

const MEASURE: &str = "crates/freerouting-krt/tests/data/x07-measure.dsn";
const GROUP: &str = "crates/freerouting-krt/tests/data/x07-group.dsn";
const TIGHT: &str = "crates/freerouting-krt/tests/data/x07-tight.dsn";
const TIME: &str = "crates/freerouting-krt/tests/data/x07-time.dsn";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `dsn` with the test profile and `extra`
/// (`path=value`).
fn prepare(dsn: &str, extra: &[(&str, &str)]) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = default_profile();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(10);
    for (key, value) in extra {
        profile
            .patch
            .set_path(key, value)
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

/// Runs `stage` on the prepared board directly (the stage after the
/// optimizer, without the routing passes).
fn run_stage(prepared: &mut PreparedJob, stage: &KrtMeander) -> MatchReport {
    let cancel = CancelToken::new();
    let ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    stage.run_on(&mut prepared.board, &ctx)
}

fn stage(groups: &[&str]) -> KrtMeander {
    KrtMeander {
        model: LengthModel::none(),
        groups: groups.iter().map(|g| g.to_string()).collect(),
    }
}

/// Routes the prepared job with this crate's registry; returns the board
/// and the log.
fn route(prepared: PreparedJob) -> (RoutingBoard, Vec<String>) {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: Some(&registry),
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    let log = outcome.log.iter().map(|l| l.text.clone()).collect();
    (outcome.board.take().expect("the routed board"), log)
}

fn ses(board: &RoutingBoard, dsn: &str) -> String {
    let name = Path::new(dsn).file_stem().unwrap().to_string_lossy();
    write_ses(board, &name, SesStyle::Java)
}

fn net(board: &BasicBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("net {name}"))
        .net_number
}

fn mm(board: &BasicBoard, value: f64) -> f64 {
    Units::of(board).to_um(value) / 1000.0
}

fn length_mm(board: &BasicBoard, name: &str) -> f64 {
    mm(board, board.net_trace_length(net(board, name)))
}

fn layer(name: &str, kind: &str, um: i64, er: Option<f64>) -> StackupLayer {
    StackupLayer {
        name: name.into(),
        kind: kind.into(),
        thickness_nm: Some(um * 1000),
        material: None,
        epsilon_r: er,
        loss_tangent: None,
        sheets: 1,
    }
}

/// KiCad's default two-layer stackup with a 1.53 mm core of εr 4.5: the
/// copper-to-copper span is 1.6 mm.
fn two_layer() -> Stackup {
    Stackup::new(vec![
        layer("F.Mask", "Top Solder Mask", 10, Some(3.3)),
        layer("F.Cu", "copper", 35, None),
        layer("dielectric 1", "core", 1530, Some(4.5)),
        layer("B.Cu", "copper", 35, None),
        layer("B.Mask", "Bottom Solder Mask", 10, Some(3.3)),
    ])
    .unwrap()
}

fn clearance_violations(board: &BasicBoard) -> usize {
    DesignRulesChecker::new(board)
        .all_clearance_violations()
        .len()
}

/// The clearance violations as (first item, second item, layer).
fn violation_keys(board: &BasicBoard) -> Vec<(i32, i32, i32)> {
    DesignRulesChecker::new(board)
        .all_clearance_violations()
        .iter()
        .map(|v| (v.first_item.0, v.second_item.0, v.layer))
        .collect()
}

fn length_violations(board: &BasicBoard) -> usize {
    DesignRulesChecker::new(board).length_violation_count()
}

// ---------------------------------------------------------------------------
// X07.1: measurement
// ---------------------------------------------------------------------------

#[test]
fn measured_lengths_equal_hand_values() {
    let board = prepare(MEASURE, &[]).board;
    let m = net(&board, "M");
    let mut model = LengthModel {
        stackup: Some(two_layer()),
        ..LengthModel::default()
    };
    model
        .die_lengths
        .insert(("M1".to_string(), "1".to_string()), 500_000);
    let len = measure_net(&board, m, &model);
    let um = |v: f64| Units::of(&board).to_um(v);
    let trace_um = 20_000.0 + 15_000.0 + 15_000.0 * std::f64::consts::SQRT_2;
    assert!((um(len.trace) - trace_um).abs() < 1.0, "{}", um(len.trace));
    assert_eq!(len.vias, 1);
    assert!(len.stackup_known);
    assert!(
        (um(len.via_barrels) - 1600.0).abs() < 1.0,
        "{}",
        um(len.via_barrels)
    );
    assert!((um(len.pad_to_die) - 500.0).abs() < 1.0);
    assert!((um(len.total()) - (trace_um + 1600.0 + 500.0)).abs() < 1.0);

    // Without stackup data: the barrels count 0 and the report says so.
    let bare = measure_net(&board, m, &LengthModel::none());
    assert!(!bare.stackup_known);
    assert_eq!((bare.via_barrels, bare.pad_to_die), (0.0, 0.0));
    assert_eq!(bare.trace, len.trace);
    // A stackup with the wrong number of copper layers does not fit.
    let three = Stackup::new(vec![
        layer("F.Cu", "copper", 35, None),
        layer("d1", "core", 500, Some(4.5)),
        layer("In1.Cu", "copper", 35, None),
        layer("d2", "core", 500, Some(4.5)),
        layer("B.Cu", "copper", 35, None),
    ]);
    let misfit = LengthModel {
        stackup: three,
        ..LengthModel::default()
    };
    assert!(!measure_net(&board, m, &misfit).stackup_known);
    let mut prepared = prepare(MEASURE, &[]);
    let report = run_stage(&mut prepared, &stage(&[]));
    assert!(report.stackup_unknown);
    let log = report.log(&prepared.board);
    assert!(
        log[0].starts_with("krt-meander: stackup unknown"),
        "{log:?}"
    );
}

#[test]
fn a_net_without_vias_measures_what_java_checks() {
    let board = prepare(MEASURE, &[]).board;
    let n = net(&board, "N");
    let len = measure_net(&board, n, &LengthModel::none());
    assert_eq!(len.vias, 0);
    let min = board
        .rules
        .nets
        .get(n)
        .unwrap()
        .minimum_trace_length(&board.rules.net_classes);
    assert!((mm(&board, min) - 100.0).abs() < 1e-9);
    // Java's NetIncompletes: violation = length - minimum for a short net.
    let violation = DesignRulesChecker::new(&board).length_violation(n);
    assert!(violation < 0.0);
    assert!((len.total() - (min + violation)).abs() < 1e-6);
    assert!((mm(&board, len.total()) - 40.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// X07.4: the matching loop
// ---------------------------------------------------------------------------

#[test]
fn a_group_of_three_ends_within_tolerance_of_the_longest() {
    let mut prepared = prepare(GROUP, &[]);
    let drc_before = clearance_violations(&prepared.board);
    let java_before = length_violations(&prepared.board);
    let report = run_stage(&mut prepared, &stage(&["G=A|B|C@100um"]));
    let board = &prepared.board;
    for line in report.log(board) {
        println!("{line}");
    }
    assert_eq!(report.errors, Vec::<String>::new());
    assert_eq!(report.nets.len(), 2, "C is the target");
    for n in &report.nets {
        assert_eq!(n.status, Status::Met, "{n:?}");
        assert!(n.bumps > 0);
    }
    let c = length_mm(board, "C");
    assert!((c - 70.0).abs() < 1e-9);
    for name in ["A", "B"] {
        let l = length_mm(board, name);
        assert!(l <= c && l >= c - 0.1, "{name}: {l} mm against {c} mm");
    }
    assert_eq!(clearance_violations(board), drc_before);
    assert!(length_violations(board) <= java_before);
    assert_eq!(board.items().filter(|i| i.is_via()).count(), 0);
    for item in board.items() {
        if let Some(t) = item.as_trace() {
            assert!(t.polyline.is_multiple_of_45_degree(), "{:?}", item.id());
        }
    }
    // The nets stay connected.
    let mut drc = DesignRulesChecker::new(board);
    assert_eq!(drc.incomplete_count(), 0);
}

#[test]
fn no_room_reports_the_shortfall_and_adds_no_drc_item() {
    let mut prepared = prepare(TIGHT, &[]);
    let before = ses(&prepared.board, TIGHT);
    let drc_before = violation_keys(&prepared.board);
    let java_before = length_violations(&prepared.board);
    let report = run_stage(&mut prepared, &stage(&[]));
    let board = &prepared.board;
    let log = report.log(board);
    println!("{log:?}");
    assert_eq!(report.nets.len(), 1);
    let a = &report.nets[0];
    assert_eq!(a.net, net(board, "A"));
    let Status::Shortfall(short) = a.status else {
        panic!("{a:?}");
    };
    assert!(
        (mm(board, short) - 10.0).abs() < 1e-6,
        "{}",
        mm(board, short)
    );
    assert_eq!(a.bumps, 0);
    assert!(a.rejected > 0);
    assert!(
        log.iter()
            .any(|l| l.contains("net A (class window)") && l.contains("shortfall"))
    );
    assert_eq!(violation_keys(board), drc_before);
    assert_eq!(length_violations(board), java_before);
    assert_eq!(ses(board, TIGHT), before);
}

#[test]
fn running_the_stage_twice_changes_nothing_the_second_time() {
    for (dsn, groups) in [
        (GROUP, vec!["G=A|B|C@100um"]),
        (TIGHT, vec![]),
        (MEASURE, vec![]),
    ] {
        let mut prepared = prepare(dsn, &[]);
        let s = stage(&groups);
        run_stage(&mut prepared, &s);
        let first = ses(&prepared.board, dsn);
        let again = run_stage(&mut prepared, &s);
        assert_eq!(ses(&prepared.board, dsn), first, "{dsn}");
        assert!(again.nets.iter().all(|n| n.bumps == 0), "{dsn}: {again:?}");
    }
}

#[test]
fn a_class_minimum_lengthens_a_net_to_the_minimum() {
    // The measure board's N: 40 mm with a 100 mm class minimum and room
    // above it (but the board edge 5 mm away): bumps go below.
    let mut prepared = prepare(MEASURE, &[]);
    let java_before = length_violations(&prepared.board);
    let report = run_stage(&mut prepared, &stage(&[]));
    let board = &prepared.board;
    println!("{:?}", report.log(board));
    let n = report
        .nets
        .iter()
        .find(|o| o.net == net(board, "N"))
        .expect("N has a target");
    // 60 mm more in a 40 mm run is more than a run holds at 1 mm
    // amplitude: the shortfall is reported, and the net got longer.
    let l = length_mm(board, "N");
    assert!(l > 40.0 + 10.0, "{l}");
    match n.status {
        Status::Met => assert!(l >= 100.0),
        Status::Shortfall(s) => assert!((mm(board, s) - (100.0 - l)).abs() < 1e-6),
        other => panic!("{other:?}"),
    }
    assert!(length_violations(board) <= java_before);
    assert_eq!(clearance_violations(board), 0);
}

#[test]
fn the_stage_is_selected_by_its_key_and_off_by_default() {
    let (off, _) = route(prepare(GROUP, &[]));
    let (builtin, _) = {
        let prepared = prepare(GROUP, &[]);
        let job = PipelineJob {
            settings: &prepared.settings,
            board_settings: &prepared.board_settings,
            cancel: Arc::new(CancelToken::new()),
            events: &NullEvents,
            deadline: None,
            registry: None,
        };
        let mut outcome = run_pipeline(prepared.board, &job).unwrap();
        (outcome.board.take().unwrap(), ())
    };
    assert_eq!(
        ses(&off, GROUP),
        ses(&builtin, GROUP),
        "registered, unselected"
    );
    // Groups alone select nothing.
    let (groups_only, _) = route(prepare(GROUP, &[("length_matching.groups", "G=A|B|C")]));
    assert_eq!(ses(&groups_only, GROUP), ses(&off, GROUP));
    let (on, log) = route(prepare(
        GROUP,
        &[
            ("special.length_matching", MEANDER_ID),
            ("length_matching.groups", "G=A|B|C@100um"),
        ],
    ));
    assert_ne!(ses(&on, GROUP), ses(&off, GROUP));
    assert!(
        log.iter()
            .any(|l| l.contains("router.special.length_matching=krt-meander")),
        "{log:?}"
    );
    let line = log
        .iter()
        .find(|l| l.starts_with("krt-meander: net A (group G)"))
        .unwrap_or_else(|| panic!("{log:?}"));
    assert!(line.ends_with(", met"), "{line}");
    let c = length_mm(&on, "C");
    let a = length_mm(&on, "A");
    assert!(a <= c && a >= c - 0.1, "{a} vs {c}");
}

#[test]
fn a_settings_error_changes_nothing() {
    for groups in [vec!["G="], vec!["G=NOPE*"], vec!["G=A@3parsecs"]] {
        let mut prepared = prepare(GROUP, &[]);
        let before = ses(&prepared.board, GROUP);
        let report = run_stage(&mut prepared, &stage(&groups));
        assert_eq!(report.nets.len(), 0, "{groups:?}");
        assert_eq!(report.errors.len(), 1, "{groups:?}");
        assert!(report.errors[0].starts_with("settings error: router.length_matching.groups"));
        assert_eq!(ses(&prepared.board, GROUP), before);
    }
}

// ---------------------------------------------------------------------------
// X07.5: time matching
// ---------------------------------------------------------------------------

#[test]
fn a_time_group_differs_in_length_by_the_permittivity_ratio() {
    let model = LengthModel {
        stackup: Some(two_layer()),
        ..LengthModel::default()
    };
    let run = |group: &str| {
        let mut prepared = prepare(TIME, &[]);
        let s = stage(&[group]).with_model(model.clone());
        let report = run_stage(&mut prepared, &s);
        println!("{:?}", report.log(&prepared.board));
        assert!(
            report.nets.iter().all(|n| n.status == Status::Met),
            "{report:?}"
        );
        prepared.board
    };
    let barrels_mm = 2.0 * 1.6;
    // Length matched: T2's trace = T1's trace plus its two barrels.
    let by_length = run("T=T1|T2@10um");
    let d_len = length_mm(&by_length, "T2") - length_mm(&by_length, "T1");
    assert!((d_len - barrels_mm).abs() < 0.011, "{d_len}");
    // Time matched: the barrels are delayed at εr, the F.Cu trace at
    // (εr + 1) / 2, so T2 needs sqrt(εr / ((εr + 1) / 2)) times as much.
    let by_time = run("T=T1|T2@0.05ps");
    let d_time = length_mm(&by_time, "T2") - length_mm(&by_time, "T1");
    let ratio = (4.5_f64 / ((4.5 + 1.0) / 2.0)).sqrt();
    assert!(
        (d_time / barrels_mm - ratio).abs() < 0.005,
        "{d_time} mm: ratio {} against {ratio}",
        d_time / barrels_mm
    );
    let timing = Timing::new(&by_time, &model).unwrap();
    let t1 = timing.net_delay_ps(&by_time, net(&by_time, "T1"));
    let t2 = timing.net_delay_ps(&by_time, net(&by_time, "T2"));
    assert!(t2 <= t1 && t2 >= t1 - 0.05, "{t1} ps, {t2} ps");
}

#[test]
fn time_targets_without_a_stackup_are_refused() {
    let mut prepared = prepare(TIME, &[]);
    let before = ses(&prepared.board, TIME);
    let report = run_stage(&mut prepared, &stage(&["T=T1|T2@1ps"]));
    assert_eq!(report.nets.len(), 0);
    assert_eq!(report.errors.len(), 1);
    assert!(
        report.errors[0].contains("needs the board stackup"),
        "{}",
        report.errors[0]
    );
    let log = report.log(&prepared.board);
    assert!(
        log.iter()
            .any(|l| l.contains("group T: a time tolerance (1 ps)"))
    );
    assert_eq!(ses(&prepared.board, TIME), before);
}

/// K05.1's stackup reaches the model: the 4-layer KiCad fixture's copper
/// layers fit the board and give the microstrip and stripline delays.
#[test]
fn the_kicad_readers_stackup_feeds_the_model() {
    use freerouting::io::kicad::{ReadOptions, read_kicad_input};
    use freerouting_krt::length::timing::ps_per_mm;
    let path = repo_root().join("fixtures/kicad/Issue269-min_fr_test/min_fr_test.kicad_pcb");
    let input = read_kicad_input(&path, ReadOptions::default()).unwrap();
    let model = LengthModel::from_kicad(&input.extras);
    let result = freerouting::io::read_kicad_board(&path);
    let board = result.board().expect("the board");
    let stackup = model
        .stackup_for(board)
        .expect("the stackup fits the board");
    assert_eq!(stackup.copper_count(), 4);
    assert_eq!(
        stackup.barrel_nm(0, 3),
        Some((4 * 35 + 100 + 1240 + 100) * 1000)
    );
    let timing = Timing::new(board, &model).unwrap();
    assert!((timing.layer_ps_per_mm[0] - ps_per_mm((4.5 + 1.0) / 2.0)).abs() < 1e-12);
    assert!((timing.layer_ps_per_mm[1] - ps_per_mm(4.5)).abs() < 1e-12);
    assert!((timing.layer_ps_per_mm[3] - ps_per_mm((4.5 + 1.0) / 2.0)).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Determinism (tools/determinism-check.sh)
// ---------------------------------------------------------------------------

/// Writes the session of each fixture after the stage into `$X07_SES_OUT`
/// (a directory), for `tools/determinism-check.sh`.
#[test]
#[ignore = "run by tools/determinism-check.sh with X07_SES_OUT"]
fn write_meander_sessions() {
    let out = PathBuf::from(std::env::var("X07_SES_OUT").expect("X07_SES_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let model = LengthModel {
        stackup: Some(two_layer()),
        ..LengthModel::default()
    };
    for (dsn, groups) in [
        (GROUP, vec!["G=A|B|C@100um"]),
        (TIGHT, vec![]),
        (MEASURE, vec![]),
        (TIME, vec!["T=T1|T2@0.05ps"]),
    ] {
        let mut prepared = prepare(dsn, &[]);
        let s = stage(&groups).with_model(model.clone());
        let report = run_stage(&mut prepared, &s);
        let name = Path::new(dsn)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        std::fs::write(out.join(format!("{name}.ses")), ses(&prepared.board, dsn)).unwrap();
        std::fs::write(
            out.join(format!("{name}.log")),
            report.log(&prepared.board).join("\n"),
        )
        .unwrap();
    }
}

// ---------------------------------------------------------------------------
// X07.6: the registered measurement (docs/roadmap/prereg/X07.json)
// ---------------------------------------------------------------------------

/// The job's load sequence on `path` with the default settings.
fn prepare_default(path: &Path) -> PreparedJob {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let name = path.to_string_lossy().to_string();
    let mut input = JobInput::new(bytes, JobInput::design_name_of(&name));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The vias whose centre lies inside a pad of any net, on a shared layer.
fn vias_in_pads(board: &BasicBoard) -> usize {
    let pins: Vec<_> = board.items().filter(|i| i.is_pin()).collect();
    board
        .items()
        .filter(|v| v.is_via())
        .filter(|v| {
            let centre = v.drill().expect("a via has a drill").center;
            pins.iter().any(|pin| {
                (0..board.tile_shape_count(pin)).any(|i| {
                    v.is_on_layer(pin.shape_layer(i))
                        && board
                            .tile_shape(pin, i)
                            .is_some_and(|s| s.contains_float(centre.to_float()))
                })
            })
        })
        .count()
}

/// One arm's metrics (the prereg's names).
fn arm_metrics(board: &BasicBoard, groups: &[String], report: Option<&MatchReport>) -> String {
    let mut drc = DesignRulesChecker::new(board);
    let out_of_tolerance = match report {
        // The stage's own verdicts: a net it left short of its window.
        Some(r) => r
            .nets
            .iter()
            .filter(|n| !matches!(n.status, Status::Met))
            .count(),
        // Off: the nets the stage would have given a target.
        None => {
            use freerouting_krt::length::targets::{board_nets, parse_groups, resolve_group};
            let nets = board_nets(board);
            let resolved: Vec<_> = parse_groups(groups)
                .unwrap()
                .iter()
                .map(|g| resolve_group(g, &nets).unwrap())
                .collect();
            count_out_of_tolerance(board, &resolved)
        }
    };
    format!(
        "{{\"via_in_pad\": {}, \"clearance_violations\": {}, \"unrouted\": {}, \
         \"java_length_violations\": {}, \"nets_out_of_tolerance\": {}}}",
        vias_in_pads(board),
        drc.all_clearance_violations().len(),
        drc.incomplete_count(),
        drc.length_violation_count(),
        out_of_tolerance
    )
}

/// The nets of `groups` and class windows that are outside their window.
fn count_out_of_tolerance(
    board: &BasicBoard,
    groups: &[freerouting_krt::length::targets::Group],
) -> usize {
    use freerouting_krt::length::targets::{class_target, class_window, group_targets};
    let model = LengthModel::none();
    let units = Units::of(board);
    let mut grouped = std::collections::BTreeSet::new();
    let mut count = 0;
    for g in groups {
        let tol = match g.tolerance {
            freerouting_krt::length::targets::Tolerance::Um(um) => units.from_um(um),
            freerouting_krt::length::targets::Tolerance::Ps(_) => continue,
        };
        let measured: Vec<(i32, f64)> = g
            .nets
            .iter()
            .map(|&n| (n, measure_net(board, n, &model).total()))
            .collect();
        count += group_targets(&measured, tol).len();
        grouped.extend(g.nets.iter().copied());
    }
    for net in 1..=board.rules.nets.max_net_number() {
        if grouped.contains(&net) {
            continue;
        }
        if let Some((min, max)) = class_window(board, net) {
            let v = measure_net(board, net, &model).total();
            count += usize::from(class_target(net, min, max, v, 0.0).is_some());
        }
    }
    count
}

/// For each row of `$X07_SET` (`<dsn>  # ... groups=G1=..;G2=..`): routes
/// with the default settings and this crate's registry, stage unselected
/// (arm `off`), then runs `krt-meander` with the row's groups on that board
/// (arm `on`, which is what selecting the stage does: it runs last), and
/// writes a `freerouting-gate-records-1` file to `$X07_RECORDS`.
#[test]
#[ignore = "the registered X07.6 run; waits for P04 (docs/roadmap/prereg/X07.json)"]
fn measurement_records_on_a_set() {
    let set = std::env::var("X07_SET").expect("X07_SET");
    let out = std::env::var("X07_RECORDS").expect("X07_RECORDS");
    let text = std::fs::read_to_string(repo_root().join(&set)).unwrap();
    let mut rows = Vec::new();
    for line in text.lines() {
        let (path, comment) = line.split_once('#').unwrap_or((line, ""));
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        let full = repo_root().join(path);
        if !full.exists() {
            eprintln!("skipped (missing): {path}");
            continue;
        }
        let groups: Vec<String> = comment
            .split_whitespace()
            .find_map(|w| w.strip_prefix("groups="))
            .map(|g| g.split(';').map(str::to_string).collect())
            .unwrap_or_default();
        let (board, _) = route(prepare_default(&full));
        let off = arm_metrics(&board, &groups, None);
        let mut prepared = prepare_default(&full);
        prepared.board = board;
        let s = stage(&groups.iter().map(String::as_str).collect::<Vec<_>>());
        let report = run_stage(&mut prepared, &s);
        for l in report.log(&prepared.board) {
            eprintln!("{path}: {l}");
        }
        let on = arm_metrics(&prepared.board, &groups, Some(&report));
        let stem = full.file_stem().unwrap().to_string_lossy().to_string();
        rows.push(format!(
            "\"{stem}\": {{\"board\": \"{path}\", \"arms\": {{\"off\": {off}, \"on\": {on}}}}}"
        ));
    }
    let json = format!(
        "{{\"format\": \"freerouting-gate-records-1\", \"arms\": {{\"off\": {{\"engine\": \
         \"freerouting-rs, krt-meander unselected\"}}, \"on\": {{\"engine\": \
         \"freerouting-rs, special.length_matching=krt-meander\"}}}}, \"rows\": {{{}}}}}",
        rows.join(", ")
    );
    std::fs::write(out, json).unwrap();
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

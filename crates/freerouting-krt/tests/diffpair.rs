//! X08: differential pairs with the `krt-diffpair` stage on synthetic
//! boards. The boards are in `tests/data/`, not `fixtures/krt/` as the
//! checklist planned: every DSN under `fixtures/` needs a Java reference
//! (`reference_load.rs`), as X02, Q01 and X07 found.
//!
//! - `x08-straight.dsn`: one pair `D_P`/`D_N` between two two-pad parts
//!   30 mm apart on F.Cu (pads 0.5 mm apart), with a 1 mm pad of net F on
//!   the straight line between them; width 0.2 mm, clearance 0.2 mm (X08.2);
//! - `x08-connector.dsn`: `USB_D+`/`USB_D-` on pins 3 and 4 of two six-pin
//!   0.5 mm pitch connectors 20 mm apart, beside VBUS, GND, ID and GND2;
//!   width 0.2 mm, clearance 0.15 mm (X08.1, X08.4).
//!
//! With name inference the pair's width is the net's trace width and its
//! gap the class's clearance; a KiCad project class (K05.3) is written here
//! as the reader returns it, since these boards are DSN files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{StageCtx, StageRegistry};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::drc::DesignRulesChecker;
use freerouting::geometry::{FloatPoint, Point, Polyline, TileShapeOps};
use freerouting::io::kicad::reader::project::Project;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};
use freerouting_krt::diffpair::centreline::{
    CentrelineParams, min_foreign_slack, net_clearance_class, pair_ends, route_centreline,
};
use freerouting_krt::diffpair::measure::polyline_distance;
use freerouting_krt::diffpair::report::coupling;
use freerouting_krt::diffpair::{
    DIFFPAIR_ID, DiffPairReport, KrtDiffPair, PairClasses, PairOptions, PairStatus, build_pairs,
};
use freerouting_krt::length::Units;

const STRAIGHT: &str = "crates/freerouting-krt/tests/data/x08-straight.dsn";
const CONNECTOR: &str = "crates/freerouting-krt/tests/data/x08-connector.dsn";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `dsn` with the test profile and `extra`.
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

fn with_ctx<T>(prepared: &PreparedJob, f: impl FnOnce(&StageCtx) -> T) -> T {
    let cancel = CancelToken::new();
    let ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    f(&ctx)
}

fn run_stage(prepared: &mut PreparedJob, stage: &KrtDiffPair) -> DiffPairReport {
    let cancel = CancelToken::new();
    let ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    stage.run_on(&mut prepared.board, &ctx)
}

fn names_stage(forced_leg_failures: usize) -> KrtDiffPair {
    KrtDiffPair {
        classes: PairClasses::none(),
        infer_from_names: true,
        options: PairOptions {
            forced_leg_failures,
            ..PairOptions::default()
        },
    }
}

/// Routes the prepared job with this crate's registry; returns the board
/// and the log.
fn route(prepared: PreparedJob, registered: bool) -> (RoutingBoard, Vec<String>) {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: registered.then_some(&registry),
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

fn um(board: &BasicBoard, v: f64) -> f64 {
    Units::of(board).to_um(v)
}

fn clearance_violations(board: &BasicBoard) -> usize {
    DesignRulesChecker::new(board)
        .all_clearance_violations()
        .len()
}

fn incompletes(board: &BasicBoard, net: i32) -> usize {
    DesignRulesChecker::new(board).incomplete_count_of(net)
}

/// A KiCad project whose class `DP` (diff pair 0.2 mm wide, 0.1 mm apart)
/// takes the nets matching `pattern`.
fn project(pattern: &str) -> Project {
    Project::parse(&format!(
        r#"{{"net_settings": {{"classes": [
            {{"name": "Default", "clearance": 0.15, "track_width": 0.2}},
            {{"name": "DP", "priority": 0, "diff_pair_width": 0.2, "diff_pair_gap": 0.1}}],
          "netclass_patterns": [{{"netclass": "DP", "pattern": "{pattern}"}}]}}}}"#
    ))
    .expect("project")
}

// ---------------------------------------------------------------------------
// X08.1: the pair model
// ---------------------------------------------------------------------------

#[test]
fn usb_d_plus_minus_pair_only_with_the_fallback_on() {
    let board = prepare(CONNECTOR, &[]).board;
    let (p, n) = (net(&board, "USB_D+"), net(&board, "USB_D-"));
    let off = build_pairs(&board, &PairClasses::none(), false);
    assert!(off.pairs.is_empty(), "{off:?}");
    let on = build_pairs(&board, &PairClasses::none(), true);
    assert_eq!(on.pairs.len(), 1, "{on:?}");
    let pair = &on.pairs[0];
    assert_eq!((pair.p, pair.n), (p, n));
    assert_eq!(pair.base, "USB_D");
    assert_eq!(pair.class, "names");
    assert_eq!(um(&board, f64::from(pair.width[0])), 200.0);
    assert_eq!(um(&board, f64::from(pair.gap[0])), 150.0);
    assert!(on.notes.is_empty(), "{:?}", on.notes);
    // A project class without the fallback: the class's width and gap.
    let classes = PairClasses::from_project(&project("USB_*"));
    let from_class = build_pairs(&board, &classes, false);
    assert_eq!(from_class.pairs.len(), 1, "{from_class:?}");
    let pair = &from_class.pairs[0];
    assert_eq!((pair.p, pair.n, pair.class.as_str()), (p, n, "DP"));
    assert_eq!(um(&board, f64::from(pair.width[1])), 200.0);
    assert_eq!(um(&board, f64::from(pair.gap[1])), 100.0);
    // A class member with no partner is reported.
    let lonely = build_pairs(&board, &PairClasses::from_project(&project("*D+")), false);
    assert!(lonely.pairs.is_empty());
    assert_eq!(lonely.notes.len(), 1, "{:?}", lonely.notes);
    assert!(
        lonely.notes[0].contains("(USB_D+) is in pair class DP but has no partner"),
        "{}",
        lonely.notes[0]
    );
}

// ---------------------------------------------------------------------------
// X08.2: the centreline
// ---------------------------------------------------------------------------

#[test]
fn the_centreline_routes_and_keeps_the_compound_clearance() {
    let prepared = prepare(STRAIGHT, &[]);
    let board = &prepared.board;
    let model = build_pairs(board, &PairClasses::none(), true);
    assert_eq!(model.pairs.len(), 1, "{model:?}");
    let pair = &model.pairs[0];
    let ends = pair_ends(board, pair).expect("ends");
    assert_eq!(ends.layer, 0);
    let params = CentrelineParams {
        layer: 0,
        start: ends.start.terminal(&ends.end, 0.0),
        end: ends.end.terminal(&ends.start, 0.0),
        work_budget: PairOptions::default().centreline_budget_ms,
    };
    let centre = with_ctx(&prepared, |ctx| route_centreline(board, ctx, pair, &params))
        .expect("the centreline routes");
    assert_eq!(centre.first_corner(), Point::Int(params.start));
    assert_eq!(centre.last_corner(), Point::Int(params.end));
    assert!(
        centre.corner_count() > 2,
        "the block forces a detour: {:?}",
        centre.corners()
    );
    assert!(centre.is_multiple_of_45_degree());
    // w + g/2 = 0.2 + 0.1 mm.
    assert_eq!(um(board, f64::from(pair.compound_half_width(0))), 300.0);
    let hw = pair.compound_half_width(0);
    let cl = net_clearance_class(board, pair.p);
    let (slack, item) =
        min_foreign_slack(board, &centre, 0, hw, cl, &[pair.p, pair.n]).expect("foreign items");
    eprintln!(
        "x08-straight centreline: {} corners, min slack {:.2} units (item {item:?}), length {:.4} mm",
        centre.corner_count(),
        slack,
        um(board, centre.length_approx()) / 1000.0
    );
    assert!(slack >= 0.0, "slack {slack} at {item:?}");
    // Nothing on the board changed.
    assert_eq!(
        ses(board, STRAIGHT),
        ses(&prepare(STRAIGHT, &[]).board, STRAIGHT)
    );
}

// ---------------------------------------------------------------------------
// X08.4: legs and insertion
// ---------------------------------------------------------------------------

/// The edge gaps along the coupled P track to the coupled N track, 100
/// samples per segment.
fn coupled_gaps(board: &RoutingBoard, report: &DiffPairReport) -> Vec<f64> {
    let PairStatus::Routed {
        p_coupled,
        n_coupled,
        layer,
        ..
    } = &report.pairs[0].status
    else {
        panic!("{:?}", report.pairs[0].status)
    };
    let hw = f64::from(report.pairs[0].pair.half_width(*layer));
    let (hw_p, hw_n) = (hw, hw);
    let corners = |poly: &Polyline| -> Vec<FloatPoint> {
        poly.corners().iter().map(Point::to_float).collect()
    };
    let (p, n) = (corners(p_coupled), corners(n_coupled));
    // The coupled tracks are on the board: every sample of P's coupled
    // track lies on P's copper.
    let p_net = report.pairs[0].pair.p;
    let p_copper: Vec<Vec<FloatPoint>> = board
        .items()
        .filter(|i| i.contains_net(p_net))
        .filter_map(|i| i.as_trace())
        .map(|t| corners(&t.polyline))
        .collect();
    for q in &p {
        let on = p_copper
            .iter()
            .map(|c| polyline_distance(*q, c))
            .fold(f64::INFINITY, f64::min);
        assert!(on <= 1.0, "coupled corner {q:?} is {on} off P's copper");
    }
    let mut out = Vec::new();
    for s in p.windows(2) {
        for k in 0..100 {
            let t = (k as f64 + 0.5) / 100.0;
            let x = FloatPoint::new(
                s[0].x + (s[1].x - s[0].x) * t,
                s[0].y + (s[1].y - s[0].y) * t,
            );
            out.push(polyline_distance(x, &n) - hw_p - hw_n);
        }
    }
    out
}

#[test]
fn the_connector_pair_is_connected_with_its_gap_and_no_drc_item() {
    let mut prepared = prepare(CONNECTOR, &[]);
    let before = clearance_violations(&prepared.board);
    let report = run_stage(&mut prepared, &names_stage(0));
    let board = &prepared.board;
    let log = report.log(board);
    eprintln!("{}", log.join("\n"));
    assert!(
        matches!(report.pairs[0].status, PairStatus::Routed { setback_um, .. } if setback_um == 0.0),
        "{log:?}"
    );
    for name in ["USB_D+", "USB_D-"] {
        assert_eq!(incompletes(board, net(board, name)), 0, "{name}");
    }
    assert_eq!(before, 0);
    assert_eq!(clearance_violations(board), 0, "no DRC item");
    let gap = f64::from(report.pairs[0].pair.gap[0]);
    for (i, g) in coupled_gaps(board, &report).iter().enumerate() {
        assert!((g - gap).abs() <= 1.0, "sample {i}: gap {g} vs {gap}");
    }
    let line = log.last().unwrap();
    assert!(
        line.contains("routed (setback 0 mm); coupled length"),
        "{line}"
    );
    assert!(line.contains("within ±10 % of the gap 0.1500 mm"), "{line}");
}

#[test]
fn a_forced_leg_failure_tries_the_1_2_4_mm_setbacks() {
    let mut prepared = prepare(CONNECTOR, &[]);
    let report = run_stage(&mut prepared, &names_stage(3));
    let board = &prepared.board;
    let log = report.log(board);
    eprintln!("{}", log.join("\n"));
    let attempts = &report.pairs[0].attempts;
    assert_eq!(attempts.len(), 4, "{attempts:?}");
    assert!(
        attempts[0].starts_with("direct: leg failed (forced)"),
        "{attempts:?}"
    );
    assert!(
        attempts[1].starts_with("setback 1 mm: leg failed (forced)"),
        "{attempts:?}"
    );
    assert!(
        attempts[2].starts_with("setback 2 mm: leg failed (forced)"),
        "{attempts:?}"
    );
    assert!(
        attempts[3].starts_with("setback 4 mm: inserted"),
        "{attempts:?}"
    );
    for a in ["direct", "setback 1 mm", "setback 2 mm", "setback 4 mm"] {
        assert!(
            log.iter()
                .any(|l| l.contains(&format!("USB_D-, class names): {a}: "))),
            "{a}: {log:?}"
        );
    }
    for name in ["USB_D+", "USB_D-"] {
        assert_eq!(incompletes(board, net(board, name)), 0, "{name}");
    }
    assert_eq!(clearance_violations(board), 0);
    let gap = f64::from(report.pairs[0].pair.gap[0]);
    for (i, g) in coupled_gaps(board, &report).iter().enumerate() {
        assert!((g - gap).abs() <= 1.0, "sample {i}: gap {g} vs {gap}");
    }
    // Every attempt failing leaves the board as it was.
    let mut failing = prepare(CONNECTOR, &[]);
    let report = run_stage(&mut failing, &names_stage(4));
    assert!(
        matches!(report.pairs[0].status, PairStatus::Failed(_)),
        "{:?}",
        report.pairs[0].status
    );
    assert_eq!(
        ses(&failing.board, CONNECTOR),
        ses(&prepare(CONNECTOR, &[]).board, CONNECTOR)
    );
}

#[test]
fn the_stage_is_selected_by_its_key_and_off_by_default() {
    let (builtin, _) = route(prepare(CONNECTOR, &[]), false);
    let (off, _) = route(prepare(CONNECTOR, &[]), true);
    assert_eq!(
        ses(&off, CONNECTOR),
        ses(&builtin, CONNECTOR),
        "registered, unselected"
    );
    // Inference alone selects nothing.
    let (infer_only, _) = route(
        prepare(CONNECTOR, &[("diff_pairs.infer_from_names", "true")]),
        true,
    );
    assert_eq!(ses(&infer_only, CONNECTOR), ses(&off, CONNECTOR));
    // Selected, without inference: no pairs on a DSN.
    let (no_pairs, _) = route(
        prepare(CONNECTOR, &[("special.diff_pairs", DIFFPAIR_ID)]),
        true,
    );
    assert_eq!(ses(&no_pairs, CONNECTOR), ses(&off, CONNECTOR));
    let (on, log) = route(
        prepare(
            CONNECTOR,
            &[
                ("special.diff_pairs", DIFFPAIR_ID),
                ("diff_pairs.infer_from_names", "true"),
            ],
        ),
        true,
    );
    assert_ne!(ses(&on, CONNECTOR), ses(&off, CONNECTOR));
    assert!(
        log.iter()
            .any(|l| l.contains("router.special.diff_pairs=krt-diffpair")),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|l| l.starts_with("krt-diffpair: pair USB_D")
                && l.contains("routed (setback 0 mm)")),
        "{log:?}"
    );
    assert_eq!(DesignRulesChecker::new(&on).incomplete_count(), 0);
    assert_eq!(clearance_violations(&on), 0);
}

// ---------------------------------------------------------------------------
// X08.5: determinism and the report
// ---------------------------------------------------------------------------

/// Writes the SES and the stage log of both boards (stage only, and the
/// full pipeline with the stage selected) into `$X08_SES_OUT`, for
/// `tools/determinism-check.sh`.
#[test]
#[ignore = "writes files; run by tools/determinism-check.sh"]
fn write_diffpair_sessions() {
    let out = PathBuf::from(std::env::var("X08_SES_OUT").expect("X08_SES_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    for dsn in [STRAIGHT, CONNECTOR] {
        let name = Path::new(dsn)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        for forced in [0, 3] {
            let mut prepared = prepare(dsn, &[]);
            let report = run_stage(&mut prepared, &names_stage(forced));
            std::fs::write(
                out.join(format!("{name}-stage{forced}.ses")),
                ses(&prepared.board, dsn),
            )
            .unwrap();
            std::fs::write(
                out.join(format!("{name}-stage{forced}.log")),
                report.log(&prepared.board).join("\n"),
            )
            .unwrap();
        }
        let (board, log) = route(
            prepare(
                dsn,
                &[
                    ("special.diff_pairs", DIFFPAIR_ID),
                    ("diff_pairs.infer_from_names", "true"),
                ],
            ),
            true,
        );
        std::fs::write(out.join(format!("{name}-pipeline.ses")), ses(&board, dsn)).unwrap();
        std::fs::write(
            out.join(format!("{name}-pipeline.log")),
            log.iter()
                .filter(|l| l.starts_with("krt-diffpair"))
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
    }
}

#[test]
fn the_report_gives_coupled_length_and_gap_share_per_pair() {
    for dsn in [STRAIGHT, CONNECTOR] {
        let mut prepared = prepare(dsn, &[]);
        let report = run_stage(&mut prepared, &names_stage(0));
        let board = &prepared.board;
        let log = report.log(board);
        eprintln!("{}", log.join("\n"));
        assert_eq!(report.pairs.len(), 1);
        let PairStatus::Routed {
            layer,
            centreline_length,
            ..
        } = report.pairs[0].status
        else {
            panic!("{dsn}: {log:?}")
        };
        let c = coupling(board, &report.pairs[0].pair, layer);
        assert!(centreline_length > 0.0);
        assert!(c.share() > 0.5 && c.share() <= 1.0, "{dsn}: {c:?}");
        assert!(
            c.copper_length >= 2.0 * centreline_length * 0.9,
            "{dsn}: {c:?}"
        );
        let line = log.last().unwrap();
        assert!(line.contains("coupled length "), "{line}");
        assert!(line.contains(" % of the pair's "), "{line}");
        assert_eq!(clearance_violations(board), 0, "{dsn}");
    }
}

// ---------------------------------------------------------------------------
// The preregistered measurement (docs/roadmap/prereg/X08.json; X09 runs it)
// ---------------------------------------------------------------------------

/// The job's load sequence on `path` with the default settings and `extra`.
fn prepare_default(path: &Path, extra: &[(&str, &str)]) -> PreparedJob {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let name = path.to_string_lossy().to_string();
    let mut input = JobInput::new(bytes, JobInput::design_name_of(&name));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    if !extra.is_empty() {
        let mut profile = default_profile();
        for (key, value) in extra {
            profile.patch.set_path(key, value).unwrap();
        }
        input.extra_sources = vec![(Source::Testing, profile.patch)];
    }
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

/// One arm's metrics (the prereg's names): the pairs are the name-inferred
/// ones of the input, measured on their X08 layer.
fn arm_metrics(board: &BasicBoard) -> String {
    let mut drc = DesignRulesChecker::new(board);
    let model = build_pairs(board, &PairClasses::none(), true);
    let mut coupled = 0;
    let mut shares = Vec::new();
    for pair in &model.pairs {
        let Ok(ends) = pair_ends(board, pair) else {
            continue;
        };
        let share = coupling(board, pair, ends.layer).share();
        shares.push(share);
        coupled += usize::from(share >= 0.5);
    }
    let mean = if shares.is_empty() {
        0.0
    } else {
        shares.iter().sum::<f64>() / shares.len() as f64
    };
    format!(
        "{{\"via_in_pad\": {}, \"clearance_violations\": {}, \"unrouted\": {}, \
         \"pairs\": {}, \"pairs_coupled\": {coupled}, \"coupled_share_mean\": {mean:.4}}}",
        vias_in_pads(board),
        drc.all_clearance_violations().len(),
        drc.incomplete_count(),
        shares.len()
    )
}

/// For each row of `$X08_SET`: routes with the default settings and this
/// crate's registry, the stage unselected (arm `off`) and selected with
/// name inference (arm `on`), and writes a `freerouting-gate-records-1`
/// file to `$X08_RECORDS`.
#[test]
#[ignore = "the registered X08/X09 run; waits for P04 (docs/roadmap/prereg/X08.json)"]
fn measurement_records_on_a_set() {
    let set = std::env::var("X08_SET").expect("X08_SET");
    let out = std::env::var("X08_RECORDS").expect("X08_RECORDS");
    let text = std::fs::read_to_string(repo_root().join(&set)).unwrap();
    let mut rows = Vec::new();
    for line in text.lines() {
        let path = line.split_once('#').map_or(line, |(p, _)| p).trim();
        if path.is_empty() {
            continue;
        }
        let full = repo_root().join(path);
        if !full.exists() {
            eprintln!("skipped (missing): {path}");
            continue;
        }
        let (off_board, _) = route(prepare_default(&full, &[]), true);
        let off = arm_metrics(&off_board);
        let on_settings = [
            ("special.diff_pairs", DIFFPAIR_ID),
            ("diff_pairs.infer_from_names", "true"),
        ];
        let (on_board, log) = route(prepare_default(&full, &on_settings), true);
        for l in log.iter().filter(|l| l.starts_with(DIFFPAIR_ID)) {
            eprintln!("{path}: {l}");
        }
        let on = arm_metrics(&on_board);
        let stem = full.file_stem().unwrap().to_string_lossy().to_string();
        rows.push(format!(
            "\"{stem}\": {{\"board\": \"{path}\", \"arms\": {{\"off\": {off}, \"on\": {on}}}}}"
        ));
    }
    let json = format!(
        "{{\"format\": \"freerouting-gate-records-1\", \"arms\": {{\"off\": {{\"engine\": \
         \"freerouting-rs, krt-diffpair unselected\"}}, \"on\": {{\"engine\": \
         \"freerouting-rs, special.diff_pairs=krt-diffpair, diff_pairs.infer_from_names=true\"}}}}, \
         \"rows\": {{{}}}}}",
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

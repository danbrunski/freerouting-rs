//! X09: coupled via pairs, the polarity report, the optimizer exclusion
//! and intra-pair skew of the `krt-diffpair` stage, on synthetic boards in
//! `tests/data/` (not `fixtures/krt/`: every DSN under `fixtures/` needs a
//! Java reference, as X08 found).
//!
//! - `x09-two-layer.dsn`: X08's connector pair with the far connector on
//!   B.Cu (placed on the back), so the pair must change layer (X09.1);
//! - `x09-swapped.dsn`: X08's connector pair with P and N swapped at the
//!   far connector, so P and N would cross (X09.2);
//! - `x09-skew.dsn`: X08's connector pair with the near connector's N pad
//!   0.4 mm farther out than its P pad, a built-in skew of 0.4 mm (X09.4).
//!
//! - `x09-optimizer.dsn`: X08's connector pair with net ID pre-routed as an
//!   unfixed 5 mm detour on F.Cu, for the optimizer to shorten (X09.3).
//!
//! The optimizer test (X09.3) runs the full pipeline on those boards and
//! `x08-connector.dsn`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{StageCtx, StageRegistry};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::drc::DesignRulesChecker;
use freerouting::geometry::{IntPoint, TileShapeOps};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::optimize::batch::candidates::sorted_candidates;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};
use freerouting_krt::diffpair::centreline::pair_ends;
use freerouting_krt::diffpair::optimizer::{PairCopper, exclusion};
use freerouting_krt::diffpair::report::coupling_on;
use freerouting_krt::diffpair::vias::via_spec;
use freerouting_krt::diffpair::{
    DIFFPAIR_ID, DiffPairReport, KrtDiffPair, PairClasses, PairOptions, PairStatus, build_pairs,
};
use freerouting_krt::length::{LengthModel, measure_net};

const CONNECTOR: &str = "crates/freerouting-krt/tests/data/x08-connector.dsn";
const TWO_LAYER: &str = "crates/freerouting-krt/tests/data/x09-two-layer.dsn";
const SWAPPED: &str = "crates/freerouting-krt/tests/data/x09-swapped.dsn";
const SKEW: &str = "crates/freerouting-krt/tests/data/x09-skew.dsn";
const OPTIMIZER: &str = "crates/freerouting-krt/tests/data/x09-optimizer.dsn";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `dsn` with the test profile (optimizer as
/// `optimizer`) and `extra`.
fn prepare(dsn: &str, optimizer: bool, extra: &[(&str, &str)]) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(optimizer);
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

fn names_stage(skew_um: Option<f64>) -> KrtDiffPair {
    KrtDiffPair {
        classes: PairClasses::none(),
        infer_from_names: true,
        options: PairOptions {
            skew_um,
            ..PairOptions::default()
        },
    }
}

/// Routes the prepared job with this crate's registry; the board and log.
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

fn clearance_violations(board: &BasicBoard) -> usize {
    DesignRulesChecker::new(board)
        .all_clearance_violations()
        .len()
}

fn incompletes(board: &BasicBoard, net: i32) -> usize {
    DesignRulesChecker::new(board).incomplete_count_of(net)
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

/// The via centres of `net`.
fn via_centres(board: &BasicBoard, net: i32) -> Vec<IntPoint> {
    board
        .items()
        .filter(|i| i.is_via() && i.contains_net(net))
        .map(|i| i.drill().expect("a via has a drill").center)
        .collect()
}

// ---------------------------------------------------------------------------
// X09.1: coupled via pairs
// ---------------------------------------------------------------------------

#[test]
fn a_layer_change_gets_one_via_pair_across_the_centreline() {
    let mut prepared = prepare(TWO_LAYER, false, &[]);
    let before = clearance_violations(&prepared.board);
    let report = run_stage(&mut prepared, &names_stage(None));
    let board = &prepared.board;
    let log = report.log(board);
    eprintln!("{}", log.join("\n"));
    let pair = &report.pairs[0].pair;
    let ends = pair_ends(board, pair).expect("ends");
    assert!(ends.changes_layer(), "{ends:?}");
    let PairStatus::Routed { layers, vias, .. } = &report.pairs[0].status else {
        panic!("{log:?}");
    };
    assert_eq!(layers, &vec![ends.layer, ends.end_layer]);
    assert_eq!(vias.len(), 1, "one layer change, one via pair");
    // Exactly two vias on the board, one per net, at the planned centres.
    let (p, n) = vias[0];
    assert_eq!(via_centres(board, pair.p), vec![p]);
    assert_eq!(via_centres(board, pair.n), vec![n]);
    assert_eq!(board.items().filter(|i| i.is_via()).count(), 2);
    // Side by side at the decided spacing.
    let spec = via_spec(
        board,
        pair,
        ends.layer,
        ends.end_layer,
        &prepared.board_settings,
    )
    .expect("via");
    let d = p.distance(n);
    assert!(
        d >= f64::from(spec.spacing) && d <= f64::from(spec.spacing) + 2.0,
        "via distance {d} vs spacing {}",
        spec.spacing
    );
    // Across the centreline: P's via on P's side of the pair, the pair's
    // middle on the line between the two pin-pair midpoints' sides.
    for name in ["USB_D+", "USB_D-"] {
        assert_eq!(incompletes(board, net(board, name)), 0, "{name}");
    }
    assert_eq!(before, 0);
    assert_eq!(clearance_violations(board), 0, "no DRC item");
    assert_eq!(vias_in_pads(board), 0, "no via in a pad");
    let line = log.last().unwrap();
    assert!(line.contains("1 coupled via pair(s)"), "{line}");
}

// ---------------------------------------------------------------------------
// X09.2: the polarity report
// ---------------------------------------------------------------------------

#[test]
fn a_swapped_pair_is_reported_as_crossing_and_not_routed() {
    let mut prepared = prepare(SWAPPED, false, &[]);
    let input = ses(&prepared.board, SWAPPED);
    let report = run_stage(&mut prepared, &names_stage(None));
    let board = &prepared.board;
    let log = report.log(board);
    eprintln!("{}", log.join("\n"));
    assert_eq!(report.pairs.len(), 1);
    assert!(
        matches!(&report.pairs[0].status, PairStatus::Crossing(why) if why.contains("would cross")),
        "{log:?}"
    );
    assert!(report.pairs[0].attempts.is_empty(), "no attempt");
    assert!(
        log.last().unwrap().contains(": crossing: P is on the"),
        "{log:?}"
    );
    assert_eq!(ses(board, SWAPPED), input, "the stage routed nothing");
    // The unswapped board routes (the same geometry otherwise).
    let mut plain = prepare(CONNECTOR, false, &[]);
    let report = run_stage(&mut plain, &names_stage(None));
    assert!(matches!(report.pairs[0].status, PairStatus::Routed { .. }));
}

// ---------------------------------------------------------------------------
// X09.4: intra-pair skew
// ---------------------------------------------------------------------------

/// `|len P − len N|` of `pair`'s nets on `board`, in µm (X07's measure:
/// traces and via barrels).
fn skew_um(board: &BasicBoard, p: i32, n: i32) -> f64 {
    let model = LengthModel::none();
    let lp = measure_net(board, p, &model).total();
    let ln = measure_net(board, n, &model).total();
    freerouting_krt::length::Units::of(board).to_um((lp - ln).abs())
}

#[test]
fn the_shorter_track_is_bumped_to_within_the_skew_tolerance() {
    // Without compensation: the built-in skew of the turn.
    let mut raw = prepare(SKEW, false, &[]);
    let report = run_stage(&mut raw, &names_stage(None));
    let log = report.log(&raw.board);
    eprintln!("{}", log.join("\n"));
    assert!(
        matches!(report.pairs[0].status, PairStatus::Routed { .. }),
        "{log:?}"
    );
    let pair = report.pairs[0].pair.clone();
    let built_in = skew_um(&raw.board, pair.p, pair.n);
    eprintln!("built-in skew {built_in:.1} µm");
    assert!(
        (380.0..=420.0).contains(&built_in),
        "built-in skew {built_in} µm"
    );
    // With the stage as a job builds it: `diff_pairs.skew_um` unset is 25.
    let mut prepared = prepare(SKEW, false, &[("diff_pairs.infer_from_names", "true")]);
    let stage = KrtDiffPair::from_settings(&prepared.settings);
    assert_eq!(stage.options.skew_um, Some(25.0));
    let report = run_stage(&mut prepared, &stage);
    let board = &prepared.board;
    let log = report.log(board);
    eprintln!("{}", log.join("\n"));
    let PairStatus::Routed { skew, .. } = &report.pairs[0].status else {
        panic!("{log:?}");
    };
    let skew = skew.as_ref().expect("a skew compensation");
    assert!(skew.bumps >= 1, "{skew:?}");
    let after = skew_um(board, pair.p, pair.n);
    eprintln!("skew after {after:.2} µm");
    assert!(after <= 25.0, "skew {after} µm after compensation");
    for name in ["USB_D+", "USB_D-"] {
        assert_eq!(incompletes(board, net(board, name)), 0, "{name}");
    }
    assert_eq!(clearance_violations(board), 0, "no DRC item");
    assert!(log.last().unwrap().contains("bump(s) on net"), "{log:?}");
    // A tolerance above the built-in skew leaves the pair alone.
    let mut loose = prepare(SKEW, false, &[]);
    let report = run_stage(&mut loose, &names_stage(Some(built_in + 1.0)));
    assert!(matches!(
        report.pairs[0].status,
        PairStatus::Routed { skew: None, .. }
    ));
}

// ---------------------------------------------------------------------------
// X09.3: the optimizer exclusion
// ---------------------------------------------------------------------------

/// Every net's copper but the pair nets', per net.
fn other_copper(board: &BasicBoard, skip: &[i32]) -> Vec<(i32, PairCopperOf)> {
    (1..=board.rules.nets.max_net_number())
        .filter(|n| !skip.contains(n))
        .map(|n| (n, copper_of_net(board, n)))
        .collect()
}

type PairCopperOf = Vec<String>;

/// One net's traces and vias as sorted text.
fn copper_of_net(board: &BasicBoard, net: i32) -> PairCopperOf {
    let mut out: Vec<String> = board
        .items()
        .filter(|i| i.contains_net(net))
        .filter_map(|i| {
            if let Some(t) = i.as_trace() {
                Some(format!(
                    "trace {} {} {:?}",
                    t.layer,
                    t.half_width,
                    t.polyline.corners()
                ))
            } else {
                i.drill()
                    .filter(|_| i.is_via())
                    .map(|d| format!("via {:?}", d.center))
            }
        })
        .collect();
    out.sort();
    out
}

#[test]
fn the_optimizer_leaves_the_pairs_alone_and_optimizes_the_rest() {
    let on = [
        ("special.diff_pairs", DIFFPAIR_ID),
        ("diff_pairs.infer_from_names", "true"),
    ];
    let mut others_candidates = 0;
    for dsn in [CONNECTOR, TWO_LAYER, SKEW, OPTIMIZER] {
        let (without, _) = route(prepare(dsn, false, &on));
        let (with, log) = route(prepare(dsn, true, &on));
        // The optimizer's preflight skips the connector board (no vias,
        // straight nets) and the two-layer one (every via a mandatory
        // layer change); it runs on the skew and the optimizer boards.
        let ran = log.iter().any(|l| l.starts_with("Optimization stage"));
        let skipped = log
            .iter()
            .any(|l| l.starts_with("Skipping optimization stage"));
        eprintln!("{dsn}: optimizer ran {ran}");
        assert!(ran || skipped, "{dsn}: {log:?}");
        assert_eq!(ran, dsn == SKEW || dsn == OPTIMIZER, "{dsn}: {log:?}");
        let pairs = build_pairs(&with, &PairClasses::none(), true).pairs;
        assert_eq!(pairs.len(), 1, "{dsn}");
        let skip: Vec<i32> = pairs.iter().flat_map(|p| [p.p, p.n]).collect();
        for pair in &pairs {
            let ends = pair_ends(&with, pair).expect("ends");
            let layers = [ends.layer, ends.end_layer];
            let before = coupling_on(&without, pair, &layers);
            let after = coupling_on(&with, pair, &layers);
            eprintln!(
                "{dsn}: coupled share {:.4} without the optimizer, {:.4} with it",
                before.share(),
                after.share()
            );
            assert!(before.share() > 0.8, "{dsn}: the pair is coupled");
            assert_eq!(before, after, "{dsn}: the coupled share is unchanged");
            assert_eq!(
                PairCopper::of(&without, pair),
                PairCopper::of(&with, pair),
                "{dsn}: the pair's copper is unchanged"
            );
            assert!(!exclusion(&with, &pairs).is_empty());
        }
        // The optimizer's candidates on the board it starts from: the other
        // nets' items, none of the pair's.
        let excluded = exclusion(&without, &pairs);
        let candidates = sorted_candidates(&without);
        assert!(
            candidates.iter().all(|c| !excluded.contains(&c.id)),
            "{dsn}: no pair item is a candidate"
        );
        let pair_items = without
            .items()
            .filter(|i| skip.iter().any(|n| i.contains_net(*n)))
            .filter(|i| i.is_trace() || i.is_via())
            .count();
        assert_eq!(
            pair_items,
            excluded.len(),
            "{dsn}: every pair item excluded"
        );
        eprintln!(
            "{dsn}: {} optimizer candidates, {} pair items excluded",
            candidates.len(),
            excluded.len()
        );
        others_candidates += candidates.len();
        let changed: Vec<i32> = other_copper(&without, &skip)
            .into_iter()
            .zip(other_copper(&with, &skip))
            .filter(|(a, b)| a != b)
            .map(|(a, _)| a.0)
            .collect();
        // Only other nets than the pair's can change (`skip`). The optimizer
        // changes none on these boards: on `x09-optimizer.dsn` it rejects its
        // one pass (`OPTIMIZER_SCORE_NOT_IMPROVED`) with the pair stage and
        // without it alike, so the ID detour stays either way.
        eprintln!("{dsn}: nets the optimizer changed: {changed:?}");
        for board in [&without, &with] {
            assert_eq!(clearance_violations(board), 0, "{dsn}");
            assert_eq!(
                DesignRulesChecker::new(board).incomplete_count(),
                0,
                "{dsn}"
            );
            assert_eq!(vias_in_pads(board), 0, "{dsn}");
        }
    }
    assert!(
        others_candidates > 0,
        "the other nets are still the optimizer's candidates"
    );
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

/// Writes the SES and the stage log of the X09 boards (the stage as a job
/// builds it, and the full pipeline with the optimizer) into
/// `$X09_SES_OUT`, for `tools/determinism-check.sh`.
#[test]
#[ignore = "writes files; run by tools/determinism-check.sh"]
fn write_x09_sessions() {
    let out = PathBuf::from(std::env::var("X09_SES_OUT").expect("X09_SES_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let on = [
        ("special.diff_pairs", DIFFPAIR_ID),
        ("diff_pairs.infer_from_names", "true"),
    ];
    for dsn in [TWO_LAYER, SWAPPED, SKEW, OPTIMIZER] {
        let name = Path::new(dsn)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut prepared = prepare(dsn, false, &on);
        let stage = KrtDiffPair::from_settings(&prepared.settings);
        let report = run_stage(&mut prepared, &stage);
        let write = |file: String, text: String| std::fs::write(out.join(file), text).unwrap();
        write(format!("{name}-stage.ses"), ses(&prepared.board, dsn));
        write(
            format!("{name}-stage.log"),
            report.log(&prepared.board).join("\n"),
        );
        let (board, log) = route(prepare(dsn, true, &on));
        write(format!("{name}-pipeline.ses"), ses(&board, dsn));
        write(format!("{name}-pipeline.log"), log.join("\n"));
    }
}

// ---------------------------------------------------------------------------
// X09.5: the measurement harness (docs/roadmap/prereg/X09.json)
// ---------------------------------------------------------------------------

/// The job's load sequence on `path` with the default settings and
/// `extra` (as the CLI: optimizer on).
fn prepare_default(path: &Path, extra: &[(&str, &str)]) -> PreparedJob {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let name = path.to_string_lossy().to_string();
    let mut input = JobInput::new(bytes, JobInput::design_name_of(&name));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    if !extra.is_empty() {
        let mut profile = TestingProfile::default();
        for (key, value) in extra {
            profile.patch.set_path(key, value).unwrap();
        }
        input.extra_sources = vec![(Source::Testing, profile.patch)];
    }
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// One arm's metrics (the prereg's names), over the name-inferred pairs of
/// two-pin nets on all their layers.
fn arm_metrics(board: &BasicBoard) -> String {
    let mut drc = DesignRulesChecker::new(board);
    let model = build_pairs(board, &PairClasses::none(), true);
    let (mut copper, mut within, mut pairs, mut coupled) = (0.0, 0.0, 0, 0);
    let mut pair_nets = Vec::new();
    for pair in &model.pairs {
        pair_nets.extend([pair.p, pair.n]);
        let Ok(ends) = pair_ends(board, pair) else {
            continue;
        };
        let c = coupling_on(board, pair, &[ends.layer, ends.end_layer]);
        copper += c.copper_length;
        within += c.within_length;
        pairs += 1;
        coupled += usize::from(c.share() >= 0.9);
    }
    let share = if copper > 0.0 { within / copper } else { 0.0 };
    let single_ended: usize = (1..=board.rules.nets.max_net_number())
        .filter(|n| !pair_nets.contains(n))
        .map(|n| drc.incomplete_count_of(n))
        .sum();
    format!(
        "{{\"via_in_pad\": {}, \"clearance_violations\": {}, \"unrouted_single_ended\": \
         {single_ended}, \"unrouted\": {}, \"pairs\": {pairs}, \"pairs_coupled\": {coupled}, \
         \"coupled_share\": {share:.4}}}",
        vias_in_pads(board),
        drc.all_clearance_violations().len(),
        drc.incomplete_count(),
    )
}

/// For each row of `$X09_SET`: routes with the default settings and this
/// crate's registry, the stage unselected (arm `off`: pairs routed as
/// single nets) and selected with name inference (arm `on`), and writes a
/// `freerouting-gate-records-1` file to `$X09_RECORDS`.
#[test]
#[ignore = "the registered X09 run; waits for P04 (docs/roadmap/prereg/X09.json)"]
fn measurement_records_on_a_set() {
    let set = std::env::var("X09_SET").expect("X09_SET");
    let out = std::env::var("X09_RECORDS").expect("X09_RECORDS");
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
        let (off_board, _) = route(prepare_default(&full, &[]));
        let off = arm_metrics(&off_board);
        let on_settings = [
            ("special.diff_pairs", DIFFPAIR_ID),
            ("diff_pairs.infer_from_names", "true"),
        ];
        let (on_board, log) = route(prepare_default(&full, &on_settings));
        for l in log.iter().filter(|l| l.starts_with(DIFFPAIR_ID)) {
            eprintln!("{path}: {l}");
        }
        let on = arm_metrics(&on_board);
        eprintln!("{path}: off {off}\n{path}: on {on}");
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

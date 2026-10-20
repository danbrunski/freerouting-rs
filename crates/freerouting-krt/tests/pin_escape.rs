//! Q21: straight pin escapes at the board minimum, and the
//! `router.fanout.pin_escape` switch that runs them as the `krt-pin-escape`
//! board stage.
//!
//! Board (synthetic, written for Q21 by `tests/data/q21-pin-escape.dsn`'s
//! generator; not under `fixtures/`, where every DSN needs a Java reference):
//! `q21-pin-escape.dsn`, two signal layers, net class width 0.15 mm,
//! clearance 0.2 mm (pad to pad 0.05 mm, `smd_smd`), and two 16-pin QFN
//! images with 0.8 × 0.2 mm pads, four per side, pad centres 1.45 mm from the
//! part centre, outline 3 × 3 mm:
//! - `QFN16_P0P4`, pitch 0.4 mm (pad gap 0.2 mm, the clearance), at 0° (U1),
//!   90° (U3), 180° (U4) and on the back (U5); net `S<k>` joins pin k of the
//!   four copies;
//! - `QFN16_P0P35`, pitch 0.35 mm (pad gap 0.15 mm), at U2 and U6; net `T<k>`
//!   joins their pin k.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use freerouting::autoroute::control::min_trace_half_width;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, PipelineOutcome, run_pipeline};
use freerouting::autoroute::stages::pin_escape::{self, PinEscape};
use freerouting::autoroute::stages::{StageRegistry, StageSelection};
use freerouting::board::BasicBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::ComponentId;
use freerouting::geometry::IntPoint;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};
use freerouting_krt::fanout::frame::{PartLayout, Side, analyze};
use freerouting_krt::fanout::qfn::{EscapeReport, PinOutcome, PinResult, Skip, StubKind, escape};
use freerouting_krt::fanout::{PIN_ESCAPE_STAGE, QFN_ESCAPE_ID};

const BOARD: &str = "crates/freerouting-krt/tests/data/q21-pin-escape.dsn";
const POWER: &str = "crates/freerouting-krt/tests/data/x10-qfn-power.dsn";
const ROTATED: &str = "crates/freerouting-krt/tests/data/x10-qfn-rotated.dsn";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `dsn` from the default settings, with `extra`
/// (`path=value`) on the testing source.
fn prepare(dsn: &str, extra: &[(&str, &str)]) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    let mut profile = TestingProfile {
        patch: SettingsPatch::default(),
    };
    for (key, value) in extra {
        profile
            .patch
            .set_path(key, value)
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn component(board: &BasicBoard, name: &str) -> ComponentId {
    board
        .components
        .get_by_name(name)
        .unwrap_or_else(|| panic!("no component {name}"))
        .id
}

fn um(board: &BasicBoard, v: f64) -> i64 {
    i64::from(freerouting::job::overrides::um_to_board_units(board, v))
}

/// The stubs of `board` at a board minimum of `minimum_um` (none: unset).
fn escaped(dsn: &str, minimum_um: Option<&str>) -> (PreparedJob, EscapeReport) {
    let extra: Vec<(&str, &str)> = minimum_um
        .map(|m| ("board_minimums.track_width_um", m))
        .into_iter()
        .collect();
    let mut prepared = prepare(dsn, &extra);
    assert!(
        prepared.board.all_clearance_violations().is_empty(),
        "{dsn}: the input is clean"
    );
    let report = escape(
        &mut prepared.board,
        &prepared.board_settings,
        &CancelToken::new(),
    );
    (prepared, report)
}

fn outcomes_of(report: &EscapeReport, part: ComponentId) -> Vec<&PinOutcome> {
    report
        .outcomes
        .iter()
        .filter(|o| o.component == part)
        .collect()
}

/// A stub trace in the part frame: its two ends and half-width.
fn trace_in_part(board: &BasicBoard, layout: &PartLayout, id: ItemId) -> (IntPoint, IntPoint, i32) {
    let t = board
        .item(id)
        .and_then(|i| i.as_trace())
        .expect("a stub trace");
    let a = layout.frame.to_part(t.first_corner().expect_int());
    let b = layout.frame.to_part(t.last_corner().expect_int());
    (a, b, t.half_width)
}

/// The along-`u` and lateral coordinates of `p` on `side`.
fn along_lateral(side: Side, p: IntPoint) -> (i64, i64) {
    let (ux, uy) = side.outward();
    let along = i64::from(p.x) * ux + i64::from(p.y) * uy;
    let lateral = match side {
        Side::W | Side::E => i64::from(p.y),
        Side::S | Side::N => i64::from(p.x),
    };
    (along, lateral)
}

// ---------------------------------------------------------------------
// Q21.2: stubs at the board minimum
// ---------------------------------------------------------------------

#[test]
fn stubs_at_a_0p2_minimum_on_a_0p4_qfn_run_straight_out_and_keep_their_clearance() {
    let (prepared, report) = escaped(BOARD, Some("200"));
    let board: &BasicBoard = &prepared.board;
    let u1 = component(board, "U1");
    let layout = analyze(board, u1).expect("U1 qualifies");
    assert_eq!(layout.pitch, um(board, 400.0));
    let mine = outcomes_of(&report, u1);
    assert_eq!(16, mine.len(), "every side pin");
    let half = um(board, 100.0) as i32;
    let outline = layout.outline.expect("the outline");
    let clearance = um(board, 200.0);
    for o in &mine {
        let pin = layout.pin(o.pin).expect("own pin");
        assert_eq!(
            PinResult::Stub {
                kind: StubKind::Narrow,
                attempt: 0
            },
            o.result,
            "U1-{}",
            pin.name
        );
        assert_eq!(1, o.items.len(), "U1-{}: one straight trace", pin.name);
        let (a, b, hw) = trace_in_part(board, &layout, o.items[0]);
        assert_eq!(half, hw, "U1-{}: 0.2 mm wide", pin.name);
        let side = o.side;
        let (a_along, a_lat) = along_lateral(side, a);
        let (b_along, b_lat) = along_lateral(side, b);
        let (_, pin_lat) = along_lateral(side, pin.centre);
        assert!(
            (a_lat - pin_lat).abs() <= 1 && (b_lat - pin_lat).abs() <= 1,
            "U1-{}: straight along the pad's axis ({a:?} -> {b:?})",
            pin.name
        );
        let (pin_along, _) = along_lateral(side, pin.centre);
        assert!((a_along - pin_along).abs() <= 1, "starts at the pin centre");
        let outline_edge = match side {
            Side::W => -i64::from(outline.ll.x),
            Side::S => -i64::from(outline.ll.y),
            Side::E => i64::from(outline.ur.x),
            Side::N => i64::from(outline.ur.y),
        };
        let field_edge = match side {
            Side::W => -i64::from(layout.field.ll.x),
            Side::S => -i64::from(layout.field.ll.y),
            Side::E => i64::from(layout.field.ur.x),
            Side::N => i64::from(layout.field.ur.y),
        };
        assert!(b_along > outline_edge, "U1-{}: past the outline", pin.name);
        // The X10 end: past the escape box (pin field ∪ outline) by at
        // least the clearance and the half-width.
        assert!(
            b_along >= field_edge.max(outline_edge) + clearance + i64::from(half),
            "U1-{}: ends past edge + c + w ({b_along})",
            pin.name
        );
    }
    // The neighbours keep their clearance: no violation on the board, and
    // the stub of each pin is `pitch - pad/2 - stub/2` = 0.2 mm from its
    // neighbour's pad and its neighbour's stub.
    assert!(
        board.all_clearance_violations().is_empty(),
        "{:?}",
        board.all_clearance_violations()
    );
    assert_eq!(
        layout.pitch - i64::from(half) - um(board, 100.0),
        clearance,
        "the gap to the neighbour pad is exactly the clearance"
    );
}

#[test]
fn a_0p35_pitch_where_no_0p2_stub_fits_gets_no_stubs() {
    let (prepared, report) = escaped(BOARD, Some("200"));
    let board: &BasicBoard = &prepared.board;
    for name in ["U2", "U6"] {
        let part = component(board, name);
        let layout = analyze(board, part).expect("qualifies at 0.35 mm");
        assert_eq!(layout.pitch, um(board, 350.0));
        let mine = outcomes_of(&report, part);
        assert_eq!(16, mine.len());
        for o in mine {
            assert_eq!(
                PinResult::Skipped(Skip::Rejected),
                o.result,
                "{name}-{}",
                layout.pin(o.pin).unwrap().name
            );
            assert!(o.items.is_empty());
        }
    }
    // The 0.4 mm copies still get theirs; nothing on the board breaks a rule.
    assert_eq!(64, report.stubs());
    assert!(board.all_clearance_violations().is_empty());
}

#[test]
fn a_0p25_minimum_fits_nowhere_on_the_0p4_qfn() {
    // 0.4 - 0.1 - 0.125 = 0.175 mm to the neighbour pad, under the 0.2 mm
    // clearance: no stub, rather than one under the minimum.
    let (prepared, report) = escaped(BOARD, Some("250"));
    assert_eq!(0, report.stubs(), "{:?}", report.log(&prepared.board));
    assert!(
        report
            .outcomes
            .iter()
            .all(|o| o.result == PinResult::Skipped(Skip::Rejected))
    );
}

#[test]
fn without_a_board_minimum_the_stubs_follow_the_narrowest_class() {
    // Unset, `enhanced`'s `board_minimums.from_classes` floors at the 0.15 mm
    // class width: the stubs are 0.15 mm, so the 0.2 mm above is the floor.
    let (prepared, report) = escaped(BOARD, None);
    let board: &BasicBoard = &prepared.board;
    let u1 = component(board, "U1");
    let layout = analyze(board, u1).unwrap();
    for o in outcomes_of(&report, u1) {
        let (_, _, hw) = trace_in_part(board, &layout, o.items[0]);
        assert_eq!(um(board, 75.0) as i32, hw);
    }
}

/// The stubs of one copy in its part frame, by pin name: the result and the
/// traces' ends and half-widths.
type Copy = Vec<(String, PinResult, Vec<(IntPoint, IntPoint, i32)>)>;

fn copy_stubs(board: &BasicBoard, report: &EscapeReport, name: &str) -> Copy {
    let part = component(board, name);
    let layout = analyze(board, part).unwrap();
    let mut out: Copy = outcomes_of(report, part)
        .into_iter()
        .map(|o| {
            let traces = o
                .items
                .iter()
                .map(|id| trace_in_part(board, &layout, *id))
                .collect();
            (layout.pin(o.pin).unwrap().name.clone(), o.result, traces)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn rotated_and_mirrored_copies_get_the_same_stubs_at_the_minimum() {
    let (prepared, report) = escaped(BOARD, Some("200"));
    let board: &BasicBoard = &prepared.board;
    let reference = copy_stubs(board, &report, "U1");
    assert_eq!(16, reference.len());
    for name in ["U3", "U4", "U5"] {
        let copy = copy_stubs(board, &report, name);
        assert_eq!(reference.len(), copy.len(), "{name}");
        for ((pin, r, traces), (pin2, r2, traces2)) in reference.iter().zip(&copy) {
            assert_eq!(pin, pin2);
            assert_eq!(r, r2, "{name}-{pin}");
            assert_eq!(traces.len(), traces2.len(), "{name}-{pin}");
            for ((a, b, w), (a2, b2, w2)) in traces.iter().zip(traces2) {
                assert_eq!(w, w2, "{name}-{pin} width");
                for (p, q) in [(a, a2), (b, b2)] {
                    assert!(
                        (p.x - q.x).abs() <= 1 && (p.y - q.y).abs() <= 1,
                        "{name}-{pin}: {p:?} vs {q:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn no_stub_is_ever_below_the_board_minimum() {
    for dsn in [BOARD, POWER, ROTATED] {
        for minimum in ["150", "200", "250", "300"] {
            let (prepared, report) = escaped(dsn, Some(minimum));
            let board: &BasicBoard = &prepared.board;
            let floor = min_trace_half_width(board, &prepared.board_settings);
            assert_eq!(
                floor,
                freerouting::job::overrides::um_to_board_units(board, minimum.parse().unwrap()) / 2,
                "{dsn} {minimum}: the floor is the board minimum"
            );
            for o in &report.outcomes {
                for id in &o.items {
                    if let Some(t) = board.item(*id).and_then(|i| i.as_trace()) {
                        assert!(
                            t.half_width >= floor,
                            "{dsn} at {minimum} µm: a stub {} < {floor}",
                            t.half_width
                        );
                    }
                }
            }
            assert!(
                board.all_clearance_violations().is_empty(),
                "{dsn} at {minimum} µm"
            );
        }
    }
}

// ---------------------------------------------------------------------
// Q21.3: the option
// ---------------------------------------------------------------------

fn krt_registry() -> StageRegistry {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    registry
}

fn run(prepared: PreparedJob, registry: &StageRegistry) -> Result<PipelineOutcome, String> {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: Some(registry),
    };
    run_pipeline(prepared.board, &job).map_err(|f| f.error.to_string())
}

/// A short routing job on the Q21 board: one pass, no optimizer, the class
/// width as the floor, plus `extra`.
fn short_job(extra: &[(&str, &str)]) -> PreparedJob {
    let mut all = vec![
        ("autorouter.max_passes", "1"),
        ("optimizer.enabled", "false"),
    ];
    all.extend_from_slice(extra);
    prepare(BOARD, &all)
}

#[test]
fn straight_selects_the_pin_escape_board_stage() {
    let registry = krt_registry();
    let on = short_job(&[("fanout.pin_escape", "straight")]);
    assert_eq!(PinEscape::of(&on.settings), PinEscape::Straight);
    let selection = StageSelection::resolve(&on.settings, &registry);
    let ids: Vec<(&str, &str)> = selection
        .stages
        .iter()
        .map(|c| (c.key, c.id.as_str()))
        .collect();
    assert_eq!(ids, vec![(pin_escape::SETTINGS_KEY, PIN_ESCAPE_STAGE)]);
    assert_eq!(
        selection.fanout.id, "freerouting-fanout",
        "the fanout slot is unchanged"
    );
    let pipeline = registry.build(&selection, &on.settings).expect("built");
    assert_eq!(1, pipeline.board_stages.len());
    assert_eq!(PIN_ESCAPE_STAGE, pipeline.board_stages[0].id());
    // Unset and `off` select nothing.
    for extra in [vec![], vec![("fanout.pin_escape", "off")]] {
        let job = short_job(&extra);
        let selection = StageSelection::resolve(&job.settings, &registry);
        assert!(selection.stages.is_empty(), "{extra:?}");
    }
    // The built-in registry does not have the stage: an unknown stage error.
    let Err(error) = StageRegistry::builtin().build(
        &StageSelection::resolve(&on.settings, &StageRegistry::builtin()),
        &on.settings,
    ) else {
        panic!("krt-pin-escape is not a built-in stage")
    };
    assert_eq!(error.key, pin_escape::SETTINGS_KEY);
    assert_eq!(error.id, PIN_ESCAPE_STAGE);
    assert_ne!(PIN_ESCAPE_STAGE, QFN_ESCAPE_ID);
}

#[test]
fn an_unknown_pin_escape_value_is_refused_before_routing() {
    let Err(error) = run(short_job(&[("fanout.pin_escape", "bent")]), &krt_registry()) else {
        panic!("an unknown value routes")
    };
    assert!(
        error.contains("router.fanout.pin_escape") && error.contains("'bent'"),
        "{error}"
    );
}

fn routed_ses(extra: &[(&str, &str)]) -> (String, Vec<String>) {
    let mut outcome = run(short_job(extra), &krt_registry()).expect("the job runs");
    let board = outcome.board.take().expect("the routed board");
    (
        write_ses(&board, "q21-pin-escape", SesStyle::Java),
        outcome.log.iter().map(|l| l.text.clone()).collect(),
    )
}

#[test]
fn off_is_byte_identical_to_unset_and_straight_logs_its_stubs() {
    let (unset, unset_log) = routed_ses(&[]);
    let (off, _) = routed_ses(&[("fanout.pin_escape", "off")]);
    assert_eq!(unset, off, "off is the unchanged engine");
    assert!(!unset_log.iter().any(|l| l.starts_with("krt-qfn-escape")));
    let (on, on_log) = routed_ses(&[("fanout.pin_escape", "straight")]);
    assert!(
        on_log
            .iter()
            .any(|l| l == "krt-qfn-escape: 6 parts, 64 stubs"),
        "{on_log:?}"
    );
    assert!(
        on_log.iter().any(|l| l
            == "krt-qfn-escape U1: 16 side pins, 16 narrow, 0 widened, 0 via stubs, 0 rejected"),
        "{on_log:?}"
    );
    assert_ne!(unset, on, "the stubs change the session");
}

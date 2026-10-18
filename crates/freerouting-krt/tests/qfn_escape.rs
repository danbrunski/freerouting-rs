//! X10: the part frame, the sides of edge-row parts and the
//! `krt-qfn-escape` stubs.
//!
//! Boards (synthetic, in `tests/data/`, written by X10; not under
//! `fixtures/`, where every DSN needs a Java reference):
//! - `x10-frame.dsn`: a 24-pin 0.5 mm QFN with an exposed pad placed at 0°
//!   (U1), 90° (U2), 37° (U3), on the back (U4) and on the back at 127°
//!   (U5); a 5 × 5 0.5 mm BGA (B1), a 1.27 mm SOIC-8 (S1) and a 0.65 mm
//!   TSSOP-8 (T1).
//! - `x10-qfn-power.dsn`: the QFN (U1) on a 4-layer board with a GND plane
//!   on In1.Cu: pin 3 on the 1.5 mm `Power_High` net PWR (to J1), pin 10 on
//!   GND, pin 20 alone on its net, the other side pins to a header H1.
//! - `x10-qfn-rotated.dsn`: the same part at 0° (U1), 90° (U2), 180° (U3)
//!   and on the back (U4); each pin's net joins that pin of every copy.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{StageCtx, StageRegistry, StageSelection};
use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::ComponentId;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::IntPoint;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::ResolveContext;
use freerouting::settings::Source;
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::profiles::TestingProfile;
use freerouting_krt::fanout::frame::{PartFrame, PartLayout, analyze, sides};
use freerouting_krt::fanout::qfn::{PinOutcome, PinResult, Skip, StubKind, escape};
use freerouting_krt::fanout::rules::via_in_pad;
use freerouting_krt::fanout::{KrtQfnEscape, QFN_ESCAPE_ID};

const FRAME: &str = "crates/freerouting-krt/tests/data/x10-frame.dsn";
const POWER: &str = "crates/freerouting-krt/tests/data/x10-qfn-power.dsn";
const ROTATED: &str = "crates/freerouting-krt/tests/data/x10-qfn-rotated.dsn";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `dsn`, with `extra` (`path=value`) on the
/// testing source.
fn prepare(dsn: &str, extra: &[(&str, &str)]) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    if !extra.is_empty() {
        let mut profile = default_profile();
        for (key, value) in extra {
            profile
                .patch
                .set_path(key, value)
                .unwrap_or_else(|e| panic!("{key}: {e:?}"));
        }
        input.extra_sources = vec![(Source::Testing, profile.patch)];
    }
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn component(board: &BasicBoard, name: &str) -> ComponentId {
    board
        .components
        .get_by_name(name)
        .unwrap_or_else(|| panic!("no component {name}"))
        .id
}

fn um(board: &BasicBoard, v: f64) -> i32 {
    freerouting::job::overrides::um_to_board_units(board, v)
}

/// The pin names per side of `part`.
fn side_names(layout: &PartLayout) -> Vec<Vec<String>> {
    layout
        .sides
        .iter()
        .map(|ids| {
            ids.iter()
                .map(|id| layout.pin(*id).expect("own pin").name.clone())
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------
// X10.1: the part frame and the sides
// ---------------------------------------------------------------------

#[test]
fn the_frame_round_trips_at_0_90_37_degrees_and_mirrored() {
    let points = [
        IntPoint::new(0, 0),
        IntPoint::new(20000, -12500),
        IntPoint::new(-24000, 4000),
        IntPoint::new(123_457, -98_765),
    ];
    for rotate_first in [false, true] {
        for (rotation, mirrored) in [
            (0.0, false),
            (90.0, false),
            (37.0, false),
            (0.0, true),
            (127.0, true),
        ] {
            let frame = PartFrame::new(
                freerouting::geometry::FloatPoint::new(250_000.0, 400_000.0),
                rotation,
                mirrored,
                rotate_first,
            );
            for p in points {
                let back = frame.to_part(frame.to_board(p));
                let exact = rotation % 90.0 == 0.0;
                let tol = if exact { 0 } else { 1 };
                assert!(
                    (back.x - p.x).abs() <= tol && (back.y - p.y).abs() <= tol,
                    "{rotation}° mirrored {mirrored} rotate_first {rotate_first}: {p:?} -> {back:?}"
                );
            }
        }
    }
}

#[test]
fn a_qfn_at_0_90_37_degrees_and_mirrored_has_the_same_sides() {
    let prepared = prepare(FRAME, &[]);
    let board: &BasicBoard = &prepared.board;
    let reference = analyze(board, component(board, "U1")).expect("U1 qualifies");
    let expected = side_names(&reference);
    let names = |range: std::ops::RangeInclusive<u32>| -> Vec<String> {
        range.map(|n| n.to_string()).collect()
    };
    assert_eq!(
        expected,
        vec![names(1..=6), names(7..=12), names(13..=18), names(19..=24)],
        "U1's sides in W, S, E, N order, by pin number"
    );
    assert!(
        reference
            .pin(reference.pins.iter().find(|p| p.name == "25").unwrap().id)
            .unwrap()
            .side
            .is_none()
    );
    assert_eq!(i64::from(um(board, 500.0)), reference.pitch);
    for name in ["U2", "U3", "U4", "U5"] {
        let id = component(board, name);
        let layout = analyze(board, id).unwrap_or_else(|| panic!("{name} qualifies"));
        assert_eq!(expected, side_names(&layout), "{name}");
        // Rounded centres at 37° and 127° move the pitch by a unit.
        assert!(
            (reference.pitch - layout.pitch).abs() <= 1,
            "{name}: {}",
            layout.pitch
        );
        // The mapped centres are the package's relative locations.
        let package = board
            .library
            .packages
            .get(board.components.get(id).unwrap().package())
            .unwrap();
        for pin in &layout.pins {
            let item = board.item(pin.id).and_then(Item::as_pin).unwrap();
            let rel = package
                .get_pin(item.pin_index)
                .unwrap()
                .relative_location
                .to_float();
            assert!(
                (f64::from(pin.centre.x) - rel.x).abs() <= 1.0
                    && (f64::from(pin.centre.y) - rel.y).abs() <= 1.0,
                "{name}-{}: {:?} vs {rel:?}",
                pin.name,
                pin.centre
            );
        }
        assert_eq!(
            sides(board, id).map(|s| s.map(|v| v.len())),
            Some([6, 6, 6, 6]),
            "{name}"
        );
    }
}

#[test]
fn a_bga_and_a_coarse_soic_return_none_and_a_tssop_has_two_sides() {
    let prepared = prepare(FRAME, &[]);
    let board: &BasicBoard = &prepared.board;
    assert_eq!(sides(board, component(board, "B1")), None, "BGA");
    assert_eq!(sides(board, component(board, "S1")), None, "1.27 mm SOIC");
    let tssop = analyze(board, component(board, "T1")).expect("0.65 mm TSSOP qualifies");
    assert_eq!(
        side_names(&tssop),
        vec![
            vec!["1", "2", "3", "4"],
            vec![],
            vec!["5", "6", "7", "8"],
            vec![]
        ]
    );
    assert_eq!(i64::from(um(board, 650.0)), tssop.pitch);
}

// ---------------------------------------------------------------------
// X10.2: the stubs
// ---------------------------------------------------------------------

fn name_of(board: &BasicBoard, layout: &PartLayout, o: &PinOutcome) -> String {
    let _ = board;
    layout.pin(o.pin).expect("own pin").name.clone()
}

/// A trace's corners and half-width, or a via's centre (half-width 0).
fn geometry(board: &BasicBoard, id: ItemId) -> (Vec<IntPoint>, i32) {
    let item = board.item(id).expect("inserted item");
    if let Some(t) = item.as_trace() {
        return (
            vec![t.first_corner().expect_int(), t.last_corner().expect_int()],
            t.half_width,
        );
    }
    (vec![item.drill().expect("a via").center], 0)
}

#[test]
fn every_connected_pin_of_the_power_qfn_gets_a_stub() {
    let mut prepared = prepare(POWER, &[]);
    let before = prepared.board.all_clearance_violations().len();
    assert_eq!(0, before, "the input is clean");
    let report = escape(
        &mut prepared.board,
        &prepared.board_settings,
        &CancelToken::new(),
    );
    let board: &BasicBoard = &prepared.board;
    let u1 = component(board, "U1");
    assert_eq!(report.parts, vec![u1], "only the QFN qualifies");
    let layout = analyze(board, u1).unwrap();
    assert_eq!(24, report.outcomes.len(), "every side pin, EP excluded");
    let by_name: BTreeMap<String, &PinOutcome> = report
        .outcomes
        .iter()
        .map(|o| (name_of(board, &layout, o), o))
        .collect();
    for (name, o) in &by_name {
        match name.as_str() {
            "20" => assert_eq!(PinResult::Skipped(Skip::Unconnected), o.result, "pin 20"),
            "3" => assert_eq!(
                PinResult::Stub {
                    kind: StubKind::Widened,
                    attempt: 0
                },
                o.result,
                "the power pin"
            ),
            "10" => assert_eq!(
                PinResult::Stub {
                    kind: StubKind::Via,
                    attempt: 0
                },
                o.result,
                "the plane pin"
            ),
            _ => assert_eq!(
                PinResult::Stub {
                    kind: StubKind::Narrow,
                    attempt: 0
                },
                o.result,
                "pin {name}"
            ),
        }
    }
    assert_eq!(23, report.stubs());
    // The power stub reaches the class width past the pin field.
    let power = by_name["3"];
    let wide = um(board, 750.0);
    let field = layout.field;
    let outside = |p: IntPoint| {
        let q = layout.frame.to_part(p);
        !field.contains_inside_int(q) && q.x < field.ll.x
    };
    let widened: Vec<(Vec<IntPoint>, i32)> = power
        .items
        .iter()
        .map(|id| geometry(board, *id))
        .filter(|(_, hw)| *hw == wide)
        .collect();
    assert_eq!(1, widened.len(), "one piece at 1.5 mm");
    assert!(
        widened[0].0.iter().all(|p| outside(*p)),
        "{widened:?} past the W field edge"
    );
    let widths: Vec<i32> = power
        .items
        .iter()
        .map(|id| geometry(board, *id).1)
        .collect();
    assert!(
        widths.windows(2).all(|w| w[0] < w[1]),
        "the taper widens outward: {widths:?}"
    );
    assert_eq!(6, widths.len(), "narrow, 4 taper steps, wide");
    // The signal stubs keep the class width: 0.2 mm.
    let signal = by_name["1"];
    assert_eq!(
        vec![um(board, 100.0)],
        signal
            .items
            .iter()
            .map(|id| geometry(board, *id).1)
            .collect::<Vec<_>>()
    );
    // No DRC item, one via and none in a pad.
    assert!(
        board.all_clearance_violations().is_empty(),
        "{:?}",
        board.all_clearance_violations()
    );
    let vias: Vec<&Item> = board.items().filter(|i| i.is_via()).collect();
    assert_eq!(1, vias.len());
    assert!(vias.iter().all(|v| !via_in_pad(board, v)), "a via in a pad");
    // Every stub is connected to its pin.
    for o in report.outcomes.iter().filter(|o| !o.items.is_empty()) {
        let net = board.item(o.pin).unwrap().net_number(0);
        let set = board.connected_set(o.pin, net, false);
        assert!(o.items.iter().all(|id| set.contains(id)), "{o:?}");
    }
}

#[test]
fn the_stage_escapes_the_qfn_and_hands_the_rest_to_freerouting() {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    assert!(registry.fanout.contains_key(QFN_ESCAPE_ID));
    let mut prepared = prepare(
        POWER,
        &[
            ("fanout.enabled", "true"),
            ("fanout.algorithm", QFN_ESCAPE_ID),
        ],
    );
    let cancel = CancelToken::new();
    let mut ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    let mut stage = KrtQfnEscape::default();
    let summary =
        freerouting::autoroute::stages::FanoutStage::run(&mut stage, &mut prepared.board, &mut ctx);
    let report = stage.last_report.expect("a report");
    assert_eq!(23, report.stubs());
    assert!(summary.completed_passes >= 1);
    assert!(prepared.board.all_clearance_violations().is_empty());
    // The header's pins went to Freerouting's fanout: none of its vias
    // sits at a QFN stub.
    assert!(
        prepared
            .board
            .items()
            .filter(|i| i.is_via())
            .all(|v| !via_in_pad(&prepared.board, v))
    );
}

// ---------------------------------------------------------------------
// X10.3: rotated and mirrored copies
// ---------------------------------------------------------------------

/// An item's corners and half-width (0 for a via).
type Piece = (Vec<IntPoint>, i32);
/// Stubs by pin name: the result and the pieces in the part frame.
type Stubs = BTreeMap<String, (PinResult, Vec<Piece>)>;

/// The stubs of `part` keyed by pin name: kind and the items' geometry in
/// the part frame.
fn stubs_in_part_frame(board: &BasicBoard, layout: &PartLayout, outcomes: &[PinOutcome]) -> Stubs {
    outcomes
        .iter()
        .filter(|o| o.component == layout.component)
        .map(|o| {
            let items = o
                .items
                .iter()
                .map(|id| {
                    let (points, hw) = geometry(board, *id);
                    (
                        points
                            .into_iter()
                            .map(|p| layout.frame.to_part(p))
                            .collect(),
                        hw,
                    )
                })
                .collect();
            (name_of(board, layout, o), (o.result, items))
        })
        .collect()
}

#[test]
fn rotated_and_mirrored_copies_get_the_same_stubs() {
    let mut prepared = prepare(ROTATED, &[]);
    assert!(prepared.board.all_clearance_violations().is_empty());
    let report = escape(
        &mut prepared.board,
        &prepared.board_settings,
        &CancelToken::new(),
    );
    let board: &BasicBoard = &prepared.board;
    let copies: Vec<PartLayout> = ["U1", "U2", "U3", "U4"]
        .iter()
        .map(|n| analyze(board, component(board, n)).unwrap())
        .collect();
    assert_eq!(
        report.parts,
        copies.iter().map(|c| c.component).collect::<Vec<_>>()
    );
    let reference = stubs_in_part_frame(board, &copies[0], &report.outcomes);
    let count = |s: &Stubs| {
        s.values()
            .filter(|(r, _)| matches!(r, PinResult::Stub { .. }))
            .count()
    };
    assert_eq!(23, count(&reference));
    for copy in &copies[1..] {
        let name = &board.components.get(copy.component).unwrap().name;
        let stubs = stubs_in_part_frame(board, copy, &report.outcomes);
        assert_eq!(count(&reference), count(&stubs), "{name}");
        for (pin, (result, items)) in &reference {
            let (r, other) = &stubs[pin];
            assert_eq!(result, r, "{name}-{pin}");
            assert_eq!(items.len(), other.len(), "{name}-{pin}");
            for ((a, wa), (b, wb)) in items.iter().zip(other) {
                assert_eq!(wa, wb, "{name}-{pin} width");
                for (p, q) in a.iter().zip(b) {
                    assert!(
                        (p.x - q.x).abs() <= 1 && (p.y - q.y).abs() <= 1,
                        "{name}-{pin}: {p:?} vs {q:?}"
                    );
                }
            }
        }
    }
    assert!(
        board.all_clearance_violations().is_empty(),
        "{:?}",
        board.all_clearance_violations()
    );
    assert!(
        board
            .items()
            .filter(|i| i.is_via())
            .all(|v| !via_in_pad(board, v))
    );
    assert_eq!(
        4,
        board.items().filter(|i| i.is_via()).count(),
        "one plane via per copy"
    );
}

// ---------------------------------------------------------------------
// Selection and the X10.4 harness
// ---------------------------------------------------------------------

fn krt_registry() -> StageRegistry {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    registry
}

#[test]
fn the_registry_selects_krt_qfn_escape_in_the_fanout_slot() {
    let registry = krt_registry();
    let prepared = prepare(POWER, &[("fanout.algorithm", QFN_ESCAPE_ID)]);
    let selection = StageSelection::resolve(&prepared.settings, &registry);
    assert!(selection.warnings.is_empty(), "{:?}", selection.warnings);
    assert_eq!(selection.fanout.id, QFN_ESCAPE_ID);
    let pipeline = registry
        .build(&selection, &prepared.settings)
        .expect("built");
    assert_eq!(pipeline.fanout.id(), QFN_ESCAPE_ID);
    // Unset, the fanout slot stays Freerouting's.
    let default = prepare(POWER, &[]);
    let selection = StageSelection::resolve(&default.settings, &registry);
    assert_eq!(selection.fanout.id, "freerouting-fanout");
    assert!(selection.fanout.from_default);
}

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

/// What X10.4 counts on a routed board against its input.
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(dead_code)] // read through Debug
struct Gates {
    /// Airlines left on the routed board (the signal).
    unrouted: usize,
    /// Clearance violations with an item the run created.
    drc_new: usize,
    /// Vias the run created whose copper overlaps a pin's pad.
    via_in_pad: usize,
    /// Vias on the routed board.
    vias: usize,
    /// Q02.1: copper under its net's class width, mm.
    under_width_mm: f64,
}

fn gates(input: &BasicBoard, routed: &BasicBoard) -> Gates {
    let max_input_id = input.communication.id_generator.max_generated_id();
    let new = |id: ItemId| id.0 > max_input_id;
    let violations = routed.all_clearance_violations();
    let widths = freerouting::quality::neckdown::width_report::report(routed);
    Gates {
        unrouted: freerouting::drc::DesignRulesChecker::new(routed)
            .all_airlines()
            .len(),
        drc_new: violations
            .iter()
            .filter(|v| new(v.first_item) || new(v.second_item))
            .count(),
        via_in_pad: routed
            .items()
            .filter(|i| i.is_via() && new(i.id()) && via_in_pad(routed, i))
            .count(),
        vias: routed.items().filter(|i| i.is_via()).count(),
        under_width_mm: widths.under_total() / widths.units_per_mm,
    }
}

/// X10.4's board eligibility on `X10_SET`: the qualifying parts of each
/// board and their side pins (load only).
#[test]
#[ignore = "a release run over a board set; set X10_SET"]
fn qualifying_parts_on_a_set() {
    for dsn in set_boards("X10_SET") {
        let prepared = prepare(&dsn, &[]);
        let board: &BasicBoard = &prepared.board;
        let parts = freerouting_krt::fanout::qfn::qualifying_parts(board);
        let names: Vec<String> = parts
            .iter()
            .map(|p| {
                let pins: usize = p.sides.iter().map(Vec::len).sum();
                format!(
                    "{} ({pins} pins, pitch {} um)",
                    p.name,
                    p.pitch as f64 / um(board, 1.0) as f64
                )
            })
            .collect();
        println!(
            "{}: {} parts: {}",
            stem(&dsn),
            parts.len(),
            names.join(", ")
        );
    }
}

/// X10.4's arms on `X10_SET` (or `X10_DSN`), default settings plus
/// `fanout.enabled`: `fanout.algorithm` per arm of `X10_ARMS` (default
/// both); the session of each run goes to `X10_SES_DIR/<stem>.<arm>.ses`
/// when set.
#[test]
#[ignore = "a release run over a board set; set X10_SET or X10_DSN"]
fn escape_arms_on_a_set() {
    let boards = match std::env::var("X10_DSN") {
        Ok(dsn) => vec![dsn],
        Err(_) => set_boards("X10_SET"),
    };
    let arms =
        std::env::var("X10_ARMS").unwrap_or_else(|_| format!("freerouting-fanout,{QFN_ESCAPE_ID}"));
    let registry = krt_registry();
    for dsn in boards {
        for arm in arms.split(',') {
            let started = Instant::now();
            let prepared = prepare(
                &dsn,
                &[("fanout.enabled", "true"), ("fanout.algorithm", arm)],
            );
            let input = prepared.board.clone();
            let routed = route_prepared(prepared, &registry);
            if let Ok(dir) = std::env::var("X10_SES_DIR") {
                std::fs::create_dir_all(&dir).unwrap();
                let path = PathBuf::from(dir).join(format!("{}.{arm}.ses", stem(&dsn)));
                std::fs::write(path, write_ses(&routed, &stem(&dsn), SesStyle::Java)).unwrap();
            }
            let g = gates(&input, &routed);
            println!(
                "{} {arm}: {g:?} ({:.1} s)",
                stem(&dsn),
                started.elapsed().as_secs_f64()
            );
        }
    }
}

#[test]
fn two_escapes_of_the_same_board_write_the_same_session() {
    let run = || {
        let mut prepared = prepare(ROTATED, &[]);
        escape(
            &mut prepared.board,
            &prepared.board_settings,
            &CancelToken::new(),
        );
        write_ses(&prepared.board, "x10-qfn-rotated", SesStyle::Java)
    };
    let (a, b) = (run(), run());
    assert!(a.contains("(wire"), "the session holds the stubs");
    assert_eq!(a, b);
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

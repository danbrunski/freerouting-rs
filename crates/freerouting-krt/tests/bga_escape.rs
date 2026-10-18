//! X12: the ball array, channel capacity and layer balancing, dog-bone
//! vias and the `krt-bga-escape` stage.
//!
//! Boards (synthetic, in `tests/data/`, written by X12 as X10's are; not
//! under `fixtures/`, where every DSN needs a Java reference):
//! - `x12-arrays.dsn` (2 layers): a full 10 × 10 0.8 mm BGA (B1), the same
//!   with its centre 4 × 4 depopulated (B2), a staggered checkerboard on
//!   the same lattice (B3), an 8 × 8 0.5 mm BGA with 0.25 mm balls (B4) and
//!   a 0.5 mm QFN-24 (Q1); each ball's net joins that position of every
//!   array; vias 0.45 and 0.6 mm.
//! - `x12-bga-0p8.dsn` (F.Cu, In1.Cu GND plane, In2.Cu, In3.Cu, B.Cu): the
//!   10 × 10 0.8 mm BGA (U1), centre 4 × 4 on GND, every other ball to its
//!   own test point on a ring around it; rules 0.1 mm width and clearance,
//!   vias 0.45 and 0.6 mm.
//! - `x12-bga-rotated.dsn`: the same part at 0° (U1), 90° (U2) and on the
//!   back (U3); each ball's net joins that ball of every copy.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{FanoutStage, StageCtx, StageRegistry, StageSelection};
use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::ComponentId;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{FloatPoint, IntBox, IntPoint};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::ResolveContext;
use freerouting::settings::Source;
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::profiles::TestingProfile;
use freerouting_krt::fanout::bga::channels::{
    Band, Demand, EscapeLayer, Slot, assign, capacity, slot_offset,
};
use freerouting_krt::fanout::bga::emit::{BallOutcome, arrays};
use freerouting_krt::fanout::bga::vias::{DogBone, choose, fits, site, via_options};
use freerouting_krt::fanout::bga::{
    ArrayKind, BGA_ESCAPE_ID, BallArray, BallResult, KrtBgaEscape, Rejection, Skip, analyze_array,
    escape,
};
use freerouting_krt::fanout::frame::{PartFrame, Side};
use freerouting_krt::fanout::rules::via_in_pad;

const ARRAYS: &str = "crates/freerouting-krt/tests/data/x12-arrays.dsn";
const BGA_0P8: &str = "crates/freerouting-krt/tests/data/x12-bga-0p8.dsn";
const ROTATED: &str = "crates/freerouting-krt/tests/data/x12-bga-rotated.dsn";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `dsn`, with `extra` (`path=value`) on the
/// testing source.
fn prepare(dsn: &str, extra: &[(&str, &str)]) -> PreparedJob {
    let path = if Path::new(dsn).is_absolute() {
        PathBuf::from(dsn)
    } else {
        repo_root().join(dsn)
    };
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

fn um(board: &BasicBoard, v: f64) -> i64 {
    i64::from(freerouting::job::overrides::um_to_board_units(board, v))
}

fn run_escape(prepared: &mut PreparedJob) -> freerouting_krt::fanout::bga::EscapeReport {
    escape(
        &mut prepared.board,
        &prepared.board_settings,
        &CancelToken::new(),
    )
}

// ---------------------------------------------------------------------
// X12.1: the array model
// ---------------------------------------------------------------------

#[test]
fn a_full_a_depopulated_and_a_staggered_array_are_recognised() {
    let prepared = prepare(ARRAYS, &[]);
    let board: &BasicBoard = &prepared.board;
    let full = analyze_array(board, component(board, "B1")).expect("B1 is an array");
    assert_eq!((10, 10), (full.rows, full.cols));
    assert_eq!(um(board, 800.0), full.pitch);
    assert_eq!(ArrayKind::Full, full.kind());
    assert_eq!(100, full.ball_of.len());
    assert_eq!(full.pad_layer, 0);
    // Row 0 is the top: ball R0C0 is at the top-left of the part frame.
    let r0c0 = full.ball_of[&(0, 0)];
    let centre = full
        .frame
        .to_part(board.item(r0c0).unwrap().drill().unwrap().center);
    assert_eq!(centre, full.centre(0, 0));
    assert!(centre.x < 0 && centre.y > 0, "{centre:?}");

    let depop = analyze_array(board, component(board, "B2")).expect("B2 is an array");
    assert_eq!((10, 10), (depop.rows, depop.cols));
    assert_eq!(ArrayKind::Depopulated, depop.kind());
    assert_eq!(84, depop.ball_of.len());
    assert!(!depop.is_populated(4, 4) && depop.is_populated(2, 2));

    assert_eq!(
        Err(Rejection::Staggered),
        analyze_array(board, component(board, "B3"))
    );
    let fine = analyze_array(board, component(board, "B4")).expect("B4 is an array");
    assert_eq!((8, 8, um(board, 500.0)), (fine.rows, fine.cols, fine.pitch));
    assert!(analyze_array(board, component(board, "Q1")).is_err());

    // The stage reports the rejected parts with enough pins.
    let (found, rejected) = arrays(board);
    let names = |ids: Vec<ComponentId>| -> Vec<String> {
        ids.iter()
            .map(|c| board.components.get(*c).unwrap().name.clone())
            .collect()
    };
    assert_eq!(
        vec!["B1", "B2", "B4"],
        names(found.iter().map(|a| a.component).collect())
    );
    assert_eq!(
        vec!["B3", "Q1"],
        names(rejected.iter().map(|r| r.0).collect())
    );
    assert_eq!(Rejection::Staggered, rejected[0].1);
}

/// A bare `rows × cols` array at pitch 800 for the planning tests.
fn bare_array(rows: u16, cols: u16) -> BallArray {
    let mut ball_of = BTreeMap::new();
    for r in 0..rows {
        for c in 0..cols {
            ball_of.insert((r, c), ItemId(i32::from(r) * 100 + i32::from(c) + 1));
        }
    }
    BallArray {
        component: ComponentId(1),
        name: "U".into(),
        frame: PartFrame::new(FloatPoint::new(0.0, 0.0), 0.0, false, false),
        rows,
        cols,
        pitch: 800,
        origin: IntPoint::new(0, 0),
        populated: vec![true; usize::from(rows) * usize::from(cols)],
        ball_of,
        pad_layer: 0,
        field: IntBox::new(0, 0, 1, 1),
        outline: None,
    }
}

#[test]
fn rings_sides_and_the_escape_order_follow_the_pinwheel() {
    let a = bare_array(10, 10);
    assert_eq!(5, a.ring_count());
    assert_eq!((0, 4, 2), (a.ring(0, 5), a.ring(4, 5), a.ring(7, 2)));
    // Each side owns the corner it starts at, clockwise from the top-left.
    assert_eq!(
        [Side::N, Side::E, Side::S, Side::W],
        [a.side(0, 0), a.side(0, 9), a.side(9, 9), a.side(9, 0)]
    );
    assert_eq!([Side::N, Side::W], [a.side(0, 8), a.side(1, 0)]);
    let order = a.escape_order();
    assert_eq!(100, order.len());
    assert_eq!(&[(0, 0), (0, 1)], &order[..2]);
    assert_eq!((0, 9), order[9]);
    assert_eq!((9, 9), order[18]);
    assert_eq!((1, 0), order[35]);
    assert_eq!((1, 1), order[36]);
    let per_side = |k: u16, s: Side| {
        a.ring_positions(k)
            .iter()
            .filter(|&&(r, c)| a.side(r, c) == s)
            .count()
    };
    for k in 0..4 {
        for s in Side::ALL {
            assert_eq!(
                usize::from(9 - 2 * k),
                per_side(k, s),
                "ring {k} side {s:?}"
            );
        }
    }
    // Every via site is distinct.
    let sites: std::collections::BTreeSet<(i32, i32)> = order
        .iter()
        .map(|&(r, c)| {
            let s = site(&a, r, c);
            (s.x, s.y)
        })
        .collect();
    assert_eq!(100, sites.len());
}

// ---------------------------------------------------------------------
// X12.2: channel capacity and layer balancing
// ---------------------------------------------------------------------

#[test]
fn the_capacity_formula_at_0p8_0p65_and_0p5_mm_pitch() {
    // (pitch, obstacle, width, clearance) in µm, expected traces.
    let cases = [
        // 0.8 mm: 0.4 mm ball, 0.127 mm rules (the checklist's form).
        (800, 400, 127, 127, 1),
        // 0.8 mm: a 0.45 mm via, 0.1 mm rules (KRT #158's example budget).
        (800, 450, 100, 100, 1),
        // 0.8 mm: a 0.3 mm micro via, 0.1 mm rules.
        (800, 300, 100, 100, 2),
        // 0.8 mm between 0.4 mm balls with 0.075 mm rules.
        (800, 400, 75, 75, 2),
        // 0.65 mm: a 0.3 mm ball, 0.1 mm rules.
        (650, 300, 100, 100, 1),
        // 0.65 mm: a 0.35 mm via, 0.1 mm rules.
        (650, 350, 100, 100, 1),
        // 0.5 mm: a 0.25 mm ball, 0.1 mm rules.
        (500, 250, 100, 100, 0),
        // 0.5 mm: a 0.25 mm ball, 0.075 mm rules.
        (500, 250, 75, 75, 1),
        // 0.5 mm: a 0.3 mm via, 0.075 mm rules.
        (500, 300, 75, 75, 0),
    ];
    for (pitch, obstacle, w, c, expected) in cases {
        let n = capacity(pitch, obstacle, w, c);
        assert_eq!(expected, n, "{pitch} {obstacle} {w} {c}");
        let gap = pitch - obstacle;
        let need = |n: i64| n * w + (n + 1) * c;
        assert!(need(i64::from(n)) <= gap, "{pitch}: {n} traces fit");
        assert!(need(i64::from(n) + 1) > gap, "{pitch}: one more does not");
    }
    assert_eq!(0, capacity(500, 600, 100, 100));
}

/// The demands of every ball of `a` in escape order, all layers allowed.
fn demands_of(a: &BallArray, layers: usize) -> Vec<Demand> {
    a.escape_order()
        .into_iter()
        .map(|(r, c)| {
            let side = a.side(r, c);
            let line = match side {
                Side::W | Side::E => r,
                Side::S | Side::N => c,
            };
            Demand {
                ring: a.ring(r, c),
                band: Band { side, line },
                allowed: (1u64 << layers) - 1,
            }
        })
        .collect()
}

fn four_layers(capacity: u32) -> Vec<EscapeLayer> {
    (0..4)
        .map(|i| EscapeLayer {
            layer: i,
            capacity,
            pad_layer: i == 0,
        })
        .collect()
}

/// Asserts that `slots` fits the capacities and returns the counts per
/// ring and layer.
fn check_fits(
    layers: &[EscapeLayer],
    demands: &[Demand],
    slots: &[Option<Slot>],
) -> BTreeMap<u16, Vec<usize>> {
    let mut used: BTreeMap<(Band, usize), Vec<u32>> = BTreeMap::new();
    let mut per_ring: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (d, s) in demands.iter().zip(slots) {
        let Some(s) = s else { continue };
        assert!(
            !layers[s.layer].pad_layer || d.ring == 0,
            "only the outer ring escapes on the pad layer"
        );
        used.entry((d.band, s.layer)).or_default().push(s.slot);
        per_ring
            .entry(d.ring)
            .or_insert_with(|| vec![0; layers.len()])[s.layer] += 1;
    }
    for ((band, layer), v) in &used {
        assert!(
            v.len() as u32 <= layers[*layer].capacity,
            "{band:?} {layer}"
        );
        let expected: Vec<u32> = (0..v.len() as u32).collect();
        assert_eq!(&expected, v, "slots in order from the via edge");
    }
    per_ring
}

#[test]
fn a_10x10_array_on_4_signal_layers_balances_each_ring() {
    let a = bare_array(10, 10);
    let demands = demands_of(&a, 4);
    // Two traces per band: every ball fits and each ring spreads evenly.
    let layers = four_layers(2);
    let slots = assign(&layers, 40, &demands);
    assert!(slots.iter().all(Option::is_some));
    let per_ring = check_fits(&layers, &demands, &slots);
    for (ring, counts) in &per_ring {
        let eligible: Vec<usize> = if *ring == 0 {
            counts.clone()
        } else {
            counts[1..].to_vec()
        };
        let (min, max) = (
            eligible.iter().min().unwrap(),
            eligible.iter().max().unwrap(),
        );
        assert!(max - min <= 1, "ring {ring}: {counts:?}");
    }
    assert_eq!(vec![9, 9, 9, 9], per_ring[&0]);
    // One trace per band: the middle bands are full, the rest fits.
    let layers = four_layers(1);
    let slots = assign(&layers, 40, &demands);
    let per_ring = check_fits(&layers, &demands, &slots);
    let placed: usize = per_ring.values().flatten().sum();
    assert!(placed < demands.len() && placed >= 80, "{placed}");
    assert!(per_ring[&0].iter().all(|n| *n == 9), "{:?}", per_ring[&0]);
    // A demand that allows no layer gets none.
    let none = Demand {
        allowed: 0,
        ..demands[0]
    };
    assert_eq!(vec![None], assign(&layers, 40, &[none]));
}

#[test]
fn slots_start_at_the_via_edge_and_stay_inside_the_band() {
    // Two slots of 200 apart: the first toward σ, the second away.
    assert_eq!(100, slot_offset(0, 2, 200, 1));
    assert_eq!(-100, slot_offset(1, 2, 200, 1));
    assert_eq!(-100, slot_offset(0, 2, 200, -1));
    assert_eq!(0, slot_offset(0, 1, 200, 1));
    // Odd products round toward the band's line.
    assert_eq!(150, slot_offset(0, 2, 301, 1));
}

// ---------------------------------------------------------------------
// X12.3: dog-bone vias
// ---------------------------------------------------------------------

#[test]
fn no_via_overlaps_a_pad_and_a_0p5_mm_array_is_not_escapable_by_vias() {
    let mut prepared = prepare(ARRAYS, &[]);
    let (found, _) = arrays(&prepared.board);
    let board: &BasicBoard = &prepared.board;
    for array in &found {
        let fine = array.name == "B4";
        for (&(r, c), &pin) in &array.ball_of {
            let net = board.item(pin).unwrap().net_number(0);
            let options = via_options(board, net);
            // The default via rule keeps the smaller via of one span (Java
            // `createDefaultViaRule`): VIA450.
            assert_eq!(1, options.len());
            let smallest = &options[0];
            assert_eq!(
                um(board, 225.0) as i32,
                smallest.radius_on(0),
                "VIA450 first"
            );
            match choose(board, array, r, c, &options, |_| true) {
                DogBone::Via { via, at } => {
                    assert!(!fine, "{} {r},{c}", array.name);
                    assert_eq!(&via, smallest, "the smallest via that fits");
                    assert!(fits(board, array, r, c, &via));
                    // The via's copper on the pad layer is clear of every
                    // pad of the board by at least its clearance.
                    for pad in board.items().filter(|i| i.is_pin()) {
                        let shape = pad.drill().unwrap().shape_on_layer(0);
                        if let Some(shape) = shape {
                            let d = shape.distance(at.to_float());
                            assert!(
                                d >= f64::from(via.radius_on(0))
                                    + f64::from(um(board, 100.0) as i32),
                                "{} {r},{c}: {d}",
                                array.name
                            );
                        }
                    }
                }
                DogBone::NoViaFits => assert!(fine, "{} {r},{c}", array.name),
            }
        }
    }
    let report = run_escape(&mut prepared);
    let board: &BasicBoard = &prepared.board;
    let b4 = component(board, "B4");
    let b4_balls: Vec<&BallOutcome> = report
        .outcomes
        .iter()
        .filter(|o| o.component == b4)
        .collect();
    assert_eq!(64, b4_balls.len());
    assert!(
        b4_balls
            .iter()
            .all(|o| o.result == BallResult::Skipped(Skip::NoViaFits))
    );
    let vias: Vec<&Item> = board.items().filter(|i| i.is_via()).collect();
    assert!(!vias.is_empty());
    assert!(vias.iter().all(|v| !via_in_pad(board, v)), "a via in a pad");
    assert!(
        board.all_clearance_violations().is_empty(),
        "{:?}",
        board.all_clearance_violations()
    );
}

// ---------------------------------------------------------------------
// X12.4: emission
// ---------------------------------------------------------------------

#[test]
fn every_signal_ball_escapes_and_every_plane_ball_gets_a_via() {
    let mut prepared = prepare(BGA_0P8, &[]);
    assert!(prepared.board.all_clearance_violations().is_empty());
    let report = run_escape(&mut prepared);
    let board: &BasicBoard = &prepared.board;
    for line in report.log(board) {
        println!("{line}");
    }
    let u1 = component(board, "U1");
    println!("layers: {:?}", report.layers);
    assert_eq!(vec![u1], report.parts);
    assert_eq!(100, report.outcomes.len());
    let array = analyze_array(board, u1).unwrap();
    let gnd = board.item(array.ball_of[&(3, 3)]).unwrap().net_number(0);
    for o in &report.outcomes {
        let item = board.item(o.pin).unwrap();
        let net = item.net_number(0);
        let vias = o
            .items
            .iter()
            .filter(|id| board.item(**id).unwrap().is_via())
            .count();
        assert_eq!(1, vias, "{o:?}");
        if net == gnd {
            assert_eq!(BallResult::PlaneVia, o.result, "{o:?}");
            assert_eq!(2, o.items.len(), "dog-bone and via");
        } else {
            assert!(matches!(o.result, BallResult::Escaped { .. }), "{o:?}");
            assert_eq!(3, o.items.len(), "dog-bone, via and escape");
            let escape = board.item(o.items[2]).unwrap();
            let BallResult::Escaped { layer, .. } = o.result else {
                unreachable!()
            };
            assert_eq!(layer, escape.first_layer());
        }
        let set = board.connected_set(o.pin, net, false);
        assert!(o.items.iter().all(|id| set.contains(id)), "{o:?}");
    }
    assert_eq!((84, 16), (report.escapes(), report.plane_vias()));
    let per_layer = report.escapes_per_layer(u1);
    println!("escapes per layer: {per_layer:?}");
    assert_eq!(4, per_layer.len(), "{per_layer:?}");
    assert!(
        board.all_clearance_violations().is_empty(),
        "{:?}",
        board.all_clearance_violations()
    );
    let vias: Vec<&Item> = board.items().filter(|i| i.is_via()).collect();
    assert_eq!(100, vias.len());
    assert!(vias.iter().all(|v| !via_in_pad(board, v)), "a via in a pad");
}

fn krt_registry() -> StageRegistry {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    registry
}

#[test]
fn the_stage_escapes_the_bga_and_hands_the_rest_to_freerouting() {
    let registry = krt_registry();
    assert!(registry.fanout.contains_key(BGA_ESCAPE_ID));
    let mut prepared = prepare(
        BGA_0P8,
        &[
            ("fanout.enabled", "true"),
            ("fanout.algorithm", BGA_ESCAPE_ID),
        ],
    );
    let cancel = CancelToken::new();
    let mut ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    let mut stage = KrtBgaEscape::default();
    let summary = FanoutStage::run(&mut stage, &mut prepared.board, &mut ctx);
    let report = stage.last_report.expect("a report");
    assert_eq!(84, report.escapes());
    assert!(summary.completed_passes >= 1);
    let board: &BasicBoard = &prepared.board;
    assert!(board.all_clearance_violations().is_empty());
    assert!(
        board
            .items()
            .filter(|i| i.is_via())
            .all(|v| !via_in_pad(board, v))
    );
}

#[test]
fn the_registry_selects_krt_bga_escape_in_the_fanout_slot() {
    let registry = krt_registry();
    let prepared = prepare(BGA_0P8, &[("fanout.algorithm", BGA_ESCAPE_ID)]);
    let selection = StageSelection::resolve(&prepared.settings, &registry);
    assert!(selection.warnings.is_empty(), "{:?}", selection.warnings);
    assert_eq!(selection.fanout.id, BGA_ESCAPE_ID);
    let pipeline = registry
        .build(&selection, &prepared.settings)
        .expect("built");
    assert_eq!(pipeline.fanout.id(), BGA_ESCAPE_ID);
    let default = prepare(BGA_0P8, &[]);
    let selection = StageSelection::resolve(&default.settings, &registry);
    assert_eq!(selection.fanout.id, "freerouting-fanout");
    assert!(selection.fanout.from_default);
}

/// The SES of the escapes of each X12 board, written to `X12_SES_OUT`
/// (a directory) for `tools/determinism-check.sh`.
#[test]
#[ignore = "writes sessions; set X12_SES_OUT"]
fn write_escape_sessions() {
    let dir = PathBuf::from(std::env::var("X12_SES_OUT").expect("X12_SES_OUT"));
    std::fs::create_dir_all(&dir).unwrap();
    for dsn in [ARRAYS, BGA_0P8, ROTATED] {
        let mut prepared = prepare(dsn, &[]);
        let report = run_escape(&mut prepared);
        let name = stem(dsn);
        let mut text = write_ses(&prepared.board, &name, SesStyle::Java);
        for line in report.log(&prepared.board) {
            text.push_str(&line);
            text.push('\n');
        }
        std::fs::write(dir.join(format!("{name}.ses")), text).unwrap();
    }
}

#[test]
fn two_escapes_of_the_same_board_write_the_same_session() {
    let run = || {
        let mut prepared = prepare(BGA_0P8, &[]);
        run_escape(&mut prepared);
        write_ses(&prepared.board, "x12-bga-0p8", SesStyle::Java)
    };
    let (a, b) = (run(), run());
    assert!(a.contains("(wire"), "the session holds the escapes");
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------
// X12.5: rotated and back-side copies
// ---------------------------------------------------------------------

/// An item's corners in the part frame, its half-width (0 for a via) and
/// the index of its layer in the part's own escape-layer order (-1 for a
/// via).
type Piece = (Vec<IntPoint>, i32, i32);

fn pieces(board: &BasicBoard, frame: &PartFrame, own: &[i32], items: &[ItemId]) -> Vec<Piece> {
    let index = |l: i32| own.iter().position(|&x| x == l).map_or(-2, |i| i as i32);
    items
        .iter()
        .map(|id| {
            let item = board.item(*id).unwrap();
            let (points, hw, layer) = if let Some(t) = item.as_trace() {
                let points = t
                    .polyline
                    .corner_approx_arr()
                    .iter()
                    .map(|p| p.round())
                    .collect::<Vec<_>>();
                (points, t.half_width, index(item.first_layer()))
            } else {
                (vec![item.drill().unwrap().center], 0, -1)
            };
            (
                points.into_iter().map(|p| frame.to_part(p)).collect(),
                hw,
                layer,
            )
        })
        .collect()
}

#[test]
fn rotated_and_back_side_copies_get_the_same_escapes() {
    let mut prepared = prepare(ROTATED, &[]);
    assert!(prepared.board.all_clearance_violations().is_empty());
    let report = run_escape(&mut prepared);
    let board: &BasicBoard = &prepared.board;
    let copies: Vec<BallArray> = ["U1", "U2", "U3"]
        .iter()
        .map(|n| analyze_array(board, component(board, n)).unwrap())
        .collect();
    assert_eq!(
        report.parts,
        copies.iter().map(|c| c.component).collect::<Vec<_>>()
    );
    assert!(copies[2].frame.mirrored() && copies[2].pad_layer == 4);
    // Layers are compared by their index in each copy's own order (the
    // pad layer first): F.Cu, In2.Cu, In3.Cu, B.Cu on the front and
    // B.Cu, In3.Cu, In2.Cu, F.Cu on the back.
    let own_layers = |copy: &BallArray| -> Vec<i32> {
        let (_, layers) = report
            .layers
            .iter()
            .find(|(c, _)| *c == copy.component)
            .expect("the copy's layers");
        layers.iter().map(|l| l.layer).collect()
    };
    assert_eq!(vec![0, 2, 3, 4], own_layers(&copies[0]));
    assert_eq!(vec![4, 3, 2, 0], own_layers(&copies[2]));
    let of = |copy: &BallArray| -> BTreeMap<(u16, u16), (BallResult, Vec<Piece>)> {
        let own = own_layers(copy);
        report
            .outcomes
            .iter()
            .filter(|o| o.component == copy.component)
            .map(|o| {
                let result = match o.result {
                    BallResult::Escaped { layer, slot } => BallResult::Escaped {
                        layer: own.iter().position(|&l| l == layer).unwrap() as i32,
                        slot,
                    },
                    r => r,
                };
                (
                    o.position,
                    (result, pieces(board, &copy.frame, &own, &o.items)),
                )
            })
            .collect()
    };
    let reference = of(&copies[0]);
    let per_layer = |m: &BTreeMap<(u16, u16), (BallResult, Vec<Piece>)>| {
        let mut out: BTreeMap<i32, usize> = BTreeMap::new();
        for (r, _) in m.values() {
            if let BallResult::Escaped { layer, .. } = r {
                *out.entry(*layer).or_default() += 1;
            }
        }
        out
    };
    let counts = per_layer(&reference);
    println!("U1 escapes per own layer index: {counts:?}");
    assert_eq!(84, counts.values().sum::<usize>());
    for copy in &copies[1..] {
        let other = of(copy);
        assert_eq!(counts, per_layer(&other), "{}", copy.name);
        for (pos, (result, items)) in &reference {
            let (r, o) = &other[pos];
            assert_eq!(result, r, "{} {pos:?}", copy.name);
            assert_eq!(items.len(), o.len(), "{} {pos:?}", copy.name);
            for ((a, wa, la), (b, wb, lb)) in items.iter().zip(o) {
                assert_eq!((wa, la), (wb, lb), "{} {pos:?}", copy.name);
                assert_eq!(a.len(), b.len(), "{} {pos:?}", copy.name);
                for (p, q) in a.iter().zip(b) {
                    assert!(
                        (p.x - q.x).abs() <= 1 && (p.y - q.y).abs() <= 1,
                        "{} {pos:?}: {p:?} vs {q:?}",
                        copy.name
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
    let vias: Vec<&Item> = board.items().filter(|i| i.is_via()).collect();
    assert_eq!(300, vias.len());
    assert!(vias.iter().all(|v| !via_in_pad(board, v)));
}

// ---------------------------------------------------------------------
// The X12.6 harness
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

/// What X12.6 counts: escape completion after the fanout stage alone, and
/// the routed board against its input.
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(dead_code)] // read through Debug
struct Gates {
    /// Escape completion: balls of the arrays whose copper after the
    /// fanout stage holds a via (Java's fanout gives a pin a trace to a
    /// via, `krt-bga-escape` a dog-bone), of `escapable`.
    fanned: usize,
    /// Of `fanned`, the balls whose copper also reaches out of the pin
    /// field (the box of the array's pads).
    past_box: usize,
    /// Balls with one net that has another terminal.
    escapable: usize,
    /// Airlines left on the routed board (the signal).
    unrouted: usize,
    /// Clearance violations with an item the run created.
    drc_new: usize,
    /// Vias the run created whose copper overlaps a pin's pad.
    via_in_pad: usize,
    /// Vias on the routed board.
    vias: usize,
}

fn inside(outer: &IntBox, b: &IntBox) -> bool {
    outer.ll.x <= b.ll.x && outer.ll.y <= b.ll.y && b.ur.x <= outer.ur.x && b.ur.y <= outer.ur.y
}

/// `(fanned, past_box, escapable)` of `arrays` on `board` (see [`Gates`]).
fn escape_completion(board: &BasicBoard, arrays: &[BallArray]) -> (usize, usize, usize) {
    let (mut fanned, mut past, mut escapable) = (0, 0, 0);
    for array in arrays {
        let field = array.field;
        for &pin in array.ball_of.values() {
            let Some(item) = board.item(pin) else {
                continue;
            };
            if item.net_count() != 1 {
                continue;
            }
            let net = item.net_number(0);
            if !board.net_terminal_items(net).iter().any(|&id| id != pin) {
                continue;
            }
            escapable += 1;
            let copper: Vec<&Item> = board
                .connected_set(pin, net, false)
                .iter()
                .filter_map(|id| board.item(*id))
                .filter(|i| i.is_via() || i.as_trace().is_some())
                .collect();
            if copper.iter().any(|i| i.is_via()) {
                fanned += 1;
                if copper
                    .iter()
                    .any(|i| !inside(&field, &array.frame.box_to_part(&i.bounding_box())))
                {
                    past += 1;
                }
            }
        }
    }
    (fanned, past, escapable)
}

fn gates(
    input: &BasicBoard,
    fanned: &BasicBoard,
    routed: &BasicBoard,
    arrays: &[BallArray],
) -> Gates {
    let max_input_id = input.communication.id_generator.max_generated_id();
    let new = |id: ItemId| id.0 > max_input_id;
    let violations = routed.all_clearance_violations();
    let (fanned, past_box, escapable) = escape_completion(fanned, arrays);
    Gates {
        fanned,
        past_box,
        escapable,
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
    }
}

/// The board after the fanout stage `arm` alone.
fn fanout_only(prepared: &PreparedJob, registry: &StageRegistry, arm: &str) -> RoutingBoard {
    let mut board = prepared.board.clone();
    let cancel = CancelToken::new();
    let mut ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    let mut stage = (registry.fanout[arm])(&prepared.settings);
    stage.run(&mut board, &mut ctx);
    board
}

/// X12.6's board eligibility on `X12_SET`: the arrays of each board and
/// the rejected components with at least 16 pins (load only).
#[test]
#[ignore = "a release run over a board set; set X12_SET"]
fn arrays_on_a_set() {
    for dsn in set_boards("X12_SET") {
        let bytes = std::fs::read(repo_root().join(&dsn)).unwrap();
        let mut input = JobInput::new(bytes, JobInput::design_name_of(&dsn));
        input.rules = RulesChoice::Adjacent;
        let prepared = match prepare_job(&input, &ResolveContext { cores: 1 }) {
            Ok(p) => p,
            Err(e) => {
                println!("{}: not loaded ({e})", stem(&dsn));
                continue;
            }
        };
        let board: &BasicBoard = &prepared.board;
        let (found, rejected) = arrays(board);
        let names: Vec<String> = found
            .iter()
            .map(|a| {
                format!(
                    "{} ({}x{}, {} balls, pitch {} um)",
                    a.name,
                    a.rows,
                    a.cols,
                    a.ball_of.len(),
                    a.pitch as f64 / um(board, 1.0) as f64
                )
            })
            .collect();
        let rejected: Vec<String> = rejected
            .iter()
            .map(|(c, why)| format!("{} {}", board.components.get(*c).unwrap().name, why.name()))
            .collect();
        println!(
            "{}: {} arrays: {}; rejected: {}",
            stem(&dsn),
            found.len(),
            names.join(", "),
            rejected.join(", ")
        );
    }
}

/// X12.6's arms on `X12_SET` (or `X12_DSN`), default settings plus
/// `fanout.enabled`: `fanout.algorithm` per arm of `X12_ARMS` (default
/// both); the session of each run goes to `X12_SES_DIR/<stem>.<arm>.ses`
/// when set. With `X12_FANOUT_ONLY` set, only the fanout stage runs and
/// the counts are of its board (escape completion, `drc_new`,
/// `via_in_pad`).
#[test]
#[ignore = "a release run over a board set; set X12_SET or X12_DSN"]
fn escape_arms_on_a_set() {
    let boards = match std::env::var("X12_DSN") {
        Ok(dsn) => vec![dsn],
        Err(_) => set_boards("X12_SET"),
    };
    let arms =
        std::env::var("X12_ARMS").unwrap_or_else(|_| format!("freerouting-fanout,{BGA_ESCAPE_ID}"));
    let registry = krt_registry();
    for dsn in boards {
        for arm in arms.split(',') {
            let started = Instant::now();
            let prepared = prepare(
                &dsn,
                &[("fanout.enabled", "true"), ("fanout.algorithm", arm)],
            );
            let input = prepared.board.clone();
            let (found, _) = arrays(&input);
            let fanned = fanout_only(&prepared, &registry, arm);
            if arm == BGA_ESCAPE_ID {
                let mut board = input.clone();
                let report = escape(&mut board, &prepared.board_settings, &CancelToken::new());
                for line in report.log(&board) {
                    println!("  {line}");
                }
            }
            if std::env::var("X12_FANOUT_ONLY").is_ok() {
                let g = gates(&input, &fanned, &fanned, &found);
                println!(
                    "{} {arm} fanout only: {g:?} ({:.1} s)",
                    stem(&dsn),
                    started.elapsed().as_secs_f64()
                );
                continue;
            }
            let routed = route_prepared(prepared, &registry);
            if let Ok(dir) = std::env::var("X12_SES_DIR") {
                std::fs::create_dir_all(&dir).unwrap();
                let path = PathBuf::from(dir).join(format!("{}.{arm}.ses", stem(&dsn)));
                std::fs::write(path, write_ses(&routed, &stem(&dsn), SesStyle::Java)).unwrap();
            }
            let g = gates(&input, &fanned, &routed, &found);
            println!(
                "{} {arm}: {g:?} ({:.1} s)",
                stem(&dsn),
                started.elapsed().as_secs_f64()
            );
        }
    }
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

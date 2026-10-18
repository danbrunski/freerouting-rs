//! Q17.2 and Q17.3: the `bounded-neck` neck-down stage and the widen-back
//! with tapers (`quality::neckdown::{bounded_neck, widen_back}`).
//!
//! The fixtures are synthetic boards in `tests/data/quality/`:
//! - `q02-qfn-power.dsn` (Q02.4's fixture): a 0.5 mm pitch QFN (a
//!   staggered double row on the bottom side) whose inner-row pin U1-2 is
//!   net PWR (class Power_High, 1.5 mm, clearance 0.25 mm), to the big pad
//!   J1 across an open board. The trace must pass between two outer-row
//!   pads 0.75 mm apart, so Java's room-local neck-down cannot reach the
//!   pin (the `legacy` profile leaves it unrouted) and Java's retry at
//!   0.24 mm ships almost the whole 22 mm narrow;
//! - `q17-pinch-22mm.dsn`: PWR (1.5 mm) between two big pads 22 mm apart
//!   on one layer, through a 1 mm gap between two wall pads of other nets
//!   at x = 14.5..15.5 mm (the pinch: 0.4 mm traces fit, nothing wider).

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::basic_board::BasicBoard;
use freerouting::board::items::ItemId;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::optimize::PullTightParams;
use freerouting::quality::neckdown::bounded;
use freerouting::scoring::BoardStatistics;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, SettingsPatch, Source};
use freerouting_geometry::{IntBox, IntPoint};

const QFN: &str = "crates/freerouting/tests/data/quality/q02-qfn-power.dsn";
const PINCH: &str = "crates/freerouting/tests/data/quality/q17-pinch-22mm.dsn";
const Q02_PINCH: &str = "crates/freerouting/tests/data/quality/q02-pinch.dsn";
const Q03_QFN: &str = "crates/freerouting/tests/data/quality/q03-qfn-0p5.dsn";

/// Board units per micrometre on every fixture (`resolution um 10`).
const UNITS_PER_UM: i64 = 10;
/// The class half width of PWR, board units (1.5 mm).
const CLASS_HALF: i32 = 7500;
/// The taper length, board units (0.5 mm).
const TAPER: i64 = 500 * UNITS_PER_UM;

/// The arms of a test run.
#[derive(Clone, Copy, Debug, Default)]
struct Arm {
    bounded: bool,
    neck_width_um: f64,
    widen_back: bool,
}

const LEGACY: Arm = Arm {
    bounded: false,
    neck_width_um: 0.0,
    widen_back: false,
};

fn patch(arm: Arm) -> SettingsPatch {
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(3);
    let mut patch = profile.patch;
    patch.profile = Some("legacy".to_string());
    patch.neck_width_um = Some(arm.neck_width_um);
    if arm.bounded {
        patch.special.neck_down = Some("bounded-neck".to_string());
    }
    if arm.widen_back {
        patch.neck_down.widen_back = Some(true);
    }
    patch
}

fn prepare(dsn: &str, arm: Arm) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::None;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, patch(arm))];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn route(dsn: &str, arm: Arm) -> RoutingBoard {
    let prepared = prepare(dsn, arm);
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{dsn}: {}", f.error));
    outcome.board.take().expect("no listener took the board")
}

fn net_number(board: &BasicBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("net {name}"))
        .net_number
}

fn incomplete(board: &BasicBoard, net: i32) -> usize {
    let mut checker = freerouting::drc::DesignRulesChecker::new(board);
    checker.net_incompletes_of(net).map_or(0, |n| n.count())
}

/// The DRC items: clearance violations and unconnected items.
fn drc_items(board: &BasicBoard) -> (usize, usize) {
    let mut checker = freerouting::drc::DesignRulesChecker::new(board);
    let findings = checker.check();
    (
        findings.clearance_violations.len(),
        findings.unconnected_items.len(),
    )
}

/// Vias whose copper overlaps an SMD pad (as `freerouting-dev gate`'s
/// `via_in_pad_count`).
fn vias_in_pads(board: &BasicBoard) -> usize {
    let pads: Vec<_> = board
        .items()
        .filter(|i| i.is_pin())
        .filter_map(|i| i.drill())
        .filter(|d| d.first_layer == d.last_layer)
        .collect();
    board
        .items()
        .filter(|i| i.is_via())
        .filter_map(|i| i.drill())
        .filter(|via| {
            (via.first_layer..=via.last_layer).any(|layer| {
                via.shape_on_layer(layer).is_some_and(|v| {
                    pads.iter().any(|pad| {
                        pad.shape_on_layer(layer).is_some_and(|p| {
                            p.bounding_box().intersects_box(&v.bounding_box()) && p.intersects(v)
                        })
                    })
                })
            })
        })
        .count()
}

/// One wire of the SES: its net, width (SES units) and corners.
#[derive(Clone, Debug, PartialEq)]
struct Wire {
    net: String,
    width: i64,
    corners: Vec<(i64, i64)>,
}

/// The wires of `ses` (`(net N (wire (path LAYER WIDTH x y ...)))`), and
/// the session's units per micrometre.
fn ses_wires(ses: &str) -> (Vec<Wire>, i64) {
    let tokens: Vec<String> = ses
        .replace('(', " ( ")
        .replace(')', " ) ")
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let mut wires = Vec::new();
    let mut net = String::new();
    let mut resolution = 0;
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i].as_str() {
            "resolution" if resolution == 0 => {
                assert_eq!(tokens[i + 1], "um", "the SES is in micrometres");
                resolution = tokens[i + 2].parse().unwrap();
            }
            "net" => net = tokens[i + 1].trim_matches('"').to_string(),
            "path" => {
                let width: i64 = tokens[i + 2].parse().unwrap();
                let mut corners = Vec::new();
                let mut j = i + 3;
                while tokens[j] != ")" {
                    corners.push((tokens[j].parse().unwrap(), tokens[j + 1].parse().unwrap()));
                    j += 2;
                }
                wires.push(Wire {
                    net: net.clone(),
                    width,
                    corners,
                });
                i = j;
            }
            _ => {}
        }
        i += 1;
    }
    (wires, resolution)
}

fn length(corners: &[(i64, i64)]) -> f64 {
    corners
        .windows(2)
        .map(|w| ((w[1].0 - w[0].0) as f64).hypot((w[1].1 - w[0].1) as f64))
        .sum()
}

/// The PWR wires of `board`'s session in board units (the SES has the
/// board's resolution).
fn pwr_wires(board: &RoutingBoard, dsn: &str) -> Vec<Wire> {
    let (wires, resolution) = ses_wires(&write_ses(board, dsn, SesStyle::Java));
    assert_eq!(resolution, UNITS_PER_UM, "SES units are board units");
    wires.into_iter().filter(|w| w.net == "PWR").collect()
}

/// The neck zone of pin `pin` as `bounded-neck` computes it: the pin's
/// box enlarged by D (courtyard plus one pitch).
fn neck_zone(board: &BasicBoard, pin: ItemId) -> (IntBox, i64) {
    let item = board.item(pin).expect("the pin");
    let pin_box = item.bounding_box();
    let component = item.component_id();
    let courtyard = board
        .items()
        .filter(|i| i.component_id() == component)
        .filter_map(|i| i.as_component_outline().map(|_| i.bounding_box()))
        .reduce(|a, b| a.union_box(&b));
    let centers: Vec<IntPoint> = board
        .component_pins(component)
        .into_iter()
        .filter_map(|p| board.item(p).and_then(|i| i.drill()).map(|d| d.center))
        .collect();
    let center = item.drill().expect("a drill").center;
    let pitch = bounded::pitch(center, &centers);
    let d = bounded::neck_distance(&pin_box, courtyard.as_ref(), pitch);
    (pin_box, i64::from(d))
}

/// The pin of net `net` on component `name`'s pin `index`.
fn pin_of(board: &BasicBoard, net: i32) -> ItemId {
    board
        .items()
        .filter(|i| i.is_pin() && i.contains_net(net))
        .filter(|i| i.as_pin().is_some_and(|p| p.pin_index == 1))
        .map(|i| i.id())
        .next()
        .expect("U1-2 (the second pin of U1)")
}

fn within(p: (i64, i64), b: &IntBox, margin: i64) -> bool {
    p.0 >= i64::from(b.ll.x) - margin
        && p.0 <= i64::from(b.ur.x) + margin
        && p.1 >= i64::from(b.ll.y) - margin
        && p.1 <= i64::from(b.ur.y) + margin
}

// ---------------------------------------------------------------------------
// Q17.2: bounded-neck on Q02.4's fixture
// ---------------------------------------------------------------------------

#[test]
fn bounded_neck_routes_the_power_pin_that_java_leaves_unrouted() {
    let legacy = route(QFN, LEGACY);
    let pwr = net_number(&legacy, "PWR");
    assert_eq!(
        incomplete(&legacy, pwr),
        1,
        "Java's default leaves PWR unrouted"
    );
    let bounded = route(
        QFN,
        Arm {
            bounded: true,
            ..LEGACY
        },
    );
    assert_eq!(incomplete(&bounded, pwr), 0, "bounded-neck routes PWR");
}

/// Walks the SES: every PWR wire narrower than the class lies within D of
/// U1-2 (its box enlarged by D, plus the taper length), and the copper
/// beyond is at the class width; Java's retry at 0.24 mm on the same board
/// ships narrow copper far beyond D.
#[test]
fn bounded_neck_is_narrow_only_within_d_of_the_pin() {
    for widen_back in [false, true] {
        let board = route(
            QFN,
            Arm {
                bounded: true,
                widen_back,
                ..LEGACY
            },
        );
        let pwr = net_number(&board, "PWR");
        let (pin_box, d) = neck_zone(&board, pin_of(&board, pwr));
        // D = the courtyard's far side (5.15 mm above the pin) plus the
        // pitch to the outer row (0.943 mm).
        assert_eq!(d, 51_500 + 9_434, "D");
        let wires = pwr_wires(&board, QFN);
        let narrow: Vec<&Wire> = wires
            .iter()
            .filter(|w| w.width < 2 * i64::from(CLASS_HALF))
            .collect();
        assert!(!narrow.is_empty(), "the pin field needs a neck");
        for wire in &narrow {
            for corner in &wire.corners {
                assert!(
                    within(*corner, &pin_box, d + TAPER),
                    "widen_back {widen_back}: {wire:?} is narrow beyond D"
                );
            }
        }
        let wide: f64 = wires
            .iter()
            .filter(|w| w.width == 2 * i64::from(CLASS_HALF))
            .map(|w| length(&w.corners))
            .sum();
        let under: f64 = narrow.iter().map(|w| length(&w.corners)).sum();
        assert!(wide > 10.0 * 10_000.0, "most of the route is wide: {wide}");
        // Within the D box: at most its diagonal from the pin.
        assert!(under < 10.0 * 10_000.0, "{under}");
    }
    let retry = route(
        QFN,
        Arm {
            neck_width_um: 240.0,
            ..LEGACY
        },
    );
    let pwr = net_number(&retry, "PWR");
    let (pin_box, d) = neck_zone(&retry, pin_of(&retry, pwr));
    let beyond = pwr_wires(&retry, QFN)
        .iter()
        .filter(|w| w.width < 2 * i64::from(CLASS_HALF))
        .any(|w| w.corners.iter().any(|c| !within(*c, &pin_box, d + TAPER)));
    assert!(beyond, "Java's retry is narrow beyond D");
}

#[test]
fn bounded_neck_adds_no_clearance_violation_and_no_via_in_a_pad() {
    for widen_back in [false, true] {
        let board = route(
            QFN,
            Arm {
                bounded: true,
                widen_back,
                ..LEGACY
            },
        );
        assert_eq!(
            BoardStatistics::new(&board)
                .clearance_violations
                .total_count,
            0
        );
        assert_eq!(drc_items(&board).0, 0);
        assert_eq!(vias_in_pads(&board), 0);
    }
}

/// `router.neck_down.path_mm` fixes D: at 2.5 mm the narrow copper stays
/// within 2.5 mm (plus the taper) of the pin's box.
#[test]
fn a_fixed_path_length_bounds_the_neck() {
    let mut patch = patch(Arm {
        bounded: true,
        ..LEGACY
    });
    patch.neck_down.path_mm = Some(2.5);
    let path = repo_root().join(QFN);
    let mut input = JobInput::new(std::fs::read(&path).unwrap(), JobInput::design_name_of(QFN));
    input.rules = RulesChoice::None;
    input.extra_sources = vec![(Source::Testing, patch)];
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let board = run_pipeline(prepared.board, &job)
        .unwrap_or_else(|f| panic!("{}", f.error))
        .board
        .take()
        .unwrap();
    let pwr = net_number(&board, "PWR");
    assert_eq!(incomplete(&board, pwr), 0);
    let pin_box = board.item(pin_of(&board, pwr)).unwrap().bounding_box();
    for wire in pwr_wires(&board, QFN) {
        if wire.width < 2 * i64::from(CLASS_HALF) {
            for corner in &wire.corners {
                assert!(
                    within(*corner, &pin_box, 2500 * UNITS_PER_UM + TAPER),
                    "{wire:?}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Q17.3: widen-back with tapers
// ---------------------------------------------------------------------------

/// The PWR wires narrower than the class: (width, x range) in board units.
fn under_width(board: &RoutingBoard, dsn: &str) -> Vec<(i64, i64, i64)> {
    pwr_wires(board, dsn)
        .into_iter()
        .filter(|w| w.width < 2 * i64::from(CLASS_HALF))
        .map(|w| {
            let xs = w.corners.iter().map(|c| c.0);
            (w.width, xs.clone().min().unwrap(), xs.max().unwrap())
        })
        .collect()
}

const RETRY_400: Arm = Arm {
    bounded: false,
    neck_width_um: 400.0,
    widen_back: false,
};

/// KRT's #1033 case: Java's retry at 0.4 mm ships the whole 22 mm run
/// narrow; with the widen-back it is narrow only at the pinch (the walls
/// at x = 14.5..15.5 mm, plus the 1 mm a 1.5 mm trace keeps from them) and
/// in the two 4-step tapers.
#[test]
fn a_22mm_run_with_one_pinch_is_narrow_only_at_the_pinch_and_its_tapers() {
    let plain = route(PINCH, RETRY_400);
    let pwr = net_number(&plain, "PWR");
    assert_eq!(incomplete(&plain, pwr), 0);
    let all_narrow: f64 = pwr_wires(&plain, PINCH)
        .iter()
        .map(|w| length(&w.corners))
        .sum();
    assert_eq!(under_width(&plain, PINCH).len(), 1);
    assert!(all_narrow >= 22.0 * 10_000.0 - 20_000.0, "{all_narrow}");

    let board = route(
        PINCH,
        Arm {
            widen_back: true,
            ..RETRY_400
        },
    );
    assert_eq!(incomplete(&board, pwr), 0);
    let under = under_width(&board, PINCH);
    let pinch: Vec<_> = under.iter().filter(|(w, _, _)| *w == 4000).collect();
    let tapers: Vec<_> = under.iter().filter(|(w, _, _)| *w > 4000).collect();
    assert_eq!(pinch.len(), 1, "{under:?}");
    // The narrow run: the walls plus the 1 mm clearance of the class
    // width, to the next piece boundary.
    let (_, x0, x1) = *pinch[0];
    assert!(
        x0 >= 13_000 * UNITS_PER_UM && x1 <= 17_000 * UNITS_PER_UM,
        "{under:?}"
    );
    assert!(
        x0 <= 14_500 * UNITS_PER_UM && x1 >= 15_500 * UNITS_PER_UM,
        "{under:?}"
    );
    // Two tapers of 4 steps each, 0.5 mm long, next to the narrow run.
    assert_eq!(tapers.len(), 8, "{under:?}");
    let taper_len: i64 = tapers.iter().map(|(_, a, b)| b - a).sum();
    assert_eq!(taper_len, 2 * TAPER);
    for (_, a, b) in &tapers {
        assert!(*b <= x0 || *a >= x1, "a taper step outside the narrow run");
        assert!(
            *a >= x0 - TAPER && *b <= x1 + TAPER,
            "a taper step next to it"
        );
    }
}

/// Pull-tight over the whole board after the run keeps the narrow run and
/// the tapers as they are.
#[test]
fn pull_tight_does_not_remove_the_tapers() {
    let mut board = route(
        PINCH,
        Arm {
            widen_back: true,
            ..RETRY_400
        },
    );
    let before = under_width(&board, PINCH);
    board.start_marking_changed_area();
    board.mark_all_changed_area();
    board.opt_changed_area(&PullTightParams::new(500));
    let after = under_width(&board, PINCH);
    assert_eq!(before.len(), 9);
    assert_eq!(before, after);
}

/// Every arm on every Q02/Q03/Q17 fixture: the widen-back (and
/// `bounded-neck`) add no clearance violation and no unconnected item to
/// the run without them, and no via in a pad.
#[test]
fn widen_back_adds_no_drc_items_on_any_fixture() {
    for dsn in [QFN, PINCH, Q02_PINCH, Q03_QFN] {
        for (base, with) in [
            (
                RETRY_400,
                Arm {
                    widen_back: true,
                    ..RETRY_400
                },
            ),
            (
                Arm {
                    bounded: true,
                    ..LEGACY
                },
                Arm {
                    bounded: true,
                    widen_back: true,
                    ..LEGACY
                },
            ),
        ] {
            let without = drc_items(&route(dsn, base));
            let board = route(dsn, with);
            let (clearance, unconnected) = drc_items(&board);
            assert!(
                clearance <= without.0,
                "{dsn} {with:?}: {clearance} > {}",
                without.0
            );
            assert!(unconnected <= without.1, "{dsn} {with:?}");
            assert_eq!(vias_in_pads(&board), 0, "{dsn} {with:?}");
        }
    }
}

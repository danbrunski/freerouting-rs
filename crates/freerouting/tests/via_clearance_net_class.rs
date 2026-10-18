//! Q14: a via's clearance follows its net's class (`router.via_clearance`).
//!
//! The fixture `tests/data/quality/q14-hv-via.dsn` has two layers, the
//! default class (clearance 0.2 mm, `use_via` 0.6 mm) and class `HV`
//! (clearance 0.25 mm, `use_via` 0.8 mm). Net HV's F.Cu pad sits in a cage of
//! fixed default-class wires (net SIG): a room, a 1.12 mm corridor and a
//! pocket. The other HV pin is a through-hole pin outside, so a via must get
//! out of the cage.
//!
//! - Q14.1, the cause, with Java's rule (`via_info`, unset): the fanout's via
//!   rule is HV's vias followed by those of the board's first via rule
//!   (`fanout.fallback_to_board_vias`), whose 0.6 mm via info carries the
//!   default class. HV's 0.8 mm via fits nowhere near the pad, the fallback
//!   via fits in the corridor with 0.2 mm, and the via the fanout inserts on
//!   net HV has the default clearance class, 0.2322 mm from SIG. Java at
//!   `a773a70ec` writes the same session
//!   (`tests/data/quality/q14-hv-via.a773a70ec.ses`, from
//!   `tools/java-reference/run.sh <pin> route 5` on the fixture alone).
//! - Q14.2, the fix (`net_class`): every via checked or inserted for net HV
//!   uses HV's via clearance class, so HV's via keeps 0.25 mm from every
//!   other net, and the DRC flags a hand-placed HV via at 0.2 mm under both
//!   settings. The rules file round-trips the class-named via infos the DSN
//!   reader makes (`<padstack>-HV`).

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::via_clearance::ViaClearanceMode;
use freerouting::board::RoutingBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::drc::DesignRulesChecker;
use freerouting::geometry::{FloatPoint, IntPoint};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::io::specctra::{read_rules, read_session, rules_from_board};
use freerouting::job::prepare::{JobError, JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::rules::ItemClass;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const FIXTURE: &str = "crates/freerouting/tests/data/quality/q14-hv-via.dsn";
const JAVA_SES: &str = "crates/freerouting/tests/data/quality/q14-hv-via.a773a70ec.ses";
const NET_CLASS: &[(&str, &str)] = &[("via_clearance", "net_class")];
/// Board units per micrometre (`resolution um 10`).
const UNITS_PER_UM: f64 = 10.0;

fn fixture_text() -> String {
    std::fs::read_to_string(repo_root().join(FIXTURE)).expect("the Q14 fixture")
}

/// The fixture loaded as `run.sh route` loads it: the optimizer off, at most
/// five passes, plus `extra`.
fn prepare(dsn: &str, extra: &[(&str, &str)]) -> Result<PreparedJob, JobError> {
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(5);
    for (key, value) in extra {
        profile
            .patch
            .set_path(key, value)
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    let mut input = JobInput::new(dsn.as_bytes().to_vec(), "q14-hv-via");
    input.rules = RulesChoice::None;
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 })
}

fn route(extra: &[(&str, &str)]) -> RoutingBoard {
    let prepared = prepare(&fixture_text(), extra).expect("the fixture loads");
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

fn net(board: &RoutingBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("net {name}"))
        .net_number
}

fn class_no(board: &RoutingBoard, name: &str) -> i32 {
    board.rules.clearance_matrix.get_no(name)
}

fn vias_of(board: &RoutingBoard, net: i32) -> Vec<ItemId> {
    board
        .items()
        .filter(|i| i.as_via().is_some() && i.net_numbers() == [net])
        .map(Item::id)
        .collect()
}

fn segment_distance(p: FloatPoint, a: FloatPoint, b: FloatPoint) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length_square = dx * dx + dy * dy;
    let t = if length_square == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / length_square).clamp(0.0, 1.0)
    };
    (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
}

/// The smallest copper gap, in micrometres, between via `via` (a circle on
/// every layer of the fixture) and the traces and pads of other nets. The
/// pads of the fixture are axis-parallel rectangles or circles.
fn gap_to_other_nets_um(board: &RoutingBoard, via: ItemId) -> f64 {
    let item = board.item(via).expect("the via");
    let drill = &item.as_via().expect("a via").drill;
    let center = drill.center.to_float();
    let padstack = board
        .library
        .padstacks
        .get(drill.padstack)
        .expect("padstack");
    let radius = 0.5 * padstack.get_shape(0).expect("F.Cu shape").max_width();
    let mut gap = f64::MAX;
    for other in board.items() {
        if other
            .net_numbers()
            .iter()
            .any(|n| item.net_numbers().contains(n))
        {
            continue;
        }
        if let Some(trace) = other.as_trace() {
            let corners = trace.polyline.corner_count();
            for i in 0..corners.saturating_sub(1) {
                let a = trace.polyline.corner(i).to_float();
                let b = trace.polyline.corner(i + 1).to_float();
                let d = segment_distance(center, a, b) - f64::from(trace.half_width) - radius;
                gap = gap.min(d);
            }
        } else if other.is_pin() {
            let bb = other.bounding_box();
            let dx = (f64::from(bb.ll.x) - center.x)
                .max(center.x - f64::from(bb.ur.x))
                .max(0.0);
            let dy = (f64::from(bb.ll.y) - center.y)
                .max(center.y - f64::from(bb.ur.y))
                .max(0.0);
            gap = gap.min(dx.hypot(dy) - radius);
        }
    }
    gap / UNITS_PER_UM
}

fn padstack_name(board: &RoutingBoard, via: ItemId) -> String {
    let drill = &board.item(via).unwrap().as_via().unwrap().drill;
    board
        .library
        .padstacks
        .get(drill.padstack)
        .unwrap()
        .name
        .clone()
}

/// The `network_out` part of a session, which the router writes; the header
/// names the design and the host.
fn network_out(ses: &str) -> &str {
    let start = ses.find("(network_out").expect("network_out");
    &ses[start..]
}

/// Q14.1: the code path that gives net HV a default-class via. HV's own via
/// rule has only its class-named 0.8 mm via; the board's first via rule,
/// which the fanout appends, has the default class's 0.6 mm via.
#[test]
fn the_fanout_fallback_via_carries_the_default_class() {
    let prepared = prepare(&fixture_text(), &[]).expect("the fixture loads");
    let board = &prepared.board;
    let hv = net(board, "HV");
    let control = AutorouteControl::for_net_default(board, hv, &prepared.board_settings)
        .expect("HV's control");
    let names = |vias: &[freerouting::rules::ViaInfoId]| -> Vec<(String, i32)> {
        vias.iter()
            .map(|v| {
                let info = board.rules.via_infos.get(*v);
                (info.name.clone(), info.clearance_class)
            })
            .collect()
    };
    let hv_class = class_no(board, "HV");
    assert_eq!(
        names(control.via_rule_vias(&board.rules)),
        [("Via[0-1]_800:400_um-HV".to_string(), hv_class)]
    );
    assert_eq!(control.via_clearance_class, hv_class);
    let first = board.rules.via_rules.nth(0).expect("a board via rule");
    assert_eq!(
        names(board.rules.via_rules.get(first).vias()),
        [("Via[0-1]_600:300_um".to_string(), 1)],
        "the fanout's fallback via is the default class's"
    );
    assert!(prepared.settings.fanout.fallback_to_board_vias);
}

/// Q14.1: with Java's rule the via on net HV is the fallback 0.6 mm via with
/// the default clearance class, 0.2322 mm from SIG, and the session is
/// Java's.
#[test]
fn via_info_gives_net_hv_a_default_class_via_as_java_does() {
    let board = route(&[]);
    let hv = net(&board, "HV");
    let vias = vias_of(&board, hv);
    assert_eq!(vias.len(), 1, "one via gets HV out of the cage");
    let via = vias[0];
    let item = board.item(via).unwrap();
    assert_eq!(padstack_name(&board, via), "Via[0-1]_600:300_um");
    assert_eq!(item.clearance_class(), 1, "the default class");
    assert_eq!(
        item.as_via().unwrap().drill.center,
        IntPoint::new(10197, 329)
    );
    let gap = gap_to_other_nets_um(&board, via);
    assert!((gap - 232.2).abs() < 0.1, "gap {gap} um");
    // Freerouting's DRC accepts it: the via carries the default class.
    assert!(board.clearance_violations(via).is_empty());
    assert_eq!(DesignRulesChecker::new(&board).incomplete_count(), 0);

    let java = std::fs::read_to_string(repo_root().join(JAVA_SES)).expect("Java's session");
    let rust = write_ses(&board, "q14-hv-via", SesStyle::Java);
    assert_eq!(network_out(&rust), network_out(&java));

    // Java's session imported: the same via, the same gap.
    let prepared = prepare(&fixture_text(), &[]).expect("the fixture loads");
    let mut imported = prepared.board;
    read_session(&mut imported, &java).expect("Java's session imports");
    let java_vias = vias_of(&imported, hv);
    assert_eq!(java_vias.len(), 1);
    let java_gap = gap_to_other_nets_um(&imported, java_vias[0]);
    assert!(
        (java_gap - gap).abs() < 1e-9,
        "Java {java_gap} um, Rust {gap} um"
    );
}

/// Q14.2: with `net_class` HV's via has HV's clearance class and keeps
/// 0.25 mm from every other net, and the connection still routes.
#[test]
fn net_class_keeps_net_hv_s_via_at_the_class_clearance() {
    let board = route(NET_CLASS);
    let hv = net(&board, "HV");
    let hv_class = class_no(&board, "HV");
    let vias = vias_of(&board, hv);
    assert!(!vias.is_empty(), "HV still gets out of the cage");
    for via in vias {
        assert_eq!(board.item(via).unwrap().clearance_class(), hv_class);
        let gap = gap_to_other_nets_um(&board, via);
        assert!(gap >= 250.0, "{via:?}: gap {gap} um");
        assert!(board.clearance_violations(via).is_empty());
    }
    assert_eq!(DesignRulesChecker::new(&board).incomplete_count(), 0);
    assert!(board.all_clearance_violations().is_empty());
}

/// Q14.2: the setting changes nothing where every via info already has its
/// net's class: the default class's net SIG, and HV's own control outside
/// the fanout.
#[test]
fn net_class_keeps_the_via_info_class_where_it_is_the_net_class() {
    let prepared = prepare(&fixture_text(), NET_CLASS).expect("the fixture loads");
    let board = &prepared.board;
    assert_eq!(
        prepared.board_settings.via_clearance(),
        ViaClearanceMode::NetClass
    );
    let hv_control =
        AutorouteControl::for_net_default(board, net(board, "HV"), &prepared.board_settings)
            .unwrap();
    let hv_class = class_no(board, "HV");
    assert_eq!(hv_control.net_class_via_clearance_class, Some(hv_class));
    assert_eq!(hv_control.via_clearance_class, hv_class);
    let sig_control =
        AutorouteControl::for_net_default(board, net(board, "SIG"), &prepared.board_settings)
            .unwrap();
    assert_eq!(sig_control.net_class_via_clearance_class, Some(1));
    for (_, info) in board.rules.via_infos.iter() {
        if !info.name.ends_with("-HV") {
            assert_eq!(
                sig_control.via_info_clearance_class(info),
                info.clearance_class
            );
        }
        assert_eq!(hv_control.via_info_clearance_class(info), hv_class);
    }
    let default = prepare(&fixture_text(), &[]).expect("the fixture loads");
    let control = AutorouteControl::for_net_default(
        &default.board,
        net(board, "HV"),
        &default.board_settings,
    )
    .unwrap();
    assert_eq!(control.net_class_via_clearance_class, None);
}

/// Q14.2: a hand-placed HV via 0.2 mm from the corridor wall is a DRC
/// violation under both settings (the DSN reader gives a wiring via its
/// net class's via clearance class).
#[test]
fn drc_reports_a_hand_placed_hv_via_at_0_2_mm_under_both_settings() {
    // The corridor's north wall: centre line y = 660 um, half width 100, so
    // a 0.6 mm via at y = 60 um is 0.2 mm from it.
    let text = fixture_text().replace(
        "  (wiring\n",
        "  (wiring\n    (via \"Via[0-1]_600:300_um\"  2500 60 (net HV))\n",
    );
    for extra in [&[][..], NET_CLASS] {
        let prepared = prepare(&text, extra).expect("the fixture loads");
        let board = prepared.board;
        let hv = net(&board, "HV");
        let vias = vias_of(&board, hv);
        assert_eq!(vias.len(), 1);
        let via = vias[0];
        assert_eq!(
            board.item(via).unwrap().clearance_class(),
            class_no(&board, "HV")
        );
        let gap = gap_to_other_nets_um(&board, via);
        assert!((gap - 200.0).abs() < 1e-9, "gap {gap} um");
        let violations = board.clearance_violations(via);
        assert!(!violations.is_empty(), "{extra:?}");
        for v in &violations {
            assert_eq!(v.expected_clearance, 2500.0);
            assert!(v.actual_clearance < 2500.0);
        }
    }
}

/// Q14.2: the rules file keeps the class-named via infos the DSN reader
/// makes for HV, and HV's via rule.
#[test]
fn the_rules_file_round_trips_the_class_named_via_infos() {
    let prepared = prepare(&fixture_text(), NET_CLASS).expect("the fixture loads");
    let board = &prepared.board;
    let views = |b: &RoutingBoard| -> Vec<(String, String, String)> {
        b.rules
            .via_infos
            .iter()
            .map(|(_, info)| {
                (
                    info.name.clone(),
                    b.library.padstacks.get(info.padstack).unwrap().name.clone(),
                    b.rules
                        .clearance_matrix
                        .get_name(info.clearance_class)
                        .unwrap()
                        .to_string(),
                )
            })
            .collect()
    };
    let original = views(board);
    assert!(original.contains(&(
        "Via[0-1]_600:300_um-HV".to_string(),
        "Via[0-1]_600:300_um".to_string(),
        "HV".to_string()
    )));
    assert!(original.contains(&(
        "Via[0-1]_800:400_um-HV".to_string(),
        "Via[0-1]_800:400_um".to_string(),
        "HV".to_string()
    )));
    let text = rules_from_board(board, Some(&prepared.settings), "q14-hv-via").to_rules();
    let fresh = prepare(&fixture_text(), NET_CLASS).expect("the fixture loads");
    let (applied, result) = read_rules(fresh.board, &text, "q14-hv-via");
    result.expect("the written rules read");
    assert_eq!(views(&applied), original);
    let hv_class = applied
        .rules
        .net_classes
        .get_by_name("HV")
        .expect("class HV");
    let class = applied.rules.net_classes.get(hv_class);
    let rule = class.via_rule().expect("HV's via rule");
    let rule_vias: Vec<String> = applied
        .rules
        .via_rules
        .get(rule)
        .vias()
        .iter()
        .map(|v| applied.rules.via_infos.get(*v).name.clone())
        .collect();
    assert_eq!(rule_vias, ["Via[0-1]_800:400_um-HV"]);
    assert_eq!(
        class.default_item_clearance_classes.get(ItemClass::Via),
        class_no(&applied, "HV")
    );
}

/// The setting's values: unset and `via_info` are Java's rule, anything
/// else fails the job.
#[test]
fn unknown_via_clearance_values_fail_the_job() {
    assert_eq!(ViaClearanceMode::parse(None), Ok(ViaClearanceMode::ViaInfo));
    assert_eq!(
        ViaClearanceMode::parse(Some("via_info")),
        Ok(ViaClearanceMode::ViaInfo)
    );
    assert_eq!(
        ViaClearanceMode::parse(Some(" net_class ")),
        Ok(ViaClearanceMode::NetClass)
    );
    let Err(error) = prepare(&fixture_text(), &[("via_clearance", "pad")]) else {
        panic!("an unknown value fails");
    };
    assert!(
        error.to_string().contains("router.via_clearance"),
        "{error}"
    );
}

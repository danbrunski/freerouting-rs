//! Q22.5: `.kicad_dru` clearances between item types reach the board.
//!
//! Each test applies one rule to `crates/freerouting/tests/data/kicad/
//! q225-item-types.dsn` (two net classes: `kicad_default` at 150 µm, `Power`
//! at 250 µm with its own clearance class; a through-hole and an SMD
//! footprint; input vias of both classes) with
//! [`freerouting_kicad::dru::apply_to_pcb`], builds the board and measures
//! the clearance between the clearance classes the items really have: a
//! net class's wires, its routed vias (the via infos of its via rule), its
//! input vias, its through-hole pins and its SMD pads.

use std::collections::BTreeMap;
use std::path::PathBuf;

use freerouting::board::RoutingBoard;
use freerouting::dsn::ast::Pcb;
use freerouting::io::specctra::{BoardReadResult, build_board};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::settings::{ResolveContext, SettingsPatch};
use freerouting_kicad::dru::{Cond, DruReport, Side, Status, apply_to_pcb, parse_condition};

/// Board units per micrometre (`(resolution um 10)`).
const UNITS_PER_UM: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum K {
    Wire,
    RoutedVia,
    InputVia,
    Pin,
    Smd,
}

const ALL: [K; 5] = [K::Wire, K::RoutedVia, K::InputVia, K::Pin, K::Smd];
const CLASSES: [&str; 2] = ["default", "Power"];

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../freerouting/tests/data/kicad/q225-item-types.dsn")
}

fn tree() -> Pcb {
    let bytes = std::fs::read(fixture()).unwrap();
    Pcb::parse(&freerouting_dsn::decode(&bytes)).unwrap()
}

fn board_of(pcb: &Pcb) -> Box<RoutingBoard> {
    match build_board(pcb) {
        BoardReadResult::Success { board, .. } => board,
        _ => panic!("the fixture does not build"),
    }
}

fn applied(rules: &str) -> (Pcb, DruReport) {
    let mut pcb = tree();
    let report = apply_to_pcb(&mut pcb, rules).unwrap();
    (pcb, report)
}

/// The clearance classes items of net class `class` and kind `k` have.
fn classes_of(board: &RoutingBoard, class: &str, k: K) -> Vec<i32> {
    let rules = &board.rules;
    let id = rules.net_classes.get_by_name(class).expect("net class");
    let net_class = rules.net_classes.get(id);
    let of_class = |nets: &[i32]| {
        nets.first()
            .and_then(|n| rules.nets.get(*n))
            .is_some_and(|n| n.net_class() == id)
    };
    let mut out: Vec<i32> = match k {
        K::Wire => vec![net_class.trace_clearance_class()],
        K::RoutedVia => {
            let rule = net_class.via_rule().expect("via rule");
            rules
                .via_rules
                .get(rule)
                .vias()
                .iter()
                .map(|v| rules.via_infos.get(*v).clearance_class)
                .collect()
        }
        K::InputVia => board
            .items()
            .filter(|i| i.as_via().is_some() && of_class(i.net_numbers()))
            .map(|i| i.clearance_class())
            .collect(),
        K::Pin | K::Smd => board
            .items()
            .filter(|i| i.as_pin().is_some() && of_class(i.net_numbers()))
            .filter(|i| (i.first_layer() == i.last_layer()) == (k == K::Smd))
            .map(|i| i.clearance_class())
            .collect(),
    };
    out.sort();
    out.dedup();
    assert!(!out.is_empty(), "no {k:?} of {class}");
    out
}

/// (smallest, largest) clearance in µm between the two item sets.
fn clearance(board: &RoutingBoard, a: (&str, K), b: (&str, K)) -> (f64, f64) {
    let matrix = &board.rules.clearance_matrix;
    let mut values = Vec::new();
    for x in classes_of(board, a.0, a.1) {
        for y in classes_of(board, b.0, b.1) {
            for layer in 0..2 {
                values.push(f64::from(matrix.get_value(x, y, layer, false)) / UNITS_PER_UM);
                values.push(f64::from(matrix.get_value(y, x, layer, false)) / UNITS_PER_UM);
            }
        }
    }
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(0.0, f64::max);
    (min, max)
}

/// Every (class, kind) pair, measured.
/// A (net class, kind) pair.
type Key = ((&'static str, K), (&'static str, K));

fn all(board: &RoutingBoard) -> BTreeMap<Key, (f64, f64)> {
    let classes: Vec<&'static str> = CLASSES
        .into_iter()
        .filter(|c| board.rules.net_classes.get_by_name(c).is_some())
        .collect();
    let mut out = BTreeMap::new();
    for &ca in &classes {
        for ka in ALL {
            for &cb in &classes {
                for kb in ALL {
                    out.insert(((ca, ka), (cb, kb)), clearance(board, (ca, ka), (cb, kb)));
                }
            }
        }
    }
    out
}

fn is_via(k: K) -> bool {
    matches!(k, K::RoutedVia | K::InputVia)
}

/// Checks the board after `rules`: `asked(a, b)` pairs are at least
/// `value`; every other pair is never lowered and, when `exact`, unchanged.
fn check(rules: &str, value: f64, asked: impl Fn(K, K) -> bool, exact: bool) -> DruReport {
    check_on(tree(), rules, value, asked, exact)
}

fn check_on(
    tree: Pcb,
    rules: &str,
    value: f64,
    asked: impl Fn(K, K) -> bool,
    exact: bool,
) -> DruReport {
    let before = all(&board_of(&tree));
    let mut pcb = tree;
    let report = apply_to_pcb(&mut pcb, rules).unwrap();
    let after = all(&board_of(&pcb));
    assert_eq!(before.len(), after.len());
    let several = before.keys().any(|((c, _), _)| *c == "Power");
    for (key, (min, max)) in &after {
        let (old_min, old_max) = before[key];
        let ((_, ka), (_, kb)) = *key;
        if asked(ka, kb) || asked(kb, ka) {
            assert!(*min >= value, "{key:?}: {min} < {value}");
        } else {
            assert!(*min >= old_min, "{key:?} lowered: {min} < {old_min}");
            if exact && !(several && default_routed_via_across(*key)) {
                assert_eq!((*min, *max), (old_min, old_max), "{key:?} changed");
            }
        }
    }
    report
}

/// The one conservative widening of the module docs: with several classes,
/// a routed via of the default class keeps the structure's `via` class, which
/// the other vias and pads see at the via-to-track value.
fn default_routed_via_across(key: ((&str, K), (&str, K))) -> bool {
    let ((ca, ka), (cb, kb)) = key;
    let one = |c: &str, k: K, other: &str, ko: K| {
        c == "default"
            && k == K::RoutedVia
            && ko != K::Wire
            && !(other == "default" && ko == K::RoutedVia)
    };
    one(ca, ka, cb, kb) || one(cb, kb, ca, ka)
}

fn status(report: &DruReport) -> &Status {
    &report.rules[0].constraints[0].status
}

fn rule(name: &str, kind: &str, min: &str, condition: &str) -> String {
    format!(
        "(version 1)\n(rule \"{name}\"\n\t(constraint {kind} (min {min}))\n\t(condition \"{condition}\"))\n"
    )
}

/// The fixture as read: Power's own 250 µm, the default class's 150 µm and
/// KiCad's quarter `smd_smd` (37.5 µm, kept even in board units).
#[test]
fn the_fixture_has_two_clearance_classes() {
    let board = board_of(&tree());
    assert_eq!(
        clearance(&board, ("default", K::Wire), ("default", K::Wire)),
        (150.0, 150.0)
    );
    assert_eq!(
        clearance(&board, ("Power", K::Wire), ("Power", K::Wire)),
        (250.0, 250.0)
    );
    assert_eq!(
        clearance(&board, ("Power", K::Pin), ("default", K::Wire)),
        (250.0, 250.0)
    );
    assert_eq!(
        clearance(&board, ("default", K::Smd), ("default", K::Smd)),
        (37.6, 37.6)
    );
}

/// The condition terms parse.
#[test]
fn item_terms_parse() {
    assert_eq!(
        parse_condition("A.Type == 'Via'"),
        Cond::Type {
            side: Side::A,
            value: "via".into()
        }
    );
    assert_eq!(parse_condition("B.Type == A.Type"), Cond::SameType);
    assert_eq!(parse_condition("A.isPlated()"), Cond::Plated(Side::A));
    assert_eq!(
        parse_condition("A.Pad_Type == 'NPTH, mechanical'"),
        Cond::PadType {
            side: Side::A,
            value: "npth, mechanical".into()
        }
    );
    assert_eq!(
        parse_condition("A.Net != B.Net"),
        Cond::Not(Box::new(Cond::SameNet))
    );
    assert!(parse_condition("A.isPlated() && B.Type == 'track'").has_item_terms());
    assert!(!parse_condition("A.NetClass == 'Power'").has_item_terms());
}

/// `A.Type == 'via' && B.Type == 'track'`: via to wire, routed and input
/// vias of both classes; nothing else changes.
#[test]
fn type_via_and_track_sets_via_wire() {
    let report = check(
        &rule(
            "via",
            "clearance",
            "0.3mm",
            "A.Type == 'via' && B.Type == 'track'",
        ),
        300.0,
        |a, b| is_via(a) && b == K::Wire,
        true,
    );
    assert!(matches!(status(&report), Status::Applied { .. }));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

/// `A.Type == 'pad' && B.Type == 'track'`: through-hole and SMD pads to
/// wire.
#[test]
fn type_pad_sets_pin_and_smd_wire() {
    let report = check(
        &rule(
            "pad",
            "clearance",
            "0.3mm",
            "A.Type == 'pad' && B.Type == 'track'",
        ),
        300.0,
        |a, b| matches!(a, K::Pin | K::Smd) && b == K::Wire,
        false,
    );
    assert!(matches!(status(&report), Status::Applied { .. }));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

/// `A.isPlated() && B.Type == 'track'` (ch32v003's "Pad to Track
/// clearance"): vias and through-hole pins to wire, in both classes and
/// across them; Power's own 250 µm stays where the rule asks less.
#[test]
fn plated_sets_via_and_pin_wire_in_every_class() {
    let report = check(
        &rule(
            "plated",
            "clearance",
            "0.3mm",
            "A.isPlated() && B.Type == 'track'",
        ),
        300.0,
        |a, b| (is_via(a) || a == K::Pin) && b == K::Wire,
        false,
    );
    assert!(matches!(status(&report), Status::Applied { .. }));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // a value between the two classes: raised for the default class only
    let (pcb, _) = applied(&rule(
        "plated",
        "clearance",
        "0.2mm",
        "A.isPlated() && B.Type == 'track'",
    ));
    let board = board_of(&pcb);
    assert_eq!(
        clearance(&board, ("default", K::Pin), ("default", K::Wire)).0,
        200.0
    );
    assert_eq!(
        clearance(&board, ("Power", K::RoutedVia), ("Power", K::Wire)),
        (250.0, 250.0)
    );
    assert_eq!(
        clearance(&board, ("default", K::Smd), ("default", K::Wire)),
        (150.0, 150.0)
    );
}

/// The fixture with one class, as ch32v003's DSN: `VCC` in the default
/// class.
fn one_class_tree() -> Pcb {
    let mut pcb = tree();
    let network = pcb.network.as_mut().unwrap();
    network.classes.retain(|c| c.name != "Power");
    network.classes[0].nets.push("VCC".into());
    pcb
}

/// With one class (ch32v003's case), the plated rule raises exactly the
/// via and pin to wire pairs and nothing else.
#[test]
fn plated_is_exact_with_one_class() {
    let report = check_on(
        one_class_tree(),
        &rule(
            "plated",
            "clearance",
            "0.2mm",
            "A.isPlated() && B.Type == 'track'",
        ),
        200.0,
        |a, b| (is_via(a) || a == K::Pin) && b == K::Wire,
        true,
    );
    assert_eq!(status(&report), &Status::Applied { effects: 2 });
    assert!(report.warnings.is_empty());
}

/// `A.Pad_Type == 'SMD'`: the DSN's `smd` kind is also an edge connector's,
/// so the rule is applied to it with a warning naming the rule.
#[test]
fn pad_type_sets_smd_wire_with_a_warning() {
    let report = check(
        &rule(
            "smd",
            "clearance",
            "0.3mm",
            "A.Pad_Type == 'SMD' && B.Type == 'track'",
        ),
        300.0,
        |a, b| a == K::Smd && b == K::Wire,
        false,
    );
    assert!(matches!(status(&report), Status::Applied { .. }));
    assert_eq!(report.warnings.len(), 1);
    assert!(
        report.warnings[0].contains("'smd'"),
        "{:?}",
        report.warnings
    );
    let board = board_of(
        &applied(&rule(
            "smd",
            "clearance",
            "0.3mm",
            "A.Pad_Type == 'SMD' && B.Type == 'track'",
        ))
        .0,
    );
    assert_eq!(
        clearance(&board, ("default", K::Pin), ("default", K::Wire)),
        (150.0, 150.0)
    );
}

/// `A.Type == 'track' && B.Type == A.Type`: wire to wire in every class.
#[test]
fn same_type_sets_wire_wire() {
    let report = check(
        &rule(
            "tracks",
            "clearance",
            "0.3mm",
            "A.Type == 'track' && B.Type == A.Type",
        ),
        300.0,
        |a, b| a == K::Wire && b == K::Wire,
        false,
    );
    assert!(matches!(status(&report), Status::Applied { .. }));
}

/// A net-class term the typed matrix cannot keep apart: applied to every
/// class, with a warning naming the rule.
#[test]
fn net_class_term_is_applied_to_every_class_with_a_warning() {
    let report = check(
        &rule(
            "power pads",
            "clearance",
            "0.3mm",
            "A.NetClass == 'Power' && A.isPlated() && B.Type == 'track'",
        ),
        300.0,
        |a, b| (is_via(a) || a == K::Pin) && b == K::Wire,
        false,
    );
    assert!(matches!(status(&report), Status::Applied { .. }));
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("'power pads'"));
}

/// A rule that asks for no more than the DSN gives, one about pads only and
/// one about zones are reported and left out; the tree is unchanged.
#[test]
fn rules_that_change_nothing_are_left_out() {
    for (condition, why) in [
        ("A.isPlated() && B.Type == 'track'", "no more than the DSN"),
        (
            "A.Type == 'pad' && B.Type == A.Type && A.Net != B.Net",
            "only pad pairs",
        ),
        (
            "A.Type == 'zone' && B.Type == 'track'",
            "matches no item pair",
        ),
        ("A.Type == 'via' && A.Net == B.Net", "matches no item pair"),
    ] {
        let (pcb, report) = applied(&rule("x", "clearance", "0.1mm", condition));
        match status(&report) {
            Status::Ignored(w) => assert!(w.contains(why), "{condition}: {w}"),
            s => panic!("{condition}: {s:?}"),
        }
        assert_eq!(pcb, tree(), "{condition}");
    }
}

/// An NPTH `hole_clearance`, and one against pad copper, become a floor of
/// `router.hole_clearance_um`, with a warning; the tree is unchanged. (A
/// plated hole against tracks becomes typed clearances, Q22.15:
/// `tests/dru_holes.rs`.)
#[test]
fn hole_clearance_is_a_floor() {
    let text = format!(
        "{}{}",
        rule(
            "NPTH to Track",
            "hole_clearance",
            "0.33mm",
            "A.Pad_Type == 'NPTH, mechanical' && B.Type == 'track'"
        ),
        "(rule \"via\" (constraint hole_clearance (min 0.254mm)) (condition \"A.Type == 'via' && B.Type == 'pad'\"))\n"
    );
    let (pcb, report) = applied(&text);
    let hole = report.hole_clearance.expect("a floor");
    assert_eq!(hole.nm, 330_000);
    assert_eq!(hole.rules, ["NPTH to Track", "via"]);
    assert_eq!(report.warnings.len(), 2);
    assert_eq!(pcb, tree());
}

fn prepared(dru: Option<&str>, cli_hole_um: Option<f64>) -> (f64, Vec<String>) {
    let bytes = std::fs::read(fixture()).unwrap();
    let mut input = JobInput::new(bytes, "q225-item-types");
    input.rules = RulesChoice::None;
    input.kicad_dru = dru.map(|t| (PathBuf::from("q225.kicad_dru"), t.to_owned()));
    if let Some(um) = cli_hole_um {
        input.cli = Some(SettingsPatch {
            hole_clearance_um: Some(um),
            ..SettingsPatch::default()
        });
    }
    let job = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
    let log = job.log.iter().map(|l| l.text.clone()).collect();
    (job.board.rules.hole_clearance() as f64 / UNITS_PER_UM, log)
}

/// The job applies `--kicad-dru`: the hole floor wins over a smaller
/// command-line value and never lowers a larger one; without a file the
/// job is unchanged.
#[test]
fn the_job_applies_the_rules_and_the_hole_floor() {
    let text = rule(
        "NPTH to Track",
        "hole_clearance",
        "0.33mm",
        "A.Pad_Type == 'NPTH, mechanical' && B.Type == 'track'",
    );
    let (none, _) = prepared(None, Some(250.0));
    assert_eq!(none, 250.0);
    let (raised, log) = prepared(Some(&text), Some(250.0));
    assert_eq!(raised, 330.0);
    assert!(
        log.iter().any(|l| l.contains("raised from 250 to 330")),
        "{log:?}"
    );
    assert!(log.iter().any(|l| l.contains("KiCad design rules file")));
    let (kept, _) = prepared(Some(&text), Some(400.0));
    assert_eq!(kept, 400.0);
}

/// ch32v003-usb-devboard's clearance and hole rules (Q22.1's pair), as the
/// report gives them.
#[test]
fn ch32v003_rules_report() {
    let text = r#"(version 1)
(rule "Minimum Trace Spacing (outer layer)"
	(constraint clearance (min 5mil))
	(layer outer)
	(condition "A.Type == 'track' && B.Type == A.Type"))
(rule "Minimum Trace Spacing (inner layer)"
	(constraint clearance (min 3.5mil))
	(layer inner)
	(condition "A.Type == 'track' && B.Type == A.Type"))
(rule "via to track clearance"
	(constraint hole_clearance (min 0.254mm))
	(condition "A.Type == 'via' && B.Type == 'track'"))
(rule "pad to pad clearance (without hole, different nets)"
	(constraint clearance (min 0.127mm))
	(condition "A.Type == 'pad' && B.Type == A.Type && A.Net != B.Net"))
(rule "NPTH to Track clearance)"
	(constraint hole_clearance (min 0.254mm))
	(condition "A.Pad_Type == 'NPTH, mechanical' && B.Type == 'track'"))
(rule "PTH to Track clearance)"
	(constraint hole_clearance (min 0.33mm))
	(condition "A.isPlated() && B.Type == 'track'"))
(rule "Pad to Track clearance)"
	(constraint clearance (min 0.2mm))
	(condition "A.isPlated() && B.Type == 'track'"))
"#;
    let (_, report) = applied(text);
    let states: Vec<String> = report
        .rules
        .iter()
        .map(|r| match &r.constraints[0].status {
            Status::Applied { .. } => "applied".to_owned(),
            Status::Ignored(w) => format!("ignored: {w}"),
            Status::Unsupported(w) => format!("unsupported: {w}"),
        })
        .collect();
    assert_eq!(
        states,
        [
            "ignored: asks for no more than the DSN gives",
            "ignored: its layers are not copper layers of the board",
            // Q22.15: 0.254 mm less the via's 150 µm ring is below 150 µm
            "ignored: asks for no more than the DSN gives",
            "ignored: covers only pad pairs, which routing never moves",
            "applied",
            "applied",
            "applied",
        ]
    );
    // Q22.15: the PTH rule became via_wire and pin_wire; the NPTH rule is
    // the floor
    assert_eq!(report.hole_clearance.unwrap().nm, 254_000);
}

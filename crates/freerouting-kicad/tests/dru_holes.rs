//! Q22.14: `.kicad_dru` `hole_to_hole` rules raise
//! `router.board_minimums.hole_to_hole_um`. Q22.15: a `hole_clearance` rule
//! between plated holes and tracks becomes typed copper clearances.
//!
//! The DSN tests use `crates/freerouting/tests/data/kicad/q225-item-types.dsn`
//! (Q22.5's fixture, `(resolution um 10)`); the `.kicad_pcb` tests use
//! `fixtures/kicad/synthetic/classes.kicad_pcb`, whose `.kicad_pro` has no
//! `min_hole_to_hole` (KiCad's 0.25 mm).

mod common;

use std::path::{Path, PathBuf};

use freerouting::dsn::ast::{Pcb, Rule};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::settings::{ResolveContext, SettingsPatch};
use freerouting_kicad::dru::{DruReport, Status, apply_to_pcb};
use freerouting_kicad::{ReadOptions, read_with_rules};

/// Board units per micrometre in the DSN fixture.
const UNITS_PER_UM: i32 = 10;

const DIFFERENT_NETS: &str = "hole to hole clearance (different nets)";

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../freerouting/tests/data/kicad/q225-item-types.dsn")
}

fn tree() -> Pcb {
    let bytes = std::fs::read(fixture()).unwrap();
    Pcb::parse(&freerouting_dsn::decode(&bytes)).unwrap()
}

fn applied(rules: &str) -> (Pcb, DruReport) {
    let mut pcb = tree();
    let report = apply_to_pcb(&mut pcb, rules).unwrap();
    (pcb, report)
}

fn rule(name: &str, kind: &str, min: &str, condition: &str) -> String {
    format!("(rule \"{name}\"\n\t(constraint {kind} (min {min}))\n\t(condition \"{condition}\"))\n")
}

fn file(rules: &[String]) -> String {
    format!("(version 1)\n{}", rules.concat())
}

fn different_nets(min: &str) -> String {
    rule(DIFFERENT_NETS, "hole_to_hole", min, "A.Net != B.Net")
}

/// ch32v003-usb-devboard's three `hole_to_hole` rules.
fn ch32v003_hole_rules() -> String {
    file(&[
        different_nets("0.5mm"),
        rule(
            "via to via clearance (same nets)",
            "hole_to_hole",
            "0.254mm",
            "A.Type == 'via' && B.Type == A.Type && A.Net == B.Net",
        ),
        rule(
            "pad to pad clearance (with hole, different nets)",
            "hole_to_hole",
            "0.5mm",
            "A.Type == 'pad' && B.Type == A.Type && A.Net != B.Net",
        ),
    ])
}

fn states(report: &DruReport) -> Vec<String> {
    report
        .rules
        .iter()
        .map(|r| match &r.constraints[0].status {
            Status::Applied { effects } => format!("applied {effects}"),
            Status::Ignored(w) => format!("ignored: {w}"),
            Status::Unsupported(w) => format!("unsupported: {w}"),
        })
        .collect()
}

// ---- Q22.14: the reader -------------------------------------------------------

/// A 0.5 mm `hole_to_hole` rule is a floor of 500 µm, named, with a warning
/// (it also holds between holes of one net); the tree is unchanged.
#[test]
fn a_hole_to_hole_rule_is_a_floor() {
    let (pcb, report) = applied(&file(&[different_nets("0.5mm")]));
    let floor = report.hole_to_hole.as_ref().expect("a floor");
    assert_eq!(floor.nm, 500_000);
    assert_eq!(floor.rules, [DIFFERENT_NETS]);
    assert_eq!(states(&report), ["applied 1"]);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(report.warnings[0].contains(DIFFERENT_NETS));
    assert!(report.warnings[0].contains("router.board_minimums.hole_to_hole_um"));
    assert!(report.hole_clearance.is_none());
    assert_eq!(pcb, tree());
}

/// ch32v003's rules: the largest that can apply wins; a same-net rule can
/// apply (two vias of one net); a rule only between pads is left out.
#[test]
fn the_largest_rule_that_can_apply_wins() {
    let (_, report) = applied(&ch32v003_hole_rules());
    assert_eq!(
        states(&report),
        [
            "applied 1",
            "applied 1",
            "ignored: covers only pad pairs, which routing never moves",
        ]
    );
    let floor = report.hole_to_hole.expect("a floor");
    assert_eq!(floor.nm, 500_000);
    assert_eq!(
        floor.rules,
        [DIFFERENT_NETS, "via to via clearance (same nets)"]
    );
}

/// A rule that names no hole is left out.
#[test]
fn a_rule_without_holes_is_left_out() {
    let (_, report) = applied(&file(&[rule(
        "tracks",
        "hole_to_hole",
        "0.5mm",
        "A.Type == 'track'",
    )]));
    assert_eq!(states(&report), ["ignored: matches no pair of holes"]);
    assert!(report.hole_to_hole.is_none());
}

fn prepared(dru: Option<&str>, board_setup_um: Option<f64>) -> (i32, Vec<String>) {
    let bytes = std::fs::read(fixture()).unwrap();
    let mut input = JobInput::new(bytes, "q225-item-types");
    input.rules = RulesChoice::None;
    input.kicad_dru = dru.map(|t| (PathBuf::from("q2214.kicad_dru"), t.to_owned()));
    let mut cli = SettingsPatch::default();
    cli.board_minimums.hole_to_hole_um = board_setup_um;
    input.cli = Some(cli);
    let job = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
    let log = job.log.iter().map(|l| l.text.clone()).collect();
    (job.board.rules.hole_to_hole(), log)
}

/// `--kicad-dru` on a DSN: the rule's 500 µm wins over Board Setup's 250
/// (the plugin's `--router.board_minimums.hole_to_hole_um`), and the log
/// names the rule; a smaller rule leaves Board Setup's value; a larger
/// Board Setup value is kept; without a file nothing changes.
#[test]
fn the_job_raises_board_setups_hole_to_hole() {
    let rules = ch32v003_hole_rules();
    let (none, log) = prepared(None, Some(250.0));
    assert_eq!(none, 250 * UNITS_PER_UM);
    assert!(!log.iter().any(|l| l.contains(".kicad_dru")), "{log:?}");

    let (raised, log) = prepared(Some(&rules), Some(250.0));
    assert_eq!(raised, 500 * UNITS_PER_UM);
    let line = log
        .iter()
        .find(|l| l.contains("router.board_minimums.hole_to_hole_um raised from 250 to 500"))
        .unwrap_or_else(|| panic!("{log:?}"));
    assert!(line.contains(DIFFERENT_NETS), "{line}");
    assert!(
        log.iter()
            .any(|l| l.contains("Board minimum hole to hole 500"))
    );

    let (kept, log) = prepared(Some(&file(&[different_nets("0.2mm")])), Some(250.0));
    assert_eq!(kept, 250 * UNITS_PER_UM);
    assert!(
        log.iter()
            .any(|l| l.contains("router.board_minimums.hole_to_hole_um is 250, no less")),
        "{log:?}"
    );

    let (larger, _) = prepared(Some(&rules), Some(600.0));
    assert_eq!(larger, 600 * UNITS_PER_UM);

    // nothing from Board Setup: the rule alone
    let (alone, _) = prepared(Some(&rules), None);
    assert_eq!(alone, 500 * UNITS_PER_UM);
}

fn classes_board() -> (String, String) {
    let dir = common::fixtures().join("synthetic");
    (
        std::fs::read_to_string(dir.join("classes.kicad_pcb")).unwrap(),
        std::fs::read_to_string(dir.join("classes.kicad_pro")).unwrap(),
    )
}

/// The `.kicad_pcb` reader gives the same floor with
/// `apply_design_rules`, and reports the rule as ignored without it.
#[test]
fn the_kicad_pcb_reader_reads_the_hole_to_hole_rules() {
    let (board, pro) = classes_board();
    let read = |apply: bool| {
        let options = ReadOptions {
            apply_design_rules: apply,
            ..ReadOptions::default()
        };
        let dru = ch32v003_hole_rules();
        read_with_rules("classes.dsn", &board, Some(&pro), Some(&dru), options).unwrap()
    };
    let on = read(true);
    assert_eq!(on.rules.hole_to_hole.as_ref().map(|f| f.nm), Some(500_000));
    let off = read(false);
    assert!(off.rules.hole_to_hole.is_none());
    assert!(
        states(&off.rules)[0].contains("apply_design_rules is off"),
        "{:?}",
        states(&off.rules)
    );
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A `.kicad_pcb` job: Board Setup's 0.25 mm (the `.kicad_pro` default) and
/// the `.kicad_dru`'s 0.5 mm compose, the larger wins.
#[test]
fn a_kicad_pcb_job_takes_the_larger_of_board_setup_and_the_rules() {
    let (board, pro) = classes_board();
    let dir = scratch("q2214-kicad-pcb");
    std::fs::write(dir.join("classes.kicad_pcb"), board).unwrap();
    std::fs::write(dir.join("classes.kicad_pro"), pro).unwrap();
    std::fs::write(dir.join("classes.kicad_dru"), ch32v003_hole_rules()).unwrap();
    let options = ReadOptions {
        apply_design_rules: true,
        ..ReadOptions::default()
    };
    let read =
        freerouting::io::kicad::read_kicad_input(&dir.join("classes.kicad_pcb"), options).unwrap();
    let mut input = JobInput::new(Vec::new(), "classes");
    input.rules = RulesChoice::None;
    input.set_kicad(read);
    let job = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
    let log: Vec<&str> = job.log.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(job.settings.board_minimums.hole_to_hole_um, Some(500.0));
    assert!(
        log.iter().any(|l| l
            .contains("router.board_minimums.hole_to_hole_um raised from 250 to 500")
            && l.contains(DIFFERENT_NETS)),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|l| l.contains("Board minimum hole to hole 500"))
    );
}

// ---- Q22.15: plated holes against tracks --------------------------------------

/// The typed clearance of kind pair `ty` in the structure (the default
/// class's), in µm.
fn structure_typed(pcb: &Pcb, ty: &str) -> Option<f64> {
    pcb.structure.as_ref()?.rules.iter().find_map(|r| match r {
        Rule::Clearance { value, types } if types.iter().any(|t| t == ty) => Some(*value),
        _ => None,
    })
}

fn pth_to_track(min: &str) -> String {
    rule(
        "PTH to Track clearance)",
        "hole_clearance",
        min,
        "A.isPlated() && B.Type == 'track'",
    )
}

/// ch32v003's `'PTH to Track clearance)'`: a via hole keeps 0.33 mm from
/// tracks as via copper 0.33 mm less the via's annular ring (the fixture's
/// `Via[0-1]_600:300_um`, 150 µm) from tracks; a plated pad as its copper
/// 0.33 mm from tracks (the DSN gives no pad drill), with a warning. No
/// floor of `router.hole_clearance_um`.
#[test]
fn a_plated_hole_rule_becomes_via_and_pin_to_track_clearances() {
    let (pcb, report) = applied(&file(&[pth_to_track("0.33mm")]));
    assert!(
        report.hole_clearance.is_none(),
        "{:?}",
        report.hole_clearance
    );
    assert_eq!(structure_typed(&pcb, "via_wire"), Some(180.0));
    assert_eq!(structure_typed(&pcb, "pin_wire"), Some(330.0));
    assert!(matches!(
        report.rules[0].constraints[0].status,
        Status::Applied { .. }
    ));
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(report.warnings[0].contains("PTH to Track clearance)"));
    assert!(report.warnings[0].contains("pad drill"));
}

/// A via-only rule uses the via's annular ring and warns about nothing; one
/// that asks no more than the DSN's via-to-track clearance is left out.
#[test]
fn a_via_hole_rule_uses_the_via_annular_ring() {
    let via = |min: &str| {
        rule(
            "via to track clearance",
            "hole_clearance",
            min,
            "A.Type == 'via' && B.Type == 'track'",
        )
    };
    let (pcb, report) = applied(&file(&[via("0.4mm")]));
    assert!(report.hole_clearance.is_none());
    assert_eq!(structure_typed(&pcb, "via_wire"), Some(250.0));
    assert_eq!(structure_typed(&pcb, "pin_wire"), None);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // 254 µm less 150 is no more than the default class's 150 µm
    let (pcb, report) = applied(&file(&[via("0.254mm")]));
    assert_eq!(
        states(&report),
        ["ignored: asks for no more than the DSN gives"]
    );
    assert!(report.hole_clearance.is_none());
    assert_eq!(pcb, tree());
}

/// An NPTH hole and a rule against pad copper stay floors of
/// `router.hole_clearance_um` (the NPTH hole is a keepout; a hole's distance
/// to other copper has no typed form).
#[test]
fn npth_and_pad_copper_rules_stay_floors() {
    let npth = rule(
        "NPTH to Track clearance)",
        "hole_clearance",
        "0.254mm",
        "A.Pad_Type == 'NPTH, mechanical' && B.Type == 'track'",
    );
    let (pcb, report) = applied(&file(std::slice::from_ref(&npth)));
    assert_eq!(report.hole_clearance.as_ref().map(|h| h.nm), Some(254_000));
    assert_eq!(pcb, tree());
    let pads = rule(
        "via hole to pads",
        "hole_clearance",
        "0.3mm",
        "A.Type == 'via' && B.Type == 'pad'",
    );
    let (_, report) = applied(&file(&[pads]));
    assert_eq!(report.hole_clearance.map(|h| h.nm), Some(300_000));
}

/// Without a via drill in any via padstack name the via part cannot be
/// converted: the rule stays a floor.
#[test]
fn a_via_without_a_known_drill_keeps_the_floor() {
    let mut pcb = tree();
    let rename = |n: &mut String| {
        if n.starts_with("Via[") {
            *n = "MyVia".to_owned();
        }
    };
    let structure = pcb.structure.as_mut().unwrap();
    for n in &mut structure.via.as_mut().unwrap().padstacks {
        rename(n);
    }
    for p in &mut pcb.library.as_mut().unwrap().padstacks {
        rename(&mut p.name);
    }
    let report = apply_to_pcb(&mut pcb, &file(&[pth_to_track("0.33mm")])).unwrap();
    assert_eq!(report.hole_clearance.map(|h| h.nm), Some(330_000));
}

/// The job with ch32v003's PTH rule keeps Board Setup's hole clearance (the
/// rule became copper clearances).
#[test]
fn the_job_keeps_board_setups_hole_clearance_for_a_plated_rule() {
    let bytes = std::fs::read(fixture()).unwrap();
    let mut input = JobInput::new(bytes, "q225-item-types");
    input.rules = RulesChoice::None;
    input.kicad_dru = Some((
        PathBuf::from("q2215.kicad_dru"),
        file(&[pth_to_track("0.33mm")]),
    ));
    input.cli = Some(SettingsPatch {
        hole_clearance_um: Some(250.0),
        ..SettingsPatch::default()
    });
    let job = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
    assert_eq!(job.board.rules.hole_clearance(), 250 * UNITS_PER_UM);
    let matrix = &job.board.rules.clearance_matrix;
    let (via, wire) = (matrix.get_no("via"), 1);
    assert!(via > 0);
    assert_eq!(matrix.get_value(via, wire, 0, false), 180 * UNITS_PER_UM);
}

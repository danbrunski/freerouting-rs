//! K07.5: `.kicad_dru` rules are read and reported; with
//! `ReadOptions::apply_design_rules` the expressible ones change the tree.

mod common;

use freerouting_dsn::ast::{ClassClass, Pcb, Rule};
use freerouting_kicad::dru::{Cond, DruFile, Status, parse_condition};
use freerouting_kicad::{KicadRead, ReadOptions, read_with, read_with_rules};

const CANARY_EXCERPT: &str = r#"(version 1)
(rule "Q2 land pattern"
	(constraint clearance (min 0.13mm))
	(condition "A.insideCourtyard('Q2') && B.insideCourtyard('Q2')"))
(rule "U8 land pattern"
	(constraint clearance (min 0.13mm))
	(condition "A.insideCourtyard('U8') && B.insideCourtyard('U8')"))
"#;

fn classes_board() -> (String, Option<String>, String) {
    let dir = common::fixtures().join("synthetic");
    (
        std::fs::read_to_string(dir.join("classes.kicad_pcb")).unwrap(),
        std::fs::read_to_string(dir.join("classes.kicad_pro")).ok(),
        std::fs::read_to_string(dir.join("rules.kicad_dru")).unwrap(),
    )
}

fn read_classes(dru: Option<&str>, apply: bool) -> KicadRead {
    let (board, pro, _) = classes_board();
    let options = ReadOptions {
        apply_design_rules: apply,
        ..ReadOptions::default()
    };
    read_with_rules("classes.dsn", &board, pro.as_deref(), dru, options).unwrap()
}

fn class_width(pcb: &Pcb, name: &str) -> f64 {
    let class = pcb
        .network
        .as_ref()
        .unwrap()
        .classes
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no class {name}"));
    class
        .rules
        .iter()
        .find_map(|r| match r {
            Rule::Width(w) => Some(*w),
            _ => None,
        })
        .unwrap()
}

/// canary-pwr's ten `insideCourtyard` rules are all UNSUPPORTED and named.
#[test]
fn canary_pwr_rules_are_unsupported_by_name() {
    let (board, pro, _) = classes_board();
    let read = read_with_rules(
        "x.dsn",
        &board,
        pro.as_deref(),
        Some(CANARY_EXCERPT),
        ReadOptions {
            apply_design_rules: true,
            ..ReadOptions::default()
        },
    )
    .unwrap();
    let names: Vec<&str> = read.rules.unsupported().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Q2 land pattern", "U8 land pattern"]);
    for r in &read.rules.rules {
        assert!(
            r.unsupported_reason().unwrap().contains("insideCourtyard"),
            "{r:?}"
        );
    }
    // the real file, when the corpus is there: all ten
    if let Some(dir) = common::corpus_dir() {
        let path = dir.join("canary-pwr/canary-pwr.kicad_dru");
        if let Ok(text) = std::fs::read_to_string(path) {
            let file = DruFile::parse(&text).unwrap();
            assert_eq!(file.rules.len(), 10);
            let read = read_with_rules(
                "x.dsn",
                &board,
                pro.as_deref(),
                Some(&text),
                ReadOptions::default(),
            )
            .unwrap();
            assert_eq!(read.rules.unsupported().count(), 10);
            assert!(
                read.rules
                    .unsupported()
                    .all(|r| r.unsupported_reason().unwrap().contains("insideCourtyard"))
            );
        }
    }
    // nothing was applied
    assert!(read.pcb.network.unwrap().class_classes.is_empty());
}

/// With the option on, the clearance matrix and the class width change exactly
/// as the two supported rules say; the third is reported.
#[test]
fn supported_rules_change_the_tree_exactly() {
    let (_, _, dru) = classes_board();
    let base = read_classes(None, false);
    let on = read_classes(Some(&dru), true);
    let (b, o) = (
        base.pcb.network.as_ref().unwrap(),
        on.pcb.network.as_ref().unwrap(),
    );

    // exactly one class pair: Fast with "Wide,Default", 0.5 mm = 500 um
    assert_eq!(
        o.class_classes,
        vec![ClassClass {
            classes: vec!["Fast".to_owned(), "Wide,Default".to_owned()],
            rules: vec![Rule::Clearance {
                value: 500.0,
                types: Vec::new()
            }],
            layer_rules: Vec::new(),
        }]
    );
    // the Fast class width, and only it
    assert_eq!(class_width(&base.pcb, "Fast"), 100.0);
    assert_eq!(class_width(&on.pcb, "Fast"), 300.0);
    for name in ["kicad_default", "Fast,Wide", "Wide,Default"] {
        assert_eq!(
            class_width(&base.pcb, name),
            class_width(&on.pcb, name),
            "{name}"
        );
    }
    // everything else equals the board without the file
    let mut stripped = on.pcb.clone();
    let sn = stripped.network.as_mut().unwrap();
    sn.class_classes.clear();
    for c in &mut sn.classes {
        if c.name == "Fast" {
            c.rules = b
                .classes
                .iter()
                .find(|x| x.name == "Fast")
                .unwrap()
                .rules
                .clone();
        }
    }
    assert_eq!(stripped, base.pcb);

    // the report: two applied, the third UNSUPPORTED naming its term
    let statuses: Vec<(&str, &Status)> = on
        .rules
        .rules
        .iter()
        .map(|r| (r.name.as_str(), &r.constraints[0].status))
        .collect();
    assert_eq!(statuses[0].1, &Status::Applied { effects: 1 });
    assert_eq!(statuses[1].1, &Status::Applied { effects: 1 });
    match statuses[2].1 {
        Status::Unsupported(why) => assert!(why.contains("intersectsArea"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(on.rules.lines().len(), 3);
}

/// With the option off, the tree equals the board without a rules file, and
/// the same three rules are reported (two ignored, one unsupported).
#[test]
fn option_off_leaves_the_tree_alone() {
    let (_, _, dru) = classes_board();
    let base = read_classes(None, false);
    let off = read_classes(Some(&dru), false);
    assert_eq!(off.pcb, base.pcb);
    assert_eq!(off.warnings, base.warnings);
    assert!(!base.rules.present);
    assert!(off.rules.present);
    let kinds: Vec<_> = off
        .rules
        .rules
        .iter()
        .map(|r| match &r.constraints[0].status {
            Status::Ignored(_) => "ignored",
            Status::Unsupported(_) => "unsupported",
            Status::Applied { .. } => "applied",
        })
        .collect();
    assert_eq!(kinds, ["ignored", "ignored", "unsupported"]);
    // read_with is read_with_rules without a file
    let (board, pro, _) = classes_board();
    let plain = read_with(
        "classes.dsn",
        &board,
        pro.as_deref(),
        ReadOptions::default(),
    )
    .unwrap();
    assert_eq!(plain.pcb, base.pcb);
}

/// A file that does not parse is a warning, not a failed read.
#[test]
fn a_broken_rules_file_is_a_warning() {
    let r = read_classes(Some("(rule \"x\" (constraint"), true);
    assert!(
        r.warnings
            .iter()
            .any(|w| w.message.contains(".kicad_dru file could not be read"))
    );
    assert!(!r.rules.present || r.rules.rules.is_empty());
}

#[test]
fn conditions_parse_into_trees() {
    use freerouting_kicad::dru::Side;
    let nc = |side, class: &str, exact| Cond::NetClass {
        side,
        class: class.to_owned(),
        exact,
    };
    assert_eq!(
        parse_condition("A.NetClass == 'x' && B.hasNetclass('y')"),
        Cond::And(vec![nc(Side::A, "x", true), nc(Side::B, "y", false)])
    );
    assert_eq!(
        parse_condition("!(A.NetClass == 'x' || A.hasExactNetclass('y'))"),
        Cond::Not(Box::new(Cond::Or(vec![
            nc(Side::A, "x", true),
            nc(Side::A, "y", true)
        ])))
    );
    assert_eq!(
        parse_condition("A.NetClass != 'x'"),
        Cond::Not(Box::new(nc(Side::A, "x", true)))
    );
    assert_eq!(
        parse_condition("onLayer('F.Cu') && A.NetClass == 'p'"),
        Cond::And(vec![Cond::Layer("F.Cu".to_owned()), nc(Side::A, "p", true)])
    );
    // the first unsupported term is named as written
    let c = parse_condition("A.NetClass == 'x' && A.inDiffPair('*') || B.Type == 'via'");
    assert_eq!(c.first_unsupported(), Some("A.inDiffPair ( '*' )"));
    assert_eq!(
        parse_condition("A.insideCourtyard('Q2')").first_unsupported(),
        Some("A.insideCourtyard ( 'Q2' )")
    );
}

/// `||`, `!`, `hasNetclass`, layers and units choose class pairs as written.
#[test]
fn conditions_choose_class_pairs() {
    let board = classes_board();
    let run = |dru: &str| -> Vec<(Vec<String>, bool)> {
        let r = read_with_rules(
            "c.dsn",
            &board.0,
            board.1.as_deref(),
            Some(dru),
            ReadOptions {
                apply_design_rules: true,
                ..ReadOptions::default()
            },
        )
        .unwrap();
        r.pcb
            .network
            .unwrap()
            .class_classes
            .into_iter()
            .map(|c| (c.classes, !c.layer_rules.is_empty()))
            .collect()
    };
    // hasNetclass('Wide') is true for both aggregates holding Wide: pairs
    // (Fast,Wide / Fast,Wide), (Fast,Wide / Wide,Default), (Wide,Default / itself)
    let got = run("(version 1)(rule \"w\" (constraint clearance (min 0.4in)) \
         (condition \"A.hasNetclass('Wide') && B.hasNetclass('Wide')\"))");
    assert_eq!(got.len(), 3, "{got:?}");
    // a layer clause makes layer rules
    let got = run(
        "(version 1)(rule \"l\" (layer \"F.Cu\") (constraint clearance (min 0.4mm)) \
         (condition \"A.NetClass == 'Fast' && B.NetClass == 'Fast'\"))",
    );
    assert_eq!(
        got,
        vec![(vec!["Fast".to_owned(), "Fast".to_owned()], true)]
    );
    // an unknown layer, a constraint kind with no form and a hole_size for one
    // class (K15.3: one board-wide value) are unsupported; a rule without a
    // condition is not (a floor, ignored here because the options leave the
    // rules off)
    let r = read_with_rules(
        "c.dsn",
        &board.0,
        board.1.as_deref(),
        Some(
            "(version 1)\n(rule \"bare\" (constraint clearance (min 1mm)))\n\
             (rule \"nolayer\" (layer \"In9.Cu\") (constraint clearance (min 1mm)) (condition \"A.NetClass == 'Fast'\"))\n\
             (rule \"hole\" (constraint hole_size (min 0.3mm)) (condition \"A.NetClass == 'Fast'\"))\n\
             (rule \"len\" (constraint length (max 50mm)) (condition \"A.NetClass == 'Fast'\"))\n",
        ),
        ReadOptions::default(),
    )
    .unwrap();
    let unsupported: Vec<&str> = r.rules.unsupported().map(|r| r.name.as_str()).collect();
    assert_eq!(unsupported, ["nolayer", "hole", "len"]);
}

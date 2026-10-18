//! N06.10 input corrections: synthetic DSNs exercise branches absent from ordinary exports.
use freerouting::board::RoutingBoard;
use freerouting::io::specctra::{BoardReadResult, read_board_from_str_with};
use freerouting::settings::JavaQuirks;

const BASE: &str = include_str!("../../../fixtures/scoring-perfect-two-pin.dsn");
const CLASS: &str = "(class kicad_default PERFECT\n    )";

fn read(text: &str, flag: &str, java: bool) -> RoutingBoard {
    match read_board_from_str_with(text, JavaQuirks::JAVA.with(flag, java).unwrap()) {
        BoardReadResult::Success { board, .. } => *board,
        other => panic!("synthetic DSN: {other:?}"),
    }
}

#[test]
fn via_alias_switch_keeps_first_class_vias_separate() {
    let text = BASE.replace("(library", "(library (padstack V1 (shape (circle F.Cu 400))) (padstack V2 (shape (circle F.Cu 600)))")
        .replace(CLASS, "(class A PERFECT (circuit (use_via V1))) (class B (circuit (use_via V2)))");
    for java in [true, false] {
        let board = read(&text, "dsn_via_alias", java);
        let class = board.rules.net_classes.get_by_name("A").unwrap();
        let rule = board.rules.net_classes.get(class).via_rule().unwrap();
        let names: Vec<_> = board
            .rules
            .via_rules
            .get(rule)
            .vias()
            .iter()
            .map(|&id| board.rules.via_infos.get(id).name.as_str())
            .collect();
        assert_eq!(names.len(), if java { 2 } else { 1 }, "{names:?}");
        assert!(names[0].contains("V1"));
    }
}

#[test]
fn class_class_switch_applies_all_three_pairs() {
    let text = BASE.replace(CLASS, "(class A PERFECT) (class B) (class C) (class_class (classes A B C) (rule (clearance 777)))");
    for java in [true, false] {
        let board = read(&text, "dsn_class_class_pairs", java);
        let matrix = &board.rules.clearance_matrix;
        let a = matrix.get_no("A");
        let b = matrix.get_no("B");
        let c = matrix.get_no("C");
        assert!(a > 0 && b > 0 && c > 0);
        let expected = board.communication.coordinate_transform.dsn_to_board(777.0) as i32;
        assert_eq!(matrix.get_value(a, b, 0, false), expected);
        assert_eq!(matrix.get_value(a, c, 0, false), expected);
        assert_eq!(
            matrix.get_value(b, c, 0, false),
            if java { 2000 } else { expected }
        );
        assert_eq!(
            matrix.get_value(c, b, 0, false),
            matrix.get_value(b, c, 0, false)
        );
    }
}

#[test]
fn pcb_unit_and_placement_resolution_switch_use_physical_units() {
    use freerouting::board::structure::Unit;
    let text = BASE.replace("(unit um)", "(unit mm)");
    for java in [true, false] {
        let board = read(&text, "dsn_unit_ignored", java);
        assert_eq!(
            board.communication.unit,
            if java { Unit::Um } else { Unit::Mm }
        );
        assert_eq!(
            board.communication.resolution,
            if java { 10 } else { 10000 }
        );
    }
    let text = BASE.replace("(placement", "(placement (resolution mil 100)");
    for java in [true, false] {
        let board = read(&text, "dsn_unit_ignored", java);
        let c = board
            .components
            .get(freerouting::board::structure::ComponentId(1))
            .unwrap()
            .location()
            .unwrap();
        assert_eq!(c.x, if java { 20000 } else { 508000 });
        assert_eq!(c.y, if java { 50000 } else { 1270000 });
        // Pin geometry stays in PCB units: the placement unit affects positions only.
        assert_eq!(board.communication.unit, Unit::Um);
        assert_eq!(board.communication.resolution, 10);
    }
    // Equal units, even with a different placement precision, do not move anything.
    let text = BASE.replace("(placement", "(placement (resolution um 100)");
    assert_eq!(
        read(&text, "dsn_unit_ignored", true)
            .components
            .get(freerouting::board::structure::ComponentId(1))
            .unwrap()
            .location(),
        read(&text, "dsn_unit_ignored", false)
            .components
            .get(freerouting::board::structure::ComponentId(1))
            .unwrap()
            .location()
    );
}

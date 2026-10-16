//! K04.6: net dialect and net table.

mod common;

use freerouting_kicad::header::read_header;
use freerouting_kicad::nets::{Dialect, NAME_NETS_FROM_VERSION, NetTable, detect_dialect};
use freerouting_kicad::sexpr::Tree;

fn dialect(src: &str) -> Dialect {
    let tree = Tree::parse(src).unwrap();
    let v = read_header(&tree).unwrap().version;
    detect_dialect(&tree, v)
}

fn load(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[test]
fn numeric_table_wins_over_a_new_stamp_issue_1028() {
    // A KiCad 10 stamp over a numeric table: ids, every net resolved.
    let src = r#"(kicad_pcb (version 20260206)
        (net 0 "") (net 1 "GND") (net 2 "VCC")
        (segment (start 0 0) (end 1 1) (width 0.2) (layer "F.Cu") (net 1))
        (via (at 1 1) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
        (footprint "X" (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "GND"))))"#;
    let tree = Tree::parse(src).unwrap();
    assert_eq!(detect_dialect(&tree, 20260206), Dialect::Ids);
    let t = NetTable::build(&tree, 20260206);
    assert_eq!(t.dialect, Dialect::Ids);
    assert_eq!(t.name(1), Some("GND"));
    assert_eq!(t.name(2), Some("VCC"));
    let board = tree.root().child("kicad_pcb").unwrap();
    let seg = board.child("segment").unwrap();
    let via = board.child("via").unwrap();
    let pad = board.child("footprint").unwrap().child("pad").unwrap();
    assert_eq!(t.net_of(seg), Some(1));
    assert_eq!(t.net_of(via), Some(2));
    assert_eq!(t.net_of(pad), Some(1));
    assert!(t.warnings.is_empty());
}

#[test]
fn a_name_reference_wins_over_the_table() {
    let src = r#"(kicad_pcb (version 20240108) (net 0 "") (net 1 "A") (segment (net "A")))"#;
    assert_eq!(dialect(src), Dialect::Names);
}

#[test]
fn a_board_with_no_nets_is_decided_by_the_stamp() {
    assert_eq!(NAME_NETS_FROM_VERSION, 20251028);
    assert_eq!(
        dialect("(kicad_pcb (version 20240108) (general))"),
        Dialect::Ids
    );
    assert_eq!(
        dialect("(kicad_pcb (version 20251028) (general))"),
        Dialect::Names
    );
    assert_eq!(
        dialect("(kicad_pcb (version 20260206) (general))"),
        Dialect::Names
    );
    assert_eq!(
        dialect("(kicad_pcb (version 20250513) (general))"),
        Dialect::Ids
    );
    let tree = Tree::parse("(kicad_pcb (version 20240108))").unwrap();
    let t = NetTable::build(&tree, 20240108);
    assert!(t.is_empty());
}

#[test]
fn names_get_codes_in_order_of_first_appearance() {
    let src = r#"(kicad_pcb (version 20260206)
        (footprint "X" (pad "1" smd rect (net "B")) (pad "2" smd rect (net "A")))
        (segment (net "A")) (via (net "C")) (zone (net "B")))"#;
    let tree = Tree::parse(src).unwrap();
    let t = NetTable::build(&tree, 20260206);
    assert_eq!(t.dialect, Dialect::Names);
    assert_eq!(
        t.by_code,
        [
            Some("".into()),
            Some("B".into()),
            Some("A".into()),
            Some("C".into())
        ]
    );
    assert_eq!(t.code_of_raw("A"), Some(2));
    let seg = tree
        .root()
        .child("kicad_pcb")
        .unwrap()
        .child("segment")
        .unwrap();
    assert_eq!(t.net_of(seg), Some(2));
}

#[test]
fn lookups_use_the_raw_escaped_text_369() {
    // The name `a"b` is written `a\"b`; a lookup by the unescaped text must miss,
    // by the raw text it must hit, and `a` / `a\"b` must stay two nets.
    let src = r#"(kicad_pcb (version 20260206) (segment (net "a\"b")) (segment (net "a")) (segment (net "a\"b")))"#;
    let tree = Tree::parse(src).unwrap();
    let t = NetTable::build(&tree, 20260206);
    assert_eq!(t.by_code.len(), 3);
    assert_eq!(t.code_of_raw(r#"a\"b"#), Some(1));
    assert_eq!(t.code_of_raw(r#"a"b"#), None);
    assert_eq!(t.name(1), Some("a\"b"));
    assert_eq!(t.code_of_raw("a"), Some(2));
    let segs: Vec<_> = tree
        .root()
        .child("kicad_pcb")
        .unwrap()
        .children("segment")
        .map(|s| t.net_of(s))
        .collect();
    assert_eq!(segs, [Some(1), Some(2), Some(1)]);
}

#[test]
fn table_problems_are_warnings() {
    let src = r#"(kicad_pcb (version 20240108) (net 0 "") (net 1 "A") (net 1 "B") (net 3 "A") (segment (net 2)) (segment (net 7)))"#;
    let tree = Tree::parse(src).unwrap();
    let t = NetTable::build(&tree, 20240108);
    assert_eq!(t.warnings.len(), 2);
    assert_eq!(t.name(1), Some("A"));
    assert_eq!(t.name(2), None, "gap");
    assert_eq!(t.name(3), Some("A"));
    assert_eq!(t.code_of_raw("A"), Some(1));
    assert_eq!(t.len(), 3);
    let segs: Vec<_> = tree
        .root()
        .child("kicad_pcb")
        .unwrap()
        .children("segment")
        .map(|s| t.net_of(s))
        .collect();
    assert_eq!(segs, [None, None]);
}

#[test]
fn cm5_minima_has_a_numeric_table_despite_its_stamp() {
    let Some(dir) = common::corpus_dir() else {
        eprintln!("no corpus; skipped");
        return;
    };
    let Some(src) = load(&dir.join("demos/cm5_minima/CM5_MINIMA_3.kicad_pcb")) else {
        eprintln!("no CM5_MINIMA_3; skipped");
        return;
    };
    let tree = Tree::parse(&src).unwrap();
    let h = read_header(&tree).unwrap();
    assert_eq!(h.version, 20250513);
    assert!(h.version >= 20250000, "the stamp alone would say names");
    let t = NetTable::build(&tree, h.version);
    assert_eq!(t.dialect, Dialect::Ids);
    assert_eq!(t.name(0), Some(""));
    assert_eq!(t.name(1), Some("GND"));
    assert_eq!(t.name(2), Some("Net-(J1-VCC)"));
    assert_eq!(t.len(), 221);
    assert!(t.warnings.is_empty());
}

#[test]
fn name_dialect_boards() {
    let tastexx = common::fixtures().join("Issue742-tastexx-pcb/tastexx-pcb.kicad_pcb");
    let src = load(&tastexx).unwrap();
    let tree = Tree::parse(&src).unwrap();
    let h = read_header(&tree).unwrap();
    assert_eq!(h.version, 20260206);
    let t = NetTable::build(&tree, h.version);
    assert_eq!(t.dialect, Dialect::Names);
    assert_eq!(t.name(1), Some("Row1"));
    assert_eq!(t.len(), 29, "{} nets", t.len());
    // every pad (and track, if any) with a net child resolves
    let board = tree.root().child("kicad_pcb").unwrap();
    let mut seen = 0;
    for fp in board.children("footprint") {
        for pad in fp.children("pad").filter(|p| p.child("net").is_some()) {
            assert!(t.net_of(pad).is_some(), "line {}", pad.line());
            seen += 1;
        }
    }
    assert_eq!(seen, 43, "{seen} pads with nets");

    if let Some(dir) = common::corpus_dir() {
        if let Some(src) = load(&dir.join("demos/pic_programmer/pic_programmer.kicad_pcb")) {
            let tree = Tree::parse(&src).unwrap();
            let t = NetTable::build(&tree, 20260206);
            assert_eq!(t.dialect, Dialect::Names);
            assert_eq!(t.name(1), Some("VCC"));
            assert_eq!(t.name(2), Some("GND"));
        }
    }
}

#[test]
fn every_corpus_board_resolves_all_its_tracks_and_vias() {
    let mut boards = common::corpus_boards("java");
    boards.extend(common::corpus_boards("demos"));
    if boards.is_empty() {
        eprintln!("no corpus; skipped");
        return;
    }
    let (mut ids, mut names) = (0, 0);
    for p in boards {
        if p.ends_with("jetson-agx-thor-baseboard.kicad_pcb") || p.ends_with("vme-wren.kicad_pcb") {
            continue; // 70-85 MB; the parse-time test covers them
        }
        let src = std::fs::read_to_string(&p).unwrap();
        let tree = Tree::parse(&src).unwrap();
        let Ok(h) = read_header(&tree) else { continue };
        let t = NetTable::build(&tree, h.version);
        match t.dialect {
            Dialect::Ids => ids += 1,
            Dialect::Names => names += 1,
        }
        let board = tree.root().child("kicad_pcb").unwrap();
        for item in board
            .items()
            .filter(|i| matches!(i.head(), Some("segment" | "via" | "arc")))
        {
            // an item without a net child is fine; one with a net child must resolve,
            // except net 0 / empty references
            if item.child("net").is_some() {
                let r = t.net_of(item);
                assert!(r.is_some(), "{}: line {}", p.display(), item.line());
            }
        }
    }
    assert_eq!((ids, names), (51, 3));
}

//! K04.3: the arena tree.

mod common;

use freerouting_kicad::sexpr::{NodeKind, Tree};

/// Rewrites `src` with every run of whitespace between tokens replaced by one
/// space (strings are kept as they are).
fn one_line(src: &str) -> String {
    use freerouting_kicad::sexpr::{Lexer, TokenKind};
    let mut out = String::with_capacity(src.len());
    for t in Lexer::new(src) {
        let t = t.unwrap();
        if !out.is_empty() {
            out.push(' ');
        }
        let (s, e) = (t.start as usize, t.end as usize);
        match t.kind {
            TokenKind::Str => {
                out.push('"');
                out.push_str(&src[s..e]);
                out.push('"');
            }
            _ => out.push_str(&src[s..e]),
        }
    }
    out
}

#[test]
fn accessors_follow_file_order() {
    let src = r#"(board (net 1 "A") (via (at 1.5 -2e-3) (net "q\"x")) (net 2 "B") (net 3 "C") atom "str")"#;
    let tree = Tree::parse(src).unwrap();
    let b = tree.root().child("board").unwrap();
    assert_eq!(b.head(), Some("board"));
    assert_eq!(b.len(), 7);
    let nets: Vec<_> = b.children("net").map(|n| n.atom(1).unwrap()).collect();
    assert_eq!(nets, ["1", "2", "3"]);
    let via = b.child("via").unwrap();
    let at = via.child("at").unwrap();
    assert_eq!(at.f64(1), Some(1.5));
    assert_eq!(at.f64(2), Some(-2e-3));
    assert_eq!(at.f64(3), None);
    assert_eq!(via.child("net").unwrap().str_value(1).unwrap(), "q\"x");
    assert_eq!(via.child("net").unwrap().raw(1), Some(r#"q\"x"#));
    assert_eq!(b.item(5).unwrap().kind(), NodeKind::Atom);
    assert_eq!(b.item(6).unwrap().kind(), NodeKind::Str);
    assert_eq!(b.atom(6), None, "a string is not a bare atom");
    assert_eq!(b.str_value(5).unwrap(), "atom");
    assert!(b.child("missing").is_none());
    assert_eq!(b.items().len(), 7);
}

#[test]
fn lines_of_nodes() {
    let tree = Tree::parse("(a\n  (b 1)\n  \"s\"\n  (c\n 2))").unwrap();
    let a = tree.root().child("a").unwrap();
    assert_eq!(a.line(), 1);
    assert_eq!(a.child("b").unwrap().line(), 2);
    assert_eq!(a.item(2).unwrap().line(), 3);
    assert_eq!(a.child("c").unwrap().position(), (4, 3));
    assert_eq!(tree.root().line(), 1, "the root reports its first item");
}

#[test]
fn syntax_errors_come_from_the_lexer() {
    assert!(Tree::parse("(a (b)").is_err());
    assert!(Tree::parse("(a))").is_err());
    assert!(Tree::parse("(a \"x)").is_err());
    assert!(Tree::parse("").unwrap().is_empty());
}

#[test]
fn one_line_rewrite_of_corney_gives_the_same_tree() {
    let path =
        common::fixtures().join("Issue368-CorneyIslandWireless/corney_island_wireless.kicad_pcb");
    let src = std::fs::read_to_string(path).unwrap();
    let flat = one_line(&src);
    assert!(!flat.contains('\n') || flat.matches('\n').count() < src.matches('\n').count());
    let a = Tree::parse(&src).unwrap();
    let b = Tree::parse(&flat).unwrap();
    assert!(a.len() > 1000, "{} nodes", a.len());
    assert_eq!(a.len(), b.len());
    assert!(a == b, "trees differ");
    // and a change is noticed
    let changed = flat.replacen("layer", "layerx", 1);
    let c = Tree::parse(&changed).unwrap();
    assert!(a != c);
}

#[test]
fn one_line_rewrites_of_every_committed_board_agree() {
    for p in common::committed_boards() {
        let src = std::fs::read_to_string(&p).unwrap();
        let flat = one_line(&src);
        let (a, b) = (Tree::parse(&src).unwrap(), Tree::parse(&flat).unwrap());
        assert!(a == b, "{}", p.display());
    }
}

#[test]
fn deeply_nested_input_does_not_overflow_the_stack() {
    let depth = 100_000;
    let mut src = String::with_capacity(depth * 2 + 8);
    for _ in 0..depth {
        src.push('(');
    }
    src.push('x');
    for _ in 0..depth {
        src.push(')');
    }
    // run in a small thread to prove it is not the main thread's big stack
    let t = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let tree = Tree::parse(&src).unwrap();
            assert_eq!(tree.len(), depth + 2);
            let mut n = tree.root().item(0).unwrap();
            let mut seen = 1;
            while let Some(c) = n.item(0) {
                if !c.is_list() {
                    break;
                }
                n = c;
                seen += 1;
            }
            assert_eq!(seen, depth);
            assert!(tree == tree.clone());
        })
        .unwrap();
    t.join().unwrap();
}

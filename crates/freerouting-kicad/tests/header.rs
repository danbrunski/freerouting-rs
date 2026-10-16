//! K04.4: header and version gate.

mod common;

use freerouting_kicad::header::{self, FIRST_SUPPORTED_VERSION, read_header};
use freerouting_kicad::sexpr::{Tree, decode};
use freerouting_kicad::{ErrorKind, KicadError};

fn check(src: &str) -> Result<header::Header, KicadError> {
    header::check(src)
}

#[test]
fn reads_version_generator_and_generator_version() {
    let h = check(
        "(kicad_pcb (version 20241229) (generator \"pcbnew\") (generator_version \"9.0\") (general))",
    )
    .unwrap();
    assert_eq!(h.version, 20241229);
    assert_eq!(h.generator, "pcbnew");
    assert_eq!(h.generator_version.as_deref(), Some("9.0"));
    assert!(!h.is_newer_than_known());
}

#[test]
fn generator_may_be_an_atom_and_absent() {
    let h = check("(kicad_pcb (version 20211014) (generator pcbnew))").unwrap();
    assert_eq!(
        (h.generator.as_str(), h.generator_version),
        ("pcbnew", None)
    );
    let h = check("(kicad_pcb (version 20211014))").unwrap();
    assert_eq!(h.generator, "");
}

#[test]
fn a_missing_version_reads_as_the_first_supported_one() {
    // KiCad's parseHeader does the same (20201115).
    assert_eq!(
        check("(kicad_pcb (general))").unwrap().version,
        FIRST_SUPPORTED_VERSION
    );
}

#[test]
fn the_gate_is_exactly_20201115() {
    assert!(check("(kicad_pcb (version 20201115))").is_ok());
    let e = check("(kicad_pcb (version 20201114))").unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnsupportedVersion);
    assert!(e.message.contains("20201114"));
}

#[test]
fn newer_than_known_is_read_but_flagged() {
    let h = check("(kicad_pcb (version 20300101))").unwrap();
    assert!(h.is_newer_than_known());
}

#[test]
fn not_a_board() {
    for src in [
        "(footprint \"x\" (version 20240108))",
        "(kicad_sch (version 20240108))",
        "kicad_pcb",
        "((kicad_pcb))",
        "",
    ] {
        let e = check(src).unwrap_err();
        assert_eq!(e.kind, ErrorKind::NotABoard, "{src}: {e}");
    }
    let e = check("(kicad_pcb (version 20240108)) (extra)").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Syntax);
    assert_eq!(e.line, 1);
}

#[test]
fn a_version_that_is_not_a_number() {
    let e = check("(kicad_pcb (version abc))").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Syntax);
    let e = check("(kicad_pcb (version -5))").unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnsupportedVersion);
}

#[test]
fn pcbench_version_3_is_refused_with_one_line() {
    let src = "(kicad_pcb (version 3) (host pcbnew \"(2013-07-07 BZR 4022)-stable\")\n  (general (links 0)))";
    let e = check(src).unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnsupportedVersion);
    assert_eq!(e.line, 1);
    let line = e.to_string();
    assert!(!line.contains('\n'), "{line}");
    assert!(
        line.contains("(KiCad 5 or older)") && line.contains("version 3"),
        "{line}"
    );
    assert!(line.contains("open and save the board in KiCad 6 or later"));
}

#[test]
fn issue069_testsensel_is_refused() {
    let Some(bytes) = common::java_show("fixtures/Issue069-TestSensel/TestSensel.kicad_pcb") else {
        eprintln!(
            "Java repo at {} not available; skipped",
            common::JAVA_COMMIT
        );
        return;
    };
    let src = decode(&bytes).unwrap();
    let e = check(src).unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnsupportedVersion);
    assert!(e.message.contains("20171130"), "{e}");
    assert!(!e.to_string().contains('\n'));
}

#[test]
fn pcbench_file_with_version_3_is_refused() {
    let p = "scripts/benchmark/fixtures/PCBench/6N137-TTL-Serial-Optoisolator_6N137-TTL-Serial-Optoisolator/raw.kicad_pcb";
    let Some(bytes) = common::java_show(p) else {
        eprintln!(
            "Java repo at {} not available; skipped",
            common::JAVA_COMMIT
        );
        return;
    };
    let e = check(decode(&bytes).unwrap()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnsupportedVersion);
    assert!(e.message.contains("version 3 "), "{e}");
}

#[test]
fn committed_boards_pass_the_gate() {
    let versions: Vec<u32> = common::committed_boards()
        .iter()
        .map(|p| {
            let src = std::fs::read_to_string(p).unwrap();
            let tree = Tree::parse(&src).unwrap();
            read_header(&tree).unwrap().version
        })
        .collect();
    let mut sorted = versions.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            20211014, 20221018, 20240108, 20240108, 20241229, 20241229, 20260206
        ]
    );
}

#[test]
fn corpus_boards_pass_the_gate_except_kicad_5() {
    let java = common::corpus_boards("java");
    let demos = common::corpus_boards("demos");
    if java.is_empty() && demos.is_empty() {
        eprintln!("no corpus; skipped (tools/kicad-reader/fetch-corpus.sh target/kicad-corpus)");
        return;
    }
    let mut refused = Vec::new();
    for p in java.iter().chain(&demos) {
        let bytes = std::fs::read(p).unwrap();
        let src = decode(&bytes).unwrap();
        match header::check(src) {
            Ok(_) => {}
            Err(e) if e.kind == ErrorKind::UnsupportedVersion => refused.push(p.clone()),
            Err(e) => panic!("{}: {e}", p.display()),
        }
    }
    if !java.is_empty() {
        assert_eq!(java.len(), 38);
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(refused[0].ends_with("TestSensel.kicad_pcb"));
    }
    if !demos.is_empty() {
        assert!(demos.len() >= 18, "{} demos", demos.len());
        assert!(
            refused
                .iter()
                .all(|p| !p.starts_with(common::corpus_dir().unwrap().join("demos")))
        );
    }
}

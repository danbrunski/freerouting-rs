//! K07.1: `io::read_design` and `io::kicad`, the direct `.kicad_pcb` entry
//! points.

use std::path::{Path, PathBuf};

use freerouting::drc::{DrcRunOptions, run_drc};
use freerouting::io::kicad::{
    DesignKind, KicadReadError, ReadOptions, design_kind, read_kicad_input,
};
use freerouting::io::{BoardReadResult, read_design, read_kicad_board};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kicad")
}

fn committed_boards() -> Vec<PathBuf> {
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                collect(&p, out);
            } else if p.extension().is_some_and(|x| x == "kicad_pcb") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    collect(&fixtures(), &mut out);
    out.sort();
    out
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("kicad-read-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn dev_board() -> PathBuf {
    fixtures().join("Issue558-dev-board-autoroute-demo/dev-board.kicad_pcb")
}

#[test]
fn the_extension_picks_the_reader() {
    assert_eq!(design_kind(Path::new("a.kicad_pcb")), DesignKind::KicadPcb);
    assert_eq!(design_kind(Path::new("A.KiCad_PCB")), DesignKind::KicadPcb);
    assert_eq!(design_kind(Path::new("a.dsn")), DesignKind::Dsn);
    assert_eq!(design_kind(Path::new("a.kicad_pcb.dsn")), DesignKind::Dsn);
    assert_eq!(design_kind(Path::new("kicad_pcb")), DesignKind::Dsn);

    let kicad = read_design(&dev_board()).unwrap();
    assert!(kicad.kicad.is_some());
    assert!(kicad.result.board().is_some());
    let dsn = read_design(&dev_board().with_extension("kicad10.dsn")).unwrap();
    assert!(dsn.kicad.is_none());
    assert!(dsn.result.board().is_some());

    // an upper-case extension on a copy is read as a KiCad board too
    let dir = scratch("case");
    let copy = dir.join("DEV.KICAD_PCB");
    std::fs::copy(dev_board(), &copy).unwrap();
    assert!(read_design(&copy).unwrap().kicad.is_some());
}

/// A missing `.kicad_pro` is not an error: KiCad's defaults.
#[test]
fn a_missing_project_file_is_not_an_error() {
    let board = fixtures().join("Issue508-DAC2020/DAC2020_bm01/bm1.unrouted.kicad_pcb");
    assert!(!board.with_extension("kicad_pro").exists());
    let input = read_kicad_input(&board, ReadOptions::default()).unwrap();
    assert!(input.project_file.is_none());
    assert!(input.rules_file.is_none());
    assert!(
        input
            .report_lines()
            .iter()
            .any(|l| l.contains("No .kicad_pro"))
    );
    assert!(read_kicad_board(&board).board().is_some());
}

/// A KiCad 5 file is refused with K04.4's one-line message.
#[test]
fn a_kicad_5_file_is_refused_in_one_line() {
    let dir = scratch("kicad5");
    let path = dir.join("old.kicad_pcb");
    std::fs::write(
        &path,
        "(kicad_pcb (version 20171130) (host pcbnew 4.0.7))\n",
    )
    .unwrap();
    match read_kicad_board(&path) {
        BoardReadResult::ParseError { location, detail } => {
            assert_eq!(location, "old.kicad_pcb");
            assert!(
                detail.contains("unsupported KiCad board file version 20171130 (KiCad 5 or older)"),
                "{detail}"
            );
            assert!(!detail.contains('\n'));
        }
        other => panic!("{other:?}"),
    }
    let design = read_design(&path).unwrap();
    assert!(matches!(design.result, BoardReadResult::ParseError { .. }));
    let err = run_drc(&DrcRunOptions::new(&path)).unwrap_err();
    assert!(err.contains("KiCad 5 or older"), "{err}");
    // a file that is not there is an I/O error, not a refusal
    let missing = dir.join("missing.kicad_pcb");
    assert!(read_design(&missing).unwrap_err().contains("Couldn't load"));
    assert!(matches!(
        read_kicad_input(&missing, ReadOptions::default()),
        Err(KicadReadError::Io(_))
    ));
}

/// The reader's warnings come first in the board's warnings, each with its
/// line; the `wire_keepout` warning of K00 question 1 reaches the caller.
#[test]
fn reader_warnings_are_merged_with_their_lines() {
    let board = fixtures().join("synthetic/rule-areas.kicad_pcb");
    let input = read_kicad_input(&board, ReadOptions::default()).unwrap();
    let wire_keepouts: Vec<&String> = input
        .warnings
        .iter()
        .filter(|w| w.contains("wire_keepout"))
        .collect();
    assert!(!wire_keepouts.is_empty());
    assert!(
        wire_keepouts.iter().all(|w| w.starts_with("line ")),
        "{wire_keepouts:?}"
    );
    let result = read_kicad_board(&board);
    let BoardReadResult::Success { warnings, .. } = result else {
        panic!("{result:?}");
    };
    assert_eq!(&warnings[..input.warnings.len()], &input.warnings[..]);
}

/// The DRC of a `.kicad_pcb` finds what the DRC of KiCad's own export finds
/// on the dev board (its one rule area bans everything, so the reader only
/// adds a placement keepout that nothing collides with).
#[test]
fn dev_board_drc_equals_the_exports() {
    let a = run_drc(&DrcRunOptions::new(dev_board())).unwrap();
    let b = run_drc(&DrcRunOptions::new(
        dev_board().with_extension("kicad10.dsn"),
    ))
    .unwrap();
    let json_a = a.report.to_json();
    let json_b = b.report.to_json();
    let field = |json: &str, key: &str| -> String {
        let at = json.find(&format!("\"{key}\"")).unwrap();
        json[at..].lines().next().unwrap().to_owned()
    };
    for key in ["unconnectedItems", "violations", "schematicParity"] {
        assert_eq!(field(&json_a, key), field(&json_b, key), "{key}");
    }
    assert!(
        a.messages
            .iter()
            .any(|m| m.starts_with("Loading KiCad board file"))
    );
    assert!(a.messages.iter().any(|m| m.starts_with("rule areas: 1")));
}

/// K07.5 through the board builder: with the rules applied, the clearance
/// matrix and the class width change as the two supported rules say.
#[test]
fn design_rules_reach_the_board() {
    use freerouting::io::specctra::build_board;
    let dir = scratch("dru");
    for name in ["classes.kicad_pcb", "classes.kicad_pro"] {
        std::fs::copy(fixtures().join("synthetic").join(name), dir.join(name)).unwrap();
    }
    let board_path = dir.join("classes.kicad_pcb");
    let dru = std::fs::read(fixtures().join("synthetic/rules.kicad_dru")).unwrap();
    std::fs::write(dir.join("classes.kicad_dru"), dru).unwrap();

    let build = |apply: bool| {
        let options = ReadOptions {
            apply_design_rules: apply,
            ..ReadOptions::default()
        };
        let input = read_kicad_input(&board_path, options).unwrap();
        assert_eq!(input.rules.rules.len(), 3);
        match build_board(&input.pcb) {
            BoardReadResult::Success { board, .. } => board,
            other => panic!("{other:?}"),
        }
    };
    let (off, on) = (build(false), build(true));
    let value = |b: &freerouting::board::RoutingBoard, x: &str, y: &str| {
        let m = &b.rules.clearance_matrix;
        let (i, j) = (m.get_no(x), m.get_no(y));
        (i >= 0 && j >= 0).then(|| m.get_value(i, j, 0, false))
    };
    // 0.5 mm in board units of 0.1 um
    assert_eq!(value(&on, "Fast", "Wide,Default"), Some(5000));
    assert_eq!(value(&on, "Wide,Default", "Fast"), Some(5000));
    assert_ne!(value(&off, "Fast", "Wide,Default"), Some(5000));
    // the pair the rules do not mention is unchanged
    assert_eq!(value(&on, "Fast", "Fast"), value(&off, "Fast", "Fast"));
}

/// Every committed board and every synthetic board opens through
/// `read_design` as a board with nets and components.
#[test]
fn every_committed_board_opens() {
    let boards = committed_boards();
    assert!(boards.len() >= 15, "{boards:?}");
    for path in boards {
        let read = read_design(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let input = read.kicad.as_ref().expect("a KiCad read");
        let board = read
            .result
            .board()
            .unwrap_or_else(|| panic!("{}: {:?}", path.display(), read.result));
        assert!(board.rules.nets.max_net_number() > 0, "{}", path.display());
        assert_eq!(
            input.project_file.is_some(),
            path.with_extension("kicad_pro").exists(),
            "{}",
            path.display()
        );
    }
}

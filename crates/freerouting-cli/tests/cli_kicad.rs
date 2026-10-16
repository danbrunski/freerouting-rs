//! K07.2: the command line opens `.kicad_pcb` files directly.

mod common;

use std::path::{Path, PathBuf};

use common::*;

fn boards() -> Vec<PathBuf> {
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
    collect(&repo("fixtures/kicad"), &mut out);
    out.sort();
    out
}

fn arg(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// `-de board.kicad_pcb -drc report.json` writes a report for every
/// committed and synthetic board and prints the read report.
#[test]
fn drc_opens_every_committed_board() {
    let dir = scratch("kicad-drc");
    let all = boards();
    assert!(all.len() >= 15);
    for (n, board) in all.iter().enumerate() {
        let report = dir.join(format!("{n}.json"));
        let out = run_in(&dir, &["-de", &arg(board), "-drc", &arg(&report)]);
        assert_eq!(code(&out), 0, "{}: {}", board.display(), stderr(&out));
        let text = std::fs::read_to_string(&report).unwrap();
        assert!(text.contains("\"violations\""), "{}", board.display());
        assert!(stderr(&out).contains("rule areas:"), "{}", board.display());
    }
}

/// The dev board's report equals the report for KiCad's own DSN export.
#[test]
fn dev_board_report_equals_the_dsn_export() {
    let dir = scratch("kicad-dev");
    let board = repo("fixtures/kicad/Issue558-dev-board-autoroute-demo/dev-board.kicad_pcb");
    let dsn = board.with_extension("kicad10.dsn");
    let (a, b) = (dir.join("a.json"), dir.join("b.json"));
    assert_eq!(
        code(&run_in(&dir, &["-de", &arg(&board), "-drc", &arg(&a)])),
        0
    );
    assert_eq!(
        code(&run_in(&dir, &["-de", &arg(&dsn), "-drc", &arg(&b)])),
        0
    );
    let line = |p: &Path, key: &str| {
        let t = std::fs::read_to_string(p).unwrap();
        let at = t.find(&format!("\"{key}\"")).unwrap();
        t[at..].lines().next().unwrap().to_owned()
    };
    for key in ["unconnectedItems", "violations", "qualityScore"] {
        assert_eq!(line(&a, key), line(&b, key), "{key}");
    }
}

/// `-de board.kicad_pcb -do out.ses` runs the job on the board and writes a
/// session that names the design.
#[test]
fn route_job_writes_a_session() {
    let dir = scratch("kicad-job");
    let board =
        repo("fixtures/kicad/Issue368-CorneyIslandWireless/corney_island_wireless.kicad_pcb");
    let out = run_in(&dir, &["-de", &arg(&board), "-do", "out.ses"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let ses = std::fs::read_to_string(dir.join("out.ses")).unwrap();
    assert!(
        ses.starts_with("(session \"corney_island_wireless\""),
        "{ses:.80}"
    );
    assert!(ses.contains("(base_design \"corney_island_wireless\")"));
    let log = stderr(&out);
    assert!(log.contains("rule areas:"), "{log}");
    assert!(
        log.contains("Job 'corney_island_wireless' finished"),
        "{log}"
    );
}

/// The extension case does not matter, and the reader's warnings (the
/// `wire_keepout` areas of K00 question 1) are printed with their lines.
#[test]
fn warnings_reach_the_console() {
    let dir = scratch("kicad-warn");
    let src = repo("fixtures/kicad/synthetic/rule-areas.kicad_pcb");
    let copy = dir.join("Areas.KICAD_PCB");
    std::fs::copy(&src, &copy).unwrap();
    let out = run_in(&dir, &["-de", &arg(&copy), "-do", "out.ses"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let log = stderr(&out);
    assert!(
        log.lines()
            .any(|l| l.starts_with("warning: KiCad reader: line ") && l.contains("wire_keepout")),
        "{log}"
    );
}

/// A KiCad 5 file exits 1 with the reader's one-line message.
#[test]
fn a_kicad_5_file_exits_1() {
    let dir = scratch("kicad-old");
    std::fs::write(
        dir.join("old.kicad_pcb"),
        "(kicad_pcb (version 20171130) (host pcbnew 4.0.7))\n",
    )
    .unwrap();
    for args in [
        vec!["-de", "old.kicad_pcb", "-do", "out.ses"],
        vec!["-de", "old.kicad_pcb", "-drc", "r.json"],
    ] {
        let out = run_in(&dir, &args);
        assert_eq!(code(&out), 1);
        let log = stderr(&out);
        let errors: Vec<&str> = log.lines().filter(|l| l.starts_with("error:")).collect();
        assert_eq!(errors.len(), 1, "{log}");
        assert!(errors[0].contains("KiCad 5 or older"), "{log}");
    }
    assert!(!dir.join("out.ses").exists());
}

//! K08.6: `-do routed.kicad_pcb` writes the routed copper into a copy of the
//! `.kicad_pcb` input; `-do` to the input itself, or a `.kicad_pcb` output of
//! a DSN input, is refused with exit 2 before any work.

mod common;

use std::path::Path;

use common::*;

const DEV: &str = "fixtures/kicad/Issue558-dev-board-autoroute-demo";

/// A copy of the dev board, its project and Java's session for it.
fn dev_board(dir: &Path) {
    for (from, to) in [
        ("dev-board.kicad_pcb", "dev-board.kicad_pcb"),
        ("dev-board.kicad_pro", "dev-board.kicad_pro"),
        ("dev-board.kicad10.java.ses", "dev-board.ses"),
    ] {
        std::fs::copy(repo(&format!("{DEV}/{from}")), dir.join(to)).unwrap();
    }
}

#[test]
fn kicad_output_gets_the_routed_copper() {
    let dir = scratch("kicad-write-out");
    dev_board(&dir);
    let before = std::fs::read(dir.join("dev-board.kicad_pcb")).unwrap();
    let out = run_in(
        &dir,
        &[
            "-de",
            "dev-board.kicad_pcb+dev-board.ses",
            "-do",
            "routed.kicad_pcb",
            "--router.autorouter.enabled=false",
            "--router.fanout.enabled=false",
            "--router.optimizer.enabled=false",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stderr(&out).contains("(KICAD_PCB)"), "{}", stderr(&out));
    let text = std::fs::read_to_string(dir.join("routed.kicad_pcb")).unwrap();
    // Java's session: 231 wires of 459 segments, 35 vias
    assert_eq!(text.matches("\t(segment\n").count(), 459);
    assert_eq!(text.matches("\t(via\n").count(), 35);
    assert!(text.starts_with("(kicad_pcb"));
    assert_eq!(
        std::fs::read(dir.join("dev-board.kicad_pcb")).unwrap(),
        before
    );
    // the output opens again
    let again = run_in(&dir, &["-de", "routed.kicad_pcb", "-drc", "drc.json"]);
    assert_eq!(code(&again), 0, "{}", stderr(&again));
}

#[test]
fn kicad_output_to_the_input_is_refused() {
    let dir = scratch("kicad-write-same");
    dev_board(&dir);
    let before = std::fs::read(dir.join("dev-board.kicad_pcb")).unwrap();
    for target in [
        "dev-board.kicad_pcb",
        "./dev-board.kicad_pcb",
        "../cli-kicad-write-same/dev-board.kicad_pcb",
    ] {
        let out = run_in(&dir, &["-de", "dev-board.kicad_pcb", "-do", target]);
        assert_eq!(code(&out), 2, "{target}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("Refusing to overwrite the input board"),
            "{}",
            stderr(&out)
        );
        assert_eq!(
            std::fs::read(dir.join("dev-board.kicad_pcb")).unwrap(),
            before
        );
    }
}

#[test]
fn kicad_output_of_a_dsn_input_is_refused() {
    let dir = scratch("kicad-write-dsn");
    std::fs::copy(
        repo(&format!("{DEV}/dev-board.kicad10.dsn")),
        dir.join("dev-board.dsn"),
    )
    .unwrap();
    let out = run_in(&dir, &["-de", "dev-board.dsn", "-do", "routed.kicad_pcb"]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("needs a .kicad_pcb input"),
        "{}",
        stderr(&out)
    );
    assert!(!dir.join("routed.kicad_pcb").exists());
}

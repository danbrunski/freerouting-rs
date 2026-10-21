//! K14.4: `--router.input_fixups.fixed_pour_classes` on a `.kicad_pcb` route. The
//! fixed wires that cover a pour's fill are on the board (the `.dsn` output holds them)
//! and in no other output: not in the session, which KiCad's `ImportSpecctraSES` reads
//! (the DSN path's property: the router never writes fixed wires into the session), and
//! not in the `.kicad_pcb` output. The fixture is the reader's
//! (`crates/freerouting-kicad/tests/data/k14-fixups.*`, made by
//! `tools/kicad-reader/k14_fixture.py`); its VBUS pour on F.Cu (class Power) gives 44
//! fixed wires. The autorouter, fanout and optimizer are off, so the board's only traces
//! are the fixed wires and the outputs are quick to check.

mod common;

use std::path::Path;

use common::*;

const DATA: &str = "crates/freerouting-kicad/tests/data";

fn fixture(dir: &Path) {
    for ext in ["kicad_pcb", "kicad_pro"] {
        std::fs::copy(
            repo(&format!("{DATA}/k14-fixups.{ext}")),
            dir.join(format!("k14-fixups.{ext}")),
        )
        .unwrap();
    }
}

fn route(dir: &Path, out: &str, extra: &[&str]) -> std::process::Output {
    let mut args = vec![
        "-de",
        "k14-fixups.kicad_pcb",
        "-do",
        out,
        "--router.autorouter.enabled=false",
        "--router.fanout.enabled=false",
        "--router.optimizer.enabled=false",
    ];
    args.extend_from_slice(extra);
    run_in(dir, &args)
}

#[test]
fn the_fixed_pour_wires_are_in_no_output_but_the_dsn() {
    let dir = scratch("k14-fixed-pours");
    fixture(&dir);
    let before = std::fs::read(dir.join("k14-fixups.kicad_pcb")).unwrap();
    let out = route(
        &dir,
        "fixed.ses+fixed.kicad_pcb+fixed.dsn",
        &["--router.input_fixups.fixed_pour_classes=Power"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let log = stderr(&out) + &stdout(&out);
    assert!(
        log.contains(
            "input fix-up: pours: 44 fixed wires for 1 pour(s) of Power; 1 outer plane(s) removed"
        ),
        "{log}"
    );
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    // the board has them: the DSN output writes the board's fixed wires
    let dsn = read("fixed.dsn");
    assert_eq!(dsn.matches("(type fix)").count(), 44, "{dsn}");
    // the session and the .kicad_pcb output do not
    let ses = read("fixed.ses");
    assert!(!ses.contains("(wire"), "{ses}");
    let pcb = read("fixed.kicad_pcb");
    assert!(!pcb.contains("(segment"), "a fixed wire became a track");
    assert_eq!(pcb.as_bytes(), &before[..], "nothing to write back");
    // the same route without the setting writes the same session
    let plain = route(&dir, "plain.ses", &[]);
    assert_eq!(code(&plain), 0, "{}", stderr(&plain));
    assert_eq!(
        ses.replace("fixed", "plain"),
        read("plain.ses"),
        "the session does not depend on the fixed wires"
    );
}

#[test]
fn an_unknown_fixed_pour_class_fails_the_run_naming_it() {
    let dir = scratch("k14-fixed-pours-unknown");
    fixture(&dir);
    let out = route(
        &dir,
        "out.ses",
        &["--router.input_fixups.fixed_pour_classes=Nope"],
    );
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out)
            .contains("router.input_fixups.fixed_pour_classes: the board has no net class Nope"),
        "{}",
        stderr(&out)
    );
    assert!(!dir.join("out.ses").exists());
}

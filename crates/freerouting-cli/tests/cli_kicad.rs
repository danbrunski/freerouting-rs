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

/// K10.3: `--apply-design-rules` puts the expressible `.kicad_dru` rules on
/// the board of a `.kicad_pcb` input, as `ReadOptions::apply_design_rules`
/// does in `design_rules_reach_the_board`. On the `classes` board the rule
/// "fast tracks" (track width 0.3 mm for class Fast) changes the width of the
/// routed Fast wires in the session from 0.1 mm to 0.3 mm; without the
/// switch the rules are read, reported as ignored, and change nothing.
#[test]
fn the_apply_design_rules_switch_changes_the_class_rules() {
    let dir = scratch("kicad-dru");
    let synthetic = repo("fixtures/kicad/synthetic");
    for (from, to) in [
        ("classes.kicad_pcb", "classes.kicad_pcb"),
        ("classes.kicad_pro", "classes.kicad_pro"),
        ("rules.kicad_dru", "classes.kicad_dru"),
    ] {
        std::fs::copy(synthetic.join(from), dir.join(to)).unwrap();
    }
    let board = arg(&dir.join("classes.kicad_pcb"));
    let route = |name: &str, extra: &[&str]| {
        // The project has no Board Setup rules, so KiCad's 0.2 mm minimum
        // track width would floor class Fast's 0.1 mm (Q18); this test is
        // about the rule file, so the board minimum is set under it.
        let mut args = vec![
            "-de",
            board.as_str(),
            "-do",
            name,
            "--max-passes",
            "1",
            "--router.board_minimums.track_width_um=100",
        ];
        args.extend_from_slice(extra);
        let out = run_in(&dir, &args);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        (
            stderr(&out),
            std::fs::read_to_string(dir.join(name)).unwrap(),
        )
    };
    let (off_log, off) = route("off.ses", &[]);
    let (on_log, on) = route("on.ses", &["--apply-design-rules"]);

    for rule in ["clearance", "track_width"] {
        assert!(
            off_log.contains(&format!(
                "constraint {rule}: ignored (apply_design_rules is off)"
            )),
            "{off_log}"
        );
        assert!(
            on_log.contains(&format!("constraint {rule}: applied (1 target(s))")),
            "{on_log}"
        );
    }
    // the rule the DSN cannot say is reported either way
    for log in [&off_log, &on_log] {
        assert!(log.contains("UNSUPPORTED"), "{log}");
    }
    let widths = |ses: &str| -> Vec<String> {
        ses.lines()
            .filter_map(|l| l.trim().strip_prefix("(path F.Cu "))
            .map(str::to_owned)
            .collect()
    };
    assert!(widths(&off).iter().any(|w| w == "1000"), "{off}");
    assert!(widths(&on).iter().any(|w| w == "3000"), "{on}");
    assert!(!on.lines().zip(off.lines()).all(|(a, b)| a == b));
}

/// K10.3: the switch means nothing to a DSN input; it says so and routes.
#[test]
fn the_apply_design_rules_switch_on_a_dsn_is_a_warning() {
    let dir = scratch("kicad-dru-dsn");
    let dsn = repo("fixtures/kicad/synthetic/classes.kicad10.dsn");
    let out = run_in(
        &dir,
        &[
            "-de",
            &arg(&dsn),
            "-do",
            "out.ses",
            "--max-passes",
            "1",
            "--apply-design-rules",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("--apply-design-rules only applies to a .kicad_pcb input"),
        "{}",
        stderr(&out)
    );
}

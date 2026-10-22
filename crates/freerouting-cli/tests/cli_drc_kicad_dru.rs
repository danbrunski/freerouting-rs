//! K15.17: `-drc` on a DSN takes `--kicad-dru`.
//!
//! The board is a DSN written by hand, standing in for KiCad's export of
//! K15.11's `f-rule-clearance` case (no KiCad is needed): the pads of U1 and
//! U2 (different nets) are 0.1670 mm apart as copper, under the class
//! clearance of 0.2 mm. The `.kicad_dru` holds the pcu board's unconditional
//! `clearance (min 0.127mm)`, which replaces the class clearance in KiCad's
//! DRC, so the KiCad-counted report has the `clearance` findings without the
//! flag and none with it. The rules reach only the report's counting
//! (`KicadReportInputs::apply_rules`), as a routing job's DRC takes them; the
//! board `-drc` checks is the DSN's own (the rules are evaluated on a copy of
//! its tree).

mod common;

use common::*;

/// Two nets whose first pins sit 967 um apart: 0.167 mm between 0.8 mm pads.
const DSN: &str = "(pcb f-rule-clearance.dsn
  (parser
    (string_quote \")
    (space_in_quoted_tokens on)
  )
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu
      (type signal)
      (property
        (index 0)
      )
    )
    (boundary
      (path pcb 0  0 0  10000 0  10000 10000  0 10000  0 0)
    )
    (rule
      (width 250)
      (clearance 200)
    )
  )
  (placement
    (component PAD
      (place U1 2000 5000 front 0)
      (place U2 2967 5000 front 0)
      (place U3 8000 2000 front 0)
      (place U4 8000 8000 front 0)
    )
  )
  (library
    (image PAD
      (pin CirclePad_F_800_um 1 0 0)
    )
    (padstack CirclePad_F_800_um
      (shape (circle F.Cu 800))
      (attach off)
    )
  )
  (network
    (net A (pins U1-1 U3-1))
    (net B (pins U2-1 U4-1))
    (class kicad_default A B
    )
  )
)
";

const RULE: &str = "(version 1)\n\
    (rule \"Minimum copper-to-copper spacing (5 mil)\"\n\
    \t(constraint clearance (min 0.127mm))\n\
    \t(severity error))\n";

/// The report's findings of `kind` in the JSON text.
fn findings(report: &str, kind: &str) -> usize {
    report.matches(&format!("\"type\": \"{kind}\"")).count()
}

/// `-drc` on the DSN with `extra` arguments: (exit code, stderr, report).
fn drc(dir: &std::path::Path, extra: &[&str]) -> (i32, String, String) {
    let mut args = vec!["-de", "board.dsn", "-drc", "report.json"];
    args.extend_from_slice(extra);
    let _ = std::fs::remove_file(dir.join("report.json"));
    let out = run_in(dir, &args);
    let report = std::fs::read_to_string(dir.join("report.json")).unwrap_or_default();
    (code(&out), stderr(&out), report)
}

#[test]
fn the_rules_clearance_is_what_the_dsn_report_counts() {
    let dir = scratch("drc-kicad-dru");
    std::fs::write(dir.join("board.dsn"), DSN).unwrap();
    std::fs::write(dir.join("rules.kicad_dru"), RULE).unwrap();

    let (exit, log, plain) = drc(&dir, &[]);
    assert_eq!(exit, 0, "{log}");
    // The class clearance of 0.2 mm: the pair is a violation.
    assert_eq!(findings(&plain, "clearance"), 1, "{plain}");
    assert!(
        plain.contains("expected: 0.2000 mm, actual: 0.1670 mm"),
        "{plain}"
    );

    let (exit, log, ruled) = drc(&dir, &["--kicad-dru", "rules.kicad_dru"]);
    assert_eq!(exit, 0, "{log}");
    // The rule's 0.127 mm replaces it, as in KiCad.
    assert_eq!(findings(&ruled, "clearance"), 0, "{ruled}");
    assert!(log.contains("rules.kicad_dru"), "the file is named: {log}");
    // Nothing else changes: the unconnected nets are the same.
    let key = |t: &str, k: &str| {
        let at = t.find(&format!("\"{k}\"")).unwrap();
        t[at..].lines().next().unwrap().to_owned()
    };
    assert_eq!(
        key(&plain, "unconnectedItems"),
        key(&ruled, "unconnectedItems")
    );
}

#[test]
fn a_rule_above_the_class_clearance_is_what_the_report_counts() {
    // A rule over the pair's 0.167 mm keeps it a violation, at the rule's value.
    let dir = scratch("drc-kicad-dru-above");
    std::fs::write(dir.join("board.dsn"), DSN).unwrap();
    std::fs::write(
        dir.join("rules.kicad_dru"),
        RULE.replace("0.127mm", "0.3mm"),
    )
    .unwrap();
    let (exit, log, report) = drc(&dir, &["--kicad-dru", "rules.kicad_dru"]);
    assert_eq!(exit, 0, "{log}");
    assert_eq!(findings(&report, "clearance"), 1, "{report}");
    assert!(
        report.contains("expected: 0.3000 mm, actual: 0.1670 mm"),
        "{report}"
    );
}

#[test]
fn a_kicad_dru_that_does_not_parse_is_the_routing_jobs_warning() {
    let dir = scratch("drc-kicad-dru-broken");
    std::fs::write(dir.join("board.dsn"), DSN).unwrap();
    std::fs::write(
        dir.join("broken.kicad_dru"),
        "(version 1)\n(rule \"x\" (constraint",
    )
    .unwrap();
    let (exit, log, report) = drc(&dir, &["--kicad-dru", "broken.kicad_dru"]);
    assert_eq!(exit, 0, "{log}");
    let warnings: Vec<&str> = log
        .lines()
        .filter(|l| {
            l.starts_with("warning:") && l.contains("the .kicad_dru file could not be read")
        })
        .collect();
    assert_eq!(warnings.len(), 1, "{log}");
    // The report is the one without the flag.
    assert_eq!(findings(&report, "clearance"), 1, "{report}");

    // The route says the same for the same file.
    let routed = run_in(
        &dir,
        &[
            "-de",
            "board.dsn",
            "-do",
            "out.ses",
            "--kicad-dru",
            "broken.kicad_dru",
        ],
    );
    let route_log = stderr(&routed);
    let route_line = route_log
        .lines()
        .find(|l| l.contains("the .kicad_dru file could not be read"))
        .unwrap_or_else(|| panic!("{route_log}"));
    assert_eq!(warnings[0], route_line);
}

#[test]
fn a_missing_kicad_dru_is_refused_as_the_route_refuses_it() {
    let dir = scratch("drc-kicad-dru-missing");
    std::fs::write(dir.join("board.dsn"), DSN).unwrap();
    let (exit, log, _) = drc(&dir, &["--kicad-dru", "nothing.kicad_dru"]);
    assert_eq!(exit, 1, "{log}");
    assert!(
        log.contains("Couldn't read the --kicad-dru file 'nothing.kicad_dru'"),
        "{log}"
    );
}

#[test]
fn on_a_kicad_pcb_the_flag_is_a_warning_and_the_boards_own_rules_count() {
    let dir = scratch("drc-kicad-dru-pcb");
    let case = repo("crates/freerouting/tests/data/kicad/pcu-k1511/f-rule-clearance");
    for ext in ["kicad_pcb", "kicad_pro", "kicad_dru"] {
        std::fs::copy(case.with_extension(ext), dir.join(format!("f.{ext}"))).unwrap();
    }
    // A rule file of another value must not replace the board's own.
    std::fs::write(
        dir.join("other.kicad_dru"),
        RULE.replace("0.127mm", "0.3mm"),
    )
    .unwrap();
    let run = |extra: &[&str]| {
        let mut args = vec!["-de", "f.kicad_pcb", "-drc", "report.json"];
        args.extend_from_slice(extra);
        let out = run_in(&dir, &args);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        (
            stderr(&out),
            std::fs::read_to_string(dir.join("report.json")).unwrap(),
        )
    };
    let (plain_log, plain) = run(&[]);
    let (log, with_flag) = run(&["--kicad-dru", "other.kicad_dru"]);
    assert!(!plain_log.contains("--kicad-dru"), "{plain_log}");
    assert!(
        log.lines()
            .any(|l| l.starts_with("warning:")
                && l.contains("--kicad-dru only applies to a DSN input")),
        "{log}"
    );
    // The board's own rule (0.127 mm) counts in both: no clearance finding.
    assert_eq!(findings(&plain, "clearance"), 0, "{plain}");
    assert_eq!(findings(&with_flag, "clearance"), 0, "{with_flag}");
}

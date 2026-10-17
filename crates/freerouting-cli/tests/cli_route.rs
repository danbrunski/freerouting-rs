//! The CLI with the router wired in (C07.8): `run_job` calls A13's
//! `run_pipeline`, turns its events into C07.6's log lines and writes the SES
//! from the final board.
//!
//! Every test runs the `freerouting` binary on a copy of a fixture. Pass
//! counts are limited with `-mp` (a work bound), never with time. Java's
//! `MaxPassesSettingRoutingTest` is the model for the pass-line count.

mod common;

use std::process::Output;

use common::*;
use freerouting::dsn::ast::{Pcb, Session, WiringItem};
use freerouting::io::specctra::read_session;
use freerouting::io::{BoardReadResult, read_board};

/// A fresh directory holding a copy of `fixture`.
fn dir_with(name: &str, fixture: &str) -> std::path::PathBuf {
    let dir = scratch(name);
    copy_fixtures(&dir, &[fixture]);
    dir
}

/// One parsed pass line:
/// `Auto-routing pass #N on board '<hash>' was completed in X.XX seconds with
/// score S (U unrouted and V violation[s])`.
#[derive(Debug, PartialEq)]
struct PassLine {
    pass: u32,
    unrouted: u32,
    violations: u32,
}

fn parse_pass_line(line: &str) -> Option<PassLine> {
    let rest = line.strip_prefix("Auto-routing pass #")?;
    let (pass, rest) = rest.split_once(" on board '")?;
    let (hash, rest) = rest.split_once("' was completed in ")?;
    if hash.len() != 32 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let (seconds, rest) = rest.split_once(" seconds with score ")?;
    let (whole, fraction) = seconds.split_once('.')?;
    if fraction.len() != 2 || whole.parse::<u32>().is_err() || fraction.parse::<u32>().is_err() {
        return None;
    }
    let (score, rest) = rest.split_once(" (")?;
    score.parse::<f64>().ok()?;
    let (unrouted, rest) = rest.split_once(" unrouted and ")?;
    let (violations, rest) = rest.split_once(" violation")?;
    let rest = rest.strip_prefix('s').unwrap_or(rest);
    if rest != ")" {
        return None;
    }
    Some(PassLine {
        pass: pass.parse().ok()?,
        unrouted: unrouted.parse().ok()?,
        violations: violations.parse().ok()?,
    })
}

/// `compare.py`'s regex (`docs/development/tools/compare.py:56` in the Java
/// fork), `Auto-routing stage completed: started with (\d+) unrouted.*?final
/// score: [\d.]+ \((\d+) unrouted and (\d+) violations\)`, hand-written because
/// the workspace has no regex crate. Returns `(started, unrouted,
/// violations)`.
fn compare_py_regex(line: &str) -> Option<(u32, u32, u32)> {
    let rest = line.strip_prefix("Auto-routing stage completed: started with ")?;
    let digits = rest.find(|c: char| !c.is_ascii_digit())?;
    let started: u32 = rest[..digits].parse().ok()?;
    let rest = rest[digits..].strip_prefix(" unrouted")?;
    let at = rest.find("final score: ")?;
    let rest = &rest[at + "final score: ".len()..];
    let score_end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    if score_end == 0 {
        return None;
    }
    let rest = rest[score_end..].strip_prefix(" (")?;
    let digits = rest.find(|c: char| !c.is_ascii_digit())?;
    let unrouted: u32 = rest[..digits].parse().ok()?;
    let rest = rest[digits..].strip_prefix(" unrouted and ")?;
    let digits = rest.find(|c: char| !c.is_ascii_digit())?;
    let violations: u32 = rest[..digits].parse().ok()?;
    rest[digits..].strip_prefix(" violations)")?;
    Some((started, unrouted, violations))
}

fn route(dir: &std::path::Path, fixture: &str, out: &str, extra: &[&str]) -> Output {
    let mut args = vec!["-de", fixture, "-do", out];
    args.extend_from_slice(extra);
    run_in(dir, &args)
}

/// Routes `fixture` with `-mp 2` and checks the contract of the item: exit 0,
/// a non-empty SES that `read_session` imports, exactly two pass lines that
/// parse, and a stage summary that `compare.py`'s regex parses.
fn route_and_check(test: &str, fixture: &str) {
    let dir = dir_with(test, fixture);
    let out = route(&dir, fixture, "out.ses", &["-mp", "2"]);
    let log = stderr(&out);
    assert_eq!(code(&out), 0, "{log}");
    assert!(log.contains("finished with state: COMPLETED"), "{log}");

    // The SES is non-empty and imports into the board it was routed from.
    let ses = std::fs::read_to_string(dir.join("out.ses")).unwrap();
    assert!(!ses.is_empty());
    let bytes = std::fs::read(dir.join(fixture)).unwrap();
    let BoardReadResult::Success { mut board, .. } = read_board(&bytes) else {
        panic!("{fixture} does not load");
    };
    let summary = read_session(&mut board, &ses).expect("the SES is a session file");
    assert_eq!(summary.errors_encountered, 0, "{log}");
    assert!(summary.wires_imported > 0, "{log}");

    // `-mp 2`: exactly two pass lines (MaxPassesSettingRoutingTest).
    let pass_lines: Vec<&str> = log
        .lines()
        .filter(|l| l.starts_with("Auto-routing pass #"))
        .collect();
    assert_eq!(pass_lines.len(), 2, "{log}");
    let parsed: Vec<PassLine> = pass_lines
        .iter()
        .map(|l| parse_pass_line(l).unwrap_or_else(|| panic!("unparsable pass line: {l}")))
        .collect();
    assert_eq!(parsed[0].pass, 1);
    assert_eq!(parsed[1].pass, 2);

    // The stage summary comes after the passes and before the save.
    let summary_at = log
        .lines()
        .position(|l| l.starts_with("Auto-routing stage completed"))
        .unwrap_or_else(|| panic!("no stage summary in:\n{log}"));
    let last_pass_at = log
        .lines()
        .enumerate()
        .filter(|(_, l)| l.starts_with("Auto-routing pass #"))
        .map(|(i, _)| i)
        .max()
        .unwrap();
    let saving_at = log
        .lines()
        .position(|l| l.starts_with("Saving output file '"))
        .unwrap();
    assert!(last_pass_at < summary_at && summary_at < saving_at, "{log}");
    let line = log.lines().nth(summary_at).unwrap();
    let (started, unrouted, _violations) =
        compare_py_regex(line).unwrap_or_else(|| panic!("compare.py's regex misses: {line}"));
    assert!(started >= unrouted, "{line}");
}

#[test]
fn routes_issue413() {
    route_and_check("route413", "Issue413-test.dsn");
}

#[test]
fn routes_j2_reference() {
    route_and_check("routej2", "Issue026-J2_reference.dsn");
}

#[test]
fn routes_dac2020_bm01() {
    route_and_check("routebm01", "Issue508-DAC2020_bm01.dsn");
}

/// A Java-format pass line parses, a malformed one does not (so the checks
/// above cannot pass on any text).
#[test]
fn the_pass_line_parser_is_strict() {
    let good = "Auto-routing pass #3 on board '0123456789abcdef0123456789abcdef' was completed \
                in 12.50 seconds with score 944.44 (4 unrouted and 0 violations)";
    assert_eq!(
        parse_pass_line(good),
        Some(PassLine {
            pass: 3,
            unrouted: 4,
            violations: 0
        })
    );
    assert!(parse_pass_line(&good.replace("12.50", "12.5")).is_none());
    assert!(parse_pass_line(&good.replace("0123456789abcdef0123456789abcdef", "abc")).is_none());
    assert!(parse_pass_line(&good.replace("(4 unrouted", "(4 open")).is_none());
}

/// The nets of the DSN class named `class`.
fn class_nets(dsn: &[u8], class: &str) -> Vec<String> {
    let pcb = Pcb::parse(&freerouting::dsn::decode(dsn)).expect("the DSN parses");
    pcb.network
        .expect("the DSN has a network")
        .classes
        .into_iter()
        .find(|c| c.name == class)
        .unwrap_or_else(|| panic!("no class {class}"))
        .nets
}

/// The wires and vias the session adds to the nets in `nets`.
fn items_on(ses: &str, nets: &[String]) -> usize {
    let session = Session::parse(ses).expect("the SES parses");
    session
        .routes
        .map(|r| r.network_out)
        .unwrap_or_default()
        .iter()
        .filter(|n| nets.contains(&n.name))
        .map(|n| {
            n.items
                .iter()
                .filter(|i| matches!(i, WiringItem::Wire(_) | WiringItem::Via(_)))
                .count()
        })
        .sum()
}

/// `-inc 'Power\,Default'` ignores the KiCad class named `Power,Default`
/// (nets `+3V3` and `+5V` of `Issue558-dev-board.dsn`): the session has no
/// wire or via on them. Without the flag the router routes them (control).
#[test]
fn ignore_net_classes_end_to_end() {
    let fixture = "Issue558-dev-board.dsn";
    let dir = dir_with("ignore", fixture);
    let nets = class_nets(&std::fs::read(dir.join(fixture)).unwrap(), "Power,Default");
    assert!(nets.len() >= 2, "{nets:?}");

    let out = route(
        &dir,
        fixture,
        "ignored.ses",
        &["-mp", "2", "-inc", "Power\\,Default"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let ignored = std::fs::read_to_string(dir.join("ignored.ses")).unwrap();
    assert_eq!(items_on(&ignored, &nets), 0, "{}", stderr(&out));
    // The other nets are routed.
    assert!(ignored.contains("(wire"), "{}", stderr(&out));

    let out = route(&dir, fixture, "plain.ses", &["-mp", "2"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let plain = std::fs::read_to_string(dir.join("plain.ses")).unwrap();
    assert!(items_on(&plain, &nets) > 0, "the control routes the class");
}

/// With routing: a far job limit gives the same SES as none, and the same
/// output on a second run; `00:00:00` stops at once (exit 0, a non-empty SES,
/// the warning, `TIMED_OUT`) and does less than the full run.
#[test]
fn timeouts_only_stop_work_with_routing() {
    let fixture = "Issue026-J2_reference.dsn";
    let dir = dir_with("timeouts-route", fixture);
    let base = ["-mp", "3", "--router.optimizer.enabled=false"];
    let run = |extra: &[&str], target: &str| {
        let mut args = base.to_vec();
        args.extend_from_slice(extra);
        route(&dir, fixture, target, &args)
    };
    let none = run(&[], "none.ses");
    assert_eq!(code(&none), 0, "{}", stderr(&none));
    let far = run(&["--router.job_timeout=24:00:00"], "far.ses");
    assert_eq!(code(&far), 0, "{}", stderr(&far));
    let again = run(&[], "again.ses");
    let none_ses = std::fs::read(dir.join("none.ses")).unwrap();
    assert!(!none_ses.is_empty());
    assert_eq!(none_ses, std::fs::read(dir.join("far.ses")).unwrap());
    assert_eq!(none_ses, std::fs::read(dir.join("again.ses")).unwrap());
    assert!(!stderr(&far).contains("wall-clock"));
    assert!(stderr(&far).contains("finished with state: COMPLETED"));
    assert!(stderr(&again).contains("finished with state: COMPLETED"));

    let zero = run(&["--router.job_timeout=00:00:00"], "zero.ses");
    assert_eq!(code(&zero), 0, "{}", stderr(&zero));
    let zero_ses = std::fs::read(dir.join("zero.ses")).unwrap();
    assert!(!zero_ses.is_empty());
    assert!(
        stderr(&zero).contains("warning: stopped by wall-clock limit; result is timing-dependent"),
        "{}",
        stderr(&zero)
    );
    assert!(stderr(&zero).contains("finished with state: TIMED_OUT"));
    assert!(
        zero_ses.len() < none_ses.len(),
        "the stopped run routed less"
    );
}

/// A router that is switched off prints no pass line and no stage summary
/// (the fanout still runs, as in Java's `RoutingPipeline`).
#[test]
fn a_disabled_router_prints_no_stage_summary() {
    let fixture = "Issue413-test.dsn";
    let dir = dir_with("off", fixture);
    let out = route(
        &dir,
        fixture,
        "out.ses",
        &["--router.autorouter.enabled=false"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(!stderr(&out).contains("Auto-routing stage completed"));
    assert!(!stderr(&out).contains("Auto-routing pass #"));
}

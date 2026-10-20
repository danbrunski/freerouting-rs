//! The job flow of the CLI: outputs, exit codes and the log (C07.4 to C07.6).
//!
//! Every test runs the `freerouting` binary. The comparisons with Java's
//! `clirun` dumps use rows whose command line turns the router off; the
//! routing runs are in `cli_route.rs` (C07.8).
//!
//! Java at the pin: `a773a70ec` fixes `cda540f`'s `-do x.dsn` (row 2 wrote a
//! 0-byte file and exited 1 there, `7bef8995f`); Rust writes a real DSN either
//! way.

mod common;

use common::*;
use freerouting::drc::json::JsonValue;
use freerouting::dsn::ast::Pcb;
use freerouting::io::{BoardReadResult, read_board};

const FIXTURE: &str = "Issue413-test.dsn";

fn dir_with_fixture(name: &str) -> std::path::PathBuf {
    let dir = scratch(name);
    copy_fixtures(&dir, &[FIXTURE]);
    dir
}

fn names_in(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Every C01.6 row: Rust's exit code and output files against Java's. Rows 1,
/// 3, 5, 6 and 9 match (row 3's `.drc.json` is the one difference, below).
/// Row 2 writes a real DSN, row 4 (`.xyz`) is refused.
#[test]
fn no_zero_byte_outputs() {
    // Row 1: exit 0 and a SES byte-identical to Java's.
    let row = clirun_row("01-out-ses");
    let dir = dir_with_fixture("row1");
    let out = run_in(&dir, &row.args());
    assert_eq!(code(&out), row.exit_code, "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(dir.join("out.ses")).unwrap(),
        row.text("out.ses").unwrap()
    );

    // Row 2: `a773a70ec` (`7bef8995f`) writes a DSN and exits 0; the old dump
    // from `cda540f` has a 0-byte file and exit 1. Rust writes the DSN either
    // way, so the dump is only compared when it is the new one.
    let row = clirun_row("02-out-dsn");
    if row.exit_code == 0 {
        let size = row.files.iter().find(|(p, _)| p == "out.dsn").map(|f| f.1);
        assert!(
            size.is_some_and(|s| s > 0),
            "Java at the pin writes a real DSN"
        );
    }
    let dir = dir_with_fixture("row2");
    let out = run_in(&dir, &row.args());
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let dsn = std::fs::read(dir.join("out.dsn")).unwrap();
    assert!(!dsn.is_empty());
    assert!(Pcb::parse(&freerouting::dsn::decode(&dsn)).is_ok());

    // Row 3: the SES is Java's; the `.drc.json` is a DRC report (Java wrote a
    // KiCad design JSON under that name, see DIVERGENCES).
    let row = clirun_row("03-out-ses-and-drc");
    let dir = dir_with_fixture("row3");
    let out = run_in(&dir, &row.args());
    assert_eq!(code(&out), row.exit_code, "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(dir.join("out.ses")).unwrap(),
        row.text("out.ses").unwrap()
    );
    let report = JsonValue::parse(&std::fs::read_to_string(dir.join("out.drc.json")).unwrap())
        .expect("the report is JSON");
    assert!(member(&report, "violations").is_some());
    assert!(
        member(&report, "designName").is_none(),
        "not Java's design JSON"
    );

    // Row 4: `.xyz` is refused before any work; nothing is written. Exit
    // code 2 (a usage error), where Java wrote the SES text into out.xyz and
    // exited 0 (DIVERGENCES, "Exit codes").
    let row = clirun_row("04-out-xyz");
    assert_eq!(row.exit_code, 0, "Java wrote the SES text into out.xyz");
    let dir = dir_with_fixture("row4");
    let out = run_in(&dir, &row.args());
    assert_eq!(code(&out), 2);
    assert_eq!(names_in(&dir), [FIXTURE]);
    assert!(stderr(&out).contains("out.xyz"), "{}", stderr(&out));

    // Row 5: no -do. Java exits 1; Rust 2, a usage error (DIVERGENCES,
    // "Exit codes").
    let row = clirun_row("05-no-do");
    assert_eq!(row.exit_code, 1);
    let dir = dir_with_fixture("row5");
    let out = run_in(&dir, &row.args());
    assert_eq!(code(&out), 2);
    assert_eq!(names_in(&dir), [FIXTURE]);
    assert!(
        stderr(&out).contains(
            "Both an input file and an output file must be specified with command line \
             arguments if you are running in CLI mode."
        ),
        "{}",
        stderr(&out)
    );

    // Row 6: a missing input file.
    let row = clirun_row("06-missing-input");
    let dir = scratch("row6");
    let out = run_in(&dir, &row.args());
    assert_eq!(code(&out), row.exit_code);
    assert!(names_in(&dir).is_empty());
    assert!(
        stderr(&out).contains("Couldn't load the input file 'missing.dsn'"),
        "{}",
        stderr(&out)
    );
    assert!(stderr(&out).contains("Couldn't read the input file 'missing.dsn', aborting."));

    // Row 9: help.
    let row = clirun_row("09-help");
    let dir = scratch("row9");
    let out = run_in(&dir, &row.args());
    assert_eq!(code(&out), row.exit_code);
    let help = stdout(&out);
    for line in [
        "USAGE",
        "PARAMETERS",
        "-de <design.dsn>",
        "-do <output.ses>",
        "-mp <number_of_passes>",
    ] {
        assert!(help.contains(line), "{line}");
    }
}

/// C06.8's test: `-do out.dsn` writes the board, and the file reads back.
#[test]
fn do_dsn_writes_the_board() {
    let dir = dir_with_fixture("dsn");
    let out = run_in(
        &dir,
        &[
            "-de",
            FIXTURE,
            "-do",
            "out.dsn",
            "--router.autorouter.enabled=false",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let bytes = std::fs::read(dir.join("out.dsn")).unwrap();
    assert!(!bytes.is_empty());
    assert!(matches!(
        read_board(&bytes),
        BoardReadResult::Success { .. }
    ));
    assert_eq!(
        names_in(&dir),
        [FIXTURE, "out.dsn"],
        "no temporary file is left"
    );
}

#[test]
fn other_formats_are_written_too() {
    let dir = dir_with_fixture("formats");
    let out = run_in(
        &dir,
        &[
            "-de",
            FIXTURE,
            "-do",
            "out.drc.json",
            "out.rules",
            "sub/dir/second.ses",
            "--router.autorouter.enabled=false",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(std::fs::metadata(dir.join("out.drc.json")).unwrap().len() > 0);
    assert!(std::fs::metadata(dir.join("out.rules")).unwrap().len() > 0);
    assert!(
        std::fs::metadata(dir.join("sub/dir/second.ses"))
            .unwrap()
            .len()
            > 0
    );
}

#[test]
fn unsupported_output_formats_are_refused_before_any_work() {
    for name in [
        "out.frb",
        "out.scr",
        "out.json",
        "out.kicad_pcb",
        "out.xyz",
        "out",
    ] {
        let dir = dir_with_fixture("refuse");
        let out = run_in(&dir, &["-de", FIXTURE, "-do", name]);
        assert_eq!(code(&out), 2, "{name}");
        assert_eq!(names_in(&dir), [FIXTURE], "{name}");
        assert!(
            stderr(&out).contains("error: Unsupported output file"),
            "{}",
            stderr(&out)
        );
    }
    // One bad output among good ones stops the whole job.
    let dir = dir_with_fixture("refuse2");
    let out = run_in(&dir, &["-de", FIXTURE, "-do", "a.ses+b.frb"]);
    assert_eq!(code(&out), 2);
    assert_eq!(names_in(&dir), [FIXTURE]);
    // JSON inputs are not read.
    let out = run_in(&dir, &["-de", "x.json", "-do", "a.ses"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("KiCad JSON"), "{}", stderr(&out));
}

#[test]
fn existing_output_is_deleted_at_start() {
    let dir = dir_with_fixture("stale");
    std::fs::write(dir.join("bad.dsn"), "this is not a design").unwrap();
    std::fs::write(dir.join("out.ses"), "stale output").unwrap();
    let out = run_in(&dir, &["-de", "bad.dsn", "-do", "out.ses"]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(!dir.join("out.ses").exists(), "the stale file is gone");
    // A good run replaces it.
    std::fs::write(dir.join("out.ses"), "stale output").unwrap();
    let out = run_in(
        &dir,
        &[
            "-de",
            FIXTURE,
            "-do",
            "out.ses",
            "--router.autorouter.enabled=false",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        std::fs::read_to_string(dir.join("out.ses"))
            .unwrap()
            .starts_with("(session")
    );
}

/// A test hook stops the process between the temporary write and the rename:
/// the target is absent and the temporary file is removed at the next start.
#[test]
fn killed_run_leaves_no_partial_file() {
    let dir = dir_with_fixture("killed");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_freerouting-rs"))
        .args([
            "-de",
            FIXTURE,
            "-do",
            "out.ses",
            "--router.autorouter.enabled=false",
        ])
        .current_dir(&dir)
        .env("FREEROUTING_TEST_ABORT_BEFORE_RENAME", "1")
        .env("XDG_CONFIG_HOME", dir.join("no-config"))
        .output()
        .unwrap();
    assert!(!status.status.success(), "the run was killed");
    assert!(!dir.join("out.ses").exists(), "no partial target");
    let temps: Vec<String> = names_in(&dir)
        .into_iter()
        .filter(|n| n.starts_with(".out.ses.") && n.ends_with(".tmp"))
        .collect();
    assert_eq!(temps.len(), 1, "{:?}", names_in(&dir));
    // The next start removes it, and the run succeeds.
    let out = run_in(
        &dir,
        &[
            "-de",
            FIXTURE,
            "-do",
            "out.ses",
            "--router.autorouter.enabled=false",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(names_in(&dir), [FIXTURE, "out.ses"]);
}

#[test]
fn refused_java_commands_exit_2() {
    let dir = scratch("refused");
    for args in [
        vec!["--compare-boards=a.dsn,b.dsn"],
        vec!["--calculate-benchmark-scores", "--input=a", "--output=b"],
    ] {
        let out = run_in(&dir, &args);
        assert_eq!(code(&out), 2, "{args:?}");
        assert!(stderr(&out).contains("not supported; use freerouting-dev ses-diff"));
    }
}

#[test]
fn missing_arguments_are_errors() {
    let dir = dir_with_fixture("missing");
    let out = run_in(&dir, &[]);
    assert_eq!(code(&out), 2);
    let out = run_in(&dir, &["-do", "a.ses"]);
    assert_eq!(code(&out), 2);
    let out = run_in(&dir, &["-drc"]);
    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("An input file must be specified with -de argument in DRC mode.")
    );
}

#[test]
fn the_drc_mode_still_writes_its_report() {
    let dir = dir_with_fixture("drc");
    let out = run_in(&dir, &["-de", FIXTURE, "-drc", "report.json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(std::fs::metadata(dir.join("report.json")).unwrap().len() > 0);
    let out = run_in(&dir, &["-de", FIXTURE, "-drc"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).trim_start().starts_with('{'));
}

#[test]
fn a_missing_session_file_warns_and_goes_on() {
    let dir = dir_with_fixture("nosession");
    let out = run_in(
        &dir,
        &[
            "-de",
            &format!("{FIXTURE}+nothing.ses"),
            "-do",
            "out.ses",
            "--router.autorouter.enabled=false",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stderr(&out).contains("warning: Session file not found: nothing.ses"));
}

// ---- C07.5 --------------------------------------------------------------

/// A far job limit and none give the same SES; `00:00:00` stops at once, and
/// the job still exits 0 with the best board, a non-empty SES and the
/// warning. (C07.8 repeats this with routing.)
#[test]
fn timeouts_only_stop_work() {
    let dir = dir_with_fixture("timeouts");
    let run = |extra: &[&str], target: &str| {
        let mut args = vec![
            "-de",
            FIXTURE,
            "-do",
            target,
            "--router.optimizer.enabled=false",
        ];
        args.extend_from_slice(extra);
        run_in(&dir, &args)
    };
    let none = run(&[], "none.ses");
    assert_eq!(code(&none), 0, "{}", stderr(&none));
    let far = run(&["--router.job_timeout=24:00:00"], "far.ses");
    assert_eq!(code(&far), 0, "{}", stderr(&far));
    assert_eq!(
        std::fs::read(dir.join("none.ses")).unwrap(),
        std::fs::read(dir.join("far.ses")).unwrap()
    );
    assert!(!stderr(&far).contains("wall-clock"));
    assert!(stderr(&far).contains("finished with state: COMPLETED"));

    let zero = run(&["--router.job_timeout=00:00:00"], "zero.ses");
    assert_eq!(code(&zero), 0, "{}", stderr(&zero));
    let zero_ses = std::fs::read(dir.join("zero.ses")).unwrap();
    assert!(!zero_ses.is_empty());
    assert!(
        stderr(&zero).contains("warning: stopped by wall-clock limit; result is timing-dependent")
    );
    assert!(stderr(&zero).contains("finished with state: TIMED_OUT"));
    // C07.8: the stopped run routed less than the full one (the fanout
    // checks the stop between steps). `cli_route.rs` repeats the timeout
    // checks on a bigger board.
    assert!(zero_ses.len() < std::fs::read(dir.join("none.ses")).unwrap().len());
}

// ---- C07.6 --------------------------------------------------------------

/// `compare.py` (the Java fork, `docs/development/tools/compare.py:56`):
/// `Auto-routing stage completed: started with (\d+) unrouted.*?final score:
/// [\d.]+ \((\d+) unrouted and (\d+) violations\)`, hand-written because the
/// workspace has no regex crate. Returns the two captured numbers.
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

#[test]
fn compare_py_regex_matches() {
    use freerouting::job::log::stage_summary_line;
    assert_eq!(
        compare_py_regex(&stage_summary_line("completed", 9, 3.0, 0.25, 4, 2)),
        Some((9, 4, 2))
    );
    assert_eq!(
        compare_py_regex(&stage_summary_line("completed", 9, 3.0, 0.25, 4, 0)),
        Some((9, 4, 0))
    );
    // A 1-violation line says `violation`, which the regex misses: Java's
    // behaviour, kept.
    assert_eq!(
        compare_py_regex(&stage_summary_line("completed", 9, 3.0, 0.25, 4, 1)),
        None
    );
    // "completed with timeout:" is not "completed:".
    assert_eq!(
        compare_py_regex(&stage_summary_line(
            "completed with timeout",
            9,
            3.0,
            0.25,
            4,
            2
        )),
        None
    );

    // A routed run's stage summary.
    let dir = dir_with_fixture("regex");
    let out = run_in(
        &dir,
        &[
            "-de",
            FIXTURE,
            "-do",
            "out.ses",
            "--router.optimizer.enabled=false",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stderr(&out);
    let summary = text
        .lines()
        .find(|l| l.starts_with("Auto-routing stage completed"))
        .unwrap_or_else(|| panic!("no stage summary in:\n{text}"));
    let (_, unrouted, violations) =
        compare_py_regex(summary).unwrap_or_else(|| panic!("compare.py's regex misses: {summary}"));
    // C07.8: the router routes the board's one open connection.
    assert_eq!(unrouted, 0);
    assert_eq!(violations, 0);
}

#[test]
fn the_log_has_no_timestamps_and_prefixed_levels() {
    let dir = dir_with_fixture("log");
    let out = run_in(
        &dir,
        &["-de", FIXTURE, "-do", "out.ses", "-ll", "debug", "-l", "de"],
    );
    let text = stderr(&out);
    assert!(
        text.contains("debug: Ignored command line argument: -l"),
        "{text}"
    );
    assert!(
        text.contains("Auto-routing stage completed: started with "),
        "{text}"
    );
    assert!(text.lines().any(|l| l.starts_with("Saving output file '")));
    let quiet = run_in(&dir, &["-de", FIXTURE, "-do", "out.ses", "-ll", "off"]);
    assert_eq!(code(&quiet), 0);
    assert!(stderr(&quiet).is_empty(), "{}", stderr(&quiet));
}

// ---- settings reaching the job -----------------------------------------

#[test]
fn the_settings_file_and_its_source_are_logged() {
    let dir = dir_with_fixture("config");
    std::fs::write(
        dir.join("freerouting.json"),
        r#"{ "router": { "scoring": { "start_ripup_costs": 1500 } } }"#,
    )
    .unwrap();
    let out = run_in(
        &dir,
        &[
            "--settings",
            "freerouting.json",
            "-de",
            FIXTURE,
            "-do",
            "out.ses",
            "--router.autorouter.enabled=false",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("Effective start_ripup_costs: 1500 (source: config file)"),
        "{}",
        stderr(&out)
    );
    let out = run_in(
        &dir,
        &["--settings", "missing.json", "-de", FIXTURE, "-do", "o.ses"],
    );
    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("Settings file not found"),
        "{}",
        stderr(&out)
    );
}

/// `-inc Power,Default` follows Java's split and warns when the parts joined
/// are a class of the board; `-inc 'Power\,Default'` is the class.
#[test]
fn comma_split_warns_when_it_looks_like_a_mistake() {
    let dir = scratch("inc");
    copy_fixtures(&dir, &["Issue558-dev-board.dsn"]);
    let base = [
        "-de",
        "Issue558-dev-board.dsn",
        "-do",
        "out.ses",
        "--router.autorouter.enabled=false",
    ];
    let mut args = base.to_vec();
    args.extend(["-inc", "Power,Default"]);
    let out = run_in(&dir, &args);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("warning: ignore_net_classes names 'Power' and 'Default' separately"),
        "{}",
        stderr(&out)
    );
    let mut args = base.to_vec();
    args.extend(["-inc", "Power\\,Default"]);
    let out = run_in(&dir, &args);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(!stderr(&out).contains("separately"), "{}", stderr(&out));
    assert!(
        !stderr(&out).contains("no net class named"),
        "{}",
        stderr(&out)
    );
}

/// `AutorouteBatchLoop`: with every layer disabled the router cannot start.
/// The job is `Terminated`, nothing is written and the exit code is 1.
#[test]
fn no_routable_layer_terminates_the_job() {
    let dir = dir_with_fixture("nolayer");
    let out = run_in(
        &dir,
        &[
            "-de",
            FIXTURE,
            "-do",
            "out.ses",
            "--router.layers.routable=false,false",
        ],
    );
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(stderr(&out).contains("Cannot start autorouter: all layers are disabled."));
    assert!(stderr(&out).contains("terminated due to an error"));
    assert_eq!(names_in(&dir), [FIXTURE]);
}

/// `RoutingJobSchedulerActionThreadTest.testRoutingJobSchedulerActionThreadEmitsAnalytics`
/// without the analytics: a 0-pass run is `Completed`.
#[test]
fn a_zero_pass_run_completes() {
    let dir = dir_with_fixture("zeropass");
    let out = run_in(&dir, &["-de", FIXTURE, "-do", "out.ses", "-mp", "0"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stderr(&out).contains("finished with state: COMPLETED"));
}

//! K15.8: `--stdin-commands off` and the default.
//!
//! A route reads `stop` and `stop-router` from standard input and consumes
//! what it reads, so in a shell `while read` loop it eats the loop's lines.
//! `--stdin-commands off` keeps the reader from starting. The tests run the
//! binary under `sh`, which then runs `cat` on the same standard input: the
//! lines `cat` still sees are the ones the binary did not read.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use common::*;

const SMALL: &str = "Issue026-J2_reference.dsn";
const BIG: &str = "Issue508-DAC2020_bm01.dsn";

/// Runs the binary and then `cat` on one standard input that carries
/// `lines`. Returns the output of `cat` (after the marker), the log and the
/// binary's exit code.
fn lines_left_after(name: &str, extra: &[&str], lines: &[&str]) -> (Vec<String>, String, i32) {
    let dir = scratch(name);
    copy_fixtures(&dir, &[SMALL]);
    let script = format!(
        "\"$BIN\" -de {SMALL} -do out.ses -mp 1 {} 2>err.log; echo \"EXIT $?\"; echo ---; cat",
        extra.join(" ")
    );
    let mut child = Command::new("sh")
        .args(["-c", &script])
        .env("BIN", env!("CARGO_BIN_EXE_freerouting-rs"))
        .env("XDG_CONFIG_HOME", dir.join("no-config"))
        .env_remove("FREEROUTING_TEST_ABORT_BEFORE_RENAME")
        .env_remove("FREEROUTING_TEST_INTERRUPT")
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("sh runs");
    let mut stdin = child.stdin.take().unwrap();
    for line in lines {
        writeln!(stdin, "{line}").unwrap();
    }
    stdin.flush().unwrap();
    // The write end stays open until the binary has finished: `cat` then
    // sees the end of input.
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut exit = String::new();
    out.read_line(&mut exit).unwrap();
    drop(stdin);
    let mut rest = Vec::new();
    for line in out.lines() {
        rest.push(line.unwrap());
    }
    child.wait().unwrap();
    let code = exit
        .trim()
        .strip_prefix("EXIT ")
        .and_then(|c| c.parse().ok())
        .expect("the exit marker");
    let marker = rest.iter().position(|l| l == "---").expect("the marker");
    let log = std::fs::read_to_string(dir.join("err.log")).unwrap_or_default();
    (rest.split_off(marker + 1), log, code)
}

#[test]
fn with_stdin_commands_off_three_lines_stay_unread() {
    let (left, log, code) = lines_left_after(
        "stdin-off",
        &["--stdin-commands", "off"],
        &["alpha", "beta", "gamma"],
    );
    assert_eq!(code, 0, "{log}");
    assert_eq!(left, ["alpha", "beta", "gamma"], "{log}");
    assert!(log.contains("finished with state: COMPLETED"), "{log}");
}

/// Standard input closed (a `< /dev/null` or a closed pipe): the run is the
/// same with the flag.
#[test]
fn with_stdin_commands_off_and_stdin_closed_the_route_completes() {
    let dir = scratch("stdin-off-closed");
    copy_fixtures(&dir, &[SMALL]);
    let output = Command::new(env!("CARGO_BIN_EXE_freerouting-rs"))
        .args(["-de", SMALL, "-do", "out.ses", "-mp", "1"])
        .args(["--stdin-commands", "off"])
        .current_dir(&dir)
        .env("XDG_CONFIG_HOME", dir.join("no-config"))
        .env_remove("FREEROUTING_TEST_ABORT_BEFORE_RENAME")
        .env_remove("FREEROUTING_TEST_INTERRUPT")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stderr(&output).contains("finished with state: COMPLETED"));
    assert!(dir.join("out.ses").metadata().unwrap().len() > 0);
}

#[test]
fn the_equals_form_works_too() {
    let (left, _, code) = lines_left_after("stdin-off-eq", &["--stdin-commands=off"], &["x", "y"]);
    assert_eq!(code, 0);
    assert_eq!(left, ["x", "y"]);
}

#[test]
fn by_default_the_route_reads_standard_input() {
    let (left, log, code) = lines_left_after("stdin-default", &[], &["alpha", "beta", "gamma"]);
    assert_eq!(code, 0, "{log}");
    assert!(left.is_empty(), "the reader consumed the lines: {left:?}");
}

#[test]
fn a_bad_value_is_a_usage_error() {
    let dir = scratch("stdin-bad");
    copy_fixtures(&dir, &[SMALL]);
    let output = run_in(
        &dir,
        &["-de", SMALL, "-do", "out.ses", "--stdin-commands", "maybe"],
    );
    assert_eq!(code(&output), 2, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("--stdin-commands: 'maybe' is not a mode"),
        "{}",
        stderr(&output)
    );
}

/// With the flag spelled out as `on`, `stop` on standard input still stops
/// the run after the first pass and the board is written.
#[test]
fn stop_still_stops_with_stdin_commands_on() {
    let dir = scratch("stdin-on-stop");
    copy_fixtures(&dir, &[BIG]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_freerouting-rs"))
        .args([
            "-de",
            BIG,
            "-do",
            "out.ses",
            "-mp",
            "50",
            "--progress",
            "json",
            "--stdin-commands",
            "on",
        ])
        .current_dir(&dir)
        .env("XDG_CONFIG_HOME", dir.join("no-config"))
        .env_remove("FREEROUTING_TEST_ABORT_BEFORE_RENAME")
        .env_remove("FREEROUTING_TEST_INTERRUPT")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    let mut stdin = child.stdin.take().unwrap();
    let reader = BufReader::new(child.stderr.take().unwrap());
    let mut sent = false;
    let mut passes = 0;
    for line in reader.lines() {
        let line = line.unwrap();
        if !sent && line.contains("\"phase\":\"route\"") {
            writeln!(stdin, "stop").unwrap();
            stdin.flush().unwrap();
            sent = true;
        }
        if line.contains("Auto-routing pass #") {
            passes += 1;
        }
    }
    assert!(sent, "no route event");
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert!(passes <= 1, "{passes} passes");
    assert!(dir.join("out.ses").metadata().unwrap().len() > 0);
}

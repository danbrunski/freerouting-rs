//! The long-form aliases and the reserved seed on the built binary (C08.6).
//! The patch-level tables are in `args_java.rs`.

mod common;

use common::*;

const FIXTURE: &str = "Issue026-J2_reference.dsn";

fn route(name: &str, extra: &[&str]) -> (std::process::Output, std::path::PathBuf) {
    let dir = scratch(name);
    copy_fixtures(&dir, &[FIXTURE]);
    let mut args = vec!["-de", FIXTURE, "-do", "out.ses"];
    args.extend_from_slice(extra);
    (run_in(&dir, &args), dir)
}

/// `--threads` never changes a result (D2).
#[test]
fn threads_1_and_8_give_the_same_ses() {
    let (one, dir1) = route("threads-1", &["-mp", "3", "--threads", "1"]);
    let (eight, dir8) = route("threads-8", &["-mp", "3", "--threads", "8"]);
    assert_eq!(code(&one), 0, "{}", stderr(&one));
    assert_eq!(code(&eight), 0, "{}", stderr(&eight));
    let a = std::fs::read(dir1.join("out.ses")).unwrap();
    let b = std::fs::read(dir8.join("out.ses")).unwrap();
    assert!(!a.is_empty());
    assert!(a == b, "the SES files differ");
}

#[test]
fn max_passes_limits_the_passes_like_mp() {
    let (long, _) = route("maxpasses-long", &["--max-passes", "1"]);
    let (short, _) = route("maxpasses-short", &["-mp", "1"]);
    let passes = |o: &std::process::Output| {
        stderr(o)
            .lines()
            .filter(|l| l.starts_with("Auto-routing pass #"))
            .count()
    };
    assert_eq!(code(&long), 0, "{}", stderr(&long));
    assert_eq!(passes(&long), 1, "{}", stderr(&long));
    assert_eq!(passes(&short), 1);
}

#[test]
fn timeout_and_seed_zero_are_accepted() {
    let (out, _) = route(
        "timeout-ok",
        &["--max-passes", "2", "--timeout", "10m", "--seed", "0"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(!stderr(&out).contains("warning"), "{}", stderr(&out));
    // Over 24 hours is capped, with a warning.
    let (out, _) = route("timeout-cap", &["--max-passes", "2", "--timeout", "25h"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("warning: --timeout 25h is longer than 24 hours"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_bad_value_is_a_usage_error_with_a_message() {
    let (out, dir) = route("seed-5", &["--seed", "5"]);
    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("seeded variation is not supported yet"),
        "{}",
        stderr(&out)
    );
    assert!(!dir.join("out.ses").exists());
    let (out, _) = route("timeout-x", &["--timeout", "x"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("--timeout: 'x' is not a duration"));
}

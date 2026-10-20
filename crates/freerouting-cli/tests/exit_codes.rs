//! Exit codes 0/1/2/130 of the built binary (C08.1).
//!
//! 0 is an output written; 1 a failure with no output; 2 a usage error found
//! before any work; 130 an aborted run (`stop.rs` covers it). Java exits 1
//! for the usage errors (DIVERGENCES, "Exit codes").

mod common;

use common::*;

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

/// Q12's portfolio keys were removed (2026-10-20, Q12.5's fail branch): a
/// command line or settings file that sets one is a usage error that names
/// the valid `router.autorouter` keys, and nothing is written.
#[test]
fn removed_portfolio_settings_are_usage_errors() {
    let dir = dir_with_fixture("exit-removed");
    for flag in [
        "--router.autorouter.portfolio=4",
        "--router.autorouter.restarts=2",
        "--router.autorouter.portfolio_memory_mb=512",
    ] {
        let out = run_in(&dir, &["-de", FIXTURE, "-do", "o.ses", flag]);
        assert_eq!(code(&out), 2, "{flag}: {}", stderr(&out));
        let key = flag.trim_start_matches("--").split('=').next().unwrap();
        let text = format!("{}{}", stdout(&out), stderr(&out));
        assert!(
            text.contains(&format!("{key} was removed on 2026-10-20")),
            "{flag}: {text}"
        );
        assert!(
            text.contains("valid router.autorouter settings: ") && text.contains("max_passes"),
            "{flag}: {text}"
        );
        assert_eq!(names_in(&dir), [FIXTURE], "{flag}");
    }
    std::fs::write(
        dir.join("portfolio.json"),
        r#"{"router": {"autorouter": {"portfolio": 4}}}"#,
    )
    .unwrap();
    let out = run_in(
        &dir,
        &[
            "--settings",
            "portfolio.json",
            "-de",
            FIXTURE,
            "-do",
            "o.ses",
        ],
    );
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        text.contains("router.autorouter.portfolio was removed on 2026-10-20"),
        "{text}"
    );
    assert!(!dir.join("o.ses").exists());
}

#[test]
fn usage_errors_exit_2() {
    let dir = dir_with_fixture("exit-usage");
    // No arguments.
    assert_eq!(code(&run_in(&dir, &[])), 2);
    // No -do.
    let out = run_in(&dir, &["-de", FIXTURE]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    // An unsupported output format: nothing is created.
    let out = run_in(&dir, &["-de", FIXTURE, "-do", "out.xyz"]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert_eq!(names_in(&dir), [FIXTURE]);
    // A missing settings file.
    let out = run_in(
        &dir,
        &["--settings", "missing.json", "-de", FIXTURE, "-do", "o.ses"],
    );
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert_eq!(names_in(&dir), [FIXTURE]);
    // A malformed one.
    std::fs::write(dir.join("bad.json"), "{ not json").unwrap();
    let out = run_in(
        &dir,
        &["--settings", "bad.json", "-de", FIXTURE, "-do", "o.ses"],
    );
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    // The DRC mode without -de.
    assert_eq!(code(&run_in(&dir, &["-drc"])), 2);
    // A Java command that is refused.
    assert_eq!(code(&run_in(&dir, &["--compare-boards=a,b"])), 2);
    // A bad value for a C08 flag.
    for flag in [
        ["--progress", "xml"],
        ["--timeout", "x"],
        ["--max-passes", "many"],
        ["--threads", "0"],
        ["--seed", "5"],
    ] {
        let out = run_in(&dir, &["-de", FIXTURE, "-do", "o.ses", flag[0], flag[1]]);
        assert_eq!(code(&out), 2, "{flag:?}: {}", stderr(&out));
        assert_eq!(names_in(&dir), [FIXTURE, "bad.json"], "{flag:?}");
    }
}

#[test]
fn failures_with_no_output_exit_1() {
    let dir = dir_with_fixture("exit-failure");
    // A missing input file.
    let out = run_in(&dir, &["-de", "missing.dsn", "-do", "o.ses"]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    // A DSN that does not parse.
    std::fs::write(dir.join("bad.dsn"), "this is not a design").unwrap();
    let out = run_in(&dir, &["-de", "bad.dsn", "-do", "o.ses"]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(!dir.join("o.ses").exists());
}

#[test]
fn an_output_exits_0() {
    let dir = dir_with_fixture("exit-ok");
    let out = run_in(
        &dir,
        &[
            "-de",
            FIXTURE,
            "-do",
            "o.ses",
            "--router.autorouter.enabled=false",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(dir.join("o.ses").metadata().unwrap().len() > 0);
    // -help prints and exits 0.
    let out = run_in(&dir, &["-help"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("EXIT CODES"), "{}", stdout(&out));
}

//! P02.3: a routing job writes its outputs and nothing else.
//!
//! Java writes no intermediate session either unless
//! `autorouter.save_intermediate_stages` is set, and then it only fires a
//! board snapshot event (`AutorouteBatchLoop.java:468-470` at `a773a70ec`).
//! The port computes the board statistics once per pass and the progress
//! payload only while someone listens (`autoroute::batch::pass`); this test
//! pins the file side: after a three-pass route the job directory holds the
//! design and the session, nothing more.

mod common;

use std::collections::BTreeSet;

use common::*;

#[test]
fn a_routing_job_writes_no_intermediate_files() {
    let dir = scratch("no-intermediate");
    copy_fixtures(&dir, &["Issue413-test.dsn"]);
    let output = run_in(
        &dir,
        &["-de", "Issue413-test.dsn", "-do", "out.ses", "-mp", "3"],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let files: BTreeSet<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let expected: BTreeSet<String> = ["Issue413-test.dsn", "out.ses"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(files, expected);
    assert!(
        stderr(&output).contains("Auto-routing pass #1"),
        "{}",
        stderr(&output)
    );
}

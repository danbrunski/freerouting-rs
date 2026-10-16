//! A01.1: the probe writer and the connection sampler against Java.
//!
//! - `probe_selftest_matches_java`: both writers print the same fixed values
//!   (`-0.0`, `0.1`, `Double.MIN_VALUE`, `Integer.MIN_VALUE`, an empty and a
//!   nested list, string escapes, the FNV-1a hash).
//! - `probe_connections_match_java`: the sampled connections (net, item,
//!   plane flag, state, start and destination ids, per variant) on every
//!   board of the fixture ladder.
//! - `digest_mismatch_prints_the_probe_command`: a forced mismatch against a
//!   digest names the `probe.sh` command that writes Java's full output.
//!
//! References: `fixtures/reference/probe/{selftest,connections}/`, written by
//! `tools/java-reference/probes/autoroute/generate.sh` at the pin.

mod autoroute_common;

use autoroute_common::{
    board_name, compare_lines, compare_probe, ids, kind_fixtures, load, sample, sample_line, writer,
};
use freerouting::autoroute::probe::{self, ProbeValue, ProbeWriter, fnv64};

fn selftest_lines() -> Vec<String> {
    let mut out = writer("selftest", "selftest");
    out.record(&[
        ("stage", "ints".into()),
        ("zero", 0.into()),
        ("intMin", i32::MIN.into()),
        ("intMax", i32::MAX.into()),
        ("longMin", i64::MIN.into()),
    ]);
    out.record(&[
        ("stage", "doubles".into()),
        ("negZero", (-0.0).into()),
        ("tenth", 0.1.into()),
        ("minValue", ProbeValue::Bits(f64::from_bits(1))),
        ("minNormal", f64::MIN_POSITIVE.into()),
        ("max", f64::MAX.into()),
        ("nan", f64::NAN.into()),
        ("negInf", f64::NEG_INFINITY.into()),
        ("intMaxAsDouble", f64::from(i32::MAX).into()),
    ]);
    out.record(&[
        ("stage", "other".into()),
        ("yes", true.into()),
        ("no", false.into()),
        ("text", "quote\" backslash\\ tab\t ctl\u{1} \u{e9}".into()),
    ]);
    out.record(&[
        ("stage", "lists".into()),
        ("empty", ProbeValue::List(Vec::new())),
        (
            "nested",
            ProbeValue::List(vec![
                ProbeValue::ints([1, -2]),
                ProbeValue::List(Vec::new()),
                ProbeValue::List(vec![ProbeValue::Bits(0.5), true.into()]),
            ]),
        ),
    ]);
    out.record(&[
        ("stage", "fnv".into()),
        ("ab", format!("{:016x}", fnv64(&["a", "b"])).into()),
        ("none", format!("{:016x}", fnv64::<&str>(&[])).into()),
    ]);
    out.into_lines()
}

#[test]
fn probe_selftest_matches_java() {
    compare_probe("selftest", "-", &selftest_lines()).unwrap();
}

/// The `connections` lines of one board: the sample line, then every
/// sampled connection with ripup off, then all again with ripup on.
fn connections_lines(dsn: &str) -> Vec<String> {
    let loaded = load(dsn);
    let conns = sample(&loaded.board);
    let mut out = writer("connections", &board_name(dsn));
    sample_line(&mut out, &loaded.board, &conns);
    // The ripup values of each variant without building the control (Java's
    // throws on boards without a via rule): the constructor's defaults with
    // ripup off, pass 1's with ripup on.
    let start_ripup_costs = loaded.router.start_ripup_costs();
    // PARITY: the variant order of the sampler: ripup off, then on.
    for (variant, ripup) in [("off", false), ("on", true)] {
        for conn in &conns {
            let ripup_costs = if ripup { start_ripup_costs } else { 1000 };
            let state = conn.state.map_or("", |s| s.java_name());
            out.record(&[
                ("stage", format!("{variant}:{}", conn.net).into()),
                ("net", conn.net.into()),
                ("item", conn.item.0.into()),
                ("plane", conn.plane.into()),
                ("state", state.into()),
                ("ripupAllowed", ripup.into()),
                ("ripupCosts", ripup_costs.into()),
                ("start", ids(&conn.start)),
                ("dest", ids(&conn.dest)),
            ]);
        }
    }
    out.into_lines()
}

#[test]
fn probe_connections_match_java() {
    let mut failures = Vec::new();
    let fixtures = kind_fixtures("connections");
    assert_eq!(fixtures.len(), 17, "the fixture ladder has 17 boards");
    for dsn in &fixtures {
        if let Err(e) = compare_probe("connections", dsn, &connections_lines(dsn)) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn digest_mismatch_prints_the_probe_command() {
    let mut java = ProbeWriter::new();
    java.header("forced", "forced-board", "a773a70ec");
    java.record(&[("stage", "one".into()), ("x", 1.into())]);
    let java_digest = probe::digest(java.lines());
    let mut rust = writer("forced", "forced-board");
    rust.record(&[("stage", "one".into()), ("x", 2.into())]);
    let report = compare_lines(
        "forced",
        "forced-board",
        "fixtures/forced-board.dsn",
        &java_digest,
        true,
        rust.lines(),
    )
    .unwrap_err();
    assert!(report.contains("line 2 differs"), "{report}");
    assert!(
        report.contains("tools/java-reference/probe.sh")
            && report.contains("AutorouteProbe.java forced 'fixtures/forced-board.dsn'"),
        "{report}"
    );
    assert!(report.contains("forced-board.rust.jsonl"), "{report}");
    // The same lines as Java's pass.
    compare_lines(
        "forced",
        "forced-board",
        "fixtures/forced-board.dsn",
        &java_digest,
        true,
        &[rust.lines()[0].clone(), java.lines()[1].clone()],
    )
    .unwrap();
}

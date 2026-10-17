//! Q02.1 and Q02.2 at the command line: the `widths` key of the result
//! manifest (`--router.special.diagnostics=widths`), and the strict-sizes
//! exit status (`--router.neck_down.strict=true`, exit 3 exactly when the
//! report has under-width length).
//!
//! Fixtures: `q02-pinch.dsn` (a pre-routed 1.5 mm net with a 0.4 mm pinch)
//! and Q03's `q03-qfn-0p5.dsn` (a 1.5 mm net that can't leave its QFN pad
//! at full width, so with the neck-downs refused it ships no copper).

mod common;

use common::*;
use freerouting::drc::json::JsonValue;

const PINCH: &str = "crates/freerouting/tests/data/quality/q02-pinch.dsn";
const QFN: &str = "crates/freerouting/tests/data/quality/q03-qfn-0p5.dsn";

/// Routes `dsn` in `dir` with `extra`; returns the exit code and manifest.
fn run(dir: &std::path::Path, dsn: &str, extra: &[&str]) -> (i32, JsonValue) {
    std::fs::copy(repo(dsn), dir.join("board.dsn")).unwrap();
    let mut args = vec![
        "-de",
        "board.dsn",
        "-do",
        "out.ses",
        "-mp",
        "2",
        "--result-json",
        "result.json",
    ];
    args.extend_from_slice(extra);
    let out = run_in(dir, &args);
    assert!(dir.join("out.ses").is_file(), "{}", stderr(&out));
    let text = std::fs::read_to_string(dir.join("result.json")).expect("the manifest exists");
    (
        code(&out),
        JsonValue::parse(&text).expect("the manifest is JSON"),
    )
}

fn under_mm(widths: &JsonValue) -> f64 {
    match member(widths, "under_mm") {
        Some(JsonValue::Number(n)) => n.parse().unwrap(),
        other => panic!("no under_mm: {other:?}"),
    }
}

#[test]
fn the_manifest_carries_the_widths_only_with_the_diagnostic() {
    let with = scratch("result-widths-on");
    let (code_on, on) = run(&with, PINCH, &["--router.special.diagnostics=widths"]);
    assert_eq!(code_on, 0, "not strict: success");
    let widths = member(&on, "widths").expect("a widths key");
    assert_eq!(
        string_of(member(widths, "schema")).as_deref(),
        Some("freerouting-widths/1")
    );
    assert!(under_mm(widths) >= 2.0, "at least the pinch");
    let Some(JsonValue::Array(nets)) = member(widths, "nets") else {
        panic!("no nets");
    };
    let names: Vec<String> = nets
        .iter()
        .filter_map(|n| string_of(member(n, "net_name")))
        .collect();
    assert_eq!(names, ["PWR", "PWR2"]);

    let without = scratch("result-widths-off");
    let (code_off, off) = run(&without, PINCH, &[]);
    assert_eq!(code_off, 0);
    assert!(member(&off, "widths").is_none());
    // Reports change nothing: the same session either way.
    assert_eq!(
        std::fs::read(with.join("out.ses")).unwrap(),
        std::fs::read(without.join("out.ses")).unwrap()
    );
}

#[test]
fn strict_sizes_exit_3_exactly_when_copper_ships_under_width() {
    let pinch = scratch("result-widths-strict-pinch");
    let (code, manifest) = run(&pinch, PINCH, &["--router.neck_down.strict=true"]);
    let widths = member(&manifest, "widths").expect("strict adds the report");
    assert!(under_mm(widths) > 0.0);
    assert_eq!(code, 3);
    assert!(matches!(
        member(widths, "strict"),
        Some(JsonValue::Bool(true))
    ));
    match member(&manifest, "exit_code") {
        Some(JsonValue::Number(n)) => assert_eq!(n, "3"),
        other => panic!("exit_code: {other:?}"),
    }

    let qfn = scratch("result-widths-strict-qfn");
    let (code, manifest) = run(&qfn, QFN, &["--router.neck_down.strict=true"]);
    let widths = member(&manifest, "widths").expect("strict adds the report");
    assert_eq!(under_mm(widths), 0.0);
    assert_eq!(code, 0);
}

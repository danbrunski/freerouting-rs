//! Q05.1: the `rip_stats` key of the result manifest
//! (`--router.special.diagnostics=rip-stats`), absent without the
//! diagnostic, with the same SES either way.

mod common;

use common::*;
use freerouting::drc::json::JsonValue;

const DSN: &str = "crates/freerouting/tests/data/quality/q06-sole-wall.dsn";

fn manifest(dir: &std::path::Path, extra: &[&str]) -> JsonValue {
    std::fs::copy(repo(DSN), dir.join("board.dsn")).unwrap();
    let mut args = vec![
        "-de",
        "board.dsn",
        "-do",
        "out.ses",
        "-mp",
        "3",
        "--result-json",
        "result.json",
    ];
    args.extend_from_slice(extra);
    let out = run_in(dir, &args);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = std::fs::read_to_string(dir.join("result.json")).expect("the manifest exists");
    JsonValue::parse(&text).expect("the manifest is JSON")
}

#[test]
fn the_manifest_carries_the_rip_stats_only_with_the_diagnostic() {
    let with = scratch("result-rip-stats-on");
    let manifest_on = manifest(&with, &["--router.special.diagnostics=verdicts,rip-stats"]);
    assert!(member(&manifest_on, "verdicts").is_some());
    let stats = member(&manifest_on, "rip_stats").expect("a rip_stats key");
    assert_eq!(
        string_of(member(stats, "schema")).as_deref(),
        Some("freerouting-rip-stats/1")
    );
    let Some(JsonValue::Array(top)) = member(stats, "top_nets") else {
        panic!("no top_nets array");
    };
    assert_eq!(top.len(), 1);
    assert_eq!(string_of(member(&top[0], "net_name")).as_deref(), Some("B"));
    assert_eq!(
        member(&top[0], "share"),
        Some(&JsonValue::Number("1.0000".into()))
    );

    let without = scratch("result-rip-stats-off");
    let manifest_off = manifest(&without, &[]);
    assert!(member(&manifest_off, "rip_stats").is_none());
    // Reports change nothing: the same session either way.
    assert_eq!(
        std::fs::read(with.join("out.ses")).unwrap(),
        std::fs::read(without.join("out.ses")).unwrap()
    );
}

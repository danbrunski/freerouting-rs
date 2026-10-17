//! Q06.4: the `verdicts` key of the result manifest
//! (`--router.special.diagnostics=verdicts`), checked against the engine's
//! own report of the same run, and absent without the diagnostic.

mod common;

use common::*;
use freerouting::drc::json::JsonValue;

const DSN: &str = "crates/freerouting/tests/data/quality/q06-fixed-cage.dsn";

fn manifest(dir: &std::path::Path, extra: &[&str]) -> JsonValue {
    std::fs::copy(repo(DSN), dir.join("board.dsn")).unwrap();
    let mut args = vec![
        "-de",
        "board.dsn",
        "-do",
        "out.ses",
        "-mp",
        "1",
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
fn the_manifest_carries_the_verdicts_only_with_the_diagnostic() {
    let with = scratch("result-verdicts-on");
    let manifest_on = manifest(&with, &["--router.special.diagnostics=verdicts"]);
    let verdicts = member(&manifest_on, "verdicts").expect("a verdicts key");
    assert_eq!(
        string_of(member(verdicts, "schema")).as_deref(),
        Some("freerouting-verdicts/1")
    );
    let Some(JsonValue::Array(connections)) = member(verdicts, "connections") else {
        panic!("no connections array");
    };
    assert_eq!(connections.len(), 1);
    assert_eq!(
        string_of(member(&connections[0], "verdict")).as_deref(),
        Some("unrouted")
    );
    assert_eq!(
        string_of(member(&connections[0], "net_name")).as_deref(),
        Some("A")
    );

    let without = scratch("result-verdicts-off");
    let manifest_off = manifest(&without, &[]);
    assert!(member(&manifest_off, "verdicts").is_none());
    // Reports change nothing: the same session either way.
    assert_eq!(
        std::fs::read(with.join("out.ses")).unwrap(),
        std::fs::read(without.join("out.ses")).unwrap()
    );
}

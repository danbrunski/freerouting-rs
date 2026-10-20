//! `--version` and `--capabilities` (C08.2).

mod common;

use common::*;
use freerouting::drc::json::JsonValue;
use freerouting_cli::args::{EXTRA_FLAGS, FLAGS, LONG_FLAGS};

/// `^freerouting-rs \d+\.\d+\.\d+ \(git ([0-9a-f]{12}|unknown)\)$`, by hand (the
/// workspace has no regex crate in the CLI).
fn matches_version_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("freerouting-rs ") else {
        return false;
    };
    let Some((version, rest)) = rest.split_once(" (git ") else {
        return false;
    };
    let parts: Vec<&str> = version.split('.').collect();
    let numeric = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    let Some(sha) = rest.strip_suffix(')') else {
        return false;
    };
    let sha_ok =
        sha == "unknown" || (sha.len() == 12 && sha.bytes().all(|b| b.is_ascii_hexdigit()));
    numeric && sha_ok
}

#[test]
fn version_prints_one_line() {
    let dir = scratch("version");
    let out = run_in(&dir, &["--version"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.ends_with('\n'));
    assert!(matches_version_line(text.trim_end()), "{text}");
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "the crate version: {text}"
    );
    assert!(stderr(&out).is_empty(), "{}", stderr(&out));
}

#[test]
fn the_version_line_matcher_rejects_other_lines() {
    assert!(matches_version_line(
        "freerouting-rs 0.1.0 (git abcdef123456)"
    ));
    assert!(matches_version_line("freerouting-rs 12.3.45 (git unknown)"));
    assert!(!matches_version_line(
        "freerouting-rs 0.1 (git abcdef123456)"
    ));
    assert!(!matches_version_line(
        "freerouting-rs 0.1.0 (git abcdef12345)"
    ));
    assert!(!matches_version_line(
        "freerouting-rs 0.1.0 (git ABCDEFG12345)"
    ));
    assert!(!matches_version_line("freerouting-rs 0.1.0"));
    // The executable's old name, which Java's launcher also has.
    assert!(!matches_version_line(
        "freerouting 0.1.0 (git abcdef123456)"
    ));
}

fn names_of(value: Option<&JsonValue>) -> Vec<String> {
    match value {
        Some(JsonValue::Array(items)) => items
            .iter()
            .filter_map(|f| string_of(member(f, "name")))
            .collect(),
        _ => panic!("no flags array"),
    }
}

#[test]
fn capabilities_is_json_and_lists_every_flag() {
    let dir = scratch("capabilities");
    let out = run_in(&dir, &["--capabilities"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let doc = JsonValue::parse(&stdout(&out)).expect("valid JSON");

    assert_eq!(
        string_of(member(&doc, "name")).as_deref(),
        Some("freerouting-rs")
    );
    assert_eq!(
        string_of(member(&doc, "version")).as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert!(string_of(member(&doc, "git")).is_some());

    // The flag list is the tables of args.rs, in order, so a new flag can't
    // be forgotten.
    let mut expected: Vec<String> = FLAGS.iter().map(|f| f.name.to_string()).collect();
    expected.extend(LONG_FLAGS.iter().map(|(n, _)| n.to_string()));
    expected.extend(EXTRA_FLAGS.iter().map(|(n, _, _)| n.to_string()));
    assert_eq!(names_of(member(&doc, "flags")), expected);

    // Every C08 flag is in the help text too.
    let help = std::fs::read_to_string(repo("crates/freerouting-cli/src/help.txt")).unwrap();
    for (name, _, _) in EXTRA_FLAGS {
        assert!(help.contains(name), "{name} is not in help.txt");
    }

    let formats = strings_of(member(&doc, "output_formats"));
    for format in [".ses", ".dsn", ".rules", ".drc.json"] {
        assert!(formats.iter().any(|f| f == format), "{formats:?}");
    }
    let schemas = member(&doc, "schemas").expect("schemas");
    for key in ["progress", "result_manifest"] {
        assert_eq!(
            member(schemas, key),
            Some(&JsonValue::Number("1".to_string())),
            "{key}"
        );
    }
    let exit_codes = member(&doc, "exit_codes").expect("exit_codes");
    for key in ["0", "1", "2", "130"] {
        assert!(member(exit_codes, key).is_some(), "{key}");
    }
    assert_eq!(
        strings_of(member(&doc, "stop_commands")),
        ["stop", "stop-router"]
    );
    // The stage registry (A10) is listed.
    let stages = member(&doc, "stages").expect("stages");
    assert!(!strings_of(member(stages, "algorithm")).is_empty());
    assert!(!strings_of(member(stages, "ordering")).is_empty());
}

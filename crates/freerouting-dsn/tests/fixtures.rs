//! Reads every Specctra fixture in the repository, then checks that writing
//! the tree and reading it back gives the same tree.

use std::path::{Path, PathBuf};

use freerouting_dsn::{Pcb, RulesFile, Session, decode};

/// Fixtures that do not parse, with the reason. The Java reader rejects them
/// too.
const EXPECTED_FAILURES: &[(&str, &str)] = &[(
    "Issue006-LPC18XX_43XX_SCH.dsn",
    "an OrCAD binary (OLE compound document) saved with a .dsn extension, not a Specctra file",
)];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn fixtures(extension: &str) -> Vec<PathBuf> {
    let mut all = Vec::new();
    collect(&fixtures_dir(), &mut all);
    all.retain(|path| {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(extension))
    });
    assert!(!all.is_empty(), "no .{extension} fixtures found");
    all
}

fn expected_failure(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?;
    EXPECTED_FAILURES
        .iter()
        .find(|(file, _)| *file == name)
        .map(|(_, why)| *why)
}

/// Parses every fixture with `parse`, writes it with `write`, parses the
/// output and compares. Panics with a list of all failures.
fn check_all<T: PartialEq + std::fmt::Debug>(
    extension: &str,
    parse: impl Fn(&str) -> freerouting_dsn::Result<T>,
    write: impl Fn(&T) -> String,
) -> usize {
    let mut failures = Vec::new();
    let mut passed = 0;
    for path in fixtures(extension) {
        let bytes = std::fs::read(&path).unwrap();
        let text = decode(&bytes);
        let shown = path.strip_prefix(fixtures_dir()).unwrap_or(&path).display();
        let parsed = parse(&text);
        if let Some(why) = expected_failure(&path) {
            if parsed.is_ok() {
                failures.push(format!("{shown}: expected to fail ({why}) but parsed"));
            }
            continue;
        }
        let tree = match parsed {
            Ok(tree) => tree,
            Err(e) => {
                failures.push(format!("{shown}: {e}"));
                continue;
            }
        };
        let written = write(&tree);
        match parse(&written) {
            Ok(again) if again == tree => {
                if write(&again) != written {
                    failures.push(format!("{shown}: writer output is not stable"));
                } else {
                    passed += 1;
                }
            }
            Ok(_) => failures.push(format!("{shown}: round trip changed the tree")),
            Err(e) => failures.push(format!("{shown}: written text does not parse: {e}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    passed
}

#[test]
fn all_dsn_fixtures_parse_and_round_trip() {
    let passed = check_all("dsn", Pcb::parse, Pcb::to_dsn);
    assert!(passed >= 150, "only {passed} .dsn fixtures checked");
}

#[test]
fn all_ses_fixtures_parse_and_round_trip() {
    let passed = check_all("ses", Session::parse, Session::to_ses);
    assert!(passed >= 30, "only {passed} .ses fixtures checked");
}

#[test]
fn all_rules_fixtures_parse_and_round_trip() {
    let passed = check_all("rules", RulesFile::parse, RulesFile::to_rules);
    assert!(passed >= 8, "only {passed} .rules fixtures checked");
}

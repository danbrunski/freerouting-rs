//! Reads every Specctra fixture in the repository, then checks that writing
//! the tree and reading it back gives the same tree.

use std::path::{Path, PathBuf};

use std::collections::BTreeSet;

use freerouting_dsn::ast::LayerType;
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
        if path == fixtures_dir().join("reference") {
            // Outputs of the Java reference harness, not inputs.
            continue;
        }
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

/// Reads the integer after `"key": ` inside the JSON object that follows
/// `"section": {`. The reference files are generated with a fixed layout, so
/// plain text search is enough.
fn json_count(json: &str, section: &str, key: &str) -> Option<usize> {
    let start = json.find(&format!("\"{section}\": {{"))?;
    let object = &json[start..start + json[start..].find('}')?];
    let at = object.find(&format!("\"{key}\": "))? + key.len() + 4;
    let digits: String = object[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn json_string<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let at = json.find(&format!("\"{key}\": \""))? + key.len() + 5;
    let end = json[at..].find('"')?;
    Some(&json[at..at + end])
}

/// Compares every design with what the Java reader built from it
/// (`fixtures/reference/load`, written by `tools/java-reference`): the same
/// files load, and the layer, component and net counts agree.
#[test]
fn designs_agree_with_java_reference() {
    let mut references = Vec::new();
    collect_json(&fixtures_dir().join("reference/load"), &mut references);
    assert!(references.len() >= 150, "{} references", references.len());
    let mut failures = Vec::new();
    for reference in &references {
        let json = std::fs::read_to_string(reference).unwrap();
        let fixture = json_string(&json, "fixture").expect("fixture name");
        let result = json_string(&json, "result").expect("result");
        let bytes = std::fs::read(fixtures_dir().join(fixture)).unwrap();
        let parsed = Pcb::parse(&decode(&bytes));
        match (result, parsed) {
            ("parse_error", Err(_)) => continue,
            ("parse_error", Ok(_)) => {
                failures.push(format!("{fixture}: Java fails to parse it, but it parsed"));
                continue;
            }
            (_, Err(e)) => {
                failures.push(format!("{fixture}: {e}"));
                continue;
            }
            // Java read the file but failed later, while computing statistics.
            ("exception", Ok(_)) => continue,
            (_, Ok(pcb)) => {
                let layers = pcb.structure.as_ref().map_or(0, |s| {
                    s.layers
                        .iter()
                        .filter(|layer| !matches!(layer.layer_type, Some(LayerType::Other(_))))
                        .count()
                });
                let components: BTreeSet<&str> = pcb
                    .placement
                    .iter()
                    .flat_map(|p| &p.components)
                    .flat_map(|c| &c.places)
                    .map(|place| place.name.as_str())
                    .collect();
                let nets: BTreeSet<(&str, i32)> = pcb
                    .network
                    .iter()
                    .flat_map(|n| &n.nets)
                    .map(|net| (net.name.as_str(), net.subnet.unwrap_or(1)))
                    .collect();
                let ours = (layers, components.len(), nets.len());
                let java = (
                    json_count(&json, "layers", "total_count").unwrap(),
                    json_count(&json, "components", "total_count").unwrap(),
                    json_count(&json, "nets", "total_count").unwrap(),
                );
                if ours != java {
                    failures.push(format!(
                        "{fixture}: (layers, components, nets) {ours:?}, Java {java:?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn collect_json(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_json(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
}

/// Sessions written by the Java router (`fixtures/reference/route`) read
/// back and round-trip. Empty files stand for designs Java could not load.
#[test]
fn java_router_sessions_parse_and_round_trip() {
    let dir = fixtures_dir().join("reference/route");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.map(|entry| entry.unwrap().path()).collect();
    paths.sort();
    let mut failures = Vec::new();
    for path in paths {
        if path.extension().is_none_or(|e| e != "ses") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        if bytes.is_empty() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        match Session::parse(&decode(&bytes)) {
            Ok(session) => match Session::parse(&session.to_ses()) {
                Ok(again) if again == session => {}
                Ok(_) => failures.push(format!("{name}: round trip changed the tree")),
                Err(e) => failures.push(format!("{name}: written text does not parse: {e}")),
            },
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

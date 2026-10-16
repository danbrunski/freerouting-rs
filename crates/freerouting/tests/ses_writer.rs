//! C05.2 to C05.5: the session writer against Java's `SesWriter` (C01.3's `ses` dumps).
//!
//! For every board in `tools/java-reference/dump-subset.txt` the test loads the board the way
//! the dump does, writes the session in Java style and compares the bytes with
//! `fixtures/reference/ses/unrouted/<board>.ses`; then it imports Java's routed session
//! (`fixtures/reference/route/<board>.ses`) into a fresh load, writes again and compares with
//! `fixtures/reference/ses/routed/<board>.ses`. Settings scenario: `default` (the session does
//! not depend on settings).
//!
//! `ses_full_set_matches_java` (ignored) does the same for every board of
//! `target/java-reference/full/ses/` (C01.7: `REFERENCE_FULL=1 REFERENCE_OUT_DIR=...`).

use std::path::{Path, PathBuf};

use freerouting::board::BasicBoard;
use freerouting::drc::load_design;
use freerouting::io::read_session;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses, write_ses_to};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The boards of the subset: (name without `.dsn`, path of the DSN).
fn subset() -> Vec<(String, PathBuf)> {
    let text =
        std::fs::read_to_string(root().join("tools/java-reference/dump-subset.txt")).unwrap();
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let name = l.split('\t').next().unwrap().to_string();
            let dsn = match name.strip_prefix("cases/") {
                Some(case) => root()
                    .join("tools/java-reference/cases")
                    .join(format!("{case}.dsn")),
                None => root().join("fixtures").join(format!("{name}.dsn")),
            };
            (name, dsn)
        })
        .collect()
}

fn design_name(dsn: &Path) -> String {
    dsn.file_stem().unwrap().to_string_lossy().to_string()
}

fn load(dsn: &Path) -> BasicBoard {
    let board = load_design(&std::fs::read(dsn).unwrap()).unwrap();
    (*board).clone()
}

/// The first line on which two texts differ, for failure messages.
fn first_difference(java: &str, rust: &str) -> String {
    for (n, (a, b)) in java.lines().zip(rust.lines()).enumerate() {
        if a != b {
            return format!("line {}: java {a:?} / rust {b:?}", n + 1);
        }
    }
    format!(
        "one text is a prefix of the other: java {} lines / rust {} lines",
        java.lines().count(),
        rust.lines().count()
    )
}

/// Differences from the Java dumps that are expected, as `(board, kind, reason)`. A routed
/// mismatch is a whole-file difference; the others are listed in `JAVA_EXTRA_PADSTACKS`.
const EXPECTED_MISMATCHES: &[(&str, &str, &str)] = &[
    (
        "Issue676-ch32v-tx118s",
        "routed",
        "Java's SesReader fails on all 48 wires with a ClassCastException (RationalVector to \
         IntVector) while normalizing the traces of this 1 000 000-per-mm board and imports \
         none; the port imports them (docs/DIVERGENCES.md, DRC section)",
    ),
    (
        "Issue029-hw48na",
        "unrouted",
        "Java's dump applies the adjacent Issue029-hw48na.rules, whose library adds three via \
         padstacks to library_out; the rules reader is C04",
    ),
    (
        "Issue029-hw48na",
        "routed",
        "same: the adjacent rules file adds three via padstacks (C04)",
    ),
];

/// The via padstacks that the adjacent rules file of a board adds to Java's `library_out`
/// (C04, not ported): the blocks are cut out of the Java text before comparing.
const JAVA_EXTRA_PADSTACKS: &[(&str, &[&str])] = &[(
    "Issue029-hw48na",
    &[
        "Via[0-1]_1092.2:685.8_um",
        "Via[0-1]_1541.78:1186.18_um",
        "Via[0-1]_1000:400_um",
    ],
)];

fn expected_mismatch(name: &str, kind: &str) -> Option<&'static str> {
    EXPECTED_MISMATCHES
        .iter()
        .find(|(n, k, _)| *n == name && *k == kind)
        .map(|(_, _, why)| *why)
}

/// `java` without the `(padstack "<name>" ...)` blocks that only the adjacent rules file
/// explains.
fn without_rules_padstacks(name: &str, java: &str) -> String {
    let Some((_, names)) = JAVA_EXTRA_PADSTACKS.iter().find(|(n, _)| *n == name) else {
        return java.to_string();
    };
    let mut out = Vec::new();
    let mut skipping: Option<&str> = None;
    for line in java.lines() {
        if let Some(indent_close) = skipping {
            if line == indent_close {
                skipping = None;
            }
            continue;
        }
        if names
            .iter()
            .any(|n| line == format!("      (padstack \"{n}\""))
        {
            skipping = Some("      )");
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

/// True when the only differences are names with non-ASCII characters: Java's scanner drops
/// them (see docs/DIVERGENCES.md, freerouting-dsn), the port keeps them. Package names that
/// become equal without them are one `(component` scope in Java, so the placement part is
/// compared as the multiset of its `(place` lines; the rest line by line, where the port's
/// line may differ if it has a non-ASCII character.
fn differs_only_by_non_ascii_names(java: &str, rust: &str) -> bool {
    let split = |text: &str| {
        let at = text.find("(library_out").unwrap_or(text.len());
        (text[..at].to_string(), text[at..].to_string())
    };
    let ((java_placement, java_rest), (rust_placement, rust_rest)) = (split(java), split(rust));
    let places = |text: &str| {
        let mut lines: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("(place "))
            .map(str::to_string)
            .collect();
        lines.sort();
        lines
    };
    places(&java_placement) == places(&rust_placement)
        && java_rest.lines().count() == rust_rest.lines().count()
        && java_rest
            .lines()
            .zip(rust_rest.lines())
            .all(|(j, r)| j == r || !r.is_ascii())
}

/// One session against its Java dump, with the expected mismatches of `EXPECTED_MISMATCHES`.
fn compare_one(name: &str, kind: &str, java: &str, rust: &str, failures: &mut Vec<String>) {
    let expected = expected_mismatch(name, kind);
    if rust == java {
        if let Some(why) = expected {
            failures.push(format!("{name} {kind}: matches Java now ({why})"));
        }
        return;
    }
    if expected.is_none() && !rust.is_ascii() && differs_only_by_non_ascii_names(java, rust) {
        eprintln!("{name} {kind}: differs from Java only in names with non-ASCII characters");
        return;
    }
    match expected {
        None => failures.push(format!("{name} {kind}: {}", first_difference(java, rust))),
        // The routed Issue676 differs as a whole (48 wires only the port imports).
        Some(_) if !JAVA_EXTRA_PADSTACKS.iter().any(|(n, _)| *n == name) => {}
        Some(_) => {
            let java = without_rules_padstacks(name, java);
            if rust != java {
                failures.push(format!("{name} {kind}: {}", first_difference(&java, rust)));
            }
        }
    }
}

/// Compares the unrouted and the routed session of `name` with the Java dumps under `refs`
/// (a directory with `ses/unrouted` and `ses/routed`). Returns the failures and the number of
/// boards compared.
fn compare_board(name: &str, dsn: &Path, refs: &Path, failures: &mut Vec<String>) -> usize {
    let design = design_name(dsn);
    let mut compared = 0;
    let unrouted_ref = refs.join("ses/unrouted").join(format!("{name}.ses"));
    if let Ok(java) = std::fs::read_to_string(&unrouted_ref) {
        let board = load(dsn);
        let rust = write_ses(&board, &design, SesStyle::Java);
        compared += 1;
        compare_one(name, "unrouted", &java, &rust, failures);
    }
    let routed_ref = refs.join("ses/routed").join(format!("{name}.ses"));
    let route_ses = root()
        .join("fixtures/reference/route")
        .join(format!("{name}.ses"));
    if let (Ok(java), Ok(session)) = (
        std::fs::read_to_string(&routed_ref),
        std::fs::read_to_string(&route_ses),
    ) {
        let mut board = load(dsn);
        let summary = read_session(&mut board, &session).unwrap();
        let rust = write_ses(&board, &design, SesStyle::Java);
        compared += 1;
        compare_one(name, "routed", &java, &rust, failures);
        if name == "Issue676-ch32v-tx118s" {
            assert_eq!(summary.wires_imported, 48, "{name}: wires imported");
        }
    }
    compared
}

#[test]
fn ses_matches_java_on_the_subset() {
    let refs = root().join("fixtures/reference");
    let mut failures = Vec::new();
    let mut compared = 0;
    let boards = subset();
    for (name, dsn) in &boards {
        compared += compare_board(name, dsn, &refs, &mut failures);
    }
    eprintln!(
        "{compared} sessions compared on {} subset boards",
        boards.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        compared >= boards.len(),
        "references missing: {compared} compared"
    );
}

/// The board names (path without `.ses`) of the session files under `dir`.
fn collect_sessions(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let file = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            collect_sessions(&path, &format!("{prefix}{file}/"), out);
        } else if let Some(stem) = file.strip_suffix(".ses") {
            out.push(format!("{prefix}{stem}"));
        }
    }
}

/// The whole set of `target/java-reference/full/` (C01.7; another directory with
/// `SES_FULL_DIR`); run with `--ignored`.
#[test]
#[ignore = "needs target/java-reference/full/ses (REFERENCE_FULL=1 regenerate-dumps.sh)"]
fn ses_full_set_matches_java() {
    let refs = std::env::var_os("SES_FULL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target/java-reference/full"));
    let mut failures = Vec::new();
    let mut compared = 0;
    let mut boards = 0;
    let mut names = Vec::new();
    collect_sessions(&refs.join("ses/unrouted"), "", &mut names);
    names.sort();
    for name in names {
        let dsn = root().join("fixtures").join(format!("{name}.dsn"));
        if !dsn.exists() {
            continue;
        }
        boards += 1;
        compared += compare_board(&name, &dsn, &refs, &mut failures);
    }
    eprintln!("{compared} sessions compared on {boards} boards");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn write_ses_to_matches_write_ses() {
    let (_, dsn) = subset()
        .into_iter()
        .find(|(n, _)| n == "Issue593-BBD_Mars-64")
        .unwrap();
    let board = load(&dsn);
    let design = design_name(&dsn);
    for style in [SesStyle::Java, SesStyle::Plain] {
        let mut bytes = Vec::new();
        write_ses_to(&board, &design, style, &mut bytes).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            write_ses(&board, &design, style)
        );
    }
}

#[test]
fn ses_writes_attach_off_for_no_attach_padstacks() {
    let dsn = root().join("fixtures/Issue508-DAC2020_bm06.dsn");
    let source = std::fs::read_to_string(&dsn).unwrap();
    assert!(
        source.contains("(attach off)"),
        "the DSN has (attach off) padstacks"
    );
    let board = load(&dsn);
    let ses = write_ses(&board, &design_name(&dsn), SesStyle::Java);
    let library = &ses[ses.find("(library_out").unwrap()..ses.find("(network_out").unwrap()];
    assert!(library.contains("\n        (attach off)\n"), "{library}");
}

/// The padstack names of `library_out` are unique (`SesRoundTripTest.assertUniqueLibraryPadstacks`).
#[test]
fn library_out_padstack_names_are_unique() {
    for (name, dsn) in subset() {
        let board = load(&dsn);
        let ses = write_ses(&board, &design_name(&dsn), SesStyle::Java);
        let start = ses.find("(library_out").expect("library_out");
        let end = ses[start..]
            .find("(network_out")
            .map(|n| n + start)
            .expect("network_out after");
        let mut names = std::collections::HashSet::new();
        for part in ses[start..end].split("(padstack ").skip(1) {
            let end = part.find(['\n', ')']).unwrap();
            let padstack = part[..end].trim_end().to_string();
            assert!(
                names.insert(padstack.clone()),
                "{name}: duplicate padstack {padstack}"
            );
        }
        if name == "Issue742-tastexx-pcb" {
            assert!(
                !names.is_empty(),
                "{name}: library_out declares a via padstack"
            );
        }
    }
}

/// C05.5: the write time of the largest fixture next to `load_design`. Run in release:
/// `cargo test --release -p freerouting --test ses_writer -- --ignored --nocapture ses_write_time`.
#[test]
#[ignore = "timing; run in release with --nocapture"]
fn ses_write_time_z80() {
    use std::time::Instant;
    let dsn = root().join("fixtures/Issue190-processor.Z80.dsn");
    let bytes = std::fs::read(&dsn).unwrap();
    let design = design_name(&dsn);
    let t = Instant::now();
    let board = load_design(&bytes).unwrap();
    let load = t.elapsed();
    let mut board = (*board).clone();
    let t = Instant::now();
    let unrouted = write_ses(&board, &design, SesStyle::Java);
    let write_unrouted = t.elapsed();
    let route =
        std::fs::read_to_string(root().join("fixtures/reference/route/Issue190-processor.Z80.ses"))
            .unwrap();
    let t = Instant::now();
    read_session(&mut board, &route).unwrap();
    let import = t.elapsed();
    let mut best = std::time::Duration::MAX;
    let mut bytes_written = 0;
    for _ in 0..5 {
        let t = Instant::now();
        let routed = write_ses(&board, &design, SesStyle::Java);
        best = best.min(t.elapsed());
        bytes_written = routed.len();
    }
    let mut sink = Vec::new();
    let t = Instant::now();
    write_ses_to(&board, &design, SesStyle::Java, &mut sink).unwrap();
    let streamed = t.elapsed();
    let t = Instant::now();
    let plain = write_ses(&board, &design, SesStyle::Plain);
    let plain_time = t.elapsed();
    eprintln!(
        "Issue190-processor.Z80: load_design {load:?}; write unrouted ({} bytes) {write_unrouted:?}; \
         import route SES {import:?}; write routed Java ({bytes_written} bytes) best of 5 {best:?}; \
         streamed {streamed:?}; plain ({} bytes) {plain_time:?}",
        unrouted.len(),
        plain.len()
    );
}

//! C05.6: ports of the Java session tests (`SesRoundTripTest`, the SES half of
//! `MultiOutputGeneratorTest`) and the round-trip property on every fixture that has a
//! route reference.
//!
//! Java tests not ported:
//! * `nullInputStreamThrowsIoException` and `nullBoardThrowsIoException`: Rust has neither a
//!   null stream nor a null board. `reading_empty_and_garbage_input_is_an_error` covers the
//!   cases a Rust caller can reach (empty text and garbage are errors from `read_session`).
//! * `MultiOutputGeneratorTest`'s KiCad session JSON and DRC summary halves: other formats
//!   (the DRC summary is `drc::generate_report`).

use std::path::PathBuf;

use freerouting::board::BasicBoard;
use freerouting::board::items::Item;
use freerouting::drc::load_design;
use freerouting::io::read_session;
use freerouting::io::specctra::ses_writer::{SesStyle, snapped_endpoint, write_ses};
use freerouting::scoring::BoardStatistics;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures").join(name)
}

fn load(name: &str) -> BasicBoard {
    let board = load_design(&std::fs::read(fixture(name)).unwrap()).unwrap();
    (*board).clone()
}

fn import(board: &mut BasicBoard, ses: &str) -> freerouting::io::SesImportSummary {
    read_session(board, &std::fs::read_to_string(fixture(ses)).unwrap()).unwrap()
}

/// `sesWriterProducesValidHeader`.
#[test]
fn ses_writer_produces_valid_header() {
    let board = load("Issue026-J2_reference.dsn");
    let content = write_ses(&board, "Issue026-J2_reference.dsn", SesStyle::Java);
    assert!(
        content.starts_with("(session "),
        "{}",
        &content[..50.min(content.len())]
    );
    assert!(content.contains("(routes"));
    for camel in [
        "(hostCad",
        "(stringQuote",
        "(writeResolution",
        "(viaRule",
        "(pullTight",
        "(shoveFixed",
        "(useLayer",
        "(clearanceClass",
    ] {
        assert!(
            !content.contains(camel),
            "SES output must not contain {camel}"
        );
    }
}

/// `sesRoundTripPreservesWireCount`.
#[test]
fn ses_round_trip_preserves_wire_count() {
    let mut board = load("Issue593-BBD_Mars-64.dsn");
    let imported = import(&mut board, "Issue593-BBD_Mars-64.ses");
    assert!(imported.wires_imported > 0);
    assert_eq!(imported.errors_encountered, 0);
}

/// `writerOutputCanBeReadBackBySesReader`.
#[test]
fn writer_output_can_be_read_back_by_ses_reader() {
    let mut source = load("Issue593-BBD_Mars-64.dsn");
    let original = import(&mut source, "Issue593-BBD_Mars-64.ses");
    assert!(original.wires_imported > 0);
    let written = write_ses(&source, "round-trip.dsn", SesStyle::Java);
    assert!(!written.is_empty());
    let mut target = load("Issue593-BBD_Mars-64.dsn");
    let summary = read_session(&mut target, &written).unwrap();
    assert!(summary.wires_imported > 0);
    assert_eq!(summary.errors_encountered, 0);
    assert_eq!(
        summary.wires_imported, original.wires_imported,
        "round-trip wire count must match the original import count"
    );
}

/// `sesWriterOutputIsNonEmpty`.
#[test]
fn ses_writer_output_is_non_empty() {
    let board = load("Issue143-rpi_splitter.dsn");
    assert!(!write_ses(&board, "test.dsn", SesStyle::Java).is_empty());
}

/// `invalidSesThrowsOnRead`, and the two null cases as far as Rust can reach them.
#[test]
fn reading_empty_and_garbage_input_is_an_error() {
    let mut board = load("Issue143-rpi_splitter.dsn");
    assert!(read_session(&mut board, "garbage").is_err());
    assert!(read_session(&mut board, "").is_err());
    // `(session x)` is a session without routes: nothing to import, no error.
    let summary = read_session(&mut board, "(session x)").unwrap();
    assert_eq!(summary.wires_imported + summary.vias_imported, 0);
}

/// `endpointSnappingIsStableAndRoundTrips`.
#[test]
fn endpoint_snapping_is_stable_and_round_trips() {
    let mut board = load("Issue593-BBD_Mars-64.dsn");
    let imported = import(&mut board, "Issue593-BBD_Mars-64.ses");
    assert!(imported.wires_imported > 0);

    let mut traces_with_drill_contacts = 0;
    for id in board.traces() {
        for start_side in [true, false] {
            let contacts = if start_side {
                board.trace_start_contacts(id)
            } else {
                board.trace_end_contacts(id)
            };
            let drills: Vec<_> = contacts
                .iter()
                .filter_map(|c| board.item(*c).and_then(Item::drill))
                .collect();
            if drills.is_empty() {
                continue;
            }
            traces_with_drill_contacts += 1;
            let trace = board.item(id).and_then(Item::as_trace).unwrap();
            let corner = if start_side {
                trace.first_corner()
            } else {
                trace.last_corner()
            }
            .to_float();
            if let Some(snapped) = snapped_endpoint(&board, id, start_side) {
                // A snap may only move the endpoint to the centre of a contacted drill item.
                assert!(
                    drills
                        .iter()
                        .any(|d| d.center.to_float().distance(snapped) < 0.5),
                    "snap target must be a contacted drill item centre"
                );
                assert!(corner.distance(snapped) > 0.5);
            }
        }
    }
    assert!(
        traces_with_drill_contacts > 0,
        "the fixture must exercise drill-contacted endpoints"
    );

    let written = write_ses(&board, "Issue593-BBD_Mars-64.dsn", SesStyle::Java);
    let mut fresh = load("Issue593-BBD_Mars-64.dsn");
    let reimported = read_session(&mut fresh, &written).unwrap();
    assert_eq!(imported.wires_imported, reimported.wires_imported);
    assert_eq!(reimported.errors_encountered, 0);
}

/// `issue742SesRoundTripsWithoutErrors`.
#[test]
fn issue742_ses_round_trips_without_errors() {
    let mut board = load("Issue742-tastexx-pcb.dsn");
    let imported = import(&mut board, "Issue742-tastexx-pcb.ses");
    assert!(imported.wires_imported > 0);
    assert_eq!(imported.errors_encountered, 0);

    let content = write_ses(&board, "Issue742-tastexx-pcb.dsn", SesStyle::Java);
    let opens = content.matches('(').count();
    let closes = content.matches(')').count();
    assert_eq!(opens, closes, "SES scopes must be balanced");
    let library = content.find("(library_out").expect("library_out");
    let network = content[library..]
        .find("(network_out")
        .expect("network_out after library_out");
    assert!(network > 0);
    assert!(
        !content.contains("0.000"),
        "whole-degree rotations must not use trailing decimals"
    );
    assert!(
        content.contains("338.5") || content.contains(" front 339"),
        "fractional or rounded component rotations must be preserved"
    );

    let mut fresh = load("Issue742-tastexx-pcb.dsn");
    let reimported = read_session(&mut fresh, &content).unwrap();
    assert_eq!(imported.wires_imported, reimported.wires_imported);
    assert_eq!(reimported.errors_encountered, 0);
}

/// The SES half of `MultiOutputGeneratorTest.testGenerateMultipleOutputs`.
#[test]
fn multi_output_ses_is_not_empty() {
    let board = load("Issue508-DAC2020_bm01.dsn");
    let ses = write_ses(&board, "bm01", SesStyle::Java);
    assert!(!ses.is_empty());
    assert!(ses.starts_with("(session bm01"));
}

/// Every fixture board that loads: both styles parse to equal trees, and Plain has no exponent.
fn dsn_fixtures() -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root().join("fixtures")).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if let Ok(inner) = std::fs::read_dir(&path) {
                for e in inner.flatten() {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "dsn") {
                        let name = format!(
                            "{}/{}",
                            path.file_name().unwrap().to_string_lossy(),
                            p.file_stem().unwrap().to_string_lossy()
                        );
                        found.push((name, p));
                    }
                }
            }
        } else if path.extension().is_some_and(|x| x == "dsn") {
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            found.push((name, path));
        }
    }
    found.sort();
    found
}

fn has_exponent(text: &str) -> bool {
    text.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .any(|token| {
            token.starts_with(|c: char| c.is_ascii_digit() || c == '-')
                && token.contains(['e', 'E'])
                && token.parse::<f64>().is_ok()
        })
}

/// `f` on every item, on all cores; the results in item order.
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<R>>> =
        items.iter().map(|_| std::sync::Mutex::new(None)).collect();
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    *results[i].lock().unwrap() = Some(f(item));
                }
            });
        }
    });
    results
        .into_iter()
        .map(|r| r.into_inner().unwrap().unwrap())
        .collect()
}

#[test]
fn both_styles_parse_to_equal_sessions_on_every_fixture() {
    let fixtures = dsn_fixtures();
    let results = par_map(&fixtures, |(name, path)| {
        let Ok(board) = load_design(&std::fs::read(path).unwrap()) else {
            return false;
        };
        let mut board = (*board).clone();
        let route = root()
            .join("fixtures/reference/route")
            .join(format!("{name}.ses"));
        if let Ok(session) = std::fs::read_to_string(&route) {
            let _ = read_session(&mut board, &session);
        }
        let java = write_ses(&board, name, SesStyle::Java);
        let plain = write_ses(&board, name, SesStyle::Plain);
        assert!(!has_exponent(&plain), "{name}: plain style has an exponent");
        let java_tree =
            freerouting::dsn::Session::parse(&java).unwrap_or_else(|e| panic!("{name}: {e}"));
        let plain_tree =
            freerouting::dsn::Session::parse(&plain).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(java_tree, plain_tree, "{name}");
        true
    });
    let boards = results.iter().filter(|ok| **ok).count();
    eprintln!("{boards} boards");
    assert!(boards >= 100);
}

const ISSUE676: &str = "Issue676-ch32v-tx118s";
const ISSUE676_WHY: &str = "Java's SesReader divides session coordinates by \
    dsn_to_board(1) / resolution and SesWriter divides board coordinates by the same factor, so \
    the two are inverse only where it is 1. It is 0.1 on this board, the only fixture where it \
    is not 1, and a written session reads back 100 times too large. The port reproduces both \
    sides (the written sessions match Java byte for byte)";

/// The fields of two statistics that differ. Counts and everything else must be equal; the
/// `f32` and `f64` values (lengths, violation sums) may differ by 1e-5 relative, because the
/// second import inserts the traces in the order of the written session and the sums add up
/// in another order.
fn statistics_differences(a: &BoardStatistics, b: &BoardStatistics) -> Vec<String> {
    let (a, b) = (format!("{a:#?}"), format!("{b:#?}"));
    assert_eq!(a.lines().count(), b.lines().count());
    let mut diff = Vec::new();
    for (x, y) in a.lines().zip(b.lines()) {
        if x == y {
            continue;
        }
        let value = |line: &str| {
            let (key, rest) = line.trim().split_once(": ")?;
            let number = rest.trim_end_matches(',');
            let is_float = number.contains(['.', 'e', 'E']);
            Some((key.to_string(), number.parse::<f64>().ok()?, is_float))
        };
        match (value(x), value(y)) {
            (Some((kx, vx, true)), Some((ky, vy, true)))
                if kx == ky && (vx - vy).abs() <= 1e-5 * vx.abs().max(vy.abs()).max(1.0) => {}
            _ => diff.push(format!("{} -> {}", x.trim(), y.trim())),
        }
    }
    diff
}

/// Boards whose written session re-imports fewer wires than the reference session, as in Java,
/// with the reason. The statistics after both imports must still be equal.
const REIMPORT_FEWER_WIRES: &[(&str, &str)] = &[(
    "Issue420-contribution-board",
    "the first import of Java's a773a70ec session makes 3933 traces of its 3934 wires, so the \
     written session has 3933; Java does the same (SesImportStats at a773a70ec: 3934 wires, \
     3933 traces, and 3933 wires when its own SesWriter output is imported again)",
)];

/// Load, import the reference session, write it, re-import into a fresh load: the statistics
/// of both imports are equal.
///
/// A session holds every trace and via of the board, the DSN's own wiring included, so both
/// loads drop the DSN's wiring first; otherwise the second import would put the DSN's wires
/// on top of themselves.
#[test]
fn written_session_reimports_to_the_same_statistics() {
    let fixtures = dsn_fixtures();
    let results = par_map(&fixtures, |(name, path)| {
        let route = root()
            .join("fixtures/reference/route")
            .join(format!("{name}.ses"));
        let session = std::fs::read_to_string(&route)
            .ok()
            .filter(|s| !s.is_empty())?;
        let loaded = load_design(&std::fs::read(path).unwrap()).ok()?;
        let mut first = (*loaded).clone();
        first.delete_all_traces_and_vias();
        let first_summary = read_session(&mut first, &session).unwrap();
        let written = write_ses(&first, name, SesStyle::Java);
        let mut second = (*load_design(&std::fs::read(path).unwrap()).unwrap()).clone();
        second.delete_all_traces_and_vias();
        let second_summary = read_session(&mut second, &written).unwrap();
        let fewer_wires = REIMPORT_FEWER_WIRES.iter().any(|(board, _)| *board == name);
        if fewer_wires && first_summary == second_summary {
            return Some(Some(format!(
                "{name}: the re-import counts the same wires now; remove it from \
                 REIMPORT_FEWER_WIRES"
            )));
        }
        if first_summary != second_summary && !fewer_wires {
            return Some(Some(format!(
                "{name}: import {first_summary:?} / re-import {second_summary:?}"
            )));
        }
        let diff = statistics_differences(
            &BoardStatistics::new(&first),
            &BoardStatistics::new(&second),
        );
        Some(match (diff.is_empty(), name == ISSUE676) {
            (true, false) | (false, true) => None,
            (false, false) => Some(format!(
                "{name}: BoardStatistics differ: {}",
                diff.join("; ")
            )),
            (true, true) => Some(format!("{name}: round-trips now ({ISSUE676_WHY})")),
        })
    });
    let boards = results.iter().flatten().count();
    let failures: Vec<String> = results.into_iter().flatten().flatten().collect();
    eprintln!("{boards} boards with a route reference");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(boards >= 100);
}

/// How many endpoint snaps fire on the routed board of every fixture with a route reference
/// (`SesWriter.snappedEndpoint`, C05.4): none, as the module docs of `ses_writer` explain.
#[test]
fn endpoint_snaps_never_fire() {
    let fixtures = dsn_fixtures();
    let counts = par_map(&fixtures, |(name, path)| {
        let route = root()
            .join("fixtures/reference/route")
            .join(format!("{name}.ses"));
        let session = std::fs::read_to_string(&route)
            .ok()
            .filter(|s| !s.is_empty())?;
        let loaded = load_design(&std::fs::read(path).unwrap()).ok()?;
        let mut board = (*loaded).clone();
        let _ = read_session(&mut board, &session);
        let mut traces = 0;
        let mut snaps = 0;
        for id in board.traces() {
            traces += 1;
            for start in [true, false] {
                if snapped_endpoint(&board, id, start).is_some() {
                    snaps += 1;
                }
            }
        }
        Some((traces, snaps))
    });
    let boards = counts.iter().flatten().count();
    let traces: usize = counts.iter().flatten().map(|c| c.0).sum();
    let snaps: usize = counts.iter().flatten().map(|c| c.1).sum();
    eprintln!("{snaps} endpoint snaps on {traces} traces of {boards} routed boards");
    assert!(boards >= 100);
    assert_eq!(
        snaps, 0,
        "snapping fires; update the module docs of ses_writer"
    );
}

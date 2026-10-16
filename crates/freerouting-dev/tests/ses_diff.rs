//! `freerouting-dev ses-diff` on Java's routed session of
//! `Issue508-DAC2020_bm01` and edited copies of it (F02.2).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use freerouting::board::RoutingBoard;
use freerouting::dsn::Session;
use freerouting::dsn::ast::{Shape, WiringItem};
use freerouting_dev::json_read::Json;
use freerouting_dev::ses_diff::{SesDiff, parse_session};
use freerouting_dev::{load_board, read_text};

const BOARD: &str = "Issue508-DAC2020_bm01";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn board() -> &'static RoutingBoard {
    static BOARD_CELL: OnceLock<RoutingBoard> = OnceLock::new();
    BOARD_CELL.get_or_init(|| load_board(&fixtures().join(format!("{BOARD}.dsn"))).unwrap())
}

fn java_ses_text() -> String {
    read_text(&fixtures().join(format!("reference/route/{BOARD}.ses"))).unwrap()
}

fn java_session() -> Session {
    parse_session(&java_ses_text()).unwrap()
}

fn diff(a: &Session, b: &Session) -> SesDiff {
    SesDiff::new(board(), a, b)
}

#[test]
fn java_session_is_equivalent_to_itself() {
    let session = java_session();
    let d = diff(&session, &session);
    assert!(d.equivalent(), "{}", d.to_text(10));
    assert!(d.nets.is_empty());
    assert!(d.to_text(10).starts_with("equivalent\n"));
    assert_eq!(d.a.import.wires_imported, 528);
    assert_eq!(d.a.import.vias_imported, 126);
}

/// Appends `.0` to every whole number on a line that holds only numbers
/// (the points of a path), as `1000` vs `1000.0`.
fn reformat_numbers(text: &str) -> String {
    let mut out = String::new();
    let mut changed = 0;
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let numeric = !tokens.is_empty()
            && tokens
                .iter()
                .all(|t| t.parse::<i64>().is_ok() || t.parse::<f64>().is_ok());
        if numeric {
            let indent = &line[..line.len() - line.trim_start().len()];
            let rewritten: Vec<String> = tokens
                .iter()
                .map(|t| {
                    if t.contains('.') {
                        t.to_string()
                    } else {
                        changed += 1;
                        format!("{t}.0")
                    }
                })
                .collect();
            out.push_str(indent);
            out.push_str(&rewritten.join(" "));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    assert!(changed > 1000, "only {changed} numbers reformatted");
    out
}

#[test]
fn reordered_and_reformatted_session_is_equivalent() {
    let mut session = java_session();
    let routes = session.routes.as_mut().unwrap();
    routes.network_out.reverse();
    for net in &mut routes.network_out {
        net.items.reverse();
        for item in &mut net.items {
            if let WiringItem::Wire(wire) = item {
                for shape in &mut wire.shapes {
                    if let Shape::Path { points, .. } = shape {
                        points.reverse();
                    }
                }
            }
        }
    }
    let text = reformat_numbers(&session.to_ses());
    assert_ne!(text, java_ses_text());
    let edited = parse_session(&text).unwrap();
    let d = diff(&java_session(), &edited);
    assert!(d.equivalent(), "{}", d.to_text(10));
}

#[test]
fn via_moved_by_one_unit_is_reported_with_net_and_position() {
    let mut session = java_session();
    let (net_name, padstack, x, y) = session
        .routes
        .as_mut()
        .unwrap()
        .network_out
        .iter_mut()
        .find_map(|net| {
            let name = net.name.clone();
            net.items.iter_mut().find_map(|item| match item {
                WiringItem::Via(via) => {
                    let old = (via.at.x, via.at.y);
                    via.at.x += 1.0;
                    Some((name.clone(), via.padstack.clone(), old.0, old.1))
                }
                _ => None,
            })
        })
        .unwrap();
    let d = diff(&java_session(), &session);
    assert!(!d.equivalent());
    assert_eq!(d.nets.len(), 1, "{}", d.to_text(10));
    let net = &d.nets[0];
    assert_eq!(net.net, net_name);
    assert!(net.wires_only_in_a.is_empty() && net.wires_only_in_b.is_empty());
    assert_eq!(net.vias_only_in_a.len(), 1);
    assert_eq!(net.vias_only_in_b.len(), 1);
    let text = d.to_text(10);
    let before = format!("only in a: via {padstack} at ({} {})", x as i64, y as i64);
    let after = format!(
        "only in b: via {padstack} at ({} {})",
        x as i64 + 1,
        y as i64
    );
    assert!(text.contains(&format!("net {net_name}:")), "{text}");
    assert!(text.contains(&before), "{text}");
    assert!(text.contains(&after), "{text}");
}

/// The net whose only wire is removed, and Java's incomplete count after
/// importing the session without it:
/// `tools/java-reference/probes/SesImportStats.java` at `a773a70ec` on J01's
/// route session with that wire cut out prints `incomplete 25` (24 with the
/// wire). The one-wire nets of that session are `N$2`, `N$3`, `PG2` and `PL6`.
const REMOVED_WIRE_NET: &str = "N$2";
const JAVA_INCOMPLETE_WITHOUT_WIRE: usize = 25;

#[test]
fn removed_wire_is_reported_and_incomplete_count_rises() {
    let mut session = java_session();
    let net = session
        .routes
        .as_mut()
        .unwrap()
        .network_out
        .iter_mut()
        .find(|n| n.name == REMOVED_WIRE_NET)
        .unwrap();
    assert_eq!(
        net.items.len(),
        1,
        "{REMOVED_WIRE_NET} should have one wire"
    );
    net.items.clear();
    let d = diff(&java_session(), &session);
    assert!(!d.equivalent());
    assert_eq!(d.nets.len(), 1, "{}", d.to_text(10));
    assert_eq!(d.nets[0].net, REMOVED_WIRE_NET);
    assert_eq!(d.nets[0].wires_only_in_a.len(), 1);
    assert!(d.nets[0].wires_only_in_b.is_empty());
    assert_eq!(d.a.statistics.incomplete_connections, 24);
    assert_eq!(
        d.b.statistics.incomplete_connections,
        JAVA_INCOMPLETE_WITHOUT_WIRE
    );
    assert_eq!(d.a.incomplete_of(REMOVED_WIRE_NET), 0);
    assert_eq!(d.b.incomplete_of(REMOVED_WIRE_NET), 1);
    let deltas = d.statistics_deltas();
    assert!(deltas.contains(&("connections.incomplete_count", 24.0, 25.0)));
    let text = d.to_text(10);
    assert!(
        text.contains(&format!("net {REMOVED_WIRE_NET}: incomplete 0 -> 1")),
        "{text}"
    );
    assert!(text.contains("only in a: path "), "{text}");
}

/// Java's route statistics come from the board in memory after routing,
/// Rust's from importing the session Java wrote. They agree on every
/// compared field except bends: Java counts 705 on its routed board (J01's
/// 3-pass route session at `a773a70ec`), and Java itself counts 704 after
/// importing the same SES into the loaded board
/// (`tools/java-reference/probes/SesImportStats.java` at `a773a70ec`:
/// incomplete 24, violations 0, vias 126, length_mm 3570.9983, bends 704).
/// So the import is compared with Java's import for bends.
const JAVA_IMPORT_BENDS: f64 = 704.0;

#[test]
fn imported_java_session_matches_java_route_statistics() {
    let json_path = fixtures().join(format!("reference/route/{BOARD}.json"));
    let json = Json::parse(&std::fs::read_to_string(json_path).unwrap()).unwrap();
    let java = |field: &str| {
        json.path(&format!("statistics.{field}"))
            .and_then(Json::as_f64)
            .unwrap_or_else(|| panic!("{field}"))
    };
    let session = java_session();
    let s = diff(&session, &session).a.statistics;
    assert_eq!(
        s.incomplete_connections as f64,
        java("connections.incomplete_count")
    );
    assert_eq!(
        s.clearance_violations as f64,
        java("clearance_violations.total_count")
    );
    assert_eq!(s.vias as f64, java("vias.total_count"));
    let length = java("traces.total_length_mm");
    assert!(
        (f64::from(s.trace_length_mm) - length).abs() <= 0.006,
        "{}",
        s.trace_length_mm
    );
    assert_eq!(java("bends.total_count"), 705.0);
    assert_eq!(s.bends as f64, JAVA_IMPORT_BENDS);
}

#[test]
fn json_output_lists_differing_nets() {
    let mut session = java_session();
    session
        .routes
        .as_mut()
        .unwrap()
        .network_out
        .retain(|n| n.name != REMOVED_WIRE_NET);
    let d = diff(&java_session(), &session);
    let json = Json::parse(&d.to_json().to_pretty_string()).unwrap();
    assert_eq!(json.get("equivalent"), Some(&Json::Bool(false)));
    let Some(Json::Array(nets)) = json.get("nets") else {
        panic!("no nets");
    };
    assert_eq!(nets.len(), 1);
    assert_eq!(
        nets[0].get("net").and_then(Json::as_str),
        Some(REMOVED_WIRE_NET)
    );
    let incomplete = json
        .path("b.statistics")
        .and_then(|s| s.get("connections.incomplete_count"))
        .and_then(Json::as_f64);
    assert_eq!(incomplete, Some(25.0));
}

//! A session repeats the input's own wiring, so every measure that imports it
//! into the input board must import it in place of that wiring
//! (`ses_diff::import_routed_session`): the board is then the routed board,
//! with each trace and via once.
//!
//! The input is the Issue508 SMD demo board with wiring of its own: net
//! NET_CROSS_A pre-routed with Java's four wires, a via at one of their
//! corners and a stub on Bottom from it, all protected (the session repeats
//! all of them), and an unfixed trace from
//! U1-2 to R1-2 that the router ripped (the session has no wiring of
//! NET_LOCAL_LEFT_MID). The session is Java's route of the bare board with
//! those changes, so the routed board is the bare board with the session
//! imported, and every measure of the wired input must equal that board's.

use std::path::{Path, PathBuf};
use std::process::Command;

use freerouting::board::BasicBoard;
use freerouting::board::structure::FixedState;
use freerouting::drc::DesignRulesChecker;
use freerouting::io::specctra::import_session;
use freerouting::quality::neckdown::width_report::report;
use freerouting_dev::bench::{measure, measure_widths};
use freerouting_dev::failed_vs_human::analyze;
use freerouting_dev::power_traces::power_layer_trace_count;
use freerouting_dev::ses_diff::{
    SesDiff, SessionStatistics, import_routed_session, parse_session, remove_session_wiring,
};
use freerouting_dev::tidy::{tidy_board, tidy_session};
use freerouting_dev::widths::widths_of;
use freerouting_dev::{load_board, read_text};

const BOARD: &str = "Issue508-SMD-routing-issue-demo";
const VIA: &str = "\"Via[0-1]_600:300_um\"";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("session-import")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn replace_once(text: &str, anchor: &str, with: &str) -> String {
    assert!(text.contains(anchor), "{anchor:?} not found");
    text.replacen(anchor, with, 1)
}

/// The bare demo board, with Bottom a power layer and NET_CROSS_A in a wide
/// class when asked (for `power-traces` and the width report).
fn bare_dsn(power: bool, wide: bool) -> String {
    let mut text = read_text(&fixtures().join(format!("{BOARD}.dsn"))).unwrap();
    if power {
        text = replace_once(
            &text,
            "(layer Bottom\n      (type signal)",
            "(layer Bottom\n      (type power)\n      (use_net NET_LOCAL_RIGHT_BOT)",
        );
    }
    if wide {
        text = replace_once(
            &text,
            "    (class default\n",
            "    (class wide NET_CROSS_A\n      (rule (width 400) (clearance 200))\n    )\n    (class default\n",
        );
    }
    text
}

/// The input's own wiring, in DSN units: Java's wires of NET_CROSS_A with a
/// via at a corner and a stub on Bottom from it (all protected), and an
/// unfixed trace from U1-2 to R1-2.
const INPUT_WIRING: &str = "    (wire (path Top 150 72000 -44600 72676.7 -44600) (net NET_CROSS_A) (type protect))
    (wire (path Top 150 73909 -42691 72676.7 -43923.3 72676.7 -44600) (net NET_CROSS_A) (type protect))
    (wire (path Top 200 130850 -34198.3 82401.7 -34198.3 73909 -42691) (net NET_CROSS_A) (type protect))
    (wire (path Top 200 130850 -35000 130850 -34198.3) (net NET_CROSS_A) (type protect))
    (via \"Via[0-1]_600:300_um\" 82401.7 -34198.3 (net NET_CROSS_A) (type protect))
    (wire (path Bottom 200 82401.7 -34198.3 82401.7 -20000) (net NET_CROSS_A) (type protect))
    (wire (path Top 200 72000 -45000 20850 -35000) (net NET_LOCAL_LEFT_MID))
";

fn wired_dsn(power: bool, wide: bool) -> String {
    replace_once(
        &bare_dsn(power, wide),
        "(wiring\n",
        &format!("(wiring\n{INPUT_WIRING}"),
    )
}

/// `text` without the balanced scope that starts at `anchor`.
fn without_scope(text: &str, anchor: &str) -> String {
    let start = text.find(anchor).expect("the scope");
    let mut depth = 0;
    for (offset, c) in text[start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + offset + 1;
                    return format!("{}{}", &text[..start], &text[end..]);
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced scope");
}

/// Java's session of the bare board as the router would write it from the
/// wired input: NET_CROSS_A's wires (Java's already) plus the protected
/// Bottom trace and via, and no wiring of NET_LOCAL_LEFT_MID.
fn session_text() -> String {
    let java = read_text(&fixtures().join(format!("reference/route/{BOARD}.ses"))).unwrap();
    let anchor = "(net \"NET_CROSS_A\"\n";
    let repeated = format!(
        "{anchor}        (wire\n          (path Bottom 2000\n            824017 -341983\n            824017 -200000\n          )\n          (type protect)\n        )\n        (via {VIA} 824017 -341983\n          (type protect)\n        )\n"
    );
    let text = replace_once(&java, anchor, &repeated);
    without_scope(&text, "(net \"NET_LOCAL_LEFT_MID\"")
}

struct Files {
    bare: PathBuf,
    wired: PathBuf,
    ses: PathBuf,
}

fn files(name: &str, power: bool, wide: bool) -> Files {
    let dir = scratch(name);
    let files = Files {
        bare: dir.join("bare.dsn"),
        wired: dir.join("wired.dsn"),
        ses: dir.join("routed.ses"),
    };
    std::fs::write(&files.bare, bare_dsn(power, wide)).unwrap();
    std::fs::write(&files.wired, wired_dsn(power, wide)).unwrap();
    std::fs::write(&files.ses, session_text()).unwrap();
    files
}

/// The old measure: the session imported on top of the input's wiring.
fn stacked(input: &BasicBoard, ses: &Path) -> BasicBoard {
    let mut board = input.clone();
    import_session(
        &mut board,
        &parse_session(&read_text(ses).unwrap()).unwrap(),
    );
    board
}

fn trace_and_via_states(board: &BasicBoard) -> Vec<FixedState> {
    board
        .items()
        .filter(|item| item.is_trace() || item.is_via())
        .map(|item| item.fixed_state())
        .collect()
}

#[test]
fn removing_the_session_wiring_keeps_only_system_fixed_wiring() {
    let dir = scratch("strip");
    let fixed =
        "    (wire (path Top 200 10000 -80000 30000 -80000) (net NET_LOCAL_LEFT_BOT) (type fix))\n";
    let text = replace_once(
        &wired_dsn(false, false),
        "(wiring\n",
        &format!("(wiring\n{fixed}"),
    );
    std::fs::write(dir.join("board.dsn"), text).unwrap();
    let mut board = load_board(&dir.join("board.dsn")).unwrap();
    let states = trace_and_via_states(&board);
    assert_eq!(states.len(), 8);
    assert_eq!(
        states
            .iter()
            .filter(|s| **s == FixedState::UserFixed)
            .count(),
        6
    );
    assert_eq!(remove_session_wiring(&mut board), 7);
    assert_eq!(trace_and_via_states(&board), [FixedState::SystemFixed]);
}

#[test]
fn the_bench_statistics_count_the_inputs_wiring_once() {
    let files = files("statistics", false, false);
    let bare = load_board(&files.bare).unwrap();
    let wired = load_board(&files.wired).unwrap();
    let routed = measure(&bare, &files.ses).unwrap();
    assert_eq!(measure(&wired, &files.ses).unwrap(), routed);
    let session = parse_session(&read_text(&files.ses).unwrap()).unwrap();
    let diff = SesDiff::new(&wired, &session, &session);
    assert_eq!(diff.a.statistics, routed);
    assert_eq!(diff.b.statistics, routed);
    // The router left NET_LOCAL_LEFT_MID unrouted.
    assert_eq!(routed.incomplete_connections, 1);
    // On top of the input the protected via counted twice and NET_CROSS_A's
    // wires twice, and the ripped trace stayed.
    let old = SessionStatistics::of(&stacked(&wired, &files.ses));
    assert_eq!(old.vias, routed.vias + 1);
    assert!(
        old.trace_length_mm > routed.trace_length_mm + 50.0,
        "{} vs {}",
        old.trace_length_mm,
        routed.trace_length_mm
    );
    // The airlines moved too: the ripped trace still connected
    // NET_LOCAL_LEFT_MID, and NET_CROSS_A, its copper doubled, read as
    // broken between U1-1 and the via.
    let mut routed_board = bare.clone();
    import_session(&mut routed_board, &session);
    assert_eq!(airline_nets(&routed_board), ["NET_LOCAL_LEFT_MID"]);
    assert_eq!(airline_nets(&stacked(&wired, &files.ses)), ["NET_CROSS_A"]);
}

fn airline_nets(board: &BasicBoard) -> Vec<String> {
    DesignRulesChecker::new(board)
        .all_airlines()
        .iter()
        .map(|line| board.rules.nets.get(line.net_number).unwrap().name.clone())
        .collect()
}

#[test]
fn the_width_report_counts_the_inputs_wiring_once() {
    let files = files("widths", false, true);
    let bare = load_board(&files.bare).unwrap();
    let wired = load_board(&files.wired).unwrap();
    let routed = widths_of(&files.bare, Some(&files.ses)).unwrap();
    let net = routed.net("NET_CROSS_A").expect("a wide net");
    assert!(net.length > 0.0 && net.under > 0.0);
    assert_eq!(widths_of(&files.wired, Some(&files.ses)).unwrap(), routed);
    assert_eq!(measure_widths(&wired, &files.ses).unwrap(), routed);
    assert_eq!(measure_widths(&bare, &files.ses).unwrap(), routed);
    let old = report(&stacked(&wired, &files.ses));
    let old_net = old.net("NET_CROSS_A").unwrap();
    assert!(old_net.length > 1.9 * net.length);
    assert!(old_net.under > 1.9 * net.under);
}

fn power_traces(dsn: &Path, ses: &Path) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_freerouting-dev"))
        .arg("power-traces")
        .arg(dsn)
        .arg(ses)
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn power_traces_counts_the_inputs_wiring_once() {
    let files = files("power", true, false);
    let wired = load_board(&files.wired).unwrap();
    let (status, routed) = power_traces(&files.bare, &files.ses);
    // Java's Bottom wire of NET_CROSS_B and the repeated protected trace: a
    // net without a plane on a power layer.
    assert_eq!(status, 1);
    let count_line = routed.lines().last().unwrap().to_string();
    assert_eq!(
        count_line,
        "power-layer traces: foreign 2, outside_own_plane 0, all 2"
    );
    assert_eq!(power_traces(&files.wired, &files.ses), (1, routed));
    let mut board = wired.clone();
    import_routed_session(
        &mut board,
        &parse_session(&read_text(&files.ses).unwrap()).unwrap(),
    );
    assert_eq!(power_layer_trace_count(&board), 2);
    assert_eq!(power_layer_trace_count(&stacked(&wired, &files.ses)), 3);
}

#[test]
fn tidiness_and_failed_connections_see_the_routed_board() {
    let files = files("tidy", false, false);
    let bare = load_board(&files.bare).unwrap();
    let wired = load_board(&files.wired).unwrap();
    let routed = tidy_session(&bare, &files.ses).unwrap();
    assert_eq!(tidy_session(&wired, &files.ses).unwrap(), routed);
    assert_ne!(tidy_board(&stacked(&wired, &files.ses)), routed);

    let session = parse_session(&read_text(&files.ses).unwrap()).unwrap();
    let report = analyze(&bare, &session, &bare, None);
    assert_eq!(report.connections.len(), 1);
    assert_eq!(report.connections[0].net, "NET_LOCAL_LEFT_MID");
    assert_eq!(analyze(&wired, &session, &bare, None), report);
}

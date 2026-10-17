//! K08.1 and K08.4: the direct `.kicad_pcb` writer on real boards.
//!
//! - `Issue180-Test` (102 segments, 627 vias, KiCad 7 file): read and written
//!   with no routing, the diff is empty and the output is the input byte for
//!   byte.
//! - `Issue558-dev-board`: Java's session for KiCad's export of the board
//!   (`dev-board.kicad10.java.ses`, `a773a70ec`, K07.4 step 1) is imported
//!   onto the board the reader built, the board is written, and the output
//!   read again gives a board with the same `BoardStatistics`.

use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::io::kicad::reader::write::CopperKind;
use freerouting::io::kicad::{
    ReadOptions, final_copper, read_kicad_input, render_kicad_board, write_kicad_board,
    write_kicad_board_checked,
};
use freerouting::io::read_session;
use freerouting::io::specctra::{BoardReadResult, build_board};
use freerouting::scoring::BoardStatistics;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kicad")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("kicad-write-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn board_at(path: &Path) -> RoutingBoard {
    let input = read_kicad_input(path, ReadOptions::default()).unwrap();
    match build_board(&input.pcb) {
        BoardReadResult::Success { board, .. } => *board,
        other => panic!("{other:?}"),
    }
}

fn issue180() -> PathBuf {
    fixtures().join("Issue180-Test/Test.kicad_pcb")
}

#[test]
fn issue180_written_without_routing_is_unchanged() {
    let path = issue180();
    let input = read_kicad_input(&path, ReadOptions::default()).unwrap();
    let index = &input.copper_index;
    let segments = index
        .items
        .iter()
        .filter(|i| i.kind == CopperKind::Segment)
        .count();
    let vias = index.items.len() - segments;
    assert_eq!((segments, vias), (102, 627));
    assert_eq!(index.style.uuid_token, "tstamp");
    assert!(!index.style.uuid_quoted);
    let board = board_at(&path);
    let items = final_copper(&board, index).unwrap();
    let (text, summary) = render_kicad_board(&path, &board, Some(input.source_hash)).unwrap();
    assert_eq!(
        (
            summary.kept,
            summary.removed,
            summary.added_segments,
            summary.added_vias
        ),
        (729, 0, 0, 0),
        "{} board items",
        items.len()
    );
    assert_eq!(text.as_bytes(), std::fs::read(&path).unwrap().as_slice());
    let written = scratch("issue180").join("Test.kicad_pcb");
    write_kicad_board(&path, &board, &written).unwrap();
    assert_eq!(
        std::fs::read(&written).unwrap(),
        std::fs::read(&path).unwrap()
    );
}

#[test]
fn the_writer_refuses_its_input_and_a_changed_file() {
    let dir = scratch("refuse");
    let copy = dir.join("Test.kicad_pcb");
    std::fs::copy(issue180(), &copy).unwrap();
    let board = board_at(&copy);
    let same = dir.join(".").join("Test.kicad_pcb");
    let e = write_kicad_board(&copy, &board, &same).unwrap_err();
    assert!(e.contains("refusing to overwrite the input"), "{e}");
    let hash = read_kicad_input(&copy, ReadOptions::default())
        .unwrap()
        .source_hash;
    let mut text = std::fs::read_to_string(&copy).unwrap();
    text.push('\n');
    std::fs::write(&copy, text).unwrap();
    let e = write_kicad_board_checked(&copy, &board, &dir.join("out.kicad_pcb"), Some(hash))
        .unwrap_err();
    assert!(e.contains("changed since the board was read"), "{e}");
    assert!(!dir.join("out.kicad_pcb").exists());
}

fn dev_board_dir() -> PathBuf {
    fixtures().join("Issue558-dev-board-autoroute-demo")
}

/// The reader's board with Java's session on it. The import marks the items
/// `protect` (user-fixed), and the reader reads KiCad's tracks as user-fixed
/// too (`(type route)`, as Java reads KiCad's export), so the fixed states
/// survive the round trip.
fn routed_dev_board() -> RoutingBoard {
    let mut board = board_at(&dev_board_dir().join("dev-board.kicad_pcb"));
    let ses = std::fs::read_to_string(dev_board_dir().join("dev-board.kicad10.java.ses")).unwrap();
    let summary = read_session(&mut board, &ses).unwrap();
    assert_eq!(summary.errors_encountered, 0);
    board
}

type TraceKey = (i32, Vec<i32>, i32, Vec<(i32, i32)>);

fn traces(board: &RoutingBoard) -> Vec<TraceKey> {
    board
        .items()
        .filter_map(|i| {
            let t = i.as_trace()?;
            let corners = t
                .polyline
                .corners()
                .iter()
                .map(|c| {
                    let p = c.as_int().expect("integer corners");
                    (p.x, p.y)
                })
                .collect();
            Some((t.layer, i.net_numbers().to_vec(), t.half_width, corners))
        })
        .collect()
}

/// Traces with their corners in a canonical direction, sorted.
fn undirected_traces(board: &RoutingBoard) -> Vec<TraceKey> {
    let mut out: Vec<TraceKey> = traces(board)
        .into_iter()
        .map(|(l, n, w, c)| {
            let r: Vec<(i32, i32)> = c.iter().rev().copied().collect();
            (l, n, w, if r < c { r } else { c })
        })
        .collect();
    out.sort();
    out
}

/// How many traces of `board` run the other way in `back` (the builder
/// orders the items of a re-read board its own way, so traces are matched by
/// their undirected geometry).
fn reversed_traces(board: &RoutingBoard, back: &RoutingBoard) -> usize {
    let directed: std::collections::BTreeSet<TraceKey> = traces(back).into_iter().collect();
    traces(board)
        .into_iter()
        .filter(|t| !directed.contains(t))
        .count()
}

#[test]
fn dev_board_round_trip_keeps_the_statistics() {
    let dir = scratch("dev-board");
    let out = dir.join("dev-board.kicad_pcb");
    std::fs::copy(
        dev_board_dir().join("dev-board.kicad_pro"),
        dir.join("dev-board.kicad_pro"),
    )
    .unwrap();
    let board = routed_dev_board();
    let input = dev_board_dir().join("dev-board.kicad_pcb");
    let summary = write_kicad_board(&input, &board, &out).unwrap();
    println!("{summary:?}");
    assert!(summary.added_segments > 0 && summary.added_vias == 35);
    let back = board_at(&out);
    // the same traces, each maybe written backwards (see `reversed_traces`)
    assert_eq!(undirected_traces(&board), undirected_traces(&back));
    let reversed = reversed_traces(&board, &back);
    println!("traces written backwards to stay apart on re-read: {reversed}");
    let (a, mut b) = (BoardStatistics::new(&board), BoardStatistics::new(&back));
    // The builder orders a re-read board's items its own way, and a trace
    // written backwards has its segment lengths summed in the other order;
    // the three per-direction sums are `f32` accumulators (Java's
    // `BoardStatisticsTraces`), so they may differ in the last bits.
    for (x, y) in [
        (
            a.traces.total_vertical_length,
            b.traces.total_vertical_length,
        ),
        (
            a.traces.total_horizontal_length,
            b.traces.total_horizontal_length,
        ),
        (a.traces.total_angled_length, b.traces.total_angled_length),
    ] {
        assert!((x - y).abs() <= 1e-5 * x.abs().max(1.0), "{x} against {y}");
    }
    b.traces.total_vertical_length = a.traces.total_vertical_length;
    b.traces.total_horizontal_length = a.traces.total_horizontal_length;
    b.traces.total_angled_length = a.traces.total_angled_length;
    assert_eq!(a, b, "every other statistic is equal");
    // writing the re-read board again changes nothing
    let (text, again) = render_kicad_board(&out, &back, None).unwrap();
    assert_eq!(
        (again.removed, again.added_segments, again.added_vias),
        (0, 0, 0)
    );
    assert_eq!(text.as_bytes(), std::fs::read(&out).unwrap().as_slice());
}

#[test]
fn writing_twice_gives_identical_files() {
    let dir = scratch("twice");
    let board = routed_dev_board();
    let input = dev_board_dir().join("dev-board.kicad_pcb");
    write_kicad_board(&input, &board, &dir.join("a.kicad_pcb")).unwrap();
    write_kicad_board(&input, &board, &dir.join("b.kicad_pcb")).unwrap();
    assert_eq!(
        std::fs::read(dir.join("a.kicad_pcb")).unwrap(),
        std::fs::read(dir.join("b.kicad_pcb")).unwrap()
    );
}

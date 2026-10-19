//! The shove insert against Java (O04.1-O04.5, O04.7).
//!
//! Probe `shove-insert` (`tools/java-reference/probes/board/ShoveInsertProbe.java`), stages in
//! the probe's order on one board (see `tests/support/shove_insert_stages.rs`):
//! - `insert`: 30 forced trace segments (200 on J2) from a sampled pin or trace end to a sampled
//!   point, depths (20, 8, 5), tidy width `i32::MAX` and 0 alternating, accuracy 500, with check;
//! - `via`: 10 forced vias with the net class's first via;
//! - `move`: 10 drill item moves of sampled vias, tidy width `i32::MAX` and 100 alternating;
//! - `attach`: a forced via onto an own-net SMD pin with the via's attach flag off, then on.
//!
//! A snapshot is taken before each step and undone after a failed one. Each step line holds the
//! outcome, the shove-failing obstacle and layer and `maxGeneratedId`; the canonical item dump
//! follows every 10th step and each stage. On a mismatch the message names the first differing
//! line, whose `step` is the first differing step (for a digest: the section, one per step).

mod board_probe;
#[path = "support/item_dump.rs"]
mod item_dump;
#[path = "support/shove_insert_stages.rs"]
mod shove_insert_stages;
#[path = "support/shove_sampler.rs"]
mod shove_sampler;

use board_probe::header;
use freerouting::settings::JavaQuirks;
use shove_insert_stages::{insert_steps, shove_insert_lines};

const KIND: &str = "shove-insert";

/// Runs every stage on `board_name` (with its fixtures.tsv session if
/// `with_session`) and compares with the probe's output.
fn check_board(board_name: &str, with_session: bool) {
    check_board_with(board_name, with_session, JavaQuirks::JAVA);
}

/// [`check_board`] with the board's switches `quirks`.
fn check_board_with(board_name: &str, with_session: bool, quirks: JavaQuirks) {
    let row = board_probe::fixture(board_name);
    let mut board = board_probe::load(&row);
    if with_session {
        board_probe::import_session(&mut board, &row);
    }
    board.set_java_quirks(quirks);
    let mut lines = vec![header(KIND, &row.board)];
    shove_insert_lines(&mut board, insert_steps(&row.board), &mut lines);
    if let Err(e) = board_probe::compare(KIND, &row, &lines) {
        panic!("{e}");
    }
}

#[test]
fn shove_insert_matches_java_issue413() {
    check_board("Issue413-test", false);
}

#[test]
fn shove_insert_matches_java_issue676() {
    check_board("Issue676-ch32v-tx118s", false);
}

#[test]
fn shove_insert_matches_java_issue957() {
    check_board("Issue957-smoke-ldo-led", false);
}

#[test]
fn shove_insert_matches_java_j2() {
    check_board("Issue026-J2_reference", true);
}

#[test]
fn shove_insert_matches_java_bm05() {
    check_board("Issue508-DAC2020_bm05", true);
}

#[test]
fn shove_insert_matches_java_bm08() {
    check_board("Issue508-DAC2020_bm08", true);
}

#[test]
fn shove_insert_matches_java_rpi_splitter() {
    check_board("Issue143-rpi_splitter", true);
}

#[test]
fn shove_insert_matches_java_smd_demo() {
    check_board("Issue508-SMD-routing-issue-demo", true);
}

#[test]
fn shove_insert_matches_java_issue110() {
    check_board("Issue110-testProjectFromFreeroutingBugTest", true);
}

#[test]
fn shove_insert_matches_java_issue742() {
    check_board("Issue742-tastexx-pcb", true);
}

/// N06.15: with `java_quirks.shove_via_into_pending_piece = false` the probe's
/// steps on J2 (200 forced inserts, with its session) still match Java's recording:
/// no via is moved onto a pending piece there, and the fix changes nothing else.
/// Its `false` half where a via does arrive is
/// `shove::insert::pending_tests::pending_piece_switch_both_behaviours_and_counters`.
#[test]
fn shove_insert_with_the_pending_piece_fix_matches_java_where_no_via_arrives() {
    let fixed = JavaQuirks::JAVA
        .with("shove_via_into_pending_piece", false)
        .unwrap();
    check_board_with("Issue026-J2_reference", true, fixed);
    check_board_with("Issue508-DAC2020_bm08", true, fixed);
}

/// `(attach off)` (D8, O04.3): a forced via onto an own-net SMD pad with a
/// via whose `ViaInfo` has attach off is refused, with the pad as the
/// shove-failing obstacle, in Rust and in the Java probe; with attach on the
/// same call succeeds. Stage `attach` of the probe on Issue742, after the
/// other stages, as the probe runs it.
#[test]
fn forced_via_respects_attach_off() {
    let row = board_probe::fixture("Issue742-tastexx-pcb");
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    let mut lines = vec![header(KIND, &row.board)];
    shove_insert_lines(&mut board, insert_steps(&row.board), &mut lines);
    let java = std::fs::read_to_string(
        board_probe::repo_root()
            .join("fixtures/reference/probe/shove-insert/Issue742-tastexx-pcb.jsonl"),
    )
    .expect("Java output");
    for (side, text) in [("Rust", lines.join("\n")), ("Java", java)] {
        let attach: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with("{\"stage\":\"attach\""))
            .collect();
        assert_eq!(attach.len(), 2, "{side}: two attach lines");
        assert!(
            attach[0].contains("\"attach\":false,\"ok\":false,\"failing\":461,"),
            "{side}: attach off must be refused at the pad: {}",
            attach[0]
        );
        assert!(
            attach[1].contains("\"attach\":true,\"ok\":true,"),
            "{side}: attach on must succeed: {}",
            attach[1]
        );
    }
}

/// Kept Java quirk (O04.5, DIVERGENCES "Java bugs kept on purpose"):
/// `DrillItemMover.insert` grows only its own copy of the tidy region, so
/// `move_drill_item` with a finite tidy width skips the pull-tight (the
/// region stays `IntOctagon.EMPTY`), while `i32::MAX` pull-tightens without
/// clip. On Issue742 with its session, traces and vias unfixed: for every via and direction the move
/// with tidy width 1000 equals the move without optimization; at least one
/// move with `i32::MAX` differs from it.
#[test]
fn move_drill_item_finite_tidy_skips_pull_tight() {
    use freerouting::board::structure::FixedState;
    use freerouting::geometry::{IntOctagon, IntVector, Vector};
    use freerouting::optimize::Clip;
    use freerouting::shove::TraceShover;

    assert_eq!(
        Clip::from_tidy_region(IntOctagon::EMPTY.enlarge(1000.0)),
        Clip::Skip
    );
    let row = board_probe::fixture("Issue742-tastexx-pcb");
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    let unit = board.min_trace_half_width();
    let mut vias = board.vias();
    vias.sort_by(|a, b| b.0.cmp(&a.0));
    // The session's traces and vias are fixed; unfix them so that the vias
    // can move and the traces be pulled tight.
    for id in board.traces().into_iter().chain(vias.iter().copied()) {
        board.set_fixed_state(id, FixedState::Unfixed);
    }
    let (mut moves, mut pulled, mut fixed_pulled) = (0, 0, 0);
    for via in vias {
        for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 1), (-1, -1)] {
            let vector = Vector::Int(IntVector {
                x: dx * 3 * unit,
                y: dy * 3 * unit,
            });
            let mut plain = board.clone();
            for contact in plain.normal_contacts(via) {
                if plain.item(contact).map(|i| i.fixed_state()) == Some(FixedState::ShoveFixed) {
                    plain.set_fixed_state(contact, FixedState::Unfixed);
                }
            }
            plain.start_marking_changed_area();
            if !TraceShover::new(&mut plain).insert_drill_item_move(via, &vector, 20, 8) {
                continue;
            }
            plain.clear_changed_area();
            let mut finite = board.clone();
            assert!(finite.move_drill_item(via, &vector, 20, 8, 1000, 500, 0));
            let mut unbounded = board.clone();
            assert!(unbounded.move_drill_item(via, &vector, 20, 8, i32::MAX, 500, 0));
            assert_eq!(
                item_dump::dump(&finite),
                item_dump::dump(&plain),
                "via {via:?} by ({dx}, {dy}): a finite tidy width must not pull tight"
            );
            let mut fixed = board.clone();
            fixed.set_java_quirks(
                freerouting::settings::JavaQuirks::JAVA
                    .with("move_drill_tidy_region", false)
                    .unwrap(),
            );
            assert!(fixed.move_drill_item(via, &vector, 20, 8, 1000, 500, 0));
            if item_dump::dump(&fixed) != item_dump::dump(&plain) {
                fixed_pulled += 1;
            }
            moves += 1;
            if item_dump::dump(&unbounded) != item_dump::dump(&plain) {
                pulled += 1;
            }
        }
    }
    assert!(
        moves > 0 && pulled > 0 && fixed_pulled > 0,
        "moves {moves}, pulled tight {pulled}"
    );
}

/// O04.8: the time of the J2 script (200 forced segments, 10 forced vias, 10
/// moves, the attach check, dumps included), without loading. Release only:
/// `cargo test --release -p freerouting --test reference_shove_insert --
/// --ignored --nocapture shove_insert_j2_timing`. The Java probe prints its
/// own time of the same script on stderr (`shove-insert script: … ms`).
#[test]
#[ignore = "release timing"]
fn shove_insert_j2_timing() {
    let row = board_probe::fixture("Issue026-J2_reference");
    let mut runs = Vec::new();
    for _ in 0..5 {
        let mut board = board_probe::load(&row);
        board_probe::import_session(&mut board, &row);
        let mut lines = Vec::new();
        let started = std::time::Instant::now();
        shove_insert_lines(&mut board, insert_steps(&row.board), &mut lines);
        runs.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    println!("shove-insert J2 script: {runs:.1?} ms");
}

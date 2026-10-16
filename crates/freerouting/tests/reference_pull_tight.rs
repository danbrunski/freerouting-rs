//! Pull-tight (O01) against Java at the pin: probe `pull-tight`
//! (`tools/java-reference/probes/board/PullTightProbe.java`, which documents
//! every mode) writes `fixtures/reference/probe/pull-tight/<board>.jsonl`,
//! one section per mode. Each test computes its sections on the Rust side and
//! compares them, section by section, with the Java file (or its `.digest`).
//!
//! - `steps_*`: every step of the 45° and the forced 90° tightener alone on
//!   200 seeded polylines per board, then the fixpoint, with the
//!   changed/unchanged flag, the result and the changed area (O01.2-O01.4);
//!   `ends_45`: the 45° end-corner smoothing of every trace.
//! - `board_*`: `opt_changed_area` of the whole marked board, 45° and forced
//!   90°, on 41 boards (the 40 smallest fixtures with a routed session, minus
//!   `Issue676-ch32v-tx118s`, plus `Issue508-DAC2020_bm05`); on J2 and
//!   DAC2020_bm08 also the default tree's fingerprint, which checks the
//!   `LineReuse` of every `change_trace` (O01.7).
//! - `pin_exit`: `pull_tight_own_net` with `pin_edge_to_turn_dist` set
//!   (O01.5); `remove_*`: `remove_items_and_pull_tight` with three tidy
//!   widths and `limit`: a work limit that fires (O01.6).
//!
//! - `steps_none`, `ends_none`, `board_none` (O02): the same with the
//!   restriction forced to `None`, against probe `pull-tight-any`
//!   (`PullTightAnyProbe.java`, `fixtures/reference/probe/pull-tight-any/`);
//!   `board_none` has no expected mismatch ([`NONE_EXPECTED_MISMATCHES`]);
//!   the reference of `Issue756-minimal-hang` comes from Java 588320207.
//!
//! The board-level modes first unfix every trace and via, because the
//! session import makes them user-fixed (Java's `SesReader` too).

mod board_probe;
#[path = "support/item_dump.rs"]
mod item_dump;

use std::collections::BTreeMap;

use board_probe::{FNV_OFFSET, Line, Row, fixture, fnv1a64, hex, repo_root};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::AngleRestriction;
use freerouting::geometry::jmath::{JavaRandom, round_to_i32};
use freerouting::geometry::{IntBox, IntOctagon, IntPoint, Line as Line2, Point, Polyline};
use freerouting::optimize::{Clip, Outcome, PullTightParams, Tightener, TrackedPolyline};
use item_dump::corners_text;

const KIND: &str = "pull-tight";
/// The any-angle probe (O02): `PullTightAnyProbe.java`.
const KIND_ANY: &str = "pull-tight-any";
const ACCURACY: i32 = 500;
const SAMPLES: usize = 200;
const STEP_BOARDS: [&str; 2] = ["Issue026-J2_reference", "Issue508-DAC2020_bm08"];

// ---------------------------------------------------------------------------
// Section comparison

/// The sections of the Java output of a probe kind for a row: name -> lines
/// (a `.jsonl`), or name -> the digest line (`name count hash`) of a `.digest`.
enum JavaSections {
    Lines(BTreeMap<String, Vec<String>>),
    Digest(BTreeMap<String, String>),
}

fn section_name(line: &str) -> Option<String> {
    line.strip_prefix("{\"stage\":\"")
        .map(|rest| rest.split('"').next().unwrap_or_default().to_string())
}

fn java_sections_of(kind: &str, row: &Row) -> JavaSections {
    let base = repo_root().join("fixtures/reference/probe").join(kind);
    let jsonl = base.join(format!("{}.jsonl", row.board));
    if let Ok(text) = std::fs::read_to_string(&jsonl) {
        let mut sections: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut current: Option<String> = None;
        for line in text.lines().skip(1) {
            if let Some(name) = section_name(line) {
                current = Some(name);
            }
            let name = current.clone().unwrap_or_default();
            sections.entry(name).or_default().push(line.to_string());
        }
        return JavaSections::Lines(sections);
    }
    let digest = base.join(format!("{}.digest", row.board));
    let text = std::fs::read_to_string(&digest).unwrap_or_else(|_| {
        panic!(
            "{kind}/{}: no Java output; generate it with {}",
            row.board,
            board_probe::probe_command(kind, row)
        )
    });
    let sections = text
        .lines()
        .skip(1)
        .map(|l| {
            (
                l.split(' ').next().unwrap_or_default().to_string(),
                l.to_string(),
            )
        })
        .collect();
    JavaSections::Digest(sections)
}

/// The digest line of one section's lines (as `generate.sh` writes it).
fn digest_line(name: &str, lines: &[String]) -> String {
    let mut hash = FNV_OFFSET;
    for line in lines {
        hash = fnv1a64(hash, line);
        hash = fnv1a64(hash, "\n");
    }
    format!("{name} {} {}", lines.len(), hex(hash))
}

/// Compares the Rust section `lines` (starting with its stage line) with
/// Java's section of the same name. Writes the Rust lines to
/// `target/probe/pull-tight/<board>.<section>.jsonl` on a mismatch.
fn compare_section(row: &Row, java: &JavaSections, lines: &[String]) -> Result<(), String> {
    let name = section_name(&lines[0]).expect("a section starts with its stage line");
    let mismatch = match java {
        JavaSections::Lines(map) => {
            let Some(j) = map.get(&name) else {
                return Err(format!("{}: Java has no section {name}", row.board));
            };
            (0..j.len().max(lines.len()))
                .find(|&i| j.get(i) != lines.get(i))
                .map(|i| {
                    format!(
                        "{}: section {name} line {} differs\n  Java: {}\n  Rust: {}",
                        row.board,
                        i + 1,
                        j.get(i).map_or("<end>", String::as_str),
                        lines.get(i).map_or("<end>", String::as_str)
                    )
                })
        }
        JavaSections::Digest(map) => {
            let rust = digest_line(&name, lines);
            match map.get(&name) {
                Some(j) if *j == rust => None,
                j => Some(format!(
                    "{}: section {name} digest differs\n  Java: {}\n  Rust: {rust}",
                    row.board,
                    j.map_or("<none>", String::as_str)
                )),
            }
        }
    };
    let Some(message) = mismatch else {
        return Ok(());
    };
    let out = repo_root()
        .join("target/probe")
        .join(KIND)
        .join(format!("{}.{name}.jsonl", row.board));
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(&out, text).unwrap();
    Err(format!("{message}\n  Rust section: {}", out.display()))
}

fn check_sections(board: &str, sections: &[Vec<String>]) {
    check_sections_of(KIND, board, sections);
}

fn check_sections_of(kind: &str, board: &str, sections: &[Vec<String>]) {
    let row = fixture(board);
    let java = java_sections_of(kind, &row);
    let failures: Vec<String> = sections
        .iter()
        .filter_map(|s| compare_section(&row, &java, s).err())
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn loaded(row: &Row, angle: AngleRestriction) -> RoutingBoard {
    let mut board = board_probe::load(row);
    board_probe::import_session(&mut board, row);
    board.rules.set_trace_angle_restriction(angle);
    board
}

/// [`loaded`], then every trace and via unfixed (`PullTightProbe.unfixed`).
fn unfixed(row: &Row, angle: AngleRestriction) -> RoutingBoard {
    let mut board = loaded(row, angle);
    let ids: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_trace() || i.as_via().is_some())
        .map(|i| i.id())
        .collect();
    for id in ids {
        board.unfix(id);
    }
    board
}

fn angle_name(angle: AngleRestriction) -> &'static str {
    match angle {
        AngleRestriction::NinetyDegree => "90",
        AngleRestriction::FortyfiveDegree => "45",
        AngleRestriction::None => "none",
    }
}

fn octagon_text(o: &IntOctagon) -> String {
    format!(
        "{} {} {} {} {} {} {} {}",
        o.left_x,
        o.bottom_y,
        o.right_x,
        o.top_y,
        o.upper_left_diagonal_x,
        o.lower_right_diagonal_x,
        o.lower_left_diagonal_x,
        o.upper_right_diagonal_x
    )
}

// ---------------------------------------------------------------------------
// Steps (O01.2-O01.4)

fn traces_ascending(board: &RoutingBoard) -> Vec<ItemId> {
    let mut ids: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_trace())
        .map(|i| i.id())
        .collect();
    ids.sort_by_key(|id| id.0);
    ids
}

/// `PullTightProbe.offset`.
fn offset(base: IntPoint, p: IntPoint, q: IntPoint, h: i32) -> IntPoint {
    let dx = q.x - p.x;
    let dy = q.y - p.y;
    let len = (f64::from(dx) * f64::from(dx) + f64::from(dy) * f64::from(dy)).sqrt();
    let nx = round_to_i32(f64::from(-dy) * f64::from(h) / len);
    let ny = round_to_i32(f64::from(dx) * f64::from(h) / len);
    IntPoint::new(base.x + nx, base.y + ny)
}

/// `PullTightProbe.sampleCorners`.
fn sample_corners(
    random: &mut JavaRandom,
    own: &Polyline,
    half_width: i32,
    variant: i32,
) -> Vec<IntPoint> {
    let n = own.corner_count();
    let c: Vec<IntPoint> = (0..n).map(|i| own.corner(i).expect_int()).collect();
    let w = half_width.max(1);
    let mut result = Vec::new();
    if variant == 1 || variant == 3 {
        let j = random.next_int(n as i32 - 1) as usize;
        let h = (random.next_int(6) + 1) * 2 * w * if random.next_boolean() { 1 } else { -1 };
        let (p, q) = (c[j], c[j + 1]);
        result.extend_from_slice(&c[..=j]);
        if variant == 1 {
            result.push(offset(p, p, q, h));
            result.push(offset(q, p, q, h));
        } else {
            let mid = IntPoint::new((p.x + q.x) / 2, (p.y + q.y) / 2);
            result.push(offset(mid, p, q, h));
        }
        result.extend_from_slice(&c[j + 1..]);
    } else {
        result.push(c[0]);
        for &corner in &c[1..n - 1] {
            let mut p = corner;
            if random.next_boolean() {
                let dx = random.next_int(2 * w + 1) - w;
                let dy = random.next_int(2 * w + 1) - w;
                p = IntPoint::new(p.x + dx, p.y + dy);
            }
            result.push(p);
        }
        result.push(c[n - 1]);
    }
    result
}

/// `PullTightProbe.clipOf`.
fn clip_of(corners: &[IntPoint]) -> IntOctagon {
    let mid = corners.len() / 2;
    let part = &corners[..=mid];
    let llx = part.iter().map(|p| p.x).min().unwrap();
    let lly = part.iter().map(|p| p.y).min().unwrap();
    let urx = part.iter().map(|p| p.x).max().unwrap();
    let ury = part.iter().map(|p| p.y).max().unwrap();
    IntBox::new(llx, lly, urx, ury).to_int_octagon()
}

fn step_names(angle: AngleRestriction) -> &'static [&'static str] {
    if angle == AngleRestriction::None {
        // `PullTightAnyProbe.STEP_NAMES`
        &[
            "skipSegmentsOfLength0",
            "reduceLines",
            "skipLines",
            "reduceCorners",
            "repositionLines",
            "smoothenCorners",
            "pullTight",
        ]
    } else if angle == AngleRestriction::NinetyDegree {
        &[
            "trySkipSecondCorner",
            "trySkipCorners",
            "repositionLines",
            "skipSegmentsOfLength0",
            "pullTight",
        ]
    } else {
        &[
            "reduceCorners",
            "smoothenCorners",
            "repositionLines",
            "skipSegmentsOfLength0",
            "pullTight",
        ]
    }
}

fn run_step(
    tightener: &mut Tightener,
    board: &mut RoutingBoard,
    step: usize,
    input: &TrackedPolyline,
) -> Outcome {
    if tightener.mode() == freerouting::optimize::AngleMode::Any {
        return match step {
            0 => tightener.skip_segments_of_length_0(board, input),
            1 => tightener.reduce_lines(board, input),
            2 => tightener.skip_lines(board, input),
            3 => tightener.reduce_corners_any(board, input),
            4 => tightener.reposition_lines_any(board, input),
            5 => tightener.smoothen_corners_any(board, input),
            _ => tightener.pull_tight(board, input.clone()),
        };
    }
    let ninety = tightener.mode() == freerouting::optimize::AngleMode::Ortho;
    match (step, ninety) {
        (0, false) => tightener.reduce_corners(board, input),
        (1, false) => tightener.smoothen_corners(board, input),
        (0, true) => tightener.try_skip_second_corner(board, input),
        (1, true) => tightener.try_skip_corners(board, input),
        (2, _) => tightener.reposition_lines(board, input),
        (3, _) => tightener.skip_segments_of_length_0(board, input),
        _ => tightener.pull_tight(board, input.clone()),
    }
}

/// The Rust lines of section `steps-<angle>` (`PullTightProbe.steps`).
fn steps_section(row: &Row, angle: AngleRestriction) -> Vec<String> {
    let mut board = loaded(row, angle);
    let mut lines = vec![
        Line::new()
            .str("stage", &format!("steps-{}", angle_name(angle)))
            .end(),
    ];
    let traces = traces_ascending(&board);
    let mut random = JavaRandom::new(20261016);
    let names = step_names(angle);
    for k in 0..SAMPLES {
        let id = traces[random.next_int(traces.len() as i32) as usize];
        let mut variant = random.next_int(5);
        let clipped = random.next_int(3) == 2;
        let item = board.item(id).unwrap().clone();
        let trace = item.as_trace().unwrap().clone();
        if (0..trace.polyline.corner_count())
            .any(|i| !matches!(trace.polyline.corner(i), Point::Int(_)))
        {
            variant = 0;
        }
        let (input, clip) = if variant == 0 {
            let clip = clipped.then(|| {
                let c: Vec<IntPoint> = (0..trace.polyline.corner_count())
                    .map(|i| match trace.polyline.corner(i) {
                        Point::Int(p) => p,
                        other => other.to_float().round(),
                    })
                    .collect();
                clip_of(&c)
            });
            (TrackedPolyline::of_trace(trace.polyline.clone()), clip)
        } else if variant == 4 {
            // a line through a corner: a segment of length 0 there
            let own = &trace.polyline;
            let j = random.next_int(own.corner_count() as i32) as usize;
            let mut dx = random.next_int(5) - 2;
            let dy = random.next_int(5) - 2;
            if dx == 0 && dy == 0 {
                dx = 1;
            }
            let p = own.corner(j).expect_int();
            let mut lines = own.lines().to_vec();
            lines.insert(j + 1, Line2::new(p, IntPoint::new(p.x + dx, p.y + dy)));
            let clip = clipped.then(|| {
                let c: Vec<IntPoint> = (0..own.corner_count())
                    .map(|i| own.corner(i).expect_int())
                    .collect();
                clip_of(&c)
            });
            (TrackedPolyline::all_new(Polyline::new(&lines)), clip)
        } else {
            let c = sample_corners(&mut random, &trace.polyline, trace.half_width, variant);
            let points: Vec<Point> = c.iter().map(|p| Point::Int(*p)).collect();
            let clip = clipped.then(|| clip_of(&c));
            (
                TrackedPolyline::all_new(Polyline::from_points(&points)),
                clip,
            )
        };
        lines.push(
            Line::new()
                .num("sample", k as i64)
                .num("trace", i64::from(id.0))
                .num("variant", i64::from(variant))
                .str(
                    "clip",
                    &clip.as_ref().map_or_else(|| "-".to_string(), octagon_text),
                )
                .str("input", &corners_text(&input.polyline))
                .end(),
        );
        if input.lines().len() < 3 {
            continue;
        }
        let mut params = PullTightParams::new(ACCURACY);
        params.clip = clip.map_or(Clip::Unbounded, Clip::Octagon);
        let mut tightener = Tightener::new(&board, &params);
        let pins = board.touching_pins_at_end_corners(id);
        tightener.set_trace(
            &board,
            trace.layer,
            trace.half_width,
            item.net_numbers(),
            item.clearance_class(),
            Some(pins),
        );
        for (step, name) in names.iter().enumerate() {
            board.clear_changed_area();
            board.start_marking_changed_area();
            let result = run_step(&mut tightener, &mut board, step, &input);
            let area = board.changed_area().unwrap().get_area(trace.layer as usize);
            let corners = match &result {
                Outcome::Unchanged => "=".to_string(),
                Outcome::Changed(p) => corners_text(&p.polyline),
            };
            lines.push(
                Line::new()
                    .str("step", name)
                    .bool("changed", result.is_changed())
                    .str("corners", &corners)
                    .str("area", &octagon_text(&area))
                    .end(),
            );
        }
        board.clear_changed_area();
    }
    lines
}

/// The Rust lines of section `ends-45` or `ends-45-split`
/// (`PullTightProbe.ends`).
fn ends_section(row: &Row, split: bool) -> Vec<String> {
    ends_section_at(row, split, AngleRestriction::FortyfiveDegree)
}

/// [`ends_section`] with the angle restriction `angle` (`ends-none` and
/// `ends-none-split` of `PullTightAnyProbe.ends`).
fn ends_section_at(row: &Row, split: bool, angle: AngleRestriction) -> Vec<String> {
    let mut board = if split {
        unfixed(row, angle)
    } else {
        loaded(row, angle)
    };
    let name = angle_name(angle);
    let stage = if split {
        format!("ends-{name}-split")
    } else {
        format!("ends-{name}")
    };
    let mut lines = vec![Line::new().str("stage", &stage).end()];
    if split {
        for id in traces_ascending(&board) {
            let Some(trace) = board
                .item(id)
                .filter(|i| i.is_on_the_board())
                .and_then(|i| i.as_trace())
            else {
                continue;
            };
            if trace.polyline.corner_count() < 3 {
                continue;
            }
            let middle = trace.polyline.corner(trace.polyline.corner_count() / 2);
            if matches!(middle, Point::Int(_)) {
                board.split_trace_at_point(id, &middle);
            }
        }
        lines.push(
            Line::new()
                .num("maxGeneratedId", max_generated_id(&board))
                .num("items", board.items().count() as i64)
                .end(),
        );
    }
    let mut tightener = Tightener::new(&board, &PullTightParams::new(ACCURACY));
    for id in traces_ascending(&board) {
        let item = board.item(id).unwrap().clone();
        let trace = item.as_trace().unwrap();
        tightener.set_trace_plain(
            trace.layer,
            trace.half_width,
            item.net_numbers(),
            item.clearance_class(),
        );
        tightener.set_contact_pins(None);
        let start = tightener.smoothen_start_corner_at_trace(&mut board, id);
        let end = tightener.smoothen_end_corner_at_trace(&mut board, id);
        if start.is_none() && end.is_none() {
            continue;
        }
        let text = |p: &Option<Polyline>| p.as_ref().map_or_else(|| "-".to_string(), corners_text);
        lines.push(
            Line::new()
                .num("trace", i64::from(id.0))
                .str("start", &text(&start))
                .str("end", &text(&end))
                .end(),
        );
    }
    lines
}

#[test]
fn steps_45_match_java() {
    for board in STEP_BOARDS {
        let row = fixture(board);
        check_sections(
            board,
            &[steps_section(&row, AngleRestriction::FortyfiveDegree)],
        );
    }
}

#[test]
fn steps_90_match_java() {
    for board in STEP_BOARDS {
        let row = fixture(board);
        check_sections(
            board,
            &[steps_section(&row, AngleRestriction::NinetyDegree)],
        );
    }
}

#[test]
fn ends_45_match_java() {
    for board in STEP_BOARDS {
        let row = fixture(board);
        check_sections(
            board,
            &[ends_section(&row, false), ends_section(&row, true)],
        );
    }
}

// ---------------------------------------------------------------------------
// Construction (O01.1)

#[test]
fn min_translate_dist_is_at_least_100() {
    let row = fixture("Issue026-J2_reference");
    let board = loaded(&row, AngleRestriction::FortyfiveDegree);
    for (accuracy, expected) in [(0, 100), (100, 100), (500, 500)] {
        let tightener = Tightener::new(&board, &PullTightParams::new(accuracy));
        assert_eq!(
            tightener.min_translate_dist(),
            expected,
            "accuracy {accuracy}"
        );
    }
}

// ---------------------------------------------------------------------------
// Board-level modes (O01.5-O01.7)

/// `PullTightProbe.BOARD_SET`.
const BOARD_SET: [&str; 41] = [
    "empty_board",
    "scoring-perfect-two-pin",
    "Issue756-minimal-ok",
    "Issue757-minimal-soe-ok",
    "Issue954-circle",
    "Issue756-minimal-hang",
    "Issue757-minimal-soe",
    "scoring-empty-net",
    "scoring-zero-length",
    "scoring-mixed-layer",
    "Issue508-SMD-routing-issue-demo",
    "Issue269-min_fr_test/min_fr_test_no_quotes",
    "Issue269-min_fr_test/min_fr_test",
    "Issue368-CorneyIslandWireless_input_design",
    "Issue143-rpi_splitter",
    "Issue270-non-ansi_bracket",
    "Issue143-rpi_splitter_mod",
    "PCBench-retroreflectors-CONGFLOCK",
    "Issue954-invalid-polygon64",
    "Issue954-polygon64",
    "Issue368-CorneyIslandWireless/corney_island_wireless",
    "Issue069-TestSensel/TestSensel-KiCad6",
    "Issue508-DAC2020/DAC2020_bm08/DAC2020_bm08.unrouted",
    "Issue508-DAC2020/DAC2020_bm08/FRv1.9.0/DAC2020_bm08.unrouted",
    "Issue508-DAC2020/DAC2020_bm08/FRv2.1.0/DAC2020_bm08.unrouted",
    "Issue508-DAC2020_bm08",
    "Issue069-TestSensel/TestSensel",
    "Issue110-testProjectFromFreeroutingBugTest",
    "Issue026-J2_reference",
    "Issue433-my-board",
    "Issue413-test",
    "Issue110-testProjectFromFreeroutingBugTest01",
    "Issue508-DAC2020/DAC2020_bm07/DAC2020_bm07.unrouted",
    "Issue508-DAC2020/DAC2020_bm07/FRv1.9.0/DAC2020_bm07.unrouted",
    "Issue508-DAC2020/DAC2020_bm07/FRv2.1.0/DAC2020_bm07.unrouted",
    "Issue508-DAC2020_bm07",
    "Issue957-smoke-ldo-led",
    "Issue508-DAC2020/DAC2020_bm05/DAC2020_bm05.unrouted",
    "Issue508-DAC2020/DAC2020_bm05/FRv1.9.0/DAC2020_bm05.unrouted",
    "Issue508-DAC2020/DAC2020_bm05/FRv2.1.0/DAC2020_bm05.unrouted",
    "Issue508-DAC2020_bm05",
];
const TREE_BOARDS: [&str; 2] = STEP_BOARDS;
const PIN_EXIT_BOARDS: [&str; 3] = [
    "Issue026-J2_reference",
    "Issue508-DAC2020_bm08",
    "Issue143-rpi_splitter",
];
const LIMIT_MS: i64 = 1;
/// Expected mismatches of `board_none_matches_java` (O02.3): none. Until
/// 588320207 (after the pin a773a70ec) `Issue756-minimal-hang` was one: Java's
/// `Item.getConnectionItems` never stopped on a closed ring, so `split_traces`
/// after the end smoothing (`removeIfCycle`) looped, in Java and in Rust. The
/// port now stops at the first item already collected, as 588320207 does,
/// and that board's `pull-tight-any` reference comes from 588320207.
const NONE_EXPECTED_MISMATCHES: [(&str, &str); 0] = [];

/// The default tree's fingerprint line, as `TreeOpsProbe.hash` writes it.
fn tree_line(stage: &str, board: &RoutingBoard) -> String {
    let tree = board.search_trees().default_tree();
    let fingerprint = tree.fingerprint();
    Line::new()
        .str("stage", stage)
        .str("tree", &tree.key())
        .num("nodes", fingerprint.nodes as i64)
        .num("leaves", fingerprint.leaves as i64)
        .num("maxDepth", fingerprint.max_depth as i64)
        .str("shapeHash", &hex(fingerprint.shape_hash))
        .str("contentDigest", &hex(tree.content_digest()))
        .end()
}

/// The sections `board-<angle>` (and `board-<angle>-tree`).
fn board_sections(row: &Row, angle: AngleRestriction, tree: bool) -> Vec<Vec<String>> {
    let mut board = unfixed(row, angle);
    let stage = format!("board-{}", angle_name(angle));
    board.start_marking_changed_area();
    board.mark_all_changed_area();
    board.opt_changed_area(&PullTightParams::new(ACCURACY));
    let mut lines = vec![Line::new().str("stage", &stage).end()];
    lines.extend(item_dump::dump(&board));
    let mut sections = vec![lines];
    if tree {
        sections.push(vec![tree_line(&format!("{stage}-tree"), &board)]);
    }
    sections
}

fn pin_at(board: &RoutingBoard, id: ItemId) -> bool {
    let contacts = board
        .trace_start_contacts(id)
        .into_iter()
        .chain(board.trace_end_contacts(id));
    contacts
        .into_iter()
        .any(|c| board.item(c).is_some_and(|i| i.is_pin()))
}

fn max_generated_id(board: &RoutingBoard) -> i64 {
    use freerouting::datastructures::IdGenerator;
    i64::from(board.communication.id_generator.max_generated_id())
}

/// The section `pin-exit` (`PullTightProbe.pinExit`).
fn pin_exit_section(row: &Row) -> Vec<String> {
    let mut board = unfixed(row, AngleRestriction::FortyfiveDegree);
    let dist = 4 * board.min_trace_half_width();
    board.rules.set_pin_edge_to_turn_dist(f64::from(dist));
    let mut lines = vec![
        Line::new()
            .str("stage", "pin-exit")
            .num("pinEdgeToTurnDist", i64::from(dist))
            .end(),
    ];
    let at_pins: Vec<ItemId> = traces_ascending(&board)
        .into_iter()
        .rev()
        .filter(|id| pin_at(&board, *id))
        .collect();
    for id in at_pins {
        if !board.item(id).is_some_and(|i| i.is_on_the_board()) {
            continue;
        }
        let result = board.pull_tight_own_net(id, ACCURACY);
        lines.push(
            Line::new()
                .num("trace", i64::from(id.0))
                .bool("result", result)
                .num("maxGeneratedId", max_generated_id(&board))
                .end(),
        );
    }
    lines.extend(item_dump::dump(&board));
    lines
}

/// The section `remove-<width>` (`PullTightProbe.remove`).
fn remove_section(row: &Row, width: i32) -> Vec<String> {
    let mut board = unfixed(row, AngleRestriction::FortyfiveDegree);
    let name = if width == i32::MAX {
        "max".to_string()
    } else {
        width.to_string()
    };
    let mut lines = vec![Line::new().str("stage", &format!("remove-{name}")).end()];
    let traces = traces_ascending(&board);
    let mut random = JavaRandom::new(20261017);
    let mut chosen: Vec<ItemId> = Vec::new();
    while chosen.len() < 10 && chosen.len() < traces.len() {
        let id = traces[random.next_int(traces.len() as i32) as usize];
        if !chosen.contains(&id) {
            chosen.push(id);
        }
    }
    let ids: Vec<String> = chosen.iter().map(|id| id.0.to_string()).collect();
    let result = board.remove_items_and_pull_tight(&chosen, width, ACCURACY);
    lines.push(
        Line::new()
            .str("removed", &ids.join(","))
            .bool("result", result)
            .end(),
    );
    lines.extend(item_dump::dump(&board));
    lines
}

/// The section `limit` (`PullTightProbe.limit`).
fn limit_section(row: &Row) -> Vec<String> {
    let mut board = unfixed(row, AngleRestriction::FortyfiveDegree);
    let mut lines = vec![
        Line::new()
            .str("stage", "limit")
            .num("limitMs", LIMIT_MS)
            .end(),
    ];
    board.start_marking_changed_area();
    board.mark_all_changed_area();
    let mut params = PullTightParams::new(ACCURACY);
    params.limit = Some(LIMIT_MS);
    let run = board.opt_changed_area(&params);
    let used = run.units_at_last_check.expect("a limited run checks") as i64;
    lines.push(
        Line::new()
            .num("checks", run.stop_checks as i64)
            .num("unitsAtLastCheck", used)
            .bool("exceeded", used > LIMIT_MS * 64_000)
            .end(),
    );
    assert_eq!(run.stopped, used > LIMIT_MS * 64_000);
    lines.extend(item_dump::dump(&board));
    lines
}

fn board_set_matches(angle: AngleRestriction) {
    let mut failures = Vec::new();
    let kind = if angle == AngleRestriction::None {
        KIND_ANY
    } else {
        KIND
    };
    for board in BOARD_SET {
        if angle == AngleRestriction::None
            && NONE_EXPECTED_MISMATCHES.iter().any(|(b, _)| *b == board)
        {
            continue;
        }
        let row = fixture(board);
        let java = java_sections_of(kind, &row);
        let tree = TREE_BOARDS.contains(&board) && angle != AngleRestriction::NinetyDegree;
        for section in board_sections(&row, angle, tree) {
            if let Err(e) = compare_section(&row, &java, &section) {
                failures.push(e);
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} boards differ:\n{}",
        failures.len(),
        BOARD_SET.len(),
        failures.join("\n")
    );
}

#[test]
fn board_45_matches_java() {
    board_set_matches(AngleRestriction::FortyfiveDegree);
}

#[test]
fn board_90_matches_java() {
    board_set_matches(AngleRestriction::NinetyDegree);
}

// ---------------------------------------------------------------------------
// Any angle (O02): probe `pull-tight-any`

/// O02.1, O02.2: every any-angle step alone on the 200 seeded polylines of
/// `steps-45` per board, then the fixpoint, with the restriction forced to
/// `None`.
#[test]
fn steps_none_match_java() {
    for board in STEP_BOARDS {
        let row = fixture(board);
        check_sections_of(
            KIND_ANY,
            board,
            &[steps_section(&row, AngleRestriction::None)],
        );
    }
}

/// O02.2: the any-angle start and end smoothing of every trace, on the
/// imported board and on the unfixed board split at middle corners.
#[test]
fn ends_none_match_java() {
    for board in STEP_BOARDS {
        let row = fixture(board);
        check_sections_of(
            KIND_ANY,
            board,
            &[
                ends_section_at(&row, false, AngleRestriction::None),
                ends_section_at(&row, true, AngleRestriction::None),
            ],
        );
    }
}

/// O02.3: `opt_changed_area` of the whole marked board with the restriction
/// forced to `None`, on the 41 boards of `board_45_matches_java`; on J2 and
/// DAC2020_bm08 also the default tree's fingerprint.
#[test]
fn board_none_matches_java() {
    board_set_matches(AngleRestriction::None);
}

#[test]
fn pin_exit_matches_java() {
    for board in PIN_EXIT_BOARDS {
        let row = fixture(board);
        check_sections(board, &[pin_exit_section(&row)]);
    }
}

#[test]
fn remove_items_and_pull_tight_matches_java() {
    let row = fixture("Issue026-J2_reference");
    let sections: Vec<Vec<String>> = [i32::MAX, 0, 2000]
        .into_iter()
        .map(|w| remove_section(&row, w))
        .collect();
    check_sections("Issue026-J2_reference", &sections);
}

#[test]
fn work_limit_cuts_off_like_java() {
    let row = fixture("Issue508-DAC2020_bm08");
    check_sections("Issue508-DAC2020_bm08", &[limit_section(&row)]);
}

/// `changed_area_lifecycle` (tests/java_ports.rs) extended to the optimizer
/// (O01.6): `opt_changed_area` drops the changed area, with `Clip::Skip` it
/// changes no trace, and without marking it does nothing.
#[test]
fn changed_area_lifecycle_with_pull_tight() {
    let row = fixture("Issue508-DAC2020_bm08");
    let mut board = unfixed(&row, AngleRestriction::FortyfiveDegree);
    let before = item_dump::dump(&board);
    // not marking: nothing happens
    let run = board.opt_changed_area(&PullTightParams::new(ACCURACY));
    assert_eq!(run, Default::default());
    assert_eq!(item_dump::dump(&board), before);
    // Clip::Skip: the tightener doesn't run, the area is dropped
    board.start_marking_changed_area();
    board.mark_all_changed_area();
    let mut params = PullTightParams::new(ACCURACY);
    params.clip = Clip::Skip;
    let run = board.opt_changed_area(&params);
    assert!(board.changed_area().is_none());
    assert_eq!(run.stop_checks, 0);
    assert_eq!(item_dump::dump(&board), before);
    // unbounded: traces change and the area is gone afterwards
    board.start_marking_changed_area();
    board.mark_all_changed_area();
    let run = board.opt_changed_area(&PullTightParams::new(ACCURACY));
    assert!(board.changed_area().is_none());
    assert!(run.stop_checks > 0 && !run.stopped);
    assert_ne!(item_dump::dump(&board), before);
}

/// O01.8: the Rust time of mode `board-45` on DAC2020_bm08 and J2 (the
/// median of 5 runs of `opt_changed_area`, load excluded), printed for
/// comparison with the `TIMING` lines the Java probe logs. Measured, not
/// gated.
///
/// ```sh
/// cargo test --release -p freerouting --test reference_pull_tight -- --ignored --nocapture board_mode_timing
/// ```
#[test]
#[ignore = "timing; run in release with --ignored"]
fn board_mode_timing() {
    for name in ["Issue508-DAC2020_bm08", "Issue026-J2_reference"] {
        let row = fixture(name);
        let mut times: Vec<std::time::Duration> = (0..5)
            .map(|_| {
                let mut board = unfixed(&row, AngleRestriction::FortyfiveDegree);
                board.start_marking_changed_area();
                board.mark_all_changed_area();
                let start = std::time::Instant::now();
                board.opt_changed_area(&PullTightParams::new(ACCURACY));
                start.elapsed()
            })
            .collect();
        times.sort();
        eprintln!(
            "TIMING {name} board-45 rust median={:?} min={:?} max={:?}",
            times[2], times[0], times[4]
        );
    }
}

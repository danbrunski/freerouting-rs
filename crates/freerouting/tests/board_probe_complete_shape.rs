//! Board probe `complete-shape` (B03): free-space room completion
//! (`BasicBoard::complete_shape`, Java `ShapeSearchTree.completeShape` and
//! its 45 and 90 degree overrides) equals Java's on seeded queries, room for
//! room, with the work units of every call (B03.7), in
//! `fixtures/reference/probe/complete-shape/<board>.jsonl`, which
//! `tools/java-reference/probes/board/CompleteShapeProbe.java` writes at
//! `a773a70ec`. See the probe's doc comment for the queries and stages.

mod board_probe;

use std::collections::BTreeMap;

use board_probe::{Line, Row, digest, fixture, header, hex, repo_root};
use freerouting::board::RoutingBoard;
use freerouting::board::search::complete_shape::CompletionScratch;
use freerouting::board::search::rooms::IncompleteRoom;
use freerouting::board::search::{RoomId, ShapeSearchTree, TreeObject};
use freerouting::board::structure::AngleRestriction;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{IntBox, TileShape};

const KIND: &str = "complete-shape";
const SEED: i64 = 20261015;
const QUERIES: usize = 500;
const ROOM_QUERIES: usize = 200;
const INSERTED_ROOMS: usize = 100;

/// The boards and their variants (stages); stage `with-rooms` follows `45`.
const VARIANTS: &[(&str, &[&str])] = &[
    ("Issue026-J2_reference", &["45", "90", "plain"]),
    ("Issue508-DAC2020_bm08", &["45", "90", "plain"]),
    ("Issue413-test", &["45"]),
    ("Issue145-smoothieboard", &["45"]),
    ("Issue676-ch32v-tx118s", &["45"]),
];

/// (board, stage, reason) pairs known to differ from Java.
const EXPECTED_MISMATCHES: &[(&str, &str, &str)] = &[];

/// The seeded queries of one variant, drawn as the Java probe draws them.
struct Sampler {
    random: JavaRandom,
    boxes: bool,
    bounds: IntBox,
    width: i32,
    height: i32,
    layers: i32,
    max_net: i32,
}

struct Query {
    layer: i32,
    x: i32,
    y: i32,
    h: i32,
    net: i32,
    bounded: bool,
    room: IncompleteRoom,
}

impl Sampler {
    fn new(board: &RoutingBoard, boxes: bool) -> Self {
        let bounds = board.bounding_box();
        Sampler {
            random: JavaRandom::new(SEED),
            boxes,
            bounds,
            width: bounds.ur.x - bounds.ll.x,
            height: bounds.ur.y - bounds.ll.y,
            layers: board.layer_count() as i32,
            max_net: board.rules.nets.max_net_number(),
        }
    }

    fn next(&mut self) -> Query {
        let layer = self.random.next_int(self.layers);
        let x = self.bounds.ll.x + self.random.next_int(self.width + 1);
        let y = self.bounds.ll.y + self.random.next_int(self.height + 1);
        let h = 1 + self
            .random
            .next_int((self.width.min(self.height) / 50).max(1));
        let net = self.random.next_int(self.max_net + 1);
        let bounded = self.random.next_boolean();
        let contained = self.square(x, y, h);
        let start = bounded.then(|| self.square(x, y, 20i32.wrapping_mul(h)));
        Query {
            layer,
            x,
            y,
            h,
            net,
            bounded,
            room: IncompleteRoom::new(start, layer, Some(contained)),
        }
    }

    /// Java `new IntBox(x - h, y - h, x + h, y + h)` (int arithmetic), as an
    /// octagon unless the variant uses boxes.
    fn square(&self, x: i32, y: i32, h: i32) -> TileShape {
        let square = IntBox::new(
            x.wrapping_sub(h),
            y.wrapping_sub(h),
            x.wrapping_add(h),
            y.wrapping_add(h),
        );
        if self.boxes {
            TileShape::IntBox(square)
        } else {
            TileShape::IntOctagon(square.to_int_octagon())
        }
    }
}

impl Query {
    fn line(&self, stage: &str, q: usize) -> Line {
        Line::new()
            .str("stage", stage)
            .num("q", q as i64)
            .num("layer", i64::from(self.layer))
            .num("x", i64::from(self.x))
            .num("y", i64::from(self.y))
            .num("h", i64::from(self.h))
            .num("net", i64::from(self.net))
            .bool("bounded", self.bounded)
    }
}

/// A shape as the probe writes it.
fn shape_text(shape: Option<&TileShape>) -> String {
    match shape {
        None => "-".to_string(),
        Some(TileShape::IntOctagon(o)) => format!(
            "O {} {} {} {} {} {} {} {}",
            o.left_x,
            o.bottom_y,
            o.right_x,
            o.top_y,
            o.upper_left_diagonal_x,
            o.lower_right_diagonal_x,
            o.lower_left_diagonal_x,
            o.upper_right_diagonal_x
        ),
        Some(TileShape::IntBox(b)) => format!("B {} {} {} {}", b.ll.x, b.ll.y, b.ur.x, b.ur.y),
        Some(TileShape::Simplex(s)) => {
            let mut text = "S".to_string();
            for line in s.lines() {
                let (a, b) = (line.a(), line.b());
                text.push_str(&format!(" {} {} {} {}", a.x, a.y, b.x, b.y));
            }
            text
        }
    }
}

fn rooms_json(rooms: &[IncompleteRoom]) -> String {
    let items: Vec<String> = rooms
        .iter()
        .map(|r| {
            format!(
                "\"{} ; {} ; {}\"",
                shape_text(r.shape.as_ref()),
                r.layer,
                shape_text(r.contained.as_ref())
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// The smallest non-zero trace clearance class of the net classes, else 0.
fn tree_class(board: &RoutingBoard) -> i32 {
    let classes = &board.rules.net_classes;
    (0..classes.count())
        .filter_map(|i| classes.nth(i))
        .map(|id| classes.get(id).trace_clearance_class())
        .filter(|&c| c > 0)
        .min()
        .unwrap_or(0)
}

struct Run<'a> {
    board: &'a mut RoutingBoard,
    class: i32,
    scratch: CompletionScratch,
    lines: Vec<String>,
}

impl Run<'_> {
    /// One completion call: its line and its rooms.
    fn complete(
        &mut self,
        line: Line,
        query: &Query,
        ignore: Option<TreeObject>,
        ignore_shape: Option<&TileShape>,
    ) -> Vec<IncompleteRoom> {
        let meter_before = self.board.search_trees().work_meter().units();
        let rooms = self.board.complete_shape(
            self.class,
            &query.room,
            query.net,
            ignore,
            ignore_shape,
            &mut self.scratch,
        );
        let units = self.board.search_trees().work_meter().units() - meter_before;
        self.lines.push(
            line.num("units", units as i64)
                .raw("rooms", &rooms_json(&rooms))
                .end(),
        );
        rooms
    }

    fn tree(&self) -> &ShapeSearchTree {
        self.board
            .search_trees()
            .tree_by_class(self.class)
            .expect("the autoroute tree")
    }

    fn hash(&mut self, step: &str, rooms: usize) {
        let tree = self.tree();
        let fingerprint = tree.fingerprint();
        let line = Line::new()
            .str("stage", "with-rooms")
            .str("step", step)
            .num("rooms", rooms as i64)
            .num("nodes", fingerprint.nodes as i64)
            .num("leaves", fingerprint.leaves as i64)
            .num("maxDepth", fingerprint.max_depth as i64)
            .str("shapeHash", &hex(fingerprint.shape_hash))
            .str("contentDigest", &hex(tree.content_digest()))
            .end();
        self.lines.push(line);
    }

    fn with_rooms(&mut self, sampler: &mut Sampler, results: &[IncompleteRoom]) {
        let mut inserted: Vec<(RoomId, TileShape)> = Vec::new();
        for (i, result) in results.iter().take(INSERTED_ROOMS).enumerate() {
            let id = RoomId(i as i32 + 1);
            let shape = result.shape.clone().expect("result rooms have shapes");
            assert!(
                self.board
                    .insert_room(self.class, id, shape.clone(), result.layer)
            );
            inserted.push((id, shape));
        }
        self.hash("inserted", inserted.len());
        for q in 0..ROOM_QUERIES {
            let query = sampler.next();
            let mut ignore = None;
            let mut ignore_shape = None;
            if !inserted.is_empty() {
                let index = sampler.random.next_int(inserted.len() as i32) as usize;
                let (id, shape) = &inserted[index];
                ignore = Some((
                    *id,
                    shape.intersection(query.room.contained.as_ref().unwrap()),
                ));
                ignore_shape = ignore.as_ref().map(|(_, s)| s.clone());
            }
            let line = query
                .line("with-rooms", QUERIES + q)
                .num(
                    "ignore",
                    ignore.as_ref().map_or(0, |(id, _)| i64::from(id.0)),
                )
                .str("ignoreShape", &shape_text(ignore_shape.as_ref()));
            let object = ignore.map(|(id, _)| TreeObject::Room(id));
            self.complete(line, &query, object, ignore_shape.as_ref());
        }
        for (id, _) in &inserted {
            assert!(self.board.remove_room(self.class, *id));
        }
        self.hash("removed", inserted.len());
    }
}

fn variant(lines: &mut Vec<String>, row: &Row, stage: &str) {
    let mut board = board_probe::load(row);
    let angle = match stage {
        "90" => AngleRestriction::NinetyDegree,
        "plain" => AngleRestriction::None,
        _ => AngleRestriction::FortyfiveDegree,
    };
    board.rules.set_trace_angle_restriction(angle);
    let class = tree_class(&board);
    let tree = board.autoroute_search_tree(class);
    lines.push(
        Line::new()
            .str("stage", stage)
            .str("tree", &tree.key())
            .num("class", i64::from(class))
            .num("leaves", tree.size() as i64)
            .end(),
    );
    let mut sampler = Sampler::new(&board, stage == "90");
    let mut run = Run {
        board: &mut board,
        class,
        scratch: CompletionScratch::new(),
        lines: Vec::new(),
    };
    let mut results = Vec::new();
    for q in 0..QUERIES {
        let query = sampler.next();
        let line = query.line(stage, q);
        results.extend(run.complete(line, &query, None, None));
    }
    if stage == "45" {
        run.with_rooms(&mut sampler, &results);
    }
    lines.append(&mut run.lines);
}

fn rust_lines(row: &Row, stages: &[&str]) -> Vec<String> {
    let mut lines = vec![header(KIND, &row.board)];
    for stage in stages {
        variant(&mut lines, row, stage);
    }
    lines
}

/// The stage of a probe line (`{"stage":"<name>"…`) or of a digest line
/// (`<name> <count> <hash>`).
fn stage_of(line: &str) -> String {
    match line.strip_prefix("{\"stage\":\"") {
        Some(rest) => rest.split('"').next().unwrap_or_default().to_string(),
        None => line.split(' ').next().unwrap_or_default().to_string(),
    }
}

fn by_stage(lines: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut result: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in lines.iter().skip(1) {
        result.entry(stage_of(line)).or_default().push(line.clone());
    }
    result
}

/// Java's lines and whether they are a digest.
fn java_lines(board: &str) -> (Vec<String>, bool) {
    let base = repo_root().join("fixtures/reference/probe").join(KIND);
    if let Ok(text) = std::fs::read_to_string(base.join(format!("{board}.jsonl"))) {
        return (text.lines().map(str::to_string).collect(), false);
    }
    let text = std::fs::read_to_string(base.join(format!("{board}.digest")))
        .unwrap_or_else(|_| panic!("{KIND}/{board}: no Java output"));
    (text.lines().map(str::to_string).collect(), true)
}

fn write_rust(board: &str, lines: &[String]) -> String {
    let dir = std::env::var("BOARD_PROBE_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("target/probe"));
    let path = dir.join(KIND).join(format!("{board}.jsonl"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

/// The query inputs of a line: everything before `"units"`.
fn inputs(line: &str) -> Option<&str> {
    line.find(",\"units\":").map(|end| &line[..end])
}

#[test]
fn complete_shape_matches_java() {
    let mut failures = Vec::new();
    for (board, stages) in VARIANTS {
        let row = fixture(board);
        let rust = rust_lines(&row, stages);
        let path = write_rust(board, &rust);
        let (java, is_digest) = java_lines(board);
        let rust_cmp = if is_digest {
            digest(&rust)
        } else {
            rust.clone()
        };
        let java_stages = by_stage(&java);
        let rust_stages = by_stage(&rust_cmp);
        let mut names: Vec<&String> = java_stages.keys().chain(rust_stages.keys()).collect();
        names.sort();
        names.dedup();
        for stage in names {
            let empty = Vec::new();
            let j = java_stages.get(stage).unwrap_or(&empty);
            let r = rust_stages.get(stage).unwrap_or(&empty);
            let differs = (0..j.len().max(r.len())).find(|&i| j.get(i) != r.get(i));
            let expected = EXPECTED_MISMATCHES
                .iter()
                .any(|(b, s, _)| b == board && s == stage);
            match (differs, expected) {
                (Some(i), false) => failures.push(format!(
                    "{board} {stage}: line {} of the stage differs\n  Java: {}\n  Rust: {}\n  Rust output: {path}",
                    i + 1,
                    j.get(i).map_or("<end>", String::as_str),
                    r.get(i).map_or("<end>", String::as_str),
                )),
                (None, true) => failures.push(format!(
                    "{board} {stage}: now equal; remove it from EXPECTED_MISMATCHES"
                )),
                _ => {}
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The Rust sampler draws Java's inputs exactly, also for the variants
/// whose results still differ: the input fields of every query line of the
/// stages `45`, `90` and `plain` (stage `with-rooms` depends on results).
#[test]
fn sampled_inputs_match_java() {
    for (board, stages) in VARIANTS {
        let (java, is_digest) = java_lines(board);
        if is_digest {
            continue;
        }
        let row = fixture(board);
        for stage in *stages {
            let mut board_state = board_probe::load(&row);
            if *stage == "90" {
                board_state
                    .rules
                    .set_trace_angle_restriction(AngleRestriction::NinetyDegree);
            }
            let mut sampler = Sampler::new(&board_state, *stage == "90");
            let rust: Vec<String> = (0..QUERIES)
                .map(|q| {
                    let line = sampler.next().line(stage, q).end();
                    line[..line.len() - 1].to_string()
                })
                .collect();
            let java_inputs: Vec<&str> = java
                .iter()
                .filter(|l| stage_of(l) == *stage)
                .filter_map(|l| inputs(l))
                .collect();
            assert_eq!(java_inputs.len(), QUERIES, "{board} {stage}: query count");
            for (q, (j, r)) in java_inputs.iter().zip(&rust).enumerate() {
                assert_eq!(
                    *j,
                    r.as_str(),
                    "{board} {stage}: inputs of query {q} differ"
                );
            }
        }
    }
}

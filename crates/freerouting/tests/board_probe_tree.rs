//! Board probe `tree` (B02.2): the search trees have Java's shape, not only
//! its query results (decision D5). Per fixture, tree and stage the node and
//! leaf counts, the maximum depth, the hash of the canonical preorder walk
//! and the content digest equal `fixtures/reference/probe/tree/<board>.jsonl`,
//! which `tools/java-reference/probes/board/TreeProbe.java` writes.
//!
//! Stages: `load` (the default tree); `autoroute` (the tree of every
//! distinct trace clearance class of the net classes, ascending, when it is
//! new); with a session `ses` (every tree after the import), `deep-copy`
//! (the default tree of `deep_copy()` of the `ses` board), and on a second
//! board `undo` and `redo` (from the `autoroute` state: `generate_snapshot`,
//! import, `undo`, `redo`; every tree).
//!
//! On a difference the test names every (board, stage) that differs. For a
//! board under 2 000 leaves it writes the Rust walk of each differing tree
//! to `target/probe/tree/<board>/<stage>.<tree>.walk`; Java's walk of the
//! same tree comes from the probe's `--walk <stage> <tree>` mode (the
//! command is in the message).

mod board_probe;

use std::collections::BTreeSet;

use board_probe::{Line, Row, compare, fixtures, header, hex, repo_root};
use freerouting::board::RoutingBoard;
use freerouting::board::search::ShapeSearchTree;

/// The boards a debug build runs (the workspace gate); release runs all.
const DEBUG_BOARDS: &[&str] = &[
    "Issue026-J2_reference",
    "Issue145-smoothieboard",
    "Issue413-test",
    "Issue433-my-board",
    "Issue508-DAC2020_bm08",
    "Issue676-ch32v-tx118s",
    "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
    "Issue723-CombineStackOverflow",
];

/// (board, stage, reason) where Rust's trees differ from Java's today.
const EXPECTED_MISMATCHES: &[(&str, &str, &str)] = &[
    (
        "Issue676-ch32v-tx118s",
        "ses",
        "Java's SesReader fails on all 48 wires with a ClassCastException (RationalVector to \
     IntVector) and imports none; the port imports them (docs/DIVERGENCES.md, as in \
     board_probe_ids)",
    ),
    (
        "Issue676-ch32v-tx118s",
        "deep-copy",
        "Java's SesReader fails on all 48 wires with a ClassCastException (RationalVector to \
     IntVector) and imports none; the port imports them (docs/DIVERGENCES.md, as in \
     board_probe_ids)",
    ),
    (
        "Issue676-ch32v-tx118s",
        "redo",
        "Java's SesReader fails on all 48 wires with a ClassCastException (RationalVector to \
     IntVector) and imports none; the port imports them (docs/DIVERGENCES.md, as in \
     board_probe_ids)",
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
        "autoroute",
        "not a tree difference: the DSN reader gives the pins and vias of nets in a net class \
     that class's clearance class (pin 2: class 3, GND), Java keeps the default ones (pin 2: \
     class 2), so their shapes in the compensated trees cc3..cc33 differ by 10; the default \
     tree and DRC are unaffected. A load bug in io/specctra/parser/network.rs (C04)",
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
        "ses",
        "not a tree difference: the DSN reader gives the pins and vias of nets in a net class \
     that class's clearance class (pin 2: class 3, GND), Java keeps the default ones (pin 2: \
     class 2), so their shapes in the compensated trees cc3..cc33 differ by 10; the default \
     tree and DRC are unaffected. A load bug in io/specctra/parser/network.rs (C04)",
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
        "undo",
        "not a tree difference: the DSN reader gives the pins and vias of nets in a net class \
     that class's clearance class (pin 2: class 3, GND), Java keeps the default ones (pin 2: \
     class 2), so their shapes in the compensated trees cc3..cc33 differ by 10; the default \
     tree and DRC are unaffected. A load bug in io/specctra/parser/network.rs (C04)",
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
        "redo",
        "not a tree difference: the DSN reader gives the pins and vias of nets in a net class \
     that class's clearance class (pin 2: class 3, GND), Java keeps the default ones (pin 2: \
     class 2), so their shapes in the compensated trees cc3..cc33 differ by 10; the default \
     tree and DRC are unaffected. A load bug in io/specctra/parser/network.rs (C04)",
    ),
];

/// Boards under this many leaves get their walks written on a difference.
const WALK_LEAF_LIMIT: usize = 2_000;

struct Probe {
    lines: Vec<String>,
    /// (stage, tree key, walk) of every logged tree under the leaf limit.
    walks: Vec<(String, String, Vec<String>)>,
}

impl Probe {
    fn hash(&mut self, stage: &str, tree: &ShapeSearchTree) {
        let fingerprint = tree.fingerprint();
        self.lines.push(
            Line::new()
                .str("stage", stage)
                .str("tree", &tree.key())
                .num("nodes", fingerprint.nodes as i64)
                .num("leaves", fingerprint.leaves as i64)
                .num("maxDepth", fingerprint.max_depth as i64)
                .str("shapeHash", &hex(fingerprint.shape_hash))
                .str("contentDigest", &hex(tree.content_digest()))
                .end(),
        );
        if fingerprint.leaves < WALK_LEAF_LIMIT {
            self.walks
                .push((stage.to_string(), tree.key(), tree.walk_lines()));
        }
    }

    fn hash_all(&mut self, stage: &str, board: &RoutingBoard) {
        for tree in board.search_trees().trees() {
            self.hash(stage, tree);
        }
    }
}

/// The distinct trace clearance classes of the net classes, ascending.
fn trace_clearance_classes(board: &RoutingBoard) -> BTreeSet<i32> {
    let classes = &board.rules.net_classes;
    (0..classes.count())
        .filter_map(|i| classes.nth(i))
        .map(|id| classes.get(id).trace_clearance_class())
        .collect()
}

fn autoroute_trees(probe: Option<&mut Probe>, board: &mut RoutingBoard) {
    let mut probe = probe;
    for class in trace_clearance_classes(board) {
        let before = board.search_trees().trees().len();
        board.autoroute_search_tree(class);
        if board.search_trees().trees().len() > before
            && let Some(probe) = probe.as_deref_mut()
        {
            let tree = board.search_trees().trees().last().expect("new tree");
            probe.hash("autoroute", tree);
        }
    }
}

fn rust_probe(row: &Row) -> Probe {
    let mut probe = Probe {
        lines: vec![header("tree", &row.board)],
        walks: Vec::new(),
    };
    let mut board = board_probe::load(row);
    probe.hash("load", board.search_trees().default_tree());
    autoroute_trees(Some(&mut probe), &mut board);
    if row.ses.is_some() {
        board_probe::import_session(&mut board, row);
        probe.hash_all("ses", &board);
        let copy = board.deep_copy();
        probe.hash("deep-copy", copy.search_trees().default_tree());

        let mut second = board_probe::load(row);
        autoroute_trees(None, &mut second);
        second.generate_snapshot();
        board_probe::import_session(&mut second, row);
        second.undo(None);
        probe.hash_all("undo", &second);
        second.redo(None);
        probe.hash_all("redo", &second);
    }
    probe
}

/// The `stage` and `tree` of a probe line.
fn stage_and_tree(line: &str) -> Option<(String, String)> {
    let field = |key: &str| {
        let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
        let end = start + line[start..].find('"')?;
        Some(line[start..end].to_string())
    };
    Some((field("stage")?, field("tree")?))
}

/// The (stage, tree) of every line that differs from the committed Java
/// output, the header excepted.
fn differing(row: &Row, rust: &[String]) -> Vec<(String, String)> {
    let path = repo_root()
        .join("fixtures/reference/probe/tree")
        .join(format!("{}.jsonl", row.board));
    let java_text = std::fs::read_to_string(path).unwrap_or_default();
    let java: Vec<&str> = java_text.lines().collect();
    let mut result = Vec::new();
    for i in 1..java.len().max(rust.len()) {
        let (j, r) = (java.get(i).copied(), rust.get(i).map(String::as_str));
        if j != r {
            let key = j
                .and_then(stage_and_tree)
                .or_else(|| r.and_then(stage_and_tree))
                .unwrap_or_else(|| ("exception".to_string(), "-".to_string()));
            if !result.contains(&key) {
                result.push(key);
            }
        }
    }
    result
}

fn write_walks(row: &Row, probe: &Probe, keys: &[(String, String)]) -> Vec<String> {
    let dir = repo_root().join("target/probe/tree").join(&row.board);
    let mut written = Vec::new();
    for (stage, tree, walk) in &probe.walks {
        if !keys.contains(&(stage.clone(), tree.clone())) {
            continue;
        }
        std::fs::create_dir_all(&dir).expect("walk directory");
        let path = dir.join(format!("{stage}.{tree}.walk"));
        let text: String = walk.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(&path, text).expect("walk file");
        written.push(format!(
            "    {} (Java: add `--walk {stage} {tree}` to the probe command)",
            path.display()
        ));
    }
    written
}

#[test]
fn trees_match_java() {
    let mut failures = Vec::new();
    let mut seen = Vec::new();
    // A debug build runs `DEBUG_BOARDS` only (`board_probe::board_filter`).
    let only = board_probe::board_filter(DEBUG_BOARDS);
    for row in fixtures() {
        if only.as_ref().is_some_and(|o| !o.contains(&row.board)) {
            continue;
        }
        let probe = rust_probe(&row);
        let Err(message) = compare("tree", &row, &probe.lines) else {
            continue;
        };
        let keys = differing(&row, &probe.lines);
        let mut unexpected = Vec::new();
        for (stage, tree) in &keys {
            match EXPECTED_MISMATCHES
                .iter()
                .find(|(board, s, _)| *board == row.board && s == stage)
            {
                Some(entry) => {
                    if !seen.contains(entry) {
                        seen.push(*entry);
                    }
                }
                None => unexpected.push(format!("{stage} {tree}")),
            }
        }
        if !unexpected.is_empty() || keys.is_empty() {
            let walks = write_walks(&row, &probe, &keys);
            failures.push(format!(
                "{}: unexpected differences in {}\n{message}\n  walks:\n{}",
                row.board,
                unexpected.join(", "),
                walks.join("\n")
            ));
        }
    }
    for entry in EXPECTED_MISMATCHES {
        let ran = only.as_ref().is_none_or(|o| o.iter().any(|b| b == entry.0));
        if ran && !seen.contains(entry) {
            failures.push(format!(
                "listed mismatch {} {} no longer occurs; remove it from EXPECTED_MISMATCHES",
                entry.0, entry.1
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

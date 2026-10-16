//! Board-probe support (B01.1): the Rust side of the Java probes in
//! `tools/java-reference/probes/board/`.
//!
//! Every B probe loads its boards from `fixtures.tsv` the same way on both
//! sides and writes the same JSONL text (B00 "Board probes"): one JSON object
//! per line, keys in a fixed order, integers in decimal, every `double` as
//! `"0x"` + 16 hex digits of its raw bits. The first line is a header; only its
//! `kind` and `board` are compared, because it also names the Java commit and
//! JDK. [`compare`] checks the rest line for line against
//! `fixtures/reference/probe/<kind>/<board>.jsonl` (or the `.digest` that
//! replaces an output above 1 MiB).
//!
//! When `BOARD_PROBE_OUT` is set, [`compare`] also writes the Rust lines to
//! `$BOARD_PROBE_OUT/<kind>/<board>.jsonl`, so `tools/determinism-check.sh`
//! can compare two runs.
//!
//! Other B checklists append helpers here; they don't change these.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::drc::load_design;
use freerouting::io::{SesImportSummary, read_session};

/// The FNV-1a 64 offset basis.
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

/// FNV-1a 64 of `text`'s UTF-8 bytes, continuing from `hash`.
pub fn fnv1a64(mut hash: u64, text: &str) -> u64 {
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// A hash as the probes write it: 16 lowercase hex digits.
pub fn hex(value: u64) -> String {
    format!("{value:016x}")
}

/// A `double` as the probes write it: `0x` + its raw bits.
pub fn bits(value: f64) -> String {
    format!("0x{:016x}", value.to_bits())
}

/// The repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One row of `fixtures.tsv`.
#[derive(Clone, Debug)]
pub struct Row {
    /// The DSN path below the repository root.
    pub dsn: String,
    /// The session path below the repository root, if any.
    pub ses: Option<String>,
    /// The DSN path below `fixtures/` without `.dsn`.
    pub board: String,
}

/// The rows of `tools/java-reference/probes/board/fixtures.tsv`.
pub fn fixtures() -> Vec<Row> {
    let path = repo_root().join("tools/java-reference/probes/board/fixtures.tsv");
    let text = std::fs::read_to_string(&path).expect("fixtures.tsv");
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| {
            let (dsn, ses) = line.split_once('\t').expect("two tab-separated columns");
            let board = dsn
                .strip_prefix("fixtures/")
                .unwrap_or(dsn)
                .trim_end_matches(".dsn")
                .to_string();
            Row {
                dsn: dsn.to_string(),
                ses: (ses != "-").then(|| ses.to_string()),
                board,
            }
        })
        .collect()
}

/// The row of `board`.
pub fn fixture(board: &str) -> Row {
    fixtures()
        .into_iter()
        .find(|r| r.board == board)
        .unwrap_or_else(|| panic!("{board} is not in fixtures.tsv"))
}

/// Loads a row's DSN the way the Java probe does (`DrcRunner`'s load plus
/// the post-load processing): `drc::load_design`.
pub fn load(row: &Row) -> RoutingBoard {
    let bytes = std::fs::read(repo_root().join(&row.dsn)).expect("DSN file");
    load_design(&bytes).unwrap_or_else(|e| panic!("{}: {e}", row.dsn))
}

/// Imports a row's session into `board` (Java `SesReader.read`).
pub fn import_session(board: &mut RoutingBoard, row: &Row) -> Option<SesImportSummary> {
    let ses = row.ses.as_ref()?;
    let bytes = std::fs::read(repo_root().join(ses)).expect("SES file");
    Some(
        read_session(board, &freerouting::dsn::decode(&bytes))
            .unwrap_or_else(|e| panic!("{ses}: {e}")),
    )
}

/// One JSON object whose keys come out in call order, formatted as the
/// Java `Boards.Line` does.
#[derive(Default)]
pub struct Line {
    text: String,
}

impl Line {
    pub fn new() -> Self {
        Line {
            text: String::from("{"),
        }
    }

    fn key(mut self, key: &str) -> Self {
        if self.text.len() > 1 {
            self.text.push(',');
        }
        self.text.push('"');
        self.text.push_str(key);
        self.text.push_str("\":");
        self
    }

    pub fn num(self, key: &str, value: i64) -> Self {
        let mut line = self.key(key);
        line.text.push_str(&value.to_string());
        line
    }

    pub fn dbl(self, key: &str, value: f64) -> Self {
        self.str(key, &bits(value))
    }

    pub fn bool(self, key: &str, value: bool) -> Self {
        let mut line = self.key(key);
        line.text.push_str(if value { "true" } else { "false" });
        line
    }

    pub fn raw(self, key: &str, json: &str) -> Self {
        let mut line = self.key(key);
        line.text.push_str(json);
        line
    }

    pub fn str(self, key: &str, value: &str) -> Self {
        let mut line = self.key(key);
        line.text.push('"');
        for c in value.chars() {
            match c {
                '"' | '\\' => {
                    line.text.push('\\');
                    line.text.push(c);
                }
                c if (c as u32) < 0x20 => line.text.push_str(&format!("\\u{:04x}", c as u32)),
                c => line.text.push(c),
            }
        }
        line.text.push('"');
        line
    }

    pub fn end(mut self) -> String {
        self.text.push('}');
        self.text
    }
}

/// The header line the Rust side writes (only `kind` and `board` are
/// compared).
pub fn header(kind: &str, board: &str) -> String {
    Line::new()
        .str("kind", kind)
        .str("board", board)
        .str("javaCommit", "rust")
        .str("jdk", "-")
        .num("format", 1)
        .end()
}

/// The `kind` and `board` of a header line.
fn header_identity(line: &str) -> Option<(String, String)> {
    let field = |key: &str| {
        let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
        let end = start + line[start..].find('"')?;
        Some(line[start..end].to_string())
    };
    Some((field("kind")?, field("board")?))
}

/// The `.digest` form of probe output (see `generate.sh`).
pub fn digest(lines: &[String]) -> Vec<String> {
    let mut out = vec![lines.first().cloned().unwrap_or_default()];
    let mut section: Option<(String, Vec<&String>)> = None;
    let flush = |section: &mut Option<(String, Vec<&String>)>, out: &mut Vec<String>| {
        if let Some((name, body)) = section.take() {
            let mut hash = FNV_OFFSET;
            for line in &body {
                hash = fnv1a64(hash, line);
                hash = fnv1a64(hash, "\n");
            }
            out.push(format!("{name} {} {}", body.len(), hex(hash)));
        }
    };
    for line in lines.iter().skip(1) {
        if let Some(rest) = line.strip_prefix("{\"stage\":\"") {
            flush(&mut section, &mut out);
            let name = rest.split('"').next().unwrap_or_default().to_string();
            section = Some((name, Vec::new()));
        }
        if let Some((_, body)) = section.as_mut() {
            body.push(line);
        }
    }
    flush(&mut section, &mut out);
    out
}

fn write_lines(path: &Path, lines: &[String]) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("output directory");
    }
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(path, text).expect("probe output");
}

/// The `probe.sh` command that writes Java's output for `board`.
pub fn probe_command(kind: &str, row: &Row) -> String {
    let class: String = kind
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect();
    format!(
        "TMPDIR=<scratch> tools/java-reference/probe.sh <pin-worktree> \
         tools/java-reference/probes/board/{class}Probe.java '{}' '{}' <out.jsonl>",
        row.dsn,
        row.ses.as_deref().unwrap_or("-")
    )
}

/// Compares the Rust lines of probe `kind` on `row` (header first) with the
/// committed Java output. Returns a description of the first difference.
pub fn compare(kind: &str, row: &Row, rust_lines: &[String]) -> Result<(), String> {
    if let Ok(dir) = std::env::var("BOARD_PROBE_OUT") {
        let path = Path::new(&dir)
            .join(kind)
            .join(format!("{}.jsonl", row.board));
        write_lines(&path, rust_lines);
    }
    let base = repo_root().join("fixtures/reference/probe").join(kind);
    let jsonl = base.join(format!("{}.jsonl", row.board));
    let digest_path = base.join(format!("{}.digest", row.board));
    let (java_text, rust_cmp) = if let Ok(text) = std::fs::read_to_string(&jsonl) {
        (text, rust_lines.to_vec())
    } else if let Ok(text) = std::fs::read_to_string(&digest_path) {
        (text, digest(rust_lines))
    } else {
        return Err(format!(
            "{kind}/{}: no Java output; generate it with {}",
            row.board,
            probe_command(kind, row)
        ));
    };
    let java: Vec<&str> = java_text.lines().collect();
    let mismatch = first_difference(&java, &rust_cmp);
    let Some(index) = mismatch else {
        return Ok(());
    };
    let out = repo_root()
        .join("target/probe")
        .join(kind)
        .join(format!("{}.jsonl", row.board));
    write_lines(&out, rust_lines);
    let context: Vec<String> = rust_cmp[index.saturating_sub(5)..index.min(rust_cmp.len())]
        .iter()
        .map(|l| format!("    {l}"))
        .collect();
    Err(format!(
        "{kind}/{}: line {} differs\n  Java: {}\n  Rust: {}\n  before it:\n{}\n  Rust output: {}\n  Java: {}",
        row.board,
        index + 1,
        java.get(index).copied().unwrap_or("<end>"),
        rust_cmp.get(index).map(String::as_str).unwrap_or("<end>"),
        context.join("\n"),
        out.display(),
        probe_command(kind, row)
    ))
}

fn first_difference(java: &[&str], rust: &[String]) -> Option<usize> {
    match (java.first(), rust.first()) {
        (Some(j), Some(r)) if header_identity(j) == header_identity(r) => {}
        _ => return Some(0),
    }
    let n = java.len().max(rust.len());
    (1..n).find(|&i| java.get(i).copied() != rust.get(i).map(String::as_str))
}

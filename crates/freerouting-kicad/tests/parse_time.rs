//! K04.3: lexing plus tree time over the corpus.
//!
//! Run in release on a quiet machine:
//!
//! ```text
//! cargo test -p freerouting-kicad --release --test parse_time -- --ignored --nocapture
//! ```
//!
//! Reads `$FR_KICAD_CORPUS` (default `target/kicad-corpus`, see
//! `tools/kicad-reader/fetch-corpus.sh`). Prints, per board, the median of
//! three runs and the throughput. Reading the file from disk is not timed.

mod common;

use std::time::{Duration, Instant};

use freerouting_kicad::sexpr::Tree;

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

#[test]
fn committed_boards_parse() {
    let boards = common::committed_boards();
    assert_eq!(boards.len(), 7);
    for p in boards {
        let src = std::fs::read_to_string(&p).unwrap();
        let tree = Tree::parse(&src).unwrap();
        assert!(tree.root().child("kicad_pcb").is_some(), "{}", p.display());
    }
}

#[test]
#[ignore = "timing run over the out-of-repo corpus; use --release --nocapture"]
fn corpus_parse_time() {
    let mut boards = common::corpus_boards("java");
    boards.extend(common::corpus_boards("demos"));
    boards.extend(common::corpus_boards("canary-pwr"));
    if boards.is_empty() {
        eprintln!(
            "no corpus (run tools/kicad-reader/fetch-corpus.sh target/kicad-corpus); skipped"
        );
        return;
    }
    let (mut total_bytes, mut total_time) = (0usize, Duration::ZERO);
    for p in &boards {
        let bytes = std::fs::read(p).unwrap();
        let Ok(src) = freerouting_kicad::sexpr::decode(&bytes) else {
            eprintln!("{}: not UTF-8", p.display());
            continue;
        };
        let mut runs = Vec::new();
        let mut nodes = 0;
        for _ in 0..3 {
            let t = Instant::now();
            let tree = Tree::parse(src).unwrap();
            runs.push(t.elapsed());
            nodes = tree.len();
        }
        let m = median(runs);
        total_bytes += src.len();
        total_time += m;
        let mb = src.len() as f64 / 1e6;
        println!(
            "{:>9.2} MB {:>9.1} ms {:>8.0} MB/s {:>10} nodes  {}",
            mb,
            m.as_secs_f64() * 1e3,
            mb / m.as_secs_f64(),
            nodes,
            p.strip_prefix(common::corpus_dir().unwrap())
                .unwrap()
                .display()
        );
    }
    println!(
        "total {:.1} MB in {:.3} s = {:.0} MB/s",
        total_bytes as f64 / 1e6,
        total_time.as_secs_f64(),
        total_bytes as f64 / 1e6 / total_time.as_secs_f64()
    );
}

//! Times board copies (B01.4): `snapshot()`, `restore()` (a build, as
//! `rebuild()`), a repeated `restore()` of one snapshot (the P02.4 memo),
//! `deep_copy()`, `Clone` and `state_hash()`, and the heap bytes a snapshot,
//! a memoized restore and a clone keep alive, on the boards given (default: the three largest fixtures by
//! item count).
//!
//! ```sh
//! cargo run --release -p freerouting --example snapshot_bench [-- board.dsn ...]
//! ```
//!
//! The heap bytes come from a counting global allocator: the bytes still
//! allocated after the operation, minus those before it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};

use freerouting::drc::load_design;

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

// SAFETY: forwards every call to `System` unchanged and only counts sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(
            new_size as isize - layout.size() as isize,
            Ordering::Relaxed,
        );
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const RUNS: usize = 7;

/// The median time of `RUNS` runs of `f`, and the heap bytes the value of the
/// last run keeps alive.
fn measure<T>(mut f: impl FnMut() -> T) -> (Duration, isize) {
    let mut times = Vec::with_capacity(RUNS);
    let mut bytes = 0;
    for _ in 0..RUNS {
        let before = LIVE.load(Ordering::Relaxed);
        let start = Instant::now();
        let value = f();
        times.push(start.elapsed());
        bytes = LIVE.load(Ordering::Relaxed) - before;
        drop(value);
    }
    times.sort();
    (times[RUNS / 2], bytes)
}

fn main() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/");
    let mut boards: Vec<String> = std::env::args().skip(1).collect();
    if boards.is_empty() {
        boards = [
            "Issue190-processor.Z80.dsn",
            "Issue054-tairakb.dsn",
            "Issue690-kit-dev-coldfire-xilinx_5213.dsn",
        ]
        .iter()
        .map(|b| format!("{root}{b}"))
        .collect();
    }
    println!(
        "{:<44} {:>6} {:>11} {:>11} {:>11} {:>11} {:>11} {:>11} {:>12} {:>12} {:>12}",
        "board",
        "items",
        "snapshot",
        "restore",
        "re-restore",
        "deep_copy",
        "clone",
        "state_hash",
        "snap bytes",
        "memo bytes",
        "clone bytes"
    );
    for path in boards {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let board = load_design(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
        let snapshot = board.snapshot();
        let (snap_t, snap_b) = measure(|| board.snapshot());
        let (restore_t, _) = measure(|| snapshot.rebuild());
        // The first restore builds, the second fills the memo.
        let before = LIVE.load(Ordering::Relaxed);
        let repeated = board.snapshot();
        drop((repeated.restore(), repeated.restore()));
        let memo_b = LIVE.load(Ordering::Relaxed) - before;
        let (again_t, _) = measure(|| repeated.restore());
        let (copy_t, _) = measure(|| board.deep_copy());
        let (clone_t, clone_b) = measure(|| board.clone());
        let (hash_t, _) = measure(|| board.state_hash());
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        println!(
            "{:<44} {:>6} {:>11.3?} {:>11.3?} {:>11.3?} {:>11.3?} {:>11.3?} {:>11.3?} {:>12} {:>12} {:>12}",
            name,
            board.item_count(),
            snap_t,
            restore_t,
            again_t,
            copy_t,
            clone_t,
            hash_t,
            snap_b,
            memo_b,
            clone_b
        );
    }
}

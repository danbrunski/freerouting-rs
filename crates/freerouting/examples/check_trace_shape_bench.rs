//! Times `check_trace_shape` (B04.3): 1 000 seeded trace segments on a board (default
//! `fixtures/Issue145-smoothieboard.dsn`), each checked with the touching pins of its
//! unsaved trace as contact pins, and counts the heap allocations per call after a warm-up
//! pass, with clearance compensation off and on.
//!
//! ```sh
//! cargo run --release -p freerouting --example check_trace_shape_bench [-- board.dsn]
//! ```
//!
//! The segments are drawn as the `board-queries` probe draws its samples (B04.1), and the
//! shapes and contact pins are built before the timing. The allocation count comes from a
//! counting global allocator in this example only.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::drc::load_design;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{IntPoint, Point, Polyline, TileShape};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to `System` unchanged and only counts calls.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const CHECKS: usize = 1000;
const RUNS: usize = 5;

struct Check {
    shape: TileShape,
    layer: i32,
    nets: [i32; 1],
    class: i32,
    pins: BTreeSet<ItemId>,
}

fn checks(board: &RoutingBoard) -> Vec<Check> {
    let bb = board.bounding_box();
    let width = bb.ur.x - bb.ll.x;
    let height = bb.ur.y - bb.ll.y;
    let r = width.min(height) / 10;
    let layers = board.layer_count() as i32;
    let max_net = board.rules.nets.max_net_number().max(1);
    let classes = board.rules.clearance_matrix.class_count() as i32;
    let mut random = JavaRandom::new(20261015);
    let mut result = Vec::new();
    while result.len() < CHECKS {
        let x = bb.ll.x + random.next_int(width + 1);
        let y = bb.ll.y + random.next_int(height + 1);
        let layer = random.next_int(layers);
        let net = random.next_int(max_net) + 1;
        let hw = 1 + random.next_int(2 * board.max_trace_half_width());
        let class = random.next_int(classes);
        let x2 = x - r + random.next_int(2 * r + 1);
        let y2 = y - r + random.next_int(2 * r + 1);
        let (from, to) = (
            Point::Int(IntPoint { x, y }),
            Point::Int(IntPoint { x: x2, y: y2 }),
        );
        if from == to {
            continue;
        }
        let polyline = Polyline::from_corners(&from, &to);
        let shape = polyline.offset_shape(hw, 0).expect("a segment");
        let pins = board.touching_pins_at_polyline_end_corners(&polyline, layer, hw, &[net], class);
        result.push(Check {
            shape,
            layer,
            nets: [net],
            class,
            pins,
        });
    }
    result
}

fn pass(board: &RoutingBoard, checks: &[Check]) -> usize {
    checks
        .iter()
        .filter(|c| board.check_trace_shape(&c.shape, c.layer, &c.nets, c.class, Some(&c.pins)))
        .count()
}

fn measure(board: &RoutingBoard, label: &str) {
    let checks = checks(board);
    let ok = pass(board, &checks); // warm-up
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(pass(board, &checks), ok);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
    let mut times = Vec::new();
    for _ in 0..RUNS {
        let start = Instant::now();
        assert_eq!(pass(board, &checks), ok);
        times.push(start.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "{label}: {CHECKS} checks, {ok} fit; ms per {CHECKS}: min {:.2} median {:.2} max {:.2}; \
         allocations per call {:.2}",
        times[0],
        times[RUNS / 2],
        times[RUNS - 1],
        allocations as f64 / CHECKS as f64
    );
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "fixtures/Issue145-smoothieboard.dsn".to_string());
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut board = load_design(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
    measure(&board, "uncompensated");
    board.set_clearance_compensation_used(true);
    measure(&board, "compensated");
}

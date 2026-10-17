//! X03.4: the vendored grid router returns the same search as KiCadRoutingTools'
//! own built Python module on `rust_router/microbench.py`'s serpentine maze.
//!
//! The expected values below were produced by KRT's `grid_router` module
//! (0486e799, Python extension build, X01.2's environment) with
//!
//! ```text
//! target/orchestrator/x01/venv/bin/python -I tools/krt/serpentine_reference.py \
//!     target/orchestrator/x01/krt/rust_router
//! ```
//!
//! Each case: the maze is `microbench.py`'s (walls every 20 columns with an 8-cell
//! alternating gap, both layers, border) for sizes 100, 200 and 400; the `sealed`
//! variant adds a full wall at column `size - 4`, so the search fails and the
//! blocked frontier is non-empty. `path_fnv` / `frontier_fnv` are FNV-1a 64 over
//! the (x, y, layer) triples as little-endian i32 (frontier sorted).

use krt_grid_router::{GridObstacleMap, GridRouter, TrackMarginArg};

const WALL_PITCH: i32 = 20;
const GAP: i32 = 8;
const LAYERS: i32 = 2;

fn fnv1a64(triples: &[(i32, i32, u8)]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &(x, y, l) in triples {
        for v in [x, y, l as i32] {
            for b in v.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
    }
    h
}

fn maze(size: i32, sealed: bool) -> GridObstacleMap {
    let mut cells: Vec<[i32; 3]> = Vec::new();
    let mut i = 0;
    let mut wx = WALL_PITCH;
    while wx < size - 1 {
        let rows = if i % 2 == 0 { 0..size - GAP } else { GAP..size };
        for wy in rows {
            for layer in 0..LAYERS {
                cells.push([wx, wy, layer]);
            }
        }
        i += 1;
        wx += WALL_PITCH;
    }
    if sealed {
        for wy in 0..size {
            for layer in 0..LAYERS {
                cells.push([size - 4, wy, layer]);
            }
        }
    }
    for c in -1..=size {
        for layer in 0..LAYERS {
            cells.extend([
                [c, -1, layer],
                [c, size, layer],
                [-1, c, layer],
                [size, c, layer],
            ]);
        }
    }
    let mut map = GridObstacleMap::new(LAYERS as usize);
    map.add_blocked_cells_batch(&cells);
    map
}

type Search = (Option<Vec<(i32, i32, u8)>>, u32, Vec<(i32, i32, u8)>);

fn run(size: i32, sealed: bool) -> Search {
    let obstacles = maze(size, sealed);
    let router = GridRouter::new(
        5000,
        1.0,
        Some(1000),
        None,
        0,
        0,
        None,
        None,
        None,
        0,
        0,
        0,
        0,
        0,
    );
    router.route_with_frontier(
        &obstacles,
        vec![(1, 1, 0)],
        vec![(size - 2, size - 2, 0)],
        50_000_000,
        false,
        0,
        None,
        None,
        2,
        TrackMarginArg::Scalar(0.0),
        0,
        2.0,
        2.0,
        0,
        0,
    )
}

/// (size, sealed, path_len, path_fnv, iterations, frontier, frontier_fnv)
const EXPECTED: [(i32, bool, usize, u64, u32, usize, u64); 6] = [
    (
        100,
        false,
        438,
        0xb0883d8dd96adab4,
        30624,
        0,
        0xcbf29ce484222325,
    ),
    (
        100,
        true,
        0,
        0xcbf29ce484222325,
        35928,
        1528,
        0x24a3c17a154a312d,
    ),
    (
        200,
        false,
        1690,
        0x2df616f481f9b335,
        126499,
        0,
        0xcbf29ce484222325,
    ),
    (
        200,
        true,
        0,
        0xcbf29ce484222325,
        146992,
        5048,
        0x1917f5f6f30ae88d,
    ),
    (
        400,
        false,
        7340,
        0x23dfecc721fded9e,
        554249,
        0,
        0xcbf29ce484222325,
    ),
    (
        400,
        true,
        0,
        0xcbf29ce484222325,
        594142,
        10000,
        0xf1b997eed9b54be2,
    ),
];

#[test]
fn serpentine_matches_krt_python_module() {
    for (size, sealed, path_len, path_fnv, iterations, frontier, frontier_fnv) in EXPECTED {
        let (path, iters, mut blocked) = run(size, sealed);
        let path = path.unwrap_or_default();
        blocked.sort_unstable();
        let case = format!("size={size} sealed={sealed}");
        assert_eq!(path.len(), path_len, "{case}: path length");
        assert_eq!(fnv1a64(&path), path_fnv, "{case}: path");
        assert_eq!(iters, iterations, "{case}: iterations");
        assert_eq!(blocked.len(), frontier, "{case}: frontier size");
        assert_eq!(fnv1a64(&blocked), frontier_fnv, "{case}: frontier");
    }
}

#[test]
fn two_runs_in_one_process_are_identical() {
    for (size, sealed) in [(100, false), (100, true), (200, false)] {
        let a = run(size, sealed);
        let b = run(size, sealed);
        assert_eq!(a, b, "size={size} sealed={sealed}");
    }
}

/// `route_multi`'s statistics struct carries KRT's dict keys and values (the
/// reference line is `stats size=100 ...` from `serpentine_reference.py`).
#[test]
fn route_multi_stats_match_krt_dict() {
    let obstacles = maze(100, false);
    let router = GridRouter::new(
        5000,
        1.0,
        Some(1000),
        None,
        0,
        0,
        None,
        None,
        None,
        0,
        0,
        0,
        0,
        0,
    );
    let (path, iterations, stats) = router.route_multi(
        &obstacles,
        vec![(1, 1, 0)],
        vec![(98, 98, 0)],
        50_000_000,
        false,
        0,
        None,
        None,
        2,
        TrackMarginArg::Scalar(0.0),
        0,
        2.0,
        2.0,
        0,
        0,
    );
    assert_eq!(path.map(|p| p.len()), Some(438));
    assert_eq!(iterations, 30624);
    assert_eq!(stats.best_h, 0.0);
    assert_eq!(stats.cells_expanded, 16459.0);
    assert_eq!(stats.cells_pushed, 33515.0);
    assert_eq!(stats.cells_revisited, 15463.0);
    assert_eq!(stats.closed_set_size, 16458.0);
    assert_eq!(stats.duplicate_skips, 14165.0);
    assert_eq!(stats.final_g, 485658.0);
    assert_eq!(stats.initial_h, 137158.0);
    assert_eq!(stats.iteration_tranches, 0.0);
    assert_eq!(stats.open_set_size, 2891.0);
    assert_eq!(stats.path_cost, 485658.0);
    assert_eq!(stats.path_length, 438.0);
    assert_eq!(stats.via_count, 0.0);
    assert_eq!(stats.expansion_ratio, Some(37.577625570776256));
    assert_eq!(stats.heuristic_ratio, Some(0.28241684477554163));
    assert_eq!(stats.revisit_ratio, Some(0.9394859955039796));
    assert_eq!(stats.skip_ratio, Some(0.4625457157784744));
}

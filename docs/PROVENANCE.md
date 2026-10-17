# Provenance ledger

One row per port of third-party (non-Java) code. Ported files carry a
`Portions ported from` line and this SPDX header:

    SPDX-License-Identifier: GPL-3.0-only AND MIT
    SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)

`tools/check-provenance.sh` checks the ledger against the tree. Table rows
below are parsed: first column is the Rust file path (backticked).

| Rust file | Symbol | Source path | Source symbol | Source commit | Date | Kind |
|---|---|---|---|---|---|---|
| `crates/krt-grid-router/src/types.rs` | whole file | `rust_router/src/types.rs` | whole file | `0486e799` | 2026-10-17 | vendored, unchanged |
| `crates/krt-grid-router/src/obstacle_map.rs` | GridObstacleMap | `rust_router/src/obstacle_map.rs` | GridObstacleMap | `0486e799` | 2026-10-17 | vendored, binding removed |
| `crates/krt-grid-router/src/router.rs` | GridRouter, TrackMarginArg | `rust_router/src/router.rs` | GridRouter, TrackMarginArg | `0486e799` | 2026-10-17 | vendored, binding removed |
| `crates/krt-grid-router/src/dubins.rs` | whole file | `rust_router/src/dubins.rs` | whole file | `0486e799` | 2026-10-17 | vendored, unchanged |
| `crates/krt-grid-router/src/pose_router.rs` | PoseRouter | `rust_router/src/pose_router.rs` | PoseRouter | `0486e799` | 2026-10-17 | vendored, binding removed |
| `crates/krt-grid-router/src/lib.rs` | module list, re-exports | `rust_router/src/lib.rs` | mod list, pub use (not identify_blocking_obstacles, #[pymodule], allocator) | `0486e799` | 2026-10-17 | replaced: module list and re-exports |
| `crates/krt-grid-router/Cargo.toml` | package manifest | `rust_router/Cargo.toml` | dependencies (pyo3, numpy, mimalloc dropped) | `0486e799` | 2026-10-17 | replaced |
| `crates/krt-grid-router/LICENSE` | MIT licence text | `LICENSE` | whole file | `0486e799` | 2026-10-17 | vendored, unchanged |

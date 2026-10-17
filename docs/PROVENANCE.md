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
| `crates/freerouting-krt/src/grid/path.rs` | simplify, plan | `py_router/geometry_utils.py`, `py_router/single_ended_routing.py` | simplify_path, _path_to_segments_vias | `0486e799` | 2026-10-17 | ported, adapted: exact integer collinearity test; no via reuse, no via resizing, connectors in `terminals.rs` |
| `crates/freerouting-krt/src/length/measure.rs` | Stackup::barrel_nm, measure_net | `py_router/kicad_parser.py`, `py_router/net_queries.py` | get_via_barrel_length, calculate_route_length, net_pad_to_die_length | `0486e799` | 2026-10-17 | ported, adapted: trace part is Java's `net_trace_length`; copper layers mapped by order; lengths in board units |
| `crates/freerouting-krt/src/length/timing.rs` | Timing, layer_eps_eff, barrel_eps | `py_router/impedance.py` | get_layer_impedance_params, get_layer_epsilon_eff, get_layer_ps_per_mm, get_via_barrel_epsilon_eff, calculate_route_propagation_time_ps | `0486e799` | 2026-10-17 | ported, adapted: flat microstrip εr_eff only (no Hammerstad or CPWG form), no die_delay |
| `crates/freerouting-krt/src/diffpair/model.rs` | name_key, pair_by_names | `py_router/net_queries.py` | extract_diff_pair_base, find_differential_pairs | `0486e799` | 2026-10-17 | ported, adapted: no regular expressions (the same rules by hand, ASCII digits), every complete pair taken (no glob patterns), a key with two nets of one polarity reported and skipped instead of the last one kept |
| `crates/freerouting-krt/src/fanout/frame.rs` | analyze, side_of, pitch_of | `py_router/qfn_fanout/layout.py` | analyze_qfn_layout, analyze_pad, single_line_reason | `0486e799` | 2026-10-17 | ported, adapted: integer part frame (DSN y-up: top, bottom, left, right = N, S, W, E); a tie between two edges goes to the pad's long axis first; parts qualify only with 4 or 2 opposite sides, pitch at most 0.65 mm and no interior pin but exposed pads |
| `crates/freerouting-krt/src/bus/detect.rs` | connections, group_connections, physical_order | `py_router/bus_detection.py` | detect_bus_groups, _find_largest_clique, _order_nets_by_position | `0486e799` | 2026-10-17 | ported, adapted: two-pin nets only; both ends in the clique test (squared distances in i128) instead of one end plus a centroid sub-clustering; greedy cliques seeded in net-number order (no Python set, no closest-first sort); at least 3 members; position along the line through the farthest two start pins |
| `crates/freerouting-krt/src/bus/order.rs` | middle_out, routing_order, regroup | `py_router/bus_detection.py` | get_bus_routing_order | `0486e799` | 2026-10-17 | ported, adapted: groups as blocks after the plane-net tasks, by smallest net number, in Freerouting's pass-1 list |
| `crates/freerouting-krt/src/fanout/bga/array.rs` | analyze_array, cluster, dominant_pitch, is_staggered | `py_router/bga_fanout/grid.py` | analyze_bga_grid, _cluster_positions, staggered_lattice_diagnosis | `0486e799` | 2026-10-17 | ported, adapted: integer part frame (X10.1), exact integer cluster means, dominant-gap ties to the smaller gap and the pitch as the exact mean of its gaps; staggered = fewer than half the balls with an orthogonal lattice neighbour (KRT: nearest neighbour above 1.4 steps and occupancy below 35 %); arrays also need one pitch, at least 6 × 6 positions and two at least half-populated outer rings |

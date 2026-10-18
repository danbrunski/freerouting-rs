# Provenance ledger

One row per port of third-party (non-Java) code. Ported files carry a
`Portions ported from` line and this SPDX header:

    SPDX-License-Identifier: GPL-3.0-only AND MIT
    SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)

`tools/check-provenance.sh` checks the ledger against the tree. Table rows
below are parsed: first column is the Rust file path (backticked).

| Rust file | Symbol | Source path | Source symbol | Source commit | Date | Kind |
|---|---|---|---|---|---|---|
| `crates/freerouting-krt/src/length/measure.rs` | Stackup::barrel_nm, measure_net | `py_router/kicad_parser.py`, `py_router/net_queries.py` | get_via_barrel_length, calculate_route_length, net_pad_to_die_length | `0486e799` | 2026-10-17 | ported, adapted: trace part is Java's `net_trace_length`; copper layers mapped by order; lengths in board units |
| `crates/freerouting-krt/src/length/timing.rs` | Timing, layer_eps_eff, barrel_eps | `py_router/impedance.py` | get_layer_impedance_params, get_layer_epsilon_eff, get_layer_ps_per_mm, get_via_barrel_epsilon_eff, calculate_route_propagation_time_ps | `0486e799` | 2026-10-17 | ported, adapted: flat microstrip εr_eff only (no Hammerstad or CPWG form), no die_delay |
| `crates/freerouting-krt/src/diffpair/model.rs` | name_key, pair_by_names | `py_router/net_queries.py` | extract_diff_pair_base, find_differential_pairs | `0486e799` | 2026-10-17 | ported, adapted: no regular expressions (the same rules by hand, ASCII digits), every complete pair taken (no glob patterns), a key with two nets of one polarity reported and skipped instead of the last one kept |
| `crates/freerouting-krt/src/fanout/frame.rs` | analyze, side_of, pitch_of | `py_router/qfn_fanout/layout.py` | analyze_qfn_layout, analyze_pad, single_line_reason | `0486e799` | 2026-10-17 | ported, adapted: integer part frame (DSN y-up: top, bottom, left, right = N, S, W, E); a tie between two edges goes to the pad's long axis first; parts qualify only with 4 or 2 opposite sides, pitch at most 0.65 mm and no interior pin but exposed pads |
| `crates/freerouting-krt/src/fanout/bga/array.rs` | analyze_array, cluster, dominant_pitch, is_staggered | `py_router/bga_fanout/grid.py` | analyze_bga_grid, _cluster_positions, staggered_lattice_diagnosis | `0486e799` | 2026-10-17 | ported, adapted: integer part frame (X10.1), exact integer cluster means, dominant-gap ties to the smaller gap and the pitch as the exact mean of its gaps; staggered = fewer than half the balls with an orthogonal lattice neighbour (KRT: nearest neighbour above 1.4 steps and occupancy below 35 %); arrays also need one pitch, at least 6 × 6 positions and two at least half-populated outer rings |
| `crates/freerouting/src/quality/ordering/conflicts.rs` | ConflictGraph, mst, properly_cross | `py_router/net_queries.py`, `py_router/connectivity.py` | compute_mps_net_ordering, get_net_mst_segments, compute_mst_edges, segments_intersect | `0486e799` | 2026-10-17 | ported, adapted: pin-centre MSTs by Prim's algorithm in exact `i64`, exact `i128` proper-crossing test (shared end points and collinear overlap never cross), pin layers instead of stub layers; re-applied by Q12 from tag `archive/q07-orderings` (2026-10-18) |
| `crates/freerouting/src/quality/ordering/conflict_rounds.rs` | rounds, net_order, conflict_rounds_order | `py_router/net_queries.py` | _greedy_order_mps_units | `0486e799` | 2026-10-17 | ported, adapted: integer MST lengths and ordered collections; no routing-aware distance tie-break, reverse rounds, diff-pair units or layer swaps; re-applied by Q12 from tag `archive/q07-orderings` (2026-10-18) |
| `crates/freerouting/src/quality/ordering/inside_out.rs` | dense_parts, dense_net_order, inside_out_order | `py_router/net_ordering.py` | order_nets_inside_out | `0486e799` | 2026-10-17 | ported, adapted: dense parts by pin pitch (0.65 mm plus 1 µm) and pin-field bounding boxes instead of BGA exclusion zones, exact doubled-coordinate `i64` distances; re-applied by Q12 from tag `archive/q07-orderings` (2026-10-18) |

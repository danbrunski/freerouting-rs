#!/usr/bin/env python3
"""Reference values for crates/krt-grid-router/tests/serpentine.rs.

Runs KiCadRoutingTools' own built `grid_router` module (the pyo3 build of
rust_router/ at 0486e799) on the serpentine maze of rust_router/microbench.py
for three sizes, in two variants, and prints one line per case:

  size=S kind=solvable|sealed path_len=N path_fnv=0x... iterations=I \
      frontier=F frontier_fnv=0x...

and, for the size-100 solvable maze, the `route_multi` statistics dict:

  stats size=100 {key: value, ...}

`solvable` is microbench.py's maze (walls every WALL_PITCH columns with
alternating gaps, both layers). `sealed` adds a full wall at column size-4, so
the target is unreachable and `route_with_frontier` returns a non-empty
blocked frontier (on success the frontier is empty).
`path_fnv` is FNV-1a 64 over the path's (x, y, layer) triples as little-endian
i32 each; `frontier_fnv` the same over the frontier sorted by (x, y, layer).

Usage (X01's environment; rebuild commands are in
docs/roadmap/tasks/X03-vendor-krt-grid-router.md, X03.4):
    <venv>/bin/python -I tools/krt/serpentine_reference.py <dir containing grid_router.so>
"""
import struct
import sys

SIZES = (100, 200, 400)
WALL_PITCH = 20
GAP = 8
LAYERS = 2
MAX_ITERATIONS = 50_000_000


def fnv1a64(triples):
    h = 0xCBF29CE484222325
    for t in triples:
        for b in struct.pack("<iii", *t):
            h ^= b
            h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def maze_cells(size, sealed):
    cells = []
    for i, wx in enumerate(range(WALL_PITCH, size - 1, WALL_PITCH)):
        rows = range(0, size - GAP) if i % 2 == 0 else range(GAP, size)
        for wy in rows:
            for layer in range(LAYERS):
                cells.append((wx, wy, layer))
    if sealed:
        for wy in range(size):
            for layer in range(LAYERS):
                cells.append((size - 4, wy, layer))
    for c in range(-1, size + 1):
        for layer in range(LAYERS):
            cells.extend([(c, -1, layer), (c, size, layer), (-1, c, layer), (size, c, layer)])
    return cells


def main():
    sys.path.insert(0, sys.argv[1])
    import numpy as np
    from grid_router import GridObstacleMap, GridRouter

    router = GridRouter(via_cost=5000, h_weight=1.0, turn_cost=1000)
    for size in SIZES:
        for kind in ("solvable", "sealed"):
            obstacles = GridObstacleMap(LAYERS)
            obstacles.add_blocked_cells_batch(
                np.array(maze_cells(size, kind == "sealed"), dtype=np.int32))
            sources = [(1, 1, 0)]
            targets = [(size - 2, size - 2, 0)]
            results = [router.route_with_frontier(obstacles, sources, targets, MAX_ITERATIONS)
                       for _ in range(2)]
            assert results[0] == results[1], "two runs differ"
            path, iterations, blocked = results[0]
            path = path or []
            blocked = sorted(tuple(c) for c in blocked)
            print(f"size={size} kind={kind} path_len={len(path)} path_fnv=0x{fnv1a64(path):016x} "
                  f"iterations={iterations} frontier={len(blocked)} "
                  f"frontier_fnv=0x{fnv1a64(blocked):016x}")
            if size == SIZES[0] and kind == "solvable":
                p2, it2, stats = router.route_multi(obstacles, sources, targets, MAX_ITERATIONS)
                assert (p2 == path and it2 == iterations)
                print(f"stats size={size} {dict(sorted(stats.items()))}")


if __name__ == "__main__":
    main()

# X03 — Vendor KRT's grid router as an MIT crate

Opened 2026-10-15. KRT's Rust core (`rust_router/`, crate `grid_router`
0.23.0, at `0486e799`) holds the grid obstacle map, the octilinear A*
(`GridRouter::route_multi`, `route_with_frontier`) and the pose A* with a
Dubins heuristic. It is plain Rust apart from about 45 lines that touch pyo3
or numpy. This checklist copies it into its own crate with the Python binding
removed and nothing else changed, so the MIT portion stays identifiable and
upstream fixes can be re-vendored by diff. It adds no behaviour to the
engine; X04 and X05 use it.

Prerequisites: board parity (rung 3); X01's decision (skip this checklist if
X01 puts the grid router last and Dan agrees); F04 (provenance scaffolding,
`LICENSES/MIT-KiCadRoutingTools.txt`); the orchestrator's answer to
`NEW DEP: rustc-hash`; X01.2's KRT environment (for the differential test).

## What exists and what this adds

| KRT file at `0486e799` | Lines | Vendored? |
|---|---|---|
| `rust_router/src/types.rs` | 326 | yes, unchanged |
| `rust_router/src/obstacle_map.rs` | 1,516 | yes: `#[pyclass]`, `#[pymethods]`, `#[pyo3(get)]`, `#[pyo3(name = "clone")]`, `#[pyo3(signature …)]` removed; numpy batch parameters → slices |
| `rust_router/src/router.rs` | 1,655 | yes: `#[pyclass]`, `#[pymethods]`, signatures removed; `TrackMarginArg`'s `FromPyObject` removed; stats `HashMap<String, f64>` → a struct |
| `rust_router/src/dubins.rs` | 215 | yes, unchanged |
| `rust_router/src/pose_router.rs` | 716 | yes: pyo3 attributes removed |
| `rust_router/src/lane_search.rs` | 199 | **no**: only used by KRT's `awx/` planner |
| `rust_router/src/lib.rs` | 150 | **replaced**: module list and re-exports only. The `#[global_allocator]` mimalloc static, `identify_blocking_obstacles` and `#[pymodule]` are not vendored |
| `rust_router/Cargo.toml` | — | replaced: dependencies pyo3, numpy and mimalloc dropped; rustc-hash kept |
| `LICENSE` (MIT, "Copyright (c) 2026 drandyhaas") | — | copied unchanged into the crate |
| `rust_router/microbench.py` | — | its serpentine maze becomes a Rust test |

freerouting-rs at HEAD: the workspace has `members = ["crates/*"]`,
`edition = "2024"`, `license = "GPL-3.0-only"` inherited by every crate, and
no hashing crate.

## Decisions made here

- **Crate:** `crates/krt-grid-router`, package `krt-grid-router`,
  `publish = false`, `license = "MIT"` (set explicitly, not inherited),
  `edition = "2021"` (KRT's edition, so the code compiles unchanged),
  `rust-version` inherited. It is a workspace member through the
  `crates/*` glob.
- **Binding removal only.** Every change outside the binding is a bug. The
  slices replacing numpy arrays keep the same row layout (`&[[i32; 3]]` for an
  `N × 3` array, and so on). Methods keep their names and argument order.
- **The global allocator goes.** A library must not choose the binary's
  allocator; the allocator trial is P05.
- **Formatting and lints.** `cargo fmt --all --check` must not reformat the
  vendored files: each `mod` declaration in the new `lib.rs` carries
  `#[rustfmt::skip]` (unverified that rustfmt honours it on out-of-line
  modules; if not, list the files in a `rustfmt.toml` `ignore` entry and
  record that). Clippy runs at the workspace level, so `lib.rs` starts with
  an explicit `#![allow(...)]` list of the lints the vendored code trips,
  each named, never `clippy::all`.
- **The diff is the record.** `tools/krt/vendor-diff.sh` regenerates
  `crates/krt-grid-router/VENDORED.diff` from `git -C ../KiCadRoutingTools
  show 0486e799:<path>` and the vendored files. The committed diff must equal
  the regenerated one.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X03.1 | `crates/krt-grid-router/{Cargo.toml, LICENSE, README.md, src/types.rs, src/obstacle_map.rs, src/router.rs, src/dubins.rs, src/pose_router.rs}` | — |
| X03.2 | `crates/krt-grid-router/src/lib.rs` | the vendored `src/*.rs` (binding lines only) |
| X03.3 | `tools/krt/vendor-diff.sh`, `crates/krt-grid-router/VENDORED.diff` | — |
| X03.4 | `crates/krt-grid-router/tests/serpentine.rs`, `tools/krt/serpentine_reference.py` | — |
| X03.5 | — | `docs/PROVENANCE.md` (append rows) |

## Checklist

- [ ] **X03.1 — Copy the files at the pin.**
  `git -C ../KiCadRoutingTools show 0486e799:rust_router/src/<file>` for the
  five vendored files, and `0486e799:LICENSE`. Write `Cargo.toml` and a
  `README.md` naming the source repository, the commit, the files and the
  licence.
  - **Accept:** the five files are byte-identical to KRT's at this point (the
    commit of this item contains no other change to them).

- [ ] **X03.2 — Remove the binding.**
  In `obstacle_map.rs`, `router.rs` and `pose_router.rs`: delete the pyo3
  attributes; change numpy parameters (`PyReadonlyArray2<i32>` and similar)
  to slices of fixed-size arrays and their `.as_array().rows()` loops to
  slice iteration; replace `HashMap<String, f64>` stats with
  `pub struct SearchStats` whose fields are the keys KRT returns; drop
  `TrackMarginArg`'s `FromPyObject` impl and keep the enum. Write `lib.rs`.
  - **Accept:**
    - `cargo build -p krt-grid-router` and the full gate pass;
    - `grep -rn 'pyo3\|numpy\|mimalloc\|global_allocator'
      crates/krt-grid-router` finds nothing;
    - `NEW DEP: rustc-hash — FxHash for the vendored KRT grid router;
      seedless, so no hash order varies between runs` is in the report.

- [ ] **X03.3 — The vendored diff.**
  `vendor-diff.sh` writes `git diff --no-index` output for each vendored file
  against KRT's version into `VENDORED.diff`, in file-name order.
  - **Accept:** the script reproduces the committed diff exactly; every hunk
    touches a binding line, a parameter type, a stats return or `lib.rs`
    (reviewed and stated in the done note); the diff is between 200 and 600
    changed lines, or the done note explains why not.

- [ ] **X03.4 — Same search as KRT's Python module.**
  Port `rust_router/microbench.py`'s serpentine maze (walls every
  `WALL_PITCH` columns with alternating gaps, both layers) to a Rust test
  that calls `GridRouter::route_with_frontier`. `serpentine_reference.py`,
  run in X01's virtual environment against KRT's own built module, prints
  the path, the iteration count and the blocked frontier size for three
  maze sizes; the expected values are copied into the test with the command
  that produced them.
  - **Accept:** for all three sizes, the vendored crate returns the same path,
    iteration count and frontier size as KRT's module; two runs in one
    process give identical results.

- [ ] **X03.5 — Provenance.**
  - **Accept:** `docs/PROVENANCE.md` has one row per vendored file (Rust
    path → KRT path, commit `0486e799`, date); F04's CI check passes; the
    crate's `LICENSE` is KRT's text unchanged.

# P05 — SIMD and allocator

Opened 2026-10-15. Dan welcomes SIMD where it helps (2026-10-15). In this
codebase that means wide, data-parallel loops over plain integers. Example: an
octagon's eight bounds compared as one `i32x8`, batched over a node's
children or a leaf block. Branchy search, priority queues and `i128`/`BigInt`
geometry don't vectorize. This checklist only acts on loops P01 shows to be
hot. It also trials an allocator, which KiCadRoutingTools found cut sustained
memory by about a third in a long-running process.

Prerequisites: P01 (profiles). P05.2 is most useful after P04.3, but needs
only a profile showing it.

## What exists and what this adds

| Exists | State |
|---|---|
| `IntBox`/`IntOctagon` overlap tests | Scalar, one shape pair at a time (`freerouting-geometry`) |
| Bounds layout in the tree | Array of structs |
| Global allocator | System default |

## Decisions made here

- **Bit-identical to the scalar path.** Integer comparisons only, or float
  code with the same operation order (D6). The scalar version stays as the
  reference, with a test that both agree on fixtures and on randomized
  inputs, including values at `CRIT_INT`.
- **Portable, stable Rust:**
  - autovectorizable loops (`chunks_exact`, struct-of-arrays bounds) first;
  - `core::arch` behind runtime feature detection with a scalar fallback
    second;
  - must build on Windows, macOS (x86_64 and aarch64) and Linux;
  - no `target-cpu=native` or other machine-specific flags in committed
    config;
  - a SIMD crate such as `wide` is a `NEW DEP`.
- **Allocator trial only with numbers.** `mimalloc` is a `NEW DEP`. Keep it
  only if it measurably helps the long-running GUI or plugin process at
  unchanged CLI speed.

## File ownership

| Item | New files | May edit |
|---|---|---|
| P05.1 | — | `crates/freerouting/src/board/search/` (bounds layout) |
| P05.2 | `crates/freerouting-geometry/src/simd.rs` | the overlap-test call sites |
| P05.3 | — | `crates/freerouting-gui/src/main.rs`, `crates/freerouting-cli/src/main.rs` (global allocator), `Cargo.toml` files |

## Checklist

- [~] **P05.1 — Struct-of-arrays bounds for the tree's hot loop.**
  If P01 shows overlap queries are hot, store each tree node's children's
  bounds as struct-of-arrays and test them in a loop the compiler can
  vectorize. Check the assembly for vector instructions
  (`cargo asm` or `objdump`) on x86_64.
  - **Accept:**
    - Identical result sets on a fixed query sample on every fixture.
    - Identical SES on `benchmark`.
    - Overlap-query microbenchmark time (P01.3) before and after.
    - Kept only if faster.

  *Partial 2026-10-18:* `ShapeTree` (`crates/freerouting/src/datastructures/shape_tree.rs`; the tree lives there, not in `board/search/`) keeps, beside the node arena, a dense `child_lanes: Vec<PairLanes>` (each inner node's two children's bounds side by side, 64 bytes, `[i32; 8]` lower and upper limits over the four octagon directions) and `links: Vec<[u32; 2]>` (the children, a leaf bit set); `overlaps` tests both children of every node it enters in one branch-free 8-lane loop (`freerouting_geometry::simd::PairLanes::{octagons_intersect, boxes_intersect}`, the scalar `max(lo) <= min(hi)` and cross comparisons per lane) and reads a `Node` only for a leaf it returns. The node's `RegularTileShape` stays the source of truth, written only through `set_bounds`/`set_children`; a tree holding a bound of the other kind than its directions make (only tests) and mixed-kind queries take the old scalar traversal, kept as `overlaps_reference`. The traversal stack starts at capacity 64 (it grew from 1 per query). `depth_first`/`best_first` (room completion) are unchanged. **Assembly** (objdump of `ShapeTree::overlaps` in release `freerouting-dev`, x86_64 baseline SSE2): the pair test is `pcmpgtd`+blend for `max`/`min`, `pcmpgtd`, two `movmskps`, no branch per lane. **Result sets:** `crates/freerouting/tests/tree_lanes.rs::lane_queries_equal_scalar_queries_on_fixtures`, release, all 164 fixtures of `fixtures.tsv`, every search tree after load plus every autoroute tree, and an orthogonal tree of the default tree's leaf boxes: 1,591,649 queries, 1,264,764 with a hit, leaves and work-meter units equal to the scalar traversal's; unit tests `shape_tree::tests::{lane_queries_equal_scalar_queries, mixed_kinds_take_the_scalar_path}` (3000 random inserts and removes per kind with the lane arrays checked against the nodes) and `crates/freerouting-geometry/tests/simd_lanes.rs` (1 M random pairs each for octagons, boxes and pairs, small grid, near `CRIT_INT`, at the `i32` limits, `EMPTY`). **SES:** `autoroute_board_parity` with `PARITY_SET=tools/bench/sets/connection-parity.txt`: 7 exact, 7 byte-identical to Java's session; 10 of the 16 `benchmark` boards (`PARITY_BOARDS`: the nine that route in under 15 s and `Issue753-CPU-85_r104`): 10 pass, 10 byte-identical; the `route_board` sessions of base and change are byte-identical on `Issue508-DAC2020_bm10`, `Issue555-CNH_Functional_Tester_1` and `Issue093-interf_u` (`cmp`). Microbench work counts are unchanged (`microbench-ab.sh` fails on a difference; none). **Instruction counts** (callgrind, `taskset -c 0`, base = `cdc9865` built in the same target dir): `kernel/overlap` 43,012,728 -> 22,748,250 (-47.1 %), `kernel/drc` -28.8 %, `kernel/pull_tight` -11.4 %, `kernel/maze` -7.5 %, `kernel/complete_shape` -0.04 %; the autorouter (`start_batch_loop`, 3 passes, optimizer off, one thread) on `Issue143-rpi_splitter` 264,586,642 -> 243,842,654 (-7.8 %), `Issue433-my-board` -10.3 %, `Issue508-SMD-routing-issue-demo` -9.6 %. Steps measured alone: lanes per node (layout A) -28.9 %, plus the stack capacity -45.5 %, children pairs (layout B, kept) -47.1 %. **Wall time, under load (1-minute load 4 to 5, not quiet; indicative only)**, `tools/bench/microbench-ab.sh`, minimum over 3 to 4 alternating rounds of 9 runs: overlap on `Issue145-smoothieboard` 20.62 ms -> 11.76 ms (1.75x), on `Issue433-my-board` 2.18 -> 1.21 ms (1.80x); layout B against A 1.22x / 1.13x; drc 38.9 -> 28.3 ms on smoothieboard; `route_board` medians of 3 alternating: `Issue508-DAC2020_bm10` 5.60 -> 4.98 s, `Issue555-CNH_Functional_Tester_1` 5.65 -> 4.84 s, `Issue093-interf_u` 0.91 -> 0.73 s. Kept (faster on every count and timing). **Remainder:** (1) the quiet-machine wall times (owned by the P01.5 quiet re-run): `cargo build --release -p freerouting-dev` at `cdc9865` and with this change, then `tools/bench/microbench-ab.sh <base>/freerouting-dev <new>/freerouting-dev --kernel overlap,drc,maze,pull_tight,complete_shape --rounds 5 fixtures/Issue145-smoothieboard.dsn fixtures/Issue433-my-board.dsn` at load at most 1.0; (2) identical SES on the six long `benchmark` boards (`Issue269-z10_module`, `Issue690-kit-dev-coldfire-xilinx_5213`, `Issue066-Project_GP8B`, `Issue145-smoothieboard`, `Issue219-LogicBoard_smt`, `Issue420-contribution-board`; 2.5 to 18 minutes each), owned by the full A14 gate: `PARITY_SET=tools/bench/sets/benchmark.txt cargo test --release -p freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1`.

- [~] **P05.2 — Explicit SIMD octagon tests.**
  Only if P05.1's autovectorized loop leaves a measurable gap. Test 8 octagon
  bounds per comparison with `core::arch` (SSE4.1/AVX2 on x86_64, NEON on
  aarch64) behind runtime detection, with a scalar fallback.
  - **Accept:**
    - A test compares SIMD and scalar on 1 M randomized octagon pairs,
      including the `CRIT_INT` bounds, with zero differences.
    - CI builds and tests pass on Linux, Windows and macOS.
    - The microbenchmark gain is in the note.
    - Kept only if at least 1.2× on the overlap-query kernel.

  *Partial 2026-10-18:* tried and **rejected** on the indicative numbers; no code is left behind. The trial (`target/orchestrator/p05/p05.2-avx2-trial.patch`, applies on top of P05.1 with `git apply`): `freerouting_geometry::simd::x86::{octagons_intersect, boxes_intersect}`, AVX2 intrinsics on a `PairLanes` (8 lanes = both children's octagon bounds in one `__m256i`: `vpmaxsd`, `vpminsd`, `vpcmpgtd`, `vmovmskps`), behind `is_x86_feature_detected!("avx2")` with the portable 8-lane loop as the fallback, and `ShapeTree::overlaps_lanes_avx2` (`#[target_feature(enable = "avx2")]`, so the kernel inlines; objdump shows 33 `ymm` uses). SSE4.1 was not tried separately: the AVX2 path holds its instructions, and aarch64 gets NEON from the portable loop already (baseline there). **Agreement:** the patch's `avx2_pair_lanes_equal_portable_pair_lanes` (1 M random pairs each of octagons and boxes, the generator of `simd_lanes.rs`: small grid, near `CRIT_INT`, `i32` limits, `EMPTY`) passed with zero differences on this CPU. **Gain:** callgrind `kernel/overlap` 22,748,250 -> 20,107,833 instructions (-11.6 %), but wall time, `tools/bench/microbench-ab.sh` P05.1 against the trial, 4 alternating rounds of 9 runs at load 5.1 (not quiet): overlap 1.219 -> 1.280 ms on `Issue433-my-board` (0.95x) and 11.844 -> 11.549 ms on `Issue145-smoothieboard` (1.03x); drc and pull_tight 0.99x to 1.01x. Far below 1.2x: after P05.1 the loop is bound by the traversal (the stack, the dependent loads of the next pair), not the comparisons, and even an instruction-proportional gain would be 1.13x. CI on Windows and macOS was not run (CI is disabled; no other targets are installed here); nothing of the trial is in the tree. **Remainder:** confirm the rejection on a quiet machine (owned by the P01.5 quiet re-run): build `freerouting-dev` with P05.1 and with the patch applied, then `tools/bench/microbench-ab.sh <p05.1>/freerouting-dev <trial>/freerouting-dev --kernel overlap --rounds 5 fixtures/Issue145-smoothieboard.dsn fixtures/Issue433-my-board.dsn` at load at most 1.0; keep it only at 1.2x or more, then CI on all three systems.

- [~] **P05.3 — Allocator trial.**
  Try `mimalloc` as the global allocator in the CLI and GUI binaries, never in
  the library crates.
  - **Accept:**
    - Identical SES.
    - CLI wall time on `benchmark` within ±2 %.
    - GUI sustained RSS after routing `canary-pwr` three times drops by at
      least 20 %.
    - Otherwise remove it and record the numbers.

  *Partial 2026-10-18:* tried (`mimalloc` 0.1.52, `libmimalloc-sys` 0.1.49, which builds mimalloc 3.3.2 from C; MIT) as an optional `mimalloc` feature of `freerouting-cli` and `freerouting-gui`, the `#[global_allocator]` in their `main.rs` only; with the default settings it fails the RSS clause, so **it is removed**: the tree has no trace of it, the trial is `target/orchestrator/p05/p05.3-mimalloc-trial.patch` (`git apply`, then `--features mimalloc`). **SES:** the CLI with and without mimalloc wrote byte-identical sessions on 7 `benchmark` boards (`Issue433-my-board`, `Issue508-DAC2020_bm02`, `Issue690-sonde_xilinx`, `Issue508-DAC2020_bm07`, `Issue093-interf_u`, `Issue508-DAC2020_bm10`, `Issue555-CNH_Functional_Tester_1`; the bench command line, optimizer off, one thread). **CLI wall time, under load 6 to 8 (not quiet)**, 3 alternating runs: `bm10` 5.31 -> 4.95 s, `Issue555` 5.82 -> 5.77 s, `interf_u` 1.16 -> 1.12 s, the small boards equal; peak RSS about twice the system allocator's (`bm10` 31.9 -> 60.4 MB, `Issue555` 70.5 -> 127.1 MB). **Sustained RSS:** not measured in the GUI binary (its `--bench route` routes once per process); the proxy is the GUI's engine path in one process (`prepare_job` + `run_pipeline` three times on `fixtures/canary-pwr/canary-pwr-9511d9a-addonly.dsn`, `-mp 1`, optimizer off, one thread, the last routed board kept alive as the GUI keeps it; a scratch harness, not in the tree): VmRSS after routes 1, 2, 3 and after dropping the board, system allocator 78.5 / 92.1 / **118.2** / 77.0 MB; mimalloc defaults 121.8 / 138.3 / **142.5** / 142.5 MB (+21 %: fails); mimalloc with `MIMALLOC_PURGE_DELAY=0` 51.5 / 52.2 / **60.3** / 32.3 MB (-49 %, would pass), route times equal within the load's noise (41 to 47 s). The CLI with `MIMALLOC_PURGE_DELAY=0` at load 8.7 was too noisy to judge (`interf_u` 1.18 -> 1.29 s, `bm10` 5.38 -> 5.02 s). The purge delay can only be set by that environment variable or by `mi_option_set` (the crate's `extended` feature, which adds the `cty` crate; the option has no named constant in the binding). **Remainder:** an operator decision (adopt mimalloc for the GUI and plugin process with the purge delay at 0, or close), then, if adopted, the GUI-binary measurement and the CLI wall time on `benchmark` on a quiet machine, owned by the P01.5 quiet re-run: apply the patch, `cargo build --release -p freerouting-cli --features mimalloc`, and time it against the default build with the bench command line (`freerouting-dev bench --set tools/bench/sets/benchmark.txt --rust <bin> ...`, once per build).

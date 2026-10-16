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

- [ ] **P05.1 — Struct-of-arrays bounds for the tree's hot loop.**
  If P01 shows overlap queries are hot, store each tree node's children's
  bounds as struct-of-arrays and test them in a loop the compiler can
  vectorize. Check the assembly for vector instructions
  (`cargo asm` or `objdump`) on x86_64.
  - **Accept:**
    - Identical result sets on a fixed query sample on every fixture.
    - Identical SES on `benchmark`.
    - Overlap-query microbenchmark time (P01.3) before and after.
    - Kept only if faster.

- [ ] **P05.2 — Explicit SIMD octagon tests.**
  Only if P05.1's autovectorized loop leaves a measurable gap. Test 8 octagon
  bounds per comparison with `core::arch` (SSE4.1/AVX2 on x86_64, NEON on
  aarch64) behind runtime detection, with a scalar fallback.
  - **Accept:**
    - A test compares SIMD and scalar on 1 M randomized octagon pairs,
      including the `CRIT_INT` bounds, with zero differences.
    - CI builds and tests pass on Linux, Windows and macOS.
    - The microbenchmark gain is in the note.
    - Kept only if at least 1.2× on the overlap-query kernel.

- [ ] **P05.3 — Allocator trial.**
  Try `mimalloc` as the global allocator in the CLI and GUI binaries, never in
  the library crates.
  - **Accept:**
    - Identical SES.
    - CLI wall time on `benchmark` within ±2 %.
    - GUI sustained RSS after routing `canary-pwr` three times drops by at
      least 20 %.
    - Otherwise remove it and record the numbers.

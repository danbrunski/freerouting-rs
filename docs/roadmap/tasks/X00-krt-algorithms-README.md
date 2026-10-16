# X00 — Algorithms from KiCadRoutingTools

Opened 2026-10-15. Decision D9 makes the engine a pipeline of stages with
interchangeable implementations, drawn from Freerouting and from
KiCadRoutingTools (KRT, MIT, `../KiCadRoutingTools`, read at `0486e799`). The
A series defines the stage traits and the registry (A10). This series ports
KRT implementations behind them: the grid A* connection router, plane region
repair, length and time matching, differential pairs, and escape planners.
The design, the licensing and the risks are in
[algorithms.md](../algorithms.md). KRT ideas that change one of Freerouting's
own stages (planes as obstacles, neck-down, diagnostics, ripup, ordering) are
the [Q series](Q00-routing-quality-README.md).

X01, a head-to-head of Java Freerouting against KRT graded by KiCad's DRC,
runs now and sets the order of everything after it. The ports start after
board parity (rung 3).

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [X01](X01-krt-head-to-head.md) | Java jar vs KRT on `canary-pwr` + 4 KiCad demos, graded by KiCad DRC; failure causes; the X order | M / medium: two toolchains, KiCad from WSL | F02.6 (with the KRT report's grading rules and a grade-only entry); F03.4; no port work |
| [X02](X02-plane-taps-and-region-repair.md) | Plane-tap verification under D8; MST straps re-joining split pour islands | M / medium | rung 3; Q01.4; A06; B09; F04 |
| [X03](X03-vendor-krt-grid-router.md) | `crates/krt-grid-router`: KRT's Rust core with the Python binding removed, MIT | S / low | rung 3; X01's decision; F04; `NEW DEP: rustc-hash` |
| [X04](X04-grid-obstacle-raster.md) | Board items → conservative obstacle raster with per-item stamps and an audit | M / high: correctness of inflation | X03; F04 |
| [X05](X05-grid-connection-router-stage.md) | `krt-grid` connection router: exact pin connectors, path → copper, exact DRC before insert | M / high | X04; A10; B05; B09 (transaction) |
| [X06](X06-grid-astar-with-pull-tight.md) | `krt-astar` bundle: grid path then pull-tight and via optimizer; measured by region class | S / medium: expected to lose in pin fields | X05; O01, O05, O06 |
| [X07](X07-length-and-time-matching.md) | Trombone meanders to length and time targets, each bump DRC-checked | M / medium | rung 3; K05.1 (stackup); O01; B09; F04 |
| [X08](X08-diff-pairs-routing.md) | Pair model, centreline at compound width, P/N offset | L / high | rung 3; K05.3, K07.5; A04 width hook; A06; B09; F04 |
| [X09](X09-diff-pairs-vias-and-optimizer.md) | Coupled via pairs, pair-aware optimizer, measurement | M / high | X08; O01, O06 |
| [X10](X10-qfn-escape.md) | QFN/QFP escape stubs as a fanout stage, paired with Q02 | S–M / medium | rung 3; A10; B05; B09; Q02; F04 |
| [X11](X11-bus-ordering-and-attraction.md) | Bus detection, middle-out order, attraction cost | S–M / medium | rung 3; A10; A04 cost hook; F04 |
| [X12](X12-bga-escape-planner.md) | Dense-BGA channel escape planner as a fanout stage | L / high | rung 3; A10; K05.5 (part frames); B09; F04 |

Sizes: S under about 2k lines of Rust, M 2–5k, L 5–10k.

## Order and critical path

The default order follows D9: plane handling and diagnostics first (Q01–Q03
and X02), then the grid router (X03 → X04 → X05 → X06), then length matching
(X07) and differential pairs (X08 → X09), then the escape planners and bus
routing (X10, X11, X12).

**X01's preregistered decision table may reorder this.** Its outcome is
written into this section by X01.8. Examples of what it can decide:
- KRT completes more than Java on at least N−1 boards with no more
  `drc_real`: the grid router moves ahead of X02;
- wide nets at fine-pitch pins are the largest class of Java-only failures:
  Q02 and X10 move up;
- plane nets are the largest class: X02 and Q01 move up;
- KRT is worse on every board: the grid router moves to last, and X03–X06
  are reconsidered after parity.

Critical path: X01 now. Then the port's own path (J01 → B → O → A to rung 3),
F04 (provenance) and the K reader (K05, K07) for X07–X09.

## Parallel lanes and file ownership

| Checklist | Owns (creates) | Appends to |
|---|---|---|
| X01 | `tools/krt-h2h/**`, `tools/bench/sets/x01-h2h.txt`, the X01 preregistration | this README's "Order" section (X01.8 only) |
| X02 | the crate skeleton (if first), `crates/freerouting-krt/src/planes/**`, `fixtures/krt/x02-*.dsn` | `crates/freerouting-krt/src/lib.rs` (module + register line) |
| X03 | `crates/krt-grid-router/**`, `tools/krt/vendor-diff.sh` | — |
| X04 | the crate skeleton (if first), `src/grid/{mod, frame, raster, terminals}.rs`, `src/grid/{stamps, audit}.rs` only if X04.6 is triggered | `crates/freerouting-krt/Cargo.toml` (the vendored dependency), `src/lib.rs` |
| X05 | `crates/freerouting-krt/src/grid/{router, path}.rs`, `fixtures/krt/x05-*.dsn` | `crates/freerouting-krt/src/lib.rs`, `src/grid/terminals.rs` (connectors) |
| X06 | `crates/freerouting-krt/src/grid/bundle.rs`, `tools/bench/sets/x06-*.txt` | `crates/freerouting-krt/src/lib.rs` |
| X07 | `crates/freerouting-krt/src/length/**` | `crates/freerouting-krt/src/lib.rs` |
| X08 | `crates/freerouting-krt/src/diffpair/{model, centreline, offset}.rs` | `crates/freerouting-krt/src/lib.rs` |
| X09 | `crates/freerouting-krt/src/diffpair/{vias, optimizer}.rs` | `crates/freerouting-krt/src/lib.rs` |
| X10 | `crates/freerouting-krt/src/fanout/qfn.rs` | `crates/freerouting-krt/src/lib.rs` |
| X11 | `crates/freerouting-krt/src/bus/**` | `crates/freerouting-krt/src/lib.rs` |
| X12 | `crates/freerouting-krt/src/fanout/bga/**` | `crates/freerouting-krt/src/lib.rs` |

`crates/freerouting-krt` (GPL-3.0-only, with MIT portions) depends on
`freerouting` and exposes `pub fn register(registry: &mut StageRegistry)`.
The CLI (C07) and the GUI (G series) each call it once; that one line is
theirs to add. **The first X port to run creates the crate skeleton**
(`Cargo.toml`, `README.md`, `src/lib.rs`): X02.1 in the default order, X04.1
if X01 moves the grid router first. X04.1 adds the `krt-grid-router`
dependency. X02 and X07–X12 don't need the vendored crate.

After X03, the X02, X07, X10, X11 and X12 lanes touch disjoint modules and
can run in parallel; X04 → X05 → X06 and X08 → X09 are sequential.

## Rules for this series

These add to the shared rules in [README.md](README.md) and to Q00's
measurement rules, which apply here unchanged.

1. **D9 gates for every implementation:** deterministic (D2, proven with
   `tools/determinism-check.sh`); no new clearance violations; no via in a
   pad (D8); default settings give byte-identical sessions (`ses-diff` on
   `connection-parity` and `benchmark`); a default changes only by the
   measurement rules, graded by KiCad (F02.6) where a `.kicad_pcb` exists.
2. **Read KRT at the pin, through git:**
   `git -C ../KiCadRoutingTools show 0486e799:<path>`. Never the working tree,
   and never write into the KRT repository; X01 builds KRT in its own export
   and virtual environment.
3. **Licensing (F04 first).** No ported line lands before F04 has added
   `LICENSES/MIT-KiCadRoutingTools.txt`, `THIRD_PARTY_NOTICES.md`,
   `docs/PROVENANCE.md`, the SPDX header convention and its CI check.
   - A file carrying KRT code structure starts with
     `// SPDX-License-Identifier: GPL-3.0-only AND MIT`, the KRT copyright
     line, and `// Portions ported from KiCadRoutingTools <path>
     (<symbol>) at 0486e799`, and gets a `docs/PROVENANCE.md` row.
   - The vendored crate keeps `license = "MIT"` and KRT's `LICENSE` file.
   - A file written from an idea alone gets a one-line credit in its module
     docs.
4. **KRT's boards stay out of the repo.** Its 479-board stress corpus is
   mostly unpinned (31 of 479 rows pinned to a commit) with licences recorded
   as "see repo", and its `kicad_files/` boards come from other projects. None
   is committed. A board-list file with URLs, pinned commits, sha256 and SPDX
   licences is fine: that is F03.3's out-of-repo corpus. Synthetic fixtures
   written for a test go in `fixtures/krt/`.
5. **D8 beats KRT's defaults.** KRT allows a via in a pad by default and
   reuses nearby same-net vias for plane taps. Neither is ported.
6. **Determinism hazards in the Python sources** are fixed in the port, not
   copied: iteration over Python `set`s of net names or ids becomes ordered
   collections with ties by net, pad or item number; scipy
   `linear_sum_assignment` ties get a lowest-index rule; `time.time()` is
   reporting only; budgets count work.
7. **Every stage registers under an id** and is selected by A10's settings
   keys. Ids used here: `krt-region-repair`, `krt-grid`, `krt-astar`,
   `krt-meander`, `krt-diffpair`, `krt-qfn-escape`, `krt-bus`,
   `krt-bga-escape`.

## Open questions for Dan

1. **X01's KRT arm: with or without KRT's own KiCad calls?** KRT's `route.py`
   can call `kicad-cli` mid-run to find what KiCad still reports unconnected,
   and uses pcbnew for exact zone fills when it can import it. From WSL,
   pcbnew is not importable and `kicad-cli.exe` needs Windows paths.
   *Recommendation:* the primary arm runs KRT without them (its pure router,
   with its documented fallbacks recorded); a second arm uses a path-translating
   `kicad-cli` shim if X01.2's smoke test shows it works.
2. **Vendor the grid A* or rewrite it?** *Recommendation:* vendor it as its
   own MIT crate (X03), only if X01 shows the grid router wins on some board
   class. Keeping KRT's code unchanged apart from the binding lets upstream
   fixes be re-vendored by diff.
3. **`NEW DEP: rustc-hash`** for the vendored crate (FxHash, seedless).
   *Recommendation:* accept it; replacing it would widen the vendored diff.
   pyo3, numpy and mimalloc are dropped.
4. **Features that need the K reader.** Length and time matching and
   differential pairs need stackup, pair and length rules the DSN doesn't
   carry. *Recommendation:* keep them after K05/K07; don't infer pairs from
   net names alone except as a fallback the user turns on.
5. **Publishing X01's numbers.** *Recommendation:* commit the demo-board
   results (KiCad's demos ship with KiCad), keep `canary-pwr` numbers in the
   done note only, and share the demo results with KRT's author if you want;
   that sends no code either way.

# A14 — The board-parity gate (rung 3)

Opened 2026-10-15. This is rung 3 of the verification ladder: every fixture routed by
Rust with J01's work bounds gives the same session as deterministic Java. After it holds,
speed work must leave results byte-identical (P series), and quality changes become
measured divergences (Q, X series). The checklist builds the comparison, classifies the
boards the way decision D4 says, ports the Java fixture tests as work-bounded Rust tests,
and proves determinism on the benchmark set ([A00](../A00-autorouter-README.md)).

**Prerequisites:**
- A13 (`run_pipeline`), and through it the whole A series except A07.
- J01: `fixtures/reference/route/*.{json,ses}` regenerated at `30baad7e2`, with
  `maxPasses`, `maxItems`, `workLimitCutoffs` and `result` per board, the optimizer off.
- C05 (the SES writer), C03 (load path), F02.2 (`ses-diff`), F02.4 (determinism check),
  F02.5 (board sets), F02.3 (`bench`, for A14.8's record only).
- B02's tree-shape proof for A14.3's second half.
- P02.1 for A14.7.
- Decisions D2, D4, D5, D8.

## What exists and what this adds

| Exists | State |
|---|---|
| `fixtures/reference/route/` | 118 boards at `cda540f`, time-bounded and not reproducible; J01 regenerates them |
| `freerouting-dev ses-diff` (F02.2) | Structural comparison: wires and vias per net, plus statistics |
| Java fixture tests under `src/test/java/app/freerouting/fixtures/` | Not ported; they assume `TestingSettings` (copper-to-edge clearance 0, the optimizer on unless disabled). 47 classes at the pin: the routing ones are A14.6; `WorkLimitDeterminismTest` is A12.6, `RoutingPipelineComparisonTest` A13.7, `Issue954CircularKeepoutInsertionTest` O04.6, `Issue955PlacementLifecycleTest` and `Issue955RotatedPadContactTest` B05.8, `BatchOptimizerDeterminismTest` O06.5, `KiCadDrcViolationRoutingTest` is the same data as B07.2, `PCBenchPreExistingClearanceViolationsTest` is below (load-only), `Issue733DsnJsonParityTest` is dropped with the KiCad JSON export (G00 question 2) |
| A13 `run_pipeline`, `examples/route_board.rs` | Prerequisite |

What this adds: `tests/autoroute_board_parity.rs` (the gate), `tests/autoroute_fixture_ports.rs`
(the Java fixture tests), the board classification, and DIVERGENCES lines for every
expected mismatch.

## Decisions made here

- **Two classes of board**, decided per board from J01's `workLimitCutoffs`:
  - `exact`: zero Java work-limit cut-offs. The SES must match.
  - `cutoff`: one or more cut-offs. Aggregates until B02 proves the tree shape (tree
    fingerprints equal on every fixture after routing, and WorkMeter counts equal on the
    `tree-visits` probe); then they move to `exact` (D5). In the Java commit's own A/B, only
    `Issue145-smoothieboard` of four boards had a limit fire.
  At `cda540f` a third class existed: boards where Java's identity-hash airline order
  reached `LastMileBlockerRipup`. `30baad7e2` keeps input order in `NetIncompletes`, so
  those boards are `exact` too; A13.3's probe confirms the order agrees.
- **"Match" means `ses-diff` equivalent**: the same wires and vias per net, as sets, and
  equal statistics. Byte equality of the SES text is C05's concern.
- **Aggregates** follow the operator's answer to A00 open question 2. Until he answers, the
  recommendation applies: incomplete connections and clearance violations no worse than
  Java's, vias and total length within ±5 %, and no worse summed over the class.
- **The gate test is `#[ignore]`** (it routes 118 boards). It runs as a long run with
  `cargo test --release -- --ignored autoroute_board_parity`, and writes
  `target/parity/summary.md` and `summary.json`. The checklist's done notes quote them.
- **Fixture-test settings** reproduce Java's `RoutingFixtureTest`: `TestingSettings`
  forces copper-to-edge clearance 0 and leaves the optimizer at its default (on). Router
  assertions run with the optimizer off unless the Java test needs it, and the done note
  says which.

## File ownership

| Item | New files | May edit |
|---|---|---|
| A14.1–A14.5 | `crates/freerouting/tests/autoroute_board_parity.rs` | `docs/DIVERGENCES.md` (append) |
| A14.6 | `crates/freerouting/tests/autoroute_fixture_ports.rs` | — |
| A14.7, A14.8 | — | `crates/freerouting/tests/autoroute_board_parity.rs` |

## Checklist

- [x] **A14.1 — The harness.** For each board with a `fixtures/reference/route/<b>.json`:
  load through C03 with J01's route-mode settings, take `maxPasses` and `maxItems` from the
  JSON, run `run_pipeline` with the optimizer off, write the SES with C05 to
  `target/parity/<b>.ses`, and compare with `ses-diff` (library call, not a subprocess).
  Classify the board. Record per board: class, equivalent or not, the first differing nets
  (up to 10), statistics for both, Rust time.
  - **Accept:** the summary is written for all boards; a board forced to differ (one wire
    removed from the reference copy in a test) is reported with its net.
  *Done 2026-10-16:* `tests/autoroute_board_parity.rs` with `tests/board_parity/{mod,record,summary}.rs`
  (the `ses-diff` library included by path, no subprocess). The full run `CARGO_TARGET_DIR=target/orchestrator/a14/target cargo test --release -p freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity --exact --test-threads=1 --nocapture` (log `target/orchestrator/a14/gate-full.log`, `EXIT 0`, `test result: ok. 1 passed`, 5228.75 s) wrote
  `target/parity/summary.md`, `summary.json` and 118 records in `target/parity/boards/`;
  summary.md: "exact: 106 boards ... cutoff: 9 boards ... java-failure: 3 boards", with
  class, equivalence, up to 10 differing nets, both statistics and Rust time per board.
  The forced difference is `a_removed_wire_is_reported_with_its_net` (J2 with one wire
  removed from Java's session copy: reported with that net, in the record and in
  summary.md), green.

- [x] **A14.2 — Exact parity.** Every `exact` board is equivalent. Each mismatch is either
  fixed or listed in the test's expected-mismatch table with its reason and a DIVERGENCES
  line (shared rules). On every routed board, also check D8: no via overlaps an SMD pad of
  its layer unless the via's padstack allows attach, and plane-net pads each have their own
  via.
  - **Accept:** the done note gives the count of `exact` boards and that all are equivalent
    or listed; the D8 check is green on all boards.
  *Done 2026-10-16:* summary.md: "exact: 106 boards, 104 pass, 2 expected, 0 fail, 103 SES
  byte-identical, 104 equivalent". The 2 expected are `Issue684-Autorouter_PCB1_2026-5-8`
  and `Issue721-Autorouter_CE2632_HarryMu_2026-6-15` (`EXPECTED_MISMATCHES`, DIVERGENCES
  "Board parity gate (A14)": quoted class nets, the port puts them in their classes; on
  Issue684 the fanout's pin-23 trace has Java's geometry but clearance class 8 for Java's 1,
  found with `fanout_probe_lines` and `PARITY_PROBE_ITEMS=1`). Port bugs fixed on the way:
  `FoundConnectionInserter` connects to the located start and target traces even after the
  insertion split or removed them (`insert.rs` and `TraceObject` in `item_ops.rs`; it fixed
  bm10's fanout from pin 63 on, bm01's pass 3, and Smoothieboard, now 19 unrouted like Java),
  test `bm10_fanout_connects_to_the_removed_start_trace_like_java`; and the fanout's three
  attempts share one engine's drill pages (`DrillCarry`, `drills.rs`), test
  `a_carried_engine_keeps_its_drills_and_occupied_sections`. D8: "D8 breaches Rust adds
  beyond Java's: 0"; no new via overlaps an SMD pad on any board. The pad-chain check
  (a plane-net SMD pad reaching the plane chained by new copper to another pad of its net)
  finds 562 chains on 8 boards (Issue219 291, Issue732-CM5 165, Issue269-z10 52, ...), all
  identical in Java's own sessions, so they are Java's batch-router behaviour; see the
  report's operator question.

- [~] **A14.3 — Cut-off boards.** Compare on aggregates as the decisions say. When B02
  records its tree-shape proof, move the class to the exact comparison.
  - **Accept:** aggregates within tolerance on every `cutoff` board, per-board numbers in
    the summary; after B02's proof, the `cutoff` boards pass A14.2's exact check (or are
    listed with reasons).
  *Partial 2026-10-16:* the first half holds: summary.md "cutoff: 9 boards, 9 pass, 0
  expected, 0 fail, 9 SES byte-identical, 9 equivalent" and "ok: cutoff class summed over 9
  boards: java 714 unrouted, 1452 violations, 3788 vias, 64363.15 mm ... rust 714 unrouted,
  1452 violations, 3788 vias, 64363.15 mm"; per-board numbers in the table. All nine are
  byte-identical even now. Remaining: moving the class to the exact check after B02's
  tree-shape proof (B02 owns it). Note: bm10's fanout still counts one work unit less than
  Java in pull-tight on pins 119 and 170 (no geometry change); B02/O-series.

- [~] **A14.4 — Per-pass agreement and sinks.**
  - On every `exact` board, A13.7's per-pass score lines agree with Java's, so a mismatch
    is located at its first pass, not only at the end.
  - The A04 sink requirement on SES output: route every board of `benchmark.txt` with the
    `noop` and `counters` sinks; the SES files are identical.
  - **Accept:** both green; boards where the last-mile ripup fired are named in the done
    note, since that path was not reproducible in Java before `30baad7e2`.
  *Partial 2026-10-16:* per-pass: the gate compares every board's score lines with Java's
  and fails an `exact` board on the first differing pass; in the full run no `exact`
  board outside `EXPECTED_MISMATCHES` has a pass difference (only Issue721, listed, differs
  from pass 1; Issue214, a `cutoff` board, has an extra leading Java line, which is
  Issue208's "pass #1 ... 72.33 (378 unrouted" line leaked into its log by the J01 harness;
  its SES is byte-identical). Last-mile ripup fired on: Issue035-ReadPlaceScope [15, 15],
  Issue054-tairakb [24], Issue107-freq_teiler_200kHz_kicad [12, 8],
  Issue107-freq_teiler_200kHz_kicad_bad [24, 24], Issue110-Pajalnaja_stancija [24, 24],
  Issue110-RelayModule [24], Issue110-Паяльная станция [24, 24], Issue153-wavefolder
  [20, 16], Issue313-FastTest [14], Issue508-DAC2020_bm06 [24],
  Issue649-kicad_ecc83-pp_input_board_v2 [2, 2], Issue690-ecc83 [2, 2],
  PCBench-retroreflectors-CONGFLOCK [8, 6]; all equivalent to Java. Remaining: the
  `counters`-sink run of `benchmark.txt` (`PARITY_SINK=counters PARITY_SET=tools/bench/sets/benchmark.txt
  PARITY_OUT=target/orchestrator/a14/counters`, then compare its SES files with
  `target/parity/`), not run within A14's budget (about 1 h of routing); A14 re-dispatch.

- [x] **A14.5 — The unrouted two-pin board and other Java failures.** Boards whose Java
  `result` is not `completed` (J01 README's "Known Java failures", and
  `scoring-perfect-two-pin.dsn`, explained by A06.6) are compared on the recorded outcome:
  the same failure, or a listed divergence.
  - **Accept:** each such board has a line in the test with its reason.
  *Done 2026-10-16:* `JAVA_FAILURES` in `tests/autoroute_board_parity.rs` gives each board
  its expected outcome and reason: `Issue006-LPC18XX_43XX_SCH` (`load_error`, not a PCB),
  `Issue179-Autorouter_PCB1_2023-3-24` (`completed`; Java's Delaunay NPE, no session; the
  existing "Board statistics" divergence), `scoring-perfect-two-pin` (`completed`, 1
  unrouted, compared exactly; A06.6). Full run: "java-failure: 3 boards, 3 pass".

- [~] **A14.6 — Ported Java fixture tests.** Work-bounded, wall-clock assertions dropped,
  each test naming its Java source:
  - `J2ReferenceRoutingTest`: at most 3 unrouted, exactly 0 violations, at most 99 passes,
    fewer than 60 drill items;
  - `Dac2020Bm01RoutingTest` (1 pass, 2 items: at most 194 unrouted) and
    `Dac2020Bm05RoutingTest` (2 and 5 items: at most 106 and 104);
  - `Dac2020BenchmarkRoutingTest`: bm01 at 2/43/61/111/151 items gives at most
    194/161/147/134/126, 1 pass at most 56, 2 passes at most 28; bm07 0 unrouted in at most
    9 passes; bm08 0 unrouted in at most 2 passes;
  - `Issue872SingleLayerRoutingTest` test 1 (1 layer, 0 unrouted, 0 violations);
    `SetonixRoutingTest` (at most 4 unrouted, 0 violations); `Display8DigitRoutingTest`
    (0 violations, at most 20 unrouted); `MultiLayerBoardRoutingTest` (layer counts 4 and
    6, traces on inner layers); `Issue676RoutingTest` routing part (layer count stays 2);
    `InactiveLayerRoutingTest` (`Issue230-CNH_Functional_Tester_1.dsn`: no traces on the
    inactive layers, at most 5 unrouted, at most 20 passes);
    `Issue954CircularKeepoutClearanceTest` routing part (0 violations after one pass);
  - `StrictDrcRoutingTest` (exactly 16 violations, at most 30 unrouted) and
    `BbdMars64PerformanceRoutingTest` (unrouted bounds only) as `#[ignore]`, slow in Java
    too.
  - Added 2026-10-16 (the review found them unclaimed; bounds read at `a773a70ec`):
    - `BendCostRoutingTest`: J2, 2 passes, 5 items, a bend cost injected for layer 0,
      the standard `RoutingResultAssertions.check()` (completes, within its bounds);
    - `Issue676FullPassTest`: `Issue676-ch32v-tx118s`, 3 passes, completes;
    - `Issue690RoutingTest`: `Issue690-ecc83`, `Issue690-kit-dev-coldfire-xilinx_5213`
      and `Issue690-sonde_xilinx`, 1 pass, 50 items each, completes;
    - `Issue420ContributionBoardRoutingTest`: `Issue420-contribution-board`, fanout off,
      1 pass and 100 items completes with 0 clearance violations; then the optimizer on,
      1 pass, 150 items, 1 thread, completes (Java's point is "no OutOfMemoryError";
      Rust records peak RSS in the done note);
    - `Issue957SmokeRoutingTest`: `Issue957-smoke-ldo-led`, 2 passes, completes; the
      board has `ComponentOutline` items; B01's `state_hash` of the routed board equals
      the hash of its `deep_copy()`;
    - `Issue955RoutingQualityTest`: checkpoints on `Issue955-rotated-pad-contacts`,
      `Issue508-DAC2020_bm01` and `Issue026-J2_reference` at `max_items` 0, 2 and 10,
      1 pass, fanout and optimizer off, 1 thread: incompletes and clearance violations
      after are at most those before;
    - `MinTraceWidthTest` (Java fork `46bf9e2ef`, after the pin; its setting is C02's
      `router.min_trace_width_um` and A06 applies it): bm05, 1 pass, optimizer off:
      without the setting at least one trace is narrower than its class width; with it
      above every class width, none is;
    - load-only, no routing, so they may be ported earlier into `tests/java_ports.rs`
      by whoever gets there first: `CombineStackOverflowTest`
      (`Issue723-CombineStackOverflow`, ~4,000 collinear segments on one net: loading
      and combining must not overflow the stack; Rust's `combine_traces` is iterative
      or has a test at this depth), `FootprintImageVariantKeepoutTest`
      (`Issue951-corney_island_wireless`: package `ceoloide:mounting_hole_npth` and its
      variant `::1` are distinct packages with their own keepout circles, MH1 and MH5
      reference the right one, S1 and S4 exist), and
      `PCBenchPreExistingClearanceViolationsTest` (eight boards under the Java repo's
      `scripts/benchmark/fixtures/PCBench/<name>/unrouted.dsn`, read at the pin
      through git and skipped when missing: 0 pre-existing clearance violations each;
      plus `testBasePinNameNormalization`).
  - **Accept:** all green (the ignored ones run once, results in the done note); any bound
    Rust misses where Java meets it is a parity bug, fixed or listed.
  *Partial 2026-10-16:* `crates/freerouting/tests/autoroute_fixture_ports.rs`, optimizer off
  unless named, run once in release with `--include-ignored --test-threads=1`
  (`target/orchestrator/a14/fixture-ports.log`): green are `J2ReferenceRoutingTest` (2
  passes, 0 unrouted, 0 violations, drills < 60), `Dac2020Bm01RoutingTest` (142 <= 194),
  `Dac2020Bm05RoutingTest` (40 <= 106, 37 <= 104), `Dac2020BenchmarkRoutingTest` bm01 at
  43/61/111/151 items (113/101/84/74), 1 pass (54 <= 56), bm07 (4 passes, 0 unrouted),
  bm08 (2 passes, 0 unrouted), `SetonixRoutingTest` (0 unrouted, 0 violations),
  `Display8DigitRoutingTest` (20 <= 20, 0 violations), `BendCostRoutingTest`,
  `Issue690RoutingTest` (all three complete), `Issue676FullPassTest`,
  `Issue957SmokeRoutingTest` (component outlines; `state_hash` equals `deep_copy()`'s).
  Remaining:
  - two red bounds, kept `#[ignore]` with their reasons, not loosened.
    `issue508Bm01First2PassesOnly` (port 34 > 28) and `StrictDrcRoutingTest` (port 6
    violations, Java asserts exactly 16) give the same numbers with the optimizer on.
    Java's optimizer-off route references give 37 unrouted (bm01 pass 2) and 6 violations
    (Issue555), so both bounds seem to need Java's optimizer. Attributing them needs a Java
    run of the two tests at the pin and O06's optimizer.
  - not ported within A14's budget (re-dispatch A14.6):
    - `Issue872SingleLayerRoutingTest`
    - `MultiLayerBoardRoutingTest`
    - `Issue676RoutingTest`
    - `InactiveLayerRoutingTest`
    - `Issue954CircularKeepoutClearanceTest`
    - `BbdMars64PerformanceRoutingTest`
    - `Issue420ContributionBoardRoutingTest`
    - `Issue955RoutingQualityTest`
    - `MinTraceWidthTest`
    - the three load-only tests

- [~] **A14.7 — Determinism on the benchmark set.** `tools/determinism-check.sh` with
  `examples/route_board.rs` on every board of `benchmark.txt`, with P02.1's audit on and
  zero audit differences.
  - **Accept:** `identical` on all boards; load averages in the done note.
  *Partial 2026-10-16:* needs P02.1's audit, which has not landed (P02 owns it). Not
  run. The full gate routes every board once deterministically (113 of 118 SES files
  byte-identical to Java's).

- [~] **A14.8 — First speed record.** One `freerouting-dev bench --set benchmark.txt`
  run with both engines, optimizer off, on a quiet machine. Not a gate: P01 owns the
  baseline and the target.
  - **Accept:** the table and the load averages are in the done note.
  *Partial 2026-10-16:* not run: the machine was not quiet (X01's KRT and Java routes, C08,
  O06 and K02 builds ran beside A14). The gate's Rust route time summed is 5156.2 s for 118
  boards (Java's `routeMillis` are in the references); the bench record stays with P01.

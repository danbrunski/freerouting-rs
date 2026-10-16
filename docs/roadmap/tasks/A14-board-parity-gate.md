# A14 — The board-parity gate (rung 3)

Opened 2026-10-15. This is rung 3 of the verification ladder: every fixture routed by
Rust with J01's work bounds gives the same session as deterministic Java. After it holds,
speed work must leave results byte-identical (P series), and quality changes become
measured divergences (Q, X series). The checklist builds the comparison, classifies the
boards the way decision D4 says, ports the Java fixture tests as work-bounded Rust tests,
and proves determinism on the benchmark set ([A00](A00-autorouter-README.md)).

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
| Java fixture tests under `src/test/java/app/freerouting/fixtures/` | Not ported; they assume `TestingSettings` (copper-to-edge clearance 0, the optimizer on unless disabled) |
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

- [ ] **A14.1 — The harness.** For each board with a `fixtures/reference/route/<b>.json`:
  load through C03 with J01's route-mode settings, take `maxPasses` and `maxItems` from the
  JSON, run `run_pipeline` with the optimizer off, write the SES with C05 to
  `target/parity/<b>.ses`, and compare with `ses-diff` (library call, not a subprocess).
  Classify the board. Record per board: class, equivalent or not, the first differing nets
  (up to 10), statistics for both, Rust time.
  - **Accept:** the summary is written for all boards; a board forced to differ (one wire
    removed from the reference copy in a test) is reported with its net.

- [ ] **A14.2 — Exact parity.** Every `exact` board is equivalent. Each mismatch is either
  fixed or listed in the test's expected-mismatch table with its reason and a DIVERGENCES
  line (shared rules). On every routed board, also check D8: no via overlaps an SMD pad of
  its layer unless the via's padstack allows attach, and plane-net pads each have their own
  via.
  - **Accept:** the done note gives the count of `exact` boards and that all are equivalent
    or listed; the D8 check is green on all boards.

- [ ] **A14.3 — Cut-off boards.** Compare on aggregates as the decisions say. When B02
  records its tree-shape proof, move the class to the exact comparison.
  - **Accept:** aggregates within tolerance on every `cutoff` board, per-board numbers in
    the summary; after B02's proof, the `cutoff` boards pass A14.2's exact check (or are
    listed with reasons).

- [ ] **A14.4 — Per-pass agreement and sinks.**
  - On every `exact` board, A13.7's per-pass score lines agree with Java's, so a mismatch
    is located at its first pass, not only at the end.
  - The A04 sink requirement on SES output: route every board of `benchmark.txt` with the
    `noop` and `counters` sinks; the SES files are identical.
  - **Accept:** both green; boards where the last-mile ripup fired are named in the done
    note, since that path was not reproducible in Java before `30baad7e2`.

- [ ] **A14.5 — The unrouted two-pin board and other Java failures.** Boards whose Java
  `result` is not `completed` (J01 README's "Known Java failures", and
  `scoring-perfect-two-pin.dsn`, explained by A06.6) are compared on the recorded outcome:
  the same failure, or a listed divergence.
  - **Accept:** each such board has a line in the test with its reason.

- [ ] **A14.6 — Ported Java fixture tests.** Work-bounded, wall-clock assertions dropped,
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
  - **Accept:** all green (the ignored ones run once, results in the done note); any bound
    Rust misses where Java meets it is a parity bug, fixed or listed.

- [ ] **A14.7 — Determinism on the benchmark set.** `tools/determinism-check.sh` with
  `examples/route_board.rs` on every board of `benchmark.txt`, with P02.1's audit on and
  zero audit differences.
  - **Accept:** `identical` on all boards; load averages in the done note.

- [ ] **A14.8 — First speed record.** One `freerouting-dev bench --set benchmark.txt`
  run with both engines, optimizer off, on a quiet machine. Not a gate: P01 owns the
  baseline and the target.
  - **Accept:** the table and the load averages are in the done note.

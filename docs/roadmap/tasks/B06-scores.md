# B06 — Scores driven by `RouterSettings`

Opened 2026-10-15. Java chooses between boards by score:
- the batch loop decides whether a pass helped and when to stop
  (`AutorouteBatchLoop.java:285,355,388,502,525,610`);
- `BoardHistory` ranks and evicts saved boards (`BoardHistory.java:69,217`,
  `BoardHistoryEntry.java:24`);
- the optimizer accepts or rejects candidates (`BatchOptimizer.java:170,294,397,412,488`).

Every call goes through `BoardStatistics.getRouterScore(RouterSettings)` or
`getOptimizerScore(RouterSettings)`. These pick the V2 formula or the legacy
one from the settings. Rust has only the V2 formulas, with their own settings
structs. This checklist adds the dispatch on C02's `RouterSettings` and the
legacy score, with Java's float and double arithmetic, and optionally the
benchmark tooling classes.

**Prerequisites:** C02 (the settings model: `RouterSettings` with
`router_scoring`, `optimizer_scoring` and `scoring`, Java's defaults). B01.6 (the
ledger feeds the connection counts; already wired into `BoardStatistics` there).
B01.1 for the probe.

## What exists and what this adds

| Rust today (`crates/freerouting/src/`, `70389b7`) | Java at `cda540f` (`src/main/java/app/freerouting/`) |
|---|---|
| `scoring/board_statistics.rs`: every metric; `router_score_v2(&RouterScoreSettings)` (`:341`), `optimizer_score_v2(&OptimizerScoreSettings)` (`:387`), defaults (`:163-215`); checked against Java's DRC-report scores on every fixture by `tests/reference_drc.rs` | `core/scoring/BoardStatistics.java`: `calculateScore` (`:642-661`), `getMaximumScore` (`:664-666`), `getLegacyNormalizedScore` (`:669-681`), `getRouterScore(RouterSettings)` (`:688-696`), `getOptimizerScore(RouterSettings)` (`:807-816`), `legacyScoringOrDefault` (`:790-795`) |
| No legacy score; `docs/DIVERGENCES.md` says "Only the V2 router and optimizer scores … are ported" | `settings/RouterScoringVersion` {`V1_LEGACY`, `V2_CONTINUOUS`}, `OptimizerScoringVersion` {`V1_LEGACY`, `V2_LOWER_BOUND`}; defaults in `settings/sources/DefaultSettings.java:212-237` |
| — | `BoardScoreBreakdown` (192 lines), `ScoringWeightComparison` (232), `BenchmarkScoreCalculator` (251): CLI benchmark tooling only (`cli/BenchmarkAndCompareCommands`) |

## Decisions made here

- **`RouterSettings` is C02's type.** B06 adds methods on `BoardStatistics` that
  take `Option<&RouterSettings>`. `None` is Java's `null` (legacy with the
  default `scoring`). The existing `RouterScoreSettings` and
  `OptimizerScoreSettings` become views built from it, so the V2 code doesn't
  change.
- **The dispatch is asymmetric, as in Java.** The router score uses V2 only
  when `router_scoring.version == V2_CONTINUOUS`. The optimizer score uses V2
  only when `optimizer_scoring.version == V2_LOWER_BOUND`. Everything else,
  including missing settings, falls back to legacy.
- **Legacy arithmetic keeps Java's types** (D6):
  - penalties are `f32`, summed left to right: `incomplete * unrouted_net_penalty
    + violations * clearance_violation_penalty + bends * bend_penalty`;
  - costs are computed in `f64` (`trace_length_for_cost as f64 *
    default_preferred_direction_trace_cost + (vias * via_costs) as f64`, where
    `vias * via_costs` is an `i32` product) and cast to `f32`;
  - `trace_length_for_cost` is `total_length_mm` when present, else
    `total_length`;
  - the score is `max_score - penalties - costs` in `f32`;
  - the normalized score is `max(0, score / max_score) * 1000` in `f32`, and 0 when
    `max_score <= 0`.
- **The benchmark classes are optional.** Port them only if the C series asks
  for Java's benchmark commands (C07).

## File ownership

| Item | New files | May edit |
|---|---|---|
| B06.1 | `scoring/legacy.rs`, `tests/scores.rs` | `scoring/mod.rs` (append) |
| B06.2 | — | `scoring/board_statistics.rs` (new methods; the V2 functions keep their bodies), `tests/scores.rs`, `docs/DIVERGENCES.md` |
| B06.3 | `tools/java-reference/probes/board/ScoresProbe.java`, `tests/board_probe_scores.rs`, `fixtures/reference/probe/scores/*` | — |
| B06.4 | `scoring/breakdown.rs`, `scoring/weight_comparison.rs` | `scoring/mod.rs` (append), `tests/scores.rs` |
| B06.5 | `scoring/benchmark_score.rs` | `scoring/mod.rs` (append), `tests/scores.rs` |

## Checklist

- [ ] **B06.1 — The legacy score.**
  - Java: `calculateScore` (`:642-661`), `getMaximumScore` (`:664-666`:
    `maximumCount * unroutedNetPenalty`), `getLegacyNormalizedScore`
    (`:669-681`).
  - Rust: `scoring/legacy.rs`: `legacy_score(&stats, &RoutingCostSettings) ->
    f32`, `legacy_maximum_score`, `legacy_normalized_score`, with the type rules
    above.
  - **Accept:** `tests/scores.rs`:
    - three hand-computed cases, written out with their `f32` intermediates;
    - a case where computing the costs in `f32` would round differently from
      Java's `f64` path, showing the split matters;
    - the `max_score <= 0` guard.

- [ ] **B06.2 — Dispatch on `RouterSettings`.**
  - Java: `getRouterScore(RouterSettings)` (`:688-696`), `getOptimizerScore(RouterSettings)`
    (`:807-816`), `legacyScoringOrDefault` (`:790-795`: the settings' `scoring`,
    else `new DefaultSettings().getSettings().scoring`).
  - Rust: `BoardStatistics::router_score(Option<&RouterSettings>) -> f32` and
    `optimizer_score(Option<&RouterSettings>) -> f32`. The `router_score_v2` and
    `optimizer_score_v2` functions stay as the V2 branches.
  - **Accept:**
    - Unit tests for each branch: `None`; version `V1_LEGACY`; version V2; a
      missing `router_scoring` section (if C02's type allows it).
    - The DIVERGENCES line "Only the V2 router and optimizer scores … are ported"
      is removed.

- [ ] **B06.3 — The `scores` probe.**
  - Java `ScoresProbe.java`: for every `fixtures.tsv` row, load with the session
    imported and compute `new BoardStatistics(board)`. Write `getRouterScore` and
    `getOptimizerScore` (raw float bits) for each of these settings:
    `null`; `DefaultSettings` (V2); router and optimizer versions set to
    `V1_LEGACY`; V2 with each weight set to a non-default value (one variant per
    field, value 2× the default); legacy with `bendPenalty`, `viaCosts` and
    `defaultPreferredDirectionTraceCost` set to 2× their defaults.
  - Rust: `tests/board_probe_scores.rs` builds the same settings with C02's type
    and compares the bits.
  - **Accept:** every line equal on every row. A mismatch caused by a known
    statistics difference already listed in `tests/reference_drc.rs`
    (`EXPECTED_MISMATCHES`, for example Issue721 and Issue676) is listed here with
    the same reason.

- [ ] **B06.4 — Optional: `BoardScoreBreakdown` and `ScoringWeightComparison`.**
  Only if C07 ports Java's benchmark commands.
  - Java: `core/scoring/BoardScoreBreakdown.java`, `ScoringWeightComparison.java`;
    test `core/scoring/ScoringWeightComparisonTest.java` (16 tests).
  - **Accept:** the 16 tests ported and green.

- [ ] **B06.5 — Optional: `BenchmarkScoreCalculator`.**
  Same condition as B06.4.
  - Java: `core/scoring/BenchmarkScoreCalculator.java`; test
    `BenchmarkScoreCalculatorTest.java`. Its elapsed-time fields
    (`:68,91`) feed only a report and stay out of any score.
  - **Accept:** the test ported and green.

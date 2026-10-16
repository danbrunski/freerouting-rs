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

- [x] **B06.1 — The legacy score.**
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
  - *Done 2026-10-16:* `scoring/legacy.rs`: `legacy_maximum_score`, `legacy_penalties`,
    `legacy_costs`, `legacy_score`, `legacy_normalized_score` (on `&BoardStatistics` and
    C02's `ScoringSettings`, Java's `RoutingCostSettings`), with Java's `int`/`float`/`double`
    types; the clamp is Java's `Math.max`, added as `jmath::max_f32`/`max_f64`
    (`max_matches_java_math_max`: 98 pairs printed by
    `tools/java-reference/probes/JavaMathMaxProbe.java` at `a773a70ec`). `tests/scores.rs`:
    `legacy_hand_case_defaults`, `legacy_hand_case_fraction`,
    `legacy_hand_case_doubled_costs` (every `f32` intermediate written out, including the
    tie 38 999 930 -> 38 999 928); `legacy_costs_are_summed_in_double` (via cost
    16 777 217: the `f32` path gives 16 777 216, Java's `double` path 16 777 218);
    `legacy_normalized_score_guards_non_positive_maximum` (no connections, zero and
    negative penalty); `legacy_via_costs_wrap_as_java_int`; and
    `legacy_scores_match_java_bits`, all nine cases equal to the bits
    `tools/java-reference/probes/LegacyScoreProbe.java` prints at `a773a70ec`. `cargo test
    -p freerouting --test scores`: 14 passed.

- [x] **B06.2 — Dispatch on `RouterSettings`.**
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
  - *Done 2026-10-16:* `BoardStatistics::router_score`, `optimizer_score` and, new at
    `a773a70ec` (68a9efed0), `optimizer_ranking_score` (`getOptimizerRankingScore`: the V2
    optimizer score unclamped, via the new `optimizer_score_v2_unclamped`), all taking
    `Option<&RouterSettings>`; `legacy_scoring_or_default` reads `DefaultSettings`'
    `scoring` for `None`. The V2 bodies are unchanged except the final clamp, now Java's
    `Math.max(0.0, x)` (`jmath::max_f64`, so a `NaN` stays `NaN`, as the probe's
    `lengthFloor = 0` variant shows). Tests in `tests/scores.rs`:
    `no_settings_select_the_legacy_score_with_default_costs` (`None`),
    `v1_legacy_uses_the_settings_costs`, `defaults_select_the_v2_scores`,
    `router_and_optimizer_versions_dispatch_separately`,
    `v2_branches_read_the_settings_weights`,
    `optimizer_ranking_score_is_the_unclamped_v2_score`. C02's resolved `RouterSettings`
    cannot miss a section (every section resolves to Java's default); the nearest case,
    `settings_without_scoring_sections_resolve_to_the_v2_defaults`, merges a source that
    sets nothing over `DefaultSettings`. The DIVERGENCES line is removed (the KiCad JSON
    constructor sentence stays).

- [x] **B06.3 — The `scores` probe.**
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
  - *Done 2026-10-16:* `tools/java-reference/probes/board/ScoresProbe.java` (Boards block
    verbatim; `board_probe_ids::probe_loader_copies_identical` green) writes, per stage
    `load` and `ses`, the statistics the scores read and then `getRouterScore`,
    `getOptimizerScore` and `getOptimizerRankingScore` (`a773a70ec`) as raw bits for 19
    variants: `null`, defaults, `v1`, each of the 7 router and 5 optimizer weights doubled,
    `bendPenalty`, `viaCosts` and `defaultPreferredDirectionTraceCost` doubled under V1,
    and `lengthFloor = 0` (a `NaN` on boards without traces, written canonically on both
    sides). `generate.sh <a773a70ec worktree> scores`: 164 boards, 1.5 MB in
    `fixtures/reference/probe/scores/`. `tests/board_probe_scores.rs::scores_match_java`
    passes (`cargo test -p freerouting --test board_probe_scores`, 126 s): every line equal
    on 161 boards; the three listed stages are `tests/reference_drc.rs`'s
    `ISSUE721_WHY` (Issue721 `load` and `ses`: violation depths) and `ISSUE676_WHY`
    (Issue676 `ses`: Java imports none of the wires). Issue289's race does not occur, as
    the probe waits for the post-load. The first run found two statistics bugs, both fixed
    in `BoardStatistics`: Rust's float `Sum` starts at `-0.0` (Java's total trace length is
    `+0.0` without traces), and Java's `DoubleStream.sum()` is compensated; the port is
    `jmath::double_stream_sum` (`double_stream_sum_matches_java`, 28 sums printed by
    `tools/java-reference/probes/JavaDoubleSumProbe.java`, 16 of them differing from a
    plain sum).

- [~] **B06.4 — Optional: `BoardScoreBreakdown` and `ScoringWeightComparison`.**
  Only if C07 ports Java's benchmark commands.
  - Java: `core/scoring/BoardScoreBreakdown.java`, `ScoringWeightComparison.java`;
    test `core/scoring/ScoringWeightComparisonTest.java` (16 tests).
  - **Accept:** the 16 tests ported and green.
  - *Partial 2026-10-16:* not ported, by this item's own condition: C07 decides that
    Java's `--compare-boards` and `--calculate-benchmark-scores` are not ported (they exit
    2, with a DIVERGENCES line), so nothing uses these classes. The whole item stays open
    with C07, should it ever port the benchmark commands.

- [~] **B06.5 — Optional: `BenchmarkScoreCalculator`.**
  Same condition as B06.4.
  - Java: `core/scoring/BenchmarkScoreCalculator.java`; test
    `BenchmarkScoreCalculatorTest.java`. Its elapsed-time fields
    (`:68,91`) feed only a report and stay out of any score.
  - **Accept:** the test ported and green.
  - *Partial 2026-10-16:* not ported, for the reason B06.4 gives: C07 does not port
    the benchmark commands. The whole item stays open with C07.

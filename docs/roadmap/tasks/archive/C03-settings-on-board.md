# C03 — Settings applied to the board, and the routing load path

Opened 2026-10-15. When Java loads a board for routing, it does three things
the DRC path does not do:
- it tunes the per-layer trace costs to the board's shape;
- it flags the net classes named in `ignore_net_classes`;
- it applies five board overrides: copper-to-edge clearance, hole clearance,
  plane nets, plane-as-obstacle and the clearance tolerance.

These change clearance classes, search-tree shapes and costs. They therefore
change routes. None of this is ported. `drc::runner::load_design` reads the
board, expands the bounding box and reduces nets, and stops there.

This checklist ports those steps, Java's load sequence around them (in
`prepare_job`), and the read-only settings view that A01's
`AutorouteControl` consumes.

**Prerequisites:**
- C02 (C02.1, C02.2, C02.7);
- C01.1's `settings` dumps for the comparisons;
- C04 finishes C03.6 for the five fixtures that have an adjacent rules file.

## What exists and what this adds

| Exists | Where |
|---|---|
| Load for DRC: read, expand bounding box, reduce nets | `drc/runner.rs` `load_design` |
| Tree maintenance | `BasicBoard::reinsert_tree_items` (`board/basic_board.rs:1959`), `reinsert_into_trees` (crate-private, `:240`) |
| Outline access, edge-pin nets | `BasicBoard::outline` (`:173`), `edge_pin_nets` (`:1311`); `board/items/mod.rs` `BoardOutline` (no `minimum_pin_gap`) |
| Plane obstacle switch, net reduction | `RoutingBoard::change_plane_as_obstacle` (`board/routing_board.rs:94`), `reduce_nets_of_route_items` (`:123`) |
| Clearance matrix and rules | `ClearanceMatrix::append_class` (`rules/clearance_matrix.rs:188`); `BoardRules::set_hole_clearance`, `clearance_tolerance_um` (`rules/mod.rs:877-943`); `NetClass::is_ignored_by_autorouter` |
| Units and rounding | `board::structure::Unit::scale` (`board/structure/mod.rs:74`); `freerouting_geometry::jmath::java_round` |
| SES import | `io::specctra::read_session` |
| Java at `cda540f` | `settings/RouterSettings.java`: `applyBoardSpecificOptimizations` (:318-509), `applyNetClassExclusions` (:513), `setLayerCount` (:568), getters (:788-1066), `getTraceCosts` (:1052). `management/HeadlessBoardManager.java`: `applyRouterSettingsForLoadedBoard` (:845), `applyCopperToEdgeClearanceOverride` (:490), `applyHoleClearanceOverride` (:370), `assignHoleKeepoutClearanceClass` (:428), `applyPlaneNetsOverride` (:597), `applyPlaneAsObstacleOverride` (:623), `applyClearanceToleranceOverride` (:636), `applyImmediatePostLoadProcessing` (:861). `board/model/structure/BoardOutline.minimumPinGap` (:143). `management/jobs/RoutingJobScheduler.java:92-191`. `cli/CliRunner.initializeCli` |

What this adds:
- `settings/board.rs`: cost tuning, net-class exclusions and
  `BoardRouterSettings`;
- `job/` with `overrides.rs` and `prepare.rs`;
- `BoardOutline::minimum_pin_gap`.

## Decisions made here

- **Reproduce Java's order, including the step that looks redundant.**
  Costs are first tuned in `applyRouterSettingsForLoadedBoard`, on the
  bounding box *before* `expandBoundingBoxToIncludeAllItems`. That sets
  `boardSpecificTraceCostsApplied`, and the second tuning call after the
  rules file becomes a no-op. So trace costs come from the unexpanded box.
  `prepare_job` runs:
  1. the first merge (C02 `resolve`: defaults, config file, DSN,
     `-dr` rules, env, CLI);
  2. read the board;
  3. `set_layer_count` if the counts differ, then `apply_board_specific`,
     net-class exclusions, edge, hole, plane nets, plane-as-obstacle,
     tolerance;
  4. expand the bounding box and reduce nets;
  5. the rules file onto the board, and its `autoroute_settings` onto the
     settings (the C04 hook);
  6. `apply_board_specific` and net-class exclusions again;
  7. SES import.
- **The scheduler's second merge is left out.** Java re-merges at
  `RoutingJobScheduler.java:101-174`. The first merge's result comes back
  through `ApiSettings` at priority 70, and after step 3 that result has no
  null field a lower source could fill. So the re-merge changes nothing.
  C03.6's comparison against C01, on every fixture and scenario, is the
  proof. If it ever fails, the second merge goes back in.
- **The quirks are kept and named in tests.**
  - One explicit layer cost from a DSN or rules `layer_rule` sets
    `costs_initialized`. Every layer without an explicit cost then keeps
    the seeded 1.0/1.0, with no aspect add and no outer-layer add.
  - The edge-clearance "default" is detected by value
    (`|v − 250.0| < 1e-9`), so an explicit 250 is capped too.
- **The overrides run only on the routing path.** `drc::runner` keeps
  `load_design` with no overrides, because Java's `DrcRunner` loads with
  empty `RouterSettings`.
- **Board units.** `(int) Math.round(Unit.scale(um * max(1, resolution), UM,
  unit))`, computed in Java's order: `um * resolution` first, then `Unit.scale`
  (`value * from.micrometers / to.micrometers`), then `java_round`, then the
  cast to `i32`.
- **The view A01 reads.** `BoardRouterSettings` holds the final per-layer
  values with Java's getter semantics. A01 maps them to its
  `ExpansionCostFactor`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| C03.1 | `crates/freerouting/src/settings/board.rs`, `crates/freerouting/tests/settings_board.rs` | `settings/mod.rs` |
| C03.2 | `crates/freerouting/tests/outline_pin_gap.rs` | `board/items/mod.rs` (append `minimum_pin_gap` and its helpers to `impl BoardOutline` only) |
| C03.3 | `crates/freerouting/src/job/{mod.rs, overrides.rs}`, `crates/freerouting/tests/job_overrides.rs` | `lib.rs` (append `pub mod job;`) |
| C03.4 | — | `job/overrides.rs`, `tests/job_overrides.rs` |
| C03.5 | — | `job/overrides.rs`, `settings/board.rs`, `tests/job_overrides.rs` |
| C03.6 | `crates/freerouting/src/job/prepare.rs`, `crates/freerouting/tests/job_prepare_reference.rs` | `job/mod.rs`, `docs/DIVERGENCES.md` (append) |
| C03.7 | — | `settings/board.rs`, `tests/settings_board.rs` |

Paths are under `crates/freerouting/src/` unless they start with `crates/`
or `docs/`.

## Checklist

- [x] **C03.1 — Board-specific cost tuning.**
  Java: `RouterSettings.applyBoardSpecificOptimizations` (:318-509), with
  `setLayerCount` (:568) and `applyBoardSpecificOptimizationsIfNeeded`.
  Rust: `settings::board::apply_board_specific(patch: &mut SettingsPatch,
  board: &BasicBoard)`. It works on C02's Java-shaped patch, because the
  flag and the arrays decide results. Reproduce these exactly:
  - `w = bbox.width() as f64` and `h = bbox.height() as f64`, from the
    board's current `bounding_box`.
  - `h_add = 0.1 * java_round(10.0 * w / h) as f64` and
    `v_add = 0.1 * java_round(10.0 * h / w) as f64`. The operation order
    is `(10 * w) / h`, then the round, then `0.1 *`.
  - If `layers` is null or the wrong length, set the flag false and
    rebuild `layers`, keeping the old elements by index and adding empty
    ones. Otherwise fill null elements.
  - If a cost array is null or the wrong length, replace it with a
    **zero** array and set the flag false.
  - Null defaults become 1.0.
  - `current = w < h`. For each layer in order:
    - if it is a signal layer, `current = !current`;
    - a non-signal layer gets `routable = false`; a null `routable`
      becomes true;
    - a null `bend_cost` becomes `default_bend_cost`, or 0.0;
    - a null direction becomes `current`.
  - The preferred cost is the explicit layer value if there is one;
    otherwise the default if initializing; otherwise the array entry stays.
  - The undesired cost is the explicit value, or, if initializing,
    `default_undesired` and then `+=` `h_add` when `current` is horizontal,
    else `v_add`.
  - If initializing and there are more than 2 signal layers: add
    `0.2 * signal_count` to both arrays at layer 0 and layer `n − 1`,
    where those layers have no explicit cost. Then set the flag true.
  - **Accept:**
    - Port of `Issue729TraceCostSettingsTest`: the 2:1 board, the flag,
      the aspect penalty, the second call a no-op, user costs
      4.5/3.2/2.0 kept, `setLayerCount(4)` resetting the flag.
    - Port of `Issue885SettingsInitializationTest`'s explicit-cost test
      (0.25/0.50 survive).
    - Port of `RouterSettingsMergeTest`'s second test: an explicit
      `routable=false` and direction survive the tuning.
    - `explicit_layer_cost_suppresses_computed_costs`: on C01's
      `aspect-4layer` and `explicit-one-layer-cost` cases, the per-layer
      `preferredCost`/`againstCost` equal C01's dump bit for bit. The
      second case shows 1.0/1.0 on every layer but `In1.Cu`.

  *Done 2026-10-15:* `settings::board::{apply_board_specific,
  apply_board_specific_if_needed, apply_net_class_exclusions}`; tests in
  `tests/settings_board.rs` (21 pass).
  - Ports: `Issue729TraceCostSettingsTest` (5 tests: tuned once, 2:1 aspect
    penalty values, user costs 4.5/3.2/2.0 kept, `if_needed` skip and layer-count
    run, `set_layer_count(4)` resets the flag), `Issue885...` explicit-cost test
    (0.25/0.50 survive, array entries written), `RouterSettingsMergeTest`'s second
    test (explicit `routable=false` and direction survive). Extra tests for the
    outer-layer add, plane layers, zero arrays, the layer-count rebuild and a
    degenerate box.
  - `explicit_layer_cost_suppresses_computed_costs` (`reference::` module in
    `settings_board.rs`): `aspect-4layer` per-layer preferred/against costs equal
    C01's dump bit for bit (`F.Cu` 1.8/3.8, `In1.Cu` 1.0/1.5); on
    `explicit-one-layer-cost` they are 1.0/1.0 everywhere but `In1.Cu` (2.0/1.0),
    also bit-equal. `job_prepare_reference.rs` compares `layers` and both cost
    arrays on all 652 scenarios (see C03.6).

- [x] **C03.2 — `BoardOutline::minimum_pin_gap`.**
  Java: `BoardOutline.minimumPinGap`, `cornerGap` and `distanceToOutline`
  (:143-200). Rules:
  - Take the minimum over every pin and every layer from its first to its
    last, using `pin.getShape(layer - first)`.
  - For a `PolylineShape`, measure from each `cornerApprox(c)`, for
    `c < borderLineCount()`. For any other shape, measure from the four
    corners of its bounding box (`corner(c).toFloat()`).
  - A point not inside the outline has distance 0. Otherwise its distance
    is the minimum `FloatLine(corner(i), corner((i+1) % n)).segmentDistance`
    over every outline shape.
  - Return 0 early once the minimum is ≤ 0. Return +∞ for a board with no
    pins.
  - **Accept:**
    - On every fixture, the value equals C01.1's `minimumPinGap`, parsed
      from Java's text, bit for bit.
    - A test on a board with no pins gives +∞.

  *Done 2026-10-15:* `BoardOutline::minimum_pin_gap` (`board/items/mod.rs`,
  appended to `impl BoardOutline`) and `BasicBoard::minimum_pin_gap`
  (`job/overrides.rs`). `tests/outline_pin_gap.rs`:
  `minimum_pin_gap_matches_java_on_every_board` compares Java's
  `Double.toString` text with the port's on all 169 boards of the `default`
  dumps (163 finite, 6 `Infinity`): 0 mismatches. Unit tests: a board without
  pins gives +infinity (`"Infinity"`), the smallest corner distance, 0 for a pad
  on or outside the outline (and the early return), a round pad measured from
  its bounding box, empty shapes skipped.

- [x] **C03.3 — Copper-to-edge clearance override.**
  Java: `applyCopperToEdgeClearanceOverride` (:490-595).
  Rust: `job::overrides::apply_edge_clearance(board, settings, state:
  &mut OverrideState) -> Vec<LogLine>`.
  - **Ignored values.** A null value is skipped. A negative value gives a
    warning and is ignored.
  - **When to apply.** `uses_fallback_outline_class` is true when the
    outline's class equals the default net class's AREA class.
    `uses_default` is `|v − 250.0| < 1e-9`. When `uses_default`, the class
    is not the fallback and `state.edge_applied` is false, return without
    a change: an explicit DSN outline class wins over the default.
  - **The value.** Convert to board units (see Decisions). When
    `uses_default` and `minimum_pin_gap < units`, use
    `floor(gap).max(0) as i32`.
  - **The class.** Append the class `board_edge` if it is missing, and set
    `(board_edge, c)` and `(c, board_edge)` to the value for every class
    `c ≥ 1` on every layer.
  - **The outline.** Remove the outline from the trees, set its class,
    clear derived data, reinsert it, and set `state.edge_applied`.
  - **Accept:**
    - Ports of `CopperToEdgeClearanceCapTest`, both tests, on
      `PCBench-retroreflectors-CONGFLOCK.dsn`: the default is capped by the
      pin gap, and an explicit 400 µm is exact.
    - Port of `DevBoardClearanceRoutingTest`'s clearance assertions on
      `Issue558-dev-board.dsn`: 650 µm exact on every layer, outline in
      `board_edge`.
    - For every fixture, the `board_edge` row and the outline class equal
      C01.1's `default` (250, capped) and `fixture` (0) scenarios.

  *Done 2026-10-15:* `job::overrides::apply_edge_clearance`,
  `um_to_board_units`, `OverrideState`; tests in `tests/job_overrides.rs`
  (21 pass).
  - `CopperToEdgeClearanceCapTest` both tests on
    `PCBench-retroreflectors-CONGFLOCK.dsn`: pin gap 2159 < default 2500 board
    units, every layer's `board_edge` self-clearance within 1.0 of the gap; the
    explicit 400 um value is exact. Also pinned: an explicit 250 is capped like
    the default (value detection), 250 + 1e-10 too, 250.001 is not; an explicit
    DSN outline class wins over the default value; the default applies again
    once `edge_applied`; negative values warn and change nothing; Java's
    rounding order (`um * resolution`, scale, half-up round).
  - `DevBoardClearanceRoutingTest` clearance assertions on
    `Issue558-dev-board.dsn` (650 um): `board_edge` exists, the outline is in it,
    the value is exact on every layer.
  - `job_prepare_reference.rs`: the clearance matrix (so the `board_edge` row)
    and `outlineClearanceClass` equal C01.1's `default` (250, capped) and
    `fixture` (0) scenarios on every board (also `cli`, `owner-config`).

- [x] **C03.4 — Hole-clearance override.**
  Java: `applyHoleClearanceOverride` (:370) and
  `assignHoleKeepoutClearanceClass` (:428).
  - A null value is skipped. A negative value gives a warning and is
    ignored.
  - Convert to board units and call `rules.set_hole_clearance`.
  - When the value is above 0:
    - collect the obstacle areas of exact type `ObstacleArea`, with
      `component_id > 0` and a `Circle` area, in item-list order. These
      are KiCad's NPTH keepouts;
    - append `hole_edge` if it is missing;
    - for every layer and every class `c ≥ 1`, set both directions to
      `max(units, matrix(default AREA class, c, layer))`;
    - reclassify the keepouts not already in `hole_edge` and clear their
      derived data.
  - Call `reinsert_tree_items()` when the value changed or any keepout was
    reclassified.
  - **Accept:**
    - Port of `HoleKeepoutClearanceTest.npthKeepoutsGetHoleEdgeClearanceClass`
      on `Issue230-CNH_Functional_Tester_1.dsn`. Use the
      `java_fixture_test()` profile with `hole_clearance_um = 250`, as
      Java's test does: every circular package keepout is in `hole_edge`,
      at least one exists, and the `hole_edge` values are at least
      250 µm in board units on every layer.
    - The done note gives the reclassified count.
    - With the default 0, nothing changes: the tree fingerprint before
      equals the one after (use B02's fingerprint once it exists,
      otherwise item-by-item tree shapes).

  *Done 2026-10-15:* `job::overrides::apply_hole_clearance` (with
  `assign_hole_keepout_clearance_class`); tests in `tests/job_overrides.rs`.
  - `HoleKeepoutClearanceTest.npthKeepoutsGetHoleEdgeClearanceClass` on
    `Issue230-CNH_Functional_Tester_1.dsn` with `java_fixture_test()` plus
    `hole_clearance_um = 250`: `hole_edge` exists, all circular package keepouts
    are in it, at least one exists, and `hole_edge` is at least 250 um in board
    units (2500) on every layer. **40 keepouts reclassified.**
  - `hole_edge` entries equal `max(units, matrix(default AREA class, c))`
    (28 entries above the hole clearance).
  - With the default 0: `a_hole_clearance_of_zero_changes_nothing` compares the
    stored tree shapes of every item in every tree item by item before and after
    (B02's fingerprint does not exist yet): equal, no `hole_edge` class.
  - A large value (5000 um) rebuilds the trees: a detector test shows the shapes
    are stale after only setting the rule and current after the override.

- [x] **C03.5 — Plane nets, plane-as-obstacle, tolerance, net-class
  exclusions.**
  Java: `applyPlaneNetsOverride` (:597), `applyPlaneAsObstacleOverride`
  (:623), `applyClearanceToleranceOverride` (:636) and
  `RouterSettings.applyNetClassExclusions` (:513).
  - **Plane nets.** Every net whose name equals a trimmed `plane_nets`
    entry, all nets of that name, gets `contains_plane = true`.
  - **Plane as obstacle.** `change_plane_as_obstacle(value)` when set.
  - **Tolerance.** A finite value ≥ 0 sets
    `rules.clearance_tolerance_um`. Anything else gives a warning and is
    ignored.
  - **Exclusions.** For each non-blank `ignore_net_classes` entry, every
    net class whose name matches ignoring case gets
    `is_ignored_by_autorouter = true`. Rust also warns when a name matches
    no class, which Java does only on its API path.
  - **Accept:**
    - Ports of `PowerPlaneValidationTest.testPlaneNetsOverride` and
      `testPlaneAsObstacleOverride`.
    - Port of `Issue885SettingsInitializationTest`'s
      `applyNetClassExclusions` test (`POWER` flagged).
    - **`ignore_net_classes_is_honoured_headless`.** Dan uses `-inc` to
      keep the router off hand-laid power nets.
      - On `Issue558-dev-board.dsn`, `prepare_job` with a CLI patch
        `ignore_net_classes = ["Power,Default"]`, as one element the way
        a config file gives it, flags that class.
      - `BasicBoard::has_ignored_nets` is true for its pins and false for
        a `kicad_default` pin.
      - C01.1's `cli` scenario flags equal Rust's on every fixture.

  *Done 2026-10-15:* `apply_plane_nets`, `apply_plane_as_obstacle`,
  `apply_clearance_tolerance` (`job/overrides.rs`) and `apply_net_class_exclusions`
  (`settings/board.rs`).
  - `PowerPlaneValidationTest.testPlaneNetsOverride` and
    `testPlaneAsObstacleOverride` ported (`job_overrides.rs`); plane nets also
    tested for trimming, case, blanks, all subnets, already-plane nets;
    tolerance for 0, negative, NaN and infinities.
  - `Issue885...applyNetClassExclusions` (`POWER` flagged) in `settings_board.rs`,
    plus case-insensitivity, blanks and the warning for an unknown name.
  - `ignore_net_classes_is_honoured_headless` on `Issue558-dev-board.dsn`:
    `ignore_net_classes = ["Power,Default"]` as one element flags the class
    `Power,Default`; `has_ignored_nets` is true for its pins and false for a pin
    of the DSN's `kicad_default` class (which is the board's `default` class).
    The real `-inc Power,Default` splits at the comma and flags `default`
    (test `the_inc_flag_splits_at_the_comma_and_flags_the_default_class`).
  - C01.1's `cli` scenario: `isIgnoredByAutorouter` of every net class equals
    Rust's on every board (inside `job_prepare_reference.rs`), except the two
    boards with a class named `''` (see C03.6).

- [x] **C03.6 — `prepare_job`: Java's load sequence.**
  Rust: `job::prepare::prepare_job(input: &JobInput, ctx: &ResolveContext)
  -> Result<PreparedJob, JobError>`.
  - `JobInput` holds:
    - the DSN bytes and design name (the file name without its last
      extension);
    - an optional SES;
    - `rules: RulesChoice { None, Explicit(PathBuf), Adjacent }`;
    - the source patches: config file, env, CLI.
  - `PreparedJob` holds `board`, the final `SettingsPatch` and
    `RouterSettings`, `board_settings: BoardRouterSettings`, `provenance`,
    `log: Vec<LogLine>` and the optional SES summary.
  - The steps are the seven in Decisions, in that order.
  - Rules hook: until C04 lands, a rules file logs `RULES files are not
    supported yet; ignoring '<path>'`, the DRC runner's text, and is
    skipped.
  - An unreadable or unparsable DSN is a `JobError` carrying Java's
    message text (`There was a parse error while reading board file at
    '<loc>': <detail>`).
  - Add a DIVERGENCES line for the dropped second merge (see Decisions).
  - **Accept:** `job_prepare_reference.rs`, on every fixture and synthetic
    case without an adjacent rules file, with all four C01.1 scenarios,
    compares:
    - every field of the dump: settings, effective layers, flag, clearance
      matrix, outline class, plane nets, net-class flags;
    - `postLoadViolations` against Rust's full DRC count after
      `prepare_job`. This is the end-to-end check of the overrides.

    Mismatches are listed in the test with reasons. The five adjacent-rules
    fixtures (`Issue029-hw48na`, `Issue107-freq_teiler_200kHz_kicad`,
    `Issue107-freq_teiler_200kHz_kicad_bad`, `Issue593-BBD_Mars-64`,
    `Issue742-tastexx-pcb/tastexx-pcb`) and the `explicit-cost-rules` case
    are listed as waiting on C04.4. The done note gives the release-build
    time of `prepare_job` on the largest fixture against `load_design`.

  *Done 2026-10-15:* `job::prepare::{prepare_job, JobInput, PreparedJob,
  RulesChoice, JobError}`; `tests/job_prepare_reference.rs`
  (`prepare_job_matches_java_dumps`, 38 s at opt-level 1, 52 s in release; cases
  run on worker threads and merge in case order).
  - **652 board scenarios** compared (every fixture and synthetic case, four
    scenarios each) without an adjacent rules file: every field of `settings`
    (including `layers` and both cost arrays), the `effective` block, the clearance
    matrix, the outline class, `minimumPinGap`, hole clearance, tolerance,
    `ignoreConduction`, plane nets, every net class (trace widths, flags, default
    item classes), and `postLoadViolations` against Rust's full clearance check
    after `prepare_job`. The dropped second merge is proven by these comparisons:
    Java's dumps run the real one.
  - Mismatches (8, listed in `EXPECTED_MISMATCHES` with the reason and a
    DIVERGENCES line): `Issue684-Autorouter_PCB1_2026-5-8` and
    `Issue721-Autorouter_CE2632_HarryMu_2026-6-15`, four scenarios each, because
    they have a net class named `''` whose quotes Java keeps; nothing else differs
    and `postLoadViolations` match.
  - 24 scenarios (6 boards) wait on C04.4: `Issue029-hw48na`,
    `Issue107-freq_teiler_200kHz_kicad`, `Issue107-freq_teiler_200kHz_kicad_bad`,
    `Issue593-BBD_Mars-64`, `Issue742-tastexx-pcb/tastexx-pcb` and the
    `explicit-cost-rules` case (asserted by name in the test). The rules hook
    `apply_rules_file` logs `RULES files are not supported yet; ignoring '<path>'`.
  - Timing (release, largest fixture `Issue187-processor.Z80.dsn`, 1.99 MB, median
    of 5, machine busy with other builds): `load_design` 0.67 s / 0.84 s,
    `prepare_job` 0.96 s / 0.69 s over two runs: within noise of each other
    (`cargo test --release --test job_prepare_reference timing -- --ignored --nocapture`).
  - `JobError` carries Java's text (`There was a parse error while reading board
    file at '<loc>': <detail>`); a board Java fails to load must fail in the port
    (checked per scenario).

- [~] **C03.7 — `BoardRouterSettings`, the router's read-only view.**
  Java getters, which A01 and later the fanout and optimizer stages call:
  - `getTraceCosts`: per layer `(getHorizontalTraceCosts,
    getVerticalTraceCosts)`, with the array length as the count;
  - `getPreferredDirectionTraceCosts` and
    `getAgainstPreferredDirectionTraceCosts`: the explicit layer value
    first, then the array entry, then 1.0;
  - `getPreferredDirectionIsHorizontal`: falls back to `layer % 2 == 1`;
  - `getBendCost`: the layer value, else the clamped `default_bend_cost`;
  - `getLayerActive`, `getViaCosts`, `getPlaneViaCosts`,
    `getStartRipupCosts`, `getViasAllowed` and `getAutomaticNeckdown`;
  - the `fanout.{min,max}_escape_length_mm` and
    `fallback_to_board_vias` fields;
  - `trace_pull_tight_accuracy`, `neck_width_um` and `strict_drc`.

  Store the final values in plain `Vec`s, computed once per job (D1), with
  no `Option` on the hot path.
  - **Accept:**
    - Unit tests of each fallback.
    - On every fixture, the view equals C01.1's `effective` block for
      the `default` scenario.
    - `owner_config_reaches_router_view`: with C01's `owner-config`
      scenario, `start_ripup_costs()` is 1500 on `Issue413-test.dsn`. On
      `Issue029-hw48na.dsn` it equals C01's dump, the rules file's value,
      once C04 lands; mark it `[~]` until then.

  *Partial 2026-10-15:* `settings::board::BoardRouterSettings` (plain `Vec`s,
  computed once from `RouterSettings`; `PreparedJob::board_settings`). Done:
  - unit tests of each fallback (`settings_board.rs`: explicit layer value, array
    entry, 1.0 past the array, `getTraceCosts` length from the array, direction
    `layer % 2 == 1`, clamped default bend cost, unset routable, scalar getters,
    neck width);
  - on every board the view equals C01.1's `effective` block (layers' active,
    direction, bend, preferred/against/horizontal/vertical costs, the flag, via
    costs, plane via costs, start ripup, the run flags, warnings) for all four
    scenarios: inside `job_prepare_reference.rs`, where `effective` is built from
    the view;
  - `owner_config_reaches_router_view`: with the `owner-config` scenario
    `start_ripup_costs()` is 1500 on `Issue413-test.dsn`, equal to C01's dump.
  - Remaining: the `Issue029-hw48na.dsn` half of `owner_config_reaches_router_view`
    (C01's dump has 100, the rules file's value, which overrides Dan's 1500) needs
    the rules file applied: C04.3/C04.4.

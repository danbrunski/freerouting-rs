# X07 — Length and time matching

Opened 2026-10-15. Java Freerouting has per-net-class minimum and maximum
trace lengths, and it reports violations (`drc/NetIncompletes.calcLengthViolation`
at cda540f, `WindowLengthViolations`), but its autorouter never lengthens a
trace. KRT tunes lengths with trombone meanders
(`py_router/length_matching.py`, 4,182 lines: `generate_trombone_meander`,
`apply_meanders_to_route`, `get_bump_segments`, `get_safe_amplitude_at_point`,
`apply_time_matching_to_group`; delay maths in `impedance.py`; at
`0486e799`). This checklist adds a post-route stage that inserts meanders as
ordinary traces, each bump checked by the exact DRC. It is gridless by
construction: KRT's algorithm uses its grid only for a clearance margin.

Prerequisites: board parity (rung 3); O06 (the optimizer, which runs before
this stage); B09 (insert → DRC → commit or revert); K05.1 (stackup thickness
and εr, pad-to-die lengths) for X07.2's via and pad terms and for X07.7; K07.5
for KiCad length rules if they are exposed (unverified); F04; F03.1, F03.4.

## What exists and what this adds

| Exists | State |
|---|---|
| Rust `rules::NetClass::{minimum_trace_length, maximum_trace_length, length_constraint}`, `rules::NetLengthConstraint` | DSN class length rules, parsed |
| Rust `drc::incompletes::NetIncompletes::calc_length_violation`, `drc::checker` `length_violation_count` | the Java length-violation check, ported |
| Java `Route.java` "length matching area" ellipse (interactive only) | not a router feature |
| KRT: target = the longest net in a group; meanders on straight runs, spilling across runs; tolerance `--length-match-tolerance` 0.1 mm; via-barrel length and pad-to-die counted; time matching with ps/mm from εr, `(εr + 1)/2` for microstrip, `εr` for stripline, FR4 default 4.3 | the design |
| KRT hazard: groups built from Python sets of net **names** (around `length_matching.py` lines 2032, 2225, 2628), whose order varies per process | not copied |

## Decisions made here

- **Stage id `krt-meander`**, selected by `router.special.length_matching`,
  run last: after the optimizer, so pull-tight can't undo the meanders in a
  batch run. An interactive optimize later may; that is documented.
- **Targets:**
  - per net, the class's `[min, max]` window from the DSN: a net shorter than
    its minimum is lengthened to the minimum;
  - groups from a setting, `router.length_matching.groups`: a list of
    `{ name, nets: [glob], tolerance_um }`; the target is the longest member,
    measured after routing. KiCad rules from K07.5 feed the same list once they
    exist.
- **Length** = trace length + via barrel spans (stackup thickness between the
  layers, from K05.1) + pad-to-die (K05.1). Without K05.1 the last two are 0 and
  the report says "stackup unknown".
- **Meander geometry:** a bump replaces part of a straight segment with a
  U of amplitude A and pitch p: `p ≥ 2·hw + c`, A from the largest that
  passes the clearance check, stepped down by halves to `2·hw + c`; corners
  at 45° where the board's trace angle restriction allows, else 90°.
  Integer board units; bump ends at exact integer points on the segment.
- **Order:** groups by name, nets by net number, a net's straight runs by
  length descending (ties by item id), bumps along a run from its start.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X07.1 | `crates/freerouting-krt/src/length/mod.rs`, `length/measure.rs` | `crates/freerouting-krt/src/lib.rs` (append) |
| X07.2 | `length/targets.rs` | the C02 settings struct (one field) |
| X07.3 | `length/meander.rs` | — |
| X07.4 | `length/matching.rs`, `fixtures/krt/x07-*.dsn`, `crates/freerouting-krt/tests/length.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X07.5 | `length/timing.rs` | — |
| X07.6 | the X07 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [x] **X07.1 — Length measurement.**
  Source: KRT's measurement (trace + via barrel + pad-to-die).
  - **Accept:** on a fixture with known geometry, measured lengths equal hand
    values within 1 µm; without stackup data the report flags it; the value
    matches `NetIncompletes`' length for a net with no vias.
  *Done 2026-10-17:* `crates/freerouting-krt/src/length/measure.rs` (`measure_net`,
  `NetLength { trace, via_barrels, pad_to_die, vias, stackup_known }`, `Stackup::barrel_nm`,
  `LengthModel::from_kicad(&Extras)`, `Units`). Trace = `BasicBoard::net_trace_length` (Java's
  sum); barrel = stackup thickness from the via's first to last copper layer, both included
  (KRT `get_via_barrel_length`); pad-to-die = every pin's `die_length`; the stackup's copper
  layers map to board layers by order and must equal the board's layer count. Tests
  (`cargo test -p freerouting-krt --test length`, 12 pass): `measured_lengths_equal_hand_values`
  on `tests/data/x07-measure.dsn` (net M: 20 mm + 15 mm + 15·√2 mm of trace, one through via,
  the 1.6 mm two-layer stackup, `die_length` 0.5 mm on M1): trace, barrel 1600 µm, pad-to-die
  500 µm and total each within 1 µm of the hand value; without stackup `stackup_known` is false,
  barrels 0, and the stage's first log line is `krt-meander: stackup unknown: ...`; a 3-copper
  stackup on the 2-layer board does not fit. `a_net_without_vias_measures_what_java_checks`: net N
  (40 mm, class minimum 100 mm) measures `min + DesignRulesChecker::length_violation(N)` within
  1e-6 board units. The board files are in `crates/freerouting-krt/tests/data/`, not
  `fixtures/krt/`: every DSN under `fixtures/` needs a Java reference (`reference_load.rs`), as X02
  found.

- [x] **X07.2 — Targets and groups.**
  - **Accept:** unit tests: a class min length of 30 mm makes a 25 mm net's
    target 30 mm; a group of three nets targets the longest; glob patterns
    match in net-number order; an empty group is a settings error.
  *Done 2026-10-17:* `crates/freerouting-krt/src/length/targets.rs` (`parse_group(s)`,
  `glob_match`, `match_nets`, `resolve_group`, `class_window`, `class_target`, `group_targets`,
  `combine`). Settings (the C02 table, `crates/freerouting/src/settings/fields.rs`): a new
  section `length_matching` with one field `groups` (`Vec<String>`, unset) and
  `special.length_matching` (the stage id, unset); `settings_defaults.rs` lists both; the GUI
  dialog lists both in `CLI_ONLY`. The settings model has no list-of-objects type, so a group is
  one string `NAME=GLOB[|GLOB...][@TOL]` with `TOL` `<n>um`, `<n>mm` or `<n>ps` (default 100 um,
  KRT's 0.1 mm); see *Operator decisions* in the X07 report. Unit tests
  (`cargo test -p freerouting-krt --lib length`, 11 pass):
  `a_class_min_of_30_mm_makes_a_25_mm_nets_target_30_mm` (lo 30 mm, aim in [30, 30.01] mm, a
  maximum caps it), `a_group_of_three_nets_targets_the_longest` (30/35/32 mm: targets 35 mm for
  nets 3 and 7, window [34.9, 35] mm), `glob_patterns_match_in_net_number_order` (`DQ*` gives
  1, 2, 5, 9 from an unordered list; `*`, `?`, case-sensitive), `an_empty_group_is_a_settings_error`
  (`DDR=` and `DDR= | @100um` are "group DDR is empty", `DDR=ADDR*` on a board without such a net
  is "group DDR is empty: ADDR* matches no net"), `groups_parse_with_tolerances_and_sort_by_name`,
  `a_group_target_meets_a_class_window`. In the stage, a settings error changes nothing
  (`length.rs::a_settings_error_changes_nothing`).

- [x] **X07.3 — Meander bumps.**
  Source: KRT `generate_trombone_meander`, `get_safe_amplitude_at_point`,
  `get_bump_segments`.
  Rust design: `length::meander::bump(segment, at, amplitude, pitch, side,
  angle_style) -> Polyline` and the added length as an exact function of A
  and p.
  - **Accept:** unit tests: added length matches the formula; bumps on a
    45° segment stay 45°-legal; both sides are tried, left first.
  *Done 2026-10-17:* `crates/freerouting-krt/src/length/meander.rs`: `bump(segment, at, amplitude,
  pitch, side, style) -> Option<Polyline>`, `bump_points`, `Run` (a 90°/45° segment in exact
  steps of its primitive direction), `added_length(A, p, style, |g|)` = `2·A·|g|` (90°) and
  `|g|·(2A − (8 − 4√2)·c)` with `c = p/6` (45°), `amplitude_for` its floor inverse,
  `AngleStyle::for_restriction` (45° unless the board is 90°). Unit tests (11 pass with X07.2's):
  `added_length_matches_the_formula` (axis and diagonal runs, both styles, both sides: polyline
  length minus run length within 1e-6 of the formula), `bumps_on_a_45_degree_segment_stay_45_degree_legal`
  (`Polyline::is_multiple_of_45_degree`, integer corners, end points kept; a 90° board takes no
  diagonal run, an any-angle run is refused), `both_sides_are_tried_left_first` (`Side::ORDER`,
  left of +x is +y), `a_bump_that_does_not_fit_is_refused` (past the run, below 2c, i32 limit).
  The pitch the stage uses is the arm spacing `s = 2·hw + clearance` (90°) or `⌈1.5·s⌉` (45°, so
  the riser spacing `p − 2c` is at least `s`); the minimum amplitude is `max(s, 2c)`.

- [x] **X07.4 — Matching loop with the DRC gate.**
  Source: KRT `apply_meanders_to_route`.
  Each bump goes in through a B09 transaction; a bump the exact DRC rejects
  is retried at half the amplitude, then skipped. Stop when the net is within
  tolerance or no run has room; report the shortfall.
  - **Accept:**
    - on `x07-group.dsn` (three nets of different lengths with room), all
      three end within 0.1 mm of the target;
    - on `x07-tight.dsn` (no room), the shortfall is reported and the board
      has no new DRC item;
    - running the stage twice is idempotent (the second run changes nothing);
    - Java's length-violation count does not rise;
    - `tools/determinism-check.sh` passes.
  *Done 2026-10-17:* `crates/freerouting-krt/src/length/matching.rs` (`Matcher`, `run_matching`,
  `MatchReport` with per-net `NetOutcome { before, after, target, bumps, rejected, status }`) and
  `length/mod.rs` (`KrtMeander`, a `BoardStage` at `StagePhase::AfterOptimizer`, registered by
  `freerouting_krt::register` as `krt-meander`; selected by `router.special.length_matching`, which
  `StageSelection::resolve` appends after `router.special.stages`). Each bump is one
  `try_change(.., CheckPolicy::NewItemsOnly)`: the trace is removed and re-inserted with the bump
  (same ends, nets, layer, width, class, fixed state); the amplitude shares the remaining length
  over the run's bumps, at most 1 mm (KRT's default) and at least `2·hw + clearance`, rounded down
  so a net never passes its aim; a rejected bump is retried on the right, then at half the
  amplitude, down to the minimum, then the position is skipped by one pitch. Only unfixed
  single-net traces are meandered (the fixtures' wires are `(type normal)`; `(type route)` loads
  user-fixed). Tests (`cargo test -p freerouting-krt --test length`):
  `a_group_of_three_ends_within_tolerance_of_the_longest` (`x07-group.dsn`, group `G=A|B|C@100um`:
  A 50 → 69.9999 mm with 12 bumps, B 60 → 70.0000 mm with 6 bumps, C 70 mm untouched; clearance
  violations 0 → 0, Java length violations not higher, no via, every trace 45°-legal, 0
  incompletes); `no_room_reports_the_shortfall_and_adds_no_drc_item` (`x07-tight.dsn`, A 50 mm with
  a 60 mm class minimum between K and L traces 0.7 mm away: `Status::Shortfall` 10.0000 mm, 0 bumps,
  426 rejected tries, log `... shortfall 10.0000 (no run has room)`, the clearance violations by
  item pair and the SES unchanged); `running_the_stage_twice_changes_nothing_the_second_time` (group,
  tight and measure boards: SES equal, 0 bumps on the second run);
  `a_class_minimum_lengthens_a_net_to_the_minimum` (N 40 → 89.7141 mm with 29 bumps, shortfall
  10.2859 mm reported: one 40 mm run holds no more); `the_stage_is_selected_by_its_key_and_off_by_default`
  (through `run_pipeline`: registered and unselected, or with groups alone, the SES equals the
  built-in registry's; with `special.length_matching=krt-meander` the log has
  `router.special.length_matching=krt-meander` and `krt-meander: net A (group G): ... met`).
  Determinism: `tools/determinism-check.sh --out <x07>/det -- env X07_SES_OUT={OUT} <length test
  binary> write_meander_sessions --ignored --exact` (four boards' SES and logs after the stage):
  `identical` (idle, then 8 burners; load 8.05 → 8.29). Default sessions: the release CLI at the
  base commit `8e0cdf2` against this tree's (`freerouting -de <board> -do <ses>`, default settings,
  `cmp`): `connection-parity` 10/10 and `benchmark`'s Issue433, ecc83, bm02, sonde and bm07
  byte-identical (14 boards).

- [x] **X07.5 — Time matching.**
  Source: KRT `apply_time_matching_to_group`.
  Delay per layer from K05.1's εr: `sqrt(εr_eff)/c` with `εr_eff = (εr + 1)/2`
  on outer layers and `εr` on inner layers; via delay from barrel length at
  the inner εr. Targets in ps through `router.length_matching.groups`
  (`tolerance_ps`).
  - **Accept:** on a fixture with a two-layer stackup, a group matched in
    time differs in length by the expected ratio; without K05.1 the stage
    refuses time targets with a clear message. If K05.1 has not landed, mark
    `[~]` with that remainder.
  *Done 2026-10-17:* `crates/freerouting-krt/src/length/timing.rs` (`Timing { layer_ps_per_mm }`,
  `ps_per_mm`, `layer_eps_eff`, `barrel_eps`, `net_delay_ps`); a group with `@<n>ps` is matched in
  time (`Metric::Time`), each run's needed length being the delay deficit over the run's layer's
  ps per board unit. Pad-to-die adds no delay (KiCad adds `die_delay`, which K05.1 does not read).
  Tests: `a_time_group_differs_in_length_by_the_permittivity_ratio` (`x07-time.dsn`, the KiCad
  default two-layer stackup with a 1.53 mm εr 4.5 core: length group `@10um` leaves T2 3.2 mm
  (two 1.6 mm barrels) longer in trace than T1 within 11 µm; time group `@0.05ps` (T2 331.89 →
  409.85 ps against 409.85 ps, 9 bumps) leaves it `sqrt(4.5 / 2.75)` = 1.2792 times that within
  0.5 %); `time_targets_without_a_stackup_are_refused` (no stackup: the report's error is "group
  T: a time tolerance (1 ps) needs the board stackup (the KiCad reader's, K05.1), and this board
  has none that fits; group skipped", SES unchanged); `the_kicad_readers_stackup_feeds_the_model`
  (K05.1's `Extras` of `fixtures/kicad/Issue269-min_fr_test/min_fr_test.kicad_pcb` fit its
  4-layer board: barrel 1.58 mm, F.Cu at (4.5 + 1) / 2, In1.Cu at 4.5);
  `timing::tests::outer_layers_are_microstrip_and_inner_layers_stripline`. The registry's stage
  has no stackup: handing the KiCad reader's extras to the stage from the CLI and the GUI is
  theirs (C07, G series).

- [x] **X07.6 — Measurement.**
  Preregister (F03.4). Boards: at least three with length rules or groups
  (fixtures with class min lengths, and corpus boards from F03.3 with
  matched groups). Signal: nets within tolerance. Guards: unrouted, `drc_real`,
  vias in pads, Java length violations.
  - **Accept:** on every board, every group is within tolerance or each
    net's shortfall is reported; no new DRC items; the stage is kept as a
    selectable stage if that holds (it adds no change when no target exists).
  *Partial 2026-10-17:* preregistered, not run. `docs/roadmap/prereg/X07.json` (F03.4 format):
  arms off (stage unselected) and on (`special.length_matching=krt-meander`, the row's groups),
  boards `tools/bench/sets/x07-length.txt` (the three synthetic boards plus picodvi,
  pcie-active-backplane and mydevboard-rp2040dev from F03.3's corpus, groups written from their
  net names), hard `via_in_pad`, guards `clearance_violations`, `unrouted`,
  `java_length_violations`, score `nets_out_of_tolerance`, and the harness
  `crates/freerouting-krt/tests/length.rs::measurement_records_on_a_set` (ignored; writes a
  `freerouting-gate-records-1` file). The off arm routes, so the registered run waits for P04's
  default-tree decision (P00 README "Order"); owner P04, then this item. "Adds no change when no
  target exists": `the_stage_is_selected_by_its_key_and_off_by_default` and the 14-board CLI
  comparison in X07.4. KiCad grading (F02.6) of the corpus rows waits for the run, and for C07
  to register the stage in the CLI (the harness is in-process). `DIVERGENCES.md` gets no line:
  no default changes. A smoke run of the harness on the three synthetic rows (debug, this tree,
  pre-P04; not the registered result; the corpus rows were missing from the worktree):
  (via_in_pad, clearance_violations, unrouted, java_length_violations, nets_out_of_tolerance)
  off → on: x07-group (0, 0, 0, 0, 2) → (0, 0, 0, 0, 0); x07-measure (0, 0, 0, 1, 1) →
  (0, 0, 0, 1, 1) with N's 10.29 mm shortfall reported; x07-tight (0, 0, 0, 1, 1) → same, 10 mm
  shortfall reported.
  *Bias note 2026-10-17:* this harness's `prepare_default` adds no settings source, so
  neither the smoke above nor the run below went through `TestingProfile::default()` (Java's
  fixture-test copper-to-edge clearance 0); not biased.
  *Done 2026-10-17 (kept as a selectable stage, off by default):* the registered run
  (`docs/roadmap/prereg/X07.json`, 0 amendments) after P04's default-tree decision (c23d8c0:
  the default tree stays Java's), release, `X07_SET=tools/bench/sets/x07-length.txt
  X07_RECORDS=… <length test> measurement_records_on_a_set --ignored --exact` (399 s, one
  route at a time, machine load 4–11 from two other agents); log
  `target/orchestrator/meas/logs/x07.log`, records, baseline and gate log
  `target/orchestrator/meas/runs/x07-*`. (via_in_pad, clearance_violations, unrouted,
  java_length_violations, nets_out_of_tolerance), off → on:

  | board | off | on | stage report (on) |
  |---|---|---|---|
  | x07-group | (0, 0, 0, 0, 2) | (0, 0, 0, 0, **0**) | A 50 → 70.00 mm, B 60 → 70.00 mm, met |
  | x07-measure | (0, 0, 0, 1, 1) | (0, 0, 0, 1, 1) | N 40 → 89.71 mm, shortfall 10.29 mm (no run has room) |
  | x07-tight | (0, 0, 0, 1, 1) | (0, 0, 0, 1, 1) | A 50 mm, 426 bumps rejected, shortfall 10 mm |
  | picodvi | (0, 0, 5, 0, 8) | (0, 0, 5, 0, 8) | 8 nets short, each with its shortfall (DVI 21.9–36.5 mm against a 38.85 mm target, USB_D+ 2.40 mm) |
  | pcie-active-backplane | (0, 0, 0, 0, 62) | (0, 0, 0, 0, **47**) | 15 nets met, 47 with their shortfall (no run has room) |
  | mydevboard-rp2040dev | (0, 0, 34, 0, 1) | (0, 0, 34, 0, **0**) | USB pair met |

  F03.1 gate (`freerouting-dev gate --records … --prereg docs/roadmap/prereg/X07.json`, baseline
  written from this run): 0 refusals, improved 3/6 (x07-group, pcie, rp2040dev), regressed 0,
  neutral 3, so the generic N−1 rule prints FAIL. The registration's decision table and the
  Accept decide instead: "a board where nets stay out of tolerance with their shortfall
  reported is not a failure", and on the three neutral boards every net out of tolerance is
  reported with its shortfall; no guard moved on any board (no new clearance violation,
  unrouted connection, Java length violation or via in a pad). So `krt-meander` stays a
  selectable stage, off by default; no `DIVERGENCES.md` line (the decision table: no default
  changes). KiCad grading of the corpus rows still waits for C07 to register the stage in the
  CLI (the harness is in-process and writes no SES); not a condition of this Accept.

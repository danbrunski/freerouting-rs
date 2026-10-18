# Q14 — A via's clearance follows its net's class

Opened 2026-10-17, from canary-7a's `canary-pwr` grading. The DSN carries the class
clearance: in `fixtures/canary-pwr/canary-pwr-1a7df77-addonly.dsn`, class
`Power_High` has `(rule (width 300) (clearance 250))` and `(use_via
"Via[0-3]_800:400_um")`; `Switch_Node` is also 250, and every other class is 200.
Yet every clearance error canary-pwr has produced involves a via on a 0.25 mm-class
net, at 0.20–0.245 mm from another net:

| Run | Item pair | Gap (mm) |
|---|---|---|
| placement a, add-only | `+12V_BUS` via vs `LTC_INTVCC` track (class Power) | 0.2018 |
| b and a+b | `+12V_BUS` via vs `LTC_INTVCC` via (×4) | 0.2347 |
| b and a+b | `VCAP` via vs U7 pad 14 (`CAN_STB_N`) | 0.2101 |
| b and a+b | `+12V_PROT` via vs D5 pad 1 (`AON_IN`) | 0.2453 |
| `routed36` (Java `46bf9e2ef`) | `PWR_FAIL` track (default) vs a `+12V_BUS` via | 0.2186 |

None of the errors involves a Power_High track. So traces get their class's
clearance, and vias don't. Java does the same, so it is in profile `legacy` too.

**Likely cause (to confirm in Q14.1):** a via's clearance class comes from its
`ViaInfo`, not from its net's class. The DSN has no `(via ...)` in its network, so
`insert_via_infos` calls `create_default_via_infos` once, for the default net class
(`io/specctra/parser/network.rs`). Every via padstack's `ViaInfo` then gets the
default class's via clearance class, and `AutorouteControl::rebuild_via_info`
(`autoroute/control.rs`) takes `via_clearance_class` from the first via of the net's
via rule. A `Power_High` via is therefore cleared at 0.2 mm.

Prerequisites: C02 (the settings model); N01 (the fix goes into `enhanced`, and
`legacy` keeps Java's result).

## Decisions made here

- **The fix is in the router, not the export.** canary's `dsn_export.py` stays as
  it is. The fixture should expose the bug, not have it worked around with
  `class_class` rules.
- **Under `enhanced`, a via inserted for net N uses N's class's via clearance
  class** (KiCad: a via's clearance is its net class's). One way to get there is
  to create, for each class whose clearance differs from the default, the
  class-named `ViaInfo`s that `create_default_via_infos` already knows how to
  build (`<padstack>#<class>`), and point the class's via rule at them. Q14.1
  picks whichever keeps the rules file and SES output sound.
- **Setting:** `router.via_clearance = via_info | net_class`. `via_info` is Java's
  rule, under `legacy`; `net_class` becomes `enhanced`'s default once Q14.3
  accepts it (N05). Like Q13 it is a correctness fix (KiCad's DRC fails the other
  result), so acceptance is expected.

## Checklist

- [x] **Q14.1 — Confirm the cause with a test.** A two-layer fixture: class `HV`
  (clearance 250, its own `use_via`) and the default class (clearance 200). Route
  an `HV` connection that must via next to a default-class track.
  - **Accept:**
    - with `via_info`, the test shows the gap Java gives; the done note records
      it and names the code path;
    - a Java probe on the same fixture agrees.

  *Done 2026-10-17:* the likely cause above is **not** it. The reader is right: on
  both canary DSNs every class with a clearance rule gets its `<padstack>-<class>` via
  infos with its own clearance class, and `Power_High`'s via rule holds only
  `Via[0-3]_800:400_um-Power_High` (class `Power_High`, 0.25 mm). The cause is the
  fanout's via rule: with `fanout.fallback_to_board_vias` (default true) Java's
  `RoutingBoard.fanout` (`RoutingBoard.java:1041-1059` at `a773a70ec`; Rust
  `autoroute::fanout_pin::fanout_control`) appends the vias of the board's first via
  rule, `default` = `[Via[0-3]_600:300_um]` with the **default** class. The maze
  search's drill check (`checkLayerWithAnyMatchingVia`, Rust
  `session::layers::check_layer_with_any_matching_via`) and the via insertion
  (`InsertFoundConnectionAlgorithm`, Rust `path::insert::insert_via`) check and insert
  each via with its via info's own clearance class, so where the class's via does not
  fit, the fallback via goes in at 0.2 mm. That matches canary-pwr: every violating
  `+12V_BUS`/`VCAP`/`+12V_PROT` via is a 0.6 mm via (Power_High's own is 0.8 mm); in
  canary-7a's placement b and a+b add-only inputs (pass 1 by Java `7f56b062d`, exported
  `type fix`) the Rust DRC flags them too once the DSN reader gives the fixed vias their
  net class (`+12V_BUS` via vs `LTC_INTVCC` via 0.2121 mm in the model, KiCad 0.2347).
  Fixture `crates/freerouting/tests/data/quality/q14-hv-via.dsn` (HV's F.Cu pad in a
  cage of fixed default-class wires: room, 1.12 mm corridor, pocket; the other HV pin is
  a through-hole pin outside). Tests in `crates/freerouting/tests/via_clearance_net_class.rs`:
  `the_fanout_fallback_via_carries_the_default_class` (HV's control rule is
  `[Via[0-1]_800:400_um-HV]`, the board's first rule `[Via[0-1]_600:300_um]` class 1) and
  `via_info_gives_net_hv_a_default_class_via_as_java_does`: the fanout puts
  `Via[0-1]_600:300_um` with the default class at (10197, 329), **0.2322 mm** from SIG, which
  Freerouting's DRC accepts. Java: `tools/java-reference/run.sh <a773a70ec worktree> route 5`
  on the fixture alone (`REFERENCE_WORK_DIR=target/orchestrator/java-ref-work`, 0 work-limit
  cut-offs) writes the same via and a `network_out` identical to Rust's; committed as
  `tests/data/quality/q14-hv-via.a773a70ec.ses` and compared by that test, which also imports
  Java's session and measures the same 232.2 µm gap.
- [x] **Q14.2 — The fix.** With `net_class`, the gap is at least 0.25 mm between
  the `HV` via and every other net's item. The DRC (`check_clearances`) reports a
  hand-placed `HV` via at 0.2 mm as a violation under both settings.
  - **Accept:**
    - the Q14.1 fixture passes;
    - with `via_info`, the A14 exact set is unchanged;
    - the rules file round-trips the class-named via infos.

  *Done 2026-10-17:* setting `router.via_clearance` (C02 table, `opt`, unset = `via_info`;
  an unknown value fails `prepare_job`, naming the key), module
  `crates/freerouting/src/autoroute/via_clearance.rs`. Under `net_class`,
  `AutorouteControl` keeps the net class's via clearance class
  (`net_class_via_clearance_class`, set in `init_net`) and `via_info_clearance_class(info)`
  returns it for every via info: the first-via class (`via_clearance_class`), the maze
  drill check and the inserted via use it. No via infos are added, so the rules file and the
  SES writer are unchanged (the SES names the padstack only). Not via the rules: the class's
  via rule already holds the class-named infos; the fallback rule is the board's.
  Tests (`tests/via_clearance_net_class.rs`, 7 tests green):
  `net_class_keeps_net_hv_s_via_at_the_class_clearance` (the fanout's via lands at
  (10526, 0) with class HV, gap ≥ 250 µm, 0 incomplete, no clearance violation),
  `net_class_keeps_the_via_info_class_where_it_is_the_net_class`,
  `drc_reports_a_hand_placed_hv_via_at_0_2_mm_under_both_settings` (a DSN wiring via of HV
  at 200 µm: expected 2500, actual below, under both settings),
  `the_rules_file_round_trips_the_class_named_via_infos` (`rules_from_board` → `read_rules`
  on a fresh load: the same via infos with padstack and class names, among them
  `Via[0-1]_600:300_um-HV` and `Via[0-1]_800:400_um-HV`, and HV's rule
  `[Via[0-1]_800:400_um-HV]`), `unknown_via_clearance_values_fail_the_job`. The release CLI
  on the fixture: `via_info` matches Java's `network_out` byte for byte; `net_class` routes
  it with 0 unrouted and 0 violations. With `via_info` the A14 gate is unchanged: release,
  `PARITY_SET` = `connection-parity` + `benchmark`, `PARITY_SUBDIRS=1`: 22 boards (16 exact,
  6 cutoff) pass, **22 of 22 SES byte-identical**, 2219.1 s; plus the three subdirectory
  boards (`min_fr_test`, `Issue269-NoViasOnPowerPlanes`, `TestSensel`, `PARITY_BOARDS`): 3 of 3
  exact and byte-identical. 25 of 25, as at Q10.
- [x] **Q14.3 — Measure.** Register before the first number (N02's totals rule), on
  `canary-pwr` (the add-only fixture and placement variants a and b) and two corpus
  boards with class-specific clearances.
  - **Accept:**
    - KiCad reports no clearance error between a 0.25 mm-class via and another
      net;
    - the unconnected totals are within the rule;
    - if accepted, N05 adds it to `enhanced`.

  *Partial 2026-10-17:* measured as registered, **not accepted**: the totals pass, two hard
  clauses fail. Registration `docs/roadmap/prereg/Q14.json` (sha256 `98380a4e…`, pinned by
  `crates/freerouting-dev/tests/prereg_q14.rs`, written before any number; N02.1's gate mode
  does not exist, so the totals rule is stated there and computed by hand): set
  `tools/bench/sets/q14-via-clearance.txt` = canary-pwr-9511d9a (orchestrator's choice of the
  canary input; the add-only fixture and variants a and b are `not_registered`: their violating
  vias are Java's fixed pass-1 input, and a pass-1 DSN of a or b needs canary's exporter on
  Windows KiCad), `fixtures/kicad/Issue180-Test` (class Power 0.635 mm over 0.381) and corpus
  `overlord-overlord` (ETH_DIFF 0.127 mm over 0.1). Arms (common flags `-mp 1
  --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150`): `mp1`,
  `mp1+netclass`, `mp1+sweep` (`--router.autorouter.sweep=free --router.autorouter.sweep_passes=2`),
  `mp1+sweep+netclass`. canary graded with canary's `grade.sh` steps on WSL KiCad 10.0.7 through
  a path shim (calibrated: Q10's `mp1.ses` grades 45 / 4 starved thermals, as at Q10.4; this
  run's `mp1` and `mp1+sweep` sessions are byte-identical to Q10's `mp1.ses` and
  `sweep-free.ses`); the KiCad boards with `tools/kicad-oracle/grade.sh`.

  | board | arm | KiCad unconnected | class-via clearance errors | drc_real | below min | other |
  |---|---|---|---|---|---|---|
  | canary-pwr-9511d9a | mp1 | 45 | 0 | 0 | - | 4 starved thermals, inner tracks 0, via-in-pad 0 |
  | | mp1+netclass | 48 | 0 | 0 | - | 6 starved, **inner tracks 2**, via-in-pad 0 |
  | | mp1+sweep | 33 | 0 | 0 | - | 9 starved, inner 0, via-in-pad 0 |
  | | mp1+sweep+netclass | 33 | 0 | 0 | - | 9 starved, **inner tracks 4**, via-in-pad 0 |
  | Issue180-Test | all four | 0 | 0 | 10 (track_width) | 10 | 2 vias in every arm |
  | overlord-overlord | mp1 / +netclass | 5 / 4 | 35 / 8 | 462 / 458 | 169 / 188 | |
  | | mp1+sweep / +netclass | 4 / 3 | 34 / 7 | 471 / 461 | 169 / 190 | |

  Totals (bands: canary 10, Issue180 max(2, 5 % of 36) = 2, overlord max(2, 5 % of 177) = 9;
  noise √185 = 13.6): pair 1 50 → 52 (+2), pair 2 37 → 36 (−1), no board beyond twice its band:
  **within the rule**. Clause 1 holds on canary (0 KiCad clearance errors in every arm, the
  baseline included: on 9511d9a the Rust pass 1 never needed the fallback via next to another
  net). It fails by the registered metric on overlord, but all 35 → 8 and 34 → 7 there are
  KiCad's *board minimum* clearance (0.175 mm, which the KiCad export does not carry into the
  DSN), none a netclass clearance; `net_class` cuts them by 77-79 %. Hard clauses that fail:
  (a) canary's validity check `inner_tracks.py`: `net_class` routes `+5V_PI` on Power.Cu (its
  own plane layer, allowed by Java `7f56b062d`'s own-plane rule, and leaving the island), 2 and
  4 segments, so those two canary results are invalid; that is Q13's plane-island problem
  exposed by a route change, not a via clearance; (b) `below_minimum` is non-zero in every arm
  on Issue180 and overlord, from the registered `min_trace_width_um=150` floor under boards whose
  minimum is 0.254 mm and 0.175 mm (worse with `net_class` on overlord, 169 → 188/190). Reading
  of `kicad_unconnected`: the routed board's count (the registered wording also subtracts the
  input's count, which shifts both arms of a pair equally). Sessions for the official grade:
  `target/canary-handoff/2026-10-17-q14/` (four SESs, `SHA256SUMS`, `README.txt` with flags).
  **Remains:** the official canary grade of those four sessions (canary-23/canary-7a), and an
  operator decision on acceptance (see the Q14 report); variants a and b need canary-7a's
  pass-1 DSNs. Not added to `enhanced` (N05's job, and not accepted here).
  *Operator 2026-10-17:* option (a). `net_class` stays off and is measured again after Q13,
  which stops plane nets leaving their islands; the Q13 measurement runs `net_class` together
  with `own_plane`. On canary-pwr's baseline there was no class-via error to fix (the errors
  canary-7a reported came from placement variants a/b and older runs), so acceptance there
  rests on variants a/b once their pass-1 DSNs are available.
  *Note from Q13.3 (2026-10-17), not a verdict:* Q14's two invalid canary results (inner tracks
  2 and 4, `+5V_PI` on Power.Cu leaving its island) are Q13's bug, and Q13's
  `--router.power_layer_traces=own_plane` removes them. Same engine for both (origin/main
  `506264f` + Q13's diff + Q14's patch), `canary-pwr-9511d9a.dsn`, canary's `grade.sh` steps on
  WSL KiCad 10.0.7: `mp1+netclass+ownplane` **49** unconnected, 3 starved thermals, 0 copper
  errors, via-in-pad 0, inner tracks 0 (against `mp1+netclass` 48 with 2 inner tracks);
  `mp1+sweep+netclass+ownplane` **33**, 8 starved, 0 copper errors, via-in-pad 0, inner tracks 0
  (against 33 with 4). This run's `mp1+netclass` and `mp1+sweep+netclass` grade as Q14.3's did
  (48 / 6 starved / inner 2, 33 / 9 starved / inner 4). On the KiCad plugin's own export
  (`tests/data/canary-pwr-q13/plugin-e21ee05e-unrouted.dsn`, the plugin's default flags)
  `net_class` changes nothing: its session is byte-identical with and without it, with or without
  `own_plane`. Sessions in `target/canary-handoff/2026-10-17-q13/`; registration
  `docs/roadmap/prereg/Q13.json` (reported pairs). Q14.3's acceptance stays with the orchestrator.
  *Re-judged by N05.3 (2026-10-18), not accepted:* registration `docs/roadmap/prereg/N05-via-clearance.json`
  (N02 totals rule with a `totals` block, `accept: no-regress`), `net_class` together with
  `power_layer_traces = none` (N05.3's accepted value) on Q14.3's set plus the DSN of Dan's plugin run
  of 2026-10-17 (`tests/data/canary-pwr-n05/plugin-b1994423.dsn`, whose Windows-build session had two
  `+12V_BUS` via clearance errors), the KiCad boards with their own minimum width (254 and 160 um).
  `freerouting-dev gate`: **FAIL**, hard clause `overlord class_via_clearance_errors = 15` (21 in the
  baseline; all of them, in both arms, KiCad's board-minimum clearance 0.175 mm, none a netclass
  clearance). Totals 119 → 130, within the noise 13.60. canary-plugin45: 37 → 44 unconnected, class-via
  errors 1 → 0, drc_real 1 → 0; 9511d9a mp1 45 → 49, mp1+sweep 33 → 33; Issue180 identical sessions.
  Numbers and the operator decision in N05.3. `net_class` stays off.

  *Done 2026-10-18:* accepted by N05.3 on the operator's decision (a registration whose clearance metric
  counts only KiCad's net-class-rule items, on boards the last one did not route). Registration
  `docs/roadmap/prereg/N05-via-clearance-netclass.json` (`08ede5c0…`, before any number, pinned by
  `crates/freerouting-dev/tests/prereg_n05_netclass.rs`), set `tools/bench/sets/n05-via-clearance-netclass.txt`
  (canary-pwr's plugin path plus pic_programmer, tinytapeout-demo, RoyalBlue54L-Feather,
  slice-dcmt-bread-slice and CM5_MINIMA_3), `enhanced` at 4555d4b against `enhanced` plus `net_class`,
  metric `netclass_via_clearance_errors` (`tools/kicad-oracle/oracle_lib.py`, `NetclassClearanceTest`).
  Clause 1: KiCad reports no clearance error between a 0.25 mm-class via and another net in the `net_class`
  arm: canary-plugin45's `+12V_PROT` (Power_High) via 0.245 mm, the plugin path's last copper error, is
  gone (net-class via errors 1 → 0, drc_real 1 → 0), and no row has one (tinytapeout-demo's pwrhi 0.25 mm
  items are the input's USB-C pad pairs, no via). Clause 2: `freerouting-dev gate` **PASS**, totals 263 →
  261 against set noise 42.70 (canary-pwr 36 → 41, band 10). Clause 3: N05.3 added `via_clearance =
  net_class` to `ENHANCED` (`settings/profile.rs`, `tests/settings_profile.rs::enhanced_sets_via_clearance_net_class`,
  DIVERGENCES "Profile `enhanced`"). Table in N05.3. Variants a and b stay unmeasured (no pass-1 DSN; not
  registered in any of the three registrations).

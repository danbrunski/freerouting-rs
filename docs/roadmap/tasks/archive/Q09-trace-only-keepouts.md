# Q09 — Trace-only keepouts

Opened 2026-10-15 from a request by the CANary layout session (canary-09).
On a 4-layer board whose inner plane layer is mostly GND but carries power
islands, opening that layer to routing for a crowded area (an MCU under a QFN)
completes far more. On `canary-pwr` it went from 47 to 30 unconnected. But the
router then ran 166 mm of track through the +12 V island. A plain `keepout`
over the island would also bar the GND and power vias that must pass through
it. What's wanted is an area that bars traces but allows vias.

Specctra has exactly that, `wire_keepout`, and KiCad exports a rule area that
bans only tracks as one ([K06](K06-kicad-pcb-rule-areas-and-copper.md)). But
Java lexes `wire_keepout` as a plain `keepout`
(`SpecctraFileDescription.flex:201` at `cda540f`), and so does freerouting-dsn
(`canonical_keyword` in `src/parse/mod.rs`). A Java probe at `cda540f`
confirmed it: `Issue754`'s board contains `ObstacleArea layer=Ground`. Java
does not drop the scope; a report saying it did was checked and is wrong.
Changing that changes routing
results on fixtures: `Issue754-avionics_hub.dsn` has a `wire_keepout` over its
whole Ground layer, which today blocks every via there. So this is a
deliberate divergence, kept behind a setting until measured under the Q rules
([routing-quality.md](../../routing-quality.md)).

**Prerequisites:**
- board parity (rung 3);
- B09.4 (the conduction-area obstacle hook);
- Q01 (the per-class plane policy this extends);
- C02 (settings);
- K06 (the KiCad reader keeps tracks-only areas apart);
- F03 (the measurement gate and preregistration).

**Fixture needed:** a `canary-pwr` variant with the inner plane layer opened
to routing and a `wire_keepout` drawn over the +12 V island. canary-09 has
been asked for it.

## What exists and what this adds

| Exists | State |
|---|---|
| freerouting-dsn `ast::KeepoutKind::{Keepout, ViaKeepout, PlaceKeepout}`; `canonical_keyword` | `wire_keepout` becomes `Keepout` at the lexer; the AST can't tell the two apart (`tests/syntax.rs` asserts this) |
| Board areas (`board::items::AreaKind`) | `Obstacle` (blocks traces and vias of foreign nets), `ViaObstacle` (vias only), `ComponentObstacle`, `Conduction { is_obstacle, … }` |
| `BasicBoard::is_trace_obstacle` | The trace-obstacle predicate the search uses; its conduction branch gets Q01's hook through B09.4 |
| Q01 | Pours of listed classes block foreign nets that have no pin or via inside them |

What this adds:
- `wire_keepout` kept as its own keyword;
- a trace-only area kind;
- a setting that chooses Java's reading or the trace-only one;
- a trace-only mode for Q01's plane policy;
- a measurement that decides the defaults.

## Decisions made here

- **Parse faithfully, build Java's way by default.** The AST keeps
  `wire_keepout` distinct and writes it back unchanged. The board builder maps
  it to a full keepout unless `router.wire_keepout = "traces_only"`. With the
  default, every route is byte-identical to today's.
- **A new area kind, `AreaKind::TraceObstacle`.** It is the counterpart of
  Java's `ViaObstacleArea`: it blocks traces of every net and no vias.
  `is_trace_obstacle` is true for it, and the via-obstacle predicate is
  false.
- **Q01 gains a block mode.** A plane policy entry can block foreign
  `traces` only, or `traces_and_vias` (Q01's current behaviour). Trace-only
  is canary-09's "no foreign traces, foreign vias OK".
- **Defaults change only by measurement**, under the Q rules: at least three
  boards, paired, better on N−1 and worse on none, graded by the KiCad
  oracle. The guards hold: no new clearance violations, no via in a pad, no
  copper narrower than its net class (D8).

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q09.1 | — | `crates/freerouting-dsn/src/parse/mod.rs`, `src/ast.rs`, `src/write/*.rs`, `tests/syntax.rs` |
| Q09.2 | `crates/freerouting/tests/trace_obstacle.rs` | `crates/freerouting/src/board/items/mod.rs`, `board/basic_board.rs`, `drc/clearance.rs` |
| Q09.3 | — | `crates/freerouting/src/io/specctra/` (board build), C02's settings table (append) |
| Q09.4 | — | Q01's area-policy module (B09.4) |
| Q09.5 | `tools/ab/q09-trace-only-keepouts.json` | `docs/DIVERGENCES.md` |
| Q09.6 | — | `crates/freerouting-dsn/src/parse/mod.rs` (`skip_unknown`), `tests/syntax.rs` |

## Checklist

- [x] **Q09.1 — Keep `wire_keepout` in the DSN syntax tree.**
  Add `KeepoutKind::WireKeepout`, read `wire_keepout` into it, and write it
  back as `wire_keepout`.
  - **Accept:**
    - `tests/syntax.rs` asserts that `wire_keepout` reads as `WireKeepout`,
      replacing today's assertion that it reads as `Keepout`.
    - `Issue754-avionics_hub.dsn` round-trips with its `wire_keepout`
      intact.
    - `reference_load` and `reference_drc` are unchanged on every fixture,
      because the board builder still maps it to a keepout.
  *Done 2026-10-17:* the kind, reader and writer landed with K06.1 (`ast.rs`, `parse/{mod,dsn,common}.rs`);
  `tests/syntax.rs` asserts the `WireKeepout` reading (`full_design`, `wire_keepout_keeps_its_kind`; the
  old `Keepout` assertion is gone). New `issue754_round_trips_its_wire_keepout`: Issue754 reads one
  `WireKeepout`, `to_dsn` writes one `(wire_keepout` and no `(keepout`, and the text reads back to the
  same tree. `cargo test -p freerouting --test reference_load --test reference_drc`: 1 and 4 passed,
  `EXPECTED_MISMATCHES` untouched; the builder still maps it to a keepout unless Q09.3's setting says
  otherwise.

- [x] **Q09.2 — A trace-only area kind.**
  `AreaKind::TraceObstacle`: an obstacle to traces of every net, never to
  vias. Wire it into the search-tree predicates and the DRC. A trace inside it
  is a violation; a via inside it is not.
  - **Accept:**
    - unit tests on a synthetic board: a trace through the area is refused
      by `is_trace_obstacle` and reported by the DRC;
    - a via inside it is accepted and not reported;
    - an existing `Obstacle` area still blocks both.
  *Done 2026-10-17:* `AreaKind::TraceObstacle` (`board/items/mod.rs`; statistics category `ObstacleArea`,
  `is_drillable` true for every net). `BasicBoard::is_trace_obstacle` true for every net it does not
  contain; `is_obstacle`: a trace against it and it against a trace true, a via either way false
  (`board/basic_board.rs`); the shover's pad checks skip it (`shove/trace_entries.rs`), its spring
  treats it as a keepout (`shove/spring.rs`); the DRC follows `is_obstacle` (`drc/clearance.rs`
  unchanged; `drc/checker.rs` names it `TraceObstacleArea`). The other exhaustive matches got an arm
  (DSN writer: `wire_keepout`; GUI scene and violations panel, selection filter, item info: a
  keepout; `freerouting-krt` raster: no via obstacle). Tests in `tests/trace_obstacle.rs` on
  `tests/data/quality/q09-wire-keepout.dsn` (synthetic, three layers):
  `a_trace_through_a_trace_only_area_is_refused_and_reported` (`is_trace_obstacle` for A and another
  net, `is_obstacle` both ways, `clearance_violations` and the full `DesignRulesChecker::check` name the
  pair), `a_via_inside_a_trace_only_area_is_accepted_and_not_reported` (drillable, no obstacle either
  way, no violation, not in the DRC), `an_obstacle_area_still_blocks_traces_and_vias` (the B.Cu keepout,
  and the wire_keepout built Java's way: trace and via reported, not drillable).
  `cargo test -p freerouting --test trace_obstacle`: 8 passed.

- [x] **Q09.3 — The `router.wire_keepout` setting.**
  `"keepout"` (default, Java's reading) or `"traces_only"`. The board builder
  maps `WireKeepout` to `Obstacle` or to `TraceObstacle` accordingly. Append
  the key to C02's settings table.
  - **Accept:**
    - with the default, `ses-diff` is identical on the whole `benchmark` set
      and on `Issue754`;
    - with `traces_only`, `Issue754`'s Ground layer accepts vias, and a test
      shows it.
  *Done 2026-10-17:* `router.wire_keepout` (`settings/fields.rs`, `opt` string, unset = `keepout`;
  appended to the C02 table, to `settings_defaults` and to the GUI's list of keys without a control;
  any other value fails the job naming it). The DSN reader records the keepouts it builds from a
  structure-level `wire_keepout` (`BasicBoard::wire_keepouts`, `parser/structure.rs`); the new
  `io::specctra::wire_keepout::apply`, called in `prepare_job` and `prepare_board_for_routing` before
  the pour guard, makes them `TraceObstacle` with `traces_only` (`make_wire_keepouts_trace_only`; the
  trees hold the same shapes, so nothing is reinserted). An image-level `wire_keepout` stays a
  package keepout. For vias to go through, the maze search (A03's drills) lets a drill pass a layer on
  which room completion at its location gives no room because only via-passable trace obstacles cover
  it: the layer keeps no room and is not expanded onto, and the via is checked there against the
  drill's shape (`autoroute/session/drills.rs`, `layers.rs`); it runs only when
  `AutorouteControl::vias_pass_trace_only_areas` (set in `init_net` from
  `BasicBoard::may_let_vias_through`) is true, never on the parity path.
  - Default identical: the A14 gate (release, `PARITY_SUBDIRS=1`) on all 16 `benchmark` boards, the
    10 `connection-parity` boards (`Issue433` is in both), `Issue754-avionics_hub` and
    `canary-pwr/canary-pwr-9511d9a-in2-open-wirekeepout`: 27 boards, exact 21 and cutoff 6, all pass,
    **27 of 27 sessions byte-identical** to Java's references (which the engine at `f03b0db` matched
    byte for byte; Q01.2's run), 2306 s; stronger than `ses-diff`.
  - `traces_only`: `issue754_ground_accepts_vias_with_traces_only` (the Ground area is `Obstacle` and
    not drillable by default, `TraceObstacle` and drillable with the setting; a via of SERVO_PWR at
    its centre is an obstacle and a violation by default, neither with the setting);
    `with_traces_only_the_router_puts_a_via_through_the_wire_keepout` (`q09-wire-keepout.dsn`: by
    default A stays unrouted with no via; with `traces_only` A routes through a via, no trace on
    In1.Cu, no new violation); `the_default_builds_a_wire_keepout_as_java_does`.
  - D2: `tools/determinism-check.sh` on `q09-wire-keepout.dsn` with `traces_only` and on
    `q09-plane-island.dsn` with Q09.4's `traces` mode (`-mp 5`, release CLI): `identical` both.

- [x] **Q09.4 — Trace-only mode for Q01's plane policy.**
  Extend Q01's policy with `block = "traces" | "traces_and_vias"`. With
  `traces`, a pour of a listed class bars foreign traces and lets foreign
  vias through, still with full clearance.
  - **Accept:**
    - a synthetic board with a +12 V island on an inner layer: a foreign GND
      via passes through the island with correct clearance;
    - a foreign trace doesn't;
    - the `traces_and_vias` mode keeps Q01's behaviour, byte-identical.
  *Done 2026-10-17:* `router.plane_obstacle_block` (`settings/fields.rs`, `opt` string, unset =
  `traces_and_vias`; any other value fails the job naming it). `quality::pour_guard::PlaneBlock` and
  `ExemptionTable::with_block`; the B09.4 hook (`rules/area_policy.rs`) gains
  `AreaObstaclePolicy::blocks_vias` (default `blocks`) and `lets_vias_through` (default false), read by
  `is_obstacle` for a via, the drill page (`is_drillable_with_policy`) and the shover's pad checks
  (`area_policy_blocks_any_net_vias`). The maze search lets a drill pass a layer where only such areas
  cover its location (shared with Q09.3, see there). Tests in `tests/trace_obstacle.rs` on
  `tests/data/quality/q09-plane-island.dsn` (a P12 plane of class `Pwr` over all of In1.Cu, GND from an
  F.Cu pad to a B.Cu pad, S across keepout walls on F.Cu and B.Cu):
  `traces_mode_lets_a_foreign_gnd_via_through_the_island` (GND complete through a via, the via has no
  clearance violation, no GND or S trace on In1.Cu, S stays open, violations as in the input);
  `traces_and_vias_mode_keeps_q01_behaviour` (key empty: GND and S complete, S on In1.Cu; Q01's mode:
  GND and S open, no GND via; the SES with the block unset equals the SES with `traces_and_vias` byte
  for byte on this board and on `q01-pour-between-pads.dsn`; `vias` is refused naming the key).
  Q01's own tests unchanged: `cargo test -p freerouting --test quality_pour_guard`: 13 passed.
  "Correct clearance" is read as: the via keeps every clearance it keeps in Java; to the island itself
  it is what a via is to any plane that is no obstacle, which KiCad refills around.

- [~] **Q09.5 — Measure and decide.**
  Preregister the A/B (F03.4) before running it:
  - **arms:** the default; `router.wire_keepout = "traces_only"`; and the
    Q01 `traces` mode;
  - **boards:** `Issue754`, the requested `canary-pwr` inner-layer variant,
    and the `benchmark` set;
  - **metrics:** unrouted connections, and track length inside foreign-net
    islands. The 166 mm through the +12 V island is the number to beat.
  Grade through the KiCad oracle (F02.6).
  - **Accept:**
    - the F03.1 gate result is in the done note with every board's numbers;
    - if it passes, the default changes and DIVERGENCES gets a line with
      the numbers;
    - if it fails, the setting stays opt-in, and the numbers stay in the
      note. The setting stays either way, because the KiCad plugin can pass
      it for boards that need it.
  *Partial 2026-10-17:* preregistered before any number in `tools/ab/q09-trace-only-keepouts.json`
  (sha256 `d1283eb9df39d66f30a75e126823028b1e6b45e298de5f8912900a6d416db01b`, pinned by
  `crates/freerouting-dev/tests/prereg_q09.rs`; arms `keepout` (baseline), `traces_only` (candidate)
  and `plane_traces` (`plane_obstacle_net_classes=Power_High`, `plane_obstacle_block=traces`, judged by
  a second gate run with it as the candidate); boards in `tools/bench/sets/q09-wire-keepouts.txt`
  (Issue754, canary-pwr `in2-open-wirekeepout` and `in2-open`, one canary group), the `benchmark` set
  as the identity check; metrics `via_in_pad`, `below_minimum` (hard), `drc_real` (guard),
  `unrouted`, `foreign_island_mm` (scores)). With two eligible boards the pass rule's three-board
  minimum can't be met, so the registration says the defaults can't change on this set and the
  setting stays opt-in unless a third eligible board is added by amendment.
  - **Pre-P04 dry run, not judged** (release CLI of this diff on `f03b0db`, the A13 flags `-mp 1
    --gui.enabled=false --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150`;
    Freerouting's own end-of-route counts in the arm's mode; `foreign_island_mm` by a sampling script
    over the SES, +12V_BUS and +5V_PI on Power.Cu, 5 um steps):

    | board | arm | unrouted | violations | vias (SES) | foreign_island_mm | s |
    |---|---|---|---|---|---|---|
    | Issue754 | keepout | 52 | 211 | 0 | - | 40.7 |
    | Issue754 | traces_only | 10 | 200 | 181 | - | 55.4 |
    | in2-open-wirekeepout | keepout | 108 | 77 | 105 | 0.00 | 50.5 |
    | in2-open-wirekeepout | traces_only | 91 | 40 | 135 | 0.00 | 49.6 |
    | in2-open | default | 94 | 40 | 138 | 196.05 (+12V_BUS 181.18) | 34.3 |
    | in2-open | plane_traces | 95 | 40 | 137 | 18.09 (+12V_BUS 13.15) | 46.9 |

    canary-pwr's input has 40 violations, so `traces_only` adds none and the keepout reading adds 37
    (Java's reference at `7f56b062d` has the same 77). The 18 mm left with `plane_traces` belong to
    /Holdup/VCAP and /Pi_Interface/PI_3V3, which have a pin inside the island, so Q01's rule exempts
    them. `freerouting-dev ses-diff` imports into a board built Java's way, where every via in a
    `wire_keepout` is a violation, so its counts (canary 77 -> 122, Issue754 286 -> 467) are not the
    candidate's; the KiCad grade (tracks-only rule areas) is the oracle. The script's 181 mm for the
    +12 V island on the Java `46bf9e2ef` in2-open session is the canary session's 166 mm measured on
    the plane polygon rather than KiCad's fill.
  - Remains (Q09.5): the registered run after P04's default-tree decision, with KiCad grading of the
    canary-pwr sessions by canary-23 (`via_in_pad`, `below_minimum`, `drc_real`, KiCad
    `unconnected_items`), a committed `foreign_island_mm` tool, and the decision. DIVERGENCES has the
    opt-in lines (`wire_keepout`, `plane_obstacle_block`); no default changed.

- [x] **Q09.6 — Warn on every skipped scope except a documented allowlist of KiCad scopes that carry no constraint.**
  (Amended 2026-10-17: the allowlist is `property` inside `layer`, KiCad's
  `(property (index N))` stack-up index, which Java's `Structure` also
  skips. It would otherwise warn on every KiCad DSN.)
  Today an unknown named scope is skipped silently. `skip_unknown` warns only
  for a scope with no keyword, as Java's `skipScope` does. A silently ignored
  constraint is how a board gets routed through an area its author barred
  (canary-09's lesson). Warn once per unknown keyword per file, naming the
  keyword, the enclosing scope and the first position. This doesn't change
  what is read, so it isn't a routing divergence. Keep these warnings in the
  parser's warning list, apart from the wiring warnings that `reference_load`
  compares with Java.
  - **Accept:**
    - a test DSN with an invented `(foo_keepout ...)` in the structure scope
      gets one warning naming `foo_keepout` and `structure`;
    - the done note lists the unknown keywords found across all fixtures,
      with counts;
    - `reference_load` and `reference_drc` are unchanged.
  *Done 2026-10-17:* `Parser::skip_unknown` (`crates/freerouting-dsn/src/parse/mod.rs`) warns the first
  time each named keyword is skipped in a file: "unknown scope '<kw>' in <scope> skipped (reported once
  per file, at its first position)", at the scope's position; a scope without a keyword warns each time
  as before. The warnings stay in `parse_with_warnings`'s list; `build_board` uses `Pcb::parse`, so the
  board reader's warnings (what `reference_load` compares) are unchanged. `tests/syntax.rs`:
  `an_unknown_keepout_warns_once_naming_keyword_and_scope` (two `(foo_keepout ...)` in `structure`: one
  warning naming `foo_keepout` and `structure` at 4:6, the tree equal to the text without them);
  `unknown_scopes_and_stray_words_are_skipped` and `invalid_shapes_are_skipped_with_a_warning` updated
  (they now also see `foo`, `property` and `polyline`). Tally over the 241 DSN, SES and rules files under
  `fixtures/` (`reference/` excluded; `tally_unknown_scopes_across_fixtures`, ignored), files per
  keyword: DSN `layer/property` 169, `structure/grid` 7, `image/clearance_class` 3,
  `parser/case_sensitive` 3, `parser/via_rotate_first` 3, `net/type` 1,
  `autoroute_settings/eu.mihosoft.freerouting.autoroute` 1; rules files
  `autoroute_settings/app.freerouting.autoroute` 3; no SES file skips a named scope.
  `reference_load` 1 passed, `reference_drc` 4 passed.
  *Allowlist 2026-10-17:* `QUIET_SKIPS` in `parse/mod.rs` = `[("layer", "property")]`, keyed by
  (enclosing scope, keyword); test `only_listed_kicad_scopes_skip_without_a_warning` (two layer
  properties: no warning; `property` in `structure`: one warning). `layer/property` was verified
  constraint-free at `a773a70ec` (`Structure.readLayerScope` reads only `type`, `rule`, `use_net` and
  passes the rest to `skipScope`). Kept warning: `parser/case_sensitive` and `parser/via_rotate_first`
  (Eagle, not KiCad; they bear on name matching and via geometry), `structure/grid`,
  `image/clearance_class`, `net/type` and the two `autoroute_settings` vendor namespaces (settings or
  constraints). The tally test now no longer lists `layer/property`. Of the 9 GUI snapshots re-recorded
  for "1 warnings", 8 are back at HEAD and pass unchanged; `ratsnest_issue143.png` stays re-recorded
  because its CadSoft DSN skips `parser/case_sensitive` and `parser/via_rotate_first` (GUI tests 24 + all
  other targets passed).


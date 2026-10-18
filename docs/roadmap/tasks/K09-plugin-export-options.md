# K09 — Plugin export options that match the graded canary flow

Opened 2026-10-17, from the Java Freerouting session (freerouting-08) on
`canary-pwr`:
- **Router parity holds.** The Windows build `97d10d6` routes canary's own DSN
  (`canary/hardware/tools/dsn_export.py` on the committed board, canary `1a7df77`) to
  an SES byte-identical to Java `7f56b062d`, using `-mp 1 --threads 2
  --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150`.
  KiCad grades it 45 unconnected.
- **The plugin run is worse.** Dan's plugin run (native KiCad export plus
  `rule_area_fixup`, unlimited passes, 2 threads, optimizer on) grades
  **68**, with 31 dangling vias and 2 `+5V_PI` tracks on `Power.Cu`.
- **The input is the difference, not the router.** The plugin's export:
  - keeps the full class widths for the power taps (1.5 mm `Power_High` and
    `Switch_Node`, 0.5 mm `Power`), which can't reach 0.25 mm QFN pins;
  - passes the outer pours as planes that other nets may cross;
  - leaves out the board-edge and fiducial clearances.

This checklist gives the plugin the transforms of `dsn_export.py`, each as an option,
so a plugin run can reproduce the graded flow (45 at pass-1 parity, 40 with the
add-only second run, 37 since canary-7a's re-run of 2026-10-17).

Prerequisites: K01 (`rule_area_fixup`, the DSN fix-up framework), K02 (the routing
dialog and options), K03 (options file schema), Q01 (`plane_obstacle_net_classes`),
Q09 (`plane_obstacle_block`, `wire_keepout`), Q10 (add-only sweep, for K09.6), Q04
(stop rule, for K09.7).

## What exists

| Exists | State |
|---|---|
| `kicad-plugin/freerouting_rs/rule_areas.py` | The K01.2 fix-up: footprint-only rule areas no longer become full keepouts (`dsn_export.py` step 1) |
| `options.py` | `max_passes`, `threads`, `optimizer`, `improvement_threshold`, `ignore_net_classes`, `seed`, `min_trace_width_um`, `timeout`, `rule_area_fixup`; per board in `freerouting-rs/route-options.json` |
| `canary/hardware/tools/dsn_export.py` (Dan's canary repo, `dbfe7b67`) | Steps 2–6 below, written for `canary-pwr`; graded 45 and 40 there |
| Engine | Q01's `router.plane_obstacle_net_classes` (pours of listed classes block pass-through nets inside the engine); Q09's `router.plane_obstacle_block = traces` and `router.wire_keepout = traces_only` |

## Decisions made here

- **Prefer an engine setting where one gives the same result.** Where it doesn't,
  rewrite the DSN in the plugin, as `dsn_export.py` does.
  - Step 2 (fixed-class pours as fixed wires) and Q01's in-engine pour policy aim
    at the same effect. K09.2 measures whether they grade the same on
    `canary-pwr`.
  - If they do, the option maps onto Q01's setting.
  - If not, the plugin writes the fixed wires.
- **Every option is off by default**, so a plugin run with the defaults is unchanged.
- **A "canary flow" preset** in the dialog sets the options to the values canary
  grades with. The preset only fills the fields, so each one can still be changed.
- **Provenance:** the transforms are ported from Dan's own canary repo, which is
  recorded in the module docs. No third-party code is involved.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K09.1–K09.5 | `kicad-plugin/freerouting_rs/dsn_fixups.py`, `kicad-plugin/tests/test_dsn_fixups.py` | `options.py` (new fields, schema bump), `plugin.py` (calls the fix-ups after `rule_area_fixup`), the options dialog |
| K09.6–K09.7 | — | `options.py`, the dialog, `routing.py` (CLI arguments) |
| K09.8 | — | `docs/development/kicad-plugin.md`, `fixtures/canary-pwr/README.md` |

## Checklist

- [x] **K09.1 — Per-class routed width.** Option `class_widths: {CLASS: mm}`, written
  into the DSN's `(class ... (rule (width W)))` as `dsn_export.py --width` does.
  - **Accept:** a unit test rewrites one class's width and leaves the others byte
    for byte; the options file round-trips.

  *Done 2026-10-17:* `kicad-plugin/freerouting_rs/dsn_fixups.py` (step 4, provenance in its
  docstring: canary `hardware/tools/dsn_export.py`, Dan's repo, `dbfe7b67`); dialog field "Class
  widths (mm)" (`CLASS=mm, ...` or `auto`). `test_dsn_fixups.py::test_one_class_width_is_rewritten_and_the_rest_stays_byte_for_byte`
  (the text before and after the one `(width ...)` atom is equal), `test_an_unknown_class_is_refused`;
  `test_options.py::test_the_k09_options_round_trip` (save, load, equal; `auto` too) and
  `test_class_widths_text_round_trips`. On the real board the preset's export equals
  `dsn_export.py --width Power_High=0.3 --width Switch_Node=0.3 --width Power=0.3 --width GND=0.4`
  (WSL KiCad 10.0.7, canary HEAD board) except the K01.2 `place_keepout` (the script drops the area)
  and one blank line (`diff` of the two DSNs).
- [x] **K09.2 — Outer pours of listed classes keep other nets out.** Option
  `fixed_pour_classes`. Measure both mappings on `canary-pwr` and keep the one that
  grades better:
  - (a) Q01's `router.plane_obstacle_net_classes`;
  - (b) `dsn_export.py`'s fixed wires covering the fill.
  - **Accept:**
    - both arms are graded by KiCad, and the chosen mapping and its numbers are in
      the done note;
    - no foreign-net track crosses a listed pour (`inner_tracks.py`-style check on
      the outer layers).

  *Done 2026-10-17:* option `fixed_pour_classes` (list or `auto`) with `fixed_pour_mode = wires |
  engine`; `wires` (b) is the default. Registered arms `preset-wires` (b) and `preset-engine` (a) of
  `docs/roadmap/prereg/K09.json` (the canary preset otherwise), headless plugin path
  (`kicad-plugin/headless_run.py`: the plugin's export, fix-ups, flags and `ImportSpecctraSES`; WSL
  KiCad 10.0.7, release CLI of this tree), graded by `kicad-cli pcb drc --schematic-parity
  --refill-zones --severity-all` on the saved board, canary's `via_in_pad.py`/`inner_tracks.py`, and
  the new `kicad-plugin/pour_crossings.py` (foreign-net tracks on F.Cu/B.Cu against the input's
  Power_High/Switch_Node fills), round 2 (after amendment 2):

  | arm | unconnected | pour crossings | edge grazes | inner tracks | other errors | s |
  |---|---|---|---|---|---|---|
  | (a) engine | 36 | 23 | 4 (max 0.121 mm) | 5 | 8 starved thermals | 77 |
  | (b) wires | 38 | **0** | 3 (max 0.061 mm) | 2 (Q13) | clearance 4 (Q14), 7 starved | 109 |
  | `legacy` (no pour policy) | 76 | 52 | 18 (max 0.232 mm) | 2 | 1 starved | 245 |

  Kept: (b). (a) grades 2 better, inside canary's ±10 noise, but lets 23 foreign track segments
  through the pours, which the second accept clause forbids; (b) has none. A graze is copper whose
  centreline stays outside the fill and whose edge reaches over it by at most 0.061 mm (the 0.25 mm
  strips of `fill_rects` leave slivers at slanted edges); the refill only pulls the pour edge back.
- [x] **K09.3 — Drop the outer pours of listed nets.** Option `drop_pour_nets`
  (canary: `GND`), so pads fan out to the inner plane instead of counting as joined
  through an outer pour that routing breaks up.
  - **Accept:** a unit test drops exactly the outer-layer `(plane ...)` blocks of
    the listed nets.

  *Done 2026-10-17:* `dsn_fixups.py` step 3, dialog field "Drop pours of nets" (list or `auto`: a net
  alone on an inner copper layer). `test_dsn_fixups.py::test_drop_pour_nets_drops_exactly_the_outer_planes_of_the_listed_nets`
  (four-layer fixture: GND's F.Cu and B.Cu planes go, its In1.Cu and In2.Cu planes and +5V's stay;
  the output equals the input with exactly those two blocks cut), `test_auto_lists_come_from_the_board`.
- [x] **K09.4 — Board-edge and pad clearances as keepouts.** Option
  `edge_and_pad_keepouts`: strips along the outline for the board-edge clearance,
  and circles for pads whose own clearance exceeds the default (fiducials). They are
  inserted after the layer definitions.
  - **Accept:**
    - a unit test on a two-layer fixture;
    - KiCad's `copper_edge_clearance` count on `canary-pwr` is 0 with the option on.

  *Done 2026-10-17:* `dsn_fixups.py` step 5; on by default under both profiles (a board rule, D8 in
  spirit; the orchestrator's K09.9 brief). dsn_export.py's bounding-box strips, plus a rectangle
  along each outline segment off the box (amendment 2: `CM5_MINIMA_3`'s legacy run had 12 edge
  errors more than its input along Edge.Cuts segments the box misses). Tests:
  `test_dsn_fixups.py::test_edge_and_pad_keepouts_on_a_two_layer_board` (exact strips and FID
  circle, inserted before the structure's `(via`, after the layers), `test_outline_segments_off_the_bounding_box_get_their_own_keepouts`,
  `test_no_edge_strips_when_the_edge_clearance_is_within_the_default`, `test_crlf_files_get_crlf_insertions`;
  `test_cli_integration.py::test_the_k09_flags_and_a_fixed_up_dsn_reach_the_real_cli` (the CLI loads
  and routes a DSN with every fix-up, no warning). `canary-pwr`: `copper_edge_clearance` **0** in every
  round-2 arm with the option on (legacy, preset-wires, preset-nosweep, enh-auto-*, add-only), against
  2 in `plugin-0` (option off; Dan's run had the same 2 GND vias at 0.297 mm).
- [x] **K09.5 — Open inner plane layers to routing.** Option `route_on_plane_layers`
  (DSN layer names): `(type power)` becomes `(type signal)`. It can be combined with
  Q09's `plane_obstacle_block = traces` so the islands keep traces out.
  - **Accept:** a unit test; a `canary-pwr` run with `Power.Cu` opened puts no
    `+5V_PI` or `+12V` track through a foreign island (Q09.5's `foreign_island_mm`).

  *Done 2026-10-17:* `dsn_fixups.py` step 6, dialog field "Route on plane layers".
  `test_dsn_fixups.py::test_route_on_plane_layers_opens_only_the_named_layer` (In2.Cu opened, In1.Cu
  stays power, an unknown or non-power layer is refused). Registered arm `preset-in2-open` (the
  preset without the sweep, `route_on_plane_layers = Power.Cu`, plus
  `--router.plane_obstacle_net_classes=Power_High --router.plane_obstacle_block=traces`), round 2:
  36 unconnected, 146 tracks on Power.Cu (the layer is open, so `inner_tracks.py` counts them), 6
  clearance errors (vias of 0.25 mm classes, Q14), 41 s. `foreign_island_mm` (5 um sampling of the
  saved board's Power.Cu tracks against the other nets' zone outlines on Power.Cu, GND excluded):
  **0.00 mm** for `+5V_PI`, `+12V_BUS`, `+12V_PROT`, `+12V_RAW`, `+12V_HAT`; 30.95 mm for other nets
  (`/Holdup/VCAP` 17.40, `/Pi_Interface/PI_3V3` 9.84, `/Holdup/LTC_VOUTSP` 3.71), which Q01 exempts
  when they have a pin inside the island (as in Q09.5's dry run).
- [x] **K09.6 — Add-only second run.** Option `fix_existing`: export every track and
  via as fixed, as `dsn_export.py --fix-existing` does, so a run only adds. If Q10's
  in-engine sweep is accepted first, the option offers it instead and the done note
  says so.
  - **Accept:** on `canary-pwr`, plugin run then second run grades ≤ 37, or the done
    note gives the number and the cause.

  *Done 2026-10-17:* option `fix_existing` (dialog "Keep the existing tracks (add-only run)"): the plugin
  locks every unlocked track and via before KiCad's export, so they are written as fixed copper,
  keeps them locked through `ImportSpecctraSES` and restores the lock states afterwards
  (`export_flow.prepare`, `kicad_board.lock_tracks`/`restore_locks`); a locked item stays locked.
  Q10's sweep is not in `enhanced` (Q10 keeps it opt-in), so the option offers the add-only run and
  the sweep separately (`sweep`, `sweep_passes`). Tests: `test_route_dialog.py::test_fix_existing_keeps_the_tracks_locked_through_the_import_only`,
  `test_options.py::test_the_k09_options_round_trip`. `canary-pwr` (headless plugin path, round 2):
  preset-nosweep's saved board (45) routed again with the canary preset, `fix_existing` on and
  `-mp 2` (Java's add-only flags): **42** unconnected, clearance 4 (Q14), inner tracks 3 (Q13),
  router 127 → 134 unrouted, 304 s (`opts/addonly.json`, arm `addonly` in
  `target/canary-handoff/2026-10-17-k09/`). Cause of the gap to 37: the first run's board, not the
  add-only step. The same plugin add-only run started from Java `7f56b062d`'s pass-1 board
  (`2026-10-17-addonly37/canary-pwr-1a7df77-pass1-45.kicad_pcb`, diagnostic `diag-addonly-from-java45`)
  grades **37**, Java's add-only number, with 0 inner tracks and no clearance error, in 69 s. The
  plugin's own pass-1 board is another 45 (K09.8's DSN differences route differently), and 42 is
  within canary's ±10 of 37.
- [~] **K09.7 — Stop wasted passes.** On `canary-pwr`, pass 1 is best (94
  router-unrouted). Passes 2–18 each end at 139–181 and the best board is restored,
  so about 190 of 210 s are wasted (Java does the same). The plugin offers Q04's stop
  rule once Q04 lands. Until then it offers a per-board pass-limit hint: the options
  file remembers the pass that won last time.
  - **Accept:** the option exists; with it on, the `canary-pwr` run is at least 3×
    faster and grades the same.

  *Partial 2026-10-17:* the option exists: `use_pass_hint` (dialog "Stop at the pass that won last
  time (N)"); the plugin records the router's `pass_end` events of the route phase
  (`progress.ProgressState.best_pass`), saves the pass with the fewest unrouted as `pass_hint` in
  `route-options.json`, and with the box ticked passes `--max-passes <hint>` (Q04's stop rule has
  not landed). Tests: `test_route_dialog.py::test_the_winning_pass_is_remembered_and_offered`,
  `test_options.py::test_each_k09_option_has_its_golden_argument_list`. `canary-pwr`, arm
  `pass-hint` (legacy options plus the hint 1 that the legacy run saved): **76** unconnected, the
  SES byte-identical to `legacy`'s (same grade, same DRC), **86.2 s** against 244.7 s = **2.84×**,
  short of 3×. Cause: the fanout's 20 passes take 65 s before route pass 1 (which ends at 87.2 s in
  the legacy log); the hint removes everything after pass 1 (route passes 2–18, 157 s, best board
  restored), so no pass limit or stop rule can exceed 245 / 87 = 2.8× on this board. Both runs
  under load (average about 11 during legacy, lower during pass-hint), so the absolute times are
  inflated. Remainder: the fanout's cost, which no checklist owns (operator decision whether 2.84×
  meets the intent), and Q04's stop rule in place of the hint once it lands.
- [x] **K09.8 — The canary preset reproduces the graded flow.**
  - The preset: `class_widths` from canary's run, `fixed_pour_classes =
    Power_High,Switch_Node`, `drop_pour_nets = GND`, edge and pad keepouts on,
    `-mp 1`, `start_ripup_costs = 1500`, `min_trace_width_um = 150`.
  - **Accept:**
    - a headless plugin run (pcbnew from WSL KiCad 10.0.7) on `canary-pwr` grades
      45 unconnected (33 with Q10's `sweep=free`), or within the differences the done note explains against
      `dsn_export.py`'s DSN (`ses-diff` against the 97d10d6 / Java 7f56b062d
      session);
    - `kicad-plugin.md` documents the options and the preset.

  *Moved 2026-10-17 (K10 session; Dan: no board-specific code in the tool):* the preset is no
  longer product code. `options.CANARY_PRESET`, `with_preset` and the "Canary flow" button are gone;
  the values are the preset file `fixtures/canary-pwr/presets/canary-graded-flow.json`, which the
  dialog's Preset drop-down lists once it is copied into the board's `freerouting-rs/presets/`
  folder (`presets.py`; the plugin ships no preset). Tests now:
  `test_presets.py::test_the_canary_fixture_preset_gives_the_old_preset_values_and_arguments`,
  `test_route_dialog.py::test_a_board_preset_fills_the_fields_and_reaches_the_router`,
  `test_dsn_fixups.py::test_the_canary_preset_file_reproduces_dsn_export_on_the_fixture`.

  *Done 2026-10-17:* `options.CANARY_PRESET` (now the preset file above), the dialog's "Canary flow" button (now Preset > Apply; fills the fields,
  each still editable; with the sweep `free` x2 of Q10). Test:
  `test_route_dialog.py::test_the_canary_preset_fills_the_fields_and_reaches_the_router`;
  `test_dsn_fixups.py::test_the_preset_reproduces_dsn_export_on_the_fixture` (both renamed above). Headless plugin runs
  on `canary-pwr` (canary HEAD `e21ee05e`, WSL KiCad 10.0.7, round 2):
  - without the sweep (arm `preset-nosweep`): **45** unconnected, as Java `7f56b062d` / `97d10d6`;
    clearance 4 (vias of the 0.3 mm classes, Q14), 2 inner tracks (Q13), 93 s. `freerouting-dev
    ses-diff fixtures/canary-pwr/canary-pwr-9511d9a.dsn` against `2026-10-17-q10/mp1.ses`: 58 nets
    differ, router incomplete 109 → 112, 40 → 40 clearance violations (all input), 157 → 155 vias;
  - with the sweep (arm `preset-wires`): **38** (clearance 4, inner 2, 109 s); `ses-diff` against
    `2026-10-17-q10/sweep-free.ses` (the 33): 62 nets differ.

  The differences against `dsn_export.py`'s DSN, measured, not inferred: `dsn_export.py` run on the
  same board with the same KiCad (10.0.7, `scratch/dsncmp/script.dsn`), routed with the preset's
  flags and imported by the plugin, grades **33** (0 inner tracks, no clearance error, 70 s), so
  KiCad's version and the import are not the cause. The plugin's DSN has two things more: the 120
  rectangles along the off-bbox outline segments (K09.4, amendment 2) and the footprint rule area as
  a `place_keepout` (K01.2; `dsn_export.py` drops it). Without the rectangles it grades 36 (inner 2,
  clearance 3); the place keepout accounts for the rest. Neither restricts routing on this board
  (`script.dsn` also has 0 `copper_edge_clearance`; a place keepout is a component obstacle, Java's
  `insertComponentObstacle`), but each adds items to the board; that this reorders the search is
  inferred, not traced, and the routing is chaotic at canary's ±10. `kicad-plugin.md` "DSN fix-ups and the canary flow (K09)"
  documents the options, the preset, the headless runs and these numbers.
- [~] **K09.9 — A plain plugin run gets the graded flow under `enhanced`.** canary-7a graded
  Dan's plugin run at `9c97117` (profile `enhanced`, default options) at 68 unconnected, with
  2 `copper_edge_clearance` errors (GND vias 0.297 mm from the edge against 0.3) and 2
  inner-layer tracks (Q13), while the CLI flow grades 33 (Q10). The plugin got none of what
  produced 33:
  - KiCad's raw export (20 planes against 4, 1.5 mm and 0.5 mm class widths, 110 fixed
    items against 220, no edge clearance);
  - profile defaults (ripup cost 100 against 1500, unlimited passes against `-mp 1`, no
    minimum width);
  - no sweep (Q10 isn't in `enhanced` yet).

  With the measurements of K09.2–K09.7, Q10 and Q04 in hand, decide which fix-ups and
  settings `enhanced` turns on in the plugin by default (N02's rule, N05's procedure), so
  that a run with default options reproduces the graded flow. The board's copper-to-edge
  clearance is always honoured: it is a board rule, not a tuning option (K09.4 makes it a
  keepout; with K10's direct read the router takes it from the board).
  - **Accept:**
    - a default-options plugin run on `canary-pwr` grades ≤ 33 with no edge-clearance or
      inner-layer errors, or the done note gives the number and the cause;
    - `legacy` keeps the plugin's current behaviour.

  *Partial 2026-10-17:* registered first as `docs/roadmap/prereg/K09.json` (pinned by
  `crates/freerouting-dev/tests/prereg_k09.rs`; two amendments before round 2, see the file), N02's
  totals rule computed by hand, headless plugin path on four boards, round 2. Mechanism:
  `options.PROFILE_DEFAULTS` holds, per profile, what the plugin's defaults change from the dataclass
  defaults; a new board, and a field an older options file lacks, start from it. `ENHANCED` in
  `settings/profile.rs` is untouched. KiCad-unconnected (noise: canary 10, interf_u 8.2,
  pic_programmer 4.3, CM5_MINIMA_3 10.4; total noise 17.14):

  | arm | canary-pwr | interf_u | pic_programmer | CM5_MINIMA_3 | total | Δ vs legacy | worst regression (2× noise) | guards on canary-pwr | wall s |
  |---|---|---|---|---|---|---|---|---|---|
  | `legacy` (baseline) | 76 | 10 | 2 | 21 | 109 | | | inner 2, drc_real 0, edge 0 | 406 |
  | `enh-auto-mp1` (candidate) | 36 | 18 | 3 | 15 | 72 | −37 | interf_u +8 (16.4) | **inner 2 (Q13)**, drc_real 0, edge 0 | 759 |
  | `enh-auto-mpdef` | 36 | 13 | 3 | 11 | 63 | −46 | interf_u +3 | **inner 2 (Q13)**, drc_real 0, edge 0 | 1837 |
  | `enh-auto-nowidth-mp1` | 51 | 18 | 2 | 15 | 86 | −23 | interf_u +8 | inner 0, **drc_real 3** (clearance), edge 0 | 551 |

  `via_in_pad` 0 and no `track_width` error (below_minimum) in every run; drc_real elsewhere:
  pic_programmer 2 clearance errors in every arm including legacy, CM5_MINIMA_3 legacy 12 (11
  clearance, 1 short), candidates 3; copper_edge_clearance on CM5_MINIMA_3 2 = its input's 2.
  Every candidate clears the totals rule (Δ beyond 17.14, no board beyond twice its noise) but
  none clears the hard guards: the `auto` arms leave 2 `+5V_PI` segments on `Power.Cu` (Q13, the
  plane-island bug, also in `legacy`), and without the width cap 3 track-to-track clearance errors
  appear (`+5V_PI` against `/Supervisor/BUTTON` on F.Cu, 0.2437 mm against Power_High's 0.25 mm).
  So under the keep rule `PROFILE_DEFAULTS["enhanced"]` stays `{}`, and a default-options plugin
  run on `canary-pwr` is `legacy`'s: **76** unconnected, **0** copper_edge_clearance (K09.4's
  keepouts, on under both profiles), **2** inner-layer tracks (Q13), 1 starved thermal. Not ≤ 33:
  the cause is that the settings that reach 36 are not accepted while Q13 is open, and 36 is the
  best any arm reached on this export (the 33 SES is reproduced byte for byte by this tree from
  canary's committed DSN; K09.8 gives the DSN differences). `legacy` keeps the plugin's behaviour
  apart from the keepouts (D8: a board rule) and the via drill fix (DIVERGENCES.md, kicad-plugin).
  Round 1 (before amendment 2, not judged): CM5_MINIMA_3 legacy 23 with 14 edge errors, mp1 10,
  nowidth 10, mpdef 8. Remainder: rerun `enh-auto-mp1`/`enh-auto-mpdef` once Q13 lands (the keep
  rule then picks `enh-auto-mpdef`, 63, unless its wall time is ruled out), and canary-7a's grade
  of the handoff sessions in `target/canary-handoff/2026-10-17-k09/`.

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

- [ ] **K09.1 — Per-class routed width.** Option `class_widths: {CLASS: mm}`, written
  into the DSN's `(class ... (rule (width W)))` as `dsn_export.py --width` does.
  - **Accept:** a unit test rewrites one class's width and leaves the others byte
    for byte; the options file round-trips.
- [ ] **K09.2 — Outer pours of listed classes keep other nets out.** Option
  `fixed_pour_classes`. Measure both mappings on `canary-pwr` and keep the one that
  grades better:
  - (a) Q01's `router.plane_obstacle_net_classes`;
  - (b) `dsn_export.py`'s fixed wires covering the fill.
  - **Accept:**
    - both arms are graded by KiCad, and the chosen mapping and its numbers are in
      the done note;
    - no foreign-net track crosses a listed pour (`inner_tracks.py`-style check on
      the outer layers).
- [ ] **K09.3 — Drop the outer pours of listed nets.** Option `drop_pour_nets`
  (canary: `GND`), so pads fan out to the inner plane instead of counting as joined
  through an outer pour that routing breaks up.
  - **Accept:** a unit test drops exactly the outer-layer `(plane ...)` blocks of
    the listed nets.
- [ ] **K09.4 — Board-edge and pad clearances as keepouts.** Option
  `edge_and_pad_keepouts`: strips along the outline for the board-edge clearance,
  and circles for pads whose own clearance exceeds the default (fiducials). They are
  inserted after the layer definitions.
  - **Accept:**
    - a unit test on a two-layer fixture;
    - KiCad's `copper_edge_clearance` count on `canary-pwr` is 0 with the option on.
- [ ] **K09.5 — Open inner plane layers to routing.** Option `route_on_plane_layers`
  (DSN layer names): `(type power)` becomes `(type signal)`. It can be combined with
  Q09's `plane_obstacle_block = traces` so the islands keep traces out.
  - **Accept:** a unit test; a `canary-pwr` run with `Power.Cu` opened puts no
    `+5V_PI` or `+12V` track through a foreign island (Q09.5's `foreign_island_mm`).
- [ ] **K09.6 — Add-only second run.** Option `fix_existing`: export every track and
  via as fixed, as `dsn_export.py --fix-existing` does, so a run only adds. If Q10's
  in-engine sweep is accepted first, the option offers it instead and the done note
  says so.
  - **Accept:** on `canary-pwr`, plugin run then second run grades ≤ 37, or the done
    note gives the number and the cause.
- [ ] **K09.7 — Stop wasted passes.** On `canary-pwr`, pass 1 is best (94
  router-unrouted). Passes 2–18 each end at 139–181 and the best board is restored,
  so about 190 of 210 s are wasted (Java does the same). The plugin offers Q04's stop
  rule once Q04 lands. Until then it offers a per-board pass-limit hint: the options
  file remembers the pass that won last time.
  - **Accept:** the option exists; with it on, the `canary-pwr` run is at least 3×
    faster and grades the same.
- [ ] **K09.8 — The canary preset reproduces the graded flow.**
  - The preset: `class_widths` from canary's run, `fixed_pour_classes =
    Power_High,Switch_Node`, `drop_pour_nets = GND`, edge and pad keepouts on,
    `-mp 1`, `start_ripup_costs = 1500`, `min_trace_width_um = 150`.
  - **Accept:**
    - a headless plugin run (pcbnew from WSL KiCad 10.0.7) on `canary-pwr` grades
      45 unconnected (33 with Q10's `sweep=free`), or within the differences the done note explains against
      `dsn_export.py`'s DSN (`ses-diff` against the 97d10d6 / Java 7f56b062d
      session);
    - `kicad-plugin.md` documents the options and the preset.
- [ ] **K09.9 — A plain plugin run gets the graded flow under `enhanced`.** canary-7a graded
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

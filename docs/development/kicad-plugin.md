# KiCad plugin: handover notes

Source: [`kicad-plugin/`](../../kicad-plugin/README.md). It is a SWIG `pcbnew.ActionPlugin`
(KiCad 10.0 still ships the SWIG bindings) that adds a toolbar button to the PCB editor.

## How it works

- `freerouting_rs/plugin.py`: the dialog. **Route** saves the options, exports the board with
  `pcbnew.ExportSpecctraDSN`, runs `freerouting-rs -de board.dsn -do board.ses --progress json
  --result-json board.result.json <options>` and imports the SES with `pcbnew.ImportSpecctraSES`
  (K02). A progress dialog shows the phase, pass and routed/failed/incomplete counts live, the
  router's log lines, **Stop and keep result** and **Abort**; a summary follows the import.
  **Route in GUI** runs `freerouting-gui -de board.dsn -do board.ses <options>` and imports the
  session the window writes. **Open in viewer** exports and launches `freerouting-gui board.dsn
  [board.ses]`. **Import SES...** imports any session file. Files go in a `freerouting-rs/` folder
  next to the `.kicad_pcb`.
- **The dialog's layout.** The board's paths, the **Presets** row and the buttons stay in view; the
  options sit on three tabs: **Route** (profile, passes, threads, optimizer, net classes, time
  limit, seed, minimum width), **Board and DSN** (board input, `.kicad_dru` switch, rule-area fix,
  the K09 fix-ups, which work on either input since K14) and **Advanced** (the K09 flow settings and the N01 stage choices). The
  tabs open tall enough for the longest one, so none scrolls where the screen has the room (owner,
  2026-10-18). On a smaller screen the dialog is cut to 90 % of the display's work area
  (`layout.py`, `Dialog.fit_to_display`) and the pages scroll; before the tabs, the stacked groups
  outgrew the screen and hid **Route in GUI** and the other buttons.
- K02 modules (all pure Python except `plugin.py`): `options.py` (the route options, per board in
  `freerouting-rs/route-options.json`, `"schema": 6` since K13.7, defensive restore; `to_args()` builds the
  flags in a fixed order and always passes `--router.min_trace_width_um` from Board Setup's
  minimum track width, D8), `progress.py` (`parse_line` for C08's JSON lines, `ProgressState`,
  `pump_lines` = the reader thread's body), `summary.py` (the text after the import, from the
  `--result-json` manifest), `routing.py` (`RouteRun`: process, reader thread, queue, `stop`,
  `abort`; `GuiRun`: the front-end GUI process, `should_import`).
- **Stop keeps, Abort discards.** Stop writes `stop\n` to the router's stdin (the router ends the
  step it is in, writes the best board, exits 0). Abort kills the process; the CLI writes outputs
  to a temporary file and renames it, so an aborted run leaves no `.ses`. Only C08's JSON lines
  are parsed; any other line is shown in the log pane as it is. pcbnew and wx are called on the
  UI thread only: the reader thread fills a `queue.Queue`, a `wx.Timer` in the dialog drains it.
- `freerouting_rs/tools.py`: finds the executables (no pcbnew/wx imports). Order: `bin_dir`
  in `settings.json`, the binaries bundled in a PCM install (`bin/<os>-<arch>/`, K03.3),
  `FREEROUTING_RS_BIN`, PATH, `~/.cargo/bin`, then on Windows
  `wsl_bin_dir` (a Linux build run through `wsl.exe`, paths translated to `/mnt/c/...`).
  Routing runs in a thread behind a cancellable progress dialog.
- `freerouting_rs/rule_areas.py` + `kicad_areas.py` (K01.2): after `ExportSpecctraDSN`, the
  plugin reads the board's rule areas with pcbnew (UI thread) and rewrites the exported DSN
  text. KiCad picks the keepout type from the tracks/vias flags only, so a footprints-only
  area (the `canary-pwr` fan inlet) becomes a full `keepout`. The rewrite turns it into
  `place_keepout`, drops pads/pour-only areas, and leaves the rest byte for byte (mapping table
  in the `rule_areas.py` docstring). On by default, switch in the dialog (saved per board in
  `freerouting-rs/plugin.json`), one log line per area in `freerouting-rs/plugin.log`.
  Footprint-owned areas (written into the `(image ...)`) are only listed in a warning.
- `freerouting_rs/version.py` (K01.3): `__version__` (equals the workspace version) and the
  check against `freerouting-rs --version` (`tools.query_version`, 10 s timeout). A major.minor
  mismatch disables Route; an unparsable answer is "unknown" (allowed, with a warning). The
  CLI has no `--version` until C08, so today every router shows "unknown".
- `install.py`: copies `freerouting_rs/` into KiCad's user `scripting/plugins` folder and
  writes `settings.json` there. It first moves any other copy of this plugin (PCM install in
  `3rdparty/plugins`, or a folder with our identifier) to `<KiCad user dir>/disabled_pcm_plugins/<version>/`
  (`--keep-pcm` refuses instead). The Java plugin (`app.freerouting.kicad-plugin`) is never
  touched. `OUR_IDENTIFIERS` is the PCM identifier `com.github.danbrunski.freerouting-rs` (K03.1).
- Route in GUI needs the GUI's front-end mode (G08.4): the window counts down 5 s, routes, and
  writes `-do` when routing ends or the window closes. A stop in the window (Cancel, Esc, a click)
  writes `-do` but leaves the window open, so `GuiRun.finished` stays false and the plugin keeps
  waiting until the window closes or the user presses Terminate; a re-route rewrites `-do` when it
  ends, and a close writes it again if the board changed. `should_import` reads the file only
  after the process exits. The router flags (`--max-passes`,
  `--threads`, `--router.*`, ...) are the CLI's (K02.5, `settings::cli_flags`); `--seed` is not
  passed to the GUI.

## Routing profile and Advanced stages (N01)

**Shared command-line board flow (K12).** `headless_run.py route BOARD WORK
OPTIONS.json ROUTER` uses `export_flow` and `routing.RouteRun`, the same functions
as the editor's Route button. Both DSN and snapshot inputs refill live zones
before export; failed filling stops preparation. This closes the stale-fill
difference from CANary's `autoroute.sh`, which refills before export. Neither
preparation path saves the source board. SES import corrects drills and refills
zones in both frontends. `export_flow.router_args` reads the current Board Setup
minimum at invocation, rather than the minimum cached when the dialog opened.
Each export writes `<stem>.args.json` with `args`, `log`, `kicad` and
`input_sha256`. Compare prepared geometry and these arguments before comparing
completion counts, and hold the router binary, configuration/environment and
KiCad version fixed. The historical external `canary/hardware/tools/dsn_export.py`
is not this entry point: it omits off-bounding-box outline strips and drops the
footprint-only fan rule area that the plugin turns into `place_keepout`. Those
objects remain in the shared flow. No engine profile or routing-quality
promotion is made here; historical 33-versus-39 counts are not an accepted A/B.

- **Profile** (a choice in the dialog, `RouteOptions.profile`): **Enhanced** (the default, no
  flag) routes with the accepted improvements over Java; **Legacy** passes `--profile legacy`
  for results identical to Java Freerouting. Until N05 accepts the first improvement the two route
  the same. The summary after the import starts with `Profile: Legacy` or `Profile: Enhanced`
  (the manifest's `settings_snapshot.profile` when the router wrote one, else the profile the
  plugin asked for).
  Under Enhanced the router checks its routed copper against the board's rules before saving
  (`router.output_check=repair`, Q20); `summary.output_check_lines` adds a **Rule check** line
  from the manifest's `output_check` (breaches found per rule, removed, retried, repaired, left
  open), a line for the board's own violations (`input`, never changed) and a warning when a
  `report` run saved breaches (`test_summary.py::test_the_output_check_is_summarised`).
- **Advanced** (the Advanced tab's Stages group): one choice per stage slot that takes one stage, read
  from the router with `freerouting-rs --list-stages=json --profile <p>` for each profile when the
  dialog opens (`tools.query_stages`, `options.parse_stage_listing`). Each choice starts at the
  saved one or the profile's; one that differs passes `--router.<key>=<id>` (sorted by key). A
  router without `--list-stages` gives an empty group with a note. Slots that take a list
  (`special.stages`, `special.diagnostics`) are not offered; the CLI sets them.
- **The options file** (`route-options.json`, `"schema": 6`; schemas 1 to 5 still load) stores the profile and only what
  differs from it: options at their defaults and stage choices equal to the profile's are left
  out, so a board follows the profile until the user picks something else. A schema 1 file
  (every option written out) still loads, as Enhanced. An unknown profile or a malformed stage
  key or id is dropped on load and refused by the form.
- Tests: `test_options.py` (both profiles' flags, stage flags in key order, the listing parser,
  `test_the_options_file_stores_only_the_differences`, schema 1), `test_route_dialog.py` (profile
  and stages reach the fake router and are restored, the summary names each profile, a router with
  no listing), `test_summary.py`, and against the real CLI `test_cli_integration.py`
  (`test_the_stage_listing_has_every_registered_id`,
  `test_both_profiles_route_and_the_summary_names_them`). The tabbed layout was checked in KiCad
  10.0's `python.exe` and wxWidgets 3.3.2 on the dev machine (the real dialog with a stub `pcbnew`,
  dark and light, full height and cut to 380 px); not yet inside KiCad's editor (owner, with K02.8).

## DSN fix-ups and the canary flow (K09)

KiCad's export is not what canary's graded flow routed: `hardware/tools/dsn_export.py` (the owner's canary
repo) rewrote it. The plugin now does the same transforms itself, each an option of the dialog's "Fix-ups"
group (Board and DSN tab) or "Flow" group (Advanced tab), saved in `route-options.json` (schema 3; schema 1 and 2 files still load, a
field they lack takes the profile's default). The four that rewrite the DSN text work on the board snapshot
too, applied by the router's reader (K14); "The K09 fix-ups on each input" below says how:

| option | what it does | default |
|---|---|---|
| `class_widths` | `{class: mm}` written into the class's `(rule (width W))`; `auto`: a class wider than the board default is routed no wider than its narrowest SMD pad, not below the board minimum | none |
| `fixed_pour_classes`, `fixed_pour_mode` | outer pours of these classes become fixed copper (`wires`: fixed paths covering the fill, its plane removed) or are kept clear by the router (`engine`: Q01's `router.plane_obstacle_net_classes`); `auto`: pours above their layer's lowest zone priority | none, `wires` |
| `drop_pour_nets` | the outer pours of these nets are dropped, so their pads fan out to the inner plane; `auto`: a net alone on an inner layer | none |
| `edge_and_pad_keepouts` | the board's copper-to-edge clearance (strips along the outline's bounding box and along every outline segment off it) and pads' own larger clearances (circles) as keepouts | **on**, both profiles |
| `route_on_plane_layers` | these inner plane layers become signal layers | none |
| `fix_existing` | every track and via is locked in memory before the export and through the import, so a run only adds (KiCad's import keeps locked copper); the lock states come back afterwards | off |
| `start_ripup_costs`, `sweep`, `sweep_passes` | `--router.scoring.start_ripup_costs`, Q10's `--router.autorouter.sweep` and `sweep_passes` | router defaults |
| `use_pass_hint`, `pass_hint` | after a run the plugin stores the routing pass that ended with the fewest incomplete connections; with the option on, the next run stops there (`--max-passes`) | off |
| `pin_escape` | Q21's checkbox "Escape fine-pitch pins straight (QFN, QFP, SOIC)" (Advanced tab, Flow group): `--router.fanout.pin_escape=straight`, a straight stub at the board minimum width out of each pin of a part with edge rows at ≤ 0.65 mm pitch before routing; route-options schema 5 | off, both profiles |
| `plane_obstacle_all_pours` | K15.14's checkbox "Keep other nets out of every pour" (Advanced tab, Flow group): `--router.plane_obstacle_all_pours=true`, every signal-layer pour is kept clear of other nets' copper (K15.13). A per-board option, not a default: K15.14's measurement found KiCad unconnected 368 -> 493 over 16 boards (it helps the pcu board, 47 -> 20, and leaves nets unrouted on boards with large pours, pcie-active-backplane 5 -> 83). No schema bump: a file without the key means off | off, both profiles |

- **Presets** (`presets.py`, dialog group "Presets": a drop-down, **Apply**, **Save as preset...**).
  A preset is a JSON file in the options format (`route-options.json`'s) with a `name` and an
  optional `description`; it may hold only the options it sets, and Apply changes just those fields
  (each can still be edited). They are listed from `<board dir>/freerouting-rs/presets/*.json` and
  from the user's folder if it exists (`$FREEROUTING_RS_PRESETS`, else
  `~/.config/freerouting-rs/presets`, `%APPDATA%\freerouting-rs\presets` on Windows; never
  created). A board preset hides a user preset of the same name. **Save as preset...** writes every
  option of the dialog to the board folder under the name typed. A file that is not JSON, has no
  `name` or has an unreadable `schema` is skipped with a WARNING in `plugin.log`; a bad value or an
  unknown key inside one is ignored and logged (the options file's rules). The plugin ships no
  preset, so it carries nothing board-specific. canary's graded flow (widths, fixed pours, GND
  dropped, keepouts, one pass, rip-up cost 1500, minimum width 150 um, sweep `free` x2) is
  `fixtures/canary-pwr/presets/canary-graded-flow.json`; copy it into the board's `presets/` folder
  to use it. Tests: `test_presets.py`, `test_route_dialog.py` (`test_a_board_preset_fills_...`,
  `test_save_as_preset_...`, `test_a_malformed_preset_file_...`).
- **After the import** the plugin sets every routed via's drill from the session padstack
  (`ses_vias.py`): KiCad 10's `ImportSpecctraSES` takes the drill from the net class, which gave
  0.6 mm vias a 0.4 mm drill and a 0.1 mm annular ring on `canary-pwr`. Then the copper that was
  on the board comes back at its own coordinates (`restore_copper.py`, Q22.3, K13.2): each imported
  track or via that matches an unlocked original (net, layer, width within 0.1 µm or via size and
  drill, every coordinate within 0.6 µm on the DSN path and 0.2 µm on the `kicad_pcb` path) gets the
  original's coordinates, width and uuid back, and routed ends within the bound of a restored end
  move onto it. KiCad's `%.6g` export had moved input copper by up to 0.6 µm (cm4-edge-ai: 25
  clearance items, nudac: 54 hole clearance items). The plugin's Route and Route in GUI,
  `headless_run.py import` and the oracle's `kicad_steps.py import` all call it; `plugin.log` gets one
  `input copper: ...` line. Tests: `test_restore_copper.py`,
  `tools/kicad-oracle/tests/test_import_restore.py` (real pcbnew).
- **Track arcs** (K13.8): KiCad 10.0.7's DSN export writes each track arc as one chord
  (CM5_MINIMA_3: 172 clearance items in K13.6, the chords of differential-pair arcs). After the
  export `export_flow.fix_track_arcs` (`track_arcs.py`, always on) replaces the chord of each
  unlocked arc in the wiring with the polyline the `.kicad_pcb` reader lays for it
  (`tracks.rs::arc_points`, ported to Python and identical on 289027 points of 2200 arcs);
  `plugin.log` gets a `track arcs: N of M unlocked arc(s) written as polylines` line. After the
  import `restore_copper` puts an original arc back as an arc where its polyline (from either
  input) came back unmoved, through `restore_copper.PcbnewEditor` (pcbnew passed in; without it no
  arc is put back), and the `input copper:` line ends with `K track arc(s) back as arcs`. Tests:
  `test_track_arcs.py`, `test_restore_copper.py` (the arc tests), `test_arc_round_trip.py`
  (real pcbnew and kicad-cli on `tests/data/arc_pair.kicad_pcb`, two arcs at exactly the class
  clearance).
- **Planes cut by zone priority** (K15.24): KiCad's export writes each copper zone's whole outline
  as its `(plane ...)` and no priority, but its filler leaves out every higher zone of another net
  (`ZONE::HigherPriority`) grown by the clearance. Right after the export,
  `export_flow.fix_plane_priorities` (always on, before every other fix-up, while the planes are
  still the export's) has `kicad_board.plane_cuts` subtract, with pcbnew's `SHAPE_POLY_SET`, the
  outlines of those zones (board and footprint zones; not their holes) grown by the larger of the
  two zones' own clearances (0.5 mm when unset, round corners, `m_MaxError` outside) from each
  plane, and `plane_priority.apply` writes each changed plane as one `(plane ...)` per piece left,
  holes as `(window ...)`; a plane with nothing left is dropped. Planes are matched to the board's
  zones by layer and net in export order; a mismatch logs a WARNING and cuts nothing. `plugin.log`
  gets one `plane NET on LAYER: cut where ...` line per cut plane, none without one, and then the
  DSN is byte-identical. The pcu board: 28 planes cut (In2.Cu's V_BUS_12V 11346 -> 8585 mm2 under
  V_BUS_24V), 0.11 s. Tests: `test_plane_priority.py` (a fake pcbnew doing rectangle
  arithmetic, and one test through KiCad's own pcbnew and DSN export).
- **Signal-layer zones' thickness and clearance** (K15.27): KiCad's DSN export drops them, and
  the router's cleanup judges a DSN signal-layer pour by KiCad's fill model (K15.26) only when it
  has them. `export_flow.pour_rule_args` (last in `prepare`, after the fix-ups; DSN input only)
  has `kicad_board.signal_pour_rules` take the smallest `GetMinThickness()` and zone clearance
  (`GetLocalClearance()`, 0.5 mm when unset, as `_clearance` takes it) over the board's copper
  zones that are no rule area and lie on a layer the exported DSN types `signal` or `jumper`
  (read back from the DSN, so a power layer opened by `route_on_plane_layers` counts, and a
  `power` or `mixed` layer does not), and passes `--router.cleanup.pour_min_thickness_um=<um>` and
  `--router.cleanup.pour_clearance_um=<um>`. No such zone: neither flag; the `.kicad_pcb` input:
  neither. `plugin.log` gets one `signal-layer zones: N on LAYERS; the cleanup judges their pours
  by min thickness T um and clearance C um (...)` line. Arguments after `--` in `headless_run.py`
  come later and win. Tests: `test_pour_rules.py` (stub pcbnew). K15.29: the same zones'
  `GetIslandRemovalMode()` give `--router.cleanup.pour_island_removal=true` (the cleanup drops
  the pours' islands no pad reaches, as KiCad's filler does) and
  `--router.cleanup.pour_islands_kept_nets=<net>,...`, the nets of the zones whose mode is not 0
  (always); a mode that cannot be read, or a kept net's name with a comma or surrounding spaces,
  passes neither. The log line then adds `, islands no pad reaches dropped (kept on N net(s))`.
  Tests: `test_pour_islands.py`. K15.32: the clearance flag is the smallest clearance KiCad's zone
  filler can apply to those zones, not the zones' alone: `export_flow.filler_clearance_nm` takes the
  minimum of the zone clearance and every `(constraint clearance (min ...))` of `<board>.kicad_dru`
  (`kicad_rules.py`, a stdlib S-expression reader; conditional rules count, other constraint types
  do not). Measured on KiCad 10.0.7 (`kicad-cli pcb drc --refill-zones` on K15.29's
  `b-no-bus-gnd`, a 0.5 mm zone, default class 0.2 mm; fill area of zone USB_OUT in mm2): no rule
  0.893; an unconditional rule of 0.127 mm 7.701 (0.127 mm gap), of 0.4 mm 7.119 and of 0.8 mm
  4.523, so a matching rule replaces the zone's value, up as well as down; a rule matching
  `A.NetName` of the neighbouring track 0.3 mm 7.408, one matching no item or no rule 0.893; a
  net class clearance of 0.1 mm 0.893 (as 0.2 mm), of 0.9 mm 4.254 (0.9 mm gap): a class below the
  zone's changes nothing, so classes are not read. Taking the smallest over all rules is the safe
  side: a floor above KiCad's gap would make the cleanup's fill smaller than KiCad's and remove a
  trace KiCad counts connected. A `.kicad_dru` that cannot be read passes no clearance flag (the router
  keeps its own clearances; thickness and island flags are unaffected) and logs a WARNING; no file
  is no rule. Tests: `test_pour_clearance.py`.
- **Always on, not options** (Q18, decided 2026-10-20: routing never breaks the board's
  manufacturing rules): `edge_graphics.py` (`export_flow.fix_edge_graphics`) writes every Margin
  shape and every Edge.Cuts shape off the outline's outer contours into the DSN as keepouts named
  `edge_clearance` on every copper layer, grown by `m_CopperEdgeClearance` (slice-dcmt-bread-slice:
  21 Margin shapes, 186 keepouts), and every board-level F.Mask/B.Mask shape as keepouts named
  `solder_mask` on its side's copper (slice-dcmt-bread-slice: 2 filled polygons, 2 keepouts;
  KiCad 10's `IsAnyFill` stands in for the removed `IsFilled`); `export_flow.solder_mask_args` passes the pads whose mask
  aperture reaches past the DSN's smallest class clearance as `--router.solder_mask_expansions`
  (pocketsdr-v3-0: its 9 test points, 0.1016 mm on B.Mask). Both log one line in `plugin.log`.
  Both grow by the board's `m_SolderMaskToCopperClearance` (Q22.12; 0 changes nothing).
  A footprint's own F.Mask/B.Mask shapes become `solder_mask_pads` keepouts (Q22.16,
  `footprint_masks.py`); `pad_trim.py` renames one over pads of several nets
  `solder_mask_shared` and cuts those pads back to their part outside it (fc-rom's J1: 72
  fingers), and logs each pad it cannot reach. Every mask keepout carries
  `(clearance_class null)` (Q22.18). Tests: `test_edge_graphics.py`, `test_solder_mask.py`,
  `test_footprint_masks.py`.
  Each thermal spoke of a pad that a zone of its net connects by thermal relief gets a
  `thermal_spoke` keepout the router fits to the pad's net (Q22.20, `thermal_spokes.py`,
  `export_flow.fix_thermal_spokes`), so other nets leave Board Setup's `min_resolved_spokes`
  room. Test: `test_thermal_spokes.py`.
- Modules: `dsn_fixups.py` (pure, the transforms, ported from `dsn_export.py` with its arithmetic),
  `kicad_board.py` (pcbnew passed in: zones and fills, pads, outline, edge clearance, track locks),
  `export_flow.py` (export, K15.24 plane cuts, K01.2 rule areas, K09 fix-ups, `after_import`),
  `plane_priority.py` (pure: the cut planes written into the DSN), `ses_vias.py`,
  `restore_copper.py` (pure: the input copper back after the import, Q22.3).
  `plugin.py` and `kicad-plugin/headless_run.py` both go through `export_flow`, so a headless run
  routes what a plugin run routes. A failing fix-up is logged as a WARNING in `plugin.log` and routing
  goes on with the DSN as it was.
- **Headless graded runs:** `python3 -I kicad-plugin/headless_run.py export BOARD WORK OPTIONS.json`
  (writes `WORK/<stem>.dsn` and the router flags in `<stem>.args.json`), run the router with those
  flags plus `--progress json --result-json`, then `headless_run.py import BOARD SES OUT OPTIONS.json`.
  Grade OUT with `kicad-cli pcb drc --refill-zones --severity-all` (`--schematic-parity` inside the
  project), canary's `via_in_pad.py` and `inner_tracks.py`, and `kicad-plugin/pour_crossings.py INPUT
  OUT CLASSES` (foreign tracks through the listed pours). K09's runs used a script doing exactly this
  (`target/orchestrator/k09/scratch/arm.sh`, not committed).
- Tests: `test_dsn_fixups.py` (every transform on `tests/data/k09_four_layer.dsn` and
  `k09_two_layer.dsn`, the `auto` rules, `ses_vias`, the drill fix), `test_options.py` (K09 flags,
  round trip, bad values, the form), `test_route_dialog.py` (a board preset reaches the router,
  `fix_existing` keeps the tracks locked through the import only, the pass hint is stored and used),
  `test_cli_integration.py::test_the_k09_flags_and_a_fixed_up_dsn_reach_the_real_cli`.
- Not yet seen inside KiCad: the layout of the new groups and the Preset drop-down (owner, with K02.8).

**Profile defaults (K09.9).** `options.PROFILE_DEFAULTS` holds what each profile's plugin defaults
change from the dataclass defaults; a new board, and a field an older options file lacks, start
from it. The engine's `ENHANCED` (`settings/profile.rs`) is not involved. An entry only lands with
the registered measurement that accepted it (`docs/roadmap/prereg/K09.json`, then
`K09.9-rerun.json`). `legacy` is empty. `enhanced` is `{"sweep": "free"}`: Q10's add-only completion
sweep after the passes, with the router's 2 sweep passes (`--router.autorouter.sweep=free`). The
rerun of 2026-10-18 (engine `571d06d`, whose `enhanced` already has `power_layer_traces = none`,
`neck_width_um = 300` and `via_clearance = net_class`) judged six arms through the headless path on
`canary-pwr` (canary `b1994423`) and the three demos (KiCad-unconnected, set noise 17.14):

| arm | canary-pwr | interf_u | pic_programmer | CM5_MINIMA_3 | total | wall s | verdict |
|---|---|---|---|---|---|---|---|
| `plugin-default` (`{}`) | 63 | 12 | 2 | 18 | 95 | 261 | baseline |
| `+mp1` | 63 | 28 | 2 | 32 | 125 | 76 | worse |
| `+sweep` | 42 | 12 | 2 | 18 | 74 | 282 | **kept** |
| `+mp1+sweep` | 41 | 16 | 2 | 22 | 81 | 103 | −14, within noise |
| `+cli-flags` (`-mp 1`, ripup 1500, sweep) | 39 | 14 | 2 | 28 | 83 | 146 | −12; 2 new clearance errors on CM5_MINIMA_3 |
| `+cli-flow` (`+cli-flags`, `auto` pours) | 45 | 14 | 2 | 12 | 73 | 439 | passes; tie to `+sweep` by wall time |

No arm put a via in a pad, copper below the minimum or a trace on a power layer. So a default
`enhanced` run on `canary-pwr` grades 42 (self-graded, 9 starved thermals, no copper error), not
the CLI's 33: the 33 is canary's own `dsn_export.py` DSN of `9511d9a`, and the plugin export of the
same flow (`+cli-flow`, or the plugin45 run's DSN with the preset's flags: 41, canary-7a's number)
lands within canary's ±10 of it (K09.8 gives the DSN differences). For the graded flow, use the
canary preset file (`fixtures/canary-pwr/presets/canary-graded-flow.json`, Preset drop-down).
Under `enhanced` the plugin keeps the Board Setup class widths and passes no neck-down flag: the
engine's `enhanced` profile sets `router.neck_width_um = 300` (Java's necked retry of a failed
connection), which N05.3 accepted against class narrowing on 2026-10-18
(`docs/roadmap/prereg/N05-neck-down.json`: KiCad unconnected 103 → 106 within noise 17.14,
copper under the Board Setup width 848.9 → 310.4 mm where the narrowing acted), and
`router.neck_down.widen_back = true` (the retry's narrow traces widened back to the class width
where they fit), which Q17.4 accepted on 2026-10-20 (`docs/roadmap/prereg/Q17.4.json`: KiCad
unconnected 164 → 168 within noise 15.42, narrowed copper on the wide classes 461.8 → 400.6 mm). `class_widths`
(a preset or `auto`) still narrows a class when chosen.

## The dev machine (Windows 11, KiCad 10.0)

- Installed at `C:\Users\thebo\Documents\KiCad\10.0\scripting\plugins\freerouting_rs`, with
  `settings.json` = `bin_dir: C:\Users\thebo\Documents\code\freerouting-rs\target\release`
  (plus a WSL fallback, `wsl_bin_dir: /home/thebo/.cache/freerouting-rs-target/release`, only
  used if the Windows exes are missing).
- Rebuild and reinstall after a change, from the Windows checkout. This builds natively, then
  installs with `bin_dir` set to `target\release` and keeps the WSL fallback:
  `python tools\install_plugin.py --wsl-bin-dir /home/thebo/.cache/freerouting-rs-target/release`
  Then use Tools > External Plugins > Refresh Plugins in the PCB editor; no restart is needed.
- `tools/install_plugin.py` checks the build environment first and offers each missing fix:
  - Rust;
  - the windows-gnu toolchain with WinLibs when there are no Visual Studio C++ tools;
  - on Windows, a viewer that is still running.

  `--check` only reports what it finds. The step-by-step form is still
  `& "C:\Program Files\KiCad\10.0\bin\python.exe" kicad-plugin\install.py --bin-dir target\release --wsl-bin-dir ...`.
- From WSL, `python3 tools/install_plugin.py --windows-kicad` builds the Linux executables and
  installs into the Windows KiCad with `wsl_bin_dir` set to that build. A `bin_dir` that was
  already in `settings.json`, the Windows exes, stays in place and is still preferred. It finds
  the Windows KiCad folder by asking `powershell.exe` for `KICAD_DOCUMENTS_HOME` and the
  Documents shell folder, so a Documents folder moved off the profile (`D:\Documents` on one
  machine) is found; `--plugins-dir` also takes a Windows path.
- The owner had not yet confirmed the button appears in KiCad when these notes were written.

## Native Windows toolchain

- rustup with the `x86_64-pc-windows-gnu` host toolchain (no Visual Studio needed), in
  `%USERPROFILE%\.cargo` / `.rustup`.
- WinLibs mingw-w64 (`winget install BrechtSanders.WinLibs.POSIX.MSVCRT`, user scope). Needed
  because `windows-sys`/`parking_lot_core` use raw-dylib, and the `dlltool.exe` bundled with
  Rust's gnu toolchain calls an assembler (`as`) it doesn't ship. The mingw `bin` must be on
  PATH for the build:
  `C:\Users\thebo\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin`
- The resulting exes only import system DLLs.

**KiCad launches the Windows exes in `target\release` of the Windows checkout.** A build in
WSL (Linux binaries, or a different checkout) does not update them. After any change to
`freerouting-gui` or `freerouting-cli`, rebuild natively. From WSL, for example:

```
cmd.exe /c "cd /d C:\Users\thebo\Documents\code\freerouting-rs && git pull && set PATH=C:\Users\thebo\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin;%USERPROFILE%\.cargo\bin;%PATH% && cargo build --release -p freerouting-gui -p freerouting-cli"
```

The build fails if the viewer is open (Windows locks the running exe), so close it first.

## Tests

`python3 -m pytest kicad-plugin/tests` (plain Python 3 + pytest, no pcbnew or wx; pytest is a
dev-only dependency). `conftest.py` puts `freerouting_rs/` on `sys.path`, so `tools`,
`rule_areas`, `version` and `kicad_areas` import as top-level modules and the package
`__init__` (which registers the plugin) never runs. Fake routers live in `tests/fake_bin/`.
`test_threads.py` fails if a `threading.Thread` target reaches `pcbnew` or `wx` (except
`wx.CallAfter`). `test_rule_areas.py::test_canary_pwr_fresh_kicad_export` runs KiCad's Python
(`FREEROUTING_RS_KICAD_PYTHON`, or a system `python3` that imports `pcbnew`) on a copy of
`../canary/hardware/canary-pwr-rev1.0/canary-pwr.kicad_pcb` (`CANARY_PCB` overrides) and skips
when either is missing. GitHub CI is disabled (owner, 2026-10-15); run the suite by hand.

K02 tests: `test_options.py` (golden argument lists, save/load round trip, damaged files),
`test_progress.py` and `test_summary.py` (replay `tests/data/progress-{j2,bm01}.jsonl` and
`result-{j2,bm01}.json`, recorded from the real CLI with `--progress json --result-json`),
`test_routing.py` (`RouteRun` and `GuiRun` against `fake_bin/fake_freerouting.py` modes `route`,
`route-slow`, `route-fail`, `route-noise` and `fake_bin/fake_gui.py`), `test_route_dialog.py` and
`test_gui_mode.py` (plugin.py's Route and Route in GUI flows with the stub `wx`/`pcbnew` of
`tests/stub_ui.py`: real dialog code, modal loops driven by their timers; also which tab each
option sits on and the cut to the screen), `test_layout.py` (the page heights), `test_dropdowns.py`
(when the dark-list fix applies, and its Win32 calls against fakes), and
`test_cli_integration.py` (the real CLI: all options, Stop, Abort, summary against `-drc`).
The last one uses `FREEROUTING_RS_BIN` (a directory), else `target/release` or `target/debug`, and
skips with the reason when no `freerouting-rs` with `--capabilities` is found.

## SWIG, IPC API and KiCad versions (K01.5)

Checked 2026-10-15 against KiCad's GitLab (`gitlab.com/kicad/code/kicad`).

- **KiCad master has removed SWIG, wxPython and the Python integration.** Verified: commit
  [`65a442b1`](https://gitlab.com/kicad/code/kicad/-/commit/65a442b1d2bf153be7979de8926ab71fd095f4bd),
  "REMOVED: SWIG, wxPython, and Python integration", 2026-03-22, "The IPC API will be used going
  forward" (fixes work item 9078). The commit is on `master` only (refs query), so the
  KiCad 10.0 series keeps SWIG (10.0.5 on the dev machine and 10.0.7 here both import `pcbnew`).
  Verified: the plugin works through 10.x; it stops loading in the release cut from master
  (presumably 11.0; the version number itself is unverified).
- **IPC API, Specctra.** Verified in the protobuf files on `master`: `RunBoardJobExportSpecctraDsn`
  (`board_jobs.proto`, "Since: 11.0", returns `RunJobResponse`) exports DSN, and
  `ImportSpecctraSession` (`board_commands.proto`, "Since: 11.0"; `session_path`, returns
  `skipped_count` and a report) imports a `.ses`: it repositions footprints, deletes unlocked
  tracks and vias, adds the routed ones, and skips locked ones. Verified: neither message
  exists in the `10.0` branch protos. Verified: `kipy` (kicad-python `main`, PyPI 0.8.0) has no
  wrapper for either (`board.py` and `board_jobs.py` do not mention Specctra), so a plugin would
  send the raw protobuf command. Both work on a nightly; how they differ from 10.0.7 is in the
  next bullet. Where the IPC calls do not exist (KiCad 10), SWIG `ExportSpecctraDSN` /
  `ImportSpecctraSES` stay. If KiCad ever drops both, K08's direct `.kicad_pcb` writer and
  K04-K07's reader replace them.
- **Nightly probe (K11.1, 2026-10-17).** The build was KiCad `10.99.0` at master `80521a1b94`
  (kicad-dev-nightly PPA, in Docker), headless through `kicad-cli api-server`. It was compared
  with 10.0.7's SWIG calls on copies of `dev-board`, `rule-areas` and `canary-pwr`.
  `tools/kicad-ipc/` holds the scripts, and the numbers are in K11.1's done note.
  - **Import: the same as 10.0.7.** `ImportSpecctraSession` lands the same tracks and vias as
    `ImportSpecctraSES`, 0.0 nm apart on all three boards, with `skipped_count` 0. Locked
    tracks and vias are kept by both. The import edits the open board in one commit
    ("Imported Specctra session via API"), so it is one undo step.
  - **Export: the job reads the file on disk, not the open document.**
    `handleRunBoardJobExportSpecctraDsn` hands `GetCurrentFileName()` to `KIWAY::ProcessJob`,
    and an unsaved track was missing from its DSN. `SaveCopyOfDocument` and
    `SaveDocumentToString` do carry the unsaved edit. `GetDocumentModifiedState` (11.0) tells
    whether there is one.
    - `SaveCopyOfDocument` with `include_project` also writes the `.kicad_pro` and `.kicad_prl`.
      Whether those carry unsaved Board Setup changes was not tested.
    - KiCad 10.0's API has `SaveCopyOfDocument` and `SaveDocumentToString` too, but no board
      jobs.
    - `kicad-cli pcb export specctra` exists on master, not in 10.0.7. It gives the same DSN
      as the job.
  - **Export: master's DSN differs from 10.0.7's in three ways.**
    1. Copper zones become fixed copper: `(wire (polygon ...) (net N) (type protect))`, with
       the fill's cut-outs as `(window ...)`, instead of `(plane ...)`. This happens for every
       zone, no-net zones included. There is no option to keep planes: the job has only
       `job_settings`, and the CLI only `-o` and `-D`.
       - `canary-pwr`: 20 planes become 29 wires with 400 windows.
       - Routed from each DSN (`-mp 1 --threads 2`, Rust `a7a164a`) and graded by 10.0.7's DRC
         with `--refill-zones`: 68 unconnected with 31 dangling vias from 10.0.7's export (the
         same 68 as K09's plugin run), and 57 with 15 from master's. Pass 1 took 39 s instead
         of 11 s.
    2. **A regression: footprint-owned rule areas land in the wrong place.** The footprint's
       position is subtracted a second time. `dev-board`'s antenna keepout is
       `(-9000 9800)-(9000 15750)` in 10.0.7, correct for U2 at `(127000 -51250)`, and
       `(-136000 61050)-(-118000 67000)` on master, off the board. Since the keepout then
       differs per placement, each instance gets its own image (`rule-areas`: 5 images become
       8). The CLI export and the API job agree.
    3. A fix: 10.0.7 writes the window of a B.Cu `via_keepout` on F.Cu, and master writes it on
       B.Cu.
  - **Plugins.**
    - `plugin.json` follows `schemas/api.v1.schema.json`. Required fields: `identifier`,
      `name`, `description`, `runtime` (`type` `python` or `exec`) and `actions`. Each action
      needs `identifier`, `name`, `description` and `entrypoint`, and may set `show-button`,
      `scopes` (`pcb`, ...), `args`, `icons-light` and `icons-dark`.
    - KiCad finds IPC plugins by `plugin.json` under the stock plugins folder, the PCM's
      `3rdparty` folder and the user's `<Documents>/KiCad/<ver>/plugins/`. That user folder is
      not `scripting/plugins` (`common/paths.cpp`).
    - It sets `KICAD_API_SOCKET` and `KICAD_API_TOKEN` for the action, and builds a Python
      plugin's environment from its `requirements.txt` (`api_plugin_manager.cpp`).
  - **The API is off by default.** The setting is `api.enable_server`, default `false`
    (`common_settings.cpp`), shown as "Enable KiCad API" on the Preferences Plugins page.
    Installing a PCM package whose version says `"runtime": "ipc"` offers to turn it on
    (`pcm.cpp`).
  - **PCM: one runtime per package version** (`pcm.v2.schema.json`: `runtime` `swig` or
    `ipc`, with `kicad_version` and `kicad_version_max`). So the SWIG and the IPC plugin
    ship as two versions, two zips, each limited to its KiCad versions.
  - **Headless API sockets.** `kicad-cli api-server [--socket PATH] FILE` serves one board, and
    an empty token is accepted. A Unix socket path must stay under 108 bytes, so the probe
    dials a relative path.
- **Copper-pour flag rename.** Verified in `pcbnew/zone.h`: the `9.0` branch has
  `GetDoNotAllowCopperPour`, the `10.0` branch and `master` have `GetDoNotAllowZoneFills`.
  Verified by probe on 10.0.7: `GetDoNotAllowZoneFills` exists, `GetDoNotAllowCopperPour` does
  not; the track, via, pad and footprint accessors (`GetDoNotAllowTracks`, `...Vias`, `...Pads`,
  `...Footprints`) exist unchanged. `kicad_areas._pour_flag` tries both names (`getattr`), so
  KiCad 9 needs no other fallback for this flag (not run on 9).
- **Probe result (KiCad 10.0.7, WSL, `pcbnew.LoadBoard` on a copy of `canary-pwr.kicad_pcb`):**
  one rule area, "Fan inlet keep-out (ASSUMED position - verify on the Pi)", layer B.Cu,
  tracks False, vias False, pads True, footprints True, pour False, outline
  (136.5, 53)-(162.5, 79) mm, not footprint-owned. Its DSN export is
  `(keepout "" (polygon B.Cu 0 136500 -53000 162500 -53000 162500 -79000 136500 -79000 ...))`.
  Not probed on Windows KiCad 10.0.5.

## PCM package (K03)

One zip per version for KiCad's Plugin and Content Manager, built locally and by R01's release
job (`docs/development/release-workflow.yml`, the "PCM zip" step). **Nothing is submitted or
uploaded:** the KiCad repository submission and the GitHub release need the owner's go-ahead.

- **Files.** `kicad-plugin/metadata.json` (identifier `com.github.danbrunski.freerouting-rs`,
  `"runtime": "swig"`, `"status": "testing"`, `"kicad_version": "10.0"`, one `versions[]` entry,
  `download_url` naming the GitHub release asset; no checksum or sizes in the source file),
  `resources/icon.png` (64 x 64, a copy of `crates/freerouting-gui/icons/icon-64.png`, so a new
  icon is a `render.py` run plus a copy), `package.py`, `update_metadata.py`, `validate_pcm.py`.
- **Build.** `python3 kicad-plugin/package.py --bin <dir> --out <dir>`; `<dir>` holds
  `bin-<platform>/` (the release workflow's artifact names) or `<platform>/`, for
  `windows-x86_64`, `macos-universal`, `linux-x86_64`, each with `freerouting-rs` and
  `freerouting-gui` (`.exe` on Windows). `--bin linux-x86_64=target/release` names one platform;
  `--allow-missing` builds a partial package for local tests. Output:
  `freerouting-rs-<version>-pcm.zip` and `<zip>.meta.json` (sidecar: `version`, `file`, `sha256`,
  `download_size`, `install_size`).
- **Layout.** `metadata.json` (inner copy without `download_*` and `install_size`),
  `resources/icon.png`, `plugins/` = the contents of `freerouting_rs/` (denylist: caches, `tests`,
  `settings.json`, a local `bin/`), `plugins/bin/<platform>/` and, because KiCad's validator accepts
  nothing outside `metadata.json`, `resources/icon.png` and `plugins/**`, also `plugins/LICENSE`,
  `plugins/THIRD_PARTY_NOTICES.md` and `plugins/LICENSES/`. KiCad copies `plugins/` to
  `3rdparty/plugins/com_github_danbrunski_freerouting-rs/`, which is the plugin package itself
  (`plugins/__init__.py` is its `__init__.py`).
- **Deterministic.** Sorted entries, directory entries included, 1980-01-01 timestamps, Unix
  attributes (0755 for directories and binaries, 0644 otherwise), deflate level 9. The same inputs
  give the same bytes with the same zlib; another zlib version may compress differently, so the
  release zip is built once and its sha256 goes into the metadata.
- **Release metadata.** `update_metadata.py metadata.json <zip>.meta.json` sets
  `download_sha256`, `download_size` and `install_size` on the version entry the sidecar names and
  refuses (naming both versions) when none matches or `download_url` does not end in the zip's
  name. `tools/release/check_version.py` already compares the entry and URL with the other versions.
- **Resolver.** `tools.bundled_bin_dir()` returns `bin/<os>-<arch>/` next to `tools.py` (win32 +
  AMD64/x86_64, any macOS architecture, linux + x86_64), or `None` (an `install.py` copy, a
  platform the package lacks, Linux or Windows on ARM). It sets the executable bit when missing
  (a fallback: KiCad keeps the zip's 0755, see below). The dialog says which platforms the package
  carries when the router is missing.
- **Size.** Real binaries (Linux CLI 8.7 MB + GUI 18.4 MB, Windows exes 1.2 + 27.8 MB, from
  2026-10-15/17 builds, no macOS): 56.3 MB installed, **19.6 MB zip**. A macOS universal pair adds
  about 2 x (3.2 + 6.6) MB compressed, so the full zip should be near 40 MB; the validator's
  download limit is 100 MiB (`MAX_DOWNLOAD_SIZE` in `ci/validate/package.py`).
- **K03.5, KiCad's validator.** `validate_pcm.py --zip Z --kicad-metadata <clone>` patches the
  sidecar values into a copy of `metadata.json`, serves the zip from 127.0.0.1, points
  `download_url` there and runs the clone's `ci/validate-package.py` (the script `ci/validate.sh`
  runs on a merge request) with its `schema-v1.json` and `schema-v2.json`. Run 2026-10-17 on
  commit `cdd6c04105220261ae5a3cc9432075311ab95a61` of `gitlab.com/kicad/addons/metadata`
  (schemas `https://go.kicad.org/pcm/schemas/v1` and `v2`; the clone's v1 equals the fetched one),
  jsonschema 4.26.0, requests 2.34.2, tqdm 4.70.1, munch 4.0.0, pillow 13.0.0 in a venv:
  `schema ok` for both schemas, then `Validation passed` for the fake-binary zip (47 KB) and the
  real-binary zip (19.6 MB). A zip with a `LICENSE` at its root is rejected (`package contains
  extra file "LICENSE"`), so the check can fail. The tests run only with
  `KICAD_METADATA_REPO=<clone>` (and `PCM_VALIDATOR_PYTHON=<venv python>`). Setup:
  `git clone https://gitlab.com/kicad/addons/metadata.git`, `python3 -m venv v`,
  `v/bin/pip install -r metadata/ci/requirements.txt`.
- **K03.6, Linux, KiCad 10.0.7 (WSL), 2026-10-17, on a virtual display with a scratch
  `KICAD_CONFIG_HOME`/`KICAD_DOCUMENTS_HOME` (nothing under the real KiCad folders).** The PCM's
  Install from File... took the real-binary zip. Result: `3rdparty/plugins/com_github_danbrunski_freerouting-rs/`
  holds the plugin, `bin/linux-x86_64/freerouting` and `freerouting-gui` came out **executable**
  (KiCad keeps the Unix mode of the zip entries), `3rdparty/resources/<id>/icon.png` is the
  icon, and no `metadata.json` is left in the plugin folder (`install.py`'s shadow check finds the
  folder by name). A pcbnew on the `pic_programmer` demo listed `freerouting-rs` under Tools >
  External Plugins; its dialog showed `Router: .../bin/linux-x86_64/freerouting`, `Router
  version: 0.1.0 (813663cea)`; **Route** ran the bundled router and imported the session (115 of
  126 connections, 6 vias). The PCM could not fetch KiCad's repository list (no network from that
  process, "Timeout was reached"), which does not affect Install from File. The first-run
  wizard of the scratch config and the stacking of dialogs without a window manager made the
  clicks awkward; steps: project manager > Plugin and Content Manager (not the "unarchive"
  toolbar button) > Install from File... > pick the zip.
- **Found by that run (K01 bug, fixed):** `fix_rule_areas` used `Path.read_text(newline=...)`,
  which exists only from Python 3.13; KiCad 10 ships 3.12, so on every run the rule-area fix-up
  was skipped with `WARNING: rule-area fix-up skipped: TypeError(...)` in `plugin.log`. It now uses
  `open()` (`tests/test_dsn_fixup_io.py`). The owner's Windows KiCad 10.0.5 has the same problem if its
  Python is older than 3.13, so re-check the K01.6 `canary-pwr` log line after reinstalling.
- **Not checked.** Windows (KiCad 10, PCM install of the zip, the toolbar button, `Router:` showing
  `plugins/bin/windows-x86_64`; remove the manual `scripting/plugins/freerouting_rs` copy first or
  K01.4's guard reports the shadow) and macOS (quarantine attribute on the unpacked binary,
  Gatekeeper, "Open Anyway"; needs R01's macOS job). The macOS universal binary and the MSVC
  Windows build have not been in a zip yet. Whether KiCad on Windows/macOS keeps the executable
  bit is moot there (Windows needs none; the resolver sets it on macOS).

## Gotchas

- **Stop and keep waits for the router's step (fixed 2026-10-18).** Stop writes `stop` to the
  router's stdin; the CLI then ends at its next stop check. Before 2026-10-18 the trace
  normalization after each route had no check, and a net whose split and combine oscillate runs
  2000 passes: on `Issue690-kit-dev-coldfire-xilinx_5213` one fanout pin took about 280 s, so the
  dialog sat at "Stopping: finishing the current step..." for minutes (a SIGTERM took over 5
  minutes too). Now a stop ends within about 0.1-0.5 s on the fixtures measured
  (`test_stop_inside_a_long_fanout_step_ends_within_seconds`; engine side
  `crates/freerouting/tests/normalize_stop.rs`). Abort still kills the process at once.
- **Never close, restart or kill KiCad, and don't touch the Java Freerouting plugin**
  (`3rdparty\plugins\app_freerouting_kicad-plugin`) or its Java processes; the owner runs it
  alongside this one.
- **Don't import the plugin package in a standalone KiCad Python.** `register()` outside the
  editor raises a wxWidgets "PgmOrNull() failed" debug dialog on the owner's desktop. Test
  `tools.py` and `export_dsn` directly instead (`pcbnew.LoadBoard` + `ExportSpecctraDSN`
  works headless). A standalone wx script in KiCad's `python.exe` must create `wx.App` before
  any other wx call (even `wx.GetLibraryVersionInfo()`), or wxWidgets shows a "failed to
  initialize wxPlatformInfo" assert box.
- **A scrolled tab page asks for no height.** A `wx.ScrolledWindow` with a vertical scroll rate
  reports its minimum size as its best height, not its contents', so a notebook of them fits to
  almost nothing. `plugin.set_page_heights` sets each page's minimum size from its sizer and its
  virtual size to the contents, which is what scrolls once `fit_to_display` cuts it.
- **A group box's controls must be its children.** wxWidgets 3.3 logs "should be created as
  child of its wxStaticBox" for a control in a `wx.StaticBoxSizer` that has another parent; the
  dialog creates each group's sizer first and its controls in `GetStaticBox()`.
- **Dark drop-downs on recent Windows 11 are white on white in KiCad 10.0.** wxWidgets 3.3.2,
  which KiCad 10.0 bundles, themes a `wx.Choice`'s list so that Windows 26200.9xxx (and Insider
  26300.8553) draws the selected row, the one under the mouse, white under white text
  ([wxWidgets #26535](https://github.com/wxWidgets/wxWidgets/issues/26535), KiCad work item
  23570; fixed in wxWidgets 3.3.3). `dropdowns.py` gives the dialog's lists the
  `DarkMode_DarkTheme` theme, as wxWidgets 3.3.3 does. It does nothing on other systems, in
  light mode, before build 26200, and with wxWidgets 3.3.3 or later. Checked on the dev machine
  (26200.9711, KiCad 10.0's `python.exe` and wx 3.3.2, a standalone window loading only
  `dropdowns.py`): unreadable before, readable after, light mode unchanged. KiCad's own
  drop-downs keep the bug until KiCad ships a newer wxWidgets.
- **"Unhandled exception" (Abort / Retry / Ignore) on closing the dialog after a route.** KiCad
  10's `ImportSpecctraSES(board, path)` deletes the unlocked tracks it replaces, but the
  action-plugin runner (`PCB_EDIT_FRAME::RunActionPlugin`) snapshots every board item before
  `Run` and, when `Run` returns, commits the missing ones as removed: a use-after-free that
  wxWidgets reports with that box (sometimes, depending on what reused the memory). The
  editor's own SES import avoids it by clearing the undo list first. `import_ses` now takes
  the unlocked tracks off the board itself (`Remove`, `thisown = 0`) before importing, so the
  importer deletes none, the runner records them as removed, and Undo restores the old
  routing. A failed import puts them back. Checked with KiCad 10.0.7's `pcbnew` module:
  the old tracks stay readable after the import, and the board reads back normally.
- **The owner's display is at 125% (120 DPI).** Screen captures of the viewer from a DPI-unaware
  process (plain PowerShell) get only the top-left part of the window and look wrongly
  fitted. Call `SetProcessDPIAware()` before `GetWindowRect`/`PrintWindow`.
- KiCad's DSN export carries no silkscreen. The viewer's reference designators come from
  the DSN placement data.
- `git` on Windows has no identity configured; commits use
  `git -c user.name=Claude -c user.email=noreply@anthropic.com`.

## Verified

- Headless (KiCad's Python): DSN export, router call, viewer launch, both native and via WSL.
- Five KiCad 10 demo boards (ecc83-pp, interf_u, complex_hierarchy, kit-dev-coldfire,
  CM5_MINIMA_3) exported to DSN and loaded by `build_board` without parse errors.
- 2026-10-16 (K02, headless, WSL KiCad 10.0.7): the plugin's Route path against the real CLI (all
  options, Stop, Abort, summary: `test_cli_integration.py`), non-ASCII net and reference names
  through export, route and import (K02.6), and the five demos graded against Java (K02.7). Not
  yet run inside KiCad (K02.8).
- Native viewer renders CM5_MINIMA_3 fitted, and ecc83-pp with reference designators.
- 2026-10-17, Windows, main at `97d10d6`: native rebuild (windows-gnu + WinLibs, 1m34s),
  `freerouting --version` = `0.1.0 (git 97d10d6315ee)`, plugin reinstalled with `install.py
  --bin-dir target\release` (no shadow copies found) and byte-compiled by KiCad 10.0.5's Python
  3.11.5. The CLI routed KiCad's `ecc83-pp` demo exported with its tracks in 0.18 s ("6 unrouted",
  where KiCad's DRC of the demo reports 0 unconnected), KiCad's `ImportSpecctraSES` took the
  session headlessly, and the GUI opened the DSN + SES. Not yet run inside KiCad.

## What's left

- **K01.6, owner:** after the orchestrator reinstalls the plugin (`install.py --bin-dir target\release`,
  then Tools > External Plugins > Refresh Plugins, no restart), check in KiCad 10.0.5: the toolbar
  button, the dialog with both versions, Open in viewer, Import SES, and on `canary-pwr` that
  `freerouting-rs/plugin.log` names the fan inlet (`keepout -> place_keepout`). Record the date and
  what he saw under "Verified". Nothing in K01.2/K01.3 has run inside KiCad yet; only headless
  (pcbnew in a plain Python, fake pcbnew in tests).
- **K02.8, owner (Windows, inside KiCad):** see "K02.8: what the owner checks" below.
- Real silkscreen in the viewer would need the plugin to export extra data from the board
  alongside the DSN (offered to the owner, not requested).

## K02.6: non-ASCII names round trip

`tools/kicad-oracle/roundtrip_names.py --router target/release/freerouting` (run with a python3 that
imports `pcbnew`; KiCad 10.0.7 here). For each case it strips the tracks of a copy, exports the DSN
with `ExportSpecctraDSN`, routes it with the CLI (`-de x.dsn -do x.ses`, nothing sanitised),
imports the SES with `ImportSpecctraSES` into the stripped copy and compares: every track and via of
the routed board has a net name that is a net name of the original, the non-ASCII names are in the
DSN as they are, every imported non-ASCII net is in the SES as it is, no footprint reference
changed, and `kicad-cli pcb drc` reports no `shorting_items`.

KiCad 10's `interf_u` demo has plain ASCII names (the older board behind fixture
`Issue093-interf_u` has Chinese text, not Omega), so both cases rename: `interf_u` gets Omega on
8 nets and on 11 references (`R`/`C` become `RΩ`/`CΩ`), `ecc83-pp` gets mu and Phi on 3
nets and 6 references.

Result, 2026-10-16, `--max-passes 3`, release CLI at this tree, KiCad 10.0.7 (WSL):

| case | tracks and vias imported | non-ASCII nets routed | name differences | `shorting_items` |
|---|---|---|---|---|
| `interf_u` (Omega) | 789 | 8 (`/8MH-OUTΩ`, `/BIT0Ω`, ...) | 0 | 0 |
| `ecc83-pp` (mu, Phi) | 33 | 2 (`Net-(P3-P1)Φ`, `Net-(U1A-G)µ`) | 0 | 0 |

`GNDµ` has no wires (a zone net), so it is in the DSN and not in the SES. Not run in Windows
KiCad 10.0.5; `pgrep -x kicad; pgrep -x pcbnew` printed nothing before and after.

## K02.7: graded runs on real boards

`tools/bench/k02-graded-runs.sh tools/bench/sets/kicad-demos.txt OUT target/release/freerouting
~/code/freerouting/logs/jars/freerouting-a773a70ec.jar` (set file: the five demos and `canary-pwr`,
out of the repo, skipped when missing). Each engine gets the same `grade.sh` export (tracks and
vias stripped, DSN exported by KiCad 10.0.7, footprint-only rule areas removed, which is what the
K01.2 rewrite achieves) and the same `--router.min_trace_width_um` = the board's Board Setup
minimum track width (D8; the DSN does not carry it). Rust runs the plugin's default options and
nothing else; Java runs `a773a70ec` with `--gui.enabled=false --router.optimizer.enabled=false`
(its optimizer still depends on thread scheduling, J01). Both end by their own stop rule (no pass
limit). Graded by KiCad's own DRC (`grade.sh`, items the unrouted input does not already have),
vias inside pads counted by `tools/kicad-oracle/via_in_pad.py`. 2026-10-16, WSL, KiCad 10.0.7,
release CLI of this tree (with other sessions' uncommitted engine work in it), `pgrep -x kicad;
pgrep -x pcbnew` empty before and after:

| board | engine | DRC violations caused by the router | unconnected / input | vias | tracks | length mm | vias in pads | below board min. | engine s |
|---|---|---|---|---|---|---|---|---|---|
| ecc83-pp | rust | none | 0 / 14 | 0 | 33 | 161.8 | 0 | 0 | 0.02 |
| ecc83-pp | java | none | 0 / 14 | 0 | 33 | 161.8 | 0 | 0 | 3.2 |
| interf_u | rust | starved_thermal 5 | 10 / 164 | 43 | 1021 | 4478.5 | 0 | 0 | 15.4 |
| interf_u | java | starved_thermal 4 | 11 / 164 | 45 | 1029 | 4470.2 | 0 | 0 | 14.7 |
| complex_hierarchy | rust | none | 80 / 87 | 1 | 28 | 112.6 | 0 | 0 | 0.07 |
| complex_hierarchy | java | none | 80 / 87 | 1 | 28 | 112.6 | 0 | 0 | 3.3 |
| kit-dev-coldfire-xilinx_5213 | rust | clearance 1, shorting_items 1, via_dangling 48 | 87 / 478 | 240 | 1650 | 5072.6 | 0 | 0 | 446 |
| kit-dev-coldfire-xilinx_5213 | java | shorting_items 1, track_dangling 1, track_width 190, via_dangling 43 | 85 / 478 | 236 | 1748 | 4876.5 | 0 | 190 | 591 |
| CM5_MINIMA_3 | rust | clearance 1, copper_edge_clearance 24, hole_clearance 2, isolated_copper 10, track_dangling 1 | 22 / 208 | 187 | 1319 | 2646.0 | 0 | 0 | 99 |
| CM5_MINIMA_3 | java | the same | 22 / 208 | 187 | 1319 | 2646.0 | 0 | 0 | 252 |
| canary-pwr | rust | copper_edge_clearance 2, starved_thermal 2, via_dangling 36 | 70 / 174 | 124 | 734 | 1056.9 | 0 | 0 | 199 |
| canary-pwr | java | copper_edge_clearance 2, starved_thermal 2, track_dangling 1, track_width 2, via_dangling 32 | 71 / 174 | 119 | 787 | 1049.2 | 0 | 2 | 213 |

Differences explained:

- `CM5_MINIMA_3`: the two sessions have the same 187 wires and vias, line for line (the CM5 board
  has no Board Setup minimum track width, so 0 was passed).
- `kit-dev`: Rust's one `clearance` item is a via 0.04 mm from the copper text `MOD0` on
  `Top_layer`. KiCad's DSN export has no copper text, so neither router can see it; Java's route
  happened to miss it. Both engines have the same `shorting_items` (a track through the copper
  text `JTAG_EN`). Java has 190 tracks narrower than the board minimum (0.198 mm), Rust none (D8).
  `via_dangling` 48 against 43 and 87 against 85 unconnected are the two routes ending in
  different places after 18 passes each.
- `interf_u`: one more `starved_thermal` (a pour effect, not a clearance) in Rust.
- Vias in pads: 0 in every graded run.

- `canary-pwr`: the same two `copper_edge_clearance` and two `starved_thermal` items in both
  engines; Java has 2 tracks under the board minimum (0.15 mm) and one `track_dangling`, Rust none;
  `via_dangling` 36 against 32 and 70 against 71 unconnected are the two routes ending in
  different places. The Rust row was graded at `813663c` (2026-10-17, `ENGINES=rust`), after the
  fix of a panic in `normalize_pass` (`8fb7e21`: a trace removed by a drill split was read after
  its removal; Java carries on with the stale object, which the fix matches). This export
  (KiCad 10.0.7, pours as 20 planes, no wiring) is a different board from
  `fixtures/canary-pwr/canary-pwr-9511d9a.dsn`, so its numbers do not compare with the canary
  grades in `fixtures/canary-pwr/README.md`.

## K02.8: what the owner checks (Windows, inside KiCad)

Nothing of K02 has run inside KiCad yet; the dialogs are tested against a stub `wx`/`pcbnew`
(`tests/stub_ui.py`) and the router against the real CLI. Needs a native Windows rebuild first
(the orchestrator does that, "Native Windows toolchain" above: mingw `bin` on PATH, viewer closed,
`cargo build --release -p freerouting-gui -p freerouting-cli`) and a reinstall
(`install.py --bin-dir target\release`, then Tools > External Plugins > Refresh Plugins).

In KiCad 10.0.5 open `canary-pwr` (save it once if KiCad asks), start the plugin, and check:

1. **Options.** Change Maximum passes to 5 and the time limit to `10m`, press Route, close the
   dialog after it ends and open it again: the values are still there.
   `freerouting-rs\route-options.json` exists next to the board. The "Minimum trace width" box
   shows the Board Setup value as its hint.
2. **Route with progress.** The progress dialog shows `Loading`, `Fanout`, `Routing, pass N: x
   routed, y failed, z incomplete (t s)` changing about every second, and the router's log lines.
   When it ends the board has the new tracks and a message shows routed/incomplete connections,
   vias, length and time, with a WARNING line when connections are left.
3. **Stop.** Route again; press "Stop and keep result" after the first pass appears. The dialog
   ends within a pass or two, the board gets the tracks so far and the message starts "Stopped
   early".
4. **Abort.** Route again; press Abort (or the window's close button) after the first pass. The
   board is unchanged, `freerouting-rs\canary-pwr.ses` does not exist.
5. **Route in GUI.** Press "Route in GUI". The freerouting-gui window opens with the board, counts
   down 5 s and routes; close it after routing (or let it finish). The plugin imports the session.
   Run it once more and press Terminate in the wait dialog: the window closes, nothing is imported.
6. **A demo board.** Repeat 2 on one demo (for example `ecc83-pp`, which routes in about a second).

Record the date, KiCad version, what worked, the routed/incomplete counts and times for
`canary-pwr` and the demo, and anything odd (a frozen dialog, a Stop that did not return, a window
that stayed behind) under "Verified" above. The item stays partial until then.

## Direct `.kicad_pcb` input (K07)

`freerouting-rs -de board.kicad_pcb`, the GUI's File > Open and `freerouting::io::read_design` read a KiCad
board without `ExportSpecctraDSN`, so a plugin can hand the CLI the board file. KiCad 11 (K00 question 5;
SWIG removal verified in K01.5) drops `ExportSpecctraDSN`; a master that exports pours as
`(wire (polygon ...))` instead of planes (it does, K11.1) would not matter either. Bringing the
result back still needs `ImportSpecctraSES` or the optional writer (K08).

**What is read** (`crates/freerouting-kicad`, tests in its `tests/` and in `crates/freerouting/tests/kicad_*.rs`):
the board file (KiCad 6 to 10; older files are refused with a one-line message, `a_kicad_5_file_exits_1`), the
`<stem>.kicad_pro` next to it (net classes; KiCad's defaults when missing, `a_missing_project_file_is_not_an_error`)
and the `<stem>.kicad_dru` (K07.5, below). Layers, nets, net classes, the outline, footprints and pads, rule
areas, copper zones as planes, tracks and arcs, vias.

**Rule areas map by their flags** (K06.2, `rule_areas_map_by_flags`), where KiCad's export makes a full
`keepout` of any area that bans neither tracks nor vias:

| Area bans | Emitted |
|---|---|
| tracks and vias | `keepout` |
| vias only | `via_keepout` |
| tracks only | `wire_keepout`, built as a keepout that also blocks vias (as Java does, K00 question 1), one warning per area |
| footprints (any other flags) | the above plus a `place_keepout` |
| only pads, only copper pour, or nothing | nothing; a warning says so |

`canary-pwr`'s fan inlet bans pads and footprints, not tracks: the export makes it a full keepout (5 traces and
15 vias of the board then violate it), the reader a `place_keepout` (0 violations; `canary-pwr` row of the
corpus run, `canary_pwr_fan_inlet`).

**Expected differences from KiCad's export** (`crates/freerouting/tests/kicad_parity.rs`, `EXPECTED_DIFFERENCES`;
the test fails on any other difference, and on a listed one that no longer occurs):

- rule-area kinds as above (K06.2): the reader has `place_keepout`s and fewer full keepouts;
- track arcs are followed as polylines, the export writes one chord (K06.5; the plugin's DSN path
  writes the same polylines since K13.8, `track_arcs.py`);
- copper zones that footprints own stay planes, the export drops them (K06.4, K00 question 6);
- the order of the non-default net classes (project order against `std::unordered_map` order) and of the
  outline's holes (`tastexx-pcb`);
- coordinates keep KiCad's nanometres where the export prints `%.6g` (K00 question 2): areas, lengths and
  extents differ by under 2e-4 of themselves, and on routed boards the export's rounding leaves pads and
  tracks a micrometre apart, so its DRC sees more incomplete connections and dangling tracks than the
  reader's (`export_rounding_explains_connectivity`: round the reader's wires like the export and the counts
  are equal on `bm1`, `bm4`, `bm10`).

**`.kicad_dru`** (`dru.rs`): every rule is read and reported (`KicadInput::report_lines`, printed by the CLI and
shown in the GUI's warnings window). `ReadOptions::apply_design_rules` (default off, K00 question 7) applies
the class-pair clearance (`class_class`) and class track width (`opt`) rules; a condition term the module
cannot evaluate (`insideCourtyard`, `intersectsArea`, `inDiffPair`, item types) makes the rule UNSUPPORTED and
is named in both modes. `canary-pwr`'s ten `insideCourtyard` rules are all UNSUPPORTED
(`canary_pwr_rules_are_unsupported_by_name`). Board minimums of the `.kicad_pro` (track 0.15, annular 0.15, edge
clearance 0.3 for `canary-pwr`) are floors of the router's settings since Q18; pad-local clearances are read since K13.1
(`pad_clearance`: a pad's own `(clearance)`, else its footprint's since K15.9, as a circle keepout), and rules without a condition
(`track_width`, `clearance`, `hole_size`, `annular_width`, `edge_clearance`, `disallow` via kinds) apply since K15.3 (`dru::floors`).

**Regenerating the reference exports and running the corpus.** `tools/kicad-reader/export.sh <board> <out.dsn>`
exports a board with KiCad 10.0.7 (WSL; `--windows` for 10.0.5), `fetch-corpus.sh target/kicad-corpus` and
`export-corpus.sh` make the out-of-repo corpus (K04.7/K04.8, `fixtures/kicad/README.md`). Then
`cargo test --release -p freerouting --test kicad_parity -- --ignored --nocapture corpus_summary` prints one
row per board (`FR_KICAD_ONLY=stem,stem` limits it) and `corpus_timing` the read times
(`FR_KICAD_TIME`); `tools/kicad-reader/frame-check.sh` imports two sessions into KiCad and compares the tracks
(K07.4).

**Coordinate frame** (K00 question 2, K07.4; `crates/freerouting/tests/kicad_frame.rs`): the export declares
`(resolution um 10)` and writes micrometres; the session import computes `nm = KiROUND(1000 * v / resolution)`.
Java's session for `Issue558-dev-board` (`dev-board.kicad10.java.ses`, 231 wires, 35 vias) lands at identical
board coordinates on the board built from the reader and on the one built from the export, with no
incomplete connection left on either; KiCad 10.0.7 imports it and the session the Rust writer produces from the
reader's board to the same 494 tracks, vias and arcs, largest endpoint distance 0.0 nm. Loss: KiCad's
nanometres round to 0.1 um board units, at most 50 nm per coordinate; on this board every pad centre is on
the grid and the largest distance of a track end to its pad centre is 0 nm (171 ends). A finer resolution is
not worth a change until a board shows a connection lost to it.

## The plugin reads the board directly (K10)

**The option.** `board_input` (dialog: "Board input", saved in `route-options.json`) is `kicad_pcb` ("Board
snapshot") or `dsn` ("KiCad's DSN export"). **Which runs by default (K13.7, decided 2026-10-21):** the
Enhanced profile reads the board snapshot (`options.PROFILE_DEFAULTS`), and falls back to the DSN export, saying
so, when the router's reader refuses the board (K13.3, below). The Legacy profile defaults to the DSN: Legacy is
Java's results and Java's plugin reads a DSN (D11). To pick the other, use the Board page's "Board input" choice;
the profile choice moves nothing else. `kicad_pcb` changes only what the router reads; the result still comes
back through `ImportSpecctraSES` (`plugin.import_ses`, which detaches the unlocked tracks first, 9bc7ee6).
KiCad 11's IPC API keeps exactly these two board operations (K01.5). Until K10.5 the default stayed `dsn`
under question 1's rule; K13.6's measurement switched it (below).

**Saved options (K13.7, schema 6).** `to_dict` leaves a default out, so before schema 6 a saved `dsn` was never
written and a file without `board_input` meant `dsn`. Now a file keeps an explicit choice (`dsn` under
Enhanced and `kicad_pcb` under Legacy are written), and a file without `board_input` takes the profile's
default, whatever fix-ups it turns on. K13.7 had one exception, removed by K14.5 (2026-10-21): a file that
named no input but turned on one of the four fix-ups that rewrite the DSN text (`class_widths`,
`fixed_pour_classes` as wires or `auto`, `drop_pour_nets`, `route_on_plane_layers`) took `dsn`, and
`presets.apply` moved a preset without an input to `dsn` with a warning, because the snapshot refused them.
The router's reader applies all four since K14.1-K14.4, so a file or preset without `board_input` keeps the
profile's input (or the one the dialog has), and a file that says `dsn` keeps `dsn`. canary-pwr's saved options
(schema 4: class widths, fixed pour classes, dropped pours, no `board_input`) open on the snapshot. `apply_kicad_rules` is gone from the options (it
decided nothing since K13.1) together with its greyed check box; a file or preset that has it still loads and
is saved again as schema 6 without it. A board with no `route-options.json` opens on the snapshot, with the Enhanced profile's
defaults (`load` returns `profile_defaults(DEFAULT_PROFILE)`, the same as a file that names only the profile;
before K13.7 it returned the dataclass defaults, so a new board started without K09.9's completion sweep
although the profile table above says a new board starts from it). Tests:
`test_board_input_default.py`, `test_input_choice.py` (the dialog), `test_snapshot.py`.

- **The snapshot** (`snapshot.py`, `export_flow.prepare_snapshot`): on KiCad's UI thread, `pcbnew.SaveBoard(path,
  board, True)` (`aSkipSettings`) writes `freerouting-rs/<stem>.kicad_pcb` beside where `<stem>.dsn` would be, so
  the session is named as on the DSN path. A track added in memory is in it. "Route", "Route in GUI" and "Open in
  viewer" all pass it. A pcbnew whose `SaveBoard` lacks `aSkipSettings` is refused (the two-argument form saves
  the project file, the call that aborted KiCad on worker threads for projects older than KiCad 10; the idea is
  KiCadRoutingTools', no code taken).
- **The project's rules (K10.2).** Tested on KiCad 10.0.7 (`test_snapshot_headless.py`): (a) copying
  `<stem>.kicad_pro` from disk misses a net class changed in Board Setup and not yet saved; (b) writing the
  project from the live board with `SettingsManager.SaveProjectCopy(path, board.GetProject())` carries it (a
  `Power` clearance of 0.333 mm, unsaved, is in the snapshot's project; the file on disk is untouched; also
  with projects saved by older KiCad versions). The plugin uses (b) and falls back to (a) with a `WARNING` in
  `plugin.log` saying what is missed. The `.kicad_prl` KiCad writes with the copy is deleted. The
  `<stem>.kicad_dru` is not in memory (KiCad parses the file), so it is copied from disk. `board.IsModified()`
  does not tell: it stayed `False` after an in-memory track and class edit (headless; inside the editor
  unverified), so no "save first" warning is shown; (b) makes it unnecessary for net classes. Unverified:
  whether unsaved edits in Board Setup's custom-rules page reach the `.kicad_dru`, and (b) inside the running
  editor (owner, with K02.8).
- **The reader's report.** The router's own log lists the reader's report lines and warnings; after a Route run
  the plugin copies those (`snapshot.reader_lines`) to `plugin.log`.
- **`apply_kicad_rules` (K10.3)** no longer decides anything (K13.1, decided 2026-10-20: the board's own rules
  reach the router on both inputs) and was dropped with schema 6 (K13.7). Every snapshot Route gets `--apply-design-rules`
  (`export_flow.kicad_dru_args`, as a DSN Route gets `--kicad-dru`), and the CLI applies a `.kicad_pcb`'s
  `.kicad_dru` by default anyway (`--no-design-rules` turns it off; `cli_kicad.rs::the_apply_design_rules_switch_changes_the_class_rules`:
  on the `classes` board the rule "fast tracks" takes the Fast wires from 0.1 mm to 0.3 mm in the session;
  `design_rules_reach_the_board` is the engine-level test). The dialog's check box is gone. "Route in GUI" reads the
  `.kicad_dru` but has no such switch and says so in the log, on the snapshot as on the DSN (no `--kicad-dru`
  either). The minimum track width stays `--router.min_trace_width_um` on both paths. The `-drc` mode does not
  apply the rules. Tests: `test_board_rules_both_inputs.py`.
- **The K09 fix-ups on each input (K10.4 refused four on the snapshot; K14 and K14.5 replaced the refusal).**
  Since K14.5 (2026-10-21) nothing refuses `kicad_pcb`: `options.input_conflicts` and `options.dsn_text_fixups`
  are gone, and so is the `Incompatible board input options` exit of `headless_run.py route`.

  | option | DSN input | board snapshot |
  |---|---|---|
  | `class_widths` (named or `auto`) | text edit of the export (`dsn_fixups`) | `--router.input_fixups.class_widths` (K14.1), `auto` resolved from the board (`dsn_fixups.resolve_on_board`) |
  | `fixed_pour_classes`, `wires` (named or `auto`) | text edit | `--router.input_fixups.fixed_pour_classes` (K14.4): the reader lays the fixed wires from the snapshot's `filled_polygon`s |
  | `fixed_pour_classes`, `engine` (named or `auto`) | `--router.plane_obstacle_net_classes` | the same engine flag, `auto` resolved from the board (`export_flow.snapshot_engine_args`) |
  | `drop_pour_nets` (named or `auto`) | text edit | `--router.input_fixups.drop_pour_nets` (K14.3) |
  | `route_on_plane_layers` | text edit | `--router.input_fixups.route_on_plane_layers` (K14.2) |
  | `edge_and_pad_keepouts` | text edit (keepouts written into the export) | done by the reader itself, so the box is *ignored* (`options.input_ignored`, `input_ignored_note`) |
  | `fix_existing`, rip-up cost, sweep, pass hint, `pin_escape`, `plane_obstacle_all_pours` | board locks and engine flags | the same |

  What covers the keepouts on the snapshot: the outline's copper-to-edge clearance is
  `min_copper_edge_clearance` of the `.kicad_pro`, passed as `router.copper_to_edge_clearance_um`
  (`crates/freerouting-kicad/src/read.rs`, `job::prepare::board_minimums_patch`); the Edge.Cuts and Margin
  shapes the outline does not carry become `edge_clearance` keepouts (`edge_keepouts::edge_rects`,
  `edge_keepouts::edge_keepouts`, Q18.5); the pads' own clearances become `pad_clearance` circles
  (`pad_clearance::read_pad_clearances`, `pad_clearance::keepouts`, K13.1). The dialog shows a note under
  the board input (`Dialog.input_note`, text from `options.input_note`, refreshed by `on_input_changed` on the
  input choice and the keepouts box, and by `show_options`). It holds the ignored-keepouts line on the snapshot
  and nothing else; before K14.5 it also named the fix-ups that kept the snapshot from running. The note
  wraps to the board input's laid-out width (`wrap_input_note`, again on the choice's `EVT_SIZE`), not a fixed
  520 px: the owner's run (2026-10-21) found the fixed width running off a dialog that was fitted while the
  note was empty. The snapshot
  run's `plugin.log` carries the same line (`snapshot.notes`), and `headless_run.py route` prints it with
  `Note:`; the run goes ahead. When the reader refuses the snapshot and the DSN runs instead, the box applies
  again and the line is dropped from the log. Tests: `test_snapshot.py`
  (`test_the_defaults_are_accepted_on_the_snapshot_and_the_keepouts_are_ignored`,
  `test_headless_route_takes_the_default_options_on_a_snapshot_and_prints_the_note`),
  `test_input_choice.py` (`test_the_defaults_route_the_snapshot_and_the_dialog_says_the_keepouts_are_ignored`,
  `test_the_note_follows_the_input_and_the_keepouts_box`,
  `test_a_board_saved_with_a_fixup_and_no_choice_opens_on_the_snapshot_and_routes_there`,
  `test_canary_pwrs_saved_options_open_on_the_snapshot_and_route_there`), `test_board_input_default.py`
  (`test_the_form_accepts_each_fixup_on_either_input`, `test_the_note_has_no_dsn_line_for_any_fixup`,
  the saved-file and preset tests) and `test_snapshot_fixups.py`. The rule-area fix-up (K01.2) does nothing on
  the snapshot (the reader maps rule areas by flags); the run log says so once.
- **Presets** (see above) work with both inputs.
- **Fallback to the DSN (K13.3, `input_choice.py`).** When the reader refuses the snapshot, the run uses the
  DSN export instead and routes that, so the input choice never fails a run. Two refusals are recognised: the
  snapshot's `(version N)` below 20201115 (before routing), and the router's `There was a parse error while
  reading board file at '<name>': <reason>` error line with exit 1 and no session (the DSN is exported and routed
  once; a failure without that line is not retried, and a `SaveBoard` failure is still an error). The run log gets
  one `input: ...` line, and `<stem>.result.json` gets `input` (`kicad_pcb`|`dsn`) and `input_fallback_reason`
  (null when none), which the result summary shows as `Input: ...`. The saved option stays what the user chose.
  `headless_run.py` has no fallback. Tests: `test_input_choice.py`.
- **Tests:** `test_snapshot.py` (paths and argument lists for both inputs and non-ASCII names, the staged
  files, the fall-back, the options check), `test_route_dialog.py` (`test_route_with_the_snapshot_...`,
  the detaching import, refusal, `fix_existing`), `test_snapshot_headless.py` (real pcbnew and CLI: the track in
  memory, the unsaved class clearance, the unchanged board's DRC equals the snapshot's, route and
  `ImportSpecctraSES` with no error), `headless_run.py snapshot`.
- **K10.5, measured against the DSN path** (`tools/kicad-reader/plugin-input-ab.sh`, one arm per call; 2026-10-20,
  release build of `c8b91b7`, WSL KiCad 10.0.7, unlimited passes, 2 threads, optimizer on, `edge_and_pad_keepouts`
  off; KiCad DRC errors with zones refilled; time = the plugin's export or snapshot + the router's load, in s; the
  full table, by violation type and with route times, is in K10.5's done note):

  | Board | dsn: unconnected / errors | kicad_pcb: unconnected / errors | dsn: time | kicad_pcb: time |
  |---|---|---|---|---|
  | `ecc83-pp` | 0 / 0 | 0 / 0 | 0.33 | 0.31 |
  | `interf_u` | 0 / 2 | 0 / 2 | 0.49 | 0.54 |
  | `complex_hierarchy` | 0 / 0 | 0 / 0 | 0.66 | 0.73 |
  | `kit-dev-coldfire-xilinx_5213` | 0 / 0 | 0 / 0 | 0.90 | 1.00 |
  | `CM5_MINIMA_3` | 0 / 195 | 0 / 4 | 1.34 | 1.68 |
  | `canary-pwr` (canary `1a7df77`) | 42 / 26 | 38 / 22 | 0.67 | 0.71 |
  | `One-Air-Max` | 3 / 228 | 3 / 229 | 1.06 | 1.11 |
  | `video` | 0 / 1 | 0 / 0 | 3.69 | 3.12 |

  `vme-wren` and `jetson-agx-thor-baseboard` were not measured: their routes take over 3 minutes (decided
  2026-10-20: measurement runs stay a few minutes). Where the arms differ (K10.5): `CM5`'s 191 extra clearance
  errors on the DSN arm are its diff-pair arcs exported as one chord each (K06.5; the reader follows them);
  `video`'s one is an existing 0.1992 mm gap from the export's `%.6g` (K00 question 2); `canary-pwr`'s 4 and 4 are
  routing noise inside canary's ±10 (K09). `One-Air-Max`'s one extra error on the snapshot is a violation the board
  ships with (existing `SYS`/`VBAT` tracks on `In1.Cu` 0.091 mm apart), which KiCad counts against one more segment
  pair after the reader's 0.1 µm rounding than after the export's `%.6g` (K07.6's precision difference). Under
  question 1's rule (decided 2026-10-18: switch only if `kicad_pcb` grades the same or better on every K10.5
  board) **the default stayed `dsn`** then, held back by `One-Air-Max` and the two unmeasured boards (K13.6 and K13.7 switched it on
  2026-10-21: `docs/roadmap/prereg/K13.json`, nine judged rows with 0 manufacturing breaches and 68 unconnected against `dsn`'s 73;
  `vme-wren` and `jetson-agx-thor-baseboard` accepted unjudged). The time to
  prepare and load is the same within 0.1 s except `CM5` (+0.34 s, the arcs) and `video` (-0.57 s). Smoke run
  before (2026-10-17, dev board `Issue558`, one pass, debug build): both arms 11 unconnected, 12 copper-to-edge
  and 4 hole-clearance errors.

**K10.6: when to write to the open board.** A live write (KiCad Routing Tools' `board.Add(PCB_TRACK)` plus
`pcbnew.Refresh()`) is not worth doing while a session carries everything a stage produces: tracks, vias and
their widths. It is taken up when a stage first produces something a session cannot carry, and not before:
teardrops, via tenting, footprint or pin swaps (the editor's pin swap, I03/G09, is the one that exists today and
works on the router's own board, not KiCad's), and plane refills after added copper (Q10 watches pours cut by
added traces; KiCad's own refill does it today). Such an item must also decide undo: whether KiCad's undo covers
either path is unverified (the detaching import, 9bc7ee6, lets the action-plugin runner record the removed
tracks, so Undo restores them; a `board.Add` would need its own commit). Until then there is no live write.

## Direct `.kicad_pcb` writer (K08)

`freerouting-rs -de board.kicad_pcb -do routed.kicad_pcb` (and `freerouting::io::kicad::write_kicad_board`)
writes the routed board into a **copy** of the `.kicad_pcb` without `ImportSpecctraSES`, so together with
the K07 reader neither direction needs KiCad's Specctra code (`crates/freerouting-kicad/src/write/`).

**What it changes.** Only copper blocks. The file's `(segment)`, `(arc)` and `(via)` items are compared
with the routed board by exact geometry in 0.1 um board units: an original segment or arc stays, byte for
byte, when the board's traces of the same layer, net and width still cover it; a via stays when the board
has a via with its position, net, span, diameter and drill; locked items and copper the reader did not
route (no net, not a copper layer) always stay. Everything else is cut out with its line. What the board
has beyond the kept copper is added before the final `)` as `(segment)` and `(via [blind|buried|micro])`
blocks in KiCad 10's multi-line layout, with the file's net dialect (`(net 3)` or `(net "GND")`), its uuid
token (`tstamp` in KiCad 7 files) and canonical layer names (`In1.Cu`); new vias carry no tenting token, so
they take the board's `(setup (tenting ...))`. Coordinates are written from nanometres by integer
formatting; an added end that is the board image of an original track end or via centre is written at
that original's exact nanometres. UUIDs are version-8 FNV-1a hashes of the input's own hash and the item,
so the same input and routed board give the same bytes (no `uuid4`: KiCad tie-breaks equal-priority zones
by KIID, so random UUIDs made KiCadRoutingTools' fills unstable).

**What it never changes.** Footprints, pads, zones (fills go stale; grade with `kicad-cli pcb drc
--refill-zones`), graphics, text, `(setup)`, the `.kicad_pro` and `.kicad_dru`, and the input file itself:
`-do` naming the input (after canonicalization) exits 2, as does a `.kicad_pcb` output of a DSN input, and
`write_kicad_board_checked` refuses a file whose FNV-1a hash differs from the one recorded when the board
was read. The output is parsed again and must have balanced parentheses before it is renamed into place.
Unlike KRT's writer it does not move copper text, strip Edge.Cuts lines or touch `.kicad_pro` DRC floors.

**Graded by KiCad** (`tools/kicad-reader/write-check.sh <board> <session.ses> [outdir]`, K08.5;
2026-10-17, WSL KiCad 10.0.7, release CLI of this tree, `pgrep -x kicad; pgrep -x pcbnew` empty before
and after). Path A is `LoadBoard` + `ImportSpecctraSES` + `SaveBoard`, path B the writer from the same
session; both are graded by `kicad-cli pcb drc --refill-zones --severity-all`:

| board | session | A: DRC by type, unconnected | B | copper |
|---|---|---|---|---|
| `Issue558-dev-board` | Java `a773a70ec` (`dev-board.kicad10.java.ses`, 231 wires, 35 vias) | copper_edge_clearance 22, hole_clearance 4, lib_footprint_issues 4, lib_footprint_mismatch 16, silk_edge_clearance 5; 0 unconnected | the same | 459 segments and 35 vias each |
| `Issue180-Test` | Rust route of the reader's board (79 wires, 3 vias: the router added nothing) | lib_footprint_issues 24, lib_footprint_mismatch 1; 0 unconnected | the same | B is the input byte for byte; A rewrote the unlocked tracks as 110 overlapping pieces of the same 102 segments (20.3 mm more length, every piece on a B segment) |
| `canary-pwr` | Rust route of the reader's board (212 s, 374 wires, 89 vias, 94 unrouted) | copper_edge_clearance 2, lib_footprint_issues 4, lib_footprint_mismatch 6, starved_thermal 1, track_dangling 2, via_dangling 30; 70 unconnected | the same | 813 segments and 130 vias each, 1154.1893 mm of track each |

No board here has track arcs; the writer keeps an arc the router left alone and replaces a changed one by
segments, where `ImportSpecctraSES` gets every arc as the session's chords.

**Using it from the plugin on KiCad 11** (not done; the plugin still imports the SES, K02). KiCad master
removed SWIG (K01.5), so `pcbnew.ImportSpecctraSES` goes away. The plugin could instead save the board,
run `freerouting-rs -de <board>.kicad_pcb -do <board>.routed.kicad_pcb` (the capabilities JSON lists
`.kicad_pcb` in `output_formats`), and have KiCad open or revert to the routed copy; through the IPC API
it would have to reload the board from disk, because the writer edits the file, not the open document.
The plugin must replace the board itself (the writer never overwrites its input) and only after the user
accepts the result. [K10](../roadmap/tasks/archive/K10-plugin-direct-board-read.md) added the direct read as a plugin
option and kept the SES import; [K11](../roadmap/tasks/K11-kicad11-ipc-plugin.md) (planned) is the KiCad 11
plugin on the IPC API.

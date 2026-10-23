# Board report: power-control-unit rev 2.0 (2026-10-19 to 2026-10-21)

A real board routed end to end through the direct `.kicad_pcb` input, and what it showed. The
board belongs to another repository (`../power-control-unit`, `hardware/rev2.0`). The two repro
boards are in [`pcu-rev2/`](pcu-rev2/).

## The board

- **Size and stack:** 180 x 120 mm, 4 layers.
  - F.Cu: parts, signals and pours.
  - In1.Cu: `power`, BUS_GND plane.
  - In2.Cu: `power`, V_BUS_24V plane.
  - B.Cu: signals.
  - All parts are on top.
- **Size of the job:** 500 footprints, about 1650 SMD pins, 393 nets, 1208 to 1339 connections.
  An LQFP-144, QFN-24/36, eight HTSSOP-20 eFuses, 10 PowerPAK FETs.
- **Input:** read directly as `.kicad_pcb` from KiCad 10.0.6, with the `.kicad_pro` (seven net
  classes) and a `.kicad_dru` (12 fab-minimum rules) next to it.
- **Placement and pours:** placed by a script.
  - 20 F.Cu pours for the 20 A path, in net classes `Pour1A` / `Pour3A`.
  - Full-board In1/In2 zones.
  - 6 rule areas: keepouts around three NPTH holes and three fiducials.
  - 155 vias placed before routing (thermal arrays, 24 V via fields).
- **Binaries:** the routing runs used `freerouting 0.1.0 (git 8c952e775dfc)`. The issues below were
  re-checked on `084f3a9c93a5` where the note says so.

### Flags that worked best

```
--router.plane_obstacle_net_classes=Pour1A,Pour3A
--router.cleanup.remove_stubs=true
--router.copper_to_edge_clearance_um=400
--router.hole_clearance_um=260
--router.min_trace_width_um=130
```

### Runs

Unrouted is the router's final count. KiCad is `kicad-cli pcb drc --refill-zones` unconnected items
after adding F.Cu/B.Cu GND fills.

| Run | Board change | Flags | Time | Unrouted | KiCad |
|---|---|---|---|---|---|
| v5 | 0.4 mm part spacing | no pour obstacles | 40 min (timeout) | 185 | 192 |
| v7 | 0.6 mm spacing, fan-out rings around fine-pitch ICs | all but min width | 60 min (timeout) | 186 | 154 |
| v8 | v7 + a VCC_3V3 island on In2 | as v7 | 60 min | 205 | - |
| v8n | v8 | no pour obstacles | 60 min | 239 | - |
| v9 | v8, pours cut back from foreign courtyards | all | 60 min | 222 | - |
| v9n | v9 without the island | all | 60 min | 188 | - |
| v7c | continuation: input = v7's output | all | 22.5 min (stopped itself) | 82 | 84 |
| v7c2 | continuation of v7c | all | 21 min (stopped itself) | 79 | 80 |
| v9nc | continuation of v9n | all | 30 min (stopped itself) | 168 | - |

- **Continuing a run** on its own output was the single most effective step: 186 down to 82.
- **Passes** took 2.5 to 5 min each.
- **Pour obstacles:** without `plane_obstacle_net_classes` (v5, v8n), signal traces crossed the
  power pours. KiCad's refill then split the pours, which shows as the larger KiCad count for v5.

## Bugs

### 1. A pad at a sub-board-unit position stops every connection on the board

- **What happens:** footprint C65 (`Capacitor_SMD:CP_Elec_10x10.5`, two roundrect pads 4.4 x
  2.5 mm, `roundrect_rratio 0.1`) at `(at 57.251006 105.242039)`. Every connection to it fails
  with "Failed to route connection between ..., because the maze search algorithm could not be
  created."
- **On the full board:** with this one footprint present, all 1484 connection attempts failed on
  every pass (`routed 0`, 1208 incomplete), not only C65's.
- **Repro:**
  - [`pcu-rev2/c65-nm-position.kicad_pcb`](pcu-rev2/c65-nm-position.kicad_pcb): two
    footprints, two nets, no `.kicad_pro`, `freerouting-rs -de c65-nm-position.kicad_pcb -do out.kicad_pcb`.
    It gives `routed 0, failed 4` and 72 "could not be created" lines.
  - [`pcu-rev2/c65-snapped.kicad_pcb`](pcu-rev2/c65-snapped.kicad_pcb) is the same file with
    y = 105.242. It gives `routed 1, failed 0`.
- **Which coordinates fail:**

  | C65 position (mm) | Result |
  |---|---|
  | y = 105.242039, 100.000039, 75.242039 | fails |
  | y = 105.242, 105.242001 | routes |
  | x = 57.000006, 57.251006 (with a round y) | routes |

  So it depends on the fraction of a 0.1 um board unit (0.39 fails, 0.01 doesn't), probably in the
  rounding of the roundrect outline.
- **Still at `084f3a9`:** yes (re-run 2026-10-21, same counts).
- **Why it matters:** KiCad stores nanometres. Any scripted or rotated placement produces such
  positions, and the failure message names neither the item nor the cause.

### 2. Tracks narrower than the class width and the board minimum

- **What happens:** Default class width 0.2 mm. v5's output had 495 segments under 0.2 mm, mostly on
  BUS_GND, SCL and SDA:

  | Width (mm) | Segments |
  |---|---|
  | 0.15 | 476 |
  | 0.12 | 11 |
  | 0.10 | 5 |

  It also had odd widths: 0.2498, 0.2502, 0.2538, 0.2792, 0.3554 mm.
- **Effect:** KiCad DRC flagged 16 `track_width` < 0.127 mm.
- **Settings:** `automatic_neckdown = true` and `min_trace_width_um` is unset by default; the
  `.kicad_dru` minimum doesn't reach the router (bug 3).
- **Still at `084f3a9`:** see the note at the end.

### 3. Unconditional `.kicad_dru` rules are reported UNSUPPORTED

- **What happens:** all 12 rules of a standard fab-minimum `.kicad_dru` are reported as
  `UNSUPPORTED (rule without a condition)`: track width, clearance, annular ring, hole size, hole
  clearance, edge clearance (outer and inner), blind/buried/micro vias, plated slots.
- **Why it matters:** a rule without a condition is the simplest case, a board-wide minimum.
- **Edge clearance:** it stays at the 250 um default. Board Setup said 0.4 mm and the dru 0.381 mm
  (inner), so the run needed `--router.copper_to_edge_clearance_um=400`.
- **Hole clearance:** the board-wide value needed `--router.hole_clearance_um=260`.
- **Still at `084f3a9`:** yes, 12 of 12.

### 4. DRC counts items KiCad doesn't

`freerouting-rs -de board.kicad_pcb -drc report.json --router.hole_clearance_um=260` on the
committed board, at `084f3a9`:

| Type | freerouting-rs | KiCad 10 |
|---|---|---|
| holeClearance | 165 (121 same net) | 8 (different nets, USB-C pegs) |
| via_dangling | 117 | 0 |
| track_dangling | 16 | 2 |
| clearance | 4 | 0 |
| isolated_island_dead_copper | 20 | - |
| unconnected | 78 | 80 |

- **Same-net hole clearance:** these are thermal vias in BUS_GND exposed pads ("Hole clearance
  violation between Pin [BUS_GND] and Via [BUS_GND]"). KiCad applies hole clearance between
  different nets only.
- **via_dangling:** vias that join an SMD pad on F.Cu to their net's inner plane, reported as
  "connected on only one layer".
- **During routing:** the router carries these as `violations` for the whole run (142 at the
  start, 217 with hole clearance on), so they are in its score from the first pass.

### 5. No stop when nothing routes

- **What happens:** with bug 1 present, the zoned board ran pass after pass at `routed 0`,
  `failed 1484` and an unchanged score (71.03) until the 40 min timeout.
- **Comparison:** the same placement without zones stopped after 18 passes ("score has not
  improved enough"). The stop rule seems not to fire while violations are present.

## Usability

- **Failure messages use internal ids.** "Failed to route connection between pin #9 of component
  #385, ..." can't be matched to the board. Component numbers didn't follow the footprints'
  order in the file either. Reference designators and pad numbers would make the 600-odd
  messages of a run usable, as would the Q06 verdicts keyed by reference.
- **An off-board footprint silently drops the edge clearance.** A fiducial left at (0, 0), outside
  the outline, gave "Copper-to-edge clearance capped from 2500 to 0 board units because a pin is
  closer to the board outline." for the whole board, with no name. Naming the item, or leaving
  off-board items out, would help.
- **stdin.** The CLI reads stop requests from standard input (documented in `--help`). In a
  shell `while read ...; do freerouting-rs ...; done` it consumed the loop's input. A flag to
  ignore stdin, or a sentence in that help section, would save the next person.
- **`--threads 24`:** the autorouter stage ran on one core (top: 92-100 % CPU for 60 min); only the
  optimizer uses the threads. The help text reads as if both do.
- **Pad-local clearances** aren't read (already documented). A fiducial's 0.6 mm local clearance
  was violated by a GND trace; rule areas around the fiducials worked around it.

## What worked

- **Direct input and output:** reading `.kicad_pcb` with the `.kicad_pro` net classes, and
  writing a routed `.kicad_pcb` copy, with no DSN round trip.
- **Zones as planes, rule areas as keepouts.**
- **`plane_obstacle_net_classes`:** this kept signal traces out of the power pours, and with it
  the pours survived KiCad's refill.
- **Scripting:** `--progress json`, `--result-json`, `--dump-settings` (which shows where every
  setting came from), and `--timeout` keeping the best board.
- **Stub cleanup:** `cleanup.remove_stubs` left 0 KiCad `via_dangling` (69 without it on v5).
- **Continuation runs:** see the run table.
- **The docs:** `DIVERGENCES.md` and `kicad-plugin.md` answered each question that came up.

## Review at main (2026-10-21)

Re-checked at `084f3a9` (main `2d38fdd` changes only docs), with the run logs and boards copied to
`target/orchestrator/pcu-rev2/`. The findings are in [K15](../roadmap/tasks/archive/K15-pcu-board-findings.md) and [Q23](../roadmap/tasks/archive/Q23-restart-from-the-best-board.md).

- **Bug 1** reproduces: 72 "could not be created" lines on the repro. It also fails after a DSN
  round trip, so it is in the engine. The DSN it writes has the board boundary
  `(rect pcb 39800 -150200 -214748264.9 -214748264.9)`, and a route of that DSN reports 0
  unrouted with `(place C65 NaN NaN ...)` in the SES. The cause is C65's F.Fab circle outline,
  not its pads. [K15.1, K15.2](../roadmap/tasks/archive/K15-pcu-board-findings.md).
- **Bug 2**: the runs' binary `8c952e7` predates Q18 (`b98074e`), which made Board Setup
  minimums router floors. On main, `--dump-settings` shows
  `board_minimums.track_width_um = 200.0 (kicad_pro board minimums)`. The 0.15/0.12/0.10 mm
  widths are the micro neck-down's candidates, and the odd widths are pad widths less 0.2 µm.
  The report's "see the note at the end" for bug 2 had no note; this is it. [K15.4](../roadmap/tasks/archive/K15-pcu-board-findings.md).
- **Bug 3**: 9 of the 12 rules fail as "rule without a condition". Of the other 3, one fails on
  `annular_width` ("no DSN form") and two on condition terms (`A.Hole_Size_X > 0.4mm`, `!=`). On
  main, edge clearance comes from the `.kicad_pro` (0.5 mm on the committed board).
  [K15.3](../roadmap/tasks/archive/K15-pcu-board-findings.md).
- **Bug 4**: same counts on main (holeClearance 165 with 121 same net, via_dangling 117,
  track_dangling 15, clearance 4, islands 20, unconnected 77). `-drc` ignores router settings,
  so `--router.hole_clearance_um=260` had no effect there. [K15.5](../roadmap/tasks/archive/K15-pcu-board-findings.md).
- **Bug 5** is not borne out by the logs. `route_v4` (score 71.03, 1484 failures a pass) stopped
  itself after 18 passes of about 1 s each. The 40-minute timeout was `route_v5`, which routed
  normally. [K15.6](../roadmap/tasks/archive/K15-pcu-board-findings.md).
- **Runs**: v9nc ended at 98, its restored best board; 168 was its last pass. The continuation's
  gain is a fanout on the routed board (186 -> 105 on v7c) and then pass 1 (-> 82).
  [Q23](../roadmap/tasks/archive/Q23-restart-from-the-best-board.md).
- **Usability**: [K15.7–K15.9](../roadmap/tasks/archive/K15-pcu-board-findings.md). Pad-local clearances are read since `5926b61`, which the
  fiducials use. A footprint-level clearance is read too since K15.9 (a pad's own value wins).

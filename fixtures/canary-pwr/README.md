# canary-pwr fixtures

Dan's CANary power board (`hardware/canary-pwr-rev1.0` in the private
`danbrunski/canary` repository): 4 layers, with In1 and In2 as planes, so
routing is on F.Cu and B.Cu. The board has hand-laid power pours, Kelvin
pairs and gate drives, locked and exported as `type fix`. Dan approved
committing these on 2026-10-15. They were handed over by the CANary layout
session.

**Exporter:** canary's `hardware/tools/dsn_export.py` at canary commit
`bf594116` (git blob `cf4cbf60f5c740504d3943b3a8913e777b073ec1`, file sha256
`08ccf583480bd724…`). The same commit adds `hardware/tools/autoroute.sh`
(refill, export, Freerouting, import, DRC) and `hardware/tools/ses_import.py`.
Per fixture:
- `pours2`, `pours3` and `pours-as-planes` were made by code equivalent to
  `bf594116`, with the edge and fiducial keepouts already in. That commit
  only adds docstring text.
- `pours1` predates the keepouts: the same script without the edge and
  fiducial clearance step, and with the board's old same-net via spacing.
- The 17:47 unpoured DSN is a plain KiCad export
  (`pcbnew.ExportSpecctraDSN`) with no post-processing.

| File | sha256 (prefix) | What it is | Used by |
|---|---|---|---|
| `canary-pwr-pours1.dsn` | `da76e69c` | Pours turned into 113 fixed path wires covering their fills; GND outer planes dropped; the fan keepout dropped; Power_High, Switch_Node and Power at 0.3 mm, GND at 0.4 mm | variant set (F02.5, Q04, Q05) |
| `canary-pwr-pours2.dsn` | `b2547bec` | pours1 plus board-edge keepout strips, fiducial circle keepouts, and same-net via spacing ≥ 0.25 mm. The best result so far | variant set |
| `canary-pwr-pours3.dsn` | `3113a991` | pours2 with taps at 0.2 mm and GND at 0.3 mm. A small input change that made routing worse | variant set; chaos evidence |
| `canary-pwr-pours-as-planes.dsn` | `5d7c28a8` | The same board with the pours left as KiCad's planes, with no fixed wires | Q01 (planes as obstacles to foreign nets) |
| `canary-pwr-2026-10-15-1747-unpoured.dsn` | `34cbcd43` | The older placement before any power copper. It has the net-class bug (VCAP, +12V_RAW and +5V_SHUNT at 0.2 mm) and the full fan keepout | Q03 (the "1.5 mm can't reach a 0.25 mm pin" cases) |
| `*.a773a70ec.ses` | — | Sessions routed by the Java fork at `a773a70ec` (deterministic) | reference outputs |
| `canary-pwr-9511d9a.dsn` | `65186b28` | The placement at canary `9511d9a` (handed over 2026-10-15 by the CANary layout session, canary-09) | the board on hold for freerouting-rs |
| `canary-pwr-9511d9a-addonly.dsn` | `05fd3b0b` | The same placement with the routing of its best board so far (36 unconnected, `routed36`) added as fixed wires (1177 wires against 179, every one `type fix`), so a router can only add; the result to beat is 36 unconnected | add-only routing (Q04, Q05) |
| `canary-pwr-9511d9a-in2-open.dsn` | `17020c17` | 9511d9a exported with In2 (Power.Cu) open for signals: `dsn_export.py --signal-layers Power.Cu --width Power_High=0.3 --width Switch_Node=0.3 --width Power=0.3 --width GND=0.4` | Q09.5 baseline |
| `canary-pwr-9511d9a-in2-open-wirekeepout.dsn` | `7b876b9d` | in2-open plus `(wire_keepout)` over the +12V_BUS and +5V_PI islands on Power.Cu. Not routed yet | Q09 (trace-only keepouts) |
| `*.46bf9e2ef.ses` | `4dc9eedf` (9511d9a), `571d1e01` (addonly), `a8848d1e` (in2-open) | Sessions routed by the Java fork at `46bf9e2ef` (byte-identical to `a773a70ec` while `router.min_trace_width_um` is unset; these runs set it) | reference outputs |

Java results at `a773a70ec` (Freerouting's own count of unrouted / violations,
then unconnected items after importing into KiCad):

| Board | Flags | Freerouting | KiCad |
|---|---|---|---|
| pours1 | `-mp 10` | 120 / 70 | 61 unconnected |
| pours2 | `-mp 1 --router.scoring.start_ripup_costs=1500` | 106 / 42 | 48 unconnected |
| pours3 | same as pours2 | 114 / 42 | 55 unconnected |

Java results at `46bf9e2ef` on the 9511d9a boards (flags as above; "KiCad" is the
grading below):

| Board | Flags | Freerouting | KiCad |
|---|---|---|---|
| 9511d9a | `-mp 1 --gui.enabled=false --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150` | 102 / 40 (all 40 in the input); 1 `WORK_LIMIT_CUTOFF` (`pull_tight`) and 3 `normalizeTraces` 2000-iteration guards, so not a zero-cut-off parity target | 41 unconnected |
| 9511d9a-in2-open | same | 94 unrouted | 28 unconnected, 2 clearance, 8 starved thermals, 0 vias in pads: not a valid result (clearance errors) |
| 9511d9a-addonly | not recorded in the handover | not recorded | the board to beat has 36 unconnected |

**Grading** (canary-09 grades): `ses_import.py`, then `kicad-cli pcb drc --schematic-parity
--refill-zones --severity-all`, then `via_in_pad.py` must report 0. A valid result has no
clearance, track width, annular ring or via-in-pad errors, and no tracks on In1/In2.

The Java route references for these boards in `fixtures/reference/route/canary-pwr/` are
bounded runs with the harness's settings (optimizer off, `route-bounds.tsv`, no extra
flags), not the flags above. They are not zero-cut-off parity targets.

## Lessons from the layout session

From the CANary layout session (canary-09) on the 9511d9a board, 2026-10-15:
1. One via in a pad is a hard failure for the whole result.
2. Routing is capacity-bound near U1 (the MCU): opening In2 for signals took the
   unconnected count from 47 to 30, so trace-only keepouts (Q09) are worth having.
3. GND fan-out dominates the failures. Pads that touch a signal-layer plane may not
   count as connected; check pad-to-conduction-area contact on signal layers.
4. The router gives up on trivial links; a per-connection report (Q03) would show why.
5. Add-only beats rip-up: pass 1 is the best pass, and freezing the routing and
   routing again took 43 to 39 unconnected (Q04, Q05: an add-only sweep or negotiated
   congestion).
6. The DSN loses KiCad's minimums and custom rules (the K series reader would keep them).
7. Fixed slab-capsule copper makes `normalizeTraces` oscillate: never normalize fixed
   copper.

Known about these boards:
- **Violations already in the inputs.** These come from KiCad's 0.13 mm
  in-courtyard rule for fine-pitch parts, which a DSN can't express:
  pad to pad inside Q2–Q7 and U3, and hand-laid copper near those pins.
  Routing adds none.
- **Pass 1 is always best.** Java's multi-pass ripup never beats pass 1 on
  this board; ripped blockers can't be restored. See A13, Q04 and Q05 in
  `docs/roadmap/tasks/`.
- **The normalization guard fires.** `normalizeTraces` hits its
  2000-iteration guard around the fixed copper on six nets.
- **Results are chaotic.** Small input changes move completion by about
  ±10 connections, so compare across the variants rather than repeat runs.
  Routing is deterministic, so repeat runs are identical.

The Java load and route references are in `fixtures/reference/load/canary-pwr/` and
`fixtures/reference/route/canary-pwr/`, generated at `a773a70ec` with
`tools/java-reference/run.sh <a773a70ec worktree> load` and `route` (J01.5).

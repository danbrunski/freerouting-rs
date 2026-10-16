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

Java results at `a773a70ec` (Freerouting's own count of unrouted / violations,
then unconnected items after importing into KiCad):

| Board | Flags | Freerouting | KiCad |
|---|---|---|---|
| pours1 | `-mp 10` | 120 / 70 | 61 unconnected |
| pours2 | `-mp 1 --router.scoring.start_ripup_costs=1500` | 106 / 42 | 48 unconnected |
| pours3 | same as pours2 | 114 / 42 | 55 unconnected |

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

The Java load references are in `fixtures/reference/load/canary-pwr/`,
generated at `cda540f` with `tools/java-reference/run.sh <cda540f checkout>
load 100 00:02:00 canary-pwr`.

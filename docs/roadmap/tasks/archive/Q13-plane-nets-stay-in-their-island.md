# Q13 — A plane net's traces stay inside its own plane

Opened 2026-10-17, from canary-7a's `canary-pwr` placement runs. Q01.7 (Java
`7f56b062d`) keeps foreign nets off a power-type layer. A net with a plane on that
layer may still route anywhere on the layer, even when its plane is only a small
island.

In placement runs a and a+b, `+5V_PI` went on `Power.Cu` along
(112.2,54.0)→(114.0,55.9)→(114.0,60.2) and (108.2,53.2)→(105.7,53.9). Both are
outside the `+5V_PI` island (x 107.3–112.2, y 45.2–55.0) and cross the `+12V_BUS`
pour on the same layer. canary's `inner_tracks.py` refuses the result.

The port has Java's rule: `AutorouteControl::own_plane_layers` is one flag per layer
(`autoroute/control.rs`). The maze search tests that flag, not the room's position
(`autoroute/session/search.rs`, the power-layer check).

Prerequisites: Q01.7; N01 (the change goes into profile `enhanced`, and `legacy` keeps Java's
rule).

## Decisions made here

- **On a power layer, a trace of net N may only lie inside the outline of one of
  N's own conduction areas on that layer.** Everything else on that layer is an
  obstacle to N, the same way Q01.7 treats foreign nets. A via through the layer is
  still allowed, as it is today.
- **Setting:** `router.power_layer_traces = layer | own_plane`. `layer` is Java's rule
  and stays under profile `legacy`. If Q13.3 measures the change as an improvement,
  `own_plane` becomes `enhanced`'s default (N05). It is a correctness fix, since KiCad
  shorts or refuses the other result, so the expected outcome is acceptance.

## Checklist

- [x] **Q13.1 — The rule in the maze search.** With `own_plane`, a room on a power
  layer is enterable by net N only where it lies within N's own conduction area on
  that layer. Rooms are clipped to the area, or the area's complement is added as
  an obstacle, whichever keeps the search's room shapes sound.
  - **Accept:**
    - a fixture with two islands on a power layer: net A's trace stays inside A's
      island, and the connection routes on a signal layer or reports unrouted;
    - with `layer`, the A14 exact set is unchanged.

  *Done 2026-10-17:* setting `router.power_layer_traces` (C02 table, `opt`, unset = `layer`; an
  unknown value fails `prepare_job`, naming the key), module
  `crates/freerouting/src/autoroute/power_layer_traces.rs`. Neither clipping nor a complement
  obstacle: power layers are never active, so on them the search only makes the one-room hop from
  the entry to a target door (`expand_to_target_doors`), and under `own_plane` that hop is
  expanded only when the entry segment and the connection point lie in one convex piece of one of
  the net's planes on that layer, in the board's own geometry
  (`AutorouteControl::own_plane_areas`, set in `init_net`;
  `MazeSearch::trace_stays_in_own_plane`). Room shapes stay Java's. Tests in
  `crates/freerouting/tests/power_layer_traces.rs` (5 green), fixture
  `tests/data/quality/q13-two-islands.dsn` (In1.Cu power, islands PA and PB, each net with a
  through-hole pin between the islands): `java_s_rule_leaves_pb_s_island_on_the_power_layer`
  (unset and `layer` identical: PB on In1.Cu from B2 at (15000, -4000) to (21957, -3932), the
  check flags it) and `own_plane_keeps_every_trace_inside_its_island` (0 unrouted, no trace on
  In1.Cu, PB routes on B.Cu to one via at (21498, -10498) inside its island). Fixture
  `q13-via-at-border.dsn` and `own_plane_refuses_the_hop_from_a_via_just_outside_the_island`
  pin the case Q13.3's first run found (a via 50 µm outside the island: `layer` lays a trace from
  it into the island, `own_plane` adds a via inside instead). With `layer` the A14 gate is
  unchanged: release, `PARITY_SET` = connection-parity + benchmark (`a14-set.txt`, as Q14 used),
  `PARITY_SUBDIRS=1`: 16 exact + 6 cutoff boards pass, **22 of 22 SES byte-identical**, D8
  breaches Rust adds beyond Java's 0, 2396.9 s; plus `min_fr_test`,
  `Issue269-NoViasOnPowerPlanes` and `TestSensel` (`PARITY_BOARDS`): 3 of 3 exact and
  byte-identical. 25 of 25.
- [x] **Q13.2 — A check that finds it.** The parity harness's power-layer report
  (`board_parity/mod.rs`, "trace of <net> on power layer") also reports a trace of
  the plane's own net that leaves the plane's outline.
  - **Accept:** the check flags the canary runs' `+5V_PI` tracks, given their SES,
    and passes on the control.

  *Done 2026-10-17:* the engine decides it: `autoroute::power_layer_traces::power_layer_breaches`
  lists every trace on a power layer whose net has no plane there (`Foreign`, Q01.7) or whose
  corners and segment middles are not all inside one of its net's planes on that layer
  (`OutsideOwnPlane`). The harness's `foreign_traces_on_power_layers` (`board_parity/mod.rs`) now
  calls it and adds `trace of <net> outside its plane on power layer <layer>` for new traces; the
  summary counts only lines Java's board does not have, as before. Test
  `autoroute_board_parity::d8_flags_a_plane_net_trace_outside_its_plane` (green): the default
  plugin run at canary `e21ee05e` (its SES against its input with the earlier routing's 691
  `type route` wires and 92 vias taken out, `tests/data/canary-pwr-q13/plugin-e21ee05e-unrouted.dsn`)
  and canary-7a's run a+b (`exp-ab-addonly.dsn` + its Java `7f56b062d` add-only SES) both give
  exactly `trace of +5V_PI outside its plane on power layer Power.Cu`; the control, Q10's pass-1
  SES of `canary-pwr-9511d9a.dsn` (KiCad: 45 unconnected, `inner_tracks.py` 0), gives no line.
  Run a's tracks are fixed in its add-only input, not new, so
  `power_layer_traces::the_check_flags_the_fixed_5v_pi_tracks_of_placement_run_a` runs the engine
  check on that DSN: both segments (112.2, 54.0) → (114.0, 55.9) → (114.0, 60.2) are
  `OutsideOwnPlane`. For any board and session, `freerouting-dev power-traces board.dsn [ses]`
  prints the same lines with coordinates (exit 1 when there is one); Q13.3 counts with it.
- [~] **Q13.3 — Measure.** Register before the first number (N02's totals rule):
  - `canary-pwr` placement variants a and a+b (canary-7a's inputs);
  - the in2-open fixtures;
  - two corpus boards with power-type layers.
  - **Accept:**
    - measured, with no plane net's trace outside its plane;
    - if accepted, N05 adds it to `enhanced`.

  *Partial 2026-10-17:* measured as registered, and **accepted by the totals rule**; what is
  left is N05's promotion and canary's official grade. Registration
  `docs/roadmap/prereg/Q13.json` (sha256 `ec1d28e7…` before any number, amended once to
  `063615a6…`, pinned by `crates/freerouting-dev/tests/prereg_q13.rs`; N02.1's gate mode does
  not exist, so the rule is stated there and computed here by hand). Set
  `tools/bench/sets/q13-power-layer-traces.txt`: canary-pwr (one board: the plugin row
  `tests/data/canary-pwr-q13/plugin-e21ee05e-unrouted.dsn`, `canary-pwr-9511d9a`, and the two
  in2-open fixtures), corpus `m-fc-g474-rev-a-m-fc` (In2.Cu split into +3V3_CORE, +3V3_SENS,
  +5V and VBUS) and `microzed-carrier-board-v4` (two VCCIO planes on In2.Cu, two GND planes on
  In1/In3/In4). Variants a and a+b are `not_registered`: only their add-only DSNs exist, with
  Java's pass 1 (and run a's bad track) fixed in, and their placements are not in canary's git;
  Q13.2 checks their sessions instead. One release build of origin/main + this diff + Q14's patch
  for every arm (calibrated: `mp1` on 9511d9a is byte-identical to Q10's `mp1.ses`). canary rows
  graded with canary's `grade.sh` steps (project at canary `e21ee05e`, WSL KiCad 10.0.7); corpus
  rows with `tools/kicad-oracle/grade.sh` plus `via_in_pad.py`/`inner_tracks.py`; plane-net
  traces counted by `freerouting-dev power-traces`.

  Verdict pair, the plugin's default arguments (`--router.min_trace_width_um=<Board Setup
  minimum>`) against the same plus `--router.power_layer_traces=own_plane`:

  | row | KiCad unconnected | outside own plane | inner tracks | drc_real | via in pad / below min | router unrouted | vias |
  |---|---|---|---|---|---|---|---|
  | canary plugin | 68 → 68 | 1 → **0** | 2 → **0** | 2 → 2 (input GND vias 0.297 mm from the edge) | 0 / 0 | 94 → 94 | 92 → 92 |
  | m-fc-g474 | 5 → 5 | 4 → **0** | 7 → **0** | 23 → 21 | 0 / 0 | 1 → 1 | 143 → 140 |
  | microzed | 0 → 0 | 0 → 0 | 652 → 652 (In3.Cu is a signal layer) | 99 → 98 | 0 / 0 | 0 → 0 | 202 → 202 |

  Totals 73 → 73 (Δ 0); bands canary 10, m-fc max(2, 5 % of 245) = 13, microzed max(2, 5 % of
  384) = 20, noise √669 = 25.9: **within the rule**; no row rises; hard clauses (outside own plane
  0, foreign power-layer traces 0, via in pad 0, below minimum 0) hold on every candidate row;
  guards (drc_real, inner tracks) no higher anywhere. Reported pairs, all meeting the hard and
  guard clauses: `mp1` and `mp1+sweep` on 9511d9a and `mp1` on both in2-open rows are
  **byte-identical** with and without `own_plane` (45, 33, 28 and 40 unconnected; on 9511d9a no
  plane net leaves its plane, and in the in2-open rows Power.Cu is a signal layer). With Q14's
  `--router.via_clearance=net_class`: `mp1+netclass` 48 → 49 (inner tracks 2 → 0, starved
  thermals 6 → 3), `mp1+sweep+netclass` 33 → 33 (inner tracks 4 → 0, starved 9 → 8), both clean;
  `plugin+netclass` is byte-identical to `plugin` and `plugin+netclass+ownplane` to
  `plugin+ownplane`. Wall time: canary plugin 225.7 → 195.5 s, m-fc 29.0 → 34.8 s, microzed
  399.9 → 309.0 s; the 9511d9a arms within 1 s.

  **Amendment.** The registered first run of the candidate failed: on m-fc-g474
  `plugin+ownplane` left one `+3V3_CORE` trace on In2.Cu from a via whose centre is 54 µm
  outside the plane into the plane (outside own plane 1), and microzed's drc_real was 101
  against 99. Cause: the search check let a hop without length pass unchecked, and the search
  measures that length on target shapes enlarged by the clearance compensation; the fix checks
  every hop in the board's own geometry. The amendment (in the registration, with this reason)
  reran every candidate arm with the fixed engine; the numbers above are that rerun. First-run
  candidate sessions on canary were byte-identical to the rerun's.

  Sessions for the official grade: `target/canary-handoff/2026-10-17-q13/` (8 SESs, `SHA256SUMS`,
  `README.txt` with flags and the self-grade). **Remains:** N05 adds
  `power_layer_traces = own_plane` to `enhanced` (not done here, by the task's instruction), and
  canary's official grade of the handoff sessions.

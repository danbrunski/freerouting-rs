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

- [ ] **Q13.1 — The rule in the maze search.** With `own_plane`, a room on a power
  layer is enterable by net N only where it lies within N's own conduction area on
  that layer. Rooms are clipped to the area, or the area's complement is added as
  an obstacle, whichever keeps the search's room shapes sound.
  - **Accept:**
    - a fixture with two islands on a power layer: net A's trace stays inside A's
      island, and the connection routes on a signal layer or reports unrouted;
    - with `layer`, the A14 exact set is unchanged.
- [ ] **Q13.2 — A check that finds it.** The parity harness's power-layer report
  (`board_parity/mod.rs`, "trace of <net> on power layer") also reports a trace of
  the plane's own net that leaves the plane's outline.
  - **Accept:** the check flags the canary runs' `+5V_PI` tracks, given their SES,
    and passes on the control.
- [ ] **Q13.3 — Measure.** Register before the first number (N02's totals rule):
  - `canary-pwr` placement variants a and a+b (canary-7a's inputs);
  - the in2-open fixtures;
  - two corpus boards with power-type layers.
  - **Accept:**
    - measured, with no plane net's trace outside its plane;
    - if accepted, N05 adds it to `enhanced`.

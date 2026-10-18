# Q17 — Neck down only through the pin field

Opened 2026-10-18 from N05.3's neck-down promotion. `enhanced` now sets
`router.neck_width_um = 300`: Java's necked retry. A connection that fails at its
class width is routed again at 0.3 mm **along its whole length**, not only where
the pins are close together. On canary-pwr with its Board Setup widths
(`canary-pwr-9511d9a-board-setup-widths.dsn`) the retry takes 49 unconnected to
35. But it ships 140.9 mm of Power_High (1.5 mm class) narrower than its class,
and the run takes 2.3× as long.

Q02 designed the better rule and built only its pure functions
(`quality/neckdown/bounded.rs`: `room_within`, `neck_distance`, `pitch`,
`neck_width`). Q02.3's width policy inside the maze search, Q02.4's fixture and
Q02.5's widen-back were never built (see their notes in
[archive/Q02](archive/Q02-neck-down-through-pin-fields.md)). This checklist
builds them. It measures them against today's `enhanced` (`neck_width_um = 300`),
not against Java.

Prerequisites: Q02.1–Q02.2 (the width report and the narrowing ledger), N02 (the
totals rule), N05 (the promotion recipe).

## Decisions made here

- **The parity path does not change.** The width hook's default policy returns
  the control's per-layer half-width, so `legacy` stays byte-identical to Java
  (A14).
- **The baseline is `enhanced` as it is.** A new stage is promoted only if it beats
  `neck_width_um = 300` under N02's totals rule on unconnected, or ties it and
  narrows less copper. If it is promoted, it replaces the retry in `enhanced`;
  the retry stays as an option.

## Checklist

- [ ] **Q17.1 — A width policy in the maze search** (Q02.3's remainder, step 1).
  A `WidthPolicy` trait in the A04 search, `fn half_width(&self, room, layer) -> i32`,
  read at every site that now reads `AutorouteControl::trace_half_width` or
  `compensated_trace_half_width`: room completion, `door_is_small`, the start and
  destination neck-downs in `session/search.rs`, the path locator and
  `path/insert.rs`. The Freerouting policy returns the control's value.
  - **Accept:** with the default policy, the A14 exact set is byte-identical; a unit
    test shows a two-width policy changes what `door_is_small` decides.
- [ ] **Q17.2 — The `bounded-neck` stage** (Q02.3's remainder, steps 2–3).
  `BoundedNeck::retry` builds the policy from the connection's start-set and
  destination-set pins, in that order: the neck width within D of a pin, the class
  width elsewhere, with D = courtyard plus one pitch from `neck_distance`. It reruns
  the search at the point and with the ripup cost of `FreeroutingNeckDown::retry`,
  and inserts each segment at its room's width. Register it as a `NeckDownPolicy`
  with the id `bounded-neck`, and add `--router.neck_down.path_mm` for a fixed-length arm.
  - **Accept:** Q02.4's fixture (`q02-qfn-power.dsn`: a 0.5 mm pitch QFN with one
    1.5 mm power pin, open board beyond the pin field):
    - the connection routes;
    - a test walking the SES finds no stretch narrower than the class width
      further than D plus the taper length from the pin;
    - no new clearance violation and no via in a pad;
    - `tools/determinism-check.sh` passes.
- [ ] **Q17.3 — Widen-back with taper** (Q02.5 as designed there). After insertion and before
  pull-tight, retry each narrowed trace in ledger order at its class width, in pieces
  of at most 0.5 mm, using the exact clearance check. Add the 4-step taper at each
  narrow-to-wide junction, and drop wide islands no longer than two tapers. Run it
  after both `bounded-neck` and the plain retry.
  - **Accept:**
    - a 22 mm run with one 0.4 mm pinch is narrow only at the pinch and its tapers;
    - no new DRC items on any fixture;
    - pull-tight does not remove the tapers.
- [ ] **Q17.4 — Measure under N02.** Register first, on the boards of
  `N05-neck-down.json` (the canary-pwr plugin DSN, the canary-pwr 9511d9a DSN with
  its Board Setup widths, interf_u, pic_programmer, CM5_MINIMA_3) plus two
  corpus boards with wide classes it has not seen.
  - Arms:
    - `enhanced` (the retry at 300);
    - `bounded-neck`;
    - `bounded-neck + widen-back`;
    - `retry 300 + widen-back`.
  - Report the narrowed copper on the wide classes from the neck-down width report,
    and the wall time.
  - **Accept:**
    - every hard guard holds: clearance, width, via in pad, power-layer traces;
    - if an arm is accepted, N05 makes it `enhanced`'s neck-down, with a
      DIVERGENCES line;
    - canary-7a grades the canary sessions.

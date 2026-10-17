# N04 — A trace end inside a pad is connected

Opened 2026-10-17 (D10), from KiCad's ecc83-pp demo routed with its existing
tracks. Both the port and Java `a773a70ec` end with 6 unrouted connections and
score 619.05, while KiCad reports the board fully connected.

The cause is the contact rule. Java's `Trace.getNormalContacts` and the port's
`BasicBoard::trace_normal_contacts` + `contact_at_point` connect a trace to a pin
only when the trace's end corner is exactly the pin centre. KiCad connects on any
overlap with the pad. ecc83's tracks end inside the pads but off-centre:
- 42–49 µm on all 9 pins of the valve socket U1;
- 80–381 µm on C2-2, R1-1 and R3-1.

Snapping U1's ends to the centres took the board from 6 unrouted to 2. The
verdicts show the router "routing" these connections (state ROUTED, search
"found") and still counting them incomplete for 18 passes.

Notes: `target/orchestrator/ecc83/`, and the operator-decisions entry of
2026-10-17.

Prerequisites: N01.

## Decisions made here

- **Under `best`, a trace end connects to a same-net pin when the end lies inside
  the pin's pad shape on the trace's layer**, as in KiCad. `java` keeps
  exact-centre contacts.
- **Not a snap.** The trace keeps its geometry: the SES writes it back exactly, and
  KiCad's import sees the same copper. Only the connectivity model changes.
- **Vias:** the same rule for a trace end inside a same-net via's pad. A via
  centre inside a pin's pad also connects, as KiCad does. Check this against
  KiCad's connectivity on the demo boards before deciding; if KiCad differs, keep
  Java's rule for vias.

## Checklist

- [ ] **N04.1 — The contact rule.** Under `best`, `contact_at_point` (or a
  profile-aware variant) accepts a trace end inside a same-net pad shape. The
  incompletes, the routing ledger and the optimizer all use it.
  - **Accept:**
    - ecc83-pp exported with its tracks has the same unconnected count in the
      router as in KiCad (0) before routing;
    - with `java`, the A14 exact set is unchanged.
- [ ] **N04.2 — Everything that walks connectivity agrees.** This covers
  pull-tight, trace tails (`remove_trace_tails`), the optimizer's connected sets,
  the DRC's dangling-trace check and the SES writer's endpoint handling (C05).
  - **Accept:**
    - a fixture with an off-centre track end: pull-tight keeps the end inside the
      pad;
    - the track is not removed as a tail;
    - the SES round-trips the end unchanged.
- [ ] **N04.3 — Measure.** Register the KiCad demos and corpus boards that have
  existing tracks, plus canary-pwr (with N02's rule).
  - **Accept:** measured; if accepted, N05 adds it to `best`.

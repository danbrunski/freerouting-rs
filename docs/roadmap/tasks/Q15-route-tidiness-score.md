# Q15 — A measured tidiness score for routes

Opened 2026-10-17 (Dan: "sure"). KRT's users and author say its routes are "far more natural
and organized than the Manhattan-style FreeRouting algorithm". freerouting-rs measures none of
that today. It grades only:
- unconnected connections;
- DRC items;
- vias;
- length.

A score that measures how tidy routes are lets a change claim, or be refused, an improvement in
looks with numbers, and lets X13 compare tools on it.

Prerequisites: F03 (the bench and gate), F02.6 (the KiCad oracle), C05 (the SES reader).

## Decisions made here

- **Per routed connection, from the routed copper, with no judgement by eye:**
  - `detour`: routed length over the straight-line pad-to-pad distance;
  - `bends`: direction changes per 10 mm;
  - `acute`: corners sharper than 90°, counted;
  - `jogs`: segments shorter than 2 × the track width between two bends;
  - `vias`: vias per connection.
- **Report the board's median and 90th percentile** of each, not a sum, so a few long nets
  don't dominate.
- **It reads an SES or a `.kicad_pcb`**, so the same tool scores freerouting-rs, Java and KRT
  output (KRT writes a board, not an SES).
- **Report-only at first.** Making tidiness part of a keep rule would be a separate decision for
  Dan, once there are numbers.

## Checklist

- [ ] **Q15.1 — The scorer.** `freerouting-dev tidy <board> <routed.ses|.kicad_pcb>` prints
  the five measures (median and p90) as JSON and as a table.
  - **Accept:** unit fixtures with known values (a straight track, an L, a zig-zag, an acute
    corner, a via pair); the same copper read from an SES and from a `.kicad_pcb` scores the
    same.
- [ ] **Q15.2 — In the bench and the grader.** `freerouting-dev bench` and the KiCad grading
  scripts (`tools/kicad-oracle/grade.sh`) add the five measures to their records, and
  `compare` shows them.
  - **Accept:** a bench run on the P04 set shows them; the records' schema is documented.
- [ ] **Q15.3 — A baseline.** Score Java's route references, freerouting-rs `legacy`, and
  `enhanced` with the sweep on the X13 boards.
  - **Accept:** a table in the done note, used by X13.

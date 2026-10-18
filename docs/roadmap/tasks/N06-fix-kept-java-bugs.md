# N06 — Fix the Java bugs kept on purpose

Opened 2026-10-17 (Dan asked whether they were fixed; they weren't). During the parity phase
the port reproduced Java's bugs wherever fixing one would change routing results. D10 ends that
phase, so each bug can now be fixed under profile `enhanced`, while `legacy` keeps it.

The bugs are listed in three places:
- `docs/DIVERGENCES.md`, "Java bugs kept on purpose". The geometry list:
  - `RationalPoint.perpendicularProjection`;
  - the int overflow in `Line.length`;
  - `stairApproximation45` evaluating `functionValueApprox` for the function-of-y case;
  - the `index_of_nearest_corner` quirk;
  - `PolygonShape.area` returning 0;
  - `Simplex.cutoutFrom` never setting its previous division line;
  - the no-op branch 3 in IntOctagon's box cutout.

  Five more such paragraphs sit in later sections, among them `ForcedPadRouter.inFrontOfPad`
  for from side 0 and `RoutingBoard.moveDrillItem` with a finite tidy width.
- About 25 `// PARITY(quirk):` markers in `crates/freerouting/src`: the autorouter, the fanout,
  the maze search, rip-up and the batch optimizer. Examples:
  - Java's off-by-one in `BatchAutorouter.java:285-287`;
  - `viaRadii` never rebuilt (`AutorouteControl.java:268`);
  - int division of via counts;
  - the optimizer taking a fraction for a percentage.
- Module docs that list "kept Java quirks": `optimize/pull_tight.rs`, `optimize/batch/`,
  `interactive/route.rs`.

Prerequisites: N01 (profiles), N02 (the totals rule), N05 (promotion). Note that the geometry
crate has no settings access.

## Decisions made here

- **First sort, then fix.** Each bug is one of three kinds:
  - **(a) no effect on results:** dead code, or a value nobody reads. Fix it outright in both
    profiles. Proof: the A14 exact set stays byte-identical, and a test pins the corrected
    value.
  - **(b) a correctness bug that changes results:** fix it under `enhanced` only, behind one
    switch per bug. `legacy` keeps Java's behaviour.
  - **(c) a quirk whose "fix" is a judgement call,** for example a tuning constant Java gets
    wrong. Measure it like any candidate (N02), or leave it with a reason.
- **One mechanism for the switches.** A `JavaQuirks` set (one flag per bug, all on under
  `legacy`) is carried where the code needs it:
  - the routing board's settings, for the autorouter and the optimizer;
  - an explicit argument to the geometry functions concerned.

  No global state. A quirk that would need a flag threaded through hot geometry code is noted,
  with its cost, before it is changed.
- **Measured in batches, not one by one:** the (b) fixes of a subsystem go in together (the
  geometry, the maze search, the optimizer). Each batch is one N02 measurement, with a fallback
  to bisecting the batch if it fails.

## Checklist

- [ ] **N06.1 — Inventory and sort.** One table of every kept bug: where it is (Rust and Java
  lines), what it gets wrong, whether it is reachable, which kind (a/b/c), and how big a change
  it is likely to make. It covers the DIVERGENCES lists, every `PARITY(quirk)` marker and the
  module-doc lists.
  - **Accept:** the table is in this file, and every marker and list entry has a row.
- [ ] **N06.2 — The (a) fixes.** Fix each one outright, with a test of the corrected value.
  Its DIVERGENCES entry moves from "kept on purpose" to the list of fixed Java bugs.
  - **Accept:** the A14 exact set stays byte-identical, and the B02, B03 and A04 probes are
    unchanged.
- [ ] **N06.3 — The quirk switches.** Add `JavaQuirks`, with all flags on under `legacy`, and
  wire the (b) bugs to it.
  - **Accept:**
    - with every quirk on, A14 is byte-identical;
    - each flag has a unit test showing both behaviours.
- [ ] **N06.4 — Measure the (b) batches.** Register each batch before its first number (N02's
  rule, at least 3 boards, canary-pwr through the plugin path as well).
  - **Accept:** each batch is measured. Accepted batches go into `enhanced` through N05. A
  rejected batch stays switchable and off, with its numbers recorded; it is never removed,
  because `legacy` needs it.
- [ ] **N06.5 — The (c) quirks.** Measure them, or record why each stays.
  - **Accept:** every (c) row ends measured or explained.

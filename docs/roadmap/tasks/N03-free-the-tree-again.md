# N03 — Free the search tree, judged by the totals rule

Opened 2026-10-17 (D10). P04 measured the canonical room order (P04.1) and the
leaf work unit (P04.2) on 17 boards. With the canonical order:
- total unrouted fell from 1,281 to 1,269;
- vias fell from 5,102 to 5,057;
- length fell from 113,870 to 113,538 mm;
- maze work halved (39.1 G nodes → 19.6 G);
- 5 boards regressed under the paired rule.

KiCad grading was mixed:
- coldfire improved (235/85 → 213/77);
- canary-pwr improved slightly (39/71 → 38/70);
- interf_u got worse (9/17 → 11/20).

GP8B went from 0 to 2 router-counted clearance violations. Under N02's rule that
would be a hard-guard failure if KiCad confirms it.

The implementation is saved at `target/orchestrator/p04/p04-rejected-canonical-leaves.patch`
(it applies to `93c326f`). Once the room order no longer depends on the tree's
shape, the tree can be replaced (P04.3). That is the largest speed lever left.

Prerequisites: N01 (profiles), N02 (the totals rule), P04's patch, P02.1's audit.

## Checklist

- [ ] **N03.1 — Restore the code behind settings.** `router.room_order = tree |
  canonical` and `router.work_unit = nodes | leaves`, rebased onto main. `legacy`
  pins `tree` and `nodes`.
  - **Accept:**
    - with `legacy`, the A14 exact set and the B02, B03 and A04 probes are
      unchanged;
    - P04's order-independence tests pass with `canonical`.
- [ ] **N03.2 — The GP8B violations.** Find out why the canonical order adds 2
  clearance violations on GP8B, and fix it or explain it, before measuring.
  - **Accept:** the cause is in the done note; KiCad grades GP8B with the
    canonical order and no new `drc_real` items.
- [ ] **N03.3 — Measure under N02's rule.**
  - Register before the first number, on boards P04 didn't use:
    - the PCBWorld D3-C set;
    - canary-pwr variants not in `p04.txt`;
    - KiCad demos with existing tracks.
  - Arms: `tree+nodes` (baseline), `canonical+nodes`, `canonical+leaves`.
  - **Accept:** measured. If one arm is accepted, it becomes `enhanced`'s default
    through N05, with a DIVERGENCES line.
- [ ] **N03.4 — The faster tree** (P04.3's plan, only after N03.3 accepts an arm).
  - Try a bulk-loaded R-tree with struct-of-arrays bounds, plus incremental leaf
    updates. The result sets must be identical, so routes are identical against
    N03.3's build.
  - `router.search_tree = legacy | fast`, where `legacy` stays the parity tree.
  - **Accept:**
    - `ses-diff` identical on the N03.3 set;
    - wall time and callgrind before and after;
    - the P02.1 audit is clean;
    - determinism in both modes.

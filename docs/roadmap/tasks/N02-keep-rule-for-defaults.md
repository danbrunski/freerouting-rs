# N02 — A keep rule that judges the total within noise

Opened 2026-10-17 (D10). The routing-quality rule "better on N−1 boards, worse on
none" (routing-quality.md) treats every board as a vote. A change that moves
routes changes most boards by a few connections either way, so the rule rejects
it even when the total improves clearly. P04.1: −12 unrouted over 17 boards and
half the maze work, yet 5 boards regressed.

## Decisions made here

The rule for a change to profile `enhanced`:
- **Hard guards, unchanged:**
  - no new clearance violations (KiCad `drc_real` where graded);
  - no via in a pad;
  - D2 determinism.
- **Signal: KiCad-graded unconnected**, summed over the registered set. Where a
  board has no `.kicad_pcb`, the router's own unrouted count is used, reported
  separately.
- **Accept when both hold:**
  1. the total improves by more than the set's noise, which is the square root of
     the sum of the per-board noise bands squared;
  2. no board regresses by more than twice its own noise band.
- **The noise band of a board** is its order spread: the half-range of
  unconnected over K pass-1 orders (Q12.4). Until Q12.4 has measured a board, the
  band is registered as max(2, 5 % of the board's connections). On `canary-pwr`
  the variants' spread of ±10 is used.
- **Speed changes** are accepted when quality is within noise by the rule above
  and the registered work measure drops by at least 20 %.
- **At least 3 distinct boards, still.** The `canary-pwr` variants count as one
  board.
- **Existing registrations keep their rules.** Their numbers exist, so changing
  their rule now would be editing the test after seeing the result. A candidate
  rejected under the old rule can be judged again only by a new registration on
  boards or variants it hasn't been measured on, such as the PCBWorld D3-C set
  (`tools/bench/sets/pcbworld-d3c.txt`).
- **A rejected candidate is kept as an opt-in or deleted** (proposed 2026-10-17). It
  stays as an option, off by default and listed under "Experimental" in the
  algorithm settings (N01.4), only when all three hold:
  1. **it wins on a class of board you can name** (layer count, pin density, BGA
     fanout, planes, size), shown by the measurement's own per-board table. Its
     help text names the class and gives the numbers;
  2. **it passes the hard guards** above;
  3. **it is cheap to keep:** a self-contained stage or setting, not a hook through
     the engine's cost or search code.

  Otherwise its code is removed. Its numbers stay in its checklist, and git
  history keeps the code. The kept Java bugs (N06) are not candidates: `legacy`
  needs them, so they always stay switchable.

## Checklist

- [x] **N02.1 — The rule in `freerouting-dev gate`.** Add a `--rule totals` mode next
  to today's paired rule. It reads each board's noise band from the
  registration (`noise` per board, or a formula).
  - **Accept:** self-tests cover a P04.1-shaped table, which passes; a total that
    improves only within noise, which fails; and a board beyond twice its band,
    which fails.
  *Done 2026-10-17:* `freerouting-dev gate --rule totals` (module `gate_totals.rs`;
  `--accept improve|no-regress`, `--noise BOARD=BAND`, `--speed METRIC[=PERCENT]`; with
  `--prereg` the registration states them and a differing option is REFUSED). The rule is as
  Decisions states it: hard guards (hard metrics, guard metrics, D2 through the baseline
  comparison), total improvement strictly above sqrt(sum band^2), no board worse than 2 x its
  band, band registered per board or max(2, ceil(5 % of `connections`)), rows of a `board`
  summed and counted once, >= 3 boards, optional speed clause (quality within noise and the work
  measure down >= 20 %), `no-regress` for N06. Self-tests in `gate_selftest.rs` (every
  invocation) and `crates/freerouting-dev/tests/gate_totals.rs` (21 tests, `cargo test -p
  freerouting-dev`: all pass). P04.1-shaped table (P04's measured counts, illustrative bands,
  maze work 39.1 G -> 19.6 G): passes **through the speed clause**; on quality alone it fails,
  because -12 is below the set noise (52.6 with the illustrative bands, 12.81 even with the
  smallest possible bands, 2 per board and 10 for canary-pwr). So "a P04.1-shaped table, which
  passes" holds for the speed clause only; the Decisions' rule cannot pass P04.1 on the
  unrouted total (the doc says so). Also pinned: improvement equal to the noise fails; a board
  at exactly 2 x band passes, above fails; hard guard fails a great total; < 3 boards refused;
  canary variants count once.
- [x] **N02.2 — Registration format.** The registration states `"rule": "totals"`,
  the per-board noise source, and the speed clause if used. `prereg` tests pin it
  as before.
  - **Accept:** the format is documented in routing-quality.md, and a sample
    registration validates.
  *Done 2026-10-17:* `"rule": "paired" | "totals"` and the `totals` block (`accept`, `signal`,
  `noise`, `min_boards`, `speed`) in `gate_prereg.rs`; unknown keys, a missing `accept`, a
  `min_boards` below 3, a speed drop below 20 %, a speed clause with `no-regress`, an
  unregistered signal or speed metric, and a noise band naming no run board are all refused or
  malformed (tests `a_bad_totals_registration_is_refused_or_malformed`,
  `a_noise_band_for_a_board_the_run_does_not_have_is_refused`). Documented in `tools/ab/README.md`
  ("The totals rule") and the module docs. Sample: `tools/ab/examples/n02-totals.prereg.json`,
  validated, sha256-pinned and run against a synthetic run in `gate_totals.rs`. Q14's registration
  (`"rule": "totals"`, prose `totals_rule`, no block, hash unchanged) still parses and
  `prereg_q14` passes; a run against it is REFUSED ("no `totals` block"), so Q14.3's verdict stays
  hand-computed unless an amendment adds the block.
- [x] **N02.3 — routing-quality.md.** Amend "How to measure a quality change":
  keep the paired rule as the rule for stages that are opt-in, and use the totals
  rule for changes to `enhanced`. Record the reason, with P04.1's numbers.
  - **Accept:** the amendment is in the doc, dated, with a pointer to D10.
  *Done 2026-10-17:* `routing-quality.md`, "How to measure a quality change", bullet "Which rule
  judges (amended 2026-10-17, N02, D10)": paired rule for opt-in stages, totals rule for changes
  to `enhanced`, the reason with P04.1's numbers (1,281 -> 1,269, maze work 39.1 G -> 19.6 G, 5
  regressing boards, the clearance guard failures), and the plain statement that P04.1 passes
  only through the speed clause.
- [ ] **N02.4 — Sort the rejected candidates.** Apply the keep-or-delete rule to
  every rejection so far: Q07 conflict-rounds and inside-out, P04's rejected arms,
  and X02, X06 and X11, which are already removed. For each one record the board
  class it wins on, or that there is none.
  - **Accept:** a table in this file. Each kept option is under "Experimental" with
    its help text; nothing else rejected is left in the code.

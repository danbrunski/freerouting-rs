# Q11 — Transactional rip-up

Opened 2026-10-17. In pass 1, a rip whose victim does not re-land is permanent (73 of
109 on canary-pwr). Java's rip cost (`MazeRipupResolver.checkRipup`, about
`ripupCosts × halfWidth / detour` [V]) ignores whether the victim can be rerouted.

Prerequisites: board parity (rung 3); A10 (`ConnectionRouter` wrapper slot), A11 (rip events, `ConnectionOutcome::ripped`), B01 (snapshots); Q05.1 (`rip-stats`); Q10's sweep for the combined arms; F03.1, F03.4; measured after P04. Code may start before P04.

Source: research by freerouting-rs-f1 at Dan's request (2026-10-17), `docs/roadmap/research/2026-10-17-routing-algorithms.md` (candidate N2); the preregistration draft is `docs/roadmap/prereg/Q11.json`, marked `DRAFT` until the implementer registers it (sha256s pinned) before the first number.

Practice and literature:
- KRT's "custody" rips [V, `docs/rip-up-reroute.md` at `0486e799`];
- rjwalters/kicad-tools #6342/#6355: all-or-nothing rollback plus a cap of 2 rips per
  (victim, blocker set) [V];
- Large Neighbourhood Search's destroy, repair, accept (Shaw, CP-98) [V].

No peer-reviewed PCB numbers exist [V: none found]. Java's own rip-cost trend is the
evidence.

## What exists

- `ConnectionOutcome::ripped` gives the ripped items in Java's order.
- B01 `BoardSnapshot` / `restore`.
- `ConnectionParams::ripup_allowed`.
- The routing ledger's incomplete count.
- Q05.1's `rip-stats` diagnostic (the re-land share is its natural metric).

## Decisions made here

- A `ConnectionRouter` wrapper, `transactional-ripup`, around the configured router.
  The search and its rip costs stay Java's; only the outcome is judged.
- Procedure:
  1. Snapshot. Route.
  2. If the outcome ripped something, re-route each victim connection at once with
     `ripup_allowed = false`, in ascending item-id order of the victims' `from` items.
  3. Commit by the arm's rule; otherwise restore the snapshot.
  4. After a restore, retry the connection once with rip-up off. If that fails too,
     record the failure as Java would. Java's after-failure net rip (`after_failure`)
     is unchanged.
- Snapshot cost: take it only when the search could rip (`ripup_allowed`), or use
  the undo journal if B01 offers one. The implementer measures which.

## File ownership

- Creates `quality/transaction.rs`.
- Appends to `quality/mod.rs` and `quality/register.rs`.
- Adds one `rip_count` field to `rip_stats` if needed.

## Checklist

- [ ] **Q11.1: rip-cost sweep, no code.** `start_ripup_costs` ∈ {1500, 5000, 20000}
  and pass 1 with rip-up off, on the eligible boards. This bounds what Q11 can gain.
  - **Accept:** a table in the done note, with Q05.1 rip stats per arm.
  - *Not run 2026-10-18 (heavy run pending, the heavy slot is another agent's):* per eligible board
    of `tools/bench/sets/q10-sweep.txt` (Q11.json's rule) and per cost `C` in 1500, 5000,
    20000: `target/release/freerouting -de {DSN} -do {SES} --gui.enabled=false --router.scoring.start_ripup_costs=C --router.min_trace_width_um=150 -mp 1
    --router.special.diagnostics=rip-stats` (the result manifest's `rip_stats` is the table's
    per-arm rip stats), graded as Q11.json says. The rip-up-off arm has no switch: the batch
    passes always allow rip-up (`router.ripup_allowed` is the fanout's), and `-mp 0` with a
    one-pass sweep is not pass 1 (`-mp 0` means no pass limit; probed on `Issue093-interf_u`).
    Operator decision (2026-10-18): the arm runs as `start_ripup_costs=1000000`, labelled
    "near-off" in the table (each item's rip cost is capped at `i32::MAX/100`, so rip-up is
    priced out, not switched off); Q11.1 stays no-code. The code here supports every other arm.
- [x] **Q11.2: the wrapper, arm `strict`.** Commit only if every victim connection
  re-lands.
  - **Accept:**
    - a fixture where A rips B and B cannot re-land: A is retried without rip-up, and
      B keeps its copper;
    - a fixture where B can re-land: both are complete;
    - off is byte-identical;
    - determinism.
  - *Done 2026-10-18:* `quality::transaction::TransactionalRipup`, registered as
    `transactional-ripup` (`router.autorouter.connection_router`), settings
    `router.transaction.{rule, depth, rip_factor}` (unset strict, 0, 0; in no profile), an
    invalid value refused before routing
    (`invalid_transaction_settings_are_refused_before_routing`). Tests in
    `crates/freerouting/tests/quality_transaction.rs` (14 green):
    - B cannot re-land (`tests/data/quality/q11-locked-cross.dsn`):
      `a_victim_that_cannot_reland_keeps_its_copper_and_a_is_retried_without_ripup`: B keeps
      its wire, A fails after its rip-up-free retry, tally "2 transactions, 0 committed, 2
      rolled back (0 retries routed)" (A1 and A2 are both pass items); Java's pass 1 rips B
      there (`without_the_router_a_rips_b_in_pass_1_on_both_fixtures`);
    - B re-lands (`q06-sole-wall.dsn`): `a_victim_that_relands_completes_both_nets_in_pass_1`,
      incomplete 0 after pass 1 (Java: B incomplete until pass 2);
    - off: `off_is_byte_identical` (SES and job log, 4 fixtures × 1 and 3 passes, with the
      transaction settings set but the router unset, and with `freerouting-connection`
      named), `without_a_rip_the_router_routes_as_java`; the fast parity subset
      `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p freerouting
      --test autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1`:
      "exact: 7 boards, 7 pass, 0 expected, 0 fail, 7 SES byte-identical". The full A14 gate
      is pending (heavy slot);
    - determinism: `every_arm_is_deterministic` (SES and log, two fixtures, 3 passes, every
      arm); canary-pwr-9511d9a pass 1 with `rule=strict` twice: the same SES (sha256
      `062a1baeb0d0…`).
    - Snapshot cost, measured: a `Clone` of the routed canary-pwr board (2318 items) takes
      1.42 ms in release (a `BoardSnapshot` of the input 0.08 ms but its restore rebuilds the
      tree, changing later work counts), against 42.8 s for Java's pass 1 there; the clone is
      taken for every connection with rip-up allowed. Release spot checks on canary-pwr-9511d9a
      (`-de fixtures/canary-pwr/canary-pwr-9511d9a.dsn --gui.enabled=false --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150 -mp 1`, router's own unrouted
      count, not a measurement): Java 104 unrouted in 42.8 s; strict 94 in 87.3 s (54
      transactions, 21 committed, 33 rolled back, 102 victim nets, 56 re-landed);
      `Issue093-interf_u` rips nothing in pass 1, every arm's SES is Java's.
- [x] **Q11.3: arm `count`.** Commit if the ledger's incomplete count strictly
  decreases. A ripped multi-pin net may re-land along a different topology.
  - **Accept:** as for Q11.2.
  - *Done 2026-10-18:* `CommitRule::Count` (`router.transaction.rule=count`): the board is kept
    when the routing ledger's incomplete count is strictly below the count before the
    transaction. The same tests cover it (`a_victim_that_cannot_reland_...`,
    `a_victim_that_relands_...`, `off_is_byte_identical`, `every_arm_is_deterministic`,
    `the_rule_decides_on_the_ledger`). Spot check on canary-pwr-9511d9a pass 1: 96 unrouted
    in 85.8 s (53 transactions, 17 committed, 36 rolled back).
- [x] **Q11.4: arm `depth1`** (optional). Victims may rip at depth 1, under the same
  commit rule, with no victim ripped twice in one transaction.
  - **Accept:** as for Q11.2, plus a termination test on a 3-net cycle fixture.
  - *Done 2026-10-18:* `router.transaction.depth=1` (0 or 1, anything else refused): wave-0
    victims route with rip-up on; nets they rip are a second wave routed with rip-up off; a
    victim attempt that rips the routed net or a net of the transaction again is undone and
    routed without rip-up. Termination on `tests/data/quality/q11-cycle.dsn` (nets A, B, C)
    with a scripted inner router that rips A→B→C→A: `depth_1_ends_on_a_three_net_cycle`
    (one transaction, two waves, 2 victim nets, rolled back, every wire back),
    `depth_1_never_rips_a_net_twice` (A rips B and C; both re-rips refused),
    `depth_0_routes_victims_without_ripup`, and the real router on the cycle board
    (`the_real_router_ends_on_the_cycle_board_with_every_arm`). The Q11.2 clauses hold for
    the `depth1` arm in the same tests. Spot check on canary-pwr-9511d9a pass 1 (`count`,
    depth 1): 89 unrouted in 109.4 s (47 transactions, 19 committed, 28 rolled back, 139
    victim nets, 113 re-landed, 43 re-rips refused).
- [x] **Q11.5: rip-count protection** (optional; needs a one-line engine hook). The rip
  cost of an item is multiplied by (1 + 2k), where k is the number of times its
  connection was ripped this run. KRT measured P(ripped again | ripped k times) ≈ 0.9.
  - The hook: `RipupPolicy::ripup_factor(item) -> i32`, 1 by default and not
    consulted in the parity path.
  - **Accept:** off is byte-identical; determinism.
  - *Done 2026-10-18:* `router.transaction.rip_factor=f`: the transactional router counts per net
    the committed transactions that ripped it (`k`) and hands the search the factors
    `1 + f·k`. The hook is `ConnectionParams::ripup_factors` (copied into
    `AutorouteControl::ripup_factors`, read in `check_ripup`), not a `RipupPolicy` method:
    the ripup policy does not reach the maze search, and the router that counts the rips is
    the one that builds the parameters. `None` everywhere else, so Java's search never reads
    it. Tests: `the_rip_factor_multiplies_the_rip_cost_of_the_ripped_net_only` (B's rip cost
    ×5 with factor 5 for B; a factor for A or of 1 changes nothing),
    `the_factors_are_one_plus_f_times_k` (unit), `a_rip_factor_of_0_is_unset`, and the
    `strict+ripfactor` arm in `off_is_byte_identical` and `every_arm_is_deterministic`. Spot
    check on canary-pwr-9511d9a pass 1 (strict, f=2): 95 unrouted in 91.6 s.
- [ ] **Q11.6: measurement** (prereg draft `docs/roadmap/prereg/Q11.json (draft)`).
  - Arms: `mp1` (baseline), `Q11.1-best`, `strict`, `count`, `depth1`, `strict+Q11.5`,
    each also followed by an Q10 sweep.
  - **Accept:** measured.
  - *Not run 2026-10-18 (heavy run pending):* registered first as `docs/roadmap/prereg/Q11.json`
    (sha256 `d1757b3d…`, pinned by `crates/freerouting-dev/tests/prereg_q11.rs`; N02's totals
    rule, Q10's board set and eligibility). Per eligible board and arm, the arm's `command`
    from Q11.json, e.g. strict: `target/release/freerouting -de {DSN} -do {SES} --gui.enabled=false --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150 -mp 1
    --router.autorouter.connection_router=transactional-ripup --router.transaction.rule=strict`,
    each also with `--router.autorouter.sweep=free --router.autorouter.sweep_passes=2`, plus
    `--router.special.diagnostics=rip-stats` for the rip table; graded as Q11.json says, then
    `freerouting-dev gate --prereg docs/roadmap/prereg/Q11.json` per pair. Q11.1's
    `ripcost-best` value and the `eligible` list go into its `amendments` first.

## Expected

 the upper bound on canary-pwr is the 73 routed-then-ripped
connections. Fewer will survive, because a rolled-back connection may then fail. No
literature number exists; a guess of 5–15 % fewer unconnected is [I].

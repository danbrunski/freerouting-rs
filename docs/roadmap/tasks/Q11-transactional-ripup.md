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
- [ ] **Q11.2: the wrapper, arm `strict`.** Commit only if every victim connection
  re-lands.
  - **Accept:**
    - a fixture where A rips B and B cannot re-land: A is retried without rip-up, and
      B keeps its copper;
    - a fixture where B can re-land: both are complete;
    - off is byte-identical;
    - determinism.
- [ ] **Q11.3: arm `count`.** Commit if the ledger's incomplete count strictly
  decreases. A ripped multi-pin net may re-land along a different topology.
  - **Accept:** as for Q11.2.
- [ ] **Q11.4: arm `depth1`** (optional). Victims may rip at depth 1, under the same
  commit rule, with no victim ripped twice in one transaction.
  - **Accept:** as for Q11.2, plus a termination test on a 3-net cycle fixture.
- [ ] **Q11.5: rip-count protection** (optional; needs a one-line engine hook). The rip
  cost of an item is multiplied by (1 + 2k), where k is the number of times its
  connection was ripped this run. KRT measured P(ripped again | ripped k times) ≈ 0.9.
  - The hook: `RipupPolicy::ripup_factor(item) -> i32`, 1 by default and not
    consulted in the parity path.
  - **Accept:** off is byte-identical; determinism.
- [ ] **Q11.6: measurement** (prereg draft `docs/roadmap/prereg/Q11.json (draft)`).
  - Arms: `mp1` (baseline), `Q11.1-best`, `strict`, `count`, `depth1`, `strict+Q11.5`,
    each also followed by an Q10 sweep.
  - **Accept:** measured.

## Expected

 the upper bound on canary-pwr is the 73 routed-then-ripped
connections. Fewer will survive, because a rolled-back connection may then fail. No
literature number exists; a guess of 5–15 % fewer unconnected is [I].

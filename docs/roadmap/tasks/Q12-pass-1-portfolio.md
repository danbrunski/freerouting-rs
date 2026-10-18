# Q12 — Pass-1 portfolio and restarts

Opened 2026-10-17. Pass 1 is the best pass on canary-pwr, yet passes 2–10 are about
60 % of routing time (Q05 file). Commercial practice spends that time on several
whole solutions and keeps the best:
- TopoR optimises several variants and drops the worst [V, Wikipedia];
- Cadence US6957408B1 "generates several solutions based on different net orders and
  returns the best" [V];
- tscircuit races sub-solvers with different parameters [V].

Prerequisites: board parity (rung 3); A10 (`NetOrdering`); B01; O06 (parallel evaluation pattern); **Q07.1–Q07.3 built** (its two orders are arms 1 and 2; Q07.4 rejected both as defaults, so the code is parked on branch `rejected/q07-orderings`, 379b413. Re-apply it here with its PROVENANCE rows, and change the engine crate's `license` to `GPL-3.0-only AND MIT`, because the files carry KRT's MIT portions); Q10 and Q11 for the combined arm; F03.1, F03.4; measured after P04.

Source: research by freerouting-rs-f1 at Dan's request (2026-10-17), `docs/roadmap/research/2026-10-17-routing-algorithms.md` (candidate N3); the preregistration draft is `docs/roadmap/prereg/Q12.json`, marked `DRAFT` until the implementer registers it (sha256s pinned) before the first number.

Evidence of order variance:
- KRT #1230: placebo perturbations moved open nets by about ±10 % [V];
- freerouting PR #793: on 70 boards, 24 better, 22 worse, 24 the same [V];
- kicad-tools #6342: a net pair routes in one order and not the other [V].

Squeaky-wheel restarts (Joslin & Clements, JAIR 1999) move failed elements to the
front and rebuild. KRT's naive "hot nets first" was **7 % worse** (#1230) [V], so it is
an arm, never the default.

## What exists

- The post-fanout board snapshot (B01).
- `NetOrdering` (A10), with Java's item order and the seeded shuffle with
  `JavaRandom`.
- Q07's conflict-round and inside-out orders, once Q07.1–Q07.3 are built.
- The board score (`BoardScore`).
- O06's parallel evaluation pattern: each candidate on its own copy, merged in
  submission order.

## Decisions made here

- A pipeline-level stage (`router.autorouter.portfolio = K`):
  1. Clone the post-fanout board K times.
  2. Run arm i's pass 1 (and Q10's sweep, if on) on clone i. Use up to
     `max_threads` threads.
  3. Keep the best by (Freerouting incomplete count, board score, arm index).
  4. Then run the optimizer once on the winner.
- **Arm 0 is the configured ordering.** The portfolio can therefore never be worse
  than the baseline by the engine's own score: a "best of" includes the baseline.
  KiCad's count can still disagree (plane nets), so measurement is still required.
- Arm list, fixed in the settings and in this order:
  - Java item order;
  - Q07 `conflict-rounds`;
  - Q07 `inside-out`;
  - seeded shuffles with seeds 1…K−3.
- `restarts = R`: a squeaky-wheel generation. Its order is the winner's order,
  stable-sorted with that run's failed connections and ripped victims first. It runs
  sequentially after the portfolio, and the best of all is kept.
- Results are independent of thread scheduling: each arm is a pure function of
  (board, arm). They are merged by arm index.

## File ownership

- Creates `quality/portfolio/{mod,arms}.rs`.
- Appends to `quality/mod.rs`, `quality/register.rs`, the C02 settings struct and the
  pipeline's stage list (one call).

## Checklist

- [ ] **Q12.1: the portfolio stage.** K clones, the fixed arm list, parallel threads,
  selection by key.
  - **Accept:**
    - with K = 1 the SES is byte-identical to the baseline;
    - a test where arm 2 beats arm 0 picks arm 2;
    - runs with `max_threads` 1 and 4 give the same SES;
    - determinism.
- [ ] **Q12.2: squeaky-wheel restarts.**
  - **Accept:** a fixture where the failed connection routes when moved first ends
    complete; off is byte-identical; determinism.
- [ ] **Q12.3: memory and time guard.** A board clone's peak memory times K must stay
  under a setting (WSL limits: see the memory notes).
  - **Accept:** canary-pwr and LogicBoard_smt with K = 4; peak resident memory in the
    done note.
- [ ] **Q12.4: order-spread report.** Per board: min, median and max incomplete count
  (and KiCad count where graded) over the K arms.
  - **Accept:** a table on the eligible boards in the done note. It is proposed as the
    per-board noise quantum for later keep rules (an F03 amendment, the
    orchestrator's call).
- [ ] **Q12.5: measurement** (prereg draft `docs/roadmap/prereg/Q12.json (draft)`).
  - Arms: `mp1`, `mpdef`, `portfolio4`, `portfolio4+restart2`, and `portfolio4` with
    Q10 and Q11 on.
  - Work is reported against `mpdef`'s, since the portfolio is meant to replace
    passes 2–10 at about the same total work.
  - **Accept:** measured.

## Expected [I]

 with the ±10 spread on canary-pwr, a best-of-4 should recover a few
connections. Upstream's clone-and-shuffle threads were deleted, but they shuffled
later passes, not pass 1.

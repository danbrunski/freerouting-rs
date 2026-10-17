# Q10 — Add-only completion sweep

Opened 2026-10-17 (research by freerouting-rs-f1). Java's best canary-pwr result (40)
comes from a second run with pass-1 wiring frozen (`dsn_export --fix-existing`,
`-mp 2`). Q06.5 shows 83 of 109 incomplete connections are routable on the final
board with rip-up off. That is the measured case for adding without ripping. Altium
Situs has the same idea as its "Completion" pass, which re-runs the main pass with
different costs such as cheaper vias [V, Altium docs].

Prerequisites: board parity (rung 3); A10 (`PassController`, `RipupPolicy`), A13 (best-board restore), B01 (snapshots); Q06 (the `found` verdicts that motivate it); F03.1, F03.4; measured after P04's default-tree decision. Code may start before P04.

Source: research by freerouting-rs-f1 at Dan's request (2026-10-17), `docs/roadmap/research/2026-10-17-routing-algorithms.md` (candidate N1); the preregistration draft is `docs/roadmap/prereg/Q10.json`, marked `DRAFT` until the implementer registers it (sha256s pinned) before the first number.

## What exists

- `ConnectionParams::ripup_allowed` exists: fanout uses it, and Q06's analysis search
  runs with rip-up off.
- `PassController` and `RipupPolicy` are A10 traits. The batch loop restores the best
  board (A13), and B01 snapshots exist.
- Java has no in-engine sweep: the two-run flow goes through KiCad's exporter.

## Decisions made here

- A sweep pass is Java's pass with `ripup_allowed = false` for the autoroute phase.
  Same ordering stage, same budgets.
- It starts from the board the loop would output: the best board after restores, not
  the last pass.
- Sweeps repeat until one routes nothing, capped at `sweep_passes` (default 2).
  Q04's quantum rule can replace the cap later.
- The `fixed` arm marks pre-sweep wiring as user-fixed for the sweep and unfixes it
  afterwards. That reproduces Java's two-run flow. The `free` arm leaves it unfixed, so
  pull-tight inside the sweep may move it.

## File ownership

- Creates `quality/sweep.rs`.
- Appends to `quality/mod.rs`, `quality/register.rs`, and the C02 settings struct
  (`router.autorouter.sweep = off|fixed|free`, `sweep_passes`, `sweep_after_optimizer`).

## Checklist

- [ ] **Q10.1: the sweep pass controller.** `SweepController` wraps the configured
  pass controller. When it finishes, it restores the best board and runs sweep passes
  with rip-up off.
  - **Accept:**
    - unit test: a fixture where pass 1 rips a connection that is routable afterwards
      (q06-sole-wall style) ends complete with the sweep and incomplete without;
    - off is byte-identical on `connection-parity` and `benchmark`;
    - determinism check on bm01.
- [ ] **Q10.2: the `fixed` arm.** Freeze wiring during the sweep, then unfreeze it. Compare
  with Java `7f56b062d`'s two-run flow on canary-pwr-9511d9a, item by item where
  possible.
  - **Accept:** the done note lists the differences and their causes. Byte equality
    is not required, because the Java flow goes through the KiCad exporter.
- [ ] **Q10.3: `sweep_after_optimizer`.** Optimizer, sweep, optimizer again: the
  optimized board has more room (Q06's 83 were found after the optimizer).
  - **Accept:** off is byte-identical; determinism.
- [ ] **Q10.4: measurement** (prereg draft `docs/roadmap/prereg/Q10.json (draft)`). The arms:
  - `mp1`: `-mp 1`, the baseline;
  - `mpdef`: the default passes;
  - `mp1+sweep-fixed`;
  - `mp1+sweep-free`;
  - `mp1+opt+sweep+opt`;
  - `mpdef+sweep`.

  The cheaper-via completion arm (`sweep_via_cost_factor` 0.5) goes in only if the
  prereg lists it before the first number.
  - **Accept:** measured. On canary-pwr the hypothesis is ≤ 40 KiCad-unconnected,
    clean (canary-23 grades).

## Expected

 a sweep only adds, so the router's incomplete count cannot rise. The
real guards are:
- pours cut by added traces, so KiCad's count can rise after refill (watch
  `pour_lost_pins` with Q01's pour guard on);
- vias and length;
- time.

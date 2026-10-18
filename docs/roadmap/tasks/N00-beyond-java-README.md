# N00 — Beyond Java

Opened 2026-10-17. Dan: "let's move beyond java freerouting now, and make this
better". The port reached board parity (A14: 135 of 140 boards byte-identical
to Java `a773a70ec`), and `canary-pwr` routes like Java's pass 1 (45 unconnected,
graded by KiCad). From here the goal is the best routed board, not Java's
board. Decision D10 in [port-completion.md](../port-completion.md) records the
change of direction; this series carries it out.

## What changes

- **Two profiles.** `router.profile = legacy` keeps every Java behaviour: it is
  the parity baseline, and the A14 gate, connection-parity, and the B02, B03 and
  A04 probes pin it. The default profile, `enhanced`, is the set of accepted
  improvements. The CLI, GUI and plugin use it unless told otherwise. Until N01
  lands and the first improvement is accepted, `enhanced` equals `legacy`.
  The profile is user-facing (named after Dan's suggestion, 2026-10-17): it sets
  the starting values, and each algorithm stage and routing option can still be
  chosen on its own in the GUI, CLI and plugin (N01.4).
- **A keep rule that can accept a real improvement.** The old rule, "better on
  N−1 boards and worse on none", rejects any change that moves routes like a coin
  flip per board. P04.1 is the example: total unrouted 1,281 → 1,269 and half the
  maze work, but 5 of 17 boards regressed. N02 gives a rule that judges the
  total, graded by KiCad, within a per-board noise band. The hard guards stay:
  - no new clearance violations;
  - no via in a pad;
  - determinism.
- **What stays.** D2 (deterministic), D8 (no via in a pad), preregistration
  before the first number (F03.4), the KiCad oracle (F02.6), and "a rejected
  candidate leaves no code".

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [N01](archive/N01-routing-profiles.md) ✅ | `router.profile = legacy \| enhanced`; parity tests pin `legacy`; CLI, GUI and plugin default to `enhanced` | M / medium: touches settings defaults everywhere | C02 settings table |
| [N02](N02-keep-rule-for-defaults.md) | The totals-within-noise keep rule in `freerouting-dev gate`, and where the noise band comes from | S / low | F03.1, F03.4 |
| [N03](N03-free-the-tree-again.md) | P04's canonical room order, judged again under N02 on fresh boards; if kept, it is `enhanced`'s default, and then the faster tree (P04.3) | L / high: changes routes everywhere | N01, N02; P04's saved patch |
| [N04](N04-contacts-inside-pads.md) | A trace end inside a same-net pad counts as connected (KiCad's rule) in `enhanced` | S–M / medium | N01 |
| [N05](N05-promote-accepted-stages.md) | Each accepted Q, X or K improvement becomes part of `enhanced`; the CLI registers the KRT stages | S per stage | N01; each stage's own measurement |

## Order

1. N01 and N02 first. They are independent of each other: N01 is engine and
   settings work, N02 is tooling.
2. Then N04, small and with a clear win on boards with hand-laid tracks, and N03.
3. N05 runs alongside: as each registered measurement (Q09.5, Q10–Q12, X02–X12,
   K09) accepts its candidate, it joins `enhanced`.

The routing-quality work already queued keeps its priority: Q10 (add-only
sweep), Q11 (transactional rip-up), Q12 (pass-1 portfolio), Q04 (stop rule) and
K09 (plugin export options) are the expected big wins on `canary-pwr`.

## File ownership

| Checklist | Owns (creates) | Appends to / may edit |
|---|---|---|
| N01 | `crates/freerouting/src/settings/profile.rs` | the settings table and defaults, `tests/settings_defaults.rs`, the parity tests' settings (pin `legacy`), CLI, GUI and plugin defaults, `docs/DIVERGENCES.md` |
| N02 | — | `crates/freerouting-dev/src/gate*.rs`, `docs/roadmap/routing-quality.md` (the rule section) |
| N03 | the restored canonical-order code (from `target/orchestrator/p04/p04-rejected-canonical-leaves.patch`), a new tree module | `board/search/complete_shape.rs`, `work.rs`, P04's files |
| N04 | — | `board/basic_board.rs` (contacts), its tests |
| N05 | — | the profile table (N01), `crates/freerouting-cli` (`freerouting_krt::register`, the C07 TODO) |

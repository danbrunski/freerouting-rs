# X13 — The head-to-head again: freerouting-rs against KRT, both held to the rules

Opened 2026-10-17 (owner: "sure"). X01 (2026-10-15) compared Java Freerouting with
KiCadRoutingTools (KRT), every board graded by KiCad's DRC under its original rules:
- **KRT's default completion lead came from breaking those rules.** `--fab-tier auto`
  narrowed tracks to 0.089 mm and lowered the board's minimums.
- **Held to the rules, KRT still left fewer connections unrouted and added fewer new DRC
  items on 4 of 5 boards.** Its strict mode is `--escalation off
  --no-power-tap-neckdown`. The boards were sonde, StickHub, canary-pwr and coldfire;
  interf_u went the other way.

Since then freerouting-rs has changed:
- the profiles (N01);
- Q10's add-only sweep: 33 on canary-pwr against Java's 37;
- the plugin export work (K09).

The X series ported KRT pieces into Freerouting's pipeline one at a time, and most were
rejected (X02, X06, X11). This checklist asks the question again, for the whole tool:
- Is the gap still there?
- If it is, which part of KRT's flow closes it? X01.3's `classify.py` sorts each unrouted
  connection into a failure class.

Online claims, such as KRT's author's log of 2026-02-04, give no settings and no DRC.
They are not evidence here.

Prerequisites: X01's harness (`tools/krt-h2h/`); Q10; K09 for the plugin arm.

## Decisions made here

- **Same boards as X01**: ecc83, sonde, StickHub, interf_u, coldfire and canary-pwr.
  canary-pwr is graded by canary-7a's `grade.sh` at the current placement. Every arm
  gets the same input board.
- **Arms:**
  - `fr-legacy`: freerouting-rs with profile `legacy`. It should reproduce X01's Java
    arm.
  - `fr-enhanced+sweep`: `enhanced` with `--router.autorouter.sweep=free
    --router.autorouter.sweep_passes=2`.
  - `fr-plugin`: a default-options plugin run, once K09.9 lands.
  - `krt-strict`: `--escalation off --no-power-tap-neckdown`, held to the board's rules.

  KRT's default arm is not rerun: it breaks the rules (D8).
- **Grading as in X01:**
  - KiCad DRC under the original rules, with the input board's DRC subtracted;
  - class width measured separately;
  - vias, length, time and peak memory;
  - Q15's tidiness score when it exists.
- **Report, not a keep rule.** This picks the next work; it changes no default.

## Checklist

- [ ] **X13.1 — Rerun.** Register the arms, boards and versions (freerouting-rs commit,
  KRT commit) before the first number. Then run every arm.
  - **Accept:**
    - one table, same columns as X01's plus the tidiness score;
    - `fr-legacy` matches X01's Java arm on the 4 demos, or the difference is explained.
- [ ] **X13.2 — Where KRT still wins, find out why.** Use `classify.py` on each board where
  `krt-strict` leaves fewer unrouted than `fr-enhanced+sweep`. For each class with at least
  3 connections, name the part of KRT's flow that routes them: its order, its fanout, its
  grid search, its rip-up, or its escalation within the rules. Say whether a stage like it
  exists in freerouting-rs.
  - **Accept:** a findings section that ends with each class turned into a checklist or
    dropped, with the reason. If KRT no longer wins anywhere, the done note says so.

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

- [~] **X13.1 — Rerun.** Register the arms, boards and versions (freerouting-rs commit,
  KRT commit) before the first number. Then run every arm.
  - **Accept:**
    - one table, same columns as X01's plus the tidiness score;
    - `fr-legacy` matches X01's Java arm on the 4 demos, or the difference is explained.
  - *Partial 2026-10-20:* registered, nothing routed. `docs/roadmap/prereg/X13.json`, sha256
    `10733a3bad3b6ace886e6bbe0a0baa31e0f2a870c563fab42a275f02ebe18049`, pinned by
    `crates/freerouting-dev/tests/prereg_x13.rs` (`cargo test -p freerouting-dev --test
    prereg_x13`: 3 passed); set `tools/bench/sets/x13-h2h.txt`. Versions: freerouting-rs
    `a25c82f`, kicad-plugin at the same commit, KRT `0486e799` (X01's export and venv), KiCad
    10.0.7 for every grade, no Java run (X01's recorded Java arm is the reference). Arms:
    `fr-legacy` (`--router.profile=legacy --router.scoring.start_ripup_costs=1500`, X01's Java
    flags) on pic_programmer, interf_u and sonde_xilinx; `fr-enhanced-sweep` on pic_programmer
    and sonde_xilinx; `fr-plugin` (default options) on canary-pwr, where it stands in for
    `fr-enhanced-sweep`; `krt-strict` (X01's krt flags plus `--no-power-tap-neckdown`) on
    pic_programmer, sonde_xilinx and canary-pwr. Rows, under the 2026-10-20 time rules:
    X01's prepared pic_programmer and interf_u, sonde_xilinx (prepared in the run; its hash goes
    in amendment 1 before the first route), canary-pwr at canary `2a7e070b` graded by canary's
    `grade.sh` with `tools/krt-h2h/wsl-kicad-bin`. Dropped before registration: ecc83 (0 for every
    pilot arm), StickHub (KRT's time unknown), coldfire and video (X01: 313 to 3825 s), so the
    `fr-legacy` check covers 2 of X01's 4 demos (DSN hash equal to X01's, then `ses-diff` against
    X01's Java SES). `tools/krt-h2h/run.sh` gained the arms `fr-legacy`, `fr-enhanced-sweep`,
    `krt-strict` and `ROUTE_ULIMIT_KB` (`bash -n` clean; not yet run on a board). **Heavy run
    pending**: the registration's `heavy_run` steps in order (setup, prepare, amendment 1,
    `cd $R && export X01_DIR=$X13 X01_TIMEOUT=120 ROUTE_ULIMIT_KB=8000000 && for row in
    pic_programmer sonde_xilinx; do for arm in fr-legacy fr-enhanced-sweep krt-strict; do
    tools/krt-h2h/run.sh --arm $arm --board $row --allow-load --load-wait 0; done; done &&
    tools/krt-h2h/run.sh --arm fr-legacy --board interf_u --allow-load --load-wait 0`, then the
    legacy check and the canary steps), every route under `ulimit -v 8000000`, one at a time;
    estimated 12 to 15 min. Then the table and the accept clauses (X13.1).
- [ ] **X13.2 — Where KRT still wins, find out why.** Use `classify.py` on each board where
  `krt-strict` leaves fewer unrouted than `fr-enhanced+sweep`. For each class with at least
  3 connections, name the part of KRT's flow that routes them: its order, its fanout, its
  grid search, its rip-up, or its escalation within the rules. Say whether a stage like it
  exists in freerouting-rs.
  - **Accept:** a findings section that ends with each class turned into a checklist or
    dropped, with the reason. If KRT no longer wins anywhere, the done note says so.

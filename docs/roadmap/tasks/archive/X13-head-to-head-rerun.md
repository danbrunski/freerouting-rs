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

- [x] **X13.1 — Rerun.** Register the arms, boards and versions (freerouting-rs commit,
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
  - *Done 2026-10-20:* both clauses, with two decisions (decided 2026-10-20: measurement runs
    stay a few minutes and Java compatibility is a lower priority). The table below has X01's
    columns plus the tidiness score for every freerouting-rs row; for the krt-strict rows it is
    recorded as not comparable rather than measured, because `freerouting-dev tidy` reads Java's
    contact rule (a track end touches a pin only at its centre) and KRT ends tracks on its 0.1 mm
    grid inside the pad, so it reads 1 of 86, 4 of 47 and 27 connections; extending tidy for KRT
    boards is not opened. `fr-legacy` against X01's Java arm: pic_programmer matches (`ses-diff`
    equivalent, 2/2/0); interf_u differs only from optimizer pass 3 on (Java stops at 280.98,
    Rust's pass is rejected and goes on to pass 7), the batch-optimizer difference DIVERGENCES
    already lists; coldfire and video were dropped before registration by the time rule (their
    routes take 313 to 3825 s), the 2-of-4 choice recorded at registration.
  - *Partial 2026-10-20 (run):* run as registered, every row graded, nothing timed out or hit
    the cap. Amendment 1 went in before the first route: sonde_xilinx prepared `bf51f2f7...`
    (208 tracks and 3 vias stripped, 1 zone and 25 footprints kept); X13.json sha256 now
    `ca960b3f9dc451b6d2c28c028b87be82c78bd3c55161b337bc85f9c206425150` (`cargo test -p
    freerouting-dev --test prereg_x13`: 3 passed). Binaries from the detached a25c82f worktree
    (`target/orchestrator/x13/fr-wt`, release). Demos 10:39–10:41, canary-pwr 10:42–10:48, one
    route or KiCad job at a time, `--allow-load` (load 6.6 to 8.6: Q17.4 routed canary-pwr
    beside it), so times are upper bounds. KiCad 10.0.7 for every grade; run.sh and the
    `wsl-kicad-bin` shim ran unchanged. Outputs: `target/orchestrator/x13/run/` (`runs/*/*/record.json`,
    `canary/*/grade.log`, `table.md`, `legacy-check.log`, `classes-sonde.json`).

    | Board | arm | unrouted | drc_real | vias | length mm | vias in pads | below min | under class width mm | time s | peak RSS MB | tidiness medians: detour / bends per 10 mm / acute / jogs / vias (connections read) |
    |---|---|---|---|---|---|---|---|---|---|---|---|
    | pic_programmer | fr-legacy | 2 | 2 (clearance 2) | 0 | 1541.1 | 0 | 0 | 35.6 | 0.35 | 14.7 | 1.076 / 1.446 / 0 / 0 / 0 (86) |
    | pic_programmer | fr-enhanced-sweep | 2 | 2 (clearance 1, shorting 1) | 0 | 1515.4 | 0 | 0 | 115.1 | 0.29 | 16.1 | 1.071 / 1.416 / 0 / 0 / 0 (86) |
    | pic_programmer | krt-strict | 0 | 6 (silk_overlap 6) | 38 | 1557.5 | 0 | 0 | 0.0 | 9.64 | 464.4 | not valid (1 of 86) |
    | sonde_xilinx | fr-legacy | 3 | 2 (clearance 1, shorting 1) | 3 | 563.7 | 0 | 0 | 34.7 | 0.19 | 28.5 | 1.099 / 2.236 / 0 / 0 / 0 (47) |
    | sonde_xilinx | fr-enhanced-sweep | 3 | 9 (clearance 7, shorting 2) | 3 | 568.7 | 0 | 0 | 34.7 | 0.21 | 27.8 | 1.089 / 2.236 / 0 / 0 / 0 (47) |
    | sonde_xilinx | krt-strict | 0 | 4 (silk_overlap 4) | 17 | 565.9 | 0 | 0 | 0.0 | 3.24 | 240.3 | not valid (4 of 47) |
    | interf_u | fr-legacy | 13 | 13 (starved_thermal 13) | 66 | 4968.5 | 0 | 0 | 150.9 | 41.3 | 352.1 | 1.127 / 1.829 / 0 / 0 / 0 (159) |
    | canary-pwr-2a7e070b | fr-plugin | 15 | 0 | 91 | 1641.2 | 0 | 0 | not measured; inner-layer tracks 0 | 109.2 | 155.3 | 1.181 / 4.359 / 0 / 0 / 0 (147) |
    | canary-pwr-2a7e070b | krt-strict | 14 | 6 (clearance 6) | 133 | 1649.1 | **4** | 0 | not measured; inner-layer tracks **289** (Gnd.Cu) | 213.0 | 2802.6 | not valid (27) |

    Time is the route alone (the grader's `engine_seconds`; canary-pwr `/usr/bin/time`). Starved
    thermals on canary-pwr (reported, not in drc_real): fr-plugin 1, krt-strict 2. Every
    freerouting-rs drc_real item on the demos is a track against PCB text on a copper layer
    ('GND', 'TCK-CCL', 'TMS-PROG' on sonde, '1=>>' on pic_programmer). The win rule
    (`report.comparisons.krt_still_wins`): krt-strict **wins sonde_xilinx** (0 against 3
    unrouted, drc_real 4 against 9, no via in a pad, nothing below minimum).
    pic_programmer is "fewer unrouted, rules broken" (drc_real 6 silk overlaps against 2).
    canary-pwr is "fewer unrouted, rules broken" (14 against 15; 4 vias in pads, 289 tracks on
    the Gnd.Cu plane, 6 clearance errors). canary-pwr's number to beat stays 14.
    **fr-legacy against X01's Java arm** (`legacy-check.log`). Both exported DSNs differ from
    X01's registered hashes in their first line only, the absolute path of the work directory
    (`diff`: 1 line changed).
    - pic_programmer matches: `ses-diff` "equivalent", exit 0; 2 unrouted, drc_real 2, 0
      vias, as X01.
    - interf_u differs: `ses-diff` exit 1 on 35 nets (first: /CS1-, /D0, /DIR); 66 vias
      against 75, 4968.5 mm against 4979.3, drc_real 13 against 11 starved thermals,
      unrouted 13 both. The engine logs show where. The fanout summary (26 of 62 pins escaped) and the four routing
      passes are equal (scores 973.47, 966.83, 1000, 1000) and so are optimizer passes 1 and 2 (272.90,
      280.44). At pass 3, Java's optimizer reached 280.98 (+0.19 %, below the 2.5 %
      threshold, stop). Rust's reached 279.01, was rejected and restored, and passes 4 to 7 went
      on to 351.16. This is the batch optimizer difference in DIVERGENCES ("Batch optimizer":
      Java reuses one worker board per thread, so its result depends on the thread count and
      the scheduling; X01's Java ran with `optimizer.max_threads=7`). It is not a routing
      difference.
    - kit-dev-coldfire-xilinx_5213 and video: not checked (dropped before registration by the
      2026-10-20 time rule).
    **Remains: the tidiness of the krt-strict rows.** `freerouting-dev tidy` read only 1, 4 and
    27 connections of KRT's boards. Cause: the board takes Java's contact rule
    (`contact_at_point`: a trace end touches a pin only at its exact centre), and KRT ends tracks
    on its 0.1 mm grid inside the pad (sonde /CLK-D1 ends at (111.8, 77.2), its pad centre is
    (111.76, 77.216)). A trial with `ContactRule::InsidePad` set for `.kicad_pcb` input (not
    kept) read 57 of 86 and 31 of 47. The rest are T junctions, which `tidy_copper::contact`
    drops. That trial also moved fr-plugin's canary-pwr count from 147 to 168, so the fix
    changes the fr rows read from a `.kicad_pcb` too. Owner: a Q15.1 follow-up (Q15 is
    archived; no open item). The routes and grades stand; only `tidy` is rerun on the kept
    boards.
- [x] **X13.2 — Where KRT still wins, find out why.** Use `classify.py` on each board where
  `krt-strict` leaves fewer unrouted than `fr-enhanced+sweep`. For each class with at least
  3 connections, name the part of KRT's flow that routes them: its order, its fanout, its
  grid search, its rip-up, or its escalation within the rules. Say whether a stage like it
  exists in freerouting-rs.
  - **Accept:** a findings section that ends with each class turned into a checklist or
    dropped, with the reason. If KRT no longer wins anywhere, the done note says so.
  - *Done 2026-10-20:* KRT strict wins one row under X13.1's registered rule, sonde_xilinx;
    pic_programmer and canary-pwr break the rules (X13.1). `python3 -I tools/krt-h2h/classify.py
    --out $X13/classes-sonde.json sonde_xilinx:$X13/runs/sonde_xilinx/fr-enhanced-sweep:$X13/runs/sonde_xilinx/krt-strict`
    (fr-enhanced-sweep in the java slot): 3 open in fr-enhanced-sweep, 0 in krt-strict, 0 shared.
    All 3 are **plane-net** (GND, the board's only zone, on B.Cu): PTH pad U1.2 against the
    zone, PTH pad U2.4 against the zone, and P1.2 against a GND track stub. No other class has
    an entry.

    **Findings.**
    - *Why:* freerouting-rs's copper cuts the GND pour. It puts 313.2 mm of its 568.7 mm on
      B.Cu, the pour's layer; krt-strict puts 39.4 of 565.9 mm there and changes layer more
      (17 vias against 3). Refilled with KiCad 10.0.7's `ZONE_FILLER`, the GND pour has 8
      islands on fr-enhanced-sweep's board (2149.9 mm² main, then 56.2, 25.5, 23.0, 22.5,
      0.9) against 4 on KRT's (2669.3 mm² main, three of 0.9 mm²; KRT's log gives 4 islands
      for the input). U1.2 and U2.4 sit on cut-off pieces.
    - *KRT's flow:* its grid search's plane handling. `route.py` registers a plane-fragility
      soft cost from KiCad's exact refill (log: "Plane fragility field: 152775 cells near the
      edges of 1 pour(s)", `KICAD_PLANE_FRAGILITY_COST=2.0mm-equiv`, dynamic refresh 28
      windows) and adds "POUR-LAUNCH" region targets (+30, +23). It is not order, fanout,
      rip-up or escalation: escalation is off, and the JSON summary shows no rip-up pairs and
      no layer swaps. Two things are not measured: how much of the layer split the cost
      causes, and how much comes from KRT's grid layer costs.
    - *In freerouting-rs:* nothing like it is on.
      - Q01's pour guard (`plane_obstacle_net_classes`) exists. It is opt-in per net class
        and blocks a listed pour to nets with no pin in it; in X02.5 its `on` arm equalled
        the default on the three demos it ran.
      - X02's region repair was removed after X02.5 (drc_real rose on 3 of 4 boards).
      - Q01.6, the port of KRT's `plane_fragility` as a cost hook, was never started: it
        needs Q05.2's lattice and A04's cost hook, and `docs/roadmap/routing-quality.md` §1
        keeps it as a later experiment.

    **Decision per class.** plane-net (3 connections, one board): **dropped here, no new
    checklist.**
    - It is the class X01 already found (89.9 % of Java-only failures, `planes-first`).
    - Its stage, the fragility cost, is already recorded as Q01.6 and in routing-quality.md §1.
    - 3 connections on one 48-connection board can't decide a measurement.
    - The two pour remedies that were measured did nothing (Q01) or regressed (X02.5).

    Reopening Q01.6 needs a pour board set (Q01's `q01-pours.txt`) and the Q05.2 and A04
    prerequisites.

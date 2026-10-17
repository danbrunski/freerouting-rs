# X01 — Head-to-head: Java Freerouting against KiCadRoutingTools

Opened 2026-10-15. D9 says a head-to-head of Java Freerouting against KRT,
graded by KiCad's DRC, sets the order of the X series. KRT publishes no
Freerouting numbers (its `awx/baseline_freerouting.py` harness exists, but no
results are committed), so nobody knows where either router wins. This
checklist routes Dan's `canary-pwr` and four KiCad demo boards with the Java
fork's jar and with KRT's own routing chain, grades both sessions with the
same KiCad DRC, sorts the connections one engine routes and the other doesn't
by cause, and writes the resulting order into
[X00](../X00-krt-algorithms-README.md).

It needs nothing from the port and starts now.

Prerequisites:
- F02.6 with the KRT report's grading rules (copper severities forced to
  error, silk off, input-board DRC subtracted, exit 2 "not graded") and an
  entry point that grades an already-routed `.kicad_pcb` against its unrouted
  input (needed because KRT writes a board, not a SES);
- F03.4, the preregistration format;
- F02.3's quiet-machine rule (reused, not reimplemented).

## What exists and what this adds

| Exists | Used for |
|---|---|
| Java fork `../freerouting`, built with `./gradlew executableJar` → `build/libs/freerouting-current-executable.jar` (HANDOVER); the plugin's copy is `C:\Users\thebo\Documents\KiCad\10.0\3rdparty\plugins\app_freerouting_kicad-plugin\jar\freerouting-dev.jar` | the Java arm |
| Dan's Java settings: `C:\Users\thebo\AppData\Roaming\freerouting\freerouting.json` sets `router.scoring.start_ripup_costs = 1500` | the Java arm's settings |
| The plugin's DSN step (`router_dsn.py`): rewrites the first line to `(pcb <name>` and strips lines with `search_n_strip`; command `java -jar <jar> -de <dsn> -do <ses> -host KiCad` | what Dan's real runs see |
| KRT at `0486e799`: `py_router/route.py <in.kicad_pcb> <out.kicad_pcb> --nets '*'` reads net-class widths and clearances from the sibling `.kicad_pro`; `build_router.py --from-source` builds `rust_router` with cargo; `requirements.txt`: numpy ≥ 1.22, scipy ≥ 1.7, shapely ≥ 2.0, Pillow ≥ 9.2 | the KRT arm |
| KRT `awx/baseline_freerouting.py` notes: layer user names must be canonical around the DSN round trip; footprints with no reference need `NOREF<n>`; SES `(placement …)` stripped before import; rule areas that forbid only zone fills dropped | traps for the Java arm (F02.6 owns the workarounds) |
| KiCad 10 demos in `C:\Program Files\KiCad\10.0\share\kicad\demos` | four of the boards |
| `~/code/canary/hardware/canary-pwr-rev1.0/canary-pwr.{kicad_pcb,kicad_pro,kicad_dru}` (open in KiCad: a `.lck` file exists) | the owner board, copied, never touched |
| Java fork `docs/development/tools/via_in_pad.py` | via-in-pad check for DSN + SES |

What this adds: `tools/krt-h2h/` (setup, prep, run, metrics, classify), the
X01 preregistration, the results, and the order in X00.

## Decisions made here

- **Boards** (preregistered; changes only by amendment):
  - `canary-pwr` as it stands (hand-laid copper kept), because that is Dan's
    real input;
  - `pic_programmer` (2 layers, 63 footprints), `interf_u` (2 layers, 25),
    `kit-dev-coldfire-xilinx_5213` (4 layers, 160), `video` (4 layers, 189),
    with tracks and vias stripped, zones and layer types kept, and zones
    refilled on the stripped board.
  `stickhub` (CC BY-NC-SA), `jetson-agx-thor-baseboard` and `vme-wren` (too
  large for a first run) are out.
- **Arms:**
  - `java`: the fork's jar built from a detached worktree at a recorded
    commit (the J01 pin if it has landed, else `main`), headless,
    `-de {DSN} -do {SES} --router.scoring.start_ripup_costs=1500`, Java's
    default pass limit and optimizer settings. The jar's sha256 is recorded
    and compared with the plugin's `freerouting-dev.jar` (read only).
  - `krt`: `python -I py_router/route.py {IN} {OUT} --nets '*'` with KRT's
    defaults, run from a `git archive` export of `0486e799`, with no
    `kicad-cli` and no pcbnew reachable. KRT's fallbacks (raster fill instead
    of exact fill, no oracle reconnect) are recorded from its log.
  - optional `krt-kicad`: the same with a `kicad-cli` shim on `PATH` that
    translates paths with `wslpath -w`, if X01.2's smoke test shows KRT's
    oracle calls work through it.
- **No writes into either source repository.** KRT is exported with
  `git -C ../KiCadRoutingTools archive 0486e799 | tar -x -C $X01_DIR/krt`;
  its build products land in the export. Java builds in a detached worktree
  under `$X01_DIR`. `$X01_DIR` defaults to
  `~/.cache/freerouting-rs/x01` and is outside both repos.
- **Same grader for both arms.** Every routed board is graded by F02.6 in the
  same KiCad 10 install, with the input board's DRC subtracted. "Not graded"
  (exit 2, a timeout, a crash) fails that board for that arm and is reported;
  it is never a pass.
- **Metrics per board and arm:** unrouted connections (KiCad
  `unconnected_items` after refill), `drc_real`, vias added, track length
  added (mm), vias in pads, wall time, the 1-minute load average before and
  after, and the engine's exit status. Lengths and via counts come from one
  independent script for both arms.
- **Safety timeout** 2 h per board and arm. It only stops a run.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X01.1 | the X01 preregistration (F03.4 path; proposed `docs/roadmap/prereg/X01.json`), `tools/bench/sets/x01-h2h.txt` | — |
| X01.2 | `tools/krt-h2h/setup-krt.sh`, `tools/krt-h2h/kicad-cli-shim.sh`, `tools/krt-h2h/README.md` | — |
| X01.3 | `tools/krt-h2h/build-java.sh` | `tools/krt-h2h/README.md` |
| X01.4 | `tools/krt-h2h/prep_board.py` (run with KiCad's Python) | — |
| X01.5 | `tools/krt-h2h/run.sh` | — |
| X01.6 | `tools/krt-h2h/copper_stats.py`, `tools/krt-h2h/via_in_pad_kicad.py`, `tools/krt-h2h/test_copper_stats.py` | — |
| X01.7 | `tools/krt-h2h/classify.py` | — |
| X01.8 | `tools/krt-h2h/results/x01-demos.json` | `X00-krt-algorithms-README.md` ("Order and critical path" only) |

## Pilot run, 2026-10-15 (not this checklist's protocol)

An informal head-to-head ran before this checklist could be followed. It had
no preregistration, used a scratchpad harness and a local KRT clone at
`0486e799`, and covered six boards that differ partly from X01's list. Its
numbers inform the plan but **tick no box here**; X01 still has to run as
written. Setup:
- boards stripped of tracks and vias, zones kept;
- Freerouting is Dan's `freerouting-dev.jar` (the fork's main at
  `de7a4fd2c`), headless, `start_ripup_costs=1500`, a 10-minute job cap;
  `canary-pwr` without the footprint-only fan-inlet keepout;
- KRT is `route.py` with defaults, then with board-rule settings;
- every result graded by KiCad 10.0.5's DRC under the board's **original**
  rules, minus the stripped input's own items, plus copper narrower than its
  net class (from the exported DSN; KiCad's DRC doesn't check that).

Unrouted connections, new copper DRC items, and copper narrower than its net
class:

| Board (connections) | Freerouting | KRT default (`--fab-tier auto`) | KRT strict (`--escalation off --no-power-tap-neckdown`) |
|---|---|---|---|
| ecc83 (14) | 0 · 0 · 0 mm | 0 · 0 · 0 mm | — (KRT exact sizes: 0 · 0 · 0 mm) |
| sonde (48) | 3 · 2 shorts¹ · 35 mm | 0 · 0 · 0 mm | 0 · 0 · 0 mm |
| StickHub (128) | 29 · 161² · 42 mm | 0 · 54 · 0 mm | 8 · 39³ · 0 mm |
| interf_u (164) | 13 · 11 · 162 mm | 6 · 729 · 1033 mm | 18 · 8 · 70 mm |
| canary-pwr (220) | 78 · 57 · 175 mm | 10 · 357 · 672 mm | 60 · 29 · 122 mm⁴ |
| coldfire (478) | 44 · 214² · 290 mm | 2 · 326 · 471 mm | 15 · 7 · 187 mm |

1. Shorts against copper text that KiCad's DSN export leaves out.
2. Mostly tracks below the board's minimum width: Freerouting's pin
   neck-down doesn't know the minimum, because the DSN doesn't carry it.
3. Mostly solder-mask bridges (37).
4. Cut off by the 10-minute cap (exit 124).

Findings:
- **KRT's default completion lead comes from breaking the board's rules.**
  It escalates to 0.089–0.127 mm tracks and 0.25/0.15 mm vias, lowers the
  project's minimums to match, and narrows power nets: +12V_BUS was routed
  at 0.089 mm against its 1.5 mm class. Dan's ruling (D8): design rules are
  hard limits.
- **Held to the board's rules (strict), KRT still routes more on four of
  five boards** (sonde, StickHub, canary-pwr, coldfire) with fewer new DRC
  items. It routes fewer on interf_u. It uses more vias (182 vs 130 on
  canary-pwr, 498 vs 319 on coldfire) and 1.4–4.2 GB of memory against
  Freerouting's 0.3–2.7 GB.
- **Time is mixed.** KRT strict is faster on coldfire (297 s vs 501 s) and
  slower on interf_u (386 s vs 47 s).
- **Freerouting's own losses are partly input fidelity:** copper text and
  the board's minimum width are missing from the DSN. The K series' direct
  `.kicad_pcb` reader and Java's new `router.min_trace_width_um` (C02)
  address both.
- **Grading lessons**, now in F02.6:
  - grade under the original rules;
  - measure class width separately;
  - KiCad reports at most 199 `track_width` items;
  - KiCad 10.0.5's `GetEffectiveNetClass()` is unusable from Python.

## Checklist

- [x] **X01.1 — Preregistration.**
  Write the F03.4 file before any run: boards with their source paths and
  sha256 (the prepared inputs' hashes are added by X01.4 as an amendment
  before the first engine run), arms with exact commands and settings, the
  metrics above, the grading rules, the timeout, and the decision table:
  - `grid-first`: `krt` has fewer unrouted than `java` on at least 4 of 5
    boards and no more `drc_real` on any → X03–X06 move ahead of X02;
  - `planes-first`: plane nets are at least 30 % of Java-only failures summed
    over boards → X02 (and Q01) first;
  - `neck-first`: wide nets at fine-pitch pins are at least 30 % → Q02 and
    X10 move ahead of the grid router;
  - `escape-first`: dense escapes are at least 30 % → X10 and X12 move ahead
    of X07–X09;
  - otherwise the D9 default order stands. If several fire, they apply in
    the order listed.
  - **Accept:** the file exists, a test pins its sha256 (F03.4), and its
    commit precedes the commit of any result.
  - *Partial 2026-10-16:* `docs/roadmap/prereg/X01.json` (F03.4 format, parsed by
    `Prereg::parse`): boards with source paths and sha256 (set file
    `tools/bench/sets/x01-h2h.txt`), both arms with exact commands and settings, the metrics,
    the grading rules, the 7200 s timeout and the decision table. Registered sha256
    `fd154707107403119cbd3176846131e2eb2fcc2f0bbddebc256074e532bf3085` (14:39, before the
    first engine run at 14:58; copy in `$X01_DIR/X01.prereg.original.json`). Amendments: 1
    (prepared-input hashes, `python -E -s`, krt-kicad not run) before any engine run;
    2 (`-Xmx3g` after the WSL out-of-memory crash, every java run repeated, SES identical to
    the `-Xmx4g` ones); 3, after the runs (KRT reached KiCad's exact fill through
    `/usr/bin/python3`; nothing changed, recorded). Pinned at
    `32a86ec80839cfe81522f8102cbcb0e13f513ac802869d1e5d9e4c79c14e4638` by
    `crates/freerouting-dev/tests/prereg_x01.rs` (`cargo test -p freerouting-dev --test
    prereg_x01`: 2 passed). *Done 2026-10-16:* the registration and its test are commit
    `d6a7d36`, before `9fe3be1`, which carries `tools/krt-h2h/results/x01-demos.json`.

- [x] **X01.2 — KRT in its own environment.**
  `setup-krt.sh`:
  1. record `git -C ../KiCadRoutingTools rev-parse HEAD` and
     `status --porcelain` (before);
  2. export `0486e799` with `git archive` into `$X01_DIR/krt`;
  3. `python3 -m venv $X01_DIR/venv`; `pip install -r requirements.txt` from
     the export; record `pip freeze`;
  4. `python -I build_router.py --from-source` inside the export; record
     `rustc --version` and the sha256 of the built `grid_router.so`;
  5. smoke test: route KRT's smallest fixture, `kicad_files/cap_chain.kicad_pcb`
     (made for KRT's tests), inside the export with `route.py`; check exit 0
     and an output board;
  6. try the `kicad-cli` shim once; record whether KRT's oracle call
     succeeded;
  7. record HEAD and `status --porcelain` again (after).
  - **Accept:** before and after are identical (no writes into the KRT
    repo); the smoke test exits 0; all recorded values are in
    `$X01_DIR/krt-env.json` and copied into the done note.
  - *Done 2026-10-16:* `tools/krt-h2h/setup-krt.sh` with
    `X01_DIR=target/orchestrator/x01` (the orchestrator's scratch rule). Recorded in
    `$X01_DIR/krt-env.json`: KRT HEAD `0486e799144050d4e826f71bb31acdc7275af2a2` before and
    after, `status --porcelain` empty before and after (`repo_unchanged: true`); `pip freeze`
    numpy==2.5.4, pillow==12.3.0, scipy==1.18.1, shapely==2.2.0 (venv Python 3.12.3, no
    system site-packages); `rustc 1.92.0 (ded5c06cf 2025-12-08)`, cargo 1.92.0;
    `rust_router/grid_router.so` sha256
    `2a2648fffd7e78c99db932d88080dda122505695531c6914f4d0615cbdccca0c`. Export patch: an empty
    `[workspace]` table appended to the export's `rust_router/Cargo.toml`, because `$X01_DIR`
    lies under this repository's cargo workspace (build-only). Smoke test
    `kicad_files/cap_chain.kicad_pcb`: exit 0, output board written, 4/4 nets, no fallbacks
    logged (`oracle_check: disabled`). `python -I` (as registered first) fails on Python 3.11+
    because `-I` implies `-P` (`ModuleNotFoundError: env_knobs`); the arm uses `-E -s`
    (registration amendment 1). Shim: `kicad-cli-shim.sh` as `kicad-cli` on
    `kicad_files/esp_prog.kicad_pcb` (zones; cap_chain has none): exit 0, **0 KiCad calls**
    (`oracle_check` `disabled`/`skipped`), so KRT's oracle was never exercised and the
    optional `krt-kicad` arm is not run. Note: two smoke runs of cap_chain wrote different
    bytes (`354a8f0b...`, `3bb95773...`).

- [x] **X01.3 — The Java jar.**
  `build-java.sh <commit>`: `git -C ../freerouting worktree add --detach
  $X01_DIR/fr-<commit> <commit>`, `./gradlew executableJar` there, copy the
  jar to `$X01_DIR/java/`, record its sha256 and `java -version`, remove the
  worktree. Never build in `../freerouting`'s working tree (another session
  edits it).
  - **Accept:** the jar routes `fixtures/Issue015-StackOverflow.dsn` headless
    with the preregistered arguments and writes a SES; the done note says
    whether its sha256 equals the plugin's `freerouting-dev.jar`.
  - *Done 2026-10-16:* `tools/krt-h2h/build-java.sh --frozen
    ~/code/freerouting/logs/jars/freerouting-a773a70ec.jar a773a70ec` (the orchestrator's
    instruction: Java comparisons use the frozen jar of the J01 pin; the script's worktree
    build mode exists but was not run). `$X01_DIR/java/java-env.json`: jar sha256
    `c9b7da4ad96f98f6a96e580454fc159e3490ab6cfff934a470038be487a02e5c`; `openjdk 25.0.4.1`;
    smoke `java -Xmx4g -jar ... -de Issue015-StackOverflow.dsn -do ... --gui.enabled=false
    --router.scoring.start_ripup_costs=1500`: exit 0, SES written (151930 bytes, sha256
    `8d1dd144...`), 101 s (43 unrouted, optimizer skipped). **Not equal** to the plugin's
    `freerouting-dev.jar` (sha256 `bc85d817956af24b83521b299c8f9c6a34c611ab39f063e239738a8881d75a90`).

- [x] **X01.4 — Prepared inputs.**
  `prep_board.py` (KiCad's Python through F02.6's invocation, on a copy):
  remove every track, arc and via; keep zones and layer types; refill zones;
  save; write the sha256. `canary-pwr` is copied as is (with its
  `.kicad_pro` and `.kicad_dru`) and only refilled. Grade every prepared
  input once with F02.6 to record its pre-existing DRC.
  - **Accept:** for the four demos, the prepared board has 0 tracks and vias
    and the original zone count; the originals' checksums are unchanged;
    `pgrep -f kicad` shows the same KiCad GUI processes before and after.
  - *Done 2026-10-16:* `tools/krt-h2h/prep.sh` (KiCad 10.0.7, WSL-native) →
    `$X01_DIR/boards/prep.json`. Tracks/vias/zones before → after: pic_programmer 370/6/1 →
    0/0/1; interf_u 731/84/1 → 0/0/1; kit-dev-coldfire-xilinx_5213 2935/253/3 → 0/0/3; video
    7932/808/2 → 0/0/2 (no arcs on any); canary-pwr 78/41/21 → 78/41/21 (copied, refilled
    only). Layer types equal before and after on every board. Originals unchanged
    (`sha256sum -c` per board). `pgrep -af kicad` empty before and after (no KiCad GUI
    running). Prepared sha256: canary-pwr `25d41d39...`, pic_programmer `796ce2f9...`,
    interf_u `832b5066...`, coldfire `5ef4ad9f...`, video `7c423c02...` (in
    `docs/roadmap/prereg/X01.json` amendment 1). Pre-existing DRC of each prepared demo
    (`grade_routed.sh` with routed = input): pic_programmer 86 unconnected, 5
    `lib_footprint_issues`; interf_u 164, 25; coldfire 478, 160; video 499, 189.
    canary-pwr is not graded here (canary-23 only).

- [x] **X01.5 — The runner.**
  `run.sh --arm <java|krt|krt-kicad> --board <name>`: refuses to start while
  the 1-minute load average is above 1.0 (F02.3's rule) and records it;
  - `java`: F02.6's `grade.sh` with the jar command (export DSN → route →
    import SES → DRC);
  - `krt`: `route.py` on a copy of the prepared board, then F02.6's
    grade-only entry on the result;
  - writes one record per (board, arm) with the metrics, the commands, the
    environment variables set, and the exit codes.
  Also run each arm twice on `pic_programmer` and record whether the two
  outputs are byte-identical (Java before J01 may not be).
  - **Accept:** all 5 boards × 2 arms produce a record or a "not graded"
    record with its reason; the determinism result for each arm is in the
    done note.
  - *Done 2026-10-16:* `tools/krt-h2h/run.sh` (F02.3's load rule with `--allow-load`
    recorded in each record: the machine is shared, so the 1-minute load was 1.0–14.3 before
    and 1.4–18.5 after the runs, and wall times are upper bounds). Records in
    `$X01_DIR/runs/<row>/<arm>/record.json`: 4 demos × 2 arms graded. canary-pwr × 2 have
    "not graded" records with the reason "graded only by canary-23". Java runs with
    `-Xmx3g`. Determinism, `pic_programmer` run twice per arm: **java byte-identical** (SES
    sha256 `3dcecedb...` in both runs, and the same in both `-Xmx4g` runs; interf_u and
    coldfire SES are also identical between `-Xmx4g` and `-Xmx3g`). **krt not
    byte-identical**: the two boards (`1426144a...`, `e093bc42...`) differ only in 339
    random `(uuid ...)` lines and are identical with those lines removed. Java's optimizer
    never ran (every board kept unrouted connections: "Skipping optimization stage").

- [x] **X01.6 — Metrics script.**
  `copper_stats.py <routed.kicad_pcb> <input.kicad_pcb>`: an S-expression
  scan, independent of both engines, counting vias and summing segment and
  arc lengths (an arc's length from its start, mid and end points) added
  relative to the input, per layer and in total. `via_in_pad_kicad.py`
  (KiCad's Python): vias whose centre hits an SMD pad of any net (avoid
  `BOARD.GetTracks()`, which fails in 10.0.5).
  - **Accept:** `test_copper_stats.py` on a hand-written 3-segment, 1-arc,
    2-via board gives the hand-computed length within 1 µm and 2 vias;
    `via_in_pad_kicad.py` reports exactly one via on a copy of a demo board
    with one via moved into a pad.
  - *Done 2026-10-16:* `python3 -m unittest discover -s tools/krt-h2h -p 'test_*.py'` →
    `Ran 5 tests ... OK`.
    `test_hand_written_board_gives_the_hand_computed_length_and_two_vias`: 3 segments, 1 arc,
    2 vias; total 5 + 2 + sqrt 2 + pi mm within 1 um, added length and per-layer lengths
    within 1e-6 mm, 2 vias. `test_one_via_moved_into_a_pad_is_reported_exactly_once`: on a
    copy of `kit-dev-coldfire-xilinx_5213` (0 vias in pads as shipped) with its first via moved
    onto the first SMD pad (`via_in_pad_kicad.py plant`), exactly 1. (`video` as shipped
    already has 13 same-net vias in SMD pads, so it can't be the test board.)

- [x] **X01.7 — Classify the differences.**
  `classify.py`: match each arm's KiCad unconnected items by net and pad
  pair; list Java-only, KRT-only and shared failures; give each a class, by
  precedence:
  1. `plane-net`: the net has a zone on the board;
  2. `wide-at-fine-pitch`: the net's class width exceeds the smaller
     dimension of an end pad that is fine-pitch by KRT's rule (a pad of the
     same footprint within 0.65 mm centre to centre, or a smaller dimension
     under 0.35 mm, `plane_pad_tap.pad_is_fine_pitch`);
  3. `dense-escape`: an end pad in a footprint with at least 32 pads and a
     pitch of at most 0.8 mm;
  4. `other`.
  - **Accept:** the per-board and per-class table is in the done note; three
    entries per class (where available) are checked by hand against the
    board.
  - *Done 2026-10-16:* `python3 -I tools/krt-h2h/classify.py` over the 4 graded demos
    (`$X01_DIR/classes.json`, also in `results/x01-demos.json`). KiCad unconnected items,
    matched by net and ends:

    | Board | java open | krt open | shared | java-only (plane / wide-fine / dense / other) | krt-only (same) |
    |---|---|---|---|---|---|
    | pic_programmer | 2 | 0 | 0 | 2 (2 / 0 / 0 / 0) | 0 |
    | interf_u | 13 | 18 | 1 | 12 (12 / 0 / 0 / 0) | 17 (0 / 0 / 0 / 17) |
    | kit-dev-coldfire-xilinx_5213 | 49 | 11 | 4 | 45 (40 / 0 / 2 / 3) | 7 (0 / 0 / 6 / 1) |
    | video | 170 | 111 | 12 | 158 (141 / 0 / 3 / 14) | 99 (0 / 0 / 77 / 22) |
    | **sum** | | | | 217 (195 / 0 / 5 / 17) | 123 (0 / 0 / 83 / 40) |

    Hand check by an independent regex scan of the prepared `.kicad_pcb`
    (`$X01_DIR/hand_check.py`):
    - plane-net: GND has a zone on pic_programmer (P2.19, U1.4) and on interf_u (C4.2,
      JP1.1); +3.3V on coldfire (BDM_PORT101.9, C101.2, C113.1); +5V on video (C24.1,
      C25.1, C26.1).
    - dense-escape: U102 on coldfire has 100 pads at 0.5 mm, U301 144 at 0.5; on video,
      U22 has 100 at 0.5, U23 100 at 0.65 and U11 160 at 0.65.
    - other: interf_u U5 (32 pads, 2.54 mm) and U9 (120, 2.54); video U3 (20, 1.27), U8
      (84, 2.54) and U12/U14 (75, 2.54). None is dense, and class width 0.2–0.4 mm is
      below every end pad's smaller side.
    - wide-at-fine-pitch: no entries. The only nets wider than their fine-pitch pads are
      zone nets, which plane-net takes first by precedence.
    Most Java-only plane-net items are a pad against a refilled zone ("zone" or "track"
    end): Java's DSN model counts the pour as a connection that KiCad's refill does not
    make (canary LESSONS 3).

- [x] **X01.8 — Results and the order.**
  Commit `results/x01-demos.json` (demo boards only; `canary-pwr` numbers go
  in the done note), apply the preregistered decision table, and rewrite
  X00's "Order and critical path" with the outcome and a link to this item.
  - **Accept:** a markdown table per board and arm (unrouted, `drc_real`,
    vias, length, vias in pads, time) is in the done note; the decision and
    the rule that produced it are stated; any board that couldn't be graded
    is listed as a failure of its arm. The orchestrator and Dan confirm the
    new order before X03 or X02 starts.
  - *Partial 2026-10-16:* `tools/krt-h2h/results.py` wrote
    `tools/krt-h2h/results/x01-demos.json` (demos only, paths as `$X01_DIR`, prereg sha256
    `32a86ec8...`). KiCad 10.0.7, input DRC subtracted, original rules. "below min" is
    copper below the board minimum (hard, D8); time is an upper bound (shared machine):

    | Board | arm | unrouted | drc_real | vias | length mm | vias in pads | below min | time s |
    |---|---|---|---|---|---|---|---|---|
    | pic_programmer | java | 2 | 2 (clearance 2) | 0 | 1541.1 | 0 | 0 | 9.5 |
    | pic_programmer | krt | 0 | 6 (silk_overlap 6) | 38 | 1557.5 | 0 | 0 | 16.6 |
    | interf_u | java | 13 | 11 (starved_thermal 11) | 75 | 4979.3 | 0 | 0 | 51.0 |
    | interf_u | krt | 18 | 9 | 163 | 4190.6 | 0 | 0 | 282.6 |
    | kit-dev-coldfire-xilinx_5213 | java | 49 | 218 (track_width ≥199) | 321 | 8017.9 | 0 | 301 | 484.6 |
    | kit-dev-coldfire-xilinx_5213 | krt | 11 | 10 | 480 | 8316.0 | **8** | 0 | 313.4 |
    | video | java | 170 | 206 (track_width ≥199) | 975 | 33569.8 | 0 | 858 | 3397.4 |
    | video | krt | 111 | 41 (clearance 5, shorting 1) | 1482 | 32437.2 | **8** | 0 | 3824.8 |

    Peak RSS: java 0.4–2.7 GB, krt 0.5–6.4 GB.
    **canary-pwr** (graded only by canary-23; both arms "not graded" until it reports, so
    no win for krt): java 682 s, 116 unrouted by Java's own count; krt 260 s, 159 vias and
    2018 mm added, and its own summary says 36 connections failed. KRT rewrote the routed
    project's `min_hole_clearance` from 0.25 to 0.2 despite `--no-fix-drc-settings`.
    Hand-over in `$X01_DIR/canary-handoff/` (`README.txt`; java SES `fb15b027...`, krt
    board `e0e3693f...`).
    **Decision: `planes-first`.** The rule "plane-net is at least 30 % of Java-only
    failures" fired at 195/217 = 89.9 %. `grid-first` did not (krt fewer unrouted on 3
    boards, not 4, and more `drc_real` on pic_programmer). `neck-first` (0 %) and
    `escape-first` (2.3 %) did not. X00's "Order and critical path" has been rewritten.
    *Done 2026-10-16:* canary-23 graded canary-pwr 9511d9a under the committed rules
    (Windows KiCad 10.0.5, `--schematic-parity --refill-zones --severity-all`, then
    `via_in_pad.py` and `inner_tracks.py`). **Neither arm is a valid result.**
    - java `a773a70ec` (18 passes): 47 unconnected; clearance 0, track width 4 (0.10–0.12 mm
      on LTC_PFI and PRIV_SDA: the canary runs' `--router.min_trace_width_um=150` prevents
      them), annular 0, via-in-pad 0, 4 starved thermals; 2 inner-layer tracks (LM_FB on
      Gnd.Cu, the power-layer bug Java fixed in `7f56b062d`, Q01.7).
    - krt `0486e799`: 35 unconnected; clearance, width and annular 0, 1 starved thermal, but
      22 vias in pads and 701 inner-layer tracks (299 on Gnd.Cu, 402 on Power.Cu: KRT routes
      both planes as signal layers). Its lower count comes from using the planes and pads.
    The canary result to beat stays 40 (Java `7f56b062d` add-only, clean). The order
    `planes-first` was confirmed by the orchestrator under Dan's standing instruction to take
    the recommended answer (`target/orchestrator/operator-decisions.md`).

# N04 — A trace end inside a pad is connected

Opened 2026-10-17 (D10), from KiCad's ecc83-pp demo routed with its existing
tracks. Both the port and Java `a773a70ec` end with 6 unrouted connections and
score 619.05, while KiCad reports the board fully connected.

The cause is the contact rule. Java's `Trace.getNormalContacts` and the port's
`BasicBoard::trace_normal_contacts` + `contact_at_point` connect a trace to a pin
only when the trace's end corner is exactly the pin centre. KiCad connects on any
overlap with the pad. ecc83's tracks end inside the pads but off-centre:
- 42–49 µm on all 9 pins of the valve socket U1;
- 80–381 µm on C2-2, R1-1 and R3-1.

Snapping U1's ends to the centres took the board from 6 unrouted to 2. The
verdicts show the router "routing" these connections (state ROUTED, search
"found") and still counting them incomplete for 18 passes.

Notes: `target/orchestrator/ecc83/`, and the operator-decisions entry of
2026-10-17.

Prerequisites: N01.

## Decisions made here

- **Under `enhanced`, a trace end connects to a same-net pin when the end lies inside
  the pin's pad shape on the trace's layer**, as in KiCad. `legacy` keeps
  exact-centre contacts.
- **Not a snap.** The trace keeps its geometry: the SES writes it back exactly, and
  KiCad's import sees the same copper. Only the connectivity model changes.
- **Vias:** the same rule for a trace end inside a same-net via's pad. A via
  centre inside a pin's pad also connects, as KiCad does. Check this against
  KiCad's connectivity on the demo boards before deciding; if KiCad differs, keep
  Java's rule for vias.
- **Widened (operator decision, 2026-10-18).** The first rule (the end inside the pad)
  left 3 of ecc83's 10 open: two ends 15 and 28 µm outside their pads and a T junction.
  The rule is now **the end's cap (a disc of half the trace width around the end
  point) overlaps the pin's pad shape on that layer**, by the board's exact shape
  intersection in integer units with no new tolerance. The T junction (a trace end on
  a same-net trace's segment or corner) becomes N04.4. Vias stay on Java's rule until
  N04.3's graded run has checked KiCad's via connectivity.

## Checklist

- [x] **N04.1 — The contact rule.** Under `enhanced`, `contact_at_point` (or a
  profile-aware variant) accepts a trace end inside a same-net pad shape. The
  incompletes, the routing ledger and the optimizer all use it.
  - **Accept:**
    - ecc83-pp exported with its tracks has the same unconnected count in the
      router as in KiCad (0) before routing;
    - with `legacy`, the A14 exact set is unchanged.

  *Partial 2026-10-18:* the rule is the settings key `router.pad_contacts = center | inside_pad`
  (new module `board/contact_rule.rs`; unset is `center`, Java's rule; an unknown value fails the
  job, naming the key). The job sets it on the board after reading (`job::prepare`, both
  `prepare_job` and `prepare_board_for_routing`). `BasicBoard::set_contact_rule` drops the routing
  ledger, and a `BoardSnapshot` (so every board copy, the optimizer's worker boards included) keeps
  it. Under `inside_pad` a trace end connects to a same-net pin when the end's cap (radius half
  the trace width) overlaps the pin's pad on the trace's layer (`pad_touches_trace_end`:
  `Shape::intersects_circle`, integer units, no added tolerance; a rational end tests the point
  alone). `trace_normal_contacts` adds those pins from a query box the half width around the
  end. `drill_normal_contacts` of a pin adds the traces from its pad box grown by the board's
  largest trace half width, so contacts stay symmetric. `BasicBoard::normal_contact_point` (used
  by `connection_items` and the rip-up walk) answers the end at the pad. The incompletes, the
  ledger, the DRC and the batch optimizer read these. Vias keep Java's rule (decision of
  2026-10-18). Not set in either profile (N04.3 pending).
  Tests `crates/freerouting/tests/pad_contacts.rs` (9 green):
  `inside_pad_connects_ecc83s_off_centre_track_ends` (`fixtures/Issue690-ecc83.dsn`, whose wires
  are byte-identical to the export of `/usr/share/kicad/demos/ecc83/ecc83-pp.kicad_pcb` in
  `target/orchestrator/ecc83/`: router incompletes before routing, DRC and ledger agreeing,
  `legacy` **10**, `inside_pad` **1**; 16 off-centre ends touch their pins, some 15-89 µm outside
  the pad), `inside_pad_contacts_are_symmetric`, `unset_and_legacy_keep_javas_rule`,
  `an_unknown_pad_contacts_value_fails_the_job`, `a_board_copy_keeps_the_rule_and_its_contacts`,
  `routing_ecc83_under_inside_pad_keeps_its_tracks_connected` (after routing, 20 passes: `legacy`
  6, `inside_pad` 1). **Remaining:**
  - First clause (KiCad's 0): reached with N04.4 (2026-10-18). The last one was the T junction on
    Net-(U1A-K), where a track ends on an inner corner of another track; `inside_pad` now leaves
    **0** before routing and 0 after routing (`inside_pad_connects_ecc83s_off_centre_track_ends`,
    `routing_ecc83_under_inside_pad_keeps_its_tracks_connected`).
  - Second clause, heavy run pending: with `legacy` nothing changes by construction, because every
    new path checks `contact_rule == InsidePad` (the SES snap guard too). On origin/main 9c49979
    `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p freerouting --test
    autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1` passes (1
    passed). With `PARITY_BOARDS=Issue690-ecc83,Issue163-pic_programmer,Issue690-sonde_xilinx`
    all three are `[exact] ... bytes true`. The full A14 exact set needs `cargo test -p freerouting --release --test
    autoroute_board_parity -- --ignored` (long run).
  - *Done 2026-10-18:* the second clause holds for N04.1 and N04.4 (both on 81fcef7): full A14 on origin/main 81fcef7 (release, `--test-threads=1`, then the 21 subdirectory references with `PARITY_SUBDIRS=1`; logs `target/orchestrator/heavy/a14-81fcef7*.log`): 1 passed, 0 failed, 138 boards (120 exact, 15 cutoff, 3 Java failures), every SES byte-identical except the known Issue684 and Issue721 (expected), Issue110-RelayModule (non-ASCII name only) and the Java-failure boards.
- [x] **N04.2 — Everything that walks connectivity agrees.** This covers
  pull-tight, trace tails (`remove_trace_tails`), the optimizer's connected sets,
  the DRC's dangling-trace check and the SES writer's endpoint handling (C05).
  - **Accept:**
    - a fixture with an off-centre track end: pull-tight keeps the end inside the
      pad;
    - the track is not removed as a tail;
    - the SES round-trips the end unchanged.

  *Done 2026-10-18:* the walkers read the rule through `normal_contacts`,
  `trace_normal_contacts`/`trace_{start,end}_contacts` and the new
  `BasicBoard::normal_contact_point`. That covers pull-tight's end contacts (`any_end_contact`,
  diag45), trace tails (`is_tail`, `remove_trace_tails`), the batch optimizer's candidates and
  connected sets, the DRC's unconnected items, `connection_items` and the rip-up walk
  (`autoroute/session/ripup.rs`, both `normal_contact_point` calls), plus the maze shover's
  `end_points_matching` (`shove_room.rs`). The SES writer's endpoint snap (`snapped_endpoint`,
  C05) is off under `inside_pad`, because it would move an off-centre end to the pin centre.
  Under Java's rule it cannot fire, so no session changes. Tests in `tests/pad_contacts.rs`:
  - `pull_tight_keeps_off_centre_ends_inside_the_pad`: ecc83 unfixed. Pull-tight of every track
    keeps 15 of the 16 off-centre ends where they were. The 16th, on Net-(P4-P1), 15 µm outside
    pin 315's pad, is straightened onto the pin's centre (Java's split at a drill centre) and stays
    connected. Every axis-parallel straight track left at an off-centre end then gets a 0.2 mm
    45-degree bump in its middle (3 tracks). Pull-tight changes 3 of 3, those ends stay, and
    incompletes stay 1.
  - `off_centre_tracks_are_not_trace_tails`: unfixed, `remove_trace_tails` keeps the 13 ends on
    complete nets. Under `legacy` the same tracks are tails and `remove_trace_tails` removes some.
  - `the_ses_writes_off_centre_ends_unchanged`: Java's snap would fire on some of the ends, and
    every end is written as it is. The session read back onto the bare board gives the same 16
    off-centre contacts and 1 incomplete.
  - In `inside_pad_connects_ecc83s_off_centre_track_ends` the DRC's dangling-trace check goes
    15 → 1.
  - `routing_ecc83_under_inside_pad_keeps_its_tracks_connected`: the router and optimizer keep
    every off-centre end (tracks user-fixed, as loaded).
- [x] **N04.3 — Measure.** Register the KiCad demos and corpus boards that have
  existing tracks, plus canary-pwr (with N02's rule).
  - **Accept:** measured; if accepted, N05 adds it to `enhanced`.

  *Partial 2026-10-18:* registered before any graded number as `docs/roadmap/prereg/N04.json`
  (sha256 `940e56c8…`, pinned by `crates/freerouting-dev/tests/prereg_n04.rs`, 3 green). It was
  amended once before any graded number, when the rule was widened to end-cap overlap. It uses
  N02's totals rule with `accept: no-regress` and arms `center` (baseline) against `inside_pad`,
  both under profile `enhanced`. Its 43 rows (set `tools/bench/sets/n04-pad-contacts.txt`) are 14
  KiCad 10.0.7 demos with tracks, without video, vme-wren and jetson; 28 corpus projects with
  tracks; and canary-pwr at canary `ab786fcc`. Every row is graded with
  `tools/kicad-oracle/grade.sh --keep-tracks`. The registration names the ecc83 router numbers of
  N04.1 seen before registering, under both rules (`seen_before_registration`). `pad_contacts`
  stays unset in both profiles until the verdict. **Heavy run pending** (kicad-cli, all rows),
  from the repo root with a release build:
  ```
  cargo build --release -p freerouting-cli -p freerouting-dev
  while IFS= read -r pcb; do case "$pcb" in '#'*|'') continue;; esac
    for arm in center inside_pad; do
      tools/kicad-oracle/grade.sh --out "target/n04/$arm/$(basename "$pcb" .kicad_pcb)" --keep-tracks "$pcb" -- \
        target/release/freerouting -de {DSN} -do {SES} --gui.enabled=false --router.pad_contacts=$arm
      python3 tools/kicad-oracle/via_in_pad.py "target/n04/$arm/$(basename "$pcb" .kicad_pcb)/routed/$(basename "$pcb")" \
        "target/n04/$arm/$(basename "$pcb" .kicad_pcb)/via_in_pad.json"
    done
  done < tools/bench/sets/n04-pad-contacts.txt
  # records from the summaries (router_initial_unrouted from the engine log), then:
  target/release/freerouting-dev gate --prereg docs/roadmap/prereg/N04.json --records target/n04/records.json \
    --baseline target/n04/baseline.json --write-baseline
  ```
  The same run also checks **via connectivity against KiCad** (decision of 2026-10-18). Count the
  boards where KiCad's input DRC (`kicad_input_unconnected`, tracks kept) connects a trace end
  that lies off a via's centre but on its pad, or a via whose centre is on a pin's pad, while
  the router (`router_initial_unrouted` under `inside_pad`) leaves it open. Those boards decide
  whether vias get the rule. If the verdict passes, N05 adds `pad_contacts = inside_pad` to
  `ENHANCED` (N05.2's procedure).

  *Partial 2026-10-20:* not run; cut to minutes (decided 2026-10-20: measurement runs stay a few
  minutes). Amendment 3 of `docs/roadmap/prereg/N04.json` (before any graded number; sha256
  `58f9ee9e2f7f534b76c542150b9f2ca069cfd6157372a7e901a40067668ec523`, re-pinned in
  `crates/freerouting-dev/tests/prereg_n04.rs`, 3 passed) keeps 20 rows: 4 demos (complex_hierarchy,
  ecc83-pp, pic_programmer, sonde xilinx) and 15 corpus projects whose longest recorded route took
  at most 25 s, plus canary-pwr; the amendment gives each time and its `target/orchestrator/`
  source and the 23 rows dropped (CM5_MINIMA_3, coldfire, RoyalBlue54L, tinytapeout, interf_u's
  262 s graded run, five demos with no recorded time, and ten slow corpus boards).
  `tools/bench/sets/n04-pad-contacts.txt` lists the kept rows. This replaces the command above.
  heavy run pending (from the repository root, one board at a time; about 25 minutes, of which
  canary-pwr's two routes and grades take up to about 15; the router runs under `ulimit -v
  8000000` and `timeout 600` for canary-pwr or `timeout 120` otherwise, KiCad without the cap):
  ```
  cargo build --release -p freerouting-cli -p freerouting-dev   # main, CARGO_BUILD_JOBS=2
  R=$(pwd)/target/release
  while IFS= read -r pcb; do case "$pcb" in '#'*|'') continue;; esac
    b=$(basename "$pcb" .kicad_pcb); t=120; case "$pcb" in *canary*) t=600;; esac
    for arm in center inside_pad; do
      tools/kicad-oracle/grade.sh --out "target/n04/$arm/$b" --keep-tracks "$pcb" -- bash -c \
        "ulimit -v 8000000; exec timeout -k 10 $t $R/freerouting -de '{DSN}' -do '{SES}' --gui.enabled=false --router.pad_contacts=$arm"
      python3 tools/kicad-oracle/via_in_pad.py "target/n04/$arm/$b/routed/$(basename "$pcb")" \
        "target/n04/$arm/$b/via_in_pad.json"
    done
  done < tools/bench/sets/n04-pad-contacts.txt
  ```
  then the records and the gate as above (`freerouting-dev gate --prereg
  docs/roadmap/prereg/N04.json ...`), one grade per distinct session (where `cmp` finds the two
  arms' sessions equal, the `inside_pad` row takes the `center` grade), and the via-connectivity
  count on the kept rows.

  *Done 2026-10-20 (verdict: reject; `pad_contacts` stays off in both profiles):* measured as registered
  in `docs/roadmap/prereg/N04.json`, **amendment 4** (before any graded number; sha256
  `6c05efdfffd50da9073a205863441fbb68fdd6c64a5fd98a437e90bce1d04c24`, pin test `prereg_n04.rs` 3
  passed). Amendment 4 folds in the rules decided 2026-10-20: (1) manufacturing rules are absolute: new
  hard metrics `edge_clearance`, `hole_clearance`, `clearance` and `below_minimum` redefined, all
  routing-caused (KiCad `drc_real_by_type`, routed minus the kept-tracks input), `via_in_pad` routed minus
  input, graded by `grade.sh` under the board's own `.kicad_pro` (`rules: project` on all 20 rows);
  (2) each row's ROW_FLAGS is its Board Setup minimums as `tools/bench/sets/q18-board-minimums.txt` reads
  them (`--router.board_minimums.{track_width,via_diameter,via_drill,via_annular}_um`,
  `--router.copper_to_edge_clearance_um`, `--router.hole_clearance_um`; the set file's lines carry them),
  the same in both arms; (3) all 20 rows have their own KiCad 6+ `.kicad_pro`, none dropped; (4) one KiCad
  grade per distinct session. canary-pwr's input is the git archive of canary ab786fcc (sha256 `15aa06a3…`
  as registered; `../canary` now has 2187d5c7 checked out, board `284c0308…`, so the old path no longer
  held the registered file). Engine: one release CLI of main b98074e (`freerouting 0.1.0 (git
  b98074e1d5e3)`), `ulimit -v 8000000`, `timeout` 120 s (600 s canary-pwr), none timed out. Commands
  (scripts in `target/orchestrator/n04q16/scratch/`; per row `n04-row.sh ROW PCB FLAGS`: `grade.sh --keep-tracks
  PCB -- bash -c 'ulimit -v 8000000; exec timeout -k 10 T R -de {DSN} -do {SES} --gui.enabled=false FLAGS
  --router.pad_contacts=center'`, then `inside_pad` routed on the same DSN and graded only if `cmp` found the
  SES different; `n04-records.py`; `freerouting-dev gate --records … --baseline … --prereg
  docs/roadmap/prereg/N04.json --write-baseline`, then the same without it). Outputs
  `target/orchestrator/n04q16/out/` (`n04-records.json`, `gate-n04.txt`). One driver bug before the run: the
  first launch's loop ended after its first row (stdin read by `grade.sh`), the whole run was started again;
  the first row's sessions are byte-identical in both launches.

  | row | KiCad unconnected c -> i | router unrouted c -> i | router initial c / i (KiCad input) | clearance / hole / below c -> i | drc_real c -> i |
  |---|---|---|---|---|---|
  | complex_hierarchy | 0 -> 0 | 7 -> 4 | 8 / 4 (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | ecc83-pp | 0 -> 0 | 6 -> 0 | 10 / 0 (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | pic_programmer | 0 -> 0 | 10 -> 3 | 22 / 3 (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | sonde-xilinx | 0 -> 0 | 11 -> 2 | 18 / 2 (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | cc1101-module | 0 -> 0 | 2 -> 0 | 2 / 0 (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | ch32v003-usb-devboard | 0 -> 0 | 32 -> 3 | 52 / 8 (3) | 5/0/0 -> 4/0/0 | 6 -> 5 |
  | cm4-edge-ai-carrier | 0 -> 0 | 15 -> 7 | 27 / 7 (0) | 25/0/0 -> 27/0/0 | 25 -> 27 |
  | flight-computer-core-board | 1 -> 1 | 12 -> 2 | 19 / 2 (0) | 0/0/0 -> 0/0/0 | 1 -> 1 |
  | laser-backplane-dvi | 0 -> 0 | 3 -> 2 | 4 / 2 (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | macroprocessor-diode-matrix-rom-dip | 0 -> 0 | 25 -> 12 | 60 / 52 (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | mydevboard-rp2040dev | 0 -> 0 | 46 -> 4 | 51 / 16 (0) | 1/0/0 -> 1/0/0 | 1 -> 2 |
  | nudac | 0 -> 0 | 3 -> 3 | 5 / 4 (0) | 0/18/0 -> 0/11/0 | 18 -> 11 |
  | openmuscle-flexgrid | 0 -> 0 | 6 -> 1 | 6 / – (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | pcietestpcb-pcietest | 3 -> 3 | 32 -> 26 | 378 / 279 (256) | 237/70/0 -> 234/72/0 | 480 -> 430 |
  | picodebugger-picoprobetester | 0 -> 0 | 28 -> 0 | 38 / 0 (0) | 1/0/0 -> 0/0/0 | 1 -> 0 |
  | slice-dcmt-bread-slice | 0 -> 0 | 17 -> 1 | 26 / – (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | solarmeshtasticnode | 0 -> 0 | 16 -> 2 | 20 / 2 (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | vfo-controller-catvfoctrl | 0 -> 0 | 76 -> 5 | 97 / 5 (0) | 0/0/0 -> 0/0/0 | 0 -> 0 |
  | vna-usb-pd | 0 -> 0 | 2 -> 3 | 2 / 3 (0) | 2/0/0 -> 2/0/0 | 2 -> 2 |
  | **canary-pwr** (ab786fcc) | **48 -> 77** | 66 -> 85 | 102 / 94 (153) | 0/0/4 -> 1/0/4 | 38 -> 56 |

  (c = `center`, i = `inside_pad`; "router initial" is the engine log's unrouted count before routing, a dash
  where the log has no such line; edge_clearance 0 and via_in_pad 0 on every row and arm; "below" on canary
  is 4 `annular_width` errors in both arms.) The two sessions were byte-identical (one shared grade) on 1 row
  (cc1101); the other 19 rows' `inside_pad` sessions differ, so 39 KiCad gradings of routed boards.
  Gate (`gate-n04.txt`): `totals rule: total 52 -> 81 (change +29); set noise 43.75`; **`FAILS: 1 worse by
  more than 2 x its noise band: canary-pwr`** (48 -> 77, +29, registered band 10); the other 19 rows are
  unchanged (0 -> 0 on 17, 1 -> 1, 3 -> 3). Verdict: **reject**. Hard guards in the gate: the candidate's
  `clearance`, `hole_clearance` and `below_minimum` are above 0 on canary-pwr (below 4, clearance 1), ch32v003
  (4), cm4 (27), mydevboard (1), nudac (hole 11), pcietest (clearance 234, hole 72) and vna-usb-pd (2).
  All but canary's one `clearance` also stand in the `center` arm (ch32v003 5, cm4 25, mydevboard 1,
  nudac hole 18, pcietest 237/70, vna 2, canary below 4), so they are not caused by the rule; they are
  routing-caused clearance and hole errors of the engine at b98074e, none of them the Margin-layer edge bug
  (edge_clearance is 0 everywhere). They are reported, not the ground of the reject. The ground is canary-pwr:
  `inside_pad` leaves 29 more KiCad-unconnected items (new open nets SWCLK_BOOT0 +3, +3V3_AON +3, +5V_PI
  +3, +12V_PROT +3, PI_3V3 +2, LTC_VCAPP5 +2, and singles), 19 more router-unrouted connections (66 -> 85),
  16 vs 1 `track_dangling`, one new `clearance`, though the router's initial count is lower (102 -> 94).
  Cause not diagnosed (the route differs from the first pass on).
  What the measurement shows for the rule itself: on the 19 rows whose input tracks load unfixed (`type
  route` in the exported DSN; only canary-pwr has `fix` wires) KiCad's unconnected total is 4 -> 4, with no
  row worse, and the router's own count falls on 17 of 19 rows (e.g. vfo 76 -> 5, picodebugger 28 -> 0,
  ecc83 6 -> 0 and 10 -> 0 before routing, N04.1's KiCad 0). The N04.4 open risk (shove, rip-up or cycle
  removal opening a T junction on unfixed input tracks) did not show: no unfixed row gained a KiCad
  unconnected item. The loss is on the one board with fixed hand-laid copper.
  **Via connectivity against KiCad (decision of 2026-10-18), proxy only:** a geometric count on each row's
  kept-tracks input (`scratch/via_contacts.py`, KiCad python): vias with no same-net track end at their
  centre but one inside the via's pad ("off-centre"): 10 vias on 6 boards (ch32v003 3, solarmeshtastic 3,
  cm4 1, pcietest 1, slice 1, vfo 1); vias with none whose centre lies on a same-net pad: 57 vias on 8 boards
  (nudac 16, slice 16, vfo 12, vna 7, openmuscle 2, solarmeshtastic 2, cc1101 1, flight 1). Of the 11
  boards with such a via, 8 also have a router initial count above KiCad's input count under `inside_pad`
  (ch32v003, cm4, flight, nudac, pcietest, solarmeshtastic, vfo, vna). But 6 boards without any such via
  also have a gap (complex_hierarchy, pic_programmer, sonde, laser-backplane, macroprocessor, mydevboard),
  so the gap is not only vias (pours and thermal connections are candidates), and the count cannot say
  which open connections are via contacts. The question stays open; with this verdict (reject) it does not
  gate anything.
- [x] **N04.4 — A trace end on a same-net trace is connected (T junction).** Under
  `inside_pad`, a trace end that lies on a segment or an inner corner of another trace of a
  common net on the same layer is a normal contact, as KiCad connects it. The walkers of N04.2
  agree, and copper is not changed.
  - **Accept:** ecc83-pp exported with its tracks reaches KiCad's 0 unconnected in the router
    before routing; with `legacy` the A14 exact set is unchanged.

  *Done 2026-10-18:* the full A14 exact set under `legacy` passes on 81fcef7 (see N04.1; every SES byte-identical but the known cases). Earlier note: *Partial 2026-10-18:* the test is N04.1's end-cap test against the other trace's copper: under
  `inside_pad` a trace end connects to a trace of a common net on its layer when the end's cap
  (radius half its width) overlaps one of that trace's segment shapes (`Polyline::offset_shape` at
  its half width, `TileShape::intersects_circle`, borders included, no tolerance; a rational end
  tests the point), and the end is none of that trace's ends (Java's contact). In
  `board/contact_rule.rs`: `trace_end_touches_trace`, `add_trace_end_trace_contacts` (from
  `trace_normal_contacts`, so the end's own list has the other trace) and
  `add_trace_body_contacts` (from `normal_contacts` of the other trace, at none of its ends), so the
  contact is symmetric; `normal_contact_point` of two traces answers the one end of either whose
  cap meets the other (none with several). **Decision, representation:** symmetric contacts, not a
  split at load, because the end-cap rule also connects ends that meet the other trace's copper
  off its centre line, where no split joins the ends, and because a split would change loaded
  copper's traces and the session's wires. The walkers that assume a trace contact ends at the
  same point are guarded: `connection_items` and the rip-up walk stop at a trace reached in its
  middle, as at Java's fork of the split trace (`is_trace_body_contact`); the end-corner smoothing
  (diag45, any-angle) and `swap_connection_to_pin` skip it. **Pull-tight splits** a trace at the
  junctions first when they all lie on its centre line (`t_junctions`, `split_trace_at_point`; the
  pieces are pulled tight in turn), so they become Java's end-to-end contacts, which pull-tight
  keeps; with one off the centre line it leaves the trace unchanged. Only an unfixed trace that
  pull-tight would move is split; loaded user-fixed copper never is. Legacy is unchanged by
  construction: every new path checks `contact_rule == InsidePad`. Tests in
  `crates/freerouting/tests/pad_contacts.rs` (12 green, `cargo test --release -p freerouting
  --test pad_contacts`):
  - first clause: `inside_pad_connects_ecc83s_off_centre_track_ends`, ecc83 before routing
    `inside_pad` **0** (was 1), the DRC and the ledger agreeing (`incompletes` asserts it), the
    DRC's dangling traces 1 → 0; after routing 0 (`routing_ecc83_under_inside_pad_...`);
  - `a_track_ending_on_a_track_corner_connects_both_ways`: the ending trace's end contacts and the
    other trace's `normal_contacts` hold each other, the contact point is the junction from both
    sides, neither connection walk crosses it; under `legacy` no contact;
  - `every_contact_is_symmetric_under_inside_pad`: every `normal_contacts` pair of ecc83 is seen
    from both sides at the same point;
  - `the_t_junction_end_cap_test_has_no_tolerance`: an end at the other track's copper border
    (centre line + 2 half widths) connects, one unit further it does not; pull-tight leaves the
    track unchanged with the off-centre-line end on it, then splits it at the corner junction (3
    traces end there);
  - `pull_tight_keeps_off_centre_ends_inside_the_pad` (unfixed): the junction's track is split, 3
    traces end at the junction before and after the kinks (now 4 kinked: a piece of the split
    track), incompletes 0; `off_centre_tracks_are_not_trace_tails`: 16 ends kept, incompletes 0;
    `the_ses_writes_off_centre_ends_unchanged`: read back, incompletes 0.

  `docs/roadmap/prereg/N04.json` amendment 2 (before any graded number) names the T junction;
  sha256 `f96e590d…` pinned by `crates/freerouting-dev/tests/prereg_n04.rs`. **Remaining:** second
  clause, the full A14 exact set under `legacy` (heavy run, `cargo test -p freerouting --release
  --test autoroute_board_parity -- --ignored`, shared with N04.1). On this diff
  `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p freerouting --test
  autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1` passes (1 passed,
  7 boards `[exact] ... bytes true`, 0.34 s), and with
  `PARITY_BOARDS=Issue690-ecc83,Issue163-pic_programmer,Issue690-sonde_xilinx` all three are
  `[exact] ... bytes true` (0.87 s).
  *Open risk for N04.3 (orchestrator, 2026-10-18):* on boards whose tracks load unfixed, shove,
  the optimizer's rip-up or cycle removal can move or delete a trace B under a T junction (on
  ecc83, trace 370 has both end caps on pin 228, which `trace_is_cycle` would see as a cycle).
  The junction then opens, and the incompletes count it. N04.3's graded run must report
  `inside_pad`'s unconnected count on the rows with unfixed input tracks separately, so a loss
  there shows.

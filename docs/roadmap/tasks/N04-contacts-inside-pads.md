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
- [~] **N04.3 — Measure.** Register the KiCad demos and corpus boards that have
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

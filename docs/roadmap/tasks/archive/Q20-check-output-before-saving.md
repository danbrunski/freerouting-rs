# Q20 — The router checks its output against the board's rules before saving

Opened 2026-10-20 at the owner's request. Routing never breaks the board's manufacturing rules
(decided 2026-10-20, Q18; `docs/roadmap/routing-quality.md`, order of importance item 1). Until
now each breach was found afterwards, by KiCad's DRC in a measurement:
- 0.15 mm neck-downs on 0.2 mm boards;
- copper inside the edge clearance;
- a Margin-layer line the DSN never carried.

This checklist moves the check into the router. Before the session is written, the router checks
its own routed copper against every rule it knows. It removes what breaks a rule and tries those
connections again, and it never writes breaching copper. A gap in the router's model then shows up
as a named, counted event instead of a silent violation.

Prerequisites: Q18 (the minimums and edge/hole clearances the check enforces), Q16 (dangling
stubs), Q04.3 (`quality::output_guard`, the batch loop's finish hook).

## Decisions made here

- **Only routed copper is changed.** Copper that was in the input is checked and reported once,
  never removed (Q20.4).
- **Setting `router.output_check = off | report | repair`.**
  - `repair` is `enhanced`'s default and joins `ENHANCED` without a preregistered keep rule
    (decided 2026-10-20: a correctness rule, as for Q19). Q20.6 measures what it costs.
  - `legacy` stays `off`, because `legacy` is Java.
  - `report` logs and counts but changes nothing.
- **The repair is bounded and deterministic.** It runs one retry pass over the affected
  connections, in ascending connection order, under the same floors and a fixed budget. What
  still breaks a rule is removed and left unrouted.
- **The check is the engine's DRC** (`crates/freerouting/src/drc/`), extended to cover the
  board's rules. It doesn't call KiCad. Q20.1 holds it to KiCad's verdicts.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q20.1 | `crates/freerouting/src/drc/board_rules.rs`, `crates/freerouting/tests/drc_board_rules.rs` | `drc/mod.rs`, `drc/checker.rs`, `drc/report.rs` |
| Q20.2–Q20.4 | `crates/freerouting/src/quality/output_check.rs`, `crates/freerouting/tests/quality_output_check.rs` | `quality/mod.rs`, `quality/register.rs`, the batch loop's finish, the settings table, `settings/profile.rs` (one entry) |
| Q20.3 | — | `crates/freerouting-cli/src/` (result JSON), `kicad-plugin/freerouting_rs/summary.py`, the GUI's DRC list |
| Q20.5–Q20.6 | `docs/roadmap/prereg/Q20.json`, `tools/bench/sets/q20-output-check.txt` | — |

## Checklist

- [x] **Q20.1 — The check covers the board's rules.** On routed copper, the engine's DRC
  reports each of these:
  - clearance between nets;
  - track width under the board minimum;
  - via diameter, drill or annular ring under the minimums;
  - copper-to-edge clearance, including Margin-layer and inner Edge.Cuts lines once Q18.5
    carries them;
  - hole-to-copper clearance;
  - copper of another net inside a pad's mask aperture, once Q18.7 carries the expansion;
  - vias in pads;
  - dangling stubs (Q16).

  Each item is marked routed or input.
  - **Accept:**
    - one fixture test per rule, written from scratch, with a breaching and a clean case;
    - on the Q18 board set (`tools/bench/sets/q18-board-minimums.txt`), the router's check and
      KiCad's DRC under the board's own rules agree on routing-caused items, with every
      disagreement explained in the done note.
  - *Partial 2026-10-20:* `drc::board_rules::check_board_rules` (new, re-exported from `drc`)
    reports, on the board's traces and vias, `clearance`, `copper_edge_clearance` (the outline's
    `board_edge` class), `keepout`, `track_width`, `via_diameter`, `drill_out_of_range`,
    `annular_width` (the floors of `AutorouteControl`, `BoardRuleLimits::of`), `hole_clearance`
    (copper of another net against a via's or through-hole pin's drill, `rules.hole_clearance`;
    the clearance pairs the search tree's hole inflation alone makes are left to it),
    `solder_mask_bridge` (each pad `router.solder_mask_expansions` lists, enlarged by its own front or back expansion: `BoardRuleLimits::mask_openings`), `via_in_pad`,
    `track_dangling` (end point on no copper of its net, KiCad's anchor rule) and `via_dangling`
    (Q16's `stub_contacts`); each breach is `Origin::Routed` or `Origin::Input`
    (`InputCopper`: the loaded traces and vias, a via with an input via's centre, padstack and
    nets, a trace piece whose ends lie on an input trace of its nets, layer and width), with the
    routed items a repair may remove. Tests (`crates/freerouting/tests/drc_board_rules.rs`, 13
    green; one synthetic board written in the test, a breaching and a clean case per rule):
    `clearance_between_nets`, `track_width_under_the_board_minimum`,
    `via_diameter_drill_and_annular_ring_under_the_minimums`, `copper_to_edge_clearance`,
    `copper_inside_a_keepout`, `hole_to_copper_clearance` (a pad's hole and a via's own hole),
    `solder_mask_bridge_for_the_listed_pads`, `vias_in_pads`, `dangling_traces_and_vias`,
    `input_copper_is_marked_input_and_routed_copper_routed`, `the_check_is_deterministic`,
    `a_straight_connection_breaks_no_rule`. KiCad agreement on the Q18 set: the check on Q18.6's
    kept `mp1` sessions (`board_rules_on_kept_sessions`, ignored; `Q20_DATA_ROOT=<main checkout>
    Q20_SES_DIR=target/orchestrator/q18/out/mp1 Q20_SET=tools/bench/sets/q18-board-minimums.txt
    Q20_FLAGS="<mp1's flags>"`) against those sessions' KiCad 10.0.7 grades (`drc_real_by_type`,
    routed minus input):

    | row | check, routed | KiCad routing-caused |
    |---|---|---|
    | mydevboard | via_dangling 6 | via_dangling 6, starved_thermal 1 |
    | slice | via_dangling 2 | via_dangling 2, copper_edge_clearance 2, starved_thermal 5 |
    | flight-computer | via_dangling 5 | via_dangling 5, starved_thermal 1 |
    | laser-backplane | 0 | track_dangling 33 |
    | vfo | via_dangling 11 | via_dangling 11, starved_thermal 4 |
    | canary-pwr-9511d9a | via_dangling 26, track_dangling 1 | via_dangling 26, track_dangling 1, starved_thermal 5 (the canary grade's totals) |

    The disagreements: `starved_thermal` is KiCad's zone fill (thermal spokes it pours), no
    routing rule; slice's 2 `copper_edge_clearance` are the "Segment on Margin" the DSN does not
    carry (Q18.5); laser's 33 `track_dangling` are trace ends inside a DSN plane outline of their
    net (e.g. `Net-(D1-K)`'s F.Cu polygon at 108.9..110.1 × 93.6..94.2 mm, the trace end at
    (109.975, 94.011) mm) where KiCad's fill of that zone does not reach: a plane is its DSN
    outline in the router's model, not KiCad's fill (Q16.3's known limit). The check takes
    0.001-0.011 s per board in release (that table was taken at the DRC's 1 µm tolerance; the
    check now has none, see below). Clearance and hole clearance are measured with tolerance 0
    (`drc::board_rules::CHECK_TOLERANCE_UM`, `BasicBoard::clearance_violations_within`; Q22's
    decision: KiCad reports clearance items 0.5-0.8 µm short, which the DRC's default
    `router.clearance_tolerance_um` = 1 µm would pass): `clearance_has_no_tolerance` (0.5 µm short
    is reported; the DRC at its default passes it). A breach is routed only if one of its items is
    copper the router placed (`InputCopper`), so Q22.1's 31 input-only items, moved under 1 µm by
    KiCad's `%.6g` DSN export, are reported as input and never removed. The mask rule
    (2026-10-20, after Q18.7 landed): `BoardRuleLimits::with_mask_expansions` reads the pads
    from `router.solder_mask_expansions` through `job::solder_mask::pin_expansions`, the lookup
    `apply_solder_mask_expansions` uses, so the check and the router's raised clearance classes
    name the same pins; a pad's top-layer opening is its F.Cu shape enlarged by the front
    expansion, the bottom one its last layer's shape by the back expansion, and copper that only
    touches the opening (one board unit smaller than the expansion) is no breach, because the
    router puts it exactly there. `OutputChecker::start` passes the settings, so
    `router.output_check` checks it. Tests: `drc_board_rules::solder_mask_bridge_for_the_listed_pads`
    (a trace 0.25 mm from a pad with a 0.3 mm opening is reported, one at exactly 0.3 mm or at
    0.6 mm is not, an unlisted pad's trace is not, a back-only expansion leaves an F.Cu trace
    alone while a front one flags it) and `quality_output_check::copper_inside_a_pads_mask_aperture_is_found_and_repaired`
    (`report` counts one `solder_mask_bridge` and keeps the trace; `repair` removes it, retries
    net A once, and the new route keeps out of the aperture: 0 breaches, 0 incomplete). In the
    router's model a breach of an opening is also a `clearance` breach (the listed pad's class
    is raised to the expansion), so the rule names the cause KiCad names. The Margin and
    Edge.Cuts lines of Q18.5 and the board's own mask apertures of Q18.7 are keepouts in the
    router's model (`edge_clearance`, `solder_mask`), which `check_board_rules` reports as
    `keepout` (every non-conduction area; `copper_inside_a_keepout`), never as
    `copper_edge_clearance` (that is the outline's `board_edge` class alone). What this note left
    open (copper graphics and net copper shapes as obstacles, Q22.4 `c2cc058`; `.kicad_dru`
    item-type and hole rules, Q22.5/Q22.14/Q22.15 `039a859`, `a18b970`; `hole_to_hole`, Q22.11
    `e49b89a`; the mask expansion grown by solder_mask_to_copper, Q22.12 `a36a5c0`; the Margin
    and inner Edge.Cuts keepouts, Q18.5; `.kicad_dru` on a `.kicad_pcb` by default, K13.1
    `5926b61`) has landed; the re-measurement is the Done note below.
  - *Done 2026-10-20:* the comparison again on main `454785c`, KiCad 10.0.7, release builds in
    `target/orchestrator/q201/target`, every run under `ulimit -v 8000000` and `timeout 300`.
    `board_rules_on_kept_sessions` gained `Q20_KICAD_DRU` (`<stem>=<file>` pairs: that row's DSN
    gets the `.kicad_dru` as `--kicad-dru` gives it, and the rules report is printed) and prints
    each routed breach's item nets.
    - *Kept sessions, their own DSNs* (`Q20_DATA_ROOT=<main checkout>
      Q20_SES_DIR=<main checkout>/target/orchestrator/q18/out/mp1
      Q20_SET=tools/bench/sets/q18-board-minimums.txt Q20_FLAGS="--gui.enabled=false
      --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150 -mp 1
      --router.special.diagnostics=rip-stats --stats json"`, Q18.6's mp1 flags): the same counts
      as the table above on every row. Again with `--router.board_minimums.hole_to_hole_um=250`
      (each row's `min_hole_to_hole` is 0.25 mm) and canary's own `.kicad_dru` (canary
      `9511d9a3`, `Q20_KICAD_DRU`): unchanged; `hole_to_hole` 0 on every row, as KiCad's
      `hole_to_hole` 0. Of the KiCad-graded rows only canary-pwr has a `.kicad_dru`; its 10
      rules are clearance 0.13 mm inside one footprint's courtyard (`A.insideCourtyard('Q2')`
      and so on), which the reader reports UNSUPPORTED: they only relax KiCad's clearance, and
      the check reports no routed `clearance` there, so no item is in one and not the other.
    - *Kept sessions, the plugin's DSN* (each row's board with its tracks and vias stripped by
      `tools/kicad-oracle/kicad_steps.py strip`, exported by `kicad-plugin/headless_run.py
      export` with `{"schema": 5}`, the row's plugin arguments; pcbnew only), so the check sees
      the new keepouts and copper the sessions were routed without:

      | row | check, routed | KiCad routing-caused (kept grade) |
      |---|---|---|
      | mydevboard | via_dangling 6 | via_dangling 6, starved_thermal 1 |
      | slice | via_dangling 2, keepout 2 | via_dangling 2, copper_edge_clearance 2, starved_thermal 5 |
      | flight-computer | via_dangling 5 | via_dangling 5, starved_thermal 1 |
      | laser-backplane | 0 | track_dangling 33 |
      | vfo | via_dangling 11, keepout 10 | via_dangling 11, starved_thermal 4 |

      slice's 2 `keepout`: one is KiCad's 2 `copper_edge_clearance` ("Segment on Margin" at
      (124.6, 129.15) mm against the +5V via at (124.4951, 122.7162) and its 1.40 mm B.Cu stub),
      now an `edge_clearance` keepout the check sees, the other a GNDPWR trace onto the net-tie
      NT1's B.Cu copper (`copper_graphic` keepout 127.7..129.7 × 113.2..117.2 mm): the plugin
      writes a net-tie shape as a keepout for every net, the tie's own nets too
      (`copper_graphics.py`), where KiCad lets them touch it. vfo's 10 `keepout` are traces inside
      the board's F.Mask/B.Mask shape (`solder_mask` keepouts, 160.0..175.8 × 80.9..105.9 mm): the
      Q18.7 keepout holds every net out, the net under the aperture too, where KiCad's
      `solder_mask_bridge` needs copper of two nets in one aperture. Both are the router's model
      being stricter than KiCad on sessions routed before the model, not breaches KiCad finds.
      canary-pwr's kept session (routed on `fixtures/canary-pwr/canary-pwr-9511d9a.dsn`, which
      carries 179 input wires) checked against the plugin's DSN of the unstripped board: 632
      `keepout` pairs of 11 routed traces against Q22.16's `solder_mask_pads` keepouts of one
      footprint (IC_LM61460AFSQRJRRQ1, 14 mask shapes, 2012 keepouts), 7 of them overlapping
      (the rest within the keepout's 0.2 mm clearance); every overlapped piece meets one routed
      net only and all of them are keepouts over pins of two or more nets (no net), entered next
      to the trace's own pin, which KiCad (one net per aperture) does not flag: 0
      `solder_mask_bridge` in its grade. Q22.16's module docs name this case.
    - *slice, routed fresh the way the plugin does* (`tools/kicad-oracle/grade.sh --out <dir>
      --export plugin {"schema": 5} target/corpus/projects/slice-dcmt-bread-slice/<board>.kicad_pcb
      -- bash -c "ulimit -v 8000000; exec timeout -k 10 300 <release freerouting-rs> -de '{DSN}'
      -do '{SES}' --result-json '{SES}.result.json' --gui.enabled=false {PLUGIN_ARGS}"`, default
      options, profile `enhanced`; the plugin's fix-ups wrote 186 `edge_clearance` keepouts for
      21 Edge.Cuts/Margin shapes at 0.025 mm, 2 `solder_mask`, 1 `copper_graphic`; route 2.96 s):

      | | KiCad routing-caused | check, routed |
      |---|---|---|
      | slice, fresh | 0 (`drc_real` 0; unconnected 9, input 54) | 0 (input 0) |

      KiCad's `copper_edge_clearance` is 0 in the routed board (it was 2 in mp1), the check
      finds 0 on the same DSN and session (`board_rules_on_kept_sessions` with `Q20_SET` the
      graded DSN and its plugin arguments), and the router's own `output_check` (repair) found 0
      of 113 checked. The Margin lines are not breached.
    - *Known disagreements, re-confirmed:* every `starved_thermal` item of the kept grades is a
      zone and a pad (mydevboard 1, slice 6, flight-computer 1, vfo 6 in `routed/drc.json`), no
      track or via: zone fill, and the current grader (Q22.2's routing-caused, an item involving
      a routed track or via) no longer counts it (fresh slice: `starved_thermal` 2 routed, 1
      input, `drc_real` 0). laser's 33 `track_dangling`: each of the 33 tracks has an end inside
      a DSN `plane` polygon of its net (25 at KiCad's item position, 8 at the other end), so a
      plane is its DSN outline in the router's model, not KiCad's fill.
    - No routing-caused item KiCad reports is missed by the check once the check sees the
      board's keepouts; every item the check adds is one of the router's stricter models named
      above.

- [x] **Q20.2 — Check and repair before saving.** After the optimizer, and before the session is
  written, run the check on routed items:
  - `report` logs and counts;
  - `repair` removes the breaching routed items, retries the affected connections once, checks
    again, and removes what still breaches.
  - **Accept:**
    - a test hook makes a stage emit a breaching trace and a breaching via, and the written
      session has neither;
    - the connections are retried, and the ones that can't be routed within the rules are left
      unrouted;
    - two runs give identical sessions;
    - with `off`, connection-parity stays 7 of 7 exact under `legacy`;
    - on a board where the check finds nothing, the session is byte-identical to `off`'s.
  - *Done 2026-10-20:* setting `router.output_check` (`settings/fields.rs`; unset is `off`;
    an unknown value fails the job before routing), `quality::output_check` (new):
    `OutputChecker::start` on the board as loaded (input copper, Q20.4's log line),
    `OutputChecker::finish` in `run_pipeline` after the optimizer, its board stages and Q16's
    stub cleanup (also after a stop; no retry after a full stop): `report` logs and counts;
    `repair` removes the routed items of every breach and the stubs that leaves (input copper
    kept), retries the incomplete connections of the affected nets once (`retry_pass`: pass 1 of
    a fresh pipeline of the job's stages, ordering filtered to those nets in ascending (net,
    item) order, rip-up off, no repeated-failure rip-up, its own stop token), checks again and
    removes what still breaches until a round finds none. `RunSummary::output_check` and
    `PipelineOutcome::output_check` carry the counts; the final figures are recomputed when the
    check changed the board. `ENHANCED` gets `output_check = repair` (decided 2026-10-20, no
    keep rule); DIVERGENCES lines under "freerouting (board model)" and "Profile `enhanced`".
    Tests (`crates/freerouting/tests/quality_output_check.rs`, 9 green, fixture
    `tests/data/quality/q20-output-check.dsn`, hook stage `Breach` after the optimizer: a 0.1 mm
    trace for net A in place of its wiring and a lone 0.4 mm via of net D, minimums 0.2 mm track
    and 0.5 mm via): `repair_removes_the_breaching_trace_and_via_and_retries_them` (found
    `track_width` and `via_diameter`, retried 1, repaired 1, left open 0, the session has no
    `Via[0-1]_400:300_um` and no trace under 2000 units, the board complete);
    `a_connection_that_fits_only_below_the_rules_is_left_unrouted` (q18-neck-floor: PWR2's
    0.12 mm trace removed, retried, left open, nothing under 0.2 mm saved);
    `two_runs_give_the_same_session`; `report_counts_and_changes_nothing` (and `off` writes the
    same session); `a_clean_board_gives_the_session_of_off` (found 0, SES byte-identical);
    `the_profiles_set_repair_and_off`; `modes_parse_and_unknown_values_are_refused`. Legacy:
    `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p freerouting --test
    autoroute_board_parity -- --ignored autoroute_board_parity`: 7 of 7 `[exact]`, `bytes true`
    (run after Q20.2 and again on the final tree, tolerance 0 included).

- [x] **Q20.3 — Say what the check did.** Report it in four places:
  - the run log, one line per rule plus a total;
  - `--result-json`, a field `output_check: {checked, found, removed, retried, repaired,
    left_open, by_rule}`;
  - the plugin's result summary;
  - the desktop app's DRC list.
  - **Accept:** a test per surface, and the fields documented in the CLI help and the plugin
    README.
  - *Done 2026-10-20:* run log: `Output check (<mode>): N routed trace(s) and via(s) checked, F
    breach(es) of the board's rules found.`, one `Output check: <rule> <n>` line per rule, then
    the repair's totals, with no time so equal runs log equal lines (`two_runs_give_the_same_session`
    compares the logs; the times are `OutputCheck::check_seconds` and `seconds`;
    `repair_removes_the_breaching_trace_and_via_and_retries_them`
    reads `track_width 1`; CLI `result_json_output_check.rs::the_run_log_has_a_line_per_rule_and_a_total`).
    `--result-json`: key `output_check` after `widths` with `{mode, checked, found, removed,
    retried, repaired, left_open, remaining, by_rule, input}` (`OutputCheck::to_json`; no time,
    so two equal runs write equal manifests but for durations), absent with `off`
    (`result_json_output_check.rs::the_manifest_carries_the_output_check`;
    `result_json.rs::a_routed_run_writes_every_key` now expects Java's keys plus `output_check`
    under `enhanced`); documented in `crates/freerouting-cli/src/help.txt` (`--result-json`).
    Plugin: `summary.output_check_lines` (a **Rule check** line, the input's violations, a
    warning when breaches were saved; `test_summary.py::test_the_output_check_is_summarised`;
    `python3 -m pytest kicad-plugin/tests`: 367 passed, 19 skipped), documented in
    `kicad-plugin/README.md`. Desktop app: Info > Board Rules (`panels/board_rules.rs`, a list
    window of the engine's check on the open board under its own rules;
    `panels::board_rules::tests::the_list_shows_the_boards_breaches`), and the run's output-check
    lines reach the GUI log with the job log; `docs/development/gui.md` updated.

- [x] **Q20.4 — Input copper is reported, not changed.** Violations already present in the
  input are logged once at load with their count and rule ("the board already has N
  violations: …"). The repair never removes or moves input copper.
  - **Accept:** a fixture with a pre-existing clearance violation and a pre-existing narrow
    track routes with both kept and reported, and nothing new breaks a rule.
  - *Done 2026-10-20:* `OutputChecker::start` checks the board as loaded once and logs
    `Output check: the board already has N violation(s): <rule n, ...>; input copper is
    reported, never changed.`; the counts are `OutputCheck::input` (the manifest's `input`). The
    repair removes only items `InputCopper` does not contain, and its stub removal keeps every
    input piece. Fixture `tests/data/quality/q20-input-violations.dsn` (net E's input track
    0.1 mm wide, net F's input track 0.1 mm from it; A and D to route): 
    `input_violations_are_reported_once_and_kept` (input `track_width` 1 and `clearance` 1,
    the load line logged once, found 0 and removed 0 after routing A and D, the 0.1 mm track
    still on the board), `the_repair_never_removes_input_copper` (the hook's breaches removed,
    every input trace and via id still on the board).

- [x] **Q20.5 — Register the cost measurement.** Write `docs/roadmap/prereg/Q20.json` before
  any number:
  - **Arms:** `off` and `repair`.
  - **Boards:** the Q18 set's KiCad 6+ rows and canary-pwr, a few minutes in all.
  - **Grading:** one KiCad grade per distinct session.
  - **Guards:** zero routing-caused breaches in the `repair` arm.
  - **Report:** KiCad unconnected, route time and the check's own time.
  - **Accept:** the registration and its pin test are in, with no number yet.
  - *Done 2026-10-20:* `docs/roadmap/prereg/Q20.json` (id `Q20.6-output-check-cost`, arms `off`
    and `repair` differing only in `--router.output_check`, Q18.6's `mp1` flags; set
    `tools/bench/sets/q20-output-check.txt`: canary-pwr-9511d9a, mydevboard, slice,
    flight-computer, laser-backplane and vfo with Q18's row flags; one KiCad grade per distinct
    session; hard `via_in_pad` and `check_remaining` (the repair arm's saved breaches, must be
    0), guard `kicad_rule_breaches`, report KiCad unconnected, unrouted, route and check
    seconds), registered before any run, sha256 246ab47e…, pinned by
    `crates/freerouting-dev/tests/prereg_q20.rs` (3 green: valid with its arms and board
    hashes, the arms differ only in the check and the rows are Q18's, the hash).

- [x] **Q20.6 — Measure the cost.** Run Q20.5 as registered.
  - **Accept:**
    - the per-board table is in the done note;
    - `repair` has zero breaches;
    - wherever the check found nothing, the sessions are identical;
    - the check's time is under 5 % of route time on every row, or the rows over it are
      named;
    - the `ENHANCED` entry and a DIVERGENCES line are in.
  - *Done 2026-10-20:* run as registered on `land/q20` (main 91b540d plus the Q20 diff, the
    check at tolerance 0 per Q22; release CLI sha256 0f32f198…), `tools/bench/quick-arm.sh`-style
    routes under `ulimit -v 8000000` and timeout, outputs in `target/orchestrator/q20/out/<arm>/`,
    graded once per distinct session by `tools/bench/grade-distinct.sh` (`kicad-cli` only while
    none ran). Per row, `off` → `repair` (route seconds are one run each; the check's figures from
    the repair log's `Output check:` lines and the manifest's `output_check`; the measuring build
    still logged the stage's and the checks' seconds, which the final tree keeps off the log so
    that equal runs log equal lines, `quality_transaction.rs::off_is_byte_identical`):

    | row | KiCad unconnected | unrouted | route s | check s (share) | stage s (share) | found | removed | retried | repaired | left open | remaining | KiCad covered breaches | SES |
    |---|---|---|---|---|---|---|---|---|---|---|---|---|---|
    | canary-pwr-9511d9a | 46 → 33 | 110 → 96 | 52.01 → 58.84 | 0.013 (0.02 %) | 6.399 (10.9 %) | 27 (track_dangling 1, via_dangling 26) | 51 | 39 | 14 | 25 | 0 | 27 → 0 | differ |
    | mydevboard | 56 → 56 | 62 → 62 | 3.79 → 4.31 | 0.005 (0.1 %) | 0.221 (5.1 %) | 6 (via_dangling) | 10 | 7 | 0 | 7 | 0 | 6 → 0 | differ |
    | slice | 11 → 11 | 6 → 6 | 0.41 → 0.45 | 0.002 (0.4 %) | 0.004 (0.9 %) | 2 (via_dangling) | 3 | 0 | 0 | 0 | 0 | 4 → 0 | differ |
    | flight-computer | 6 → 4 | 4 → 3 | 0.30 → 0.53 | 0.008 (1.5 %) | 0.112 (21 %) | 5 (via_dangling) | 11 | 3 | 1 | 2 | 0 | 5 → 0 | differ |
    | laser-backplane | 70 → 70 | 45 → 45 | 0.39 → 0.52 | 0.001 (0.2 %) | 0.001 (0.2 %) | 0 | 0 | 0 | 0 | 0 | 0 | 33 → 33 | identical |
    | vfo | 14 → 12 | 1 → 0 | 0.42 → 0.58 | 0.004 (0.7 %) | 0.035 (6.0 %) | 11 (via_dangling) | 18 | 1 | 1 | 0 | 0 | 11 → 0 | differ |

    - *Zero breaches:* `check_remaining` (the manifest's `output_check.remaining`) is 0 on every
      row. KiCad's covered-type breaches (`kicad_rule_breaches`) are 0 in `repair` on five rows;
      laser-backplane keeps its 33 `track_dangling` in both arms (identical sessions): trace ends
      inside a DSN plane outline that KiCad's fill does not reach, a breach the router's model
      cannot see (named in the registration and Q20.1). slice's 2 `copper_edge_clearance` (off)
      were the dangling +5V via at (124.50, 122.72) and its 1.40 mm stub against the Margin line;
      `repair` removed both. `via_in_pad` is 0 in both arms on every row (canary's
      `via_in_pad.py`; KiCad and the check report none on the corpus rows). Not covered and
      unchanged or fewer: `starved_thermal` (zone fill) 5/5 slice, 1 → 0 mydevboard, 1/1
      flight-computer, 4 → 3 vfo, 5/5 canary.
    - *Identical where nothing was found:* laser-backplane is the only row with `found` 0, and
      its `off` and `repair` sessions are byte-identical (`cmp`); its grade is shared.
    - *Time:* the check itself is under 5 % of route time on every row (at most 1.5 %). The
      whole stage (checks, removals and the retry pass) is over 5 % on canary-pwr (10.9 %),
      flight-computer (21 %), vfo (6.0 %) and mydevboard (5.1 %); all of that is the retry pass.
    - *Score:* KiCad unconnected is never worse (−13 canary, −2 flight-computer, −2 vfo), as is
      unrouted (−14, −1, −1): the retry pass also routes connections of the affected nets that
      were open before the check.
    - *ENHANCED and DIVERGENCES:* `settings/profile.rs` sets `output_check = repair` in
      `ENHANCED` (Q20.2); `docs/DIVERGENCES.md` has the `output_check` line and the profile line.

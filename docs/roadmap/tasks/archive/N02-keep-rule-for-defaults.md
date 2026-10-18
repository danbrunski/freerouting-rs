# N02 — A keep rule that judges the total within noise

Opened 2026-10-17 (D10). The routing-quality rule "better on N−1 boards, worse on
none" (routing-quality.md) treats every board as a vote. A change that moves
routes changes most boards by a few connections either way, so the rule rejects
it even when the total improves clearly. P04.1: −12 unrouted over 17 boards and
half the maze work, yet 5 boards regressed.

## Decisions made here

The rule for a change to profile `enhanced`:
- **Hard guards, unchanged:**
  - no new clearance violations (KiCad `drc_real` where graded);
  - no via in a pad;
  - D2 determinism.
- **Signal: KiCad-graded unconnected**, summed over the registered set. Where a
  board has no `.kicad_pcb`, the router's own unrouted count is used, reported
  separately.
- **Accept when both hold:**
  1. the total improves by more than the set's noise, which is the square root of
     the sum of the per-board noise bands squared;
  2. no board regresses by more than twice its own noise band.
- **The noise band of a board** is its order spread: the half-range of
  unconnected over K pass-1 orders (Q12.4). Until Q12.4 has measured a board, the
  band is registered as max(2, 5 % of the board's connections). On `canary-pwr`
  the variants' spread of ±10 is used.
- **Speed changes** are accepted when quality is within noise by the rule above
  and the registered work measure drops by at least 20 %.
- **At least 3 distinct boards, still.** The `canary-pwr` variants count as one
  board.
- **Existing registrations keep their rules.** Their numbers exist, so changing
  their rule now would be editing the test after seeing the result. A candidate
  rejected under the old rule can be judged again only by a new registration on
  boards or variants it hasn't been measured on, such as the PCBWorld D3-C set
  (`tools/bench/sets/pcbworld-d3c.txt`).
- **A rejected candidate is kept as an opt-in or deleted** (proposed 2026-10-17). It
  stays as an option, off by default and listed under "Experimental" in the
  algorithm settings (N01.4), only when all three hold:
  1. **it wins on a class of board you can name** (layer count, pin density, BGA
     fanout, planes, size), shown by the measurement's own per-board table. Its
     help text names the class and gives the numbers;
  2. **it passes the hard guards** above;
  3. **it is cheap to keep:** a self-contained stage or setting, not a hook through
     the engine's cost or search code.

  Otherwise its code is removed. Its numbers stay in its checklist, and git
  history keeps the code. The kept Java bugs (N06) are not candidates: `legacy`
  needs them, so they always stay switchable.

## Checklist

- [x] **N02.1 — The rule in `freerouting-dev gate`.** Add a `--rule totals` mode next
  to today's paired rule. It reads each board's noise band from the
  registration (`noise` per board, or a formula).
  - **Accept:** self-tests cover a P04.1-shaped table, which passes; a total that
    improves only within noise, which fails; and a board beyond twice its band,
    which fails.
  *Done 2026-10-17:* `freerouting-dev gate --rule totals` (module `gate_totals.rs`;
  `--accept improve|no-regress`, `--noise BOARD=BAND`, `--speed METRIC[=PERCENT]`; with
  `--prereg` the registration states them and a differing option is REFUSED). The rule is as
  Decisions states it: hard guards (hard metrics, guard metrics, D2 through the baseline
  comparison), total improvement strictly above sqrt(sum band^2), no board worse than 2 x its
  band, band registered per board or max(2, ceil(5 % of `connections`)), rows of a `board`
  summed and counted once, >= 3 boards, optional speed clause (quality within noise and the work
  measure down >= 20 %), `no-regress` for N06. Self-tests in `gate_selftest.rs` (every
  invocation) and `crates/freerouting-dev/tests/gate_totals.rs` (21 tests, `cargo test -p
  freerouting-dev`: all pass). P04.1-shaped table (P04's measured counts, illustrative bands,
  maze work 39.1 G -> 19.6 G): passes **through the speed clause**; on quality alone it fails,
  because -12 is below the set noise (52.6 with the illustrative bands, 12.81 even with the
  smallest possible bands, 2 per board and 10 for canary-pwr). So "a P04.1-shaped table, which
  passes" holds for the speed clause only; the Decisions' rule cannot pass P04.1 on the
  unrouted total (the doc says so). Also pinned: improvement equal to the noise fails; a board
  at exactly 2 x band passes, above fails; hard guard fails a great total; < 3 boards refused;
  canary variants count once.
- [x] **N02.2 — Registration format.** The registration states `"rule": "totals"`,
  the per-board noise source, and the speed clause if used. `prereg` tests pin it
  as before.
  - **Accept:** the format is documented in routing-quality.md, and a sample
    registration validates.
  *Done 2026-10-17:* `"rule": "paired" | "totals"` and the `totals` block (`accept`, `signal`,
  `noise`, `min_boards`, `speed`) in `gate_prereg.rs`; unknown keys, a missing `accept`, a
  `min_boards` below 3, a speed drop below 20 %, a speed clause with `no-regress`, an
  unregistered signal or speed metric, and a noise band naming no run board are all refused or
  malformed (tests `a_bad_totals_registration_is_refused_or_malformed`,
  `a_noise_band_for_a_board_the_run_does_not_have_is_refused`). Documented in `tools/ab/README.md`
  ("The totals rule") and the module docs. Sample: `tools/ab/examples/n02-totals.prereg.json`,
  validated, sha256-pinned and run against a synthetic run in `gate_totals.rs`. Q14's registration
  (`"rule": "totals"`, prose `totals_rule`, no block, hash unchanged) still parses and
  `prereg_q14` passes; a run against it is REFUSED ("no `totals` block"), so Q14.3's verdict stays
  hand-computed unless an amendment adds the block.
- [x] **N02.3 — routing-quality.md.** Amend "How to measure a quality change":
  keep the paired rule as the rule for stages that are opt-in, and use the totals
  rule for changes to `enhanced`. Record the reason, with P04.1's numbers.
  - **Accept:** the amendment is in the doc, dated, with a pointer to D10.
  *Done 2026-10-17:* `routing-quality.md`, "How to measure a quality change", bullet "Which rule
  judges (amended 2026-10-17, N02, D10)": paired rule for opt-in stages, totals rule for changes
  to `enhanced`, the reason with P04.1's numbers (1,281 -> 1,269, maze work 39.1 G -> 19.6 G, 5
  regressing boards, the clearance guard failures), and the plain statement that P04.1 passes
  only through the speed clause.
- [x] **N02.4 — Sort the rejected candidates.** Apply the keep-or-delete rule to
  every rejection so far: Q07 conflict-rounds and inside-out, P04's rejected arms,
  and X02, X06 and X11, which are already removed. For each one record the board
  class it wins on, or that there is none.
  - **Accept:** a table in this file. Each kept option is under "Experimental" with
    its help text; nothing else rejected is left in the code.
  *Done 2026-10-18:* every rejection found by grepping `docs/roadmap/tasks/` and
  `tasks/archive/` for "rejected", for gate FAIL lines and for "stays off" is in the table. The
  rule applied is the Decisions' (keep only if it wins on a nameable board class, passes the hard
  guards and is self-contained; otherwise delete). "Hard guards" are N02's: no new clearance
  violation, no via in a pad, D2 determinism. A unconnected/DRC regression counts as failing a
  guard where the candidate's own registration made it one.

  | # | Candidate | Rejected at | Board class it wins on (numbers from its note) | Verdict | Where the code is now |
  |---|---|---|---|---|---|
  | 1 | Q07 `conflict-rounds` pass-1 order | Q07.4, `prereg/Q07.json` (`b31fb555…`), gate FAIL: improved 9 of 57, regressed 24 | None. Improvers span 2-6 layers, 0-6 planes, 27-375 nets, and every range also holds regressions. Unrouted 1,810 → 1,884 over 61 rows, pass 1 2,615 → 2,712; KiCad unconnected 981 → 1,038, drc_real higher on 16 of 44 projects | **delete** (no class; guard: KiCad drc_real rises on 16 projects) | Not in the tree as an ordering: tag `archive/q07-orderings` (379b413). Re-applied by Q12 (b599290) **only as portfolio arms** (`quality/ordering/`, not registered as `router.autorouter.ordering` ids), so nothing selectable is left; Q12.5 judges the portfolio. |
  | 2 | Q07 `inside-out` pass-1 order | Q07.4, `prereg/Q07-inside-out.json` (`b32f7baa…`), gate FAIL: improved 6 of 36, regressed 21 | None. The six improvers are 4-6 layer corpus boards with 2-15 plane scopes, but so are five of the regressions. Unrouted 1,685 → 1,824; KiCad unconnected 855 → 918 | **delete** (same) | Same tag; same Q12 note. |
  | 3 | P04.1 canonical room order (`router.room_order = canonical`) | P04.1, `prereg/P04.1.json` (`237311ec…`), gate FAIL: improved 2 of 17, regressed 5-6 | None yet. Total unrouted 1,281 → 1,269 and maze work 39.1 G → 19.6 G nodes (a speed win on every board), but LogicBoard 285 → 274, contribution-board 57 → 43, GP8B 61 → 51 are offset by CPU-85 +7, smoothieboard +9, canary-pwr +5; GP8B router-counted clearance 0 → 2 (N03.2); interf_u KiCad drc_real 9 → 11 | **re-judged in N03.3** (totals rule, boards P04 did not use). Kept pending N03.3, listed under Experimental | N03.1 restored it: `board/search/complete_shape.rs`, `settings/fields.rs` `room_order`, `tests/search_order_independence.rs`. Off in both profiles. |
  | 4 | P04.2 leaf work unit (`router.work_unit = leaves`) | P04.2, `prereg/P04.2.json` (`187cad44…`), gate FAIL: improved 1 of 17, regressed 3 | None yet. Against `canonical`: unrouted 1,269 → 1,279, contribution-board 43 → 37 the only improver, canary-pwr 534 → 550 | **re-judged in N03.3**; kept pending it, listed under Experimental | N03.1: `board/search/work.rs`, field `work_unit`, same test file. Off in both profiles. |
  | 5 | X02.3-X02.4 pour region repair (`krt-region-repair`) | X02.5, `prereg/X02.json`, gate FAIL: improved 1 of 4, regressed 3 | Boards with split pours: islands lost fall on every board (interf_u 27 → 13, pic_programmer 5 → 0, complex_hierarchy 82 → 36, canary-pwr 18 → 1). But drc_real rises on pic_programmer 2 → 5, complex_hierarchy 0 → 7 and canary-pwr 33 → 35, and canary-pwr KiCad unconnected 48 → 49 | **delete** (guard fails: drc_real up on 3 of 4; new shorting items) | Removed 2026-10-17. `grep -ri "krt-region-repair\|region_repair" crates` finds comments only. Kept by X02.5: `planes::components`, the tap report. |
  | 6 | X06.1 `krt-astar` bundle (grid A* + pull-tight) | X06.3, `prereg/X06.json`, hard-metric stop | None. pin-field: ties (bm07 0 of 39 unrouted both arms). open: ties on ecc83, bm02, bm07, pic_programmer, sonde 11 → 18 unrouted | **delete** (no class; drc_real 2 → 6 on pic_programmer) | Removed 2026-10-17. No `krt-astar`/`KRT_ASTAR` in code. |
  | 7 | X11 `krt-bus-order` and `krt-bus` (bus ordering, attraction) | X11.4, `prereg/X11.json`, first board | None. Issue039: unrouted 6 → 10 and 6 → 9, bus vias 27 → 28/33, bus length 372.5 → 394.8/402.3 mm | **delete** | Removed 2026-10-17 with the engine cost hook. `grep -ri "krt-bus\|connection_cost" crates` finds comments only. `tools/bench/sets/x11-bus.txt` stays (the registration names it). |
  | 8 | X05 `krt-grid` connection router, with the X04 raster, X03 vendored `krt-grid-router` and X06.2 region classes | X05.6, `prereg/X05.json`, gate FAIL: improved 0 of 10, regressed 4 (TestSensel 12 → 3 routed, Issue143, Issue413 1 → 0, Issue508-SMD 5 → 2); median 2.0-119 ms against 0.2-11 ms; X06.3 then found no class either (open: sonde loss) | **delete** (no class; kept only because X06 left it when it removed the bundle) | **Deleted by this item** (last on main at b599290; restore with `git checkout b599290 -- <paths>`): `crates/krt-grid-router/`, `freerouting-krt/src/grid/`, `tests/{grid_router,raster,astar_bundle}.rs`, five `x05-*`/`x06-*` DSNs, `tools/krt/*`, the PROVENANCE and THIRD_PARTY_NOTICES rows. Moved: `NetRules`, `ViaChoice` and `via_in_pad`, which the X10/X12 escape stubs use, to `freerouting-krt/src/fanout/rules.rs`. Test `krt_stages::a_krt_stage_is_accepted_and_krt_grid_is_gone`. |
  | 9 | X08/X09 `krt-diffpair` (coupled differential pairs) | X09.5, `prereg/X08.json`, `X09.json`, gate FAIL on picodvi | **Boards with one or a few pairs that share a signal layer** (USB, a connector pair): coupled share 0.91-0.99 on x08-straight, x08-connector, x09-two-layer, x09-skew, rp2040dev; via_in_pad 0 and 0 new violations everywhere. Loses on pair-dense boards: picodvi (10 pairs) share 0.04 and single-ended unrouted 3 → 4, pcie-active-backplane (33 pairs) 0.28 | **keep, Experimental** (class named, hard guards hold, a self-contained board stage) | `crates/freerouting-krt/src/diffpair/`. Marked in `catalog::EXPERIMENTAL`; `AutorouteControl::destinations_only` stays (its hook is a single flag). |
  | 10 | Q14/N05.3 `router.via_clearance = net_class` | Q14.3 (`prereg/Q14.json`) and N05.3 (`prereg/N05-via-clearance.json`, `75794c38…`), gate FAIL on overlord's class-via count 15 | **Boards whose net classes ask a larger via clearance than the default class:** canary-plugin45 class-via errors 1 → 0 and drc_real 1 → 0, overlord drc_real 249 → 209 (class-via errors 21 → 15; 35 → 8 in Q14.3), `mp1sweep` clean at 33. Hard guards: it adds no violation and no via in a pad; the only hard clause that failed is overlord's class-via count of 15, which the baseline has too (21) and which is KiCad's *board-minimum* clearance, not a netclass one. Cost: KiCad unconnected 37 → 44 (plugin45), 45 → 49 (mp1) | **keep, Experimental** (self-contained: a mode read in `AutorouteControl::init_net`, `via_clearance.rs`). Stays opt-in while a new registration, counting netclass violations only, is planned. *Later 2026-10-18:* that registration (`prereg/N05-via-clearance-netclass.json`) passed, and N05.3 put `net_class` into `enhanced`, so it left the Experimental list | `autoroute/via_clearance.rs`, `autoroute/control.rs` (`net_class_via_clearance_class`), `tests/via_clearance_net_class.rs`; DIVERGENCES "Autorouter". |
  | 11 | Q16.3 `router.cleanup.remove_stubs` | Q16.3, `prereg/Q16.json` (`d3d20814…`), gate FAIL: canary-pwr KiCad unconnected 37 → 38 and CM5_MINIMA_3 `track_dangling` 1 → 1 | **Boards with fixed pour strips and dangling vias:** via_dangling 15 → 0 and 16 → 0 (canary-pwr), 9 → 0 (interf_u), drc_real 1 → 0 on both canary rows. The two failures are named (contact test misses a via crossing a pour strip; a trace ending in a plane outline outside KiCad's fill) | **kept pending Q16.4** (re-registers it with KiCad's contact rule on boards Q16.3 did not use); listed under Experimental. Not judged against the bar here, because its guards failed and Q16.4's measurement decides | `quality/stubs.rs`, `quality/stub_contacts.rs`, field `cleanup.remove_stubs`; `tests/stub_cleanup.rs`, `tests/stub_contacts.rs`. |
  | - | Not rejections, checked so the grep is complete: X07 `krt-meander` (its registered table accepts it; the generic N-1 rule only prints FAIL), X10 `krt-qfn-escape` and X12 `krt-bga-escape` (registered, run not finished), K09.9 `class_widths = auto` and the other `auto` plugin options (the arms failed on Q13's inner tracks; K09.9 stays `[~]` and reruns them), Q09 `wire_keepout` / `plane_obstacle_*` and N04 `pad_contacts` (registered, verdict pending), Q05.2-Q05.4 (never built: the trigger was not met) | | | not sorted here | |

  **Experimental, in code.** `autoroute/stages/catalog.rs`: `EXPERIMENTAL` (kind, key, slot, board
  class, help text with the numbers) for `via_clearance`, `cleanup.remove_stubs`, `room_order`,
  `work_unit` and the stage `krt-diffpair`; `StageOption::experimental`, `StageSlot::experimental_ids`;
  `via_clearance` is now an option of the connection-router slot. Shown by the CLI (`--list-stages`:
  `[experimental]` on the option or id and an "Experimental" section; `--list-stages=json`:
  `experimental` on each option, `experimental_ids` on each slot and a top-level `experimental`
  array; `help.txt` "EXPERIMENTAL OPTIONS"), by the GUI's "Algorithms" section ("(experimental)" and the
  help on hover, plus an "Experimental" header with each class and its numbers) and by the plugin's
  Advanced group (`options.experimental_stage_ids`, `options.stage_label`). An entry must be a real
  field or registered stage, and unset or unselected in both profiles.
  - **Tests.** `stage_catalog::experimental_options_are_real_listed_marked_and_unset_in_every_profile`,
    `freerouting-stages` `registry::experimental_stages_are_registered_listed_and_selected_by_no_profile`,
    CLI `profiles::{list_stages_marks_the_experimental_options_and_stages,
    help_names_every_experimental_entry}`, GUI `ui_routing::the_algorithms_section_offers_the_krt_stages`
    (the diff-pairs stage is offered and marked), plugin `test_options.py::
    test_the_advanced_group_marks_experimental_stage_ids`.
  - **Not done / for later.** Rows 3, 4 and 11 are kept pending N03.3 and Q16.4; when those decide,
    an accepted option leaves Experimental for `enhanced` through N05 and a rejected one is deleted
    with its tests and DIVERGENCES line. The Q12 agent's re-applied Q07 orders are portfolio arms,
    not defaults, and are outside this table.

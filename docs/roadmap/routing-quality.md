# Routing quality beyond Java

Recorded 2026-10-15, and updated the same day with what KiCadRoutingTools
(KRT, read at commit `0486e799`) teaches. This is what the port should do
better than Java once it routes like Java (rung 6 of the verification ladder
in [port-completion.md](port-completion.md)). Sources:
- the lessons of laying out the owner's `canary-pwr` board, sent by the CANary
  layout session (canary-09) on 2026-10-15, including a list of 17 pins that
  no 1.5 mm trace can reach;
- the open ideas in the Java fork's `docs/development/HANDOVER.md`;
- the shelved neck-down experiment in the Java fork
  (`docs/development/shelved/neckdown/`);
- KRT's neck-down, plane, diagnostic and ripup code, and its A/B rules
  (`CLAUDE.md` "Stress testing & A/B replay", `tests/stress/RUNBOOK.md`),
  from the roadmap research's KRT report.

The checklists are the [Q series](tasks/Q00-routing-quality-README.md). KRT
algorithms that are new stages rather than changes to Freerouting's own
(grid A*, plane repair, length matching, differential pairs, escape planners)
are the X series, described in [algorithms.md](algorithms.md).

**Nothing here starts before board parity (rung 3)**, with one exception. A
quality change made before parity can't be measured against Java, and it
hides port bugs. The exception is Q03's static diagnostic: it depends only on
the loaded board and its rules and changes no routes, so it starts now.
Every change here is a
deliberate divergence. It gets measured on the benchmark set and the owner's board
variants, kept only if it helps, and recorded in
[DIVERGENCES.md](../DIVERGENCES.md) with its numbers. That is the Java fork's
own rule: "a change that does not measurably help is not committed, even if it
looked promising".

Under D9 each candidate is an alternative implementation of a pipeline stage
(the A10 traits) or a report, chosen by a setting. The Freerouting
implementation stays untouched, so "identical when off" holds by
construction. It is still tested with `ses-diff`.

## How to measure a quality change

- **Boards:** the `benchmark` set plus `owner` (F02.5). On `canary-pwr`, a small
  input change moves completion by about ±10 unrouted connections, so compare
  across several board variants (different keepouts, net-class exclusions,
  component nudges), not repeat runs of one file. Routing is deterministic
  (D2), so repeat runs of one file are identical and tell you nothing. Compare
  only runs with identical input bytes and settings.
- **KiCad-graded rows use KiCad 6+ projects only** (decided 2026-10-20). A row that KiCad
  grades needs a project with its own `.kicad_pro`; 5.99 nightlies such as `picodvi` and
  `vna-usb-pd` count. A KiCad 4/5 board (`(version 4` or `20171130`, as in all 11
  `target/corpus-d3c` projects) gets its rules from what KiCad converts. For settings those
  versions lacked, such as the minimum annular ring or hole clearance, the conversion fills in
  KiCad's defaults, and those can contradict the design: d3c-0496's own 0.46/0.3 mm via has a
  0.08 mm ring against the converted 0.1 mm minimum. Such boards stay usable as router-only
  (DSN) rows. Registrations already made keep their sets.
- **Order of importance:**
  1. manufacturing rules: none, ever (decided 2026-10-20). Under the board's own rules, routed
     copper adds no clearance, track width, via diameter, drill or annular ring, copper-to-edge
     or hole-clearance error. This is an absolute guard, not "no new breach versus baseline".
  2. vias in pads: none, ever
  3. unrouted connections
  4. vias and length
  5. time
- **Same settings and work bounds on both sides.** Report per board, and the
  median across boards.
- **Preregister before the first number.** The arms, boards and variants,
  eligibility (decided from inputs), signal, guards, keep rule and stop rule
  go in a preregistration file whose sha256 a test pins (F03.4). Changes go
  in its `amendments` list with the reason. KRT's rule: "arms, cells, signals,
  guards and thresholds are not edited after a number exists".
- **Judge on at least three distinct boards, paired and directional.** A
  change is kept only if it improves on at least N−1 boards and regresses on
  none. Rows from one board count once. A two-board result never changes a
  default. Neutral boards are reported, not dropped. Print the luck rate: a
  change whose per-board direction is a coin flip passes this rule once in
  2^N runs, so N=3 is weak evidence. These rules come from KRT's A/B practice,
  where per-board spread is ±2–3 nets and two defaults were shipped and then
  reverted.
- **Which rule judges (amended 2026-10-17, N02, D10).** The paired rule above
  stays the rule for stages that are **opt-in** (off by default, a new
  algorithm, an `experimental` setting). A change to **profile `enhanced`**
  (a new default, a changed cost, a reordering) is judged by the **totals
  rule**, `freerouting-dev gate --rule totals` or `"rule": "totals"` in its
  registration:
  - *Why.* A change that moves routes moves most boards by a few connections
    either way, so "N-1 improve, none regress" rejects it even when the total
    improves clearly. P04.1 (canonical obstacle order) cut the total from 1,281
    to 1,269 unrouted over 17 boards and the maze work from 39.1 G to 19.6 G
    nodes, yet 5 boards regressed (bm01 +2, CPU-85 +7, GP8B on the clearance
    guard, smoothieboard +9, canary-pwr +5) and the paired rule rejected it.
  - *Hard guards, unchanged:* no hard metric above zero (`via_in_pad`,
    `below_minimum`), no guard metric worse (`drc_real`, clearance), D2
    determinism (the baseline comparison). Any failure rejects the change.
  - *Signal:* KiCad-graded `kicad_unconnected`, summed over the registered set;
    for a board with no `.kicad_pcb`, the router's own `unrouted`, reported
    separately. The `canary-pwr` variants are one board, summed.
  - *Accept when both hold:* (1) the total improves by **more than the set's
    noise**, sqrt(sum of the per-board bands squared); (2) **no board is worse
    than twice its own band**. The band of a board is its order spread (the
    half-range over K pass-1 orders, Q12.4), registered per board; until it is
    measured, max(2, 5 % of the board's connections) (`connections` in the
    record), and 10 for `canary-pwr`. At least **3 distinct boards**, still.
  - *Speed changes:* accepted when the total is not worse than the noise and
    the registered work measure (maze nodes, tree nodes visited) drops by at
    least 20 %. The bands still bind.
  - *Correctness fixes* (`"accept": "no-regress"`, N06): the guards hold, no
    board is worse than twice its band, and the total is not worse than the
    set's noise. No improvement is needed.
  - *What it does to P04.1.* Its unrouted counts alone (its clearance
    violations on GP8B, 0 to 2, and smoothieboard, 210 to 211, fail the hard
    guards under either rule, so P04.1 as measured is still rejected) pass by
    the speed clause only: a 50 % drop of the maze work with the total 12 better. On quality
    alone it fails: the smallest possible bands (2 for each of the 16 boards, 10
    for canary-pwr) give noise sqrt(16 x 4 + 100) = 12.8, above the 12
    improvement. The rule accepts speed-ups that cost nothing within noise; it
    does not turn a -12 on 1,281 into a quality win. The numbers are pinned by
    `crates/freerouting-dev/tests/gate_totals.rs`.
  - *Existing registrations keep their rules*: a result that exists is not
    re-judged by a rule written after it. The format is in `tools/ab/README.md`
    (`tools/ab/examples/n02-totals.prereg.json`).
- **Keep the board that disagrees.** A candidate that helps on one board of
  three isn't a candidate. Deleting the dissenting row is how a
  coincidence becomes folklore.
- **Numbers live in a baseline file**, not in prose: per Q checklist, a JSON
  of per-board, per-arm values that every re-run compares key by key with
  `freerouting-dev gate` (F03.1). The comparator names each problem:
  - INVERTED: a direction or mark reversed, checked with no tolerance;
  - DRIFT: a value moved (integers exactly, since D2 makes them exact);
  - ORPHAN: a baseline row nobody declares any more, never scoped away;
  - MISSING, NEW ROW, and MALFORMED (reported, not raised).

  A missing baseline fails. A partial run may not write the baseline. The
  gate self-tests on every invocation.
- **Leaving a row out.** A registration that states a `validity` rule (K15.14's:
  a row whose arms don't all complete is left out, reported by name with its
  reason) is judged over the rest with `--left-out ROW=REASON`, repeatable, on
  `--write-baseline`, the verdict and `--all-pairs`. The row must be one of the
  registered boards, with a reason; every other registered row must still be
  present and graded, and a missing one that isn't named is still refused. The
  row's arms may be ungraded, but a measured hard-limit breach in them still
  fails (guard (1) covers every graded arm, judged row or not). The judged rows
  must meet `min_boards`, and a `must_judge` row of the registration can't be
  left out; otherwise the gate prints `NOT MEASURED`, not a verdict. The
  baseline records the rows and reasons (`left_out`) and the verdict prints
  them. The gate takes the operator's word for which rows the rule leaves out,
  so a registration lists `must_judge` rows it can't do without; see
  `tools/ab/README.md`.
- **Grade with an independent oracle where one exists.** For boards that have
  a `.kicad_pcb` (the owner's boards, KiCad's bundled demos), import the session
  into KiCad and run KiCad's own DRC (F02.6). Freerouting's DRC grades the
  board by the same model the router used, so it can't see what that model
  gets wrong. The grading rules (F02.6, amended from KRT):
  - grade a staged copy, at the board's own rules;
  - force the copper severities to `error`, because an author's `warning`
    hides real grazes, and switch the silk checks off;
  - subtract the DRC items already present on the unrouted input, and report
    the remainder as `drc_real`;
  - record the KiCad version and the engine commit in every row, and never
    compare rows from two KiCad versions;
  - "not graded" (exit 2) is never a pass. A board that can't be graded fails
    the run, so the set can't quietly shrink to the boards that still work.
- **A rejected candidate leaves no code behind.** Its measurements stay in its
  checklist item's done note, so the lesson is kept, but the code isn't
  committed.
- Use `freerouting-dev bench` and `ses-diff` (F02), `freerouting-dev gate`
  (F03.1), and a via-in-pad check like the Java fork's `via_in_pad.py`.
- **Engine-model `via_in_pad`** (`gate_collect::session_via_in_pad`, the
  metric for boards without a `.kicad_pcb`): a session via whose copper
  overlaps an SMD pad of any net, on a layer both reach, and that the input
  has no via of the same centre, padstack and nets. The session repeats the
  input's vias (all but system-fixed ones), so importing it adds a second copy
  of each. Up to 2026-10-18 the metric took "in-pad count after import minus
  in-pad count of the input", which counted every repeated input via in a pad
  again. Those were the designer's vias, never the router's: 42 thermal vias
  in smoothieboard's exposed pads (centres inside the pad) and 6 on
  kit-dev-coldfire (annulus over a same-net pad, centre outside). Java's
  reference sessions read the same 42 and 6. Corrected, both read 0 in every
  arm of P04.1, P04.2, Q07.2, Q07.3 and Q10.4. No verdict changes; the
  corrected counts are in Q10.4's note in N05.3. The KiCad oracle
  (`tools/kicad-oracle/via_in_pad.py`) counts a via whose centre lies in any
  pad, through-hole pads included. The engine model is stricter on annuli
  (it counts a touching annulus) and looser on through-hole pads (it skips
  them). Other measures that import a session into the input board double
  the input's wiring the same way. One of them is Q10.4's helper's
  `below_minimum_dsn`: the unpoured canary DSN's 3 were input traces.
  *Since 2026-10-18* every `freerouting-dev` measure imports in place of
  the wiring the session repeats (`ses_diff::import_routed_session`): the
  bench's `unrouted`, `clearance_violations`, `vias`, `length_mm`, `bends`,
  widths and tidiness, `ses-diff`, `widths`, `power-traces`, `tidy` and
  `failed-vs-human` (`tests/session_import.rs`). Re-measured from the saved
  sessions, the doubling was a constant per board in every arm of P04.1,
  P04.2, Q07.2, Q07.3 and Q10.4, so no comparison and no verdict moves; only
  the absolute numbers drop, on the seven boards whose input has unfixed or
  protected wiring. `clearance_violations` was mostly the doubled copper:
  kit-dev-coldfire 1,056 to 10, z10_module 375 to 7, the unpoured canary
  322 to 6, smoothieboard 210 to 0 (P04.1's 210 to 211 is 0 to 1),
  interf_u 168 to 0, sonde_xilinx 6 to 0. `vias` drops by the input's
  repeated vias (253, 92, 79, 84, 42, 3) and `length_mm` by its repeated
  copper (9,412, 1,754, 1,574, 5,101, 0 and 638 mm; GP8B 160 mm). `unrouted`
  and `pour_lost_pins` don't change; `below_minimum_dsn` on the unpoured
  canary reads 0. The KiCad-graded metrics don't have the problem in the
  oracle's default flow, which strips the tracks before it exports the DSN,
  so the session repeats nothing (`--keep-tracks` rests on KiCad's
  importer, not checked). The plugin's DSNs, used by N05, Q13, Q14, Q16
  and K09.9, mark the existing copper `fix`, which a session does not
  repeat, so `power-traces` and the other measures read them right. The
  router's own `--result-json` statistics are taken on the routed board in
  memory and were right all along.
  Checked 2026-10-18: every affected verdict (P04.1, P04.2, Q07.2, Q07.3,
  Q10.4) used absolute differences. They are paired-rule verdicts (improve on
  N-1 boards, regress on none), with no `totals` block, noise band or
  percentage rule: a hard metric above 0 in the candidate, an integer guard
  (`clearance_violations`, `drc_real`, `pour_lost_pins`) worse by any amount
  (`tol` 0), and `unrouted` / `kicad_unconnected` as the score. `vias`,
  `length_mm` and `bends` were report-only, or `compared: false`, and never
  decided a mark; P04.2's 5 % clause is on work-limit hits, which the
  doubling doesn't touch. The re-measured offsets are constant per board
  across arms, so no ratio-based rule could have moved either.
  *Since 2026-10-18* `freerouting-dev` also reads every session at the scale
  the writer used (`ses_diff::import_at_writer_scale`, `ses_reader_scale`
  off whatever the route used), where it had read Java's divide-by-scale. The
  two differ only where `dsn_to_board(1) / resolution` is not 1, which among
  the 185 board fixtures is `Issue676-ch32v-tx118s` (0.1) alone: its wiring
  landed 100 times off the board, so the dev tools read 12 connections unrouted
  where the router's `--result-json` says 0 (`tests/ses_scale.rs`). Boards of
  the P04, Q07 and Q10 sweeps have factor 1 and read the same as before.

"Measured" in a Q or X **Accept:** clause means all of the above:
preregistered, at least three distinct boards plus the owner variants,
improves on N−1 and regresses on none, no new clearance violations, no via in
a pad, and numbers in a baseline file compared by F03.1.

## Candidates

### 1. Planes that block other nets (Q01)

Specctra `plane` scopes and polygon `wire` shapes load as `ConductionArea` with
`is_obstacle = false`. This happens at the Java `Structure` plane insertion
and the `Wiring` polygon wires. Java's `router.plane_as_obstacle` flips every
plane at once (`HeadlessBoardManager.applyPlaneAsObstacleOverride` →
`RoutingBoard.changePlaneAsObstacle`; Rust
`RoutingBoard::change_plane_as_obstacle`).

On `canary-pwr`, power pours on F.Cu and B.Cu got crossed by signal traces,
which cut the pour apart when KiCad refilled it. The useful option is per net
class. A blanket obstacle would also block nets whose pads lie inside the
pour, which is the ordinary GND-pour case on a two-layer board. KRT's pour
guard (`py_router/pour_guard.py`) shows the refinement: **block a pour only
for nets with no pin inside it.** A conduction area of a listed class on layer
L is an obstacle for net M on L unless M has a pin or via inside the area's
outer boundary, holes included. The table is computed once per job from the
input board, so routing order can't change it (D2). KRT's own test ("any net
in this run has a terminal inside") depends on the run's scope; ours is per
net. Same-net items still connect to the area, including at the edge.

The check that the pours survived is a report: per pour net, pins attached to
the pour's islands before and after routing. Rust already has Java's detailed
fill (`drc::zone_islands::fill_islands`, the port of
`ConductionArea.getDetailedFillArea`), so the report needs no new fill model.
KRT's soft "fragility" cost near pour edges was measured on one board only
(its docstring says "Corpus A/B pending"), so it is a later experiment.

### 2. Neck-down through pin fields (Q02)

Java at the pin narrows in two ways:
- `automatic_neckdown` (on by default) narrows a trace only inside the
  expansion room next to a start or destination pin, to that pin's width
  (`MazeSearchEngine`, `Pin.getTraceNeckdownHalfwidth`). A 1.5 mm power net
  can therefore never reach a 0.25 mm QFN pin through the neighbouring pins.
- `router.neck_width_um` (opt-in, default 0) retries a failed connection with
  every half-width clamped to the neck width
  (`AutorouteConnectionRouter.retryConnectionNecked`). The whole path is then
  narrow.

The Java fork measured the variants on `canary-pwr` (3 passes, baseline 88
unrouted, neck width 400 µm unless noted):
- necked retry only: 91;
- retry plus widening back where it fits: 83, but with 40–60 mm of 0.4 mm
  power trace;
- plus a gate accepting only routes whose narrow part is within 2 mm of a
  pad: 95, and 102 at 500 µm;
- plus no ripup for the retry: 91, and 98 at 500 µm.

**KRT has the same weakness.** Its narrow retry chooses the whole path for the
narrow width, and widening is a post-pass, so narrow length is unbounded
wherever the wide body never fits. KRT made this acceptable by *reporting* it,
not bounding it: one +3V3 net shipped 34 % of its length at signal width
before KRT's #1033 added the report. Its 2.5 mm neck length is a constant with
no measurement behind it. KRT also found a width ratchet: a project write-back
made one 0.127 mm neck the board's own width on the next run.

So Q02 is four things: a delivered-width report per wide net; a no-ratchet
rule (nothing written lowers a class width below the input's, and every
narrowing is logged); a neck bounded by distance **in the search itself**,
allowed only within D of the pin (default: the pin's courtyard plus one
pitch) and never below the class minimum, rather than a gate on a finished
narrow route (the variant that measured 95/102); and a piecewise widen-back
with the exact clearance check and a stepped taper. Measure against the
baseline, Java's `neck_width_um` retry and the shelved variants.

### 3. Say why a connection can't be routed (Q03)

Today an infeasible connection is reported as plain "unrouted", the same as one
the router merely gave up on. Example: a trace width plus clearance that cannot
pass between the pads it must reach. On `canary-pwr` there are 17 such pins
(U1, U2, U3, U7, Q2, Q3; U7 pin 3 fails by 0.03 mm).

KRT has the design: `py_placer/placement/reachability.py`. It computes a
clearance-aware distance field (the widest track that fits at each point), then
a Kruskal widest path from the pin to the rest of the net. That gives the exact
bottleneck for the raster, the throat location and the items forming it. It
shares no code with the router, so it can't share the router's bugs. KRT's
rule: "no impossibility claim without a numeric field". Q03 reports "cannot
route `<net>` to `<pin>` at width W: the widest corridor is C at (layer, x, y)
between A and B". It **can start before parity**: it needs only the loaded
board and rules, and it changes no routes.

Its acceptance is zero hard false positives: no pin reported infeasible at W
may have a clean trace of width ≥ W ending on it in a Java-routed session.

### 4. Stop when restores stop helping (Q04)

On `canary-pwr`, later passes never beat pass 1. The batch loop restores the
best board every few passes and stops after about 18 passes, which wastes
3–5 minutes (Java fork HANDOVER, open idea 1). The candidate stop rule must be
work-based and deterministic (D2): stop after N consecutive restore checks
that don't beat the best score by a quantum (the pass-level analogue of KRT's
earned-budget rule). It is a pure time saving, so the routed
result must be byte-identical to running out the passes on every board where
the rule fires after the last improving pass. Otherwise it is a quality change
and must be measured as one.

With it comes KRT's two-axis output guard: never write a board worse than the
input on connections lost against gained or on unconnected pins, neither axis
outvoting the other. It matters for boards that arrive with wiring, like the owner's.

### 5. Congestion-aware ripup (Q05)

HANDOVER open idea 2: smarter ripup between passes, for example history costs
in the style of PathFinder negotiated congestion. These raise the cost of
regions that keep failing. It is the only candidate likely to raise completion
on dense boards. It is also the biggest change here, so it starts as a
measured experiment behind a setting.

**KRT's evidence for it is weak.** KRT's history cost
(`py_router/history_congestion.py`) is flat, capped, never escalated and kept
for one run. On its corpus it gave −47 unrouted nets on sets 1–10 (20 boards
better, 11 worse, p = 0.075), nothing on sets 11–20, and −15 on sets 21–27,
where two boards supplied 113 % of the gain. A preregistered confirmation on
sets 21–27 failed its own thresholds. KRT shipped it anyway on the congested
result, and says so. Escalation walled off fine-pitch BGA escapes. Churn was
dominated by large nets: one 87-pad rail was 40 % of all torn copper.

So Q05 instruments first (rips and torn length per net) and builds history
costs only if the instrument and Q06 show congestion on at least three boards.
History lives on a fixed lattice in board units, because Freerouting rebuilds
rooms between passes. Expect gains only on congested boards.

### 6. Name the blockers and give one verdict (Q06)

After the maze search exists (A04), a failed search can name the routed nets in
its way: per net, how many blocked expansions, how many it alone caused, and
how many near the pins (KRT `blocking_analysis.py`). Each incomplete
connection then carries exactly one verdict: `infeasible` (Q03),
`work_limit` (a budget cut it off), `blocked_by` (routed nets) or `unrouted`
(walled in by fixed copper). The CLI report and the GUI incompletes list read
the same structure. This is report-only.

### 7. First-pass net ordering (Q07)

Pass 1 in Java routes in item order; later passes shuffle with the D2 seed. KRT
orders pass 1 by greedy rounds over a crossing-conflict graph of net MSTs.
KRT has no A/B of it ("MPS typically achieves higher success rates" is
unmeasured), so it enters as an experiment against the Java order.

### 8. Earned budget extension (Q08)

When a maze search hits its work limit, grant one more budget only if the
search's closest approach improved by a quantum during the last one. KRT
reports −29 incomplete nets over 150 boards for this rule; its "grace"
variant measured worse and is not ported. It is a quality change because it
changes where Java's limits cut.

Status: measured and removed (decided by Q08.3's keep rule, 2026-10-20). No
maze work limit fired on any quick benchmark row or on `canary-pwr-pours1`, so
no extension was granted: unrouted 122 -> 122 on the only eligible row, gate
FAIL. The numbers are in Q08.3's note in
[Q08](tasks/archive/Q08-earned-budget-extension.md); the registration stays in
`docs/roadmap/prereg/Q08.json`.

### 9. Pass-1 portfolio (Q12)

Route pass 1 in K deterministic orders (the configured one, Q07's
`conflict-rounds` and `inside-out`, seeded shuffles) on board clones in
parallel and keep the best, then rerun pass 1 with the failed and ripped nets
first (squeaky-wheel restarts). Its order spread became the per-board band of
the totals rule above (Q12.4).

Status: measured and removed (decided by Q12.5's fail branch, 2026-10-20).
`portfolio4` against `mpdef`: hard guards fail on `Issue508-DAC2020_bm10`
(below_minimum 7), `vfo-controller-catvfoctrl` (below_minimum 3, drc_real
75 -> 85) and `flight-computer-core-board` (drc_real 1 -> 6); the total
163 -> 159 is within the set noise 25.75; the work guard (at most 1.5x
`mpdef`'s) fails on 6 rows. `quality/portfolio/` and the Q07 orders it
re-applied (`quality/ordering/`, still on tag `archive/q07-orderings`) are
removed, and its three settings keys are refused. The numbers are in Q12.5's
note in [Q12](tasks/archive/Q12-pass-1-portfolio.md); the registration stays
in `docs/roadmap/prereg/Q12.json`.

### 10. Restart from the best board (Q23)

When the pass loop stops, take its best board, run the fanout on it and the
passes again from pass 1, and keep the result only if it beats the best
board (`router.autorouter.restarts_from_best`). Q23.1 found that a second
CLI run's gain on the PCU board and canary-pwr came from that second pass 1.

Status: measured and removed (decided by Q23.4's fail branch, 2026-10-22).
With `--max-passes 1`, `restart` against `base` improved 3 of 5 boards
(GP8B 222 -> 159, CPU-85 146 -> 142, coldfire-5213 181 -> 180 unrouted). On
the PCU board (95) and canary-pwr (331 summed) the restart did not beat the
best board, so it was dropped. The paired rule needs 4 of 5. The work guard
(at most 2.0x `base`'s maze work) failed on CPU-85 at 2.16x. No hard limit
or `drc_real` guard broke. `quality/restart.rs` is removed and its key is
refused. The numbers are in Q23.4's note in
[Q23](tasks/archive/Q23-restart-from-the-best-board.md); the registration stays in
`docs/roadmap/prereg/Q23.json`.

### Handled elsewhere

- **KiCad rule areas.** KiCad's DSN export turns a rule area that bans only
  footprints into a full `(keepout ...)`. On `canary-pwr` that is the 26 × 26 mm
  fan inlet on B.Cu, and it costs routing room. The fix belongs to a direct
  `.kicad_pcb` reader, which maps per-area flags to keepout, wire_keepout,
  via_keepout and place_keepout ([K series](tasks/K00-kicad-README.md)).
- **Ignoring net classes** (`-inc`, `router.ignore_net_classes`) is Java
  behaviour (`RouterSettings.applyNetClassExclusions`), so it is CLI parity
  ([C series](tasks/C00-cli-settings-io-README.md)), not a quality change.
  The owner uses it to keep the router off hand-laid power nets.
- **Headless `-do something.dsn` writes a 0-byte file** without an error in
  Java (HANDOVER open idea 4). The Rust CLI writes a real DSN or refuses with
  an error (C series).
- **KRT stages new to Freerouting** (grid A*, plane region repair, length
  matching, differential pairs, bus routing, escape planners) are the
  [X series](tasks/X00-krt-algorithms-README.md). Measurement tooling is F03
  and F02.6; KRT's speed techniques are the P series.
- **Plane-pad vias.** KRT allows a via in a pad by default and reuses nearby
  same-net vias for plane taps. Owner rule D8 forbids both. Don't copy them.

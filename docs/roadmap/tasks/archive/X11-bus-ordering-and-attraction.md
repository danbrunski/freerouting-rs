# X11 — Bus ordering and attraction

Opened 2026-10-15. Groups of nets that run side by side between the same two
parts (data buses, address lines) route more cleanly when routed together,
middle first, each member following its neighbour. KRT does this
(`docs/bus-routing.md`; `py_router/bus_detection.py` `detect_bus_groups`,
`get_bus_routing_order`; `bus_corridor.py`; an attraction bonus toward the
routed neighbour in `rust_router/src/router.rs`, `--bus-attraction-bonus
5000`; at `0486e799`). This checklist ports the detection and the ordering,
and turns the attraction into a gridless cost term. It changes routes, so it
is measured.

Prerequisites: board parity (rung 3); A10 (`TaskOrdering`); A04 (the cost
hook in maze expansion); Q07 (shares the pass-1-only ordering wrapper
pattern); F04; F03.1, F03.4.

## What exists and what this adds

| Exists | State |
|---|---|
| A10 `FreeroutingOrdering` | pass-1 item order, later passes shuffled |
| KRT detection: cliques of nets whose source or target endpoints lie pairwise within 5 mm, sub-clustered by their other endpoints so a group hangs together at both ends | the design |
| KRT order: the middle member first, alternating outward | the design |
| KRT hazard: `remaining = set(net_endpoints.keys())` iterates a Python set; closest-first sorting has distance ties | not copied |

## Decisions made here

- **Detection in integers:** two connections are bus neighbours if their
  start pins are within 5 mm (squared distance in `i64`) and so are their end
  pins. Groups are maximal cliques found greedily in net-number order
  (`BTreeSet` throughout); groups of fewer than 3 nets are ignored.
- **Order id `krt-bus`**, selected by `router.autorouter.ordering`: within
  pass 1, each group is routed as a block, middle member first (by position
  along the line through the start pins), then alternating outward; groups
  ordered by their first net number; non-bus tasks keep Freerouting's order.
  Plane-net tasks stay first.
- **Attraction as a cost:** A04's cost hook adds, for a door of a bus member,
  a cost that rises with the door's distance from the routed neighbour's
  trace beyond one pitch (`2·hw + c`), capped. No negative costs, so the
  search's estimates stay admissible.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X11.1 | `crates/freerouting-krt/src/bus/mod.rs`, `bus/detect.rs` | `crates/freerouting-krt/src/lib.rs` (append) |
| X11.2 | `bus/order.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X11.3 | `bus/attraction.rs` | — |
| X11.4 | `fixtures/krt/x11-bus8.dsn`, `crates/freerouting-krt/tests/bus.rs`, the X11 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [x] **X11.1 — Detection.**
  - **Accept:** on `x11-bus8.dsn` (8 nets between two connectors plus 2
    unrelated nets) one group of 8 is found; moving one end connector 10 mm
    away splits nothing off wrongly (the moved nets leave the group).
  *Done 2026-10-17:* `crates/freerouting-krt/src/bus/detect.rs` (ports KRT
  `detect_bus_groups`, `_find_largest_clique`, `_order_nets_by_position` from
  `py_router/bus_detection.py` at `0486e799`: SPDX header and a `docs/PROVENANCE.md`
  row) and `bus/mod.rs`. Connections are nets with exactly two single-net pins,
  no plane, not ignored (ends: the pin centres, lower item id first); two are
  neighbours when, oriented to each other (direct, else crossed), both start
  pins and both end pins are within 5 mm (`i128` squared distances); groups are
  greedy maximal cliques seeded in net-number order over a `BTreeSet`, kept at
  3 or more members, members in physical order along the line through the two
  farthest start pins. The fixture is `crates/freerouting-krt/tests/data/x11-bus8.dsn`
  (as X05–X10's, not under `fixtures/`, where every DSN needs a Java reference):
  D0–D7 from a 10-pin 0.5 mm connector J1 to a 6-pin J2 (D0–D5) and a 2-pin J3
  (D6, D7) end to end 32 mm right and 5 mm lower, past a keepout; J1 pins 9
  and 10 (X1, X2) to U2 7 mm away. Tests (`cargo test -p freerouting-krt --test
  bus`, 4 passed with X11.2's, 6 ignored; `--lib bus`, 8 passed):
  `the_bus_of_eight_is_one_group_and_the_unrelated_nets_stay_out` (one group,
  physical order D7 … D0, every member oriented to the seed; X1 and X2 start
  within 5 mm of the bus but end 7 mm off and stay out),
  `moving_j3_ten_mm_away_takes_its_two_nets_out_and_splits_nothing_else` (J3
  moved 10 mm right: one group D5 … D0, D6 and D7 a pair below the 3-member
  floor); unit tests `distances_are_exact_at_the_coordinate_limits` (`i32`
  extremes and ±`CRIT_INT`), `a_crossed_member_is_oriented_to_the_seed`,
  `groups_need_both_ends_and_three_members`,
  `the_physical_order_follows_the_start_line_whatever_the_net_order`.

- [x] **X11.2 — Middle-out ordering.**
  - **Accept:** the pass-1 order on `x11-bus8.dsn` is the hand-derived
    middle-out order; with the default ordering, `ses-diff` is identical on
    `benchmark`.
  *Done 2026-10-17:* `crates/freerouting-krt/src/bus/order.rs` (ports KRT
  `get_bus_routing_order`: SPDX header and a `docs/PROVENANCE.md` row):
  `middle_out(n)` (the middle `n / 2` first, then alternating outward, lower
  side first), `regroup` and the `krt-bus` ordering `KrtBusOrdering`
  (registered by `freerouting_krt::register`, selected by
  `router.autorouter.ordering`; also in the `krt-bus` bundle): Freerouting's
  `select_items` list with plane-net tasks first unchanged, then each bus group
  as one block (groups by smallest net number, members middle out, a member's
  tasks in Freerouting's order), then every other task in Freerouting's order;
  passes after the first are Freerouting's `reorder`, which sorts the signal
  tasks and so discards the blocks (pass 1 only, as decided). Tests:
  `the_pass_1_order_is_middle_out_then_the_other_nets` (hand-derived: physical
  D7 D6 D5 D4 D3 D2 D1 D0, middle index 4 = D3, then 3, 5, 2, 6, 1, 7, 0: the
  pass-1 list is D3 D4 D2 D5 D1 D6 D0 D7 then X2 X1 X2 X1 as Freerouting has
  them; the same task set as `select_items`; pass 2 equals
  `FreeroutingOrdering`'s), `the_stages_are_selected_by_their_keys_and_off_by_default`,
  unit tests `middle_out_matches_krts_examples` (`[2, 1, 3, 0, 4]` for KRT's
  A B C D E) and `regroup_keeps_planes_first_and_others_in_order`. Default
  ordering, sessions byte-identical: release CLI at the base `e837fce` against
  this tree (`freerouting -de <board> -do <ses>`, default settings, `cmp`):
  `connection-parity` 10/10 and `benchmark`'s Issue433-my-board, Issue649
  ecc83, Issue508 bm02, Issue690 sonde and Issue508 bm07 identical (14 boards;
  the tree's engine change, the cost hook of X11.3, is on that path, and the
  CLI does not register the krt stages, C07).

- [~] **X11.3 — Attraction cost.**
  - **Accept:** on `x11-bus8.dsn` with attraction on, the mean distance
    between neighbouring members' traces over their common length is within
    10 % of one pitch; with it off, the value is reported for comparison;
    `tools/determinism-check.sh` passes.
  *Partial 2026-10-17:* built, measured, deterministic; the 10 % clause is
  **not met**. Engine hook (A04's, reached from a connection router; small and
  additive): `ExtraCost::section_cost(layer, door, from, entry)` (a provided
  method defaulting to `door_cost`, `session/sink.rs`; the search calls it with
  the two section middles, and a door shape that cannot be built adds no cost
  instead of ending the expansion, `session/search.rs`),
  `connection::route_connection_with_cost`, and
  `batch/connection_cost.rs` (new): `ConnectionCost` (`begin(board, task, p,
  control)`, `section_cost`) and `FreeroutingConnectionRouter::with_cost`
  (first attempt and plane fallback only; without a cost the search is called
  as before). `crates/freerouting-krt/src/bus/attraction.rs` (written from
  KRT's `get_path_attraction_bonus` idea, credit in the module docs):
  `BusAttraction` costs `weight · |e − f| · (x(f) + x(e)) / 2 / pitch` per
  expansion from section middle `f` to `e`, `x(p) = min(cap, |d(p) − pitch|)`,
  `d` the distance to the nearest segment of a routed physical neighbour (any
  layer), `pitch = hw + hw_n + c` (500 µm here); never negative; defaults
  weight 1, cap 5 mm, two-sided, pass 1 only; `KrtBusAttractionRouter`
  (`krt-bus-attraction`) and the `krt-bus` bundle (`krt-bus` ordering plus
  it). `spacing` is the measure: per pair of physical neighbours, the
  same-layer centreline distance sampled every 10 µm where the nearest point
  on the neighbour is not an end of its traces, weighted by that common
  length. Release, default settings, `x11-bus8.dsn` (`cargo test --release -p
  freerouting-krt --test bus -- --include-ignored the_attraction`, pinned in
  `the_attraction_pulls_the_bus_closer`): **off** (Freerouting) 1834 µm,
  0 unrouted, 14 bus vias, 296.7 mm on bus nets; bus order alone 2054 µm,
  **1 unrouted**, 16 bus vias; **on** (`krt-bus`) **689 µm (+38 % of a
  pitch)**, 0 unrouted, 10 bus vias, 282.5 mm; no new clearance violation or
  via in a pad in any arm. The defaults are the best of a sweep
  (`attraction_trial`; weights 1, 4, 16, one- or two-sided, pass 1 or every
  pass, earlier also caps 1 mm and same-layer only, weights to 64): every
  setting with a smaller spacing left connections unrouted (e.g. one-sided,
  every pass, weight 16: 659 µm, 2 unrouted), larger weights make ripping
  cheaper than a path, and the later passes and the optimizer rebuild what
  pass 1 laid down; within a room the located path runs straight, so a door
  cost steers the room sequence, not the position inside a room. Determinism:
  `tools/determinism-check.sh --out <scratch> -- env X11_DSN=…/x11-bus8.dsn
  X11_ARMS=krt-bus X11_SES_DIR={OUT} <bus test binary> bus_arms_on_a_set
  --ignored --exact`: `identical` (idle load 7.1, loaded 13.4);
  `two_attracted_runs_write_the_same_session` passes. The hook itself changes
  nothing: `a_zero_or_declining_connection_cost_routes_as_freerouting`
  (Freerouting's router with a cost that declines, or that applies and adds
  0, writes Freerouting's session on Issue433). Remainder: reaching one pitch
  needs more than a door cost, e.g. KRT's planned corridor (`bus_corridor.py`,
  a probe route at inflated clearance as the attraction path) or attraction
  inside the pull-tight and optimizer; owner: a follow-up X11 item after
  X11.4 decides whether the stages stay.
  *Moot 2026-10-17:* X11.4's registered run rejected both stages and their code is removed
  (X11.4's note), so the 10 %-of-pitch clause will not be met and no follow-up item is opened
  (the operator's recommendation for a rejection). Left `[~]` because its Accept never held.

- [x] **X11.4 — Measurement.**
  Preregister (F03.4). Boards: at least three with buses (connectors or
  memory parts with parallel nets). Signal: vias and length on bus nets.
  Guards: unrouted (no regression on any board), `drc_real`, vias in pads.
  - **Accept:** measured: vias or length on bus nets improve on N−1 boards and
    completion regresses on none; otherwise the code is removed and the
    numbers stay here.
  *Partial 2026-10-17:* registered, not run: the measurement changes routes,
  so it waits for P04's default-tree decision (P00 README "Order"); owner P04,
  then this item. Preregistration `docs/roadmap/prereg/X11.json` (F03.4
  format, `freerouting-dev gate`'s global rules: ≥ 3 boards, N−1, no
  regression, `via_in_pad` hard): baseline `freerouting` (the defaults),
  `candidate_arm` `krt-bus` (`router.autorouter.algorithm`, ordering plus
  attraction) and `second_candidate_arm` `krt-bus-order`
  (`router.autorouter.ordering` alone), each judged on its own; boards
  `tools/bench/sets/x11-bus.txt` (input sha256 in the registration):
  Issue508 bm01 (port buses PA0–7, PC0–7, PL0–7), Issue113-Protein (data bus
  DA2–DA13, DC10–15), Issue039-bug-design (DATA1–4 and their `_R` copies),
  Issue208-freerouting (connector J7 rows of 11) and kit-dev-coldfire (KiCad
  demo, XIL_D data, GPT0–3; graded by `tools/kicad-oracle/grade.sh`), each with
  bus groups by `bus_groups_on_a_set` (load only, release; 51 of the 140
  fixtures have a group, one does not load); signal bus vias and bus length;
  guards unrouted (any increase is a regression), drc_real/drc_new,
  via_in_pad; harness
  `bus_arms_on_a_set` (`X11_SET` or `X11_DSN`, `X11_ARMS`, `X11_SES_DIR`) in
  `crates/freerouting-krt/tests/bus.rs` (the CLI does not register the krt
  stages before C07); every arm runs the defaults plus its key (an empty
  settings patch, not the Java fixture-test profile). **Default-off gate:**
  release CLI at the base `e837fce` against this tree, `connection-parity`
  (10) and `benchmark`'s first five: 14/14 `cmp` identical (rerun after the
  last engine edit); registered-but-unselected cannot differ (the registry
  builds only selected stages) and Freerouting's router with a declining or
  zero cost writes Freerouting's session
  (`a_zero_or_declining_connection_cost_routes_as_freerouting`). **Pre-P04
  dry run** (informational, not the registered result; this tree, release,
  Rust DRC only, machine load 5–7), Issue039-bug-design: `freerouting`
  6 unrouted, 27 bus vias, 372.5 mm on bus nets, spacing 2068 µm, 151 s;
  `krt-bus-order` **10 unrouted**, 28 bus vias, 394.8 mm, 2470 µm, 109 s;
  `krt-bus` **9 unrouted**, 33 bus vias, 402.3 mm, 1566 µm, 238 s; drc_new
  and via_in_pad 0 in all. Both candidates regress completion and bus vias
  there, as the bus order alone does on `x11-bus8.dsn` (X11.3), so the
  registered run is likely to reject them. bm01's Freerouting arm had not
  finished after 25 minutes, so the full set takes hours. Remainder: the
  registered run on all five boards after P04, KiCad grading of coldfire,
  the keep or reject decision per arm (reject removes the code as the keep
  rule says), and (if kept) the `docs/DIVERGENCES.md` line; owner P04, then
  X11.4.
  *Bias note 2026-10-17:* this harness already started from an empty settings patch, so the dry
  run above and the run below are not biased by `TestingProfile::default()`.
  *Done 2026-10-17 (both candidate arms rejected; the bus module and the engine cost hook
  removed):* the registered run after P04's default-tree decision (c23d8c0: the default tree
  stays Java's), `docs/roadmap/prereg/X11.json` with one amendment before the first run
  (kit-dev-coldfire registered as the prepared demo grade.sh routes; a 45 min timeout per route),
  `X11_DSN=<board> X11_ARMS=<arm> <bus test> bus_arms_on_a_set` per arm (release, one route at a
  time, machine load 4–11 from two other agents), log `target/orchestrator/meas/runs/x11-engine.log`.
  Issue039-bug-design, the first board of the run (Rust DRC):

  | arm | unrouted | drc_new | via_in_pad | vias | bus vias | bus length mm | spacing µm | s |
  |---|---|---|---|---|---|---|---|---|
  | freerouting | 6 | 0 | 0 | 216 | 27 | 372.50 | 2068 | 190 |
  | krt-bus-order | **10** | 0 | 0 | 210 | 28 | 394.81 | 2470 | 313 |
  | krt-bus | **9** | 0 | 0 | 237 | 33 | 402.31 | 1566 | 372 |

  The same numbers as the pre-P04 dry run. The keep rule needs unrouted to regress on no board for
  each arm; both arms regress on this board (6 → 10, 6 → 9), and both also lose on the score (bus
  vias and length up), so neither can be kept whatever the other four boards show. The remaining
  boards were therefore not run (Issue113-Protein's first arm was stopped after 15 s;
  Issue208-freerouting, DAC2020 bm01 and kit-dev-coldfire not run), a choice to save routing time
  for X10 and X12; the F03.1 gate would refuse the partial run (registered boards missing), so no
  baseline was written. **Removed** per the keep rule ("neither: the bus module goes"; the
  attraction, the bundle and the engine cost hook go with `krt-bus`): `crates/freerouting-krt/src/bus/`
  (`detect.rs`, `order.rs`, `attraction.rs`, `mod.rs`), its registration in `lib.rs`,
  `tests/bus.rs`, `tests/data/x11-bus8.dsn`, the two `docs/PROVENANCE.md` rows (`bus/detect.rs`,
  `bus/order.rs`), and the engine hook X11.3 added, reverted to its state before X11
  (`git diff 9685d7f^ 9685d7f` reversed on `autoroute/batch/connection.rs`,
  `batch/connection_cost.rs` (deleted), `batch/mod.rs`, `autoroute/connection.rs`,
  `session/search.rs`, `session/sink.rs`; no other code used it). Kept:
  `tools/bench/sets/x11-bus.txt` (the registration names it). No `DIVERGENCES.md` line.

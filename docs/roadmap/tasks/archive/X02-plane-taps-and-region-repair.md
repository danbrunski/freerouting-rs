# X02 — Plane-tap verification and pour region repair

Opened 2026-10-15. A pour can end up split into islands after routing, and a
plane-net pin can look connected while its via lands on a floating island.
KRT handles both: its tap code checks that a tap lands on the plane's main
connected component (`py_router/plane_component_oracle.py`, used by
`plane_pad_tap.py`), and its plane finalize re-joins a net's disconnected pour
regions with straps along a Kruskal MST (`py_router/repair_planes.py`,
`plane_region_connector.py`: `find_disconnected_zone_regions`,
`find_region_connection_points`, `route_plane_connection_wide`, at
`0486e799`). Freerouting has neither. This checklist adds the verification as
a report, then region repair as a post-route stage.

Q01 tries to stop routing from cutting pours; X02 repairs what is cut anyway.
They are measured together.

Prerequisites: board parity (rung 3); Q01.4 (the pour audit and its use of
`fill_islands`); A06 (a connection-routing entry point with a trace-width
override); A10 (the plane-repair stage slot); B09 (insert → DRC → commit or
revert); F04; F03.1, F03.4, F02.6.

## What exists and what this adds

| Exists | State |
|---|---|
| Rust `drc::zone_islands::fill_islands` (port of Java `ConductionArea.getDetailedFillArea`) and `zone_island_violations` | islands per area; Java DRC's isolated-island findings |
| Rust `drc::incompletes::NetIncompletes`, `drc::unconnected` | own-net connectivity, the same code DRC uses |
| Java plane-net routing at cda540f: plane-net tasks first; plane targets with a fallback (pipeline report §3.5) | ported by A; D8: plane-net pads get their own via |
| KRT `plane_component_oracle.py` (267 lines): a tap may only target copper on the main component | the verification |
| KRT region repair: flood-fill regions from anchors (vias, through-hole pads), Kruskal MST over regions, multi-source to multi-target routing per MST edge in both directions, strap widths stepping 2.0 → 1.0 → 0.5 → … → minimum | the repair |
| KRT #675: its fill source depends on whether KiCad is installed | the hazard not to copy |

## Decisions made here

- **One fill source:** Java's detailed fill through `fill_islands`. No
  KiCad-dependent fallback.
- **Main component** of a plane net on a layer: the island touching the most
  anchors of that net (pins and vias), ties by the lowest anchor `ItemId`
  inside it.
- **D8 holds:** a plane-net pin counts as connected to the plane only through
  its own via, and that via must lie on the main component. Via reuse and
  via-in-pad from KRT's tap ladder are not ported.
- **Repair is a stage**, id `krt-region-repair`, selected by
  `router.special.plane_repair`, run after the last routing pass and before
  the optimizer. Default off.
- **Straps are routed by the Freerouting connection router** with a width
  override (A06), from the anchors of one island to the anchors of the other.
  Widths step down 2.0, 1.0, 0.5 mm, then the net class width. Each strap is
  inserted in a B09 transaction and kept only if the exact DRC adds nothing.
- **MST order:** edge weight is the smallest anchor-to-anchor distance
  between two islands (squared, `i64`), ties by the pair of island indices;
  islands are indexed by their lowest anchor `ItemId`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X02.1 | `crates/freerouting-krt/{Cargo.toml, README.md, src/lib.rs}` if the crate doesn't exist yet; `crates/freerouting-krt/src/planes/mod.rs`, `planes/components.rs` | `crates/freerouting-krt/src/lib.rs` (append) |
| X02.2 | `planes/verify.rs`, `fixtures/krt/x02-floating-island.dsn` | — |
| X02.3 | `planes/repair.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X02.4 | `fixtures/krt/x02-split-pour.dsn`, `crates/freerouting-krt/tests/planes.rs` | — |
| X02.5 | the X02 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [x] **X02.1 — Crate skeleton; islands, anchors and main components.**
  If `crates/freerouting-krt` doesn't exist, create it as X04's decisions
  describe (`license = "GPL-3.0-only AND MIT"`, `publish = false`, depends on
  `freerouting` only, `register()` in `lib.rs`, a README stating the licence
  split).
  Rust design: `planes::components::PlaneComponents::build(board)`: for each
  filled conduction area in ascending `ItemId`, its islands (`fill_islands`),
  each island's anchors (pins and vias of the area's net whose shape centre
  lies inside the island) sorted by `ItemId`, and the main island.
  - **Accept:** the gate passes with the crate; unit tests: an area cut in
    two by a foreign trace has two islands with the expected anchors; the main island is the one with more
    anchors; a tie picks the lower anchor id.
  *Done 2026-10-17:* `crates/freerouting-krt` created: `Cargo.toml` (`license =
  "GPL-3.0-only AND MIT"`, `publish = false`, edition and `rust-version` from the
  workspace, depends on `freerouting` only), `README.md` (the licence split), `src/lib.rs`
  (`pub fn register(registry: &mut StageRegistry)`). `planes::components::PlaneComponents::build`:
  filled planes (conduction areas of no component) ascending by id, their islands in
  `conduction_fill` order, anchors (pins and vias, drill centre on the island) ascending
  by id number (`ItemId`'s own `Ord` is Java's highest-first order, so every sort is by
  `.0`), the main island per plane and per (net, layer), and fill-aware electrical groups
  (`net_components(.., stop_at_plane)` joined through each island's anchors by a
  lowest-root union-find), `regions(net, layer)` for X02.3. No KRT code ported (idea
  credit in the module docs); `tools/check-provenance.sh`: `provenance ok`. Tests
  (`cargo test -p freerouting-krt --lib`, 11 passed):
  `a_pour_cut_by_a_foreign_trace_has_two_islands_with_their_anchors` (left island {G1, G2},
  right {G3}, main = left), `the_main_island_is_the_one_with_more_anchors`,
  `a_tie_picks_the_island_with_the_lower_anchor_id` (both placements),
  `an_uncut_pour_is_one_island_holding_every_anchor`,
  `a_trace_between_the_halves_joins_their_islands` (two fill islands, one group). Gate in
  the worktree: `cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets --
  -D warnings` clean, `cargo test -p freerouting -p freerouting-krt --no-fail-fast` 1090
  passed, 0 failed, 31 ignored.

- [x] **X02.2 — Tap verification (report only).**
  For each plane-net pin: the via that connects it (D8), and whether that via
  lies on the main component of the plane on each plane layer it reaches.
  A pin attached only to a non-main island is reported as
  `floating_tap`. Emitted in the C08 result JSON.
  - **Accept:**
    - `x02-floating-island.dsn` (two plane pins whose vias land on a
      same-net island cut off from the main pour) reports both pins;
    - the incomplete count Rust's DRC reports for the net equals the count
      of reported pins' connections;
    - the SES is byte-identical with the report on and off.
  *Done 2026-10-17:* `planes::verify`: `TapReport::{build, from_components, floating,
  floating_connections, to_json}`, per pin its taps (pins and vias connected without a
  plane that anchor an island; `vias`, `direct`), per reached island `on_main` and `joined`,
  status `main` / `floating_tap` / `no_tap`; `TapReportStage` (`krt-plane-tap-report`,
  `StagePhase::AfterOptimizer`) logs the JSON as one line and changes nothing. The fixture is
  `crates/freerouting-krt/tests/data/x02-floating-island.dsn`, not `fixtures/krt/`: every
  DSN under `fixtures/` needs a Java reference (`reference_load.rs` failed on
  `fixtures/reference/load/krt/*.json`), as Q01 found. Its cut-off island is two small GND
  pours, one per pin's via: with a separate pour the DRC's incomplete count is defined; on
  one pour cut by foreign copper Freerouting counts the pour as one conductor and reports 0
  incompletes (pinned in `the_split_pour_has_two_regions_and_a_floating_pin_before_repair`).
  Tests (`cargo test -p freerouting-krt --test planes`, 6 passed, 2 ignored helpers):
  `both_pins_on_the_cut_off_islands_are_floating_taps` (P1-1, P2-1, one own via each; G1-1,
  G2-1 `main`), `the_drc_incompletes_of_the_net_equal_the_floating_connections`
  (`incomplete_count_of(GND)` = 2 = floating connections = floating pins),
  `the_session_is_the_same_with_the_tap_report_on_and_off` (SES equal, log line present).
  The JSON reaches the CLI's C08 result once the CLI calls `freerouting_krt::register` (the
  CLI's one line, X00 "Parallel lanes"; C07); until then it is the stage's log line.

- [x] **X02.3 — Region repair stage.**
  Rust design: `planes::repair::RegionRepair` implements A10's plane-repair
  trait. For each plane net with more than one island on a layer, in
  ascending net number: build the MST; for each edge in MST order, try the
  width ladder; route with A06's width override; insert in a B09
  transaction; keep the first width whose insertion adds no DRC item. A
  strap that fails at every width is reported, not retried.
  - **Orders that decide results:** nets ascending; MST edges in Kruskal
    order with the tie-break above; widths in ladder order.
  - **Accept:** unit test of the MST on a synthetic set of four islands;
    with the stage off, `ses-diff` shows identical sessions on `benchmark`.
  *Done 2026-10-17:* `planes::repair`: `RegionRepair` is a `BoardStage` at
  `StagePhase::AfterRouting` (A10's plane-repair slot; there is no
  `router.special.plane_repair` key, the stage is selected with
  `router.special.stages=krt-region-repair`). Nets ascending, layers ascending,
  `kruskal_edges` (smallest squared anchor distance, `i64`, ties by region index pair;
  regions by lowest anchor id), `kruskal_with` (failed edges reported, not retried),
  `strap_half_widths` (2.0, 1.0, 0.5 mm above the class width, then the class width), each
  width in `RoutingBoard::try_change(.., CheckPolicy::NewItemsOnly)`. Two findings, both
  handled: (1) the maze search ends at any own-net item outside the start set, so the pour
  itself is found at once; **engine hook** `AutorouteControl::destinations_only:
  Option<BTreeSet<ItemId>>` (`None` everywhere in Freerouting's stages), read by
  `Session::is_destination_door`, new `is_start_door`/`is_ignored_door`
  (`session/doors.rs`), `queue_start_doors` (`session/init.rs`, `!is_start_door`, the same
  test under `None`) and `expand_to_target_doors` (`session/search.rs`, skips ignored doors,
  never under `None`); (2) a strap is a cycle through the pour to Freerouting and
  `normalize_traces` removed it, so it is routed with the class's
  `ignore_cycles_with_areas` set (restored at once) and then user fixed (`protect` in the
  SES). Straps use the net's routing layers, no vias, no ripup, no neck-down. Unit tests
  (`--lib`): `the_edges_are_in_kruskal_order_with_the_closest_anchor_pair`,
  `kruskal_takes_the_minimum_spanning_tree_in_order` (four islands: (2,3), (0,2), (1,3)),
  `a_failed_edge_is_not_retried_and_the_next_one_joins_its_regions`,
  `the_squared_distance_is_exact_at_the_coordinate_limits` (±`CRIT_INT`). Stage off, HEAD
  `b502cc0` CLI against the worktree CLI (release), `freerouting -de <board> -do <ses>`,
  `cmp` and `freerouting-dev ses-diff` (exit 0 every board): `connection-parity` 10/10
  bytes-identical; `benchmark` the first 5 boards (Issue433, ecc83, bm02, sonde, bm07) with
  default settings bytes-identical, bm01, bm10, interf_u and Issue555 (inner planes) with
  `-mp 3 --router.optimizer.enabled=false` bytes-identical (bm01 with default settings ran
  over 10 min per arm and was stopped); the 7 boards above 237 connections not run. Also
  in-process, registry with and without `freerouting_krt::register`, stage unselected
  (`default_off_sessions_on_a_set`, release, `X02_SET=connection-parity`): 10/10 identical;
  `registering_the_stages_changes_no_session_while_they_are_unselected` on both fixtures.
  Engine tests with the hook: `cargo test -p freerouting --no-fail-fast` all green (1090
  passed with the krt crate).

- [x] **X02.4 — Repair fixture.**
  `x02-split-pour.dsn`: a GND pour on B.Cu cut in two by a pre-routed signal
  trace, with room for a strap around the trace's end.
  - **Accept:** with the stage on, the pour has one island after routing
    (`fill_islands`), Rust's DRC reports no new item, no via is in a pad,
    and `tools/determinism-check.sh` passes.
  *Done 2026-10-17:* `crates/freerouting-krt/tests/data/x02-split-pour.dsn` (not
  `fixtures/krt/`, see X02.2): GND pour on B.Cu, pins G1, G2 left and G3 right of a
  pre-routed S trace on B.Cu from (20, 1) to (20, 27) mm, room above its upper pad, net A
  routed outside the pour. `the_repair_joins_the_split_pour_with_one_strap` (test profile,
  stage on vs off): one strap at 2.0 mm (20000 board units, class 0.25 mm), user fixed,
  `protect` in the SES; after it the pour's islands form one region (one electrical group)
  and no pin is a `floating_tap`, against two regions with the stage off. `fill_islands`
  itself still gives 2 islands: Java's detailed fill ignores same-net traces, so no strap
  can change that count; "one island" is read as one joined region (the islands lost of
  X02.5's preregistration). DRC (`design_rules_check`): 0 clearance violations on and off,
  unconnected items not more than off, the `isolated_island_unconnected` finding 1 → 0; no
  via in any pad. `tools/determinism-check.sh --out <x02>/det-x024 -- env
  X02_SES_OUT={OUT} <release planes test> write_the_repaired_session --ignored --exact`:
  `identical` (idle, then 8 burners; SES plus the stage's log lines).

- [x] **X02.5 — Measurement, with Q01.**
  Preregister (F03.4). Arms: default; `krt-region-repair`; Q01's rule plus
  repair. Boards: Q01's `q01-pours.txt` set and `canary-pwr` with its
  variants. Signal: islands lost (Q01.4). Guards: KiCad `unconnected_items`
  for zone nets (must not rise), unrouted, `drc_real`, vias in pads.
  - **Accept:** measured. Kept only if islands lost fall on N−1 boards and
    nothing regresses on any; otherwise X02.3–X02.4's code is removed and
    the numbers stay here. X02.2's report may stay if the owner wants it.
  *Partial 2026-10-17:* preregistered, not measured. `docs/roadmap/prereg/X02.json`
  (F03.4 format; `freerouting_dev::gate_prereg::Prereg::parse` accepts it: id
  `X02.5-region-repair`, baseline `default`, candidate `repair`, third arm `q01-repair`
  in `secondary_comparisons`, Q01's five `q01-pours.txt` boards with their sha256, score
  `islands_lost`, guards `pour_unconnected`, `unrouted`, `drc_real`, hard `via_in_pad`,
  `below_minimum`; sha256 `742601e25db66b764040e6506f41fb3c8ae37a8fb88ca60e50433cea207243ae`).
  The run waits for P04's default-tree decision (Q00 and P00 order route-changing
  measurements after it), for the CLI's `freerouting_krt::register` line (C07; X00
  "Parallel lanes"), and for canary-pwr's grading by the peer session (canary-23). No
  `DIVERGENCES.md` line until the stage is kept.
  *Done 2026-10-17 (rejected; X02.3–X02.4's code removed):* the registered run after P04's
  default-tree decision (c23d8c0: the default tree stays Java's), in-process (the CLI does not
  register the crate's stages, C07): engine command `route_one_dsn` (an ignored test added to
  `tests/planes.rs` for this run, default settings plus each arm's keys, removed with the stage)
  under `tools/kicad-oracle/grade.sh` (KiCad 10.0.7, demos stripped and exported by the grader);
  canary-pwr routed from `fixtures/canary-pwr/canary-pwr-9511d9a.dsn` and graded by canary-23's
  tools on WSL KiCad 10.0.7 (`ses_import.py` onto the board at canary HEAD, sha256 15aa06a3, then
  `tools/krt-h2h/grade_routed.sh`, `via_in_pad.py`, `inner_tracks.py`; the path reproduces
  canary-23's 41 unconnected on Java's `46bf9e2ef` session). `islands_lost` is the number of
  electrical groups of a pour net's anchored islands minus one, per net and layer, summed
  (`PlaneComponents::regions`; 0 when all are joined); `pour_unconnected` counts KiCad's
  unconnected items on a zone net. Logs and summaries `target/orchestrator/meas/runs/x02/`,
  `runs/x02-metrics.tsv`. Per board, (islands_lost, pour_unconnected, unrouted = KiCad
  unconnected, drc_real, via_in_pad, below_minimum; straps), default → repair → q01-repair →
  Q01.5's `on` arm:

  | board | default | repair | q01-repair | Q01 on |
  |---|---|---|---|---|
  | interf_u | (27, 13, 13, 13, 0, 0) | (13, 9, 9, **12**, 0, 0; 18) | (13, 9, 9, 12, 0, 0; 18) | = default |
  | pic_programmer | (5, 2, 2, 2, 0, 0) | (0, 1, 1, **5**, 0, 0; 5) | (0, 1, 1, 5, 0, 0; 5) | = default |
  | complex_hierarchy | (82, 80, 80, 0, 0, 0) | (36, 38, 38, **7**, 0, 0; 55) | (34, 35, 35, 7, 0, 0; 56) | = default |
  | canary-pwr (9511d9a) | (18, 14, 48, 33, 0, 3) | (1, **15**, **49**, **35**, 0, 3; 17) | not run | not run |
  | kit-dev-coldfire | not run | not run | not run | not run |

  The new KiCad items: pic_programmer 3 `shorting_items` (straps that KiCad's refilled pour
  shorts), complex_hierarchy 4 starved thermals and 3 dangling tracks, canary-pwr 2 more dangling
  vias. canary-pwr's `below_minimum` 3 (KiCad `track_width`) is in the default arm too.
  `inner_tracks.py` 0 and `via_in_pad.py` 0 on canary-pwr in both arms. Decision by the
  registered rule: islands lost fall on all four boards run, but `drc_real` rises on
  pic_programmer, complex_hierarchy and canary-pwr, and canary-pwr's `pour_unconnected` and
  unrouted rise by one, so three boards regress against `max_regress` 0. That is decided whatever
  coldfire would show, so its four arms and canary-pwr's two Q01 arms were not run (a choice to
  save about two hours of routing for X10 and X12). F03.1 gate: with the registration it refuses
  the partial run ("board … kit-dev-coldfire … is registered but missing from the run") and writes
  no baseline; without it (`--arms default,repair`, `runs/x02-np-*`): improved 1/4 (interf_u),
  regressed 3, **FAIL**. The secondary comparison (Q01's rule plus repair against Q01's rule
  alone) is moot: Q01's `on` arm equals the default on the three demos run (no listed pour is cut
  there), and q01-repair regresses like repair. **Removed** per the decision table:
  `crates/freerouting-krt/src/planes/repair.rs` (`krt-region-repair`, its registration in
  `planes/mod.rs`), `crates/freerouting-krt/tests/data/x02-split-pour.dsn` and the X02.3–X02.4
  tests on it (`the_split_pour_has_two_regions_and_a_floating_pin_before_repair`,
  `the_repair_joins_the_split_pour_with_one_strap`, `write_the_repaired_session`). Kept: the
  engine hook `AutorouteControl::destinations_only` (X08's centreline and leg routing use it too),
  `planes::components` and X02.2's tap report (`krt-plane-tap-report`, off by default; the
  decision table lets it stay). No `DIVERGENCES.md` line.

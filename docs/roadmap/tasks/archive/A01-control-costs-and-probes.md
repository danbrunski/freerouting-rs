# A01 — Autoroute control, costs, Java ids and the probe harness

Opened 2026-10-15. Every later A checklist compares Rust against a Java probe, and every
search reads its parameters from `AutorouteControl`. This checklist builds both: the
`autoroute` module skeleton, the per-net control values and cost bounds, the Java
`getId` hash formulas that break queue ties, and the shared probe harness with its
connection sampler. It is the first rung-1 comparison of
the series ([A00](../A00-autorouter-README.md)).

**Prerequisites:**
- C02 (the `RouterSettings` type), C03.6 (`prepare_job`, Java's load sequence with the
  load-time overrides) and C03.7 (`BoardRouterSettings`, the router's read-only view with
  the per-layer trace costs from `applyBoardSpecificOptimizations`). The control values
  depend on all three.
- B04.7 for the item helpers the probes and A02 call (`trace_connection_shape`,
  `min_width`, `pin_drill_allowed`, `tree_shape_on_layer`); A01 itself does not port them.
- F02.1 (`tools/java-reference/probe.sh`).
- Decisions D1, D2, D6.

## What exists and what this adds

| Exists (Rust at HEAD `70389b7`, Java at `cda540f`) | State |
|---|---|
| `freerouting_geometry::jmath` (`JavaRandom` with `new`, `set_seed`, `next_int`, `shuffle`; `java_round`, `round_to_i32`) | Bit-exact; no `next_double` yet (A04 adds it) |
| `get_id` on `Point`, `IntBox`, `IntOctagon`, `Simplex`; `java_hash_code` (`freerouting-geometry/src/big_int_aux.rs`) | Ported; the inputs of the room and door id formulas |
| `rules/mod.rs`: `net_class`, `trace_clearance_class`, `via_rule`, `is_active_routing_layer`, `trace_half_width`, `default_via_rule`; `ViaInfo.attach_smd_allowed`; `library/padstack.rs` `from_layer`/`to_layer`/`shape` | Present |
| `BasicBoard::{connectable_items, connected_set, unconnected_set, power_plane_areas}` (`board/basic_board.rs`) | Present, descending id order like Java's `TreeSet<Item>` |
| `autoroute/maze/AutorouteControl.java` (313 lines) | Missing |
| `autoroute/maze/DestinationDistance.java` (391) | Missing |
| `getId` of `ExpansionDoor`, `TargetItemExpansionDoor`, `ObstacleExpansionRoom`, `IncompleteFreeSpaceExpansionRoom`, `ExpansionDrill`, `DrillPage` | Missing |
| `Connectable.getTraceConnectionShape`, `DrillItem.minWidth`, `Pin.drillAllowed`, `getTreeShapeOnLayer` | B04.7 ports them |
| A Java probe harness for the autorouter | Missing. F02.1 gives the one-command runner |

What this adds: `crates/freerouting/src/autoroute/` with `control.rs`, `distance.rs`,
`java_ids.rs` and `probe.rs`; the Java probe file and its generator; the Rust test
helpers; and committed probe outputs for `selftest`, `ids`, `connections`, `control`
and `distance`.

## Decisions made here

- **One Java probe file.** `tools/java-reference/probes/autoroute/AutorouteProbe.java`,
  `package app.freerouting.autoroute.maze;`, takes `<kind> <fixture> <out>` and switches
  on the kind. Package-private maze state is read directly. Private state elsewhere is
  read by reflection, which works because the Java repo has no `module-info.java`. Later
  checklists append a kind; none rewrites another's.
- **Canonical lines, compared as text.** `autoroute::probe::ProbeWriter` and the Java
  writer emit identical lines (format in [A00](../A00-autorouter-README.md)). No JSON parser
  is needed on the Rust side. A test fails at the first differing line and prints it with
  the five lines before it. Doubles are printed as raw bits; the failure message decodes
  them for people.
- **Digest above 1 MiB.** FNV-1a 64 over each sampled connection's lines (each line plus
  `\n`), written by hand on both sides. No new dependency.
- **The connection sampler is defined once** and reused by A02–A06:
  1. Load the board exactly as J01's `route` mode does: `DefaultSettings` +
     `DsnFileSettings` + the harness settings, through the headless load path. Rust uses
     C03.6's `prepare_job` with the same patches.
  2. Build the router's trace costs as `BatchAutorouter` does
     (`settings.getTraceCosts()`, preferred directions on); Rust reads them from C03.7's
     `BoardRouterSettings`.
  3. For each net number from 1 to `maxNetNumber`, take the first item of
     `getConnectableItems(net)` (descending id) with a non-empty unconnected set on that
     net and no ignored nets.
  4. Compute start and destination as `AutorouteConnectionRouter.route` does
     (`AutorouteConnectionRouter.java:47-80`): a plane net already joined to a
     `ConductionArea` is skipped (`CONNECTED_TO_PLANE`). Otherwise start is the connected
     set and destination is `powerPlaneAreas(unconnected)`, or the unconnected set when
     that is empty. A signal net starts from the unconnected set and ends at the
     connected set.
  5. Run all sampled connections with `ripupAllowed = false`, then all again with
     `ripupAllowed = true` and `ripupCosts = startRipupCosts` (pass 1), in that order, on
     the one loaded board. A fresh engine per connection, nothing inserted. The autoroute
     tree carries over between connections (tie-pin reductions, room insert and remove),
     which is why the order is fixed.
  6. At most 200 connections per board (`max_connections`, recorded in the header).
- **`AutorouteControl` is plain data.** One struct with `Vec`s per layer, built by
  `AutorouteControl::for_net(board, net, settings, via_costs, trace_costs)` and
  `AutorouteControl::for_net_default(board, net, settings)` (Java's three-argument
  constructor), where `settings` is C03.7's `BoardRouterSettings`. Fields keep Java's
  names in snake case, so probe output maps one to one.
- **`rebuild_via_info` keeps Java's accumulation.** Java never resets `viaRadii` or
  `maxViaRadius` between calls, so a rebuild can only grow them
  (`AutorouteControl.java:240-286`). `RoutingBoard.fanout` rebuilds up to three times.
  Mark it `// PARITY(quirk):`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| A01.1 | `autoroute/mod.rs`, `autoroute/probe.rs`, `tools/java-reference/probes/autoroute/{AutorouteProbe.java, generate.sh, fixtures.tsv, README.md}`, `crates/freerouting/tests/autoroute_common/mod.rs`, `crates/freerouting/tests/autoroute_probe_harness.rs`, `fixtures/reference/probe/{selftest,connections}/` | `lib.rs` (append `pub mod autoroute;`) |
| A01.2 | `autoroute/java_ids.rs`, `fixtures/reference/probe/ids/` | `AutorouteProbe.java` (append kind) |
| A01.3 | `autoroute/control.rs` | — |
| A01.4 | `autoroute/distance.rs` | — |
| A01.5 | `crates/freerouting/tests/autoroute_probe_control.rs`, `fixtures/reference/probe/{control,distance}/` | `AutorouteProbe.java`, `fixtures.tsv` |

## Checklist

- [x] **A01.1 — Module skeleton, probe writer and connection sampler.**
  - `autoroute/mod.rs`: module docs naming the Java packages, and
    `AttemptState { Unknown, Skipped, NoUnconnectedNets, ConnectedToPlane,
    AlreadyConnected, NoConnections, Routed, Failed, InsertError }` plus
    `AttemptResult { state, details: String }` (`autoroute/AutorouteAttemptState.java`,
    `AutorouteAttemptResult.java`).
  - `autoroute/probe.rs`: `ProbeWriter` with `header(kind, board, commit)`,
    `record(&[(&str, ProbeValue)])` where `ProbeValue` is `Int(i64) | Bits(f64) |
    Bool | Str | List(Vec<ProbeValue>)`, and `fnv64(lines)`. `Bits` prints
    `"0x%016x"` of `f64::to_bits`.
  - Java: `AutorouteProbe.java` with the same writer, the sampler described under the
    decisions, and kinds `selftest` and `connections`. `generate.sh <checkout> <kind>|all`
    reads `fixtures.tsv` (kind, then fixture paths) and writes
    `fixtures/reference/probe/<kind>/<board>.jsonl`, or `.digest` above 1 MiB.
  - `tests/autoroute_common/mod.rs`: the Rust sampler, `compare_probe(kind, board,
    lines)` with the first-difference report, and digest handling (write
    `target/probe/<kind>/<board>.rust.jsonl`, print the `probe.sh` command).
  - Orders that decide results: the sampler's net order (ascending), item choice (first in
    descending id), and variant order (ripup off, then on).
  - **Accept:**
    - `selftest`: both sides print the same fixed values, including `-0.0`, `0.1`,
      `Double.MIN_VALUE`, `Integer.MIN_VALUE`, an empty list and a nested list.
      `probe_selftest_matches_java` passes.
    - `connections` (net, plane flag, start ids, destination ids, per variant) is equal on
      every fixture in the autoroute research's fixture table: `empty_board`,
      `scoring-perfect-two-pin`, `Issue954-circle`, `scoring-zero-length`,
      `scoring-mixed-layer`, `Issue508-SMD-routing-issue-demo`,
      `Issue269-min_fr_test/min_fr_test`, `Issue368-CorneyIslandWireless_input_design`,
      `Issue143-rpi_splitter`, `Issue143-rpi_splitter_mod`,
      `PCBench-retroreflectors-CONGFLOCK`, `Issue508-DAC2020_bm08`,
      `Issue110-testProjectFromFreeroutingBugTest`, `Issue026-J2_reference`,
      `Issue413-test`, `Issue676-ch32v-tx118s`, `Issue957-smoke-ldo-led`. This list is
      "the fixture ladder" in later A checklists; record it in `fixtures.tsv`.
    - A digest-mode mismatch, forced in a unit test, prints the `probe.sh` command.
  - *Done 2026-10-16:* `autoroute/mod.rs` (docs, `AttemptState::java_name`, `AttemptResult`
    with Java's `toString`), `autoroute/probe.rs` (`ProbeWriter`, `ProbeValue`, `fnv64`,
    `digest`, `first_difference`, `decode_bits`; 5 unit tests),
    `tools/java-reference/probes/autoroute/{AutorouteProbe.java, generate.sh, fixtures.tsv,
    README.md}` (the `ladder` row holds the 17 boards above), `tests/autoroute_common/mod.rs`
    (loader through `prepare_job`, sampler, `router_control`, `compare_probe`) and
    `tests/autoroute_probe_harness.rs`. `cargo test -p freerouting --test
    autoroute_probe_harness`: 3 passed — `probe_selftest_matches_java` (ints incl.
    `Integer.MIN_VALUE`/`Long.MIN_VALUE`, `-0.0`, `0.1`, `Double.MIN_VALUE`, NaN, -inf,
    escapes, `[]`, `[[1,-2],[],["0x3fe…",true]]`, FNV-1a), `probe_connections_match_java`
    (all 17 ladder boards equal, both variants), `digest_mismatch_prints_the_probe_command`.
    References from `generate.sh <a773a70ec worktree> all`; a second full run was
    byte-identical for all five kinds (`diff -r`). The `connections` kind prints the ripup
    values of each variant without building the control, because Java's constructor throws
    on `scoring-perfect-two-pin` (no via rule).

- [x] **A01.2 — Java id formulas.** `autoroute/java_ids.rs`, all `i32` with
  `wrapping_mul`/`wrapping_add`, `// PARITY:` on each:
  - `expansion_door_id(a, b) = min(a, b) * 31 + max(a, b)` (`ExpansionDoor.java:185-190`);
  - `target_door_id(item_id, room_id_or_0) = 31 * item_id + room_id`
    (`TargetItemExpansionDoor.java:71-74`; a null room gives 0);
  - `obstacle_room_id(item_id, index) = (item_id << 10) | index`
    (`ObstacleExpansionRoom.java:49-51`);
  - `incomplete_room_id(shape_id, layer) = 31 * shape_id + layer`
    (`IncompleteFreeSpaceExpansionRoom.java:38-41`; Java throws on a null shape, so the
    Rust function takes a shape, and the caller decides what a missing shape means);
  - `expansion_drill_id(location_id, first, last) = 31 * (31 * location_id + first) + last`
    (`ExpansionDrill.java:127-130`);
  - `drill_page_id(box_id, net) = 31 * box_id + net` (`DrillPage.java:193-196`).
  The complete free room's id is the engine counter (A02), not a formula.
  - **Accept:** probe kind `ids`: 1,000 inputs per formula drawn from `new Random(7)`
    (`nextInt()` over the full range, so overflow is exercised), plus the shape ids of
    the first 200 tree shapes of J2. Rust equal (`java_ids_match_java`).
  - *Done 2026-10-16:* `autoroute/java_ids.rs` (six functions, `// PARITY:` with the Java
    line on each, unit test `ids_overflow_like_java_int`). The kind is named `java-ids`
    (`fixtures/reference/probe/java-ids/Issue026-J2_reference.jsonl`), because B01 already
    owns `ids`. Random part: 6 × 1,000 inputs from `new Random(7).nextInt()` (Rust
    `JavaRandom::next_int_unbounded`, appended to `jmath` with `min_f64`). Tree part: the
    first 200 default-tree shapes of J2 in item-list order, with the ids of real Java
    `ObstacleExpansionRoom`, `IncompleteFreeSpaceExpansionRoom`, `ExpansionDrill` and
    `DrillPage` objects (a new page's net is -1) plus the door and target formulas on them.
    `cargo test -p freerouting --test autoroute_probe_control java_ids_match_java`: ok.

- [x] **A01.3 — `AutorouteControl`.** Port `autoroute/maze/AutorouteControl.java` as
  `autoroute/control.rs`:
  - the private constructor (`:140-193`): per-layer bend costs, the plane-layer rule (a
    non-signal layer that the settings mark active is forced inactive, with Java's warning
    text), defaults (`ripup_costs = 1000`, `ripup_pass_no = 1`, `pull_tight_accuracy =
    500`, shove depths 20/8/8, `tidy_region_width = i32::MAX`, `via_lower_bound = 0`,
    `via_upper_bound = layer_count`, `remove_unconnected_vias = true`);
  - `init_net` (`:210-238`): net class or the fallback (class 1, `viaRules.firstElement()`;
    check that `rules::default_via_rule` is the first rule, else add `first_via_rule`),
    half widths (net 0 uses net 1), compensated widths, inactive routing layers of the
    class;
  - `rebuild_via_info` (`:240-286`): via clearance class, `ViaMask`s, `attach_smd_allowed`,
    radii as `0.5 * shape.max_width()`, then `max` with the trace half width, then
    `max_via_radius`; `via_cost_factor = max(max_via_radius, 1)`, times 0.1 for a pure SMD
    net (`is_pure_smd_net`, `:195-208`); `min_normal_via_cost = via_costs as f64 *
    factor`; `min_cheap_via_cost = 0.8 * min_normal_via_cost`. Keep that operation order.
  - `ExpansionCostFactor { horizontal, vertical }` (O05 uses it), `ViaMask`, and the
    `add_via_costs` table (always 0 at the pin; keep it).
  - Fanout fields: `is_fanout`, `is_plane_fanout`, `fanout_start_pin_center`,
    `fanout_start_pin_layer`. Drop `fanoutStartPinName` (debug only).
  - **Accept:** unit tests for the plane-layer rule, the pure-SMD factor, the null-net
    fallback, and the radius accumulation across two `rebuild_via_info` calls. Full
    equality comes in A01.5.
  - *Done 2026-10-16:* `autoroute/control.rs`: `AutorouteControl::{for_net,
    for_net_default, rebuild_via_info}`, `ViaMask`, `ViaCost`, `ExpansionCostFactor` (the
    settings' `TraceCostFactor` re-exported, one type), `ControlError` for Java's throws
    (missing net, no via rules, class without via rule; `java_exception()` names Java's
    exception), `warnings` for the plane-layer text. `// PARITY(quirk):` on the two
    never-reset accumulations. The fallback rule is `via_rules.nth(0)`, which is what
    `rules::default_via_rule` returns. Unit tests in `tests/autoroute_probe_control.rs` on a
    synthetic 3-layer board with a GND plane: `control_plane_layer_is_forced_inactive`,
    `control_pure_smd_net_has_cheaper_vias` (bit-exact factor order), `control_missing_net_falls_back`
    (net 0 and net 99), `control_rebuild_only_grows_the_via_radii`; all 4 ok.

- [x] **A01.4 — `DestinationDistance`.** Port `autoroute/maze/DestinationDistance.java`
  (391 lines) as `autoroute/distance.rs`: `new(trace_costs, layer_active,
  min_normal_via_cost, min_cheap_via_cost)`, `join(box, layer)`, `calculate_point`,
  `calculate_box` and `calculate_cheap_distance`.
  - Must match: every `Math.min` chain and sum in Java's order (`:117-391`), the
    component-side, solder-side and inner-layer boxes, and `Integer.MAX_VALUE` returned for
    an empty box. No `mul_add`, no reassociation (D6).
  - Layout: the three boxes with their empty flags and the cost minima computed in the
    constructor (`:45-100`), plus the per-layer trace costs and active flags; no allocation
    per `calculate` call.
  - **Accept:** covered by A01.5's `distance` probe.
  - *Done 2026-10-16:* `autoroute/distance.rs`: Java's sums and `Math.min` chains in
    order (`jmath::min_f64`, `max_f64`; no `mul_add`), `int` deltas with `wrapping_sub`,
    `Integer.MAX_VALUE` for an empty destination, the minima in the constructor, the cheap
    distance as a parameter instead of Java's swap of the field (same values, no mutation);
    3 unit tests. `distance_matches_java` (A01.5) is bit-equal on all 17 ladder boards.

- [x] **A01.5 — `control` and `distance` probes; Java tests.**
  - Kind `control`: for every net, every field of `AutorouteControl` built as the
    router builds it (via costs `getPlaneViaCosts()` for a plane net, else
    `getViaCosts()`; the router's trace costs), with `ripupAllowed = true` and
    `ripupCosts = startRipupCosts`.
  - Kind `distance`: for each sampled connection, a `DestinationDistance` built in the
    probe and joined as `MazeSearchEngine.init` joins it (`MazeSearchEngine.java:996-1016`:
    every destination item in set order, every autoroute-tree shape's bounding box on its
    layer), without calling `getInstance`, so A01 needs no room code. Then 1,000 points
    and 1,000 boxes drawn from `new Random(11)` inside the board box, on every layer.
    Print the bits of each result.
  - Port `core/scoring/ScoringMazeCostIndependenceTest` (two tests; fixture
    `scoring-zero-length.dsn`).
  - **Accept:** `control` equal for every net on the whole fixture ladder;
    `distance` bit-equal on the ladder up to J2;
    `scoring_maze_cost_independence` (both cases) green.
  - *Done 2026-10-16:* `cargo test -p freerouting --test autoroute_probe_control`: 7 passed;
    `control_matches_java` equal on all 17 ladder boards (every field per net, plus a
    `rebuilt:` line after a second `rebuildViaInfo`; `scoring-perfect-two-pin` throws
    `NullPointerException` in Java and gives `ControlError::NoViaRule` in Rust, both printed
    as the same record); `distance_matches_java` bit-equal on all 17 boards (J2, DAC2020_bm08,
    the SMD demo and CONGFLOCK through their committed digests). `cargo test -p freerouting
    --test java_ports scoring_maze`: `scoring_maze_cost_independence::{maze_search_costs_stay_put_when_v2_board_score_weights_change,
    autoroute_control_via_cost_tracks_maze_settings_not_v2_weights}` ok.

# N06 — Fix the Java bugs kept on purpose

Opened 2026-10-17 (the owner asked whether they were fixed; they weren't). During the parity phase
the port reproduced Java's bugs wherever fixing one would change routing results. D10 ends that
phase, so each bug can now be fixed under profile `enhanced`, while `legacy` keeps it.

The inventory (N06.1, 2026-10-17) is the table below, with 54 rows:
- **12 of kind (a):** dead code or values nothing reads. Fixed outright, in N06.2.
- **31 of kind (b):** they change routes, imports or reports. Fixed under `enhanced` behind one
  switch each, in eight batches (N06.4–N06.11).
- **11 of kind (c):** judgement calls, in N06.12.
- **2 more of kind (b),** added 2026-10-18 by N06.14: the two simplex cutout failures that N06.4's
  invariant test found outside its rows (`cutout_hole_crosses_border`,
  `simplex_octagon_half_corner`).

The rows come from:
- `docs/DIVERGENCES.md` "Java bugs kept on purpose" (geometry, DSN, board model, shove,
  pull-tight, batch optimizer, connection insertion, interactive) and the SES scale paragraph;
- all 27 `// PARITY(quirk):` lines in `crates/freerouting` (12 rows);
- the module docs and comments that say "Java bug kept", "Kept Java quirk" or "Faithful to a Java
  bug";
- bugs that had no marker, found by reading the code around these.

Prerequisites: N01 (profiles), N02 (the totals rule), N05 (promotion). The geometry crate has no
settings access.

## What exists and what this adds

| Exists | Where |
|---|---|
| Profiles: `router.profile`, with `legacy` = Java and `enhanced` the default | `crates/freerouting/src/settings/profile.rs`; `ENHANCED` is still empty |
| The rule that each `enhanced` difference has a DIVERGENCES line | `tests/settings_profile.rs::every_enhanced_difference_is_a_setting_with_its_divergence_line` |
| Settings fields, paths and the settings file | `settings/fields.rs` (`RouterSettings`), `settings/path.rs` |
| What the switches ride on | `BoardRouterSettings` (`settings/board.rs`), `AutorouteControl` (`autoroute/control.rs`), `BasicBoard` (`board/basic_board.rs`), `ReadContext` (`io/specctra/parser/mod.rs`), geometry `split_to_convex` (`freerouting-geometry/src/shapes/`) |
| A test pinning each bug's Java behaviour | the "Test pinning Java today" columns below |
| The A14 exact set (106 boards, byte-identical to Java) | `tests/autoroute_board_parity.rs`; the command is in [A14](archive/A14-board-parity-gate.md)'s done note (about 1.5 h, release) |
| Measurement: preregistration, board sets, baselines | `docs/roadmap/routing-quality.md` "How to measure a quality change"; `docs/roadmap/prereg/*.json`; `tools/bench/sets/*.txt`; Q14 is a worked example ([Q14](archive/Q14-via-clearance-follows-net-class.md), `prereg/Q14.json`, `sets/q14-via-clearance.txt`) |

This checklist adds the `router.java_quirks.*` switches, the fixes behind them, and their
measurements.

## How to work this checklist

Whoever implements an item (the `implementer` agent, or Codex):
- **Follow `.claude/agents/implementer.md`:** the Java reference at the pin, the targeted gate,
  the `[x]`/`[~]` notes, and the hard rules (no windows on the owner's desktop, hands off Windows and
  KiCad, `CARGO_BUILD_JOBS=2`). Where the owner's dispatch says otherwise, the dispatch wins.
- **Read Java through git, never the checkout:**
  `git -C ../freerouting show a773a70ec:src/main/java/app/freerouting/<path>`. Line numbers here
  are at that pin; Rust line numbers drift, so find code by symbol.
- **Take one item per dispatch,** in the order of N00's note 4: N06.2 and N06.3 first.
- **Don't start heavy runs.** That means the A14 exact set, a batch measurement, or anything
  that routes the registered set. WSL crashes when two run at once, and the orchestrator runs
  them one at a time. Finish the code, the tests and the counters, then leave the item `[~]`
  with `*Partial <date>:* heavy run pending: <the exact command>`. The orchestrator runs it and
  ticks the box.
- **Write the preregistration yourself, before any number exists:** the board set in
  `tools/bench/sets/n06-<batch>.txt`, and `docs/roadmap/prereg/N06.<n>.json` with one arm per
  switch plus the all-on arm.
- **Keep every `// PARITY(quirk):` marker.** Add the switch's id to it
  (`// PARITY(quirk, java_quirks.<id>):`), so `legacy`'s path stays findable.
- **Build in your own target dir.** Run the gate with
  `CARGO_TARGET_DIR=target/orchestrator/n06-target` (or another folder of your own), never the
  main checkout's `target/`: other sessions' routing runs use it.
- **Q13 is in flight** (2026-10-17) and touches the autoroute and plane code. `autoroute/control.rs`
  and `autoroute/batch/pass.rs` may conflict when the two merge. Whichever lands second rebases.

## File ownership

| Item | Owns (creates or edits) |
|---|---|
| N06.2 | the files of the 12 (a) rows and their tests; the DIVERGENCES lines of those rows |
| N06.3 | `settings/fields.rs`, `settings/path.rs`, a new `settings/java_quirks.rs`, `settings/board.rs`, `autoroute/control.rs` (the `quirks` field only), `board/basic_board.rs` (the field, `deep_copy`, the snapshot), `io/specctra/parser/mod.rs` (`ReadContext`), the geometry `split_to_convex_with` entry points, `tests/settings_java_quirks.rs` |
| N06.4–N06.11 | the files in their rows' Rust column, their tests, `tools/bench/sets/n06-*.txt`, `docs/roadmap/prereg/N06.*.json`. The batches touch disjoint files except `autoroute/connection.rs` (M and F) and `optimize/pull_tight.rs` (P only), so run M and F one after the other |
| N06.14 | `geometry/shapes/simplex.rs`, `geometry/line.rs` (`nearest_through_integer_points`), `geometry/quirks.rs`, `geometry/tests/cutout_invariant.rs`, its two keys in `settings/fields.rs` and `settings/java_quirks.rs`, its two counters in `run_stats.rs`, `tests/n06_cutout_reach.rs`, `tools/bench/sets/n06-g2.txt`, `docs/roadmap/prereg/N06.14.json` |
| N06.12, N06.13 | their rows' files; `settings/profile.rs` (`ENHANCED`) and the DIVERGENCES "Profile `enhanced`" section, through N05 |

## Decisions made here

- **The three kinds:**
  - **(a) no effect on any output:** dead code, or a value nothing reads. Fix it outright in both
    profiles. Proof: the A14 exact set stays byte-identical, and a test pins the corrected
    value.
  - **(b) changes an output:** routes, an imported board, or a report. Fix it under `enhanced`
    only, behind one switch per bug; `legacy` keeps Java's behaviour. A value that only a report
    shows, but that a Java reference recording pins (statistics, `improvement_percentage`), is
    (b): it can't be fixed under `legacy`.
  - **(c) a judgement call:** a tuning constant, an inferred typo, or a "fix" that isn't clearly
    right. Measure it like any candidate (N02), or leave it with a reason.
- **The switches are settings:** `router.java_quirks.<id>` (the ids in the table), `true` =
  Java's behaviour.
  - Every key defaults to `true`. `enhanced`'s table (N05) turns one off when its batch is
    accepted.
  - So `legacy` is Java, and every fix can still be chosen by hand, as for any stage or option
    (N01).
- **One `JavaQuirks` value, no global state.** It is built from the settings at job start and
  carried where each bug lives:
  - `BoardRouterSettings`, copied into `AutorouteControl`: the autorouter, the fanout and the
    batch optimizer.
  - `BasicBoard`: shove and pull-tight see only the board. It must survive `deep_copy` and
    snapshot restore, because optimizer candidates run on copies.
  - An explicit argument in the geometry crate: `split_to_convex_with(quirks)` beside
    `split_to_convex`, and a flag on `TileShape::nearest_border_point`. A piece cache (the
    area's `OnceLock`) must not be shared across flag values.
  - The DSN `ReadContext` and `import_session` for the readers.
- **The accept rule for a (b) fix (owner, 2026-10-17: "if it's a bug fix and we fix it but it's
  neutral, might as well leave it in"):**
  - It is a correctness fix, so it need not beat noise.
  - A batch is accepted when N02's hard guards hold, no board regresses by more than twice its
    noise band, and the total is not worse by more than the set's noise.
  - A neutral fix stays in.
- **"No board reaches it" is measured, not argued.** A counter (or a `debug_assert!` in a debug
  run) on the buggy branch, run over the registered set, shows that the branch never runs. Such a
  fix is accepted on its tests: both behaviours pinned. A branch that does run is measured like
  the batch members.
- **One switch per bug, even inside a batch.** The batch is the first measurement. A fix that is
  neutral on the totals but is later suspected somewhere is measured alone by its own switch.
- **Measured in batches:** one N02 registration per batch, at least 3 boards plus `canary-pwr`
  through the plugin path. If a batch fails, bisect it switch by switch. A measurement needs
  N02.1–N02.2 (the totals rule in `freerouting-dev gate`). Until they land, a batch item ends `[~]`
  with its code and tests done.
- **Heavy runs go through the orchestrator** (see "How to work this checklist"). A14 is a 1.5 h
  release run, and every batch measurement routes many boards.

## Inventory

Rust paths are under `crates/freerouting/src/` unless they start with `geometry/`
(`crates/freerouting-geometry/src/`) or `tests/`. Java paths are under `app/freerouting/` at
`a773a70ec`.

"Reach" says where a real run hits the bug:
- **batch:** an autoroute or optimizer run (the CLI or the plugin);
- **GUI:** interactive use only;
- **in:** reading a file;
- **out:** a report;
- **dead:** never.

### (a) No effect on any output: N06.2

| id | Rust | Java | What goes wrong | Reach | Test pinning Java today |
|---|---|---|---|---|---|
| `line_length_overflow` | geometry/line.rs:497 | Line.java:561-566 | The squared length is computed in wrapping `i32`: NaN past ~46k units (axis-aligned), ~32k (45°) | dead (no caller; `Trace::length` is another function) | `function_values_and_length` |
| `stair45_y_case` | geometry/line_segment.rs:402 | LineSegment.java:432 | The function-of-y branch calls `function_value_approx` | dead (tests only, in Java too) | `stair_approximations_like_java`, `stair_approximation_corners` |
| `nearest_corner_min_value` | geometry/shapes/tile_shape.rs:325-339 | TileShape.java:449 | The minimum starts at `Double.MIN_VALUE`: the result is a corner at distance 0, else corner 0 | dead | none |
| `polygon_area_zero` | geometry/shapes/polygon_shape.rs:356 | PolygonShape.java:414-418 | The guard `dimension() <= 2` returns 0 for every 2-D polygon | dead (callers pass convex tile pieces) | `normalizes_orientation_and_start_corner` |
| `outline_transform_noop` | board/items/mod.rs:599-612, board/item_ops.rs:27-30, 291-311 | BoardOutline.java:265-306 | The for-each loops assign the moved shape to the loop variable, so the outline never moves | dead (the outline is `SystemFixed`; tests only) | `edge_pin_cache_invalidated_on_outline_transform` |
| `trace_rotate_no_clear` | board/item_ops.rs:28, 249-255 | PolylineTrace.java:156-159 | `rotateApprox` skips `clearDerivedData`; in the port only the `InfoCleared` journal entry is lost | GUI, journal only | none |
| `optimizer_pass_count_off_by_one` | autoroute/batch/pass.rs:565 | BatchAutorouter.java:285-287 | The pass count is one too many when routing completes | dead (`optimize/batch/item.rs:154` drops it) | `route_passes_for_optimizer_counts_as_java` |
| `door_section_realloc` | autoroute/session/doors.rs:178, search.rs:33/360/719, path/locate45.rs:14/314 | ExpansionDoor.java:193-201, MazeSearchEngine.java:394 | A changed section count replaces the array and loses its state; the pop writes to the old one | dead in practice (every caller in a session passes the same half width); prove it with a counter over A14 | `a_door_whose_section_count_changes_loses_its_state` |
| `via_radii_never_reset` | autoroute/control.rs:17/386/399, tests/autoroute_probe_control.rs:604 | AutorouteControl.java:264, 275 | `rebuildViaInfo` never resets `viaRadii`/`maxViaRadius` | dead (rebuilds only grow the rule) | `control_rebuild_only_grows_the_via_radii` |
| `fanout_only_negative_max_passes` | autoroute/pipeline.rs:38 | RoutingPipeline.java:104-114, AutorouteBatchLoop.java:285-288 | `max_passes < 0` turns the router off, but the fanout-only loop gets 0 (unlimited) and routes | dead (validation resets a negative value; API only) | `negative_max_passes_with_fanout_routes_as_javas_fanout_only_run` |
| `smooth_double_remove` | optimize/pull_tight.rs:181 | TraceTightener.java:443 | Removes an already removed trace a second time | no-op | none |
| `keep_point_pins_unrestored` | optimize/pull_tight.rs:20-21, 190-192 | TraceTightener.java:459-466 | The early return leaves the last step's contact pins | nothing reads them before `set_trace`/`take_contact_pins` | none |

### (b) Changes an output: N06.4–N06.11

| id | Rust | Java | What goes wrong | Reach | Batch | Test pinning Java today |
|---|---|---|---|---|---|---|
| `simplex_prev_division_line` | geometry/shapes/simplex.rs:460-463 | Simplex.java:730, 860 | `nextDivisionLine = prevDivisionLine` is backwards, so the previous line stays null and pieces at sharp inner corners can overlap | batch, hot: drill pages of every pass (`split_to_convex`), area tree shapes, any shape with a simplex | G | maybe `simplex_cutout_and_rotation_like_java` |
| `octagon_box_cutout_branch3` | geometry/shapes/int_octagon.rs:742-746 | IntOctagon.java:1206-1223 | Branch 3 moves the box to the octagon's right edge but rebuilds the octagon unchanged (branch 2 trims it), so the pieces overlap | batch, hot: drill pages, area tree shapes | G | none |
| `cutout_hole_crosses_border` | geometry/shapes/simplex.rs:586 (`CrossFirstLine`, :219) | Simplex.java:731-732, 756-768, 809-824 | A corner of the cut-out part on the outer border is not divided; its stand-in division line (the hole edge before it) still clips later pieces as the first division line, and turns that clipping on as a corner's last line, so pieces of a hole crossing the border vanish (half the rest in the pinned case). Fix: only divided corners' lines take part | batch, any-angle mode only: drill pages with simplex obstacles (45 and 90 degree trees keep octagons and boxes); the search trees' area splits (unwired, Java's code) | G2 (N06.14) | `java_breaks_the_invariant_outside_the_two_rows` |
| `simplex_octagon_half_corner` | geometry/shapes/simplex.rs:737 (`calc_division_lines`, `Division`, :170) | Simplex.java:1042-1057 | A corner that is not an integer point is taken to lie on the outer border and is not divided; an odd-parity octagon's diagonal corner or an enlarged simplex's corner lies inside, and the pieces beside it overlap (755,786 units² in the pinned case). Fix: divide a corner on no outer line at its exact point, each side taking the nearest line through integer points on its own side (`Line::nearest_through_integer_points`) | as `cutout_hole_crosses_border` | G2 (N06.14) | `java_breaks_the_invariant_outside_the_two_rows` |
| `init_failure_room_leak` | autoroute/connection.rs:31-40, 280, 670; session/rooms.rs:263 | AutorouteEngine.java:142-150 | A failed `getInstance` returns before `clear()`, so completed start rooms stay as obstacles | batch, rare (an init work limit or room error) | M | `an_init_failure_leaks_its_rooms_and_a_later_session_moves_them_aside` |
| `failed_attempt_ripped_carryover` | autoroute/connection.rs:26, 236; fanout_pin.rs:34 | AutorouteEngine.java:188, 227; RoutingBoard.java:1090-1125 | A rejected attempt leaves the ripped set filled, so the next attempt rips the earlier path's items | batch, rare (a retry after a disabled-layer or locate rejection) | M | `a_path_ending_on_a_disabled_signal_layer_fails_and_keeps_the_ripped_set` |
| `ortho_zero_width_room` | autoroute/session/neighbours90.rs:419 | SortedOrthogonalRoomNeighbours.java:296-301 | `roomShape.ur.x` written twice: the room right of an obstacle has width 0 and is never made (cases 0 and 2 are right) | batch, 90° mode only, on most obstacle doors | M | indirect: `variants_match_java` |
| `edge_corner_not_advanced` | autoroute/session/incomplete45.rs:32 | Sorted45DegreeRoomNeighbours.java:263 | `currentCorner` never advances, so a full sweep always skips side 7 | batch, 45° (default), rare (an obstacle room with no sorted neighbours); count it first | M | indirect: `maze_log_matches_java` |
| `connect_to_trace_swapped_width` | autoroute/path/insert.rs:128 | FoundConnectionInserter.java:113, 128 | The start and target stubs take each other's layer half width | batch, rare (a trace end, layers differ, per-layer widths differ) | M | none |
| `fanout_stale_drill_carry` | autoroute/connection.rs:269; fanout_pin.rs:34, 270; session/drills.rs:515 | AutorouteEngine.java:91, 321-331; RoutingBoard.java:1077-1125 | Fanout attempts 2 and 3 reuse attempt 1's drill pages: occupied sections stay occupied and cached drills hold stale rooms, so nearly every via site is rejected | batch, every fanout pin that needs a retry (fanout is on by default) | F | `a_carried_engine_keeps_its_drills_and_occupied_sections` |
| `drill_destination_room_index` | autoroute/path/backtrack.rs:111 | FoundConnectionLocator.java:255 | `roomArr[firstLayer + section]` on an array that starts at `firstLayer` | batch, very rare (a fanout destination drill of a blind or buried via) | F | none |
| `fanout_pin_order_names` | autoroute/batch/fanout_rules.rs:44-48 | BatchFanout.java:811-846 | The documented names `distance_to_closest_on_net` and `surroundings_density` fall back to pin-index order | batch, only when a user sets those names | F | `pin_sorting_order_names` |
| `in_front_of_pad_typo` | shove/drill_check.rs:9-12, 456-458 | ForcedPadRouter.java:78 | Side 0 takes `min(a.x+a.y, b.x+b.x)` | batch, occasional (the via check moving a via that has substitute traces; side 0 only) | S | `in_front_of_pad_keeps_java_typo` |
| `store_trace_class_self_compare` | shove/trace_entries.rs:19-22, 327-329 | ShapeTraceEntries.java:385 | The contact trace's clearance class is compared with itself | batch, rare (same-net traces of different classes meeting at an end corner) | S | none |
| `rational_perp_projection` | geometry/point.rs:517 (via tile_shape.rs:219, 236) | RationalPoint.java:239-283 | The y term adds `det·vx·z` where it should subtract, and the denominator misses `z` | batch, rare (shove, a rational end corner inside the offset shape) | S | none |
| `smooth_loses_trace` | optimize/pull_tight.rs:16-19, 167-178 | TraceTightener.java:431-441 | The end-corner smoothing removes the trace first and leaves it removed when the smoothed insert fails | batch, rare: after every routed connection and in every optimizer candidate; no probe board reaches it | P | none |
| `pull_tight_reused_lines_change` | optimize/pull_tight.rs:22-23, 100-105; board/trace_ops.rs:682-687 | PolylineTrace.java:837-839, 966-979 | Reports a change when `change_trace` returns early because every line is reused, so the end smoothing is skipped | batch, rare | P | none (the `pull-tight` probe covers the paths) |
| `opt_first_pass_stops` | optimize/batch/mod.rs:33-38, 351-355, 490-492 | BatchOptimizer.java:404, 462-467, 812-815 | A pass with no winner (or a 0 % one) clears the increased ripup costs "to keep going", but the loop ignores it and stops | batch, every default run (pass 1 often ends after 12 failures) | O | `first_pass_without_winner_stops` |
| `opt_equal_key_skip` | optimize/batch/candidates.rs:17-21, 160-169 | BatchOptimizer.java:1186-1239 | The strictly-greater cursor skips every candidate whose (x, y, layer) key equals an earlier one, for the whole pass | batch, every pass; a few % of traces (shared end corners) | O | `equal_key_skips_rest_of_pass` |
| `dsn_via_alias` | io/specctra/parser/network.rs:122-140 | parser/Network.java:1328-1335 | With no structure `via`, the first class's `use_via` list is shared, so later classes' vias join its rule | in; no corpus board (all 206 with `use_via` have a structure via) | I | none |
| `dsn_class_class_pairs` | io/specctra/parser/network.rs:743-765 | parser/Network.java:563-593 | Two loops share one iterator: with 3 or more classes only (first, other) pairs get rules | in; no corpus board | I | none |
| `dsn_shape_bbox` | io/specctra/parser/shape.rs:150-197 | parser/Rectangle.java:25, Circle.java:57-63, PolygonPath.java:111-130 | An unsorted rectangle stays unsorted, a circle takes its diameter as radius, a path adds the half width to max x on every step | in (the board box and scale); no corpus board | I | none |
| `dsn_unit_ignored` | io/specctra/dsn_reader.rs:155-157 (reads only the pcb `resolution`) | parser/Placement.java (never reads its `resolution`) | The placement `resolution` and the pcb-level `unit` are ignored | in; no corpus board has a differing unit | I | none |
| `ses_reader_scale` | io/specctra/ses_reader.rs:46-47 | SesReader.java:70-72 | The reader divides by `dsn_to_board(1)/resolution` where it should multiply (the writer is right) | in: every SES import; wrong where the factor isn't 1 (Issue676: 100×) | I | `written_session_reimports_to_the_same_statistics` (Issue676 expected) |
| `stats_line_ab_length` | scoring/board_statistics.rs:582-597 | BoardStatistics.java:252-263 | Sums the distance between each line's two defining points instead of the segment lengths | out (statistics, manifest) | I | `kicad_parity.rs:650` expected mismatch, `reference_load.rs` |
| `route_result_int_via_ratio` | autoroute/item_route_result.rs:60-65 | ItemRouteResult.java:59-65 | `viaCountAfter / viaCountBefore` in `int`: 2/3 counts as 0 | out (`improvement_percentage`); its one routing reader, `== 0.0`, differs only on a float coincidence | I | `integer_division_differs_from_real_division`, `improved_and_percentage_match_java` |
| `drc_units_10000_per_mm` | drc/checker.rs:519-521, 691-697, 750 | DesignRulesChecker.java:455-475, 1338-1365 | Summary messages, congestion zones and hints hard-code 10000 board units per mm | out, at other resolutions | I | none |
| `snapshot_drops_compensation` | board/snapshot.rs:19-21 | BasicBoard.java:1508-1520 | Restore and deep copy rebuild the default tree uncompensated and drop the compensated trees | GUI (compensation is only on there) | U | `deep_copy_drops_clearance_compensation`, `deep_copy_drops_compensated_trees` |
| `rules_matrix_trees_not_rebuilt` | io/specctra/rules_reader.rs:21-29 | RulesReader.java:284-288 | A rules file changes the clearance matrix in place without rebuilding the trees | GUI with compensation on (in batch the trees are built later) | U | none |
| `move_drill_tidy_region` | shove/insert.rs:30-33, shove/forced.rs:487-528 | DrillItemMover.java:145, RoutingBoard.java:270-291 | `insert` unions into its own copy of the tidy region, so a finite tidy width never pull-tightens after a move | GUI (drag, move), non-default `trace_tidy_width` | U | `move_drill_item_finite_tidy_skips_pull_tight` |
| `route_change_layer_no_via` | interactive/route/layer.rs:155-195, interactive/route.rs:25-28 | gui/interactive/Route.java:282 | With no via spanning both layers, the layer change "succeeds" without a via | GUI | U | `layer_change_without_spanning_via_quirk` |
| `route_connect_inner_break` | interactive/route.rs:28-30, 641-646 | gui/interactive/Route.java:411-415 | Only the inner loop is left, so the second corner pair is tried from the added corner | GUI | U | none |
| `select_extension_ignores_filter` | interactive/select.rs:19-22 | InspectedItemState.java:75ff | After an extension's filter, the inspect state stays even with an empty selection | GUI | U | none |

Batches: **G** geometry cutouts (**G2**: N06.14's two rows), **M** maze search and rooms, **F** fanout, **S** shove, **P** pull-tight,
**O** batch optimizer, **I** inputs and reports, **U** GUI and interactive.

### (c) Judgement calls: N06.12

| id | Rust | Java | The question | Reach | Proposal |
|---|---|---|---|---|---|
| `ripup_pass_no_unset` | autoroute/session/ripup.rs:12, 337; control.rs:138 | MazeRipupResolver.java:158-162, AutorouteControl.java:192 | `ripupPassNo` is never set, so the random detour factor `0.5 + r²` never fires; the batch router never had it (the 1.x import set it only in `ExpandTestState`) | batch, every rip-up from pass 4 on if enabled | Its own registration: switching on randomness Java never ran, deterministic by its reseed. Likely large either way. **Outcome (N06.12, 2026-10-18): measured, run pending.** Wired (`batch::connection::router_control` sets the pass number under `false`), tested (`tests/n06_judgement.rs`), counted (`ripup_random_pass_connections`, `ripup_random_factor_draws`) and registered as the verdict pair of `prereg/N06.12-ripup-pass.json` (accept: improve, since it adds behaviour Java never ran); stays `true` until the run accepts it |
| `insert_stale_trace_object` | autoroute/path/insert.rs:100 | FoundConnectionInserter.java:108-137 | `connectToTrace` uses the start or target trace object after the insertion split or removed it | batch, occasional | Leave: the split keeps the old geometry as the new pieces, so the stub lands on the same copper. **Outcome (N06.12): left as Java.** The stale object only supplies the geometry `connectToTrace` connects to (`insert.rs`, `trace_object`); a split keeps that geometry as its pieces, so the stub lands on the same copper and a fix that reads the pieces instead changes nothing on a split. The switch stays inert (`false` changes nothing) |
| `in_front_of_pad_side0_flank` | shove/drill_check.rs:461 | ForcedPadRouter.java:86 | Side 0's flank clause takes `min`, where cases 2, 4 and 6 take `max`; inferred from symmetry | as `in_front_of_pad_typo` | An arm of batch S, only together with the typo fix. **Outcome (N06.12): follows N06.7's run.** Wired and tested by N06.7 (`side0_flank_switch_both_behaviours`, counter `shove_in_front_of_pad_flank_flips`) and registered as `prereg/N06.7.json`'s reported arm `all_flank`, judged against `all` only if batch S is accepted; otherwise it stays `true`. N06.7's done note records the result |
| `opt_threshold_fraction` | optimize/batch/mod.rs:197-212, settings/path.rs:357-364 | BatchOptimizer.java:367-377 | A threshold in (0, 0.1) is multiplied by 100 (Java logs it as a heuristic); a CLI value below 0.001 is scaled twice | only JSON or environment values in (0, 0.1) | Leave Java's heuristic; fix the double scaling of the CLI. **Outcome (N06.12): heuristic left as Java (`true` in both profiles, `false` tested by `threshold_fraction_switch_both_behaviours`); the double scaling fixed as a port fix in both profiles** (`SettingsPatch::improvement_threshold_scaled`; `a_cli_threshold_below_a_thousandth_is_scaled_once`, `a_cli_threshold_is_scaled_once`; DIVERGENCES "Settings"). (a)-like: no A14 board, Java reference case (`-oit 0.035`, `4.0`, `0.05`) or plugin option (1-100 %) passes a CLI value below 0.001 |
| `rules_unknown_layer_scope` | io/specctra/rules_reader.rs:21-29 | RulesReader.java | A layer scope naming an unknown layer applies to all layers | in, `-dr` only | Decide: ignore the scope with a warning, or keep it. **Outcome (N06.12, decided 2026-10-18): ignore it with a warning under `enhanced`, keep Java's under `legacy`.** Wired in `rules_reader::apply_rules` (`false` logs a second warning and skips the scope), tested (`unknown_layer_scope_switch_both_behaviours`), counted (`rules_unknown_layer_scopes`). Defaults stay `true`; N06.13 adds it to `ENHANCED` through N05. A wrong layer name in a rules file is an input error, and spreading its widths and clearances to every layer is the one reading that is surely wrong |
| `rules_autoroute_settings_reenable` | io/specctra/rules_reader.rs:21-29 | RulesReader.java | A rules file's `autoroute_settings` re-enables every layer and the optimizer | in, `-dr` only | Decide with C04: a settings semantics choice. **Outcome (N06.12): left as Java.** C04's decisions keep it on purpose: a rules file's `autoroute_settings` are the file's whole routing setup, laid over the merged settings and the CLI (C04.3, `rules_autoroute_settings_override_cli`), and `AutorouteSettings.readScope` seeds every layer and the optimizer on. C04 makes no other settings-semantics choice, so the switch stays inert (`false` changes nothing) |
| `direction_compare_zero` | geometry/direction.rs:346 | Direction.java | `compare_to` handles the zero direction lopsidedly | batch, edge case | Leave: a quirk, not clearly wrong. **Outcome (N06.12): left as Java.** An ordering of the zero direction is a convention, not a bug; changing it would reorder sorted directions with no correct order to aim for. The switch stays inert (the geometry crate has no settings access) |
| `route_stubs_layer_test` | drc/route_stubs.rs:9-12, 136 | WindowRouteStubs.java:73-74 | Both via bounds are compared with the contact's first layer, so the stub layer is always the last | out (route-stubs list) | Leave until the intent is clear. **Outcome (N06.12): left as Java.** It changes only the route-stubs report's layer, not routing; which layer the report should name for a via stub is not clear from Java, so there is no fix to test. The switch stays inert |
| `any_angle_skip_length_squared` | optimize/tighten/any_angle.rs:43-45 | TraceTightenerAnyAngle.java:22, 774, 896 | `SKIP_LENGTH = 10` is compared with a squared distance (effectively 3.2 units) | batch, any-angle mode only | Measure in any-angle mode, or leave. **Outcome (N06.12): measured, run pending.** Cheap to include: a reported arm of `prereg/N06.12-ripup-pass.json` over three any-angle variant boards (A07's `-any.dsn`), counted (`any_angle_skip_length_flips`) and tested (`any_angle::tests::skip_length_switch_both_behaviours_and_counter`); judged no-regress (a typo-like fix), stays `true` until the run |
| `polyline_array_reversal` | optimize/tighten/mod.rs:19-22, 491 | Polyline.java (`new Polyline(Line[])`) | The constructor reverses lines in the caller's array, and later reads see that | batch | Leave: no clear correct fix. **Outcome (N06.12): left as Java.** Later reads of the caller's array (bisection steps, changed areas, accepted lines) depend on the reversal, and `normalized` replays it; copying the array would change those reads with no reference for the intended ones. The switch stays inert |
| `avoid_acid_traps_disabled` | optimize/tighten/mod.rs:377-381 | TraceTightener (`if (true) return polyline;`) | Java turned the acid-trap step off | batch | Leave: enabling it is a feature (a Q candidate), not a fix. **Outcome (N06.12): left as Java; noted as a Q candidate** (turn on Java's dead acid-trap step and measure it as a quality change under N02). The switch stays inert |

### Reviewed and left out

These are marked or look like quirks but keep no bug:
- `autoroute/distance.rs:7`: Java's arithmetic order only. Its inner bound becomes 0 when an
  outer layer is inactive, which is admissible, just slower; that would be a Q item.
- `autoroute/batch/control.rs:72, 195`: `globalBestScore` is never reset by a restore. That is
  Java's design.
- `autoroute/control.rs:59`: `ViaCost` is always 0, a Java feature never filled in.
- `autoroute/batch/strict_drc.rs:8`: both rollback forms are kept by design.
- `autoroute/path/insert.rs:12-17, 338, 347`: Java's reference identity on points.
- The "Java warns" branches (backtrack.rs:146, locate, locate45, neighbours90, search): error-path
  parity, inert unless the data is already inconsistent.
- The GUI and `freerouting-kicad` "kept" notes are UI rules and KiCad emulation, not Java bugs.
  The two Net Classes dialog bugs are already fixed.
- `PolylineTrace.change` also ignores a new polyline that is a strict prefix or suffix of the old
  one by identity (trace_ops.rs:685). Pull-tight never drops end lines, so it is probably
  unreachable. N06.8 checks it: the counter `trace_change_prefix_suffix_ignored` read 0 in
  every light run (N06.8's note); the registered run confirms it.

## Checklist

- [x] **N06.1 — Inventory and sort.** The tables above.
  - *Done 2026-10-17:*
    - Read-only sweep of the DIVERGENCES lists, every `PARITY(quirk)` line, and the module docs
      and comments in `crates/freerouting`, `freerouting-geometry`, `freerouting-dsn`,
      `freerouting-kicad` and `freerouting-gui`, each checked against the Java at `a773a70ec`.
    - Result: 54 rows (12 a, 31 b, 11 c) and the reviewed list.
    - The two geometry cutout rows were confirmed by hand: `Simplex.java:860` assigns
      `nextDivisionLine = prevDivisionLine`, and `IntOctagon.java`'s branch 3 rebuilds the
      octagon with its old `topY`.
- [x] **N06.2 — The (a) fixes.** Fix the 12 (a) rows outright, each with a test of the
  corrected value. Before fixing `door_section_realloc`, add a counter that the A14 run
  reports; the fix lands only if it reads 0. Correct the doc at `geometry/line.rs:495`: the overflow starts at ~46k units
  axis-aligned, ~32k only at 45°.
  - **Accept:**
    - the A14 exact set stays byte-identical (a heavy run: leave the command in a `[~]` note);
    - the B02, B03 and A04 probes are unchanged;
    - each row's DIVERGENCES entry moves from "kept on purpose" to the fixed Java bugs.
  - *Partial 2026-10-17:* Eleven corrections implemented and verified by the gates
    below; `door_section_realloc` deliberately retains Java behavior until A14 proves
    its new counter is zero. The session counter distinguishes initial allocation,
    reuse, and replacement; the existing state-loss regression test pins these values.
    `PARITY_N06_COUNTERS=1` makes A14 write `n06-counters.json` beside its summaries and
    refuses cached/resumed records. No heavy run was started.
    - [x] Four geometry helpers corrected; private-target `cargo test -p freerouting-geometry`
      passed 111 unit tests, 5 reference tests and 3 predicate tests, including the
      steep-stair transpose regression (`target/orchestrator/n06/geometry-tests.log`).
    - [x] Outline transforms, trace rotation journal, pass count, via-radius rebuild and
      fanout-only routing corrected; focused `autoroute_pass`, `autoroute_pipeline`,
      `autoroute_probe_control` and `item_transform` tests passed (55 tests before the
      additional trace-rotation regression test).
    - [x] Pull-tight contact-pin restoration and single-removal regression:
      `keep_point_smoothing_restores_contact_pins` passes in release in the private target;
      it asserts that smoothing split at the interior keep point, restored the saved pins,
      and logged one removal of the original trace.
    - [x] Private-target release B02/B03/A04 and pull-tight probes: 26 tests passed,
      2 timing/debug tests ignored. Command (with `CARGO_BUILD_JOBS=2` and
      `CARGO_TARGET_DIR=target/orchestrator/n06-target`):
      `cargo test --release -p freerouting --test board_probe_tree --test board_probe_tree_ops --test board_probe_tree_visits --test autoroute_search_sinks --test reference_pull_tight --test autoroute_probe_control`.
      Log: `target/orchestrator/n06/probes-release.log`. Existing Java expected mismatches
      are unchanged; no comparison was loosened.
    - [x] `cargo fmt --all --check` and `python3 tools/roadmap/checklists.py status --strict`.
    - [x] Private-target `cargo clippy --workspace --all-targets -- -D warnings`
      passed with `CARGO_BUILD_JOBS=2`; log `target/orchestrator/n06/clippy-final.log`.
    - [x] CLI statistics regression (`cargo test -p freerouting-cli --test stats`,
      private target): 3 passed. The total tree-work assertion now reads parsed JSON,
      rather than relying on `tree_nodes_visited` being the last counter field; it also
      checks that the new reallocation counter is present. SES equality and repeatability
      assertions are unchanged.
    - [x] Final private-target `cargo test --workspace` passed: 2197 tests passed,
      56 intentionally ignored, zero failures across 203 test/doc-test suites.
      Log: `target/orchestrator/n06/workspace-tests-final.log`.
    - [x] A14 (orchestrator, 2026-10-18, on `df655e1`, release, `PARITY_N06_COUNTERS=1`, 3996 s):
      `test result: ok`. exact 105 boards: 103 pass, 2 expected (Issue684, Issue721), 0 fail,
      102 SES byte-identical and 103 equivalent (Issue110-RelayModule differs only in a
      non-ASCII component name, which Java's scanner drops); cutoff 10/10 pass, all
      byte-identical, class sums identical (771 unrouted, 1528 violations, 3971 vias);
      java-failure 3/3 as listed; 0 D8 breaches added. `n06-counters.json`:
      `door_section_reallocations` = 0 over 118 boards. Summary:
      `target/orchestrator/n06-2/parity/summary.md`.
    - [x] `door_section_realloc`: the counter reads 0, so the fix may land. A change confined to
      the reallocation branch of `allocate_sections` cannot change any A14 session, since that
      branch never runs there, so it needs its unit test, not another A14 run. N06.2 is
      ticked when it lands.
      - **Condition for skipping A14 (reviewer checks it):** every changed line must run
        only after `allocate_sections`' `len != 0` branch has fired in that session.
        - A fix to "the pop writes to the old one" (`search.rs:33/360/719`,
          `locate45.rs:14/314`) that makes every pop resolve sections through the door's
          current `(start, len)` runs on every pop, so it is not covered by the counter. It
          needs a short argument that without a reallocation the resolution gives the same
          slot as today.
        - If that argument is in any doubt, rerun A14 with the command above.
  - *Done 2026-10-18:* final door fix in worktree `target/orchestrator/codex-n06`, branch
    `codex/n06-door` (from `origin/main` at `7a4f73b`). The A14 prerequisite above was also
    checked against `target/orchestrator/n06-2/parity/n06-counters.json`: 0 over 118 boards.
    - `allocate_sections` preserves all fields of the surviving prefix, initializes newly
      added sections and drops removed sections. All changed executable allocation lines
      are inside the existing `len != 0` branch; initial allocation and same-count reuse
      follow the original paths. The counter is retained.
    - Queue-pop occupation now calls `occupy_current_section`, resolving the surviving
      section after expansion. **No-reallocation argument:** a popped door already has
      a nonempty range. Its `(start, len)` can change only in the counted reallocation
      branch, so with zero reallocations the second lookup returns exactly the original
      `SectionSlot::Flat(start + section)`. Appending other objects does not move arena
      indices. Drill ranges and target-door slots are stable during a pop; drill-page
      pops return before this occupation write. Removed indices can be skipped only
      after a reallocation. Thus the new lookup writes the same field of the same slot
      on every no-reallocation execution, with the same queue order and work budget.
      `path/locate45.rs` is unchanged. No further A14 is needed under this dispatch's rule.
    - Regression (historical name retained):
      `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo test -p freerouting --lib a_door_whose_section_count_changes_loses_its_state`:
      **1 passed**, 307 filtered out. Covers complete state retention, stable reuse slots,
      appends for another door, shrink/grow/defaults, occupation of the current rather
      than obsolete range, removed indices, and emptying/reallocating a range.
      Log (relative to the worktree): `target/orchestrator/n06-door/unit.log`.
    - Additional checks passed: `cargo fmt --all --check`,
      `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo clippy -p freerouting --lib -- -D warnings`,
      `git diff --check`, and `python3 tools/roadmap/checklists.py status --strict`.
- [~] **N06.3 — The quirk switches.** Add `router.java_quirks.<id>` for every (b) and (c) row
  that might be fixed, and the `JavaQuirks` value carried as the decisions say. Wire each
  switch to its site, keeping Java's code path when the switch is `true`.
  - **Accept:**
    - with every switch `true`, A14 is byte-identical;
    - each switch has a unit test showing both behaviours (the tests in the tables become the
      `true` half);
    - the keys round-trip through a settings file and `--router.java_quirks.<id>=false`;
    - the board's quirks survive `deep_copy` and snapshot restore.
  - *Partial 2026-10-18:* the plumbing, 26 of the 42 read sites, and the tests below. N06.10
    and N06.11 wired six more (`dsn_unit_ignored` and the five U rows); the G and (c) read
    sites below remain.
    - [x] A14 with every switch `true` (orchestrator, 2026-10-18): full A14 on origin/main
      `7a4f73b` (release, `--test-threads=1`, `PARITY_SUBDIRS=1`; log
      `target/orchestrator/heavy/a14-7a4f73b.log`): 1 passed, 138 boards (120 exact, 15 cutoff,
      3 Java failures), every SES byte-identical except the known Issue684 and Issue721
      (expected), Issue110-RelayModule (non-ASCII name only) and the Java-failure boards.
    - [x] The switches: 42 `opt` bool keys in a `java_quirks` section of `settings/fields.rs`
      (31 (b) + 11 (c), the inventory's ids and order), unset = `true` = Java in both profiles,
      so Java's settings dump and `legacy` are unchanged and `ENHANCED` stays empty.
      `settings/java_quirks.rs` generates `JavaQuirks` (`JAVA`, `ALL_FIXED`, `IDS`,
      `from_settings`, `get`, `with`, `fix_item`, `split`). Carried as the decisions say:
      `BoardRouterSettings::java_quirks` → `AutorouteControl::quirks`; `BasicBoard::java_quirks`
      (kept by `BoardSnapshot`, so by `deep_copy` and every restore); `ReadContext::java_quirks`
      through `build_board_with`/`read_board_from_str_with`; `prepare_job` reads the board with
      the first merge's switches and puts the final ones on the board before the session import,
      `prepare_board_for_routing` too. Geometry: `freerouting_geometry::SplitQuirks`,
      `PolylineArea::split_to_convex_with` (one `OnceLock` per switch value, so no piece cache is
      shared across values), `Area::split_to_convex_with`, `TileShape::cutout_with`,
      `Simplex::cutout_from_with`, `IntOctagon::cutout_from_box_with`, and the projection flag
      (`Point`/`Line::perpendicular_projection_with`, `TileShapeOps::nearest_point_with` and
      `nearest_border_point_with`).
    - [x] Read sites with Java's path under `true` and the fix under `false` (26):
      `ortho_zero_width_room`, `edge_corner_not_advanced`, `connect_to_trace_swapped_width`,
      `init_failure_room_leak`, `failed_attempt_ripped_carryover`, `fanout_stale_drill_carry`,
      `drill_destination_room_index`, `fanout_pin_order_names`, `in_front_of_pad_typo`,
      `store_trace_class_self_compare`, `rational_perp_projection` (shove's end-corner
      projection), `smooth_loses_trace`, `pull_tight_reused_lines_change`, `opt_first_pass_stops`,
      `opt_equal_key_skip`, `dsn_via_alias`, `dsn_class_class_pairs`, `dsn_shape_bbox`,
      `ses_reader_scale`, `stats_line_ab_length`, `route_result_int_via_ratio`,
      `drc_units_10000_per_mm`, `ripup_pass_no_unset`, `opt_threshold_fraction`,
      `any_angle_skip_length_squared`, `route_connect_inner_break`. Each carries
      `// PARITY(quirk, java_quirks.<id>):`; every older `PARITY(quirk)` marker of a (b)/(c) row
      got its id (the (a) rows' markers are N06.2's).
    - [ ] Read sites deferred (16, now 9 after N06.10, N06.11 and N06.7 wired theirs; N06.7 wired `in_front_of_pad_side0_flank`; the switch exists and reaches the code, `false` changes
      nothing yet, and its doc line says so): `simplex_prev_division_line`,
      `octagon_box_cutout_branch3` (both reach the cutouts, N06.4 writes the fixes after its
      invariant test; the search tree's area shapes in `board/search/mod.rs` still call
      `split_to_convex`), `dsn_unit_ignored` (N06.10), the six U rows `snapshot_drops_compensation`,
      `rules_matrix_trees_not_rebuilt`, `move_drill_tidy_region`, `route_change_layer_no_via`,
      `select_extension_ignores_filter` (N06.11; `route_connect_inner_break` is wired), and the (c)
      rows `insert_stale_trace_object`, `in_front_of_pad_side0_flank`, `rules_unknown_layer_scope`,
      `rules_autoroute_settings_reenable`, `direction_compare_zero` (geometry, no settings access),
      `route_stubs_layer_test`, `polyline_array_reversal`, `avoid_acid_traps_disabled` (N06.12).
      No deferred site is in `doors.rs`, `search.rs` or `locate45.rs`.
    - [x] Both behaviours tested (7): `in_front_of_pad_keeps_java_typo` (extended),
      geometry `rational_perpendicular_projection_switch_both_behaviours`,
      `parser::shape::tests::bounding_box_switch_both_behaviours`,
      `optimize::batch::tests::threshold_fraction_switch_both_behaviours`, and in
      `tests/settings_java_quirks.rs` `fanout_pin_order_names_both_behaviours`,
      `route_result_via_ratio_both_behaviours`, `trace_length_statistics_both_behaviours`. The
      other 19 wired switches get their `false` half in their batch item (N06.5–N06.10,
      N06.12), whose measurement needs them anyway.
    - [x] Round trips (`tests/settings_java_quirks.rs`, 9 tests, all pass):
      `each_key_round_trips_through_a_settings_file` (`{"router": {"java_quirks": {"<id>":
      false}}}`, every id), `each_key_is_set_on_the_command_line_and_in_the_environment`
      (`--router.java_quirks.<id>=false`, `FREEROUTING__ROUTER__JAVA_QUIRKS__<ID>`, the CLI
      beating a file), `both_profiles_keep_every_java_bug`,
      `every_switch_is_a_settings_key_in_inventory_order`,
      `the_job_carries_the_switches_to_the_board_and_the_control` (`prepare_job` → board,
      `BoardRouterSettings`, `AutorouteControl`),
      `the_board_keeps_its_switches_through_deep_copy_and_restore` (deep copy and three
      restores of one snapshot: rebuild, memo fill, memo clone).
    - [x] With every switch `true` routing is unchanged on the fast subset (release, private
      target): `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p
      freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
      --test-threads=1`: exact 7 boards, 7 pass, 0 fail, 7 SES byte-identical.
- [~] **N06.4 — Batch G, geometry cutouts:** `simplex_prev_division_line`,
  `octagon_box_cutout_branch3`.
  - First, an invariant test that fails on Java's code: over random holes in boxes and
    simplexes, the piece areas sum to outer minus inner, with no two pieces overlapping.
  - Then the fixes, behind `split_to_convex_with(quirks)`.
  - **Accept:** the invariant holds with both switches `false`, and the batch is measured
    under the decisions' accept rule. The drill pages change on most boards, so this is likely
    the largest geometry effect.
  - *Partial 2026-10-18:* code, tests, counters and registration done (worktree
    `target/orchestrator/n06g/wt`, branch `land/n06-4`, on main `b93bcd0`); heavy run pending.
    Its verdict (N02's totals rule) feeds N06.13's promotion through N05; wiring the search
    trees' splits (below) joins N06.3's deferred read sites.
    - [x] Invariant first: `crates/freerouting-geometry/tests/cutout_invariant.rs`, 5 tests,
      all pass. 600 seeded (`JavaRandom`) cases per outer/inner pair of box, octagon and simplex.
      The domain is holes inside the outer shape whose corners, as a simplex, are integer
      points: 1584 cutouts. With Java's code (`SplitQuirks::JAVA`) 111 of them break the
      invariant, every one a box minus an octagon with overlapping pieces; with both switches
      `false` 0 (`fixed_cutouts_tile_the_rest_of_the_outer_shape`,
      `java_breaks_the_invariant_only_through_branch_3`). Overlaps at most 1.5 units wide are
      not counted: an odd-parity octagon keeps an edge of length -1 after `normalize`, and the
      pieces beside it share a one-unit strip in both arms.
    - [x] `octagon_box_cutout_branch3` fixed: under `false` branch 3 trims the upper right
      octagon's top to the widened upper box (`int_octagon.rs`, as branch 2 does on the left).
      `box_cutout_branch3_switch_both_behaviours` pins both values on a hand-made case.
    - [x] `simplex_prev_division_line`: the inventory's fix (`prevDivisionLine =
      nextDivisionLine`) is under `false`, and it is **inert**. The assigned line is the
      corner's own first division line `D[i][0]`. Both merge tests then fail: the sharp piece's
      is `det(D[i][0], D[i][0]) = 0`, and `calc_division_lines` keeps a second line only while
      `det(D[i][0], D[i][1]) >= 0`. The proof is in `Simplex::cutout_from_counted`'s docs.
      `the_simplex_switch_changes_no_piece` finds identical pieces under both values in all
      5400 cases (3000 involve a simplex), with the merge counter at 0. The overlaps that the
      inventory row blames on this line come from elsewhere (next bullet). One alternative,
      `prev = D[i-1][last]`, does fire, but it opened gaps on 10 contained simplex-minus-simplex
      cases, so it was not kept.
    - [x] Two Java failures outside both rows, pinned under all four switch values by
      `java_breaks_the_invariant_outside_the_two_rows` and not fixed (candidate new rows):
      - a hole crossing the outer border loses pieces, because its corners on the border are
        not integer points and get no division line (a triangle across a box: half the rest
        is missing);
      - an odd-parity octagon cut out of a simplex: its half-integer corner (12761.5,
        -2288.5) counts as a border corner, so nothing is divided there and pieces overlap by
        755,786 units².
    - [x] Reachability counters: `SplitReach`, filled by the new `_counted` variants
      (`PolylineArea::split_to_convex_counted`, `TileShape::cutout_counted`,
      `Simplex::cutout_from_counted`, `IntOctagon::cutout_from_box_counted`). Both arms count
      the branch-3 firings that change a piece, and the merges the simplex fix would add.
      The run statistics expose them as `octagon_box_cutout_branch3_reached` and
      `simplex_prev_division_line_reached`. They are recorded at the drill-page split
      (`autoroute/session/drills.rs`) and at the tie-pin cutout
      (`BasicBoard::reduce_trace_shape_at_tie_pin`).
      - `the_branch_3_counter_counts_the_changed_cutouts`: 247 random cutouts change and 247
        are counted.
      - `tests/n06_cutout_reach.rs::drill_page_splits_report_both_counters_in_both_arms`:
        pass 1 of `Issue269-min_fr_test` reaches branch 3 7 times in each arm, and the simplex
        merge 0 times.
    - [ ] Not wired: the search trees' shapes of obstacle and conduction areas and of the
      outline's generated keepout still split with Java's code in every arm. Those sites are
      `board/search/mod.rs` `item_tree_shapes`/`board_outline_tree_shapes` and
      `BoardOutline::keepout_piece_count` in `board/items/mod.rs`. They are outside this row's
      files: the trees take a `ShapeContext`, which `shove/cutout.rs` also builds, and the
      board's switches are set only after the reader inserts the items, so the tree shapes
      would need a rebuild. `unwired_area_splits_corpus_inventory` (ignored, read-only)
      reports per corpus board how often those splits reach the two branches.
    - [x] Registered before any number: `docs/roadmap/prereg/N06.4.json` (sha256
      `392f0310ee74989380bf1e8a5fb4f57f470cdeab0e3e98a238ee4d710d3dd598`) and
      `tools/bench/sets/n06-g.txt`.
      - Arms: `baseline`, `simplex_prev_division_line`, `octagon_box_cutout_branch3`, `all`.
        All run the default profile and default passes, and every arm sets both keys
        explicitly.
      - Rule: totals, no-regress. Rows: the four DSN fixtures of N06.10 and N06.11, plus
        canary-pwr through the plugin path.
      - Hard checks: the simplex arm must equal the baseline byte for byte, and `all` must
        equal the branch-3 arm.
      - Pinned by `crates/freerouting-dev/tests/prereg_n06_4.rs` (4 tests, pass).
    - [x] Defaults unchanged: both keys unset are `true` in both profiles, and Java's path is
      the `true` branch at both `PARITY(quirk, java_quirks.<id>)` markers.
      `cargo test -p freerouting-geometry -p freerouting --no-fail-fast` (private target):
      134 suites, 1508 passed, 0 failed, 33 ignored. No parity set was run (dispatch rule).
    - [ ] Heavy run pending (orchestrator, one job at a time):
      `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.4.json --out target/orchestrator/n06-g`.
      - `n06_run.py` exits non-zero when sessions differ, which is expected for the branch-3
        arms. It writes `summary.json` with every run's counters first.
      - Then grade each SES with `gate collect` and `freerouting-dev widths`, and judge the
        pairs with `freerouting-dev gate --rule totals`.
      - Then the plugin canary run and grade, per the registration's `plugin_canary` block.
      - Also the corpus inventory (read-only; the path must be absolute):
        `N06_CUTOUT_COUNTERS_OUT=/home/thebo/code/freerouting-rs/target/orchestrator/n06-g-corpus.json CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo test --release -p freerouting --test n06_cutout_reach unwired_area_splits_corpus_inventory -- --ignored --test-threads=1`.
- [~] **N06.5 — Batch M, maze search and rooms:** `init_failure_room_leak` (with the fix
  `session.clear()` before `InitFailed`, which also makes the rekey dead under `enhanced`),
  `failed_attempt_ripped_carryover` (snapshot and restore the set per attempt),
  `ortho_zero_width_room` (`b.ur.x`), `edge_corner_not_advanced` (advance the corner; first
  count how often the branch at `neighbours45.rs:286` runs), `connect_to_trace_swapped_width`
  (swap the indices).
  - **Accept:** the batch is measured, with one 90° board in the set for
    `ortho_zero_width_room`.
  - *Partial 2026-10-18:* code, both-behaviour tests, reachability counters and the
    registration are done; heavy run pending (commands below). Defaults stay `true` (Java)
    in both profiles; `ENHANCED` is unchanged. Every `// PARITY(quirk, java_quirks.<id>)`
    marker is kept.
    - [x] The five switches were wired by N06.3; N06.5 checked each `false` path against Java
      at `a773a70ec` and changed one: `failed_attempt_ripped_carryover = false` now restores
      the ripped set and the ripup costs only for an attempt rejected before its rip
      (`connection::conclude_attempt`; N06.3 restored the set after any non-routed result,
      which also dropped the items an attempt had ripped before a failed insert). The 90° and
      45° room code moved into testable helpers with Java's order kept
      (`neighbours90::incomplete_room_90`, `ortho_right_of_obstacle_room`;
      `incomplete45::edge_room_sides`, `edge_room_octagon`, `incomplete_room_45`). The
      stale Java line reference at the 90° site (271-276) is now 296-301.
    - [x] Both behaviours, one test per switch, all pass:
      `tests/n06_maze.rs::init_failure_switch_both_behaviours_and_counter` (Java leaves the
      completed start room and the next session moves it to a negative key and fails; `false`
      leaves an empty tree and the next search routes);
      `connection::tests::rejected_attempt_ripped_set_switch_both_behaviours` (rejected on a
      disabled layer: Java keeps the added item and costs, `false` restores both; a routed
      attempt keeps them in both arms);
      `neighbours90::tests::right_of_obstacle_room_switch_both_behaviours` (Java's box
      `[200, 100, 200, 160]` is never made, the fixed `[200, 100, 1000, 160]` is, door
      dimension 1); `incomplete45::tests::edge_room_sides_switch_both_behaviours` (full sweep:
      regular octagon Java sides 0-6, fixed 0-7; box Java 0-5, fixed 0, 2, 4, 6) and
      `of_the_flipped_box_sides_only_the_left_room_is_made`;
      `tests/n06_maze.rs::connect_to_trace_width_switch_both_behaviours_and_counter` (the
      `F.Cu` stub to a target wire from a `B.Cu` path: half width 2000 under Java, 1000 fixed).
      `an_init_failure_leaks_its_rooms_and_a_later_session_moves_them_aside` and
      `a_path_ending_on_a_disabled_signal_layer_fails_and_keeps_the_ripped_set` (the `true`
      halves) are unchanged and pass.
    - [x] Reachability counters (`run_stats`, `--stats json` `total.counts`, counted in either
      arm, only with statistics on; the configured value is always the one used):
      `maze_init_failure_room_leaks`, `maze_ripped_carryovers`,
      `maze_ortho_zero_width_room_flips` (rooms the fix would make), `maze_edge_sweeps_without_neighbours`
      (the branch at `neighbours45.rs` that calls the full sweep, as the item asks),
      `maze_edge_corner_flips` (edge rooms the other value adds or drops),
      `insert_connect_to_trace_width_flips` (stubs whose half width differs between the
      layers). Pinned by the two `n06_maze.rs` tests above (1 each), the ripped-set unit test
      (1 rejected, 0 routed) and, on one connection per net of J2 with ripup on (debug test
      build, not a registered run): `ortho_zero_width_room_reaches_a_90_degree_board` (J2 at
      90°: 32 flips under Java, 33 fixed; maze pops 6618 against 6610, so the fix changes the
      searches) and `edge_corner_counters_on_a_45_degree_board` (J2 at 45°: 27 sweeps, 27
      flips in each arm, pops 8858 in both). `run_stats::Counts` has 36 counters now, so its
      `Default` is written out (arrays derive it only up to 32).
    - [x] Registered before any number: `docs/roadmap/prereg/N06.5.json` (sha256
      `bf9a692bca53916a6d8f0d02bb49f58ae34c67be872d35c45b34152c82748ec3`) and
      `tools/bench/sets/n06-m.txt`, pinned by `crates/freerouting-dev/tests/prereg_n06_5.rs`
      (4 tests pass). Arms: `baseline`, each switch alone, `all` (the verdict pair with
      `baseline`). Rows: A07's 90° variants `Issue026-J2_reference-90` and
      `Issue143-rpi_splitter-90` (no fixture routes at 90°), J2, bm08 and Issue676 at 45°,
      profile `legacy`, default passes, plus canary-pwr through the plugin path (canary
      9511d9a, N06.10's procedure). Rule: reachability per switch (counter 0 and sessions
      identical: accepted on tests), else N02 totals, no-regress, set noise 10.95. No
      registered row has per-layer trace widths (only Issue103 and Issue187 have
      `layer_rule`), so `insert_connect_to_trace_width_flips` is expected to read 0. A smoke
      run of the `baseline` and `all` commands (debug CLI, `fixtures/scoring-perfect-two-pin.dsn`,
      not a registered row) exits 0, writes the six counters (all 0) and gives the same SES.
    - [ ] Heavy run pending (orchestrator, release build of the measured tree, run from its
      root, e.g. `/home/thebo/code/freerouting-rs` after landing):
      `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.5.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-m`
      (35 runs; the arms' commands call
      `/home/thebo/code/freerouting-rs/target/orchestrator/n06-target/release/freerouting`;
      it exits non-zero after the last run if a session differs, and then the totals rule
      applies to `summary.json`'s `statistics`). Then the canary-pwr plugin row as in N06.10
      (the registration's `plugin_canary`: export once, route the 7 arms with the exported
      args plus `--router.profile=legacy` and the arm's five flags, grade each distinct
      session with canary's `grade.sh`).
- [~] **N06.6 — Batch F, fanout:** `fanout_stale_drill_carry` (a fresh `DrillCarry` per
  attempt, as the batch router already does), `drill_destination_room_index` (index by
  section), `fanout_pin_order_names` (accept both spellings).
  - **Accept:**
    - the batch is measured on boards with fanout on;
    - the done note gives how many fanout retries succeed before and after.
  - *Partial 2026-10-18:* code, both-behaviour tests, counters (the retry counter included)
    and the registration are done; heavy run pending (commands below). Defaults stay `true`
    (Java) in both profiles; `ENHANCED` is unchanged. Every
    `// PARITY(quirk, java_quirks.<id>)` marker is kept.
    - [x] The three switches were wired by N06.3 and match the inventory's fixes (checked
      against Java at `a773a70ec`: `RoutingBoard.java:1074-1126`,
      `FoundConnectionLocator.java:255`, `BatchFanout.java:811-846`). The fanout's per-pin
      engine is now `fanout_pin::PinEngine` (the drill pages plus an attempt count);
      `begin_attempt` drops the pages under `false`, as before, and counts the retries.
      The `drill_destination_room_index` comment no longer says that every fixture's drill
      starts at layer 0: page drills do, a blind or buried via's drill does not.
    - [x] Both behaviours, one test per switch, all pass (`tests/n06_fanout.rs`, 3 tests):
      `stale_drill_carry_switch_both_behaviours_and_counters` (`fanout_pin` on an `F.Cu`
      pin whose two targets are `B.Cu`-only pins, every via site refused by a 50 mm minimum
      escape: the closest target fails, the all-targets retry runs and fails in both arms;
      `drills_computed` 25 under Java, whose retry finds the net's page drills cached, 50
      fixed, whose retry computes them again);
      `drill_destination_room_index_switch_both_behaviours_and_counter` (`backtrack` from a
      blind via's drill on layers 1-2 of 3: section 0 gives the layer-2 room under Java and
      the layer-1 room fixed; section 1 is `IndexOutOfBounds` under Java and the layer-2
      room fixed); `pin_order_names_switch_reaches_the_fanout_run_and_counter` (`FanoutRun::new`
      with `distance_to_closest_on_net`: Java pin-index order `[0, 1, 2]`, fixed the
      camelCase name's order, which differs). `settings_java_quirks.rs`
      `fanout_pin_order_names_both_behaviours` is unchanged and passes.
    - [x] Counters (`run_stats`, `--stats json` `total.counts`, counted in either arm, only
      with statistics on; the configured value is always the one used; `Counts` has 43
      now): `fanout_retries` (a pin fanout's attempts after its first),
      `fanout_retries_succeeded` (of those, the ones that end routed or already connected:
      the number the accept clause asks for, before = `baseline`, after =
      `fanout_stale_drill_carry` and `all`), `fanout_stale_drill_carries` (retries whose
      engine carried an earlier attempt's pages), `drill_destination_room_index_flips`
      (backtracks from a destination drill not starting at layer 0),
      `fanout_pin_order_name_flips` (fanout runs with one of the two spellings). Pinned by
      the three tests above (1 retry, 0 succeeded, 1 carried in each arm; 4 flips for 4
      backtracks; 1, 1 and 0 flips for the snake name in both arms and the camelCase name).
      One fanout pass in a debug test on eight boards outside the registered set
      (Issue269 min_fr_test, Issue508 SMD-routing-issue-demo, Issue558 dev-board, Issue153,
      Issue690 sonde_xilinx and ecc83, Issue093, Issue753) made no retry (`fanout_retries`
      0 on each); how many the registered rows make, over all fanout passes, is the heavy
      run's number.
    - [x] Registered before any number: `docs/roadmap/prereg/N06.6.json` (sha256
      `5f49d83f452bd05a5690dbf0b52c13554aa4d46663cedade3de1dad276bf51f6`) and
      `tools/bench/sets/n06-f.txt`, pinned by `crates/freerouting-dev/tests/prereg_n06_6.rs`
      (4 tests pass). Arms: `baseline`, each switch alone, `all` (the verdict pair with
      `baseline`); every command gives `--router.profile=legacy`,
      `--router.fanout.enabled=true` and the three switches explicitly, default passes.
      Rows, chosen by what the loaded boards hold (SMD pins, and SMD pins on a plane net,
      whose fanout makes a plane attempt first), counted without routing: Issue676 (3 of 14
      SMD pins on a plane net), Issue957-smoke-ldo-led (4 of 12), Issue367-Charger (44 of
      76), Issue756-tomu-fpga (116 of 205), Issue269-z10_module (113 of 315, 4 layers) and
      bm08 (A12's fanout probe board, no plane), plus canary-pwr through the plugin path
      (canary 9511d9a, N06.10's procedure, with `--router.fanout.enabled=true` added). Rule:
      reachability per switch, else N02 totals, no-regress, set noise 11.14. No registered
      board has a blind or buried via, so `drill_destination_room_index_flips` is expected
      to read 0, and no arm sets `fanout.pin_sorting_order`, so `fanout_pin_order_name_flips`
      reads 0. A smoke run of the `baseline` and `all` commands (debug CLI,
      `fixtures/scoring-perfect-two-pin.dsn`, not a registered row) exits 0, runs the
      `fanout` stage, writes the five counters (all 0) and gives the same SES.
    - [ ] Heavy run pending (orchestrator, release build of the measured tree, run from its
      root, e.g. `/home/thebo/code/freerouting-rs` after landing):
      `cd /home/thebo/code/freerouting-rs && CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 /home/thebo/code/freerouting-rs/tools/bench/n06_run.py /home/thebo/code/freerouting-rs/docs/roadmap/prereg/N06.6.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-f`
      (30 runs; the arms call
      `/home/thebo/code/freerouting-rs/target/orchestrator/n06-target/release/freerouting`;
      the board paths are relative, so it runs from the root; it exits non-zero after the
      last run if a session differs, and then the totals rule applies to `summary.json`'s
      `statistics`). Then the canary-pwr plugin row as in N06.10 (the registration's
      `plugin_canary`: export once, route the 5 arms with the exported args plus
      `--router.profile=legacy`, `--router.fanout.enabled=true` and the arm's three flags,
      grade each distinct session with canary's `grade.sh`). The done note then gives
      `fanout_retries` and `fanout_retries_succeeded` summed over the rows for `baseline`
      and for `fanout_stale_drill_carry` and `all`.
- [~] **N06.7 — Batch S, shove:** `in_front_of_pad_typo`, `store_trace_class_self_compare`,
  `rational_perp_projection` (the arm with `in_front_of_pad_side0_flank` follows if the batch
  is accepted).
  - **Accept:** the batch is measured; the (c) arm is measured or explained.
  - *Partial 2026-10-18:* code, both-behaviour tests, reachability counters and the
    registration are done; heavy run pending (commands below). Defaults stay `true` (Java)
    in both profiles; `ENHANCED` is unchanged.
    - [x] The three (b) switches were wired by N06.3 (`shove/drill_check.rs`
      `in_front_of_pad_with`, `shove/trace_entries.rs` `store_end_corner`, geometry
      `perpendicular_projection_with`). The (c) switch `in_front_of_pad_side0_flank` is now
      wired too: under `false` side 0's second flank clause takes `max(x + y)`
      (`ForcedPadRouter.java:86` takes `min`, where sides 2, 4 and 6 test the extreme past
      their diagonal). Every `// PARITY(quirk, java_quirks.<id>)` marker is kept.
    - [x] Both behaviours, one test per switch, all pass:
      `shove::drill_check::tests::in_front_of_pad_keeps_java_typo` (typo);
      `tests/n06_shove.rs::side0_flank_switch_both_behaviours` (flank: Java not in front,
      fixed in front; sides 1-7 and `with_sides = false` unchanged);
      `tests/n06_shove.rs::store_trace_class_switch_both_behaviours` (two same-net traces of
      different classes meeting inside the shape: Java stores, `false` stops at the contact;
      same classes: both store); `shove::trace_entries::tests::rational_end_corner_projection_switch_both_behaviours`
      (rational end corner (5001, 5000.5): Java's entry is the corner (7317, 7317) on edge 2,
      `false` the exact foot (7317, 5000.5) on edge 1); geometry
      `rational_perpendicular_projection_switch_both_behaviours` (unchanged).
    - [x] Reachability counters (`run_stats`, in `--stats json` `total.counts`, counted in
      either arm, only with statistics on, the returned value always the configured one):
      `shove_in_front_of_pad_typo_flips` and `shove_in_front_of_pad_flank_flips` (side-0
      tests whose result the switch's other value flips), `shove_store_trace_class_flips`
      (contacts not blocked by Java's test, of another class, with the end corner inside),
      `shove_rational_projection_flips` (rational end corners whose entry point, side and
      position, differs between the two projections). Pinned by
      `tests/n06_shove.rs::in_front_of_pad_counters_count_flips_in_both_arms`,
      `rational_projection_counter_counts_in_both_arms` (1 for a rational corner, 0 for an
      integer one) and the class test above.
    - [x] Registered before any number: `docs/roadmap/prereg/N06.7.json` (sha256
      `5f9b910c8a0258c0aa5fc29b0acde6b788626b481169b79ece235acaaeda94a4`) and
      `tools/bench/sets/n06-s.txt`, pinned by `crates/freerouting-dev/tests/prereg_n06_7.rs`
      (4 tests pass). Arms: `baseline`, each switch alone, `all` (the verdict pair with
      `baseline`) and the reported (c) arm `all_flank`, judged against `all` only if the batch
      is accepted. Rows: Issue026-J2_reference, bm08, Issue676 (N06.10/11's), Issue313-FastTest
      and Issue027-zMRETestFixture (several net classes), profile `legacy`, default passes,
      plus canary-pwr through the plugin path. Rule: reachability (every flip counter 0 and
      sessions identical: accepted on tests), else N02 totals, no-regress, set noise 10.95.
      `tools/bench/n06_run.py` now also copies each run's final `unrouted`,
      `clearance_violations` and `vias` (the manifest's `board_statistics`) into
      `summary.json`; a smoke run on `fixtures/scoring-perfect-two-pin.dsn` (debug build, 6
      arms) wrote them and the four counters.
    - [ ] Heavy run pending (orchestrator, release build of this tree):
      `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.7.json --out target/orchestrator/n06-s`
      (30 runs; it exits non-zero after the last run if a session differs, and then the
      totals rule applies to `summary.json`'s `statistics`). Then the canary-pwr plugin row as
      in N06.10 (the registration's `plugin_canary`: export once, route the 6 arms with the
      exported args plus `--router.profile=legacy` and the arm's four flags, grade each
      distinct session with canary's `grade.sh`).
- [~] **N06.8 — Batch P, pull-tight:**
  - `smooth_loses_trace`: test the failure conditions before removing, or re-insert the old
    trace;
  - `pull_tight_reused_lines_change`: report a change only when `change_trace` changed the
    trace;
  - the prefix/suffix identity case of the reviewed list: confirm it is unreachable, or add it.
  - **Accept:** the batch is measured.
  - *Partial 2026-10-18:* code, both-behaviour tests, reachability counters and the
    registration are done; heavy run pending (commands below). Defaults stay `true` (Java)
    in both profiles; `ENHANCED` is unchanged.
    - [x] Both switches were wired by N06.3; their sites are now two board methods that the
      pull-tight paths call, each keeping its `// PARITY(quirk, java_quirks.<id>)` marker:
      `RoutingBoard::replace_smoothed_trace` (the smoothing's remove and insert; returns
      `SmoothedReplace::{Inserted, Kept, Lost}`) and `RoutingBoard::apply_pull_tight_result`
      (`change_trace` and the reported change). `smooth_loses_trace = false` tests the
      insert's failure conditions first (`smoothed_insert_fails`: fewer than two corners, or
      zero length and not user fixed) and keeps the trace, so the rejected insert takes no
      item id either; a `debug_assert!` checks under Java's path that the test agrees with
      `insert_trace_without_cleaning` (held through the whole `cargo test -p freerouting` run).
    - [x] Both behaviours, all pass (`tests/n06_pull_tight.rs`, debug):
      `smooth_loses_trace_switch_both_behaviours` (a closed polyline and a two-line one: Java
      `Lost`, the trace gone; `false` `Kept`, the trace unchanged; a valid polyline and a
      user-fixed zero-length one insert in both arms),
      `pull_tight_reused_lines_switch_both_behaviours` (a result with every line reused: Java
      reports a change, `false` none, the trace unchanged in both; a changed result reports
      in both), `prefix_suffix_identity_case_is_ignored_and_counted`.
    - [x] Reachability counters (`run_stats`, in `--stats json` `total.counts`, counted in
      either arm, only with statistics on): `pull_tight_smooth_lost_traces` (smoothed
      inserts that fail), `pull_tight_reused_lines_flips` (pull-tight results `change_trace`
      ignores) and `trace_change_prefix_suffix_ignored` (`change_trace` early returns with
      another line count); with N06.4's, N06.5's and N06.6's, `COUNTERS` has 46 entries. A smoke run of `tools/bench/n06_run.py`
      on a copy of the registration pointed at `fixtures/scoring-perfect-two-pin.dsn`
      (release, 4 arms) wrote all three counters (0) and identical sessions.
    - [x] The prefix/suffix identity case (reviewed list): kept as Java has it (no switch),
      counted by `trace_change_prefix_suffix_ignored`. Light evidence that it and the two
      switch branches do not run (temporary prints, removed): 0 hits outside
      `tests/n06_pull_tight.rs` in the whole `cargo test -p freerouting` run (1383 passed;
      it includes `reference_pull_tight`'s 41-board `opt_changed_area`), 0 in release CLI
      runs (`--router.profile=legacy`) of five unregistered boards (Issue297-myboard,
      Issue209-split10, Issue742-tastexx-pcb, Issue508-DAC2020_bm10, Issue433-my-board),
      and 0 in 50,000 random trace bundles pulled tight on the two-pin board. The registered
      run decides it: 0 in every run confirms the case unreachable; above 0 adds it as a (b)
      row with its own switch.
    - [x] Registered before any number: `docs/roadmap/prereg/N06.8.json` (sha256
      `c9cbe6f11146a1157bfc9d1d92cc588f4bd6567afac85fd6e107cd5778aee633`) and
      `tools/bench/sets/n06-p.txt`, pinned by `crates/freerouting-dev/tests/prereg_n06_8.rs`
      (4 tests pass). Arms: `baseline`, each switch alone, `all` (the verdict pair with
      `baseline`). Rows, chosen from the A14 exact boards that complete (so the optimizer,
      pull-tight in every candidate, runs): Issue026-J2_reference, Issue155-CH376_MCP795_Module,
      Issue508-DAC2020_bm06, Issue558-dev-board, Issue217-8088sbc, Issue034-Green14SegLED,
      profile `legacy`, default passes, plus canary-pwr through the plugin path (N06.10's
      procedure). Rule: reachability (both switch counters 0 and sessions identical:
      accepted on tests), else N02 totals, no-regress, set noise 11.14.
    - [ ] Heavy run pending (orchestrator, release build of the tree carrying this diff, from
      its root): `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.8.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-p`
      (24 runs; the arms' binary is
      `/home/thebo/code/freerouting-rs/target/orchestrator/n06-target/release/freerouting`;
      it exits non-zero after the last run if a session differs, and then the totals rule
      applies to `summary.json`'s `statistics`). Then the canary-pwr plugin row as in N06.10
      (the registration's `plugin_canary`: export once, route the 4 arms with the exported
      args plus `--router.profile=legacy` and the arm's two flags, grade each distinct
      session with canary's `grade.sh`).
- [~] **N06.9 — Batch O, batch optimizer:** `opt_first_pass_stops` (a clear inside a pass
  allows one more pass at normal ripup costs), `opt_equal_key_skip` (keep every sorted
  candidate).
  - This is the batch most likely to move `canary-pwr`, because it runs in every default run.
  - `opt_first_pass_stops` adds a pass, so the registration records time as well as quality.
  - Measure each fix alone, then both.
  - Correct the `optimize::batch` module docs and DIVERGENCES: 0 % winners come from boards
    without vias, not from the int division.
  - **Accept:** the three arms are measured.
  - *Partial 2026-10-18:* code, tests, docs and the registration are done; heavy run pending
    (the three arms against `java`, commands below).
    - [x] `opt_first_pass_stops` fix: the loop's after-pass decision is
      `optimize::batch::pass_end_decision`. Under `false` a pass in which `opt_route_pass`
      cleared the increased ripup costs goes on for exactly one more pass at normal costs,
      whatever the score did. That is the intent of Java's `routeImproved = -1; // to keep the
      optimizer going with lower ripup costs` (`BatchOptimizer.java:812-815`; 1.x's loop went on
      for `route_improved < 0`). N06.3's wiring went on only when the score had not risen. Under
      `true` nothing changes: `cleared_in_pass` stays false, and the decision is Java's
      `:462-469`. Tests: `optimize::batch::tests::pass_end_decision_both_behaviours` (8 cases),
      and `tests/batch_optimizer.rs::first_pass_switch_both_behaviours`. In that test, on bm08
      with every candidate failing, `true` runs 1 pass and `false` 2: pass 1 is bit-identical,
      pass 2 has no winner and one threshold stop, and both runs end on the same board hash.
      `first_pass_without_winner_stops` is unchanged and still the `true` half.
    - [x] `opt_equal_key_skip` (wired in N06.3, `candidates.rs`): the `false` half is
      `equal_key_switch_keeps_every_candidate`. At the shared key it gives [higher-id via,
      lower-id via, trace]; the keys are non-decreasing; Java's list is exactly the first of
      each key; `prepare_candidates` follows the switch. `equal_key_skips_rest_of_pass` is the
      `true` half (its setup moved into the shared `equal_keys()` helper).
    - [x] Docs corrected: the `optimize::batch` module docs (a new "A winner of 0 %" paragraph,
      and the kept quirks listed with their switches and tests), the `candidates` module docs,
      and the DIVERGENCES "Batch optimizer" kept-on-purpose line. An improved result is 0 %
      exactly when the board had no vias (or no length) before. The integer via ratio gives 0
      only when the two ratios sum to exactly 2 (a connection routed at exactly the old
      length). Pinned by `zero_improvement_needs_a_board_without_vias`; it agrees with Java's
      probe row `[9, 0, 1, 1000, 900, 1, 0] -> improved, 0x0` in `item_route_result.rs`.
    - [x] Registered before any number: `docs/roadmap/prereg/N06.9.json` (sha256
      `b8be8b1aae14f390d3a2de0a52f72853d525a13bf7881f03fced9541df8ec449`) and
      `tools/bench/sets/n06-o.txt`.
      - Arms: `java` (baseline), `first_pass`, `equal_key` and `both` (the candidate, the
        verdict pair); the two single-switch pairs are reported.
      - Rule: totals, no-regress.
      - Rows: canary-pwr through the plugin DSN `plugin-b1994423.dsn`, bm01 and bm02, and
        K09's stripped interf_u, pic_programmer and CM5_MINIMA_3.
      - Metrics include `optimizer_passes`, `optimizer_seconds` and `wall_seconds`.
      - Pinned by `crates/freerouting-dev/tests/prereg_n06_9.rs` (4 tests).
    - [x] Defaults unchanged. With every switch at its default, the fast subset in release
      (private target) gives exact 7 boards, 7 pass, 0 fail, 7 SES byte-identical. The command
      is `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p freerouting
      --test autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1`.
      The optimizer's `true` paths are pinned by the unchanged `tests/batch_optimizer.rs`,
      `tests/reference_optimize.rs` and `java_ports.rs` tests in the workspace run
      (`cargo test --workspace --no-fail-fast`, private target: 230 suites, 2421 passed,
      0 failed, 53 ignored).
    - [ ] Heavy run pending (orchestrator, one job at a time; release build of this tree):
      - Each row and each arm runs `freerouting -de {DSN} -do {SES} --gui.enabled=false
        --result-json {OUT}/result.json {ROW_FLAGS} {ARM_FLAGS}`. `{ARM_FLAGS}` is empty for
        `java`, `--router.java_quirks.opt_first_pass_stops=false` for `first_pass`,
        `--router.java_quirks.opt_equal_key_skip=false` for `equal_key`, and both flags for
        `both`.
      - How each row is graded:
        - demos: `tools/kicad-oracle/grade.sh <demo .kicad_pcb> -- <that command>`, then
          `via_in_pad.py`;
        - canary: `ROW_FLAGS` from the registration, then canary's grade.sh steps;
        - bm01 and bm02: `freerouting-dev widths <DSN> <SES>` plus `gate collect`'s statistics.
      - Then `freerouting-dev gate --records <records> --prereg docs/roadmap/prereg/N06.9.json`
        for the verdict pair and each reported pair.
      - Note before running: preflight guard 1 skips the optimizer on any board left with an
        unrouted connection. So on canary-pwr, which the plugin path does not complete, every
        arm should give the same session. The registration records `optimizer_bypassed` per
        row. The claim that batch O "most likely moves canary-pwr" holds only where the
        router completes.
- [x] **N06.10 — Batch I, inputs and reports:** the five DSN and SES rows, `stats_line_ab_length`,
  `route_result_int_via_ratio`, `drc_units_10000_per_mm`.
  - **Accept:**
    - a unit test of both behaviours for each;
    - `ses_round_trip.rs` passes for Issue676 with `ses_reader_scale` off;
    - the reference tests (`reference_load.rs`, `reference_optimize.rs`, `kicad_parity.rs`)
      still compare Java's values under `legacy`;
    - a counter on each DSN branch shows which corpus boards reach it (expected: none). Any board
      that does is measured;
    - with every fix of the batch on, every registered board's session is byte-identical, and a
      counter shows how often `route_result_int_via_ratio` changes the `== 0.0` test (expected:
      never).
  - *Partial 2026-10-18:* Code and both-behavior tests complete. DSN units now
    honor PCB coordinate units and placement units (precision is not a coordinate
    multiplier); the other seven switches' implementations are exercised by
    `via_alias_switch_keeps_first_class_vias_separate`,
    `class_class_switch_applies_all_three_pairs`,
    `bounding_box_switch_both_behaviours`,
    `pcb_unit_and_placement_resolution_switch_use_physical_units`,
    `issue676_round_trips_with_reader_scale_fixed`,
    `trace_length_statistics_both_behaviours`,
    `route_result_via_ratio_both_behaviours`, and
    `drc_units_switch_uses_the_actual_board_scale` (all passed).
    `run_stats::tests::n06_via_ratio_counter_observes_the_actual_zero_test`
    pins a positive zero-test difference; the isolated `n06_reachability` test
    observes all four DSN counters in both arms. The actual optimizer reader
    counts adopted winners while increased ripup costs are active.
    Release gate: 418 tests passed across the engine lib, `board_snapshot`,
    `interactive_editor`, `interactive_route`, `kicad_parity`, `n06_inputs`,
    `n06_reachability`, `reference_load`, `reference_shove_insert`, `rules_reader`,
    `ses_round_trip`, and `settings_java_quirks`; 5 intentional ignores.
    `reference_optimize::candidate_order_matches_java` also passed. Full
    `reference_optimize` reroutes candidates/boards, so it is reserved for the
    orchestrator. Clippy for engine/CLI all targets, formatting and checklist
    validation passed. Defaults remain Java in both profiles.
    Registration: `prereg/N06.10.json` and `tools/bench/sets/n06-i.txt`, baseline,
    each fix alone and all fixes; includes plugin canary export/import procedure.
    heavy run pending: `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.10.json --out target/orchestrator/n06-i`.
    Also run the corpus counter inventory (reads only):
    `N06_INPUT_COUNTERS_OUT=target/orchestrator/n06-i-corpus.json CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo test --release -p freerouting --test n06_reachability dsn_corpus_branch_inventory -- --ignored --test-threads=1`,
    then the optimizer reference gate:
    `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo test --release -p freerouting --test reference_optimize`.
    Check per-board DSN reachability, byte-identical sessions and zero
    `route_result_zero_test_changes`; complete the separate plugin canary run
    and grade before accepting or promoting. Any reached DSN fix needs N02 totals.
  *Partial 2026-10-18 (registered runs):* the orchestrator ran `prereg/N06.10.json` at 298af02
    (release CLI in `target/orchestrator/n06-run`, logs in `target/orchestrator/heavy/n06-*.log`):
    - `n06_run.py`: 40 runs (4 boards × 10 arms), `session_differences` 0 in every run, so every
      arm's SES equals the baseline's; `route_result_zero_test_changes` 0 and the four
      `dsn_*_reached` counters 0 in every run.
    - `dsn_corpus_branch_inventory`: 205 DSN files, 203 load, and no file reaches any of the four
      DSN branches, so no N02 totals measurement is owed. The registered command's relative
      `N06_INPUT_COUNTERS_OUT` fails, because cargo runs the test in `crates/freerouting`; it ran
      with an absolute path.
    - `reference_optimize`: 3 passed (`candidate_order_matches_java`,
      `optimizer_runs_match_java`, `candidate_items_match_java`), 160 s.
    Small parity checks on 7e70541: `connection-parity` 7/7 and `PARITY_MAX_JAVA_S=20` 57/57 exact
    boards byte-identical, D8 0. Remaining: the canary-pwr plugin-path grade.
  *Done 2026-10-18:* every **Accept:** clause is met: the switch tests above, the runs above
    and the plugin grade.
    Plugin path: canary-pwr at canary 9511d9a (`git archive`, source unedited), exported with
    `headless_run.py export` and `canary-graded-flow.json` (KiCad 10.0.7), routed once per arm
    with the exported args plus `--router.profile=legacy` and the arm's flags
    (`target/orchestrator/n06-plugin/route_arms.py`): all 18 N06.10 and N06.11 arms give one SES
    (sha256 978a5e39…), every reachability counter 0.
    The canary-pwr plugin-path grade (canary-7a, canary `grade.sh` with `GRADE_REV=9511d9a`,
    KiCad 10.0.5; canary 34c43d3) of that one session: 38 unconnected, 4 clearance errors
    (+12V_BUS vias at 0.2 mm instead of 0.25), 0 via-in-pad, 0 below-minimum, 2 inner tracks
    (+12V_BUS on Power.Cu, 12.94 mm), 22 dangling vias and 1 dangling track. These are the
    legacy profile's errors (no `power_layer_traces=none`, no `via_clearance=net_class`), and
    every arm has them identically, so no arm adds a real DRC, via-in-pad or below-minimum error.
- [x] **N06.11 — Batch U, GUI and interactive:** `snapshot_drops_compensation` (the snapshot
  stores the compensation flag), `rules_matrix_trees_not_rebuilt`, `move_drill_tidy_region`,
  `route_change_layer_no_via`, `route_connect_inner_break`, `select_extension_ignores_filter`.
  The GUI's profile selector (N01) reaches them.
  - **Accept:**
    - a test of both behaviours for each (kittest where it is a GUI flow);
    - a counter shows that a batch run over the registered set never restores a snapshot with
      compensation on. The other five are interactive code that batch runs never call.
  - *Partial 2026-10-18:* All six switches now have both-behavior tests:
    `snapshot_compensation_switch_including_memo_restores`,
    `rules_matrix_refresh_switch_updates_compensated_shapes`,
    `move_drill_item_finite_tidy_skips_pull_tight`,
    `layer_change_without_spanning_via_quirk`,
    `failed_first_corner_pair_switch_stops_disconnected_second_leg`, and
    `extension_filter_switch_exits_empty_inspection` passed. The isolated
    `n06_reachability` test pins compensated-restore counting including memo hits
    and explicit rebuilds (10 requests across both arms). Snapshot, selection,
    rules, drill and layer tests ran in release; the private connection test ran
    with `cargo test -p freerouting --lib failed_first_corner_pair_switch`.
    Registration: `prereg/N06.11.json`, `tools/bench/sets/n06-u.txt`, baseline,
    each fix alone and all fixes; defaults remain Java in both profiles.
    heavy run pending: `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.11.json --out target/orchestrator/n06-u`.
    Check every run's `compensated_snapshot_restores` for zero and complete the
    separate canary-pwr plugin grade before accepting or promoting. The final
    release gate above also covers every U regression and legacy interactive
    comparison; no new UI code is involved (the GUI delegates to these engine APIs).
    The heavy run is queued with the orchestrator; an accepted switch joins `enhanced` through
    N05 (see N06.13).
  *Partial 2026-10-18 (registered runs):* the orchestrator ran `prereg/N06.11.json` at 298af02:
    32 runs (4 boards × 8 arms), `session_differences` 0 and `compensated_snapshot_restores` 0 in
    every run. Remaining: the canary-pwr plugin-path grade.
  *Done 2026-10-18:* every **Accept:** clause is met: the switch tests above, the 32 runs
    above, and the shared N06.10 plugin-path run and grade (one SES for all 18 arms; no arm adds
    a real DRC, via-in-pad or below-minimum error).
- [x] **N06.12 — The (c) rows.** Each one ends measured or explained, with the reason written in
  its row.
  - `ripup_pass_no_unset` gets its own registration.
  - The fix of `opt_threshold_fraction`'s CLI double scaling counts as a port fix, not a quirk.
  - **Accept:** every (c) row has its outcome.
  - *Done 2026-10-18:* every (c) row has its outcome, measured (registered, run pending) or
    explained, with the reason in its row of the (c) table (worktree
    `target/orchestrator/n06c/wt`, branch `land/n06-12`, on main `bbd1336`). Defaults stay
    `true` (Java) in both profiles; `ENHANCED` is unchanged. The decisions are logged in
    `target/orchestrator/operator-decisions.md` ("2026-10-18 N06.12").
    - Measured (2): `ripup_pass_no_unset` (verdict pair, accept improve) and
      `any_angle_skip_length_squared` (reported arm, no-regress), registered before any number
      in `docs/roadmap/prereg/N06.12-ripup-pass.json` (sha256
      `b39a51fe095788593e2bc4c27ebc4b7d67f9b35b235a101fa63d03c1fc76de23`) with
      `tools/bench/sets/n06-ripup-pass.txt` (5 boards that run past pass 4, 3 any-angle variant
      boards, plus canary-pwr through the plugin path), pinned by
      `crates/freerouting-dev/tests/prereg_n06_12.rs` (4 tests pass).
      - Both behaviours: `tests/n06_judgement.rs::ripup_pass_no_switch_both_behaviours_and_counters`
        (Green14SegLED, pass 1 then passes numbered 4 and 6: both arms count pass 4's connections,
        only `false` draws, nobody draws in pass 6, the sessions differ, `false` repeats itself
        exactly) and `the_random_factor_fires_from_pass_4_outside_every_third_pass`;
        `optimize::tighten::any_angle::tests::skip_length_switch_both_behaviours_and_counter`.
      - Counters (`run_stats`, `--stats json` `total.counts`): `ripup_random_pass_connections`,
        `ripup_random_factor_draws`, `any_angle_skip_length_flips`, `rules_unknown_layer_scopes`.
      - Heavy run pending (orchestrator, from the worktree root, release build of this tree):
        `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.12-ripup-pass.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-c`
        (24 runs; exits non-zero after the last run when a session differs, as it should for the
        ripup arm), then the canary-pwr plugin row as in N06.10 (the registration's
        `plugin_canary`: export once, route the 3 arms with the exported args plus
        `--router.profile=legacy` and the arm's two flags, grade each distinct session with
        canary's `grade.sh`). N06.13 records the verdict.
    - `in_front_of_pad_side0_flank`: follows N06.7's registered run (arm `all_flank`).
    - `opt_threshold_fraction`: heuristic left as Java; the CLI double scaling fixed as a port
      fix in both profiles (`a_cli_threshold_below_a_thousandth_is_scaled_once` in
      `tests/settings_path.rs`, `optimize::batch::tests::a_cli_threshold_is_scaled_once`;
      DIVERGENCES "Settings"); (a)-like, no A14 board passes such a value.
    - `rules_unknown_layer_scope`: decided (ignore under `enhanced`, Java under `legacy`), wired
      in `rules_reader::apply_rules`, tested by
      `tests/rules_reader.rs::unknown_layer_scope_switch_both_behaviours`; promotion is N06.13's.
    - Left as Java with the reason in the row (switch inert, its doc line says so):
      `insert_stale_trace_object`, `rules_autoroute_settings_reenable` (C04),
      `direction_compare_zero`, `route_stubs_layer_test`, `polyline_array_reversal`,
      `avoid_acid_traps_disabled` (a Q candidate).
    - DIVERGENCES: "Java quirk switches, opt-in (N06)" gains an N06.12 block; "Settings" the CLI
      line; "Rules files" names the switch.
    - Gate (private target, `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`): `cargo fmt --all --check`;
      `cargo clippy -p freerouting -p freerouting-dev -p freerouting-cli --all-targets -- -D warnings`;
      `cargo test -p freerouting -p freerouting-cli --no-fail-fast` 1574 passed, 0 failed, 33
      ignored (155 suites); the N06 pin tests `prereg_n06_{4,5,6,7,8,9,12}` 28 passed;
      `python3 tools/roadmap/checklists.py status --strict` clean.
- [ ] **N06.13 — Results and promotion.**
  - Each batch's registration and numbers go in its item.
  - Each accepted switch is turned off in `enhanced` through N05, with its DIVERGENCES line under
    "Profile `enhanced`".
  - A rejected switch stays `true` in both profiles, with its numbers. It is never removed,
    because `legacy` needs it.
  - The DIVERGENCES "kept on purpose" paragraphs point to this file.
  - **Accept:** every (b) row ends accepted, rejected with numbers, or accepted on tests under
    the decisions' rule.

- [~] **N06.14 — Simplex cutouts outside N06.4's domain:** `cutout_hole_crosses_border`,
  `simplex_octagon_half_corner` (the two (b) rows added above, from N06.4's note and
  `target/orchestrator/operator-decisions.md`, "2026-10-18 N06.4" and "N06.12 follow-ups").
  - Switches in `settings/java_quirks.rs` and the geometry `SplitQuirks`, `true` = Java; fixes
    behind them in `Simplex::cutout_from`, as N06.4 did.
  - **Accept:** the invariant holds with all four cutout switches `false` on the cases
    `java_breaks_the_invariant_outside_the_two_rows` pins and on an extended seeded set with holes
    crossing the border; the engine's reach is counted; registered before any number, and
    measured under the decisions' accept rule.
  - *Partial 2026-10-18:* code, tests, counters and registration done (worktree
    `target/orchestrator/n06g2/wt`, branch `land/n06-14`, on main `3b1bde6`, whose code is
    `81afaba`'s, the engine the registration names); heavy run pending.
    - [x] Switches: `router.java_quirks.cutout_hole_crosses_border` and
      `simplex_octagon_half_corner` (`settings/fields.rs`, after N06.4's two; `JavaQuirks::IDS`
      now 44, `fix_item` = N06.14), carried into `SplitQuirks` by `JavaQuirks::split`.
      `SplitQuirks` has 4 fields, `COMBINATIONS` = 16 and `SplitQuirks::all()`, so the area's
      piece cache keeps one slot per value (`default_is_java_and_cache_slots_are_distinct`).
      Unset is `true` in both profiles (`both_profiles_keep_every_java_bug`,
      `every_switch_is_a_settings_key_in_inventory_order`,
      `each_key_round_trips_through_a_settings_file`, all in `tests/settings_java_quirks.rs`).
    - [x] `cutout_hole_crosses_border=false`: a corner `calc_division_lines` does not divide gets
      a stand-in line that neither clips as the first division line nor turns the clipping on
      (`CrossFirstLine` in `simplex.rs`, both arms followed side by side).
    - [x] `simplex_octagon_half_corner=false`: an inexact corner on no outer line (exact test,
      `side_of_intersection`) is divided at its exact rational point. Its division lines come
      from `Line::nearest_through_integer_points` (exact, `BigInt`): the piece on each side
      takes the nearest line through integer points on its own side, so the two overlap by a
      strip at most `1/|dir|` wide instead of leaving a gap. A single rounded-point line left
      gaps of up to 8 units² on 21 enlarged cases. `nearest_lines_through_integer_points_bracket_the_point`
      checks the bracketing, adjacency and anchors exactly, at `CRIT_INT` with directions up to
      `2 * CRIT_INT`.
    - [x] Invariant (`crates/freerouting-geometry/tests/cutout_invariant.rs`, 9 tests, pass):
      - `java_breaks_the_invariant_outside_the_two_rows`: both pinned cases under all 16
        switch values; the gap goes exactly when `cutout_hole_crosses_border` is `false`, the
        overlap exactly when `simplex_octagon_half_corner` is `false`.
      - `all_fixed_cutouts_tile_the_rest_on_the_extended_set`: N06.4's 5400 cases plus 5400
        holes centred on the outer border and 5400 enlarged holes (half crossing):
        0 of 16,200 cutouts broken with every switch `false`; Java breaks 1,817
        (`java_breaks_the_extended_set_through_each_switch`: each N06.14 switch alone removes
        every failure of its kind). A one-off run at 4000 cases per pair: 0 of 107,997.
      - `the_n06_14_switches_change_nothing_in_the_domain`: inside N06.4's domain the two
        switches change no piece and count nothing; N06.4's 5 tests still pass.
      - `the_n06_14_counters_count_the_changed_cutouts`: every cutout a switch changes is
        counted (631 of 697 counted change for the border switch, 1554 of 1554 for the
        half-corner switch).
    - [x] Reach, counted as N06.4 did: run statistics `cutout_hole_crosses_border_reached` and
      `simplex_octagon_half_corner_reached` (`run_stats.rs`, `record_split_reach`), at the
      drill-page split and the tie-pin cutout. The fixes sit in `Simplex::cutout_from`, which
      a drill page reaches only with simplex obstacles: the 45 and 90 degree search trees
      keep octagons and boxes, the plain (any-angle) tree keeps simplexes.
      `tests/n06_cutout_reach.rs::drill_page_splits_report_both_counters_in_both_arms`: pass 1
      of `Issue269-min_fr_test` reaches neither (all arms); pass 1 of
      `Issue143-rpi_splitter-any` reaches them 6 and 578 times under Java (0 and 659 with both
      fixes). While choosing that board, pass 1 of `Issue026-J2_reference-any` gave 14 and 512,
      `scoring-mixed-layer-any` 0 and 0, and the 45 degree fixtures Issue413-test,
      Issue433-my-board, TestSensel, Issue143-rpi_splitter, Issue954-polygon64 and
      Issue026-J2_reference 0 and 0. So the switches are reachable, in any-angle mode only, and
      are measured, not accepted on tests. `unwired_area_splits_corpus_inventory` reports the
      two new columns for the search trees' area splits (Java's code in every arm).
    - [x] Registered before any routed number: `docs/roadmap/prereg/N06.14.json` (sha256
      `1b945407a39a72fd9c27f061c73f842b28e6cb1529dfc5408173721f38680c99`) and
      `tools/bench/sets/n06-g2.txt`. Arms `baseline`, `cutout_hole_crosses_border`,
      `simplex_octagon_half_corner`, `all`, each setting both keys; default profile; totals rule,
      no-regress. Rows: A07's three any-angle variants plus `Issue026-J2_reference` and
      canary-pwr, the 45 degree identity checks (every arm's sessions must equal the
      baseline's); canary-pwr also through the plugin path. Pinned by
      `crates/freerouting-dev/tests/prereg_n06_14.rs` (5 tests, pass).
    - [x] Defaults unchanged: both keys unset are `true` in both profiles; Java's path is the
      `true` branch at both `PARITY(quirk, java_quirks.<id>)` markers. DIVERGENCES: "N06.14
      simplex cutouts outside N06.4's domain" under "Java quirk switches, opt-in (N06)".
    - [ ] Heavy run pending (orchestrator, one job at a time):
      `cd /home/thebo/code/freerouting-rs && CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.14.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-g2`.
      - `n06_run.py` exits non-zero when sessions differ, expected on the any-angle rows; it
        writes `summary.json` with every run's counters first.
      - Then grade each SES with `gate collect` and `freerouting-dev widths`, and judge the
        pairs with `freerouting-dev gate --rule totals`; then the plugin canary run and grade,
        per the registration's `plugin_canary` block.
      - Also the corpus inventory (read-only):
        `cd /home/thebo/code/freerouting-rs && N06_CUTOUT_COUNTERS_OUT=/home/thebo/code/freerouting-rs/target/orchestrator/n06-g2-corpus.json CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo test --release -p freerouting --test n06_cutout_reach unwired_area_splits_corpus_inventory -- --ignored --test-threads=1`.

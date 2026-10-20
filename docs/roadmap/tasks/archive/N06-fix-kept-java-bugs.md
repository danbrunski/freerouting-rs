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
- **1 more of kind (b),** added 2026-10-18 by N06.15: the shove bug N03.2 found
  (`shove_via_into_pending_piece`).
- **1 more of kind (b),** added 2026-10-19 by N06.16: the any-angle room side indexes whose
  Java path runs out of memory (`any_angle_room_side_index`).

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
| Profiles: `router.profile`, with `legacy` = Java and `enhanced` the default | `crates/freerouting/src/settings/profile.rs`; `ENHANCED` was empty when N06 opened; N06.13 added the 30 accepted switches |
| The rule that each `enhanced` difference has a DIVERGENCES line | `tests/settings_profile.rs::every_enhanced_difference_is_a_setting_with_its_divergence_line` |
| Settings fields, paths and the settings file | `settings/fields.rs` (`RouterSettings`), `settings/path.rs` |
| What the switches ride on | `BoardRouterSettings` (`settings/board.rs`), `AutorouteControl` (`autoroute/control.rs`), `BasicBoard` (`board/basic_board.rs`), `ReadContext` (`io/specctra/parser/mod.rs`), geometry `split_to_convex` (`freerouting-geometry/src/shapes/`) |
| A test pinning each bug's Java behaviour | the "Test pinning Java today" columns below |
| The A14 exact set (106 boards, byte-identical to Java) | `tests/autoroute_board_parity.rs`; the command is in [A14](A14-board-parity-gate.md)'s done note (about 1.5 h, release) |
| Measurement: preregistration, board sets, baselines | `docs/roadmap/routing-quality.md` "How to measure a quality change"; `docs/roadmap/prereg/*.json`; `tools/bench/sets/*.txt`; Q14 is a worked example ([Q14](Q14-via-clearance-follows-net-class.md), `prereg/Q14.json`, `sets/q14-via-clearance.txt`) |

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
| N06.14 | `geometry/shapes/simplex.rs`, `geometry/line.rs` (`nearest_through_integer_points`), `geometry/quirks.rs`, `geometry/tests/cutout_invariant.rs`, its two keys in `settings/fields.rs` and `settings/java_quirks.rs`, its two counters in `run_stats.rs`, `tests/n06_cutout_reach.rs`, `tests/n06_cutout_reach_45.rs`, `tools/bench/sets/n06-g2.txt`, `tools/bench/sets/n06-g2b.txt`, `docs/roadmap/prereg/N06.14.json`, `docs/roadmap/prereg/N06.14b.json`, `freerouting-dev/tests/prereg_n06_14b.rs` |
| N06.12, N06.13 | their rows' files; `settings/profile.rs` (`ENHANCED`) and the DIVERGENCES "Profile `enhanced`" section, through N05 |
| N06.16 | `autoroute/session/neighbours_any.rs`, the key (`settings/fields.rs`, `settings/java_quirks.rs`, `tests/settings_java_quirks.rs`), one `run_stats` counter, `tests/n06_any_angle_rooms.rs` |
| N06.15 | `shove/insert.rs` and its `insert_pending_tests.rs`, the unmetered clearance query (`board/search/mod.rs`, `board/basic_board.rs`), the key (`settings/fields.rs`, `settings/java_quirks.rs`, `tests/settings_java_quirks.rs`), two `run_stats` counters, `tests/reference_shove_insert.rs`, `tools/bench/n06_run.py` (an arm's own `baseline`), `prereg/N06.15.json`, `sets/n06-pending-piece.txt`, `freerouting-dev/tests/prereg_n06_15.rs` |

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
| `cutout_hole_crosses_border` | geometry/shapes/simplex.rs:586 (`CrossFirstLine`, :219) | Simplex.java:731-732, 756-768, 809-824 | A corner of the cut-out part on the outer border is not divided; its stand-in division line (the hole edge before it) still clips later pieces as the first division line, and turns that clipping on as a corner's last line, so pieces of a hole crossing the border vanish (half the rest in the pinned case). Fix: only divided corners' lines take part | batch: drill pages and the tie-pin cutout holding a simplex tree shape, so any-angle mode and 45 degree boards with a trace off 45 degrees (corrected 2026-10-20: canary-pwr's fixed wiring; the 90 degree tree keeps boxes only); every `Area` split since 2026-10-18 (stored areas: tomu-fpga8 and -fpga9) | G2 (N06.14) | `java_breaks_the_invariant_outside_the_two_rows` |
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
| `shove_via_into_pending_piece` (added by N06.15, found by N03.2) | shove/insert.rs:259-274 (`insert_substitute_pieces`), 312-381 (`checks_pending_piece`, `foreign_via_in_pending_piece`) | optimize/TraceShover.java:544-569, actions/ForcedPadRouter.java:413-438, actions/DrillItemMover.java:173-248 (`shoveVias`) | A substitute piece goes on the board only after the recursive inserts of its own shapes; a deeper level's `shoveVias` may move a via of another net onto the piece meanwhile, and its move check (`check`, `forcedPad`) cannot see the piece, so the piece is inserted over the via: a short. **Question:** fail the shove there? **Proposal:** before the piece is inserted, query its shapes again with its clearance and fail the shove at the first via of another net (the prototype of N03.2). **Remaining case:** a trace of another net inserted onto the pending piece later in the same recursion; a clearance re-query also finds the pieces placed against it at exactly the clearance (1,076 pieces on bm01 in three `legacy` passes, final board 0 violations), so it is only counted, as copper overlap (`shove_pending_piece_foreign_traces`) | batch, rare: GP8B `legacy` tree pass 3 once, canonical twice, `enhanced` tree pass 1 once (N03.2); 0 on the 15 A14 boards and the probe stages N06.15 tried | N06.15 | none (the bug has no Java recording; `pending_piece_switch_both_behaviours_and_counters` pins both halves) |
| `any_angle_room_side_index` (added by N06.16) | autoroute/session/neighbours_any.rs:285 (`sorted_neighbours_any`), 623 (`side_simplex`), 507 (`gap_rooms_any`) | expansion/SortedRoomNeighbours.java:189, 253, 288, 296 (sides on `roomShape`), 419, 511 (`toSimplex()`) | An any-angle neighbour's touching side indexes the room's own shape (an `IntOctagon`: 8 border lines, degenerate ones too), the edge removal and the gap walk its simplex (redundant lines removed). A side past the simplex's last line is never reached by `prevNo`, so the gap walk adds incomplete rooms until memory runs out. Fix: take the sides, corners and the sort on the simplex | batch, any-angle mode only: rooms of the plain tree with an octagon shape (3 calculations in a whole `Issue026-J2_reference-any` route, 10 in `Issue143-rpi_splitter-any`) | N06.16 | `room_side_index_switch_both_behaviours` (Java's path is unbounded on the two boards, so no recording) |

Batches: **G** geometry cutouts (**G2**: N06.14's two rows), **M** maze search and rooms, **F** fanout, **S** shove, **P** pull-tight,
**O** batch optimizer, **I** inputs and reports, **U** GUI and interactive. The last row was found after the
inventory (N03.2) and has its own item, N06.15.

### (c) Judgement calls: N06.12

| id | Rust | Java | The question | Reach | Proposal |
|---|---|---|---|---|---|
| `ripup_pass_no_unset` | autoroute/session/ripup.rs:12, 337; control.rs:138 | MazeRipupResolver.java:158-162, AutorouteControl.java:192 | `ripupPassNo` is never set, so the random detour factor `0.5 + r²` never fires; the batch router never had it (the 1.x import set it only in `ExpandTestState`) | batch, every rip-up from pass 4 on if enabled | Its own registration: switching on randomness Java never ran, deterministic by its reseed. Likely large either way. **Outcome (N06.12, 2026-10-18): measured; rejected by the run of 2026-10-19.** Wired (`batch::connection::router_control` sets the pass number under `false`), tested (`tests/n06_judgement.rs`), counted (`ripup_random_pass_connections`, `ripup_random_factor_draws`) and registered as the verdict pair of `prereg/N06.12-ripup-pass.json` (accept: improve, since it adds behaviour Java never ran). The run: total 47 → 47 and 1 new class-width breach (ReadPlaceScope), `gate: FAIL`, so it stays `true` in both profiles (N06.12's note) |
| `insert_stale_trace_object` | autoroute/path/insert.rs:100 | FoundConnectionInserter.java:108-137 | `connectToTrace` uses the start or target trace object after the insertion split or removed it | batch, occasional | Leave: the split keeps the old geometry as the new pieces, so the stub lands on the same copper. **Outcome (N06.12): left as Java.** The stale object only supplies the geometry `connectToTrace` connects to (`insert.rs`, `trace_object`); a split keeps that geometry as its pieces, so the stub lands on the same copper and a fix that reads the pieces instead changes nothing on a split. The switch stays inert (`false` changes nothing) |
| `in_front_of_pad_side0_flank` | shove/drill_check.rs:461 | ForcedPadRouter.java:86 | Side 0's flank clause takes `min`, where cases 2, 4 and 6 take `max`; inferred from symmetry | as `in_front_of_pad_typo` | An arm of batch S, only together with the typo fix. **Outcome (N06.12): follows N06.7's run.** Wired and tested by N06.7 (`side0_flank_switch_both_behaviours`, counter `shove_in_front_of_pad_flank_flips`) and registered as `prereg/N06.7.json`'s reported arm `all_flank`, judged against `all` only if batch S is accepted; otherwise it stays `true`. N06.7's done note records the result |
| `opt_threshold_fraction` | optimize/batch/mod.rs:197-212, settings/path.rs:357-364 | BatchOptimizer.java:367-377 | A threshold in (0, 0.1) is multiplied by 100 (Java logs it as a heuristic); a CLI value below 0.001 is scaled twice | only JSON or environment values in (0, 0.1) | Leave Java's heuristic; fix the double scaling of the CLI. **Outcome (N06.12): heuristic left as Java (`true` in both profiles, `false` tested by `threshold_fraction_switch_both_behaviours`); the double scaling fixed as a port fix in both profiles** (`SettingsPatch::improvement_threshold_scaled`; `a_cli_threshold_below_a_thousandth_is_scaled_once`, `a_cli_threshold_is_scaled_once`; DIVERGENCES "Settings"). (a)-like: no A14 board, Java reference case (`-oit 0.035`, `4.0`, `0.05`) or plugin option (1-100 %) passes a CLI value below 0.001 |
| `rules_unknown_layer_scope` | io/specctra/rules_reader.rs:21-29 | RulesReader.java | A layer scope naming an unknown layer applies to all layers | in, `-dr` only | Decide: ignore the scope with a warning, or keep it. **Outcome (N06.12, decided 2026-10-18): ignore it with a warning under `enhanced`, keep Java's under `legacy`.** Wired in `rules_reader::apply_rules` (`false` logs a second warning and skips the scope), tested (`unknown_layer_scope_switch_both_behaviours`), counted (`rules_unknown_layer_scopes`). Defaults stay `true`; N06.13 adds it to `ENHANCED` through N05. A wrong layer name in a rules file is an input error, and spreading its widths and clearances to every layer is the one reading that is surely wrong |
| `rules_autoroute_settings_reenable` | io/specctra/rules_reader.rs:21-29 | RulesReader.java | A rules file's `autoroute_settings` re-enables every layer and the optimizer | in, `-dr` only | Decide with C04: a settings semantics choice. **Outcome (N06.12): left as Java.** C04's decisions keep it on purpose: a rules file's `autoroute_settings` are the file's whole routing setup, laid over the merged settings and the CLI (C04.3, `rules_autoroute_settings_override_cli`), and `AutorouteSettings.readScope` seeds every layer and the optimizer on. C04 makes no other settings-semantics choice, so the switch stays inert (`false` changes nothing) |
| `direction_compare_zero` | geometry/direction.rs:346 | Direction.java | `compare_to` handles the zero direction lopsidedly | batch, edge case | Leave: a quirk, not clearly wrong. **Outcome (N06.12): left as Java.** An ordering of the zero direction is a convention, not a bug; changing it would reorder sorted directions with no correct order to aim for. The switch stays inert (the geometry crate has no settings access) |
| `route_stubs_layer_test` | drc/route_stubs.rs:9-12, 136 | WindowRouteStubs.java:73-74 | Both via bounds are compared with the contact's first layer, so the stub layer is always the last | out (route-stubs list) | Leave until the intent is clear. **Outcome (N06.12): left as Java.** It changes only the route-stubs report's layer, not routing; which layer the report should name for a via stub is not clear from Java, so there is no fix to test. The switch stays inert |
| `any_angle_skip_length_squared` | optimize/tighten/any_angle.rs:43-45 | TraceTightenerAnyAngle.java:22, 774, 896 | `SKIP_LENGTH = 10` is compared with a squared distance (effectively 3.2 units) | batch, any-angle mode only | Measure in any-angle mode, or leave. **Outcome (N06.12): measured; accepted on its test by the run of 2026-10-19.** Cheap to include: a reported arm of `prereg/N06.12-ripup-pass.json` over A07's any-angle variant boards, counted (`any_angle_skip_length_flips`) and tested (`any_angle::tests::skip_length_switch_both_behaviours_and_counter`); judged no-regress (a typo-like fix). Two of the three any-angle rows were dropped because the engine can't route them in bounded memory (amendments 2 and 3). The counter is 0 and every session is identical, so it is accepted on its test for N06.13 |
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
  every light run and in every run of N06.8's registered run (24 DSN runs, 4 plugin arms), which
  confirms it unreachable on those boards (N06.8's done note).

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
- [x] **N06.3 — The quirk switches.** Add `router.java_quirks.<id>` for every (b) and (c) row
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
    - [x] Read sites, audited 2026-10-18 at 45 switches (main `94d0620`, which has N06.15).
      Every switch reaches its site with Java's path under `true`; 38 are wired
      (`false` changes the behaviour) and 7 are inert on purpose (their doc line in
      `settings/fields.rs` says so). Of the 16 deferred sites, the batch items wired 9: N06.4
      `octagon_box_cutout_branch3`, N06.7 the flank, N06.10 `dsn_unit_ignored`, N06.11 the five
      U rows, N06.12 `rules_unknown_layer_scope`. The other 7 are inert:
      `simplex_prev_division_line` (N06.4) and the six (c) rows N06.12 left as Java. N06.14 (2)
      and N06.15 (1) added and wired three more switches. The five inert switches that had no
      marker now carry one at Java's site (comment only). Sites are `file:line` in
      `crates/freerouting/src` (geometry: `crates/freerouting-geometry/src`); tests are the
      both-behaviours tests.

      | id | site | state | test |
      |---|---|---|---|
      | `simplex_prev_division_line` | geometry `shapes/simplex.rs:593`, through `JavaQuirks::split` at `autoroute/session/drills.rs:578` and `board/basic_board.rs:2585` | inert by construction (N06.4: the fix never passes a merge test) | `the_simplex_switch_changes_no_piece` (5400 cases, same pieces under both values) |
      | `octagon_box_cutout_branch3` | geometry `shapes/int_octagon.rs:762`, same callers | wired (drill pages, tie-pin cutout) | `box_cutout_branch3_switch_both_behaviours` |
      | `cutout_hole_crosses_border` | geometry `shapes/simplex.rs:586`, same callers | wired | `java_breaks_the_invariant_outside_the_two_rows`, `java_breaks_the_extended_set_through_each_switch` |
      | `simplex_octagon_half_corner` | geometry `shapes/simplex.rs:737`, same callers | wired | the same two |
      | `init_failure_room_leak` | `autoroute/connection.rs:431`, `:919`, `autoroute/session/rooms.rs:263` | wired | `n06_maze.rs::init_failure_switch_both_behaviours_and_counter` |
      | `failed_attempt_ripped_carryover` | `autoroute/connection.rs:234`, `:249`, `:375` | wired | `connection::tests::rejected_attempt_ripped_set_switch_both_behaviours` |
      | `ortho_zero_width_room` | `autoroute/session/neighbours90.rs:209`, `:454` | wired | `neighbours90::tests::right_of_obstacle_room_switch_both_behaviours` |
      | `edge_corner_not_advanced` | `autoroute/session/incomplete45.rs:29` | wired | `incomplete45::tests::edge_room_sides_switch_both_behaviours` |
      | `connect_to_trace_swapped_width` | `autoroute/path/insert.rs:143` | wired | `n06_maze.rs::connect_to_trace_width_switch_both_behaviours_and_counter` |
      | `fanout_stale_drill_carry` | `autoroute/fanout_pin.rs:168`, `:323`, `autoroute/connection.rs:419`, `autoroute/session/drills.rs:523` | wired | `n06_fanout.rs::stale_drill_carry_switch_both_behaviours_and_counters` |
      | `drill_destination_room_index` | `autoroute/path/backtrack.rs:111` | wired | `n06_fanout.rs::drill_destination_room_index_switch_both_behaviours_and_counter` |
      | `fanout_pin_order_names` | `autoroute/batch/fanout_rules.rs:68` | wired | `settings_java_quirks.rs::fanout_pin_order_names_both_behaviours`, `n06_fanout.rs::pin_order_names_switch_reaches_the_fanout_run_and_counter` |
      | `in_front_of_pad_typo` | `shove/drill_check.rs:533` | wired | `shove::drill_check::tests::in_front_of_pad_keeps_java_typo` |
      | `store_trace_class_self_compare` | `shove/trace_entries.rs:340` | wired | `n06_shove.rs::store_trace_class_switch_both_behaviours` |
      | `rational_perp_projection` | `shove/trace_entries.rs:399`, geometry `point.rs:533` | wired | `shove::trace_entries::tests::rational_end_corner_projection_switch_both_behaviours`, geometry `rational_perpendicular_projection_switch_both_behaviours` |
      | `smooth_loses_trace` | `optimize/pull_tight.rs:301` | wired | `n06_pull_tight.rs::smooth_loses_trace_switch_both_behaviours` |
      | `pull_tight_reused_lines_change` | `optimize/pull_tight.rs:269` | wired | `n06_pull_tight.rs::pull_tight_reused_lines_switch_both_behaviours` |
      | `opt_first_pass_stops` | `optimize/batch/mod.rs:431` | wired | `optimize::batch::tests::pass_end_decision_both_behaviours`, `batch_optimizer.rs::first_pass_switch_both_behaviours` |
      | `opt_equal_key_skip` | `optimize/batch/candidates.rs:163` | wired | `batch_optimizer.rs::equal_key_switch_keeps_every_candidate` (`true` half `equal_key_skips_rest_of_pass`) |
      | `dsn_via_alias` | `io/specctra/parser/network.rs:141` | wired | `n06_inputs.rs::via_alias_switch_keeps_first_class_vias_separate` |
      | `dsn_class_class_pairs` | `io/specctra/parser/network.rs:752` | wired | `n06_inputs.rs::class_class_switch_applies_all_three_pairs` |
      | `dsn_shape_bbox` | `io/specctra/parser/shape.rs:170` | wired | `parser::shape::tests::bounding_box_switch_both_behaviours` |
      | `dsn_unit_ignored` | `io/specctra/dsn_reader.rs:173` | wired | `n06_inputs.rs::pcb_unit_and_placement_resolution_switch_use_physical_units` |
      | `ses_reader_scale` | `io/specctra/ses_reader.rs:48` | wired | `ses_round_trip.rs::issue676_round_trips_with_reader_scale_fixed` |
      | `stats_line_ab_length` | `scoring/board_statistics.rs:576` | wired | `settings_java_quirks.rs::trace_length_statistics_both_behaviours` |
      | `route_result_int_via_ratio` | `autoroute/item_route_result.rs:210` | wired | `settings_java_quirks.rs::route_result_via_ratio_both_behaviours` |
      | `drc_units_10000_per_mm` | `drc/checker.rs:285` | wired | `drc::checker::tests::drc_units_switch_uses_the_actual_board_scale` |
      | `snapshot_drops_compensation` | `board/basic_board.rs:325` | wired | `board_snapshot.rs::snapshot_compensation_switch_including_memo_restores` |
      | `rules_matrix_trees_not_rebuilt` | `io/specctra/rules_reader.rs:236` | wired | `rules_reader.rs::rules_matrix_refresh_switch_updates_compensated_shapes` |
      | `move_drill_tidy_region` | `shove/forced.rs:518` | wired | `reference_shove_insert.rs::move_drill_item_finite_tidy_skips_pull_tight` |
      | `route_change_layer_no_via` | `interactive/route/layer.rs:155` | wired | `interactive_route.rs::layer_change_without_spanning_via_quirk` |
      | `route_connect_inner_break` | `interactive/route.rs:605` | wired | `interactive::route::tests::failed_first_corner_pair_switch_stops_disconnected_second_leg` |
      | `select_extension_ignores_filter` | `interactive/select.rs:162` | wired | `interactive_editor.rs::extension_filter_switch_exits_empty_inspection` |
      | `shove_via_into_pending_piece` | `shove/insert.rs:259` | wired (N06.15) | `shove::insert::pending_tests::pending_piece_switch_both_behaviours_and_counters` |
      | `any_angle_room_side_index` | `autoroute/session/neighbours_any.rs:623` | wired (N06.16, added after this audit) | `neighbours_any::tests::room_side_index_switch_both_behaviours` |
      | `ripup_pass_no_unset` | `autoroute/batch/connection.rs:202`, `autoroute/session/ripup.rs:343` | wired | `n06_judgement.rs::ripup_pass_no_switch_both_behaviours_and_counters` |
      | `insert_stale_trace_object` | `autoroute/path/insert.rs:115` | inert (N06.12: the split keeps the old geometry, so the stub lands on the same copper) | none, the behaviours do not differ |
      | `in_front_of_pad_side0_flank` | `shove/drill_check.rs:542` | wired | `n06_shove.rs::side0_flank_switch_both_behaviours` |
      | `opt_threshold_fraction` | `optimize/batch/mod.rs:238` | wired | `optimize::batch::tests::threshold_fraction_switch_both_behaviours` |
      | `rules_unknown_layer_scope` | `io/specctra/rules_reader.rs:105` | wired | `rules_reader.rs::unknown_layer_scope_switch_both_behaviours` |
      | `rules_autoroute_settings_reenable` | `io/specctra/rules_reader.rs:154` (marker new) | inert (N06.12: C04's settings semantics) | none |
      | `direction_compare_zero` | geometry `direction.rs:349` (marker new) | inert (N06.12: an ordering convention; the geometry crate has no settings access) | none |
      | `route_stubs_layer_test` | `drc/route_stubs.rs:136` (marker new) | inert (N06.12: the intended stub layer is unclear) | none |
      | `any_angle_skip_length_squared` | `optimize/tighten/any_angle.rs:49` | wired | `optimize::tighten::any_angle::tests::skip_length_switch_both_behaviours_and_counter` |
      | `polyline_array_reversal` | `optimize/tighten/mod.rs:505`, `diag45.rs:333`, `any_angle.rs:78` (markers new) | inert (N06.12: later reads depend on it, no fix is clearly right) | none |
      | `avoid_acid_traps_disabled` | `optimize/tighten/mod.rs:390` (marker new) | inert (N06.12: turning it on is a feature, a Q candidate) | none |

      - Every test in the table passes:
        `cargo test -p freerouting -p freerouting-geometry --no-fail-fast -- <the 43 names plus
        the three settings tests below>`, debug, private target: 46 passed, 0 failed, over 139
        test binaries.
      - Area splits left on Java's code in every arm, on purpose. The cutout switches act
        at the drill-page split and the tie-pin cutout only. These sites still call
        `split_to_convex`: the search trees' obstacle and conduction areas
        (`board/search/mod.rs:758`), the outline's generated keepout (`:981`) and its
        `BoardOutline::keepout_piece_count` (`board/items/mod.rs:594`, `:611`), as N06.4's note
        says. The board's other `Area` splits do too (`board/queries.rs:101` `check_shape`,
        `board/basic_board.rs:1451` `overlapping_items`, `shove/spring.rs:340`,
        `interactive/construct.rs:435`, `io/specctra/parser/dsn_file.rs:46`). Wiring them under
        the same switches changes what `false` means after N06.4 accepted
        `octagon_box_cutout_branch3` on a run without them. That run had canary-pwr as a row,
        and canary-pwr is not in the corpus inventory, whose tree splits reach branch 3 on
        tomu-fpga8 (1) and tomu-fpga9 (6). It also changes the engine that N06.14's pending
        registration names. Wiring needs a re-measurement or a new switch, so it is an
        operator decision and outside N06.3's scope. Two more things block it: the reader
        inserts items before it puts the switches on the board, and `prepare_job` puts the
        final switches on afterwards, so a split value that differs would need a tree rebuild.
        `unwired_area_splits_corpus_inventory` counts the reach.
        Wired since by the follow-up decided 2026-10-18 (option (b)): see N06.4's "Wired at every
        `Area` split" bullet; the re-measurement is N06.14's amendment 1.
      - `every_switch_has_a_parity_marker_at_its_site` (new, `tests/settings_java_quirks.rs`)
        scans `crates/freerouting/src` and `crates/freerouting-geometry/src`. Every id in
        `JavaQuirks::IDS` has a marker, and every marker names a known id. Before this audit
        it would have failed on the five inert ids without a marker.
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
      At 45 switches (2026-10-18): 11 tests, all pass. The new
      `every_switch_survives_deep_copy_and_restore` sets each switch alone to `false` and checks
      it through a deep copy and the three restores. The new
      `every_switch_has_a_parity_marker_at_its_site` is described above.
    - [x] With every switch `true` routing is unchanged on the fast subset (release, private
      target): `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p
      freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
      --test-threads=1`: exact 7 boards, 7 pass, 0 fail, 7 SES byte-identical.
  - *Done 2026-10-18:* every **Accept:** clause holds at 45 switches.
    - A14 with every switch `true`: the full A14 on `7a4f73b` (above), plus the small parity
      checks of the later batches (N06.9's and N06.10's `connection-parity` 7/7 runs, N06.10's
      `PARITY_MAX_JAVA_S=20` 57/57). This audit changed comments and tests only, no `true`-path
      code, so it reran no parity check.
    - Both behaviours: the 38 wired switches have tests (table above, 46 tests pass). In the 7
      inert switches the behaviours do not differ, and `simplex_prev_division_line`'s sameness
      is pinned by `the_simplex_switch_changes_no_piece`.
    - Round trips: `each_key_round_trips_through_a_settings_file` and
      `each_key_is_set_on_the_command_line_and_in_the_environment` iterate `JavaQuirks::IDS`
      (45, `SWITCH_COUNT`).
    - `deep_copy` and restore: `every_switch_survives_deep_copy_and_restore`, plus
      `the_board_keeps_its_switches_through_deep_copy_and_restore`.
    - Gate: `cargo test -p freerouting --test settings_java_quirks` 11 passed.
    - Left for an operator decision, not in this item: the search trees' and the board's other
      area splits still run Java's cutout code under every switch value (above).
- [x] **N06.4 — Batch G, geometry cutouts:** `simplex_prev_division_line`,
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
    - [x] Wired at every `Area` split (N06 follow-up, decided 2026-10-18, option (b) of N06.3's
      audit; branch `land/n06-splits` on `490d177`). Until then the search trees' shapes of
      obstacle and conduction areas and of the outline's generated keepout, and the board's other
      area splits, kept Java's code in every arm. Now every split takes the board's
      `JavaQuirks::split`: `board/search/mod.rs` (area tree shapes through the new
      `ShapeContext::split_quirks`; the outline's keepout through `BoardOutline::keepout_pieces`),
      `BoardOutline`'s piece count (`keepout_split`, retaken by generation, moves and a switch
      change), `board/queries.rs` `check_shape`, `board/basic_board.rs` `overlapping_items`,
      `shove/spring.rs`, `interactive/construct.rs`, `io/specctra/parser/dsn_file.rs` and the
      interactive route's nearest target point (`Area::nearest_point_approx_with`, geometry). The
      reader builds the board with its final switches before the first item
      (`RoutingBoard::new_with_java_quirks`); `BasicBoard::set_java_quirks` reinserts, in item-list
      order, only the items whose pieces change when the split switches change
      (`board/split_switches.rs`). Each site carries a
      `PARITY(quirk, java_quirks.octagon_box_cutout_branch3)` marker. Tests (all pass):
      `tests/n06_area_splits.rs` (6: area tree shapes on the branch-3, border and half-corner
      shapes, outline keepout and its move, `check_shape` and `overlapping_items`, the
      interactive hole, a switch change after insertion, and `Issue756-tomu-fpga9` read with
      every switch `false` against read with Java's and switched afterwards: equal pieces in every
      tree, 2 of 1688 items differ from Java's), and the unit tests
      `dsn_file::n06_tests::plane_share_follows_the_split_switches`,
      `spring::n06_tests::spring_shape_of_a_partitioned_area_uses_the_board_split` and
      `route::n06_tests::nearest_target_point_follows_the_split_switches`. With every switch
      `true`: `PARITY_SET=tools/bench/sets/connection-parity.txt PARITY_MAX_JAVA_S=20 cargo test
      --release -p freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
      --test-threads=1`: exact 7 boards, 7 pass, 0 fail, 7 SES byte-identical. This item's
      acceptance stands for the drill-page and tie-pin reach it was measured at; the wider reach
      is re-measured under N06.14 (its amendment 1, the `octagon_box_cutout_branch3` arm). Corpus:
      the stored-area splits reach branch 3 on `Issue756-tomu-fpga8` (1) and `-fpga9` (6) only;
      canary-pwr's fixture and its plugin export have no stored area with holes (0).
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
    - [x] Heavy run (done 2026-10-18, below):
      `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.4.json --out target/orchestrator/n06-g`.
      - `n06_run.py` exits non-zero when sessions differ, which is expected for the branch-3
        arms. It writes `summary.json` with every run's counters first.
      - Then grade each SES with `gate collect` and `freerouting-dev widths`, and judge the
        pairs with `freerouting-dev gate --rule totals`.
      - Then the plugin canary run and grade, per the registration's `plugin_canary` block.
      - Also the corpus inventory (read-only; the path must be absolute):
        `N06_CUTOUT_COUNTERS_OUT=/home/thebo/code/freerouting-rs/target/orchestrator/n06-g-corpus.json CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo test --release -p freerouting --test n06_cutout_reach unwired_area_splits_corpus_inventory -- --ignored --test-threads=1`.
  - *Done 2026-10-18:* both **Accept:** clauses hold. The invariant holds with both switches
    `false` (above), and the batch is measured and passes the accept rule. Worktree
    `target/orchestrator/n06r1/wt` (branch `land/n06-runs1`, main `3b1bde6`), one Linux release
    CLI (sha256 `693ec4aa…`, git `3b1bde6b8e05`), default profile `enhanced`.
    - Two amendments, both written before any N06.4 route (sha256 now
      `3e2a4eac471ce1f37ab11f48dc0a590b1525f2c87f2386df874e8031141ac402`, re-pinned in
      `prereg_n06_4.rs`, 4 tests pass).
      - The engine base moves from b93bcd0 to `3b1bde6` (N06.4's diff is `5607634`).
      - Hard-guard check: the registered `below_minimum` (nets under their class width) counts
        `enhanced`'s neck-down tapers. Amendment 1 changed it to traces under each board's
        narrowest class width (option a). N06.7's run, on the three rows the batches share, then
        showed that the router's pin neck-down breaks that too: 37 traces on Issue026's baseline
        and 11 on bm08's. So amendment 2 makes both `below_minimum` and `via_in_pad` "no new breach
        versus baseline" (option b): the arm's count minus the baseline arm's, floored at 0, with
        the raw counts reported.
      - `via_in_pad` is counted as main `4885a30`'s `added_via_in_pad`.
      - The router's statistics come from its result manifest.
    - Measurement tools: none of the grading uses `freerouting-dev gate collect`, `bench` or
      `widths`. The engine-model counts come from a scratch tool on the engine API
      (`target/orchestrator/n06r1/n06m`). Before importing the SES, it strips the input's wiring
      that the session repeats, as main `019b260` does; it skips vias and traces the input already
      has, as `4885a30` does; and it reads the SES at the writer's scale (`ses_reader_scale =
      false`; Java's reader misplaces Issue676's session). Its unrouted, clearance and via counts
      match the router's manifests on all 16 runs. The gate is `freerouting-dev` built at
      `3b1bde6`; it only judges records and imports nothing.
    - `n06_run.py`: 16 runs (4 rows × 4 arms). It exits 1 because sessions differ, which is
      expected. Hard checks hold: on every row the simplex arm's SES equals the baseline's and
      `all`'s equals the branch-3 arm's (sha256), and `simplex_prev_division_line_reached` is 0 in
      every run.

      | row | arm | unrouted | clearance | vias | length mm | branch3 reached | drills computed | below class width (raw) |
      |---|---|---|---|---|---|---|---|---|
      | Issue026-J2_reference | baseline | 0 | 0 | 16 | 371.16 | 1770 | 10724 | 37 |
      | | all | 0 | 0 | 14 | 379.61 | 1589 | 7318 | 36 |
      | Issue508-DAC2020_bm08 | baseline | 0 | 0 | 1 | 94.82 | 277 | 974 | 11 |
      | | all (same SES) | 0 | 0 | 1 | 94.82 | 277 | 867 | 11 |
      | Issue676-ch32v-tx118s | baseline | 0 | 0 | 6 | 14.26 | 0 | 0 | 0 |
      | | all (same SES) | 0 | 0 | 6 | 14.26 | 0 | 0 | 0 |
      | canary-pwr-9511d9a | baseline | 122 | 40 | 142 | 1436.80 | 393149 | 3144790 | 33 |
      | | all | 125 | 40 | 138 | 1452.04 | 386471 | 2713302 | 30 |

      `via_in_pad` is 0 everywhere, raw and new.
    - Plugin row: the same export as N06.7's (canary `9511d9a`, `canary-graded-flow.json`, WSL
      KiCad 10.0.7). The 4 arms were routed with the exported args and each arm's two flags. The
      baseline and simplex arms give one SES, `bed461b2…`, byte-identical to Q16.4's plugin-path
      session at b93bcd0. The branch-3 arm and `all` give `2953523d…`. The router's own count is 102
      → 104 unrouted, 40 → 40 clearance, 166 → 161 vias; branch 3 is reached 106178 → 138839 times
      and the simplex merge 0.
    - Grades (`headless_run.py import`, then canary `grade.sh` at `2a7e070b` with
      `GRADE_REV=9511d9a`, WSL KiCad 10.0.7):
      - baseline: 41 unconnected, 8 starved thermals, 17 dangling vias (warning);
      - candidate: 43 unconnected, 7 starved thermals, 24 dangling vias.
      - Both have 0 via-in-pad, 0 `track_width`, 0 clearance and 0 inner-layer tracks.

      Handoff: `target/canary-handoff/2026-10-18-n06-g/` (README.txt, SHA256SUMS).
    - Gate, run from this worktree: `freerouting-dev gate --records
      target/orchestrator/n06r1/records-g.json --baseline target/orchestrator/n06r1/baseline-g.json
      --prereg docs/roadmap/prereg/N06.4.json` (records built by
      `target/orchestrator/n06r1/build_records_g.py`).
      - 0 refusals and **PASS** (totals verdict: accept).
      - Signals: Issue026, bm08 and Issue676 unrouted 0 → 0 (band 2 each); canary-pwr
        `kicad_unconnected` 41 → 43 (+2, band 10).
      - The total changes by +2 against a set noise of 10.58, and no board is worse by more than
        twice its band.
      - Guards: `drc_real` 8 → 7, clearance unchanged, no new via in a pad or trace under the
        minimum.
      - The DSN unrouted of canary-pwr (122 → 125) is reported, not the row's signal.
      - The reported pairs, judged by script (the gate judges only the primary pair): on every row,
        the branch-3 arm's records equal `all`'s, so the same verdict holds, and the simplex arm's
        equal the baseline's (neutral).
    - Corpus inventory (the registered command): 1 passed. Of 205 DSN files, 203 load. The unwired
      search-tree area splits reach branch 3 on two boards only, Issue756-tomu-fpga8 (1) and
      tomu-fpga9 (6), neither a registered row, and the simplex merge on none. The outline-keepout
      splits reach neither. Output in `target/orchestrator/n06-g-corpus.json`.
    - Verdict: batch G **accepted** under the decisions' rule. Accepted for N06.13:
      `octagon_box_cutout_branch3` (measured, neutral within noise) and
      `simplex_prev_division_line` (inert: no session changes and its counter is 0; accepted on its
      tests, and N06.13 decides on promoting inert switches). `ENHANCED` is unchanged here.
    - *Wider reach re-measured 2026-10-20 (N06.14 amendment 1's arm, amendment 2's engine:
      main `08062e2`, `--router.profile=enhanced` with the 30 promoted fixes,
      `any_angle_room_side_index=false` in every arm):* the arm `octagon_box_cutout_branch3` sets
      both of this batch's switches `false` against the baseline's `true`, on N06.14's 7 DSN rows
      plus the plugin row (canary `9511d9a`, graded-flow options).
      - `gate --prereg docs/roadmap/prereg/N06.14.json --arms baseline,octagon_box_cutout_branch3`:
        0 refusals, 8 rows. Total 196 → 201 (+5, all from canary-pwr's DSN, 122 → 127 unrouted;
        plugin row 39 → 39 KiCad unconnected; J2 0 → 0 with 16 → 14 vias; tomu-fpga8 and -fpga9
        sessions identical), set noise 21.40.
      - Hard guard broken: canary-pwr's DSN has 33 → 34 traces under the narrowest class width
        (`below_minimum` 1), so `totals verdict: reject`, `gate: FAIL`. N06.4's own run at
        `3b1bde6` had 33 → 30 on that row.
      - The wider reach adds nothing on canary-pwr: amendment 1's load-only count
        (`N06_CUTOUT_DSNS`) found 0 stored areas with holes in canary-pwr-9511d9a and N06.4's plugin
        export (the corpus inventory has only `canary-pwr-1a7df77-addonly`, also 0). So the change from N06.4's numbers comes from the engine and profile moving
        on (`3b1bde6` → `08062e2`, `enhanced`'s 30 fixes), not from the newly wired splits. This is
        an inference from the counts; no run isolated it.
      - `simplex_prev_division_line_reached` is 0 in all 40 runs, and the inventory's stored-area
        and outline-keepout columns for it are 0 on all 203 boards. The arm's differences are
        therefore branch 3's, and `simplex_prev_division_line` stays inert at the wider reach (it
        remains accepted on tests).
      - Outcome: this item's acceptance of `octagon_box_cutout_branch3` stands only for the
        drill-page reach at `3b1bde6`. At the wider reach under today's `enhanced` it fails, and
        N06.14's decision table makes the next step an operator decision: a separate switch for
        the newly wired splits, or the switch stays `true` in both profiles. Until then N06.13
        does not promote it.
- [x] **N06.5 — Batch M, maze search and rooms:** `init_failure_room_leak` (with the fix
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
    - [x] Heavy run (done 2026-10-18, below), release build of the measured tree, run from its
      root:
      `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.5.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-m`
      (35 runs; the arms' commands call
      `/home/thebo/code/freerouting-rs/target/orchestrator/n06-target/release/freerouting`;
      it exits non-zero after the last run if a session differs, and then the totals rule
      applies to `summary.json`'s `statistics`). Then the canary-pwr plugin row as in N06.10
      (the registration's `plugin_canary`: export once, route the 7 arms with the exported
      args plus `--router.profile=legacy` and the arm's five flags, grade each distinct
      session with canary's `grade.sh`).
  - *Done 2026-10-18:* the **Accept:** clause holds. The batch is measured, with two 90° rows
    in the set, and `ortho_zero_width_room`'s counter reads above 0 on both. Worktree
    `target/orchestrator/n06r2/wt` (branch `land/n06-runs2`, main `ce17855`), one Linux release
    CLI (sha256 `06e916e5…`, `--version` git `ce17855bcadf`).
    - Amendment 1, written before any N06.5 route (sha256 now
      `e49fba98edbe53bb3b7dc3189b7efd0a40e783f9fd207d0e8d46accb5dd2545c`, re-pinned in
      `prereg_n06_5.rs`, 4 tests pass).
      - The engine base moves to main `ce17855` (N06.5's diff is `e6dc8bb`). Every switch added
        since defaults to `true`, and `ENHANCED` is unchanged.
      - The plugin row joins the gate. `boards.files` gains `canary-pwr-plugin`, hashed as its
        exported DSN (`65ef16a8…`), and `boards.outside_runner` names it, so `n06_run.py` skips it
        (the script now reads that key).
      - Hard metrics are counted on every row, so the gate grades every row. On a DSN row they
        are counted as N06.4's amendment 2 counts them: `added_via_in_pad`, and new traces under
        the board's narrowest class width. On the plugin row they are `via_in_pad.py` and KiCad's
        `track_width`. Both use option (b), no new breach versus baseline (raw counts reported).
      - Hard-guard check: N06.7's `legacy` sessions of J2 and bm08 (rows here too) already break
        an absolute DSN `below_minimum`, through the router's pin neck-down (37 and 11 traces under
        the narrowest class width). This run confirms it: 37 on J2, 11 on bm08 and 1 on J2-90, the
        same in every arm. The plugin row's baseline configuration
        is N06.7's graded one (0 via-in-pad, 0 `track_width`).
      - The verdict comes from `freerouting-dev gate --prereg --all-pairs`.
    - `n06_run.py`: 35 runs (5 rows × 7 arms), exit 0, "Sessions identical": every arm's SES
      equals its row's baseline byte for byte. Numbers from the router's manifests (the
      engine-model counts of the scratch tool `target/orchestrator/n06r2/n06m`, N06.4's, which
      reads each SES at the writer's scale, agree on all 35; no `freerouting-dev` import is used):

      | row | unrouted | clearance | vias | length mm | ortho flips | edge sweeps = edge flips | below class width (raw) |
      |---|---|---|---|---|---|---|---|
      | Issue026-J2_reference-90 | 0 | 0 | 16 | 400.52 | 254 (255 in `ortho_zero_width_room`, `all`) | 0 | 1 |
      | Issue143-rpi_splitter-90 | 0 | 0 | 2 | 118.63 | 61 | 0 | 0 |
      | Issue026-J2_reference | 0 | 0 | 16 | 371.16 | 0 | 373 | 37 |
      | Issue508-DAC2020_bm08 | 0 | 0 | 1 | 94.82 | 0 | 22 | 11 |
      | Issue676-ch32v-tx118s | 0 | 0 | 6 | 14.26 | 0 | 0 | 0 |

      In every run `maze_init_failure_room_leaks`, `maze_ripped_carryovers` and
      `insert_connect_to_trace_width_flips` are 0, and so is `via_in_pad` (raw and new).
    - Plugin row: canary 9511d9a (`git archive`, unedited), exported once at `ce17855` with
      `canary-graded-flow.json` on WSL KiCad 10.0.7. The DSN body equals N06.7's export (only line
      1, its own path, differs), and so do the args. The 7 arms were routed with the exported args,
      `--router.profile=legacy` and each arm's five flags (`target/orchestrator/n06r2/route_arms.py`).
      They give one session, `986e30bd…`, byte-identical to N06.7's. Router's count: 96 unrouted, 40
      clearance, 187 vias.
      - `maze_edge_sweeps_without_neighbours` and `maze_edge_corner_flips` read 12121, or 12116 in
        the two arms with `edge_corner_not_advanced=false`.
      - The other four counters read 0.
    - Grade of that session (`headless_run.py import`, then canary `grade.sh` at `2a7e070b`,
      `GRADE_REV=9511d9a`): 38 unconnected, 4 clearance (Power_High 0.25 mm), 5 starved
      thermals, 20 dangling vias (warning), 0 via-in-pad, 0 `track_width`, 2 inner-layer tracks
      (+12V_BUS on Power.Cu, 12.94 mm). This is N06.7's grade, in every arm. Handoff:
      `target/canary-handoff/2026-10-18-n06-m/` (README.txt, SHA256SUMS).
    - Gate, run from the worktree: `freerouting-dev gate --records
      target/orchestrator/n06r2/records-m.json --prereg docs/roadmap/prereg/N06.5.json --baseline
      target/orchestrator/n06r2/baseline-m.json --all-pairs`. The records are built by
      `target/orchestrator/n06r2/build_records.py`, and the baseline file was written by the same
      command with `--write-baseline`. Result: 0 refusals, `gate: PASS`.
      - The verdict pair and all five reported pairs PASS. Every row is neutral, the total is
        38 → 38 (unrouted 0 → 0, `kicad_unconnected` 38 → 38) against a set noise of 10.95, and
        `drc_real` is 9 → 9.
    - Verdict: the reachability rule decides each switch, and the totals rule passes as well.
      - `init_failure_room_leak`, `failed_attempt_ripped_carryover` and
        `connect_to_trace_swapped_width`: the counter is 0 in every run and the sessions equal the
        baseline's. They are not reached, so they are accepted on their tests (both behaviours
        pinned above). No registered row has per-layer widths, as expected.
      - `ortho_zero_width_room` (both 90° rows, 254 and 61 rooms) and `edge_corner_not_advanced`
        (three rows and the plugin row): the counter is above 0 but the sessions are identical,
        so each is **neutral and accepted**.
      - Batch M is **accepted**. All five switches are accepted for N06.13; `ENHANCED` is
        unchanged here.
- [x] **N06.6 — Batch F, fanout:** `fanout_stale_drill_carry` (a fresh `DrillCarry` per
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
    - [x] Heavy run (done 2026-10-18, below), release build of the measured tree, run from its
      root:
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
  - *Done 2026-10-18:* both **Accept:** clauses hold. The batch is measured with the fanout
    on: every run's statistics show the `fanout` stage. The retry counts are below. Worktree
    `target/orchestrator/n06r2/wt` (branch `land/n06-runs2`, main `ce17855`), with the same CLI as
    N06.5's (sha256 `06e916e5…`).
    - Amendment 1, written before any N06.6 route (sha256 now
      `78fb583f15761d289e860bd4d3e1dccf625df65a3d9f437856a37c8ea62a6d48`, re-pinned in
      `prereg_n06_6.rs`, 4 tests pass). It matches N06.5's amendment, with these specifics:
      - The engine base moves to main `ce17855` (N06.6's diff is `2e031ac`).
      - The plugin row joins `boards.files` and `boards.outside_runner`.
      - Hard metrics are counted on every row with option (b), no new breach versus baseline.
        The reason is the pin neck-down on the DSN rows (bm08's 11). On the plugin row, no graded
        session with the fanout on existed before this run.
      - The verdict comes from `gate --prereg --all-pairs`.
    - `n06_run.py`: 30 runs (6 rows × 5 arms), exit 0, "Sessions identical". Numbers from the
      router's manifests. Every arm gives the same values; `drills_computed` is the baseline's
      (→ the `fanout_stale_drill_carry` and `all` arms' where it differs):

      | row | unrouted | clearance | vias | length mm | fanout retries | succeeded | stale carries | drills computed |
      |---|---|---|---|---|---|---|---|---|
      | Issue676-ch32v-tx118s | 0 | 0 | 6 | 14.26 | 0 | 0 | 0 | 0 |
      | Issue957-smoke-ldo-led | 1 | 0 | 14 | 45.22 | 0 | 0 | 0 | 0 |
      | Issue367-Charger | 9 | 1 | 15 | 233.94 | 0 | 0 | 0 | 1737 |
      | Issue756-tomu-fpga | 15 | 72 | 162 | 1597.45 | 0 | 0 | 0 | 441 |
      | Issue269-z10_module | 37 | 10 | 129 | 1918.00 | 43 | 0 | 43 | 92609 → 101521 |
      | Issue508-DAC2020_bm08 | 0 | 0 | 1 | 94.82 | 0 | 0 | 0 | 974 |

      `drill_destination_room_index_flips` and `fanout_pin_order_name_flips` are 0 in every run,
      and so is `via_in_pad` (raw and new). Raw `below_minimum` is 11 on bm08 and 0 elsewhere.
    - Engine-model counts: the scratch tool `target/orchestrator/n06r2/n06m` reads each SES at
      the writer's scale; no `freerouting-dev` import is used. Its unrouted and via counts match
      the manifests on all 30 runs. Its clearance count does not on two rows: tomu-fpga 50
      against the manifest's 72, and z10_module 7 against 10, the same in every arm. Clearance is
      registered from the manifest, so that difference decides nothing.
    - Plugin row: the same export and args as N06.5's. The 5 arms were routed with the exported
      args, `--router.profile=legacy`, `--router.fanout.enabled=true` and each arm's three flags.
      - They give one session, `986e30bd…`, the same bytes as N06.5's and N06.7's. The fanout is
        on by default (`fanout.enabled` defaults to `true`), so N06.5's and N06.7's plugin runs
        already ran it, and the added flag changes nothing.
      - Fanout: 367 SMD pins, 255 escaped. `fanout_retries` 264, `fanout_retries_succeeded` 0
        and `fanout_stale_drill_carries` 264 in every arm. The other two counters are 0.
        `drills_computed` is 676873, and 734368 in the two arms with
        `fanout_stale_drill_carry=false`.
      - The grade of that session is N06.5's: 38 unconnected, 4 clearance, 5 starved thermals, 20
        dangling vias (warning), 0 via-in-pad, 0 `track_width`, 2 inner-layer tracks. Handoff:
        `target/canary-handoff/2026-10-18-n06-f/` (README.txt, SHA256SUMS).
    - Retries, summed over the six DSN rows plus the plugin row:
      - before (`baseline`): 307 retries (43 + 264), 0 succeeded;
      - after (`fanout_stale_drill_carry`, and `all`): 307 retries, 0 succeeded.
      - Over the DSN rows alone it is 43 and 0 in all three arms.
      - The fresh pages make each retry compute its drills again (z10 +8912 drills, the plugin
        row +57495), and no retry that fails under Java routes with them.
    - Gate: `freerouting-dev gate --records target/orchestrator/n06r2/records-f.json --prereg
      docs/roadmap/prereg/N06.6.json --baseline target/orchestrator/n06r2/baseline-f.json
      --all-pairs` (records built by `target/orchestrator/n06r2/build_records.py`). Result: 0
      refusals, `gate: PASS`.
      - The verdict pair and the three reported pairs PASS. Every row is neutral, the total is
        100 → 100 (unrouted 62 → 62, `kicad_unconnected` 38 → 38) against a set noise of 11.14,
        and `drc_real` is 9 → 9.
    - Verdict: the reachability rule decides each switch, and the totals rule passes as well.
      - `drill_destination_room_index` and `fanout_pin_order_names`: the counter is 0 and the
        sessions are identical. They are not reached, so they are accepted on their tests. No
        blind or buried via, and no arm sets `pin_sorting_order`, as expected.
      - `fanout_stale_drill_carry`: the counter is above 0 (z10_module and the plugin row) and the
        sessions are identical, so it is **neutral and accepted**.
      - Batch F is **accepted**. All three switches are accepted for N06.13; `ENHANCED` is
        unchanged here.
- [x] **N06.7 — Batch S, shove:** `in_front_of_pad_typo`, `store_trace_class_self_compare`,
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
    - [x] Heavy run (done 2026-10-18, below):
      `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.7.json --out target/orchestrator/n06-s`
      (30 runs; it exits non-zero after the last run if a session differs, and then the
      totals rule applies to `summary.json`'s `statistics`). Then the canary-pwr plugin row as
      in N06.10 (the registration's `plugin_canary`: export once, route the 6 arms with the
      exported args plus `--router.profile=legacy` and the arm's four flags, grade each
      distinct session with canary's `grade.sh`).
  - *Done 2026-10-18:* both **Accept:** clauses hold. The batch is measured, and the (c) arm
    `all_flank` is measured. Worktree `target/orchestrator/n06r1/wt` (branch `land/n06-runs1`, main
    `3b1bde6`), one Linux release CLI (sha256 `693ec4aa…`, `--version` git `3b1bde6b8e05`).
    - Amendment 1, written before any route (sha256 now
      `45a043ee9dea8b66457f66bc239e557dc5802fd68cbb885c9dfcb1246b594dc1`, re-pinned in
      `prereg_n06_7.rs`, 4 tests pass). It moves the engine base from b93bcd0 to `3b1bde6`
      (N06.7's diff is `1dac842`) and says how the verdict is applied. The gate's `--prereg` mode
      can't take these records: the DSN rows have no KiCad grade, so their hard metrics would be
      null, and `canary-pwr-plugin` isn't in `boards.files`. So the accept list is applied by
      script. Hard-guard check: `via_in_pad` and `below_minimum` belong to the plugin row only, and
      N06.10's graded run of the same configuration had 0 of each, so the baseline breaks no hard
      guard. The guards are unchanged.
    - `n06_run.py`: 30 runs (5 rows × 6 arms), exit 0, "Sessions identical". In every run
      `session_differences` is 0, and so are all four counters (`shove_in_front_of_pad_typo_flips`,
      `shove_in_front_of_pad_flank_flips`, `shove_store_trace_class_flips`,
      `shove_rational_projection_flips`).

      | row | unrouted | clearance_violations | vias (every arm) |
      |---|---|---|---|
      | Issue026-J2_reference | 0 | 0 | 16 |
      | Issue508-DAC2020_bm08 | 0 | 0 | 1 |
      | Issue676-ch32v-tx118s | 0 | 0 | 6 |
      | Issue313-FastTest | 2 | 40 | 24 |
      | Issue027-zMRETestFixture | 0 | 0 | 24 |
      These come from the router's own result manifests. No `freerouting-dev` import-based
      measure is used, so neither main `4885a30` nor main `019b260` affects them.
    - Plugin row: canary-pwr at canary `9511d9a` (`git archive`, unedited), exported once with
      `headless_run.py export` and `canary-graded-flow.json` (sha256 `a9124f66…`) on WSL KiCad
      10.0.7. Its args equal N06.10's export. The DSN differs from N06.10's export only in the
      fiducial footprint's image (FID4–6 have their own image with a mask outline): plugin changes
      since 298af02. The 6 arms were routed with the exported args, `--router.profile=legacy` and each
      arm's four flags (`target/orchestrator/n06r1/plugin/route_arms.py`): one session for all 6
      (sha256 `986e30bd…`), with every flip counter 0. The router's own count was 96 unrouted, 40
      clearance and 187 vias.
    - Grade of that one session (`headless_run.py import`, then canary `hardware/tools/grade.sh` at
      canary `2a7e070b` with `GRADE_REV=9511d9a`, WSL KiCad 10.0.7): 38 unconnected, 4 clearance
      errors (Power_High 0.25 mm), 5 starved thermals, 20 dangling vias (warning), 0 via-in-pad,
      0 `track_width` (below minimum), and 2 inner-layer tracks (+12V_BUS on Power.Cu, 12.94 mm).
      These are the legacy profile's errors, as in N06.10, and every arm has them identically.
      Handoff: `target/canary-handoff/2026-10-18-n06-s/` (README.txt, SHA256SUMS).
    - Verdict: the reachability rule. No registered board reaches any of the branches, and every
      session equals its baseline, so the batch is **accepted on its tests**: both behaviours are
      pinned per switch (above). `all_flank` against `all`: the sessions are identical and the flank
      counter is 0, so it is accepted by the same rule. No totals arithmetic is needed. Accepted for
      N06.13: `in_front_of_pad_typo`, `store_trace_class_self_compare`, `rational_perp_projection`
      (all (b)) and `in_front_of_pad_side0_flank` ((c), N06.12). `ENHANCED` is unchanged here.
- [x] **N06.8 — Batch P, pull-tight:**
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
    - [x] Heavy run (done 2026-10-18, below), release build of the measured tree, from
      its root: `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.8.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-p`
      (24 runs; the arms' binary is
      `/home/thebo/code/freerouting-rs/target/orchestrator/n06-target/release/freerouting`;
      it exits non-zero after the last run if a session differs, and then the totals rule
      applies to `summary.json`'s `statistics`). Then the canary-pwr plugin row as in N06.10
      (the registration's `plugin_canary`: export once, route the 4 arms with the exported
      args plus `--router.profile=legacy` and the arm's two flags, grade each distinct
      session with canary's `grade.sh`).
  - *Done 2026-10-18:* the **Accept:** clause holds: the batch is measured. Worktree
    `target/orchestrator/n06r3/wt` (branch `land/n06-runs3`, main `ceeebdf`), one Linux release
    CLI (sha256 `b3951c88…`, `--version` git `ceeebdf3771c`).
    - Amendment 1, written before any N06.8 route (sha256 now
      `82559f27411307f47d9765163e4c32e5b6cd5babce1a06ce4d4ee54c7478d9de`, re-pinned in
      `prereg_n06_8.rs`, 4 tests pass). It follows N06.5's amendment:
      - The engine base moves to main `ceeebdf` (N06.8's diff is `bbd1336`). Every switch added
        since (N06.12, N06.14, N06.15) defaults to `true`, and `ENHANCED` is unchanged.
      - The plugin row joins `boards.files` (hashed as its export, `dff4a914…`) and
        `boards.outside_runner`.
      - Hard metrics are counted on every row with option (b), no new breach versus baseline,
        because the pin neck-down puts J2 (a row) under its narrowest class width on Java's
        baseline. The plugin row's baseline configuration is the graded `986e30bd…` one (0
        via-in-pad, 0 `track_width`).
      - The verdict comes from `gate --prereg --all-pairs`.
      - `seen_before_registration` is added (it was missing). It names the light runs at
        registration, and the baseline-configuration numbers that existed before the amendment
        (J2 by N06.7/N06.5, the plugin row's `986e30bd…`). No candidate arm had been run on any
        row.
    - `n06_run.py`: 24 runs (6 rows × 4 arms), exit 0, "Sessions identical": every arm's SES
      equals its row's baseline byte for byte. Every run's statistics show `optimizer_pass`, so
      the batch optimizer ran its pull-tight candidates on every row. The numbers come from the
      router's manifests, and every arm gives the same values. Below class width is the scratch
      tool `target/orchestrator/n06r3/n06m`'s raw count (it reads each SES at the writer's
      scale), and its unrouted and via counts match the manifests on all 24 runs:

      | row | connections | unrouted | clearance | vias | length mm | below class width (raw) |
      |---|---|---|---|---|---|---|
      | Issue026-J2_reference | 33 | 0 | 0 | 16 | 371.16 | 37 |
      | Issue155-CH376_MCP795_Module | 53 | 0 | 0 | 14 | 828.10 | 13 |
      | Issue508-DAC2020_bm06 | 98 | 0 | 0 | 20 | 577.09 | 23 |
      | Issue558-dev-board | 96 | 0 | 0 | 33 | 1120.69 | 55 |
      | Issue217-8088sbc | 235 | 0 | 0 | 20 | 4852.11 | 55 |
      | Issue034-Green14SegLED | 147 | 0 | 0 | 44 | 2909.44 | 79 |

      In every run `pull_tight_smooth_lost_traces`, `pull_tight_reused_lines_flips` and
      `trace_change_prefix_suffix_ignored` are 0, and so is `via_in_pad` (raw and new).
    - Plugin row: canary 9511d9a (`git archive`, unedited), exported once at `ceeebdf` with
      `canary-graded-flow.json` on WSL KiCad 10.0.7. The DSN body equals N06.5's export (only
      line 1, its own path, differs), and so do the args. The 4 arms were routed with the exported
      args, `--router.profile=legacy` and each arm's two flags
      (`target/orchestrator/n06r3/route_arms.py`). They give one session, `986e30bd…`,
      byte-identical to N06.7's, N06.5's and N06.6's. Router's count: 96 unrouted, 40 clearance,
      187 vias. The three counters read 0 in every arm.
    - Grade of that session (`headless_run.py import`, then canary `grade.sh` at `2a7e070b`,
      `GRADE_REV=9511d9a`): 38 unconnected, 4 clearance, 5 starved thermals, 20 dangling vias
      (warning), 0 via-in-pad, 0 `track_width`, 2 inner-layer tracks. This is N06.5's grade, in
      every arm. Handoff: `target/canary-handoff/2026-10-18-n06-p/` (README.txt, SHA256SUMS).
    - Gate, run from the worktree with `freerouting-dev` built at `ceeebdf`: `freerouting-dev gate
      --records target/orchestrator/n06r3/records-p.json --prereg docs/roadmap/prereg/N06.8.json
      --baseline target/orchestrator/n06r3/baseline-p.json --all-pairs`. The records are built by
      `target/orchestrator/n06r3/build_records.py`, and the baseline file was written by the same
      command with `--write-baseline`. Result: 0 refusals, `gate: PASS`.
      - The verdict pair and both reported pairs PASS. Every row is neutral, the total is 38 → 38
        (unrouted 0 → 0, `kicad_unconnected` 38 → 38) against a set noise of 11.14, and
        `drc_real` is 9 → 9.
    - Verdict: the reachability rule decides it. Both switch counters are 0 in every run and every
      session equals its baseline, so no registered board reaches the branches.
      - `smooth_loses_trace` and `pull_tight_reused_lines_change` are **accepted on their tests**
        (both behaviours pinned above). The totals rule passes as well.
      - The prefix/suffix identity case: `trace_change_prefix_suffix_ignored` is 0 in every run,
        which confirms the reviewed list's claim that it is unreachable. It gets no switch.
      - Batch P is **accepted**. Both switches are accepted for N06.13; `ENHANCED` is unchanged
        here.
- [x] **N06.9 — Batch O, batch optimizer:** `opt_first_pass_stops` (a clear inside a pass
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
    - [x] Heavy run pending (orchestrator, one job at a time; release build of this tree):
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
  - *Done 2026-10-20:* the registered heavy run (heavy slot; worktree
    `target/orchestrator/n06r4/wt`, branch `land/n06-runs4` on main `ed603f5`; one Linux release
    CLI, sha256 `14712a93…`, `--version` git `ed603f538d3c`; every route under `ulimit -v
    8000000`, one route and one kicad-cli at a time).
    - Amendment 1, written after the build and the input hash checks and before any route
      (sha256 now `c5af6d94f4788a36f0d4184026cae59d6554a4fce36a83cd5940203283de07bb`, re-pinned in
      `prereg_n06_9.rs`, which now also checks that every arm names `--router.profile=enhanced`;
      4 tests pass):
      - The engine moves to main `ed603f5`. The registration ran "the default profile (enhanced,
        whose ENHANCED table does not touch java_quirks)"; since N06.13 (main `e0610fc`)
        `ENHANCED` sets 30 `java_quirks` keys to `false`, so every arm now gives
        `--router.profile=enhanced` explicitly and names the binary, and an arm means enhanced with
        those 30 fixes. Neither of the two keys is among them; the arms still differ only in them.
      - No row routes in any-angle mode (no `snap_angle` in the DSN rows or in the demos' KiCad
        exports), so `any_angle_room_side_index` keeps its default in every arm.
      - `n06_run.py` cannot run this registration (per-row `ROW_FLAGS`, `{OUT}`, demo rows), so
        `target/orchestrator/n06r4/run_o.py` substitutes the registered commands; every route
        appends `--stats json` (observation only) because `phases.optimizer.duration_seconds` is
        null at `ed603f5`, and `optimizer_seconds` is the stats' `optimizer_pass` seconds.
      - canary-plugin45 (already in `boards.files`, `0c49b990…`) is graded with canary's
        `grade.sh` (tools at 2a7e070b, the first that takes `GRADE_REV`) and `GRADE_REV=b1994423`,
        twice per distinct session, the first grade counting.
      - Hard metrics are option (b), no new breach versus the `java` arm (raw counts reported).
        bm01 and bm02 read with the scratch tool `target/orchestrator/n06r4/n06m` on the engine at
        `ed603f5` (input wiring stripped, SES at the writer's scale); `below_minimum` is the
        registered nets-under-class-width count. Demo `drc_real` is `grade.sh`'s minus its
        starved thermals, as registered.
      - The verdict comes from `freerouting-dev gate --prereg --all-pairs`.
      - `seen_before_registration` is unchanged; the amendment names the earlier routes of these
        rows under other engines and settings (N05.3, Q14, K09, the Q series). No row had been
        routed under this engine, profile and arms.
    - First attempt aborted on a runner bug: a relative `--out` with the arm directory as working
      directory, so the CLI could not write the four canary-plugin45 sessions (exit 1, no SES);
      its `run.log` tail showed the `java` arm's router count (97 unrouted, 40 clearance). The bm01
      `java` route was stopped. The same commands then ran with an absolute directory
      (`target/orchestrator/n06r4/n06-o`, the attempt kept in `n06-o-attempt1`).
    - 24 routes, all `COMPLETED`, none at the cap. On every row the four arms wrote the same
      session byte for byte, so each row's values hold in every arm:

      | row | connections | unrouted | KiCad unconnected | clearance | drc_real | vias | optimizer passes | optimizer s | wall s (java / both) | session |
      |---|---|---|---|---|---|---|---|---|---|---|
      | canary-plugin45 | 313 | 97 | 39 | 40 | 0 | 181 | bypassed (97 unrouted) | 0 | 78.3 / 78.2 | `ad6a4f0d` |
      | bm01 | 195 | 0 | – | 0 | – | 141 | 1 | 19.2 / 19.7 | 48.4 / 49.0 | `c7c63cf1` |
      | bm02 | 34 | 0 | – | 0 | – | 9 | 4 | 0.25 / 0.25 | 0.4 / 0.4 | `7d1d1ee8` |
      | interf_u | 201 | 0 | 12 | 0 | 0 | 43 | 1 | 2.4 / 2.5 | 8.3 / 8.5 | `400acd82` |
      | pic_programmer | 126 | 0 | 2 | 1 | 2 | 0 | bypassed (no vias, length within 5 %) | 0 | 0.3 / 0.3 | `7cae2a6b` |
      | CM5_MINIMA_3 | 409 | 7 | 23 | 1 | 40 | 171 | bypassed (7 unrouted) | 0 | 40.3 / 40.1 | `5c63e547` |

      Hard metrics: `via_in_pad` raw 0 on every row and arm; `below_minimum` raw 0 on every row
      (KiCad `track_width` 0; bm01 and bm02 0 nets under their class width, while the traces under
      the narrowest class width are 105 and 16, equal in every arm); option (b) 0 everywhere.
      `optimizer_score`: bm01 0.0, bm02 820.0, interf_u 569.26 in every arm.
    - canary-plugin45 grade (canary `grade.sh`, tools 2a7e070b, `GRADE_REV=b1994423`, WSL KiCad
      10.0.7), graded twice on separate copies, both grades equal: 39 unconnected, 5
      starved_thermal, 0 clearance, 0 shorting_items, 0 `track_width`, 0 schematic parity, 12
      via_dangling (warning), 0 via-in-pad, 0 inner-layer tracks. Handoff:
      `target/canary-handoff/2026-10-20-n06-o/` (README.txt, SHA256SUMS).
    - Gate (`freerouting-dev` built at `ed603f5`, from the worktree): `freerouting-dev gate
      --records target/orchestrator/n06r4/records-o.json --baseline
      target/orchestrator/n06r4/baseline-o.json --prereg docs/roadmap/prereg/N06.9.json
      --all-pairs` (records by `target/orchestrator/n06r4/build_records_o.py`, the baseline
      written first by the same command with `--write-baseline`): 0 refusals, 6 of 6 rows judged,
      every row neutral, total 76 → 76 (KiCad unconnected 76 → 76, unrouted 0 → 0), set noise
      19.95, `gate: PASS` for the verdict pair `java → both` and both reported pairs.
    - Reach: neither switch has a `run_stats` counter. The optimizer ran on 3 of the 6 rows (bm01
      1 pass, bm02 4, interf_u 1, the same in every arm): `opt_first_pass_stops = false` added no
      pass anywhere, and `opt_equal_key_skip = false` changed no session.
    - Verdict: batch O passes as a neutral fix. Both switches are **accepted for N06.13**. By the
      decision table N06.13 turns both off in `ENHANCED`; `ENHANCED` is unchanged here. The
      claim that batch O "most likely moves canary-pwr" does not hold on the plugin path: the
      optimizer never runs there (preflight guard 1).
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
      - Heavy run (done 2026-10-19, below), from the worktree root, release build of the
        measured tree, every route under the memory cap of amendment 2:
        `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && N06_RUN_ULIMIT_KB=8000000 python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.12-ripup-pass.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-c`
        (18 runs after amendments 2 and 3; exits non-zero after the last run when a session
        differs, as it does for the ripup arm), then the canary-pwr plugin row as in N06.10 (the
        registration's `plugin_canary`: export once, route the 3 arms with the exported args plus
        `--router.profile=legacy` and the arm's two flags, grade each distinct session with
        canary's `grade.sh`).
      - *Run 2026-10-19:* worktree `target/orchestrator/n06r3/wt` (branch `land/n06-runs3`,
        main `ceeebdf`), the CLI of N06.8's run (sha256 `b3951c88…`, `--version` git
        `ceeebdf3771c`).
        - Amendment 1, written before any route (sha256 then `4264d309…`). It follows N06.5's
          amendment: the engine base moves to main `ceeebdf` (N06.12's diff is `81afaba`). The
          plugin row joins `boards.files` (`dff4a914…`) and `boards.outside_runner`. Hard metrics
          on every row use option (b). The verdict pair is judged by `gate --prereg --arms
          baseline,ripup_pass_no_unset`, and the reported pair by its own clause, since the
          gate would apply `accept: improve` to it. `seen_before_registration` is added.
        - Amendment 2 (2026-10-19) drops `Issue026-J2_reference-any` before any number of it
          existed. The engine at `ceeebdf` can't route it in bounded memory: the baseline arm's
          RSS was 5.4 GB at 11 s, and it aborted at a 6 GB cap within 30 s. Two uncapped
          attempts of that route took the WSL machine down. This is tracked as a separate engine
          bug. Every route now runs under an 8000000 kB address-space cap (`n06_run.py`'s
          opt-in `N06_RUN_ULIMIT_KB`). Both cut-off attempts had completed the same 15 runs of
          the 45° rows, byte for byte, and only their session hashes were seen
          (`seen_before_registration` says so).
        - Amendment 3 (2026-10-19, sha256 now
          `f50fe24ea15af1bdad433eedbc7fdc8b37a2e6ba4a14c99e1d57a42561a465b6`, re-pinned in
          `prereg_n06_12.rs`, 4 tests pass) drops `Issue143-rpi_splitter-any`. Its baseline arm
          aborted at once under the cap (a 939524096-byte allocation failed), the same bug.
          `scoring-mixed-layer-any` (1 connection) is the one any-angle row left; its baseline
          arm routed under the cap in a probe whose exit status alone was read. The set noise is
          now 11.14.
        - `n06_run.py`: 18 runs (6 rows × 3 arms), exit 1, because the `ripup_pass_no_unset`
          session differs on Green14SegLED, ReadPlaceScope and Mars-64. The 15 runs of the 45°
          rows equal those of the cut-off attempts byte for byte. The numbers come from the
          router's manifests, given as baseline → `ripup_pass_no_unset`. Below class width is
          n06m's raw count. `any_angle_skip_length_squared` equals the baseline on every row:

          | row | connections | unrouted | clearance | vias | length mm | below class width (raw) | random-pass connections | factor draws |
          |---|---|---|---|---|---|---|---|---|
          | Issue034-Green14SegLED | 147 | 0 → 0 | 0 → 0 | 44 → 43 | 2909.44 → 2950.27 | 79 → 79 | 36 → 38 | 0 → 7088 |
          | Issue035-ReadPlaceScope | 240 | 2 → 2 | 152 → 152 | 170 → 171 | 5366.31 → 5370.73 | 80 → 81 | 89 → 72 | 0 → 5627 |
          | Issue313-FastTest | 83 | 2 → 2 | 40 → 40 | 24 → 24 | 69.82 → 69.82 | 7 → 7 | 44 → 44 | 0 → 1103 |
          | Issue054-tairakb | 264 | 5 → 5 | 0 → 0 | 147 → 147 | 3585.82 → 3585.82 | 42 → 42 | 109 → 109 | 0 → 45900 |
          | Issue102-Mars-64-revE-rot00 | 106 | 0 → 0 | 64 → 64 | 149 → 146 | 3941.26 → 3875.96 | 55 → 44 | 5538 → 212 | 0 → 103457 |
          | scoring-mixed-layer-any | 1 | 0 → 0 | 0 → 0 | 1 → 1 | 9.32 → 9.32 | 0 → 0 | 0 → 0 | 0 → 0 |

          `any_angle_skip_length_flips` is 0 in every run, and `via_in_pad` is 0 (raw and new).
          On FastTest and tairakb the factor draws, but the sessions are still identical.
        - Plugin row: N06.8's export. The 3 arms give one session, `986e30bd…`. The exported
          `--max-passes 1` runs no pass numbered 4 or later, so all three counters are 0 there.
          Router's count: 96 unrouted, 40 clearance, 187 vias.
        - Grade (the same procedure as N06.8's): 38 unconnected, 2 clearance, 5 starved
          thermals, 20 dangling vias (warning), 0 via-in-pad, 0 `track_width`, 2 inner-layer
          tracks. A second import and grade of the same bytes gave 4 clearance, as the grades of
          N06.5 to N06.8 did. The import writes the board's items in another order each time, and
          KiCad counts the +12V_BUS via's clearance pairs with +3V3_AON differently. All arms
          share the one session and the first grade, so the pairs are unaffected. Handoff:
          `target/canary-handoff/2026-10-18-n06-r/` (README.txt, SHA256SUMS).
        - Gate (`freerouting-dev` at `ceeebdf`; records from `target/orchestrator/n06r3/build_records.py`;
          baseline file written with `--write-baseline`): `freerouting-dev gate --records
          target/orchestrator/n06r3/records-c.json --prereg docs/roadmap/prereg/N06.12-ripup-pass.json
          --baseline target/orchestrator/n06r3/baseline-c.json --arms baseline,ripup_pass_no_unset`
          gives 0 refusals and **`gate: FAIL`**.
          - The total is 47 → 47 (unrouted 9 → 9, `kicad_unconnected` 38 → 38), which is no
            improvement against a set noise of 11.14.
          - Hard guard: ReadPlaceScope has 1 new trace under its narrowest class width (80 → 81).
          - `--all-pairs` gives the same verdict pair and FAIL for the reported pair, which is
            only reported (the gate applies `accept: improve` to it).
        - Verdict:
          - `ripup_pass_no_unset` is **rejected**. The random detour changes three sessions and
            no unrouted count. It adds one class-width breach on ReadPlaceScope, and on Mars-64 it
            saves 3 vias and 65 mm. It stays `true` (Java) in both profiles, with these numbers.
            The two rows whose baseline never reaches a drawing pass (the any-angle row and the
            plugin row) keep their sessions, as the reachability clause requires.
          - `any_angle_skip_length_squared` is **accepted on its test**: `any_angle_skip_length_flips`
            is 0 and every session equals its baseline. The one any-angle row left has a single
            connection, so the run barely exercises the any-angle tightener (amendments 2 and 3).
            It is accepted for N06.13 as a no-regress fix; `ENHANCED` is unchanged here.
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
- [x] **N06.13 — Results and promotion.**
  - Each batch's registration and numbers go in its item.
  - Each accepted switch is turned off in `enhanced` through N05, with its DIVERGENCES line under
    "Profile `enhanced`".
  - A rejected switch stays `true` in both profiles, with its numbers. It is never removed,
    because `legacy` needs it.
  - The DIVERGENCES "kept on purpose" paragraphs point to this file.
  - **Accept:** every (b) row ends accepted, rejected with numbers, or accepted on tests under
    the decisions' rule.
  - *Partial 2026-10-19:* the first promotion round, through N05 (N05.3's 2026-10-19 lines).
    Worktree `target/orchestrator/n0613/wt`, branch `land/n06-13` on main `0e3f1d5`. 30 switches
    promoted, 1 rejected, 7 pending, 7 left as Java (45 ids). Not ticked: 7 (b) rows are still
    pending (their batches' runs), and the promotion's confirmation run under `enhanced` is a
    heavy run.
    - [x] The list, from the done notes of N06.4 to N06.12 and the state of N06.9, N06.14 and
      N06.15. "On tests" is the decisions' reachability rule (counter 0, every session identical);
      "neutral" is measured, the counter above 0 and every session identical.

      | id | kind | batch | outcome | evidence |
      |---|---|---|---|---|
      | `simplex_prev_division_line` | b | G (N06.4) | accepted on tests; promoted in round 2 | N06.4 accepted it inert at the drill-page reach (counter 0, sessions identical); at every `Area` split (N06.14's run, 2026-10-20) `simplex_prev_division_line_reached` 0 in 40 runs and 0 in the corpus inventory; `the_simplex_switch_changes_no_piece` |
      | `octagon_box_cutout_branch3` | b | G (N06.4) | rejected | N06.4 gate PASS at the drill-page reach (+2 against noise 10.58); at every `Area` split (N06.14's run under its amendment 2, amendment 1's pair) `gate: FAIL`: `below_minimum` 33 → 34 on canary-pwr's DSN, total 196 → 201 (noise 21.40). Decided 2026-10-20: no separate switch for the newly wired splits (canary-pwr has 0 stored areas with holes, so the breach is at the drill-page reach under today's engine); stays `true` in both profiles |
      | `cutout_hole_crosses_border` | b | G2 (N06.14) | accepted (with `simplex_octagon_half_corner`); promoted in round 3 | N06.14b's run (amendment 1, 2026-10-20: the two switches together in arm `all`, engine `c8b91b7`): `gate: PASS`, totals 196 → 208 (+12, noise 21.40), no hard-guard or guard breach on 8 of 8 rows, consistency check holds; counter 109 → 384 on canary-pwr's DSN |
      | `simplex_octagon_half_corner` | b | G2 (N06.14) | accepted (with `cutout_hole_crosses_border`); promoted in round 3 | the same run and pair (counter 4686 → 5372 on canary-pwr's DSN, canary DSN 122 → 129 and plugin 39 → 44 KiCad unconnected, inside bands 10) |
      | `init_failure_room_leak` | b | M (N06.5) | accepted on tests | `maze_init_failure_room_leaks` 0 in 35 runs and the plugin row; gate PASS, 38 → 38 |
      | `failed_attempt_ripped_carryover` | b | M (N06.5) | accepted on tests | `maze_ripped_carryovers` 0 |
      | `ortho_zero_width_room` | b | M (N06.5) | accepted, neutral | 254 and 61 flips on the two 90° rows, sessions identical |
      | `edge_corner_not_advanced` | b | M (N06.5) | accepted, neutral | 373 (J2), 22 (bm08) and 12121 (plugin) sweeps, sessions identical |
      | `connect_to_trace_swapped_width` | b | M (N06.5) | accepted on tests | `insert_connect_to_trace_width_flips` 0 (no row has per-layer widths) |
      | `fanout_stale_drill_carry` | b | F (N06.6) | accepted, neutral | 307 retries, 0 succeeding in either arm, sessions identical; gate PASS, 100 → 100 |
      | `drill_destination_room_index` | b | F (N06.6) | accepted on tests | flips 0 (no blind or buried via) |
      | `fanout_pin_order_names` | b | F (N06.6) | accepted on tests | flips 0 (no arm sets the order) |
      | `in_front_of_pad_typo` | b | S (N06.7) | accepted on tests | flip counter 0, 30 runs and the plugin row identical |
      | `store_trace_class_self_compare` | b | S (N06.7) | accepted on tests | flip counter 0 |
      | `rational_perp_projection` | b | S (N06.7) | accepted on tests | flip counter 0 |
      | `smooth_loses_trace` | b | P (N06.8) | accepted on tests | counter 0, 24 runs and the plugin row identical; gate PASS, 38 → 38 |
      | `pull_tight_reused_lines_change` | b | P (N06.8) | accepted on tests | counter 0 |
      | `opt_first_pass_stops` | b | O (N06.9) | accepted, neutral; promoted in round 2 | under `enhanced`: gate PASS, total 76 → 76 (noise 19.95), every row's four arms one session; the optimizer ran on 3 of 6 rows and added no pass |
      | `opt_equal_key_skip` | b | O (N06.9) | accepted, neutral; promoted in round 2 | likewise; no session changed |
      | `dsn_via_alias` | b | I (N06.10) | accepted on tests | `dsn_via_alias_reached` 0 in 40 runs and on 203 corpus boards |
      | `dsn_class_class_pairs` | b | I (N06.10) | accepted on tests | its counter 0, likewise |
      | `dsn_shape_bbox` | b | I (N06.10) | accepted on tests | its counter 0, likewise |
      | `dsn_unit_ignored` | b | I (N06.10) | accepted on tests | its counter 0, likewise |
      | `ses_reader_scale` | b | I (N06.10) | accepted on tests | `issue676_round_trips_with_reader_scale_fixed`; 40 runs identical (no route reads a session) |
      | `stats_line_ab_length` | b | I (N06.10) | accepted on tests | a report value (`trace_length_statistics_both_behaviours`); 40 runs identical |
      | `route_result_int_via_ratio` | b | I (N06.10) | accepted on tests | `route_result_zero_test_changes` 0 in every run |
      | `drc_units_10000_per_mm` | b | I (N06.10) | accepted on tests | a report value (`drc_units_switch_uses_the_actual_board_scale`) |
      | `snapshot_drops_compensation` | b | U (N06.11) | accepted on tests | `compensated_snapshot_restores` 0 in 32 runs |
      | `rules_matrix_trees_not_rebuilt` | b | U (N06.11) | accepted on tests | interactive (GUI with compensation); 32 runs identical |
      | `move_drill_tidy_region` | b | U (N06.11) | accepted on tests | interactive only |
      | `route_change_layer_no_via` | b | U (N06.11) | accepted on tests | interactive only |
      | `route_connect_inner_break` | b | U (N06.11) | accepted on tests | interactive only |
      | `select_extension_ignores_filter` | b | U (N06.11) | accepted on tests | interactive only |
      | `shove_via_into_pending_piece` | b | N06.15 | accepted (no-regress); promoted in round 2 | both pairs `gate: PASS`; `enhanced` 312 → 319 (noise 69.24), Protein 89 → 96 within its band of 23 (via counter 2 → 3), no new clearance violation or short |
      | `any_angle_room_side_index` | b | N06.16 | accepted (abort rule); promoted in round 2 | under `enhanced`: J2-any and rpi_splitter-any abort at the 8000000 kB cap under `true` and route under `false` (0 unrouted, 0 clearance, `via_in_pad_raw` 0); the other 6 rows gate PASS, 0 → 0, sessions identical |
      | `ripup_pass_no_unset` | c | N06.12 | rejected | total 47 → 47 (noise 11.14), 1 new trace under ReadPlaceScope's narrowest class width (80 → 81), `gate: FAIL`; stays `true` in both profiles |
      | `insert_stale_trace_object` | c | N06.12 | left as Java (inert) | the split keeps the old geometry |
      | `in_front_of_pad_side0_flank` | c | S (N06.7, arm `all_flank`) | accepted on tests | flank counter 0, sessions identical to `all`'s |
      | `opt_threshold_fraction` | c | N06.12 | left as Java (heuristic) | the CLI's double scaling is a port fix in both profiles |
      | `rules_unknown_layer_scope` | c | N06.12 | accepted (decided 2026-10-18), on its test | `unknown_layer_scope_switch_both_behaviours`; `-dr` only |
      | `rules_autoroute_settings_reenable` | c | N06.12 | left as Java (inert) | C04's settings semantics |
      | `direction_compare_zero` | c | N06.12 | left as Java (inert) | an ordering convention |
      | `route_stubs_layer_test` | c | N06.12 | left as Java (inert) | the intended stub layer is unclear |
      | `any_angle_skip_length_squared` | c | N06.12 (reported arm) | accepted on tests | `any_angle_skip_length_flips` 0, every session identical |
      | `polyline_array_reversal` | c | N06.12 | left as Java (inert) | later reads depend on it |
      | `avoid_acid_traps_disabled` | c | N06.12 | left as Java (inert) | a Q candidate |
    - [x] Promoted: the 30 "accepted" rows, as `router.java_quirks.<id> = false` in `ENHANCED`
      (`settings/profile.rs`, one `Difference` each; the measurement strings are per batch), one
      DIVERGENCES line each under "Profile `enhanced`" (30 lines, with an opening paragraph that
      says they were measured under `legacy`), and each field's help text naming `enhanced`'s
      `false` (`settings/fields.rs`). The "kept on purpose" paragraphs of the promoted bugs now say
      "kept under `legacy`" and point to N06 (geometry `RationalPoint`, DSN reading, board model,
      rules files, the SES reader, shove checks, shove insert, pull-tight, connection insertion,
      interactive routing), and the N06 section's blocks say which batches are in `enhanced`.
      `legacy` stays Java: its table is empty.
    - [x] Tests: `tests/settings_java_quirks.rs::legacy_keeps_every_java_bug_and_enhanced_fixes_exactly_the_promoted_set`
      (was `both_profiles_keep_every_java_bug`) pins the exact list (`PROMOTED`, 30 ids in
      inventory order) against the resolved `enhanced`, the table and `legacy`;
      `the_job_carries_the_switches_to_the_board_and_the_control` checks that `prepare_job` with no
      flag puts the promoted set on the board, the router settings and the control, and `legacy`
      Java's; the round-trip tests pin `legacy`. `tests/settings_profile.rs::enhanced_turns_off_the_promoted_java_quirks`
      checks each entry under both profiles and that an explicit `true` beats the profile.
      Three tests that compare with Java now pin it: the unit test
      `connection::tests::an_init_failure_leaks_its_rooms_and_a_later_session_moves_them_aside`
      sets `init_failure_room_leak = true` on its control (its board loads under the default
      profile), and `tests/reference_optimize.rs` loads under `legacy` with `require_legacy` (its
      `candidate_items_match_java` failed under `enhanced` on Issue954-circle and J2's
      `improvement_percentage`, the `route_result_int_via_ratio` fix). The P01.3 microbench pins
      the default engine's work, so `crates/freerouting-dev/tests/microbench.rs` takes the new
      Issue433 maze counts (`maze_work_units` 90488 → 90636, `tree_nodes_visited` 173255 →
      173403; 7 routed, queue pops 952 and every other count unchanged).
    - [x] End to end with no flag (debug CLI, `fixtures/Issue756-minimal-ok.dsn`,
      `--dump-settings`, no routing): the 30 promoted switches read `false  (profile enhanced)`, the
      other 15 `(unset)`; with `--profile legacy` all 45 are unset; with
      `--router.java_quirks.ses_reader_scale=true` that switch reads `true  (CLI)`. Pinned by
      `crates/freerouting-cli/tests/profiles.rs::dump_settings_shows_the_promoted_java_quirks` (J2,
      `--dump-settings=json`). `both_profiles_route_and_enhanced_is_the_default` still passes: J2
      at `-mp 3` gives the same session under `legacy` and the new `enhanced`.
    - [x] `legacy` unchanged: `PARITY_SET=tools/bench/sets/connection-parity.txt PARITY_MAX_JAVA_S=20
      cargo test --release -p freerouting --test autoroute_board_parity -- --ignored
      autoroute_board_parity --test-threads=1` under `ulimit -v 8000000` (private target): exact 7
      boards, 7 pass, 7 SES byte-identical (`target/parity/summary.json` in the worktree).
    - [x] Gate (private target, `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`): `cargo fmt --all --check`;
      `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test -p freerouting --test
      settings_profile --test settings_java_quirks --test settings_path` 15 + 11 + 22 passed;
      `cargo test -p freerouting-cli` and `cargo test -p freerouting-dev --test prereg_n06_13` (5
      passed); `cargo test --workspace --no-fail-fast` before the last two fixes: 258 suites, the
      microbench (fixed above) and `freerouting-cli` `stop::two_signals_abort` failed; the latter
      passes alone (3 of 3 runs): its two back-to-back `kill -INT` race the end of pass 1 under
      the parallel load, not a settings effect.
    - [x] Confirmation registered before any number: `docs/roadmap/prereg/N06.13-promotion.json`
      (sha256 `cd57a3af1e9836f075d8bb5059076202844186ba56ffdafea66cdcd4bd6b5d24`), set `tools/bench/sets/n06-13-promotion.txt`, pinned by
      `crates/freerouting-dev/tests/prereg_n06_13.rs` (5 tests). Arms `enhanced_main` (the parent of
      the promotion commit) and `enhanced_promoted` (the promotion commit), both with no router
      flag. Rows: the 21 DSN rows of the promoted batches (M, F, S, P, I, U and N06.12's set,
      without the any-angle J2 and rpi_splitter variants) plus canary-pwr at canary 2a7e070b
      through the plugin path with the plugin's default options (`outside_runner`, hashed as its
      source board). N02 totals, `accept: no-regress`; hard guards option (b), no new breach versus
      baseline; every route under an 8000000 kB cap.
    - [x] Heavy run (done 2026-10-20, see Measured below) (orchestrator, one job at a time), after this diff lands as one commit:
      `cd /home/thebo/code/freerouting-rs && PROMOTION=<sha of the commit that lands land/n06-13> && git worktree add --detach target/orchestrator/n06-13-base-wt "$PROMOTION~1" && git worktree add --detach target/orchestrator/n06-13-cand-wt "$PROMOTION" && (cd target/orchestrator/n06-13-base-wt && CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-13-base cargo build --release -p freerouting-cli) && cd target/orchestrator/n06-13-cand-wt && CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-13-cand cargo build --release -p freerouting-cli && N06_RUN_ULIMIT_KB=8000000 python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.13-promotion.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-13`
      (42 runs; it exits non-zero when a session differs, which is expected). Then the plugin row
      (the registration's `plugin_canary`: write `{"schema": 4, "profile": "enhanced"}` as the
      options, export canary 2a7e070b once, route both arms' binaries with the exported args under
      `ulimit -v 8000000`, grade each distinct session with canary's `grade.sh`,
      `GRADE_REV=2a7e070b`), then `freerouting-dev gate --records <records> --baseline <baseline>
      --prereg docs/roadmap/prereg/N06.13-promotion.json` (baseline file first with
      `--write-baseline`). A FAIL is bisected by batch as the registration's `accept` says.
    - *Measured 2026-10-20:* the registered confirmation ran (heavy slot, one route at a time,
      every route under the 8000000 kB cap). Amendment 1, written after the builds and the plugin
      export and before any route, names the engines: baseline `enhanced_main` main `0e3f1d5`
      (CLI sha256 `8f9cbacc…`), candidate `enhanced_promoted` `c04b3f5` (CLI `a1a4f4c5…`), the
      promotion as registered; it landed as main `e0610fc`, the same diff (added and removed lines
      equal) rebased over `8c952e7` (plugin installer only) and `5006992` (N06.16's default-true
      switch). Registration sha256 now
      `8c15ca4ce63f320b834a87b5c064830021508fd77284cd282e3cc0cd3ff7b390`, re-pinned in
      `crates/freerouting-dev/tests/prereg_n06_13.rs`. Plugin row: canary 2a7e070b's board
      (sha256 `17b597a0…`, as registered) exported with `{"schema": 4, "profile": "enhanced"}`:
      DSN sha256 `52942eba5f1ba63bde8a5c69f9863af5badfb61efab9fa083542b348cd8e0ba3`, args
      `ec2dcc99…` (`--router.min_trace_width_um=150 --router.autorouter.sweep=free`).
      `n06_run.py`: 42 runs, all `COMPLETED` (no abort at the cap), exit 1 because 4 sessions
      differ (zMRETestFixture, dev-board, ReadPlaceScope, Mars-64). Below-minimum and via-in-pad
      raw counts by one tool on the engine at `0e3f1d5` for both arms (the input's wiring
      stripped, the session read at the writer's scale).

      | row | unrouted | KiCad unconnected | clearance | vias | below min. raw | via in pad raw | session |
      |---|---|---|---|---|---|---|---|
      | `Issue026-J2_reference-90` | 0 | – | 0 | 16 | 1 | 0 | identical |
      | `Issue143-rpi_splitter-90` | 0 | – | 0 | 2 | 0 | 0 | identical |
      | `Issue026-J2_reference` | 0 | – | 0 | 16 | 37 | 0 | identical |
      | `Issue508-DAC2020_bm08` | 0 | – | 0 | 1 | 11 | 0 | identical |
      | `Issue676-ch32v-tx118s` | 0 | – | 0 | 6 | 0 | 0 | identical |
      | `Issue957-smoke-ldo-led` | 1 | – | 0 | 14 | 0 | 0 | identical |
      | `Issue367-Charger` | 9 | – | 1 | 15 | 0 | 0 | identical |
      | `Issue756-tomu-fpga` | 15 | – | 72 | 162 | 0 | 0 | identical |
      | `Issue269-z10_module` | 37 | – | 10 | 131 | 2 | 0 | identical |
      | `Issue313-FastTest` | 2 | – | 40 | 24 | 7 | 0 | identical |
      | `Issue027-zMRETestFixture` | 0 | – | 0 | 35 → 31 | 40 → 39 | 0 | differs |
      | `Issue155-CH376_MCP795_Module` | 0 | – | 0 | 14 | 13 | 0 | identical |
      | `Issue508-DAC2020_bm06` | 0 | – | 0 | 20 | 23 | 0 | identical |
      | `Issue558-dev-board` | 0 | – | 0 | 34 | 61 | 0 | differs |
      | `Issue217-8088sbc` | 0 | – | 0 | 20 | 55 | 0 | identical |
      | `Issue034-Green14SegLED` | 0 | – | 0 | 44 | 79 | 0 | identical |
      | `canary-pwr-9511d9a` | 122 | – | 40 | 142 | 33 | 0 | identical |
      | `Issue035-ReadPlaceScope` | 2 | – | 152 | 170 | 80 → 79 | 0 | differs |
      | `Issue054-tairakb` | 5 | – | 0 | 147 | 42 | 0 | identical |
      | `Issue102-Mars-64-revE-rot00` | 0 | – | 64 | 149 → 126 | 55 → 53 | 0 | differs |
      | `scoring-mixed-layer-any` | 0 | – | 0 | 1 | 0 | 0 | identical |
      | `canary-pwr-plugin` | router 29 → 31 | 17 → 15 | router 109 → 109; KiCad 0 → 0 | 193 → 186 | 0 | 0 | differs |
      Plugin row grades (plugin import with the same options, canary `grade.sh` and tools at
      2a7e070b, `GRADE_REV=2a7e070b`, WSL KiCad 10.0.7), each session imported and graded twice on
      separate copies, both grades equal: `enhanced_main` (`c5370c72`) 17 unconnected, 4
      starved_thermal, 0 clearance, 0 shorting_items, 0 track_width, 0 schematic parity, 0
      via-in-pad, 0 inner-layer tracks (drc_real 4); `enhanced_promoted` (`d28e8e86`) 15
      unconnected, 1 starved_thermal, 0 clearance, 0 shorting_items, 0 track_width, 0 parity, 0
      via-in-pad, 0 inner (drc_real 1). Hard guards: via_in_pad and below_minimum option (b) 0 on
      every row (the raw counts fall or stay); no DSN row's clearance count rises; plugin drc_real
      4 → 1, shorting 0 → 0. `freerouting-dev gate --records … --baseline … --prereg
      docs/roadmap/prereg/N06.13-promotion.json` (built at `0e3f1d5`, whose freerouting-dev source
      equals `c04b3f5`'s; baseline file written first with `--write-baseline`): 0 refusals, 22 of
      22 rows judged, 21 neutral and the plugin row improve; unrouted 193 → 193, KiCad unconnected
      17 → 15, total 210 → 208 (change −2), set noise 65.25; `totals verdict: accept`,
      `gate: PASS`. Reported: Mars-64's route took 120 → 189 s (`wall_seconds`), zMRETestFixture
      27 → 23 s, the plugin row 129 → 103 s. Reach counters (counted in either arm), summed
      over the 22 rows, main → promoted: `maze_edge_corner_flips` 392473 → 572724,
      `maze_ortho_zero_width_room_flips` 307 → 308 (J2-90 254 → 255, its session identical),
      `fanout_stale_drill_carries` 330 → 329 (`fanout_retries_succeeded` 8 → 5; plugin row 5 → 2),
      and `shove_rational_projection_flips` 764 → 2088, which N06.7's legacy runs never reached:
      under `enhanced` it fires on CH376 (6, 6), bm06 (22, 22) and the canary DSN (10, 10) with
      their sessions identical, ReadPlaceScope (20, 20) and Mars-64 (706 → 2030), whose sessions
      differ. Every other promoted switch's counter is 0 in both arms. Verdict:
      **promotion confirmed**; the 30 entries stay in `ENHANCED`. Canary handoff:
      `target/canary-handoff/2026-10-20-n06-13/` (README.txt, SHA256SUMS). Not ticked: the 7
      pending (b) rows' batches.
    - *Outcomes 2026-10-20 (N06.14's and N06.16's runs, main `08062e2`):* `ENHANCED` is
      unchanged here; promotion is the next step.
      - `any_angle_room_side_index` (b, N06.16): **accepted**, measured under `enhanced`. Under
        `true` J2-any and rpi_splitter-any abort at the cap; the abort rule gives both rows to the
        fix, and the other 6 rows gate PASS, 0 → 0, sessions identical. Next round: promote.
      - `cutout_hole_crosses_border` and `simplex_octagon_half_corner` (b, N06.14): **no
        verdict**. N06.14's registered 45 degree identity check fails on canary-pwr (both counters
        above 0, and the half-corner fix changes its sessions). Both stay `true` in both profiles
        until a corrected registration measures them (N06.14 `[~]`).
      - `octagon_box_cutout_branch3` (b, N06.4): **failed at the wider reach** (gate FAIL, 1 new
        trace under the narrowest class width on canary-pwr's DSN). Operator decision per N06.14's
        decision table; not promoted.
      - `simplex_prev_division_line` (b, N06.4): inert at the wider reach (counter 0 in 40 runs,
        inventory columns 0); accepted on tests, as before.
      - `opt_first_pass_stops`, `opt_equal_key_skip` (N06.9) and `shove_via_into_pending_piece`
        (N06.15) were accepted on 2026-10-20 (their items) and are not promoted yet.
      - Not ticked: the two N06.14 switches have no verdict, and the branch-3 decision is open.
  - *Partial 2026-10-20 (second round):* worktree `target/orchestrator/n06p2/wt`, branch
    `land/n06-promo2` on main `c8b91b7`. 35 switches promoted (30 + 5), 2 rejected, 2 pending, 7
    left as Java (46 ids; the table above is updated). Not ticked: `cutout_hole_crosses_border` and
    `simplex_octagon_half_corner` have no verdict (N06.14 `[~]`); the round's confirmation
    passed on the quick rows (below).
    - [x] Promoted: `simplex_prev_division_line` (accepted on tests), `opt_first_pass_stops` and
      `opt_equal_key_skip` (N06.9), `shove_via_into_pending_piece` (N06.15) and
      `any_angle_room_side_index` (N06.16), as `router.java_quirks.<id> = false` in `ENHANCED`
      (`settings/profile.rs`, one `Difference` each, measurement strings `N06_4_PREV_DIVISION_LINE`,
      `N06_9_OPTIMIZER`, `N06_15_PENDING_PIECE`, `N06_16_ROOM_SIDES`), one DIVERGENCES line each
      under "Profile `enhanced`" (the section's opening paragraph now says round 1's confirmation
      passed and names round 2's registration), and each field's help text naming `enhanced`'s
      `false` (`settings/fields.rs`). "Kept on purpose" pointers: the geometry paragraph (the
      previous division line kept under `legacy` only, branch 3 rejected), the shove-insert bullet
      and the optimizer bullet now say "kept under `legacy`"; the N06 section's N06.4, N06.15 and
      N06.16 blocks say they are in `enhanced`; the module docs of `shove::insert`,
      `autoroute::session::neighbours_any` and `optimize::batch` name the profile.
    - [x] Not promoted, recorded in the table: `octagon_box_cutout_branch3` **rejected** (N06.14's
      run under its amendment 2, the branch-3 pair: `gate: FAIL`, `below_minimum` 33 → 34 on
      canary-pwr's DSN, total 196 → 201; decided 2026-10-20: no separate switch for the newly wired
      splits, because canary-pwr has 0 stored areas with holes, so the breach is at the drill-page
      reach under today's engine); it stays `true` in both profiles. `cutout_hole_crosses_border`
      and `simplex_octagon_half_corner` stay `true` in both profiles until N06.14's corrected
      registration measures them.
    - [x] Tests: `tests/settings_java_quirks.rs::legacy_keeps_every_java_bug_and_enhanced_fixes_exactly_the_promoted_set`
      (`PROMOTED` now 35 ids in inventory order; `the_job_carries_the_switches_to_the_board_and_the_control`
      turns `ripup_pass_no_unset` off as the switch the profile leaves `true`, where it took
      `opt_equal_key_skip`); `tests/settings_profile.rs::enhanced_turns_off_the_promoted_java_quirks`
      (35 entries, each `false` under `enhanced` and unset under `legacy`, an explicit `true` beats
      the profile); `crates/freerouting-cli/tests/profiles.rs::dump_settings_shows_the_promoted_java_quirks`
      and `crates/freerouting-gui/tests/ui_load_quirks.rs` (`PROMOTED` 35). Tests of a kept quirk's
      `true` half now pin `legacy`: `tests/batch_optimizer.rs` loads `equal_key_skips_rest_of_pass`,
      `equal_key_switch_keeps_every_candidate`, `first_pass_without_winner_stops` and
      `first_pass_switch_both_behaviours` with `routed_legacy` (they failed under `enhanced`).
      `crates/freerouting-dev/tests/prereg_n06_13.rs` keeps round 1's 30 (ENHANCED without the
      second round's five). The P01.3 microbench takes Issue433's new maze count:
      `tree_nodes_visited` 173403 → 174040, `maze_work_units` 90636 and the routes unchanged; with
      `--router.java_quirks.shove_via_into_pending_piece=true` added to its load the count is 173403
      again, so it is the pending-piece check's tree query, which leaves the work meter alone.
    - [x] `legacy` unchanged: `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release
      -p freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
      --test-threads=1` under `ulimit -v 8000000` (private target, at `0bf562b`'s tree): 7 boards,
      all `exact`, all `byte_equal`, all `pass` (`target/parity/summary.json` in the worktree).
    - [x] Gate (private target, `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`): `cargo fmt --all --check`;
      `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace
      --no-fail-fast`: 263 suites, 2606 passed, 0 failed, 56 ignored (the first run, before the
      `batch_optimizer` and microbench fixes above, failed only those 5 tests);
      `python3 tools/roadmap/checklists.py status --strict` clean.
    - [x] Confirmation registered before any number: `docs/roadmap/prereg/N06.13-promotion-2.json`
      (sha256 `773ec7a061a67f130fd5c6d4f113b066da68d1f289ecff8ae5b01d062b6eb921`, 0 amendments), set
      `tools/bench/sets/n06-13-promotion-2.txt`, pinned by
      `crates/freerouting-dev/tests/prereg_n06_13_2.rs` (5 tests pass). Arms `enhanced_main` (main
      `c8b91b7`, the round's parent) and `enhanced_promoted` (`0bf562b`, the round's commit), both
      `--router.profile=enhanced` and no other router flag. Rows: round 1's 21 DSN rows, then
      `Issue026-J2_reference-any` and `Issue143-rpi_splitter-any` (their baseline aborts at the cap
      under `any_angle_room_side_index = true`, so N06.16's `abort_rule` judges them: the
      candidate's win if it completes with `via_in_pad_raw` 0 and router clearance 0, reported not
      gated), plus canary-pwr at canary 2a7e070b through the plugin path with the plugin's default
      options (`outside_runner`, routed with `--router.profile=enhanced` added to the exported
      args). N02 totals, `accept: no-regress`; hard guards option (b), no new breach versus the
      baseline (the baseline already breaks an absolute `below_minimum`); every route under the
      8000000 kB cap. A FAIL is bisected by switch group. The registration is in the commit after
      `0bf562b`, so the run reads it from the main checkout once landed.
    - [x] Confirmation run (the quick rows only).
      *Done 2026-10-20:* amendment 1 written before any route (decided 2026-10-20: the
      confirmation keeps only the quick rows, to save time): 20 DSN rows kept in the set's order;
      dropped `Issue269-z10_module`, `canary-pwr-9511d9a`, `Issue102-Mars-64-revE-rot00` and the
      plugin row (each above 2 min per route in round 1's confirmation; they include the only
      round-1 row where `shove_via_into_pending_pieces` was reached, Mars-64 with 2, and both
      canary-pwr rows; N06.15 and N06.9 measured those switches under `enhanced` on their own
      rows). Set file and `boards.files` match, `totals.noise` and `outside_runner` emptied;
      registration sha256 `852080d15efcf2683780c0712c529e099ebff409841bcbc2860aec5e3ea70436`
      (1 amendment; the amendment's row count was corrected from 19 to 20 after the run started,
      before any number was read), re-pinned in `prereg_n06_13_2.rs` (5 passed). Run:
      `N06_RUN_ULIMIT_KB=8000000 python3 tools/bench/n06_run.py <worktree>/docs/roadmap/prereg/N06.13-promotion-2.json
      --out target/orchestrator/n06-13c` from a worktree of main `f86b9c5`, binaries
      `n06-13b-base` (sha256 `95d2ada2…`, git `c8b91b7d75f4`) and `n06-13b-cand` (`e748023f…`,
      git `0bf562b81f8d`): 40 routes, exit 1 (sessions differ only on the two aborted-baseline
      rows). Abort rule: `Issue026-J2_reference-any` and `Issue143-rpi_splitter-any` baselines
      abort at the cap (exit −6, "memory allocation of 1879048192 / 939524096 bytes failed", peak
      RSS 6793088 / 7009724 kB); the candidate completes on both (exit 0, 0 unrouted,
      `via_in_pad_raw` 0, clearance 0; `below_minimum_raw` 25 / 13 reported; peak RSS 58000 /
      46856 kB; `any_angle_room_side_mismatches` 3 / 40), so both rows are the candidate's win.
      The other 18 rows: every session byte-identical, measured by round 1's `n06m` rebuilt on
      the engine at `c8b91b7` for both arms (`via_in_pad_raw` 0 everywhere, `below_minimum_raw`
      equal per row, 447 → 447):

      | row | unrouted | clearance | vias | below min. raw | session |
      |---|---|---|---|---|---|
      | `Issue026-J2_reference-90` | 0 | 0 | 16 | 1 | identical |
      | `Issue143-rpi_splitter-90` | 0 | 0 | 2 | 0 | identical |
      | `Issue026-J2_reference` | 0 | 0 | 16 | 37 | identical |
      | `Issue508-DAC2020_bm08` | 0 | 0 | 1 | 11 | identical |
      | `Issue676-ch32v-tx118s` | 0 | 0 | 6 | 0 | identical |
      | `Issue957-smoke-ldo-led` | 1 | 0 | 14 | 0 | identical |
      | `Issue367-Charger` | 9 | 1 | 15 | 0 | identical |
      | `Issue756-tomu-fpga` | 15 | 72 | 162 | 0 | identical |
      | `Issue313-FastTest` | 2 | 40 | 24 | 7 | identical |
      | `Issue027-zMRETestFixture` | 0 | 0 | 31 | 39 | identical |
      | `Issue155-CH376_MCP795_Module` | 0 | 0 | 14 | 13 | identical |
      | `Issue508-DAC2020_bm06` | 0 | 0 | 20 | 23 | identical |
      | `Issue558-dev-board` | 0 | 0 | 34 | 61 | identical |
      | `Issue217-8088sbc` | 0 | 0 | 20 | 55 | identical |
      | `Issue034-Green14SegLED` | 0 | 0 | 44 | 79 | identical |
      | `Issue035-ReadPlaceScope` | 2 | 152 | 170 | 79 | identical |
      | `Issue054-tairakb` | 5 | 0 | 147 | 42 | identical |
      | `scoring-mixed-layer-any` | 0 | 0 | 1 | 0 | identical |
      | `Issue026-J2_reference-any` | aborted → 0 | – → 0 | – → 10 | – → 25 | abort rule |
      | `Issue143-rpi_splitter-any` | aborted → 0 | – → 0 | – → 2 | – → 13 | abort rule |

      `freerouting-dev gate --records … --baseline … --prereg
      docs/roadmap/prereg/N06.13-promotion-2.json --rows <the 18 rows>` (built at `c8b91b7`;
      baseline file written first with `--write-baseline` on the full records): 0 refusals,
      judged 18 of 20 rows, 18 neutral; unrouted 34 → 34 (change 0), set noise 61.01; `totals
      verdict: accept`, `gate: PASS`. Reported: `simplex_prev_division_line_reached`,
      `shove_via_into_pending_pieces` and `shove_pending_piece_foreign_traces` 0 in every run;
      `optimizer_passes` equal per row (22 → 22); `rooms_completed` 1064740 → 1094485 with the
      sessions identical (9 rows rise); router wall seconds summed 80.0 → 83.7. Verdict:
      **promotion confirmed** on the quick rows; the five entries stay in `ENHANCED`.
  - *Done 2026-10-20 (third and last round):* worktree `target/orchestrator/n06p3/wt`, branch
    `land/n06-promo3` on main `d920207`. 37 switches promoted (30 + 5 + 2), 2 rejected, 0 pending,
    7 left as Java (46 ids; the table above is updated). Every (b) row now ends accepted (34,
    promoted) or rejected with numbers (`octagon_box_cutout_branch3`), which is the **Accept:**
    clause; the (c) rows end promoted (3), rejected (`ripup_pass_no_unset`) or left as Java (7).
    - [x] Promoted: `cutout_hole_crosses_border` and `simplex_octagon_half_corner` (N06.14,
      accepted together by `prereg/N06.14b.json` amendment 1, sha256 `68f90bf5…`: `gate: PASS`,
      total 196 → 208 against noise 21.40, no hard-guard or guard breach on 8 of 8 rows), as
      `router.java_quirks.<id> = false` in `ENHANCED` (`settings/profile.rs`, one `Difference`
      each, measurement string `N06_14_SIMPLEX_CUTOUTS`), one DIVERGENCES line each under
      "Profile `enhanced`" (the opening paragraph now says round 2's confirmation passed and
      names round 3's), each field's help text naming `enhanced`'s `false` (`settings/fields.rs`).
      "Kept on purpose" pointers: the geometry paragraph names the two cutout bugs as kept under
      `legacy` only; the N06 section's opening paragraph and its N06.14 block say they are in
      `enhanced` and give N06.14b's numbers; the module docs of `settings::java_quirks` and
      `board::split_switches` name the profile.
    - [x] Confirmation: no separate run (decided 2026-10-20: measurement runs stay a few
      minutes; the measurement is the confirmation). N06.14b's candidate arm `all` routed
      `--router.profile=enhanced` at main `c8b91b7` (the first round's 30) with the second round's
      five `false`, `octagon_box_cutout_branch3=true` and both N06.14 keys `false`: every
      `java_quirks` key at the value this round's `ENHANCED` gives it, and no other router flag,
      so that arm is post-promotion `enhanced` and its `gate: PASS` against `baseline` (the
      parent's `enhanced`) is this round's confirmation. Between `c8b91b7` and this round's parent
      `d920207` the engine and CLI source changed only in docs and in round 2's `ENHANCED`
      entries, which the arm set explicitly (`git diff --stat c8b91b7 d920207` over every crate's `src`:
      7 files, profile tables, help texts and module docs).
    - [x] Tests: `tests/settings_java_quirks.rs::legacy_keeps_every_java_bug_and_enhanced_fixes_exactly_the_promoted_set`
      (`PROMOTED` now 37 ids in inventory order); `tests/settings_profile.rs::enhanced_turns_off_the_promoted_java_quirks`
      (37 entries, each `false` under `enhanced` and unset under `legacy`, an explicit `true` beats
      the profile); `crates/freerouting-cli/tests/profiles.rs::dump_settings_shows_the_promoted_java_quirks`
      and `crates/freerouting-gui/tests/ui_load_quirks.rs` (`PROMOTED` 37); README "37 of them".
      `crates/freerouting-dev/tests/prereg_n06_13.rs` keeps round 1's 30 (`ENHANCED` without
      the later rounds' seven, `LATER_ROUNDS`); `prereg_n06_13_2.rs` already pins only its own
      five. No test of a Java half moved: the cutout tests set their switches explicitly
      (`n06_cutout_reach.rs`, `n06_area_splits.rs`, `cutout_invariant.rs`), and the P01.3
      microbench's work counts are unchanged (`work_counts_of_issue433` passes as it was).
    - [x] `legacy` unchanged: `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release
      -p freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
      --test-threads=1` under `ulimit -v 8000000` (private target): 7 boards, all `exact`, all
      `byte_equal`, all `pass` (`target/parity/summary.json` in the worktree).
    - [x] Gate (private target, `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`): `cargo fmt --all --check`;
      `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace
      --no-fail-fast`: 267 suites, 2620 passed, 0 failed, 56 ignored (first run, no test needed a
      fix); `python3 tools/roadmap/checklists.py status --strict` clean.

- [x] **N06.14 — Simplex cutouts outside N06.4's domain:** `cutout_hole_crosses_border`,
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
      keep octagons and boxes, the plain (any-angle) tree keeps simplexes. (Wrong for the 45
      degree tree's traces: see "Reach corrected 2026-10-20" below.)
      `tests/n06_cutout_reach.rs::drill_page_splits_report_both_counters_in_both_arms`: pass 1
      of `Issue269-min_fr_test` reaches neither (all arms); pass 1 of
      `Issue143-rpi_splitter-any` reaches them 6 and 578 times under Java (0 and 659 with both
      fixes). While choosing that board, pass 1 of `Issue026-J2_reference-any` gave 14 and 512,
      `scoring-mixed-layer-any` 0 and 0, and the 45 degree fixtures Issue413-test,
      Issue433-my-board, TestSensel, Issue143-rpi_splitter, Issue954-polygon64 and
      Issue026-J2_reference 0 and 0. So the switches are reachable and are measured, not
      accepted on tests. (The claim written here, "in any-angle mode only", was wrong.) `unwired_area_splits_corpus_inventory` reports the
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
    - [x] Amendment 1 (2026-10-18, before any N06.14 route): every `Area` split of the board now
      follows the cutout switches (N06.4's "Wired at every `Area` split" bullet), so the engine
      this registration names reaches further. `docs/roadmap/prereg/N06.14.json` sha256 now
      `2255839225cbdf88e97d654df5157edcf46bb14cf7c431559094fee1f1f64d6a`, re-pinned in
      `crates/freerouting-dev/tests/prereg_n06_14.rs` (6 tests, pass).
      - Engine: the commit that lands the follow-up (on `490d177`); every arm sets
        `octagon_box_cutout_branch3` explicitly.
      - A fifth arm, `octagon_box_cutout_branch3` (`false`, both N06.14 keys `true`, its baseline
        `baseline`), re-measures N06.4's accepted switch at the wider reach. Its pair
        (`baseline`, `octagon_box_cutout_branch3`) is reported and judged on its own by the same
        totals rule (`--arms baseline,octagon_box_cutout_branch3`).
      - Hard guards: `via_in_pad` and `below_minimum` are "no new breach versus baseline" for
        every pair, as N06.4's amendment 2 defines them (the baseline breaks the raw
        `below_minimum` by design, through the pin neck-down).
      - Rows: `fixtures/Issue756-tomu-fpga8.dsn` and `-fpga9.dsn` join `tools/bench/sets/n06-g2.txt`
        (45 degree identity checks for N06.14's own arms). They are the only corpus boards where
        the newly wired splits reach a switch: branch 3 in 1 and 6 stored-area cutouts (N06.4's
        corpus inventory). Counted for this amendment (`N06_CUTOUT_DSNS`, load only): the five
        old rows, canary-pwr's fixture and its plugin export
        (`target/canary-handoff/2026-10-18-n06-g/pluginpath-9511d9a.dsn`) have 0 stored areas
        with holes, so there the new sites reach no switch.
      - `seen_before_registration` adds N06.4's routed numbers of the branch-3 switch on two
        rows (Issue026-J2_reference 0 → 0 unrouted; canary-pwr 122 → 125 DSN, 41 → 43 plugin)
        and A14's legacy numbers of the new rows (14 and 21 unrouted). No route was run for the
        amendment.
    - [x] Heavy run (done 2026-10-20 under amendment 2, see below) (orchestrator, one job at a time), from main once `land/n06-3` and
      `land/n06-splits` have landed:
      `cd /home/thebo/code/freerouting-rs && CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.14.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-g2`.
      - `n06_run.py` (5 arms × 7 rows) exits non-zero when sessions differ, expected on the
        any-angle rows and for the `octagon_box_cutout_branch3` arm; it writes `summary.json`
        with every run's counters first.
      - Then grade each SES with `gate collect` and `freerouting-dev widths`, and judge the
        verdict pair with `freerouting-dev gate --prereg docs/roadmap/prereg/N06.14.json` and
        amendment 1's pair with `freerouting-dev gate --prereg docs/roadmap/prereg/N06.14.json
        --arms baseline,octagon_box_cutout_branch3`; then the plugin canary run and grade, per
        the registration's `plugin_canary` block.
      - Hold (2026-10-19, N06.16): the fix exists (`any_angle_room_side_index = false`), but the
        hold is not lifted by it. Every N06.14 arm leaves that key unset (`true`, Java's unbounded
        path), so the two any-angle rows still run out of memory. Lifting it needs an amendment of
        `prereg/N06.14.json` that sets `router.java_quirks.any_angle_room_side_index=false` in
        every arm (the same value in all arms), or drops the two rows as N06.12's amendments 2
        and 3 did. With the key `false`, each row routed under a 4 GB cap in a check of 4 runs
        whose peak RSS and exit status alone were read (default profile; N06.14's keys and
        `octagon_box_cutout_branch3` all `true`, then all `false`): at most 57 MB, exit 0.
        Lifted 2026-10-20 by amendment 2 (the key `false` in every arm).
      - Also the corpus inventory (read-only):
        `cd /home/thebo/code/freerouting-rs && N06_CUTOUT_COUNTERS_OUT=/home/thebo/code/freerouting-rs/target/orchestrator/n06-g2-corpus.json CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo test --release -p freerouting --test n06_cutout_reach unwired_area_splits_corpus_inventory -- --ignored --test-threads=1`.
    - [x] Amendment 2 (2026-10-20, after the build and the plugin export, before any N06.14
      route): `docs/roadmap/prereg/N06.14.json` sha256 now
      `987fbb62c5bac23da88584445f41aee20ccb151cd5b7e818272d66f682f8f431`, re-pinned in
      `crates/freerouting-dev/tests/prereg_n06_14.rs` (7 tests, pass).
      - Engine main `08062e2`, CLI sha256 `ff058860…` (git `08062e2f2615`). The registration named
        the default profile, `enhanced`, "whose ENHANCED table does not touch java_quirks"; since
        `e0610fc` it sets the 30 keys N06.13 promoted. Every arm now gives
        `--router.profile=enhanced` explicitly. N06.9's and N06.15's accepted switches are not
        promoted, so they are Java's in every arm; no cutout key is in `ENHANCED`.
      - Every arm sets `--router.java_quirks.any_angle_room_side_index=false` (decided 2026-10-20;
        the same value in all arms) and `simplex_prev_division_line` explicitly: `false` only in
        the `octagon_box_cutout_branch3` arm, which so measures N06.4's two switches together.
        `simplex_prev_division_line_reached` and `any_angle_room_side_mismatches` join the report
        metrics.
      - Plugin row `canary-pwr-plugin` in `boards.files` (hashed as its export) and
        `boards.outside_runner`, band 10. Export: canary `9511d9a` (git archive, board sha256
        `15aa06a3…`), `canary-graded-flow.json`, kicad-plugin at `08062e2`, WSL KiCad 10.0.7: DSN
        `e6297c7d…`, args `c6351b8d…` (`--max-passes 1 --router.min_trace_width_um=150
        --router.scoring.start_ripup_costs=1500 --router.autorouter.sweep=free
        --router.autorouter.sweep_passes=2`). N06.4's export of the same source (`77302b7d…`)
        differs: the plugin changed since.
      - Hard guards `via_in_pad` and `below_minimum` stay option (b), no new breach versus
        baseline; the guards `clearance_violations` (DSN rows) and `drc_real` (plugin row) are no
        new breach versus baseline. Verdict pairs as registered: `gate --prereg` for (`baseline`,
        `all`), `--arms baseline,octagon_box_cutout_branch3` for amendment 1's pair.
      - `seen_before_registration`: N06.13's confirmation had routed this baseline's configuration
        (but for the any-angle key) on J2, canary-pwr's DSN and scoring-mixed-layer-any; N06.16
        had routed J2-any and rpi_splitter-any under `legacy`; no route of these arms at this
        engine was seen.
  - *Partial 2026-10-20:* the registered run is done and graded, but it gives no verdict for the
    two N06.14 switches: the registration's own 45 degree identity check fails on canary-pwr, which
    its `pairs_rule` calls "a bug in N06.14's diff or in its reach claim, not a verdict". The
    branch-3 pair fails its hard guard (N06.4's note below). Worktree
    `target/orchestrator/n06r5/wt` (branch `land/n06-runs5`, main `08062e2`); every route under
    `ulimit -v 8000000`, one at a time. Remaining (2026-10-20): the reach claim is corrected and
    a new measurement is registered (`docs/roadmap/prereg/N06.14b.json`, below); its heavy run
    is pending.
    - Runs: `N06_RUN_ULIMIT_KB=8000000 python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.14.json
      --out target/orchestrator/n06-g2`: 35 runs, all exit 0, exit 1 overall because sessions
      differ. Plugin row: 5 routes with the exported args, exit 0 each. Measuring tool (the engine
      at `08062e2`, input wiring stripped, the SES read at the writer's scale) on every DSN run;
      `via_in_pad` raw 0 on every row of every arm.

      | row | baseline | `cutout_hole_crosses_border` | `simplex_octagon_half_corner` | `all` | `octagon_box_cutout_branch3` |
      |---|---|---|---|---|---|
      | J2-any unrouted / vias / below min. raw | 0 / 10 / 25 | 0 / 10 / 22, differs | 0 / 12 / 15, differs | 0 / 12 / 15, differs | 0 / 10 / 16, differs |
      | rpi_splitter-any | 0 / 2 / 13 | identical | 0 / 2 / 7, differs | 0 / 2 / 7, differs | identical |
      | scoring-mixed-layer-any | 0 / 1 / 0 | identical | identical | identical | identical |
      | Issue026-J2_reference (45°) | 0 / 16 / 37 | identical | identical | identical | 0 / 14 / 36, differs |
      | Issue756-tomu-fpga8 (45°) | 14 / 162 / 0 | identical | identical | identical | identical |
      | Issue756-tomu-fpga9 (45°) | 21 / 163 / 2 | identical | identical | identical | identical |
      | canary-pwr-9511d9a DSN (45°) | 122 / 142 / 33 | identical | 129 / 144 / 29, differs | 129 / 144 / 29, differs | 127 / 134 / **34**, differs |
      | canary-pwr-plugin KiCad unconnected / router unrouted / vias | 39 / 97 / 181 | identical | 44 / 101 / 165, differs | 44 / 101 / 165, differs | 39 / 96 / 193, differs |

      Router clearance is equal across the arms on every DSN row (0; tomu 72; canary 40).
      Wall time on canary's DSN: 430, 433, 431, 409 and 377 s; plugin row 78, 79, 101, 101 and 93 s.
    - Reach counters (`--stats`, counted in both arms), baseline arm → the arm's own:
      `cutout_hole_crosses_border_reached` J2-any 2127 → 2274, rpi-any 425 → 425, canary DSN 109,
      plugin 16, every other row 0; `simplex_octagon_half_corner_reached` J2-any 22833 → 11939,
      rpi-any 14820 → 14852, canary DSN 4686 → 5372, plugin 1552 → 1921, every other row 0;
      `octagon_box_cutout_branch3_reached` J2 1770 → 1589, tomu-fpga9 408 → 408, canary DSN 398259 →
      420225, plugin 101679 → 118538; `simplex_prev_division_line_reached` 0 in all 40 runs;
      `any_angle_room_side_mismatches` J2-any 3, rpi-any 36 (0 in the half-corner arms), 0 on every
      45 degree row.
    - Identity check (registered for J2, tomu-fpga8, tomu-fpga9 and canary-pwr in the four N06.14
      arms: sessions equal to the baseline's, both N06.14 counters 0): holds on J2 and both tomu
      boards; **fails on canary-pwr**, DSN and plugin. Its baseline reaches both switches (109 and
      4686 on the DSN, 16 and 1552 on the plugin row) although the board is 45 degree and
      `any_angle_room_side_mismatches` is 0, so the any-angle neighbour calculation is not used. The
      half-corner fix then changes both sessions. The border fix alone leaves both sessions
      identical. So the reach claim ("only in any-angle mode") is wrong.
    - Grades of the plugin row (plugin import with the same options, canary `grade.sh` and tools at
      `2a7e070b`, `GRADE_REV=9511d9a`, WSL KiCad 10.0.7), each distinct session graded twice on
      separate copies, both grades equal: baseline = border arm (`22de6d8b`) 39 unconnected, 5
      starved_thermal, 0 clearance, 0 shorting_items, 0 track_width, 0 schematic parity, 0
      via-in-pad, 0 inner-layer tracks; half-corner = `all` (`27ff331a`) 44, 5, 0, 0, 0, 0, 0, 0;
      branch 3 (`71dd5fdb`) 39, 5, 0, 0, 0, 0, 0, 0 (plus 1 `track_dangling` warning).
      `drc_real` 5 in every arm. Handoff: `target/canary-handoff/2026-10-20-n06-g2/` (README.txt,
      SHA256SUMS).
    - Gate (`freerouting-dev` at `08062e2`, baseline file written first with `--write-baseline`;
      0 refusals, 8 of 8 rows judged in every pair):
      - (`baseline`, `all`): total 196 → 208 (+12; canary DSN +7, plugin +5), set noise 21.40,
        `totals verdict: accept`, `gate: PASS`. Under the `pairs_rule` this is not a verdict,
        because the identity check failed on canary-pwr.
      - (`baseline`, `cutout_hole_crosses_border`): 196 → 196, PASS. (`baseline`,
        `simplex_octagon_half_corner`): 196 → 208, PASS (the whole of `all`'s change).
      - (`baseline`, `octagon_box_cutout_branch3`): 196 → 201 (+5, canary DSN 122 → 127), but
        `FAILS: hard guard: fixtures/canary-pwr/canary-pwr-9511d9a.dsn.below_minimum = 1` (33 → 34
        traces under the narrowest class width), `totals verdict: reject`, `gate: FAIL`.
    - Corpus inventory (the registered read-only command; 205 DSN files, 203 load,
      `target/orchestrator/n06-g2-corpus.json`): stored-area splits reach branch 3 on
      Issue756-tomu-fpga8 (1) and tomu-fpga9 (6), as N06.4 found. They also reach
      `cutout_hole_crosses_border` on the same two boards (12 and 8). N06.4's inventory had no
      N06.14 columns, so amendment 1's "no other switch on any board" did not cover them. Both tomu
      rows' sessions are identical in every N06.14 arm. No other board reaches any of the four
      switches at those splits.
    - Verdict: **none for `cutout_hole_crosses_border` and `simplex_octagon_half_corner`**: both
      stay `true` in both profiles, and N06.13 does not promote them. Amendment 1's pair
      **fails**, so N06.4's branch-3 acceptance does not carry to the wider reach (decision table:
      operator decision).
  - *Partial 2026-10-20 (reach corrected, re-registered):* worktree `target/orchestrator/n06b/wt`
    (branch `land/n06-14b`, on main `c8b91b7`). No route was run.
    - [x] Reach corrected 2026-10-20. A 45 degree board reaches `Simplex::cutout_from` through
      the tree shapes of traces with a line off 45 degrees. Java at `a773a70ec` does the same.
      - Tree shapes: the 45 degree tree replaces pin, via, area and outline shapes by bounding
        octagons (`ShapeSearchTree45Degree.java:510-563`; `board/search/mod.rs`
        `drill_item_tree_shapes`, `regularize`). Trace shapes come from the base class
        (`ShapeSearchTree.java:1120-1131` → `Polyline.offsetShapes`, `Polyline.java:358-512`;
        `board/search/mod.rs` `tree_shapes_of` → `trace_offset_shape` →
        `geometry/polyline.rs` `offset_shape`). That result simplifies to an octagon only when
        every line is a multiple of 45 degrees (`Simplex.simplify`/`isIntOctagon`,
        `Simplex.java:60-70`, `:365-379`). Any other trace keeps a simplex tree shape.
      - Drill pages: `DrillPage.getDrills` (`DrillPage.java:63-103`) → `PolylineArea.splitToConvex`
        → `TileShape.cutout`. In Rust: `Session::page_drills` → `calculate_page_drills` and
        `page_cutouts` (`autoroute/session/drills.rs`) → `PolylineArea::split_to_convex_counted`
        (`geometry/shapes/polyline_area.rs`) → `TileShape::cutout_counted`
        (`geometry/shapes/tile_shape.rs`, `cutout_from_box`/`_octagon`/`_simplex`) →
        `Simplex::cutout_from_counted`. The cutout calls it when either shape is a simplex:
        Java's `IntBox.cutoutFrom(Simplex)` (`IntBox.java:759-760`) and
        `IntOctagon.cutoutFrom(Simplex)` (`IntOctagon.java:1708-1709`) call `toSimplex` the same
        way. Every piece a simplex hole cuts is a simplex, so the page's later octagon holes
        (pads, vias) go through the simplex code too.
      - Tie-pin cutout: `MazeSearchEngine.java:166-178` → `reduceTraceShapeAtTiePin`
        (`ShapeSearchTree.java:935-965`). In Rust: `autoroute/session/init.rs` →
        `BasicBoard::reduce_trace_shape_at_tie_pin` (`board/basic_board.rs`). Only a trace of
        another net with a simplex tree shape that ends on a pin of more than one net reaches
        it. The four boards checked have 0 such pins.
      - Gates: the tree kind (45 degree for these boards; the 90 degree tree keeps boxes,
        `offsetBox`, so it never gets there) and the trace's angles. The counters count at
        both sites in both arms and are recorded only with statistics on. No switch gates it.
        `octagon_box_cutout_branch3` is not on this path: branch 3 lives in
        `IntOctagon::cutout_from_box_counted` and calls no simplex code, so the lead about it
        is refuted.
      - Sources on the rows: canary-pwr has 5 fixed any-angle segments in its DSN and its plugin
        export (6 simplex tree shapes). tomu-fpga8 has 140 protected any-angle traces (147
        shapes). Amendment 2's canary-pwr baseline SES also has one router-made segment one
        unit off 45 degrees (B.Cu 1500, (1160960,-544113) to (1157596,-540750); its origin was
        not traced). The registered rows and the plugin export have 0 tie pins.
      - Test, new: `crates/freerouting/tests/n06_cutout_reach_45.rs::a_45_degree_board_reaches_the_simplex_cutout_only_through_any_angle_traces`
        (pass, 3.8 s). It computes every drill page of one net's session at load:
        - canary-pwr: 460 pages, 5 holding a simplex shape; reach (`cutout_hole_crosses_border`,
          `simplex_octagon_half_corner`) (0, 14);
        - tomu-fpga8: 448 pages, 40 holding one; (33, 701);
        - J2: 84 pages, none; (0, 0).

        No page without a simplex tree shape reaches either switch, and no non-trace item has
        a simplex shape in the 45 degree tree. A run reaches only the pages its maze search
        computes: amendment 2's tomu-fpga8 run computed 0 drills, and tomu-fpga9 2695 with both
        counters 0.
      - Load-only inventory (`unwired_area_splits_corpus_inventory`, `N06_CUTOUT_DSNS` = the 7
        DSN rows and amendment 2's plugin export, 1 passed): stored areas with holes only on
        tomu-fpga8 (`cutout_hole_crosses_border` 12) and tomu-fpga9 (8). The plugin export has
        none.
      - Docs corrected: `geometry/quirks.rs` (`SplitQuirks`), `tests/n06_cutout_reach.rs`
        header, DIVERGENCES "N06.14 simplex cutouts outside N06.4's domain", and this file's
        two inventory rows.
      - Not changed: `tools/bench/sets/n06-g2.txt`, the old registration's set, which keeps its
        old comment.
    - [x] Registered again before any number under it: `docs/roadmap/prereg/N06.14b.json`
      (sha256 `9417fb2c253a4e54d04406599ee7b2b8742ef64aae1ce4c0c2115fa1c6ef6b6b`), set
      `tools/bench/sets/n06-g2b.txt` (the same 7 DSN rows), pinned by
      `crates/freerouting-dev/tests/prereg_n06_14b.rs` (5 tests, pass).
      - A new file, not amendment 3: `N06.14.json` has numbers, and routing-quality.md does not
        edit arms, cells, signals or guards after a number exists.
      - Engine: main `c8b91b7`. Profile `--router.profile=enhanced`. Every arm sets
        `octagon_box_cutout_branch3=true`, and `false` for `simplex_prev_division_line`,
        `any_angle_room_side_index`, `opt_first_pass_stops`, `opt_equal_key_skip` and
        `shove_via_into_pending_piece` (the same values in every arm).
      - Arms: `baseline` (both N06.14 keys `true`), each key alone `false`, and `all`.
      - Rows: every row is measured, canary-pwr's DSN and plugin rows included.
        `Issue026-J2_reference` is the only identity row. Its predicted counters are 0, and a
        counter above 0 there is reported as a wrong prediction, not a void verdict. A
        consistency check replaces the old identity check: on every row but tomu-fpga8/9, a
        pair whose arms both count 0 must have identical sessions.
      - Hard guards `via_in_pad`, `below_minimum` and guards `clearance_violations`,
        `drc_real`: no new breach versus baseline (option (b)). Rule N02 totals, no-regress.
        Noise: canary-pwr 10, the plugin row 10, others the default.
      - `seen_before_registration` gives amendment 2's numbers: DSN 122 → 129, plugin 39 → 44,
        total 196 → 208 (noise 21.40); counters 109 / 4686 (DSN) and 16 / 1552 (plugin).
    - [x] Heavy run (orchestrator, one route at a time, every route under the cap), as first
      registered: `cd /home/thebo/code/freerouting-rs && CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && N06_RUN_ULIMIT_KB=8000000 python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.14b.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-g2b`
      (exits 1 when sessions differ, after writing `summary.json`), then the plugin row:
      `cd /home/thebo/code/freerouting-rs && OWN_FLAGS=9 N06_RUN_ULIMIT_KB=8000000 python3 target/orchestrator/n06r5/route_plugin.py docs/roadmap/prereg/N06.14b.json target/orchestrator/n06r5/plugin arms-14b`
      (the exported DSN `target/orchestrator/n06r5/plugin/work/canary-pwr.dsn`, sha256
      `e6297c7d…`, and its args `c6351b8d…`).
      - Then grade as amendment 2 did: the measuring tool (`n06r5/n06m_all.sh`
        `/home/thebo/code/freerouting-rs/target/orchestrator/n06-g2b`) and `freerouting-dev
        widths` on every DSN run. Grade each distinct plugin session twice
        (`n06r5/grade.sh <arm>/output.ses <dir>`). Build the records with
        `n06r5/build_records.py docs/roadmap/prereg/N06.14b.json …`.
      - Then `freerouting-dev gate --records <r> --baseline <b> --prereg
        docs/roadmap/prereg/N06.14b.json --write-baseline` first. After that, the verdict pair
        without `--arms`, and `--arms baseline,cutout_hole_crosses_border` and `--arms
        baseline,simplex_octagon_half_corner`. Check the consistency rule from the two
        summaries.
      - `n06m/Cargo.toml` and `grade.sh` point at `target/orchestrator/n06r5/wt`, which is gone.
        Recreate it first with `git worktree add --detach target/orchestrator/n06r5/wt <landed
        commit>`, and rebuild `n06m` into `n06-target`.
    - [x] Amendment 1 (2026-10-20; decided 2026-10-20: measurement runs stay a few minutes, Java
      compatibility ranks below short measurement cycles): `docs/roadmap/prereg/N06.14b.json`
      sha256 now `68f90bf5e1acb3b812115244541edfcf3d0a03ecd7d68c8c6f4465b56195acb7`, re-pinned in
      `crates/freerouting-dev/tests/prereg_n06_14b.rs` (5 tests, pass).
      - Two arms, `baseline` and `all` (both N06.14 keys `false`); their commands are unchanged.
        The one-switch arms and their reported pairs are dropped, so the verdict covers both
        switches together. Rows unchanged: every DSN row but canary-pwr's routes in under 30 s
        (tomu-fpga9 29.0–29.7 s, the slowest), and canary-pwr's DSN and plugin rows stay
        measured. Each distinct plugin session is graded once.
      - Written after numbers existed, and recorded in `seen_before_registration`: the run as
        first registered was started twice (four arms). The first start was cut off by a full
        disk after 24 of 28 routes. The second start was stopped on the amendment's instruction
        after the same 24 routes. All 24 sessions were byte-identical between the two starts. The
        numbers read are listed in `seen_before_registration`. No plugin route or grade ran
        before the amendment.
  - *Done 2026-10-20 (N06.14b, amendment 1):* worktree `target/orchestrator/n06d/wt` (branch
    `land/n06-14c`, main `a25c82f`; engine and CLI at `c8b91b7`).
    - CLI `target/orchestrator/n06-target/release/freerouting`, sha256 `95d2ada2…` (`freerouting
      0.1.0 (git c8b91b7d75f4)`), the release build of `c8b91b7` copied from
      `target/orchestrator/n06-13b-base/release/`. `freerouting-dev` (`48a2db10…`) and `n06m`
      (`9ad443bd…`) were built from `target/orchestrator/n06r5/wt`, recreated detached at
      `c8b91b7`.
    - Runs: `N06_RUN_ULIMIT_KB=8000000 python3 tools/bench/n06_run.py
      docs/roadmap/prereg/N06.14b.json --out target/orchestrator/n06-g2b` gave 14 runs, all exit
      0, and exit 1 overall because sessions differ. Plugin row: `OWN_FLAGS=9 route_plugin.py …
      arms-14b`, 2 routes, exit 0 each. The plugin sessions are `22de6d8b` (baseline) and
      `27ff331a` (`all`), byte-identical to N06.14 amendment 2's at `08062e2`. Measuring tool
      (`n06m`, input wiring stripped) on every DSN run: `via_in_pad` raw 0 on every row.

      | row | `baseline` unrouted / vias / below min. raw / clearance | `all` |
      |---|---|---|
      | J2-any | 0 / 10 / 25 / 0 | 0 / 12 / 15 / 0, differs |
      | rpi_splitter-any | 0 / 2 / 13 / 0 | 0 / 2 / 7 / 0, differs |
      | scoring-mixed-layer-any | 0 / 1 / 0 / 0 | identical |
      | Issue026-J2_reference (45°, identity row) | 0 / 16 / 37 / 0 | identical |
      | Issue756-tomu-fpga8 | 14 / 162 / 0 / 72 | identical |
      | Issue756-tomu-fpga9 | 21 / 163 / 2 / 72 | identical |
      | canary-pwr-9511d9a DSN | 122 / 142 / 33 / 40 | 129 / 144 / 29 / 40, differs |
      | canary-pwr-plugin (KiCad unconnected / router unrouted / vias) | 39 / 97 / 181 | 44 / 101 / 165, differs |

      Wall time: canary DSN 441.7 and 412.2 s, plugin row 78.5 and 99.7 s, every other route
      at most 29.0 s.
    - Reach counters (`--stats`), `baseline` → `all`:
      - `cutout_hole_crosses_border_reached`: J2-any 2460 → 28, rpi-any 455 → 20, canary DSN
        109 → 384, plugin 16 → 106, every other row 0.
      - `simplex_octagon_half_corner_reached`: J2-any 26710 → 13175, rpi-any 15867 → 16072,
        canary DSN 4686 → 5372, plugin 1552 → 1921, every other row 0.
      - `octagon_box_cutout_branch3_reached`: canary DSN 398259 → 413484, plugin 101679 → 128803.
      - `simplex_prev_division_line_reached`: 0 in all 16 runs.
      - `any_angle_room_side_mismatches`: J2-any 3 → 3, rpi-any 40 → 0, 0 on every 45 degree row.
    - Grade (plugin import with the same options, canary `grade.sh` with tools at `2a7e070b`,
      `GRADE_REV=9511d9a`, WSL KiCad 10.0.7, one grade per session, run only while no other
      `kicad-cli` was running):
      - `22de6d8b`: 39 unconnected, 5 starved_thermal, 0 clearance, 0 shorting_items,
        0 track_width, 0 via-in-pad, 0 inner-layer tracks;
      - `27ff331a`: 44, 5, 0, 0, 0, 0, 0.

      `drc_real` is 5 in both arms. The grades equal amendment 2's two grades of the same
      sessions.
    - Gate (`freerouting-dev` at `c8b91b7`). The baseline file was written first with
      `--write-baseline`. Then `gate --records … --baseline … --prereg
      docs/roadmap/prereg/N06.14b.json` reported `1 amendment(s): 0 refusal(s)`, `judged 8 of 8
      row(s)`:
      - total 196 → 208 (+12; canary DSN 122 → 129 in band 10, plugin 39 → 44 in band 10, the
        other six rows 0 change); set noise 21.40;
      - `totals verdict: accept`, `gate: PASS`;
      - hard metrics `via_in_pad` and `below_minimum` 0 → 0 on every row, and guards
        `clearance_violations` and `drc_real` equal on every row.
    - Consistency check (`pairs_rule`) holds. The rows outside tomu-fpga8/9 where both arms
      count 0 for both switches are scoring-mixed-layer-any and J2, and both have
      `session_differences` 0. The J2 identity prediction holds: counters 0 in both arms,
      identical sessions. tomu-fpga8/9 are also identical.
    - Verdict: **both switches accepted together** (the decision table's "pass"). N06.13's table
      records both as accepted. Promotion into `ENHANCED` (N05, with their DIVERGENCES "Profile
      enhanced" lines) is a separate N06.13 step, and both keys stay `true` in both profiles
      until then. No switch was measured alone; per amendment 1, measuring either alone needs
      a new registration.
    - Accept clauses: the invariant and reach clauses are the `[x]` items above. The
      registration came before any number (N06.14b as registered). Its amendment 1 came after
      the numbers listed in it (decided 2026-10-20). The measurement under the accept rule is
      this run.
- [x] **N06.15 — The shove bug N03.2 found:** `shove_via_into_pending_piece` (the last (b) row). A
  deeper level of `TraceShover.insert`'s recursion moves a via of another net (`shoveVias`) onto a
  substitute piece that is not on the board yet; the piece is then inserted over it, a short. Make
  N03.2's env-gated prototype (`target/orchestrator/n03/scratch/n03-2-fix-prototype.patch`) a switch.
  - **Accept:** the row, the key (`SWITCH_COUNT`, the settings docs), the `false` path with
    `// PARITY(quirk, java_quirks.shove_via_into_pending_piece)` on Java's, a both-behaviour test, a
    reach counter that only observes, the trace case investigated, and the registration under
    `legacy` and `enhanced` (N03.3 reads the `enhanced` answer); the fix is measured.
  - *Partial 2026-10-18:* code, tests, counters and the registration are done; heavy run pending
    (commands below). Defaults stay `true` (Java) in both profiles; `ENHANCED` is unchanged.
    - [x] Row and key: the (b) row above; `router.java_quirks.shove_via_into_pending_piece`
      (`settings/fields.rs`, its doc string is the settings documentation; `settings/java_quirks.rs`,
      owning item N06.15), `SWITCH_COUNT` 44 → 45 (`tests/settings_java_quirks.rs`, 9 tests pass:
      file, command line and environment round trips include the new key). `--help` lists no
      quirk keys, so it is unchanged. DIVERGENCES: the kept bug under "Shove insert" and an
      "N06.15 shove fix" block in "Java quirk switches, opt-in (N06)".
    - [x] The `false` path (`shove::insert`, `insert_substitute_pieces`): the prototype's re-query
      without its env var. Before a piece goes on the board its shapes (the `ShapeAndEntrySide`
      shapes its own inserts cleared) are queried again with its clearance, in order, on the work
      meter; the first via of another net found fails the shove, as the shove-failing obstacle.
      It covers `forcedPad`'s pieces too (the same tail, `ForcedPadRouter.java:413-438`). Java's
      path keeps the marker; the module docs name the bug.
    - [x] Reach counters (`run_stats`, `--stats json` `total.counts`, either arm, only with
      statistics on): `shove_via_into_pending_pieces` (pieces with such a via) and
      `shove_pending_piece_foreign_traces` (pieces whose shapes overlap the copper of a trace of
      another net). Under `true` they query through the tree's reference traversal, which leaves
      the work meter alone (`overlapping_items_with_clearance_unmetered`, new in
      `board/basic_board.rs` and `board/search/mod.rs`), so no work limit cuts off elsewhere;
      Issue508-DAC2020_bm01 (`legacy`, 3 passes, optimizer off) writes a byte-identical SES with
      and without `--stats json`.
    - [x] Both behaviours: no small board was found where a shove moves a via onto a pending
      piece. It needs a cascade of pushes (GP8B: depths 16 to 19) with a via moved back onto an
      ancestor's piece. Random forced inserts on synthetic two-layer boards (118,000, dense
      trace bundles, bends, vias), 400 on each of 80 probe boards with their sessions and 3,000
      on each of four dense ones, the probe's stages (200 steps) on 30 probe boards with and
      without their sessions, and three-pass `legacy` routes of 15 A14 boards never reached it
      (counter 0); only GP8B's routes do. So the test is unit
      level, on the board state at the re-query: `shove::insert::pending_tests::pending_piece_switch_both_behaviours_and_counters`
      (a piece over a foreign via: `true` inserts, `false` returns the via, both count 1; within
      the clearance only: the same; an own-net via or nothing: neither; a foreign trace: counted
      only on copper overlap) and `pending_piece_counting_leaves_the_work_meter_alone` (Java's
      arm adds 0 work units with statistics on and does not query with them off; the fix
      queries on the meter). End to end, `reference_shove_insert.rs::shove_insert_with_the_pending_piece_fix_matches_java_where_no_via_arrives`:
      with the switch `false` the probe's J2 (200 steps) and bm08 steps still match Java's
      recording.
    - [x] The trace case: not covered. A clearance re-query for traces of another net fires on
      pieces that deeper levels placed against the pending piece at exactly the clearance
      (1,076 pieces on Issue508-DAC2020_bm01, 117 on Issue593-BBD_Mars-64, in three `legacy`
      passes, whose final boards have 0 violations), so failing on it would fail legitimate
      shoves. Copper overlap is counted instead: 0 on those two boards, 1 on
      Issue015-StackOverflow, 1 on GP8B `legacy` (both arms). Recorded in the row as the
      remaining case.
    - [x] The switch reproduces the prototype on GP8B (release, `fixtures/Issue066-Project_GP8B.dsn`,
      `--router.autorouter.max_passes=3 --router.optimizer.enabled=false`, one run per arm, one at
      a time, 18:13:30 to 18:33:50): `legacy` `true` 61 unrouted, 0 violations, counter 2;
      `false` 59 unrouted, 0 violations (N03.2's prototype: 61 → 59); `enhanced` `true` 56
      unrouted (pass 1 ends with 1 violation, gone by pass 2, as N03.2 saw), 0 violations, counter
      1; `false` 52 unrouted, pass 1 0 violations, 0 at the end. These four were seen after the
      registration was written.
    - [x] Registered before any number of the switch: `docs/roadmap/prereg/N06.15.json` (sha256
      `397fd09cc28f80dc88e778adef33e0d9a8d8e059b21896f841aca6c9e2ceb663` after its one amendment,
      which rewrote `pairs` into the array form main `4d44676`'s gate reads, same two pairs) and
      `tools/bench/sets/n06-pending-piece.txt` (GP8B, Issue145-smoothieboard, Issue113-Protein,
      Issue269-z10_module, Issue753-CPU-85_r104, Issue508-DAC2020_bm01, Issue593-BBD_Mars-64, plus
      canary-pwr at canary 2a7e070b through the plugin path), pinned by
      `crates/freerouting-dev/tests/prereg_n06_15.rs` (4 tests pass). Arms `legacy_baseline`,
      `legacy_fix`, `enhanced_baseline`, `enhanced_fix`, P04's bounds on the DSN rows; two judged
      pairs (the `pairs` array, `gate --prereg --all-pairs`), one per profile, each by N02's totals rule, no-regress, bands max(2, 5 % of
      connections) and 10 for canary-pwr; router `clearance_violations` and KiCad
      `shorting_items`/clearance are hard guards (any worsening rejects). The file names what was
      seen before it: N03.2's GP8B `legacy` prototype numbers (canonical 51 → 64, tree 61 → 59
      unrouted) and the reach counter's 0 on bm01 and BBD_Mars-64. `tools/bench/n06_run.py` now
      compares an arm with its own `baseline` arm when it names one, and copies each run's
      `connections` (the band's input) into `summary.json`; a smoke run of the four arms on
      `fixtures/scoring-perfect-two-pin.dsn` wrote them.
    - [x] Heavy run pending (orchestrator, from the worktree root, release build of this tree):
      `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/n06-target cargo build --release -p freerouting-cli && python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.15.json --out /home/thebo/code/freerouting-rs/target/orchestrator/n06-v`
      (28 runs; it exits non-zero after the last run when a session differs, and then the totals
      rule applies to `summary.json`'s `statistics` per verdict pair). Then the canary-pwr plugin
      row as in N06.10 on canary 2a7e070b (the registration's `plugin_canary`: export once with
      `canary-graded-flow.json`, route the 4 arms with the exported args plus the arm's
      `--router.profile` and switch flag, grade each distinct session with canary's `grade.sh`,
      `GRADE_REV=2a7e070b`, recording `shorting_items` and clearance). N06.13 records the verdict.
    - Gate (worktree `target/orchestrator/n06v/wt`, branch `land/n06-15` on main `03eaf07`, private
      target, `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`): `cargo fmt --all --check`;
      `cargo clippy -p freerouting -p freerouting-dev -p freerouting-cli --all-targets -- -D warnings`;
      `cargo test -p freerouting -p freerouting-cli --no-fail-fast` 1577 passed, 0 failed, 33
      ignored (155 suites); the N06 pin tests `prereg_n06_{4,5,6,7,8,9,12,14,15}` 37 passed;
      `python3 tools/roadmap/checklists.py status --strict` clean.
  - *Done 2026-10-20:* the registered heavy run (heavy slot; worktree
    `target/orchestrator/n06r4/wt`, branch `land/n06-runs4` on main `ed603f5`; one Linux release
    CLI at the path the arms name, sha256 `14712a93…`, `--version` git `ed603f538d3c`; every route
    under `N06_RUN_ULIMIT_KB=8000000` or `ulimit -v 8000000`, one route and one kicad-cli at a
    time).
    - Amendment 2, written after the build and the plugin export and before any registered route
      (sha256 now `04376de54af271cdb1e5bb58a69de0bc627be3a9036df2f935c82f5b9b8efe6e`, re-pinned in
      `prereg_n06_15.rs`, whose input test now leaves out `boards.outside_runner`; 4 tests pass):
      - The engine moves to main `ed603f5`. Since N06.13 (main `e0610fc`) `ENHANCED` sets 30
        `java_quirks` keys to `false`, so the `enhanced` arms measure the switch on top of those
        30 fixes, which is the enhanced the fix would join. The arms already gave
        `--router.profile`; their commands are unchanged. `legacy` is unaffected.
      - The plugin row `canary-pwr-plugin` joins `boards.files`, hashed as its export (`155313a1…`:
        canary 2a7e070b, `canary-graded-flow.json`, kicad-plugin at `ed603f5`, args
        `ed73bb12…`), and `boards.outside_runner`.
      - Hard metrics are option (b), no new breach versus each arm's own baseline arm. The DSN
        rows are read with the scratch tool `target/orchestrator/n06r4/n06m` on the engine at
        `ed603f5`.
      - The verdict comes from `gate --prereg --all-pairs`, after the reachability clauses.
      - No row is any-angle, so `any_angle_room_side_index` keeps its default in every arm.
      - `seen_before_registration` is unchanged. The amendment names N06.13's routes of z10_module
        and of a default-options canary export under enhanced at the default bounds.
    - `n06_run.py`: 28 runs, all `COMPLETED`, none at the cap. It exits 1 because three sessions
      differ from their baseline: GP8B `legacy_fix`, Protein `legacy_fix` and Protein
      `enhanced_fix`. The numbers below come from the router's manifests. Counters are
      `shove_via_into_pending_pieces` / `shove_pending_piece_foreign_traces` per arm, in the order
      legacy baseline, legacy fix, enhanced baseline, enhanced fix:

      | row | connections (band) | legacy unrouted | enhanced unrouted | vias (4 arms) | via counter | trace counter | router clearance |
      |---|---|---|---|---|---|---|---|
      | Issue066-Project_GP8B | 759 (38) | 61 → **59** | 48 → 48 | 690, 691, 695, 695 | 2, 1, 0, 0 | 1, 1, 0, 0 | 0 in all arms |
      | Issue145-smoothieboard | 793 (40) | 19 → 19 | 15 → 15 | 488, 488, 481, 481 | 0 | 0 | 42 in all arms |
      | Issue113-Protein | 444 (23) | 78 → **87** | 89 → **96** | 250, 246, 263, 264 | 1, 1, 2, 3 | 0 | 0 in all arms |
      | Issue269-z10_module | 237 (12) | 37 → 37 | 37 → 37 | 129, 129, 131, 131 | 0 | 0 | 10 in all arms |
      | Issue753-CPU-85_r104 | 569 (29) | 57 → 57 | 57 → 57 | 183, 183, 167, 167 | 0 | 0 | 76 in all arms |
      | Issue508-DAC2020_bm01 | 195 (10) | 24 → 24 | 21 → 21 | 126, 126, 132, 132 | 0 | 0 | 0 in all arms |
      | Issue593-BBD_Mars-64 | 106 (6) | 33 → 33 | 30 → 30 | 126, 126, 130, 130 | 0 | 0 | 0 in all arms |

      GP8B's `legacy` arms repeat the light run at registration (61 → 59, counter 2). The
      `enhanced` arms no longer reach the branch on GP8B: the 30 fixes change its route, so it
      ends at 48 unrouted in both arms, with the counter at 0. Hard metrics: `via_in_pad` raw is 0
      in every run. The raw count of traces below the narrowest class width falls or stays in
      every fix arm (GP8B 267 → 257, Protein 98 → 86 and 106 → 80, the others equal), so option
      (b) is 0 everywhere.
    - Plugin row (canary 2a7e070b's board, sha256 `17b597a0…`, exported once; each arm's CLI
      runs with the exported args, then its `--router.profile` and switch flag,
      `target/orchestrator/n06r4/route_v.py`). Each fix arm writes its baseline's session:
      `legacy` gives `0722867c…` (router: 80 unrouted, 133 clearance, 193 vias, 41 s) and
      `enhanced` gives `a6b070cd…` (77 unrouted, 133 clearance, 195 vias, 53–55 s). Both
      counters are 0 in all four arms.
    - Grades of the two sessions: plugin import with the same options, then canary `grade.sh` with
      tools at 2a7e070b and `GRADE_REV=2a7e070b`, on WSL KiCad 10.0.7. Each session was imported
      and graded twice on separate copies, and both grades agree.
      - `legacy`: 14 unconnected, 4 starved_thermal, 0 clearance, 0 shorting_items, 0
        `track_width`, 0 schematic parity, 0 via-in-pad, 0 inner-layer tracks.
      - `enhanced`: 15 unconnected, 4 starved_thermal, 0 clearance, 0 shorting_items, 0
        `track_width`, 0 parity, 0 via-in-pad, 0 inner-layer tracks.
      - Handoff: `target/canary-handoff/2026-10-20-n06-v/` (README.txt, SHA256SUMS).
    - Gate, run from the worktree with `freerouting-dev` built at `ed603f5`: `freerouting-dev gate
      --records target/orchestrator/n06r4/records-v.json --baseline
      target/orchestrator/n06r4/baseline-v.json --prereg docs/roadmap/prereg/N06.15.json
      --all-pairs`. The records come from `target/orchestrator/n06r4/build_records_v.py`, and the
      baseline file was written first by the same command with `--write-baseline`. Result: 0
      refusals, 8 of 8 rows judged per pair, `gate: PASS`.
      - `legacy_baseline → legacy_fix`: GP8B improve, Protein regress (+9, band 23), the others
        neutral. The total is 323 → 330 (unrouted 309 → 316, plugin KiCad unconnected 14 → 14)
        against a set noise of 69.24. PASS.
      - `enhanced_baseline → enhanced_fix`: Protein regress (+7), the others neutral. The total is
        312 → 319 (unrouted 297 → 304, plugin 15 → 15), noise 69.24. PASS.
      - No row is worse than twice its band, and the hard guards hold: no DSN row's router
        clearance rises, and the plugin row's shorting_items, clearance and `drc_real` (4 → 4)
        are unchanged.
    - Verdict:
      - Reachability: the counter is above 0 on GP8B (`legacy`) and Protein (both profiles), and
        those sessions differ. So the totals rule decides, and both pairs pass as a no-regress
        correctness fix.
      - The cost is Protein's 7 to 9 more unrouted connections, within its band. Under
        `enhanced`, GP8B no longer reaches the branch.
      - `shove_via_into_pending_piece` is **accepted for N06.13** under `enhanced`, which is
        N03.3's answer; the `legacy` pair passes too, and `legacy` keeps the switch `true`.
        `ENHANCED` is unchanged here.
- [x] **N06.16 — Any-angle memory blow-up:** `any_angle_room_side_index` (the (b) row added above).
  A `legacy` route of A07's `Issue026-J2_reference-any` reached 5.4 GB at 11 s and two uncapped
  attempts took the WSL machine down (N06.12's amendment 2); `Issue143-rpi_splitter-any` aborted
  at once (amendment 3).
  - **Accept:** the stage, function and data structure named; Java's behaviour on both boards
    recorded from a Java run at the pin; a bounded fix behind a switch (`true` = Java) with a
    both-behaviours test, a counter in both arms and a bounded regression test on the smallest
    reproducing case; defaults unchanged; the small parity check's exact boards byte-identical.
  - *Done 2026-10-19:* worktree `target/orchestrator/anymem/wt`, branch `land/anyangle-mem` on
    main `fed332f`; every route and test of these boards ran under `ulimit -v 4000000`.
    - Cause: `autoroute/session/neighbours_any.rs`, `gap_rooms_any` (the loop of
      `SortedRoomNeighbours.calculateNewIncompleteRooms`, Java :555-648) never ends, and each
      lap pushes an incomplete room (and a door) onto the session's `rooms` vector until an
      allocation fails (`RawVec::grow_one` under `calculate_neighbours_any` ←
      `add_complete_room` ← `complete_expansion_room` ← `calculate_expansion_rooms` ←
      `page_drills` ← `find_connection` ← `fanout_pin`, by gdb at the abort). The room is an
      `IntOctagon` (a triangle, J2: x ≥ 1189922, y ≥ -982651, x + y ≤ 335144), 8 border lines;
      the neighbours' sides come from `roomShape.touchingSides` on those 8 (Java :189, 253, 288,
      296), the walk indexes `roomShape.toSimplex()` (Java :511), 3 lines. A neighbour on the
      diagonal has side 3; `prevNo` on 3 lines never returns 3, so `current == first_side`
      never holds. rpi: a 4-line simplex, side 5, the same loop.
    - Java at `a773a70ec` (frozen jar `freerouting-a773a70ec.jar`, `-Xmx4g`, default settings,
      one run at a time): J2-any logs `Simplex.corner: no must be less than lines.length - 1`
      (Java clamps the index) in the fanout and ends in `OutOfMemoryError` ("thrown from the
      UncaughtExceptionHandler"), 4.56 GB RSS, killed at 1:55 after the job thread died at
      1:00. rpi-any routes (pass 1 complete, score 1000), runs optimizer passes 1 and 2 and
      ends in `OutOfMemoryError: Java heap space` in pass 3, exit 1 at 0:59, 4.56 GB. The port
      fails at the same stages (J2: fanout pass 1; rpi: optimizer pass 3), so this is Java's own
      behaviour, kept under `true`.
    - Switch: `router.java_quirks.any_angle_room_side_index` (`settings/fields.rs`,
      `settings/java_quirks.rs`, `fix_item` N06.16, `IDS` now 46). `false` takes the touching
      sides, the point-touch sides, the first and last corners and the neighbour sort on the
      room's simplex (`side_simplex`, marker `PARITY(quirk, java_quirks.any_angle_room_side_index)`),
      so every side indexes the lines `tryRemoveEdge` and the walk use; a debug assertion in
      `gap_rooms_any` checks the fix's bound (at most `n + 1` steps). Counter
      `any_angle_room_side_mismatches` (`run_stats.rs`, both arms): neighbour calculations whose
      room has more border lines than its simplex.
    - Tests: `neighbours_any::tests::room_side_index_switch_both_behaviours` (the J2 triangle
      scaled down: octagon side 3 past the 3-line simplex, which `prev_no` never reaches, under
      `true`; simplex side 1, the same line, under `false`); `tests/n06_any_angle_rooms.rs::any_angle_fanout_pass_stays_bounded_with_the_fix`
      (fanout pass 1 of J2-any under `false`: 39 connections, 37 routed, 2,210 rooms completed,
      limit 20,000, 3 side mismatches, 0.61 s); `tests/settings_java_quirks.rs` 11 pass
      (`SWITCH_COUNT` 46); `tests/n06_cutout_reach.rs` 1 pass. The `true` arm is not run in a
      test: it is unbounded.
    - Peak RSS, release CLI, `--router.profile=legacy`, 4 GB cap: J2-any 3.40 GB and abort
      (`memory allocation of 939524096 bytes failed`) at 5.2 s under `true`; 56 MB, 5.8 s, exit
      0, 0 incomplete, 0 violations under `false`. rpi-any 3.53 GB and abort at 2.6 s (optimizer
      pass 3) under `true`; 49 MB, 1.65 s, exit 0, 0 incomplete under `false`.
      `scoring-mixed-layer-any`: identical SES in both arms, 9 MB, 0 mismatches. A dev build
      (debug assertions on) routed both boards under `false`: 54 MB and 58 MB, no panic.
    - Defaults unchanged: unset is `true` in both profiles, and the `true` path computes the
      same shapes as before. DIVERGENCES: "N06.16 any-angle room sides" under "Java quirk
      switches, opt-in (N06)". Promotion needs a registered measurement (N06.13).
    - Parity, release, under the cap: `PARITY_SET=tools/bench/sets/connection-parity.txt
      PARITY_MAX_JAVA_S=20 cargo test --release -p freerouting --test autoroute_board_parity --
      --ignored autoroute_board_parity --test-threads=1`: 7 boards, all `exact`, all
      `byte_equal` (`target/parity/summary.json`). The default path is unchanged, so the 60-board
      set was not run.
    - Gate: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`;
      `cargo test --workspace --no-fail-fast -- --test-threads=2` under the cap: 2,533 passed,
      0 failed, 55 ignored; four suites that route none of these boards (`reference_drc` and
      the GUI's `ui_render`, `ui_viewer`, `ui_windows`) abort on the address-space cap itself
      and pass without it (4, 7, 5 and 19 tests).
    - [x] Measured 2026-10-20 for promotion (worktree `target/orchestrator/n06r5/wt`, branch
      `land/n06-runs5`, main `08062e2`; CLI sha256 `ff058860…`). It was registered before any number
      under it: `docs/roadmap/prereg/N06.16.json` (sha256
      `9dad8e7a0c826d08d33b9667325456af6a201205ead4a2b1803c6b4498a08753`, 0 amendments),
      `tools/bench/sets/n06-16.txt`, pinned by `crates/freerouting-dev/tests/prereg_n06_16.rs`
      (6 tests, pass). It was written and pinned while N06.14's run was routing this fix arm's
      configuration, before any of that run's numbers was read.
      - Arms: `baseline` (`true`, Java) and `fix` (`false`), both `--router.profile=enhanced` (the
        30 promoted fixes), the key set explicitly.
      - Rows: the three `*-any.dsn` variants (J2, rpi_splitter, scoring-mixed-layer); identity rows
        `Issue026-J2_reference`, `Issue143-rpi_splitter` and `Issue508-DAC2020_bm08` (45°) and the
        J2 and rpi_splitter `-90` variants.
      - Rule: N02 totals, no-regress. The registered abort rule: a row whose baseline aborts at
        the `ulimit -v 8000000` cap (SIGABRT or Rust's `memory allocation of … failed`) is the
        fix's win if the fix completes with `via_in_pad_raw` 0 and router clearance 0. Its totals
        contribution is reported, not gated, and the other rows are gated with `--rows`.
      - Plain-tree check, by code reading: the any-angle neighbour calculation runs only on the
        plain search tree (`select_calculation_mode`), and `get_autoroute_tree` builds that tree
        only for a board with no angle restriction, or for a net in clearance class 0. N06.16's note
        did not say this; the identity rows test it.
      - `tools/bench/n06_run.py` now honours an `abort_rule` block (it logs the abort and goes
        on) and records each route's exit status and peak RSS (`os.wait4`).
      - Run: `N06_RUN_ULIMIT_KB=8000000 python3 tools/bench/n06_run.py docs/roadmap/prereg/N06.16.json
        --out target/orchestrator/n06-16`, 16 routes, one at a time.
      - Abort rule:
        - J2-any: the baseline aborts with exit −6 and `memory allocation of 1879048192 bytes
          failed` at a peak RSS of 6.79 GB. The fix completes in 56.7 MB and 4.4 s with 0
          unrouted, 0 clearance, 10 vias and `via_in_pad_raw` 0 (25 traces under the narrowest
          class width, reported).
        - rpi_splitter-any: the baseline aborts with exit −6 and `… 939524096 bytes failed` at
          7.00 GB. The fix completes in 48.3 MB and 2.4 s with 0 unrouted, 0 clearance, 2 vias and
          `via_in_pad_raw` 0 (13 reported).
        - Both rows are the fix's wins; their totals contribution is fix 0 + 0 unrouted, reported.
      - The other 6 rows: both arms' sessions are byte-identical, and
        `any_angle_room_side_mismatches` is 0 in both arms on all five 45 and 90 degree rows (and
        on scoring-mixed-layer-any). So the plain tree is not used there.
        `freerouting-dev gate --prereg docs/roadmap/prereg/N06.16.json --rows <those 6>` (baseline
        file first): 0 refusals, 6 neutral, total 0 → 0, set noise 4.90, `totals verdict: accept`,
        `gate: PASS`.
      - Counter on the fix arm: J2-any 3 and rpi_splitter-any 36 side mismatches; 35971 and 15354
        rooms completed.
      - Cross-check: the fix arm's sessions on J2-any, rpi_splitter-any, scoring-mixed-layer-any and
        Issue026-J2_reference are byte-identical to N06.14's baseline arm (the same configuration).
      - The switch's tests pass in release at `08062e2` under the cap:
        `neighbours_any::tests::room_side_index_switch_both_behaviours` and
        `tests/n06_any_angle_rooms.rs::any_angle_fanout_pass_stays_bounded_with_the_fix`.
      - Verdict: **accepted** (abort rule on 2 rows, totals PASS on 6). It is a candidate for
        N06.13's next promotion round into `ENHANCED`; `ENHANCED` is unchanged here.
      - Canary: no row (45 degree board, the counter is 0 on canary-pwr in every N06.14 arm);
        `target/canary-handoff/2026-10-20-n06-16/README.txt`.

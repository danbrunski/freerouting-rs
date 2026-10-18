# N06 — Fix the Java bugs kept on purpose

Opened 2026-10-17 (Dan asked whether they were fixed; they weren't). During the parity phase
the port reproduced Java's bugs wherever fixing one would change routing results. D10 ends that
phase, so each bug can now be fixed under profile `enhanced`, while `legacy` keeps it.

The inventory (N06.1, 2026-10-17) is the table below, with 54 rows:
- **12 of kind (a):** dead code or values nothing reads. Fixed outright, in N06.2.
- **31 of kind (b):** they change routes, imports or reports. Fixed under `enhanced` behind one
  switch each, in eight batches (N06.4–N06.11).
- **11 of kind (c):** judgement calls, in N06.12.

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
| Measurement: preregistration, board sets, baselines | `docs/roadmap/routing-quality.md` "How to measure a quality change"; `docs/roadmap/prereg/*.json`; `tools/bench/sets/*.txt`; Q14 is a worked example ([Q14](Q14-via-clearance-follows-net-class.md), `prereg/Q14.json`, `sets/q14-via-clearance.txt`) |

This checklist adds the `router.java_quirks.*` switches, the fixes behind them, and their
measurements.

## How to work this checklist

Whoever implements an item (the `implementer` agent, or Codex):
- **Follow `.claude/agents/implementer.md`:** the Java reference at the pin, the targeted gate,
  the `[x]`/`[~]` notes, and the hard rules (no windows on Dan's desktop, hands off Windows and
  KiCad, `CARGO_BUILD_JOBS=2`). Where Dan's dispatch says otherwise, the dispatch wins.
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

## File ownership

| Item | Owns (creates or edits) |
|---|---|
| N06.2 | the files of the 12 (a) rows and their tests; the DIVERGENCES lines of those rows |
| N06.3 | `settings/fields.rs`, `settings/path.rs`, a new `settings/java_quirks.rs`, `settings/board.rs`, `autoroute/control.rs` (the `quirks` field only), `board/basic_board.rs` (the field, `deep_copy`, the snapshot), `io/specctra/parser/mod.rs` (`ReadContext`), the geometry `split_to_convex_with` entry points, `tests/settings_java_quirks.rs` |
| N06.4–N06.11 | the files in their rows' Rust column, their tests, `tools/bench/sets/n06-*.txt`, `docs/roadmap/prereg/N06.*.json`. The batches touch disjoint files except `autoroute/connection.rs` (M and F) and `optimize/pull_tight.rs` (P only), so run M and F one after the other |
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
- **The accept rule for a (b) fix (Dan, 2026-10-17: "if it's a bug fix and we fix it but it's
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

Batches: **G** geometry cutouts, **M** maze search and rooms, **F** fanout, **S** shove, **P** pull-tight,
**O** batch optimizer, **I** inputs and reports, **U** GUI and interactive.

### (c) Judgement calls: N06.12

| id | Rust | Java | The question | Reach | Proposal |
|---|---|---|---|---|---|
| `ripup_pass_no_unset` | autoroute/session/ripup.rs:12, 337; control.rs:138 | MazeRipupResolver.java:158-162, AutorouteControl.java:192 | `ripupPassNo` is never set, so the random detour factor `0.5 + r²` never fires; the batch router never had it (the 1.x import set it only in `ExpandTestState`) | batch, every rip-up from pass 4 on if enabled | Its own registration: switching on randomness Java never ran, deterministic by its reseed. Likely large either way |
| `insert_stale_trace_object` | autoroute/path/insert.rs:100 | FoundConnectionInserter.java:108-137 | `connectToTrace` uses the start or target trace object after the insertion split or removed it | batch, occasional | Leave: the split keeps the old geometry as the new pieces, so the stub lands on the same copper |
| `in_front_of_pad_side0_flank` | shove/drill_check.rs:461 | ForcedPadRouter.java:86 | Side 0's flank clause takes `min`, where cases 2, 4 and 6 take `max`; inferred from symmetry | as `in_front_of_pad_typo` | An arm of batch S, only together with the typo fix |
| `opt_threshold_fraction` | optimize/batch/mod.rs:197-212, settings/path.rs:357-364 | BatchOptimizer.java:367-377 | A threshold in (0, 0.1) is multiplied by 100 (Java logs it as a heuristic); a CLI value below 0.001 is scaled twice | only JSON or environment values in (0, 0.1) | Leave Java's heuristic; fix the double scaling of the CLI |
| `rules_unknown_layer_scope` | io/specctra/rules_reader.rs:21-29 | RulesReader.java | A layer scope naming an unknown layer applies to all layers | in, `-dr` only | Decide: ignore the scope with a warning, or keep it |
| `rules_autoroute_settings_reenable` | io/specctra/rules_reader.rs:21-29 | RulesReader.java | A rules file's `autoroute_settings` re-enables every layer and the optimizer | in, `-dr` only | Decide with C04: a settings semantics choice |
| `direction_compare_zero` | geometry/direction.rs:346 | Direction.java | `compare_to` handles the zero direction lopsidedly | batch, edge case | Leave: a quirk, not clearly wrong |
| `route_stubs_layer_test` | drc/route_stubs.rs:9-12, 136 | WindowRouteStubs.java:73-74 | Both via bounds are compared with the contact's first layer, so the stub layer is always the last | out (route-stubs list) | Leave until the intent is clear |
| `any_angle_skip_length_squared` | optimize/tighten/any_angle.rs:43-45 | TraceTightenerAnyAngle.java:22, 774, 896 | `SKIP_LENGTH = 10` is compared with a squared distance (effectively 3.2 units) | batch, any-angle mode only | Measure in any-angle mode, or leave |
| `polyline_array_reversal` | optimize/tighten/mod.rs:19-22, 491 | Polyline.java (`new Polyline(Line[])`) | The constructor reverses lines in the caller's array, and later reads see that | batch | Leave: no clear correct fix |
| `avoid_acid_traps_disabled` | optimize/tighten/mod.rs:377-381 | TraceTightener (`if (true) return polyline;`) | Java turned the acid-trap step off | batch | Leave: enabling it is a feature (a Q candidate), not a fix |

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
  unreachable. N06.8 checks it.

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
- [ ] **N06.2 — The (a) fixes.** Fix the 12 (a) rows outright, each with a test of the
  corrected value. Before fixing `door_section_realloc`, add a counter that the A14 run
  reports; the fix lands only if it reads 0. Correct the doc at `geometry/line.rs:495`: the overflow starts at ~46k units
  axis-aligned, ~32k only at 45°.
  - **Accept:**
    - the A14 exact set stays byte-identical (a heavy run: leave the command in a `[~]` note);
    - the B02, B03 and A04 probes are unchanged;
    - each row's DIVERGENCES entry moves from "kept on purpose" to the fixed Java bugs.
- [ ] **N06.3 — The quirk switches.** Add `router.java_quirks.<id>` for every (b) and (c) row
  that might be fixed, and the `JavaQuirks` value carried as the decisions say. Wire each
  switch to its site, keeping Java's code path when the switch is `true`.
  - **Accept:**
    - with every switch `true`, A14 is byte-identical;
    - each switch has a unit test showing both behaviours (the tests in the tables become the
      `true` half);
    - the keys round-trip through a settings file and `--router.java_quirks.<id>=false`;
    - the board's quirks survive `deep_copy` and snapshot restore.
- [ ] **N06.4 — Batch G, geometry cutouts:** `simplex_prev_division_line`,
  `octagon_box_cutout_branch3`.
  - First, an invariant test that fails on Java's code: over random holes in boxes and
    simplexes, the piece areas sum to outer minus inner, with no two pieces overlapping.
  - Then the fixes, behind `split_to_convex_with(quirks)`.
  - **Accept:** the invariant holds with both switches `false`, and the batch is measured
    under the decisions' accept rule. The drill pages change on most boards, so this is likely
    the largest geometry effect.
- [ ] **N06.5 — Batch M, maze search and rooms:** `init_failure_room_leak` (with the fix
  `session.clear()` before `InitFailed`, which also makes the rekey dead under `enhanced`),
  `failed_attempt_ripped_carryover` (snapshot and restore the set per attempt),
  `ortho_zero_width_room` (`b.ur.x`), `edge_corner_not_advanced` (advance the corner; first
  count how often the branch at `neighbours45.rs:286` runs), `connect_to_trace_swapped_width`
  (swap the indices).
  - **Accept:** the batch is measured, with one 90° board in the set for
    `ortho_zero_width_room`.
- [ ] **N06.6 — Batch F, fanout:** `fanout_stale_drill_carry` (a fresh `DrillCarry` per
  attempt, as the batch router already does), `drill_destination_room_index` (index by
  section), `fanout_pin_order_names` (accept both spellings).
  - **Accept:**
    - the batch is measured on boards with fanout on;
    - the done note gives how many fanout retries succeed before and after.
- [ ] **N06.7 — Batch S, shove:** `in_front_of_pad_typo`, `store_trace_class_self_compare`,
  `rational_perp_projection` (the arm with `in_front_of_pad_side0_flank` follows if the batch
  is accepted).
  - **Accept:** the batch is measured; the (c) arm is measured or explained.
- [ ] **N06.8 — Batch P, pull-tight:**
  - `smooth_loses_trace`: test the failure conditions before removing, or re-insert the old
    trace;
  - `pull_tight_reused_lines_change`: report a change only when `change_trace` changed the
    trace;
  - the prefix/suffix identity case of the reviewed list: confirm it is unreachable, or add it.
  - **Accept:** the batch is measured.
- [ ] **N06.9 — Batch O, batch optimizer:** `opt_first_pass_stops` (a clear inside a pass
  allows one more pass at normal ripup costs), `opt_equal_key_skip` (keep every sorted
  candidate).
  - This is the batch most likely to move `canary-pwr`, because it runs in every default run.
  - `opt_first_pass_stops` adds a pass, so the registration records time as well as quality.
  - Measure each fix alone, then both.
  - Correct the `optimize::batch` module docs and DIVERGENCES: 0 % winners come from boards
    without vias, not from the int division.
  - **Accept:** the three arms are measured.
- [ ] **N06.10 — Batch I, inputs and reports:** the five DSN and SES rows, `stats_line_ab_length`,
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
- [ ] **N06.11 — Batch U, GUI and interactive:** `snapshot_drops_compensation` (the snapshot
  stores the compensation flag), `rules_matrix_trees_not_rebuilt`, `move_drill_tidy_region`,
  `route_change_layer_no_via`, `route_connect_inner_break`, `select_extension_ignores_filter`.
  The GUI's profile selector (N01) reaches them.
  - **Accept:**
    - a test of both behaviours for each (kittest where it is a GUI flow);
    - a counter shows that a batch run over the registered set never restores a snapshot with
      compensation on. The other five are interactive code that batch runs never call.
- [ ] **N06.12 — The (c) rows.** Each one ends measured or explained, with the reason written in
  its row.
  - `ripup_pass_no_unset` gets its own registration.
  - The fix of `opt_threshold_fraction`'s CLI double scaling counts as a port fix, not a quirk.
  - **Accept:** every (c) row has its outcome.
- [ ] **N06.13 — Results and promotion.**
  - Each batch's registration and numbers go in its item.
  - Each accepted switch is turned off in `enhanced` through N05, with its DIVERGENCES line under
    "Profile `enhanced`".
  - A rejected switch stays `true` in both profiles, with its numbers. It is never removed,
    because `legacy` needs it.
  - The DIVERGENCES "kept on purpose" paragraphs point to this file.
  - **Accept:** every (b) row ends accepted, rejected with numbers, or accepted on tests under
    the decisions' rule.

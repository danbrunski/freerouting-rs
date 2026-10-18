# Divergences from Java Freerouting

Places where the Rust port deliberately behaves differently from the Java reference
(`danbrunski/freerouting` at `a773a70ec`; entries that name `cda540f` describe Java at
the previous pin). Anything not listed here is meant to match
Java; a difference that isn't listed is a bug in the port.

Each crate's docs carry the full detail; this file is the index.

## freerouting-geometry

**Java bugs fixed**
- Polygon-vs-polygon `intersects` tests the convex pieces; Java recursed forever.
- `TileShape::rotate_approx` with two corners uses segment index 1; Java used index 0
  and threw a `NullPointerException`.

**Java failures that became defined results**
- `distance`/`border_distance` on an empty shape or polyline return infinity.
- `nearest_border_points_approx` drops missing points instead of returning nulls.
- `remove_redundant_lines` on an empty simplex returns it unchanged; a `PolygonShape`
  built from an empty polygon is an empty shape.
- A failed polygon split counts as no pieces in contains/intersects. In
  `PolylineArea::split_to_convex` a failed cutout fails the split, results are cached only
  on success, and a stop request returns `None`.

**Stricter than Java (panic instead of a later failure)**
- `Line` accepts only integer points; out-of-range `LineSegment`/`Polyline`
  constructors and `Polyline::corner()` on fewer than two lines panic.

**API shape**
- `null` returns become `Option`; indices are `usize` and "-1 = not found" is `None`.
- IntOctagon `is_empty` compares by value; its `toSimplex` cache is gone. Logging is gone.
- `sin`/`cos` may differ from Java's `Math` by one ulp.

**Java bugs kept on purpose** (changing them would change routing results; revisit once
the router is ported and measurable): `RationalPoint.perpendicularProjection`, the int
overflow in `Line.length`, `stairApproximation45` evaluating `functionValueApprox` for
the function-of-y case, the `index_of_nearest_corner` quirk, `PolygonShape.area`
returning 0, `Simplex.cutoutFrom` never setting its previous division line, the no-op
branch 3 in IntOctagon's box cutout.

## freerouting-dsn

- Non-ASCII characters in names are kept; Java's JFlex scanner drops them.
- Tabs and newlines count as blanks in raw string reads; `'` is accepted as a quote;
  empty quoted names are skipped anywhere in a list.
- Malformed scopes where Java loses track of brackets (a class with no sub-scopes, a
  layer_rule with several rule scopes, a place without side or rotation, extra tokens
  before `)`) produce warnings instead.
- `autoroute_settings` is always read; Java reads it only before the first keepout or
  plane.
- The writer emits `snap_angle` and `via_rule`; Java's writer emits `snapAngle` and
  `viaRule`, which its own reader doesn't recognise.
- `.5` is a number; `pcb` and `signal` layer names match in any case; degenerate shapes
  are kept; names may be keywords or numbers.
- A non-integer value for an integer setting leaves it unset with a warning.
- An unterminated string or `/*` comment is an error at its start, not at end of file.
- The Java-layout session writer (`write::java_style`, C05.1) removes the outer quotes of a name
  without dropping its last character; Java's `IdentifierType.write` unwraps with
  `substring(1, len - 2)` and loses it (`identifier_with_outer_quotes_keeps_last_char`).
- The syntax tree keeps `wire_keepout` as its own kind (`KeepoutKind::WireKeepout`, test
  `wire_keepout_keeps_its_kind`); Java lexes it as `keepout`. The board builds it as a
  keepout, as Java does (`issue754_item_ids_do_not_change`, `issue754_ground_area_still_blocks_vias`).
- Every unknown named scope is reported once per keyword per file, with the keyword, the
  enclosing scope and its first position (Q09.6), except a quiet list of KiCad scopes that carry
  no constraint: `property` inside `layer` (`only_listed_kicad_scopes_skip_without_a_warning`).
  Java's `skipScope` warns only for a scope without a keyword. What is read does not change, and the board reader does not pass these
  warnings on, so the warnings compared with Java's are unchanged; the warnings of a rules file
  reach the job log (`an_unknown_keepout_warns_once_naming_keyword_and_scope`).

## freerouting (board model)

**Reading a DSN file** (`io::specctra`, port of `DsnReader` and `io.specctra.parser`)
- The DSN AST merges repeated top-level scopes and does not keep their order, so the
  board is built in a fixed order: parser, resolution, structure, placement (flip style
  only), library, network, wiring. Java acts on the scopes in file order; within the
  structure scope, layers are assumed to come before keepouts and planes.
- `autoroute_settings` counts as read wherever it is (see freerouting-dsn), so
  `adjustPlaneAutorouteSettings` runs only for files without that scope; Java also runs
  it when the scope comes after the first keepout or plane.
- Where Java ends with an exception the port keeps going: keepouts, planes and windows
  whose area does not resolve are dropped (NullPointerException); circles and polyline
  paths in the boundary are left out of the outline (ClassCastException); a wire on an
  unknown polyline-path layer is dropped. A file without a structure scope is a
  `ParseError` (Java returns a null board or throws).
- The placement scope's `resolution` and the pcb-level `unit` are ignored, as in Java.
- The scope identifier in warning messages is the net or padstack name; Java prints the
  scanner's current identifier.
- Logged messages go to `BasicBoard::log_messages`; only the wiring warnings are
  returned, as in Java.
- Kept on purpose: the `via_padstack_names` aliasing with the first class's `use_via`
  list, and class-class rules pairing only the first known class with the others (Java
  shares one iterator).

**Board model**
- Items live in an arena keyed by `ItemId`; the methods of `Item` and its subclasses
  that need the board are `BasicBoard` methods taking an id. Observers and the
  graphics update box are replaced by the change journal (`board::journal`), which a
  consumer turns on and drains with `take_changes`.
- Java serialization is `BoardSnapshot` (`board::snapshot`): it holds what Java
  serializes and no search trees; `restore()` rebuilds the default tree in item-list
  order, as `readObject` does. `Clone` on the boards is not a Java copy.
- `state_hash` (Java `getHash`) has Java's equality but leaves out the DRC cache
  `Item.smallestClearance`, which Java's `clearanceViolations()` writes as a side
  effect; the port has no such cache. The `state-hash` probe shows one step where this
  changes an equality: on Issue413 with the `a773a70ec` session, stage h7 (after
  `BoardStatistics` on a deep copy) no longer equals h5/h6 in Java
  (`EXPECTED_MISMATCHES` in `tests/board_probe_state_hash.rs`).
- The routing ledger (`drc::RoutingLedger`) counts exactly: undo, redo, in-place trace
  changes and net changes mark the nets they touch. Java's `NetRoutingLedger` hears only
  inserts and removals and goes stale after them; the `ledger` probe
  (`fixtures/reference/probe/ledger/`) shows Java's count differing from a fresh count
  after an undo, net changes and cutouts (`tests/drc_ledger.rs`).
- Java bug kept on purpose: a snapshot restore or deep copy rebuilds the default tree
  without clearance compensation and drops the compensated trees, even when
  compensation was on (`deep_copy_drops_clearance_compensation`).
- `ShapeTree` stores leaves in an arena; `overlaps` returns them sorted by object
  (rooms, then items, each by id descending) and shape index, as Java sorts them.
- Known bug, not a deliberate difference (B02.2): on Issue721 the DSN reader gives the pins
  and vias of nets in a net class that class's clearance class, where Java keeps the default
  ones, so their shapes in the compensated autoroute trees differ by the clearance step; the
  default tree and DRC are unaffected (`EXPECTED_MISMATCHES` in `tests/board_probe_tree.rs`).
- Java iterates the nets in `normalizeAllTraces` in `HashMap` order; the port simulates
  that order, but without the treeification of buckets with 9 or more keys.
- Stale `LeafId`s and `ItemId`s cannot be used after removal; Java's locking tests
  (`MinAreaTreeLeafLifecycleTest`, `ShapeSearchTreeLeafLifecycleTest`) have no
  counterpart because the borrow checker rules out concurrent reads and writes.
- `delete_all_traces_and_vias` removes the search-tree entries of the traces and vias too.
  Java's `deleteAllTracksAndVias` deletes from the item list only, so its trees keep the
  removed items and later queries return them (GUI-only: `BoardToolbar`, `BoardMenuOther`).
  `delete_all_traces_and_vias_leaves_no_tree_entries` (`tests/java_ports.rs`, B08.7).
- `unfill_conduction_areas` and `set_conduction_is_filled` save the area for undo before they
  change it, so an undo restores the filled flag and the obstacle flag. Java's
  `ConductionArea.setIsFilled` and `setIsObstacle` change the object the undo snapshot shares,
  so an undo keeps the change (`unfill_is_undone_by_undo`, `tests/board_gui_ops.rs`, B08.5).

**Board statistics** (`scoring`, port of `BoardStatistics` and
`BoardStatisticsBoundsCalculator`)
- The constructor from a KiCad JSON file is not ported.
- Java throws a NullPointerException in the Delaunay triangulation of
  `Issue179-Autorouter_PCB1_2023-3-24.dsn` while counting incomplete connections (a
  corner lies outside the bounding triangle) and writes no statistics or scores; the
  port skips such corners and computes them.
- The statistics are computed on construction and not cached on the board.

**Design rule check** (`drc`, port of `app.freerouting.drc` and `cli.DrcRunner`)
- Java collects items in `HashSet`s ordered by identity hash codes; the port uses
  fixed orders instead. An unconnected group lists its items by id, highest first, and
  its representative is the first pin, else the first trace, else the first item. Java
  picks a different representative, and the representative tail traces are not
  reported as `track_dangling`, so those two findings can differ by representatives
  while the groups are the same. `NetIncompletes` groups its items as Java does since
  `30baad7e2` (input order, then connected-set order), so the airline orders now match:
  the `last-mile` probe (A13.3, `tests/autoroute_pipeline.rs`) finds Java's airlines,
  corridors and removed blockers in Java's order; at `cda540f` Java started each group
  from a `HashSet` in identity-hash order.
- Copper islands (`zone_islands`): Java builds the fill with `java.awt.geom.Area` and
  takes its subpaths as islands, which assigns holes to the wrong outer subpath. The
  port computes the same fill (border minus holes, foreign and same-net clearance areas,
  plus the same-net thermal spokes) with its own polygon sweep on a grid of cells,
  flattening AWT's Bézier circles with 16 steps per quarter. Island boundaries match
  the AWT area except for slivers below 1e5 board units² without items. The islands
  are ordered by lowest y, then lowest x; Java lists them in AWT path order.
- The copper islands are compared with Java in `tests/reference_islands.rs` (B07.3,
  probe `IslandsProbe.java`): the port's islands and findings against Java's fill with
  the hole pairing fixed (equal on all 42 boards with conduction areas), and against
  Java's own findings on an allowlist of the boards whose hole pairing (40) or island
  order (1) changes them (`ALLOWLIST`, with the evidence the test re-checks). The
  16-step flattening makes clearance circles smaller than AWT's, so island areas differ
  by up to their perimeter times 5 µm. Java has islands the port lacks, all below 1e6
  board units² and without items: where a round pad's thermal spoke ends on its
  clearance circle, AWT's `Area.intersect` leaves pieces of the spoke outside the
  circle (Issue756-tomu-fpga: 1457 by 296 units). On Issue367-Charger (area 19) it
  leaves a whole spoke end unclipped, so Java's island reaches 297 units further
  (`EXPECTED_ISLAND_MISMATCHES`).
- `DesignRulesChecker::set_check_zone_islands` is a Rust addition. It turns the island
  check off, and the reference tests use it.
- The clearance violations and island fills are computed on the analysis threads
  (`freerouting::parallel`, P03.1: a job's `autorouter.max_threads`, else all cores). The
  results are merged in item order, so the output does not depend on the thread count. So are
  the airlines (per net, merged in net order) and the per-pass board statistics' fanout counts
  (`drc_findings_do_not_depend_on_the_thread_budget`,
  `statistics_do_not_depend_on_the_thread_budget` in `tests/parallel_analyses.rs`).
- The board statistics compute Java's values with less work (P03.1): the fanout counts take
  the pins' connectable items from one scan and each item's contacts once, where Java's
  `getUnconnectedSet` scans the board and walks the contacts again for every SMD pin; the
  bounds take every net's terminal items from one scan, where Java scans once per net.
  The counts and bounds are Java's (`reference_load`,
  `fanout_counts_equal_the_per_pin_definition`); only the search-tree work the statistics
  add to a board's work meter is smaller. No work limit is open while they run.
- The DRC command line (`freerouting -de board.dsn [board.ses] -drc [report.json]`)
  does the post-load processing before importing the session. Java runs the post-load
  DRC in a background thread while it imports the session. On
  `Issue289-Autorouter_PCB_FHT-VGA_2024-03-25.dsn` with the `cda540f` route session that
  race left net +5V in the outline's edge-pin-net cache and hid 6 trace-to-outline
  violations; with the `a773a70ec` session Java and the port report the same.
- The session reader imports the wires of `Issue676-ch32v-tx118s.dsn` (1 000 000 units
  per mm). Java's `SesReader` fails on all 48 with a ClassCastException
  (RationalVector to IntVector) and imports none.
- The report's `date` is in UTC; Java uses the local zone. `freeroutingVersion` is
  `rs-<crate version>`.
- `RoutingBoard::reduce_nets_of_route_items` reports whether a net was removed.
- For an unknown net, the item descriptions print `null`; Java throws.
- `Issue721-Autorouter_CE2632_HarryMu_2026-6-15.dsn` quotes its class nets with `'`.
  Java keeps the quotes, so its nets stay in the default class; the port reads them
  (see freerouting-dsn), so the expected hole clearances and the scores differ.
- Board parity gate (A14): `Issue684-Autorouter_PCB1_2026-5-8` and
  `Issue721-Autorouter_CE2632_HarryMu_2026-6-15` quote their class nets with `'`, so the
  port puts those nets in their classes where Java leaves them in the default one and
  their traces get another clearance class (on Issue684 the fanout's trace at pin 23 has
  Java's corners and widths but class 8 for Java's 1); both route differently from Java
  (`EXPECTED_MISMATCHES` in `tests/autoroute_board_parity.rs`).

**Settings** (`settings`, port of `app.freerouting.settings`, `util.ReflectionUtil` and
`LegacyRouterSettingsBridge`; tests `tests/settings_*.rs`)
- Machine-dependent defaults take an explicit `cores` (`ResolveContext`): `max_threads`
  defaults to `max(1, cores - 1)` and validation caps it at `cores`. Java reads
  `Runtime.availableProcessors()`.
- Environment variables are processed in sorted order of their names, so when two
  variables set the same field the later name wins on every platform. Java iterates
  `System.getenv()` in hash order. (`env_collision_is_deterministic`)
- The settings file is JSON in Java's shape, read by the workspace's own parser:
  strict JSON plus `//`, `#` and `/* */` comments. Gson's other lenient forms
  (unquoted names or strings, single quotes) are errors, and an error in a default
  config file is a warning and an empty source, as in Java. A `null` element of
  `layers` is read as an empty layer and a `null` element of `ignore_net_classes` is
  dropped; Java keeps the nulls (`merge_matches_java`, scenario `json-every`).
- The default config file is `<config dir>/freerouting-rs/freerouting.json`
  (`%APPDATA%`, `~/Library/Application Support`, `$XDG_CONFIG_HOME` or `~/.config`),
  not Java's `freerouting/` directory; Java's file is read only through `--settings`,
  which replaces the default source and is an error when missing or malformed. A
  `FREEROUTING__LOGGING__FILE__LOCATION` fallback moves the user-data path as in
  Java.
- `min_trace_width_um` (Java fork commit `46bf9e2ef`, after the pin) is a settings
  field, unset by default.
- `plane_obstacle_net_classes` (Q01, `quality::pour_guard`) is a settings field Java does not
  have, unset by default (no policy, Java's routing). Listing net classes makes their
  signal-layer pours obstacles for the nets with no pin or input via inside them, through the
  B09 area policy; an unknown class name fails the job, naming it and the board's classes; with
  `plane_as_obstacle` set it changes nothing (`tests/quality_pour_guard.rs`).
- `plane_obstacle_block` (Q09.4) is a settings field Java does not have, unset by default
  (`traces_and_vias`, Q01's rule). `traces` makes a listed pour bar the traces of the foreign
  nets it blocks and let their vias through, so the maze search lets a drill pass a layer where
  only such pours cover its location; any other value fails the job, naming it
  (`tests/trace_obstacle.rs`, `traces_mode_lets_a_foreign_gnd_via_through_the_island`).
- `wire_keepout` (Q09.3) is a settings field Java does not have, unset by default (`keepout`:
  Java's reading, where a Specctra `wire_keepout` is a full keepout that also blocks vias).
  `traces_only` builds each structure-level `wire_keepout` as `AreaKind::TraceObstacle`, which
  Java has no class for: an obstacle to the traces of every net and to no via. The maze search
  then lets a drill pass a layer on which only such areas cover its location (the layer keeps no
  room, the via is checked against the drill's shape there), and the DSN writer writes the area
  back as `wire_keepout`. It changes routes on `Issue754-avionics_hub` (its Ground layer accepts
  vias) and on canary-pwr's `in2-open-wirekeepout` variant; an image-level `wire_keepout` stays a
  keepout (`tests/trace_obstacle.rs`). Off by default until Q09.5's measurement.
- `autorouter.sweep`, `autorouter.sweep_passes` and `autorouter.sweep_after_optimizer` (Q10,
  `quality::sweep`) are settings fields Java does not have, unset by default (no sweep, Java's
  routing). An unknown `sweep` value or a negative `sweep_passes` fails the job before routing,
  naming the key (`unknown_sweep_settings_are_refused_before_routing`).
- The D9 stage keys (A10.2) are settings fields Java does not have, unset by default
  (unset uses the `autorouter.algorithm` bundle): `fanout.algorithm`,
  `autorouter.{ordering, connection_router, ripup_policy, pass_controller}` and the
  `special` section (`plane_taps`, `neck_down`, `stages`). An unknown id in them is an
  error naming the key and the known ids; the two Java keys keep Java's warn-and-default
  (`autoroute_batch_control.rs`, `unknown_per_stage_id_is_an_error_naming_the_key_and_the_known_ids`).
- The `debug` section (P02.1) is a settings section Java does not have: `audit_incremental`,
  off by default, compares every search tree with a fresh build after each ripup and restore
  and at the end of each pass, and logs the tally and the first difference. It only reads
  the board, so the routed result is the same with it on (`tests/search_audit.rs`,
  `board::search::audit`).
- Floating point text: hexadecimal floating point literals (`0x1p3`) and Unicode
  decimal digits in `Integer.parseInt` are not accepted (`settings_text.rs`).
- A rules file's `autoroute_settings` build their patch from the layer names found in
  the rules file, as `RulesReader.discoverLayerStructure` does; a non-integer value in
  a DSN or rules `autoroute_settings` integer scope is left unset (see freerouting-dsn)
  where Java sets 0 and then clamps it to 1.

**Loading a job for routing** (`job`, port of the load sequence of `CliRunner.initializeCli`,
`RoutingJobScheduler` and `HeadlessBoardManager`; tests `tests/job_*.rs`,
`tests/settings_board.rs`, `tests/outline_pin_gap.rs`)
- The scheduler's second merge (`RoutingJobScheduler.java:101-174`) runs only when the job
  has a rules file: its `ApiSettings` carry the first merge's result, which after the board
  tuning has no null field a lower source could fill, so without a rules file the merge
  changes nothing. With one it does (`explicit-cost-rules`, see "Rules files").
  `job_prepare_reference.rs` compares the settings, the tuned layer arrays, the clearance
  matrix and the post-load DRC count with Java's dumps, which run the real second merge, on
  every board and scenario.
- `ignore_net_classes` names that match no net class are warned about in the job log; Java
  warns only on its API path.
- The tuning's debug log ("changed parameters") and `job.drcSettings.clearanceToleranceUm`
  (the override also writes it) are not ported; log lines of the overrides are returned as
  `LogLine`s.
- `Issue684-Autorouter_PCB1_2026-5-8` and `Issue721-Autorouter_CE2632_HarryMu_2026-6-15`
  have a net class named `''`. Java keeps the quote characters in the name; the port's DSN
  reader accepts `'` as a quote (see freerouting-dsn) and reads an empty name, so the class
  name differs in the matrix and the class dump, and `-inc ''` flags the class in Java but
  is a blank name to the port. Everything else, `postLoadViolations` included, matches
  (`EXPECTED_MISMATCHES` in `job_prepare_reference.rs`).

**Rules files** (`io::specctra::{rules_reader, rules_writer}`, port of `RulesReader`,
`RulesWriter` and the rules discovery of `RoutingJobScheduler`; tests `tests/rules_reader.rs`,
`tests/rules_writer.rs`, `tests/reference_drc.rs`)
- Java's behaviour that the port keeps, each pinned by a test: a rules file's
  `autoroute_settings` override the merged settings, the CLI included, and switch every layer
  and the optimizer back on; a `layer` scope with an unknown layer applies to all layers; a
  via type with an existing name moves to the end; the clearance matrix changes without
  re-inserting the tree items; a different header name is a warning. A rule in the file for
  one layer lands on two layers when the file names a single layer: the second merge reads
  the rules file's layer names alone (`discoverLayerStructure`), so `In1.Cu`, the only layer
  `explicit-cost-rules.rules` names, is layer 0 (`F.Cu`) there, and `RulesReader.read` then
  sets the real `In1.Cu` (C01's observation; `explicit_layer_cost_suppresses_computed_costs`).
- The discovery order is Java's: the job's own file (`-dr`, `a.dsn+b.rules`), else the
  adjacent `<file name up to its last '.'>.rules` for a DSN input. A named file that does not
  exist is not read, silently, and the adjacent file is not tried; `processor.Z80.dsn` does
  not pick up `processor.rules`.
- The file is parsed completely before anything is applied, so a syntax error leaves the
  board untouched; Java applies the scopes before the error and returns `false`.
- Java's scanner prints `DsnFile.read_string_scope: closing bracket expected at ...` while it
  recovers from a net class with `(via_rule 1A EXTERNAL 1oz)` (unquoted words). The port's
  parser skips the extra words with its own warnings (`line N, column M: unexpected 'EXTERNAL'
  in via_rule scope ignored`); the other log lines and the DRC report are Java's
  (`Issue029-hw48na_invalid.rules`). The scope identifier in the header warning is the last
  one a DSN read noted on the thread, as Java's static field.
- After an `autoroute_settings` scope that names an unknown layer, Java's scanner is left in
  the middle of the scope and reads on from there; the port drops the scope and continues
  with the next one.
- The DRC runner reads the file like Java (`Loading RULES file for DRC`, `RULES file loaded
  for DRC successfully` even after a failed read), with its own `autoroute_settings` ignored.
- The writer builds the plain syntax tree, so the text has `snap_angle` and `via_rule`, which
  both readers accept; Java's text has `snapAngle` and `viaRule`, which its own reader skips,
  so a Java round trip loses the snap angle and the via rules. The tests compare Java's text
  with those two words renamed. Trace costs are Java's `float` text read as a `double`
  (`2.8`), the design name goes through the tree writer's quoting where Java writes it raw (a
  name with blanks reads back as its first word in Java), and an empty clearance class name
  is `(clearance v)` where Java writes `(type )` (both read as the default rule). Names go
  through `IdentifierType.write`'s removal of the string quote.
- The format is lossy, in Java and here: the default rule holds layer 0 only, a named class
  has one `(clearance v (type name))` that Java's reader skips (a type needs an underscore
  to be read as a pair, and `MIN_EXTERN_188A` becomes `MIN` and `EXTERN_188A`), and reading
  starts by setting every cell to the default. `rules_round_trip_on_every_fixture` states
  what does come back.
- Five boards differ from Java's dumps (`EXPECTED_MISMATCHES` in `tests/rules_writer.rs`): two
  design names with blanks, the `''` class names of `Issue684` and `Issue721`, and a via type
  with an empty class name in `Issue179` (Java's text does not parse in the port).

**Writing a session file** (`io::specctra::ses_writer`, port of `SesWriter`; tests
`tests/ses_writer.rs`, `tests/ses_round_trip.rs`)
- The default output is Java's exact text (`SesStyle::Java`); `SesStyle::Plain` is the
  syntax-tree writer's layout. Both parse to equal trees.
- A component that is not placed is written as `(place name)`; Java throws a
  `NullPointerException` on it.
- Java's `SesReader` and `SesWriter` both divide by `dsn_to_board(1) / resolution`, so a session
  they wrote reads back scaled wrongly where that factor is not 1 (`Issue676-ch32v-tx118s`,
  0.1: 100 times too large). The port reproduces both sides (kept bug; the written text is
  Java's byte for byte); `ses_round_trip.rs` lists the board as an expected mismatch of the
  round-trip property.
- The library of a board whose adjacent rules file adds via padstacks (`Issue029-hw48na`) lacks
  them in `library_out` until the rules reader (C04) lands; `ses_writer.rs` strips them from
  Java's text before comparing.

**Writing a design file** (`io::specctra::dsn_writer`, port of `DsnWriter` and the `write*`
methods of `io.specctra.parser`; tests `tests/dsn_writer.rs`, `tests/dsn_round_trip.rs`)
- The layout is the plain one of `Pcb::to_dsn`, not `IndentFileWriter`'s; the comparison with
  Java is on parsed trees (19 dumps in `fixtures/reference/dsn`). `compatMode` is not ported
  (only the GUI's compatible export uses it). No `autoroute_settings` is written, as in Java.
- `-do out.dsn` writes the board as it is, including the classes `prepare_job` added
  (`board_edge`, `hole_edge`); a re-read board then has an explicit outline class, so the
  default edge override is skipped on the next run. Java's headless `-do x.dsn` writes a 0-byte
  file and no error (C07.4 registers the format in `job/outputs.rs`; `write_dsn_to` is its
  writer).
- Java's writer is not the inverse of its reader in several places. Where the engine reads
  something that Java's writer loses, Rust writes what the reader needs, so that
  `build_board(parse(write_dsn(board)))` has the same layers, outline, rules, areas, components,
  library, nets, classes, via types and wiring (the `*_survive_the_round_trip` tests) and the
  same statistics and DRC result (`round_trip_gives_the_same_statistics_and_drc_result_on_every_fixture`).
  The differences, each pinned by a test in `tests/dsn_writer.rs`:
  * Named clearance classes are written as pairs `(clearance v (type a b))` that rebuild the
    matrix, class by class and layer by layer; Java writes `(clearance v (type name))`, which
    its reader skips (`named_clearance_classes_are_written_as_pairs`). A layer's rule repeats
    the layer's own trace width, not layer 0's (`layer_rules_carry_the_width_of_their_layer`).
  * A net class that has `(rule (clearance v))` makes the reader create its clearance class
    (and make it the default clearance class of every item kind of the class): such a class is
    written with that rule instead of in the structure rules, the via types the reader makes
    for it are left out, the via rules are left out, each class that has a rule of its own gets
    its `use_via` and the structure's via list loses the names the classes append. Java writes
    the classes, via types and rules, which makes the re-read net classes default their item
    clearance classes. The plan checks on a copy of the matrix and the rules that the reader ends
    with the board's, and falls back to Java's form if not
    (`net_classes_are_written_the_way_the_reader_makes_them`). A `use_layer` is left out when
    it lists the signal layers (Java's would zero the trace width of the power layers).
  * Items are written oldest first. Java writes the newest first (`startReadObject`), which
    reverses the item list on every write and read (`items_are_written_oldest_first`).
  * The `pcb` rectangle is the board's box shrunk by the 1000 the reader adds, so a board does
    not grow with each write (`the_bounding_box_does_not_grow`).
  * A `place_keepout` of the structure scope is written as `place_keepout` (Java: `keepout`),
    and the place keepouts of an image are written (Java leaves them out). A clearance class is
    written for the outline, a plane or a keepout whenever the reader's default would be
    another one (Java: only for keepouts of a class that is not `default`). The fixed state of a
    conduction area is written, and Java's `shoveFixed` is `shove_fixed`
    (`keepout_kinds_classes_and_fixed_states_are_written`). Names of image keepouts are written
    when the file named them: the keepouts of placed components refer to them.
  * A power layer lists the nets of its planes in `use_net`, and a fixed area of a plane net on
    a signal layer is a `plane` scope (Java: wiring): only these make the reader mark a net as
    a plane net (`plane_nets_survive_through_use_net_and_plane_scopes`).
- A placed component that owns no item (a logo without pads) is not written, as in Java, which
  cannot tell it from a component whose items were all deleted. A padstack shape with no corner
  (`Issue179`'s `p7`) and the nets of the ignored copper pours of `canary-pwr-pours-as-planes`
  are expected mismatches of the round trip (`EXPECTED` in `tests/dsn_support/mod.rs`).
- Java writes an `(image` without a name for a package made without one (`Issue145`).

**Infeasible-connection diagnostic** (`quality::feasibility`, Q03; tests
`tests/quality_feasibility.rs`, `tests/quality_feasibility_java.rs`)
- New, with no Java counterpart (Java's `AutorouteUnroutedReport` only lists unrouted
  connections): a static report of the pins no trace of the net class's width can reach,
  with the widest corridor and the items forming it. Nothing in routing reads it, so routes
  are unchanged. Its own pin widths (`Pin.getMinWidth`, `getTraceNeckdownHalfwidth`) follow
  Java until B05 ports them on the board.

**Failure attribution and verdicts** (`quality::{attribution, verdict}`, Q06; tests
`tests/quality_attribution.rs`, `crates/freerouting-cli/tests/result_json_verdicts.rs`)
- New, with no Java counterpart: with `router.special.diagnostics=verdicts` (not a Java key)
  every incomplete connection gets one verdict (`infeasible`, `work_limit`, `blocked_by`,
  `unrouted`) and the routed nets in its way, from a ripup-off replay of the Freerouting maze
  search on a copy of the final board. The connection router is wrapped to observe each
  attempt's outcome only when a diagnostic is selected; the SES is byte-identical with it on
  and off (`the_ses_is_byte_identical_with_the_verdicts_on_and_off`).

**Delivered widths, the narrowing ledger and strict sizes** (`quality::neckdown`, Q02.1 and
Q02.2; tests `tests/quality_widths.rs`, `tests/quality_no_ratchet.rs`,
`crates/freerouting-cli/tests/result_json_widths.rs`)
- New, with no Java counterpart: with `router.special.diagnostics=widths` (not a Java key) the
  run reports, for each net whose class is wider than the board's narrowest class, the length
  under its class width and the length per width, and lists every shipped trace under its class
  width with the stage that made it (from item ids). It reads the final board only; the SES is
  byte-identical with it on and off (`the_ses_is_byte_identical_with_the_report_on_and_off`).
- `neck_down.strict` (Q02.2) is a settings field Java does not have, unset by default (off, Java's
  routing). When true, `automatic_neckdown` and `neck_width_um` act as off and 0 on the board,
  so Java's two neck-downs never narrow (the fanout's micro neck-downs still may); routes
  change where Java would have necked
  (`strict_sizes_turn_the_neck_downs_off_and_add_the_report`).

**Add-only completion sweep** (`quality::sweep`, Q10; tests `tests/quality_sweep.rs`)
- New, with no Java counterpart (Java reaches it only with a second run on frozen wiring, through
  KiCad's exporter): with `router.autorouter.sweep=fixed|free` the job runs Java's pass again
  after the batch loop's best-board restore, with rip-up off and no repeated-failure ripup, up to
  `sweep_passes` (2) times, keeping a pass only if it routed something and raised neither the
  incomplete count nor the clearance violations. `fixed` marks the earlier wiring user-fixed for
  the sweep and gives it its state back afterwards; `free` lets pull-tight and the shover move it.
  With `sweep_after_optimizer` the sweep runs between a first optimizer run and the pipeline's
  own. On `q06-sole-wall.dsn` with `-mp 1` the board ends complete with either arm and with one
  incomplete connection without (`sole_wall_ends_complete_with_the_sweep_and_incomplete_without`);
  off is the unchanged engine (`off_is_the_unchanged_engine`, and the A14 gate). Off by default
  until Q10.4's measurement.

**Shove checks** (`shove`, O03; tests `tests/board_probe_shove.rs`)
- The fast cutout (`ShapeTraceEntries.fastCutoutTrace`) of a trace with a segment that has
  no tree leaf (a zero-length segment, left by an earlier cutout): Java throws a
  `NullPointerException` in `ShapeSearchTree.reuseEntriesAfterCutout` and leaves the board
  half changed; the port skips the missing leaf and finishes the cutout. The
  `trace-entries` probe hits it on `Issue026-J2_reference` at cutout sample 37
  (`JAVA_CUTOUT_EXCEPTIONS`); every line before it is equal.
- Java bugs kept on purpose: `ForcedPadRouter.inFrontOfPad` for from side 0 takes
  `min(lineA.x + lineA.y, lineB.x + lineB.x)` (`ForcedPadRouter.java:78`; test
  `in_front_of_pad_keeps_java_typo`), and `ShapeTraceEntries.storeTrace` compares a
  contact trace's clearance class with itself, so only the shove-fixed state and the half
  width decide there (`shove::trace_entries` module docs).
- `ForcedPadRouter.calcCheckShapeForFromSide` with a rational shape centre (Java's
  `Line(Point, Direction)` only warns and builds a rational line) or a polyline whose offset
  shape is `null` (Java then throws in `checkTraceShape`): the port skips that side
  (`shove::drill_check::calc_check_shape_for_from_side`, test
  `calc_check_shape_skips_rational_centre`). Every caller passes an integer via location, so
  no probe query reaches it.

**Shove insert** (`shove::insert`, `shove::forced`, O04; tests `tests/reference_shove_insert.rs`,
`tests/issue954_keepout_insertion.rs`)
- Java bugs kept on purpose: `RoutingBoard.moveDrillItem` with a finite tidy width never
  pull-tightens. `DrillItemMover.insert` unions the moved shapes into its own copy of the tidy
  region (`tidyRegion = tidyRegion.union(..)`, `DrillItemMover.java:145`), so the caller's region
  stays `IntOctagon.EMPTY`, `EMPTY.enlarge(w)` is `EMPTY` again and `optChangedArea` skips the
  tightener; only `Integer.MAX_VALUE` (no clip) pull-tightens after a move
  (`RoutingBoard::move_drill_item`, test `move_drill_item_finite_tidy_skips_pull_tight`).
- Where Java throws out of an insert (a substitute piece without a tree shape in
  `ShapeAndEntrySide`; `insertTraceWithoutCleaning` rejecting the new trace, fewer than two
  corners or zero length, so that `combine` throws a `NullPointerException`), the port reports
  a failure instead: `TraceShover::insert` returns false and `insert_forced_trace_polyline`
  `ForcedInsert::DatabaseDamaged`, after which callers undo. No `shove-insert` probe step
  reaches either.

**Pull-tight** (`optimize`, O01; tests `tests/reference_pull_tight.rs`)
- The any-angle end smoothing (`TraceTightenerAnyAngle.smoothenEndCornerAtTrace`, O02) of a
  trace with three corners whose non-integer end corner has a last segment shorter than
  `SKIP_LENGTH`, at a bend with a contact trace: Java calls `repositionLine` with start index
  `-1` and throws an `ArrayIndexOutOfBoundsException` that nothing catches; the port returns no
  smoothing (`optimize::tighten::any_angle` module docs). No probe board reaches it.
- Follows Java 588320207, which is after the pin a773a70ec: `connection_items` (Java
  `Item.getConnectionItems`) stops at the first item already collected. At the pin the walk looped
  forever on a closed ring with no fork, so on `Issue756-minimal-hang` with its routed session (a
  second copy of each wire) and the restriction forced to `None`, `opt_changed_area` never
  returned, in Java and in Rust (`split_traces -> removeIfCycle`). The returned set is unchanged
  wherever the old walk finished. The `pull-tight-any` reference of that board
  (`fixtures/reference/probe/pull-tight-any/Issue756-minimal-hang.jsonl`, `javaCommit`
  588320207) comes from 588320207, not the pin; pull-tight removes all four traces.
  `board_none_matches_java` has no expected mismatch any more; the ring is pinned by
  `any_angle_tightener_cycle` in `tests/java_ports.rs`.
- Java bugs kept on purpose (`optimize::pull_tight` module docs): the end-corner smoothing
  removes the trace before it inserts the smoothed copy and leaves it removed when the
  insertion fails (fewer than two corners, or zero length and unfixed), then removes the old
  trace a second time (a no-op); a keep-point split that ends the smoothing early leaves the
  tightener's contact pins unrestored; `pull_tight_trace` reports a change when
  `change_trace` finds every line reused and returns early. No probe board reaches the first
  two; the `pull-tight` probe compares the paths around them.
- Via optimizer (`optimize::via`, O05; `tests/reference_opt_vias.rs`): a trace-cost array without
  an entry for a contact trace's layer leaves a via between two traces unchanged, where Java's
  `ViaOptimizer.optViaLocation` throws an `ArrayIndexOutOfBoundsException`
  (`short_trace_costs_leave_a_via_between_traces`). The router always passes one entry per layer.

**Batch optimizer** (`optimize::batch`, O06; tests `tests/batch_optimizer.rs`,
`tests/reference_optimize.rs`)
- Determinism: every candidate is rerouted on a fresh copy of the pass baseline
  (`snapshot.restore()` plus the transient reset, which is `deep_copy`), and results are
  merged in candidate order. Java reuses one worker board per thread (`WorkerBoardState`),
  whose undo restores neither the id generator nor the trace half-width bounds nor the
  ledger, so its result depends on the thread count and the scheduling; the port's does not
  (`optimizer_is_thread_count_independent`, `chunk_size_does_not_change_results`,
  `candidate_copy_equals_deep_copy`). Compared with Java on aggregates until Java's worker
  fix is pinned (`fixtures/reference/optimize/`, O06.7).
- Stops a candidate requests: when a candidate's inner autorouter reaches
  `autorouter.max_items` it requests a full stop (Java `AutoroutePassRunner`
  `requestStop` on the shared job thread, which the candidates running at that moment on
  other threads see too). The port gives each candidate its own stop token, relays the job's
  stops to it, and applies a stop a candidate requested in candidate order: the later
  candidates of the chunk are skipped, as single-threaded Java would
  (`cross_thread_optimization_produces_identical_results`).
- Scheduling (P03.2): Java waits for a whole chunk of candidates before it merges it; the
  port's threads run up to 16 candidates per thread ahead of the merge and the merge takes
  each result as it comes, still chunk by chunk with Java's rules, so a stop or the failure
  limit ends the pass at the same candidate and later results are dropped
  (`lookahead_past_an_early_stop_changes_nothing`). A candidate's before and after figures
  (Java `new BoardStatistics(board, null, false)`) compute only the via count and the
  weighted trace length it reads, bit for bit
  (`lean_optimizer_figures_equal_the_cheap_statistics`), so a candidate's work meter counts
  only its routing.
- Time: Java forces a candidate to "not improved" once the optimizer deadline has passed
  (`optRouteItemOnBoard`); the port reads no clock there. `optimizer.timeout` is armed by the
  pipeline as a full stop (`StopLevel::All`), checked before each pass and each chunk, and
  the pass it lands in adopts no unfinished winner (`optimizer_timeout_is_a_full_stop`).
- Java bugs kept on purpose (`optimize::batch` module docs): a first pass without a winner,
  or whose winner has an `improvement_percentage` of 0 (`ItemRouteResult`'s integer
  division), clears the increased ripup costs inside the pass, so the loop stops after one
  pass (`first_pass_without_winner_stops`); the candidate order skips every item whose key
  equals an earlier one for the whole pass (`equal_key_skips_rest_of_pass`); an improvement
  threshold in (0, 0.1) is multiplied by 100.

**Autoroute room graph** (`autoroute::session`, A02; tests `tests/autoroute_probe_start_rooms.rs` and
`autoroute::session::tests`)
- Any-angle room graph (`autoroute::session::neighbours_any`, A07.4): a neighbour that touches
  the room in one point lying on no border line of the room or of the neighbour (Java logs
  "touchingSideNoOfRoom >= 0 expected" and goes on with side -1, which each shape class then
  clamps or rejects in its own way) ends the neighbour calculation with
  `RoomError::IndexOutOfBounds`: `completeExpansionRoom` swallows it like Java's exceptions, an
  obstacle room's door calculation passes it on (`a_point_touch_on_no_border_line_is_an_error`). It needs a point-sized tree shape strictly
  inside a room; no `variants.txt` board reaches it.

**Maze search** (`autoroute::session::{search, layers, ripup, shove_room}`, A04; tests
`tests/autoroute_probe_maze.rs`, `tests/autoroute_search_sinks.rs`)
- `Connection.get` walks a closed loop of traces without forks forever in Java; the port stops
  the walk after `4 * (item count + 1)` steps (`a_closed_loop_of_traces_ends_the_connection_walk`).
  Normalization combines such traces, so no loaded board has one.
- The maze queue keys hold the door's Java id at the time of the push; Java's `TreeSet`
  compares ids at comparison time, and a drill page's id changes when its drills are first
  calculated. The orders differ only for a full tie of both doubles with an element of such a
  page; no `maze-log` or `maze-routed` board has one.
- Adopted Java fix `7f56b062d` (after the pin; ledger "Port: yes"; Q01.7): a power layer carries
  only its planes' nets. `AutorouteControl::own_plane_layers` marks the power layers with a
  conduction area of the current net, and `expand_to_room_doors` does not expand on an inactive
  power layer without one, as on an inactive signal layer (before, an existing via or
  through-hole pin of a foreign net could start a trace across the plane). Test
  `autoroute_fixture_ports::power_layer_carries_no_foreign_net_trace` (Java's
  `PowerLayerForeignNetTest` and its fixture, `tests/data/PowerLayer-foreign-via-pair.dsn`).
  It changes Java's own routes, so the route references of the four boards whose Java output
  changes are regenerated at `7f56b062d` (`Issue145-smoothieboard`, `Issue753-CPU-85_r104`,
  `canary-pwr-9511d9a-in2-open-wirekeepout`, `canary-pwr-pours-as-planes`, with their DRC
  digest rows and smoothieboard's `ses` dumps; see `tools/java-reference/README.md`); the other
  17 references of boards with a power layer came out byte-identical there and stay at the pin.
  17 probe references are regenerated there too (`pass`, `insert`, `maze-log` of `min_fr_test`,
  whose maze work units change, and the board probes that load the two new sessions); the
  other 59 probe runs were identical and stay at the pin.

**Path location** (`autoroute::path`, A05; tests `tests/autoroute_probe_locate.rs` and
`autoroute::path::tests`)
- Where `FoundConnectionLocator45Degree.calculateNextTraceCorners` warns ("ExpansionDoor
  expected", "lineSections inconsistent") after it has found corners, it returns them without
  advancing, and Java calls it again with the same state until it runs out of memory (an
  `OutOfMemoryError`, which `autorouteConnection` does not catch); the port returns
  `LocateError::EndlessLoop` (`a_warning_branch_without_corners_ends_the_trace_and_with_corners_loops`).
  No `locate` or `locate-routed` board reaches either branch.

**Connection insertion** (`autoroute::path::insert`, `autoroute::connection`, A06; tests
`tests/autoroute_probe_insert.rs`, `autoroute::connection::tests`)
- Java bugs kept on purpose (to be fixed after rung 3): when `MazeSearchEngine.getInstance`
  returns null or throws, `AutorouteEngine.autorouteConnection` returns `FAILED` before
  `clear()` (`AutorouteEngine.java:142-150`), so the start rooms it completed stay in the
  autoroute tree as obstacles for later connections of the same clearance class; the port keeps
  them (`route_connection`, `// PARITY(quirk):`). A later session counts its room ids from 1
  again; Java then holds two rooms with one id, and the port moves the leaked room to a free
  negative key in place (`ShapeSearchTree::rekey_room_aside`, the tree's shape unchanged). A
  leaked room is an obstacle in room completion but never a neighbour with a door, while Java
  can build a door into it and reach its stale doors; not reproduced, and no probe board has an
  init failure (`an_init_failure_leaks_its_rooms_and_a_later_session_moves_them_aside`).
- Java's eager trace-log strings are dropped, their tree visits kept: `insertTrace` asks a found
  trace stub for its start and end contacts while building `compare_trace_stub_found`
  (`FoundConnectionInserter.java:441-458`); the port runs both contact queries so the work
  meter counts what Java counts (`insert_matches_java`, `Issue957-smoke-ldo-led`).
- A connection without traces that changes the layer makes Java's `insertVia` dereference a null
  `lastCorner`, a `NullPointerException` that reaches the batch router's catch (`FAILED`, no
  details); the port returns `InsertError::NullCorner` and `Failed` with empty details
  (`an_empty_located_path_across_layers_fails_like_javas_null_pointer`).
- The opt-in `router.min_trace_width_um` floor of Java `46bf9e2ef` (after the pin; ledger
  "Port: yes") is ported: pin neck-downs in the maze search and the inserter use
  `AutorouteControl::neckdown_half_width` and the micro-neck-down skips narrower candidates.
  Unset (the default) it changes nothing, so the pin's behaviour is unchanged.

**Batch passes** (`autoroute::batch::{connection, strict_drc, pass}`, A11; tests
`tests/autoroute_pass.rs`, `pass_matches_java`)
- The airline of the connection being routed (`ConnectionOutcome::airline`, the GUI's overlay
  only; it decides nothing) breaks a distance tie by the sets' descending item-id order; Java's
  `AutorouteAirlineCalculator` iterates its `HashSet`s, so a tie can pick another pair.
- The pull-tight after a routed connection and in `remove_tails` runs without the cancel token
  (`PullTightParams::stop` takes an owned `Arc`, a stage only borrows the token); Java's checks
  the thread's full stop. Only a requested stop sees it: Java stops tightening at once, the port
  at the 1,000 nominal ms work limit. Results without a stop are the same.
- Java's `Necked retry routed net ...` info line and its trace logging are not written; the pass
  runner returns the `Max items limit reached` and `Autorouter <details>` job log lines in
  `PassOutcome::log`.

**Fanout** (`autoroute::{fanout_pin, batch::fanout}`, A12; tests `tests/autoroute_fanout.rs`,
`fanout_matches_java`)
- Where `RoutingBoard.fanout` builds a control that Java's `AutorouteControl` constructor or
  `rebuildViaInfo` cannot build (a net class without a via rule while the board has via rules,
  so `BatchFanout` lets the pin through on the fallback), Java's exception leaves
  `BatchFanout.fanoutBoard`; the port returns `Failed` with the error text for that pin and goes
  on (`a_control_error_fails_the_pin`).
- The pull-tight after a fanned-out pin runs without the cancel token, as in the batch passes.
- `fanout.timeout` is read once when the stage starts, as a deadline that only stops work; Java's
  progress throttle (one update a second) is not ported: a `Progress` goes out at the start of a
  pass, after each pin and at its end, while someone listens.

**Interactive routing** (`interactive`, I01, port of `gui.interactive.Route`, `RouteState`,
`DynamicRouteState`, `StitchRouteState` and `GuiBoardRoutingSettings`; tests
`tests/interactive_route.rs`, `interactive::settings::tests`)
- Java bug kept on purpose: when the via rule has no via spanning both layers,
  `Route.changeLayer` returns true without a via and without changing the route's layer, and the
  route state still switches the current layer and takes a snapshot
  (`layer_change_without_spanning_via_quirk`).
- Java bug kept on purpose: `Route.connect` leaves only the inner loop when a corner pair fails,
  so with an added 45 or 90 degree corner the second pair is still tried from the added corner.
- Java failures that became defined results: a route net number without a net (Java's
  `NullPointerException` in `getTraceHalfWidth`, `getTraceClearanceClass`, `getViaRule`) gives half
  width 0, the default clearance class and no via rule, and the start fails with
  `StartError::UnknownNet`; a manual via-rule index out of range (Java's
  `ArrayIndexOutOfBoundsException`) falls back to the net class's rule
  (`a_net_without_a_net_class`, `manual_via_rule_index_out_of_range_falls_back_to_the_net_class`);
  a neckdown try at the end of a target connection that damages the board (Java's
  `NullPointerException` on `currentOkPoint.equals`) is undone and ends the connection as the
  first insert's damage does.
- The current layer of the route state lives in the session (`RouteSession::current_layer`);
  Java keeps it in the workspace settings. Visible layers are input (`EditSettings::visible_layers`);
  Java's `setLayer` also makes a hidden layer visible, which is left to the GUI.
- Java's `Route` holds its target objects, so a target removed from the board keeps the shape
  it had at its removal. The session keeps the shape of each trace and conduction-area target as
  it last saw it (at the start and before each nearest-target search), which differs from Java's
  only when a target changes in place and is removed within one step. A removed trace that is the
  nearest target is not connected to (Java's `connectToTrace` on the removed object may succeed
  when the corner lies on its old polyline). A trace start whose `connectToTrace` splits the
  picked trace computes the targets from the removed trace's contacts at its end corners, as
  Java's `getUnconnectedSet` on the removed object does. No `route-script` board shows a
  difference.

**Interactive drag** (`interactive`, I02, port of `board.actions.MoveComponent`, `DragState`,
`DragItemState` and `MakeSpaceState`; tests `tests/interactive_drag.rs`, `tests/java_ports.rs`
`board_actions_move_component`)
- Java's message for dragging a user-fixed item is a literal English text ("Please unfix item
  before dragging"), not a resource key; the session reports `DragStep::Fixed` and the GUI owns
  the text (`press_on_a_trace_starts_nothing_and_a_fixed_via_does_not_move`).
- Component dragging and the item filter of a press are input (`DragSettings`); Java keeps them
  in the workspace settings. `MoveComponent`'s identity-hash sets are sorted sets; at the pin no
  result depends on their order (`move_component_order_is_item_id_order`).

**Interactive editor** (`interactive::{editor, select, pin_swap, construct, history, edit_ops}`,
I03, port of `gui.interactive.InteractiveStateController`, the menu, inspect, region, pin swap and
construction states and `GuiBoardHistoryController`; tests `tests/interactive_editor.rs`)
- Delete of the selection is disabled in Java's GUI at the pin (`GuiBoardLegacyEditActions` makes
  it a no-op); enabled here by decision (I00 decision 1, accepted by Dan): one snapshot, the
  deletion-forbidden and user-fixed items stay with Java's
  `some_items_are_fixed_and_could_therefore_not_be_removed`, the rest go through
  `remove_items_and_pull_tight` when push is on, else `remove_items`
  (`delete_selection_keeps_fixed_items_and_undo_restores`).
- Fix and unfix of the selection are disabled in Java's GUI at the pin; enabled here by decision
  (I00 decision 1). They take one snapshot and save each changed item for undo, so one undo
  restores the fixed states; Java's `Item.setFixedState` and `unfix` change the item without
  saving it (`fix_and_unfix_are_undoable`).
- A route, drag or pin swap starts only from a menu mode, as Java's keys and popups do; Java's
  `InteractiveStateController.startRoute` would also start one from another state after a
  warning. `SetTool` drops a session in progress as Java's `setRouteMenuState` does.
- A circle keepout's radius is the distance from its centre to the completing click (or the last
  `Move`); Java takes it from the last repaint of the pointer position, which is the same point
  unless the pointer moved without a repaint.
- Java failures that became defined results: a region selection completed before its first
  corner was set (Java's `NullPointerException` in `createSelectionBox`) reports
  `nothing_selected`; a hole in an area whose border or a hole is not a polyline shape (Java's
  `ClassCastException`) reports `adding_hole_failed`.

**Interactive move and copy of a selection** (`interactive::{transform, move_items, copy}`, I04,
port of `board.actions.ComponentItemTransform`, `MoveItemState` and `CopyItemState`; tests
`tests/interactive_transform.rs` against the `transform` probe, `tests/java_ports.rs`
`board_actions_component_item_transform`)
- Move, rotate, mirror and copy of the selection are disabled in Java's GUI at the pin
  (`GuiBoardLegacyEditActions.moveSelectedItems` and `copySelectedItems` are no-ops and nothing
  calls `MoveItemState` or `CopyItemState`); enabled here by decision (I00 decision 1, accepted by
  Dan). The state classes are transcribed headless by the probe and compared line by line on
  three boards (`transform_matches_java_*`).
- `apply_transform` returns the transformed copies and, for a refusal, the reason (Java returns
  `null`); the editor shows Java's `insertion_failed_because_of_obstacles` for every reason.
  Java's `HashMap`/`HashSet` of contacts are sorted maps and sets; no result depends on their
  order.
- A selected item that the board keeps when detached for a move (the conduction area of a power
  plane, which `detachItemForMove` does not remove) is refused with
  `some_items_cannot_be_moved_because_they_are_fixed`; Java would insert its copy beside the
  original. A conduction area of several nets, which `ConductionArea.copy` refuses with a
  `null`, is left out of a copy preview; Java adds the `null` and fails with a
  `NullPointerException` when the copy is inserted.
- The editor commands are not Java's keys: `MoveSelection { to }` starts `MoveItemState` at the
  last pointer position (the command's own position if there was none) and moves to `to`;
  `RotateSelection { degrees }` is `turn90Degree`, `rotate` and `turn45Degree` in one (a
  multiple of 90 turns exactly); `Click` in a copy first moves the preview to the click, where
  Java's click inserts at the last mouse move. After a move or its cancel the editor is in the
  menu, as Java returns `parentState.returnState`; a copy returns to the selection.

## freerouting-kicad

Differences from KiCad's own DSN export, which is what Java reads. The reader builds the same
tree as the export everywhere else (`crates/freerouting-kicad/tests/{structure,copper}.rs`
compare them on 12 committed boards and the 56-board corpus).

- Rule areas that ban only footprints, pads or copper pour, or nothing (a KiCad 9 placement
  area), are not keepouts; KiCad's export makes each a full `keepout`. An area that bans
  footprints adds a `place_keepout`; a copper-pour-only area warns that the plane is not cut
  (`rule_areas_map_by_flags`, `canary_pwr_fan_inlet`, `rule_area_flag_defaults_follow_kicad`).
- A tracks-only area is read as `wire_keepout`, which the board builds as a full keepout that
  also blocks vias, as Java does (K00 question 1), with one warning per area
  (`rule_areas_map_by_flags`).
- Track arcs are followed as polylines (chords of at most 0.1 rad and 1 um from the curve);
  KiCad's export writes one chord from start to end (`arcs_follow_the_curve`,
  `cm5_trace_length_matches_kicad`).
- Coordinates keep KiCad's nanometres instead of `%.6g` (K00 question 2): exported values
  agree within half a `%.6g` step (`copper_equals_the_export`).
- A footprint rule area's holes are written on the zone's first layer, as the export does
  (`makeIMAGE` uses `zone->GetLayer()`), whichever layer its keepout is on
  (`copper_equals_the_export`, the `rule-areas` board).
- Copper zones owned by footprints are kept as planes, after the board's own zones, with one
  warning each (K00 question 6); KiCad's export drops them. `ReadOptions::drop_footprint_zones`
  gives KiCad's behaviour (`planes_and_netless_zones`).
- The vias of net classes are listed in project-file order, KiCad's in `std::unordered_map`
  order; the Default class's via is first in both (`copper::via_lists` compares the rest as a set).
- The board's `<stem>.kicad_dru` is read and every rule reported (applied, ignored or UNSUPPORTED
  with the first unsupported term); KiCad's export ignores the file, so Java has never seen these
  rules. The expressible ones (class-pair clearance, class track width) change the board only with
  `ReadOptions::apply_design_rules`, off by default (K00 question 7; `supported_rules_change_the_tree_exactly`,
  `option_off_leaves_the_tree_alone`, `design_rules_reach_the_board`). `canary-pwr`'s ten
  `insideCourtyard` rules are all UNSUPPORTED (`canary_pwr_rules_are_unsupported_by_name`).
- Against the export's board the reader's differences are exactly the ones above and nothing
  else: `crates/freerouting/tests/kicad_parity.rs` lists each with its reason and fails on any
  other, and on a listed one that no longer occurs; the item order is the export's except for
  the class order, the order of the outline's holes (`tastexx-pcb`) and what the area and arc
  differences shift.

## freerouting-cli

**Command line and job outputs** (`freerouting-cli`; `freerouting::job::{run, outputs, deadline,
log}`; tests `crates/freerouting-cli/tests/{args_java, cli_outputs, settings_parity}.rs`)
- One parser does the work of Java's `GlobalSettings.applyCommandLineArguments` and
  `CliSettings`; Java runs both and logs some warnings twice (`-mp`, `--router.max_passes`),
  Rust logs each once. `-mt` still does nothing headless; Rust adds an info line naming
  `--router.optimizer.max_threads` (`mt_does_nothing_headless`).
- `-inc` keeps Java's comma split and adds a backslash: `-inc 'Power\,Default'` names the class
  `Power,Default` that KiCad writes. When the plain split's parts joined with `,` are a class
  of the board, the job warns (`inc_escaped_comma_names_one_class`,
  `comma_split_warns_when_it_looks_like_a_mistake` on `Issue558-dev-board.dsn`).
- No 0-byte outputs (`no_zero_byte_outputs`). Java's `-do x.xyz` writes the SES text into
  `x.xyz` and exits 0, and a format it cannot produce headless (`.frb`, `.scr`, `.kicad_pcb`) gives
  a 0-byte file or nothing. Rust refuses every format except `.ses`, `.dsn`, `.rules`, `.drc`,
  `.drc.json` and `.kicad_pcb` before any work and exits 2 (a usage error, "Exit codes" below). (Java at `cda540f`
  also wrote a 0-byte `x.dsn` and exited 1; `a773a70ec` writes a DSN, and so does Rust.)
- `-do a.ses+b.drc.json`: Java reads `.drc.json` as plain `.json` and writes a KiCad design JSON
  under that name; Rust writes the DRC report there (`.drc` too, also as the main output).
- Every output is written to `.<name>.<pid>.tmp` and renamed, so a killed process leaves no
  partial file; stale temporary files are removed at the next start
  (`killed_run_leaves_no_partial_file`). A serializer result of 0 bytes is an error.
- Wall-clock limits only stop work: one watcher thread sleeps until the earliest deadline and stops
  the cancel token. Java's monitor polls once a second and waits 30 s of grace before it marks
  the job `TIMED_OUT`; the Rust stages check the token between steps, so there is no grace
  (`timeouts_only_stop_work`). A timed-out job exits 0 and logs
  `warning: stopped by wall-clock limit; result is timing-dependent`.
- Log lines go to stderr without timestamps, prefixed `warning: ` or `error: `. The stage
  summary stops after the score (no JVM CPU or heap figures). `1 violation` is singular, as in
  Java, so `compare.py`'s regex misses a one-violation board.
- `--compare-boards` and `--calculate-benchmark-scores` are recognized and exit 2 with
  `not supported; use freerouting-dev ses-diff`; a command line without `-de` and `-do` is an
  error with exit 2 (Java opens the GUI), and `-de x.json` (KiCad JSON) is refused, also with 2.
- The pass line's `in X.XX seconds` is the time from the last progress event of the previous pass
  (or of the fanout, or the start of the run) to the last one of this pass; Java times the pass
  itself. The loop's `Auto-routing stage started with baseline score ...` line has no board
  hash. Logs are not results.
- `-de board.kicad_pcb` is accepted (Java: "Unknown file type"). The board is read directly with
  its `.kicad_pro` and `.kicad_dru` (`io::read_design`), the reader's warnings and read report are
  printed before the job's own log, and a file the reader refuses (KiCad 5 or older) exits 1 with
  its one-line message (`crates/freerouting-cli/tests/cli_kicad.rs`). The session names the board
  by its file stem, as for a DSN.
- `-de board.kicad_pcb -do routed.kicad_pcb` writes the routed tracks and vias into a copy of the
  input (K08, `io::kicad::write_kicad_board`): the copper the board no longer has is cut out by
  byte span, new copper goes in before the final `)` with deterministic UUIDs, every other byte is
  kept and zones are not refilled. Java writes nothing for `.kicad_pcb`. A `.kicad_pcb` output of a
  DSN input, or one that is the input file itself, is refused with exit 2 before any work
  (`crates/freerouting-cli/tests/cli_kicad_write.rs`, `crates/freerouting/tests/kicad_write.rs`).

**Plugin-facing CLI extras** (C08; `freerouting-cli::{exit, version, progress, manifest, stop}`;
tests `crates/freerouting-cli/tests/{exit_codes, version, progress_json, result_json, stop,
plugin_flags}.rs`)
- Exit codes. Java exits 1 for every failure. Rust adds 2 for a usage error found before any
  work (no `-de`, no `-do` outside DRC and help, an unsupported output or input format, a
  missing or malformed `--settings` file, a refused Java command, a bad value for a C08 flag)
  and 130 for a run aborted before it wrote an output (a second interrupt, or a stop during the
  load). 0 and 1 are Java's; a load error stays 1. The plugin only tests for non-zero
  (`usage_errors_exit_2`, `no_zero_byte_outputs`, `second_interrupt_aborts`).
- New flags that Java lacks: `--version`, `--capabilities`, `--progress json`, `--result-json`
  (the same field as Java's `--router.result_json=`), `--timeout`, `--max-passes`, `--threads`
  and `--seed`. The last four are aliases of settings and never add behaviour. `--threads`
  sets `autorouter.max_threads`, which Java's batch router ignores; the port sizes the job's
  analysis threads with it (DRC, statistics, airlines; P03.1), which changes no output. `--seed 0`
  means Java's own seeds; any other value exits 2 (`seed_zero_is_accepted_and_any_other_value_is_refused`).
  A `--timeout` over 24 hours is capped with a warning.
- `--progress json` writes every stderr line as a JSON object; the `progress` events are counted
  (phase change, end of a pass, every 50 connections), never timed (`progress_json.rs` pins the
  stream against a golden file). Java has no such stream.
- The result manifest has Java's field names and schema version 1, but keeps every key and writes
  `null` for the figures Rust has no source for (`cpu_seconds`, `total_allocated_gb`,
  `peak_heap_mb`, `resource_usage`, `cpu_score`, `fixture.sha256`); Java leaves null fields out.
  Only `phases.autorouter.before`/`after` (and `optimizer.after` when the optimizer ran) hold a
  board snapshot; the other phase snapshots are `null` (the pipeline reports the job, not each
  stage). It is also written when the run fails (`a_failed_load_still_writes_the_manifest`).
- With `--router.special.diagnostics=verdicts` the manifest gets one key Java does not have,
  `verdicts`, after Java's keys (Q06, `docs/development/verdicts.md`); without it the key is
  absent (`the_manifest_carries_the_verdicts_only_with_the_diagnostic`).
- With `--router.special.diagnostics=widths`, or `--router.neck_down.strict=true`, the manifest
  gets `widths` (Q02.1, `freerouting-widths/1`) after `verdicts`; without either the key is
  absent (`the_manifest_carries_the_widths_only_with_the_diagnostic`). With strict sizes a run
  that wrote its output exits 3 instead of 0 when some reported net has copper under its class
  width (KRT's `--strict-sizes`; `strict_sizes_exit_3_exactly_when_copper_ships_under_width`).
- Stopping: a stdin reader acts on `stop` (stop everything) and `stop-router` (Java's
  `requestStopAutoRouter`); end of input is not a stop (`stdin_eof_is_not_a_stop`). Java's CLI
  has no such channel. Ctrl-C and SIGTERM (SIGINT, SIGTERM and SIGHUP on Unix, Ctrl-C and
  Ctrl-Break on Windows, through the `ctrlc` crate) are handled: the first is `stop`, the
  second aborts with exit 130 and no output (`sigint_and_sigterm_are_a_stop`,
  `two_signals_abort`); Java's CLI has no such two-level stop.
- `--stats json` and `--stats-out <file>` (P01.2) are developer flags that Java lacks. They are
  taken out of the command line before the Java parsers see it, so they are not plugin flags and
  not in `--capabilities`. They write per-stage work counts and times and change no result: the
  session is byte-identical with and without them and the counts of two runs are equal
  (`stats_change_no_session_and_the_counts_repeat`).

## freerouting-gui

(The GUI is rebuilt with egui, not translated from Swing; only behaviour that Java
users would notice is listed. Texts follow the English Java bundles.)

**Info lists** (G03, `panels/{violations,length}.rs`)
- Java keeps the clearance violations of `WindowClearanceViolations` in a `TreeSet`
  ordered by shortfall (largest first), then layer, so violations with the same
  shortfall on the same layer compare equal and all but one vanish from the list (on
  `Issue575-drc_BBD_Mars-64_6_track_1_hole_clearance_violations.dsn` 67 violations
  show as 3 rows). The port keeps every violation, in the same order. The length
  violation list has the same fix for nets whose names are equal ignoring case.
  Pinned by `tests/ui_drc.rs::java_a11y_list_counts`.
- Unconnected traces and vias (`WindowUnconnectedRoute`): rows come from the engine's
  `all_unconnected_items()` (split nets, dangling traces, dangling vias), with Java's
  row text; Java lists groups of connected traces and vias that touch no pin or area.
- Zone islands have an Info list (Info > Zone Islands) that Java lacks, with the text of
  the DRC report. Info > Run DRC is new too: the check runs in the background after
  every edit, and Run DRC forces a run.
- Delete All Traces and Vias asks for confirmation and can be undone (U, Ctrl+Z, B,
  Ctrl+Y); Java does neither. Like Java it unfills the conduction areas and sets "ignore
  conduction" (`unfillConductionAreas`, B08.5); an undo restores the areas, which Java's
  does not (`tests/ui_edit.rs::delete_all_unfills_conduction_areas`).
- Zoom to the selection (W) fits the box of the selection plus 10 %; Java offsets the
  box by the largest trace half width.

**Board windows and display options** (G04, `look.rs`, `windows.rs`, `panels/{nets,components,library,layers,display,colors,status,log,about}.rs`; tests `tests/ui_windows.rs`)
- Info > Nets is in the Info menu; Java has it among the rules windows. Its Select on Board button is
  Java's double-click made visible (Java hides the button); the filter looks at `Net #n (name)` as
  Java's does, so a class name never matches. The nets window prints `Net.printInfo` for the chosen
  rows as one line each (the terminal items are counted, not a button that opens them).
- Placed Components lists every component (`Components.count()`, as Java does; the task text said
  "placed ones only", which is the same set on `Issue159`). Their windows are titled like the menu
  items, because Java's three windows have no bundle of their own.
- The object sliders of Display > Layer and Object Visibility are relative to the default look:
  100 % is the colour and alpha the canvas always had (planes at 0.35 alpha), where Java's own
  default for conduction areas and keepouts is 20 %. Only the ten kinds the port draws have a
  slider (traces, vias, pins, conduction areas, the three keepouts, component outlines, highlight,
  incomplete connections); Java's length matching areas and drill holes are not drawn. A level
  below 100 % mixes opaque shapes into the background instead of using alpha (the joints of a
  trace would show as dark dots); 0 hides. The dimming slider starts at 40 % (factor 0.6, what the
  canvas always used; Java's factor is 0.7, slider 30 %).
- The virtual layers are not dimmed when a copper layer is active (Java dims them by the same
  factor); the part numbers are drawn under the reference designator, in a smaller type, and only
  while the fabrication layer of their side is on (Java draws them at the component's location in
  a fixed 12 point font, whatever the zoom).
- The colour manager has no columns for fixed traces and fixed vias (the scene does not tell
  fixed items apart), nor for the component, length matching area and drill hole colours (nothing
  drawn uses them). The defaults are the port's own colours, not Java's table, except the courtyard
  and fabrication colours. Reset to Defaults is new.
- Rotation and mirroring turn the map from the board to the screen about the view centre, so the
  displayed section stays (Java keeps the viewport centre when it mirrors). Text stays upright and
  unmirrored; a label that follows the tall side of a footprint turns with a quarter turn.
- The status bar shows `Errors: n` and `Warnings: n` as words (Java: icons) and the cursor position
  in the design file's frame with Y up, in the chosen unit (Java prints `-y`, because its user
  coordinate runs down). The counters open the log window, which lists the loader warnings, the
  engine's log messages and the app's events (loads, design rules checks, exports), newest last.
  The unit label is Java's `Unit.toString`: `mil`, `inch`, `mm`, `um`.
- About has no donate, sponsor or telemetry line; it shows the version, the git commit
  (`FREEROUTING_GIT_COMMIT` at build time, `unknown` until R01.2 sets it), the license and links.

**Inspect mode, selection and safe edits** (G05, `tools/`, `panels/{select_settings,selection_toolbar,item_info}.rs`, `overlays/construction.rs`; tests `tests/ui_edit.rs`)
- **Delete, Fix and Unfix of the selection** (decision G00 question 6, accepted by Dan
  2026-10-16). Java's inspect toolbar has no such buttons at the pin: the edits it has texts
  for call the empty methods of `GuiBoardLegacyEditActions`. Delete removes the unfixed items
  after one snapshot and keeps the ones whose deletion is forbidden (items of components, user
  fixed items, power planes), saying `Some items are fixed and therefore could not be removed.`;
  Fix and Unfix set the fixed state. New Net, New Component, Cutout, Assign Clearance Class,
  Autoroute and Pull Tight stay out. Pinned by `delete_keeps_fixed_items_and_undo_restores`,
  `fix_and_unfix_set_the_fixed_state`, `toolbar_buttons_match_java_set`.
- Ctrl+click toggles the item under the cursor also when nothing is selected (Java toggles only
  while items are selected, where a plain click does it too, as here). Pinned by `ctrl_click_toggles`.
- Undo and redo work while items are selected (Java allows history in a menu state only, and
  an inspected selection is not one; its inspect toolbar has no undo). They are refused while a
  keepout is under construction, as in Java. Pinned by `undo_with_a_selection_works` and
  `undo_is_refused_while_a_keepout_is_drawn`.
- The selection toolbar floats over the top of the canvas while items are selected, so the
  canvas keeps its size (Java swaps the whole toolbar). A region starts where the button went
  down (Java: at the first drag event). Keepouts are completed with Enter (Java also uses Space,
  which pans here); Esc closes a menu first, then cancels a keepout or a pending zoom rectangle,
  then clears the selection.
- The Components button and menu entry have no (B) in their tooltip: B is redo, as in Java's key
  handler, whose tooltip is wrong. The item info window shows lengths and points in the unit of
  the toolbar's selector (Java: the design file's unit), puts a space after `padstack` in the line
  of a via, and shows the names of components, nets and padstacks as text, not as buttons that
  open their own info window. Pinned by `item_info_fields`.
- Keepouts are inserted with the engine's `check_shape` and `insert_obstacle` called from the GUI
  (`tools/keepout.rs`), not through the editor command of I03.4; Start route and Swap pin in the
  context menu are enabled since G09.

**Interactive routing and editing** (G09, `tools/{host,route,drag,move_component}.rs`, `overlays/route.rs`; tests `tests/ui_interactive.rs`)
- The app starts in route mode like Java, so a left click on a pin starts a route; the inspect tool is
  `S` (or the toolbar), and the G05 tests switch to it first. The toolbar has Inspect, Route and Drag
  buttons that Java's toolbar lacks (Java has the keys `S`, `R`, `D` only).
- Java completes a route with Enter or Space. Space pans here (G01), so Enter is the only key. Esc
  closes a menu first, then cancels a session, a keepout or a pending zoom rectangle, then clears the selection. `P` toggles push-and-shove and
  `T`, `W`, `R`, `D`, `S` act only without a selection, as Java's menu states do (a selection is
  `InspectedItemState` there, where `W` zooms and `S` extends to connected sets).
- The GUI keeps its own selection (G05) and the engine's `Editor` holds none, except for a move or a
  copy. Undo and redo run through `Editor::apply`, which refuses history while it holds a selection,
  as Java does; the GUI clears the editor's selection first and keeps allowing undo with a selection
  (the G05 line above), and refuses it, as Java does, while a route, drag, pin swap, move or copy is
  under way. Pinned by `undo_clears_the_editors_selection_but_not_the_guis`.
- The route popups' texts (`end_route`, `cancel_route`, `change_layer`) have no English bundle at the
  pin; they read "End route", "Cancel route" and "Change layer". The current layer is disabled in the
  submenu as Java's `disableLayerItem` does.
- Make-space asks for a pointer warp when the board has no angle restriction (Java's
  `moveMouse`); egui cannot move the operating system pointer, so the warp is ignored and the
  drag goes on from where the pointer is. Dragging a user-fixed item shows no
  "Please unfix item before dragging" because the editor returns no status for it; it simply does not
  move. `E` (`ExpandTestState`, a debug state) is not ported.
- Move (`M`) and copy (`Ctrl+D`) of the selection, `+`/`-`/`/` to turn and mirror it while it floats,
  the popup of a floating selection and the "Component Movement Settings" are new: Java's
  `MoveItemState` and `CopyItemState` cannot be entered at the pin (I00 decision 1, G00 question 6).
  `*` (180 degrees) is Shift+8. The wheel setting of `WindowMoveParameter` (zoom or rotate) is not
  ported.
- The interactive part of Settings > Routing keeps the pull-tight radius as a checkbox ("entire board")
  and a field in the display unit, where Java has a slider whose maximum is the whole board. The
  manual rules (trace width per layer, clearance class, via rule) belong to the open board and are not
  stored with the settings, as in Java's workspace settings. A trace width becomes
  `round(userToBoard(0.5 * width))`, as in G06.
- The pointer reaches the engine as the board point under it, unrounded; the editor rounds as Java does.
  A frame that moves the pointer sends one move, not one per mouse event, so a fast drag takes fewer,
  larger steps than Java's. Pinned by the drag tests, which replay the pointer positions on an
  `Editor` that owns a copy of the board.
- While a session is under way the background DRC and ratsnest wait (`Analysis::set_held`); they run
  when it ends. The airlines on the screen are those of the board before the session.

**Rules dialogs** (G06, `dialogs/`; tests `tests/ui_rules.rs`)
- Net Classes > Remove checks every net for the class. Java's loop (`for i = 1; i < maxNetNumber`,
  and `maxNetNumber()` is the net count) stops one net short, so a class that only the last net
  uses can be removed and the net is left with a class that is not in the list. No routing effect.
  Pinned by `remove_used_class_refused`.
- Net Classes > Select selects the items of the nets of any chosen class. Java compares the item's
  net class with `selectedClassArr[i]`, where `i` is the item's net index (a Java typo for `j`), so
  it only ever matches the first chosen class and throws for an item with more nets than chosen
  classes. Pinned by `select_takes_the_items_of_all_chosen_classes`.
- Lengths in the dialogs (trace width, minimum and maximum trace length, clearances, pad-to-turn
  gap, via radii) are shown and typed in the display unit of the toolbar (G04.6); Java uses the
  design file's unit. The conversion to board units is Java's: `Math.round(userToBoard(..))`, a trace
  width becomes `round(userToBoard(0.5 * width))`.
- The Net Classes table has no row selection model of its own: a click on a class name chooses the
  row (Ctrl or Cmd adds), a double click renames it. Show Nets opens a "Contained Nets" window with
  the net info lines of the chosen classes (Java's `WindowObjectInfo`). Filter Incomplete
  Connections shows the airlines of the chosen classes' nets only, until "Show all connections".
- Create Via Padstack is one form (name, start and end layer, default radius and a radius per layer)
  where Java asks in a sequence of prompts; a name that exists is refused in the form, as Java asks
  again, and an empty name is refused, which Java accepts.
- The Via Rules window's Info buttons list the padstacks, vias or the rule as text lines, not Java's
  `printInfo` texts of padstack shapes.
- The clearance matrix shows `-1` for a layer-dependent cell on "all layers" or "inner" and the
  value as a Java `float` otherwise (Java's table renderer rounds to three fraction digits).
- Rule edits are not part of the board history: U and B restore items and components only, as in
  Java (rules are not in `UndoableObjects`, and the outline keepout, the conduction obstacle flags
  and the fixed state of traces that these edits change are changed in place without a saved
  version). G06's decision that "U undoes a rule edit" can't be met without engine work.

**Dropped Java features** (G00 question 2, accepted by Dan 2026-10-15; recorded 2026-10-16)
- `.frb` snapshots (Java serialization, `BoardSnapshotManager`, `SafeObjectInputStream`):
  not ported. DSN + SES + `.rules` carry everything they did; the GUI's own session state
  is G01's persistence.
- The Autodesk Fusion `.scr` export (`SessionToFusion`) and the KiCad session JSON export
  (`KiCadJsonWriter`, `KiCadBoardJson`): not ported. The plugin imports the SES.
- The tutorial board, Sample Designs and Router Demonstrations: not ported; Open Recent
  replaces the tutorial.
- The telemetry, sponsor, e-mail and user-profile dialogs (`WindowUserSettings`: user id,
  e-mail, consent flags, usage statistics) and everything under `analytics`: not ported.
  No data leaves the machine.
- The router's debug play/pause/step toolbar (`debug/DebugControl`): not ported as such;
  a developer-only stepping view may come after the A series (G00 question 5).
- The headless `--compare-boards` and `--calculate-benchmark-scores` commands
  (`BoardComparator`, `BenchmarkAndCompareCommands`): not ported; `freerouting-dev ses-diff`
  is the comparison tool (C07.3).

**Autorouter run** (G07, `routing.rs`, `app_routing.rs`, `dialogs/autoroute_settings.rs`,
`panels/{progress,routing_summary}.rs`, `overlays/routing.rs`; tests `tests/ui_routing.rs`)
- The GUI draws a running job by patching chunks from the router's delta events (the geometry of
  the added traces and vias travels in the event) once per frame, where Java repaints the shared
  board on every `BoardUpdatedEvent`. A `Replaced` board (a history restore, the final best board)
  and the `Finished` board are compared with the drawn scene item by item and only the differences
  are patched; the scene is not rebuilt. Pinned by `final_scene_equals_fresh_build` and
  `live_redraw_patches_before_the_job_ends`.
- The job routes a copy of the session board (the session keeps its own until `Finished`), so the
  panels read the board the job started from while it runs. Java routes the shared board and makes
  it read-only. Edits are refused with a status message in both (`edits_are_refused_while_routing`).
- The autoroute settings dialog edits the engine's settings patch: the design's `autoroute_settings`
  and the board tuning of the layer costs come first, then the values the user applied (only values
  that differ from them are kept between runs). Java's window has no control for `start_pass` and
  `speed` (its bundle has the labels), so the dialog has none. The "Optimization" stage checkbox is
  enabled (the batch optimizer, O06, is ported). Fields without a control are listed with a
  reason in `CLI_ONLY` (`every_router_setting_has_a_control`).
- The routing summary is a window, not a modal dialog, and has no sponsor message or button (G00
  question 2). "Vias Added" is the board's via count and the trace length the board's total, as in
  Java; the figures are computed by the job thread from the returned board.
- Cancel is a full stop of the engine's cancel token: the pipeline finishes the step it is in and
  hands back the best board (D8); Java's `requestStop` ends the thread the same way. Esc, the
  toolbar's Cancel and a click on the canvas all stop it (Java: "Click anywhere on the canvas").
  After a stop request the status line and the progress panel say the router is stopping (Java
  keeps "Autorouter is running" until the thread ends), and the log records how long the stop
  took. A frame reads at most `EVENTS_PER_FRAME` router events, so a router that sends as fast as
  the window reads can't keep the window from redrawing or seeing input.

**Saving, the questions and the front-end mode** (G08, `files.rs`, `app_files.rs`, `dialogs/autostart.rs`, `file_dialog.rs`; tests `tests/ui_files.rs`)
- The Save button of the unsaved-changes question (open and drop) saves: it opens Save As and loads
  the new file only after a successful save. Java's `BoardFrame.confirmSaveBeforeLoad` logs and loads
  without saving ("would be handled by the file menu"). File > Open and the recent files ask too;
  at the pin only a dropped file does. Pinned by `drop_with_changes_save_then_load`.
- The Exit question counts a running autorouter job as a change (its result is not in the board
  yet). Pinned by `close_with_changes_asks` (the job case is the same code path).
- "Changed" is Java's definition (`isBoardChanged`: the CRC32 of the board written as DSN against the
  one at load or at the last save), behind the cheap edit counter, so an undone edit is clean
  (`undo_to_saved_state_is_clean`). A saved DSN counts as saved too; Java updates the checksum only
  after a session. A load with a session on top is the clean state (Java's checksum is taken before
  the session is imported, so the opened board is already "changed" there).
- Save As (`Ctrl+S`, the only save as in Java) offers `.ses` and `.dsn` only (G00 question 2). The
  rules question follows a session only, default No, as in Java; a `.rules` file written by it has no
  `autoroute_settings` scope (Java's `saveRulesAs` passes none either).
- The rules file next to the design is read at start only (`-de`, a path on the command line), as in
  Java; File > Open does not look for it. In the front-end mode with `-dct 0` the question is not
  asked and the file is read, as the CLI does (Java would block on the question). `-dr <file>` is
  read without a question and without Java's "DSN generated by the host" condition (which Java
  applies to its question; with `-dr` the question has no text). Pinned by
  `rules_next_to_dsn_imported_after_confirm`, `dr_flag_imports_without_asking`,
  `file_open_does_not_look_for_rules`. The import is an edit of the board's rules, which like every
  rule edit is not in the undo history (Java's `RulesReader` is not undoable either).
- Front-end flags: `-de`, `-do`, `-dr`, `-dct`, `-di` keep Java's meaning. The router flags are
  the CLI's, read by the same engine code (`settings::cli_flags::parse_router_flags`, K02.5): `-mp`,
  `-mt`, `-inc`, `-oit`, the scoring flags, `--router.<path>=`, `--optimizer.<path>=` and the
  aliases `--max-passes`, `--threads`, `--timeout`, `--result-json`; they are laid over the job's
  settings (over the saved ones). `--result-json` and `--timeout` are read but the interactive
  job writes no manifest and stops on no time limit; `--seed` (a CLI flag) is not read and any
  other unknown flag is refused with exit 2. `-mp` is clamped to 0..=9999 as in Java, and a value
  never starts with `-` (`-mp -1` is an unknown option, as on the CLI). `--autoroute` is
  `-dct 0`. Java writes only the first `-do` file; so does the port.
- The process exits with 1 when the front-end write failed or the design did not load, and with 0
  after a written file. Java's `exportBoardToFile` exits 0 after "not overwriting" a 0-byte
  result and does not exit at all after an exception. The window stays open after a failed load so
  the error can be read. Closing the window in the front-end mode stops the job and writes the
  output without a question, like Java's `windowClosing`, and waits for the job to hand back its
  board before it closes. If that stop takes longer than `STOP_GRACE` (3 s), closing again quits
  without waiting and writes nothing, so the plugin imports nothing (Java has no way out short of
  killing the process). Pinned by `frontend_failed_write_exits_with_1`,
  `frontend_writes_on_close_when_routed`, `frontend_close_again_after_a_slow_stop_quits_without_waiting`.

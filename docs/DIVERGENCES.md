# Divergences from Java Freerouting

Places where the Rust port deliberately behaves differently from the Java reference
(`danbrunski/freerouting` at `cda540f`). Anything not listed here is meant to match
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
  effect; the port has no such cache. The `state-hash` probe found no step where this
  changes an equality (`tests/board_probe_state_hash.rs`).
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
  while the groups are the same. `NetIncompletes` starts each connected group from the
  first remaining item in board order.
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
- The clearance violations and island fills are computed on all cores. The results are
  merged in item order, so the output does not depend on the thread count.
- The DRC command line (`freerouting -de board.dsn [board.ses] -drc [report.json]`)
  does the post-load processing before importing the session. Java runs the post-load
  DRC in a background thread while it imports the session. On
  `Issue289-Autorouter_PCB_FHT-VGA_2024-03-25.dsn` that race leaves net +5V in the
  outline's edge-pin-net cache and hides 6 trace-to-outline violations.
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
- The D9 stage keys (A10.2) are settings fields Java does not have, unset by default
  (unset uses the `autorouter.algorithm` bundle): `fanout.algorithm`,
  `autorouter.{ordering, connection_router, ripup_policy, pass_controller}` and the
  `special` section (`plane_taps`, `neck_down`, `stages`). An unknown id in them is an
  error naming the key and the known ids; the two Java keys keep Java's warn-and-default
  (`autoroute_batch_control.rs`, `unknown_per_stage_id_is_an_error_naming_the_key_and_the_known_ids`).
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
  Ctrl+Y); Java does neither. Until the engine ports `unfillConductionAreas` the
  conduction areas are not unfilled and "ignore conduction" is not set, as Java does.
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

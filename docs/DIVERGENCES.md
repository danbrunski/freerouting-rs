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
  that need the board are `BasicBoard` methods taking an id. Observers, the routing
  ledger, Java serialization, `getHash` and the graphics update box are not ported.
- `ShapeTree` stores leaves in an arena; `overlaps` returns them sorted by item and
  shape index, as Java sorts them. Combining traces removes and reinserts the tree
  entries instead of Java's `mergeEntries`; queries return the same items, only the
  tree's internal shape differs.
- `PolylineTrace.change` compares lines by value to find the unchanged part; Java
  compares references.
- Java iterates the nets in `normalizeAllTraces` in `HashMap` order; the port simulates
  that order, but without the treeification of buckets with 9 or more keys.
- Stale `LeafId`s and `ItemId`s cannot be used after removal; Java's locking tests
  (`MinAreaTreeLeafLifecycleTest`, `ShapeSearchTreeLeafLifecycleTest`) have no
  counterpart because the borrow checker rules out concurrent reads and writes.

**Board statistics** (`scoring`, port of `BoardStatistics` and
`BoardStatisticsBoundsCalculator`)
- Only the V2 router and optimizer scores (Java's defaults) are ported; the legacy
  formulas need the router's cost settings. The constructor from a KiCad JSON file is
  not ported.
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
- Rules files (`-dr`, `board.dsn+board.rules`) are not supported. The runner logs a
  message and ignores them.
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
- Floating point text: hexadecimal floating point literals (`0x1p3`) and Unicode
  decimal digits in `Integer.parseInt` are not accepted (`settings_text.rs`).
- A rules file's `autoroute_settings` build their patch from the layer names found in
  the rules file, as `RulesReader.discoverLayerStructure` does; a non-integer value in
  a DSN or rules `autoroute_settings` integer scope is left unset (see freerouting-dsn)
  where Java sets 0 and then clamps it to 1.

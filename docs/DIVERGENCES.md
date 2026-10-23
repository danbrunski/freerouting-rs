# Divergences from Java Freerouting

Places where the Rust port deliberately behaves differently from the Java reference
(`danbrunski/freerouting` at `a773a70ec`; entries that name `cda540f` describe Java at
the previous pin). Anything not listed here is meant to match
Java; a difference that isn't listed is a bug in the port.

Each crate's docs carry the full detail; this file is the index.

## freerouting-geometry

**Java bugs fixed**
- N06.2 (both profiles): `Line::length` uses floating-point deltas and cannot overflow
  Java's squared `int`; `stair_approximation_45` uses the inverse function in its y branch;
  `index_of_nearest_corner` starts its minimum at infinity; `PolygonShape::area` computes
  the area of 2-D polygons. Tests: `function_values_and_length`,
  `stair_approximations_like_java`, `nearest_corner_uses_the_smallest_distance`,
  `normalizes_orientation_and_start_corner` and `l_shape_is_split_into_convex_pieces`.
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
- K15.1 (both profiles): `IntBox::offset` saturates at `i32::MIN` and `i32::MAX`. Java's `int`
  sums wrap (`IntBox.java:448-456`), so the box of an unbounded shape, whose infinite corners
  round to the limits, came out empty when offset outwards; inside the limits the result is
  Java's. `IntBox::has_infinite_coordinate` tells such a box. Tests
  `tests/int_box_limits.rs`.

**Stricter than Java (panic instead of a later failure)**
- `Line` accepts only integer points; out-of-range `LineSegment`/`Polyline`
  constructors and `Polyline::corner()` on fewer than two lines panic.

**API shape**
- `null` returns become `Option`; indices are `usize` and "-1 = not found" is `None`.
- IntOctagon `is_empty` compares by value; its `toSimplex` cache is gone. Logging is gone.
- `sin`/`cos` may differ from Java's `Math` by one ulp.

**Java bugs kept on purpose** (changing them would change routing results; N06 sorts every
kept bug and schedules its fix under profile `enhanced`; see
[N06](roadmap/tasks/archive/N06-fix-kept-java-bugs.md)): `Simplex.cutoutFrom` never setting its
previous division line (kept under `legacy` only: profile `enhanced` assigns it,
`router.java_quirks.simplex_prev_division_line = false`, N06.4, promoted by N06.13's second round;
the pieces are Java's either way), the no-op branch 3 in IntOctagon's box cutout
(`octagon_box_cutout_branch3`, `true` in both profiles: N06.14's run at every `Area` split
rejected its fix; see "Java quirk switches, opt-in" below), and the two simplex cutout bugs
outside N06.4's domain, a hole crossing the outer border and an undivided half-integer corner
(kept under `legacy` only: profile `enhanced` fixes both,
`router.java_quirks.cutout_hole_crosses_border` and `simplex_octagon_half_corner = false`,
N06.14, promoted by N06.13's third round). `RationalPoint.perpendicularProjection`
is kept under `legacy` only: profile `enhanced`
fixes it (`router.java_quirks.rational_perp_projection = false`, N06.7, promoted by N06.13).

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
- A wire's `(attr a)` is read into `Wire::attr` and written back (Q22.4); Java skips it.
  `(attr obstacle)` (`ast::WIRE_ATTR_OBSTACLE`) on a conduction-area wire is this port's own
  value, written only by the KiCad plugin and the `.kicad_pcb` reader for a copper shape of a net
  (see freerouting and freerouting-kicad below; `the_written_dsn_keeps_the_areas`).
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
- A conduction-area wire with `(attr obstacle)` (Q22.4, `parser/wiring.rs`) becomes a
  conduction area that is an obstacle to the traces and vias of other nets
  (`BasicBoard::insert_fixed_copper_area`), and `router.plane_as_obstacle` never clears it
  (`RoutingBoard::change_plane_as_obstacle` skips it), and two such areas of a net that meet are
  in contact (`conduction_area_normal_contacts`; Java's areas touch only traces, pins and vias), so
  the pieces of one stroked shape are one copper. Java skips the attribute and reads the wire as
  a conduction area other nets may cross. Only the KiCad plugin and the `.kicad_pcb`
  reader write the attribute, for a copper shape of a net; a DSN without it reads as before.
  Pinned by `tests/copper_graphics.rs` (`plane_as_obstacle_never_clears_a_net_shape`,
  `the_dsn_text_gives_the_same_areas`, `the_pieces_of_a_stroked_shape_are_one_copper`).
- Where Java ends with an exception the port keeps going: keepouts, planes and windows
  whose area does not resolve are dropped (NullPointerException); circles and polyline
  paths in the boundary are left out of the outline (ClassCastException); a wire on an
  unknown polyline-path layer is dropped. A file without a structure scope is a
  `ParseError` (Java returns a null board or throws).
- The placement scope's `resolution` and the pcb-level `unit` are ignored, as in Java, under
  `legacy`; profile `enhanced` honours them (`router.java_quirks.dsn_unit_ignored = false`,
  N06.10, promoted by N06.13).
- The scope identifier in warning messages is the net or padstack name; Java prints the
  scanner's current identifier.
- Logged messages go to `BasicBoard::log_messages`; only the wiring warnings are
  returned, as in Java.
- Kept under `legacy` ([N06](roadmap/tasks/archive/N06-fix-kept-java-bugs.md); profile `enhanced`
  fixes both, `dsn_via_alias` and `dsn_class_class_pairs`, N06.10, promoted by N06.13): the
  `via_padstack_names` aliasing with the first class's `use_via` list, and class-class rules
  pairing only the first known class with the others (Java shares one iterator).

**Board model**
- Java bugs fixed outright in N06.2: outline transforms store the transformed shapes as
  well as the generated keepout (`edge_pin_cache_invalidated_on_outline_transform`);
  approximate trace rotation clears derived autoroute info
  (`trace_rotation_clears_derived_autoroute_info`). Both profiles use these corrections.
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
- Java bug kept under `legacy` (N06; profile `enhanced` fixes it,
  `router.java_quirks.snapshot_drops_compensation = false`, N06.11, promoted by N06.13): a
  snapshot restore or deep copy rebuilds the default tree without clearance compensation and drops the compensated trees, even when
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
- K15.1 (both profiles): `expand_bounding_box_to_include_all_items` leaves out an item whose
  box has a coordinate at `i32::MIN` or `i32::MAX` (an unbounded shape) and returns a warning
  naming it, which the job logs: "The fabrication outline of component C65 at (57251.006,
  -105242.039) um has an unbounded shape after placement; it is left out of the board's
  bounding box." (`board::unbounded_items`). Bug 1 of
  `docs/development/pcu-rev2-board-report.md`: C65's F.Fab circle, placed at y = 105.242039 mm,
  rounds to a non-convex polygon and `PlacementTransform::shape` builds a simplex with three
  parallel lines from it (`router.java_quirks.placed_shape_convex = false` takes the convex hull
  instead, K15.2; unset in both profiles). Java at the pin takes the union, `IntBox.offset(1000)` wraps,
  the board box is empty, every room completion fails, and Java aborts in `DrillPageArray`
  with a `NegativeArraySizeException`; the port routed nothing on such a board. A board with no
  such item gets Java's box. Tests `tests/unbounded_item_box.rs` and
  `freerouting-cli/tests/cli_unbounded_outline.rs` (the report's repro, a board with nets far
  from C65, and KiCad's own DSN export of both, in both profiles).

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
- The DRC command line (`freerouting-rs -de board.dsn [board.ses] -drc [report.json]`)
  does the post-load processing before importing the session. Java runs the post-load
  DRC in a background thread while it imports the session. On
  `Issue289-Autorouter_PCB_FHT-VGA_2024-03-25.dsn` with the `cda540f` route session that
  race left net +5V in the outline's edge-pin-net cache and hid 6 trace-to-outline
  violations; with the `a773a70ec` session Java and the port report the same.
- A routing job's pre-existing clearance violations (`BasicBoard.preExistingClearanceViolationsCount`,
  the statistics' `pre_existing_count`, which `router_introduced_count` subtracts) are counted
  once, on the board `prepare_job` (and the GUI's `prepare_board_for_routing`) hands to the
  router: after the rules file, the session import and the Rust-only `router.wire_keepout`,
  `router.pad_contacts` and pour guard. Java counts them in a background thread started
  after the overrides, which races the rules file and the session import. Without those the
  count is Java's (`tests/pre_existing_violations.rs`). Before 2026-10-19 the port never
  counted them, so every violation of a loaded board was reported as router introduced.
- The session reader imports the wires of `Issue676-ch32v-tx118s.dsn` (1 000 000 units
  per mm). Java's `SesReader` fails on all 48 with a ClassCastException
  (RationalVector to IntVector) and imports none.
- The report's `date` is in UTC; Java uses the local zone. `freeroutingVersion` is
  `rs-<crate version>`.
- `RoutingBoard::reduce_nets_of_route_items` reports whether a net was removed.
- **The report counts what KiCad counts** (K15.5, `drc::kicad_report`, `ReportRules::Kicad`, the
  default of `DesignRulesChecker::generate_report`, `-drc`, a route's `.drc` output and the GUI's
  export). Java's types and texts stay; three counts change, under both profiles:
  `clearance`/`holeClearance` leave out a pair whose items share a net unless one is copper the
  router placed (a trace or via neither user nor system fixed): every `.kicad_pcb` padstack is
  `(attach off)`, so Java's `Item.isObstacle` makes each via in or beside a pad of its own net a
  violation, which KiCad never reports; a via the run put in a pad stays reported (D8).
  `via_dangling` is a via whose copper meets its net on fewer than two layers
  (`stub_contacts::via_contact_layers`), not Java's `isTail` (contacts only at its centre), so a via
  joining an SMD pad off its centre to a plane is connected. `track_dangling` is a trace with an
  end point on no copper of its net on its layer (Q20's anchor test), so a T junction or an end
  inside a pad off its centre is connected. The split nets, the scores, `check()` and
  `all_unconnected_items()` (the GUI's lists) stay Java's. On the pcu board (K15.5) the counts go
  from holeClearance 165, via_dangling 117, track_dangling 15 to 44, 17, 2. Pinned by
  `tests/drc_kicad_report.rs`; `tests/reference_drc.rs` compares `ReportRules::Java` with Java.
  K15.11 (`drc::kicad_report`, `drc::kicad_copper`, `drc::kicad_rules`), under `ReportRules::Kicad`
  only: a kept clearance pair is measured again on the items' own copper (round trace ends,
  circles; Java measures the search-tree octagons) and is a violation only if closer than the
  clearance by more than `clearance_tolerance_um`; its type is `clearance` (no `holeClearance`), once
  per pair and layer as KiCad 10.0.7 reports it. Where a `.kicad_pcb`'s `.kicad_dru` (or `--kicad-dru` on a
  DSN input, for a route's DRC and for `-drc`, K15.17) has an unconditional `clearance` rule and no conditional one, a copper pair
  is measured against the rule's `min` raised to Board Setup's `min_clearance`, not the net-class
  clearance (KiCad's rule replaces the class value; the router keeps the larger, K15.3). A new type
  `hole_clearance`: a trace, via or pad closer to a drilled hole of another net (a via's, a plated
  pad's, an NPTH, read from the `.kicad_pcb`: `KicadInput.extras.holes`) than Board Setup's
  `min_hole_clearance` raised by the `.kicad_dru`'s `hole_clearance`, unless the pair is already a
  `clearance` finding on that layer; a DSN board has no pad holes. An NPTH's own pad (KiCad's
  export makes a copper-less NPTH that is not round, J10's oval slot on wisweep's driver board,
  a pin whose pad is the hole) is no copper against its own hole (K15.19, the pin at the hole's
  centre is skipped; it gave 4 findings of 0 mm, one per layer:
  `tests/k15_wisweep_edge_hole.rs::an_npth_slots_own_pad_is_no_hole_clearance`). `via_dangling` and
  `track_dangling` count a signal-layer pour the reader drops (a net with a power-layer plane, Java
  `Structure.insertPlanes`; recorded as `BasicBoard::dropped_pours`, no item) as copper of its net.
  `track_dangling` follows KiCad's `TestTrackEndpointDangling` per segment of the design (a board
  trace split at its corners and at the wire corners the reader drops as collinear,
  `BasicBoard::wire_joints`): a segment whose both ends lie on one other track is met only at the
  nearer end, and a dangling trace that represents a split net's group is reported too (Java leaves
  it out). On the pcu board (`76163c7`) `-drc` goes from clearance 78, holeClearance 276,
  via_dangling 248, track_dangling 1 to clearance 0, hole_clearance 4, via_dangling 0,
  track_dangling 0 (KiCad 10.0.7: `hole_clearance` 4); on Issue575 Natural Tone Preamp the 5
  `track_dangling` are KiCad 9.0.1's 5. Pinned by `tests/drc_kicad_pcu.rs` and
  `tests/drc.rs::issue575_unconnected_items`. The scores and `check()` stay Java's.
  K15.16 (`drc::kicad_connectivity`), under `ReportRules::Kicad` only: the split nets and the zone
  islands are KiCad 10's connectivity, not Java's `getAllUnconnectedItems` (every item an area's
  outline touches is joined; contacts at centres and corners) and `getZoneIslandViolations`. A
  zone's copper is its fill, one node per island: the `.kicad_pcb`'s `filled_polygon`s while they
  are current (every zone filled and the board's `state_hash` unchanged since the read:
  `KicadReportRules::fills_current`, so `-drc` on a `.kicad_pcb`), else the router's fill model
  (each conduction area's detailed fill, a pad or via within the clearance of an island joining it;
  a dropped pour's outline); a DSN carries no fill. Items join where their own copper touches on a
  common layer, a pad or via joins an island it overlaps, a trace one that contains an end of one
  of its segments. An island whose group has no pin is isolated: removed as its zone's
  `island_removal_mode` says (0 always, 1 never, 2 below `island_area_min`; a DSN zone takes KiCad's
  default 0) unless all islands of the zone on that layer are isolated (KiCad 10.0.7 then keeps
  them all); a kept one is an `isolated_island_dead_copper` finding, and there is no
  `isolated_island_unconnected`. A net of `n` groups has `n - 1` `unconnectedItems` (KiCad's
  ratsnest edges; Java one per split net), a kept isolated island alone being no group. On the
  pcu board (`76163c7`) `-drc` goes from unconnected 74 (31 split nets, 43 islands) and
  `isolated_island_dead_copper` 5 to 0 and 0 (KiCad 10.0.7: 0 and 0); on Issue575 Natural Tone
  Preamp from 44 to 7, the 7 nets of KiCad 9.0.1's report. Pinned by `tests/drc_kicad_fills.rs`
  (small boards against `kicad-cli pcb drc --refill-zones`), `tests/drc.rs` and
  `tests/drc_kicad_report.rs`.
- `-drc` reads no router settings, as Java's `DrcRunner`; the CLI now warns once per router
  setting given on the command line or in the environment (K15.5, `tests/drc_settings.rs`).
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
- A command-line `optimizer.improvement_threshold` (`-oit` or
  `--router.optimizer.improvement_threshold`) in (0, 1) is scaled by 100 once. Java's CLI
  scales it and the optimizer's fraction heuristic then scales a result below 0.1 again, so
  Java reads `-oit 0.0005` as 5 % and the port as 0.05 %, in both profiles (a port fix,
  N06.12; `SettingsPatch::improvement_threshold_scaled`,
  `a_cli_threshold_below_a_thousandth_is_scaled_once`, `a_cli_threshold_is_scaled_once`). No
  A14 board, Java reference case (`-oit 0.035`, `4.0`, `0.05`) or plugin option (1-100) passes
  such a value.
- `min_trace_width_um` (Java fork commit `46bf9e2ef`, after the pin) is a settings
  field, unset by default.
- `profile` (`router.profile`, N01) is a settings field Java does not have: `legacy` or
  `enhanced`, unset is `enhanced`. The merge adds the chosen profile's differences as a source at
  priority 5, between the defaults and the settings file, so any explicit key wins; an unknown
  name is a settings error naming the valid ones. With `legacy` (and while `enhanced` has no
  entries) the merge is Java's (`tests/settings_profile.rs`); see "Profile `enhanced`".
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
- `plane_obstacle_all_pours` (K15.13, `quality::pour_guard`) is a settings field Java does not
  have, unset by default (no policy, Java's routing; `legacy` and `enhanced` leave it unset).
  `true` lists every signal-layer pour for the pour guard whatever its net class, with Q01's
  exemptions (a foreign net with a pin or input via inside the pour is not blocked), so a board
  whose pours have no class of their own (one net class) needs no class names. Java at the pin
  inserts every DSN plane as a conduction area that is no obstacle (`Structure.java` 1141) and
  counts it as one connected item however foreign traces cross it; KiCad's refill cuts it into
  islands. On the pcu board (`../power-control-unit` `76163c7`, tracks stripped, plugin default
  options, `--max-passes 2 --timeout 240`): `.kicad_pcb` input KiCad unconnected 96 -> 26,
  Zone-Zone items 28 -> 6, router incomplete 65 -> 23; DSN input 55 -> 31, 38 -> 7, 12 -> 26;
  canary-pwr (`95882f00`, stripped) 40 -> 36, 18 -> 7, 27 -> 38 and 38 -> 36, 17 -> 7, 24 -> 39
  (`tests/quality_pour_guard.rs`, `with_every_pour_listed_s_goes_around_and_the_pour_stays_whole`).
  Not made a default (K15.14, preregistered, decided 2026-10-22): over 16 corpus rows the
  `.kicad_pcb` arms' KiCad unconnected went 368 -> 493 against a set noise of 67.09 (pcu 47 -> 20,
  canary-pwr 40 -> 36, but pcie-active-backplane 5 -> 83 and openmuscle 7 -> 28, whose large
  pours leave the other nets unrouted), so `enhanced` leaves it unset too. The KiCad plugin
  offers it as a per-board checkbox (Advanced tab, Flow group; `RouteOptions.plane_obstacle_all_pours`,
  off in both profiles; `tests/test_options.py`, `tests/test_route_dialog.py`).
- `power_layer_traces` (Q13, `autoroute::power_layer_traces`) is a settings field Java does not
  have, unset by default (`layer`: Java `7f56b062d`'s rule, a net with a plane on a power layer
  may route anywhere on that layer). `own_plane` lets the maze search lay a trace of net N on a
  power layer only inside one of N's planes there: the one-room hop from the entry to a target
  door is expanded only when the entry segment and the connection point lie in one convex piece
  of N's plane, in the board's own geometry (a via landing in the plane is such a hop); room
  shapes are unchanged. On `tests/data/quality/q13-two-islands.dsn` Java's rule joins PB's outer pin to its
  island with a trace on In1.Cu that leaves the island, and `own_plane` routes it on B.Cu with a
  via landing in the island (`tests/power_layer_traces.rs`); an unknown value fails the job,
  naming the key. Off by default; Q13.3 measured it within N02's totals rule with the plugin's
  default flags (KiCad unconnected canary-pwr 68 → 68, m-fc-g474 5 → 5, microzed 0 → 0; plane-net
  traces outside their plane 5 → 0). N05 made the stricter `none` `enhanced`'s default instead
  (see "Profile `enhanced`").
  `none` (N05) lays no trace of any net on a power layer, as canary's validity check
  (`inner_tracks.py`) demands: a hop there is expanded only when it has no length and lies in one
  of the net's planes (a via landing in its plane); vias through the layer are unchanged. On the
  Q13 fixtures it routes as `own_plane` does (no trace on In1.Cu, PB's via inside its island), and
  `hop_allowed` refuses the in-island trace `own_plane` allows
  (`tests/power_layer_traces.rs`, `none_allows_only_a_hop_without_length_inside_the_plane`).
  K15.18: under `own_plane` and `none` the pull-tight after a connection (and the pass's tail
  removal, the fanout's) does not move a via between two traces onto a conduction area of its net
  that it does not touch, on a layer neither trace is on (`optimize/via.rs`,
  `opt_via_location_keeping_planes`; `PullTightParams::keep_vias_off_new_planes`); Java's
  `optViaLocation` moves it, and `layer` keeps that. A plane on a trace's layer still takes the
  via: that trace runs into the plane and the move replaces it (Q13's fixtures,
  `tests/power_layer_traces.rs`, unchanged). With a plane cut by a window (K15.15) a connection
  runs on the signal layers through a via in the window to a via in the plane; moving the first via
  into the plane made a cycle through the plane, and the cycle removal (`remove_if_cycle`) took the
  connection, both vias included, off the board in every pass. On K15.15's fixture `enhanced`
  goes from 1 open to 0 with J1's pad alone (its plane via at (180.347, 44.9141) mm, right of
  V_BUS_24V's window) and with J1's input track and via in the window
  (`tests/k15_overlapping_planes.rs`, `the_router_reaches_the_lower_plane_where_it_is_filled`,
  `the_via_in_the_overlap_stays_open_under_legacy`). The pcu board's `.kicad_pcb` route under the
  plugin's default options is byte-identical to main's (session `a2d99014…`, KiCad unconnected 47).
- `pour_fill_target` (K15.25, `autoroute::pour_target`) is a settings field Java does not have,
  unset by default and in both profiles (Java's rule: the maze search reaches a conduction area
  anywhere inside its outline, its tree shapes). `true` (opt-in) makes the search reach a signal-layer
  pour of a `.kicad_pcb` input (a zone of the file; the areas K15.23's `PourFills` judges by
  their opened fill) only in the core of that fill, the router's fill model on the board as the
  connection's search begins eroded by half the zone's `min_thickness` less 1 µm: a target door to
  the pour is expanded only where its tile within the room meets the core in two dimensions, its
  connection point is the nearest point of those parts, the located path ends at Java's point if
  a part holds it and else at the centre of gravity of the nearest part, and a start door whose
  room meets no core is not queued. The board's cycle rule follows it: Java removes a trace both
  of whose ends lie in one conduction area of its net (`PolylineTrace.split`, `Trace.isOverlap`),
  so the insertion cut a found path back to its first corner inside the outline, outside the fill;
  under `true` such a trace is kept only as a bridge into the fill: the core holds one end and the
  other end is not at a pin or via that the opened fill reaches; with both ends in the core, with
  neither, or with the far end at such a pin or via it is removed as Java removes it
  (`BasicBoard::area_joins_trace_ends`; `tests/k15_pour_fill_stub.rs`,
  `a_trace_from_the_fill_to_the_strip_is_a_cycle_only_under_javas_rule`,
  `the_cycle_rule_keeps_only_a_bridge_into_the_fill`). The router's contacts
  stay the outline; power layers and a DSN input are unchanged. On `tests/data/kicad/pcu-k1523/a-stub-beside-a-crossing` without the
  `/CH6_NO` stub, Java's rule routes J2 to (124.5279, 109.2002) mm, in a 0.347 mm strip of
  EFUSE_NO_6 past a crossing track that KiCad's refill drops (`track_dangling`, 1 unconnected;
  `enhanced`'s cleanup then removes the trace and leaves the connection open), and `true` routes
  it to (124.3789, 110.5231) mm, where KiCad 10.0.7's refill reaches (no `track_dangling`, 0
  unconnected), with and without the cleanup and the output check
  (`tests/k15_pour_fill_stub.rs`, `the_route_from_j2_leaves_no_stub_with_the_fill_target`).
  Not in `enhanced` (K15.25, decided 2026-10-22: a correctness rule joins only if neither the pcu
  board's KiCad unconnected nor a second board's goes up): on top of `enhanced`, the pcu board's
  `.kicad_pcb` route (tracks stripped, plugin default options, `--max-passes 2 --timeout 240`)
  goes from KiCad unconnected 47 to 49 and router incomplete 11 to 14, canary-pwr's (`95882f00`,
  stripped, K15.14's `pcb-off` command) from 40 to 31, with 0 routing-caused items in each arm.
  The pcu board's extra items are connections the router counts done through a pour's outline
  where KiCad's fill does not join them, and two nets without a pour (`SCL`, `DRV_SUP`) left
  unrouted. Not made a default (K15.30, preregistered, decided 2026-10-22): over 16 `.kicad_pcb`
  corpus rows (K15.14's set, `pocketsdr-v3-0` left out, both arms timed out) KiCad unconnected
  went 370 -> 363 against a set noise of 67.09, with 0 manufacturing breaches and 0
  below-minimum items in all 34 graded arms (pcu 47 -> 49, canary-pwr 40 -> 31, the other
  rows within their bands), so `enhanced` leaves it unset too. The KiCad plugin offers it as a
  per-board checkbox (Advanced tab, Flow group; `RouteOptions.pour_fill_target`, off in both
  profiles; `tests/test_options.py`, `tests/test_route_dialog.py`). K15.37 measured it with
  `pour_fill_contacts` (the pair, 338 -> 272 over 14 rows, not kept on routing-caused DRC 12 ->
  27 and route time 1.95 x; the entry below), so it stays opt-in too.
  K15.43 measured the pair again after K15.40-K15.45 (preregistered, decided 2026-10-22, "not
  kept, guard", guard 4 only): over 16 judged `.kicad_pcb` corpus rows (`pocketsdr-v3-0` left out,
  both arms TIMED_OUT) KiCad unconnected went 370 -> 269 against a set noise of 67.09 (pcu 47 ->
  26, canary-pwr 40 -> 26) and routing-caused DRC 15 -> 11, with 0 manufacturing breaches and 0
  below-minimum items in all 34 graded arms, but route time went 625.8 s -> 798.6 s, 1.28 x (limit
  1.25 x; one board 120.7 s -> 215.0 s, two others 23.3 s -> 40.0 s and 24.9 s -> 43.9 s), so the
  pair stays opt-in; K15.47 takes the route time and K15.48 measures again.
  K15.48 measured it again after K15.46 and K15.47 (preregistered, decided 2026-10-23, "not kept,
  guard", guard 4 alone): over 16 judged `.kicad_pcb` corpus rows (`pocketsdr-v3-0` left out, both
  arms TIMED_OUT) KiCad unconnected went 368 -> 264 against a set noise of 67.09 (pcu 47 -> 18,
  canary-pwr 40 -> 24) and routing-caused DRC 15 -> 10, with 0 manufacturing breaches and 0
  below-minimum items in all 34 graded arms, but route time went 445.5 s -> 562.4 s, 1.262 x
  (limit 1.25 x, 556.9 s; kit-dev-coldfire 73.4 s -> 109.2 s, canary-pwr 82.0 s -> 111.2 s, of
  which about 24 s is the fanout's two-pass rip-up cycle), so the pair stays opt-in; K15.50 stops
  that cycle and K15.51 measures again.
  K15.42 (opt-in, so `legacy`
  and `enhanced` unchanged): under `true` on a `.kicad_pcb` input the pipeline runs an island
  repair after the optimizer and the stub cleanup, before the output check
  (`quality::island_repair`): per island of a mode-1 zone's fill model (`island_removal_mode` 1,
  KiCad keeps it) that no pin, trace or via of its net touches, the routed traces and vias of
  each foreign net that overlap the zone's outline near the island are removed with the rest of
  their connections (`Item.getConnectionItems`), one net and then all of them together, and
  routed again (one add-only pass of the job's stages, as Q20's retry) with the zone an obstacle
  for those nets; a trial is kept only if such islands go down and the incomplete count does not
  go up, else the board is put back. Java has no such pass (a conduction area is no obstacle and
  joins everything in its outline). On `tests/data/kicad/moteus-k1542/c-cut-island` (GNDPWR's
  mode-1 zone and moteus's `+3V` track ends) the straight In1.Cu route leaves two islands (KiCad
  10.0.7: 2 `isolated_copper`) and the repair routes it around the zone (1, the zone has no pad)
  (`tests/k15_pour_pair_drc.rs`, `a_cut_that_isolates_a_kept_island_is_routed_around_it`). On
  moteus with the pair (K15.37's `pcb-on` command) KiCad `isolated_copper` goes 9 -> 5 (`pcb-off`
  3), KiCad unconnected 32 -> 29, route time 106 s -> 130 s; interf_u, openmuscle, pcu and
  canary-pwr route as before (no trial kept). K15.46: the repair judges islands with K15.45's
  zone-join model (`isolated_islands`), and keeps a trial only if no net has more incomplete
  connections than before it (the routing ledger per net, not the board's total), with the
  islands down, or as many islands and fewer incomplete connections; nets that opened are routed
  once more without the obstacle before the trial is judged; the job log names each routed trial
  (island, nets, counts, kept or undone) and each island left. A trial that opened one net's
  connection and closed another's was kept before: on `tests/data/kicad/moteus-k1542/g-open-trade`
  (moteus's GND zone of priority 40 and `/Current Sense/FCUR3`'s route) removing FCUR3 joins
  the GND island and closes a +3V connection, and a re-route that fails leaves FCUR3 open; it is
  now undone (`a_re_route_that_opens_a_connection_is_undone`; KiCad 10.0.7 counts the trade the
  same: +3V unconnected with FCUR3's route, FCUR3 without it). On moteus with the pair KiCad
  unconnected 29 -> 30 and `isolated_copper` 5 -> 5 against K15.42's model (V+ priority 34
  joined; GND priority 40's island kept, its FCUR3 trial now undone).
- `pour_fill_contacts` (K15.31, `board::pour_contacts`) is a settings field Java does not have,
  unset by default and in both profiles (Java's rule: a conduction area joins every pin and via of
  its net centred in its outline and every trace of its net ending there,
  `ConductionArea.getNormalContacts`). `true` (opt-in) makes the board's connectivity (connected
  sets, so the routing ledger's incomplete count, the DRC's incompletes and the connections the
  batch router routes) take a signal-layer pour of a `.kicad_pcb` input (an area K15.23's
  `PourFills` judges by its opened fill) from that fill, island by island: a pin, via or trace of
  the area's net belongs to every island of the opened fill (core grown back by the opening's
  radius, every island kept) whose pieces its copper on the area's layer meets, also where its
  Java contact point (centre, trace end) lies outside the outline, and to none elsewhere (K15.46,
  KiCad 10.0.7's rule; until K15.46 a Java contact belonged to the island whose piece held its
  point); the area item stands for its main island (largest,
  among those with a pin contact first), the items of each other island are in contact with each
  other directly, and the cycle rule joins two trace ends only within one island
  (`area_joins_trace_ends`, `BasicBoard::pour_cycle`). The fills are cached on the board and
  forgotten at every item and search-tree change near them; a forgotten fill keeps its inputs, its
  fill model's grid cells and its pieces with their shape tests, and a fill made again takes what
  is equal, with the same islands (K15.41: with `pour_fill_target` too, complex_hierarchy routes in
  3.7 s instead of 44.3 s and pcie-active-backplane in 49.7 s instead of timing out, K15.37's
  sessions unchanged on complex_hierarchy, interf_u, pcu and canary-pwr). `BasicBoard::normal_contacts` stays
  Java's; power layers, a DSN input and an area whose fill has no piece keep the outline. With
  `pour_fill_target` too the search reaches such an area only in its main island's core. On
  `tests/data/kicad/pcu-k1531/a-pads-in-three-islands` (EFUSE_NO_2 cut in three by `VCC_3V3`,
  KiCad 2 unconnected `/CH2_NO` items) Java's rule counts `/CH2_NO` complete, `true` counts 2 open,
  and `true` with `pour_fill_target` routes the islands together (0; KiCad 0)
  (`tests/k15_pour_fill_contacts.rs`). It is meant together with `pour_fill_target` (alone it
  counts the open islands but does not route them); the two stay separate settings (decided
  2026-10-22). Not in `enhanced` (its default needs a measurement, K15.37): on top of
  `enhanced` (main `48fad8c`, plugin default options, `--max-passes 2 --timeout 240`, KiCad
  10.0.7) the pcu board's `.kicad_pcb` route goes from KiCad unconnected 47 (router incomplete 11)
  to 48 (49) with `true` and 21 (22) with `true` and `pour_fill_target` (49 (14) with
  `pour_fill_target` alone), 0 routing-caused, route time 163 s -> 175 s and 176 s; canary-pwr's
  (`95882f00`, K15.14's `pcb-off` command) from 40 (27) to 38 (39) and 26 (27) (31 (19) with
  `pour_fill_target` alone), route time 92 s -> 181 s and 153 s, with 1 routing-caused
  `track_dangling` in the second arm (`/Supervisor/CAN_EN`, no pour net; K15.31's note). Not made a
  default (K15.37, preregistered, decided 2026-10-22: "not kept, guard" for the pair with
  `pour_fill_target`): over 14 judged `.kicad_pcb` corpus rows (K15.30's set; `pocketsdr-v3-0`,
  `kit-dev-coldfire-xilinx_5213` and `pcie-active-backplane-pcie-active-backplane-x16-4layer`
  left out, each arm TIMED_OUT at `--timeout 240` in one or both) KiCad unconnected went 338 ->
  272 against a set noise of 59.34 (pcu 47 -> 20, canary-pwr 40 -> 26), so the gain holds, but
  routing-caused DRC went 12 -> 27 (guard 2; `interf_u` 1 -> 7, `moteus-moteus-c1` 4 -> 13,
  `openmuscle-flexgrid-om-flexgrid-rigid-pcb` 1 -> 2) and route time 1.95 x over the judged rows
  and the two rows that timed out only with the pair (limit 1.25 x; guard 4), with 0
  manufacturing breaches and 0 below-minimum items in all 34 graded arms. So `enhanced` leaves
  it unset too. The KiCad plugin offers it as a per-board checkbox (Advanced tab, Flow group,
  below `pour_fill_target`'s; `RouteOptions.pour_fill_contacts`, off in both profiles;
  `tests/test_options.py`, `tests/test_route_dialog.py`). K15.41-K15.43 fix the costs and
  measure again.
  K15.43 measured the pair again after K15.40-K15.45 (preregistered, decided 2026-10-22, "not
  kept, guard", guard 4 only): over 16 judged `.kicad_pcb` corpus rows (`pocketsdr-v3-0` left out,
  both arms TIMED_OUT) KiCad unconnected went 370 -> 269 against a set noise of 67.09 (pcu 47 ->
  26, canary-pwr 40 -> 26) and routing-caused DRC 15 -> 11, with 0 manufacturing breaches and 0
  below-minimum items in all 34 graded arms, but route time went 625.8 s -> 798.6 s, 1.28 x (limit
  1.25 x; one board 120.7 s -> 215.0 s, two others 23.3 s -> 40.0 s and 24.9 s -> 43.9 s), so the
  pair stays opt-in; K15.47 takes the route time and K15.48 measures again.
  K15.48 measured it again after K15.46 and K15.47 (preregistered, decided 2026-10-23, "not kept,
  guard", guard 4 alone): over 16 judged `.kicad_pcb` corpus rows (`pocketsdr-v3-0` left out, both
  arms TIMED_OUT) KiCad unconnected went 368 -> 264 against a set noise of 67.09 (pcu 47 -> 18,
  canary-pwr 40 -> 24) and routing-caused DRC 15 -> 10, with 0 manufacturing breaches and 0
  below-minimum items in all 34 graded arms, but route time went 445.5 s -> 562.4 s, 1.262 x
  (limit 1.25 x, 556.9 s; kit-dev-coldfire 73.4 s -> 109.2 s, canary-pwr 82.0 s -> 111.2 s, of
  which about 24 s is the fanout's two-pass rip-up cycle), so the pair stays opt-in; K15.50 stops
  that cycle and K15.51 measures again.
  K15.46 (pair only): KiCad joins a pad, a via or a track to every fill its copper meets
  (`tests/data/kicad/k1546-shape-contacts/`: a pad or via whose centre lies up to 0.45 mm
  outside the outline, a track ending outside it or crossing it, a pad, via or track end at a
  rounded fill corner; none of the five gaps of 0.05 mm or fills taken by another net's track),
  and so does the contact rule now (`tests/k15_pour_fill_contacts.rs`,
  `a_pad_a_via_or_a_track_joins_the_fill_its_copper_meets`,
  `moteus_u2_pad_2_joins_the_zone_its_copper_meets`; moteus's routed `pcb-on` boards carry 27
  such contacts, U2 pad 2 among them). The cycle rule (`pour_cycle`, `area_joins_trace_ends`)
  and the lone-pin rule keep K15.31's rule (a Java contact joins the island holding its point):
  with KiCad's rule in the cycle search too, `pcu-k1531/a-pads-in-three-islands` routed with
  both settings kept 1 connection open. With K15.43's `pcb-on` commands (release binary
  `k1546b/bin`, KiCad 10.0.7) moteus went from KiCad unconnected 30 and 6 routing-caused
  (session `daff0cba`) to 30 and 10 (`d124cb86`), pcu from 26 to 18 (0 and 0, `e9572570`),
  canary-pwr from 26 to 24 (0 and 0, `cf00bc73`), interf_u and openmuscle unchanged, moteus
  `pcb-off` unchanged (`9f998f1d`), every arm with 0 manufacturing breaches and 0 below-minimum
  items.
  K15.46's third part (pair only): under `pour_fill_contacts` the stub cleanup's fill model
  (`quality::pour_fill`'s `opening`, so `PourFills::pieces`) keeps the zone's own clearance as
  the connectivity's does, so the two read one fill; with the rule's clearance it reached copper
  KiCad's fill leaves bare and kept moteus's +3V via at (160.9345, 105.914) mm (`via_dangling`,
  the nearest fill 0.844 mm away) and two V+ In2.Cu stubs (`track_dangling`) on session
  `d124cb86` (`tests/k15_pour_pair_drc.rs::a_via_whose_pocket_kicad_leaves_unfilled_is_a_stub_under_the_setting`,
  `tests/data/kicad/moteus-k1542/i-via-pocket`). moteus `pcb-on` then gives 30 unconnected and 6
  routing-caused, all `isolated_copper`, no dangling copper (`2d505df0`); interf_u, openmuscle,
  pcu and canary-pwr keep their sessions, and `pcb-off` is unchanged. The cycle search keeps
  K15.31's point rule: with KiCad's rule there, a fanout trace from a pad to a via whose pad
  meets the pad's island is a cycle, and `remove_if_cycle` removes the trace's connection by
  Java's contacts (the trace and the via), so R8 pad 1's escape on
  `pcu-k1531/a-pads-in-three-islands` is undone on every attempt.
  K15.42 (pair only, so `legacy` and `enhanced` unchanged): the fill model the
  pour target and the contacts read (`quality::pour_fill::opened_fill_core`) keeps the zone's
  own clearance (`(connect_pads (clearance C))`, `drc::BoardZone::fill_clearance`) from other
  nets' copper where it is above the rule's, as KiCad's filler does, unless the `.kicad_dru`
  has a clearance rule (then 0; the cleanup's `PourFills` model is unchanged); and a pin alone
  in a non-main island of a pour is reached only on that pour's layer
  (`autoroute::pour_target::lone_pin_layers`), because KiCad 10 reports `starved_thermal`
  ("N spokes connected to isolated island") for such a pad joined by a track whose other end
  lies over the fill. On `tests/data/kicad/interf_u-k1542/` the via chain of
  `a-via-chain-cut-off` joins no pin (KiCad: `via_dangling`) and `b-lone-pad`'s pads U1-10 and
  U8-12 take no F.Cu trace (`tests/k15_pour_pair_drc.rs`). K15.45: a zone-join model
  (`quality::island_repair::isolated_islands_with_zone_joins`, the repair's since K15.46) judges a
  `.kicad_pcb` pour's islands with the zone's own clearance and no blob slack
  (`PourFills::zone_island_model`), cuts same-net zones of higher priority out at their outline
  (`subtractHigherPriorityZones`), matches a file zone by an outline vertex before containment
  (`drc::kicad_connectivity::file_zone_by_vertex`; moteus's +3V priority-44 zone lies inside
  +3V's priority-1 zone) and keeps an island another zone of its net joins to a pin
  (`IslandCache::reaches_pin`), as KiCad's connectivity counts zone-to-zone joins. K15.46 makes
  it the island repair's (the `pour_fill_target` entry). On `tests/data/kicad/moteus-k1542/` boards `d-zone-join` (V+ priority 34
  joined to pad C1 through V+ priority 0: kept) and `e-zone-cut` (the +5V chain cuts it off:
  isolated, as KiCad reports).
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
- `fanout.pin_escape` (Q21, `autoroute::stages::pin_escape`) is a settings field Java does not
  have, unset (`off`) in both profiles. An unknown value fails the job before routing, naming the
  key (`tests/pin_escape_setting.rs`; `freerouting-krt`'s
  `an_unknown_pin_escape_value_is_refused_before_routing`).
- `via_clearance` (Q14, `autoroute::via_clearance`) is a settings field Java does not have,
  unset by default (`via_info`: Java's rule, a routed via takes its via info's clearance class).
  `net_class` gives every via the autorouter checks or inserts for a net its net class's via
  clearance class (KiCad's rule). It matters where the fanout's `fallback_to_board_vias` rule
  appends the board's first via rule, whose infos carry the default class: on
  `tests/data/quality/q14-hv-via.dsn` Java and `via_info` put a default-class 0.6 mm via on the
  0.25 mm class's net 0.2322 mm from another net, and `net_class` keeps 0.25 mm
  (`tests/via_clearance_net_class.rs`); an unknown value fails the job, naming the key. Unset
  (Java's rule) under profile `legacy`; N05.3 made `net_class` `enhanced`'s default (see
  "Profile `enhanced`").
- `board_minimums.*` (Q18, `settings::board::BoardMinimums`, `autoroute::control`) are settings
  fields Java does not have, unset by default: the board's minimum manufacturing rules as hard
  floors, decided 2026-10-20 (routing never breaks them, in every profile, whenever they are
  known). `track_width_um` floors `AutorouteControl::min_trace_half_width`, so no trace the
  router creates is narrower: Java's pin neck-downs (search and inserter), the fanout's micro
  neck-down, the necked retry, `bounded-neck` and KRT's fanouts all read that floor; a class
  width below it is raised to it. A via type below `via_diameter_um`, `via_drill_um` or
  `via_annular_um` is made at the floor size before routing (`job::via_minimums`: a round
  padstack `Via[<from>-<to>]_<diameter>:<drill>_um` at `max(drill, min drill)` and
  `max(diameter, min diameter, drill + 2 · min annular)`, the via type's name, clearance class
  and rules unchanged; one warning per net class names the class), and the control's via rule
  drops any via type still below them (a backstop). `from_classes` takes the
  narrowest class width of the design as the track minimum when `track_width_um` is unset
  (`enhanced`; see "Profile `enhanced`"). Java's `min_trace_width_um` floor alone let the
  fanout's micro neck-down (`autoroute/path/insert.rs`, `micro_neckdown_half_widths`: 3/4, 3/5
  and 1/2 of the class width) ship 0.15 mm tracks on boards whose Board Setup minimum is 0.2 mm
  (Q11.6's grades at a 150 µm floor: vfo 72, mydevboard 74 KiCad `track_width` errors; every
  0.15 mm wire of their `mp1` sessions, 47 each, is the 3/4 candidate, as their smallest pads,
  0.25 and 0.2 mm, give pin neck-downs of 0.2498 and 0.1998 mm; K15.4 corrected Q18's credit to
  the pin neck-down), and with no floor 0.15, 0.12 and 0.10 mm on the pcu board (K15.4); with
  the floor a neck that does not fit leaves the connection open. Unset, nothing changes (`tests/board_minimums.rs`:
  `java_pin_neck_down_goes_under_the_class_width`, `the_board_minimum_floors_the_pin_neck_down`,
  `the_necked_retry_stays_at_the_board_minimum`, `vias_under_the_board_minimums_are_made_at_the_floor`,
  `routed_vias_meet_the_board_minimums`).
- `solder_mask_expansions` (Q18.7, `job::solder_mask`) is a settings field Java does not have,
  unset by default: pads whose solder-mask aperture reaches past their clearance, as
  `<component>:<pin>=<front um>,<back um>` entries separated by `;`. Each listed pin gets a
  clearance class of its own (`<class>+mask<front>/<back>`, a copy of its class with every
  clearance on the top layer raised to at least the front expansion and on the bottom layer to
  the back one), so other nets' traces and vias keep out of the aperture, which KiCad's DRC
  flags as `solder_mask_bridge` (decided 2026-10-20: routing never breaks the board's
  manufacturing rules). The KiCad plugin and a `.kicad_pcb` input fill it in. Unset, nothing
  changes (`tests/solder_mask.rs`: `a_front_expansion_keeps_other_nets_out_of_the_aperture`,
  `a_back_expansion_leaves_the_front_alone`, `an_expansion_within_the_clearance_changes_nothing`).
- `output_check` (Q20, `quality::output_check`, `drc::board_rules`) is a settings field Java does
  not have (`off | report | repair`; unset is `off`, Java's: the session is written as the router
  leaves it). On, `run_pipeline` checks the input copper once before the fanout (one log line,
  "the board already has N violation(s)", never changed) and the routed copper last, after the
  stub cleanup and also after a stop, against the board's rules as the router models them
  (`check_board_rules`: clearance, copper-to-edge clearance, keepouts, track width and via
  diameter, drill and annular ring under `board_minimums.*`, hole clearance, vias in pads,
  dangling traces and vias); `report` logs and counts, `repair` removes the routed traces and
  vias of every breach and the stubs that leaves, retries the affected nets' incomplete
  connections once with rip-up off (one pass, ascending net and item order, not after a full
  stop) and removes what still breaches, so routed copper that breaks a board rule is never
  saved (`tests/quality_output_check.rs`, `tests/drc_board_rules.rs`). A board whose routed
  copper breaks nothing is written as with `off` (`a_clean_board_gives_the_session_of_off`).
  Copper that only differs from the input by the router's split counts as input
  (`InputCopper`). A trace end is dangling when its end point lies on no copper of its net
  (KiCad's anchor rule; Q16's cleanup uses the round end's overlap).
- `board_minimums.clearance_um` (Q22.10, `job::clearance_floor`) is a settings field Java does
  not have, unset by default: the board's minimum clearance (KiCad Board Setup
  `m_MinClearance`), a floor under every clearance of the matrix between two classes other than
  `null`, on every layer, applied before the copper-to-edge and hole overrides (whose classes
  keep their own values). KiCad's DRC applies it under every net class ("board minimum
  clearance"); pocketsdr-v3-0 (Default class 0.09 mm, minimum 0.127 mm) had 446 routing-caused
  `clearance` items in Q18's grades. Unset, nothing changes (`tests/clearance_floor.rs`:
  `without_a_minimum_clearance_the_matrix_is_the_dsns`,
  `the_board_minimum_clearance_raises_every_clearance_below_it`,
  `routed_copper_keeps_the_board_minimum_clearance`).
- `board_minimums.hole_to_hole_um` (Q22.11, `job::hole_to_hole`, `board::hole_spacing`) is a
  settings field Java does not have, unset by default: the board's minimum distance between two
  drill holes, edge to edge, of any net (KiCad Board Setup `m_HoleToHoleMin`). Set, the maze
  search drops a drill location whose via hole (the largest drill of the rule's vias) comes
  closer to a via's or a multi-layer pin's drill, the via insertion skips such a via type, a
  via move (optimizer, shove) to such a place is refused, and Q20's output check reports
  `hole_to_hole` for a via that breaks it. Clearance keeps different nets apart; two vias of
  one net had no floor. Unset, nothing changes (`tests/hole_to_hole.rs`:
  `without_a_minimum_the_vias_are_javas`,
  `vias_of_one_net_keep_the_board_hole_to_hole_distance`,
  `the_output_check_reports_holes_closer_than_the_minimum`).
- `cleanup.remove_stubs` (Q16.2, `quality::stubs`) is a settings field Java does not have, unset
  by default and in `legacy` (off: Java leaves the stubs of unfinished connections on the
  board); profile `enhanced` turns it on (N05.3, the "Profile `enhanced`" line below). On, `run_pipeline` ends with a cleanup after the last routing or optimizer step and
  that phase's board stages, also after a stop: every trace or via the router inserted during
  the run (no item of the board as handed to `run_pipeline` is ever removed, locked or not, so a
  designer's unfinished track stays) that is unfixed, of one net, dangling as KiCad counts it
  and whose contacts are connected through one another (Q16.5: the graph of which contacts touch
  is connected; Q16.4 asked that every two touch) is removed, in item-id order, round after round until
  a round removes nothing; fixed and protected copper stays, and no complete connection becomes
  incomplete (`tests/stub_cleanup.rs` on `tests/data/quality/q16-stubs.dsn`). Dangling is read on
  KiCad's contacts, not Java's normal contacts (Q16.4, `quality::stub_contacts`): copper of a net
  touches wherever the board's exact shapes intersect on a common layer (a via on a trace's body,
  a trace end on another trace's body, a via off a pad's centre), so a trace with an end whose cap
  meets no other copper of its net, or a via whose pad meets copper of its net on at most one
  layer, is a stub (`tests/stub_contacts.rs` on `tests/data/quality/q16-overlap-stubs.dsn`; Q16.3
  measured Java's contacts and removed a via KiCad counted as a connection). A plane of the net is
  a contact, so a via stitching a trace to its plane stays. Known limit: a plane is its DSN
  outline, not KiCad's zone fill, so a trace ending inside the outline where the fill does not
  reach stays and KiCad reports it dangling (Q16.3, CM5_MINIMA_3). K15.23 lifts it for a
  signal-layer pour of a `.kicad_pcb` input (a zone of the file): the cleanup takes that pour's
  copper from the router's fill model (outline minus other nets' copper grown by the router's
  clearances, the routed copper included) opened by the zone's `min_thickness` as KiCad's filler
  opens it (eroded by half of it less 1 µm, grown back; the net's own pads filled over), never
  from the outline or the file's stale fill (`quality::pour_fill`; `tests/k15_pour_fill_stub.rs`
  on `tests/data/kicad/pcu-k1523/`, cut from the pcu board: a `/CH6_NO` trace ending in
  EFUSE_NO_6's outline 0.347 mm of fill past a crossing track, which KiCad 10.0.7 reports
  `track_dangling`, is removed with its via and B.Cu trace). K15.26 does the same for every
  signal-layer pour of a DSN input, which carries no zone: the fill model with the router's
  clearances and no opening, unless `router.cleanup.pour_min_thickness_um` and
  `pour_clearance_um` (unset in both profiles; values no larger than every zone's, as the KiCad
  plugin knows them) give the zones' minimum thickness to open it by and a clearance floor for
  other nets' copper (`drc::opened_fill_islands`; `tests/k15_dsn_pour_fill_stub.rs` on
  `tests/data/quality/k1526-dsn-pour-crossing.dsn`: a stub ending in the 0.55 mm strip a crossing
  track leaves is removed with 0.5 mm and 0.5 mm given and kept without them or without the
  crossing). K15.29 leaves out the fill islands KiCad's filler removes, on both inputs
  (`quality::pour_islands`): an island of the pour's fill model (opened alike, with the router's
  clearances and no `pour_clearance_um` floor) whose copper group reaches no pin, where another
  island of the pour meets a pin's pad, on a zone of `island_removal_mode` 0 (a `.kicad_pcb`'s
  own; on a DSN `router.cleanup.pour_island_removal`, except the nets of
  `pour_islands_kept_nets`, both unset in both profiles and passed by the KiCad plugin), and
  never on a net with a pour the reader dropped (`tests/k15_pour_island_via.rs` on
  `tests/data/kicad/pcu-k1529/`, cut from the pcu board's DSN route: `Net-(U24-OUT)`'s via in
  the part of zone USB_OUT that `BUS_GND`'s track and via cut off from U24's pad, which KiCad
  10.0.7 reports `via_dangling`, is removed on the `.kicad_pcb` and the DSN; kept without the
  `BUS_GND` copper, with `island_removal_mode` 1, or without the setting). Before K15.29 every
  island was kept. K15.33, on both inputs: the fill model also drops the blobs KiCad's filler
  deletes (`ZONE_FILLER::fillCopperZone` deletes every deflated polygon whose bounding box is
  smaller than the zone's `min_thickness` on both axes, before the inflate; the model's eroded
  island counts as a quarter of the radius larger on each side, to keep a blob KiCad deletes
  rather than drop one it keeps); a positive `pour_clearance_um` is the clearance to every other
  net's copper, no longer the larger of it and the router's (a `.kicad_dru` rule replaces the net
  class clearance in KiCad's filler, so the router's 0.15 mm class clearance made the model
  narrower than KiCad's 0.127 mm fill); and a DSN net named in
  `router.cleanup.pour_min_thickness_nets` (`<µm>:<net>`, unset in both profiles, passed by the
  KiCad plugin) has its own zones' thickness for the blob test, the opening keeping the board's
  smallest, so an island goes only when every piece of it the net's own erosion leaves is a blob
  (`tests/k15_pour_blob.rs` on `tests/data/kicad/pcu-k1533/`, cut from the pcu board's DSN route:
  `/CH5_NO`'s via in the corner of zone EFUSE_NO_5 that `SCL`'s track cuts off, a blob 0.29 mm
  across at the zone's 0.5 mm, which KiCad 10.0.7 reports `via_dangling`, is removed on the
  `.kicad_pcb` and the DSN at a 127 µm clearance; kept with the zone at 0.4 mm, where KiCad keeps
  the triangle). K15.35, on a DSN input: the fill model takes the keepouts as KiCad's filler
  does, not with the floor (or Java's clearance class): a `keepout` (a rule area that bans tracks
  and vias) and an NPTH (a keepout the export already grew by the hole clearance) cut the fill at
  their outline with no clearance (`ZONE_FILLER::buildCopperItemClearances`: "Keepouts use
  outline with no clearance"); a fitted mask-aperture or thermal-spoke keepout and a
  `via_keepout`, `wire_keepout` or `place_keepout` are left out; a copper keepout
  (`copper_graphic`, `copper_text`, `net_tie`) keeps the clearance; Java's fill is unchanged
  (`tests/k15_pour_keepout.rs` on `tests/data/kicad/pcu-k1535/`, cut from the pcu board's DSN
  route: KiCad 10.0.7 fills BUCK_SW up to L1's rule area and V_PD_IN to 1.6566 mm from F1's NPTH,
  and the model covers both, where it lay 0.127 mm short). K15.38, on a `.kicad_pcb` input (the
  cleanup's model, and so `router.pour_fill_target` and `router.pour_fill_contacts`): the reader
  says which rule areas ban copper pour (`Extras::rule_areas`, which the DSN cannot carry), and the
  model cuts the keepouts of a rule area that bans it at their outline, leaves out those of one
  that allows it (whatever their kind: `keepout`, `via_keepout`, `wire_keepout`,
  `place_keepout`), cuts an NPTH's keepout circle at its outline, and keeps the clearance from
  every keepout the reader names (`edge_clearance`, `pad_clearance`, the copper ones, and the
  fitted mask-aperture and thermal-spoke keepouts, which the DSN path leaves out) and from a
  board outline's cutout
  (same tests on the two boards' `.kicad_pcb`, and
  `a_rule_area_that_allows_the_pour_does_not_cut_the_kicad_pcb_model` on
  `tests/data/kicad/k1538-pour-allowed/`, refilled by KiCad 10.0.7). K15.39, on a DSN input:
  K15.35's handling stays (the fitted mask-aperture keepouts left out, the plugin's unnamed edge
  strips and pad-clearance circles cut at their outline): on the pcu board's DSN route (session
  `6a90062c`) against KiCad 10.0.7's refill, KiCad fills 80.35 mm2 inside the 58
  `solder_mask_pads` apertures and the model holds exactly that (0 mm2 wider, 0 narrower), no
  judged pour comes within 0.6 mm of the 8 edge strips (the nearest pour edge lies 0.95 mm from
  the outline, the strips reach 0.35 mm in), and the DSN has no `solder_mask_shared` keepout and
  no pad-clearance circle, so nothing was changed. K15.40, on both inputs: a pad-clearance circle
  (the plugin's and the reader's structure-level circle round a pad with its own clearance, found
  by the pin or NPTH keepout on its centre) is left out round a pin of the pour's net (KiCad joins
  the pad) and otherwise kept exactly the default clearance it was drawn less (the keepout's class
  against itself, without Java's 16-unit safety margin), not cut at its outline (DSN) or kept the
  router's clearance (`.kicad_pcb`); and the board outline is kept the edge clearance KiCad's
  filler keeps (`EDGE_CLEARANCE_CONSTRAINT` from the Edge.Cuts line): the router's
  `copper_to_edge_clearance_um` without the safety margin, less the outline lines' 10 µm half
  width and 5 µm for KiCad's arc chords, never the pour-clearance floor, and no more than the
  least of Board Setup's value and the `.kicad_dru`'s `edge_clearance` rules (a rule below Board
  Setup's lowers KiCad's fill's clearance, where the router's setting only rises to it). On
  CM5_MINIMA_3's route (main `7bbbc09`) against KiCad 10.0.7's refill the DSN model reached 0.135
  mm into the 1.7 mm clearance KiCad keeps round its two netless NPTHs (F.Cu, B.Cu) and 0.13 mm
  into the 0.372 mm edge clearance (to the plugin's strips), and the `.kicad_pcb` model stopped
  11-13 µm short of KiCad's fill along the edge (`tests/k15_pad_clearance_fill.rs` on
  `tests/data/kicad/cm5-k1540/`, cut from CM5_MINIMA_3, 8 tests, with a GND mounting-hole pad and
  the pcu board's 0.254 mm outer `edge_clearance` rule). The pad-clearance circles are written on
  the outer layers only, so on inner layers the model keeps only the NPTH's hole clearance (1.5
  mm wider than KiCad's fill on CM5). K15.45 (`.kicad_pcb` and DSN alike for the first): the
  model keeps a pad's or NPTH's own clearance on the layers without a circle too, as KiCad's
  `knockoutPadClearance` keeps it on every copper layer (`drc::zone_islands`'
  `unwritten_pad_clearance`; on 56 corpus routes no track, via or fill of another net lies inside
  such a clearance on those layers and KiCad's DRC reports none, so the circles stay on the outer
  layers and the routes are unchanged); on a `.kicad_pcb` a pad's or footprint's `(clearance v)`
  below the net class's replaces it round that pad, as KiCad's local override does (CM5's
  Module302 pads: 0.1275 mm, not 0.2016 mm). Another net's higher zone is still cut by its
  outline where KiCad cuts by its fill (`knockoutZoneClearance`, K15.15's limit), so the model
  stays narrower there (CM5's largest slivers: 0.18 mm2 of `+5V` In3.Cu at (116.13, 31.41) mm in
  M2_3V3's priority-5 outline, and 0.08 mm2 pieces round the smoothed corners of its priority-9
  fill; decided 2026-10-22: kept, no new item) (`tests/k15_pad_clearance_fill.rs` boards `d-inner-layers` and `e-pad-override`, cut from
  CM5_MINIMA_3, 11 tests). K15.36: a trace is dangling as KiCad 10.0.7's `TestTrackEndpointDangling` counts
  its end segments (`stub_contacts::trace_dangling`): a pad, via or pour that meets both ends of a
  segment connects it, and a track segment that meets both counts only for the end nearer its own
  ends (KiCad's `getMinDist`; a tie counts for both), so a stub off a junction whose far end lies
  in the junction traces' copper is removed (`tests/k15_junction_stub.rs` on
  `tests/data/kicad/canary-k1536/`, cut from canary-pwr's `.kicad_pcb` route: `/Supervisor/CAN_EN`'s
  0.0725 mm B.Cu stub, which KiCad reports `track_dangling`, goes and the junction's traces stay;
  kept with a track ending at its far end). Power layers and the
  router's own contacts keep the outline. Q16.3 measured the first
  version (`docs/roadmap/prereg/Q16.json`, rejected), Q16.4 the overlap contacts
  (`docs/roadmap/prereg/Q16-4.json`, rejected); Q16.5 measures this one
  (`docs/roadmap/prereg/Q16-5.json`); N05 decides `enhanced`.
- `autorouter.connection_router = transactional-ripup` (Q11, `quality::transaction`) is a
  connection router Java does not have, selected by no profile (unset is Java's
  `freerouting-connection`), with the settings fields `transaction.{rule, depth, rip_factor}`
  (unset: `strict`, 0, 0). It wraps Java's connection router: a connection whose attempt rips
  copper of another net becomes a transaction; the victim nets are routed again at once with
  rip-up off (one attempt per piece, ascending lowest pin id, then net), and the board is kept
  only if every victim net has no more incomplete connections than before (`strict`) or the
  ledger's incomplete count fell (`count`); otherwise the board before the attempt (a `Clone`:
  same items, ids and search tree) comes back, the attempt's work stays on the meter, and the
  connection is routed once more with rip-up off, whose outcome the pass records as Java would.
  `depth = 1` lets the victims rip once, never a net of the same transaction twice. On
  `tests/data/quality/q11-locked-cross.dsn` Java's pass 1 routes A by ripping B for good, and
  every arm keeps B's wire and leaves A unrouted; on `q06-sole-wall.dsn` every arm completes both
  nets in pass 1 where Java needs pass 2 (`tests/quality_transaction.rs`). Its tally goes into
  the job log ("Transactional rip-up ..."). An invalid value fails the job before routing,
  naming the key. Q11.6 measures it (`docs/roadmap/prereg/Q11.json`).
- `transaction.rip_factor` (Q11.5) is rip-count protection Java does not have: with factor
  `f > 0` the transactional router hands the maze search, through
  `ConnectionParams::ripup_factors` and `AutorouteControl::ripup_factors`, a factor `1 + f * k`
  per net, `k` being the committed transactions that ripped the net this run, and
  `MazeRipupResolver.checkRipup`'s cost of an item (after Java's rounding, before its
  `Integer.MAX_VALUE / 100` cap) is multiplied by the largest factor of its nets. Every other
  caller passes `None`, so Java's search never reads it
  (`the_rip_factor_multiplies_the_rip_cost_of_the_ripped_net_only`).
- `autorouter.pass_controller = quantum-stop` (Q04.1, `quality::stop_rule`) is a pass controller
  Java does not have, selected by no profile (unset is Java's `freerouting-passes`), with the
  settings fields `quantum_stop.{grace, checks, quantum_abs, quantum_pct}` (unset: 0, 2, 1.0,
  0.01). It wraps Java's controller and adds one stop: at each of Java's periodic restore checks
  (pass >= 8, pass % 4 == 0, at least 8 history boards) the gain is the best score (the history's
  best or this pass's) minus the best at the previous check (at the first: the score after pass
  1); a gain below `max(quantum_abs, quantum_pct * |best|)` in `f32` is a miss, the first `grace`
  checks never count, and `checks` consecutive misses stop the loop (`StopReason::Quantum`,
  logged "Stopping the auto-router (quantum-stop)."), in place of any decision Java's controller
  made at that pass except its own stop, which wins. The stop requests a router stop like Java's
  stagnation stops (no final tail removal), and the finish's best-board restore is Java's. A flat
  score stops at pass 12 where Java stops at 18 (`tests/quality_stop_rule.rs`); where the stop
  falls after the last score improvement the saved session is Java's
  (`a_stop_after_the_last_improvement_saves_the_same_session`; `Issue690-sonde_xilinx`: 12 passes
  instead of 18, byte-identical SES). An invalid value fails the job before routing, naming the
  key. Measured 2026-10-20 (Q04.2): it fired on `Issue690-sonde_xilinx` and canary-pwr-pours1
  (18 → 12 passes, 513 → 315 s), both after the last improvement, both with byte-identical SES.
  It stays opt-in and outside `enhanced` (Q04.4 did not pass; `docs/roadmap/prereg/Q04.json`,
  amendment 3).
- `quantum_stop.repeat` (Q04.5, `autoroute::batch::batch_loop`) is a settings field Java does not
  have, unset by default and in both profiles (off: Java runs on to its own stops; its same-hash
  stop is commented out). It is independent of the pass controller. On, a pass that leaves the
  board with the state hash it started with (`RoutingBoard::state_hash`, the hash the history
  and the pass log line use) ends the loop (`StopReason::Repeat`, logged "Stopping the
  auto-router (quantum-stop repeat)."), in place of any decision but the controller's own stop;
  the finish's best-board restore is Java's, and a router stop is requested (no final tail
  removal), as for the other stops. The session equals the full run's only if the passes after the
  repeat would also have left the board alone: `Issue093-interf_u` stops at pass 2 of 18 with a
  byte-identical SES (`tests/quality_stop_rule.rs`), while `Issue555-CNH_Functional_Tester_1`
  repeats at pass 4 and its default run later changes the board at pass 12 (same score, other
  hash), so its SES differs (vias 15 against 16). Q04.4 measured it on CNH 2026-10-20 and
  failed (`docs/roadmap/prereg/Q04.json`, amendment 3); it stays opt-in and outside `enhanced`.
- `autorouter.output_guard` (Q04.3, `quality::output_guard`) is a settings field Java does not
  have, unset by default and in both profiles (off: Java saves the best board of the run whatever
  the input was). On, the batch loop keeps a B01 snapshot of the board it was handed and, in its
  finish after the best-board restore and before the final tail removal, compares the board
  with it on two axes: completed connections lost against gained (per net, pins minus groups of
  connected pins, measured against the groups both boards share) and unconnected pins (pins of a
  net with two or more pins that touch no other pin of their net). If either axis is worse it
  saves instead the first history board, best first, that is worse on neither, or the input
  board, sends `Replaced` and logs both axes ("Output guard: ..."). On
  `tests/data/quality/q04-prerouted.dsn` with `-mp 1` the router rips two routed walls, loses 2
  connections and gains 3 with unconnected pins 3 -> 4, and the guard saves the board before
  pass 1 (`tests/quality_output_guard.rs`); a board with no input wiring never fires it.
- `room_order` and `work_unit` (N03.1, restored from P04.1 and P04.2, `board::search`) are
  settings fields Java does not have, unset by default and unset in profile `legacy` (`tree`
  and `nodes`: Java's depth-first room completion and its search-tree nodes-visited work unit,
  D5). `room_order = canonical` completes 45° and 90° rooms best first by (the leaf bound's
  distance from the shape the room must contain, the leaf's key, its shape index), so rooms
  and routes depend only on the trees' contents, not on their shape; `work_unit = leaves`
  counts the leaves an overlap query returns (and the leaves meeting a completion's bound) at
  3,000 per nominal millisecond instead of 64,000 nodes. Both change routes
  (`tests/search_order_independence.rs`: a board built in reversed item order routes the same
  under `canonical`, not under `tree`); an unknown value fails the job, naming the key. A
  non-Java pair logs `Search trees: room order <o>, work unit <u>`. Not in `enhanced` until
  N03.3's measurement accepts an arm (through N05).
- `pad_contacts` (N04, `board::contact_rule`) is a settings field Java does not have, unset by
  default and unset in profile `legacy` (`center`: Java's `Trace.getNormalContacts` and
  `DrillItem.getNormalContacts`, a trace end touches a pin only at the pin's exact centre).
  `inside_pad` also connects a trace end to a pin of a common net when the end's cap (the disc of
  half the trace's width around the end point) overlaps the pin's pad shape on the trace's layer,
  borders included, by the board's own shape intersection (`Shape::intersects_circle`) in integer
  units with no added tolerance; an end at a rational point tests the point alone. This is how
  KiCad connects a track whose copper meets a pad. Only connectivity changes, never copper: the
  incompletes, the routing ledger, the DRC's dangling traces, trace tails, the batch optimizer's
  connected sets, pull-tight's end contacts, the rip-up walk and the maze shover's end match all
  read the rule, and the SES writer's endpoint snap (`snapped_endpoint`, which cannot fire under
  Java's rule) is off, so the session carries the ends as they are. Vias keep Java's rule. A
  trace end whose cap overlaps the copper of a trace of a common net on the same layer (a T
  junction: the end on the other trace's segment or inner corner, or near enough that the copper
  meets; same exact test, N04.4) is a contact as well, which Java's end-to-end trace contacts
  never are. It is in both traces' `normal_contacts` (the ending trace's contacts at that end,
  the other trace's at none of its ends); a connection walk stops at it as at Java's fork, and
  pull-tight first splits a trace at the junctions on its centre line (making them Java's
  end-to-end contacts) and leaves it unchanged when one meets it off the centre line. On KiCad's
  ecc83-pp demo with its tracks (`fixtures/Issue690-ecc83.dsn`) the open connections before
  routing go 10 → 0, KiCad's count: 16 off-centre ends touch their pins, some 15-89 µm outside the
  pad, and a track ends on another track's inner corner (`tests/pad_contacts.rs`). Pull-tight can
  still straighten a connected track onto the pin's centre (Java's split at a drill centre; one
  end on ecc83). An unknown value fails the job, naming the key; a board copy keeps the rule. Not
  in `enhanced` until N04.3's measurement accepts it (through N05).
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
- The log line of the default copper-to-edge cap names the pin that sets the gap and the gap in
  um, at Info (the port logged it at Debug, unnamed, as Java does): `Copper-to-edge clearance capped from 2500 to 0 board units
  because pin FID1-1 is 0.0 um from the board outline` (K15.7,
  `legacy_keeps_javas_cap_and_names_the_off_board_pin`). The value is Java's unless
  `router.edge_cap_skips_off_board_pins=true` (opt-in, unset in both profiles until measured):
  then a pin wholly outside the outline (a fiducial left at (0, 0)) is left out of the cap, each
  one named in a warning; a pin that crosses the outline still counts
  (`an_off_board_pin_is_left_out_of_the_edge_cap_when_the_setting_is_on`).
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
  and the optimizer back on; a `layer` scope with an unknown layer applies to all layers
  under `legacy` (profile `enhanced` ignores it with a warning,
  `router.java_quirks.rules_unknown_layer_scope = false`, N06.12, promoted by N06.13); a
  via type with an existing name moves to the end; the clearance matrix changes without
  re-inserting the tree items under `legacy` (`enhanced` rebuilds the trees,
  `rules_matrix_trees_not_rebuilt = false`, N06.11, promoted by N06.13); a different header
  name is a warning. A rule in the file for
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
  round-trip property. Under profile `enhanced` the reader multiplies instead
  (`router.java_quirks.ses_reader_scale = false`, N06.10, promoted by N06.13;
  `issue676_round_trips_with_reader_scale_fixed`), so an import is the inverse of the export;
  the writer's text is Java's in both profiles.
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

**Bounded neck-down and widen-back** (`quality::neckdown::{bounded_neck, widen_back}`, the
maze search's `autoroute::width_policy`, Q17; tests `tests/quality_neckdown.rs`,
`tests/autoroute_width_policy.rs`, `autoroute::session::tests`)
- The maze search, the path locator and the inserter read each room's trace half width through
  a width policy (Q17.1). Without one (every Java path) they read the control's per-layer value
  directly, so routing is unchanged (A14 `connection-parity` byte-identical,
  `the_freerouting_policy_routes_like_no_policy`).
- New, with no Java counterpart: `router.special.neck_down=bounded-neck` (not a Java id, unset
  by default and in no profile) replaces Java's necked retry after a failed class-width attempt,
  at the same point and ripup cost: the search uses the neck width only in rooms within D of the
  connection's pins (D = courtyard plus one pitch, or `router.neck_down.path_mm`), the class
  width elsewhere; each segment is inserted at its room's width, and the new narrow copper
  outside every pin's D box is widened back to the class width where the exact clearance check
  passes, before the pull-tight. On `q02-qfn-power.dsn` it routes the pin Java leaves unrouted and
  no copper beyond D is narrow (`bounded_neck_is_narrow_only_within_d_of_the_pin`).
- New, with no Java counterpart: `router.neck_down.widen_back=true` (unset: off, Java's routing)
  widens a neck-down retry's narrow traces (Java's `neck_width_um` retry and `bounded-neck`) in
  0.5 mm pieces where the class width passes the exact clearance check, drops wide islands no
  longer than two tapers and adds a 4-step 0.5 mm taper at each narrow-to-wide junction, before
  the pull-tight. A 22 mm run with one pinch is narrow only at the pinch and its tapers
  (`a_22mm_run_with_one_pinch_is_narrow_only_at_the_pinch_and_its_tapers`); it adds no DRC item
  (`widen_back_adds_no_drc_items_on_any_fixture`).

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
- K15.12, the sweep's violation guard (the plugin's default `enhanced` run, which passes
  `sweep=free`; `legacy` leaves the sweep unset and never reaches it): a sweep connection whose
  routing adds clearance violations, counted as the router counts them on the items it inserted or
  changed, is undone at once (the board before it comes back), counts as failed and is named in
  the job log with the first new violation's items, layer and centre ("undid the connection of
  ..."); the pass goes on and keeps the connections that add none. Before, the whole pass was
  undone when the board's violations rose (the pcu board at `41cf105`: 53 connections routed,
  violations 4 -> 5, all undone). A pass that raises the incomplete count is still undone whole.
  `a_sweep_connection_that_adds_a_violation_is_undone_and_the_others_kept`
  (`tests/quality_sweep.rs`, `k15-sweep-guard.dsn`).

**Straight pin escapes** (`router.fanout.pin_escape`, Q21; `freerouting-krt` stage
`krt-pin-escape`, tests `crates/freerouting-krt/tests/pin_escape.rs`)
- New, with no Java counterpart (Java's only pre-pass is `BatchFanout`, which ends each SMD pin in
  a via): with `router.fanout.pin_escape=straight` the job runs X10's QFN/QFP/SOIC stubs
  (`fanout::qfn::escape`) before the fanout. On each part with edge rows at a pitch of at most
  0.65 mm, each pin gets one straight trace along its outward axis, past the package outline, at
  `max(board minimum, min(class width, neck-down width))`; a stub that fails the exact clearance
  check at that width is not inserted. On a 0.4 mm QFN with 0.2 mm pads and a 0.2 mm minimum the
  stubs are 0.2 mm and leave every neighbour its 0.2 mm
  (`stubs_at_a_0p2_minimum_on_a_0p4_qfn_run_straight_out_and_keep_their_clearance`), at 0.35 mm
  pitch none fits (`a_0p35_pitch_where_no_0p2_stub_fits_gets_no_stubs`), and no stub is below the
  minimum (`no_stub_is_ever_below_the_board_minimum`). Off (unset) in both profiles and the
  unchanged engine (`off_is_byte_identical_to_unset_and_straight_logs_its_stubs`).

**Shove checks** (`shove`, O03; tests `tests/board_probe_shove.rs`)
- The fast cutout (`ShapeTraceEntries.fastCutoutTrace`) of a trace with a segment that has
  no tree leaf (a zero-length segment, left by an earlier cutout): Java throws a
  `NullPointerException` in `ShapeSearchTree.reuseEntriesAfterCutout` and leaves the board
  half changed; the port skips the missing leaf and finishes the cutout. The
  `trace-entries` probe hits it on `Issue026-J2_reference` at cutout sample 37
  (`JAVA_CUTOUT_EXCEPTIONS`); every line before it is equal.
- Java bugs kept under `legacy` ([N06](roadmap/tasks/archive/N06-fix-kept-java-bugs.md); profile
  `enhanced` fixes all four, N06.13): `ForcedPadRouter.inFrontOfPad` for from side 0 takes
  `min(lineA.x + lineA.y, lineB.x + lineB.x)` (`ForcedPadRouter.java:78`; test
  `in_front_of_pad_keeps_java_typo`), and `ShapeTraceEntries.storeTrace` compares a
  contact trace's clearance class with itself, so only the shove-fixed state and the half
  width decide there (`shove::trace_entries` module docs). Each has its switch,
  `router.java_quirks.in_front_of_pad_typo` and `store_trace_class_self_compare`, with
  `rational_perp_projection` for the end-corner projection; side 0's second flank clause
  (`min` where sides 2, 4 and 6 take `max`, `ForcedPadRouter.java:86`) is kept behind
  `in_front_of_pad_side0_flank`. N06.7 accepted all four on their tests
  (`tests/n06_shove.rs`); `legacy` keeps them `true` (Java), profile `enhanced` sets them
  `false` (N06.13).
- `ForcedPadRouter.calcCheckShapeForFromSide` with a rational shape centre (Java's
  `Line(Point, Direction)` only warns and builds a rational line) or a polyline whose offset
  shape is `null` (Java then throws in `checkTraceShape`): the port skips that side
  (`shove::drill_check::calc_check_shape_for_from_side`, test
  `calc_check_shape_skips_rational_centre`). Every caller passes an integer via location, so
  no probe query reaches it.

**Shove insert** (`shove::insert`, `shove::forced`, O04; tests `tests/reference_shove_insert.rs`,
`tests/issue954_keepout_insertion.rs`)
- Java bug kept under `legacy` (N06; profile `enhanced` fixes it,
  `router.java_quirks.move_drill_tidy_region = false`, N06.11, promoted by N06.13):
  `RoutingBoard.moveDrillItem` with a finite tidy width never pull-tightens. `DrillItemMover.insert` unions the moved shapes into its own copy of the tidy
  region (`tidyRegion = tidyRegion.union(..)`, `DrillItemMover.java:145`), so the caller's region
  stays `IntOctagon.EMPTY`, `EMPTY.enlarge(w)` is `EMPTY` again and `optChangedArea` skips the
  tightener; only `Integer.MAX_VALUE` (no clip) pull-tightens after a move
  (`RoutingBoard::move_drill_item`, test `move_drill_item_finite_tidy_skips_pull_tight`).
- Java bug kept under `legacy` (found by N03.2; profile `enhanced` fixes it,
  `router.java_quirks.shove_via_into_pending_piece = false`, N06.15, promoted by N06.13's second
  round): a shove substitute piece goes on the board only
  after the recursive inserts of its own shapes, and a deeper level's `shoveVias` may move a
  via of another net onto the piece meanwhile. The move's check (`DrillItemMover.check`,
  `forcedPad`) cannot see a piece that is not on the board, so the piece is inserted over the
  via, a short (`TraceShover.java:544-569`, `ForcedPadRouter.java:413-438`; N03.2 traced it on
  GP8B). `legacy` keeps the switch `true` (Java); N06.15 measured the fix (`shove::insert` module
  docs).
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
- Java bugs kept under `legacy` (`optimize::pull_tight` module docs; N06, fixed under
  `enhanced` since N06.13): the end-corner smoothing
  removes the trace before it inserts the smoothed copy and leaves it removed when the
  insertion fails (fewer than two corners, or zero length and unfixed); `pull_tight_trace` reports a change when
  `change_trace` finds every line reused and returns early. Each has its switch,
  `router.java_quirks.smooth_loses_trace` (`false` tests the failure conditions first and keeps
  the trace) and `pull_tight_reused_lines_change` (`false` reports a change only when
  `change_trace` changed the trace). N06.8 accepted both on their tests
  (`tests/n06_pull_tight.rs`); `legacy` keeps them `true` (Java), profile `enhanced` sets them
  `false` (N06.13). `change_trace` also ignores, as Java's `change` does, a new
  polyline that is a strict prefix or suffix of the old one by line identity; pull-tight is not
  expected to drop or add an end line, so this is expected never to run, and the counter
  `trace_change_prefix_suffix_ignored` checks it (`prefix_suffix_identity_case_is_ignored_and_counted`).
- Java bugs fixed outright in N06.2: end-corner smoothing removes the old trace once and
  restores the saved contact pins when a keep-point split ends smoothing early
  (`keep_point_smoothing_restores_contact_pins`).
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
- Java bugs kept under `legacy` (`optimize::batch` module docs; profile `enhanced` fixes the
  first two since N06.13's second round, N06.9): a first pass without a winner,
  or whose winner has an `improvement_percentage` of 0 (which needs a board without vias:
  `ItemRouteResult`'s guard, not its integer division,
  `zero_improvement_needs_a_board_without_vias`), clears the increased ripup costs inside the
  pass, so the loop stops after one pass (`first_pass_without_winner_stops`); the candidate
  order skips every item whose key equals an earlier one for the whole pass
  (`equal_key_skips_rest_of_pass`); an improvement threshold in (0, 0.1) is multiplied by 100.
  N06.9 puts the first two behind `router.java_quirks.opt_first_pass_stops` and
  `opt_equal_key_skip` (`true`, Java's, under `legacy`; `false` under `enhanced`):
  `false` goes on for one pass at normal ripup costs after such a clear, as Java's comment
  intends (`first_pass_switch_both_behaviours`, `pass_end_decision_both_behaviours`), and
  keeps every sorted candidate (`equal_key_switch_keeps_every_candidate`).

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
- `DrillPageArray::new` (`autoroute/session/drills.rs`) on a board box with a negative width
  or height builds no pages: the capacity is clamped with `.max(0)` and the loops run zero
  times. Java's `new DrillPage[rowCount][columnCount]` throws `NegativeArraySizeException`
  (`DrillPageArray.java:42`) and the job aborts. Before K15.1 an unbounded item box made the
  board box empty, so the port ran on and failed every connection where Java aborted (bug 1 of
  `docs/development/pcu-rev2-board-report.md`). Since K15.1 a read board's box is the DSN
  boundary's box (non-empty, or the read fails) grown by bounded item boxes only, so only a
  board box set empty by hand reaches the clamp; it stays as a guard.
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
- Java bug kept under `legacy` ([N06](roadmap/tasks/archive/N06-fix-kept-java-bugs.md); fixed under
  `enhanced` since N06.13): when `MazeSearchEngine.getInstance`
  returns null or throws, `AutorouteEngine.autorouteConnection` returns `FAILED` before
  `clear()` (`AutorouteEngine.java:142-150`), so the start rooms it completed stay in the
  autoroute tree as obstacles for later connections of the same clearance class; the port keeps
  them (`route_connection`, `// PARITY(quirk):`). A later session counts its room ids from 1
  again; Java then holds two rooms with one id, and the port moves the leaked room to a free
  negative key in place (`ShapeSearchTree::rekey_room_aside`, the tree's shape unchanged). A
  leaked room is an obstacle in room completion but never a neighbour with a door, while Java
  can build a door into it and reach its stale doors; not reproduced, and no probe board has an
  init failure (`an_init_failure_leaks_its_rooms_and_a_later_session_moves_them_aside`).
  `router.java_quirks.init_failure_room_leak` keeps it under `legacy`; N06.5 accepted the fix
  with the other four maze-search switches, and profile `enhanced` sets all five `false`
  (N06.13; see "Java quirk switches, opt-in").
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
- The `Autorouter Failed to route connection between ...` line (and the failure log's reason) names
  each side's pins as `Ref-Pad` and counts the rest of a connected set in brackets, `between
  U12-9 and C66-1 (+212 vias and traces of net GND), because ...` (K15.7); Java prints every
  item's `Item.toString` (`pin #9 of component #385`), up to 10416 characters a line. The
  sentence frame and the reason are Java's, and `AttemptResult::details` keeps Java's whole text,
  so the insert probe still compares it (`a_failed_connection_names_its_pins_by_reference_and_pad_in_the_log`,
  `a_large_side_is_counted_in_brackets_and_the_line_stays_short`). Both profiles log the new text;
  the SES does not depend on it.
- Java bugs fixed outright in N06.2: the optimizer's helper returns the number of
  nonempty routing passes rather than Java's count plus one
  (`route_passes_for_optimizer_counts_nonempty_passes`); a fanout-only run explicitly
  disables the autorouter, including for negative API `max_passes`
  (`negative_max_passes_with_fanout_does_not_route`). The optimizer discards the pass count.

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
- Java bug fixed outright in N06.2: rebuilding a via rule resets the layer radii and
  maximum radius before computing costs (`control_rebuild_resets_the_via_radii`).
  Java retained radii from larger rules; routing's rebuilds only grow the rule.
- Java bug fixed outright in N06.2: resizing a door's section range preserves every field
  of surviving section indices, initializes new indices, and drops removed ones. Queue-pop
  occupation resolves the current range, so it cannot write to an obsolete allocation.
  `a_door_whose_section_count_changes_loses_its_state` retains its historical name and now
  tests the correction, including shrink/grow, another door's storage, removed sections,
  and the post-expansion occupation write. The orchestrator measured zero reallocations
  over 118 A14 boards. Without reallocation the range and section index stay unchanged,
  so the post-expansion lookup selects the exact same arena slot as the saved pop slot;
  drill ranges and target slots are stable, and drill-page pops return before this write.
  Both profiles get this correction. `PARITY_N06_COUNTERS=1` still reports reallocations.

**Interactive routing** (`interactive`, I01, port of `gui.interactive.Route`, `RouteState`,
`DynamicRouteState`, `StitchRouteState` and `GuiBoardRoutingSettings`; tests
`tests/interactive_route.rs`, `interactive::settings::tests`)
- Java bug kept under `legacy` (N06; profile `enhanced` fixes it,
  `router.java_quirks.route_change_layer_no_via = false`, N06.11, promoted by N06.13): when the
  via rule has no via spanning both layers,
  `Route.changeLayer` returns true without a via and without changing the route's layer, and the
  route state still switches the current layer and takes a snapshot
  (`layer_change_without_spanning_via_quirk`).
- Java bug kept under `legacy` (N06; profile `enhanced` fixes it,
  `router.java_quirks.route_connect_inner_break = false`, N06.11, promoted by N06.13):
  `Route.connect` leaves only the inner loop when a corner pair fails,
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
  it a no-op); enabled here by decision (I00 decision 1, accepted by the owner): one snapshot, the
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
  the owner). The state classes are transcribed headless by the probe and compared line by line on
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

- The `.kicad_pro`'s Board Setup minimums (`min_track_width`, `min_via_diameter`,
  `min_through_hole_diameter`, `min_via_annular_width`, `min_copper_edge_clearance`,
  `min_hole_clearance`; Q18, `project::BoardMinimums`, in `Extras::board_minimums`) are read,
  KiCad's defaults where the file leaves one out or there is no project file (0.2, 0.5, 0.3,
  0.1, 0.5 and 0.25 mm), unknown where it sets 0; the DSN export does not carry them. A
  `.kicad_pcb` job sets them as `router.board_minimums.*`, `router.copper_to_edge_clearance_um`
  and `router.hole_clearance_um` at the DSN's settings priority
  (`job::prepare::board_minimums_patch`; the command line wins), so the router never goes below
  them (`tests/board_minimums.rs`, `a_kicad_pcb_input_passes_its_board_setup_minimums`,
  `kicad_board_minimums_default_to_kicads_and_zero_is_none`).
- **Edge.Cuts and Margin shapes the export leaves out are copper keepouts** (Q18.5,
  `edge_keepouts`): KiCad's DRC keeps copper `min_copper_edge_clearance` from every Edge.Cuts
  shape (its centreline) and Margin shape (its stroke), but the export writes only the outline
  (the boundary, and its holes as `signal` keepouts held at the default clearance). The reader
  adds keepouts named `edge_clearance` on every copper layer: the rectangle around each straight
  piece of every Margin shape and every Edge.Cuts shape not on an outer contour (holes, lines
  that close nothing, all of Edge.Cuts when the bounding box stands in for a malformed outline),
  grown by the edge clearance plus half the Margin stroke minus the default clearance (at least
  1 µm; curves by their chord error and 1 µm more). slice-dcmt-bread-slice's Margin lines gave
  2 KiCad `copper_edge_clearance` errors in Q18.6. Pinned by `tests/edge_keepouts.rs`
  (`margin_and_loose_edge_cuts_shapes_become_keepouts_on_every_copper_layer`,
  `the_keepouts_follow_the_projects_edge_clearance`) and `kicad_parity.rs`'s
  `edge_keepouts_are_the_only_extra_keepouts` (motorizedopener, tastexx-pcb and the synthetic
  outline have them; the comparison with the export leaves them out).
- **A pin's hole clearance is kept from its real hole** (K15.19, `board::pad_holes`,
  `DrillData::hole`): the export writes no pad drill, so Java takes a pin's drill radius as 0.45
  of the smallest pad radius (`Padstack.getDrillRadius`) and keeps `router.hole_clearance_um` from
  that. A `.kicad_pcb` job gives each pin on more than one layer the hole its pad has in the file
  (matched by the hole's centre; a slot as its centre segment and half width), under both
  profiles; the search trees' hole inflation and Q20's `hole_clearance` check measure it. On
  wisweep's driver board J10's copper-less 0.95 x 0.65 mm NPTH slot is a pin whose pad is the
  slot, estimated at a 0.146 mm drill radius, so tracks kept only the 0.2 mm copper clearance from
  its wall (K15.14's run: 0.2148 and 0.2247 mm under `min_hole_clearance` 0.25 mm). A DSN board
  is unchanged; the hole-to-hole distance (Q22.11) measures the real hole too (K15.22, next
  line). Pinned by
  `tests/k15_wisweep_edge_hole.rs::no_routed_track_comes_nearer_than_the_hole_clearance_to_an_npth`
  (0.22 mm before, at least 0.25 mm after, both profiles).
- **A via keeps the hole-to-hole distance from a pin's real hole** (K15.22, `board::hole_spacing`,
  `BasicBoard::hole_to_hole_breaches`): with `router.board_minimums.hole_to_hole_um` set, the gap
  from a via's drill to a `.kicad_pcb` pin that has its real hole (K15.19's `DrillData::hole`) is
  the distance to the hole's centre segment less the via's radius and the hole's radius (a slot's
  half width), not the padstack's estimated round drill. The maze search, the via insertion, a
  via move and Q20's `hole_to_hole` check all ask this one query, under both profiles; a DSN board
  has no real holes and is unchanged (Java has no hole-to-hole rule). On wisweep's J10 slot
  (0.95 x 0.65 mm) the estimate was a 0.146 mm radius round the centre, 0.329 mm short of the
  slot's end. Pinned by `tests/k15_wisweep_hole_to_hole.rs`:
  `a_via_keeps_the_hole_to_hole_distance_from_an_npth_slot` (a 1.4 mm minimum: the via 1.2632 mm
  from the slot before, 1.4144 mm after, both profiles) and
  `the_output_check_finds_a_via_nearer_than_the_hole_to_hole_distance_to_a_slot`.
- **Pads' solder-mask expansions** (Q18.7, `mask`, `Extras::solder_mask`): a pad's
  `solder_mask_margin`, else its footprint's, else the board's `pad_to_mask_clearance`, per
  side with a mask layer, for the pads whose expansion exceeds the smallest class clearance, each
  side judged alone (K13.1, `mask::keep_above_floor`: a side at or under it is 0, as the plugin's
  `kicad_board.mask_expansions`; `tests/k13_inputs.rs`); a
  `.kicad_pcb` job passes them as `router.solder_mask_expansions` at the DSN's priority
  (`job::prepare::solder_mask_patch`). The export carries no mask. Pinned by
  `tests/solder_mask.rs::a_kicad_pcb_input_carries_the_pads_mask_expansions`.
- **A mask-aperture `keepout` is fitted to the pins under it** (Q22.16, Q22.18,
  `io::specctra::parser::pad_keepouts`; Java reads the name and ignores it): the KiCad plugin and
  the `.kicad_pcb` reader write a board-level F.Mask / B.Mask shape as a keepout named
  `solder_mask`, a footprint's own as `solder_mask_pads`, and a footprint's own over pads of two
  or more nets as `solder_mask_shared`. Each is inserted after the components, after the pins
  whose shape on its layer meets it (Q22.18: a keepout with no pin under it, such as the 1 µm
  strips that grow an aperture, takes those of the mask keepouts it meets, and their
  `solder_mask_shared` name): pins of one net make it a keepout that carries that net,
  which routes through it while every other net keeps out; pins of two or more nets, or the
  `solder_mask_shared` name, make it a keepout that carries no net, which holds every net out,
  the pins' own too (KiCad flags a track of a pad's own net inside an aperture other nets'
  pads share; the load log names a `solder_mask_pads` or `solder_mask` keepout the router finds
  over two nets); no pin with a net leaves Java's keepout. The first two keep the name and,
  being no copper, are obstacles for traces and vias of the nets they do not carry, never for a
  pin or another area as Java's net-carrying keepout is (`ObstacleArea::is_pad_keepout`). Until
  Q22.16's remainder the pins of a multi-net keepout were cut out of it, so a trace ran along its
  own pad into the aperture to the pin's centre; the plugin and the reader now cut such pads back
  to their part outside instead (see freerouting-kicad). The DSN writer writes these keepouts back
  under their names. A DSN without the names reads as Java reads it. Pinned by
  `tests/footprint_mask.rs` (`the_plugins_keepouts_keep_other_nets_out_and_let_each_pads_net_in`,
  `the_readers_keepouts_keep_other_nets_out_and_let_each_pads_net_in`,
  `a_footprint_keepout_over_pads_of_two_nets_keeps_every_net_out`,
  `plain_keepouts_over_the_pads_leave_them_unrouted`,
  `the_output_check_reports_another_nets_trace_in_the_aperture`,
  `the_dsn_writer_keeps_the_net_of_the_keepout`,
  `the_one_net_keepout_is_an_obstacle_for_other_nets_traces_only`,
  `the_shared_keepout_holds_every_nets_traces_out_and_no_pin`,
  `a_pad_of_the_shapes_own_net_under_a_board_level_mask_shape_connects`).
- **A net tie's copper keepout lets the tie's nets in** (Q22.19,
  `io::specctra::parser::pad_keepouts`, `NET_TIE_KEEPOUT_NAME`; Java reads the name and ignores
  it): the KiCad plugin and the `.kicad_pcb` reader write each piece of a net-tie footprint's
  copper shape as a keepout named `net_tie` at the default clearance. It is inserted after the
  components and carries the nets of the pins under it and under every `net_tie` keepout on its
  layer joined to it by touching ones (a shape's pieces), however many: those nets route through
  it, every other net keeps its clearance from it, as KiCad lets copper of the tie's nets touch
  the shape. Like the mask keepouts it is an obstacle for traces and vias only, and never fitted
  with them; with no pin under the joined pieces it is Java's keepout. A pin of another footprint
  touching the shape counts too (KiCad counts the tie's own pads; on a board KiCad accepts only
  those touch it). Q22.4 wrote these as `copper_graphic`, a keepout for every net, which left
  slice's J1-10 to NT1-2 (GNDPWR) open. The DSN writer writes it back under its name. Pinned by
  `tests/net_tie.rs` (`the_net_tie_keepout_carries_the_nets_of_the_tie_pads`,
  `the_plugins_net_tie_lets_its_nets_in_and_keeps_c_out`,
  `the_readers_net_tie_lets_its_nets_in_and_keeps_c_out`,
  `with_a_keepout_for_every_net_the_ties_pads_stay_unrouted`,
  `the_output_check_lets_the_ties_nets_in_and_keeps_the_clearance`,
  `a_ties_pieces_share_its_nets_and_a_piece_with_no_pin_is_a_plain_keepout`,
  `the_dsn_writer_keeps_the_net_tie_keepout`).
- **A thermal-spoke keepout carries its pad's net** (Q22.20,
  `io::specctra::parser::thermal_keepouts`, `THERMAL_SPOKE_KEEPOUT_NAME`; Java reads the name
  and ignores it): the KiCad plugin and the `.kicad_pcb` reader write one keepout named
  `thermal_spoke` along each thermal spoke of a pad that a zone of its net connects by thermal
  relief (see freerouting-kicad). It is inserted after the components and carries the net of
  the pins under it, so the pad's net routes through it and traces and vias of every other net
  keep out; like the mask keepouts it is an obstacle for traces and vias only. One with no pin
  or pins of several nets under it is left out, with one log line. The DSN writer writes it back
  under its name. A DSN without the name (Java's inputs) reads as before, so `legacy` on Java's
  DSNs is unchanged. Pinned by `tests/thermal_spokes.rs` (`the_keepouts_carry_the_pads_net`,
  `another_nets_trace_leaves_the_spokes_free`, `the_pad_is_still_reachable_by_its_own_net`,
  `the_output_check_reports_only_another_nets_trace_in_a_spoke_way`,
  `the_dsn_writer_keeps_the_keepouts`).
- **Q20's output check finds copper inside a keepout that keeps no clearance** (Q22.18,
  `drc::board_rules`, `zero_clearance_keepouts`): the clearance check (Java's
  `clearance_violations`, as ported) reports a shortfall of the required clearance, so copper
  overlapping a keepout whose clearance to it is 0 (the mask keepouts' `(clearance_class null)`)
  was no breach. The output check adds one `keepout` breach per such keepout and layer whose
  copper overlaps the item's. The DRC and its Java digest are unchanged. Pinned by
  `tests/footprint_mask.rs::another_nets_trace_may_run_at_the_apertures_edge_but_not_in_it`.
- **The board's own solder-mask apertures are copper keepouts** (Q18.7, `edge_keepouts`): KiCad's
  DRC flags copper of two nets in one mask aperture (`solder_mask_bridge`), and a board-level
  F.Mask or B.Mask shape is an aperture no pad owns (slice-dcmt-bread-slice's two filled
  `gr_poly` over its motor-driver zones gave 2 Track bridges in the Q18.7 re-measure). The reader
  adds keepouts named `solder_mask` on the top (F.Mask) or bottom (B.Mask) copper layer: a filled
  shape's own polygon (a circle grown by the maximum error and 1 µm), and its stroke, or an
  unfilled shape's, the rectangles within half the stroke of each piece. They held every net
  out, the net of the copper under the aperture too, until Q22.18: the router now fits them to
  the pins under them like a footprint's (see freerouting), so a pad of the shape's own net
  under it connects. Footprint mask shapes were left to the pads' expansions until Q22.16
  (below). Pinned by
  `tests/edge_keepouts.rs::the_boards_mask_apertures_become_keepouts_on_their_copper_side`; the
  `kicad_parity.rs` comparison leaves them out.
- **Mask apertures grow by the board's solder-mask-to-copper clearance** (Q22.12, `mask`,
  `edge_keepouts::mask_polygons`, `Project::solder_mask_to_copper`): KiCad's DRC keeps copper of
  another net `solder_mask_to_copper_clearance` from a mask aperture (`solder_mask_bridge`;
  fc-rom-vomitter-nescart 0.05 mm, mydevboard and solarmeshtastic 0.005 mm). The reader takes it
  from the `.kicad_pro` (`board.design_settings.rules`, KiCad's default 0 when the key is
  missing, a negative value as 0) and folds it into both mask inputs: a pad side with an
  aperture expands by margin + clearance (at least 0) before the smallest-class-clearance filter,
  so `router.solder_mask_expansions` and Q20's output check carry the grown aperture with no
  setting of their own; each board F.Mask / B.Mask shape is taken with a stroke twice the
  clearance wider (square corners, a superset of the rounded growth). With 0 the settings are
  Q18.7's byte for byte; the DSN's mask keepouts have Q22.18's 1 µm strips. Pinned by
  `tests/edge_keepouts.rs::the_mask_apertures_grow_by_the_projects_mask_to_copper_clearance` and
  `crates/freerouting/tests/solder_mask.rs::the_projects_mask_to_copper_clearance_grows_the_pad_apertures`.
- **A footprint's own solder-mask shapes are copper keepouts** (Q22.16, `footprint_masks`,
  `mask::FootprintMasks::allow_bridges`): KiCad's export leaves a footprint's F.Mask / B.Mask
  shapes out (fc-rom-vomitter-nescart's J1, a 93.5 x 11.3 mm `fp_poly` over 72 edge fingers
  whose pads have no mask layer, let other nets' tracks through: 7 routing-caused
  `solder_mask_bridge`, KiCad's per-type cap hiding the rest). The reader adds each, as filed,
  as `edge_keepouts::mask_polygons` makes a board shape (grown by Q22.12's clearance), named
  `solder_mask_pads` on its side's copper; the router fits it to the pins under it (see
  freerouting). A footprint with `(attr ... allow_soldermask_bridges)` gets none: KiCad's DRC
  ignores its apertures. Pinned by `tests/footprint_masks.rs`.
- **Room for each thermal spoke is kept out of other nets** (Q22.20, `thermal_spokes`, every
  profile; decided 2026-10-20: routing never breaks the board's manufacturing rules): KiCad's
  DRC (`starved_thermal`) wants `min_resolved_spokes` (default 2) spokes per thermal pad to
  reach fill connected to something else, and routed tracks of other nets in the spokes' way
  took that fill (canary-pwr `ab786fcc`: 6 on J1's GND header pads). For every pad that a copper
  zone of its net connects by thermal relief (the pad's `zone_connect`, else the footprint's,
  else the zone's; `thru_hole_only` for plated through-hole pads) on a signal layer, the reader
  writes one rectangle per spoke KiCad builds (`ZONE_FILLER::buildThermalSpokes`: a circle's
  turned by its spoke angle, another shape's at the spoke angle to its unrotated box, then the
  pad's orientation), from the pad's centre to the spoke's end plus the spoke's width and the
  zone's clearance, the spoke's width plus twice the zone's clearance wide, named
  `thermal_spoke` with `(clearance_class null)`; the router fits it to the pad's net (see
  freerouting). None where the zone's outline misses the pad's centre or the far end, where a
  higher-priority zone of another net or a no-fill rule area holds the far end, for a custom
  pad, for a spoke narrower than the zone's minimum width, or when the project's
  `min_resolved_spokes` is 0. Custom DRC rules are not read. Pinned by `tests/thermal_spokes.rs`
  (`every_thermal_pad_gets_a_keepout_along_each_spoke`, `the_plugin_writes_the_same_keepouts`,
  `min_resolved_spokes_zero_writes_none`, `no_keepout_where_the_zone_does_not_reach_the_spoke`).
- **Pads under a footprint aperture that pads of several nets share are cut back to their part
  outside it** (Q22.16, decided 2026-10-20: option (b); `pad_trim`): a `solder_mask_pads`
  keepout over pads (any footprint's, overlapping by more than 1 µm²) of two or more nets becomes
  `solder_mask_shared`, and each one-layer pad under such keepouts gets a padstack of its largest
  part outside them (the pad cut by one keepout edge's outer half-plane; a polygon, corners
  rounded to 1 µm, named `<padstack>_outside_mask_<k>`) and its pin's anchor at that part's
  centroid, unrotated (an image other components use too is copied as `<image>_<ref>`, unless
  they all change alike). KiCad's DRC flags a track of the pad's own net inside such an
  aperture, and the router ends a trace at the pin's centre, which on fc-rom's edge fingers lies
  inside. A pad with no part outside, or a concave one, is named with its net in a warning and
  stays unreachable; a pad on several layers stays as it is. The SES and the routed
  `.kicad_pcb` carry only traces and vias, so the cut pads never reach KiCad. Pinned by
  `tests/footprint_masks.rs` (`a_pad_partly_inside_a_shared_aperture_is_cut_back_to_its_part_outside`,
  `a_pad_wholly_inside_a_shared_aperture_is_named_in_a_warning`,
  `a_shared_image_is_copied_for_the_component_whose_pads_are_cut`).
- **Mask keepouts keep no clearance of their own** (Q22.18, `edge_keepouts::mask_keepout`): the
  `solder_mask`, `solder_mask_pads` and `solder_mask_shared` keepouts carry
  `(clearance_class null)`, the router's class 0, so copper keeps exactly the board's
  solder-mask-to-copper clearance (Q22.12's growth) from the aperture rather than the default
  area clearance on top of it, as KiCad's `solder_mask_bridge` asks. So that copper on a
  keepout's edge never lies inside KiCad's aperture, every mask shape grows by
  `MASK_MARGIN_NM` (1 µm) on top of the clearance: the written keepouts, after the chords and the
  DSN's rounding, hold the exact grown aperture (worst inward distance measured with pcbnew's
  exact apertures and shapely: 2.6 nm on canary-pwr before, 0 after, on q22-fp-mask, dev-board,
  canary-pwr, vfo-controller and slice-dcmt-bread-slice). A pad partly inside is cut back to its
  part outside the grown aperture. Pinned by
  `tests/footprint_masks.rs::the_mask_keepouts_keep_no_clearance_of_their_own` and
  `tests/edge_keepouts.rs::the_mask_keepouts_hold_the_exact_grown_apertures_after_rounding`.
- **Copper shapes and copper texts are obstacles** (Q22.4, `copper_graphics`): KiCad's DRC
  keeps other copper its clearance from every shape and visible text on a copper layer, and the
  export writes none of them (footprint copper lines only as drawing outline; pcietest had 127
  routed tracks over its connectors' B.Cu `fp_line`s, sonde-xilinx one over a `gr_text`). The
  reader adds keepouts named `copper_graphic` on the shape's layer (a filled body, and the
  rectangles within half the stroke of each piece) for a shape of no net or of a net without
  pins, the same pieces named `net_tie` for a shape of a net-tie footprint (Q22.19: the router
  lets the tie's pads' nets in, see above; Q22.4 named them `copper_graphic`, which kept those
  nets out too), and keepouts named `copper_text` for a visible text: a
  box that holds the glyphs (no font engine: `1.25 × width` per character plus the stroke, by
  `1.8 × height` plus the stroke, a footprint text the square of half side `hypot(width,
  height)`). A shape of a net with pins becomes copper of that net (decided 2026-10-20, option
  (a)): each polygon of the same outline code (the filled body, the rectangle of each stroke
  piece; a circle as a polygon) becomes a conduction-area wire of the net, `(type fix)` with
  `(attr obstacle)`, at the end of the wiring. The engine makes it a conduction area that joins
  the net and is an obstacle to other nets whatever `router.plane_as_obstacle` says; it is
  system-fixed, so neither the session nor the `.kicad_pcb` output carries it. An earlier model,
  fixed wires of the net covering each shape in 50 µm strips, gave vfo-controller 665 wires
  under the 0.2 mm minimum width and cut its route at 120 s; it was dropped.
  Pinned by `crates/freerouting-kicad/tests/copper_graphics.rs`
  (`a_footprint_copper_line_becomes_a_keepout_on_its_layer`,
  `a_copper_text_becomes_a_keepout_box_that_holds_its_glyphs`,
  `shapes_of_a_net_become_conduction_areas_of_their_net`, `the_written_dsn_keeps_the_areas`,
  `net_tie_shapes_and_shapes_of_a_net_without_pins_are_keepouts`,
  `a_net_tie_shape_is_a_net_tie_keepout_the_same_as_the_plugins`) and the engine's
  `tests/copper_graphics.rs` (`the_route_goes_round_the_copper_line_and_the_copper_text`,
  `other_nets_keep_clear_of_a_net_s_copper_and_the_net_connects_through_it`,
  `the_session_and_the_kicad_output_leave_the_net_shapes_out`,
  `the_output_check_counts_a_crossing_as_routed_and_keeps_the_area`).
- The `.kicad_pro`'s minimum clearance (`min_clearance`, `m_MinClearance`; Q22.10,
  `project::BoardMinimums::clearance`) is read, none where the file leaves it out (KiCad's
  default is 0) or sets 0; a `.kicad_pcb` job sets it as `router.board_minimums.clearance_um`
  (`job::prepare::board_minimums_patch`). Pinned by
  `tests/clearance_floor.rs::a_kicad_pcb_input_passes_its_minimum_clearance`.
- The `.kicad_pro`'s minimum hole-to-hole distance (`min_hole_to_hole`, `m_HoleToHoleMin`;
  Q22.11, `project::BoardMinimums::hole_to_hole`) is read, KiCad's default of 0.25 mm where the
  file leaves it out, none where it sets 0; a `.kicad_pcb` job sets it as
  `router.board_minimums.hole_to_hole_um` (`job::prepare::board_minimums_patch`). Pinned by
  `tests/hole_to_hole.rs::a_kicad_pcb_input_passes_its_hole_to_hole_minimum`.
- **`.kicad_dru` `hole_to_hole` rules reach the router** (Q22.14, `dru::item_types::hole_to_hole`;
  Java's plugin never applies the `.kicad_dru`): the largest `hole_to_hole` minimum among the
  rules that can match a pair of holes with a via (of one net or of two) raises
  `router.board_minimums.hole_to_hole_um`, over Board Setup's `min_hole_to_hole` and the command
  line (the larger wins), on both inputs (`--kicad-dru`, and the `.kicad_pcb` reader with
  `apply_design_rules`), and the log names the rules. Conservative (decided 2026-10-20, option
  (a)): a rule for holes of two nets also holds between holes of one net, with a warning; a rule
  between pads only is left out. ch32v003: 0.5 mm from "hole to hole clearance (different nets)"
  (`tests/dru_holes.rs`: `a_hole_to_hole_rule_is_a_floor`,
  `the_largest_rule_that_can_apply_wins`, `the_job_raises_board_setups_hole_to_hole`,
  `a_kicad_pcb_job_takes_the_larger_of_board_setup_and_the_rules`).
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
  `ReadOptions::apply_design_rules`, off by default in the reader and on for a CLI route since K13.1 (K00 question 7; `supported_rules_change_the_tree_exactly`,
  `option_off_leaves_the_tree_alone`, `design_rules_reach_the_board`). `canary-pwr`'s ten
  `insideCourtyard` rules are all UNSUPPORTED (`canary_pwr_rules_are_unsupported_by_name`).
- **`.kicad_dru` clearances between item types reach the router** (Q22.5, `dru::item_types`;
  Java's plugin never applies the `.kicad_dru`): conditions on `A.Type` (`track`, `via`, `pad`),
  `B.Type == A.Type`, `A.isPlated()` and `A.Pad_Type` become the DSN's typed clearances
  (`via_wire`, `pin_wire`, `smd_wire`, ...), applied to every net-class pair of the kind pairs
  they match (a warning names a rule that is applied more widely than its condition); a rule
  asking no more than the DSN gives, one about pads only, and one about zones, text or
  same-net pairs is reported and left out. A `hole_clearance` rule becomes a floor of
  `router.hole_clearance_um`, the command line's value included, unless it is between plated
  holes and tracks only (Q22.15): then a via hole's becomes `via_wire` of the rule's value less
  the smallest annular ring of the via padstack names (`Via[a-b]_<d>:<drill>_um`), and a plated
  pad's `pin_wire` of the value itself (the DSN gives no pad drill; warned). On ch32v003 the floor
  of 330 µm also kept every routed via's copper from pad and via drills, which the rule does not
  ask: KiCad unconnected 38 → 34 with Q22.14 in, 0 routing-caused breaches (`tests/dru_holes.rs`:
  `a_plated_hole_rule_becomes_via_and_pin_to_track_clearances`,
  `a_via_without_a_known_drill_keeps_the_floor`). With several net classes a
  routed via of the default class is kept from other vias and pads at the via-to-track value
  (conservative). A `(layer ...)` that names no copper layer of the board (`inner` on two
  layers) now leaves its rule out instead of applying it on every layer. Pinned by
  `tests/dru_item_types.rs` (`plated_is_exact_with_one_class`,
  `plated_sets_via_and_pin_wire_in_every_class`, `hole_clearance_is_a_floor`,
  `ch32v003_rules_report`, ...).
- **Pads' own larger clearances are copper keepouts** (K13.1, `pad_clearance`): KiCad's DRC keeps
  other copper a pad's `(clearance v)` from it, and the export writes no per-pad clearance
  (canary-pwr's six 0.6 mm fiducials gave one routing-caused `clearance` on the bare export in
  Q22.6; tinytapeout-demo and CM5_MINIMA_3 have such pads too). The reader adds, for each pad
  whose own clearance exceeds the default, a circle keepout named `pad_clearance` on each outer
  copper layer of the pad, `max(size) + 2 · (clearance − default)` across, as the plugin's
  `edge_and_pad_keepouts` writes it on the DSN path (at the file's nanometres; the plugin rounds to
  µm). The keepout holds every net out, the pad's own too, as on the DSN path. Pinned by
  `tests/k13_inputs.rs::pads_own_clearances_become_circle_keepouts` and
  `kicad-plugin/tests/test_mask_floor.py::test_pad_clearance_circles_match_the_readers`; the
  `kicad_parity.rs` comparison leaves them out.
- **A footprint's own clearance reaches its pads** (K15.9, `pad_clearance`; KiCad's
  `PAD::GetLocalClearance`, checked on 10.0.7): a pad with no positive `(clearance v)` of its own
  takes the footprint's `(clearance v)` (a pad's `(clearance 0)` loads as "none" in KiCad and
  inherits too), and the pad's own positive value wins. The plugin's DSN fix-up reads the pad's own
  value only, so a footprint-level one is kept on a `.kicad_pcb` input and not on the plugin's DSN
  path. Pinned by `tests/k15_pad_clearance.rs` in `freerouting-kicad`
  (`a_footprint_clearance_reaches_the_pads_without_their_own`,
  `the_pads_own_clearance_wins_over_the_footprints`, `the_pcu_fiducial_has_its_circle`) and, in
  `freerouting`, `tests/k15_pad_clearance.rs::a_route_keeps_the_pads_own_clearance` (the pcu's
  FID1 cut into `k15-fiducial.kicad_pcb`: the route keeps 0.652 mm from the pad with the keepout,
  0.245 mm without).
- **Board-wide `.kicad_dru` rules without a condition apply** (K15.3, `dru::floors`; Java's plugin
  never applies the `.kicad_dru`): a rule with no condition holds for every item it can.
  `track_width` sets `router.board_minimums.track_width_um`, `clearance`
  `router.board_minimums.clearance_um` (every class pair), `hole_size` `via_drill_um`, `annular_width`
  of vias `via_annular_um`, `edge_clearance` `router.copper_to_edge_clearance_um`, after every merge.
  K15.44 (decided 2026-10-22, as KiCad 10.0.7's DRC does): a custom rule takes precedence over Board
  Setup's constraint of the same kind, so where the rules cover every copper layer their value
  **replaces** one whose source is Board Setup (the `.kicad_pro`, or the plugin's
  `--board-setup.router.*` flags) or a default, larger or smaller; of several rules the last in the
  file wins (per layer); where no rule applies Board Setup's value stays. A value the user set
  (command line, environment, settings file) is only raised to the rule (K15.3's floor), and so is
  Board Setup's when a layer-limited rule leaves a layer to it; `hole_clearance` and `hole_to_hole`
  only raise as before (their floors also come from rules about some item pairs). The log line and
  `--dump-settings`'s source (`kicad_dru, over <source>`) say which value won and why.
  **Narrower than KiCad:** the router keeps one value for each, so a rule limited by a layer
  clause (the pcu file's `edge_clearance` 0.254 mm on `outer` and 0.381 mm on `inner`: one edge
  clearance for all layers, the largest, 0.381 mm) is applied to every layer with a warning naming
  it. A `min` limited to some net classes is never a board-wide floor (it would over-constrain every
  net): `track_width` raises the width rule of each selected class to at least the `min` (never
  lowers one; the Default class's structure width follows), the other kinds are UNSUPPORTED,
  "limited to some net classes; the router keeps one board-wide value". `disallow blind_via|buried_via|micro_via` is reported "met" when no via
  padstack of the board (`Via[a-b]_...`) is of that kind, else those padstacks leave the router's
  choices (the DSN cannot tell a micro via from a blind one of the same span, so `micro_via` removes
  both, with a warning). Left out and reported by name with the reason: `hole_size` with only a
  `max` and a pad's annular ring or hole (`A.Hole_Size_X ...`, "the router places no pads"), any
  other `disallow` argument. Same on both inputs (the reader and `--kicad-dru`). Pinned by
  `freerouting-kicad/tests/dru_floors.rs` (`every_rule_of_the_pcu_file_is_applied_or_reported_with_its_reason`,
  `the_floors_are_the_rules_values`, `each_mapped_setting_has_the_rules_value_on_both_inputs`,
  `a_rule_below_the_command_line_leaves_it_and_replaces_board_setups_value`,
  `an_unconditional_rule_replaces_board_setups_minimum_on_both_inputs`,
  `no_rule_leaves_board_setups_minimum_on_both_inputs`, `an_explicit_track_minimum_stays_on_both_inputs`,
  `the_last_unconditional_rule_in_the_file_wins`,
  `an_edge_rule_replaces_board_setup_only_when_it_covers_every_layer`,
  `disallow_removes_the_via_kinds_a_board_offers`,
  `a_class_limited_track_width_min_raises_only_that_class_on_a_dsn`,
  `..._on_a_kicad_pcb`, `a_class_limited_hole_size_min_is_unsupported`) on
  `tests/data/pcu-rev2.kicad_dru`.
- **Class widths on a `.kicad_pcb` input** (K14.1, `fixups`, `router.input_fixups.class_widths`,
  off by default under both profiles): `<class>=<um>` entries set each class's first width rule
  in the reader's tree, as the plugin's DSN fix-up `class_widths` writes `(rule (width W))` into
  KiCad's export; Java has neither. The plugin's snapshot passes it, `auto` resolved from the
  board (`dsn_fixups.resolve_on_board`, the classes from `export_flow.board_classes`). A class the
  board has no width rule for is an error naming it. A class whose width an applied `.kicad_dru`
  `track_width` rule set keeps the rule's width, with one line naming the class and the rule, as on
  the DSN path, where `--kicad-dru` runs after the text fix-up. Pinned by
  `tests/k14_fixups.rs::class_widths_equal_the_fixed_up_export` (the tree's class rules equal the
  plugin's fixed-up export of `tests/data/k14-fixups.kicad_pcb`, parsed, exactly),
  `a_class_the_kicad_dru_track_width_sets_keeps_the_rules_width`,
  `an_unknown_class_is_an_error_that_names_it` and
  `kicad-plugin/tests/test_snapshot_fixups.py::test_auto_on_the_snapshot_gives_the_dsn_paths_lists`.
- **Plane layers opened to routing on a `.kicad_pcb` input** (K14.2, `fixups`,
  `router.input_fixups.route_on_plane_layers`, off by default): each listed power layer becomes
  a signal layer and keeps its planes, as the plugin's `route_on_plane_layers` turns the export's
  `(type power)` into `(type signal)`. An unknown layer, or one that is not a power layer, is an
  error naming it. Pinned by
  `tests/k14_fixups.rs::opened_plane_layers_equal_the_fixed_up_export_and_keep_their_planes` and
  `an_unknown_layer_or_a_signal_layer_is_an_error_that_names_it`.
- **Pours of listed nets dropped on a `.kicad_pcb` input** (K14.3, `fixups`,
  `router.input_fixups.drop_pour_nets`, off by default): the planes of the listed nets on the
  first and last copper layer leave the tree, as the plugin's `drop_pour_nets` drops their
  `(plane ...)`; their inner-layer planes stay. A net the board does not have is a warning naming
  it and is left out (the DSN fix-up passes it silently). A DSN input ignores all four `router.input_fixups.*`
  settings with one warning (`k14_fixups.rs::engine::a_dsn_job_ignores_the_settings_with_a_warning`).
  Pinned by `tests/k14_fixups.rs::dropped_pours_equal_the_fixed_up_export_and_inner_planes_stay`,
  `an_unknown_net_is_a_warning_that_names_it` and
  `kicad-plugin/tests/test_snapshot_fixups.py::test_auto_on_the_snapshot_gives_the_dsn_paths_lists`.
- **Pours of listed classes as fixed wires on a `.kicad_pcb` input** (K14.4, `fixups`,
  `fill_wires`, `router.input_fixups.fixed_pour_classes`, off by default under both profiles): each
  filled zone on the first or last copper layer whose net is in a listed class, and not in
  `drop_pour_nets` (dropping wins), gives `(type fix)` wires of its net covering its fill, first in
  the wiring, and the planes of that net on that layer leave the tree, as the plugin's
  `fixed_pour_classes` in `wires` mode rewrites KiCad's export; Java has neither. The fill is the
  file's `(filled_polygon ...)` (read for this setting only), the rectangles and wires are the
  plugin's `fill_rects` and `_wire` arithmetic, bit-identical. An unknown class is an error naming
  it. The wires are `SYSTEM_FIXED`: the session leaves them out, as on the DSN path, and the
  engine's `.kicad_pcb` writer now leaves every `SYSTEM_FIXED` trace out too (a locked original,
  which its diff keeps anyway, or such a wire). The plugin's snapshot passes it, `auto` resolved
  from the board's classes (`dsn_fixups.auto_fixed_pour_classes_of_board`), and resolves `auto`
  for `fixed_pour_mode = engine` the same way. Pinned by
  `tests/k14_fixups.rs::fixed_pours_equal_the_fixed_up_export` (44 wires, net, layer, width and
  coordinates in order, and the planes, exactly equal to the plugin's export parsed),
  `fixed_pours_alone_equal_the_fixed_up_export`, `several_fixed_pours_come_in_the_plugins_zone_order`,
  `a_dropped_net_is_not_covered_by_fixed_wires`, `an_unfilled_pour_keeps_its_plane`,
  `the_fills_read_from_the_file_equal_the_plugins_board_facts`,
  `an_unknown_fixed_pour_class_is_an_error_that_names_it`,
  `engine::the_fixed_pour_wires_are_on_the_board_and_in_no_output`,
  `crates/freerouting-cli/tests/cli_k14_fixed_pours.rs` and
  `kicad-plugin/tests/test_snapshot_fixups.py::test_auto_on_the_snapshot_gives_the_dsn_paths_lists`.
- **A plane is cut where a higher-priority zone of another net fills** (K15.15,
  `zone_priority`; KiCad 10.0.0 `ZONE::HigherPriority` and
  `ZONE_FILLER::buildDifferentNetZoneClearances`): KiCad's export writes every copper zone's whole
  outline as its plane, and Java (`Structure.java` 1141, `ConductionArea.getNormalContacts`) then
  counts a pin or via centred anywhere in it as connected, but KiCad's filler leaves out of the
  zone every zone of another net on the layer that is higher (a teardrop above any other zone,
  then the larger `(priority N)`, then the larger UUID), grown by the clearance between the two.
  The reader gives the lower plane one convex window per pair of convex pieces of the two outlines
  that overlap, the higher piece grown by the larger of the two zones' `(connect_pads (clearance
  C))` (0.5 mm without one); equal priorities without both UUIDs cut nothing.
  `ReadOptions::uncut_planes` writes the export's whole outlines (the export comparisons in
  `tests/{copper,k14_fixups}.rs` use it). On the power-control unit In2.Cu's V_BUS_24V (priority
  41) over V_BUS_12V (priority 1) left three V_BUS_12V vias that the router counted on V_BUS_12V's
  plane and KiCad found dangling. The cut uses the higher zone's outline, not its fill, so the
  router may count a connection open where the higher zone's fill does not reach its outline. A
  DSN input cannot know it (the export writes no priority), so the DSN path and `legacy` on a DSN
  are unchanged; the plugin's own DSN export cuts its planes before the router reads them (K15.24,
  the kicad-plugin section). Pinned by `tests/k15_zone_priority.rs` in `freerouting-kicad`
  (`the_lower_zones_plane_has_the_higher_zone_cut_out`,
  `the_higher_priority_decides_which_plane_is_cut`, `equal_priorities_fall_to_the_larger_uuid`,
  `a_zone_of_the_same_net_cuts_nothing`, `a_teardrop_is_above_every_zone`,
  `zones_on_different_layers_cut_nothing`; the fixture is the pcu's two In2.Cu zones), the
  `zone_priority` unit tests, and `crates/freerouting/tests/k15_overlapping_planes.rs`
  (`the_via_in_the_overlap_is_not_on_the_lower_plane`, `the_via_in_the_overlap_stays_open`,
  `the_router_reaches_the_lower_plane_where_it_is_filled`).
- Against the export's board the reader's differences are exactly the ones above and nothing
  else: `crates/freerouting/tests/kicad_parity.rs` lists each with its reason and fails on any
  other, and on a listed one that no longer occurs; the item order is the export's except for
  the class order, the order of the outline's holes (`tastexx-pcb`) and what the area and arc
  differences shift.

## freerouting-cli

**Command line and job outputs** (`freerouting-cli`; `freerouting::job::{run, outputs, deadline,
log}`; tests `crates/freerouting-cli/tests/{args_java, cli_outputs, settings_parity, version}.rs`)
- The executable is `freerouting-rs` (`freerouting-rs.exe` on Windows); Java's launcher is
  `freerouting`. Decided 2026-10-20, so that both can be installed side by side and nothing that
  looks up `freerouting-rs` (the KiCad plugin, the installer, scripts) can pick up Java's.
  `--version` prints `freerouting-rs <x.y.z> (git <sha>)` and `--capabilities` gives
  `"name": "freerouting-rs"` (`the_version_line_matcher_rejects_other_lines`,
  `capabilities_is_json_and_lists_every_flag`). The flags, outputs and exit codes are Java's.
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
- A router setting the port had and removed (`settings::removed`: Q12's `autorouter.portfolio`,
  `autorouter.restarts` and `autorouter.portfolio_memory_mb`, removed 2026-10-20 by Q12.5's fail
  branch; Q23's `autorouter.restarts_from_best`, removed 2026-10-22 by Q23.4's fail branch) is
  refused, where Java warns about an unknown field and routes on. On the command line
  it exits 2 before any work; in a settings file it fails the read (an explicit `--settings`
  file exits 2, the default file is skipped with a warning); from an environment variable it is
  a warning. The message names the key, the date, the reason and the valid `router.autorouter`
  keys (`removed_portfolio_settings_are_usage_errors`, `tests/settings_removed.rs`).
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
  analysis threads with it (DRC, statistics, airlines; P03.1), which changes no output. The
  autorouter's passes are sequential (D5) at any count; only the optimizer and the analyses use the
  threads, and `--help` says so (K15.8). `--seed 0`
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
- With `--router.output_check=report` or `repair` (profile `enhanced`'s value, Q20.3) the
  manifest gets `output_check` after `widths`: `{mode, checked, found, removed, retried, repaired,
  left_open, remaining, by_rule, input}`; with `off` (profile `legacy`) the key is absent
  (`result_json_output_check.rs`, `the_manifest_carries_the_output_check`; under `enhanced`
  `a_routed_run_writes_every_key` expects Java's keys plus `output_check`).
- Stopping: a stdin reader acts on `stop` (stop everything) and `stop-router` (Java's
  `requestStopAutoRouter`); end of input is not a stop (`stdin_eof_is_not_a_stop`). Java's CLI
  has no such channel. Ctrl-C and SIGTERM (SIGINT, SIGTERM and SIGHUP on Unix, Ctrl-C and
  Ctrl-Break on Windows, through the `ctrlc` crate) are handled: the first is `stop`, the
  second aborts with exit 130 and no output (`sigint_and_sigterm_are_a_stop`,
  `two_signals_abort`); Java's CLI has no such two-level stop.
  The reader consumes standard input from the start of a route, so a shell `while read` loop
  loses its lines; `--stdin-commands off` (K15.8, default `on`; the plugin relies on `stop`)
  does not start it, and signals still work (`tests/stdin_commands.rs`,
  `with_stdin_commands_off_three_lines_stay_unread`, `by_default_the_route_reads_standard_input`).
- A stop request also ends a trace normalization (`BasicBoard::normalize_traces_until`, from the
  insertion after a route and from pull-tight): the first pass runs to its end, a later pass ends
  at its next trace and the traces stay as they are, as at the iteration limit, but the net is not
  suppressed. Java's `normalizeTraces` has no stop check, so a net whose split and combine
  oscillate runs all 2000 passes after a stop: on `Issue690-kit-dev-coldfire-xilinx_5213` the GND
  net held one fanout pin for about 280 s, and a SIGTERM took over 5 minutes. Without a stop request
  the result is Java's (`tests/normalize_stop.rs`). A sweep pass, which routes on its own token,
  also gets the job's stop while a connection runs (`with_stop_relay`, every 20 ms), where Java
  has no such stage.
- `--profile legacy|enhanced` (N01.3) is the routing profile (`router.profile`, see "Profile
  `enhanced`"); Java has no such flag, and `--profile.<key>=<value>` (Java's user profile) is still
  accepted and ignored. `--list-stages[=json]` and `--dump-settings[=json]` (N01.4) print the stage
  slots with their registered ids and options, and the effective settings with their sources, and
  exit 0; Java has neither (`tests/profiles.rs`).
- `--stats json` and `--stats-out <file>` (P01.2) are developer flags that Java lacks. They are
  taken out of the command line before the Java parsers see it, so they are not plugin flags and
  not in `--capabilities`. They write per-stage work counts and times and change no result: the
  session is byte-identical with and without them and the counts of two runs are equal
  (`stats_change_no_session_and_the_counts_repeat`).
- `--kicad-dru FILE` (Q22.5) applies a KiCad `.kicad_dru` to a DSN input (`JobInput::kicad_dru`,
  `prepare_job`); Java has no such flag. An unreadable file fails the run; with a `.kicad_pcb`
  input the flag is ignored with a warning. Pinned by
  `dru_item_types.rs::the_job_applies_the_rules_and_the_hole_floor`.
- **A `.kicad_pcb` route applies its `.kicad_dru` rules by default** (K13.1, decided 2026-10-20:
  the board's own rules reach the router on both inputs): `cli.rs` reads the board with
  `ReadOptions::apply_design_rules` on, so the class-pair, track-width, item-type, hole-clearance
  and hole-to-hole rules reach the route as `--kicad-dru` gives them to a DSN route. K10.3 applied
  them only with `--apply-design-rules`, which now only says the default; `--no-design-rules`
  (new) reads and reports the file and applies nothing; both together are a usage error. Java
  reads no `.kicad_pcb`. The DRC mode and the GUI still read the rules without applying them, on
  both inputs. Pinned by `cli_kicad.rs`
  (`a_kicad_pcb_route_applies_the_item_type_and_hole_rules_by_default`,
  `the_apply_design_rules_switch_changes_the_class_rules`,
  `the_two_design_rules_switches_together_are_a_usage_error`) and
  `args_java.rs::no_design_rules_is_a_switch`.

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
- Info > Board Rules (Q20.3, `panels/board_rules.rs`) lists the engine's board-rule check
  (`drc::check_board_rules`) on the open board under its own rules; Java has no such list
  (`the_list_shows_the_boards_breaches`).
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
- **Delete, Fix and Unfix of the selection** (decision G00 question 6, accepted by the owner
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

**Dropped Java features** (G00 question 2, accepted by the owner 2026-10-15; recorded 2026-10-16)
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
- The dialog adds what Java's window lacks (N01.3, N01.4): a Profile selector (Legacy / Enhanced;
  switching moves the values that still have the old profile's value) and an "Algorithms"
  section with one row per stage slot (the registered ids to pick from, then the slot's options),
  each marked "changed" when it differs from the profile, with a Reset to the profile's value
  (`profile_selector_covers_both_profiles`, `an_algorithm_stage_changes_and_resets`).
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
- A stop keeps the front-end window open (decided 2026-10-19). Java's `GuiManager` exports and
  calls `System.exit(0)` on both `autorouterAborted` and `autorouterFinished` when
  `exitWhenFinished`. Here, when the user stopped the job (Cancel, Esc, a canvas click, the stop
  command), the front-end mode writes the output as for any end, shows "Routing stopped. The result
  is saved to <file>; close the window to return it, or route again." in the status line and the
  log, and does not close. A job that ends on its own still closes the window when
  `exit_when_finished`; closing the window (also while a job runs) writes the output if the board
  changed or has traces, then closes, as before. Test:
  `frontend_stop_writes_the_output_and_keeps_the_window`, `frontend_close_during_a_job_still_closes_after_the_stop`.
- "Stop routing on a canvas click" is a preference (Settings > Autorouter, kept between runs,
  `PersistedSettings::stop_on_canvas_click`). On, the default, is Java's behaviour. Off, a click on
  the canvas during a job only shows a hint that Cancel or Esc stops it, and the status lines say
  "Press Esc or Cancel to stop it" instead of "Click anywhere on the canvas to cancel." Cancel, Esc
  and the stop command work either way. Tests: `with_the_preference_off_a_click_is_ignored_and_esc_still_stops`,
  `stop_on_canvas_click_round_trips`.
- A press of the left button that had Space held, or panned, is a pan until the button is up,
  even if it stayed below egui's drag threshold or Space was let go first; it never yields
  `CanvasClick`. (Java pans with the middle button and has no Space pan.) Test:
  `a_space_pan_never_stops_the_job`.

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

## kicad-plugin

The plugin is freerouting-rs's own (Java's KiCad plugin is separate code); these lines say where a
plugin run routes a different DSN than KiCad's export, or imports differently than
`ImportSpecctraSES`, under both profiles (K09, `kicad-plugin/freerouting_rs/dsn_fixups.py` and
`export_flow.py`; the options and their defaults are in `options.py`):
- **The board's edge clearance and pads' own clearances become keepouts** (`edge_and_pad_keepouts`,
  on by default under both profiles: they are board rules KiCad's export leaves out). Strips along
  the outline's bounding box and rectangles along the outline segments off it, `edge - default
  clearance` wide, and a circle round each pad whose local clearance exceeds the default. Pinned by
  `test_dsn_fixups.py::test_edge_and_pad_keepouts_on_a_two_layer_board` and
  `test_outline_segments_off_the_bounding_box_get_their_own_keepouts`.
- **Routed vias keep their session drill.** KiCad 10's `ImportSpecctraSES` drills a via with its net
  class's drill, not its padstack's (a 0.6 mm via on a 0.4 mm-drill class got a 0.1 mm annular
  ring on `canary-pwr`); the plugin sets the session's drill after the import
  (`ses_vias.py`, pinned by `test_imported_vias_get_the_sessions_drill_not_the_class_drill`).
- **The board's own copper comes back at its own coordinates** (Q22.3, K13.2,
  `restore_copper.py`, called by `export_flow.after_import`, `headless_run.py import` and the
  oracle's `kicad_steps.py import`). Java's plugin imports the SES as it is, so input tracks and
  vias come back where KiCad's `%.6g` DSN export (or the `.kicad_pcb` reader's 0.1 µm) and the
  session's 0.1 µm put them, up to 0.6 µm off. After the import each track or via that matches an
  unlocked original (same net and layer, width within 0.1 µm or the same via size and drill, every
  coordinate within 0.6 µm on the DSN path, 0.2 µm on the `kicad_pcb` path) gets the original's
  coordinates, width and uuid back, and routed ends within that bound of a restored end move onto
  it, unless that leaves a zero-length track. Anything moved further stays as imported. Under both
  profiles. Pinned by `test_restore_copper.py` (`test_an_input_track_that_the_export_rounds_comes_back_at_its_nanometre_coordinates`,
  `test_a_routed_track_that_ends_on_a_restored_track_follows_it`,
  `test_a_track_the_router_moved_2_um_stays_as_imported`) and
  `tools/kicad-oracle/tests/test_import_restore.py`.
- **Track arcs go to the router as polylines and come back as arcs** (K13.8, `track_arcs.py`,
  `export_flow.fix_track_arcs`, always on; `restore_copper.py`). KiCad 10.0.7's DSN export writes
  each track arc as one chord, and Java's plugin routes and imports that chord: on CM5_MINIMA_3
  the chords came back in place of the arcs, 172 clearance items inside differential pairs
  (K13.6). The plugin replaces the chord of each unlocked arc in the DSN's wiring with the
  polyline the `.kicad_pcb` reader lays (ends as exported, points on the arc, no chord over 1 µm
  from it or 0.1 rad long), so routed copper keeps its clearance from the arc to within 1 µm.
  After the import, on both inputs, an original unlocked arc whose polyline came back unmoved
  (the same net and layer, width within 0.1 µm, ends within the input's bound, every point and
  chord midpoint within that bound plus 1 µm of the arc) is put back as the arc, with its own
  start, mid, end, width and uuid; a chord or a polyline the router moved stays as imported.
  Locked arcs stay chords in the DSN and are kept by the import. Under both profiles. Pinned by
  `test_track_arcs.py`, `test_restore_copper.py::test_an_arc_whose_polyline_comes_back_unmoved_is_put_back_as_the_arc`,
  `test_a_chord_or_a_moved_polyline_stays_as_imported` and
  `test_arc_round_trip.py::test_two_arcs_at_the_class_clearance_come_back_as_arcs_with_no_clearance_item`
  (real pcbnew and kicad-cli).
- **Board Setup's minimums are passed as hard floors** (Q18, `options.board_minimum_args`,
  `export_flow.router_args`): `m_TrackMinWidth`, `m_ViasMinSize`, `m_MinThroughDrill`,
  `m_ViasMinAnnularWidth`, `m_CopperEdgeClearance` and `m_HoleClearance` become
  `--router.board_minimums.{track_width_um, via_diameter_um, via_drill_um, via_annular_um}`,
  `--router.copper_to_edge_clearance_um` and `--router.hole_clearance_um`, under both profiles;
  a value of 0 passes nothing. Pinned by `test_options.py::test_board_minimums_become_hard_floors` and
  `test_router_args_carry_the_board_minimums`.
- **Edge.Cuts and Margin shapes off the outline become keepouts** (Q18.5, `edge_graphics.py`,
  `export_flow.fix_edge_graphics`, always on): after KiCad's export, the same keepouts as the
  `.kicad_pcb` reader's (see freerouting-kicad), named `edge_clearance`, corners rounded to
  1 µm, before the structure's `(via ...)`; the DSN then differs from KiCad's export. Pinned by
  `test_edge_graphics.py` (`test_a_margin_line_becomes_a_keepout_on_every_copper_layer`,
  `test_edge_cuts_on_the_outline_add_nothing`,
  `test_a_malformed_outline_exports_the_bounding_box_so_every_line_inside_is_kept`).
- **Pads' solder-mask apertures go to the router** (Q18.7, `kicad_board.mask_expansions`,
  `export_flow.solder_mask_args`): every pad whose `GetSolderMaskExpansion` on an outer side
  with a mask layer exceeds the exported DSN's smallest class clearance becomes an entry of
  `--router.solder_mask_expansions` (pin names as KiCad's export writes them, `<n>@<k>` for a
  repeated number). Pinned by `test_solder_mask.py`.
- **The board's own solder-mask apertures become keepouts** (Q18.7, `edge_graphics.mask_polygons`,
  part of `export_flow.fix_edge_graphics`, always on): the reader's `solder_mask` keepouts (see
  freerouting-kicad) written into the exported DSN, corners rounded to 1 µm. Pinned by
  `test_edge_graphics.py` (`test_a_filled_front_mask_polygon_keeps_the_top_copper_out_of_it`,
  `test_a_back_mask_shape_goes_on_the_bottom_copper`,
  `test_kicad_board_collects_the_boards_mask_shapes_and_the_footprints`).
- **Mask apertures grow by the board's solder-mask-to-copper clearance** (Q22.12,
  `kicad_board.mask_to_copper_nm`, `kicad_board.mask_expansions`, `edge_graphics.mask_polygons`):
  the plugin reads `GetDesignSettings().m_SolderMaskToCopperClearance` (0 when this pcbnew has
  none) and grows both mask inputs as the `.kicad_pcb` reader does (see freerouting-kicad): each
  pad side with an aperture by `GetSolderMaskExpansion` + the clearance, before the DSN floor
  filter, and each board mask shape through a stroke twice the clearance wider
  (`edge_graphics` facts `mask_to_copper_mm`). With 0 the flags are Q18.7's, and the DSN's mask
  keepouts have Q22.18's 1 µm strips. Pinned
  by `test_solder_mask.py` (`test_the_mask_to_copper_clearance_grows_every_aperture_with_a_mask`,
  `test_a_negative_margin_takes_the_clearance_off_first`,
  `test_the_clearance_comes_from_the_design_settings`, `test_the_flow_passes_the_grown_apertures`)
  and `test_edge_graphics.py` (`test_the_mask_to_copper_clearance_grows_each_mask_shape`,
  `test_kicad_board_reports_the_mask_to_copper_clearance`).
- **A footprint's own solder-mask shapes become keepouts** (Q22.16, `footprint_masks`,
  `kicad_board.footprint_mask_facts`, `edge_graphics.mask_polygons`, part of
  `export_flow.fix_edge_graphics`): the reader's `solder_mask_pads` keepouts (see
  freerouting-kicad) written into the exported DSN, none for a footprint whose
  `AllowSolderMaskBridges()` is true; the router fits them to the pins under them (see
  freerouting). Pinned by `test_footprint_masks.py`, whose output is the DSN
  `crates/freerouting/tests/footprint_mask.rs` routes.
- **Pads under a footprint aperture that pads of several nets share are cut back to their part
  outside it** (Q22.16, `pad_trim.trim`, run by `edge_graphics.apply`): the reader's `pad_trim`
  (see freerouting-kicad) applied to KiCad's export, operation for operation (the same pads and
  corners on the shared fixture); the log names each pad that cannot be reached. Pinned by
  `test_footprint_masks.py` (`test_a_pad_partly_inside_a_shared_aperture_is_cut_back_to_its_part_outside`,
  `test_a_pad_wholly_inside_a_shared_aperture_is_named_in_the_log`,
  `test_a_shared_image_is_copied_for_the_component_whose_pads_are_cut`,
  `test_the_placement_frame_inverts_for_both_sides_and_flip_styles`).
- **Mask keepouts keep no clearance of their own** (Q22.18, `edge_graphics.MASK_CLEARANCE_CLASS`):
  the `solder_mask`, `solder_mask_pads` and `solder_mask_shared` keepouts carry
  `(clearance_class null)`, and every mask shape grows by `MASK_MARGIN_MM` (1 µm) on top of the
  clearance, as in the reader (see freerouting-kicad; the plugin's 1 µm rounding left canary-pwr's
  keepouts 342 nm inside KiCad's aperture at worst before, 0 after). Pinned by
  `test_footprint_masks.py::test_a_footprints_mask_shapes_become_keepouts_named_for_their_pads`,
  `test_edge_graphics.py::test_a_filled_front_mask_polygon_keeps_the_top_copper_out_of_it` and
  `test_edge_graphics.py::test_the_mask_keepouts_hold_the_exact_grown_apertures_after_rounding`.
- **Copper shapes and copper texts become obstacles** (Q22.4, `copper_graphics.py`,
  `kicad_board.copper_graphics`, `export_flow.fix_copper_graphics`, always on): after KiCad's
  export, the reader's `copper_graphic` keepouts and, for a net-tie footprint's shape, its
  `net_tie` keepouts (Q22.19, see freerouting-kicad), corners rounded to
  1 µm, and for a visible text keepouts of the outer outlines of KiCad's own glyph strokes
  (`TransformTextToPolySet`; the box for a knockout text or a text box) named `copper_text`. A
  shape of a net the DSN lists becomes conduction-area wires of that net with `(attr obstacle)`
  at the end of the wiring, the reader's polygons, as in the reader. Pinned by
  `test_copper_graphics.py` (`test_a_footprint_line_on_copper_becomes_a_keepout_on_its_layer`,
  `test_a_copper_text_becomes_keepouts_of_its_glyph_outlines`,
  `test_a_filled_shape_of_a_net_becomes_a_conduction_area_of_its_net`,
  `test_a_stroked_shape_of_a_net_becomes_the_area_of_its_stroke`,
  `test_the_fixture_through_pcbnew_gives_the_reader_s_obstacles`,
  `test_kicad_board_collects_copper_shapes_and_visible_texts`,
  `test_a_net_tie_shape_is_a_net_tie_keepout`,
  `test_the_net_tie_fixture_through_pcbnew_gives_the_routed_dsn`).
- **Room for each thermal spoke is kept out of other nets** (Q22.20, `thermal_spokes.py`,
  `kicad_board.thermal_facts`, `export_flow.fix_thermal_spokes`, always on): after KiCad's
  export, the reader's `thermal_spoke` keepouts (see freerouting-kicad), from pcbnew's own pad
  and zone settings (`GetThermalSpokeAngleDegrees`, `GetLocalZoneConnection`, the thermal
  overrides, `m_MinResolvedSpokes`), corners rounded to 1 µm, before the structure's `(via ...)`.
  Pinned by `test_thermal_spokes.py` (`test_the_fixture_gives_the_routed_dsn`,
  `test_the_fixture_through_pcbnew_gives_the_facts_and_kicads_spokes`: after KiCad's own fill,
  each keepout holds a spoke), whose output is the DSN `crates/freerouting/tests/thermal_spokes.rs`
  routes.
- **The project's `.kicad_dru` is passed for a DSN route** (Q22.5, `export_flow.kicad_dru_args`,
  always on, every profile): `--kicad-dru <board>.kicad_dru` when the file exists and the router
  reads KiCad's export; Java's plugin never applies the file. Not passed for the `.kicad_pcb`
  snapshot (which reads it itself) or the GUI. Pinned by `test_kicad_dru_args.py`.
- **A snapshot route applies the `.kicad_dru` too** (K13.1, `export_flow.kicad_dru_args`, always on,
  every profile): `--apply-design-rules` for every Route with `board_input = kicad_pcb`, so the
  board's own rules reach the router on both inputs (decided 2026-10-20). K10.3's option
  `apply_kicad_rules` no longer decides it, and schema 6 of the options file dropped it with its
  check box (K13.7; a file that has it still loads). Route in GUI applies the rules on neither
  input. Pinned by `test_board_rules_both_inputs.py`.
- **The plugin reads the open board directly by default** (K13.7, decided 2026-10-21;
  `options.PROFILE_DEFAULTS`): under the `enhanced` profile `board_input` is `kicad_pcb`, a
  `SaveBoard` snapshot read by the router's own reader, and the run falls back to KiCad's DSN export,
  saying so, when the reader refuses the board (K13.3). Java's plugin always exports a DSN, and the
  `legacy` profile keeps `dsn`. A saved file keeps an explicit choice; one without it takes the
  profile's input, whatever fix-ups it turns on (K14.5: the reader applies all four K09 fix-ups that
  rewrite the DSN text, so K13.7's rule that sent such a file, or a preset without an input, to `dsn`
  is gone, and nothing refuses a snapshot; `class_widths`, `fixed_pour_classes` in `wires` and
  `engine` mode, `drop_pour_nets` and `route_on_plane_layers` work on both inputs, and
  `edge_and_pad_keepouts` is done by the reader itself on the snapshot). Pinned by
  `test_board_input_default.py` (the defaults, the saved choice, the schema migration, the presets,
  the form and the note for each fix-up) and `test_input_choice.py` (the dialog, canary-pwr's saved
  shape on the snapshot, and the fallback).
- **Board Setup's minimum clearance is passed** (Q22.10, `options.BOARD_MINIMUM_SETTINGS`):
  `m_MinClearance` becomes `--router.board_minimums.clearance_um` under both profiles; 0 passes
  nothing. Pinned by `test_min_clearance.py`.
- **Board Setup's minimum hole-to-hole distance is passed** (Q22.11,
  `options.BOARD_MINIMUM_SETTINGS`): `m_HoleToHoleMin` becomes
  `--router.board_minimums.hole_to_hole_um` under both profiles; 0 passes nothing. Pinned by
  `test_hole_to_hole.py`.
- **A plane is cut where a higher-priority zone of another net fills** (K15.24,
  `plane_priority.py`, `kicad_board.plane_cuts`, always on under both profiles): KiCad's export
  writes each copper zone's whole outline as its `(plane ...)`, which Java's router and its KiCad
  plugin route as it is, but KiCad's filler leaves out every zone of another net higher in
  `ZONE::HigherPriority` on that layer, grown by the clearance. The plugin subtracts those zones'
  outlines (board and footprint zones, not their holes), grown by the larger of the two zones' own
  clearances (0.5 mm when unset) with round corners, from each plane with pcbnew's
  `SHAPE_POLY_SET`, and writes each changed plane as one `(plane ...)` per piece left, its holes as
  `(window ...)`; a plane with nothing left is dropped. The `.kicad_pcb` reader's rule (K15.15),
  except that a plane split in two becomes two planes, and the grown corners are KiCad's round
  ones, not the reader's mitred ones. A board without such an overlap exports byte-identically.
  Pinned by `test_plane_priority.py`
  (`test_the_lower_plane_leaves_out_the_higher_zones_area`,
  `test_without_an_overlap_of_a_higher_zone_the_dsn_is_byte_identical`,
  `test_kicads_export_of_two_overlapping_zones_through_pcbnew`).
  K15.34: where the board's `.kicad_dru` has a `clearance` rule the gap is the smaller of the
  zones' and the rules' smallest positive value (`export_flow.dru_clearance_nm`; KiCad 10.0.7's
  filler keeps a matching rule's value between two zones' fills, up or down, as between a zone and
  a track: two 0.5 mm zones, no rule 0.5055 mm, rule 0.127 mm 0.1325 mm, 0.4 mm 0.4055 mm, a rule
  matching nothing 0.5055 mm). A rule above the zones' clearance or of 0 does not widen the
  cut, and net classes are not read; an unreadable `.kicad_dru` cuts at the zones' value with a
  WARNING. A conditional rule counts for every pair of zones, so where its condition does not
  match (KiCad keeps the zones' value there) the DSN plane is up to the difference wider than
  KiCad's fill; on the pcu board the one rule is unconditional, so the cut is KiCad's. Pinned by
  `test_plane_clearance.py`.
- **The DSN path passes the signal-layer zones' thickness and clearance** (K15.27,
  `export_flow.pour_rule_args`, `kicad_board.signal_pour_rules`, always on under both profiles):
  KiCad's export drops each zone's `min_thickness` and clearance, and the router's cleanup (K15.26)
  needs them to judge a signal-layer pour by KiCad's fill. The plugin passes
  `--router.cleanup.pour_min_thickness_um` (the smallest `min_thickness`) and
  `--router.cleanup.pour_clearance_um` (K15.32: the smallest clearance KiCad's filler can apply: the
  smallest zone clearance, 0.5 mm for a zone without its own, KiCad's `ZONE_SETTINGS` default, as
  K15.24 takes it; every `clearance` constraint of the board's `.kicad_dru`, conditional ones
  included, because a custom rule replaces the zone's own value in KiCad 10.0.7's filler, whatever
  it says; an unreadable `.kicad_dru` passes none; net classes below a zone's clearance change
  nothing, so they are not read) over the board's copper zones on the
  exported DSN's signal layers (type `signal`, `jumper` or none, after the fix-ups; a `power` plane
  and a `mixed` layer, which the router does not load, are none), rule areas and footprint zones
  left out, in µm. No such zone: neither flag. A value of 0 passes no flag. The `.kicad_pcb` input
  passes neither, since its reader has each zone's values. One `plugin.log` line names the zone
  count, layers and both values. Arguments after `--` of `headless_run.py` come after and win; the
  dialog has no field for these flags. K15.33: also
  `--router.cleanup.pour_min_thickness_nets=<µm>:<net>,...`, each net whose zones' smallest
  `min_thickness` is above the board's smallest, sorted by name (a name the list cannot carry, with
  a comma or spaces around it, is left out and keeps the board's smallest); the log line counts
  them. Java's plugin passes nothing. Pinned by `test_pour_rules.py`, (K15.32)
  `test_pour_clearance.py` and (K15.33) `test_pour_net_thickness.py`.
- The other fix-ups (class widths, fixed pours, dropped pours, opened plane layers, add-only runs)
  are off unless chosen, and change nothing in a run with the defaults.
- **A default `enhanced` plugin run adds Q10's completion sweep** (`options.PROFILE_DEFAULTS`,
  K09.9): the plugin passes `--router.autorouter.sweep=free` (the router's 2 sweep passes) unless
  the dialog turns it off; the engine's own `enhanced` leaves the sweep off, and `legacy` passes
  nothing. Accepted by `docs/roadmap/prereg/K09.9-rerun.json` (N02's totals rule on canary-pwr
  and three KiCad demos through the plugin's headless path: KiCad unconnected 95 → 74 against a
  set noise of 17.14, no new clearance error). Pinned by
  `test_options.py::test_enhanced_adds_the_completion_sweep_and_legacy_does_not`.
- **Straight pin escapes are a checkbox, off under both profiles** (`options.pin_escape`, Q21,
  route-options schema 5): "Escape fine-pitch pins straight" on the Advanced tab passes
  `--router.fanout.pin_escape=straight`; off passes nothing. Pinned by
  `test_options.py::test_pin_escape_is_off_under_both_profiles_and_passes_its_flag_when_on` and
  `test_route_dialog.py::test_the_pin_escape_checkbox_is_saved_and_reaches_the_router`.

## Java quirk switches, opt-in (N06)

Fixes of kept Java bugs behind `router.java_quirks.<id>`: unset (`true`) is Java's
behaviour, `false` the fix, and `legacy` keeps every switch unset. N06.13 (2026-10-19)
promoted the 30 switches of the accepted batches into `enhanced`, and its second round
(2026-10-20) five more (`simplex_prev_division_line`, N06.9's two, N06.15's and N06.16's), and
its third round (2026-10-20) N06.14's two, one line each under "Profile `enhanced`"; the others
stay `true` in both profiles for good: their measurement rejected them, or they are (c) rows left
as Java
([N06](roadmap/tasks/archive/N06-fix-kept-java-bugs.md)). K15.2 added `placed_shape_convex` after
N06; it stays `true` in both profiles until its preregistered measurement. K15.19 added
`edge_pin_net_crosses_outline`, which `enhanced` sets `false` as a correctness rule. K15.50 added
`fanout_two_pass_cycle`; it stays `true` in both profiles until its preregistered measurement.

**N06.4 geometry cutout fixes (`simplex_prev_division_line` in profile `enhanced` since N06.13's second round)**

Unset is `true` (Java). N06.14's amendment 1 re-measured both at every `Area` split:
`simplex_prev_division_line` stayed inert (counter 0 in 40 runs), and N06.13's second round
(2026-10-20) sets it `false` in `enhanced`. `octagon_box_cutout_branch3` failed there (gate
FAIL: `below_minimum` 33 → 34 on canary-pwr's DSN, total 196 → 201) and, decided 2026-10-20,
stays `true` in both profiles with no separate switch for the newly wired splits. With the
named switch `false`:
- `octagon_box_cutout_branch3`: branch 3 of `IntOctagon::cutout_from_box` (the upper
  right divide line switched to horizontal, `IntOctagon.java:1206-1223`) trims the upper
  right octagon's top to the widened upper box, as branch 2 does on the left; Java's
  pieces overlap there. `box_cutout_branch3_switch_both_behaviours`, and
  `tests/cutout_invariant.rs`: inside the outer shape the pieces then tile it.
- `simplex_prev_division_line`: `Simplex::cutout_from` assigns its previous division
  line (`prevDivisionLine = nextDivisionLine`, the reverse of `Simplex.java:860`). That
  line is the corner's own first division line, so neither merge test passes and the
  pieces are Java's (`the_simplex_switch_changes_no_piece`). The switch is inert.

Every `Area` split of the board follows the switches (the N06 follow-up decided
2026-10-18): the drill pages' split (`autoroute/session/drills.rs`), the tie-pin trace
cutout (`BasicBoard::reduce_trace_shape_at_tie_pin`), the search trees' area and
outline-keepout shapes and the outline's piece count (`ShapeContext::split_quirks`,
`BoardOutline::keepout_split`), `check_shape`, `overlapping_items`, the spring-over shape,
the interactive hole, the plane adjustment after reading and the interactive route's
nearest target point (`board/split_switches.rs`, `tests/n06_area_splits.rs`). The reader
builds the board with its final switches; a later change reinserts the items whose pieces
change, with the same pieces but other leaf positions, so with a split switch `false` a
board switched after reading can route differently from one read with the switches. The CLI
and the GUI both read with the job's switches (the GUI's `LoadSettings`: the profile, the
saved settings and the front-end flags, decided 2026-10-20; `tests/ui_load_quirks.rs`), so
they agree; only a switch changed in the GUI after the board is open, or set by a rules
file, takes the switched-after-read path. Run statistics count `octagon_box_cutout_branch3_reached` and
`simplex_prev_division_line_reached` in both arms, at the drill pages and the tie-pin
cutout. N06.4 measured and accepted the batch at that reach; the wider reach is
re-measured under N06.14's amendment 1.

**N06.14 simplex cutouts outside N06.4's domain (profile `enhanced` since N06.13's third round)**

Unset is `true` (Java): `legacy` keeps both, `enhanced` sets both `false` (N06.13, 2026-10-20).
With the named switch `false`
(`Simplex::cutout_from`, `crates/freerouting-geometry/src/shapes/simplex.rs`):
- `cutout_hole_crosses_border`: the stand-in division line of a corner on the outer
  border (the hole edge before it) no longer acts as a division line: it neither clips
  later pieces as the first division line nor turns that clipping on
  (`Simplex.java:731-732`, `:756-768`, `:809-824`). Java's pieces go missing where a hole
  crosses the outer border (half the rest in the pinned case).
- `simplex_octagon_half_corner`: a corner that is not an integer point and lies on no
  outer border line (an odd-parity octagon's diagonal corner, an enlarged simplex's
  corner) is divided at its exact point, along the nearest lines through integer points
  on either side (`Line::nearest_through_integer_points`); Java leaves it undivided
  (`Simplex.java:1042-1057`) and its pieces overlap (755,786 units² in the pinned case).

`tests/cutout_invariant.rs`: `java_breaks_the_invariant_outside_the_two_rows` pins both
cases under all 16 switch values, and `all_fixed_cutouts_tile_the_rest_on_the_extended_set`
finds no violation in 16,200 cutouts with every switch `false` (Java: 1,817). The
switches reach a batch run wherever a drill page or the tie-pin cutout holds a simplex
tree shape: in any-angle mode (the plain search tree keeps every obstacle's simplex) and
on 45 degree boards whose wiring has a trace with a line off 45 degrees, whose tree shape
the 45 degree tree keeps as a simplex as Java's does (canary-pwr's 5 fixed segments;
`tests/n06_cutout_reach.rs`, `tests/n06_cutout_reach_45.rs`). The 90 degree tree keeps
boxes only. Run statistics count `cutout_hole_crosses_border_reached` and
`simplex_octagon_half_corner_reached` in both arms. Like N06.4's switches they act at
every `Area` split of the board since the follow-up of 2026-10-18; in the corpus the
stored areas' splits reach `cutout_hole_crosses_border` on Issue756-tomu-fpga8 (12) and
-fpga9 (8) only. `prereg/N06.14b.json`'s run of 2026-10-20 (amendment 1, both switches
together under `enhanced`) accepted them: `gate: PASS`, total 196 → 208 against noise 21.40, no
hard-guard or guard breach; `N06.14.json`'s run gave no verdict.

**N06.5 maze search and room fixes (profile `enhanced` since N06.13)**

Unset is `true` (Java): `legacy` keeps all five, `enhanced` sets them `false` (N06.13). With
the named `router.java_quirks` key `false`:
- `init_failure_room_leak`: a failed maze-search init clears the session before it
  returns, so its completed start rooms leave the tree and a later session has no
  leaked room to move aside (`init_failure_switch_both_behaviours_and_counter`).
- `failed_attempt_ripped_carryover`: an attempt rejected before its rip (a disabled
  layer, no located path) restores the ripped set and costs it started with; an
  attempt that ripped keeps them (`rejected_attempt_ripped_set_switch_both_behaviours`).
- `ortho_zero_width_room`: the 90 degree room right of an obstacle room reaches the
  board's right edge instead of having width 0
  (`right_of_obstacle_room_switch_both_behaviours`,
  `ortho_zero_width_room_reaches_a_90_degree_board`).
- `edge_corner_not_advanced`: the edge sweep of an obstacle room advances its corner, so
  exactly the sides of length 0 are skipped (`edge_room_sides_switch_both_behaviours`).
- `connect_to_trace_swapped_width`: the stubs to a target and a start trace take their
  own layer's half width (`connect_to_trace_width_switch_both_behaviours_and_counter`).

`run_stats` counts each branch in either arm (`maze_init_failure_room_leaks`,
`maze_ripped_carryovers`, `maze_ortho_zero_width_room_flips`,
`maze_edge_sweeps_without_neighbours`, `maze_edge_corner_flips`,
`insert_connect_to_trace_width_flips`); N06.5's registered run (`prereg/N06.5.json`) accepted
the batch under `legacy`.

**N06.6 fanout fixes (profile `enhanced` since N06.13)**

Unset is `true` (Java): `legacy` keeps all three, `enhanced` sets them `false` (N06.13). With
the named `router.java_quirks` key `false`:
- `fanout_stale_drill_carry`: each attempt of a pin's fanout (the plane attempt, the
  closest target, all targets) starts on fresh drill pages, as the batch router's
  connections do; Java's one `AutorouteEngine` (`RoutingBoard.java:1074-1126`) hands a
  retry the first attempt's occupied sections and the drills it cached for the net
  (`stale_drill_carry_switch_both_behaviours_and_counters`: the retry computes the 25
  page drills again, 50 against Java's 25).
- `drill_destination_room_index`: a destination drill's room is `roomArr[section]`, not
  `roomArr[firstLayer + section]` (`FoundConnectionLocator.java:255`); only a blind or
  buried via's drill differs, where Java takes the wrong layer's room or fails the
  locate past the end (`drill_destination_room_index_switch_both_behaviours_and_counter`).
- `fanout_pin_order_names`: `fanout.pin_sorting_order` also accepts the documented
  spellings `distance_to_closest_on_net` and `surroundings_density`; Java compares the
  camelCase names only (`BatchFanout.java:811-846`) and sorts by pin index otherwise
  (`fanout_pin_order_names_both_behaviours`,
  `pin_order_names_switch_reaches_the_fanout_run_and_counter`).

`run_stats` counts in either arm `fanout_retries`, `fanout_retries_succeeded`,
`fanout_stale_drill_carries`, `drill_destination_room_index_flips` and
`fanout_pin_order_name_flips`; N06.6's registered run (`prereg/N06.6.json`) accepted the
batch under `legacy`.

**N06.11 interactive fixes (profile `enhanced` since N06.13)**

Unset is `true` (Java): `legacy` keeps all six, `enhanced` sets them `false` (N06.13). Setting
the named
`router.java_quirks` key to `false` selects these corrections instead of the
kept Java behavior documented above:
- `snapshot_drops_compensation`: snapshots capture the compensation flag and
  restore the default tree with that flag, including memoized restores. Auxiliary
  trees remain transient. `snapshot_compensation_switch_including_memo_restores`.
- `rules_matrix_trees_not_rebuilt`: rules application rebuilds the trees against
  the updated matrix. `rules_matrix_refresh_switch_updates_compensated_shapes`.
- `move_drill_tidy_region`: drill insertion returns the union of translated pad
  bounds to the caller, enabling finite clipped pull-tight after the move.
  `move_drill_item_finite_tidy_skips_pull_tight` tests both arms.
- `route_change_layer_no_via`: a rule without a spanning via fails without
  switching the editor layer or adding a snapshot. `layer_change_without_spanning_via_quirk`.
- `route_connect_inner_break`: the first failed corner pair stops the connection;
  no disconnected second leg is drawn.
  `failed_first_corner_pair_switch_stops_disconnected_second_leg`.
- `select_extension_ignores_filter`: an empty filtered extension exits inspection,
  matching an explicit filter operation. Components still bypass the filter.
  `extension_filter_switch_exits_empty_inspection`.

`run_stats` counts every `compensated_snapshot_restores` request even in the Java
arm and on memo hits. N06.11's registered runs and the plugin canary grade accepted the
batch under `legacy`, and N06.13 promoted all six into `enhanced`.

**N06.10 input and report fixes (profile `enhanced` since N06.13)**

Unset is `true` (Java): `legacy` keeps all eight `router.java_quirks` switches, `enhanced` sets
them `false` (N06.13).
With the named switch `false`:
- `dsn_via_alias` copies the first class's via names; later classes cannot add
  vias to its rule (`via_alias_switch_keeps_first_class_vias_separate`).
- `dsn_class_class_pairs` applies every unordered pair of known classes
  (`class_class_switch_applies_all_three_pairs`).
- `dsn_shape_bbox` sorts rectangle bounds, uses half the circle diameter and
  applies path half-width once to each coordinate extent
  (`bounding_box_switch_both_behaviours`).
- `dsn_unit_ignored` honors the PCB coordinate unit, converts the resolution to
  preserve its physical grid, and converts placement positions from a declared
  placement unit to the PCB unit. Placement precision is not a coordinate multiplier;
  package-local geometry remains in PCB units
  (`pcb_unit_and_placement_resolution_switch_use_physical_units`).
- `ses_reader_scale` multiplies by the writer's divisor, making import the inverse
  of export even when overflow reduced the board scale
  (`issue676_round_trips_with_reader_scale_fixed`).
- `stats_line_ab_length` sums actual trace segments instead of defining points of
  lines (`trace_length_statistics_both_behaviours`).
- `route_result_int_via_ratio` divides via counts as real numbers
  (`route_result_via_ratio_both_behaviours`). The optimizer's actual equality-to-zero
  reader counts differences between both formulas while increased ripup costs are
  active; rejected candidates do not inflate this count.
- `drc_units_10000_per_mm` uses the board's physical scale for summary messages,
  congestion zones and hints (`drc_units_switch_uses_the_actual_board_scale`).

The DSN counters observe both switch arms: an aliased first via list with a later
nonempty list, a class-pair scope with at least three names, a shape box whose
formulas differ, and a unit or placement-resolution mismatch. The pair and unit
counters are conservative: unknown class names and precision-only placement
changes can count even when geometry does not change. `dsn_corpus_branch_inventory`
produces a per-input JSON report without routing. N06.10's registered sessions, the
optimizer reference run and the plugin canary grade accepted the batch under `legacy`, and
N06.13 promoted all eight into `enhanced`.

**N06.12 judgement calls (three in profile `enhanced` since N06.13)**

Unset is `true` (Java). N06.13 promoted three into `enhanced` (`any_angle_skip_length_squared`,
`rules_unknown_layer_scope`, `in_front_of_pad_side0_flank`); the others stay `true` in both
profiles. With the named switch `false`:
- `ripup_pass_no_unset`: the batch pass number reaches `AutorouteControl::ripup_pass_no`,
  so `MazeRipupResolver`'s random detour factor `0.5 + r * r` (`:158-162`), which Java's
  batch router never draws, fires from pass 4 on outside every third pass; the generator is
  the search's, reseeded with the ripup costs, so the run is deterministic
  (`tests/n06_judgement.rs::ripup_pass_no_switch_both_behaviours_and_counters`). Registered
  in `prereg/N06.12-ripup-pass.json`; the run of 2026-10-19 rejected it (total 47 → 47, one
  new trace under ReadPlaceScope's narrowest class width), so it stays `true` in both
  profiles.
- `any_angle_skip_length_squared`: the any-angle end smoothing skips an end segment of a
  rational end corner shorter than 10 units (`SKIP_LENGTH` squared), not 3.2
  (`any_angle::tests::skip_length_switch_both_behaviours_and_counter`); a reported arm of the
  same registration.
- `rules_unknown_layer_scope`: a rules-file `layer` scope naming a layer the board does not
  have is ignored with a second warning instead of applying to every layer
  (`tests/rules_reader.rs::unknown_layer_scope_switch_both_behaviours`). Decided 2026-10-18
  for `enhanced`; in its table since N06.13.
- `in_front_of_pad_side0_flank` is N06.7's reported arm `all_flank`. `opt_threshold_fraction`
  keeps Java's heuristic (its `false` half: `threshold_fraction_switch_both_behaviours`).
- Left as Java in both profiles, with no switch (N07.4 removed the six switches no code read;
  `false` changed nothing): `insert_stale_trace_object`, `rules_autoroute_settings_reenable`,
  `direction_compare_zero`, `route_stubs_layer_test`, `polyline_array_reversal`,
  `avoid_acid_traps_disabled` (the reasons are in N06's (c) table). A settings file that names
  one is now an unknown key (`tests/settings_java_quirks.rs::removed_inert_switches_are_unknown_keys`).

`run_stats` counts `ripup_random_pass_connections` and `any_angle_skip_length_flips` in
either arm, `ripup_random_factor_draws` (only under `false`) and `rules_unknown_layer_scopes`.

**N06.15 shove fix (profile `enhanced` since N06.13's second round)**

Unset is `true` (Java): `legacy` keeps it, `enhanced` sets it `false` (N06.13, 2026-10-20). With
it `false`, a
shove substitute piece's shapes are queried again before the piece goes on the board, and the
shove fails at a via of another net found there (the shove-failing obstacle is the via), as the
insert fails at a via in its way; callers undo as for any failed shove
(`shove::insert::pending_tests::pending_piece_switch_both_behaviours_and_counters`). Where no
via arrives the shove is Java's (`shove_insert_with_the_pending_piece_fix_matches_java_where_no_via_arrives`).
A trace of another net inserted onto a pending piece later in the same recursion is not
covered. `run_stats` counts `shove_via_into_pending_pieces` and
`shove_pending_piece_foreign_traces` in either arm, with queries that leave the work meter
alone. Registered in `prereg/N06.15.json` under `legacy` and `enhanced`; its run of 2026-10-20
passed both pairs (under `enhanced` 312 → 319, noise 69.24).

**N06.16 any-angle room sides (profile `enhanced` since N06.13's second round)**

`any_angle_room_side_index` is `true` (Java) unset and under `legacy`, and `false` under
`enhanced` (N06.13, 2026-10-20). Java's path is unbounded: an any-angle room neighbour's
touching side is taken on the room's own shape (an
`IntOctagon` has 8 border lines, degenerate ones too), while `tryRemoveEdge` and
`calculateNewIncompleteRooms` index its `toSimplex()`; a side past the simplex's last line never
ends the gap walk, which adds incomplete rooms until memory runs out. Java at the pin and the
port both run out of memory on A07's `Issue026-J2_reference-any` (fanout pass 1) and
`Issue143-rpi_splitter-any` (optimizer pass 3). With it `false`, the sides, the corners and the
neighbour sort are taken on the simplex, so a gap walk ends within `n + 1` steps
(`neighbours_any::tests::room_side_index_switch_both_behaviours`,
`tests/n06_any_angle_rooms.rs`). Only the plain (any-angle) tree reaches it; `run_stats`
counts `any_angle_room_side_mismatches` in either arm. `prereg/N06.16.json`'s run of
2026-10-20 accepted it under `enhanced`: both boards abort at the 8000000 kB cap under `true`
and route under `false`, and the other 6 rows are identical.

**K15.5 same-net vias in pads (in neither profile)**

`same_net_via_pad_violation` is `true` (Java) unset and in both profiles. Every padstack the
KiCad reader writes is `(attach off)`, so Java's `Item.isObstacle` counts each via in or beside
a pad of its own net as a clearance violation, and the router's score and pre-existing count
carry them (121 of the committed pcu board's 165 hole clearance items, all input vias). With it
`false`, `DesignRulesChecker::all_clearance_violations` leaves out a pair whose items share a net
unless one is copper the router placed (a trace or via neither user nor system fixed), so a via
the run puts in a pad still counts; the search's obstacle test is unchanged (D8)
(`tests/drc_kicad_report.rs::score_switch_leaves_out_only_the_inputs_same_net_pairs`). No
measurement has run, so `enhanced` keeps Java's count.

**K15.2 placed shapes convex (in neither profile)**

`placed_shape_convex` is `true` (Java) unset, under `legacy` and under `enhanced`; it joins
`ENHANCED` only after a preregistered measurement. Java's `PlacementTransform.shape`
(`PlacementTransform.java:54-68`) rounds a placed tile shape's corners and hands them to
`TileShape.getInstance` as a convex polygon. Where short edges (an enlarged simplex's chamfers,
a unit or two long) round to reflex vertices, the edges' half-planes cut corners off a pad or
leave a shape unbounded (C65's F.Fab circle in bug 1 of
`docs/development/pcu-rev2-board-report.md`, whose box reaches `i32::MAX`; K15.1 leaves such an
item out of the board box). With it `false`, `PlacementTransform::shape_with` and `area_with`
take the convex hull of the rounded corners first, for every placed pin shape, component outline,
component area and auxiliary pad shape; where the rounded corners are convex both values give the
same shape (`freerouting-geometry/tests/placed_shape_convex.rs`: C65's outline is bounded, with
the box of its rounded corners; a 24-gon pad enlarged by 75 units at a position that rounds to
reflex vertices keeps the 8 corners Java cuts off). On bug 1's repro the switch `false` leaves no
unbounded item and no K15.1 warning, and the boards route 2 of 2 and 6 of 6 in both profiles
(`tests/k15_placed_shape_convex.rs`, `freerouting-cli/tests/cli_placed_shape_convex.rs`). A
switch changed after reading places the component items again and reinserts those whose shapes
change (`board/placed_shapes.rs`), so the board has the shapes of one read with it. Not a Java
change: `bounded-neck`'s courtyard (an `enhanced` stage) leaves an unbounded outline out of a
component's courtyard box, whatever the switch (`bounded_neck::tests::the_courtyard_fallback_leaves_an_unbounded_outline_out`).

**K15.19 edge-pin nets cross the outline (`false` in profile `enhanced`)**

`edge_pin_net_crosses_outline` is `true` (Java) unset and under `legacy`. Java at the pin
(`BoardOutline.blocksNets`, `isTraceObstacle`, from `e79ed32ea`) makes the outline no obstacle
for a trace whose nets all have a pin with its centre or a pad corner outside the outline, so such
a net's traces may cross the outline anywhere, also far from the pin (on wisweep's driver board J1
and JP3 lie across the notch line x = 64.5: K15.14's runs laid /TMC_AIN and /TMC_REFL tracks
across the edge, 1 and 5 KiCad `copper_edge_clearance`). With it `false` the outline blocks
every trace (the maze search, the shove, the clearance check and Q20's output check); a pin whose
centre lies outside the outline cannot be reached (a trace ends at a pin's centre) and its
connection stays open. `tests/k15_wisweep_edge_hole.rs::no_routed_trace_crosses_the_outline_beside_a_pin_on_the_edge`:
`legacy` crosses with /TMC_AIN and GND and routes all; `enhanced` crosses nowhere and leaves J1-1
and J1-2 open.

**K15.50 the fanout's two-pass cycle (in neither profile)**

`fanout_two_pass_cycle` is `true` (Java) unset, under `legacy` and under `enhanced`; it joins
`ENHANCED` only after a preregistered measurement. Java's fanout pass loop
(`BatchFanout.fanoutBoard`, `BatchFanout.java:113-164`) stops on a pass outcome
`((long) routedCount << 32) ^ board.getVias().size()` equal to the previous pass's three times
running, or on an unchanged board hash, so both compare a pass with the one before only. When two
groups of pins rip each other's escapes the passes alternate between two outcomes and the loop
runs to `fanout.max_passes` (20), each pass with a higher rip-up cost and per-pin budget. With it
`false`, `FanoutPassTracker::after_pass` (`autoroute/batch/fanout_rules.rs`) also stops after a
pass whose outcome equals the outcome two passes before while the pass in between had another
(`FanoutStop::TwoPassCycle`). The state is the pass outcome, not the board hash, which covers the
item ids and the undo stack and so never repeats over a pass that changes the board
(`tests/k15_fanout_two_pass_cycle.rs`: canary-pwr `pcb-on`'s 20 measured pass outcomes end after
pass 11, the first A B A, with it `false` and run to the 20-pass cap unset; Java's own stops are
unchanged). Measured
2026-10-23 on K15.48's plugin inputs with the switch `false`: canary-pwr `pcb-on` stops after
pass 11 of 20 (route 111.2 s -> 78.4 s, KiCad unconnected 24 -> 28), `pcb-off` after pass 17
(82.0 s -> 72.2 s, 40 -> 40), loratracker after pass 10 in both arms (44 -> 44); pcu, kit-dev
(whose passes cycle with period 3, which the rule does not see), wisweep and pcie route as
before.

## Profile `enhanced`

`router.profile` (N01, D10) picks the defaults a job starts from: `legacy` is Java Freerouting
at the pin (every section above still holds for it, and every parity test and gate pins it),
`enhanced` is the default and adds the improvements a measurement accepted. A profile only
changes defaults: any key set in a settings file, the design, the environment or on the command
line wins, so a settings file that sets every key routes the same under both. The table is
`ENHANCED` in `crates/freerouting/src/settings/profile.rs`; each entry below names its settings
key, its `enhanced` value and the measurement that accepted it (N05 adds them).
`tests/settings_profile.rs::every_enhanced_difference_is_a_setting_with_its_divergence_line`
checks that this list and the table agree.

- `power_layer_traces` = `none` (N05.3, `docs/roadmap/prereg/N05-power-layer-traces.json`,
  amended once): no trace of any net on a power layer; vias still land in their net's plane
  (Autorouter, `power_layer_traces`). canary's validity check refuses any track on an inner
  plane layer, and the owner's plugin run of 2026-10-17 had a `+12V_BUS` track on Power.Cu. Judged by
  N02's totals rule (`accept: no-regress`, `freerouting-dev gate`: PASS) against `own_plane`,
  Q13's rule: KiCad unconnected 256 → 256 over canary-pwr (6 rows), m-fc-g474 and microzed,
  every session byte-identical to `own_plane`'s; against Java's `layer` 257 → 256, traces on
  power layers 6 → 0, inner-layer tracks on the canary plane rows 4 → 0.
- `neck_width_um` = `300` (N05.3, `docs/roadmap/prereg/N05-neck-down.json`): Java's necked
  retry (`retryConnectionNecked`, Autorouter: a connection that fails at its class width is tried
  once more with every trace clamped to 300 µm, never below `min_trace_width_um`) is on, where
  Java's default 0 leaves it off. It lets a board keep its Board Setup class widths: copper is
  full width wherever the first attempt fits, and only failed connections ship narrow. Judged by
  N02's totals rule (`accept: no-regress`, `freerouting-dev gate`: PASS) against the same
  version routing class widths narrowed the way the plugin flow narrows them (canary's 0.3/0.4 mm
  export, the plugin's `class_widths auto`): KiCad unconnected 103 → 106 over canary-pwr
  (2 rows), interf_u, pic_programmer and CM5_MINIMA_3 (set noise 17.14), no new DRC error,
  via in a pad or power-layer trace; copper under the Board Setup width on the rows where the
  narrowing acted 848.9 → 310.4 mm. The retry acts on every class wider than 300 µm, not only
  the wide ones.
- `via_clearance` = `net_class` (N05.3, `docs/roadmap/prereg/N05-via-clearance-netclass.json`):
  a via the autorouter checks or inserts for a net takes its net class's via clearance class
  (Q14, Autorouter `via_clearance`), as KiCad clears a via, where Java takes the via info's;
  the fanout's fallback via of the board's first via rule no longer lands at the default
  class's clearance on a wider class's net. Judged by N02's totals rule (`accept: no-regress`,
  `freerouting-dev gate`: PASS) against `enhanced` without it, its clearance clause counting
  only KiCad's net-class-rule items (board-minimum clearance, which the DSN does not carry, is
  not judged): KiCad unconnected 263 → 261 over canary-pwr's plugin path, pic_programmer,
  tinytapeout-demo, RoyalBlue54L-Feather, slice-dcmt-bread-slice and CM5_MINIMA_3 (set noise
  42.70; canary-pwr 36 → 41), net-class via clearance errors 2 → 1 (canary-pwr's `+12V_PROT`
  via 1 → 0), no via in a pad, copper below the minimum or power-layer trace.
- `neck_down.widen_back` = `true` (N05.3 from Q17.4, `docs/roadmap/prereg/Q17.4.json`, sha256
  `b3d4ad49…`, no amendment): after the necked retry at 300 µm (`neck_width_um` above) inserts a
  connection, its new traces under the class width are widened back to the class width in 0.5 mm
  pieces wherever the exact clearance check passes, with 4-step tapers (Q17.3,
  `quality::neckdown::widen_back`); Java ships the retry's traces narrow over their whole length.
  Judged by N02's totals rule (`accept: no-regress`, `freerouting-dev gate`: PASS) against
  `enhanced` without it: KiCad unconnected 164 → 168 over canary-pwr (plugin path and the
  9511d9a Board Setup widths DSN), interf_u, pic_programmer, flight-computer-core-board and
  cm4-edge-ai-carrier (set noise 15.42; canary-pwr +5 within its band of 10), narrowed copper on
  the wide classes (`freerouting-dev widths`, under the class width) 461.8 → 400.6 mm, no new via
  in a pad, copper below the minimum, KiCad clearance-type error or power-layer trace.

- `output_check` = `repair` (decided 2026-10-20, Q20: routed copper that breaks a board rule is
  never saved; a correctness rule, joined without a keep rule; its cost measured in Q20.6,
  `docs/roadmap/prereg/Q20.json`): see `output_check` under "freerouting (board model)".
  Java writes the session as the router leaves it (`tests/quality_output_check.rs`,
  `the_profiles_set_repair_and_off`).

- `board_minimums.from_classes` = `true` (decided 2026-10-20, Q18.2; its cost measured in Q18.6,
  `docs/roadmap/prereg/Q18.json`): with no board minimum supplied (a bare DSN carries none), the
  narrowest class width of the design is the minimum track width, so no neck-down ships copper
  under it (`tests/board_minimums.rs`, `from_classes_floors_at_the_narrowest_class_width`); a
  known minimum (`board_minimums.track_width_um`, the plugin's or a `.kicad_pro`'s) replaces it.
  Java necks down to the pin's width, floored only by `min_trace_width_um`.
- `cleanup.remove_stubs` = `true` (N05.3 from Q16.5, `docs/roadmap/prereg/Q16-5.json`, amendment 2,
  sha256 `a17c579f…`; decided 2026-10-20): the final cleanup (above, `quality::stubs`) removes the
  routed vias and traces that join fewer than two items of their net, as KiCad counts contacts;
  Java leaves them. Judged by N02's totals rule (`accept: no-regress`) over canary-pwr's plugin
  path, sonde-xilinx and complex_hierarchy: KiCad unconnected 108 → 108 (set noise 9.11),
  canary-pwr's dangling vias 8 → 0 with unconnected 26 → 26, no new DRC item, and the cleanup
  run and its rerun byte-identical on all three. The gate printed FAIL on one hard guard,
  sonde-xilinx `clearance` = 1, which is in both arms (their sessions are byte-identical) and
  comes from copper text the DSN does not carry (Q22): decided 2026-10-20 that it is not the
  candidate's effect. Thin evidence: only canary-pwr has a stub to remove. The known limit
  (a trace ending inside a plane's outline outside KiCad's fill is kept as a joint) stands.

The `java_quirks` lines below are N06's Java-bug fixes that N06.13 promoted on 2026-10-19, each
under N06's accept rule: measured and neutral, or not reached by any registered row and accepted
on its tests (both behaviours pinned). They were measured under `legacy`; their confirmation
under `enhanced` (`enhanced` at the parent commit against `enhanced` with them,
`docs/roadmap/prereg/N06.13-promotion.json`) passed on 2026-10-20 (total 210 → 208). The last
five lines are N06.13's second round (2026-10-20): N06.9, N06.15 and N06.16 measured under
`enhanced` with the first round's 30, and `simplex_prev_division_line` is accepted on its tests;
the round's confirmation (`docs/roadmap/prereg/N06.13-promotion-2.json`) passed on the quick rows
on 2026-10-20 (unrouted 34 → 34). The last two lines are its third round (2026-10-20): N06.14b
measured them under `enhanced` with the 35 earlier fixes, and that run's candidate arm is
`enhanced` with them, so it is the round's confirmation. An explicit `--router.java_quirks.<id>=true` brings Java's bug back.

- `java_quirks.init_failure_room_leak` = `false` (N06.13 from N06.5,
  `docs/roadmap/prereg/N06.5.json`, batch M: gate PASS, total 38 → 38, set noise 10.95): a failed
  maze-search init clears its session, so its completed start rooms do not stay in the autoroute
  tree as obstacles (Connection insertion). Not reached: `maze_init_failure_room_leaks` 0 in every
  run; accepted on `init_failure_switch_both_behaviours_and_counter`.
- `java_quirks.failed_attempt_ripped_carryover` = `false` (N06.13 from N06.5,
  `docs/roadmap/prereg/N06.5.json`, batch M: gate PASS, total 38 → 38, set noise 10.95): an attempt
  rejected before its rip restores the ripped set and the ripup costs it started with. Not reached:
  `maze_ripped_carryovers` 0; accepted on `rejected_attempt_ripped_set_switch_both_behaviours`.
- `java_quirks.ortho_zero_width_room` = `false` (N06.13 from N06.5,
  `docs/roadmap/prereg/N06.5.json`, batch M: gate PASS, total 38 → 38, set noise 10.95): the 90
  degree room right of an obstacle room reaches the board's right edge instead of having width 0.
  Reached on both 90 degree rows (254 and 61 rooms) with every session identical: neutral
  (`right_of_obstacle_room_switch_both_behaviours`).
- `java_quirks.edge_corner_not_advanced` = `false` (N06.13 from N06.5,
  `docs/roadmap/prereg/N06.5.json`, batch M: gate PASS, total 38 → 38, set noise 10.95): the 45
  degree edge sweep of an obstacle room advances its corner, so only sides of length 0 are skipped.
  Reached (373 sweeps on J2, 22 on bm08, 12121 on the plugin path) with every session identical:
  neutral (`edge_room_sides_switch_both_behaviours`).
- `java_quirks.connect_to_trace_swapped_width` = `false` (N06.13 from N06.5,
  `docs/roadmap/prereg/N06.5.json`, batch M: gate PASS, total 38 → 38, set noise 10.95): the stubs
  to a start and a target trace take their own layer's half width. Not reached (no row has per-layer
  widths); accepted on `connect_to_trace_width_switch_both_behaviours_and_counter`.
- `java_quirks.fanout_stale_drill_carry` = `false` (N06.13 from N06.6,
  `docs/roadmap/prereg/N06.6.json`, batch F with the fanout on: gate PASS, total 100 → 100, set
  noise 11.14): each attempt of a pin's fanout starts on fresh drill pages. Reached (307 retries on
  z10_module and the plugin path, 0 succeeding in either arm) with every session identical: neutral
  (`stale_drill_carry_switch_both_behaviours_and_counters`).
- `java_quirks.drill_destination_room_index` = `false` (N06.13 from N06.6,
  `docs/roadmap/prereg/N06.6.json`, batch F with the fanout on: gate PASS, total 100 → 100, set
  noise 11.14): a destination drill's room is indexed by section, not by `firstLayer + section`. Not
  reached (no blind or buried via); accepted on
  `drill_destination_room_index_switch_both_behaviours_and_counter`.
- `java_quirks.fanout_pin_order_names` = `false` (N06.13 from N06.6,
  `docs/roadmap/prereg/N06.6.json`, batch F with the fanout on: gate PASS, total 100 → 100, set
  noise 11.14): `fanout.pin_sorting_order` also accepts `distance_to_closest_on_net` and
  `surroundings_density`. Not reached (no arm sets the order); accepted on
  `pin_order_names_switch_reaches_the_fanout_run_and_counter`.
- `java_quirks.in_front_of_pad_typo` = `false` (N06.13 from N06.7, `docs/roadmap/prereg/N06.7.json`,
  batch S: every flip counter 0 and every session identical on 5 DSN rows and canary-pwr's plugin
  path): side 0 of the in-front-of-pad test takes `b.x + b.y` (Shove checks); accepted on
  `in_front_of_pad_keeps_java_typo`.
- `java_quirks.store_trace_class_self_compare` = `false` (N06.13 from N06.7,
  `docs/roadmap/prereg/N06.7.json`, batch S: every flip counter 0 and every session identical on 5
  DSN rows and canary-pwr's plugin path): a contact trace's clearance class is compared with the
  shoved trace's (Shove checks); accepted on `store_trace_class_switch_both_behaviours`.
- `java_quirks.rational_perp_projection` = `false` (N06.13 from N06.7,
  `docs/roadmap/prereg/N06.7.json`, batch S: every flip counter 0 and every session identical on 5
  DSN rows and canary-pwr's plugin path): the shove's end-corner projection of a rational corner is
  the exact foot (geometry `perpendicular_projection_with`); accepted on
  `rational_end_corner_projection_switch_both_behaviours`.
- `java_quirks.in_front_of_pad_side0_flank` = `false` (N06.13 from N06.7,
  `docs/roadmap/prereg/N06.7.json`, batch S, reported arm `all_flank` against `all`: every flip
  counter 0 and every session identical on 5 DSN rows and canary-pwr's plugin path): side 0's second
  flank clause takes `max(x + y)`, as sides 2, 4 and 6 take the extreme past their diagonal (a (c)
  row, N06.12); accepted on `side0_flank_switch_both_behaviours`.
- `java_quirks.smooth_loses_trace` = `false` (N06.13 from N06.8, `docs/roadmap/prereg/N06.8.json`,
  batch P: gate PASS, total 38 → 38, both counters 0, every session identical): the end-corner
  smoothing tests the insert's failure conditions first and keeps the trace (Pull-tight); accepted
  on `smooth_loses_trace_switch_both_behaviours`.
- `java_quirks.pull_tight_reused_lines_change` = `false` (N06.13 from N06.8,
  `docs/roadmap/prereg/N06.8.json`, batch P: gate PASS, total 38 → 38, both counters 0, every
  session identical): pull-tight reports a change only when `change_trace` changed the trace;
  accepted on `pull_tight_reused_lines_switch_both_behaviours`.
- `java_quirks.dsn_via_alias` = `false` (N06.13 from N06.10, `docs/roadmap/prereg/N06.10.json`,
  batch I: 40 runs, every session identical): with no structure `via`, later classes no longer add
  vias to the first class's rule (Reading a DSN file). No corpus board reaches it
  (`dsn_corpus_branch_inventory`, 203 boards); accepted on
  `via_alias_switch_keeps_first_class_vias_separate`.
- `java_quirks.dsn_class_class_pairs` = `false` (N06.13 from N06.10,
  `docs/roadmap/prereg/N06.10.json`, batch I: 40 runs, every session identical): a `class_class`
  rule applies to every pair of its classes. No corpus board reaches it; accepted on
  `class_class_switch_applies_all_three_pairs`.
- `java_quirks.dsn_shape_bbox` = `false` (N06.13 from N06.10, `docs/roadmap/prereg/N06.10.json`,
  batch I: 40 runs, every session identical): DSN bounding boxes sort rectangle bounds, take half a
  circle's diameter and add a path's half width once. No corpus board reaches it; accepted on
  `bounding_box_switch_both_behaviours`.
- `java_quirks.dsn_unit_ignored` = `false` (N06.13 from N06.10, `docs/roadmap/prereg/N06.10.json`,
  batch I: 40 runs, every session identical): the pcb-level `unit` and the placement `resolution`
  are honoured. No corpus board has a differing unit; accepted on
  `pcb_unit_and_placement_resolution_switch_use_physical_units`.
- `java_quirks.ses_reader_scale` = `false` (N06.13 from N06.10, `docs/roadmap/prereg/N06.10.json`,
  batch I: 40 runs, every session identical): the SES reader multiplies by the writer's divisor, so
  an import is the inverse of the export (Issue676: Java's import is 100 times too large). Every SES
  import; no routed session reads one; accepted on `issue676_round_trips_with_reader_scale_fixed`.
- `java_quirks.stats_line_ab_length` = `false` (N06.13 from N06.10,
  `docs/roadmap/prereg/N06.10.json`, batch I: 40 runs, every session identical): board statistics
  sum the trace segments, not the distances between each line's defining points: the trace lengths
  of the result manifest and `--stats` change, routing does not; accepted on
  `trace_length_statistics_both_behaviours`.
- `java_quirks.route_result_int_via_ratio` = `false` (N06.13 from N06.10,
  `docs/roadmap/prereg/N06.10.json`, batch I: 40 runs, every session identical): a route result's
  via ratio is a real division (`improvement_percentage`). `route_result_zero_test_changes` 0 in
  every run, so no optimizer decision changed; accepted on `route_result_via_ratio_both_behaviours`.
- `java_quirks.drc_units_10000_per_mm` = `false` (N06.13 from N06.10,
  `docs/roadmap/prereg/N06.10.json`, batch I: 40 runs, every session identical): DRC summary
  messages, congestion zones and hints use the board's own scale, not 10000 units per mm (reports
  only); accepted on `drc_units_switch_uses_the_actual_board_scale`.
- `java_quirks.snapshot_drops_compensation` = `false` (N06.13 from N06.11,
  `docs/roadmap/prereg/N06.11.json`, batch U: 32 runs, every session identical,
  `compensated_snapshot_restores` 0): a snapshot restore or deep copy keeps clearance compensation
  (Board model); compensation is on only in the GUI; accepted on
  `snapshot_compensation_switch_including_memo_restores`.
- `java_quirks.rules_matrix_trees_not_rebuilt` = `false` (N06.13 from N06.11,
  `docs/roadmap/prereg/N06.11.json`, batch U: 32 runs, every session identical,
  `compensated_snapshot_restores` 0): a rules file's clearance matrix rebuilds the search trees
  (Rules files); accepted on `rules_matrix_refresh_switch_updates_compensated_shapes`.
- `java_quirks.move_drill_tidy_region` = `false` (N06.13 from N06.11,
  `docs/roadmap/prereg/N06.11.json`, batch U: 32 runs, every session identical,
  `compensated_snapshot_restores` 0): a moved drill item pull-tightens within a finite tidy width
  (Shove insert); accepted on `move_drill_item_finite_tidy_skips_pull_tight`.
- `java_quirks.route_change_layer_no_via` = `false` (N06.13 from N06.11,
  `docs/roadmap/prereg/N06.11.json`, batch U: 32 runs, every session identical,
  `compensated_snapshot_restores` 0): an interactive layer change without a via spanning both layers
  fails instead of switching the layer (Interactive routing); accepted on
  `layer_change_without_spanning_via_quirk`.
- `java_quirks.route_connect_inner_break` = `false` (N06.13 from N06.11,
  `docs/roadmap/prereg/N06.11.json`, batch U: 32 runs, every session identical,
  `compensated_snapshot_restores` 0): an interactive connect stops at the first failed corner pair;
  accepted on `failed_first_corner_pair_switch_stops_disconnected_second_leg`.
- `java_quirks.select_extension_ignores_filter` = `false` (N06.13 from N06.11,
  `docs/roadmap/prereg/N06.11.json`, batch U: 32 runs, every session identical,
  `compensated_snapshot_restores` 0): an extension whose filter empties the selection leaves the
  inspect state; accepted on `extension_filter_switch_exits_empty_inspection`.
- `java_quirks.rules_unknown_layer_scope` = `false` (N06.13 from N06.12, decided 2026-10-18): a
  rules-file `layer` scope naming a layer the board does not have is ignored with a second warning,
  where Java applies it to every layer (Rules files; `-dr` only); accepted on
  `unknown_layer_scope_switch_both_behaviours`.
- `java_quirks.any_angle_skip_length_squared` = `false` (N06.13 from N06.12,
  `docs/roadmap/prereg/N06.12-ripup-pass.json`, reported arm: `any_angle_skip_length_flips` 0 and
  every session identical on 6 DSN rows and canary-pwr's plugin path): the any-angle end smoothing
  compares a squared end segment with `SKIP_LENGTH` squared (10 units, not 3.2; any-angle mode
  only); accepted on `skip_length_switch_both_behaviours_and_counter`.
- `java_quirks.simplex_prev_division_line` = `false` (N06.13 from N06.4,
  `docs/roadmap/prereg/N06.4.json`, batch G, and N06.14's run at every `Area` split,
  `docs/roadmap/prereg/N06.14.json`: `simplex_prev_division_line_reached` 0 in 40 runs and in the
  corpus inventory): `Simplex::cutout_from` assigns its previous division line (geometry
  cutouts). Neither merge test passes with it, so the pieces are Java's; accepted on
  `the_simplex_switch_changes_no_piece`.
- `java_quirks.opt_first_pass_stops` = `false` (N06.13 from N06.9, `docs/roadmap/prereg/N06.9.json`,
  batch O under `enhanced`: gate PASS, every row's four arms one session, total 76 → 76, set noise
  19.95, 6 rows with canary-pwr's plugin path): an optimizer pass without a winner goes on for one
  pass at normal ripup costs instead of stopping the loop (Batch optimizer); neutral, accepted on
  `first_pass_switch_both_behaviours` and `pass_end_decision_both_behaviours`.
- `java_quirks.opt_equal_key_skip` = `false` (N06.13 from N06.9, `docs/roadmap/prereg/N06.9.json`,
  batch O under `enhanced`: gate PASS, total 76 → 76, set noise 19.95): the optimizer's candidate
  order keeps every sorted candidate, not only the first of each `(x, y, layer)` key; neutral,
  accepted on `equal_key_switch_keeps_every_candidate`.
- `java_quirks.shove_via_into_pending_piece` = `false` (N06.13 from N06.15,
  `docs/roadmap/prereg/N06.15.json`, both pairs gate PASS; under `enhanced` total 312 → 319, set
  noise 69.24, Protein 89 → 96 unrouted within its band of 23, no new router clearance violation
  or KiCad short on 7 DSN rows and canary-pwr's plugin path): a shove fails at a via of another
  net that its recursion moved onto a substitute piece not yet on the board, instead of inserting
  the piece over it, a short (Shove insert); accepted as a no-regress correctness fix
  (`pending_piece_switch_both_behaviours_and_counters`).
- `java_quirks.any_angle_room_side_index` = `false` (N06.13 from N06.16,
  `docs/roadmap/prereg/N06.16.json`, under `enhanced`: under Java's indexes
  `Issue026-J2_reference-any` and `Issue143-rpi_splitter-any` abort at the 8000000 kB cap, under
  the fix they route with 0 unrouted and 0 clearance violations (the registered abort rule); the other 6 rows gate PASS, 0 → 0,
  sessions identical): any-angle room neighbours take their touching sides, corners and sort on
  the room's simplex, so the gap walk ends (Autoroute room graph; any-angle mode only); accepted
  on `room_side_index_switch_both_behaviours`.
- `java_quirks.cutout_hole_crosses_border` = `false` (N06.13 from N06.14,
  `docs/roadmap/prereg/N06.14b.json`, amendment 1, both N06.14 switches together under `enhanced`
  with the 35 earlier fixes: gate PASS, total 196 → 208, set noise 21.40, canary-pwr's DSN
  122 → 129 and plugin path 39 → 44 KiCad unconnected within their bands of 10, no hard-guard or
  guard breach on 8 rows): a simplex cutout's stand-in division line at a corner on the outer
  border no longer clips later pieces, so no piece goes missing where a hole crosses the outer
  border (geometry cutouts; reached in any-angle mode and on 45 degree boards with a trace off 45
  degrees); accepted on `java_breaks_the_invariant_outside_the_two_rows` and
  `all_fixed_cutouts_tile_the_rest_on_the_extended_set`.
- `java_quirks.simplex_octagon_half_corner` = `false` (N06.13 from N06.14, the same run and pair):
  a simplex cutout divides a hole corner that is not an integer point at its exact point, along
  the nearest lines through integer points, so the pieces no longer overlap (geometry cutouts);
  accepted on the same tests.
- `java_quirks.edge_pin_net_crosses_outline` = `false` (decided 2026-10-20, K15.19: routing never
  breaks the board's manufacturing minimums; a correctness rule, joined without a keep rule; its
  cost on wisweep's driver board in K15.19's note): no trace crosses the board outline, an
  edge-pin net's included (see "K15.19 edge-pin nets cross the outline").

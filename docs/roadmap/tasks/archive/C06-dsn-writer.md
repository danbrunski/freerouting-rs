# C06 — DSN writer

Opened 2026-10-15. Writing a board back out as a Specctra design is needed for
three things:
- the GUI's "Save DSN" (G08);
- `-do out.dsn` on the CLI. Java's headless path writes a **0-byte file**
  there and logs no error (HANDOVER; C01.6 records the run);
- a future direct `.kicad_pcb` front end (K series), which hands a DSN to
  the same engine.

The `freerouting-dsn` crate already writes a `Pcb` syntax tree as text
(`Pcb::to_dsn`). This checklist builds the `Pcb` from a board: the inverse of
the DSN reader in `io/specctra/parser/`, ported from the `write*` methods of
Java's `io/specctra/parser/*`, about 1.7k lines.

**Prerequisites:**
- C05.1, for the shared name rules;
- C01.4's `dsn` dumps for C06.7.

Nothing in it needs C02–C04. Java's `DsnWriter` passes `null` settings, so a
DSN it writes never contains `autoroute_settings`.

## What exists and what this adds

| Exists | Where |
|---|---|
| `Pcb` syntax tree and plain writer | `crates/freerouting-dsn/src/{ast.rs, write/dsn.rs, write/common.rs}` |
| DSN → board, the direction this inverts | `crates/freerouting/src/io/specctra/dsn_reader.rs` (`build_board`), `parser/{structure.rs, layer.rs, library.rs, network.rs, shape.rs, wiring.rs}` |
| Board statistics comparison against Java | `crates/freerouting/tests/reference_load.rs` (`COUNT_FIELDS` and the compared fields) |
| Java at `cda540f` | `io/specctra/DsnWriter.java` (`write`, `writePcbScope`); `io/specctra/parser/`: `Structure.writeScope` (:55) and `writeLayers`, `writeDefaultRules`, `writeViaPadstacks`, `writeControlScope`, `writeBoundaries`, `writeKeepouts`, `writeKeepoutScope`, `writeConductionAreas`, `writeSnapAngle`; `Rule.writeScope`, `writeDefaultRule`, `writeNonDefaultClearanceRules`, `writeNamedClearanceRules`, `writeItemClearanceClass`; `Placement.writeScope`; `Package.writePlacementScope` (:356), `Package.writeScope` (:152), `writePackageKeepout`; `Component.writeScope`, `writePinInfo`, `writeKeepoutInfos`; `Library.writeScope`, `writePadstackScope`; `PartLibrary.writeScope`; `Network.writeScope`, `writeViaInfos`, `writeViaRules`, `writeNetClasses`, `writeNetClass`; `Net.writeScope`, `writeCircuit`, `writePin`; `Plane.writeScope`; `Wiring.writeScope`, `writeViaScope`, `writeWireScope`, `writeConductionAreaScope`, `writeFixedState`; `Layer.writeScope`; `Parser.writeScope` (full form); `Resolution.writeScope`; `Unit.writeScope` |

What this adds: `io/specctra/dsn_writer/`, which turns a board into a `Pcb`,
and the `-do out.dsn` output.

## Decisions made here

- **Semantic parity, not byte parity.**
  - Rust writes with `Pcb::to_dsn`, the plain style.
  - The Java comparison is on parsed trees: the AST groups repeated scopes,
    so scope order doesn't count.
  - Java's DSN text isn't a byte oracle anyway. It writes `snapAngle` and
    `viaRule`, which its own reader skips (DIVERGENCES, freerouting-dsn), so
    the comparison test rewrites those two keywords in Java's text before
    parsing it.
- **The real check is the round trip.** For every fixture,
  `build_board(parse(write_dsn(build_board(dsn))))` must equal the first
  board on every field `reference_load.rs` compares. That proves the writer
  keeps whatever the engine reads, independently of Java's writer quirks.
- **`compatMode` is not ported.** Only the GUI's compat export uses it, and
  nothing headless does. The module docs say so.
- **The board is written as it is.** After `prepare_job` the board may carry
  C03's `board_edge` and `hole_edge` classes, and they are written out. A
  re-read then sees an explicit outline class, so the default edge override
  is skipped on the next run, which is Java's rule for explicit classes.
  The DIVERGENCES line for `-do out.dsn` says this.
- **D1 speed.** Build each scope in one pass over the item list, bucketed by
  kind and net, never with one scan per net or per component.

## File ownership

| Item | New files | May edit |
|---|---|---|
| C06.1 | `crates/freerouting/src/io/specctra/dsn_writer/{mod.rs, structure.rs}`, `crates/freerouting/tests/dsn_writer.rs` | `io/specctra/mod.rs` (append exports) |
| C06.2 | `dsn_writer/rules.rs` | `dsn_writer/{mod.rs, structure.rs}` |
| C06.3 | `dsn_writer/placement.rs` | `dsn_writer/mod.rs` |
| C06.4 | `dsn_writer/library.rs` | `dsn_writer/mod.rs` |
| C06.5 | `dsn_writer/network.rs` | `dsn_writer/mod.rs` |
| C06.6 | `dsn_writer/wiring.rs` | `dsn_writer/mod.rs` |
| C06.7 | `crates/freerouting/tests/dsn_round_trip.rs` | `tests/dsn_writer.rs` |
| C06.8 | — | `crates/freerouting/src/job/outputs.rs` (append the DSN format; C07.4 owns the file), `docs/DIVERGENCES.md` (append) |

Paths are under `crates/freerouting/src/io/specctra/` unless they start with
`crates/` or `docs/`. Each item's test goes in `tests/dsn_writer.rs` as a
section test; C06.7 runs the full fixture set.

## Checklist

- [x] **C06.1 — Frame, parser, resolution, unit, layers, boundary, via list,
  control.**
  Java: `DsnWriter.writePcbScope`, the full form of `Parser.writeScope`
  (`string_quote`, `space_in_quoted_tokens`, host,
  `generated_by_freerouting`), `Resolution.writeScope`, `Unit.writeScope`,
  and from `Structure.writeScope`: `writeLayers` (with `Layer.writeScope`),
  `writeBoundaries`, `writeViaPadstacks`, `writeSnapAngle`,
  `writeControlScope`.
  Rust: `dsn_writer::pcb_from_board(board, design_name) -> Pcb`.
  - **Accept:** on every fixture, the re-read board has the same layer
    count and layer names, signal flags, outline shapes, resolution, unit,
    via padstack list and snap angle as the original. That is a section
    test comparing those fields directly.

  *Done 2026-10-16:* `dsn_writer::pcb_from_board`, `write_dsn`, `write_dsn_to` (`io/specctra/dsn_writer/{mod,structure,rules}.rs`).
  `tests/dsn_writer.rs::frame_survives_the_round_trip` compares, on all 165 fixtures that load, the layer names and signal flags, the outline shapes, resolution and unit, the via padstack list, the snap angle, `via_at_smd`, the bounding box and the outline's clearance class of the board and of the board read back from its own DSN: no difference. `tests/dsn_round_trip.rs::rust_tree_equals_the_tree_of_javas_dump_scope_by_scope` compares the parser, resolution, unit, layers, boundaries and via list with Java's dumps (19 boards, `a773a70ec`; the Java files are identical to `cda540f`: `git diff cda540f a773a70ec --stat -- src/main/java/app/freerouting/io/specctra` is empty). Differences from Java (docs/DIVERGENCES.md, "Writing a design file"): the `pcb` rectangle is shrunk by the 1000 the reader adds (`the_bounding_box_does_not_grow`); the via list loses the names the classes append.

- [x] **C06.2 — Rules, keepouts and planes in the structure.**
  Java: `Rule.writeDefaultRule`, `writeNonDefaultClearanceRules`,
  `writeNamedClearanceRules`, `writeLayerRule`, `writeItemClearanceClass`;
  `Structure.writeKeepouts` and `writeKeepoutScope`, covering `keepout`,
  `via_keepout` and `place_keepout`; `Structure.writeConductionAreas` and
  `Plane.writeScope`.
  - **Accept:**
    - On every fixture, the re-read board's clearance matrix (names and
      values per layer) equals the original's.
    - Its obstacle areas (kind, layer, shape, clearance class) and its
      conduction areas (net, layer, shape, `is_obstacle`) are equal, in
      item-list order.

  *Done 2026-10-16:* `rules.rs` (clearance matrix as pairs, layer rules, owned classes), `structure.rs` (keepouts, planes). `clearance_matrix_survives_the_round_trip` (names and values per layer, default trace widths, `smd_to_turn_gap`) and `areas_survive_the_round_trip` (kind, layer, shape, class, and for conduction areas net, fixed state and `is_obstacle`, in item-list order) pass on all 165 fixtures with no expected mismatch. Pinned differences: `named_clearance_classes_are_written_as_pairs`, `layer_rules_carry_the_width_of_their_layer`, `keepout_kinds_classes_and_fixed_states_are_written` (a place keepout stays a `place_keepout`, a class of `null` or `default` that the reader would not infer is written).

- [x] **C06.3 — Placement.**
  Java: `Placement.writeScope`, `Package.writePlacementScope`,
  `Component.writeScope`, `writePinInfo`, `writeKeepoutInfos`.
  - **Accept:** on every fixture, every component's name, package, side,
    rotation, position (exact), fixed flag, and pin and keepout
    clearance-class overrides survive the round trip.

  *Done 2026-10-16:* `placement.rs`. `placement_survives_the_round_trip` compares name, package, side, placed flag, exact position, rotation, fixed flag, logical part and the pin and keepout clearance classes of every component on all 165 fixtures; equal. A placed component that owns no item (a logo without pads) is not written (Java's `writePlacementScope` cannot tell it from a deleted one) and is left out of the comparison on both sides.

- [x] **C06.4 — Library images, padstacks, part library.**
  Java: `Library.writeScope`; `Package.writeScope` (pins, outlines,
  keepouts through `writePackageKeepout`); `Library.writePadstackScope`
  (shapes per layer, `(attach off)`, rotation flags); `PartLibrary.writeScope`
  (logical parts, pin swap groups).
  - **Accept:** on every fixture, every package and every padstack equals
    the original on re-read. Packages are compared on pin names,
    padstacks, relative positions, rotations, outlines and keepouts.
    Padstacks are compared on name, shapes per layer and the attach flag.
    The logical parts are equal too.

  *Done 2026-10-16:* `library.rs`. `library_survives_the_round_trip` compares each package (pins with padstack, exact relative position and rotation; outlines by their corners; keepouts, via keepouts and place keepouts with names and layers), each padstack (shapes and auxiliary shapes per layer, `attach`, `absolute`) and the logical parts with their components, on all 165 fixtures. One expected mismatch (`EXPECTED` in `tests/dsn_support/mod.rs`): `Issue179` padstack `p7`, a two-line sliver with no corners to write. Port of `NonConvexPadstackTest.dsnExportPreservesAuxiliaryShapesInRoundTrip`: `dsn_export_preserves_auxiliary_shapes_in_round_trip`.

- [x] **C06.5 — Network: nets, via infos, via rules, classes.**
  Java: `Network.writeScope`, `Net.writeScope` (pins, `writeCircuit`),
  `writeViaInfos`, `writeViaRules`, `writeNetClasses`, `writeNetClass`, and
  the class-class rules.
  - **Accept:** on every fixture, the re-read nets (names, pins,
    contains-plane), net classes (all fields), via infos (in order) and via
    rules equal the original. `BoardStatistics` `nets.*` are equal.

  *Done 2026-10-16:* `network.rs`. `network_survives_the_round_trip` compares each net (number, name, subnet, class, contains-plane, length constraint, pins), each net class (widths per layer, active layers, trace clearance class, the four default item clearance classes, via rule, shove-fixed, pull-tight, lengths), the via infos in order and the via rules (two rules of one name collapse to the last, as `addViaRule` does) on all 165 fixtures. One expected mismatch: `canary-pwr/canary-pwr-pours-as-planes` (the reader numbers the nets of ignored copper pours in file order; the pours are not items). `BoardStatistics` (so `nets.*`) are equal after the round trip (`round_trip_gives_the_same_statistics_and_drc_result_on_every_fixture`). Class-class rules are not written by Java and the board keeps none. Pinned: `net_classes_are_written_the_way_the_reader_makes_them`, `plane_nets_survive_through_use_net_and_plane_scopes`.

- [x] **C06.6 — Wiring.**
  Java: `Wiring.writeScope`, `writeWireScope`, `writeViaScope`,
  `writeConductionAreaScope` (non-plane areas only), `writeNet`,
  `writeFixedState`.
  - **Accept:** on every fixture with wiring, and on every fixture with its
    route reference SES imported, the re-read board's traces (layer, width,
    corners, net, fixed state) and vias (padstack, position, nets, fixed
    state) equal the original. Compare them as sets keyed by geometry,
    since item ids are renumbered.

  *Done 2026-10-16:* `wiring.rs`. `wiring_survives_the_round_trip` (traces by layer, width, corners, nets, fixed state and class; vias by padstack, position, nets, fixed state and class; compared as sorted lists) passes on all 165 fixtures, and `wiring_survives_with_the_route_reference_imported` on every fixture that has a Java route reference (the session imported, then the traces normalized the way the reader does). It also asserts that the writer writes every trace with a net and every via. A session can state a via the design already has; the board has both and the reader skips the second, so the comparison deduplicates the original's vias by position.

- [x] **C06.7 — Full round trip, the Java comparison, `DsnWriterTest`.**
  - **Accept:**
    - `dsn_round_trip.rs`: on every fixture that Java loads (the
      `reference_load` set), the round trip gives a board equal to the
      original on every field `reference_load.rs` compares, and the same
      full DRC result.
    - For every board in C01.4's committed subset, Rust's `Pcb` equals the
      tree of Java's dump (after the keyword rewrite) scope by scope.
      Every difference is listed in the test with its cause: Java bug,
      Rust divergence already listed, or a new finding.
    - Port of `DsnWriterTest`: on `Issue143-rpi_splitter.dsn` the output
      starts with `(pcb`, contains `(structure`, and a re-read keeps the
      layer count.
    - The release-build write time of `Issue190-processor.Z80` is in the
      done note.

  *Done 2026-10-16:* `tests/dsn_round_trip.rs`.
  - `round_trip_gives_the_same_statistics_and_drc_result_on_every_fixture`: `BoardStatistics` (every field, a superset of `reference_load.rs`'s) and the full DRC findings (type, severity, description without item ids, items sorted; an unconnected pair by its net, because the pair names the lowest-id item of each group and ids are renumbered) equal on all 165 fixtures; `components.total_count` is compared without the item-less components Java does not write.
  - `rust_tree_equals_the_tree_of_javas_dump_scope_by_scope`: Rust's `Pcb` against the tree of Java's dump (keywords `snapAngle`, `viaRule`, `logicalPart`, `shoveFixed` rewritten) on the 19 boards, scope by scope; every scope has its explained difference noted in the test (`unexplained`), and all else is equal.
  - Ports of `DsnWriterTest`: `writes_valid_dsn_header`, `roundtrip_preserves_layer_count`, `output_stream_contains_data_after_write`. Not ported: `compatModeProducesOutput` (`compatMode` is not ported). Also ported: `NonConvexPadstackTest.dsnExportPreservesAuxiliaryShapesInRoundTrip` and `Issue955PlacementLifecycleTest.dsnRoundTripPreservesBothContacts` (three round trips).
  - Release build, `write_dsn` of `Issue190-processor.Z80`: 46.3 ms for 2 355 821 bytes (best of 5; `cargo test --release --test dsn_round_trip write_time -- --nocapture`).

- [~] **C06.8 — `-do out.dsn`.**
  Register the DSN format in C07.4's output table (`job/outputs.rs`), so
  `-do out.dsn` and extra `.dsn` outputs write the final board. If C06
  lands before C07.4, C07.4 registers it instead and ticks this item's
  remainder.
  - Add a DIVERGENCES line: Java's headless `-do x.dsn` writes a 0-byte
    file and no error. Rust writes the board, including any classes added
    by `prepare_job`.
  - **Accept:**
    - The CLI test `do_dsn_writes_the_board`: `freerouting -de
      Issue413-test.dsn -do out.dsn --router.enabled=false
      --router.fanout.enabled=false --router.optimizer.enabled=false`
      exits 0.
    - `out.dsn` is non-empty and loads with `build_board` to the same
      `BoardStatistics` as the input.

  *Partial 2026-10-16:* `crates/freerouting/src/job/outputs.rs` does not exist yet (C07.4 owns it), so the format is not registered and the CLI test `do_dsn_writes_the_board` is not written: C07.4 registers `.dsn` with `io::specctra::dsn_writer::write_dsn_to` (it takes any `Write`; wrap the temporary file in a `BufWriter`) and adds that test. Done here: the DIVERGENCES entry ("Writing a design file": Java's headless `-do x.dsn` writes 0 bytes and no error; Rust writes the board as it is, including the classes `prepare_job` added) and the engine side of the accept clause, `tests/dsn_round_trip.rs::prepare_job_classes_are_written` (after `prepare_job` the matrix has `board_edge`; the re-read board has the same matrix, frame, areas and outline class). The non-empty output and `BoardStatistics` equality hold for all fixtures (`round_trip_gives_the_same_statistics_and_drc_result_on_every_fixture`).

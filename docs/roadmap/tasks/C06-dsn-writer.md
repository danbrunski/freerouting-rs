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

- [ ] **C06.1 — Frame, parser, resolution, unit, layers, boundary, via list,
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

- [ ] **C06.2 — Rules, keepouts and planes in the structure.**
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

- [ ] **C06.3 — Placement.**
  Java: `Placement.writeScope`, `Package.writePlacementScope`,
  `Component.writeScope`, `writePinInfo`, `writeKeepoutInfos`.
  - **Accept:** on every fixture, every component's name, package, side,
    rotation, position (exact), fixed flag, and pin and keepout
    clearance-class overrides survive the round trip.

- [ ] **C06.4 — Library images, padstacks, part library.**
  Java: `Library.writeScope`; `Package.writeScope` (pins, outlines,
  keepouts through `writePackageKeepout`); `Library.writePadstackScope`
  (shapes per layer, `(attach off)`, rotation flags); `PartLibrary.writeScope`
  (logical parts, pin swap groups).
  - **Accept:** on every fixture, every package and every padstack equals
    the original on re-read. Packages are compared on pin names,
    padstacks, relative positions, rotations, outlines and keepouts.
    Padstacks are compared on name, shapes per layer and the attach flag.
    The logical parts are equal too.

- [ ] **C06.5 — Network: nets, via infos, via rules, classes.**
  Java: `Network.writeScope`, `Net.writeScope` (pins, `writeCircuit`),
  `writeViaInfos`, `writeViaRules`, `writeNetClasses`, `writeNetClass`, and
  the class-class rules.
  - **Accept:** on every fixture, the re-read nets (names, pins,
    contains-plane), net classes (all fields), via infos (in order) and via
    rules equal the original. `BoardStatistics` `nets.*` are equal.

- [ ] **C06.6 — Wiring.**
  Java: `Wiring.writeScope`, `writeWireScope`, `writeViaScope`,
  `writeConductionAreaScope` (non-plane areas only), `writeNet`,
  `writeFixedState`.
  - **Accept:** on every fixture with wiring, and on every fixture with its
    route reference SES imported, the re-read board's traces (layer, width,
    corners, net, fixed state) and vias (padstack, position, nets, fixed
    state) equal the original. Compare them as sets keyed by geometry,
    since item ids are renumbered.

- [ ] **C06.7 — Full round trip, the Java comparison, `DsnWriterTest`.**
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

- [ ] **C06.8 — `-do out.dsn`.**
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

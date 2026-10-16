# C05 — SES writer

Opened 2026-10-15. The CLI's main output is a Specctra session file. KiCad
imports it, the KiCad plugin depends on it, A14's board-parity gate compares
it, and the GUI saves it. The Rust engine can read a session
(`io::specctra::read_session`) and can write a `Session` syntax tree as text
(`Session::to_ses`). Nothing turns a board into a `Session` yet. This
checklist ports Java's `SesWriter`. By default it writes Java's exact text, so
the plugin's output does not change when the engine changes, and every Java
reference SES is a byte-for-byte oracle.

**Prerequisites:**
- C01.3's `ses` dumps;
- the 160 SES files in `fixtures/reference/route/`, which Java's `SesWriter`
  wrote, for C05.1;
- F02.1's `probe.sh` for C05.1's number-format probe (the jdk4py `java` from
  `run.sh` works without it).

C03 is not required: the tests load boards with `load_design`, as the `ses`
dump does.

## What exists and what this adds

| Exists | Where |
|---|---|
| `Session` syntax tree: placement, `was_is`, routes with `resolution`, `parser`, `library_out`, `network_out` | `crates/freerouting-dsn/src/ast.rs` (`Session`, `Routes`, `SessionNet`, `ParserInfo`, `Place`, `Wire`, `Via`, `Padstack`) |
| Plain writer: `)` on the scope's line, quotes only when needed, numbers without `.0`, no exponents | `crates/freerouting-dsn/src/write/{mod.rs, ses.rs, common.rs}` |
| Board data the writer needs | `board.communication.{resolution, unit, coordinate_transform, specctra_parser_info}`; `host_cad_is_kicad()`; `library.via_padstack_ids()`, `Padstack::attach_allowed`; `Component::exact_location`, `position_fixed`; `BasicBoard::{items, connectable_items, trace_start_contacts, trace_end_contacts}`; pad shapes with `border_distance` |
| Java at `cda540f` | `io/specctra/SesWriter.java` (`write` :54, `writeSessionScope` :72, `writePlacement` :95, `writeComponents` :116, `writeComponent` :152, `writeWasIs` :182, `writeRoutes` :219, `writeLibrary` :234, `formatPlacementRotation` :259, `writePadstack` :262, `writeNetwork` :306, `writeNet` :320, `writeWire` :363, `snappedEndpoint` :417, `writeVia` :446, `writeFixedState` :469, `writePath` :483, `writeConductionArea` :505); `datastructures/{IndentFileWriter, IdentifierType}.java`; `io/specctra/parser/{Parser.writeScope (reduced), Resolution.writeScope, Circle, Rectangle, Polygon, PolygonPath, PolylinePath}.writeScopeInt` |

What this adds:
- `freerouting-dsn`'s Java-layout session serializer and the Java number
  and name formatters;
- `io/specctra/ses_writer.rs`, which turns a board into a `Session`.

## Decisions made here

- **Content and layout are separate.**
  - `ses_writer::session_from_board(board, design_name) -> Session` decides
    content.
  - `freerouting_dsn::write::java_style::write_session_java(&Session) ->
    String` reproduces `IndentFileWriter`'s layout exactly:
    - `startScope()` writes `\n`, the indent and `(`;
    - `endScope()` writes `\n`, the indent and `)`;
    - two spaces per level, UTF-8, no newline at the end of the file;
    - Java's literal fragments, among them `(routes `, `(library_out `,
      `(network_out ` (with the trailing space), `" (lock_type position)"`
      after a newline, and `(constant a b )` with its space before `)`.
  - The serializer knows from each scope's kind whether Java wrote an
    `int` (path coordinates, wire widths, padstack shapes) or a `double`
    (place and via coordinates).
- **Java-style text is the default output.** `Session::to_ses` stays as the
  plain style, used where Java isn't the oracle. Exponent form
  (`1.2345E8` at 10⁷ and above) is kept for parity. KiCad exports at
  `(resolution um 10)`, so its boards reach 10⁷ units only at 1 m, and the
  question doesn't arise for Dan's boards.
- **Numbers.**
  - `java_double(f64)` is `Double.toString` as JDK 19+ prints it: the
    shortest digits that round-trip (the same digits Rust's `{}` and `{:e}`
    choose), at least one digit after the point, and scientific form
    `d.dddE±n` below 10⁻³ and at or above 10⁷.
  - `java_plain_decimal(f64)` is `BigDecimal.valueOf(d).stripTrailingZeros()
    .toPlainString()`, used for rotations.
- **Names: `IdentifierType.write` with one fix.**
  - Quote a name when it contains any of `( ) space ; - _ / ~ { }`, any
    byte ≤ 0 of its UTF-8 encoding (any non-ASCII byte), or matches
    `^-?\d.*`.
  - The quote character is first deleted from the name.
  - Java's loop that unwraps a name already in quotes cuts
    `substring(1, len - 2)`, which also drops the last real character.
    Rust cuts only the two quotes. That gets a DIVERGENCES line and a test,
    and C05.1's self-check confirms no reference SES depends on it.
- **D1 speed.** Java scans every board item once per component
  (`writeComponents`) and once per net (`getConnectableItems`). Rust makes
  one pass over the item list to collect the component ids that still own
  items, and one pass to bucket connectable items by net, keeping item-list
  order inside each net. The output is unchanged.
- **Endpoint snapping is ported exactly.** It probably never fires.
  `Trace.getNormalContacts` (`Trace.java:173-203`) admits a `DrillItem`
  only when the trace corner equals the drill centre, and `snappedEndpoint`
  then returns `null` because the distance is 0 ≤ 0.5. That conclusion comes
  from reading the code. C05.4 counts snaps on every fixture to confirm it.

## File ownership

| Item | New files | May edit |
|---|---|---|
| C05.1 | `crates/freerouting-dsn/src/write/java_style.rs`, `crates/freerouting-dsn/tests/java_style_ses.rs`, `tools/java-reference/probes/JavaNumberFormatProbe.java` | `crates/freerouting-dsn/src/write/mod.rs` and `src/lib.rs` (append `pub mod java_style;` and `Session::to_ses_java`), `docs/DIVERGENCES.md` (append) |
| C05.2 | `crates/freerouting/src/io/specctra/ses_writer.rs`, `crates/freerouting/tests/ses_writer.rs` | `io/specctra/mod.rs` (append exports) |
| C05.3 | — | `ses_writer.rs`, `tests/ses_writer.rs` |
| C05.4 | — | `ses_writer.rs`, `tests/ses_writer.rs` |
| C05.5 | — | `ses_writer.rs`, `tests/ses_writer.rs` |
| C05.6 | `crates/freerouting/tests/ses_round_trip.rs` | — |

## Checklist

- [ ] **C05.1 — Java-layout serializer, `java_double`, names.**
  Java: `IndentFileWriter`, `IdentifierType.write`, `SesWriter`'s write
  order, the reduced `Parser.writeScope`, `Resolution.writeScope` and the
  `writeScopeInt` of each shape class. Rust: `JavaIndentWriter` (start and
  end scope, new line, write), `java_ident(name, quote) -> Cow<str>`,
  `java_double`, `java_plain_decimal` and `write_session_java`.
  - **Accept:**
    - **Self-check on Java's own output.** Every
      `fixtures/reference/route/**/*.ses` (160 files) gives back exactly
      its own bytes from `Session::parse` → `write_session_java`. The test
      lists any exception with its cause. Boards whose names Java's
      scanner stripped of non-ASCII characters are expected exceptions
      (see DIVERGENCES, freerouting-dsn).
    - Port of `IdentifierTypeTest`, every case.
    - `identifier_with_outer_quotes_keeps_last_char` covers the fix.
    - Port of `SesRoundTripTest.placementRotationFormattingMatchesKicadStyle`:
      `0.0→"0"`, `339.0→"339"`, `338.5→"338.5"`,
      `12.2501234567→"12.2501234567"`, `-45.25→"-45.25"`.
    - A `java_double` table of at least 30 values: integers below and at
      10⁷, `1.2345E8`, `0.001`, `1.0E-4`, `-0.0`, subnormals, and values
      from the Issue676 references. Its expected strings come from
      `JavaNumberFormatProbe.java` run at the pin. The probe output is
      pasted into the test with the command that produced it.

- [ ] **C05.2 — Board → `Session`: header, placement, `was_is`, routes
  header.**
  Java: `write`, `writeSessionScope`, `writePlacement`, `writeComponents`,
  `writeComponent`, `writeWasIs` and the first part of `writeRoutes`.
  - The session name is `design_name.replace(".dsn", ".ses")`, which
    replaces every occurrence. `base_design` is `design_name`.
  - The SES transform is `scale = coordinate_transform.dsn_to_board(1) /
    resolution` with offset 0, so the file is in resolution units.
  - Placement:
    - `(resolution unit res)`;
    - for each library package `1..=n`, in order, the components
      `1..=m` that use that package and still own an item, under
      `(component <package>`;
    - each as `(place <name> x y front|back <rotation>)`, with `x` and `y`
      from `exact_location` through the transform as doubles;
    - `(lock_type position)` only when `position_fixed` and the host is
      not KiCad.
  - `was_is` is empty for batch routing. Rust has no pin swap state.
  - The routes header is the resolution, then the reduced parser scope:
    `host_cad`, `host_version`, the constants and `write_resolution`.
  - **Accept:** for every board in `dump-subset.txt`, Rust's unrouted SES
    up to the `(library_out` line equals C01.3's `ses/unrouted/<b>.ses`
    byte for byte.

- [ ] **C05.3 — `library_out` and `(attach off)`.**
  Java: `writeLibrary`, `writePadstack`.
  - Walk the via padstacks in library order and skip any name already
    written (`LinkedHashSet`).
  - The layer range runs from the first to the last layer with a shape.
  - Write one `(shape …)` per layer that has a shape, through
    `boardToDsnRel` as an integer shape.
  - Write `(attach off)` after a newline when `!attach_allowed`. That is
    D8's "never a via in a pad" carried into the session, from fork commit
    `0b3ff9051`.
  - **Accept:**
    - The `library_out` section is byte-identical to C01.3 on every subset
      board.
    - `ses_writes_attach_off_for_no_attach_padstacks` runs on
      `Issue508-DAC2020_bm06.dsn`, whose DSN has `(attach off)` padstacks.
    - Port of `SesRoundTripTest`'s check that `library_out` padstack names
      are unique.

- [ ] **C05.4 — `network_out`: wires, vias, conduction areas.**
  Java: `writeNetwork`, `writeNet`, `writeWire`, `snappedEndpoint`,
  `writeVia`, `writeFixedState`, `writePath`, `writeConductionArea`.
  - Nets run `1..=max_net_number`. A net's header is written only when it
    has a wire, a via or a signal-layer conduction area.
  - Items come in item-list order and SYSTEM_FIXED items are skipped.
  - **Wire width.** `java_round(board_to_dsn(2 * half_width)) as i32`.
  - **Corners.** Map each corner through `board_to_dsn` and round with
    `java_round`. Drop a corner equal to the previous *written* one. The
    first and last corners first go through `snapped_endpoint`.
  - **Vias.** `(via <padstack> x y)` with doubles.
  - **Fixed state.** An ordinal at or below SHOVE_FIXED writes nothing.
    Otherwise write `(type protect)`, since SYSTEM_FIXED never reaches the
    writer.
  - **Conduction areas** on signal layers: `(wire <shape> (window <hole>)*)`.
  - **Accept:**
    - Rust's routed SES equals C01.3's `ses/routed/<b>.ses` byte for byte
      on every subset board. There is one expected exception,
      `Issue676-ch32v-tx118s`: Java's `SesReader` imports none of its wires
      and Rust's imports all 48 (see DIVERGENCES, DRC section). Its
      *unrouted* SES must still match.
    - The `#[ignore]` test `ses_full_set_matches_java` does the same for
      all boards in `target/java-reference/full/ses/`. Its done note gives
      the count and lists any exception with its cause.
    - The test counts endpoint snaps over every fixture's routed board and
      reports the number. If it is 0, the module docs say snapping never
      fires at the pin.

- [ ] **C05.5 — Public API and styles.**
  - `ses_writer::write_ses(board, design_name, SesStyle::{Java, Plain}) ->
    String`, with `Java` as the default.
  - `write_ses_to(board, design_name, style, &mut impl Write)` streams for
    large boards. Java style is byte-identical whichever entry point is
    used.
  - **Accept:**
    - On every fixture, both styles parse to equal `Session` trees.
    - `Plain` contains no exponent.
    - The release-build write time of `Issue190-processor.Z80` (Java
      style) is in the done note, next to `load_design` for the same
      board.

- [ ] **C05.6 — Round trip and the Java test ports.**
  - **Accept:**
    - Ports of `SesRoundTripTest`:
      - `sesWriterProducesValidHeader`: starts with `(session `; contains
        `(routes`; has none of `(hostCad`, `(stringQuote`,
        `(writeResolution`, `(viaRule`, `(pullTight`, `(shoveFixed`,
        `(useLayer`, `(clearanceClass`;
      - `sesRoundTripPreservesWireCount` and
        `writerOutputCanBeReadBackBySesReader` on `Issue593-BBD_Mars-64`
        with `fixtures/Issue593-BBD_Mars-64.ses`;
      - `issue742SesRoundTripsWithoutErrors`: balanced parentheses,
        `library_out` before `network_out`, no `"0.000"`, rotation `338.5`;
      - `endpointSnappingIsStableAndRoundTrips`;
      - the three invalid-input cases, as errors from `read_session`.
    - Port of the SES half of `MultiOutputGeneratorTest`:
      `Issue508-DAC2020_bm01` unrouted gives a non-empty SES.
    - **Property on every fixture with a route reference.** Load, import
      the reference SES, write it, re-import into a fresh load. The
      `BoardStatistics` of both imports are equal.

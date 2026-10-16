# K07 — `.kicad_pcb` entry points, parity and design rules

Opened 2026-10-15. K04–K06 build a DSN syntax tree from a `.kicad_pcb` file.
This checklist makes it usable and proves it: the engine, the CLI and the GUI
open `.kicad_pcb` directly; a parity test compares every committed board
against KiCad 10's own DSN export of it, with every difference named; a round
trip through KiCad checks the coordinate frame (K00 question 2); and the
reader reads `.kicad_dru`, reporting every rule it can't express.

It also matters for KiCad 11. KRT's docstring says KiCad master removed the
SWIG Python bindings in 2026-03 (commit `65a442b1`; unverified here), and
KiCad master exports pours as `(wire (polygon ...))` rather than planes (GUI
report §6.2, unverified). A plugin that hands the CLI the `.kicad_pcb`
needs neither `ExportSpecctraDSN` nor KiCad's exporter. Bringing the result
back still needs `ImportSpecctraSES` or the optional writer
([K08](K08-kicad-pcb-writer.md)).

Prerequisites: [K06](K06-kicad-pcb-rule-areas-and-copper.md). K07.2 needs
[C07](C00-cli-settings-io-README.md) (CLI argument handling, which owns
`crates/freerouting-cli`) and [G01](archive/G01-session-and-test-harness.md) (the
GUI's `Session`); without them it is partial. K07.4 needs
[C05](C00-cli-settings-io-README.md) (SES writer) for its last step and
[F02.6](F02-parity-and-benchmark-harness.md) for KiCad DRC grading.

## What exists and what this adds

| Exists | State |
|---|---|
| `freerouting::io` | `read_board(bytes)`, `read_board_from_str`, `build_board(&Pcb)`, `read_session`; DSN only |
| `drc::run_drc` (`drc/runner.rs`) | Reads the design with `read_board(bytes)`; the CLI's `-drc` mode uses it |
| `freerouting-cli` | `-de` takes design files told apart by extension (`.dsn`, `.ses`, `.rules`) |
| `freerouting-gui` | Opens `.dsn` and `.ses`; the built-in file browser filters to those (`file_dialog.rs`) |
| `freerouting::scoring::BoardStatistics::new(&BasicBoard)` | The aggregate fields the reference tests compare |
| KiCad exporter and SES importer, `10.0.0` | Export: `(unit um)`, `(resolution um 10)` with the comment "tenths of micrometers", µm values, no rescaling (`FromBOARD`). Import: `nm = KiROUND(1000 × value / resolution)` for `um` units, Y negated (`scale`, `mapPt` in `specctra_import.cpp`) |
| KRT `py_router/design_rules.py`, `kicad_dru.py` | `.kicad_dru` parsing; supported condition functions (`hasNetclass`, `hasExactNetclass`, `onLayer`, ...) and a known-unsupported list (`intersectsArea`, `insideCourtyard`-style area functions, `inDiffPair`, ...); unsupported rules are kept and reported, never dropped (KRT #770) |
| `canary-pwr.kicad_dru` | Ten rules of the form `(constraint clearance (min 0.13mm)) (condition "A.insideCourtyard('Q2') && B.insideCourtyard('Q2')")` |

## Decisions made here

- **One dispatch point.** `freerouting::io::read_design(path) ->
  DesignRead` picks the reader by extension (`.dsn` or `.kicad_pcb`, case
  ignored) and, for `.kicad_pcb`, reads `<stem>.kicad_pro` and
  `<stem>.kicad_dru` next to it when present. The CLI, the DRC runner and
  the GUI call it, so they can't drift.
- **The parity reference is KiCad's export, read by our DSN path.** For
  each committed board, `build_board(read_kicad(board))` is compared with
  `build_board(parse(<board>.kicad10.dsn))`. Expected differences live in
  the test, each with its reason, like `EXPECTED_MISMATCHES` in
  `tests/reference_load.rs`. An unexplained difference is a reader bug.
- **Tolerances come from the export's formatting.** KiCad prints shapes,
  wires and vias with `%.6g`, so a coordinate `v` in the export is exact to
  half a unit in its sixth significant digit: 0.5 µm at 100–999 mm, 0.05 µm
  at 10–99 mm. Board units are 0.1 µm. Compare each coordinate within that
  bound plus one board unit; compare placements within one board unit.
- **Design rules from `.kicad_dru` are read and reported, not applied by
  default.** KiCad's DSN export ignores `.kicad_dru`, so Java has never
  seen these rules. Applying the expressible subset (net-class pair
  clearances, net-class track widths) changes routing. It is an option,
  `ReadOptions::apply_design_rules` (default off), until measured with the
  KiCad oracle on at least three boards (Q-series rules). Rules that can't
  be expressed are reported once each, by name, in both modes.
- **Item order is compared and reported, not forced.** Where the reader's
  item order differs from the export's (net-class order, padstack hashes),
  `build_board` assigns different ids, which can change routing tie-breaks.
  The parity test prints the first differing position per item kind; fixing
  an order difference is optional unless a later route comparison shows it
  matters.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K07.1 | `crates/freerouting/src/io/kicad.rs`, `crates/freerouting/tests/kicad_read.rs` | `crates/freerouting/Cargo.toml` (`freerouting-kicad`), `crates/freerouting/src/io/mod.rs`, `crates/freerouting/src/drc/runner.rs` (call `read_design`) |
| K07.2 | — | `crates/freerouting-cli/src/main.rs` (extension case and report printing only), `crates/freerouting-gui/src/{session.rs,file_dialog.rs}` (open `.kicad_pcb`) |
| K07.3 | `crates/freerouting-kicad/tests/parity.rs` (or `crates/freerouting/tests/kicad_parity.rs` if the engine is needed) | — |
| K07.4 | `tools/kicad-reader/frame_check.py`, `tools/kicad-reader/frame-check.sh` | `fixtures/kicad/README.md` |
| K07.5 | `crates/freerouting-kicad/src/dru.rs`, `tests/dru.rs`, `fixtures/kicad/synthetic/rules.kicad_dru` | `src/read.rs`, `src/classes.rs` |
| K07.6 | `crates/freerouting-kicad/tests/corpus.rs` (`#[ignore]`) | `tools/bench/sets/kicad-reader.txt` |
| K07.7 | — | `docs/development/kicad-plugin.md` (append a section), `docs/development/engine.md` (one line in the module map) |

## Checklist

- [ ] **K07.1 — `freerouting::io::kicad` and `read_design`.**
  - `freerouting` depends on `freerouting-kicad`; `io/kicad.rs` re-exports
    the reader and adds `read_kicad_board(path) -> BoardReadResult`, which
    reads the three files, calls the reader and `build_board`, and merges
    the reader's warnings into `BoardReadResult`'s warnings (prefixed with
    the `.kicad_pcb` line number).
  - `io::read_design(path)` dispatches by extension; other extensions keep
    today's DSN behaviour. `run_drc` uses it.
  - **Accept:**
    - `freerouting -de fixtures/kicad/Issue558-dev-board-autoroute-demo/dev-board.kicad_pcb -drc out.json`
      writes a report; its clearance-violation count equals the count for
      the board's `.kicad10.dsn` (its one rule area bans everything, so the
      reader only adds a placement keepout, which no trace, via or pin
      collides with);
    - a missing `.kicad_pro` is not an error (KiCad defaults, K05.3);
    - a KiCad 5 file gives exit 1 and K04.4's one-line message.

- [ ] **K07.2 — CLI and GUI open `.kicad_pcb`.**
  - CLI: `freerouting -de board.kicad_pcb [-do out.ses]` uses
    `read_design`. Only the extension case and the printing of the read
    report (K06.7) are added here; flag semantics belong to C07. The
    session written later must name the base design so KiCad's import finds
    it (C05).
  - GUI: File > Open and drag and drop accept `.kicad_pcb`; `Session` loads
    through `read_design`; the warnings window shows the read report.
  - **Accept:**
    - a CLI integration test opens each committed board;
    - a GUI test (G01's egui_kittest harness) opens
      `Issue368-CorneyIslandWireless` and the board's item count equals
      `build_board` of the reader's tree;
    - partial until C07 and G01 land, with the remainder named.

- [ ] **K07.3 — Parity with KiCad's export.**
  For each committed board, compare the two boards:
  - item counts by kind (pins, traces, vias, each `AreaKind`, component
    outlines, board outline), and per kind the sequence of items (first
    differing position printed);
  - nets: names, pin sets, net classes and their rules;
  - `BoardStatistics` fields, except `host`;
  - DRC: `DesignRulesChecker::check()` findings by type, and the
    incomplete-connection count;
  - geometry: pins, traces, vias and areas matched by (kind, layer, net)
    and nearest position, within the tolerance of the decisions.
  `EXPECTED_DIFFERENCES` holds each known difference with its reason:
  rule-area kinds (K06.2), arcs (K06.5), net-class order (K05.3), `Cust`
  padstack names (K05.6), dropped footprint copper zones (K06.4), and
  anything K05–K06 listed.
  - **Accept:**
    - the test passes on all seven committed boards and the synthetic
      boards with every difference listed and explained;
    - the done note gives, per board, the counts that differ and why, and
      the item-order comparison (where the sequences first differ, or "same
      order");
    - removing one expected difference makes the test fail (negative check
      run once, noted).

- [ ] **K07.4 — Coordinate frame round trip (K00 question 2).**
  Facts read in KiCad 10.0.0 on 2026-10-15: the export declares
  `(resolution um 10)` and writes µm values without rescaling; the SES import
  computes `nm = KiROUND(1000 × v / resolution)`. So a session in resolution
  units (tenths of µm), as Freerouting writes it, lands on a 100 nm grid in
  KiCad. Check it end to end:
  1. Route the KiCad export of `Issue558-dev-board` with Java at the pin
     (`tools/java-reference`, or F02.3's `bench` once it exists) and keep the
     SES.
  2. `read_session` that SES onto `build_board(reader)` and onto
     `build_board(export)`: every imported trace and via must be at the same
     board coordinates in both (this checks the reader's frame against
     KiCad's).
  3. With C05: write a SES from the reader-built board after step 2. Import
     it and the Java SES into two copies of the `.kicad_pcb` with WSL KiCad
     (`frame_check.py`: `LoadBoard`, `ImportSpecctraSES`, then dump every
     track's start and end in nm), and compare.
  - **Accept:**
    - step 2: identical coordinates on all imported items;
    - step 3: every track endpoint within 100 nm between the two imports;
      partial until C05 lands;
    - the done note answers K00 question 2: what is lost (KiCad nm → 0.1 µm
      board units, at most 50 nm per coordinate) and whether a finer
      resolution is worth a measured change later.

- [ ] **K07.5 — `.kicad_dru`.**
  - Tokenize with K04's lexer (the format is s-expressions: `(version 1)`,
    `(rule "name" (constraint kind (min|opt|max v)) (condition "expr")
    (layer ...) (severity ...))`).
  - Parse conditions into a small expression tree. Supported: `A.NetClass
    == 'x'`, `B.NetClass == 'y'`, `A.hasNetclass('x')`, `&&`, `||`, `!`,
    and layer selection (`(layer ...)`, `onLayer`). Anything else (for
    example `insideCourtyard`, `intersectsArea`, `inDiffPair`) makes the rule
    UNSUPPORTED, kept with its name and the first unsupported term.
  - Mapping when `apply_design_rules` is on: a clearance rule between two
    net classes → `ClassClass` clearance; a track-width rule on one net
    class → that class's width (KiCad's `min` for clearance, `opt` for
    width). Rules apply in file order, later ones overriding earlier ones
    for the same target (KiCad gives no priority beyond file order; KRT
    `design_rules.py`).
  - The report lists every rule: applied, ignored (option off), or
    UNSUPPORTED with its reason.
  - **Accept:**
    - `canary-pwr.kicad_dru`: all ten rules reported UNSUPPORTED by name
      (`insideCourtyard`);
    - synthetic `rules.kicad_dru` with one class-pair clearance, one track
      width, one `intersectsArea` rule: with the option on, the board's
      clearance matrix and class width change exactly as the two supported
      rules say; the third is reported;
    - with the option off, `build_board` equals the board without the
      `.kicad_dru` (parity is unaffected).

- [ ] **K07.6 — Corpus run and timing.**
  - `tests/corpus.rs` (`#[ignore]`): every board in `$FR_KICAD_CORPUS`
    (K04.7: 30 more Java boards, 18 KiCad demos, `canary-pwr`) with an export
    made by K04.8 runs the K07.3 comparison and prints one summary row per
    board. F03.3's corpus (`$FR_CORPUS_DIR`), when present, is added the same
    way.
  - Time in release, quiet machine (1-minute load ≤ 1.0), three runs,
    median: read only, and read plus `build_board`, against parse plus
    `build_board` of the same board's export.
  - **Accept:**
    - the done note has the summary table and every board's unexplained
      differences, which become new reader items or listed differences;
    - the timing for `jetson-agx-thor-baseboard` and `vme-wren` (the two
      largest demos), with the DSN path's time beside it.

- [ ] **K07.7 — Documentation.**
  - `kicad-plugin.md`, a new section "Direct `.kicad_pcb` input": what is
    read; the rule-area mapping table from K06 with the `canary-pwr` example;
    the expected differences from KiCad's export; how to regenerate the
    reference exports (K04.8) and run the corpus (K07.6); the KiCad 11 notes
    from the header of this checklist.
  - `engine.md` module map: one line for `freerouting-kicad` and
    `io::read_design`.
  - **Accept:** both sections exist, and each fact in them names its test
    or checklist item.

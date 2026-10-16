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
([K08](../K08-kicad-pcb-writer.md)).

Prerequisites: [K06](K06-kicad-pcb-rule-areas-and-copper.md). K07.2 needs
[C07](../C00-cli-settings-io-README.md) (CLI argument handling, which owns
`crates/freerouting-cli`) and [G01](G01-session-and-test-harness.md) (the
GUI's `Session`); without them it is partial. K07.4 needs
[C05](../C00-cli-settings-io-README.md) (SES writer) for its last step and
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

- [x] **K07.1 — `freerouting::io::kicad` and `read_design`.**
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
  *Done 2026-10-16:* `crates/freerouting/src/io/kicad.rs` (`read_kicad_input`, `read_kicad_board`, `read_design`, `design_kind`, re-export of the reader), `Cargo.toml` (`freerouting-kicad`), `drc/runner.rs` (`run_drc` calls `read_design`; messages carry the read report and the reader's warnings), `job/prepare.rs` (`JobInput.pcb` and `read_log`: a design read by the caller replaces the DSN bytes). Tests `crates/freerouting/tests/kicad_read.rs` (7 pass): `dev_board_drc_equals_the_exports_board` and, through the binary, `crates/freerouting-cli/tests/cli_kicad.rs::dev_board_report_equals_the_dsn_export` (`-de dev-board.kicad_pcb -drc out.json`: unconnectedItems 45 = 45, violations 0 = 0, qualityScore 13.89 = 13.89 against the `.kicad10.dsn`); `a_missing_project_file_is_not_an_error` (`bm1.unrouted` has no `.kicad_pro`; the report says "No .kicad_pro next to the board"); `a_kicad_5_file_is_refused_in_one_line` and `cli_kicad::a_kicad_5_file_exits_1` (exit 1, one `error:` line, "unsupported KiCad board file version 20171130 (KiCad 5 or older); open and save the board in KiCad 6 or later"); `reader_warnings_are_merged_with_their_lines` (K06's `wire_keepout` warning reaches `BoardReadResult` as `line N: ...`: decided to surface it); `the_extension_picks_the_reader` (case ignored); `every_committed_board_opens` (15 boards). `design_rules_reach_the_board` builds `classes` with the synthetic `.kicad_dru`: the Fast x "Wide,Default" clearance entry is 5000 units (0.5 mm) with `apply_design_rules`, not without. K07.1 also gave `ReadOptions::apply_design_rules` (default off) and `KicadInput::report_lines` (K07.5/K07.2).

- [x] **K07.2 — CLI and GUI open `.kicad_pcb`.**
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
  *Done 2026-10-16:* CLI `-de board.kicad_pcb [-do out.ses | -drc]` through `read_kicad_input` (`cli.rs`; `args.rs` accepts `.kicad_pcb` in `-de` and in `a.kicad_pcb+b.ses`, a deliberate difference from Java's "Unknown file type", in DIVERGENCES; `help.txt`). The reader's warnings print as `warning: KiCad reader: line N: ...` and the read report as info lines before the job's log. `crates/freerouting-cli/tests/cli_kicad.rs` (5 pass): `drc_opens_every_committed_board` runs `-drc` on all 15 committed and synthetic boards; `route_job_writes_a_session` (`-do out.ses`: `(session "corney_island_wireless" (base_design "corney_island_wireless")`; KiCad 10.0.7's `ImportSpecctraSES` imports sessions of that form, K07.4); `warnings_reach_the_console` (upper-case `.KICAD_PCB`, `wire_keepout` line). GUI: `session::load` dispatches by extension (the reader's warnings and read report go to `Session::warnings`, which the warnings window shows), `find_design` also finds `<base_design>.kicad_pcb`, the file browser lists `.kicad_pcb`, the empty-state message names it; drag and drop sends `Command::Open(path)` for any file, so it takes the same path (not driven by a test: the harness has no file-drop injection). `crates/freerouting-gui/tests/ui_kicad.rs::opens_a_kicad_board`: `Issue368-CorneyIslandWireless` opens, `board.item_count()` = `build_board(reader tree).item_count()` (43), the report line is in the warnings; `file_dialog::tests::lists_kicad_boards`. `docs/development/gui.md` has the section.

- [x] **K07.3 — Parity with KiCad's export.**
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
  *Done 2026-10-16:* `crates/freerouting/tests/kicad_parity.rs` (4 tests pass on the 15 boards; the file also has the `--ignored` corpus tests of K07.6). Kinds compared: item counts and sequences (pin, trace, via, keepout, via_keepout, place_keepout, plane, component outline, board outline), nets (names, pin sets, classes), class rules and the clearance matrix by class name, every `BoardStatistics` field except `host`, DRC findings by type and the incomplete count, geometry by (kind, layers, nets, name) and nearest bounding box within `2 x half step of %.6g at the largest coordinate + 1` unit (3 to 11 units; the worst deviation seen was 5). `EXPECTED_DIFFERENCES` has 17 rows, each with its reason; `rule_area_and_plane_differences_are_exactly_the_documented_ones` checks the arithmetic behind the area rows on every board (export keepouts = reader keepouts + area-layers that ban neither tracks nor vias; reader place_keepouts = area-layers that ban footprints; planes differ by the kept footprint zones); `trace_differences_come_from_arcs`. **Per board:** `Test`: place_keepout 20/0, hence statistics total/other_count; `total_angled_length` 2412 vs 3273 (BoardStatistics sums `Line.a`-`Line.b`, which for the first and last line of a trace is a direction vector; the export's rounded corners make 45-degree segments inexact so the vector is long, 3 traces). `dev-board`: place_keepout 2/0. `rule-areas`: keepout 16/21 (the 5 area-layers that ban neither tracks nor vias are full keepouts in the export), place_keepout 7/0, plane 7/6 (a footprint-owned zone), zone islands 1/0, their statistics. `arcs`: traces differ (arcs followed: 812 segments and 763 bends against 49 and 0). `tastexx-pcb`, `bm1.unrouted`: statistics rounding only (< 2e-4 relative). `motorizedopener`, `min_fr_test`, `corney_island_wireless`, `backside`, `classes6`, `outline`, `pads`, `vias`: no difference. **Item order** (compared, printed, and asserted through the same table): the same order for every kind on every board except the net classes of `classes` (project order against KiCad's `unordered_map` order), the 5 outline holes of `tastexx-pcb` (first difference at keepout 42 of 50: another order and start vertex than Clipper's; the set equals the export's), and the sequences that the area and arc differences shift (`rule-areas`, `arcs`). **Negative check:** `removing_an_expected_difference_fails` removes each row in turn and requires unexplained differences (all 17 rows are needed); the test also fails on a row nothing uses.

- [x] **K07.4 — Coordinate frame round trip (K00 question 2).**
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
  *Done 2026-10-16:* Java `a773a70ec` (own worktree and `REFERENCE_WORK_DIR` under `target/orchestrator/k07/`; the cached classes of `target/java-reference` were from `cda540f`) routed KiCad 10.0.7's export of `Issue558-dev-board` (`-mp 100`, optimizer off): 0 unrouted, 7.5 s, 231 wires and 35 vias; kept as `fixtures/kicad/Issue558-dev-board-autoroute-demo/dev-board.kicad10.java.ses`. **Step 2** `crates/freerouting/tests/kicad_frame.rs::java_session_lands_at_the_same_coordinates`: `read_session` onto the reader's board and onto the export's board imports the same counts with 0 errors, the trace corners (layer, net, half width, corners) and via centres are equal sets (0 units apart), and both boards have 0 clearance violations, 0 incomplete connections, 0 unconnected items. `session_written_from_the_reader_board_has_the_same_routes`: the session the Rust writer produces from the reader's board has the Java session's wires and vias per net and the same placement (imported items are written back as `(type protect)`, as Java does). **Step 3** `tools/kicad-reader/frame-check.sh` + `frame_check.py` (KiCad 10.0.7, WSL, no KiCad process touched): `ImportSpecctraSES` of the Java session and of the Rust-written one into two copies of the board gives 494 tracks, arcs and vias each, largest endpoint distance 0.0 nm (tolerance 100 nm). **Answer to K00 question 2:** the frame is KiCad's: `(resolution um 10)` with micrometre values in the export; a session in resolution units lands on a 100 nm grid (`nm = 1000 v / 10`). What is lost is KiCad's nanometre to 0.1 um board units at the DSN read, at most 50 nm per coordinate; on this board every pad centre is on that grid and the largest distance of a track end to its pad (171 ends, probed with pcbnew after the import) is 0 nm. No finer resolution is justified until a board shows a connection lost to it. Not run: Windows KiCad 10.0.5 (`frame-check.sh --windows` exists; the task said to leave the Windows side alone).

- [x] **K07.5 — `.kicad_dru`.**
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
  *Done 2026-10-16:* `crates/freerouting-kicad/src/dru.rs` (parser on K04's lexer with `#` comments blanked, condition tokenizer and recursive-descent parser into `Cond`, evaluation over the board's class pairs, report types), `read.rs` (`read_with_rules`, `ReadOptions::apply_design_rules`, `KicadRead::rules`), `fixtures/kicad/synthetic/rules.kicad_dru`; `crates/freerouting-kicad/tests/dru.rs` (6 pass). `canary_pwr_rules_are_unsupported_by_name`: the ten rules of the real `canary-pwr.kicad_dru` are all UNSUPPORTED naming `insideCourtyard ( 'Q2' )` etc. (checked on the file when the corpus is present, on an excerpt otherwise). `supported_rules_change_the_tree_exactly`: with the option on, on the `classes` board, the tree gains exactly `class_class Fast x "Wide,Default"` with clearance 500 um and the Fast class width goes 100 to 300 um, nothing else differs; the `intersectsArea` rule is UNSUPPORTED naming its term. `option_off_leaves_the_tree_alone`: with the option off the tree and the warnings equal the board without the file, and the rules are reported (two ignored, one unsupported). Also `conditions_parse_into_trees`, `conditions_choose_class_pairs` (hasNetclass on aggregates, layer clause to a layer rule, no condition / unknown layer / hole_size are UNSUPPORTED), `a_broken_rules_file_is_a_warning`. Semantics chosen: `A.NetClass == 'x'` compares the effective class name exactly (an aggregate is `Fast,Wide`), `hasNetclass` also matches members; the Default class is `Default` in a condition and `kicad_default` in the DSN.

- [~] **K07.6 — Corpus run and timing.**
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
  *Partial 2026-10-16:* the remainder is the quiet-machine timing: the 1-minute load was 2.8 to 3.0 (other sessions were building), not <= 1.0, so the numbers below are upper bounds; re-run `corpus_timing` when the machine is idle. **Corpus run** (`kicad_parity.rs::corpus_summary`, `--ignored`, release; the K07.6 test lives beside K07.3's because it needs the engine, `FR_KICAD_ONLY` limits it, `FR_CORPUS_DIR` adds F03.3's corpus when present): 57 of 58 boards run (`TestSensel` is KiCad 5 and refused); the two boards over 20 MB skip the statistics and DRC. No board panics. 25 boards have no difference beyond the listed ones (or only statistics rounding): `microwave`, `sonde xilinx`, `motorizedopener`, `min_fr_test`, `corney`, `dev-board`, `tutorial_board`, all `bmN.unrouted`, `Issue269-NoViasOnPowerPlanes`, `ecc83` x2, `interf_u`, `pic_programmer`, `complex_hierarchy`, `kit-dev-coldfire`, `Mars-64_2-layer`, `Test`. **Differences the corpus shows, by cause:** (1) rule areas (K06.2): `canary-pwr` (export keepout fan inlet: 5 traces and 15 vias violate it, the reader's `place_keepout` none), `multichannel_mixer` x2, `One-Air-Max`, `jetson`, `vme-wren` (keepout counts, area/trace and area/via violations). (2) `%.6g` precision (K00 question 2): every routed board's incomplete/dangling counts, e.g. `bm1.routed` 11 vs 255; `export_rounding_explains_connectivity` rounds the reader's wires like the export and gets the export's counts exactly on `bm1`, `bm4`, `bm10`. (3) arcs (K06.5): `CM5_MINIMA_3`, `RoyalBlue54L-NFC-Antenna`. (4) net class order: `canary-pwr`, `CM5`, `jetson`, `tinytapeout`, `sample`, `vme-wren`, `CNH`. **Unexplained, new reader items:** (a) *fixed here:* `vme-wren` 32 nets got class `VMEPX` where KiCad says Default: the wildcard `*VMEPX.*` was also accepted as the regex `^*VMEPX.*$` by the `regex` crate (no start anchor); `classes.rs` now refuses a leading quantifier (`leading_star_pattern_is_a_wildcard_only`; the vme-wren `nets:class` row is gone after the fix). (b) The reader has 1 to 27 traces more than the export without arcs on `video` (7514/7513), `processor.Z80` (5990/5981), `proba` (1807/1800), `UltraFlactyl` (427/408), `Mars-64` (645/644), `bm9.routed` (375/373), `vme-wren` (24772/24745), `jetson` (unmatched traces): the segment merge differs from KiCad's in some cases (K06.5; not diagnosed). (c) `CNH_Functional_Tester_1` 808/804 and `Mini Auto Pilot` 453/452 component outlines: the reader emits courtyards the export drops (K05.5; not diagnosed). (d) `bm6`, `UltraFlactyl`, `Mini Auto Pilot`, `vme-wren`, `jetson` keepout order (hole or area order, like `tastexx-pcb`). (e) `One-Air-Max`: pin/via violations 11 vs 10. **Timing** (release, median of 3, ms; reader read = file read + parse + conversion; DSN parse = file read + parse of the 7 MB / 4 MB export): `jetson-agx-thor-baseboard` (85 MB) reader read 874, read + `build_board` 3698, DSN parse 35, parse + `build_board` 1636; `vme-wren` (70 MB) 257, 3930, 21, 1842. The reader's tree takes about 2x as long to build into a board; not investigated.

- [x] **K07.7 — Documentation.**
  - `kicad-plugin.md`, a new section "Direct `.kicad_pcb` input": what is
    read; the rule-area mapping table from K06 with the `canary-pwr` example;
    the expected differences from KiCad's export; how to regenerate the
    reference exports (K04.8) and run the corpus (K07.6); the KiCad 11 notes
    from the header of this checklist.
  - `engine.md` module map: one line for `freerouting-kicad` and
    `io::read_design`.
  - **Accept:** both sections exist, and each fact in them names its test
    or checklist item.
  *Done 2026-10-16:* `docs/development/kicad-plugin.md`, new last section "Direct `.kicad_pcb` input (K07)": what is read, the rule-area table with the `canary-pwr` example, the expected differences (each naming its test), `.kicad_dru`, how to regenerate the exports and run the corpus and the timing, the coordinate frame, the KiCad 11 notes; `docs/development/engine.md`: one row in the module map for `freerouting-kicad` and `io::read_design`; also `docs/development/gui.md` ("Opening .kicad_pcb"), `fixtures/kicad/README.md` and two `docs/DIVERGENCES.md` entries (kicad reader, CLI). Each fact names its test or checklist item.

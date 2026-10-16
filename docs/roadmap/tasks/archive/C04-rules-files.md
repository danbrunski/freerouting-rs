# C04 — Rules files: reader and writer

Opened 2026-10-15. A Specctra `.rules` file carries board rules (clearances,
widths, padstacks, via infos, via rules, net classes, snap angle) and
`autoroute_settings`. Java reads one in three cases:
- given with `-dr`;
- given as `-de a.dsn+a.rules`;
- found automatically next to the DSN as `<design>.rules`.

The `freerouting-dsn` crate parses rules files into `RulesFile`, but nothing
applies them. The DRC runner prints "RULES files are not supported yet".

Five fixtures have an adjacent rules file, so Java's route references for
them were made with it:
- `Issue029-hw48na`;
- `Issue107-freq_teiler_200kHz_kicad` and `Issue107-freq_teiler_200kHz_kicad_bad`;
- `Issue593-BBD_Mars-64`;
- `Issue742-tastexx-pcb/tastexx-pcb`.

The GUI's "save rules" (G08) needs the writer.

**Prerequisites:**
- C02, C03 (C03.6's rules hook);
- C01.1's `settings` dumps, C01.5's `rules` dumps and C01.6's `clirun` rows
  7–8.

## What exists and what this adds

| Exists | Where |
|---|---|
| Rules parser and plain writer | `freerouting_dsn::RulesFile::{parse, to_rules}`; `ast::RulesItem` (`SnapAngle`, `AutorouteSettings`, `Rule`, `Layer`, padstack, via, via rule, class), kept in file order |
| DSN-side helpers the rules reader reuses | `io/specctra/parser/{structure.rs, network.rs, library.rs}`: clearance rules, via infos, via rules, net-class insertion, padstack reading |
| `autoroute_settings` → patch | C02.7 (`settings/autoroute_scope.rs`) |
| DRC runner ignoring rules | `drc/runner.rs:87` (message), `docs/DIVERGENCES.md` ("Rules files … are not supported") |
| Java at `cda540f` | `io/specctra/RulesReader.java` (`read` :65, `applyRules` :280, `applyLayerRules` :308, `applyViaInfo` :340, `applyViaRule` :352, `applyNetClass` :359); `io/specctra/RulesWriter.java`; `io/specctra/parser/AutorouteSettings.writeScope`; `management/jobs/RoutingJobScheduler.java:115-157` (rules discovery); `cli/DrcRunner.java:57-72` (rules in DRC) |

What this adds: `io/specctra/rules_reader.rs` and `rules_writer.rs`, rules
discovery in `prepare_job`, and rules in the DRC runner.

## Decisions made here

- **Apply in file order through the DSN reader's own helpers.** The rules
  reader calls the functions the DSN reader already uses for clearance rules,
  via infos, via rules, net classes and padstacks. There is no second copy.
  Making them `pub(crate)` is the only edit to those files.
- **Java's quirks are kept and named in tests.**
  - A rules file's `autoroute_settings` override the merged settings,
    including the CLI. Because `AutorouteSettings.readScope` seeds every
    layer as routable and defaults `autoroute` and `postroute` to on, a
    rules file also re-enables all layers and the optimizer.
  - A `layer` scope naming an unknown layer is applied to **all** layers:
    `applyRules` keeps `layerIndex = -1`, which means "default".
  - A via info with an existing name is removed and appended again, so it
    moves to the end of the list.
  - The rules reader does not reinsert tree items after changing the
    clearance matrix.
- **The design name is checked, not enforced.** A header name that differs
  from the design name gives a warning and reading continues.
- **The writer emits the plain AST style.** That writes `snap_angle` and
  `via_rule`, which both Java's and Rust's readers accept. Java's
  `RulesWriter` writes `snapAngle` and `viaRule`, which its own reader skips,
  so a Java round trip loses the snap angle and via rules; this is already in
  DIVERGENCES (freerouting-dsn).
- **Trace costs are written as Java's `float` text.** Java writes
  `String.valueOf((float) cost)`. Rust stores
  `format!("{}", cost as f32).parse::<f64>()` in the AST. A reader of the
  Rust file then gets the same doubles as a reader of Java's file: `2.8`,
  not `2.799999952316284`.
- **Discovery order is Java's.**
  1. The job's own rules (`-de a.dsn+b.rules`).
  2. Otherwise `-dr`.
  3. Otherwise, only for a DSN input, `<file name up to the last '.'>.rules`
     in the DSN's directory.

  So `fixtures/Issue191-processor.Z80/processor.rules` is **not** picked up
  for `processor.Z80.dsn`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| C04.1 | `crates/freerouting/src/io/specctra/rules_reader.rs`, `crates/freerouting/tests/rules_reader.rs` | `io/specctra/mod.rs` (append exports); `io/specctra/parser/{structure.rs, network.rs, library.rs}` (visibility only) |
| C04.2 | — | `rules_reader.rs`, `tests/rules_reader.rs` |
| C04.3 | — | `rules_reader.rs`, `tests/rules_reader.rs` |
| C04.4 | — | `job/prepare.rs` (the rules hook and discovery only), `tests/job_prepare_reference.rs` (remove the C04 waiting list) |
| C04.5 | — | `drc/runner.rs`, `crates/freerouting/tests/reference_drc.rs` (append), `docs/DIVERGENCES.md` |
| C04.6 | `crates/freerouting/src/io/specctra/rules_writer.rs`, `crates/freerouting/tests/rules_writer.rs` | `io/specctra/mod.rs` (append) |

## Checklist

- [x] **C04.1 — Reader core: header, `rule`, `layer`.**
  Java: `RulesReader.read` and `applyRules`, `applyLayerRules`.
  Rust: `rules_reader::apply_rules_file(board: &mut BasicBoard, rules:
  &RulesFile, design_name: &str) -> RulesOutcome { autoroute:
  Option<SettingsPatch>, log: Vec<LogLine> }`.
  - Walk `rules.items` in order.
  - A width rule sets the default trace half-width,
    `java_round(dsn_to_board(w) / 2) as i32`, on every layer, or on one
    layer inside a `layer` scope.
  - A clearance rule goes through the same function the DSN reader uses
    for `(rule (clear …))`.
  - An unparsable file is an error. Java returns `false`.
  - **Accept:**
    - After `prepare_job` on `Issue029-hw48na.dsn` with its adjacent rules
      file, the clearance matrix and the default trace half-widths equal
      C01.1's `default` scenario dump.
    - `rules_layer_scope_with_unknown_layer_applies_to_all_layers` covers
      a synthetic rules file.
    - Port of `RulesRoundTripTest.invalidRulesFileReturnsFalse` (garbage
      gives an error, and the board is unchanged).

  *Done 2026-10-15:* `io/specctra/rules_reader.rs`. The signature is
  `apply_rules_file(board: RoutingBoard, rules: &RulesFile, design_name) ->
  (RoutingBoard, RulesOutcome)`: the DSN reader's helpers keep the board in
  their `ReadContext`, so the board goes in and comes back (`read_rules` does
  the same from text and returns `Result`). `RulesOutcome.log` holds
  `LogLine`s.
  - `tests/job_prepare_reference.rs::prepare_job_matches_java_dumps`: 676
    board scenarios compared with C01.1's dumps, 24 of them with an adjacent
    rules file (`Issue029-hw48na`, both `Issue107`, `Issue593-BBD_Mars-64`,
    `Issue742-tastexx-pcb/tastexx-pcb`, `explicit-cost-rules`, four scenarios
    each): clearance matrix, class names, default trace half widths per class
    and layer all equal.
  - `tests/rules_reader.rs::rules_layer_scope_with_unknown_layer_applies_to_all_layers`
    (and `..._applies_to_the_named_layer_only`, `width_rule_sets_...`,
    `clearance_rule_goes_through_the_dsn_helper`).
  - `invalid_rules_file_returns_an_error_and_leaves_the_board_unchanged`
    (`not a rules file` and a truncated file; the port parses first, so the
    board is unchanged also when Java would have applied the scopes before the
    error: DIVERGENCES, "Rules files").
  - `cargo test -p freerouting --test rules_reader`: 19 passed.

- [x] **C04.2 — Padstacks, via infos, via rules, net classes, snap angle.**
  Java:
  - `Library.readPadstackScope` puts padstacks into
    `board.library.padstacks`;
  - `applyViaInfo` removes a same-named info and appends the new one;
  - `applyViaRule` calls `Network.addViaRule`;
  - `applyNetClass` calls `Network.insertNetClass(…, false)`;
  - a snap angle sets `rules.trace_angle_restriction`.
  - **Accept:**
    - For the five adjacent-rules fixtures, the net classes, via infos (in
      order), via rules and snap angle equal C01.1's dump.
    - Port of
      `RulesRoundTripTest.rulesViaWithStrippedPadstackNameResolvesToDecimalPadstack`
      on `Issue143-rpi_splitter.dsn`: `(via "Round1$13")` resolves to the
      padstack `Round1$13.779528`.

  *Done 2026-10-15:* `rules_reader::apply_rules_file` applies the scopes in file
  order through the DSN reader's helpers (`parser/{network,library,structure}.rs`:
  `insert_rules_net_class`, `read_rules_via_info`, `replace_via_info`,
  `add_rules_via_rule`, `read_padstack_scope`, `snap_angle` made `pub(crate)`;
  `read_network` now builds its classes with the same `dsn_net_class`).
  - `job_prepare_reference.rs` compares, besides the matrix and classes, the via
    infos in order (name, padstack, clearance class, attach flag), the via
    rules, the snap angle, the padstack names and the via padstack names with
    C01.1's `board` block for the six rules boards and all four scenarios:
    equal (24 scenarios with rules, 676 compared in all).
  - `rules_reader.rs::rules_via_with_stripped_padstack_name_resolves_to_decimal_padstack`
    on `Issue143-rpi_splitter.dsn` (the Java test is weaker than its name: a
    `via` inside a `rule` scope is skipped; a `via` scope naming
    `Round1$13.779528` resolves it, tested too), plus tests for a via type that
    replaces an existing one and moves to the end, an unknown via padstack
    (Java's warning text), a padstack scope, via rules and a net class in
    file order, and the snap angle.
  - One fix in the DSN-side code: `Network.insertNetClass` logs
    `boardNetClass.getName()`; the port logged the class name of the file, which
    differs for `kicad_default` (Java: `default`).

- [x] **C04.3 — `autoroute_settings` on top of the merged settings.**
  Java: the `AUTOROUTE_SETTINGS` branch of `RulesReader.read`. It runs
  `AutorouteSettings.readScope` and then
  `targetSettings.applyNewValuesFrom(parsed)`.
  Rust: `RulesOutcome.autoroute` comes from C02.7's builder. `prepare_job`
  applies it with `SettingsPatch::apply_from` onto the merged patch, after
  the CLI. This is the quirk the series reproduces.
  - **Accept:**
    - **`rules_autoroute_settings_override_cli`.** On `Issue029-hw48na.dsn`
      with C01.1's `cli` scenario arguments (`--router.via_costs=77
      --router.optimizer.enabled=false …`), the final settings show the
      rules file's via costs and `optimizer.enabled = true`, with every
      layer routable. They equal the dump field by field.
    - Port of `RulesFileSettingsTest` through this path (C02.7 already
      checks the patch itself).
    - Port of `RulesRoundTripTest.rulesRoundTripWithAutorouteSettings`
      after C04.6: 75/8/120; L0 horizontal 1.2/2.8; L1 vertical 1.0/3.1,
      as exact doubles.

  *Done 2026-10-15:* `prepare_job` (step 5) reads the file, runs **the second
  merge** (the sources again, `Source::RulesFile` from `patch_from_rules` at
  priority 40, the first merge's patch as `Source::Api` at 70), applies the file
  to the board and puts `RulesOutcome.autoroute` on top with
  `SettingsPatch::apply_from`. The second merge is needed: see C04.4 and
  DIVERGENCES, "Rules files". The step-3 tuning has run before, so the
  `autoroute_settings` scope re-seeds `costs_initialized = false` and the step-6
  tuning runs again.
  - `rules_autoroute_settings_override_cli` (`tests/rules_reader.rs`): on
    `Issue029-hw48na.dsn` with C01.1's `cli` arguments the settings are
    `via_costs` 50, `plane_via_costs` 5, `start_ripup_costs` 100,
    `optimizer.enabled` true, every layer routable, while `max_passes` 7 and
    `default_undesired_direction_trace_cost` 1.5 stay from the CLI; without the
    rules file they are 77, 7 and the optimizer off. The same fields equal the
    dump's `cli` `settings`, and `job_prepare_reference.rs` compares the whole
    settings tree of the four scenarios.
  - Port of `RulesFileSettingsTest` through `apply_rules_file`:
    `rules_file_settings_of_processor_rules`, `rules_file_settings_of_hw48na_rules`
    (`priorityIs40` is Java source metadata: `Source::RulesFile`).
  - `rulesRoundTripWithAutorouteSettings` is `tests/rules_writer.rs::rules_round_trip_with_autoroute_settings`
    (C04.6): 75, 8, 120; L0 horizontal 1.2/2.8; L1 vertical 1.0/3.1, `assert_eq!`
    on the doubles.
  - `settings_reference.rs::settings_match_java_dumps` applies the rules file
    (second merge and `autoroute_patch`) and compares 484 board scenarios,
    20 of them with a rules file; the C04 waiting list is gone.

- [x] **C04.4 — Rules discovery in `prepare_job`; finish C03.6.**
  Java: `RoutingJobScheduler.java:115-157` and the `-de` `+` splitting in
  `GlobalSettings`. Implement `RulesChoice::{Explicit, Adjacent}` (see
  Decisions). Replace C03.6's "not supported" stub with
  `apply_rules_file`.
  - **Accept:**
    - `job_prepare_reference.rs` passes on the five adjacent-rules
      fixtures and the `explicit-cost-rules` case, in all four scenarios.
      Their entries leave the waiting list.
    - A unit test shows `processor.Z80.dsn` does not pick up
      `processor.rules`.
    - `explicit_layer_cost_suppresses_computed_costs` (C03.1) now passes
      for the `explicit-cost-rules` case too.
    - C03.7's `owner_config_reaches_router_view` second half passes, and
      its box is ticked.

  *Done 2026-10-15:* `RulesChoice::{None, Explicit, Adjacent}` and
  `find_rules_file` follow `RoutingJobScheduler.java:115-157`: a named file that
  does not exist is not read and the adjacent file is not tried.
  - `job_prepare_reference.rs`: `WAITING_ON_C04` is gone; the six boards pass in
    all four scenarios (24 scenarios, asserted), `explicit-cost-rules` included.
  - `adjacent_rules_use_the_file_name_up_to_the_last_dot`: `processor.Z80.dsn`
    in `fixtures/Issue191-processor.Z80/` does not pick up `processor.rules`;
    `an_explicit_rules_file_replaces_the_adjacent_one`,
    `an_explicit_rules_file_is_applied_by_prepare_job`.
  - `settings_board.rs::explicit_layer_cost_suppresses_computed_costs` now
    covers `explicit-cost-rules`: costs `[(2.0, 1.0), (2.0, 1.0), (1.0, 1.0),
    (1.0, 1.0)]`, bit-equal to the dump. **C01's observation is Java's
    behaviour and is matched:** the second merge's rules source reads the layer
    names of the rules file alone, so `In1.Cu`, the only layer the file names,
    is layer 0 (`F.Cu`) there; `RulesReader.read` then sets the real `In1.Cu`.
    The single-layer DSN scope has no such source, hence only `In1.Cu`.
  - `owner_config_reaches_router_view` second half: `Issue029-hw48na.dsn`
    with the `owner-config` scenario and its rules file gives
    `start_ripup_costs` 100, equal to the dump. C03.7 is ticked in
    `archive/C03-settings-on-board.md`.

- [~] **C04.5 — Rules in the DRC runner.**
  Java: `DrcRunner.java:57-72`. It reads the rules before the session, with
  `RulesReader.read(stream, designName, board, routerSettings)`. A missing
  file gives the warning `RULES file for DRC not found: <path>`.
  Rust: `run_drc` reads `options.rules_file`, applies it with
  `apply_rules_file` (the settings part is ignored for DRC) and logs Java's
  info lines. Remove the DIVERGENCES line "Rules files (`-dr`,
  `board.dsn+board.rules`) are not supported".
  - **Accept:**
    - For C01.6's rows 7 and 8 (`Issue029-hw48na.dsn` with `_valid.rules`
      and `_invalid.rules`), the Rust DRC report's violation counts by
      type equal Java's report from the `clirun` dump.
    - The log lines match Java's text.
    - `reference_drc.rs` gains these two cases.

  *Partial 2026-10-15:* `run_drc` applies `options.rules_file` before the
  session (`apply_rules` in `drc/runner.rs`): `Loading RULES file for DRC: <path>`,
  the reader's lines, `RULES file loaded for DRC successfully` (also after a
  failed read, as Java ignores the result), `RULES file for DRC not found: <path>`
  for a missing file; the design name is the DSN file name up to its last dot.
  The DIVERGENCES line "Rules files ... are not supported" is removed.
  - `tests/reference_drc.rs::drc_with_a_rules_file_matches_java`: rows 7 and 8
    (`Issue029-hw48na.dsn` with `_valid.rules` and `_invalid.rules`). The clirun
    dump keeps only the report's size, so the expected values come from a Java
    run at `cda540f` with the rows' arguments in a scratch directory and
    `java_drc_digest.py` over the two reports, committed as
    `tests/data/java_drc_rules_digest.tsv`: clearance 2, holeClearance 9,
    via_dangling 1, unconnectedItems 131, track_dangling 92 and the scores
    453.16 / 949.0 for both files (21, 80, 1, 131, 92 without the rules). The
    Rust report equals them: counts and hashes of the violations in report order.
  - The `RulesReader` and `Network.insert_net_class` log lines equal the dump's
    `logLines` for both rows, in order (the header warning prints
    `'/J801_5V'`, the DSN read's last scope identifier, as Java's static
    field). `a_missing_rules_file_is_a_warning` covers the missing file.
  - Remaining: for the invalid file Java's scanner also prints six
    `DsnFile.read_string_scope: closing bracket expected at 'Via[0-1]_1541.8:1186.18_um', got: EXTERNAL`
    lines between the class messages while it recovers from `(via_rule 1A
    EXTERNAL 1oz)`. The port's parser skips the extra words with warnings of its
    own. Reproducing the lines needs the `freerouting-dsn` class AST to carry the
    first ignored word of a `via_rule` scope (a change to a crate that the
    concurrent DSN writer, C06, builds on), so it is left; the test leaves those
    lines out of the comparison. DIVERGENCES, "Rules files".

- [~] **C04.6 — Writer: board and settings → `RulesFile`.**
  Java: `RulesWriter.write(board, settings, out, name)`:
  - `rules PCB <name>` (the name is written raw, never quoted);
  - the snap angle;
  - `autoroute_settings` when settings are given
    (`AutorouteSettings.writeScope`): `autoroute`, `postroute`, `vias`,
    `via_costs`, `plane_via_costs`, `start_ripup_costs`, then per layer
    `active`, `preferred_direction` and the two costs as `float`;
  - the default rule for layer 0 (`Rule.writeDefaultRule`);
  - library padstacks 1..n that are via padstacks
    (`Library.writePadstackScope`);
  - via infos, via rules, net classes (`Network.writeViaInfos`,
    `writeViaRules`, `writeNetClasses`).

  Rust: `rules_writer::rules_from_board(board, settings: Option<&RouterSettings>,
  name) -> RulesFile`, then `to_rules()`.
  - **Accept:**
    - Port of `RulesRoundTripTest.rulesRoundTrip`: the output contains
      `(rules PCB` and `(rule`, and reads back without error.
    - **Round trip on every fixture.** Write the rules of the board
      prepared in the `default` scenario. Apply them to a fresh
      `prepare_job` of the same DSN with no rules. The clearance matrix,
      net classes, via infos, via rules, snap angle and resolved settings
      equal the original.
    - **Against Java.** For every C01.5 dump, rewrite `(snapAngle` →
      `(snap_angle` and `(viaRule` → `(via_rule` in Java's text (its
      writer bug, noted in the test). Parse it, and compare it with
      Rust's `RulesFile` item by item: same order, same values.

  *Partial 2026-10-15:* `io/specctra/rules_writer.rs`: `rules_from_board(board,
  settings, name) -> RulesFile` (and `write_rules`), the pieces `via_infos`,
  `via_rules`, `net_classes`, `padstack_scope` and `board_shape_to_dsn`
  `pub(crate)` for the DSN writer (C06; C05's `ses_writer.rs` has its own private
  copy of the shape conversion, with integer rounding).
  - `rules_round_trip` and `rules_round_trip_with_autoroute_settings`
    (`tests/rules_writer.rs`): ports of the two Java tests, the second with
    `assert_eq!` on 75/8/120 and the exact doubles 1.2/2.8/1.0/3.1.
  - **Against Java:** `rules_match_javas_dumps` compares the tree of each of the
    164 `fixtures/reference/rules` dumps (`snapAngle` and `viaRule` renamed) with
    `rules_from_board` on the `default`-scenario board, item by item (same order,
    same values, `autoroute_settings` included): 159 equal; five listed in
    `EXPECTED_MISMATCHES` with reasons (two design names with blanks, which Java
    writes raw; the `''` class names of `Issue684` and `Issue721`; a via type
    with an empty clearance class in `Issue179`, whose Java text the port's parser
    rejects).
  - **Round trip:** `rules_round_trip_on_every_fixture` (164 boards) writes the
    rules of the prepared board, applies them to a fresh `prepare_job` without
    rules and compares what the file expresses: via types in order, via rules by
    name, each net class by name, snap angle, pin edge to turn distance, the
    class names and the default clearance of every layer, and the resolved
    settings (costs as `float`). 159 boards equal; five listed
    (`ROUND_TRIP_MISMATCHES`: classes created by pair rules in an adjacent file,
    and an empty class name overriding the default clearance).
  - Remaining: the clause "the clearance matrix ... equal the original" cannot
    hold with Java's format, which the port reproduces. The file has one
    `(clearance v (type name))` per class, and Java's reader skips a type without
    an underscore and splits `a_b` into the pair `a` and `b` (`MIN_EXTERN_188A`
    reads as `MIN` and `EXTERN_188A`, two new classes); reading starts by setting
    every cell to the default. So named-class and off-diagonal clearances, and
    the clearances of layers other than 0, do not come back. Operator decision
    below: accept the narrower property the test pins (DIVERGENCES, "Rules
    files").

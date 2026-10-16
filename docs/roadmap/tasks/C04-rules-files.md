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

- [ ] **C04.1 — Reader core: header, `rule`, `layer`.**
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

- [ ] **C04.2 — Padstacks, via infos, via rules, net classes, snap angle.**
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

- [ ] **C04.3 — `autoroute_settings` on top of the merged settings.**
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

- [ ] **C04.4 — Rules discovery in `prepare_job`; finish C03.6.**
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

- [ ] **C04.5 — Rules in the DRC runner.**
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

- [ ] **C04.6 — Writer: board and settings → `RulesFile`.**
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

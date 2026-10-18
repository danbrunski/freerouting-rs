# C07 — CLI parity

Opened 2026-10-15. Today `freerouting` (`crates/freerouting-cli`) understands
`-de`, `-drc` and `-dr` only. Anything except a DRC run exits 2 with "routing
is not implemented yet". The KiCad plugin, the owner's scripts and the Java fork's
`compare.py` need the Java command line:
- the same flags and the same settings they set;
- the same output files and exit codes;
- the log lines tools parse.

This checklist ports that contract, with two deliberate fixes. Outputs are
never 0 bytes, and wall-clock limits only stop work. The routing itself plugs
in at C07.8 when the A series' pipeline lands. Until then the CLI runs the
whole flow with routing disabled, so outputs, exit codes and settings can be
built and tested now.

**Prerequisites:**
- C02 (settings and the path setter), C03 (`prepare_job`), C05 (SES output);
- C04 for `-dr` and `+rules`, C06 for `.dsn` outputs. Until those land, the
  missing format is an error;
- C01.2's `argv` and C01.6's `clirun` dumps;
- A08's two-level cancel token for C07.5, and A13's `run_pipeline` for C07.8;
- F02.4 for C07.8's determinism check.

## What exists and what this adds

| Exists | Where |
|---|---|
| Partial argument parser and DRC dispatch | `crates/freerouting-cli/src/main.rs` (`parse_args`, `parses_drc_arguments`) |
| DRC run and report | `freerouting::drc::{run_drc, DrcRunOptions}` |
| The Rust plugin's call | `kicad-plugin/freerouting_rs/tools.py` `start_route`: `-de x.dsn -do x.ses`, stdout and stderr merged, `stdin=DEVNULL`. Failure is a non-zero code or a missing SES |
| The Java plugin's call | `integrations/KiCad/kicad-freerouting/plugins/router_dsn.py` `_build_command` at the pin: `-de in.dsn -do out.ses -host <host> --gui.enabled=<b> --api_server.enabled=false --mcp_server.enabled=false --logging.file.location=<dir>` |
| Java at `cda540f` | `settings/GlobalSettings.applyCommandLineArguments` (:579-960; `-de` :646, `-do` :737, `-drc` :767, `-dr` :777, `-mp` :782 clamped to 0..9999, `-mt` :797, `-oit` :809, `-inc` :927; `isMultiFileConcatenation` with `DE_VALID_EXTENSIONS`/`DO_VALID_EXTENSIONS`); `settings/sources/CliSettings` (implicit router-on for `-de`+`-do`, short-flag map `mp`→`router.max_passes`, `mt`→`router.max_threads`, `inc`, `oit`); `cli/CliRunner` (`initializeCli`, `writeCliOutputIfAvailable` :208, `computeCliExitCode` :286); `Freerouting.main` (:508-742; CLI dispatch :701-721); `management/jobs/RoutingJobSchedulerActionThread` (timeout monitor :47-92, stage summary line :150-180, `assignTerminalState`); `autoroute/pipeline/AutorouteBatchLoop.java:416` (pass line); `logger/FRLogger.formatScore` (:78) |
| The Java fork's `compare.py` (at `main`, not the pin) | `docs/development/tools/compare.py:56`: `Auto-routing stage completed: started with (\d+) unrouted.*?final score: [\d.]+ \((\d+) unrouted and (\d+) violations\)` |

What this adds:
- in `crates/freerouting-cli/src/`: `args.rs` and `log.rs`;
- in `crates/freerouting/src/job/`: `run.rs` and `outputs.rs`. The job flow
  lives in the engine (D7), so the GUI's front-end mode (G08.4) reuses it.

## Decisions made here

- **One parser with Java's semantics.** Java runs two parsers on the same
  arguments: `GlobalSettings` for files and app settings, and `CliSettings`
  for router settings. Rust has one parser that yields both: the file lists
  and a CLI `SettingsPatch`, built through C02.3's `cli_property`. Java logs
  some warnings twice, once per parser. Rust logs each once; logs are not
  results.
- **Java's quirks that change results are reproduced and named in tests.**
  - **`-mt` does nothing in a headless run.** It sets the flat
    `max_threads`, which no stage reads. `optimizer.max_threads` is
    unchanged. Rust adds an info line naming
    `--router.optimizer.max_threads`. Thread counts never change results
    anyway (D2).
  - `-mp` clamps to 0..9999. Validation turns values above 9999 into 0,
    which means unlimited.
  - `-de` together with `-do` switches the router on unless
    `--router.enabled` or `--router.autorouter.enabled` is given.
  - `-inc` splits on commas.
- **One addition to `-inc`.** A backslash keeps a comma:
  `-inc 'Power\,Default'` names the class `Power,Default`, which KiCad
  writes (it appears in `Issue558-dev-board.dsn`, `Issue575-…`,
  `Issue754-…`). Java can't express that name. Its `-inc Power,Default`
  looks for the classes `Power` and `Default`. On `Issue558-dev-board.dsn`
  (classes `Power,Default`, `kicad_default`, and Java's built-in
  `default`) that flags the built-in `default` class, by its
  case-insensitive match. That class holds only nets no DSN class lists.
  The power nets are left to the router.
  When the plain split's parts joined back with `,` match an existing class,
  Rust warns. It still follows Java's split. Gets a DIVERGENCES line.
- **No 0-byte outputs.**
  - Java deletes an existing primary output at the start. Rust does too,
    so a failed run can't leave a stale file behind.
  - Every output is written to `.<name>.<pid>.tmp` in the target directory
    and renamed into place, so a killed process leaves no partial file.
  - A serializer result of 0 bytes is an error, never a file.
  - Formats:
    - `.ses` (C05), `.dsn` (C06), `.rules` (C04) and `.drc` or
      `.drc.json` (the DRC report) are written;
    - `.frb`, `.scr`, `.json`, `.kicad_pcb` and anything else are rejected
      **before** any work, with an error and exit 1. C08 makes that exit 2.
  - DIVERGENCES: Java writes a 0-byte file for formats it can't produce
    headless.
- **Timeouts only stop work (D2).**
  - `job_timeout` (default `12:00:00`, capped at 24 h), `fanout.timeout` and
    `optimizer.timeout` are handed to a deadline watcher. When one fires,
    it calls A08's stop-all.
  - The run ends at the next stage check. The best board is written, the
    state is `TimedOut`, and the exit code is 0, as Java's TIMED_OUT. The
    line `warning: stopped by wall-clock limit; result is timing-dependent`
    is logged.
  - Work bounds (`-mp`, `max_items`, the fanout and optimizer bounds) go
    to the pipeline as C02's `WorkBounds` and are the only bounds tests
    use.
- **Exit codes in C07 are Java's.** 0 when the job is Completed or TimedOut
  and at least one output was written with more than 0 bytes; otherwise 1.
  `-help` exits 0. C08 adds 2 for usage errors and 130 for aborts.
- **Log lines.**
  - Messages go to stderr with no timestamp, so two runs diff cleanly
    apart from times. Warnings are prefixed `warning: ` and errors
    `error: `.
  - Java's message texts are kept for the lines tools parse:
    - the load line;
    - the pass line `Auto-routing pass #N on board '<hash>' was completed
      in X.XX seconds with score S (U unrouted and V violations)`, with B01's
      state hash as `<hash>`;
    - the stage summary `Auto-routing stage completed: started with N
      unrouted nets, completed in X.XX seconds, final score: S (U unrouted
      and V violations).`, cut after the score because Rust has no JVM CPU
      or heap figures;
    - `Saving output file '<abs path>' (SES)...` and `Successfully saved
      output file '<abs path>' (<n> bytes).`
  - Scores are formatted like `DecimalFormat("0.00")`, which rounds
    HALF_EVEN, and `formatScore` writes `1 violation` in the singular.
    `compare.py`'s regex therefore misses a 1-violation board; that is
    Java's behaviour, kept.

## File ownership

| Item | New files | May edit |
|---|---|---|
| C07.1 | `crates/freerouting-cli/src/args.rs`, `crates/freerouting-cli/tests/args_java.rs` | `crates/freerouting-cli/src/main.rs` |
| C07.2 | — | `args.rs`, `tests/args_java.rs` |
| C07.3 | `crates/freerouting-cli/src/help.txt` | `args.rs`, `main.rs` |
| C07.4 | `crates/freerouting/src/job/{run.rs, outputs.rs}`, `crates/freerouting-cli/tests/cli_outputs.rs` | `crates/freerouting/src/job/mod.rs`, `main.rs`, `docs/DIVERGENCES.md` (append) |
| C07.5 | `crates/freerouting/src/job/deadline.rs` | `job/run.rs`, `tests/cli_outputs.rs` |
| C07.6 | `crates/freerouting-cli/src/log.rs`, `crates/freerouting/src/job/log.rs` | `job/run.rs`, `main.rs` |
| C07.7 | `crates/freerouting-cli/tests/settings_parity.rs` | — |
| C07.8 | `crates/freerouting-cli/tests/cli_route.rs` | `job/run.rs` |

C06.8 may append the DSN format to `job/outputs.rs`. A08 owns the cancel
token and A13 owns `run_pipeline`; C07 only calls them.

## Checklist

- [x] **C07.1 — Argument parser: files and modes.**
  Java: `GlobalSettings.applyCommandLineArguments`.
  - Matching is by prefix (`startsWith("-de")`).
  - A value is taken only when the next argument doesn't start with `-`.
  - `-de` takes arguments until the next `-…`. An argument that is an
    existing path is used whole. Otherwise it is split on `+` when
    `isMultiFileConcatenation` holds (every part ends in a valid
    extension: `.dsn .json .ses .rules`).
  - Files are sorted by extension, ignoring case, and the last DSN wins,
    with Java's warnings. `.json` is an input before a DSN and a session
    after one; Rust rejects JSON inputs (KiCad JSON is not ported).
  - `-do`: the first file is the primary output; the rest, and `+` parts,
    are extra outputs. Valid `-do` extensions are `.ses .dsn .scr .rules
    .json .drc .frb .kicad_pcb`.
  - `-drc [file]` is DRC mode. Without a file Rust keeps its current
    behaviour (the report on stdout). That is already in DIVERGENCES; Java
    falls through to routing.
  - `-dr file` names the rules file.
  - **Accept:**
    - `tests/args_java.rs` ports every case of
      `GlobalSettingsCommandLineTest`'s `-de`/`-do` part and
      `GlobalSettingsTest`'s file cases:
      - `+` splitting by extension;
      - names with spaces;
      - names containing `+` that aren't split (`mechkeys_MF68+10--…ses`,
        `perf++`, `board.v1+final.dsn`);
      - the last DSN winning;
      - a bare `-de`.
    - For every C01.2 `argv.json` entry, the file fields equal Java's.
    - `parses_drc_arguments` still passes.

  *Done 2026-10-16:* `crates/freerouting-cli/src/args.rs` (one walk with Java's prefix order, `FLAGS` table; the crate now has a lib target so tests can call it) and `tests/args_java.rs`: every `-de`/`-do` case of `GlobalSettingsCommandLineTest` and the argument cases of `GlobalSettingsTest` are ported by name (`+` splitting, spaces, `mechkeys_MF68+10--…ses`, `perf++`, `board.v1+final.dsn`, last DSN wins with Java's warning, bare `-de`, an existing path with `+` used whole). `argv_json_file_fields_equal_java` and `argv_json_rejection_warnings_equal_java` compare all 126 `fixtures/reference/argv.json` entries (file fields, help flag, DRC report file; the rejection warnings). `parses_drc_arguments` moved to `args.rs` unit tests and passes. `cargo test -p freerouting-cli --test args_java`: 48 passed. Files are classified in argument order as Java does (the checklist's "sorted by extension" is not what Java does; the dumps agree with argument order).

- [x] **C07.2 — Settings flags into the CLI patch.**
  Java: `CliSettings.parseArguments` and `applyRouterSetting`, plus
  `mapFlagToProperty`.
  - Short flags: `-mp` → `router.max_passes`, `-mt` → `router.max_threads`
    (flat), `-inc` → `router.autorouter.ignore_net_classes`, `-oit` →
    `router.optimizer.improvement_threshold`. `-oit` and `-inc` warn as
    deprecated.
  - `--router.<path>=v`, `--optimizer.scoring.version=v`,
    `--scoring-version=`, `--router-scoring-version=` and
    `--optimizer-scoring-version=` all go through C02.3.
  - The implicit `autorouter.enabled = true` for `-de`+`-do` uses exact
    `-de`/`-do` flags, as `CliSettings` does.
  - `--settings FILE` and `--user_data_path=DIR` choose the config file
    (C02.5).
  - **Accept:**
    - Ports of `SettingsMergerTest`'s legacy cases:
      `legacyBatchModeEnablesRouterWhenJsonDisablesIt`,
      `explicitRouterEnabledFalseOverridesLegacyBatchMode`,
      `cliRoutableLayersDoesNotDisableRouter`, `cliCanDisableRouterAndOptimizer`.
    - Port of `Issue885SettingsInitializationTest`'s `-inc POWER,GND` case.
    - `mt_does_nothing_headless`: `-mt 3` leaves
      `optimizer.max_threads` and `autorouter.max_threads` at their
      defaults and sets only the flat field.
    - `inc_escaped_comma_names_one_class`: `-inc 'Power\,Default'` gives
      `["Power,Default"]`. `-inc Power,Default` on `Issue558-dev-board.dsn`
      gives Java's split plus the warning.
    - `mp_10000_is_unlimited`.
    - For every C01.2 entry, the CLI patch equals Java's
      `CliSettings.getSettings()` non-null fields.
    - C02.8's `cli` scenario test switches to this parser and still
      passes.

  *Done 2026-10-16:* `args::parse` hands the router settings to the engine's `parse_cli_arguments` (CliSettings port) and adds the `\,` escape for `-inc` (`apply_escaped_inc`). `tests/args_java.rs`: `legacy_batch_mode_enables_router_when_json_disables_it`, `explicit_router_enabled_false_overrides_legacy_batch_mode`, `cli_routable_layers_does_not_disable_router`, `cli_can_disable_router_and_optimizer`, `cli_settings_parses_inc_flag_into_ignore_net_classes`, `mt_does_nothing_headless`, `inc_escaped_comma_names_one_class`, `mp_10000_is_unlimited` (patch 10000, resolved 0 with Java's validation warning), `settings_flags_choose_the_config_file`. `argv_json_cli_patch_equals_java` compares the CLI patch (`dump_patch`) and the parsed arguments with `cliSettings` for all 126 entries. The `-inc Power,Default` warning on `Issue558-dev-board.dsn` is `comma_split_warns_when_it_looks_like_a_mistake` (`cli_outputs.rs`). C02.8's `cli` scenario: `settings_reference.rs` still passes unchanged on the engine's parser, and `the_cli_scenario_patch_is_the_engines` (`settings_parity.rs`) shows `args::parse` gives the same patch and parsed arguments for that scenario's arguments (the engine test cannot call the CLI crate).

- [x] **C07.3 — Flags accepted and ignored; help.**
  - Accepted with a debug log and no effect: `-host`, `-di`, `-l`, `-dct`,
    `-da`, `-us`, `-is`, `-dl`, and any `--gui.*`, `--api_server.*`,
    `--mcp_server.*`, `--usage_and_diagnostic_data.*`,
    `--feature_flags.*`, `--profile.*` or `--debug.*`.
  - `-ll` and `--logging.console.level` set the console level;
    `--logging.file.*` is accepted and ignored.
  - `-im`, `-hr`, unknown `-x` and `--x` without `=` warn
    `Unknown command line argument: <arg>`, as Java does.
  - Java's two headless utility commands are not ported, each with a
    DIVERGENCES line: `--compare-boards=<a>,<b>` (`cli/BenchmarkAndCompareCommands`
    over `board/state/BoardComparator.java`, 758 lines: units, bounding box, layers,
    items and nets of two boards with a tolerance) and
    `--calculate-benchmark-scores --input=<f> --output=<f>`. Both exit 2 with
    `not supported; use freerouting-dev ses-diff` (F02.2). A board-level
    comparison beyond `ses-diff` is added only if a P or Q item needs it.
  - `-help`, `--help` and `-h` print `help.txt` and exit 0. The help lists
    every flag in `args.rs` with its settings path.
  - **Accept:**
    - `java_plugin_command_line_parses_cleanly`: the Java plugin's exact
      argument list (above), with a host string containing spaces and
      quotes, parses with zero warnings and gives the same inputs, outputs
      and patch as `-de in.dsn -do out.ses` alone.
    - `GlobalSettingsTest`'s unknown-flag cases each give exactly one
      warning.
    - `freerouting -help` exits 0.

  *Done 2026-10-16:* `FLAGS` and `LONG_FLAGS` in `args.rs`, `src/help.txt` (Java's usage plus every flag with its settings path). `accepted_and_ignored_flags` (debug log only: `-host -di -l -dct -dl -da -us -is`, `--gui.* --api_server.* --mcp_server.* --usage_and_diagnostic_data.* --feature_flags.* --profile.* --debug.* --logging.*`, `--user_data_path`, `--settings`), `unknown_flags_each_give_exactly_one_warning` (`-im`, `-hr`, `-x`, `--x`, `--`, `--=x`, `--a=b=c`), `-ll`/`--logging.console.level` set the console level. `--compare-boards` and `--calculate-benchmark-scores` are recognized and exit 2 with `not supported; use freerouting-dev ses-diff` (`refused_java_commands_exit_2`, DIVERGENCES). `java_plugin_command_line_parses_cleanly`: the plugin's exact arguments with a host containing spaces and quotes give no warning and the same inputs, outputs and patch as `-de in.dsn -do out.ses`. `freerouting -help` exits 0 (`no_zero_byte_outputs`, row 9).

- [x] **C07.4 — Job flow and outputs, never 0 bytes.**
  Java: `CliRunner.initializeCli` and `writeCliOutputIfAvailable`,
  `RoutingJobSchedulerActionThread.setJobOutput`, `MultiOutputGenerator`
  (SES, DRC JSON).
  - `job::run::run_job(input: JobInput, outputs: &[OutputSpec], ctx:
    &JobContext) -> JobOutcome { state: Completed | TimedOut | Cancelled |
    Terminated, written: Vec<(PathBuf, u64)>, log }`.
  - It runs `prepare_job` (C03.6), then the pipeline, then writes the
    outputs from the final board. The pipeline is a stub that leaves the
    board as it is until C07.8.
  - `job::outputs` holds the format table and the atomic writer (see
    Decisions).
  - The exit code follows `computeCliExitCode`, with "written" meaning
    more than 0 bytes.
  - "No routable layer" (`AutorouteBatchLoop`) is `Terminated` and exit 1.
  - **Accept:**
    - `no_zero_byte_outputs`: for every C01.6 row, Rust's exit code and
      output files are compared with Java's. Rows 1, 3, 5, 6 and 9 match.
      Row 2 (`.dsn`) writes a real DSN once C06 is in, and is an error
      before that. Row 4 (`.xyz`) is rejected before any work. Each
      deliberate difference has its DIVERGENCES line.
    - `existing_output_is_deleted_at_start`.
    - `killed_run_leaves_no_partial_file`: a test hook stops the process
      between the temp write and the rename. The target is absent; the
      temp file is removed at the next start.

  *Done 2026-10-16:* `freerouting::job::run::{run_job, JobContext, JobOutcome, JobState}`, `job::outputs` (format table, `write_atomic`, stale-temp cleanup; a `BufWriter` wraps the temporary file, as C06.8 asked), `cli.rs` (reads files, calls `run_job`). `.dsn` is registered (`do_dsn_writes_the_board`, C06.8's test), also `.rules` and `.drc`/`.drc.json`. `tests/cli_outputs.rs` (17 pass): `no_zero_byte_outputs` runs C01.6 rows 1, 2, 3, 4, 5, 6 and 9 (row 1 and row 3's SES byte-identical to Java's; row 5 and 6 exit 1 with Java's messages; row 4 refused before any work with nothing written; row 3's `.drc.json` is a DRC report, Java wrote a KiCad design JSON, DIVERGENCES). Row 2 is asserted at the new pin: `a773a70ec` (`7bef8995f`) writes a DSN and exits 0, as Rust does (the committed dump is still `cda540f`'s exit 1 and 0 bytes; the test accepts the regenerated dump and does not hard-code the old row). `existing_output_is_deleted_at_start`, `killed_run_leaves_no_partial_file` (env hook `FREEROUTING_TEST_ABORT_BEFORE_RENAME` aborts between write and rename; the target is absent, one `.out.ses.<pid>.tmp` is left, the next run removes it), `unsupported_output_formats_are_refused_before_any_work`, `no_routable_layer_terminates_the_job` (exit 1), `a_zero_pass_run_completes`. Engine unit tests: `exit_code_follows_the_state_and_the_written_files`, format table, atomic writer. The CLI uses `ResolveContext::from_machine()` (cores only set default thread counts, D2).

- [x] **C07.5 — Deadlines stop work; the outcome mapping.**
  Java: `RoutingJobSchedulerActionThread.threadAction`. It has a monitor
  that polls once a second, calls `requestStop()` at the deadline and
  allows a 30 s grace period. It also has `assignTerminalState`.
  - Rust: `job::deadline::Watcher`. One thread sleeps until the earliest
    deadline and then calls A08's stop-all. There is no polling and no
    grace period, because Rust stages check the token between steps.
  - Outcome mapping, ported from `RoutingJobSchedulerActionThreadTest`:
    - a stop after the deadline is TimedOut;
    - a stop before it is Completed;
    - a user cancel beats an expired deadline;
    - a Completed job is never downgraded;
    - an engine error is Terminated.
  - **Accept:**
    - A table test of the mapping, with each Java test method named.
    - `timeouts_only_stop_work`, before C07.8: with `job_timeout=24:00:00`
      and with none, the SES is byte-identical. With `job_timeout=00:00:00`
      on the stub pipeline: exit 0, a non-empty SES and the warning line.
    - C07.8 repeats this with routing.

  *Done 2026-10-16:* `job::deadline::{Watcher, StopLevel, assign_terminal_state, mark_timed_out_if_still_active, terminate_on_error}`: one thread waits on a condvar for the earliest armed deadline and calls A08's `CancelToken` (`stop_all`, or `stop_router` for a router-level limit); no polling, no grace; a zero limit stops before `arm` returns. The job limit is armed by `run_job`; the fanout and optimizer limits are stage-relative, so the pipeline arms them at stage start (`Watcher::arm`, C07.8; the stub has no stages). `terminal_state_mapping_table` names each `RoutingJobSchedulerActionThreadTest` method (`jobClockStopBecomesTimedOut`, `stopBeforeTheJobClockCompletes`, `userCancelWinsOverTheExpiredJobClock`, ...) and `monitor_and_error_paths` the rest (`finishedJobStaysCompletedWhenTheMonitorAssignsTimeout`, `monitorTimeoutIsKeptWhenThePipelineFinishes`, `testFatalErrorSetsJobStateToTerminated`); watcher tests for earliest-fires, disarm, drop. `timeouts_only_stop_work` (stub): `job_timeout=24:00:00` and none give byte-identical SES; `00:00:00` exits 0 with a non-empty SES, the warning line `warning: stopped by wall-clock limit; result is timing-dependent` and state TIMED_OUT. C07.8 repeats it with routing.

- [x] **C07.6 — Log lines tools parse.**
  `job::log::LogLine { level, text }`, collected by the engine;
  `freerouting-cli/src/log.rs` prints them with the console level filter.
  The texts are those listed in Decisions.
  - `format_score(score: f32, unrouted: u32, violations: u32)` matches
    `FRLogger.formatScore`: `DecimalFormat("0.00")`, HALF_EVEN, on the
    float widened to double; `violation` in the singular for 1.
  - **Accept:**
    - A `format_score` table, with tie cases such as `0.125 → "0.12"` and
      `0.375 → "0.38"`, and 0, 1 and 2 violations.
    - `compare_py_regex_matches`: the stub run's stage summary matches
      `compare.py`'s regex, copied into the test. A 1-violation line does
      not match, as in Java.

  *Done 2026-10-16:* `job::log` (`format_score`, `decimal_format_0_00`, `format_duration`, pass, stage-summary and save lines) and `freerouting-cli/src/log.rs` (stderr, no timestamps, `warning: `/`error: ` prefixes, console level filter). `format_score_table` covers `0.125 → 0.12`, `0.375 → 0.38`, 0/1/2 violations (singular for 1), negative zero, non-finite values. `compare_py_regex_matches` (`cli_outputs.rs`, regex hand-written because the workspace has no regex crate): the stub run's stage summary matches (1 unrouted, 0 violations on `Issue413-test.dsn`), a 1-violation line does not, as in Java. The summary stops after the score. The stub logs the stage summary only when the router is enabled, with a warning that routing is not implemented; pass lines wait for C07.8. Every run also logs `Effective start_ripup_costs: N (source: …)` (C00 open question 1; `the_settings_file_and_its_source_are_logged`).

- [x] **C07.7 — Settings parity gates (KRT's rule).**
  Mechanical tests, each with a negative control that makes it fail:
  - every flag `args.rs` accepts maps to a `FIELDS` path, or appears in a
    `CLI_ONLY` list with a one-line reason (`-de`, `-do`, `-drc`, `-dr`,
    `--settings`, `-help`, the accept-and-ignore flags);
  - no default in `help.txt` or `args.rs` differs from `FIELDS`;
  - `main.rs` holds no behaviour. Its function bodies call only
    `args::parse`, `job::run::run_job`, `drc::run_drc` and `log::print`,
    checked by a source scan;
  - the CLI path and a direct `prepare_job` call with the same patch give
    equal `settings::dump` output.

  The GUI half of the gate, mapping each settings form control to a
  `FIELDS` path, is G06/G07's.
  - **Accept:** `settings_parity.rs` passes. Each negative control (an
    unmapped test flag, a changed default, a stray call in a copy of
    `main.rs`) fails it, as shown in the done note.

  *Done 2026-10-16:* `tests/settings_parity.rs` (10 pass): `every_flag_maps_to_a_fields_path_or_is_cli_only`, `mapped_flags_set_their_fields`, `every_field_is_reachable_from_the_command_line` (every `FIELDS` path with a default via `--router.<path>=`), `no_default_in_help_or_args_differs_from_fields` (the `Setting: <path> [default: v]` lines of `help.txt` against `FIELDS`; no inlined `unwrap_or(<number>)` in `args.rs` or `cli.rs`), `main_rs_holds_no_behaviour` (its calls are `main`, `args`, `skip`, `collect`, `run`; `cli.rs` reaches only `drc`, `job`, `settings` of the engine; the dispatch lives in `cli.rs` and `run_job`, so `main.rs` is one line), `the_cli_path_and_prepare_job_give_equal_dumps`. Negative controls (each fails the gate on a damaged copy): `an_unmapped_flag_fails_the_gate` (a flag with a non-`FIELDS` path, absent from the help, and a CLI-only flag with no reason), `a_changed_default_fails_the_gate` (`[default: 2.5]` → `3.5`, a renamed path, an inlined `unwrap_or(25)`), `a_stray_call_in_main_rs_fails_the_gate` (`std::fs::read`, and a `freerouting::board` use in `main.rs` and `cli.rs`).

- [x] **C07.8 — Wire the router.**
  Replace the stub with A13's `run_pipeline`, passing:
  - the prepared board;
  - `BoardRouterSettings`;
  - `WorkBounds`;
  - A08's cancel token;
  - a progress sink that turns A08's router events into C07.6's log lines.
  The router-or-fanout-only rule and the optimizer hand-off belong to A13.
  The output is written from the final board after the best-board restore.
  - **Accept:**
    - `cli_route.rs`, on `Issue413-test.dsn`, `Issue026-J2_reference.dsn`
      and `Issue508-DAC2020_bm01.dsn`:
      - exit 0, a non-empty SES that `read_session` imports;
      - pass lines and a stage summary that `compare.py`'s regex parses;
      - with `-mp 2`, exactly two pass lines, as in
        `MaxPassesSettingRoutingTest`.
    - `ignore_net_classes_end_to_end`: on `Issue558-dev-board.dsn` with
      `-inc 'Power\,Default'`, the output SES has no new wires or vias on
      that class's nets.
    - `timeouts_only_stop_work` with routing: `job_timeout` far in the
      future gives byte-identical output to no timeout.
    - `tools/determinism-check.sh --out <dir> -- freerouting -de
      fixtures/Issue026-J2_reference.dsn -do {OUT} -mp 3` reports
      `identical`.
    - Mark `[~]` until A13 lands.

  *Done 2026-10-16:* `job::run::run_job` now calls `run_pipeline` (A13) with the prepared board, `settings`, `board_settings`, the job's `CancelToken` and the job limit as the pipeline's `deadline` (its watcher requests the full stop; `run_job` no longer arms a second one). A private `ProgressSink` (listening) times the passes from `Progress` events and hands the final board back from `finished`; the loop's own pass lines are replaced by Java's (`Auto-routing pass #N on board '<hash>' was completed in X.XX seconds with score S (U unrouted and V violations)`), and a run with the router enabled that ran the loop ends with the stage summary (`completed`, `completed with timeout`, `cancelled` or `interrupted`). The SES is written from `PipelineOutcome::board` (after the best-board restore). `NoRoutableLayer` is `Terminated`, exit 1 (`no_routable_layer_terminates_the_job` still passes). `tests/cli_route.rs` (7 pass, 31 s in the dev profile): `routes_issue413`, `routes_j2_reference`, `routes_dac2020_bm01` (`-mp 2`: exit 0, COMPLETED, an SES that `read_session` imports with 0 errors and wires, exactly two pass lines that a strict parser accepts, the stage summary after the passes and before the save, parsed by `compare.py`'s regex), `ignore_net_classes_end_to_end` (`-inc 'Power\,Default'` on `Issue558-dev-board.dsn`: 0 wires/vias on `+3V3`/`+5V`, other nets routed; the control without `-inc` routes them), `timeouts_only_stop_work_with_routing` (J2 `-mp 3`: `job_timeout=24:00:00`, none and a second run are byte-identical; `00:00:00` exits 0, non-empty smaller SES, the warning, `TIMED_OUT`), `a_disabled_router_prints_no_stage_summary`, `the_pass_line_parser_is_strict`. `tools/determinism-check.sh --out <dir> -- bash -c 'freerouting -de fixtures/Issue026-J2_reference.dsn -do "$1.ses" -mp 3 && mv "$1.ses" "$1"' _ {OUT}` (the script's `{OUT}` has no extension, the CLI refuses one without) printed `identical`, with the machine at load 15 before the loaded run. Listening versus no listener (the event sink of the CLI against `route_board --sink noop`) writes the same SES on `Issue508-DAC2020_bm01` `-mp 2` and costs nothing measurable (three runs each, 9.6 to 13.0 s on a loaded machine). `cli_outputs.rs` lost its stub-era assertions (`compare_py_regex_matches` now routes the open connection: 0 unrouted; the zero-timeout SES is smaller than the routed one). Notes: `--router.autorouter.enabled=false` still runs the fanout (A13, as Java's `RoutingPipeline`), so it is no longer "the board as loaded"; `optimizer.timeout` is not armed because the optimizer stage is a placeholder until O06; the log is printed after the job, not live.

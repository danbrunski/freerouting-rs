# C08 — Plugin-facing CLI extras

Opened 2026-10-15. Java's CLI was built for people, not for a plugin that
watches it. The Rust KiCad plugin (K02) needs:
- progress it can draw;
- a result it can show;
- a way to stop early and keep the board;
- exit codes that tell a usage error apart from a failed route;
- a version handshake.

The GUI and plugin report calls this checklist CLI-R. Everything here is a
Rust addition on top of C07's Java-compatible contract. Without the new
flags, the CLI behaves exactly as C07 defines.

**Prerequisites:**
- C07;
- A08 for the router events and the two-level cancel token;
- A13 for routing-phase progress and the routed parts of the tests. C08.1,
  C08.2 and C08.4–C08.6 can land on C07's stub pipeline, with the routing
  checks marked `[~]` until A13.

## What exists and what this adds

| Exists | Where |
|---|---|
| The plugin's runner | `kicad-plugin/freerouting_rs/tools.py` `start_route`/`finish_route`: `stdin=DEVNULL`, stdout and stderr merged, failure is a non-zero code or a missing SES, Cancel calls `proc.kill()` |
| Java's result manifest | `core/results/RoutingResultManifest.java` (field names via `@SerializedName`: `schema_version`, `generated_at`, `app_version`, `git_sha`, `fixture{filename, sha256}`, `settings_snapshot`, `phases{fanout, autorouter, optimizer}{before, after, duration_seconds, cpu_seconds, passes_completed, total_allocated_gb, peak_heap_mb}`, `board_statistics`, `bounds`, `normalized_score`, `optimizer_score`, `resource_usage`, `final_state`, `exit_code`, `output_written`, `cpu_score`); `CliRunner.writeCliResultManifestIfRequested`; test `core/results/RoutingResultManifestTest` |
| Java's two stop levels | `core/StoppableThread` (`requestStopAutoRouter` vs `requestStop`), test `core/StoppableThreadTest` |
| C07's job runner, outputs and exit codes | `job/{run.rs, outputs.rs, deadline.rs}`, `freerouting-cli/src/{args.rs, log.rs}` |

What this adds, in `crates/freerouting-cli/src/`:
- `exit.rs`, `version.rs`, `progress.rs`, `manifest.rs` and `stop.rs`;
- `build.rs`.

## Decisions made here

- **Exit codes:**

  | Code | Meaning |
  |---|---|
  | 0 | An output was written, even with incomplete connections. The details are in the manifest. |
  | 1 | Failure with no output: a load error, an engine error, a write failure. |
  | 2 | A usage error found before any work: no `-de`, or no `-do` outside DRC/help; an unsupported output format; a missing or malformed `--settings` file; a bad value for a C08 flag. |
  | 130 | Aborted before an output could be written: a second Ctrl-C, or a stop during load. |

  - Java uses 1 for usage errors. Codes 2 and 130 get DIVERGENCES lines. The
    plugin only checks for non-zero, so it is unaffected.
  - Unknown flags and bad `--router.*` values still only warn, as in Java,
    because scripts rely on that.
- **Progress is JSON lines on stderr.**
  - With `--progress json`, every stderr line is one JSON object; log
    lines become `{"event":"log","level":…,"text":…}`.
  - Events are:
    - `start`;
    - `progress`, with `phase` one of `load`, `fanout`, `route`,
      `optimize`, `write`, and the fields `pass`, `routed`, `failed`,
      `ripped`, `incomplete`, `violations`, `score` and `elapsed_ms`;
    - `done` last.
  - Events are emitted on each phase change, at the end of each pass, and
    every 50 connections. They are counted, never timed, so apart from
    `elapsed_ms` the stream is the same on every run.
- **The manifest uses Java's field names, schema version 1.** Fields that
  need JVM figures or a hash Rust doesn't compute are written as `null`:
  `cpu_seconds`, `total_allocated_gb`, `peak_heap_mb`, `resource_usage`,
  `cpu_score` and `fixture.sha256`. Java's `--router.result_json=PATH`
  setting and the new `--result-json PATH` flag write the same file.
- **Stopping.**
  - A stdin reader thread acts on whole lines, ignoring case and
    surrounding whitespace:
    - `stop` calls stop-all: finish the current step, write the best
      board, exit 0;
    - `stop-router` stops the router and lets the optimizer run, Java's
      `requestStopAutoRouter`.
  - **EOF on stdin is not a stop.** Today's plugin passes
    `stdin=DEVNULL`, which gives immediate EOF.
  - The first Ctrl-C or SIGTERM (Ctrl-C or Ctrl-Break on Windows) is
    `stop`; a second one aborts with exit 130 and writes nothing.
  - Signal handling needs `NEW DEP: ctrlc` (features `termination`). It is
    the smallest portable option, and the orchestrator decides. Without it
    only the stdin `stop` exists, and a signal kills the process as
    today; C07's temp-file write still prevents a partial file.
- **The new flags are aliases of existing settings, never new behaviour:**
  - `--timeout D` (`90s`, `10m`, `2h` or `HH:MM:SS`) is `job_timeout`, so
    stop-only;
  - `--max-passes N` is `-mp N`;
  - `--threads N` sets `autorouter.max_threads` and
    `optimizer.max_threads`, which never change results (D2).
- **`--seed` is reserved.** `--seed 0` is accepted and means Java's own
  seeds: the pass shuffle `pass·1000003 + max_net` and the maze reseed with
  the ripup cost. Any other value exits 2 with "seeded variation is not
  supported yet". What a seed should vary is a quality question for the
  Q series (C00, question 4).
- **The version handshake.** `--version` prints one line,
  `freerouting <crate version> (git <sha>)`. `--capabilities` prints
  JSON describing:
  - the version;
  - the output formats;
  - every flag, from C07's table;
  - the progress and manifest schema versions;
  - the stage ids, once A10's registry exists.

  The plugin pins on it (K01.4, K02).

## File ownership

| Item | New files | May edit |
|---|---|---|
| C08.1 | `crates/freerouting-cli/src/exit.rs`, `crates/freerouting-cli/tests/exit_codes.rs` | `main.rs`, `args.rs`, `docs/DIVERGENCES.md` (append) |
| C08.2 | `crates/freerouting-cli/src/version.rs`, `crates/freerouting-cli/build.rs` | `main.rs`, `args.rs`, `crates/freerouting-cli/Cargo.toml` (`build = "build.rs"`) |
| C08.3 | `crates/freerouting-cli/src/progress.rs`, `crates/freerouting-cli/tests/progress_json.rs` | `main.rs`, `log.rs` |
| C08.4 | `crates/freerouting-cli/src/manifest.rs`, `crates/freerouting-cli/tests/result_json.rs` | `main.rs` |
| C08.5 | `crates/freerouting-cli/src/stop.rs`, `crates/freerouting-cli/tests/stop.rs` | `main.rs`, `crates/freerouting-cli/Cargo.toml` (only if `ctrlc` is approved) |
| C08.6 | — | `args.rs`, `tests/args_java.rs` (append), `help.txt` |

## Checklist

- [ ] **C08.1 — Exit codes 0/1/2/130.**
  `exit::Code` with one constructor per row of the table, used only by
  `main.rs`. C07's outcome mapping feeds it.
  - **Accept:**
    - `tests/exit_codes.rs` runs the built binary
      (`env!("CARGO_BIN_EXE_freerouting")`):
      - no arguments → 2;
      - `-de x.dsn` without `-do` → 2;
      - `-do out.xyz` → 2, with no file created;
      - a missing input file → 1;
      - a DSN that doesn't parse → 1;
      - routing disabled on `Issue413-test.dsn` → 0;
      - `--settings missing.json` → 2;
      - `-help` → 0.
    - The 130 case is in C08.5.
    - The DIVERGENCES lines are added.
    - C07.4's `no_zero_byte_outputs` now expects 2 for C01.6 rows 4
      (`.xyz`) and 5 (no `-do`), where Java exits 1. The test cites the
      DIVERGENCES line.

- [ ] **C08.2 — `--version` and `--capabilities`.**
  `build.rs` uses no new dependency. It runs `git rev-parse --short=9
  HEAD` in the workspace root. `FREEROUTING_GIT_SHA` overrides it, and when
  git is unavailable it falls back to `unknown`. It emits
  `cargo:rerun-if-env-changed=FREEROUTING_GIT_SHA` and
  `rerun-if-changed` on `.git/HEAD`.
  - **Accept:**
    - `--version` prints exactly one line matching
      `^freerouting \d+\.\d+\.\d+ \(git [0-9a-f]{9}|unknown\)$` and exits
      0.
    - `--capabilities` prints valid JSON. A test checks that its flag list
      equals C07's `args.rs` table, so a new flag can't be forgotten.

- [ ] **C08.3 — `--progress json`.**
  `progress::JsonSink` implements the router event sink. It maps A08's
  events to the schema in Decisions; `load` and `write` events come from
  the job runner itself.
  - **Accept:**
    - `progress_json.rs` on the stub pipeline: every stderr line parses
      as JSON; `start` comes first and `done` last; the phases include
      `load` and `write`; `done.exit_code` equals the process exit code.
      A golden file stores the stream with `elapsed_ms` and paths
      removed, and the test compares two runs to it.
    - After A13, the same on `Issue026-J2_reference.dsn` with `-mp 2`:
      two `route` pass events, `pass` 1 and 2, with `incomplete` equal to
      the pass log lines' counts. `[~]` until A13.

- [ ] **C08.4 — `--result-json PATH` and `router.result_json`.**
  `manifest::write(path, &JobOutcome, &PreparedJob-derived data)` writes
  the manifest with C07's atomic writer, **also when the run failed**,
  with `output_written: false`. `board_statistics` is written from
  `scoring::BoardStatistics` with Java's JSON key names (those
  `reference_load.rs` reads), in `manifest.rs`, without editing
  `scoring/`.
  - **Accept:**
    - `result_json.rs`: every top-level key and every `phases.*` key of
      Java's manifest is present; `schema_version` is 1; `exit_code` and
      `output_written` match the process.
    - A failed load still writes the manifest, with `output_written:
      false` and `final_state: "terminated"`.
    - `--router.result_json=` and `--result-json` give equal files apart
      from `generated_at`.
    - After A13, `phases.autorouter.passes_completed` equals the number
      of pass lines. `[~]` until then.

- [ ] **C08.5 — Stopping: stdin, Ctrl-C, SIGTERM.**
  `stop::spawn_stdin_reader(token)` and, if approved, `stop::install_signals(token)`.
  Map Java's two levels to A08's token as in Decisions.
  - **Accept** (`tests/stop.rs`, which spawns the binary with piped
    stdin):
    - `stdin_eof_is_not_a_stop`: closing stdin at once still completes
      normally with exit 0.
    - `stop_line_keeps_the_board`: after A13, on
      `Issue508-DAC2020_bm01.dsn` with `-mp 50`, send `stop\n` when the
      first `route` progress event arrives. The run exits 0 with a SES
      that `read_session` imports and the `done` event's state
      `completed`. Before A13, the same on the stub with the stop sent at
      `load`.
    - `stop_router_lets_the_optimizer_run`: after A13, a test pipeline
      sees the router stopped and the optimizer phase still emitted.
    - `second_interrupt_aborts`: exit 130, no output file, no temp file
      left. If `ctrlc` is not approved, this uses a test hook that injects
      the second stop.

- [ ] **C08.6 — Long-form aliases and the reserved seed.**
  `--timeout`, `--max-passes`, `--threads` and `--seed`, as in Decisions,
  parsed in `args.rs` into the same patch fields as their Java forms.
  - **Accept:**
    - Table tests: `--timeout 90s` → `job_timeout` `00:01:30`;
      `--timeout 10m` → `00:10:00`; `--timeout 25h` → capped at 24 h with
      a warning; `--timeout x` → exit 2.
    - `--max-passes 3` gives the same patch as `-mp 3`.
    - `--threads 4` sets both thread fields.
    - `--seed 0` is accepted; `--seed 5` exits 2 with the message.
    - After A13, `--threads 1` and `--threads 8` give byte-identical SES
      on `Issue026-J2_reference.dsn` (`-mp 3`). `[~]` until then.

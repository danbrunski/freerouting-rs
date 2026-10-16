# Settings, CLI and writers

Recorded 2026-10-15. This area covers everything between the command line
and the router:
- how a run's settings are assembled;
- what those settings change on the board before routing starts;
- the rules, session and design files the engine reads and writes;
- the CLI contract that the KiCad plugin, Dan's scripts and the Java fork's
  measurement tools depend on.

The checklists are the [C series](tasks/C00-cli-settings-io-README.md).
Sources:
- the roadmap research's pipeline, settings and CLI report, and its Java
  test inventory;
- the GUI and plugin report's CLI-R and ENG-GUI-B sections;
- KiCadRoutingTools' settings-parity gates.

Every fact used in a checklist was checked against Java at `cda540f`, read
through git, and against the Rust tree at `70389b7`.

## What exists

The Rust port reads DSN, SES and rules files into syntax trees, and can write
those trees back as text. It builds a board from a DSN and imports a session
into it. It runs Java's design rules check from the command line
(`freerouting -de board.dsn -drc report.json`).

What it lacks:
- **Settings.** There is no `RouterSettings`, no config file, no
  environment variables and no settings flags.
- **Load-time behaviour.** Java tunes per-layer trace costs to the board's
  shape and applies five board overrides before routing. None of that
  exists.
- **Rules.** A rules file parses but is never applied.
- **Writers.** Nothing turns a board into a session, a design or a rules
  file.
- **CLI.** Any command other than `-drc` exits 2 with "routing is not
  implemented yet".

## How Java assembles settings, and why Rust copies the quirks

Java merges `RouterSettings` from up to six sources:
- defaults;
- `freerouting.json`;
- the DSN's `autoroute_settings`;
- a rules file;
- environment variables;
- the CLI.

Each source is a nullable copy of the whole settings object. The merge
copies every non-null field upward by priority, through reflection.

That machinery goes away. Its observable behaviour stays, because routes
depend on it (decision D1):
- **A rules file overrides the CLI.** After the merge, `RulesReader`
  applies the rules file's `autoroute_settings` on top. That re-enables
  every layer and the optimizer, whatever the command line said.
- **One explicit layer cost turns off cost tuning for every layer.** The
  tuning is guarded by a transient flag. Any explicit layer cost from a DSN
  or rules file sets it, so the other layers keep a flat 1.0/1.0 with no
  aspect-ratio or outer-layer surcharge.
- **Costs come from the unexpanded board box.** They are tuned once
  before the board's bounding box grows to include all items. The second
  tuning call is a no-op.
- **`-mt` does nothing in a headless run.** It sets a field no stage reads.
- **The fork's flat keys work.** `router.via_costs`,
  `router.start_ripup_costs` and the rest reach the cost settings (D8, fork
  commit `77d6158a8`).
- **`ignore_net_classes` is honoured headless.** Dan relies on `-inc` to
  keep the router off power nets he lays by hand.

Each of these is reproduced and pinned by a named test (C00, "Rules for this
series").

The Rust design keeps one Java-shaped patch type that mirrors the nullable
object field for field, including the transient flag and cost arrays. It
also has a resolved type that everything else reads. Both, and the lookup
table used by the CLI, environment variables, JSON files, help text and later
the GUI, are generated from one field table. That is KiCadRoutingTools'
lesson: drift between the CLI, the GUI and the engine starts wherever a
default or a flag is written twice. C07.7's gates fail the build when that
happens.

The config file is JSON in Java's shape, so Dan's `freerouting.json` works
through `--settings` unchanged. There is no TOML and no serde, so no new
dependency (C02).

## Settings that change the board

When Java loads a board for routing, it changes the board before the first
connection is routed:
- it appends a `board_edge` clearance class for copper-to-edge clearance
  and moves the outline into it. With the default 250 µm, the value is
  capped at the smallest pin-to-outline gap;
- it appends a `hole_edge` class for drilled-hole keepouts;
- it marks plane nets;
- it switches planes to obstacles or back;
- it sets the clearance tolerance;
- it flags the ignored net classes.

These change clearance classes, search-tree shapes and costs, so they change
routes. Java's DRC path skips all of them, and the Rust DRC runner keeps
doing so. The routing path gets them through `prepare_job` (C03), which runs
Java's load sequence step for step, down to the redundant second tuning
call.

## Writers

- **SES** is the file KiCad imports and the one A14's parity gate compares.
  It is written byte for byte as Java writes it: Java's indentation, name
  quoting, `Double.toString` numbers and `(attach off)` on padstacks that
  forbid via-in-pad (D8).
  - Content (board → `Session` tree) and layout (a Java-layout serializer in
    `freerouting-dsn`) are separate, so the layout can be proved alone.
  - Parsing each of the 160 Java-written reference sessions and serializing
    it again must give back its bytes (C05.1).
  - The plain writer stays for uses where Java isn't the oracle.
- **DSN and rules** are compared as parsed trees, not text. Java's own
  writers emit `snapAngle` and `viaRule`, which Java's readers skip, so a
  Java round trip loses the snap angle and via rules. Rust writes the
  keywords both readers accept.
  - For DSN the main check is the round trip: every fixture, written and
    read back, gives the same board on every compared statistic (C06).
  - For rules it is the round trip plus a field-by-field comparison with
    Java's output (C04).

## The CLI contract

The CLI accepts Java's flags with Java's parsing:
- prefix matching, and values taken only when the next argument doesn't
  start with `-`;
- `+`-joined multi-file arguments;
- an implicit "router on" for `-de` with `-do`;
- every flag the Java KiCad plugin passes.

It writes Java's log lines where tools parse them. `compare.py`'s
stage-summary regex matches Rust's output (C07). Two things differ from Java
on purpose:
- **No output is ever 0 bytes.** Java's headless `-do out.dsn` writes an
  empty file and logs no error. Rust writes a real DSN, or refuses an
  unsupported format before doing any work. Every output is written to a
  temporary file and renamed, so a killed run leaves nothing half-written.
- **Wall-clock limits only stop work (D2).** Work bounds (`max_passes`,
  `max_items`, the per-pin fanout budget) and deadlines (`job_timeout`,
  `fanout.timeout`, `optimizer.timeout`) are separate types. Stage code
  receives work bounds and nothing else. A deadline can only trigger the
  same stop-all as a user stop, and the board is then written as it stands.

The plugin needs more than Java offers, so C08 adds:
- `--progress json`, with events counted rather than timed;
- `--result-json`, with Java's manifest field names;
- a graceful `stop` line on stdin. EOF is not a stop, because today's
  plugin passes an empty stdin;
- exit codes 2 for usage errors and 130 for aborts;
- a version and capabilities handshake.

None of this changes the default behaviour.

## How it is verified

- **Java dumps, not reasoning.** C01 adds six modes to the reference
  harness:
  - `settings`: the resolved settings, the per-layer costs, the clearance
    matrix and the override results, after Java's full load sequence;
  - `argv`: what Java's two argument parsers produce;
  - `ses`, `dsn` and `rules`: what Java's writers write;
  - `clirun`: the exit codes and files of real headless runs.
- **Named scenarios.** The settings dump runs four of them per board:
  - `default`, the CLI with no config;
  - `fixture`, Java's test settings: edge clearance 0 and the optimizer
    on, which is what Java's fixture-test numbers assume;
  - `cli`, a fixed flag set that also shows rules files overriding the
    CLI;
  - `owner-config`, Dan's `start_ripup_costs = 1500`.
- **Synthetic boards** cover the explicit-layer-cost quirk, which no
  fixture shows.
- **Ported Java tests.** Roughly 25 Java test classes port before the
  router exists, and each checklist names the ones it ports. Examples:
  `SettingsMergerTest`, `FlatCostSettingsTest`,
  `CopperToEdgeClearanceCapTest`, `SesRoundTripTest`, `RulesRoundTripTest`,
  `GlobalSettingsCommandLineTest`.
- **End to end.**
  - C03.6 compares the full settings dump and Java's post-load DRC count
    on every fixture.
  - C05 requires byte-identical sessions.
  - C07.8 runs the routed CLI under F02.4's determinism check once A13
    lands.

## Risks

- **Reproducing quirks.** Some of them, such as rules overriding the CLI
  and one explicit cost freezing all the others, will surprise users.
  They stay until the quality phase measures a change (D4). Each is listed
  with its test so it can be found and changed later.
- **Byte-exact SES depends on details far from the routing.** Name
  quoting, number formatting and Java's non-ASCII handling all matter.
  The Rust parser keeps non-ASCII names, which Java's scanner drops, so the
  Cyrillic fixtures are known exceptions. C05.1's self-check against all
  160 Java sessions finds such cases before the board-to-session code is
  written.
- **Repository size.** Full SES and DSN dumps would add about 30 MB, so
  only a subset is committed. The full set is generated on demand for the
  `#[ignore]` tests.
- **Where Dan's settings come from.** Dan's `start_ripup_costs = 1500`
  lives in the Windows Java config. A WSL run never sees it, and the Rust
  CLI reads it only through `--settings`. The CLI logs the effective value
  and its source on every run; the plugin's handling is open question 1
  in C00.
- **Routing parts wait on the A series.** C07.8 and parts of C08 depend on
  A08 and A13. Everything else lands first, on a stub pipeline, so the
  contract is tested before routing exists.

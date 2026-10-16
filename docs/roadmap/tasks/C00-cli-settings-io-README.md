# C00 — Settings, CLI and writers

Opened 2026-10-15. The C series ports everything between the command line and
the router:
- Java's settings model and its sources;
- the settings that change the board at load time;
- the rules, SES and DSN files;
- the CLI contract that the KiCad plugin, Dan's scripts and the Java fork's
  `compare.py` rely on.

The narrative is in [cli-settings-io.md](../cli-settings-io.md). Decisions
D1–D9 in [port-completion.md](../port-completion.md) hold throughout.

**C02 and C03 come early.** B06 (scores) needs C02's `RouterSettings`.
A01, the first checklist of the A series' sequential search core, needs
C02 and C03's `BoardRouterSettings`. A10 appends the D9 stage keys to C02's
field table. Start C02 now, and C03 as soon as C02.2 lands.

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [C01](archive/C01-java-reference-dumps.md) ✅ | Six deterministic Java dump modes (`settings`, `argv`, `ses`, `dsn`, `rules`, `clirun`), synthetic settings cases, committed dumps plus an on-demand full set | S–M / low (Java harness only) | none; J01.5 before regenerating the routed SES dumps |
| [C02](archive/C02-settings-model.md) ✅ | `settings/`: one field table, Java's names, aliases and defaults; Java's merge and precedence; config file (`--settings`, Dan's `freerouting.json`); env vars; DSN and rules `autoroute_settings`; validation; test profiles | M / low | none (C01.1 for C02.8) |
| [C03](archive/C03-settings-on-board.md) ✅ | Cost tuning, net-class exclusions, the five board overrides, `minimum_pin_gap`, `prepare_job` in Java's order, `BoardRouterSettings` for A01 | M / medium | C02, C01.1 |
| [C04](archive/C04-rules-files.md) ✅ | Rules reader (`-dr`, `+rules`, adjacent `<design>.rules`), rules in DRC, rules writer | M / medium | C02, C03, C01.1, C01.5, C01.6 |
| [C05](archive/C05-ses-writer.md) ✅ | SES writer, byte-identical to Java, with `(attach off)` | M / low | C01.3 (F02.1 optional) |
| [C06](C06-dsn-writer.md) | DSN writer (board → `Pcb`), round trip on every fixture, `-do out.dsn` | L / medium | C05.1, C01.4 |
| [C07](C07-cli-parity.md) | CLI parity: Java flags and settings, outputs never 0 bytes, Java exit codes, parsed log lines, wall-clock deadlines that only stop work, settings parity gates; the router wired in at C07.8 | M / low; C07.8 high | C02, C03, C05 (C04 and C06 for their formats); A08 for C07.5; A13 for C07.8 |
| [C08](C08-plugin-cli-extras.md) | Exit codes 0/1/2/130, `--version`, `--capabilities`, `--progress json`, `--result-json`, stop through stdin or Ctrl-C, long-form aliases, reserved `--seed` | S–M / low | C07, A08; A13 for the routed checks |

The research report's P0–P6 are C01–C07. Its P7 and P8 are A10–A13, and
its P9 is P02. The GUI and plugin report's CLI-R is C08, and its
ENG-GUI-B.1–3 is C04–C06.

## Order and critical path

```
C01 (Java dumps) ·····> the comparisons in C02.8, C03–C07
C02 ──> C03 ──> C04 ──┐
 │       └──> A01     ├──> C07 ──> C08
 └──> B06, A10        │     ^       ^
C05 ──> C06 ──────────┘     A08     A08, A13 (routed checks)
 └──> A14, G07, G08, K02    A13 ──> C07.8
```

- **C02 → C03 is on the port's critical path,** because A01 waits on it.
- **C05 comes next.** A14's board-parity gate, the GUI's save (G08) and
  the plugin (K02) all need it.
- **C07.8 sits on the route to a routing CLI:** J01 → B → O → A13 →
  C07.8 → K02. Everything in C07 except C07.8 can land before A13, on the
  stub pipeline.

## Parallel lanes and file ownership

There are four lanes, and they touch disjoint files:
- the Java harness: C01;
- settings: C02 → C03 → C04;
- writers: C05 → C06;
- the CLI: C07 → C08, after the other lanes.

C01 can run beside C02. The comparison items C02.8, C03.1 and C03.6 wait for
C01.1's dumps.

| Area | Owner | Others may |
|---|---|---|
| `tools/java-reference/src/reference/ReferenceDump.java` | C01 owns its six modes | J01.3 owns `route`; O06 adds `optimize`. Every edit is append-only (C01, Decisions) |
| `tools/java-reference/{cases/, dump-subset.txt}`, `fixtures/reference/{settings, cases, argv.json, ses, dsn, rules, clirun}` | C01 | — |
| `tools/java-reference/run.sh` | J01.3 | C01.7 changes the final command line only |
| `crates/freerouting/src/settings/` | C02 (`board.rs`: C03) | A10 appends stage keys to `fields.rs` |
| `crates/freerouting/src/job/` | C03 (`mod.rs`, `overrides.rs`, `prepare.rs`); C07 (`run.rs`, `outputs.rs`, `deadline.rs`, `log.rs`) | C04 edits the rules hook in `prepare.rs`; C06.8 appends to `outputs.rs` |
| `board/items/mod.rs` (`BoardOutline`) | — | C03.2 appends `minimum_pin_gap` |
| `drc/json.rs` | — | C02.4 appends `parse` |
| `drc/runner.rs` | — | C04.5 |
| `io/specctra/{rules_reader.rs, rules_writer.rs}` | C04 | — |
| `io/specctra/ses_writer.rs`, `crates/freerouting-dsn/src/write/java_style.rs` | C05 | — |
| `io/specctra/dsn_writer/` | C06 | — |
| `crates/freerouting-cli/**` | C07, then C08 | — |
| `lib.rs` | — | C02.1 appends `pub mod settings;`, C03.3 appends `pub mod job;` |
| `docs/DIVERGENCES.md` | — | every checklist appends |

## Rules for this series

These add to the shared rules in [README.md](README.md).

- **Quirks that change results are reproduced, each with a named test:**

  | Quirk | Test | Item |
  |---|---|---|
  | A rules file's `autoroute_settings` override the CLI | `rules_autoroute_settings_override_cli` | C04.3 |
  | One explicit layer cost suppresses the computed costs on every other layer | `explicit_layer_cost_suppresses_computed_costs` | C03.1, C04.4 |
  | `-mt` does nothing in a headless run | `mt_does_nothing_headless` | C07.2 |
  | The fork's flat cost keys (`router.via_costs`, `router.start_ripup_costs`, …; D8, fork commit `77d6158a8`) work on the CLI, in env and in JSON | `flat_cost_keys_reach_scoring` | C02.3, C02.4 |
  | `ignore_net_classes` (`-inc`) is honoured headless. Dan uses it to keep the router off hand-laid power nets | `ignore_net_classes_is_honoured_headless`, `ignore_net_classes_end_to_end` | C03.5, C07.8 |
  | Dan's `freerouting.json` (`router.scoring.start_ripup_costs = 1500`) is read through `--settings` | `owner_settings_file_sets_start_ripup_costs`, `owner_config_reaches_router_view` | C02.4, C03.7 |

- **Name the settings scenario.**
  - Every comparison against a Java dump names one of C01's scenarios:
    `default`, `fixture`, `cli` or `owner-config`.
  - Java's fixture-test numbers assume edge clearance 0 and the optimizer
    on (`TestingSettings`). Ports of those tests use C02.8's
    `java_fixture_test()` profile.
  - Route references (J01.3) use the production defaults, edge clearance
    250 µm capped at the minimum pin gap, with the optimizer off.
- **No 0-byte outputs.** Java's headless `-do x.dsn` writes a 0-byte file and
  no error. Rust writes a real DSN (C06) or exits with an error before doing
  any work. Every output is written to a temporary file and renamed.
- **Timeouts only stop work (D2).** Stage code gets C02's `WorkBounds`
  (`max_passes`, `max_items`, the per-pin budget) and never a `Duration`.
  Only C07's deadline watcher sees wall-clock limits, and it can only call
  stop-all.
- **The field table is the one definition.** No flag, default or settings
  path is written anywhere else. C07.7's gates check this.
- **No new dependency.** That means no serde and no TOML. The only one
  proposed is `ctrlc` for C08.5, as a `NEW DEP` line; the orchestrator
  decides.
- **Oracle strength is chosen per format.** SES is byte-identical to Java.
  DSN and rules are compared as parsed trees, because Java's writers emit
  `snapAngle` and `viaRule`, which its own readers skip.
- **The re-pin doesn't move the C scope.** Between `cda540f` and Java
  `30baad7e2` (J01's target), the Java files these checklists port differ
  only in one doc comment (`FanoutSettings.maxMillisecondsPerPin` is now a
  work budget). After J01.5, rerun C01's modes, `ses` after `route`.

## Dependencies on other series

| Needs from | What | Used by |
|---|---|---|
| A08 | Two-level cancel token (stop-router, stop-all), router events | C07.5, C08.3, C08.5 |
| A13 | `run_pipeline` | C07.8; the routed checks in C08 |
| A10 | Stage registry ids | C08.2 `--capabilities` (optional) |
| B01 | `state_hash` for the pass log line | C07.6, C07.8 |
| J01.5 | Regenerated `fixtures/reference/route` | C01.3 regeneration |
| F02.1, F02.4 | `probe.sh`, `determinism-check.sh` | C05.1 (optional), C07.8 |

| Gives to | What |
|---|---|
| B06 | `RouterSettings` with the score versions and weights (C02) |
| A01 | `BoardRouterSettings` (C03.7), from a prepared board (C03.6) |
| A10 | The field table to append stage keys to (C02.1) |
| A14 | The SES writer (C05) and the `java_fixture_test()` profile (C02.8) |
| O06 | Settings (C02) and the SES writer (C05) |
| G07, G08 | Settings, `prepare_job`, the rules reader and writer, SES and DSN writers. The G series depends on C04–C06 (ENG-GUI-B) |
| K02 | The routing CLI (C07), the plugin extras (C08), `--settings` for Dan's file |

G00 lists "the interactive settings, serde" under C02. C02 covers Java's
`app.freerouting.settings` only, and uses no serde. Java's interactive
editing settings live in GUI classes (for example
`gui/workspace/controllers/GuiBoardRoutingSettings.java`) and belong with I01
and G09.

## Open questions for Dan

1. **Should the Rust CLI read Java's `freerouting.json` without being told?**
   - Java reads `%APPDATA%\freerouting\freerouting.json`, where your
     `start_ripup_costs = 1500` lives. A WSL run reads
     `~/.config/freerouting/freerouting.json` instead, which on this
     machine has empty router sections.
   - *Recommendation:* no. The Rust CLI reads its own
     `freerouting-rs/freerouting.json`, in the same format. The plugin (K02)
     passes `--settings` with Java's file when "use Java Freerouting
     settings" is ticked, translating the path for WSL. The CLI logs the
     effective `start_ripup_costs` and its source at the start of every
     run.
2. **Exit codes 2 (usage error) and 130 (aborted) differ from Java's 1.**
   - *Recommendation:* adopt them. The plugin only tests for non-zero, and
     scripts gain the distinction.
3. **Config file format.**
   - *Recommendation:* JSON in Java's shape only, with no TOML. That means
     one format for both engines and no new dependency.
4. **`--seed`.**
   - *Recommendation:* reserve it. `--seed 0` means Java's seeds, and any
     other value is refused until a Q-series measurement shows a use for
     seeded variation.
5. **Net-class names with commas.**
   - KiCad writes classes such as `Power,Default`. Java's `-inc
     Power,Default` looks for the classes `Power` and `Default`. Neither is
     `Power,Default`, so those nets are still routed, and `Default` matches
     Java's built-in `default` class.
   - *Recommendation:* keep Java's split, add `\,` to keep a comma, and
     warn when the split looks like a mistake (C07.2).
   - Do any classes on `canary-pwr` contain commas?

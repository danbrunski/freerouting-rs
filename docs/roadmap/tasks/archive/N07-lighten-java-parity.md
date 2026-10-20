# N07 — Lighten the Java burden

Opened 2026-10-20 at the owner's request. Java compatibility is now a lower priority (decided
2026-10-20: "I'm getting less interested in perfect compatibility with freerouting java"). Parity
checks are already small by default: the connection-parity set, with full A14 only on request
(AGENTS.md, "The machine"). Matching Java still costs work on every change:
- the rule that anything not in `docs/DIVERGENCES.md` must match Java;
- a deliberate difference needs a test, a module note and a DIVERGENCES line;
- expected values come from running Java at the pin (`tools/java-reference/probe.sh`);
- every Java bug fix sits behind a `router.java_quirks.<id>` switch with a `// PARITY(quirk, …)`
  marker. There are 82 markers today, and `enhanced` turns off 37 switches.

This checklist decides what `legacy` must still guarantee. It then trims the rules, tests and
code to that contract, so the effort goes to `enhanced`.

Prerequisites: N01 (profiles), N06 (the quirk switches).

## Decisions made here

- **The owner chooses `legacy`'s contract in N07.2**, between:
  - (a) frozen exact: `legacy` stays byte-identical to Java at `a773a70ec` on connection-parity
    and the probes, nothing new is ported from Java, and the pin never moves;
  - (b) Java-like: `legacy` keeps Java's algorithms but not byte identity, the quirk switches go,
    and the fixes apply in both profiles.

  Recommendation: (a). The parity set runs in about a minute, (a) changes no route, and (a) keeps
  a regression net for the shared search code. **Decided 2026-10-20: (a)**, the small parity set
  (D11 in `docs/roadmap/port-completion.md`).
- **Nothing `enhanced` already has is lost.** Under either choice its 37 fixes and every accepted
  stage stay, and its sessions stay byte-identical through the cleanup (N07.4).

## File ownership

| Item | New files | May edit |
|---|---|---|
| N07.1 | — | — (inventory) |
| N07.2–N07.3 | — | `AGENTS.md` ("Matching Java"), `.claude/agents/implementer.md`, `docs/roadmap/port-completion.md` (a decision line), `docs/roadmap/tasks/README.md`, `docs/development/engine.md` |
| N07.4 | — | `crates/freerouting/src/settings/java_quirks.rs` and the `PARITY(quirk, …)` sites, `settings/profile.rs`, `docs/DIVERGENCES.md`, the parity tests' settings |
| N07.5 | `tools/parity-check.sh` | `docs/development/ci-workflow.yml`, `docs/development/engine.md` |

## Checklist

- [x] **N07.1 — What parity costs today.** Make an inventory, with numbers:
  - the quirk switches: total, in `ENHANCED`, rejected, and left as Java;
  - the `PARITY(quirk, …)` markers;
  - the tests that read Java output: the probe tests (`crates/freerouting/tests/*probe*`), the
    board-parity tests (`autoroute_board_parity.rs`, `board_parity/`), and the differential
    tests, with their run times;
  - the Java tooling: `tools/java-reference/`, the frozen jars, and the dumps;
  - the AGENTS.md and checklist rules that need Java.
  - **Accept:** the table is in the done note, with each test group's run time from one
    `cargo test` run.


  *Done 2026-10-20:* the inventory at `de971ee`, before N07.4's trim. Test times are from one
  `cargo test --workspace --no-fail-fast` run (the `dev` profile at opt-level 1, 2 build jobs):
  268 test binaries, 2612 passed, 55 ignored, 0 failed; the per-binary times sum to 739 s
  (the tests inside a binary run in parallel, so the wall time is lower).

  | What | Count | Cost |
  |---|---|---|
  | Quirk switches (`router.java_quirks.<id>`) | 46 | 35 kind (b) fixes, 11 kind (c) |
  | in `ENHANCED` (fix on) | 37 | `legacy` runs none of them |
  | rejected, stay Java in both profiles | 2 | `octagon_box_cutout_branch3`, `ripup_pass_no_unset` |
  | left as Java, stay `true` in both profiles | 7 | six of them read by no code (below) |
  | `PARITY(quirk, …)` markers | 82 | 64 in `crates/freerouting/src` and `crates/freerouting-geometry/src`, the rest in tests and docs comments |
  | Probe tests, `crates/freerouting/tests/*probe*.rs` (+ `freerouting-dev/tests/probe_values.rs`) | 22 (+1) files, 67 tests | 78.1 s summed (dev profile); 185 s in release, where `board_probe_tree` (83 s) and `board_probe_tree_ops` (65 s) run every board instead of a sample (`BOARD_PROBE_ALL`) |
  | Differential tests `reference_*.rs` (load, drc, optimize, pull_tight, shove_check, shove_insert, islands, opt_vias) | 8 files, 44 tests | 44.0 s summed |
  | Board parity `autoroute_board_parity.rs` (A14 gate, `#[ignore]`) + `board_parity/` | 1 file; 7 pin tests run, the gate is ignored | 0.7 s in the plain run; connection-parity set (7 of its 10 rows are gated: the other three have their Java route reference in a `fixtures/reference/route/` subdirectory, which needs `PARITY_SUBDIRS=1`) 2–4 s in release; full gate about 1.5 h |
  | Other test files that read `fixtures/reference/` (DSN, SES, settings, rules, route dumps) | 43 files in `crates/freerouting` | about 272 s summed (largest: `dsn_writer` 71 s, `dsn_round_trip` 57 s, `job_prepare_reference` 40 s) |
  | `fixtures/reference/` | 2085 files, 104 MB | `probe` 1099 files (55 MB), `route` 339, `load` 171, `rules` 170, `settings` 170 (20 MB), `ses` 56, `dsn` 38, `optimize` 28, `clirun` 9, `cases` 4 |
  | Java tooling | `tools/java-reference/` 62 files, 1.1 MB (`run.sh`, `probe.sh`, `regenerate-dumps.sh`, harness `src/`, `cases/`, `probes/`, `route-bounds.tsv`); frozen jars in `../freerouting/logs/jars` (7, incl. `freerouting-a773a70ec.jar`); `crates/freerouting/tests/data/java_drc_digest.tsv`, `java_drc_rules_digest.tsv`, `settings/MergeProbe.java.txt` | needs Java 25 and ECJ to regenerate; the Rust tests read only the committed dumps |
  | Rules that need Java | AGENTS.md "Matching Java" (3 bullets: pin, DIVERGENCES, expected values from Java); `.claude/agents/implementer.md` "The Java reference" (11 mentions of the pin or probes); `docs/roadmap/tasks/README.md` "Shared rules" (3 bullets); the per-item "Java parity" part of every implementer report | N07.3 limits these to code `legacy` runs |

  N07.4 found the dead plumbing: six of the seven left-as-Java switches (N06.12's
  `insert_stale_trace_object`, `rules_autoroute_settings_reenable`, `direction_compare_zero`,
  `route_stubs_layer_test`, `polyline_array_reversal`, `avoid_acid_traps_disabled`) have no read
  site: `false` changed nothing and no code named the field outside its settings entry.
- [x] **N07.2 — Choose `legacy`'s contract.** Put options (a) and (b), N07.1's numbers and the
  recommendation to the owner.
  - **Accept:**
    - the owner's choice is recorded as "decided <date>: …" in `docs/roadmap/port-completion.md`
      and here;
    - if the owner doesn't answer, it stays open.

  *Done 2026-10-20:* decided 2026-10-20: option (a), "yeah do the small parity set". `legacy`
  stays byte-identical to Java at `a773a70ec` on connection-parity and the probes. Nothing new
  is ported from Java, and the full A14 runs only on request. Recorded as D11 in
  `docs/roadmap/port-completion.md`. The choice was made before N07.1's inventory, on the
  recommendation's grounds. N07.4 therefore takes its (a) branch: only dead switch plumbing goes.

- [x] **N07.3 — Rules for changes that only touch `enhanced`.** A change behind a profile entry
  or a setting that `legacy` leaves unset needs only its own test and a DIVERGENCES line. It needs
  no Java probe, no differential test and no Java-derived expected value. Its expected values
  come from its own measured behaviour. Shared code that `legacy` runs still keeps the parity set
  exact (under (a)).
  - **Accept:**
    - AGENTS.md "Matching Java", the implementer agent definition, the tasks README and
      `docs/development/engine.md` say so;
    - `python3 tools/roadmap/checklists.py status --strict` passes.


  *Done 2026-10-20:* the rule is in AGENTS.md "Matching Java" (two bullets, "`legacy` is frozen
  Java (D11)" and "A change that only touches `enhanced`"; the differential-test bullet now says
  "code `legacy` runs"), `.claude/agents/implementer.md` ("The Java reference", two bullets),
  `docs/roadmap/tasks/README.md` ("Shared rules", one bullet) and `docs/development/engine.md`
  ("Promoting an improvement into `enhanced`"); D11 in `docs/roadmap/port-completion.md` names
  it too. `python3 tools/roadmap/checklists.py status --strict` passes (run at the end of this
  item's diff).
- [x] **N07.4 — Trim the code to the contract.**
  - Under (a): remove only dead switch plumbing (a switch no code reads, a marker without a
    switch), and keep the rest.
  - Under (b): remove each `router.java_quirks.<id>` switch. Make its fix unconditional, keep
    the rejected and (c) rows as Java's code without a switch, and replace the per-switch
    DIVERGENCES lines with one line per changed behaviour of `legacy`.
  - **Accept:**
    - `enhanced`'s sessions are byte-identical before and after on the connection-parity set
      and Q18's set;
    - under (a), connection-parity under `legacy` stays 7 of 7 exact;
    - under (b), `legacy`'s changes are listed per board in the done note;
    - `cargo test --workspace` passes.


  *Done 2026-10-20 (option (a)):* removed the dead switch plumbing only: six switches that no
  code read (`insert_stale_trace_object`, `rules_autoroute_settings_reenable`,
  `direction_compare_zero`, `route_stubs_layer_test`, `polyline_array_reversal`,
  `avoid_acid_traps_disabled`; N06.12's (c) rows, `false` changed nothing). Switches 46 -> 40,
  `PARITY(quirk, …)` markers 82 -> 74 (their eight sites keep a plain comment, "Java, kept in
  both profiles"). Files: `settings/fields.rs` (six entries), `settings/java_quirks.rs` (six ids,
  module docs), the eight marker sites, `tests/settings_java_quirks.rs` (`SWITCH_COUNT` 46 -> 40
  and the new `removed_inert_switches_are_unknown_keys`) and the DIVERGENCES line. Kept: the
  other switches, including `simplex_prev_division_line` (read in `Simplex::cutout_from_counted`,
  in `ENHANCED`, its fix inert) and the rejected and read (c) rows.
  - `enhanced` byte-identical: `tools/bench/quick-arm.sh` with the binary built from `de971ee`
    (before) and from this diff (after), default profile (`enhanced`), `cmp` on every SES:
    connection-parity set 10 of 10 identical; Q18's set (`ROW_FLAGS=1`, 12 rows, `canary-pwr`
    included) 12 of 12 identical. The `--result-json` manifests differ only in `generated_at`,
    the output path and `duration_seconds` (checked on `min_fr_test`).
  - `legacy`: `tools/parity-check.sh`, connection parity 7 of 7 exact, 7 SES byte-identical
    (`summary.md`: "exact: 7 boards, 7 pass, 0 fail, 7 SES byte-identical").
  - `cargo test --workspace --no-fail-fast`: 2612 passed, 0 failed, 55 ignored.
- [x] **N07.5 — One parity command.** Write `tools/parity-check.sh`: the connection-parity set
  and the probe tests in one command, timed, with the full A14 behind a flag for on-request runs.
  The CI workflow kept in `docs/development/ci-workflow.yml` runs that command instead of the
  full set.
  - **Accept:** the script, its run time on this machine, and the docs that name it.


  *Done 2026-10-20:* `tools/parity-check.sh [--full] [--no-probes] [--no-parity]`. Step 1: the
  22 `crates/freerouting/tests/*probe*.rs` files (release). Step 2: `autoroute_board_parity`
  on `tools/bench/sets/connection-parity.txt` (`PARITY_SET`); `--full` drops the set (the full
  A14, about 1.5 h, on request, not run here). It prints each step's time and `PARITY OK` or
  `PARITY FAILED`, and exits 1 on a failure. Run time here (2 build jobs, warm release build):
  probe tests 185 s, connection parity 2 s, total 187 s (the first run with a cold step 2: 208 s +
  4 s = 212 s). Both steps PASS; connection parity 7 of 7 exact. Named in
  `docs/development/engine.md` (tools table), AGENTS.md, the tasks README and D11, and run by the
  new `parity` job of `docs/development/ci-workflow.yml`.
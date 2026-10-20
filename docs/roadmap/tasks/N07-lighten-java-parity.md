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
  a regression net for the shared search code.
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

- [ ] **N07.1 — What parity costs today.** Make an inventory, with numbers:
  - the quirk switches: total, in `ENHANCED`, rejected, and left as Java;
  - the `PARITY(quirk, …)` markers;
  - the tests that read Java output: the probe tests (`crates/freerouting/tests/*probe*`), the
    board-parity tests (`autoroute_board_parity.rs`, `board_parity/`), and the differential
    tests, with their run times;
  - the Java tooling: `tools/java-reference/`, the frozen jars, and the dumps;
  - the AGENTS.md and checklist rules that need Java.
  - **Accept:** the table is in the done note, with each test group's run time from one
    `cargo test` run.

- [ ] **N07.2 — Choose `legacy`'s contract.** Put options (a) and (b), N07.1's numbers and the
  recommendation to the owner.
  - **Accept:**
    - the owner's choice is recorded as "decided <date>: …" in `docs/roadmap/port-completion.md`
      and here;
    - if the owner doesn't answer, it stays open.

- [ ] **N07.3 — Rules for changes that only touch `enhanced`.** A change behind a profile entry
  or a setting that `legacy` leaves unset needs only its own test and a DIVERGENCES line. It needs
  no Java probe, no differential test and no Java-derived expected value. Its expected values
  come from its own measured behaviour. Shared code that `legacy` runs still keeps the parity set
  exact (under (a)).
  - **Accept:**
    - AGENTS.md "Matching Java", the implementer agent definition, the tasks README and
      `docs/development/engine.md` say so;
    - `python3 tools/roadmap/checklists.py status --strict` passes.

- [ ] **N07.4 — Trim the code to the contract.**
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

- [ ] **N07.5 — One parity command.** Write `tools/parity-check.sh`: the connection-parity set
  and the probe tests in one command, timed, with the full A14 behind a flag for on-request runs.
  The CI workflow kept in `docs/development/ci-workflow.yml` runs that command instead of the
  full set.
  - **Accept:** the script, its run time on this machine, and the docs that name it.

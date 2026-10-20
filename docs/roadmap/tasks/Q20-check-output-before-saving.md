# Q20 — The router checks its output against the board's rules before saving

Opened 2026-10-20 at the owner's request. Routing never breaks the board's manufacturing rules
(decided 2026-10-20, Q18; `docs/roadmap/routing-quality.md`, order of importance item 1). Until
now each breach was found afterwards, by KiCad's DRC in a measurement:
- 0.15 mm neck-downs on 0.2 mm boards;
- copper inside the edge clearance;
- a Margin-layer line the DSN never carried.

This checklist moves the check into the router. Before the session is written, the router checks
its own routed copper against every rule it knows. It removes what breaks a rule and tries those
connections again, and it never writes breaching copper. A gap in the router's model then shows up
as a named, counted event instead of a silent violation.

Prerequisites: Q18 (the minimums and edge/hole clearances the check enforces), Q16 (dangling
stubs), Q04.3 (`quality::output_guard`, the batch loop's finish hook).

## Decisions made here

- **Only routed copper is changed.** Copper that was in the input is checked and reported once,
  never removed (Q20.4).
- **Setting `router.output_check = off | report | repair`.**
  - `repair` is `enhanced`'s default and joins `ENHANCED` without a preregistered keep rule
    (decided 2026-10-20: a correctness rule, as for Q19). Q20.6 measures what it costs.
  - `legacy` stays `off`, because `legacy` is Java.
  - `report` logs and counts but changes nothing.
- **The repair is bounded and deterministic.** It runs one retry pass over the affected
  connections, in ascending connection order, under the same floors and a fixed budget. What
  still breaks a rule is removed and left unrouted.
- **The check is the engine's DRC** (`crates/freerouting/src/drc/`), extended to cover the
  board's rules. It doesn't call KiCad. Q20.1 holds it to KiCad's verdicts.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q20.1 | `crates/freerouting/src/drc/board_rules.rs`, `crates/freerouting/tests/drc_board_rules.rs` | `drc/mod.rs`, `drc/checker.rs`, `drc/report.rs` |
| Q20.2–Q20.4 | `crates/freerouting/src/quality/output_check.rs`, `crates/freerouting/tests/quality_output_check.rs` | `quality/mod.rs`, `quality/register.rs`, the batch loop's finish, the settings table, `settings/profile.rs` (one entry) |
| Q20.3 | — | `crates/freerouting-cli/src/` (result JSON), `kicad-plugin/freerouting_rs/summary.py`, the GUI's DRC list |
| Q20.5–Q20.6 | `docs/roadmap/prereg/Q20.json`, `tools/bench/sets/q20-output-check.txt` | — |

## Checklist

- [ ] **Q20.1 — The check covers the board's rules.** On routed copper, the engine's DRC
  reports each of these:
  - clearance between nets;
  - track width under the board minimum;
  - via diameter, drill or annular ring under the minimums;
  - copper-to-edge clearance, including Margin-layer and inner Edge.Cuts lines once Q18.5
    carries them;
  - hole-to-copper clearance;
  - copper of another net inside a pad's mask aperture, once Q18.7 carries the expansion;
  - vias in pads;
  - dangling stubs (Q16).

  Each item is marked routed or input.
  - **Accept:**
    - one fixture test per rule, written from scratch, with a breaching and a clean case;
    - on the Q18 board set (`tools/bench/sets/q18-board-minimums.txt`), the router's check and
      KiCad's DRC under the board's own rules agree on routing-caused items, with every
      disagreement explained in the done note.

- [ ] **Q20.2 — Check and repair before saving.** After the optimizer, and before the session is
  written, run the check on routed items:
  - `report` logs and counts;
  - `repair` removes the breaching routed items, retries the affected connections once, checks
    again, and removes what still breaches.
  - **Accept:**
    - a test hook makes a stage emit a breaching trace and a breaching via, and the written
      session has neither;
    - the connections are retried, and the ones that can't be routed within the rules are left
      unrouted;
    - two runs give identical sessions;
    - with `off`, connection-parity stays 7 of 7 exact under `legacy`;
    - on a board where the check finds nothing, the session is byte-identical to `off`'s.

- [ ] **Q20.3 — Say what the check did.** Report it in four places:
  - the run log, one line per rule plus a total;
  - `--result-json`, a field `output_check: {checked, found, removed, retried, repaired,
    left_open, by_rule}`;
  - the plugin's result summary;
  - the desktop app's DRC list.
  - **Accept:** a test per surface, and the fields documented in the CLI help and the plugin
    README.

- [ ] **Q20.4 — Input copper is reported, not changed.** Violations already present in the
  input are logged once at load with their count and rule ("the board already has N
  violations: …"). The repair never removes or moves input copper.
  - **Accept:** a fixture with a pre-existing clearance violation and a pre-existing narrow
    track routes with both kept and reported, and nothing new breaks a rule.

- [ ] **Q20.5 — Register the cost measurement.** Write `docs/roadmap/prereg/Q20.json` before
  any number:
  - **Arms:** `off` and `repair`.
  - **Boards:** the Q18 set's KiCad 6+ rows and canary-pwr, a few minutes in all.
  - **Grading:** one KiCad grade per distinct session.
  - **Guards:** zero routing-caused breaches in the `repair` arm.
  - **Report:** KiCad unconnected, route time and the check's own time.
  - **Accept:** the registration and its pin test are in, with no number yet.

- [ ] **Q20.6 — Measure the cost.** Run Q20.5 as registered.
  - **Accept:**
    - the per-board table is in the done note;
    - `repair` has zero breaches;
    - wherever the check found nothing, the sessions are identical;
    - the check's time is under 5 % of route time on every row, or the rows over it are
      named;
    - the `ENHANCED` entry and a DIVERGENCES line are in.

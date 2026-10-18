# Q15 — A measured tidiness score for routes

Opened 2026-10-17 (Dan: "sure"). KRT's users and author say its routes are "far more natural
and organized than the Manhattan-style FreeRouting algorithm". freerouting-rs measures none of
that today. It grades only:
- unconnected connections;
- DRC items;
- vias;
- length.

A score that measures how tidy routes are lets a change claim, or be refused, an improvement in
looks with numbers, and lets X13 compare tools on it.

Prerequisites: F03 (the bench and gate), F02.6 (the KiCad oracle), C05 (the SES reader).

## Decisions made here

- **Per routed connection, from the routed copper, with no judgement by eye:**
  - `detour`: routed length over the straight-line pad-to-pad distance;
  - `bends`: direction changes per 10 mm;
  - `acute`: corners sharper than 90°, counted;
  - `jogs`: segments shorter than 2 × the track width between two bends;
  - `vias`: vias per connection.
- **Report the board's median and 90th percentile** of each, not a sum, so a few long nets
  don't dominate.
- **It reads an SES or a `.kicad_pcb`**, so the same tool scores freerouting-rs, Java and KRT
  output (KRT writes a board, not an SES).
- **Report-only at first.** Making tidiness part of a keep rule would be a separate decision for
  Dan, once there are numbers.

## Checklist

- [x] **Q15.1 — The scorer.** `freerouting-dev tidy <board> <routed.ses|.kicad_pcb>` prints
  the five measures (median and p90) as JSON and as a table.
  - **Accept:** unit fixtures with known values (a straight track, an L, a zig-zag, an acute
    corner, a via pair); the same copper read from an SES and from a `.kicad_pcb` scores the
    same.

  *Done 2026-10-17:* `crates/freerouting-dev/src/tidy.rs` (command, report, JSON `freerouting-tidy-1`,
  table), `tidy_copper.rs` (which pin pairs are connections), `tidy_geometry.rs` (bend, acute,
  jog rules), `tidy_input.rs` (SES or `.kicad_pcb` into the one `import_session` path). The
  definitions are in those module docs and in `tools/bench/README.md` (`freerouting-dev tidy`).
  Choices made:
  - a bend is a turn of more than 10 degrees (below any corner a router makes on purpose, above
    the 5.7 degrees of one chord step of an approximated arc, so a smooth arc is not a row of
    bends); acute is a turn of more than 90 degrees (a right angle is not acute); a jog is a
    segment between two bends shorter than 2 track widths, collinear pieces counting as one
    segment; vias and pads end a run, so the turn where a path changes layer is not a bend;
  - a connection is an edge of the minimum spanning tree of the pins of one piece of copper,
    pin distance being the shortest copper path (Mehlhorn's construction, ties broken by node and
    edge order): two pins are one connection, a Y of three pins its two shorter legs; pairs with
    no copper or at one point are `skipped`; planes and pours carry no path;
  - `detour` is path length over pin-centre distance; `bends_per_10mm` divides by the path
    length; `vias` counts vias on the path plus through-hole pins it changes layer in;
  - median of an even count is the mean of the middle two, p90 is nearest rank `ceil(0.9 n)`;
    a measure over no connections is left out of the JSON.
  - `tests/tidy.rs` (15 tests, `cargo test -p freerouting-dev --test tidy`, all pass): the straight track
    (detour 1, 0 bends), the L (30 mm, 1 bend, 0.333 bends/10 mm), the zig-zag (3 bends, none
    acute: 45, 90, 45), a jog (2 bends, 1 jog), an acute corner (1 bend, 1 acute), a via pair
    (2 vias, 2 runs), a Y net (2 connections of 16.2 mm, not 3), the board medians and p90 of
    those six hand-computed, `distribution` edge cases, the command's table and JSON, a via
    padstack the DSN lacks (substituted, counted). `the_same_copper_from_a_session_and_a_kicad_pcb_scores_the_same`:
    one description generates the DSN, SES and `.kicad_pcb`; the two scores are equal as values
    and as `--connections` JSON; the Y net gives the same from both. Five unit tests in
    `tidy_geometry.rs` pin the bend threshold, acute, jog and zero-length rules.
    `java_route_references_are_scored` pins connection counts on three Java references.
  - A `.kicad_pcb` must be the board the DSN was exported from (KiCad's DSN frame); the reader's
    wires are cut where another wire touches a corner, because the engine's connectivity only
    joins trace ends.

- [~] **Q15.2 — In the bench and the grader.** `freerouting-dev bench` and the KiCad grading
  scripts (`tools/kicad-oracle/grade.sh`) add the five measures to their records, and
  `compare` shows them.
  - **Accept:** a bench run on the P04 set shows them; the records' schema is documented.

  *Partial 2026-10-17:* done: (1) `bench.json`: each run has a `tidiness` object (the Q15.1 JSON
  without `format`), and `bench.md` a second table (`tests/bench.rs::tidiness_is_in_the_run_record_and_the_report`);
  (2) the gate, which is the repository's comparison (`freerouting-dev gate`; there is no
  separate `compare` command): `gate collect` writes `tidy_<measure>_median` and `_p90` into the
  arm values from the bench run's `tidiness` (or the oracle summary's), and `default_metrics`
  lists them as `report` metrics, not judged and not kept in a baseline, so the gate's row lines
  print `tidy_detour_median a->b` (`tests/gate_collect.rs::tidiness_of_a_bench_run_reaches_the_records_and_the_report`:
  also asserts a baseline written from those records has no `tidy_` member); (3) the grader:
  `tools/kicad-oracle/grade.sh` and `tools/krt-h2h/grade_routed.sh` run `freerouting-dev tidy`
  (on the engine's SES, and on the routed `.kicad_pcb`) when `FREEROUTING_DEV` (default
  `target/release/freerouting-dev`) exists and add `tidiness` to `summary.json` through
  `oracle_lib.py add-tidiness`; it costs no KiCad call and never changes the verdict
  (`tools/kicad-oracle/tests/test_oracle_lib.py`, 2 new tests; `bash -n` on both scripts; the
  script paths themselves were not run, since KiCad was not started); (4) the schema is
  documented in `tools/bench/README.md` (`freerouting-dev tidy`, "The record") and in the
  kicad-oracle README and the `gate_collect` module table.
  Remaining: the P04-set bench run showing them, heavy run pending:
  `target/release/freerouting-dev bench --set tools/bench/sets/p04.txt --rust target/release/freerouting --runs 1 --max-load off --timeout 7200 --out <dir>`
  (release build of `freerouting` and `freerouting-dev`; the heavy-job slot; the P04 wrapper of
  `docs/roadmap/prereg/P04.1.json` if the arm's settings are wanted), then read `tidiness` in
  `<dir>/bench.json` and the second table of `<dir>/bench.md`. Also pending, one live
  `grade.sh` on a small board with `FREEROUTING_DEV` set, to see `tidiness` in `summary.json`.

- [~] **Q15.3 — A baseline.** Score Java's route references, freerouting-rs `legacy`, and
  `enhanced` with the sweep on the X13 boards.
  - **Accept:** a table in the done note, used by X13.

  *Partial 2026-10-17:* the Java arm is scored; the two routing arms are pending (they run the
  router). `tools/bench/q15-baseline.sh ARM OUT_DIR` does all three arms with the same boards and
  pass bounds as the Java reference (`tools/java-reference/route-bounds.tsv`) and prints the
  table below. X13 boards with a Java route reference in `fixtures/reference/route`: ecc83
  (`Issue690-ecc83`), sonde (`Issue690-sonde_xilinx`), interf_u (`Issue093-interf_u`; the
  `Issue733-kicad_interf_u_input_design` reference scores identically) and coldfire
  (`Issue690-kit-dev-coldfire-xilinx_5213`). StickHub and canary-pwr have no Java reference.

  Java route references (`tools/bench/q15-baseline.sh java-ref <dir>`; cells are `median / p90`
  per connection; `incomplete` is the unrouted-connection count of the same session, because
  these are bounded Java runs and only the routed pin pairs are scored):

  | board | arm | connections | incomplete | detour | bends/10mm | acute | jogs | vias/conn |
  |---|---|---|---|---|---|---|---|---|
  | Issue690-ecc83 | java-ref | 8 | 6 | 1.12 / 1.27 | 2.60 / 3.89 | 0.00 / 0.00 | 1.00 / 4.00 | 0.00 / 0.00 |
  | Issue690-sonde_xilinx | java-ref | 38 | 11 | 1.17 / 1.64 | 2.74 / 3.99 | 0.00 / 0.00 | 1.00 / 3.00 | 0.00 / 0.00 |
  | Issue093-interf_u | java-ref | 162 | 19 | 1.08 / 1.48 | 0.73 / 2.44 | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 2.00 |
  | Issue690-kit-dev-coldfire-xilinx_5213 | java-ref | 266 | 181 | 1.14 / 1.74 | 2.79 / 7.79 | 0.00 / 0.00 | 0.00 / 2.00 | 0.00 / 2.00 |

  Read with care: ecc83 has 8 scored connections of 21, and coldfire 266 of 541 with 181
  incomplete, so the medians describe the easy connections the bounded Java run finished. Compare
  arms on the same boards at equal `incomplete`, or restrict to connections both arms routed.

  Remaining, heavy runs pending (release builds of `freerouting` and `freerouting-dev`, quiet
  machine, the heavy-job slot; each arm runs the router on four boards):
  - `legacy`: `tools/bench/q15-baseline.sh legacy <dir>`
    (per board: `target/release/freerouting --profile legacy -de fixtures/<board>.dsn -do <dir>/<board>.legacy.ses --gui.enabled=false --router.autorouter.max_passes=<100, coldfire 3> --router.optimizer.enabled=false`,
    then `freerouting-dev tidy`);
  - `enhanced+sweep`: `tools/bench/q15-baseline.sh enhanced-sweep <dir>`
    (the same with `--profile enhanced --router.autorouter.sweep=free --router.autorouter.sweep_passes=2`).
  The script's routing arms were not run (no router run was allowed); only `java-ref` was.

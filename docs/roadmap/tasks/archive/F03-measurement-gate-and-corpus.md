# F03 — Measurement gate and corpus

Opened 2026-10-15. Quality changes (Q series), alternative algorithms (X
series) and the one deliberate performance divergence (P04) are kept only if
measurements say so. The rules are in
[routing-quality.md](../../routing-quality.md):
- at least three distinct boards, paired and directional;
- improve on N−1 and regress on none;
- a baseline file;
- the KiCad oracle.
This checklist builds the tool that enforces them, the corpus they run on,
and the record that ties a failure to what a human did on the same board. The
methods come from KiCadRoutingTools (KRT), which A/B-tests every engine change
this way. Its numbering in the research report is superseded by these ids.

Prerequisites: F02.2 (`ses-diff`), F02.3 (`bench`), F02.6 (the KiCad oracle).

## What exists and what this adds

| Exists | State |
|---|---|
| `bench` (F02.3), `ses-diff` (F02.2), the KiCad oracle (F02.6) | Produce per-board, per-engine numbers |
| KRT's `tests/test_placement_ab.py` gate and `tests/placement_ab_baseline.json` | The model for the comparator: named problem classes, a self-test, a baseline file |
| KRT's corpus manifests (`tests/stress/manifest_set*.json`) | 479 GitHub boards; only 31 pinned to a commit; 372 with licence "see repo". Usable as candidate lists only |

## Decisions made here

- **The corpus stays out of the repo.** A manifest with pinned commits,
  hashes and SPDX licences is committed. The boards are fetched into
  `$FR_CORPUS_DIR`. A committed board needs an allow-listed licence.
- **The gate is a program, not a convention.** The pass rule lives in code,
  with a self-test that runs on every invocation, so it can't drift from the
  docs.
- **Preregistration.** Before an A/B, its arms, boards, metrics and pass rule
  go in a JSON file in `tools/ab/`, committed first. The result is compared
  against what was registered. That is KRT's `*_prereg.json` practice; it
  stops a result from choosing its own criteria afterwards.

## File ownership

| Item | New files | May edit |
|---|---|---|
| F03.1 | `crates/freerouting-dev/src/gate.rs` | `crates/freerouting-dev/src/main.rs` (append a subcommand) |
| F03.2 | `crates/freerouting-dev/src/failed_vs_human.rs` | `crates/freerouting-dev/src/main.rs` (append) |
| F03.3 | `tools/corpus/manifest.json`, `tools/corpus/fetch.sh`, `tools/corpus/README.md`, `tools/bench/sets/kicad-corpus.txt` | — |
| F03.4 | `tools/ab/README.md`, `tools/ab/TEMPLATE.json` | `crates/freerouting-dev/src/gate.rs` (read prereg files) |

## Checklist

- [x] **F03.1 — `freerouting-dev gate`: a baseline comparator.**
  - **Input:** per-board, per-arm records (from `bench` and the oracle) and a
    baseline JSON.
  - **Problem classes:**
    - INVERTED: a direction or mark reversed; zero tolerance
    - DRIFT: integers must be exact; floats get a relative tolerance
    - ORPHAN: a baseline row the run doesn't produce; never scoped away
    - MISSING
    - NEW ROW
    - MALFORMED: reported, not raised
  - **Pass rule:** at least 3 distinct boards; improves on at least N−1,
    regresses on none. Neutral boards are printed. The 1-in-2^N chance of
    passing on coin flips is printed with the result.
  - **Self-test** on every invocation.
  - **Baseline file:** `--write-baseline` refuses a partial run, and a
    missing baseline fails.
  - **Accept:**
    - Unit tests reach every branch.
    - A reversed row reports INVERTED, not DRIFT.
    - A deleted row reports ORPHAN.
    - A non-object baseline reports MALFORMED with exit 1 and no panic.
    - Two improving rows on one board are refused as "N=1 distinct boards".

  *Done 2026-10-16:* `crates/freerouting-dev/src/gate.rs` (types, mark and pass
  rule, MISSING/UNGRADED/HARD checks), `gate_baseline.rs` (the comparison and
  `--write-baseline`), `gate_selftest.rs` (runs first on every invocation of
  `freerouting-dev gate`), `gate_cli.rs`. Problem classes INVERTED, DRIFT,
  ORPHAN, MISSING, NEW ROW, MALFORMED plus UNGRADED ("not graded" is never a
  pass), HARD (via in pad and copper below the board minimums fail the gate in
  the candidate arm, D8) and REFUSED. `crates/freerouting-dev/tests/gate.rs`
  (38 tests, `cargo test -p freerouting-dev --test gate`) reaches every branch:
  `a_reversed_row_is_inverted_not_drift` (the INVERTED line replaces the DRIFT
  lines of that key), `a_deleted_row_is_an_orphan_and_scope_never_hides_it`,
  `malformed_baselines_are_reported_not_raised` plus
  `cli_reports_a_non_object_baseline_as_malformed_with_exit_1` (baselines
  `[1, 2]`, `42`, `"rows"`, not JSON: exit 1, "MALFORMED", no panic),
  `two_improving_rows_on_one_board_are_refused_as_n_1` (message contains
  "N=1 distinct boards"), `the_pass_rule_branches`,
  `cli_refuses_a_partial_baseline_and_two_rows_of_one_board` (`--write-baseline`
  refuses a partial run; a missing baseline fails with exit 1). The luck rate
  is printed (`N=3: ... 1 run in 8`). Real data: `gate collect` + `gate` on
  six corpus boards, Java at `-mp 1` against `-mp 3`, graded by the KiCad
  oracle (KiCad 10.0.7): the gate fails the run with `HARD ... below_minimum`
  on 5 of 6 boards (Java's copper is below the board's track-width minimum,
  which the DSN loses) and marks 0 boards improved, so it refuses a result a
  count of unrouted connections alone would have accepted.

- [x] **F03.2 — Failed connections against the human original.**
  For corpus boards and Dan's boards, list every connection an engine leaves
  unrouted next to the human-routed original's layers, vias, widths and
  clearance headroom. Tag each one:
  - "router gap": the human routed it;
  - "infeasible": the Q03 diagnostic says width plus clearance can't fit.
  The pattern is KRT's `tests/stress/compare_failed_nets.py`.
  - **Accept:**
    - On one corpus board, every unrouted connection has a human counterpart
      or "no human route".
    - Both tags appear, or their absence is explained.

  *Done 2026-10-16:* `crates/freerouting-dev/src/failed_vs_human.rs`,
  `freerouting-dev failed-vs-human board.dsn engine.ses human.dsn [--json f]`;
  `tools/corpus/prep.sh` writes the human original as `<id>.human.dsn`. Tags
  `router gap`, `no human route`, `infeasible` (the Q03 `check_board` verdict).
  Human connectivity is judged by overlap of same-net copper, because the
  engine's contact model joins a trace to a pin only at its centre and KiCad's
  DSN export rounds track ends to whole micrometres (the first version reported
  25 of 40 connections of `mydevboard-rp2040dev` as "no human route" for that
  reason). On the corpus board `mydevboard-rp2040dev` (Java `a773a70ec`,
  `-mp 3`): 40 unrouted connections, 38 `router gap`, 2 `no human route` (GND
  connections the human closed with a pour the DSN doesn't carry), each with the
  human's layers, vias, widths, length and headroom. Other boards: eprom-card-80bus
  35/35 gap, rio-iceshield 23/23, laser-backplane-dvi 45/45, pcietest 5/5. Both
  tags: Dan's canary-pwr 9511d9a (Java `46bf9e2ef` session against the routed36
  board, exported with tracks kept): 102 unrouted, 49 `router gap`, 53 `no human
  route`, 7 `infeasible` (Q03: GND to U3-7, +12V_PROT to U1-34, ...). Both
  tags also appear on the corpus board `cm4-edge-ai-carrier`: 7 unrouted, all
  7 `router gap`, 4 of them `infeasible` (Q03: no 0.5 mm trace reaches U2-7,
  U2-14 or Module1-77; the human routed those nets at 0.3 to 0.5 mm). Q03 finds
  candidate pins on only 5 of the 30 corpus boards (uniform class widths give no
  neck-down candidates elsewhere) and infeasible ones on only that board, so
  the other 29 boards show only `router gap` and `no human route`.
  `crates/freerouting-dev/tests/failed_vs_human.rs` (6 tests, synthetic QFN with
  hand values: both tags appear, headroom -0.625 mm and +0.725 mm by hand).

- [x] **F03.3 — An out-of-repo corpus of open KiCad boards.**
  - **Manifest:** `tools/corpus/manifest.json` with about 30 boards: 2, 4 and
    6 layers, easy to hard, at least 5 with planes. Seed it from KRT's
    manifests as candidate data only.
  - **Per entry:** repo, a 40-hex commit, path, sha256, the SPDX licence read
    from the repo at that commit, attribution, layer count, tier, and
    whether it has planes.
  - **Fetch:** `tools/corpus/fetch.sh` fills `$FR_CORPUS_DIR` and checks the
    hashes.
  - **Prep:** strip tracks and vias but **keep zones and layer types**
    (KRT's prep drops both), then export through F02.6.
  - **Accept:**
    - Every entry has a commit, a sha256, and an SPDX id or `LicenseRef-none`.
    - Fetching twice gives byte-identical files.
    - A test refuses any committed board whose licence isn't allow-listed.
    - `tools/bench/sets/kicad-corpus.txt` routes Java-only through `bench`
      and is graded through F02.6.

  *Done 2026-10-16:* `tools/corpus/manifest.json` (30 boards: 12 two-layer, 12
  four-layer, 6 six-layer; tiers easy 5, medium 16, hard 9; 18 with inner-layer
  planes; plus `owner_boards`: Dan's three canary-pwr 9511d9a DSNs, noted "graded
  by canary-09 through Windows KiCad"), `fetch.sh`, `prep.sh`, `seed.py` (how the
  manifest was built from KRT's candidate lists: commit, licence, companions and
  hashes resolved with `gh api`), `allowed-licenses.txt`, `README.md`,
  `tools/bench/sets/kicad-corpus.txt`. Licences: 15 permissive (10 MIT, 2 CC0, BSD-2, BSD-3, Apache-2.0), 9 CERN-OHL or
  GPL, 6 `LicenseRef-none`; the 15 permissive ones are committable, none is committed. Accept:
  - every entry has a 40-hex commit, a sha256 and an SPDX id or `LicenseRef-none`:
    `every_entry_has_a_commit_a_sha256_and_an_spdx_id_or_licenseref_none`;
  - fetching twice: `FR_CORPUS_DIR=a tools/corpus/fetch.sh` and `=b` give
    `diff -r a/src b/src` identical (64 files, 29 MB, 12 s); also
    `fetch_accepts_files_that_match_their_hash_without_the_network`; `prep.sh` run
    twice gives identical DSNs;
  - `a_committed_board_with_a_licence_that_is_not_allow_listed_is_refused` and
    `no_committed_board_in_this_repository_has_a_licence_that_is_not_allow_listed`
    (`tests/corpus.rs`, 9 tests);
  - `freerouting-dev bench --set tools/bench/sets/kicad-corpus.txt --java
    freerouting-a773a70ec.jar --max-passes 3 --max-load off`: all 30 boards
    routed Java-only with statistics, none timed out (wall time 5 s to 595 s,
    taken on a busy machine and not a timing result; unrouted 0 to 59);
    `tools/kicad-oracle/grade-set.sh target/corpus/grade-set.txt ...` graded all
    30 boards with KiCad 10.0.7 (none "not graded"; 2 clean, 28 with
    router-attributable items, mostly copper below the board's track-width
    minimum, which the DSN loses).
  Two bugs of `tools/kicad-oracle/grade-set.sh` were fixed on the way: its `tr`
  set `'A-Za-z0-9_.-\n'` is a reversed range (the output directory name came out
  empty and every board overwrote the previous), and `| head -n 1` killed
  `grade.sh` with SIGPIPE, so every status read 141 instead of 0 or 1.

- [x] **F03.4 — Preregistered A/B runs.**
  `tools/ab/TEMPLATE.json` declares the arms (engine and settings), the
  boards (set file plus hashes), the metrics with directions, and the pass
  rule. `gate --prereg <file>` refuses a run whose arms, boards or metrics
  differ from the registration, and reports the result against it.
  - **Accept:**
    - Changing a board between registration and run is refused with the
      board named.
    - A registered metric missing from the run is reported as MISSING.
    - The template is documented in `tools/ab/README.md`, with one worked
      example: the X01 head-to-head, re-registered.

  *Done 2026-10-16:* `tools/ab/TEMPLATE.json`, `tools/ab/README.md`,
  `tools/ab/examples/x01-head-to-head.prereg.json` (the X01 head-to-head
  re-registered: arms `java` and `krt`, five boards with their input hashes,
  hard/guard/score metrics, the default pass rule; sha256 pinned by
  `the_example_registration_is_pinned_by_its_hash`),
  `crates/freerouting-dev/src/gate_prereg.rs` (`gate --prereg <file>`).
  Tests in `tests/gate.rs`: `changing_a_board_between_registration_and_run_is_refused_naming_the_board`
  ("board b changed between registration and run"),
  `a_registered_metric_missing_from_the_run_is_missing` (MISSING at the list and
  per row), `arms_that_differ_from_the_registration_are_refused`,
  `a_registration_may_not_loosen_the_global_rule` (fewer than 3 boards, more than
  N-1 non-improving, any regression, or a downgraded hard limit is refused),
  `the_x01_example_registration_checks_a_run_and_refuses_a_changed_board`,
  `the_template_with_its_placeholders_filled_in_is_a_valid_registration`.


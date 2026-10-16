# F03 — Measurement gate and corpus

Opened 2026-10-15. Quality changes (Q series), alternative algorithms (X
series) and the one deliberate performance divergence (P04) are kept only if
measurements say so. The rules are in
[routing-quality.md](../routing-quality.md):
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

- [ ] **F03.1 — `freerouting-dev gate`: a baseline comparator.**
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

- [ ] **F03.2 — Failed connections against the human original.**
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

- [ ] **F03.3 — An out-of-repo corpus of open KiCad boards.**
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

- [ ] **F03.4 — Preregistered A/B runs.**
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

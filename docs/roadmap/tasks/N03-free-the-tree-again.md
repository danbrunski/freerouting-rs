# N03 — Free the search tree, judged by the totals rule

Opened 2026-10-17 (D10). P04 measured the canonical room order (P04.1) and the
leaf work unit (P04.2) on 17 boards. With the canonical order:
- total unrouted fell from 1,281 to 1,269;
- vias fell from 5,102 to 5,057;
- length fell from 113,870 to 113,538 mm;
- maze work halved (39.1 G nodes → 19.6 G);
- 5 boards regressed under the paired rule.

KiCad grading was mixed:
- coldfire improved (235/85 → 213/77);
- canary-pwr improved slightly (39/71 → 38/70);
- interf_u got worse (9/17 → 11/20).

GP8B went from 0 to 2 router-counted clearance violations. Under N02's rule that
would be a hard-guard failure if KiCad confirms it.

The implementation is saved at `target/orchestrator/p04/p04-rejected-canonical-leaves.patch`
(it applies to `93c326f`). Once the room order no longer depends on the tree's
shape, the tree can be replaced (P04.3). That is the largest speed lever left.

Prerequisites: N01 (profiles), N02 (the totals rule), P04's patch, P02.1's audit.

## Checklist

- [x] **N03.1 — Restore the code behind settings.** `router.room_order = tree |
  canonical` and `router.work_unit = nodes | leaves`, rebased onto main. `legacy`
  pins `tree` and `nodes`.
  - **Accept:**
    - with `legacy`, the A14 exact set and the B02, B03 and A04 probes are
      unchanged;
    - P04's order-independence tests pass with `canonical`.
  *Partial 2026-10-17:* the code is restored and Accept 2 holds; Accept 1 holds
  for the probes and 15 of A14's 16 exact boards. Remainder: heavy run pending
  (N03.1, the A14 boards not yet run, 1 exact + 6 cutoff):
  `PARITY_BOARDS=Issue690-kit-dev-coldfire-xilinx_5213,Issue269-z10_module,Issue753-CPU-85_r104,Issue066-Project_GP8B,Issue145-smoothieboard,Issue219-LogicBoard_smt,Issue420-contribution-board cargo test --release -p freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1 --nocapture`
  (want: exact 1 of 1 and cutoff 6 of 6 pass and byte-identical, as N01.2).
  *Done 2026-10-18:* the remainder ran on `f13ac02` (release, `--test-threads=1`):
  coldfire (exact) and z10, CPU-85, GP8B, smoothieboard, LogicBoard, contribution-board
  (cutoff) all pass, `eq true bytes true`, 0 nets differ, D8 breaches as Java's; 1 passed,
  0 failed in 2,284 s. Accept 1 now holds for all 16 exact boards and the probes.
  - **Code.** `target/orchestrator/p04/p04-rejected-canonical-leaves.patch`
    applied to `84ad1a3` with `git apply --3way`, every file cleanly. On top:
    the settings fields `room_order` and `work_unit` (`settings/fields.rs`,
    unset by default = `tree`, `nodes`; trimmed, blank is unset); the
    connection router slot's options in the stage catalog
    (`autoroute/stages/catalog.rs`), so the CLI's `--list-stages`
    (`router.room_order = (unset)`, and the JSON), the GUI's Algorithms section
    (a control, so no `CLI_ONLY` row) and the listing the plugin reads show them;
    `job/prepare.rs` refuses an unknown value (strict path: `JobError::Settings`
    naming the key and the valid values; GUI path: logged, and the pipeline
    refuses it); the pipeline logs `Search trees: room order <o>, work unit <u>`
    only for a non-Java pair (`SearchConfig::is_java`), so a `legacy` log is
    unchanged; the run-statistics counters `maze_work_limit_hits`,
    `fanout_work_limit_hits`, `tree_leaves_returned` sit before
    `tree_nodes_visited`. Neither key is in `ENHANCED` (N03.3 decides, through
    N05). DIVERGENCES: board-model line "`room_order` and `work_unit` (N03.1…)".
  - **Legacy pins both.** `tests/search_order_independence.rs::legacy_pins_tree_order_and_nodes`:
    `legacy` and `enhanced` resolve to `SearchConfig::default()` (tree, nodes);
    with a dummy `enhanced` difference (`room_order = canonical`,
    `work_unit = leaves`) `enhanced` gets both and `legacy` still none; both keys
    are connection-router options with an unset default.
    `prepare_job_refuses_unknown_search_values`: `sideways` and `seconds` fail
    the job naming `router.<key>` and the valid values; ` canonical ` and
    `leaves` pass.
  - **Accept 2.** `cargo test -p freerouting --test search_order_independence`:
    5 passed (`canonical_routes_do_not_depend_on_tree_shape`: my-board and
    CONGFLOCK route to equal SES and pass records in reversed-build trees under
    `canonical` with either unit, and my-board differs under `tree`;
    `leaf_counts_and_canonical_rooms_do_not_depend_on_tree_shape`;
    `the_job_sets_the_configuration_and_refuses_unknown_values`; the two above).
    The `complete_shape` unit tests
    `canonical_order_is_the_sorted_fold_and_ignores_insertion_order` and
    `the_gap_never_shrinks_inside_a_bound` pass in the workspace run.
  - **Accept 1, probes.** In `cargo test --workspace --no-fail-fast`:
    `board_probe_tree` 1, `board_probe_tree_visits` 1, `board_probe_tree_ops` 2,
    `board_probe_queries` 13 (B02), `board_probe_complete_shape` 2 (B03),
    `autoroute_probe_maze` 2, `autoroute_probe_control` 7 (A04), `work_limit` 13,
    `reference_load` 1, `reference_drc` 4, all pass.
  - **Accept 1, A14 exact set** (release, the gate pins `legacy`; defaults are
    `tree`, `nodes`): `PARITY_SET=tools/bench/sets/connection-parity.txt`: exact
    7 of 7 pass, 7 SES byte-identical; `PARITY_SET=tools/bench/sets/benchmark.txt
    PARITY_MAX_JAVA_S=10`: exact 6 of 6, 6 byte-identical; `PARITY_BOARDS=
    Issue508-DAC2020_bm01,Issue508-DAC2020_bm10,Issue555-CNH_Functional_Tester_1`:
    exact 3 of 3, 3 byte-identical. 0 D8 breaches beyond Java's in each run.
  - **Gate** (worktree at `84ad1a3` plus this change): `cargo fmt --all --check`
    clean; `cargo clippy --workspace --all-targets -- -D warnings` clean;
    `cargo test --workspace --no-fail-fast`: 206 test binaries, 2,245 passed, 0
    failed, 56 ignored; `tools/check-provenance.sh`: provenance ok.
- [ ] **N03.2 — The GP8B violations.** Find out why the canonical order adds 2
  clearance violations on GP8B, and fix it or explain it, before measuring.
  - **Accept:** the cause is in the done note; KiCad grades GP8B with the
    canonical order and no new `drc_real` items.
- [ ] **N03.3 — Measure under N02's rule.**
  - Register before the first number, on boards P04 didn't use:
    - the PCBWorld D3-C set;
    - canary-pwr variants not in `p04.txt`;
    - KiCad demos with existing tracks.
  - Arms: `tree+nodes` (baseline), `canonical+nodes`, `canonical+leaves`.
  - **Accept:** measured. If one arm is accepted, it becomes `enhanced`'s default
    through N05, with a DIVERGENCES line.
- [ ] **N03.4 — The faster tree** (P04.3's plan, only after N03.3 accepts an arm).
  - Try a bulk-loaded R-tree with struct-of-arrays bounds, plus incremental leaf
    updates. The result sets must be identical, so routes are identical against
    N03.3's build.
  - `router.search_tree = legacy | fast`, where `legacy` stays the parity tree.
  - **Accept:**
    - `ses-diff` identical on the N03.3 set;
    - wall time and callgrind before and after;
    - the P02.1 audit is clean;
    - determinism in both modes.

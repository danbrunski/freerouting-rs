# Plugin and command-line preparation

Opened 2026-10-18: CANary-PWR's historical command-line and plugin runs used
different exporters, fill states and inputs. Matching router flags alone did
not establish equivalent jobs.

Use `export_flow` for the editor and the supported scripted board flow.
Refill the current board before preparing DSN or a direct-reader snapshot;
stop if filling fails. Keep outline and pad clearance protection. Record
the prepared input hash and router arguments beside each export. This does
not change an engine algorithm, profile or Java behaviour.

Files: `kicad-plugin/freerouting_rs/export_flow.py`, `plugin.py`,
`kicad-plugin/headless_run.py`, plugin tests and documentation.

- [x] **K12.1 — One board preparation and invocation.**
  **Accept:** tests establish refill-before-export/snapshot, refusal on fill
  failure, restored locks on export failure, current Board Setup minimum
  width in both frontends, identical prepared input/arguments from an editor
  run and a scripted run, and a real small-board routing run through the
  shared CLI entry. Verify a CANary copy exports reproducibly without saving
  its source. Record tests and input comparison; do not claim the historical
  33-connection result has been reproduced without grading a routed board.

  *Done 2026-10-18:* `test_export_flow.py` checks refill order for DSN and
  snapshot, fill failure/exception refusal, restored locks after export
  failure/exception, current minimum width and recorded SHA256, and drill
  correction before post-import refill. `test_route_dialog.py::test_route_uses_and_records_the_current_board_rules`
  checks the actual dialog invocation. `test_scripted_flow.py` compares real
  KiCad exports with cached fill present/cleared, checks source preservation,
  refuses preparing over the source project, and compares sessions from
  `headless_run.py route` and the editor's `RouteRun` byte for byte except
  the output session name, on Issue558. With KiCad 10.0.7 and the release
  router built from `7e7054186996`,
  `FREEROUTING_RS_BIN=$PWD/target/orchestrator/k12-target/release /home/thebo/code/canary/.venv/bin/python -m pytest kicad-plugin/tests -q`:
  **375 passed, 3 skipped**, 68.51 s. The initial run against the frozen
  `298af027db60` binary failed the existing long-fanout Stop test; the current
  binary passes it. `cargo fmt --all --check` and
  `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/orchestrator/k12-target cargo clippy --workspace --all-targets --offline -- -D warnings`
  pass. No Rust source, engine defaults or dependency changed.
  After strengthening the cached-fill fixture to save actual filled copper,
  the same command limited to `kicad-plugin/tests/test_scripted_flow.py -q`
  reports **3 passed**, 4.18 s.

  CANary export-only check: `python -I target/k12-check-canary.py`, evidence
  `target/k12-canary/comparison.json`. The saved board SHA256 was
  `e16d17844f99a1df3a9a8a806e0ba67df6b14d273abcfaf6d79478c58fcf9952`.
  Scripted export versus `export_flow.prepare` after clearing cached fills:
  identical complete DSN trees except the destination pathname, identical
  arguments, matching recorded input hashes, and unchanged source PCB,
  project and design-rule hashes. Both retain 144 fixed pour wires, 128 edge
  strips and six pad circles, with 16 outer plane scopes removed. No CANary
  routing count was measured and the historical 33 result is not claimed.
  The older external CANary exporter remains separate; the documented shared
  CLI entry replaces it for comparisons. No live editor or Windows install
  was touched.

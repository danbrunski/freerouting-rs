# X01 head-to-head: Java Freerouting against KiCadRoutingTools

The scripts of [X01](../../docs/roadmap/tasks/archive/X01-krt-head-to-head.md): route
`canary-pwr` and four KiCad demo boards with the Java fork's jar and with
KiCadRoutingTools (KRT), grade both with the same KiCad DRC
([tools/kicad-oracle](../kicad-oracle/README.md), F02.6), and sort the
connections one engine routes and the other doesn't by cause. The registration
is [docs/roadmap/prereg/X01.json](../../docs/roadmap/prereg/X01.json), pinned by
`crates/freerouting-dev/tests/prereg_x01.rs`; the board set is
[tools/bench/sets/x01-h2h.txt](../bench/sets/x01-h2h.txt).

```sh
export X01_DIR=~/.cache/freerouting-rs/x01        # the default; outside both source repos
tools/krt-h2h/setup-krt.sh                        # KRT export, venv, grid_router, smoke tests
tools/krt-h2h/build-java.sh --frozen ~/code/freerouting/logs/jars/freerouting-a773a70ec.jar
tools/krt-h2h/prep.sh                             # prepared inputs + their own DRC
tools/krt-h2h/run.sh --arm java --board pic_programmer [--allow-load]
tools/krt-h2h/run.sh --arm krt  --board pic_programmer [--allow-load]
python3 -I tools/krt-h2h/classify.py --out classes.json \
  pic_programmer:$X01_DIR/runs/pic_programmer/java:$X01_DIR/runs/pic_programmer/krt
python3 -m unittest discover -s tools/krt-h2h -p 'test_*.py'
```

## No writes into either source repository

- KRT is exported with `git -C ../KiCadRoutingTools archive 0486e799 | tar -x
  -C $X01_DIR/krt`; its venv (`$X01_DIR/venv`, no system site-packages, so no
  `pcbnew`) and its `grid_router.so` (`build_router.py --from-source`, cargo)
  live there. `setup-krt.sh` records the KRT repo's HEAD and `status
  --porcelain` before and after and fails if they differ. When `$X01_DIR` lies
  under another cargo workspace (for example this repository's `target/`), it
  appends an empty `[workspace]` table to the export's `rust_router/Cargo.toml`
  so cargo builds it standalone, and records that in `krt-env.json`.
- The Java jar is either built in a detached worktree of `../freerouting`
  under `$X01_DIR` (removed afterwards; never in its working tree, which another
  session edits) or copied from the Java session's frozen jars (`--frozen`).
  `build-java.sh` records the jar's sha256, `java -version` and whether it
  equals the KiCad plugin's `freerouting-dev.jar` (read only), and routes
  `fixtures/Issue015-StackOverflow.dsn` as a smoke test.
- Boards are copied: `prep.sh` checksums the originals before and after.

## The arms

- `java`: `java -Xmx3g -jar freerouting-a773a70ec.jar -de {DSN} -do {SES}
  --gui.enabled=false --router.scoring.start_ripup_costs=1500`, run by
  `tools/kicad-oracle/grade.sh --keep-tracks` on the prepared board (export DSN,
  route, import SES, DRC). For `canary-pwr` the jar routes the committed
  `fixtures/canary-pwr/canary-pwr-9511d9a.dsn` instead and nothing is graded
  here.
- `krt`: `python -E -s py_router/route.py {IN} {OUT} --nets '*' --fab-tier
  standard --escalation off --same-net-pad-clearance 0.1 --no-fix-drc-settings
  --keep-input-copper` in the export, with `PATH` limited to the venv and a
  directory of links to `/usr/bin` without any `kicad*` name, and `KICAD_CLI`
  unset: no `kicad-cli` reachable and no `pcbnew` in KRT's own interpreter.
  **Not hidden:** KRT's exact-fill step probes `/usr/bin/python3` by absolute
  path (`kicad_locate.kicad_python_candidates`), and the WSL-native KiCad
  installs `pcbnew` there, so the X01 runs used KiCad's exact zone fill, not
  KRT's raster fallback (registration amendment 3). Hiding it needs a mount
  namespace over `/usr/lib/python3/dist-packages/pcbnew.py`. The flags hold KRT to the
  board's rules (D8): KRT's default `--fab-tier auto --escalation fab` drops to
  advanced-tier widths and vias below the board's minimums, its default allows a
  via in a pad, and by default it rewrites the output project's DRC minimums.
  `python -I` is not usable: it implies `-P` on Python 3.11+, and route.py then
  can't import its own modules.
- `krt-kicad` (optional): the same with `kicad-cli-shim.sh` as `kicad-cli`. The
  shim runs the native `/usr/bin/kicad-cli` (or a Windows `kicad-cli.exe` with
  `wslpath -w` path translation, never used here) and logs every call. In the
  smoke tests KRT made no KiCad call at all, so the arm was not run.

## Grading

`grade_routed.sh INPUT ROUTED` is the grade-only entry for engines that write a
board: the same `oracle_lib.py`/`kicad_steps.py` steps as `grade.sh` after its
SES import (original rules copied over both boards, copper severities forced to
error, `kicad-cli pcb drc --severity-all --refill-zones`, copper from geometry,
input items subtracted). Passing the input as ROUTED grades the input itself;
`prep.sh` does that once per demo. `canary-pwr` is graded only by the CANary
layout session (canary-23) through Windows KiCad, never here.

`run.sh` adds two engine-independent counts: `copper_stats.py` (an
S-expression scan: vias, segment and arc length added relative to the input,
per layer) and `via_in_pad_kicad.py` (KiCad's python: vias whose centre hits an
SMD pad of any net; uses `BOARD.Tracks()`). It waits for the 1-minute load
average to reach 1.0 (F02.3's rule) and refuses to start above it unless
`--allow-load`, recording the load before and after either way, and runs one
router or KiCad job at a time. One `record.json` per board and arm.

## Files

| File | Runs with | Purpose |
|---|---|---|
| `common.sh` | bash (sourced) | `X01_DIR`, pins, the no-KiCad `PATH` |
| `setup-krt.sh` | bash | X01.2: KRT export, venv, build, smoke tests, `krt-env.json` |
| `kicad-cli-shim.sh` | bash | X01.2: logging `kicad-cli` for the optional arm |
| `build-java.sh` | bash | X01.3: the jar, `java/java-env.json` |
| `prep.sh`, `prep_board.py` | bash, KiCad's python | X01.4: prepared inputs, `boards/prep.json` |
| `run.sh`, `grade_routed.sh` | bash | X01.5: one board and arm, `runs/ROW/ARM/record.json` |
| `copper_stats.py` | python3 | X01.6: copper added, independent of both engines |
| `via_in_pad_kicad.py` | KiCad's python | X01.6: vias in SMD pads |
| `test_copper_stats.py` | python3 (+ KiCad's python) | X01.6 tests |
| `classify.py` | python3 (+ KiCad's python for the pad list) | X01.7: failures by cause |
| `results/x01-demos.json` | — | X01.8: the demo boards' records and classes |

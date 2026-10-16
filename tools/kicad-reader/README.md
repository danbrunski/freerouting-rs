# Tools for the `.kicad_pcb` reader (K04)

| Script | What it does |
|---|---|
| `fetch-corpus.sh <dir>` | Fills the out-of-repo corpus: the Java repo's 38 KiCad boards (`git show cda540f`), KiCad's demos, a snapshot of `canary-pwr`. `--committed` re-extracts `fixtures/kicad/`. |
| `export.sh [--windows] <board> <out.dsn>` | Exports KiCad's own DSN of a **copy** of the board. Checks the KiCad processes before and after (`pgrep -x`, or `tasklist.exe` for `--windows`) and fails if they differ. |
| `export_dsn.py` | The KiCad-side step of `export.sh` (`pcbnew.LoadBoard`, `ExportSpecctraDSN`, prints the build version). Never imports the plugin package. |
| `export-corpus.sh <dir>` | `export.sh` for every board of a corpus folder. |
| `manifest.py write\|check` | Writes and verifies `fixtures/kicad/MANIFEST.tsv`. |
| `old_arcs.py <scratch dir>` | Loads old-syntax arcs and overbar net names in KiCad 10 and prints what it holds (pinned in `crates/freerouting-kicad/tests/legacy_kicad.rs`). |

Rules: run KiCad only on copies; never close or restart a running KiCad (the
`pgrep` check is there to prove it); `export.sh` refuses a scratch folder inside
`fixtures/` or the canary repo. Use `TMPDIR=<scratchpad>` to keep scratch
folders out of `/tmp`.

Two KiCads: the default is WSL's KiCad 10.0.7 (system `python3` imports
`pcbnew`, no path translation); `--windows` runs Windows KiCad 10.0.5's
`python.exe` (the version Dan's plugin runs in) with paths through `wslpath -w`.

Determinism of the exports: KiCad orders non-default net classes through a
`std::unordered_map` in `SPECCTRA_DB::FromBOARD`. On the seven committed boards
exporting twice in two processes gave identical files (checked with
`KEEP_FIRST_LINE=1`, first line aside), but a board with several non-default
classes may order them differently on another libstdc++. Compare class by name,
not by position.

Also checked: `CNH_Functional_Tester_1` (9 classes) exported three times with 10.0.7 gave
identical files. 10.0.5 and 10.0.7 differ on that board (inner layer `(type signal)` vs
`(type power)`, outline path segmentation) but not on the seven committed boards.

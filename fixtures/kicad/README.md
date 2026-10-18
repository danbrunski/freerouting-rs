# `.kicad_pcb` test boards and KiCad 10 reference exports (K04.7, K04.8)

Test data for `crates/freerouting-kicad`, the direct `.kicad_pcb` reader (K04 to
K07). Seven boards from the Java repository's fixtures, one per KiCad file
generation (6, 7, 8, 9, 10) plus the two rule-area boards, and for each board
the Specctra DSN that KiCad 10 itself exports, so the reader can be compared
with KiCad bit for bit without KiCad installed.

| Board | KiCad | Why |
|---|---|---|
| `Issue368-CorneyIslandWireless/corney_island_wireless` | 8 (`20240108`) | back-side footprints, NPTH holes |
| `Issue269-min_fr_test/min_fr_test` | 8 (`20240108`) | 4 copper layers, a zone |
| `Issue508-DAC2020/DAC2020_bm01/bm1.unrouted` | 9 (`20241229`) | no `.kicad_pro` (KiCad defaults) |
| `Issue558-dev-board-autoroute-demo/dev-board` | 9 (`20241229`) | a rule area, back side, oval and roundrect pads |
| `Issue742-tastexx-pcb/tastexx-pcb` | 10 (`20260206`) | name-net dialect |
| `Issue184-motorizedopener/motorizedopener` | 6 (`20211014`) | KiCad 6 project format, 10 back-side parts |
| `Issue180-Test/Test` | 7 (`20221018`) | 4 layers, 627 locked vias, a copper zone and 5 rule areas |

None of them has the old arc or overbar syntax (the oldest is `20211014`); that
code is tested on hand-written snippets and against KiCad 10.0.7
(`tools/kicad-reader/old_arcs.py`).

## Files

- `<board>.kicad_pcb` and, where the Java repository has one, `<board>.kicad_pro`:
  byte for byte `git -C ../freerouting show cda540f:fixtures/<path>`.
- `<board>.kicad10.dsn`: `pcbnew.ExportSpecctraDSN` of KiCad 10.0.7 (WSL, Ubuntu
  package), made by `tools/kicad-reader/export.sh`. The first line holds the
  export path in KiCad's output; the script replaces it with the board's name
  (`(pcb "<stem>.dsn"`) so that re-exporting gives an identical file. Parity
  tests ignore the `pcb` name anyway.
- `MANIFEST.tsv`: path, source (`freerouting@cda540f:<path>`), sha256, bytes,
  file version, `.kicad_pro` present, licence, and for the export its path,
  sha256 and the `(host_version ...)` it recorded. `python3 -I
  tools/kicad-reader/manifest.py check` verifies it against the files and
  against `git show`; `crates/freerouting-kicad/tests/fixtures.rs` checks the
  same without hashing.

## Provenance and licence

The boards come from the Java repository `danbrunski/freerouting` at `cda540f`
and carry the licence tag `LicenseRef-freerouting-fixture`, the status of the
existing `fixtures/*.dsn` that came from the same place. F03.3's licence rule
(`tools/corpus/allowed-licenses.txt`, enforced by
`crates/freerouting-dev/tests/corpus.rs`) matches committed files by sha256
against the corpus manifest; these boards are not in that manifest, so the rule
accepts them. If the rule is ever extended to whole directories, these seven
move out of the repository into `$FR_KICAD_CORPUS`.

## The rest of the corpus (out of the repository)

`tools/kicad-reader/fetch-corpus.sh target/kicad-corpus` copies the other 31
Java-repo boards (`git show`), KiCad 10's 19 bundled demos with their licence
files, and a snapshot of `canary-pwr` (`SNAPSHOT.sha256` pins it, because the
board is being edited) into `$FR_KICAD_CORPUS`. `tools/kicad-reader/export-corpus.sh`
adds a `.kicad10.dsn` next to every board. `tools/bench/sets/kicad-reader.txt`
lists them all. Tests that need the corpus skip with a note when it is missing.

## Regenerating the exports

```sh
tools/kicad-reader/export.sh fixtures/kicad/Issue368-CorneyIslandWireless/corney_island_wireless.kicad_pcb \
    fixtures/kicad/Issue368-CorneyIslandWireless/corney_island_wireless.kicad10.dsn
python3 -I tools/kicad-reader/manifest.py write
```

`--windows` uses the owner's KiCad 10.0.5 instead. On the seven boards the two
exports differ only in the `(host_version ...)` line (Windows KiCad writes CRLF;
`export.sh` converts it to LF).

## K07 additions

- `Issue558-dev-board-autoroute-demo/dev-board.kicad10.java.ses`: the session Java (`a773a70ec`) writes for
  `dev-board.kicad10.dsn` (the export copied to `dev-board.dsn`, `-mp 100`, optimizer off: 0 unrouted, 231
  wires, 35 vias, 7.5 s). Not a reference output: it feeds the coordinate-frame test
  (`crates/freerouting/tests/kicad_frame.rs`) and `tools/kicad-reader/frame-check.sh`. Regenerate with the Java
  CLI of a detached pin worktree:
  `java -cp <classes>:<resources>:<deps> app.freerouting.Freerouting --user_data_path=<tmp>/userdata -da
  --gui.enabled=false --api_server.enabled=false --mcp_server.enabled=false -de dev-board.dsn -do java.ses
  --router.optimizer.enabled=false -mp 100`.
- `synthetic/rules.kicad_dru`: three rules for the `classes` board (K07.5): a class-pair clearance, a class track
  width and an `intersectsArea` rule the DSN cannot say.
- Frame check (K07.4): `tools/kicad-reader/frame-check.sh <board.kicad_pcb> <a.ses> <b.ses> [outdir]` imports the
  two sessions into copies of the board with KiCad's `ImportSpecctraSES` and compares every track endpoint
  (`frame_check.py`). Default interpreter: the WSL system python3 (KiCad 10.0.7).
- `python3 -I tools/kicad-reader/manifest.py check` reports "manifest and files disagree on the board list" since
  the synthetic boards (K05.8) were added; they are deliberately not in the manifest.

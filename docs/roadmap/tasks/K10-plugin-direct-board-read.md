# K10 — The plugin reads the board directly

Opened 2026-10-17, after Dan asked whether DSN and SES are the right interface
for the plugin, or whether it should integrate with KiCad more closely, as
KiCadRoutingTools (KRT) does.
- **The answer:** keep DSN and SES as the default. Add a direct read of the board as an
  option, measure it, and let Dan decide whether it becomes the default.
- **The result still comes back through `ImportSpecctraSES`.**

Prerequisites:
- K02 (the routing dialog and options);
- K07 (`-de board.kicad_pcb`, the reader's parity);
- K09, for K10.4 only.

## What exists and what this adds

**How KRT integrates** (read at `0486e799`; ideas only, no code is copied):
- **It is a SWIG action plugin,** like this one (`kicad_routing_plugin/action_plugin.py`).
  It stops loading in KiCad 11 in the same way (K01.5).
- **It snapshots the open board** with `pcbnew.SaveBoard(tmp, board, aSkipSettings=True)`
  and parses the copy itself (`gui_utils.py`).
  - `aSkipSettings` avoids the settings save that aborted KiCad on worker threads for
    projects older than KiCad 10.
  - It also means no `.kicad_pro` is written. KRT re-authors that from the live board
    (`stage_live_project_rules`), because the file on disk lacks the rules changed in
    Board Setup and not yet saved (KRT #627).
- **It writes results onto the open board** with `board.Add(pcbnew.PCB_TRACK(...))`, the
  same for vias, then calls `pcbnew.Refresh()` (`fanout_gui.py`, `board_swaps.py`).
  - It does this because it produces what a session can't carry: teardrops, via tenting,
    footprint swaps and plane refills.
- **The current selection picks the nets to route** (`_get_selected_net_names`).

**What the DSN path loses**, with the item that measured each loss:

| Lost in KiCad's export | Today | With the reader |
|---|---|---|
| Board Setup minimum track width (D8) | the plugin passes `--router.min_trace_width_um` (`board_min_track_width_um`) | not in the reader either: still passed |
| `.kicad_dru` rules | not exported | K07.5 reads the expressible subset (`ReadOptions::apply_design_rules`, off by default; no CLI switch yet) |
| Rule-area kinds | full keepouts; K01.2 rewrites the DSN text (`rule_area_fixup`) | mapped by their flags (K06.2) |
| Footprint-owned copper zones | dropped | kept as planes (K00 question 6) |
| Net-class order | KiCad's `unordered_map` order, not reproducible | the project's order |
| Shape precision | `%.6g`, about 1 µm lost above 100 mm (K00 question 2) | KiCad's nm, rounded to 0.1 µm units |
| Track arcs | one chord per arc | followed as polylines (K06.5) |

**The pieces this checklist joins:**
- `freerouting -de board.kicad_pcb -do board.ses` routes a KiCad board. KiCad 10.0.7's
  `ImportSpecctraSES` imports the session it writes (K07.2, K07.4,
  `cli_kicad.rs::route_job_writes_a_session`).
- The reader reads `<stem>.kicad_pro` and `<stem>.kicad_dru` beside the board
  (`crates/freerouting/src/io/kicad.rs`, `sibling`). A missing one is not an error.
- The GUI opens a `.kicad_pcb` (K07.2), so "Route in GUI" and "Open in viewer" can take
  the snapshot too.

**The open differences from KiCad's export** (K07.6, 57 corpus boards). These are what
K10.5 has to show don't hurt:
- traces the reader merges differently (`video`, `vme-wren` and others);
- courtyards the export drops;
- keepout order;
- one board with an extra pin/via violation (`One-Air-Max`);
- the reader's tree builds into a board about 2× slower (`jetson`: 3.7 s against 1.6 s
  for the DSN).

## Decisions made here

- **DSN and SES stay the default.**
  - The DSN path is the Java comparison (A14) and what the plugin was graded with
    (K02, K09).
  - KiCad 11's IPC API keeps exactly these two board operations,
    `RunBoardJobExportSpecctraDsn` and `ImportSpecctraSession` (K01.5).
  - KiCad's import already does the merge: it skips locked tracks, deletes unlocked
    ones and places footprints.
- **One option, `board_input`, with the values `dsn` (the default) and `kicad_pcb`.**
  - It changes only what the router reads.
  - With `kicad_pcb`, `rule_area_fixup` does nothing, because the reader maps rule areas
    by flags. The run log says so once.
- **The snapshot is taken on KiCad's UI thread,** with `aSkipSettings=True`. It goes into
  the work folder as `freerouting-rs/<stem>.kicad_pcb`, beside `<stem>.dsn`, so the
  session's name is the same as on the DSN path.
- **Results keep coming back through `ImportSpecctraSES`.**
  - Writing to the open board (KRT's `board.Add`) is not part of this checklist.
  - K10.6 records when it becomes worth doing.
- **Making `kicad_pcb` the default is Dan's decision,** after K10.5's measurement. Until
  then the option is off and labelled as experimental in the dialog.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K10.1–K10.4 | `kicad-plugin/freerouting_rs/snapshot.py`, `kicad-plugin/tests/test_snapshot.py` | `plugin.py` (the snapshot call and the input passed on), `options.py` (`board_input`, `apply_kicad_rules`, schema bump), `routing.py` (input path), the options dialog |
| K10.3 | — | `crates/freerouting-cli/src/args.rs`, `cli.rs`, `help.txt` (one switch for `apply_design_rules`), `crates/freerouting-cli/tests/cli_kicad.rs` |
| K10.5 | `tools/kicad-reader/plugin-input-ab.sh` (or a mode of `frame-check.sh`) | — |
| K10.6–K10.7 | — | `docs/development/kicad-plugin.md`, `kicad-plugin/README.md` |

## Checklist

- [ ] **K10.1 — The option and the snapshot.** Add `board_input` to the options, with a
  dialog field. With `kicad_pcb`:
  - `plugin.py` calls `pcbnew.SaveBoard(path, board, aSkipSettings=True)` on the UI
    thread instead of `ExportSpecctraDSN`.
  - "Route", "Route in GUI" and "Open in viewer" pass the snapshot instead of the DSN.
  - The pure parts live in `snapshot.py`, with no `pcbnew` and no `wx` (K00 rule): the
    paths, the files to stage, and the arguments.
  - **Accept:**
    - unit tests give the argument lists for both values, and for a board path with
      non-ASCII characters;
    - in a headless pcbnew run (WSL KiCad 10.0.7, on a copy of `Issue558-dev-board`),
      a track added to the board in memory before the snapshot is on the reader's board;
    - `freerouting -de` on the snapshot writes a session that `ImportSpecctraSES`
      imports with 0 errors.
- [ ] **K10.2 — The project's rules come along.** Stage `<stem>.kicad_pro` and
  `<stem>.kicad_dru` beside the snapshot. Decide by test between two ways:
  - (a) copy the files on disk;
  - (b) re-author the net classes and clearances from the live board, as KRT does.

  The test changes one net class's clearance in memory, without saving: (a) misses it,
  (b) must not.
  - **Accept:**
    - the chosen way passes the test;
    - or, if (a) is chosen, the done note names what is missed, and the dialog tells
      the user to save first when the board has unsaved changes (whether pcbnew can
      tell is unverified: check it here).
  - On KiCad 11, K11.1 found that `SaveCopyOfDocument` with `include_project` writes the
    `.kicad_pro` with the copy, and that `GetDocumentModifiedState` reports unsaved
    changes. Whether that `.kicad_pro` carries unsaved Board Setup changes is still to
    test (K11.6).
- [ ] **K10.3 — Pass what the reader lacks.**
  - The minimum track width stays a CLI argument on both paths.
  - Add one CLI switch for `ReadOptions::apply_design_rules`, named by C02's settings
    conventions, and a plugin option `apply_kicad_rules`, off by default (K00 question
    7).
  - The run log lists the reader's report lines and warnings, as the CLI prints them.
  - **Accept:**
    - `cli_kicad.rs` shows the switch changes the class rules on the `classes` board
      exactly as `design_rules_reach_the_board` does;
    - a unit test shows the plugin passes the switch only when the option is on.
- [ ] **K10.4 — K09's DSN rewrites and the direct read.** K09's options rewrite DSN text,
  so they can't apply to a `.kicad_pcb` input.
  - While any of them is on, the dialog refuses `board_input = kicad_pcb` and names the
    option, until that option has a reader-tree or engine form.
  - Options that already map to an engine setting are allowed, for example K09.2's arm
    (a) if it wins.
  - **Accept:** unit tests of the options check, for each K09 option that exists when
    this item runs.
- [ ] **K10.5 — Measured against the DSN path.** Route each board both ways with the
  same options and grade both with KiCad's DRC (F02.6). Boards:
  - the five K02 demo boards;
  - `canary-pwr` with the options of Dan's plugin run in K09's header (68 unconnected on
    the DSN path);
  - the K07.6 corpus boards that have open differences, when the corpus is present.

  For each board, record:
  - unconnected items and violations by type;
  - the time to snapshot and read, against the time to export and parse.
  - **Accept:**
    - the table is in the done note;
    - every board where the direct read grades worse is explained, or tied to one of
      K07.6's open differences;
    - Dan's answer on the default is recorded (question 1).
- [ ] **K10.6 — When to write to the open board.** Write down the trigger in
  `kicad-plugin.md`. A live write (KRT's `board.Add` plus `pcbnew.Refresh()`) is taken
  up when a stage first produces something a session can't carry, such as teardrops or
  zone fills, and not before.
  - That item must also cover undo. Whether KiCad's undo covers either path is
    unverified.
  - **Accept:** the paragraph exists and names the stages that would need it, with
    their checklist ids where they have one.
- [ ] **K10.7 — Documentation.** `kicad-plugin.md` and the plugin README describe
  `board_input`, the staged project files, `apply_kicad_rules`, the K09 restriction and
  K10.5's numbers.
  - **Accept:** each fact names its test or item.

## Open questions for Dan

1. **The default input after K10.5.** *Recommendation:* switch to `kicad_pcb` only if it
   grades the same or better on every K10.5 board. Otherwise keep `dsn` and list the
   boards that hold it back.
2. **Selection-aware routing**, which KRT has: route only the nets of the selected items.
   It needs a CLI way to route a subset of nets, which does not exist yet.
   *Recommendation:* not in K10. Open it as its own item if Dan wants it.

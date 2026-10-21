"""The direct read of the open board (K10): a snapshot of the board the
router reads with its own `.kicad_pcb` reader, in place of KiCad's DSN export.

`board_input = kicad_pcb` (options.py) makes the plugin call
`pcbnew.SaveBoard(path, board, aSkipSettings=True)` on KiCad's UI thread and
pass the copy to the router. The result still comes back as a session through
`ImportSpecctraSES` (plugin.py `import_ses`, which detaches the unlocked
tracks first). DSN and SES stay the default: the DSN path is what the Java
comparison and the graded runs of K02 and K09 used.

This file is pure Python, like the other modules but `plugin.py`: the paths,
the files to stage and the arguments need neither pcbnew nor wx. The two
functions that call pcbnew (`take` and the live project copy it makes) get the
module passed in, as `kicad_board.py` does, and run on the UI thread only.

**The snapshot** goes into the work folder as `<stem>.kicad_pcb`, beside
`<stem>.dsn`, so the session is named as on the DSN path. `aSkipSettings`
avoids the settings save that aborted KiCad on worker threads for projects
older than KiCad 10 (KiCadRoutingTools issue #688; the idea is theirs, no code
was taken from it). A pcbnew whose `SaveBoard` lacks that argument is refused:
the two-argument form writes the project file itself.

**The project's rules** (K10.2). The reader reads `<stem>.kicad_pro` (net
classes, their clearances and widths) and `<stem>.kicad_dru` beside the board.
Two ways were tested on KiCad 10.0.7 (`tests/test_snapshot_headless.py`):
(a) copy the files on disk, which misses a net class changed in Board Setup and
not yet saved; (b) write the project from the live board.
`SettingsManager.SaveProjectCopy(path, board.GetProject())` does (b): it
carries the unsaved clearance and leaves the project on disk untouched. So
the plugin uses it, and falls back to (a) with a WARNING line when the call
is missing or fails. The `.kicad_dru` rules are not in memory (KiCad parses
the file), so they are copied from disk.

**Options that do nothing here.** `edge_and_pad_keepouts` (on by default) is
ignored: the reader writes the board-edge and pad-clearance keepouts itself
(K13.7, `options.input_ignored`), and `notes` says so once in the run log.

**Rule areas.** The reader maps rule areas by their flags (K06.2), so the
rule-area fix-up of K01.2, which rewrites DSN text, is not applied; `notes`
says so once in the run log.
"""

import shutil
from pathlib import Path

try:  # inside the plugin package
    from . import options
except ImportError:  # tests and headless scripts put this folder on sys.path
    import options

SNAPSHOT_SUFFIX = ".kicad_pcb"

# What the reader prints about the board, in the router's log, that the run
# log keeps (`reader_lines`).
READER_PREFIXES = (
    "KiCad project file",
    "No .kicad_pro",
    "KiCad design rules file",
    "KiCad reader:",
    "design rule '",
    "rule areas:",
    "planes:",
    "wires:",
    "arcs followed:",
    "footprint copper zones:",
    "custom pads",
)


def wanted(opts):
    """True when `opts` asks for the board snapshot."""
    return opts.board_input == options.INPUT_KICAD_PCB


def design_path(work_dir, stem, board_input):
    """The file the router is given: `<work>/<stem>.dsn` or, for the
    snapshot, `<work>/<stem>.kicad_pcb`."""
    suffix = SNAPSHOT_SUFFIX if board_input == options.INPUT_KICAD_PCB else ".dsn"
    return Path(work_dir) / (stem + suffix)


def snapshot_files(work_dir, stem):
    """The files of a snapshot in the work folder: the board, the project and
    the rules beside it (named as the reader looks for them), and the
    `.kicad_prl` KiCad writes with a project copy and nothing needs."""
    base = Path(work_dir) / stem
    return {
        "pcb": base.with_suffix(".kicad_pcb"),
        "pro": base.with_suffix(".kicad_pro"),
        "dru": base.with_suffix(".kicad_dru"),
        "prl": base.with_suffix(".kicad_prl"),
    }


def disk_sources(board_dir, stem):
    """`{"pro": path, "dru": path}` of the project files that exist next to
    the board on disk: what is copied when the live project copy is not
    available (`.kicad_pro`) and always for the rules (`.kicad_dru`)."""
    out = {}
    for kind, suffix in (("pro", ".kicad_pro"), ("dru", ".kicad_dru")):
        path = Path(board_dir) / (stem + suffix)
        if path.is_file():
            out[kind] = path
    return out


def router_args(opts):
    """The router flags the snapshot adds: `--apply-design-rules`, for every
    snapshot route (K13.1: the board's own rules reach the router on both
    inputs, as `--kicad-dru` gives them to a DSN route; the router applies
    them by default anyway). `export_flow.kicad_dru_args` passes it."""
    return ["--apply-design-rules"] if wanted(opts) else []


def notes(opts):
    """Lines for the run log that explain what a snapshot run does not do,
    each once."""
    out = []
    if opts.rule_area_fixup:
        out.append(
            "rule-area fix-up not applied: the router reads the board's rule areas by their flags (K06.2)"
        )
    ignored = options.input_ignored_note(opts)
    if ignored:
        out.append(ignored)
    out.append("the .kicad_dru rules are applied (--apply-design-rules; a DSN route gets --kicad-dru; Route in GUI reads them only)")
    return out


def reader_lines(lines):
    """The reader's report and warnings among the router's log lines, as
    `[info] ...`/`[warn] ...` text or bare, in order, for `plugin.log`."""
    out = []
    for line in lines:
        text = line.split("] ", 1)[1] if line.startswith("[") and "] " in line else line
        if text.startswith(READER_PREFIXES):
            out.append(line)
    return out


def _save_board(pcbnew, board, path):
    # Positional: the SWIG overload has no keyword. The 2-argument form also
    # saves the project file, which is the call that aborted KiCad (see above).
    return bool(pcbnew.SaveBoard(str(path), board, True))


def _live_project(pcbnew, board, pro):
    """Writes the project of the live board to `pro`. Returns True when the
    file exists afterwards."""
    pcbnew.GetSettingsManager().SaveProjectCopy(str(pro), board.GetProject())
    return pro.is_file() and pro.stat().st_size > 0


def take(board, pcbnew, work_dir, stem, board_dir):
    """Takes the snapshot of `board` into `work_dir`. Returns `(ok, lines)`;
    `lines` are for the plugin log (a WARNING line names what was not
    carried). Call on KiCad's UI thread."""
    files = snapshot_files(work_dir, stem)
    Path(work_dir).mkdir(parents=True, exist_ok=True)
    for path in files.values():  # nothing stale from an earlier run
        path.unlink(missing_ok=True)
    try:
        saved = _save_board(pcbnew, board, files["pcb"])
    except TypeError as e:
        return False, [f"WARNING: this KiCad's SaveBoard has no aSkipSettings argument ({e}); use the DSN input"]
    except Exception as e:
        return False, [f"WARNING: SaveBoard failed: {e!r}"]
    if not saved or not files["pcb"].is_file() or files["pcb"].stat().st_size == 0:
        return False, ["WARNING: SaveBoard wrote no board file"]
    lines = [f"snapshot: {files['pcb'].name}, {files['pcb'].stat().st_size} bytes, SaveBoard(aSkipSettings=True)"]
    disk = disk_sources(board_dir, stem)
    live = False
    try:
        live = _live_project(pcbnew, board, files["pro"])
    except Exception as e:
        lines.append(f"WARNING: the live project copy failed ({e!r})")
    files["prl"].unlink(missing_ok=True)
    if live:
        lines.append("project: written from the live board (SaveProjectCopy), unsaved Board Setup changes included")
    elif "pro" in disk:
        shutil.copyfile(disk["pro"], files["pro"])
        lines.append(
            f"WARNING: project: {disk['pro'].name} copied from disk; a net class changed in Board Setup and not saved is not in the snapshot"
        )
    else:
        lines.append("project: no .kicad_pro, the reader uses KiCad's default net class")
    if "dru" in disk:
        shutil.copyfile(disk["dru"], files["dru"])
        lines.append(f"design rules: {disk['dru'].name} copied from disk (read and applied by the router)")
    return True, lines

"""The direct read of the open board (K10): a snapshot of the board the
router reads with its own `.kicad_pcb` reader, in place of KiCad's DSN export.

`board_input = kicad_pcb` (options.py) makes the plugin call
`pcbnew.SaveBoard(path, board, aSkipSettings=True)` on KiCad's UI thread and
pass the copy to the router. The result still comes back as a session through
`ImportSpecctraSES` (plugin.py `import_ses`, which detaches the unlocked
tracks first). The snapshot is the default input of the `enhanced` profile
(K13.7); `legacy` keeps the DSN, which is what the Java comparison and the
graded runs of K02 and K09 used, and the DSN is the fallback when the reader
refuses the board (`input_choice.py`).

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

**The zone order** (K15.49). KiCad 10.0.7 writes a teardrop zone without a
`(uuid ...)`, so every `LoadBoard` gives it a new random KIID, and `SaveBoard`
writes the zones sorted by `BOARD_ITEM::ptr_cmp`: type, layer set, then uuid.
The teardrops thus came at another place among the zones of their layer set on
every run, and the router, which reads zones in file order, routed the same
board differently (K15.43, K15.47: `wisweep-driver-board`,
`complex_hierarchy`). `take` rewrites the saved file with the zones in a fixed
order (`order_zones`): within each layer set, the zones with a uuid as KiCad
sorted them, then those without one by priority and text. A board without such
zones is left byte for byte as KiCad saved it.
"""

import re
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
    "input fix-up:",  # K14: the router's lines for `router.input_fixups.*`
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
    `.kicad_prl` KiCad writes with a project copy and nothing needs. The
    suffix is appended, not swapped: a stem such as `v3.0` keeps its dot
    (K15.20)."""
    work = Path(work_dir)
    return {
        "pcb": work / (stem + ".kicad_pcb"),
        "pro": work / (stem + ".kicad_pro"),
        "dru": work / (stem + ".kicad_dru"),
        "prl": work / (stem + ".kicad_prl"),
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


# A quoted string (with backslash escapes), a lone quote (an unterminated
# string: the scan gives up) or a parenthesis.
_TOKEN = re.compile(r'"(?:[^"\\]|\\.)*"|"|[()]', re.S)
_HEAD = re.compile(r"\(\s*([^\s()\"]+)")


def _children(text, start=0, end=None):
    """The `(start, end)` spans of the direct children of the list that opens
    at `text[start]`, or None when the text is not one balanced list."""
    end = len(text) if end is None else end
    depth, spans, child = 0, [], 0
    for m in _TOKEN.finditer(text, start, end):
        token = m.group()
        if token == '"':
            return None
        if token == "(":
            depth += 1
            if depth == 2:
                child = m.start()
        elif token == ")":
            if depth == 2:
                spans.append((child, m.end()))
            depth -= 1
            if depth == 0:
                return spans if text[m.end() : end].strip() == "" else None
            if depth < 0:
                return None
    return None


def _head(text, span):
    m = _HEAD.match(text, span[0])
    return m.group(1) if m else ""


def _zone_key(text, span):
    """`(layers, uuid?, priority, text)` of the zone at `span`."""
    layers, has_uuid, priority = "", False, 0
    for child in _children(text, *span) or ():
        head = _head(text, child)
        if head in ("layer", "layers"):
            layers = text[child[0] : child[1]]
        elif head in ("uuid", "tstamp"):  # `tstamp` before KiCad 8
            has_uuid = True
        elif head == "priority":
            value = text[child[0] : child[1]].strip("()").split()[-1]
            priority = int(value) if value.isdigit() else 0
    return layers, has_uuid, priority, text[span[0] : span[1]]


def order_zones(text):
    """The saved board `text` with its zones in an order that does not depend
    on the random uuids KiCad gives the zones it saves without one (K15.49).
    KiCad's saver sorts the zones by layer set, then uuid, so the layer sets
    come in a fixed order and, within one, the zones with a uuid too; the
    zones without one go after those of their layer set, by priority, then
    text. Returns `(text, zones without a uuid)`; a text the scan cannot read
    comes back unchanged with 0."""
    spans = _children(text)
    if not spans:
        return text, 0
    zones = [span for span in spans if _head(text, span) == "zone"]
    keys = [_zone_key(text, span) for span in zones]
    loose = sum(1 for key in keys if not key[1])
    if not loose:
        return text, 0
    rank = {}
    for layers, *_ in keys:
        rank.setdefault(layers, len(rank))

    def order(key):
        layers, has_uuid, priority, block = key
        return (rank[layers], 0) if has_uuid else (rank[layers], 1, priority, block)

    blocks = [key[3] for key in sorted(keys, key=order)]
    out, at = [], 0
    for span, block in zip(zones, blocks):
        out += [text[at : span[0]], block]
        at = span[1]
    out.append(text[at:])
    return "".join(out), loose


def _fix_zone_order(path):
    """`order_zones` on the saved file; a line for the log when it had zones
    without a uuid."""
    with open(path, encoding="utf-8", newline="") as f:
        text = f.read()
    ordered, loose = order_zones(text)
    if ordered != text:
        with open(path, "w", encoding="utf-8", newline="") as f:
            f.write(ordered)
    return [f"snapshot: {loose} zone(s) without a uuid (teardrops) put in a fixed order"] if loose else []


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
    try:
        lines += _fix_zone_order(files["pcb"])
    except (OSError, UnicodeError) as e:
        lines.append(f"WARNING: the snapshot's zones keep KiCad's order, which changes from run to run ({e!r})")
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

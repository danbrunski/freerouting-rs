"""A plugin run without the editor (K09): the plugin's own export, fix-ups,
router arguments and SES import, driven from a plain Python that imports
pcbnew (WSL: the system python3 of KiCad 10.0.7).

    python3 -I kicad-plugin/headless_run.py export BOARD WORK OPTIONS.json
    python3 -I kicad-plugin/headless_run.py import BOARD SES OUT OPTIONS.json

`export` loads BOARD (a copy: nothing is saved), writes WORK/<stem>.dsn the
way the plugin's Route button does (`export_flow.prepare`), and
WORK/<stem>.args.json: the router flags `RouteOptions.to_args` gives, with
the board's minimum track width, and the fix-up log. OPTIONS.json is a
`route-options.json` (what the dialog saves).

`import` loads BOARD again in a new process (a second LoadBoard in one
process returned an unwrapped SwigPyObject in 10.0.7), locks the tracks as
the plugin does for `fix_existing`, calls `ImportSpecctraSES` and saves the
result as OUT, with the original lock states restored.

It never imports the plugin package: `register()` outside the editor raises a
wxWidgets dialog on the desktop. Only the pure modules are put on sys.path.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "freerouting_rs"))

import pcbnew  # noqa: E402

import export_flow  # noqa: E402
import kicad_board  # noqa: E402
import options  # noqa: E402


def _options(path):
    with open(path, encoding="utf-8") as f:
        return options.RouteOptions.from_dict(json.load(f))


def _min_track_um(board):
    nm = board.GetDesignSettings().m_TrackMinWidth
    return nm / 1000.0 if nm and nm > 0 else None


def export(board_path, work, options_path):
    opts = _options(options_path)
    board = pcbnew.LoadBoard(board_path)
    work = Path(work)
    work.mkdir(parents=True, exist_ok=True)
    stem = Path(board_path).stem
    dsn = work / (stem + ".dsn")
    ok, extra, lines, _locked = export_flow.prepare(
        board, pcbnew, dsn, opts, lambda b, p: bool(pcbnew.ExportSpecctraDSN(b, str(p))) and p.is_file()
    )
    if not ok:
        print("DSN export failed", file=sys.stderr)
        return 1
    args = opts.to_args(_min_track_um(board), extra_args=extra)
    with open(work / (stem + ".args.json"), "w", encoding="utf-8") as f:
        json.dump({"args": args, "log": lines, "kicad": pcbnew.GetBuildVersion()}, f, indent=2)
    print("EXPORTED " + " ".join(args))
    return 0


def import_ses(board_path, ses, out, options_path):
    opts = _options(options_path)
    board = pcbnew.LoadBoard(board_path)
    locked = kicad_board.lock_tracks(board) if opts.fix_existing else []
    if not pcbnew.ImportSpecctraSES(board, ses):
        print("SES import failed", file=sys.stderr)
        return 1
    lines = export_flow.after_import(board, ses)
    kicad_board.restore_locks(locked)
    pcbnew.SaveBoard(out, board)
    print("IMPORTED " + "; ".join(lines))
    return 0


if __name__ == "__main__":
    if len(sys.argv) == 5 and sys.argv[1] == "export":
        sys.exit(export(*sys.argv[2:]))
    if len(sys.argv) == 6 and sys.argv[1] == "import":
        sys.exit(import_ses(*sys.argv[2:]))
    print(__doc__, file=sys.stderr)
    sys.exit(2)

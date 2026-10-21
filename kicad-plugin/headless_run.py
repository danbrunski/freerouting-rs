"""A plugin run without the editor (K09): the plugin's own export, fix-ups,
router arguments and SES import, driven from a plain Python that imports
pcbnew (WSL: the system python3 of KiCad 10.0.7).

    python3 -I kicad-plugin/headless_run.py export BOARD WORK OPTIONS.json
    python3 -I kicad-plugin/headless_run.py import BOARD SES OUT OPTIONS.json
    python3 -I kicad-plugin/headless_run.py snapshot BOARD WORK OPTIONS.json [EDIT ...]
    python3 -I kicad-plugin/headless_run.py route BOARD WORK OPTIONS.json ROUTER

`export` loads BOARD (a copy: nothing is saved), writes WORK/<stem>.dsn the
way the plugin's Route button does (`export_flow.prepare`), and
WORK/<stem>.args.json: the router flags `RouteOptions.to_args` gives, with
the board's minimum track width, and the fix-up log. OPTIONS.json is a
`route-options.json` (what the dialog saves).

`import` loads BOARD again in a new process (a second LoadBoard in one
process returned an unwrapped SwigPyObject in 10.0.7), locks the tracks as
the plugin does for `fix_existing`, calls `ImportSpecctraSES`, puts the input
copper back at its own coordinates and its track arcs back as arcs
(`restore_copper`, Q22.3, K13.8) and saves the result as OUT, with the
original lock states restored.

`snapshot` (K10) loads BOARD, applies the in-memory EDITs (nothing is saved),
and takes the board snapshot the plugin's `board_input = kicad_pcb` takes
(`export_flow.prepare_snapshot`: WORK/<stem>.kicad_pcb, .kicad_pro,
.kicad_dru and WORK/<stem>.args.json). EDIT is one of
`--track=X1,Y1,X2,Y2,WIDTH,LAYER,NET` (mm, a track added to the board),
`--class-clearance=CLASS=MM` (a net class changed in Board Setup, unsaved) or
`--no-live-project` (the live project copy fails, as on a KiCad without it).

`route` prepares the board and runs ROUTER with the same RouteRun and arguments
as the plugin. It writes WORK/<stem>.ses and <stem>.result.json; use `import`
to apply the session to a copy. BOARD is never saved. Both input modes refill
zones before preparing the input. The args JSON records its SHA256.

It never imports the plugin package: `register()` outside the editor raises a
wxWidgets dialog on the desktop. Only the pure modules are put on sys.path.
"""

import hashlib
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "freerouting_rs"))

import pcbnew  # noqa: E402

import export_flow  # noqa: E402
import kicad_board  # noqa: E402
import options  # noqa: E402
import restore_copper  # noqa: E402
import routing  # noqa: E402
import tools  # noqa: E402


def _options(path):
    with open(path, encoding="utf-8") as f:
        return options.RouteOptions.from_dict(json.load(f))


def export(board_path, work, options_path):
    opts = _options(options_path)
    board = pcbnew.LoadBoard(board_path)
    work = Path(work)
    work.mkdir(parents=True, exist_ok=True)
    stem = Path(board_path).stem
    dsn = work / (stem + ".dsn")
    ok, extra, lines, _locked = export_flow.prepare(
        board,
        pcbnew,
        dsn,
        opts,
        lambda b, p: bool(pcbnew.ExportSpecctraDSN(b, str(p))) and p.is_file(),
    )
    if not ok:
        print("DSN export failed: " + "; ".join(lines), file=sys.stderr)
        return 1
    export_flow.save_arguments(board, pcbnew, dsn, opts, extra, lines)
    args = export_flow.router_args(board, opts, extra)
    print("EXPORTED " + " ".join(args))
    return 0


def import_ses(board_path, ses, out, options_path):
    opts = _options(options_path)
    board = pcbnew.LoadBoard(board_path)
    locked = kicad_board.lock_tracks(board) if opts.fix_existing else []
    originals = restore_copper.originals(board.GetTracks())  # the unlocked ones the import replaces
    if not pcbnew.ImportSpecctraSES(board, ses):
        print("SES import failed", file=sys.stderr)
        return 1
    lines = export_flow.after_import(board, ses, pcbnew, originals, opts.board_input)
    kicad_board.restore_locks(locked)
    pcbnew.SaveBoard(out, board)
    print("IMPORTED " + "; ".join(lines))
    return 0


def snapshot_mode(board_path, work, options_path, edits):
    opts = _options(options_path)
    board = pcbnew.LoadBoard(board_path)
    live = True
    for edit in edits:
        name, _, value = edit.partition("=")
        if name == "--track":
            x1, y1, x2, y2, width, layer, net = value.split(",")
            track = pcbnew.PCB_TRACK(board)
            track.SetStart(pcbnew.VECTOR2I(pcbnew.FromMM(float(x1)), pcbnew.FromMM(float(y1))))
            track.SetEnd(pcbnew.VECTOR2I(pcbnew.FromMM(float(x2)), pcbnew.FromMM(float(y2))))
            track.SetWidth(pcbnew.FromMM(float(width)))
            track.SetLayer(board.GetLayerID(layer))
            track.SetNet(board.FindNet(net))
            board.Add(track)
        elif name == "--class-clearance":
            cls, _, mm = value.partition("=")
            board.GetDesignSettings().m_NetSettings.GetNetclasses()[cls].SetClearance(pcbnew.FromMM(float(mm)))
        elif name == "--no-live-project":
            live = False
        else:
            print(f"unknown edit {edit}", file=sys.stderr)
            return 2
    if not live:

        class _NoManager:
            def SaveProjectCopy(self, path, project):
                raise RuntimeError("no live project copy")

        class _Pcbnew:  # pcbnew with the live project copy removed
            def __getattr__(self, name):
                return _NoManager if name == "GetSettingsManager" else getattr(pcbnew, name)

        module = _Pcbnew()
    else:
        module = pcbnew
    work = Path(work)
    stem = Path(board_path).stem
    ok, extra, lines, _locked = export_flow.prepare_snapshot(board, module, work, stem, Path(board_path).parent, opts)
    if not ok:
        print("snapshot failed: " + "; ".join(lines), file=sys.stderr)
        return 1
    export_flow.save_arguments(board, pcbnew, work / (stem + ".kicad_pcb"), opts, extra, lines)
    args = export_flow.router_args(board, opts, extra)
    print("SNAPSHOT " + " ".join(args))
    return 0


def route(board_path, work, options_path, router):
    if Path(work).resolve() == Path(board_path).resolve().parent:
        print(
            "Use a separate work directory so preparation cannot replace the source project",
            file=sys.stderr,
        )
        return 2
    opts = _options(options_path)
    conflicts = options.input_conflicts(opts)
    if conflicts:
        print("Incompatible board input options: " + ", ".join(conflicts), file=sys.stderr)
        return 2
    mode = "snapshot" if opts.board_input == options.INPUT_KICAD_PCB else "export"
    # Keep pcbnew preparation in its own process (KiCad 10 SWIG lifetimes).
    prepared = subprocess.run([sys.executable, "-I", __file__, mode, board_path, work, options_path])
    if prepared.returncode:
        return prepared.returncode
    work = Path(work)
    stem = Path(board_path).stem
    design = work / (stem + (".kicad_pcb" if mode == "snapshot" else ".dsn"))
    recipe = json.loads((work / (stem + ".args.json")).read_text(encoding="utf-8"))
    if hashlib.sha256(design.read_bytes()).hexdigest() != recipe["input_sha256"]:
        print("Prepared input changed before routing", file=sys.stderr)
        return 1
    ses = work / (stem + ".ses")
    result = routing.manifest_path(work, stem)
    ses.unlink(missing_ok=True)
    result.unlink(missing_ok=True)
    run = routing.RouteRun(
        tools.Tool(tools.ROUTER, router),
        design,
        ses,
        recipe["args"],
        result,
        opts.profile,
    )
    try:
        run.start()
        rc = run.wait()
    except OSError as e:
        print(f"Could not start the router: {e}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        if run.proc is not None:
            run.abort()
        return 130
    print("\n".join(run.lines))
    return rc if rc else (0 if run.ses_written() else 1)


if __name__ == "__main__":
    if len(sys.argv) == 6 and sys.argv[1] == "route":
        sys.exit(route(*sys.argv[2:]))
    if len(sys.argv) == 5 and sys.argv[1] == "export":
        sys.exit(export(*sys.argv[2:]))
    if len(sys.argv) == 6 and sys.argv[1] == "import":
        sys.exit(import_ses(*sys.argv[2:]))
    if len(sys.argv) >= 5 and sys.argv[1] == "snapshot":
        sys.exit(snapshot_mode(*sys.argv[2:5], sys.argv[5:]))
    print(__doc__, file=sys.stderr)
    sys.exit(2)

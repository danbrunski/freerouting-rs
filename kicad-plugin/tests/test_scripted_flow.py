"""The supported board CLI and editor preparation use the same copper/args.

Real pcbnew runs only in isolated processes; the plugin package is never
imported. Routing is limited to the small Issue558 fixture.
"""

import hashlib
import json
import re
import shutil
import subprocess

import pytest

import routing
import tools as router_tools
from conftest import PLUGIN_SRC
from test_snapshot_headless import BOARD_DIR, HEADLESS, pcbnew_python
from test_cli_integration import find_router


def run(*args):
    result = subprocess.run(list(map(str, args)), capture_output=True, text=True, timeout=120)
    assert result.returncode == 0, result.stdout + result.stderr
    return result


@pytest.fixture
def board(tmp_path):
    python = pcbnew_python()
    if python is None:
        pytest.skip("no Python with pcbnew")
    if not (BOARD_DIR / "dev-board.kicad_pcb").is_file():
        pytest.skip("Issue558 fixture missing")
    project = tmp_path / "project"
    project.mkdir()
    for name in ("dev-board.kicad_pcb", "dev-board.kicad_pro"):
        shutil.copyfile(BOARD_DIR / name, project / name)
    pcb = project / "dev-board.kicad_pcb"
    opts = tmp_path / "options.json"
    opts.write_text(
        json.dumps({"schema": 6, "board_input": "dsn", "max_passes": 1, "optimizer": False, "threads": 2}),
        encoding="utf-8",
    )
    return python, pcb, opts, tmp_path


def design_tokens(text):
    # KiCad puts the destination pathname in (pcb ...); no copper normalization.
    import rule_areas

    root = rule_areas.parse(text)[0]

    def values(node):
        return tuple(values(child) for child in node.children) if node.is_list else node.text

    return (root.head, "DESIGN", *(values(child) for child in root.children[2:]))


def recipe(work):
    design = work / "dev-board.dsn"
    data = json.loads((work / "dev-board.args.json").read_text(encoding="utf-8"))
    assert data["input_sha256"] == hashlib.sha256(design.read_bytes()).hexdigest()
    return data


def test_cached_fill_does_not_change_export_or_arguments(board):
    python, pcb, opts, folder = board
    # This demo has only a footprint keepout. Add a real copper zone so the
    # fill comparison cannot pass without exercising ZONE_FILLER.
    add_zone = """
import pcbnew, sys
b = pcbnew.LoadBoard(sys.argv[1])
z = pcbnew.ZONE(b)
z.SetLayer(pcbnew.F_Cu)
z.SetNet(b.GetNetsByName()['GND'])
z.Outline().NewOutline()
for x, y in [(113, 42), (141, 42), (141, 93), (113, 93)]:
    z.Outline().Append(pcbnew.FromMM(x), pcbnew.FromMM(y))
b.Add(z)
filler = pcbnew.ZONE_FILLER(b)
assert filler.Fill(b.Zones())
assert z.IsFilled()
assert pcbnew.SaveBoard(sys.argv[1], b, True)
"""
    run(python, "-I", "-c", add_zone, pcb)
    before = {p: p.read_bytes() for p in pcb.parent.iterdir()}
    scripted = folder / "scripted"
    run(python, "-I", HEADLESS, "export", pcb, scripted, opts)
    editor = folder / "editor"
    editor.mkdir()
    # Same call as Dialog.export, with deliberately absent cached fill. The
    # source files are never saved during either export.
    script = """
import json, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
import pcbnew, export_flow, options
board = pcbnew.LoadBoard(sys.argv[2])
for zone in board.Zones():
    zone.UnFill()
opts = options.RouteOptions.from_dict(json.loads(Path(sys.argv[4]).read_text()))
dsn = Path(sys.argv[3]) / 'dev-board.dsn'
ok, extra, lines, _locked = export_flow.prepare(board, pcbnew, dsn, opts,
    lambda b, p: pcbnew.ExportSpecctraDSN(b, str(p)))
assert ok, lines
assert 'zones refilled before routing export' in lines
assert any(z.IsFilled() for z in board.Zones() if not z.GetIsRuleArea())
export_flow.save_arguments(board, pcbnew, dsn, opts, extra, lines)
"""
    run(python, "-I", "-c", script, PLUGIN_SRC, pcb, editor, opts)
    assert design_tokens((scripted / "dev-board.dsn").read_text()) == design_tokens((editor / "dev-board.dsn").read_text())
    assert recipe(scripted)["args"] == recipe(editor)["args"]
    assert {p: p.read_bytes() for p in pcb.parent.iterdir()} == before


def test_scripted_route_matches_direct_plugin_invocation(board):
    python, pcb, opts, folder = board
    router = find_router()
    if router is None:
        pytest.skip("no freerouting-rs executable")
    work = folder / "route"
    original = pcb.read_bytes()
    run(python, "-I", HEADLESS, "route", pcb, work, opts, router)
    assert pcb.read_bytes() == original
    assert (work / "dev-board.result.json").is_file()
    # Repeat only the small routing job through the editor's actual class,
    # taking the saved input and exact arguments rather than a separate preset.
    direct = folder / "direct" / "dev-board.ses"
    direct.parent.mkdir()
    job = routing.RouteRun(
        router_tools.Tool("freerouting-rs", router),
        work / "dev-board.dsn",
        direct,
        recipe(work)["args"],
    )
    job.start()
    assert job.wait(timeout=120) == 0
    assert job.ses_written()

    # Only the top-level output session name is allowed to differ.
    def session(path):
        return re.sub(
            r'^\(session\s+("[^"]*"|[^\s()]+)',
            '(session "SESSION"',
            path.read_text(encoding="utf-8"),
        )

    assert session(work / "dev-board.ses") == session(direct)


def test_scripted_route_refuses_to_prepare_over_the_source_project(board):
    python, pcb, opts, _folder = board
    original = {p: p.read_bytes() for p in pcb.parent.iterdir()}
    result = subprocess.run(
        [
            python,
            "-I",
            str(HEADLESS),
            "route",
            str(pcb),
            str(pcb.parent),
            str(opts),
            "unused-router",
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 2 and "separate work directory" in result.stderr
    assert {p: p.read_bytes() for p in pcb.parent.iterdir()} == original

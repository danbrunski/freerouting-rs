"""K10.1 and K10.2 against a real pcbnew (WSL: KiCad 10.0.7's system python3)
and the real `freerouting` CLI, on a copy of the Issue558 dev board.

Skipped, with the reason, when no python3 imports pcbnew, no router that reads
a `.kicad_pcb` is found (`FREEROUTING_RS_BIN`, else `target/release` or
`target/debug`), or the board is missing. The scripts never import the plugin
package: `register()` outside the editor raises a wxWidgets dialog.
"""

import json
import re
import shutil
import subprocess
import sys

import pytest
from conftest import REPO, TESTS
from test_cli_integration import find_router

BOARD_DIR = REPO / "fixtures" / "kicad" / "Issue558-dev-board-autoroute-demo"
HEADLESS = REPO / "kicad-plugin" / "headless_run.py"
OPTIONS = {"schema": 4, "board_input": "kicad_pcb", "edge_and_pad_keepouts": False}
NOISE = ("property.h", "memory leak", "Font '")


def pcbnew_python():
    for candidate in (shutil.which("python3"), "/usr/bin/python3", sys.executable):
        if not candidate:
            continue
        try:
            ok = subprocess.run([candidate, "-I", "-c", "import pcbnew"], capture_output=True, timeout=120).returncode == 0
        except (OSError, subprocess.TimeoutExpired):
            continue
        if ok:
            return candidate
    return None


@pytest.fixture(scope="module")
def tools():
    python = pcbnew_python()
    if python is None:
        pytest.skip("no python3 that imports pcbnew")
    router = find_router()
    if router is None:
        pytest.skip("no freerouting executable (set FREEROUTING_RS_BIN)")
    probe = subprocess.run([router, "--capabilities"], capture_output=True, text=True, timeout=60)
    if '"flags"' not in probe.stdout or "--apply-design-rules" not in probe.stdout:
        pytest.skip(f"{router} predates K10.3")
    if not (BOARD_DIR / "dev-board.kicad_pcb").is_file():
        pytest.skip("fixtures/kicad/Issue558-dev-board-autoroute-demo is missing")
    return python, router


@pytest.fixture
def board(tmp_path):
    folder = tmp_path / "proj"
    folder.mkdir()
    for name in ("dev-board.kicad_pcb", "dev-board.kicad_pro"):
        shutil.copy(BOARD_DIR / name, folder / name)
    opts = tmp_path / "opts.json"
    opts.write_text(json.dumps(OPTIONS), encoding="utf-8")
    return folder / "dev-board.kicad_pcb", opts, tmp_path / "work"


def headless(python, *args):
    out = subprocess.run([python, "-I", str(HEADLESS), *map(str, args)], capture_output=True, text=True, timeout=300)
    err = "\n".join(l for l in out.stderr.splitlines() if not any(n in l for n in NOISE))
    assert out.returncode == 0, err
    return out.stdout


def reader_wires(router, design):
    """The reader's own count, from its report: `wires: N, vias: M`."""
    out = subprocess.run([router, "-de", str(design), "-drc", "-"], capture_output=True, text=True, timeout=300)
    match = re.search(r"wires: (\d+), vias: (\d+)", out.stdout + out.stderr)
    assert match, out.stdout + out.stderr
    return int(match.group(1))


def project_clearances(path):
    classes = json.loads(path.read_text(encoding="utf-8"))["net_settings"]["classes"]
    return {c["name"]: c["clearance"] for c in classes}


def test_a_track_added_in_memory_is_on_the_readers_board(tools, board):
    python, router = tools
    pcb, opts, work = board
    before = reader_wires(router, pcb)
    headless(python, "snapshot", pcb, work, opts, "--track=100,100,110,100,0.25,F.Cu,GND")
    snapshot = work / "dev-board.kicad_pcb"
    assert snapshot.is_file() and (work / "dev-board.kicad_pro").is_file()
    assert not (work / "dev-board.kicad_prl").exists()
    assert reader_wires(router, snapshot) == before + 1
    assert reader_wires(router, pcb) == before  # the board on disk was not touched


def test_an_unsaved_net_class_change_reaches_the_snapshot_only_through_the_live_project(tools, board):
    python, _router = tools
    pcb, opts, work = board
    on_disk = project_clearances(pcb.with_suffix(".kicad_pro"))
    assert on_disk["Power"] == 0.2
    # (b) the live project copy: the unsaved change is in the snapshot's project
    headless(python, "snapshot", pcb, work, opts, "--class-clearance=Power=0.333")
    live = project_clearances(work / "dev-board.kicad_pro")
    assert live["Power"] == 0.333 and live["Default"] == on_disk["Default"]
    assert project_clearances(pcb.with_suffix(".kicad_pro"))["Power"] == 0.2  # the saved file is unchanged
    # (a) copying the file on disk misses it, and the log says what is missed
    out = headless(python, "snapshot", pcb, work, opts, "--class-clearance=Power=0.333", "--no-live-project")
    assert project_clearances(work / "dev-board.kicad_pro")["Power"] == 0.2
    log = json.loads((work / "dev-board.args.json").read_text(encoding="utf-8"))["log"]
    assert any(l.startswith("WARNING: project") and "not saved is not in the snapshot" in l for l in log), out


def test_a_snapshot_of_an_unchanged_board_checks_like_the_board(tools, board):
    python, router = tools
    pcb, opts, work = board
    headless(python, "snapshot", pcb, work, opts)
    reports = []
    for design in (pcb, work / "dev-board.kicad_pcb"):
        report = work.parent / (design.parent.name + ".json")
        subprocess.run([router, "-de", str(design), "-drc", str(report)], capture_output=True, timeout=300, check=True)
        reports.append(json.loads(report.read_text(encoding="utf-8")))
    # KiCad's save renumbers the items and may reorder them, so compare what
    # the report says, not the ids: the violations as a sorted multiset.
    def content(report):
        out = {}
        for key, value in report.items():
            if isinstance(value, list):
                out[key] = sorted(f"{v.get('type')}|{v.get('severity')}|{v.get('description')}" for v in value)
        return out

    first, second = content(reports[0]), content(reports[1])
    assert first == second and sum(len(v) for v in first.values()) > 0


def test_the_router_session_of_a_snapshot_imports_with_no_error(tools, board):
    python, router = tools
    pcb, opts, work = board
    headless(python, "snapshot", pcb, work, opts)
    ses = work / "dev-board.ses"
    run = subprocess.run(
        [router, "-de", str(work / "dev-board.kicad_pcb"), "-do", str(ses), "--max-passes", "1", "--router.optimizer.enabled=false"],
        capture_output=True,
        text=True,
        timeout=300,
    )
    assert run.returncode == 0 and ses.is_file(), run.stderr[-800:]
    assert ses.read_text(encoding="utf-8").startswith('(session "dev-board"')
    routed = work / "routed.kicad_pcb"
    out = headless(python, "import", pcb, ses, routed, opts)
    assert out.startswith("IMPORTED")  # headless_run exits 1 with "SES import failed" otherwise
    text = routed.read_text(encoding="utf-8")
    assert text.count("(segment") > 0 and "(via" in text

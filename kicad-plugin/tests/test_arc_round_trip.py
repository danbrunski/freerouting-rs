"""K13.8 against a real pcbnew (WSL: KiCad 10.0.7's system python3) and
kicad-cli: two track arcs at exactly the class clearance go through the
plugin's export and fix-ups (`headless_run.py export`), a session that gives
the input copper back unmoved, and the plugin's import (`headless_run.py
import`), and come back as arcs at their own coordinates, with no clearance
item in KiCad's DRC.

The fixture, `data/arc_pair.kicad_pcb`: nets A and B on F.Cu, 0.2 mm wide,
each a straight lead-in, a 90-degree arc and a straight lead-out; A's arc has
radius 1.0 mm and B's 1.4 mm around (100, 100), so the copper gap is
1.4 - 1.0 - 0.2 = 0.2 mm, the Default class clearance. Every coordinate is
exact in nanometres (the mid points are 3-4-5 triangles). KiCad's export lays
each arc as its chord; B's chord passes 0.21 mm inside its arc, through A.

The session is written from the exported DSN's wiring, as the router writes
copper it kept (`(type protect)`, 0.1 µm units), so the test needs no router.
Skipped, with the reason, without a python3 that imports pcbnew or without
kicad-cli. Never imports the plugin package: `register()` outside the editor
raises a wxWidgets dialog.
"""

import json
import shutil
import subprocess

import pytest
from conftest import REPO, TESTS
from dsn_fixups import Dsn, _unquote
from test_snapshot_headless import NOISE, pcbnew_python

HEADLESS = REPO / "kicad-plugin" / "headless_run.py"
BOARD = TESTS / "data" / "arc_pair.kicad_pcb"
OPTIONS = {"schema": 5, "profile": "enhanced", "board_input": "dsn"}

# (net, start, mid, end) of the fixture's arcs, nanometres
ARCS = [
    ("A", (101000000, 100000000), (100800000, 100600000), (100000000, 101000000)),
    ("B", (101400000, 100000000), (101120000, 100840000), (100000000, 101400000)),
]

DUMP = """
import json, sys
import pcbnew
board = pcbnew.LoadBoard(sys.argv[1])
out = []
for t in board.GetTracks():
    xy = lambda p: [p.x, p.y]
    row = {"cls": t.GetClass(), "net": t.GetNetname(), "width": t.GetWidth(), "uuid": t.m_Uuid.AsString(),
           "start": xy(t.GetStart()), "end": xy(t.GetEnd())}
    if t.GetClass() == "PCB_ARC":
        row["mid"] = xy(t.GetMid())
    out.append(row)
json.dump(out, open(sys.argv[2], "w"))
"""


@pytest.fixture(scope="module")
def kicad():
    python = pcbnew_python()
    if python is None:
        pytest.skip("no python3 that imports pcbnew")
    cli = shutil.which("kicad-cli")
    if cli is None:
        pytest.skip("no kicad-cli")
    return python, cli


def run(cmd, **kw):
    out = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, timeout=300, **kw)
    noise = "\n".join(line for line in out.stderr.splitlines() if not any(n in line for n in NOISE))
    assert out.returncode == 0, out.stdout + noise
    return out.stdout


def session_from_wiring(dsn_text, name):
    """The DSN's wires as a session: every wire unmoved, in 0.1 µm units."""
    dsn = Dsn(dsn_text)
    nets = {}
    for wire in dsn.wiring.children:
        if not wire.is_list or wire.head != "wire":
            continue
        path = next(c for c in wire.children if c.is_list and c.head == "path")
        net = next(c for c in wire.children if c.is_list and c.head == "net")
        tokens = [c.text for c in path.children[1:]]
        values = " ".join(str(round(float(v) * 10)) for v in tokens[2:])
        nets.setdefault(_unquote(net.children[1].text), []).append(
            f"(wire (path {tokens[0]} {round(float(tokens[1]) * 10)} {values}) (type protect))"
        )
    body = " ".join(f"(net {net} {' '.join(wires)})" for net, wires in nets.items())
    return (
        f'(session "{name}" (base_design "{name}") (routes (resolution um 10) '
        f'(parser (host_cad "KiCad\'s Pcbnew") (host_version "10")) (library_out) '
        f"(network_out {body})))\n"
    )


def dsn_paths(dsn_text):
    """{net: [[(x, y) µm, ...] per wire]} of the DSN's wiring."""
    dsn = Dsn(dsn_text)
    out = {}
    for wire in dsn.wiring.children:
        if wire.is_list and wire.head == "wire":
            path = next(c for c in wire.children if c.is_list and c.head == "path")
            net = next(c for c in wire.children if c.is_list and c.head == "net")
            v = [float(c.text) for c in path.children[3:]]
            out.setdefault(_unquote(net.children[1].text), []).append(list(zip(v[::2], v[1::2])))
    return out


def test_two_arcs_at_the_class_clearance_come_back_as_arcs_with_no_clearance_item(kicad, tmp_path):
    python, cli = kicad
    proj = tmp_path / "proj"
    proj.mkdir()
    board = proj / BOARD.name
    shutil.copy(BOARD, board)
    opts = tmp_path / "route-options.json"
    opts.write_text(json.dumps(OPTIONS), encoding="utf-8")
    work = tmp_path / "work"
    run([python, "-I", HEADLESS, "export", board, work, opts])
    dsn_text = (work / "arc_pair.dsn").read_text(encoding="utf-8")
    assert "(clearance 200)" in dsn_text  # the Default class, the arcs' gap
    log = json.loads((work / "arc_pair.args.json").read_text(encoding="utf-8"))["log"]
    assert any(line.startswith("track arcs: 2 of 2 unlocked arc(s) written as polylines") for line in log)
    paths = dsn_paths(dsn_text)
    for net, start, _mid, end in ARCS:
        # the arc's wire: its two ends as KiCad wrote them, points on the arc between
        wire = next(p for p in paths[net] if (start[0] / 1000, -start[1] / 1000) in p and (end[0] / 1000, -end[1] / 1000) in p)
        assert len(wire) > 10

    ses = work / "arc_pair.ses"
    ses.write_text(session_from_wiring(dsn_text, "arc_pair"), encoding="utf-8")
    routed = tmp_path / "routed" / BOARD.name
    routed.parent.mkdir()
    out = run([python, "-I", HEADLESS, "import", board, ses, routed, opts])
    assert "2 track arc(s) back as arcs (K13.8)" in out and "0 input item(s) left as routed" in out

    dump = tmp_path / "tracks.json"
    run([python, "-I", "-c", DUMP, routed, dump])
    before = tmp_path / "before.json"
    run([python, "-I", "-c", DUMP, board, before])
    got = json.loads(dump.read_text(encoding="utf-8"))
    want = json.loads(before.read_text(encoding="utf-8"))
    arcs = [t for t in got if t["cls"] == "PCB_ARC"]
    assert sorted((a["net"], tuple(a["start"]), tuple(a["mid"]), tuple(a["end"])) for a in arcs) == ARCS
    key = lambda t: (t["cls"], t["net"], t["start"])  # noqa: E731
    assert sorted(got, key=key) == sorted(want, key=key)  # every track and arc as it was, uuids included

    report = tmp_path / "drc.json"
    run([cli, "pcb", "drc", "--format", "json", "-o", report, routed])
    violations = json.loads(report.read_text(encoding="utf-8"))["violations"]
    assert [v for v in violations if v["type"] == "clearance"] == []

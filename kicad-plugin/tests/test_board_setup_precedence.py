"""K15.44: an unconditional `.kicad_dru` minimum replaces Board Setup's, as in
KiCad (decided 2026-10-22).

A route passes Board Setup's minimums as `--board-setup.router.*`, Board
Setup's place in the router's merge (the DSN's priority, below the command
line), and no default `--router.min_trace_width_um`; the router keeps every
neck-down at or above `router.board_minimums.track_width_um` (Q18), which the
rule now sets. A command-line flag would be the user's, which a rule only
raises. The user's `min_trace_width_um` option is still passed and wins.

The last test runs the real router's `--dump-settings` with the plugin's
arguments on both inputs; it is skipped without a router that knows the flag
(`FREEROUTING_RS_BIN`, else `target/release` or `target/debug`).
"""

import json
import os
import shutil
import subprocess
import types
from dataclasses import replace

import pytest

import export_flow
import options
import tools
from conftest import REPO
from options import RouteOptions

TRACK = "--board-setup.router.board_minimums.track_width_um=200"
RULE = '(version 1)\n(rule "fab minimum" (constraint track_width (min 0.127mm)))\n'


def board_at(path, track_nm=200000):
    settings = types.SimpleNamespace(m_TrackMinWidth=track_nm)
    return types.SimpleNamespace(GetFileName=lambda: str(path), GetDesignSettings=lambda: settings)


def test_a_route_passes_board_setup_where_a_rule_can_replace_it(tmp_path):
    for board_input in options.BOARD_INPUTS:
        opts = replace(RouteOptions(), board_input=board_input)
        args = export_flow.router_args(board_at(tmp_path / "b.kicad_pcb"), opts)
        assert TRACK in args, args
        assert not any(a.startswith("--router.min_trace_width_um") for a in args), args
        assert not any(a.startswith("--router.board_minimums") for a in args), args


def test_the_users_minimum_width_is_still_passed(tmp_path):
    opts = replace(RouteOptions(), min_trace_width_um=180.0)
    args = export_flow.router_args(board_at(tmp_path / "b.kicad_pcb"), opts)
    assert "--router.min_trace_width_um=180" in args and TRACK in args


def test_route_in_gui_keeps_the_command_line_form(tmp_path):
    # the GUI takes no `--board-setup.*` flag and applies no rule on a DSN
    args = export_flow.router_args(board_at(tmp_path / "b.kicad_pcb"), RouteOptions(), for_gui=True)
    assert args == ["--router.min_trace_width_um=200", "--router.board_minimums.track_width_um=200"]


def find_router():
    candidates = []
    if os.environ.get("FREEROUTING_RS_BIN"):
        candidates.append(os.environ["FREEROUTING_RS_BIN"])
    candidates += [str(REPO / "target" / "release"), str(REPO / "target" / "debug")]
    for d in candidates:
        exe = os.path.join(d, "freerouting-rs" + tools.EXE_SUFFIX)
        if os.path.isfile(exe):
            return exe
    return None


@pytest.fixture(scope="module")
def router():
    exe = find_router()
    if exe is None:
        pytest.skip("no freerouting-rs executable (set FREEROUTING_RS_BIN or build target/release)")
    probe = subprocess.run([exe, "--capabilities"], capture_output=True, text=True, timeout=60)
    if "--board-setup.router." not in probe.stdout:
        pytest.skip(f"{exe} predates K15.44 (--board-setup.router.*)")
    return exe


def dump(exe, design, args, cwd):
    done = subprocess.run(
        [exe, "-de", str(design), *args, "--dump-settings=json"], capture_output=True, text=True, timeout=300, cwd=cwd
    )
    assert done.returncode == 0, done.stdout + done.stderr
    rows = json.loads(done.stdout)["settings"]
    return {row["path"]: (row.get("value"), row.get("source")) for row in rows}


def test_the_router_takes_the_rules_value_on_both_inputs(router, tmp_path):
    # the DSN path: KiCad's export, the board's `.kicad_dru` by `--kicad-dru`
    dsn_dir = tmp_path / "dsn"
    dsn_dir.mkdir()
    shutil.copy(REPO / "crates/freerouting/tests/data/kicad/q225-item-types.dsn", dsn_dir / "b.dsn")
    (dsn_dir / "b.kicad_dru").write_text(RULE, encoding="utf-8")
    opts = replace(RouteOptions(), board_input=options.INPUT_DSN)
    args = export_flow.router_args(board_at(dsn_dir / "b.kicad_pcb"), opts)
    assert "--kicad-dru" in args
    rows = dump(router, dsn_dir / "b.dsn", args, dsn_dir)
    assert rows["board_minimums.track_width_um"] == (127.0, "kicad_dru, over board setup flags")
    assert rows["min_trace_width_um"][0] is None

    # the snapshot: the router reads the `.kicad_pro` and the `.kicad_dru` itself
    snap = tmp_path / "snap"
    snap.mkdir()
    synthetic = REPO / "fixtures/kicad/synthetic"
    shutil.copy(synthetic / "classes.kicad_pcb", snap / "b.kicad_pcb")
    shutil.copy(synthetic / "classes.kicad_pro", snap / "b.kicad_pro")
    (snap / "b.kicad_dru").write_text(RULE, encoding="utf-8")
    opts = replace(RouteOptions(), board_input=options.INPUT_KICAD_PCB)
    args = export_flow.router_args(board_at(snap / "b.kicad_pcb"), opts)
    assert "--apply-design-rules" in args
    rows = dump(router, snap / "b.kicad_pcb", args, snap)
    assert rows["board_minimums.track_width_um"] == (127.0, "kicad_dru, over board setup flags")

    # without the rule Board Setup's value stays; the user's width still wins
    (snap / "b.kicad_dru").write_text("(version 1)\n", encoding="utf-8")
    opts = replace(opts, min_trace_width_um=180.0)
    rows = dump(router, snap / "b.kicad_pcb", export_flow.router_args(board_at(snap / "b.kicad_pcb"), opts), snap)
    assert rows["board_minimums.track_width_um"] == (200.0, "board setup flags")
    assert rows["min_trace_width_um"] == (180.0, "CLI")

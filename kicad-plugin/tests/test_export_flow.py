"""The current board, rather than its cached fill/old dialog rules, is routed."""

import hashlib
import json
from types import SimpleNamespace

import pytest

import export_flow
import options
from stub_ui import FakeBoard, FakeTrack


class Zone:
    def GetIsRuleArea(self):
        return False


@pytest.fixture
def prepared(tmp_path, monkeypatch):
    events = []
    board = FakeBoard(
        tmp_path / "board.kicad_pcb",
        tracks=[FakeTrack("unlocked"), FakeTrack("locked", True)],
    )
    board.Zones = lambda: [Zone()]

    class Filler:
        def __init__(self, live):
            assert live is board

        def Fill(self, zones):
            assert len(zones) == 1
            events.append("fill")
            return True

    pcbnew = SimpleNamespace(ZONE_FILLER=Filler, GetBuildVersion=lambda: "test KiCad")
    monkeypatch.setattr(export_flow, "fix_plane_priorities", lambda *_: events.append("planes") or [])
    monkeypatch.setattr(export_flow, "fix_rule_areas", lambda *_: events.append("areas") or [])
    monkeypatch.setattr(export_flow, "fix_edge_graphics", lambda *_: events.append("edges") or [])
    monkeypatch.setattr(export_flow, "fix_copper_graphics", lambda *_: events.append("copper") or [])
    monkeypatch.setattr(export_flow, "fix_thermal_spokes", lambda *_: events.append("thermal") or [])
    monkeypatch.setattr(export_flow, "solder_mask_args", lambda *_: ([], events.append("mask") or []))
    monkeypatch.setattr(export_flow, "fix_dsn", lambda *_: ([], events.append("fixups") or []))

    def export(live, path):
        assert live is board
        events.append("export")
        path.write_text("(pcb board)", encoding="utf-8")
        return True

    return board, pcbnew, events, export, tmp_path / "board.dsn"


def test_refill_precedes_export_and_fixups(prepared):
    board, pcbnew, events, export, path = prepared
    ok, args, lines, locked = export_flow.prepare(board, pcbnew, path, options.RouteOptions(), export)
    assert ok and args == [] and locked == []
    assert events == ["fill", "export", "planes", "areas", "edges", "copper", "thermal", "mask", "fixups"]
    assert lines == ["zones refilled before routing export"]


@pytest.mark.parametrize("raises", [False, True])
def test_failed_refill_stops_before_export_and_does_not_lock_tracks(prepared, raises):
    board, pcbnew, events, export, path = prepared

    def fail(_zones):
        if raises:
            raise RuntimeError("fill exception")
        return False

    pcbnew.ZONE_FILLER = lambda _: SimpleNamespace(Fill=fail)
    ok, _, lines, locked = export_flow.prepare(board, pcbnew, path, options.RouteOptions(fix_existing=True), export)
    assert not ok and not locked and not events and not path.exists()
    assert "routing preparation failed" in lines[0]
    assert [t.IsLocked() for t in board.GetTracks()] == [False, True]


@pytest.mark.parametrize("raises", [False, True])
def test_export_failure_restores_original_locks(prepared, raises):
    board, pcbnew, _, _, path = prepared

    def fail(*_):
        assert all(t.IsLocked() for t in board.GetTracks())
        if raises:
            raise RuntimeError("export exception")
        return False

    ok, _, lines, locked = export_flow.prepare(board, pcbnew, path, options.RouteOptions(fix_existing=True), fail)
    assert not ok and not locked and "routing preparation failed" in lines[0]
    assert [t.IsLocked() for t in board.GetTracks()] == [False, True]


def test_snapshot_also_refills_before_saving(prepared, monkeypatch):
    board, pcbnew, events, _, path = prepared
    monkeypatch.setattr(export_flow.snapshot, "take", lambda *_: (events.append("snapshot") or True, []))
    ok, _, lines, _ = export_flow.prepare_snapshot(board, pcbnew, path.parent, path.stem, path.parent, options.RouteOptions())
    assert ok and events == ["fill", "snapshot"]
    assert "zones refilled before routing export" in lines


def test_a_board_without_copper_zones_needs_no_filler(prepared):
    board, _, events, export, path = prepared
    board.Zones = lambda: []
    ok, _, _, _ = export_flow.prepare(board, object(), path, options.RouteOptions(), export)
    assert ok and events == ["export", "planes", "areas", "edges", "copper", "thermal", "mask", "fixups"]


def test_recipe_records_prepared_input_and_current_minimum(prepared):
    board, pcbnew, _, export, path = prepared
    opts = options.RouteOptions(max_passes=1, profile="legacy", sweep="free", sweep_passes=2)
    ok, extra, lines, _ = export_flow.prepare(board, pcbnew, path, opts, export)
    assert ok
    board.min_track_nm = 225000  # Board Setup changed after opening the dialog
    recipe = json.loads(export_flow.save_arguments(board, pcbnew, path, opts, extra, lines).read_text())
    assert recipe["args"] == export_flow.router_args(board, opts, extra)
    # K15.44: Board Setup's minimum at Board Setup's place in the merge, no default width
    assert "--board-setup.router.board_minimums.track_width_um=225" in recipe["args"]
    assert not any(a.startswith("--router.min_trace_width_um") for a in recipe["args"])
    assert recipe["input_sha256"] == hashlib.sha256(path.read_bytes()).hexdigest()
    assert recipe["log"] == lines and recipe["kicad"] == "test KiCad"


def test_session_import_fixes_drills_before_refilling(prepared, monkeypatch):
    board, pcbnew, events, _, path = prepared
    path.write_text("(session board)", encoding="utf-8")
    monkeypatch.setattr(
        export_flow.kicad_board,
        "fix_via_drills",
        lambda *_: events.append("drills") or 1,
    )
    lines = export_flow.after_import(board, path, pcbnew)
    assert events == ["drills", "fill"]
    assert lines[-1] == "zones refilled after session import"

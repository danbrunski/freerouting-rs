"""K15.27: the DSN input passes the thickness and clearance of the board's
signal-layer zones (`router.cleanup.pour_min_thickness_um`, `pour_clearance_um`),
which KiCad's DSN export drops; the `.kicad_pcb` input passes neither."""

from types import SimpleNamespace

import pytest

import export_flow
import options
from stub_ui import FakeBoard

F_CU, IN1_CU, IN2_CU, B_CU = 0, 1, 2, 31
NAMES = {F_CU: "F.Cu", IN1_CU: "In1.Cu", IN2_CU: "In2.Cu", B_CU: "B.Cu"}
MM = 1_000_000
THICKNESS = "--router.cleanup.pour_min_thickness_um="
CLEARANCE = "--router.cleanup.pour_clearance_um="


class Layers:
    def __init__(self, layers):
        self.layers = list(layers)

    def Seq(self):
        return sorted(self.layers)


class Zone:
    def __init__(self, layers, thickness_mm, clearance_mm, rule_area=False):
        self.layers, self.rule_area = layers, rule_area
        self.thickness, self.clearance = round(thickness_mm * MM), (None if clearance_mm is None else round(clearance_mm * MM))

    def GetIsRuleArea(self):
        return self.rule_area

    def GetLayerSet(self):
        return Layers(self.layers)

    def GetMinThickness(self):
        return self.thickness

    def GetLocalClearance(self):
        return self.clearance


def dsn_text(types):
    layers = "".join(f"    (layer {n}\n      (type {t})\n    )\n" for n, t in types.items())
    return f"(pcb test.dsn\n  (structure\n{layers}  )\n)\n"


STACK = {"F.Cu": "signal", "In1.Cu": "power", "In2.Cu": "power", "B.Cu": "signal"}
PCBNEW = SimpleNamespace(IsCopperLayer=lambda layer: layer in NAMES)


class Board:
    def __init__(self, zones):
        self.zones = zones

    def Zones(self):
        return self.zones

    def GetLayerName(self, layer):
        return NAMES[layer]


@pytest.fixture
def run(tmp_path, monkeypatch):
    """`prepare` on a stub pcbnew, the other fix-ups off: the router args and log."""
    for name in ("fix_plane_priorities", "fix_rule_areas", "fix_edge_graphics", "fix_copper_graphics",
                 "fix_thermal_spokes", "fix_track_arcs"):
        monkeypatch.setattr(export_flow, name, lambda *_: [])
    monkeypatch.setattr(export_flow, "refill", lambda *_: [])
    monkeypatch.setattr(export_flow, "solder_mask_args", lambda *_: ([], []))
    monkeypatch.setattr(export_flow, "fix_dsn", lambda *_: ([], []))

    def go(zones, types=STACK):
        path = tmp_path / "board.dsn"

        def export(_board, dsn):
            dsn.write_text(dsn_text(types), encoding="utf-8")
            return True

        ok, args, lines, _ = export_flow.prepare(Board(zones), PCBNEW, path, options.RouteOptions(), export)
        assert ok
        return args, lines

    return go


def test_the_smallest_thickness_and_clearance_of_the_signal_layer_zones(run):
    zones = [
        Zone([F_CU], 0.5, 0.5),
        Zone([F_CU], 0.4, 0.6),
        Zone([IN1_CU], 0.2, 0.2),  # a power layer: no signal-layer pour
        Zone([B_CU], 0.5, 0.8),
    ]
    args, lines = run(zones)
    assert args == [THICKNESS + "400", CLEARANCE + "500"]
    assert lines == [
        "signal-layer zones: 3 on F.Cu, B.Cu; the cleanup judges their pours by min thickness 400 um and "
        f"clearance 500 um ({' '.join(args)})"
    ]


def test_a_zone_across_a_signal_and_a_power_layer_counts_its_signal_layer(run):
    args, _ = run([Zone([F_CU, IN1_CU], 0.45, 0.55), Zone([IN2_CU], 0.1, 0.1)])
    assert args == [THICKNESS + "450", CLEARANCE + "550"]


def test_a_zone_without_its_own_clearance_counts_the_zone_default(run):
    args, _ = run([Zone([F_CU], 0.5, None), Zone([B_CU], 0.5, 0.7)])
    assert args == [THICKNESS + "500", CLEARANCE + "500"]  # kicad_board.ZONE_CLEARANCE_NM


def test_a_board_without_signal_layer_zones_passes_neither(run):
    # inner plane zones, a rule area on a signal layer, a zone on a mixed layer, and no zone at all
    zones = [Zone([IN1_CU], 0.5, 0.5), Zone([F_CU], 0.2, 0.2, rule_area=True)]
    assert run(zones) == ([], [])
    mixed = dict(STACK, **{"B.Cu": "mixed"})
    assert run([Zone([B_CU], 0.5, 0.5)], mixed) == ([], [])
    assert run([]) == ([], [])


def test_a_power_layer_opened_to_routing_is_a_signal_layer(run):
    # route_on_plane_layers rewrites (type power) to (type signal) before the flags are read
    args, _ = run([Zone([IN1_CU], 0.3, 0.4)], dict(STACK, **{"In1.Cu": "signal"}))
    assert args == [THICKNESS + "300", CLEARANCE + "400"]


def test_a_zero_value_passes_no_flag_for_it(run):
    args, _ = run([Zone([F_CU], 0.0, 0.5)])
    assert args == [CLEARANCE + "500"]


def test_a_failing_read_is_a_warning_and_no_flags(run):
    args, lines = run([object()])
    assert args == [] and lines[0].startswith("WARNING: zone thickness and clearance not passed")


def test_the_flags_reach_the_router_command_line(run):
    args, _ = run([Zone([F_CU], 0.5, 0.5), Zone([F_CU], 0.4, 0.5)])
    board = SimpleNamespace(GetDesignSettings=lambda: SimpleNamespace(m_TrackMinWidth=0))
    full = export_flow.router_args(board, options.RouteOptions(), args)
    assert full == [THICKNESS + "400", CLEARANCE + "500"]


def test_the_kicad_pcb_input_passes_neither(tmp_path, monkeypatch):
    monkeypatch.setattr(export_flow, "refill", lambda *_: [])
    monkeypatch.setattr(export_flow.snapshot, "take", lambda *a: (True, []))
    board = Board([Zone([F_CU], 0.4, 0.5)])
    opts = options.RouteOptions(board_input=options.INPUT_KICAD_PCB)
    ok, args, lines, _ = export_flow.prepare_snapshot(board, PCBNEW, tmp_path, "b", tmp_path, opts)
    assert ok and not [a for a in args if "pour_" in a] and not [x for x in lines if "signal-layer zones" in x]
    board_args = export_flow.router_args(SimpleNamespace(GetDesignSettings=lambda: SimpleNamespace(m_TrackMinWidth=0)), opts, args)
    assert not [a for a in board_args if "cleanup.pour_" in a]

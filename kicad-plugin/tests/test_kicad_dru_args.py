"""Q22.5: the plugin passes the project's .kicad_dru to the router for a DSN input."""

import types
from dataclasses import replace

import export_flow
import options
from options import RouteOptions


def board_at(path):
    return types.SimpleNamespace(
        GetFileName=lambda: str(path),
        GetDesignSettings=lambda: types.SimpleNamespace(m_TrackMinWidth=0),
    )


def test_a_dsn_route_gets_the_kicad_dru(tmp_path):
    pcb = tmp_path / "b.kicad_pcb"
    pcb.write_text("(kicad_pcb)", encoding="utf-8")
    (tmp_path / "b.kicad_dru").write_text("(version 1)", encoding="utf-8")
    args = export_flow.router_args(board_at(pcb), RouteOptions())
    assert args[-2:] == ["--kicad-dru", str(tmp_path / "b.kicad_dru")]
    # every profile: a fix to the input
    for profile in options.PROFILES:
        assert "--kicad-dru" in export_flow.router_args(board_at(pcb), options.profile_defaults(profile))


def test_no_flag_without_a_file_for_the_gui_or_the_kicad_pcb_input(tmp_path):
    pcb = tmp_path / "b.kicad_pcb"
    assert "--kicad-dru" not in export_flow.router_args(board_at(pcb), RouteOptions())
    (tmp_path / "b.kicad_dru").write_text("(version 1)", encoding="utf-8")
    assert "--kicad-dru" not in export_flow.router_args(board_at(pcb), RouteOptions(), for_gui=True)
    snapshot = replace(RouteOptions(), board_input=options.INPUT_KICAD_PCB)
    assert "--kicad-dru" not in export_flow.router_args(board_at(pcb), snapshot)
    unsaved = types.SimpleNamespace(GetFileName=lambda: "", GetDesignSettings=lambda: None)
    assert export_flow.kicad_dru_args(unsaved, RouteOptions()) == []

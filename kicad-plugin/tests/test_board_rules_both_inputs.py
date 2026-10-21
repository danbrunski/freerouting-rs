"""K13.1: the board's `.kicad_dru` rules reach the router on both inputs
(decided 2026-10-20). A DSN route gets `--kicad-dru <board>.kicad_dru` (Q22.5);
a snapshot route gets `--apply-design-rules`, whatever the K10.3 option
`apply_kicad_rules` says, which the router applies by default since K13.1
anyway. Route in GUI applies them on neither input (its command line has no
such flag)."""

import types
from dataclasses import replace

import export_flow
import options
import snapshot
from options import RouteOptions


def board_at(path):
    return types.SimpleNamespace(
        GetFileName=lambda: str(path),
        GetDesignSettings=lambda: types.SimpleNamespace(m_TrackMinWidth=0),
    )


def snapshot_options(**kw):
    return replace(RouteOptions(), board_input=options.INPUT_KICAD_PCB, **kw)


def test_a_snapshot_route_applies_the_rules_whatever_the_old_option_says(tmp_path):
    pcb = tmp_path / "b.kicad_pcb"
    (tmp_path / "b.kicad_dru").write_text("(version 1)", encoding="utf-8")
    for old in (False, True):
        args = export_flow.router_args(board_at(pcb), snapshot_options(apply_kicad_rules=old))
        assert args.count("--apply-design-rules") == 1, args
        assert "--kicad-dru" not in args
        assert snapshot.router_args(snapshot_options(apply_kicad_rules=old)) == ["--apply-design-rules"]


def test_every_profile_applies_the_rules_on_both_inputs(tmp_path):
    pcb = tmp_path / "b.kicad_pcb"
    (tmp_path / "b.kicad_dru").write_text("(version 1)", encoding="utf-8")
    for profile in options.PROFILES:
        dsn = options.profile_defaults(profile)
        assert "--kicad-dru" in export_flow.router_args(board_at(pcb), dsn)
        snap = replace(dsn, board_input=options.INPUT_KICAD_PCB)
        assert "--apply-design-rules" in export_flow.router_args(board_at(pcb), snap)


def test_route_in_gui_applies_the_rules_on_neither_input(tmp_path):
    pcb = tmp_path / "b.kicad_pcb"
    (tmp_path / "b.kicad_dru").write_text("(version 1)", encoding="utf-8")
    for opts in (RouteOptions(), snapshot_options(apply_kicad_rules=True)):
        args = export_flow.router_args(board_at(pcb), opts, for_gui=True)
        assert "--kicad-dru" not in args and "--apply-design-rules" not in args


def test_the_option_itself_no_longer_adds_the_switch():
    # `export_flow.kicad_dru_args` decides for both inputs; `to_args` adds neither.
    assert snapshot_options(apply_kicad_rules=True).to_args() == []
    assert RouteOptions(apply_kicad_rules=True).to_args() == []


def test_the_snapshot_log_says_the_rules_are_applied():
    notes = snapshot.notes(snapshot_options(rule_area_fixup=False, edge_and_pad_keepouts=False))
    assert notes == ["the .kicad_dru rules are applied (--apply-design-rules; a DSN route gets --kicad-dru; Route in GUI reads them only)"]

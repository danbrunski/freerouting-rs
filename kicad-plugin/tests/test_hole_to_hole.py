"""Q22.11: Board Setup's minimum hole-to-hole distance (`m_HoleToHoleMin`)
reaches the router as `--router.board_minimums.hole_to_hole_um`: no routed via
hole comes closer to another drill, of any net (KiCad's DRC: `hole_to_hole`,
"board setup constraints")."""

import types

import export_flow
from options import BOARD_MINIMUM_SETTINGS, RouteOptions, board_minimum_args


def test_the_hole_to_hole_minimum_is_a_board_minimum():
    assert ("m_HoleToHoleMin", "router.board_minimums.hole_to_hole_um") in BOARD_MINIMUM_SETTINGS
    # KiCad's default, 0.25 mm
    assert board_minimum_args({"m_HoleToHoleMin": 250000}) == ["--router.board_minimums.hole_to_hole_um=250"]
    assert board_minimum_args({"m_HoleToHoleMin": 254000}) == ["--router.board_minimums.hole_to_hole_um=254"]
    # 0 is KiCad's "no minimum": nothing passed
    assert board_minimum_args({"m_HoleToHoleMin": 0}) == []


def test_router_args_carry_the_hole_to_hole_minimum():
    board = types.SimpleNamespace(
        GetDesignSettings=lambda: types.SimpleNamespace(
            m_TrackMinWidth=152400, m_MinClearance=127000, m_HoleToHoleMin=254000
        )
    )
    assert export_flow.router_args(board, RouteOptions()) == [
        "--router.min_trace_width_um=152.4",
        "--router.board_minimums.track_width_um=152.4",
        "--router.board_minimums.clearance_um=127",
        "--router.board_minimums.hole_to_hole_um=254",
    ]

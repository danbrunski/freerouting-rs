"""Q22.10: Board Setup's minimum clearance (`m_MinClearance`) reaches the
router as `--router.board_minimums.clearance_um`, a floor under every net
class clearance (KiCad's DRC: "board minimum clearance")."""

import types

import export_flow
from options import BOARD_MINIMUM_SETTINGS, RouteOptions, board_minimum_args


def test_the_minimum_clearance_is_a_board_minimum():
    assert ("m_MinClearance", "router.board_minimums.clearance_um") in BOARD_MINIMUM_SETTINGS
    assert board_minimum_args({"m_MinClearance": 127000}) == ["--router.board_minimums.clearance_um=127"]
    # KiCad's default is 0: no floor
    assert board_minimum_args({"m_MinClearance": 0}) == []


def test_router_args_carry_the_minimum_clearance():
    board = types.SimpleNamespace(
        GetDesignSettings=lambda: types.SimpleNamespace(m_TrackMinWidth=152400, m_MinClearance=127000)
    )
    # K15.44: as Board Setup's values, which a `.kicad_dru` rule replaces
    assert export_flow.router_args(board, RouteOptions()) == [
        "--board-setup.router.board_minimums.track_width_um=152.4",
        "--board-setup.router.board_minimums.clearance_um=127",
    ]

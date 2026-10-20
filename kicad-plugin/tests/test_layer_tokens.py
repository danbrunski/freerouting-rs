"""A DSN layer whose name KiCad quotes (vfo-controller's "Top Layer, Top": a
copper layer with a user name) is written quoted by every fix-up that names a
layer. Unquoted, the router reads `(polygon Top Layer, Top 0 ...)` as layer
`Top` and drops the shape ("wire has no shape"): vfo's Q18.5/Q18.7 edge and
mask keepouts (54 lines) never reached the router."""

import copper_graphics
import dsn_fixups
import edge_graphics
from conftest import TESTS

TOP, BOTTOM = '"Top Layer, Top"', '"Bottom Layer, Bottom"'


def spaced(name):
    text = (TESTS / "data" / name).read_text(encoding="utf-8")
    return text.replace("F.Cu", TOP).replace("B.Cu", BOTTOM)


def unquoted(text):
    """The lines that name a spaced layer without its quotes."""
    return [
        line
        for line in text.splitlines()
        if any(f"({kind} Top Layer" in line or f"({kind} Bottom Layer" in line for kind in ("path", "polygon", "rect", "circle"))
    ]


def test_the_layer_token_is_the_dsn_s_own_spelling():
    dsn = dsn_fixups.Dsn(spaced("k09_two_layer.dsn"))
    assert dsn.layer_token("Top Layer, Top") == TOP
    assert dsn_fixups.Dsn(spaced("k09_two_layer.dsn").replace(TOP, "F.Cu")).layer_token("F.Cu") == "F.Cu"
    # a name the DSN does not list stays as it is
    assert dsn.layer_token("In1.Cu") == "In1.Cu"


def test_edge_and_pad_keepouts_quote_the_layer():
    board = {
        "edge_clearance_mm": 0.5,
        "outline_bbox_mm": (100.0, 50.0, 150.0, 90.0),
        "pads": [
            {"ref": "FID1", "local_clearance_mm": 0.6, "size_mm": (1.0, 1.0), "at_mm": (120.0, 60.0), "layers": ["Top Layer, Top"]}
        ],
        "zones": [],
    }
    out, _ = dsn_fixups.apply(spaced("k09_two_layer.dsn"), board, {"edge_and_pad_keepouts": True})
    assert f"(rect {TOP} " in out and f"(rect {BOTTOM} " in out
    assert f"(circle {TOP} " in out
    assert unquoted(out) == []


def test_fixed_pours_quote_the_layer():
    board = {
        "zones": [
            {
                "net": "+5V",
                "layer": "Top Layer, Top",
                "outer": True,
                "priority": 2,
                "filled": True,
                "fill": [[(100.0, 50.0), (110.0, 50.0), (110.0, 52.0), (100.0, 52.0)]],
            }
        ]
    }
    out, _ = dsn_fixups.apply(spaced("k09_four_layer.dsn"), board, {"fixed_pour_classes": ["Power"]})
    assert unquoted(out) == []


def test_edge_graphics_quote_the_layer():
    ring = [(100.0, 50.0), (150.0, 50.0), (150.0, 90.0), (100.0, 90.0)]
    facts = {
        "edge_clearance_mm": 0.5,
        "outline_mm": [ring],
        "shapes": [{"layer": "Margin", "width_mm": 0.1, "kind": "polyline", "points": [(120.0, 60.0), (120.0, 80.0)]}],
    }
    out, _ = edge_graphics.apply(spaced("k09_two_layer.dsn"), facts)
    assert f"(polygon {TOP} 0 " in out and f"(polygon {BOTTOM} 0 " in out
    assert unquoted(out) == []


def test_copper_graphics_quote_the_layer():
    line = {"layer": "Bottom Layer, Bottom", "width_mm": 0.5, "kind": "polyline", "points": [(13.2, 10.0), (16.8, 10.0)], "filled": False, "net": ""}
    text = {"layer": "Top Layer, Top", "polygons": [[(13.0, 9.2), (14.2, 9.2), (14.2, 10.8), (13.0, 10.8)]]}
    out, _ = copper_graphics.apply(spaced("q22-copper-graphics.dsn"), {"shapes": [line], "texts": [text]})
    assert f'(keepout "copper_graphic" (polygon {BOTTOM} 0 ' in out
    assert f'(keepout "copper_text" (polygon {TOP} 0 ' in out
    assert unquoted(out) == []

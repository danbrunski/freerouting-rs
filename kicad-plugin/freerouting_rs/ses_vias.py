"""The via sizes of a Specctra session, to correct KiCad's SES import (K09).

KiCad 10's `ImportSpecctraSES` takes a routed via's diameter from the
session's padstack but its drill from the via's net class. When the router
puts a `Via[0-3]_600:300_um` on a net whose class drills 0.4 mm, the board
gets a 0.6 mm via with a 0.4 mm drill, and KiCad's DRC reports an annular
ring of 0.1 mm (canary-pwr, four `+12V_BUS`/`+12V_PROT` vias, K09.8's first
graded plugin run). canary's `ses_import.py` reads the drill from the
padstack name instead, which is what `vias` does here: `kicad_board`'s
`fix_via_drills` then sets the drill of each imported via that sits on a
session via of the same net.

Padstack names that KiCad's export writes: `Via[<layers>]_<dia>:<drill>_um`.
A name without the `<dia>:<drill>` part is skipped (its via keeps KiCad's
drill).

Pure Python: no pcbnew, no wx.
"""

import re

try:  # inside the plugin package
    from .rule_areas import parse
except ImportError:  # tests and headless scripts put this folder on sys.path
    from rule_areas import parse

_SIZE = re.compile(r"_(\d+(?:\.\d+)?):(\d+(?:\.\d+)?)_um$")
_UNITS_NM = {"um": 1000.0, "mm": 1e6, "mil": 25400.0, "inch": 25.4e6}


def _unquote(text):
    return text[1:-1] if len(text) >= 2 and text[0] == text[-1] == '"' else text


def _child(node, head):
    return next((c for c in node.children if c.is_list and c.head == head), None)


def vias(ses_text):
    """`[(net, x_nm, y_nm, diameter_nm, drill_nm)]` for the session's vias
    whose padstack names carry a size, in KiCad's frame (Y down)."""
    roots = parse(ses_text)
    session = next((r for r in roots if r.head == "session"), None)
    routes = _child(session, "routes") if session else None
    if routes is None:
        return []
    res = _child(routes, "resolution")
    try:
        nm_per_unit = _UNITS_NM[res.children[1].text] / float(res.children[2].text)
    except (AttributeError, IndexError, KeyError, ValueError, ZeroDivisionError):
        return []
    network = _child(routes, "network_out")
    out = []
    for net in network.children if network else []:
        if not net.is_list or net.head != "net" or len(net.children) < 2:
            continue
        name = _unquote(net.children[1].text)
        for via in net.children[2:]:
            if not via.is_list or via.head != "via" or len(via.children) < 4:
                continue
            m = _SIZE.search(_unquote(via.children[1].text))
            if not m:
                continue
            try:
                x, y = float(via.children[2].text), float(via.children[3].text)
            except ValueError:
                continue
            out.append(
                (name, round(x * nm_per_unit), round(-y * nm_per_unit), round(float(m[1]) * 1000), round(float(m[2]) * 1000))
            )
    return out

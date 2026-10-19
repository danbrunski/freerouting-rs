import difflib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest
import rule_areas
from conftest import REPO, TESTS

DATA = TESTS / "data"


def rect(x0, y0, x1, y1):
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]


def area(name, layers, outline, **flags):
    base = dict(tracks=False, vias=False, pads=False, footprints=False, copper_pour=False)
    base.update(flags)
    return dict(name=name, layers=layers, outline=outline, holes=[], **base)


FIVE = [
    area("tv", ["F.Cu"], rect(1000, -1000, 2000, -2000), tracks=True, vias=True, pads=True),
    area("vo", ["F.Cu"], rect(3000, -1000, 4000, -2000), vias=True),
    area("to", ["B.Cu"], rect(5000, -1000, 6000, -2000), tracks=True),
    area("fo", ["B.Cu"], rect(7000, -1000, 8000, -2000), footprints=True),
    area("pp", ["B.Cu"], rect(9000, -1000, 10000, -2000), pads=True, copper_pour=True),
]


def scopes(text):
    """(keyword, name) of every keepout-like scope directly in (structure)."""
    out = []
    for root in rule_areas.parse(text):
        for c in root.children:
            if c.is_list and c.head == "structure":
                out += [(s.head, s.children[1].text) for s in c.children if s.is_list and s.head and "keepout" in s.head]
    return out


def test_five_board_level_areas():
    text = (DATA / "five_areas.dsn").read_text()
    new, log = rule_areas.rewrite(text, FIVE)
    assert scopes(new) == [
        ("keepout", '"tv"'),
        ("via_keepout", '"vo"'),
        ("wire_keepout", '"to"'),
        ("place_keepout", '"fo"'),
    ]
    assert [l for l in log if l.startswith("WARNING")] == [
        "WARNING: rule area 'to' on B.Cu: tracks-only area is written as wire_keepout, which the router "
        "reads as a full keepout: it blocks vias too, unless the route passes --router.wire_keepout=traces_only"
    ]
    assert "rule area 'fo' on B.Cu: keepout -> place_keepout (bans: footprints)" in log
    assert any("rule area 'pp' on B.Cu: keepout removed" in l for l in log)
    # Only those two scopes changed: a diff shows the 'fo' keyword and the 'pp' lines.
    diff = [l for l in difflib.unified_diff(text.splitlines(), new.splitlines(), lineterm="", n=0) if l[0] in "+-" and l[:3] not in ("---", "+++")]
    assert diff == [
        '-    (keepout "fo" (polygon B.Cu 0  7000 -1000  8000 -1000  8000 -2000  7000 -2000',
        '+    (place_keepout "fo" (polygon B.Cu 0  7000 -1000  8000 -1000  8000 -2000  7000 -2000',
        '-    (keepout "pp" (polygon B.Cu 0  9000 -1000  10000 -1000  10000 -2000  9000 -2000',
        "-            9000 -1000))",
    ]


def test_image_keepouts_are_never_changed():
    text = (DATA / "five_areas.dsn").read_text()
    new, _ = rule_areas.rewrite(text, FIVE)
    image = lambda t: t[t.index("(library") :]
    assert image(new) == image(text)
    assert '(keepout "" (polygon F.Cu 0  7000' in image(new)
    # even when the image's polygon is identical to a footprint-only board area
    new2, _ = rule_areas.rewrite(text, [area("x", ["F.Cu"], rect(7000, -1000, 8000, -2000), footprints=True)])
    assert image(new2) == image(text)


def test_keyword_must_agree_with_flags_and_areas_match_one_to_one():
    text = (DATA / "five_areas.dsn").read_text()
    # An area whose polygon matches but whose flags would have made KiCad write
    # another keyword does not match.
    wrong = [area("w", ["B.Cu"], rect(7000, -1000, 8000, -2000), vias=True)]
    new, log = rule_areas.rewrite(text, wrong)
    assert new == text
    assert any("matches no rule area" in l for l in log)
    # Two identical areas consume two identical keepouts, not one twice.
    dup = text.replace("(via \"Via", '(keepout "fo2" (polygon B.Cu 0  7000 -1000  8000 -1000  8000 -2000  7000 -2000))\n    (via "Via')
    two = [area("a", ["B.Cu"], rect(7000, -1000, 8000, -2000), footprints=True)] * 2
    new, _ = rule_areas.rewrite(dup, two)
    assert [k for k, n in scopes(new) if n in ('"fo"', '"fo2"')] == ["place_keepout", "place_keepout"]


def test_match_tolerance_and_orientation():
    text = (DATA / "five_areas.dsn").read_text()
    shifted = [(x + 0.9, y - 0.9) for x, y in reversed(rect(7000, -1000, 8000, -2000))]
    new, _ = rule_areas.rewrite(text, [area("s", ["B.Cu"], shifted, footprints=True)])
    assert ("place_keepout", '"fo"') in scopes(new)
    far = [(x + 1.5, y) for x, y in rect(7000, -1000, 8000, -2000)]
    new, _ = rule_areas.rewrite(text, [area("s", ["B.Cu"], far, footprints=True)])
    assert new == text


def test_wrong_layer_does_not_match():
    text = (DATA / "five_areas.dsn").read_text()
    new, log = rule_areas.rewrite(text, [area("fo", ["F.Cu"], rect(7000, -1000, 8000, -2000), footprints=True)])
    assert new == text
    assert any("has no keepout in the DSN" in l for l in log)


def test_multi_layer_area_rewrites_each_layer():
    text = (
        "(pcb x (structure\n"
        '  (keepout "" (polygon F.Cu 0  0 0  10 0  10 -10  0 -10))\n'
        '  (keepout "" (polygon B.Cu 0  0 0  10 0  10 -10  0 -10))\n'
        "))\n"
    )
    a = area("fan", ["F.Cu", "B.Cu"], rect(0, 0, 10, -10), footprints=True)
    new, _ = rule_areas.rewrite(text, [a])
    assert new.count("place_keepout") == 2 and "(keepout" not in new


def test_footprint_owned_area_only_warns():
    text = (DATA / "five_areas.dsn").read_text()
    a = area("own", ["F.Cu"], rect(7000, -1000, 8000, -2000), footprints=True)
    a["footprint"] = "U1"
    new, log = rule_areas.rewrite(text, [a])
    assert new == text
    assert log[0] == (
        "WARNING: rule area 'own' of footprint U1 (bans: footprints) is written into the footprint "
        "image by KiCad; left as exported"
    )
    assert not any(l.startswith("WARNING") for l in log[1:])


def test_crlf_and_trailing_text_preserved():
    text = (DATA / "five_areas.dsn").read_text().replace("\n", "\r\n")
    new, _ = rule_areas.rewrite(text, FIVE)
    assert "\n" not in new.replace("\r\n", "")
    assert "\r\n" in new and 'place_keepout "fo"' in new and '"pp"' not in new


def test_quoted_parentheses_and_names_do_not_confuse_the_scanner():
    text = '(pcb x (structure (keepout "a)(b" (polygon F.Cu 0  0 0  10 0  10 -10 0 -10))))\n'
    new, _ = rule_areas.rewrite(text, [area("n", ["F.Cu"], rect(0, 0, 10, -10), footprints=True)])
    assert new == text.replace("(keepout", "(place_keepout", 1)


def test_malformed_dsn_is_returned_unchanged_with_a_warning():
    for bad in ("(pcb (structure", "(pcb))", '(pcb "open)'):
        new, log = rule_areas.rewrite(bad, FIVE)
        assert new == bad and log[0].startswith("WARNING: DSN not rewritten")


def test_avionics_hub_wire_keepout_byte_identical_with_warning():
    path = REPO / "fixtures" / "Issue754-avionics_hub.dsn"
    text = path.read_bytes().decode("utf-8")
    ground = [(11900, -11700), (186900, -11700), (186900, -142500), (11900, -142500), (11900, -11700)]
    new, log = rule_areas.rewrite(text, [area("", ["Ground"], ground, tracks=True)])
    assert new == text
    assert len(log) == 1 and log[0].startswith("WARNING") and "blocks vias too" in log[0]


CANARY_FAN = [(136500, -53000), (162500, -53000), (162500, -79000), (136500, -79000), (136500, -53000)]
FAN_AREA = area("Fan inlet keep-out (ASSUMED position - verify on the Pi)", ["B.Cu"], CANARY_FAN, pads=True, footprints=True)


def assert_only_fan_scope_changed(old, new, log):
    assert "place_keepout" in new
    old_lines, new_lines = old.splitlines(), new.splitlines()
    changed = [l for l in difflib.unified_diff(old_lines, new_lines, lineterm="", n=0) if l[0] in "+-" and l[:3] not in ("---", "+++")]
    assert len(changed) == 2 and changed[0].startswith("-    (keepout \"\" (polygon B.Cu 0  136500 -53000")
    assert changed[1] == changed[0].replace("-    (keepout", "+    (place_keepout", 1)
    assert not any("(keepout \"\" (polygon B.Cu 0  136500 -53000" in l for l in new_lines)
    assert any("keepout -> place_keepout (bans: pads, footprints)" in l for l in log)


def test_canary_fan_inlet_in_committed_kicad_export():
    # fixtures/canary-pwr/canary-pwr-2026-10-15-1747-unpoured.dsn is a plain KiCad
    # pcbnew.ExportSpecctraDSN output. Area flags are from the board's zone.
    path = REPO / "fixtures" / "canary-pwr" / "canary-pwr-2026-10-15-1747-unpoured.dsn"
    text = path.read_bytes().decode("utf-8")
    new, log = rule_areas.rewrite(text, [FAN_AREA])
    assert_only_fan_scope_changed(text, new, log)


def kicad_python():
    cand = [os.environ.get("FREEROUTING_RS_KICAD_PYTHON"), "/usr/bin/python3", "python3"]
    for c in filter(None, cand):
        exe = shutil.which(c)
        if exe and subprocess.run([exe, "-c", "import pcbnew"], capture_output=True).returncode == 0:
            return exe
    return None


def test_canary_pwr_fresh_kicad_export(tmp_path):
    """Needs KiCad's Python (FREEROUTING_RS_KICAD_PYTHON, or a system python3
    that imports pcbnew) and the board (CANARY_PCB, or ../canary/...); skipped
    otherwise. Works on a copy; the exported DSN stays in tmp_path."""
    board = Path(os.environ.get("CANARY_PCB", REPO.parent / "canary/hardware/canary-pwr-rev1.0/canary-pwr.kicad_pcb"))
    py = kicad_python()
    if py is None or not board.is_file():
        pytest.skip("KiCad Python (pcbnew) or the canary-pwr board is not available")
    copy = tmp_path / "canary-pwr.kicad_pcb"
    shutil.copy(board, copy)
    dsn, js = tmp_path / "canary-pwr.dsn", tmp_path / "areas.json"
    subprocess.run([py, str(TESTS / "kicad_dump.py"), str(copy), str(dsn), str(js)], check=True, capture_output=True, timeout=300)
    areas = json.loads(js.read_text())["areas"]
    fan = [a for a in areas if a["layers"] == ["B.Cu"] and a["footprints"] and a["pads"]]
    assert len(fan) == 1 and not fan[0]["tracks"] and not fan[0]["vias"]
    text = dsn.read_bytes().decode("utf-8")
    new, log = rule_areas.rewrite(text, areas)
    assert_only_fan_scope_changed(text, new, log)

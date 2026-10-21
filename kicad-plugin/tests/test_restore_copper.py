"""Q22.3 and K13.2: after `ImportSpecctraSES` the board's own copper comes back
at its own coordinates (`restore_copper`), on the DSN path (`%.6g` plus the
session's 0.1 µm) and on the `.kicad_pcb` path (the reader's 0.1 µm plus the
session's). Fake pcbnew items here; the plugin's Route flow through the stub
`pcbnew` and `wx` of `stub_ui`."""

import pytest
import restore_copper
import stub_ui
import track_arcs

F_CU, B_CU = 0, 2


class Pt:
    def __init__(self, x, y):
        self.x, self.y = x, y


class Kiid:
    count = 0

    def __init__(self, text=None):
        if text is None:
            Kiid.count += 1
            text = "uuid-%d" % Kiid.count
        self.text = text

    def AsString(self):
        return self.text


class Item:
    """A PCB_TRACK or PCB_VIA with the methods `restore_copper` and
    `kicad_board.fix_via_drills` call."""

    def __init__(self, cls, net, points, width, layer=F_CU, drill=0, locked=False, name=""):
        self.cls, self.net, self.layer, self.width, self.drill = cls, net, layer, width, drill
        self.points = [tuple(p) for p in points]
        self.locked, self.name = locked, name
        self.freed = False
        self.m_Uuid = Kiid()

    def SetUuid(self, kiid):
        self.m_Uuid = kiid

    def GetClass(self):
        return self.cls

    def IsLocked(self):
        return self.locked

    def SetLocked(self, value):
        self.locked = value

    def GetNetname(self):
        return self.net

    def GetLayer(self):
        return self.layer

    def TopLayer(self):
        return F_CU

    def BottomLayer(self):
        return B_CU

    def GetWidth(self, *_layer):
        return self.width

    def SetWidth(self, width):
        self.width = width

    def GetDrillValue(self):
        return self.drill

    GetDrill = GetDrillValue

    def SetDrill(self, drill):
        self.drill = drill

    def GetStart(self):
        return Pt(*self.points[0])

    def GetEnd(self):
        return Pt(*self.points[1])

    def GetPosition(self):
        return Pt(*self.points[0])

    def SetStart(self, p):
        self.points[0] = (p.x, p.y)

    def SetEnd(self, p):
        self.points[1] = (p.x, p.y)

    SetPosition = SetStart

    def copy(self, points=None, width=None):
        return Item(self.cls, self.net, points or self.points, width or self.width, self.layer, self.drill, name=self.name + "'")


def track(net, a, b, width=200000, **kw):
    return Item("PCB_TRACK", net, (a, b), width, **kw)


def via(net, at, size=600000, drill=300000, **kw):
    return Item("PCB_VIA", net, (at,), size, drill=drill, **kw)


def dsn_trip(nm):
    """One coordinate through KiCad's export (`%.6g` of micrometres, Y negated
    for the DSN), the session (0.1 µm units) and KiCad's import, in nm."""
    dsn_um = float("%.6g" % (-nm / 1000.0))
    return -round(dsn_um * 10) * 100


def reader_trip(nm):
    """One coordinate through the `.kicad_pcb` reader (0.1 µm) and the session."""
    return round(nm / 100) * 100


def imported(item, trip):
    return item.copy([(trip(x), trip(y)) for x, y in item.points])


# cm4-edge-ai's `/CM4 Core/PCIe_RX_N` end (Q22.1): 192.322488 mm is 192322 in the DSN.
DIFF_N = ((192322488, 73575000), (194175000, 75427512))


def test_an_input_track_that_the_export_rounds_comes_back_at_its_nanometre_coordinates():
    original = track("PCIe_RX_N", *DIFF_N)
    back = imported(original, dsn_trip)
    assert back.points == [(192322000, 73575000), (194175000, 75427500)]  # the round trip moved it
    counts = restore_copper.restore([back], restore_copper.originals([original]), restore_copper.bound_nm("dsn"))
    assert back.points == list(DIFF_N)
    assert counts == {"tracks": 1, "vias": 0, "arcs": 0, "ends": 0, "unmatched": 0}


def test_a_restored_item_gets_the_originals_uuid_and_a_moved_one_keeps_its_own():
    """K13.2: KiCad names the segments of a clearance item in uuid order."""
    original, moved_original = track("SYS", *DIFF_N), track("SYS", (1000000, 2000000), (5000000, 2000000))
    back, moved = imported(original, dsn_trip), track("SYS", (1002000, 2000000), (5000000, 2000000))
    new_ids = (back.m_Uuid.AsString(), moved.m_Uuid.AsString())
    restore_copper.restore([back, moved], restore_copper.originals([original, moved_original]), 600)
    assert back.m_Uuid.AsString() == original.m_Uuid.AsString() != new_ids[0]
    assert moved.m_Uuid.AsString() == new_ids[1]


def test_a_routed_track_that_ends_on_a_restored_track_follows_it():
    original = track("PCIe_RX_N", *DIFF_N)
    back = imported(original, dsn_trip)
    routed = track("PCIe_RX_N", back.points[0], (190000000, 73575000))  # the router met the rounded end
    other_net = track("GND", back.points[0], (190000000, 70000000))
    counts = restore_copper.restore([back, routed, other_net], restore_copper.originals([original]), 600)
    assert routed.points == [DIFF_N[0], (190000000, 73575000)]
    assert other_net.points[0] == (192322000, 73575000)  # another net never follows
    assert counts["ends"] == 1


def test_a_track_the_router_moved_2_um_stays_as_imported():
    original = track("SIG", (150000000, 50000000), (160000000, 50000000))
    moved = track("SIG", (150002000, 50000000), (160000000, 50000000))
    routed = track("SIG", (150002000, 50000000), (150002000, 40000000))  # ends on the moved track
    counts = restore_copper.restore([moved, routed], restore_copper.originals([original]), 600)
    assert moved.points == [(150002000, 50000000), (160000000, 50000000)]
    assert routed.points == [(150002000, 50000000), (150002000, 40000000)]
    assert counts == {"tracks": 0, "vias": 0, "arcs": 0, "ends": 0, "unmatched": 1}


def test_the_bound_is_the_exports_rounding_plus_the_sessions():
    assert restore_copper.bound_nm("dsn") == 600 and restore_copper.bound_nm("kicad_pcb") == 200
    original = track("SIG", (150000000, 50000000), (160000000, 50000000))
    for shift, dsn_back, pcb_back in ((600, True, False), (601, False, False), (200, True, True)):
        for bound, expect in ((600, dsn_back), (200, pcb_back)):
            back = track("SIG", (150000000 + shift, 50000000), (160000000, 50000000 - shift))
            restore_copper.restore([back], restore_copper.originals([original]), bound)
            assert (back.points == original.points) == expect, (shift, bound)


def test_vias_come_back_and_need_the_same_size_and_drill():
    at = (89141384, 121858616)  # vna-usb-pd: DSN `89141.4 -121859`
    original, other_drill = via("VBUS", at), via("VBUS", (100000488, 1000000), drill=400000)
    back, back_drill = imported(original, dsn_trip), imported(other_drill, dsn_trip)
    assert back.points == [(89141400, 121859000)]
    back_drill.drill = 300000  # not the original's drill
    routed = track("VBUS", back.points[0], (80000000, 121859000), width=300000)
    counts = restore_copper.restore([back, back_drill, routed], restore_copper.originals([original, other_drill]), 600)
    assert back.points == [at] and routed.points[0] == at
    assert back_drill.points == [(100000000, 1000000)]
    assert counts == {"tracks": 0, "vias": 1, "arcs": 0, "ends": 1, "unmatched": 1}


def test_a_track_in_the_other_direction_gets_the_originals_direction_and_width():
    original = track("SIG", (1000049, 2000049), (5000049, 2000049), width=158750)  # 6.25 mil
    back = track("SIG", (5000000, 2000000), (1000000, 2000000), width=158800)  # session width 0.1 µm
    counts = restore_copper.restore([back], restore_copper.originals([original]), 200)
    assert back.points == original.points and back.width == 158750 and counts["tracks"] == 1


def test_another_width_layer_or_net_is_not_a_match():
    original = track("SIG", (1000000, 2000000), (5000000, 2000000))
    others = [
        track("SIG", (1000100, 2000000), (5000000, 2000000), width=250000),
        track("SIG", (1000100, 2000000), (5000000, 2000000), layer=B_CU),
        track("GND", (1000100, 2000000), (5000000, 2000000)),
    ]
    restore_copper.restore(others, restore_copper.originals([original]), 600)
    assert all(t.points[0] == (1000100, 2000000) for t in others)


def test_duplicates_match_the_nearest_then_in_board_order():
    first = track("SIG", (1000000, 2000000), (5000000, 2000000), name="first")
    second = track("SIG", (1000300, 2000000), (5000000, 2000000), name="second")
    near_second = track("SIG", (1000250, 2000000), (5000000, 2000000))
    near_both = track("SIG", (1000150, 2000000), (5000000, 2000000))
    records = restore_copper.originals([first, second])
    restore_copper.restore([near_both, near_second], records, 600)
    # nearest first: near_second takes `second` (50 nm), so near_both gets `first`
    assert near_second.points[0] == (1000300, 2000000) and near_both.points[0] == (1000000, 2000000)
    tie_a = track("SIG", (1000000, 2000100), (5000000, 2000000))
    tie_b = track("SIG", (1000000, 2000100), (5000000, 2000000))
    same = [track("SIG", (1000000, 2000000), (5000000, 2000000)), track("SIG", (1000000, 2000200), (5000000, 2000000))]
    restore_copper.restore([tie_a, tie_b], restore_copper.originals(same), 600)
    assert tie_a.points[0] == (1000000, 2000000) and tie_b.points[0] == (1000000, 2000200)


def test_locked_tracks_and_arcs_are_left_alone():
    kept = track("SIG", (1000049, 2000000), (5000000, 2000000), locked=True)
    arc = ArcItem("SIG", A_ARC, locked=True)
    assert restore_copper.originals([kept, arc]) == []
    locked_copy = track("SIG", (1000000, 2000000), (5000000, 2000000), locked=True)
    original = track("SIG", (1000049, 2000000), (5000000, 2000000))
    counts = restore_copper.restore([locked_copy], restore_copper.originals([original]), 600)
    assert locked_copy.points[0] == (1000000, 2000000) and counts["unmatched"] == 1


# --- K13.8: track arcs -----------------------------------------------------------

# `data/arc_pair.kicad_pcb`'s arc of net A: radius 1 mm around (100, 100) mm.
A_ARC = ((101000000, 100000000), (100800000, 100600000), (100000000, 101000000))


class ArcItem(Item):
    """A PCB_ARC: start, mid, end."""

    def __init__(self, net, points, width=200000, **kw):
        super().__init__("PCB_ARC", net, points, width, **kw)

    def GetMid(self):
        return Pt(*self.points[1])

    def GetEnd(self):
        return Pt(*self.points[2])

    def GetNet(self):
        return self.net


class Editor:
    """`restore`'s editor on a list of board items."""

    def __init__(self, board):
        self.board, self.added, self.removed = board, [], []

    def add_arc(self, rec, like):
        arc = ArcItem(like.GetNetname(), rec.points, rec.width, layer=rec.layer[0])
        arc.m_Uuid = Kiid(rec.uuid)
        self.board.append(arc)
        self.added.append(arc)
        return arc

    def remove(self, item):
        self.board.remove(item)
        self.removed.append(item)


def polyline(net, points, trip, width=200000):
    """The imported tracks of a polyline that went through `trip`."""
    pts = [(trip(x), trip(y)) for x, y in points]
    return [track(net, a, b, width=width) for a, b in zip(pts, pts[1:])]


def test_an_arc_whose_polyline_comes_back_unmoved_is_put_back_as_the_arc():
    """The DSN path: `track_arcs` lays the arc through the export's `%.6g` ends;
    the session gives the polyline back; the arc returns with its uuid."""
    original = ArcItem("A", A_ARC)
    lead = track("A", (101000000, 97000000), A_ARC[0])
    records = restore_copper.originals([lead, original])
    pieces = polyline("A", track_arcs.arc_points(*A_ARC), dsn_trip)
    board = [imported(lead, dsn_trip)] + pieces
    editor = Editor(board)
    counts = restore_copper.restore(list(board), records, 600, editor)
    assert counts == {"tracks": 1, "vias": 0, "arcs": 1, "ends": 0, "unmatched": 0}
    assert editor.removed == pieces and len(editor.added) == 1
    arc = editor.added[0]
    assert arc.points == list(A_ARC) and arc.width == 200000 and arc.net == "A" and arc.layer == F_CU
    assert arc.m_Uuid.AsString() == original.m_Uuid.AsString()
    assert board == [board[0], arc] and board[0].points == list(lead.points)


def test_the_readers_polyline_comes_back_as_the_arc_on_the_kicad_pcb_path():
    records = restore_copper.originals([ArcItem("A", A_ARC)])
    pieces = polyline("A", track_arcs.arc_points(*A_ARC), reader_trip)
    editor = Editor(list(pieces))
    counts = restore_copper.restore(pieces, records, 200, editor)
    assert counts["arcs"] == 1 and editor.board == editor.added


def test_a_reversed_polyline_split_at_a_junction_is_still_the_arc():
    """The session may run the path end to start, and a routed track that
    joins it splits one chord in two (the new point lies inside the arc)."""
    pts = track_arcs.arc_points(*A_ARC)[::-1]
    a, b = pts[5], pts[6]
    pts.insert(6, ((a[0] + b[0]) // 2, (a[1] + b[1]) // 2))
    pieces = polyline("A", pts, dsn_trip)
    routed = track("A", pieces[5].points[1], (99000000, 99000000))
    board = pieces[::-1] + [routed]
    editor = Editor(board)
    counts = restore_copper.restore(list(board), restore_copper.originals([ArcItem("A", A_ARC)]), 600, editor)
    assert counts["arcs"] == 1 and sorted(map(id, editor.removed)) == sorted(map(id, pieces))
    assert board == [routed, editor.added[0]]


def test_a_chord_or_a_moved_polyline_stays_as_imported():
    records = restore_copper.originals([ArcItem("A", A_ARC)])
    chord = [track("A", A_ARC[0], A_ARC[2])]  # what KiCad's export writes
    moved = track_arcs.arc_points(*A_ARC)
    moved[9] = (moved[9][0] + 3000, moved[9][1])  # one point 3 µm off the arc
    for pieces in (chord, polyline("A", moved, dsn_trip), polyline("A", track_arcs.arc_points(*A_ARC)[:-3], dsn_trip)):
        editor = Editor(list(pieces))
        counts = restore_copper.restore(list(pieces), records, 600, editor)
        assert counts["arcs"] == 0 and counts["unmatched"] == 1 and editor.added == editor.removed == []


def test_another_width_layer_or_net_is_not_the_arc_and_no_editor_puts_back_nothing():
    records = restore_copper.originals([ArcItem("A", A_ARC)])
    pts = track_arcs.arc_points(*A_ARC)
    for pieces in (polyline("A", pts, dsn_trip, width=250000), polyline("B", pts, dsn_trip)):
        assert restore_copper.restore(list(pieces), records, 600, Editor(list(pieces)))["arcs"] == 0
    other_layer = polyline("A", pts, dsn_trip)
    for t in other_layer:
        t.layer = B_CU
    assert restore_copper.restore(list(other_layer), records, 600, Editor(list(other_layer)))["arcs"] == 0
    assert restore_copper.restore(polyline("A", pts, dsn_trip), records, 600)["arcs"] == 0


def test_a_routed_track_that_met_the_arcs_end_follows_it():
    records = restore_copper.originals([ArcItem("A", A_ARC)])
    pieces = polyline("A", track_arcs.arc_points(*A_ARC), dsn_trip)
    routed = track("A", (100000400, 101000000), (97000000, 101000000))  # 0.4 µm off the arc's end
    board = pieces + [routed]
    counts = restore_copper.restore(list(board), records, 600, Editor(board))
    assert counts["arcs"] == 1 and counts["ends"] == 1 and routed.points[0] == A_ARC[2]
    assert restore_copper.log_line(counts).endswith("left as routed (Q22.3), 1 track arc(s) back as arcs (K13.8)")


def test_the_order_of_the_imported_items_does_not_change_the_result():
    originals = [track("N%d" % (i % 3), (1000000 + i * 1000, 2000049), (9000049, 2000049 + i)) for i in range(40)]
    first = [imported(t, dsn_trip) for t in originals]
    second = [t.copy() for t in reversed(first)]
    records = restore_copper.originals(originals)
    restore_copper.restore(first, records, 600)
    restore_copper.restore(second, records, 600)
    assert sorted(t.points for t in first) == sorted(t.points for t in second) == sorted(t.points for t in originals)


# The plugin's Route flow, with the stub pcbnew: the import replaces the
# detached tracks by their round-tripped copies and a routed track.


@pytest.fixture
def route_env(tmp_path, monkeypatch, make_fake_router, isolated_env):
    plugin, ui = stub_ui.install(monkeypatch)
    pcb = tmp_path / "board dir" / "b.kicad_pcb"
    pcb.parent.mkdir()
    pcb.write_text("", encoding="utf-8")

    def make_dialog(tracks, session, board_input="dsn"):
        def import_ses(board, path):
            ui.imports.append(path)
            for t in [t for t in board.tracks if not t.IsLocked()]:  # none: the plugin detached them
                board.tracks.remove(t)
            board.tracks.extend(session)
            return True

        monkeypatch.setattr(plugin.pcbnew, "ImportSpecctraSES", import_ses)
        router = make_fake_router("route")
        monkeypatch.setattr(plugin.tools, "find_tool", lambda name: plugin.tools.Tool(name, router))
        monkeypatch.setattr(plugin.tools, "query_version", lambda tool, timeout=10: (0, 1, 0, "abc"))
        dialog = plugin.Dialog(None, stub_ui.FakeBoard(pcb, tracks=tracks))
        if board_input == "kicad_pcb":
            dialog.board_input.SetSelection(1)  # the keepouts stay on (K13.7)
        return dialog

    return make_dialog, ui, pcb.parent / "freerouting-rs"


def test_route_puts_the_input_copper_back_after_the_import(route_env):
    make_dialog, ui, work = route_env
    original = track("PCIe_RX_N", *DIFF_N)
    input_via = via("PCIe_RX_N", DIFF_N[1])
    back, back_via = imported(original, dsn_trip), imported(input_via, dsn_trip)
    routed = track("PCIe_RX_N", back.points[0], (190000000, 73575000))
    dialog = make_dialog([original, input_via], [back, back_via, routed])
    dialog.on_route(None)
    assert len(ui.imports) == 1 and ui.messages[-1][0].startswith("Routed and imported")
    assert dialog.board.tracks == [back, back_via, routed]
    assert back.points == list(DIFF_N) and back_via.points == [DIFF_N[1]]
    assert routed.points[0] == DIFF_N[0]
    log = (work / "plugin.log").read_text(encoding="utf-8")
    assert "input copper: 1 track(s) and 1 via(s) back at their own coordinates, 1 routed end(s) moved" in log


def test_a_fully_routed_board_on_the_kicad_pcb_path_comes_back_unchanged(route_env):
    """K13.2: nothing left to route; the session gives back every input track
    and via through the reader's and the session's 0.1 µm."""
    make_dialog, ui, work = route_env
    board = [
        track("SYS", (63647050, 175333656), (70000049, 175333656), width=250000, layer=4),
        track("SYS", (70000049, 175333656), (80000000, 180000051), width=250000, layer=4),
        via("SYS", (80000000, 180000051)),
        track("VBAT", (63647050, 175583706), (70000000, 175583706), width=250000, layer=4),
    ]
    geometry = [(t.cls, t.net, t.layer, t.width, list(t.points), t.m_Uuid.AsString()) for t in board]
    session = [imported(t, reader_trip) for t in board]
    assert any(s.points != t.points for s, t in zip(session, board))
    dialog = make_dialog(board, session, board_input="kicad_pcb")
    dialog.on_route(None)
    assert len(ui.snapshots) == 1 and ui.exports == []  # the kicad_pcb path
    assert [(t.cls, t.net, t.layer, t.width, t.points, t.m_Uuid.AsString()) for t in dialog.board.tracks] == geometry
    assert "input copper: 3 track(s) and 1 via(s) back at their own coordinates" in (work / "plugin.log").read_text(encoding="utf-8")


def test_fix_existing_keeps_the_input_copper_and_restores_nothing(route_env):
    make_dialog, ui, work = route_env
    original = track("SIG", (1000049, 2000000), (5000000, 2000000))
    routed = track("SIG", (1000000, 2000000), (1000000, 9000000))
    dialog = make_dialog([original], [routed])
    dialog.fix_existing.SetValue(True)
    dialog.on_route(None)
    assert dialog.board.tracks == [original, routed] and not original.IsLocked()
    assert original.points[0] == (1000049, 2000000) and routed.points[0] == (1000000, 2000000)
    assert "input copper:" not in (work / "plugin.log").read_text(encoding="utf-8")


def test_a_routed_stub_that_would_shrink_to_nothing_stays_as_imported():
    """cm4-edge-ai: the input track ends at a pad centre, 179.2875 mm, which the
    DSN writes as 179288; the router joins the pad centre to that end with a
    0.5 µm stub. Following both ends would leave a zero-length track (KiCad's
    `tracks_crossing`)."""
    original = track("SS", (179520000, 85195000), (179287500, 84962500))
    back = track("SS", (179520000, 85195000), (179288000, 84962500))
    stub = track("SS", (179287500, 84962500), (179288000, 84962500))
    counts = restore_copper.restore([back, stub], restore_copper.originals([original]), 600)
    assert back.points == original.points
    assert stub.points == [(179287500, 84962500), (179288000, 84962500)] and counts["ends"] == 0

"""Q22.2 unit tests (plain python3, no pcbnew): the grader counts what routing caused.

    python3 -m unittest discover -s tools/kicad-oracle/tests -p 'test_*.py'

The boards are small .kicad_pcb texts written in KiCad 10's layout (top-level items after one
tab), the DRC reports are dicts in the shape of `kicad-cli pcb drc --format json`.
"""

import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import oracle_lib as lib  # noqa: E402


def segment(uuid, x1, y1, x2, y2, width=0.2, layer="F.Cu", net="A"):
    return (
        '\t(segment\n\t\t(start %s %s)\n\t\t(end %s %s)\n\t\t(width %s)\n\t\t(layer "%s")\n'
        '\t\t(net "%s")\n\t\t(uuid "%s")\n\t)\n' % (x1, y1, x2, y2, width, layer, net, uuid)
    )


def arc(uuid, start, mid, end, width=0.2, layer="F.Cu", net="A"):
    return (
        '\t(arc\n\t\t(start %s %s)\n\t\t(mid %s %s)\n\t\t(end %s %s)\n\t\t(width %s)\n\t\t(layer "%s")\n'
        '\t\t(net "%s")\n\t\t(uuid "%s")\n\t)\n' % (*start, *mid, *end, width, layer, net, uuid)
    )


def via(uuid, x, y, size=0.6, drill=0.3, net="A"):
    return (
        '\t(via\n\t\t(at %s %s)\n\t\t(size %s)\n\t\t(drill %s)\n\t\t(layers "F.Cu" "B.Cu")\n'
        '\t\t(net "%s")\n\t\t(uuid "%s")\n\t)\n' % (x, y, size, drill, net, uuid)
    )


def board(*items):
    # A footprint with its own (segment ...)-free graphics sits between the tracks, as in KiCad's files.
    head = '(kicad_pcb\n\t(version 20250114)\n\t(footprint "T:P"\n\t\t(fp_line (start 0 0) (end 1 1) (layer "F.SilkS"))\n\t)\n'
    return head + "".join(items) + ")\n"


def item(description, uuid, x=0.0, y=0.0):
    return {"description": description, "pos": {"x": x, "y": y}, "uuid": uuid}


PAD = item("Pad 1 [A] of U1", "pad-1", 10.0, 10.0)
OTHER_PAD = item("Pad 2 [B] of U2", "pad-2", 40.0, 10.0)


def track(uuid, net="A", x=0.0, y=0.0):
    return item("Track [%s] on F.Cu, length 3.0000 mm" % net, uuid, x, y)


def violation(kind, *items, description="Clearance violation"):
    return {"type": kind, "description": description, "severity": "error", "items": list(items)}


INPUT = board(
    segment("t-in", 10, 10, 13, 10),
    segment("t-diag", 10.1234567, 20, 13, 21.5),
    arc("a-in", (1, 1), (2, 1.5), (3, 1)),
    via("v-in", 15, 15),
)


class ReadCopperTest(unittest.TestCase):
    def test_reads_segments_arcs_and_vias_in_nanometres(self):
        copper = lib.read_copper(INPUT)
        self.assertEqual(set(copper), {"t-in", "t-diag", "a-in", "v-in"})
        self.assertEqual(copper["t-in"]["start"], (10_000_000, 10_000_000))
        self.assertEqual(copper["t-in"]["width"], 200_000)
        self.assertEqual(copper["t-in"]["layer"], "F.Cu")
        self.assertEqual(copper["t-in"]["net"], "A")
        self.assertEqual(copper["t-diag"]["start"], (10_123_457, 20_000_000))  # 0.1 nm rounds off
        self.assertEqual(copper["a-in"]["mid"], (2_000_000, 1_500_000))
        self.assertEqual(copper["v-in"]["at"], (15_000_000, 15_000_000))
        self.assertEqual((copper["v-in"]["size"], copper["v-in"]["drill"]), (600_000, 300_000))
        self.assertEqual(copper["v-in"]["layers"], ("F.Cu", "B.Cu"))

    def test_key_ignores_the_order_of_the_ends(self):
        a = lib.read_copper(board(segment("x", 1, 2, 3, 4)))["x"]
        b = lib.read_copper(board(segment("y", 3, 4, 1, 2)))["y"]
        self.assertEqual(lib.copper_key(a), lib.copper_key(b))

    def test_key_tells_nanometres_width_layer_net_and_drill_apart(self):
        base = lib.copper_key(lib.read_copper(board(segment("x", 1, 2, 3, 4)))["x"])
        for other in (
            segment("x", 1.000001, 2, 3, 4),
            segment("x", 1, 2, 3, 4, width=0.2001),
            segment("x", 1, 2, 3, 4, layer="B.Cu"),
            segment("x", 1, 2, 3, 4, net="B"),
        ):
            self.assertNotEqual(base, lib.copper_key(lib.read_copper(board(other))["x"]), other)
        v = lib.copper_key(lib.read_copper(board(via("v", 1, 1)))["v"])
        self.assertNotEqual(v, lib.copper_key(lib.read_copper(board(via("v", 1, 1, drill=0.4)))["v"]))


class RoutingCausedTest(unittest.TestCase):
    def split(self, violations, routed_items, input_text=INPUT):
        routed = lib.read_copper(board(*routed_items))
        return lib.split_routing_caused(violations, routed, lib.read_copper(input_text))

    def test_an_input_pair_listed_in_the_other_order_is_pre_existing(self):
        # The input report lists (pad, track), the routed report (track, pad). The pre-Q22 matcher
        # compared the first item's position; picodebugger's pair, 13.1 mm apart, was counted twice.
        routed_items = [segment("t-in", 10, 10, 13, 10)]
        routed_report = violation("clearance", track("t-in", x=11.5, y=10.0), PAD)
        real, pre = self.split([routed_report], routed_items)
        self.assertEqual((real, pre), ([], [routed_report]))
        far = item("Pad 1 [A] of U1", "pad-1", 24.6, 10.0)  # 13.1 mm from the track item
        routed_far = violation("clearance", track("t-in", x=11.5, y=10.0), far)
        input_far = violation("clearance", far, track("t-in", x=11.5, y=10.0))
        old_real, _ = lib.subtract_preexisting([routed_far], [input_far])
        self.assertEqual(old_real, [routed_far], "the order-dependent matcher counted it")
        self.assertEqual(self.split([routed_far], routed_items), ([], [routed_far]))

    def test_an_input_pair_the_input_report_left_out_is_pre_existing(self):
        # KiCad stops a type at its cap, so the input report need not list every pair.
        routed_items = [segment("t-in", 10, 10, 13, 10), via("v-in", 15, 15)]
        pairs = [
            violation("clearance", track("t-in"), PAD),
            violation("hole_clearance", item("Via [A] on F.Cu - B.Cu", "v-in", 15, 15), OTHER_PAD),
            violation("shorting_items", PAD, OTHER_PAD),
        ]
        real, pre = self.split(pairs, routed_items)
        self.assertEqual((real, pre), ([], pairs))

    def test_a_moved_input_track_is_routing_caused(self):
        moved = violation("clearance", track("t-moved"), PAD)
        for end in (13.000001, 12.999999):  # one nanometre either way
            routed_items = [segment("t-moved", 10, 10, end, 10)]
            real, pre = self.split([moved], routed_items)
            self.assertEqual((real, pre), ([moved], []), end)

    def test_a_moved_input_via_or_one_with_another_drill_is_routing_caused(self):
        v = violation("annular_width", item("Via [A] on F.Cu - B.Cu", "v-new", 15, 15))
        for routed in (via("v-new", 15.0005, 15), via("v-new", 15, 15, drill=0.4)):
            self.assertEqual(self.split([v], [routed]), ([v], []))
        self.assertEqual(self.split([v], [via("v-new", 15, 15)]), ([], [v]))

    def test_a_routed_track_is_routing_caused(self):
        v = violation("clearance", track("t-new"), OTHER_PAD)
        real, pre = self.split([v], [segment("t-new", 30, 10, 38, 10)])
        self.assertEqual((real, pre), ([v], []))

    def test_a_routed_track_over_a_footprint_graphic_is_routing_caused_even_with_an_input_twin(self):
        # pcietest: J8's two pads numbered 1 sit at the track's two ends, one on the graphic. The
        # input report holds (pad at the track's start, graphic) with the track's net; the old
        # matcher called the routed (track, graphic) its twin.
        graphic = item("Segment [<no net>] of J8 on B.Cu", "fp-line", 145.72, 133.52)
        pad = item("PTH pad 1 [A] of J8", "pad-j8", 145.72, 130.48)
        routed_report = violation("clearance", track("t-new", x=145.72, y=130.48), graphic)
        input_report = violation("clearance", pad, graphic)
        old_real, _ = lib.subtract_preexisting([routed_report], [input_report])
        self.assertEqual(old_real, [], "the old matcher hid it")
        real, _ = self.split([routed_report], [segment("t-new", 145.72, 130.48, 145.72, 133.52)])
        self.assertEqual(real, [routed_report])

    def test_an_arc_matches_like_a_segment(self):
        v = violation("clearance", item("Track [A] on F.Cu", "a-in", 2, 1.5), PAD)
        same = arc("a-in", (3, 1), (2, 1.5), (1, 1))
        self.assertEqual(self.split([v], [same])[1], [v])
        flipped_mid = arc("a-in", (1, 1), (2, 1.4), (3, 1))
        self.assertEqual(self.split([v], [flipped_mid])[0], [v])

    def test_a_track_the_routed_board_cannot_identify_is_routing_caused(self):
        unknown = violation("clearance", track("not-in-the-board"), PAD)
        self.assertEqual(self.split([unknown], [])[0], [unknown])
        pads_only = violation("clearance", PAD, OTHER_PAD)
        self.assertEqual(self.split([pads_only], [])[1], [pads_only])

    def test_copper_unconnected_to_a_net_is_matched_by_the_net_name_as_written(self):
        text = board(segment("t-in", 10, 10, 13, 10, net="unconnected-(J8-Pin_1-Pad1)"))
        routed = [segment("t-in", 10, 10, 13, 10, net="unconnected-(J8-Pin_1-Pad1)")]
        v = violation("clearance", track("t-in", net="unconnected-(J8-Pin_1-Pad1)"), PAD)
        self.assertEqual(self.split([v], routed, text)[1], [v])


class NumberedNetsTest(unittest.TestCase):
    """Q22.13: KiCad 6-9 files name a track's net by number, KiCad 10 files by name."""

    TABLE = '\t(net 0 "")\n\t(net 1 "A")\n\t(net 12 "/Solar_module/SYS")\n'

    @staticmethod
    def numbered(text, name, number):
        return text.replace('(net "%s")' % name, "(net %d)" % number)

    def old_board(self, *items, table=None):
        table = self.TABLE if table is None else table
        head = '(kicad_pcb\n\t(version 20241229)\n' + table
        return head + "".join(items) + ")\n"

    def test_a_segment_written_both_ways_has_one_key(self):
        old = self.old_board(self.numbered(segment("s", 1, 2, 3, 4, net="/Solar_module/SYS"), "/Solar_module/SYS", 12))
        new = board(segment("s", 1, 2, 3, 4, net="/Solar_module/SYS"))
        old_record = lib.read_copper(old)["s"]
        self.assertEqual(old_record["net"], "/Solar_module/SYS")
        self.assertEqual(lib.copper_key(old_record), lib.copper_key(lib.read_copper(new)["s"]))

    def test_an_arc_and_a_via_written_both_ways_have_one_key(self):
        old = self.old_board(
            self.numbered(arc("a", (1, 1), (2, 1.5), (3, 1), net="A"), "A", 1),
            self.numbered(via("v", 15, 15, net="A"), "A", 1),
        )
        new = board(arc("a", (1, 1), (2, 1.5), (3, 1)), via("v", 15, 15))
        old_copper, new_copper = lib.read_copper(old), lib.read_copper(new)
        for uuid in ("a", "v"):
            self.assertEqual(lib.copper_key(old_copper[uuid]), lib.copper_key(new_copper[uuid]), uuid)

    def test_net_zero_is_the_empty_name(self):
        old = self.old_board(self.numbered(segment("s", 1, 2, 3, 4, net="X"), "X", 0))
        self.assertEqual(lib.read_copper(old)["s"]["net"], "")

    def test_a_quoted_name_that_looks_like_a_number_stays_a_name(self):
        text = board(segment("s", 1, 2, 3, 4, net="12"))
        self.assertEqual(lib.read_copper(text)["s"]["net"], "12")

    def test_numbers_without_a_net_table_are_an_error(self):
        old = self.old_board(self.numbered(segment("s", 1, 2, 3, 4, net="A"), "A", 1), table="")
        with self.assertRaises(ValueError) as caught:
            lib.read_copper(old)
        self.assertIn("net table", str(caught.exception))

    def test_a_number_missing_from_the_table_is_an_error(self):
        old = self.old_board(self.numbered(segment("s", 1, 2, 3, 4, net="A"), "A", 99))
        with self.assertRaises(ValueError):
            lib.read_copper(old)

    def test_a_footprint_pad_net_is_not_taken_for_the_net_table(self):
        pad = '\t(footprint "T:P"\n\t\t(pad "1" smd rect (at 0 0) (net 5 "P"))\n\t)\n'
        old = self.old_board(pad, self.numbered(segment("s", 1, 2, 3, 4, net="A"), "A", 1))
        self.assertEqual(lib.read_copper(old)["s"]["net"], "A")

    def test_an_old_input_track_is_pre_existing_next_to_a_new_routed_board(self):
        input_old = self.old_board(self.numbered(segment("in", 10, 10, 13, 10, net="A"), "A", 1))
        routed = board(segment("in", 10, 10, 13, 10), segment("new", 30, 10, 38, 10))
        violations = [
            violation("clearance", track("in"), PAD),
            violation("clearance", track("new"), OTHER_PAD),
        ]
        real, pre = lib.split_routing_caused(violations, lib.read_copper(routed), lib.read_copper(input_old))
        self.assertEqual([v["items"][0]["uuid"] for v in real], ["new"])
        self.assertEqual(len(pre), 1)


class FillTypesTest(unittest.TestCase):
    """A starved thermal or isolated copper item names only a zone (and a pad), so it carries no
    track to tell input from routing; the fill depends on what routing cut. These two types keep
    the report-to-report match (a documented exception to the copper rule)."""

    def test_a_zone_only_item_is_pre_existing_when_the_input_report_has_it(self):
        zone = item("Zone [GND] on F.Cu", "zone-1", 5, 5)
        v = violation("starved_thermal", zone)
        real, pre = lib.split_routing_caused([v], {}, {}, input_violations=[v])
        self.assertEqual((real, pre), ([], [v]))

    def test_a_zone_only_item_the_input_report_lacks_is_counted(self):
        zone = item("Zone [GND] on F.Cu", "zone-1", 5, 5)
        v = violation("starved_thermal", zone)
        real, pre = lib.split_routing_caused([v], {}, {}, input_violations=[])
        self.assertEqual((real, pre), ([v], []))

    def test_other_types_ignore_the_input_report_for_zone_only_items(self):
        zone = item("Zone [GND] on F.Cu", "zone-1", 5, 5)
        v = violation("clearance", zone, PAD)
        real, pre = lib.split_routing_caused([v], {}, {}, input_violations=[])
        self.assertEqual((real, pre), ([], [v]))


class SummaryTest(unittest.TestCase):
    def test_summarize_uses_the_copper_when_given(self):
        routed_copper = lib.read_copper(board(segment("t-in", 10, 10, 13, 10), segment("t-new", 30, 10, 38, 10)))
        input_copper = lib.read_copper(INPUT)
        kept = violation("clearance", track("t-in"), PAD)
        new = violation("clearance", track("t-new"), OTHER_PAD)
        routed = {"violations": [kept, new], "unconnected_items": []}
        unrouted = {"violations": [], "unconnected_items": []}
        summary, code = lib.summarize(routed, unrouted, {}, {}, routed_copper, input_copper)
        self.assertEqual(summary["drc_real"], 1)
        self.assertEqual(summary["drc_real_by_type"], {"clearance": 1})
        self.assertEqual(summary["preexisting"], 1)
        self.assertEqual(summary["routing_caused_basis"], "copper")
        self.assertEqual(code, lib.EXIT_ROUTER_ITEMS)

    def test_summarize_without_copper_keeps_the_report_match(self):
        v = violation("clearance", track("t"), PAD)
        summary, code = lib.summarize({"violations": [v]}, {"violations": [v]}, {}, {})
        self.assertEqual((summary["drc_real"], summary["preexisting"]), (0, 1))
        self.assertEqual(summary["routing_caused_basis"], "report")
        self.assertEqual(code, lib.EXIT_CLEAN)

    def test_summarize_command_reads_the_two_boards(self):
        with tempfile.TemporaryDirectory() as tmp:
            def write(name, content):
                path = os.path.join(tmp, name)
                with open(path, "w", encoding="utf-8") as f:
                    f.write(content if isinstance(content, str) else json.dumps(content))
                return path

            routed = write("routed.json", {"violations": [violation("clearance", track("t-in"), PAD)]})
            unrouted = write("unrouted.json", {"violations": []})
            copper = write("copper.json", {})
            meta = write("meta.json", {})
            routed_pcb = write("routed.kicad_pcb", board(segment("t-in", 10, 10, 13, 10)))
            input_pcb = write("input.kicad_pcb", INPUT)
            out = os.path.join(tmp, "summary.json")
            code = lib.main(["summarize", routed, unrouted, copper, meta, out, routed_pcb, input_pcb])
            self.assertEqual(code, lib.EXIT_CLEAN)
            with open(out, encoding="utf-8") as f:
                self.assertEqual(json.load(f)["preexisting"], 1)
            # An unreadable board falls back to the report match, flagged in the summary.
            code = lib.main(["summarize", routed, unrouted, copper, meta, out, routed_pcb, os.path.join(tmp, "missing")])
            with open(out, encoding="utf-8") as f:
                self.assertEqual(json.load(f)["routing_caused_basis"], "report")
            self.assertEqual(code, lib.EXIT_ROUTER_ITEMS)


if __name__ == "__main__":
    unittest.main()

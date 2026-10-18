"""Unit tests of oracle_lib (plain python3, no pcbnew):

    python3 -m unittest discover -s tools/kicad-oracle/tests -p 'test_*.py'
"""

import json
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import oracle_lib as lib  # noqa: E402

DSN = """(pcb "x"
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal))
    (keepout "" (polygon B.Cu 0  136500 -53000  162500 -53000  162500 -79000  136500 -79000
            136500 -53000))
    (keepout "real" (polygon B.Cu 0  0 0  10 0  10 10  0 0))
    (via "Via[0-1]_800:400_um")
    (rule (width 250) (clearance 200))
  )
  (library
    (image A (keepout "" (polygon B.Cu 0  136500 -53000  162500 -53000  162500 -79000  136500 -79000)))
  )
  (network
    (class kicad_default "" GND "+5V"
      (circuit (use_via Via[0-1]_800:400_um))
      (rule (width 250) (clearance 200))
    )
    (class Power "+12V_BUS" VBAT
      (rule (width 1500) (clearance 200))
    )
  )
)
"""

FAN_INLET = {
    "layers": ["B.Cu"],
    "outline": [[136500.0, -53000.0], [162500.0, -53000.0], [162500.0, -79000.0], [136500.0, -79000.0]],
    "tracks": False,
    "vias": False,
}


def violation(kind, nets, x, y):
    return {
        "type": kind,
        "items": [{"description": "Track [%s] on F.Cu" % n, "pos": {"x": x, "y": y}} for n in nets],
    }


class OracleLibTest(unittest.TestCase):
    def test_removes_only_the_open_rule_area_keepout(self):
        text, removed = lib.remove_keepouts_of_open_rule_areas(DSN, [FAN_INLET])
        self.assertEqual(removed, 1)
        self.assertNotIn("136500 -53000  162500", text.split("(library")[0])
        self.assertIn('(keepout "real"', text)
        # The footprint image keeps its keepout.
        self.assertIn("(image A (keepout", text)
        banned = dict(FAN_INLET, tracks=True)
        self.assertEqual(lib.remove_keepouts_of_open_rule_areas(DSN, [banned]), (DSN, 0))
        other_layer = dict(FAN_INLET, layers=["F.Cu"])
        self.assertEqual(lib.remove_keepouts_of_open_rule_areas(DSN, [other_layer])[1], 0)

    def test_class_widths_in_mm(self):
        widths = lib.class_widths(DSN)
        self.assertEqual(widths["GND"], 0.25)
        self.assertEqual(widths["+5V"], 0.25)
        self.assertEqual(widths["+12V_BUS"], 1.5)
        self.assertEqual(widths["VBAT"], 1.5)

    def test_strip_placement(self):
        ses = '(session x (placement (resolution um 10) (component "a(b)" (place U1 1 2 front 0))) (routes (network_out)))'
        self.assertEqual(lib.strip_placement(ses), "(session x  (routes (network_out)))")
        self.assertEqual(lib.strip_placement("(session x)"), "(session x)")

    def test_forces_copper_classes_to_error(self):
        pro = json.dumps({"board": {"design_settings": {"rule_severities": {"clearance": "warning", "silk_overlap": "ignore"}}}})
        severities = json.loads(lib.force_copper_errors(pro))["board"]["design_settings"]["rule_severities"]
        for name in lib.FORCED_ERROR_CLASSES:
            self.assertEqual(severities[name], "error")
        self.assertEqual(severities["silk_overlap"], "ignore")
        self.assertIn("clearance", json.loads(lib.force_copper_errors("{}"))["board"]["design_settings"]["rule_severities"])

    def test_subtracts_preexisting_by_type_nets_and_distance(self):
        unrouted = [
            violation("copper_edge_clearance", ["N1"], 1.0, 1.0),
            violation("clearance", ["A", "B"], 5.0, 5.0),
        ]
        routed = [
            violation("copper_edge_clearance", ["N1"], 1.3, 1.0),  # within 0.5 mm
            violation("clearance", ["A", "B"], 9.0, 9.0),  # same pair, far away
            violation("clearance", ["A", "C"], 5.0, 5.0),  # other pair
            violation("copper_edge_clearance", ["N1"], 1.0, 1.1),  # input item already used
        ]
        real, pre = lib.subtract_preexisting(routed, unrouted)
        self.assertEqual(len(pre), 1)
        self.assertEqual(len(real), 3)
        self.assertEqual(lib.item_nets(routed[2]), ("A", "C"))

    def test_summary_and_exit_codes(self):
        unrouted = {"violations": [violation("copper_edge_clearance", ["N1"], 1, 1)], "unconnected_items": [{}]}
        summary, code = lib.summarize(unrouted, unrouted, {"below_minimum": {"track_width": 0}}, {"kicad_version": "10.0.7"})
        self.assertEqual((summary["preexisting"], summary["drc_real"], code), (1, 0, lib.EXIT_CLEAN))
        self.assertEqual(summary["unconnected_items"], 1)
        self.assertEqual(summary["kicad_version"], "10.0.7")
        routed = {"violations": unrouted["violations"] + [violation("clearance", ["A", "B"], 3, 3)]}
        summary, code = lib.summarize(routed, unrouted, {}, {})
        self.assertEqual((summary["drc_real"], summary["drc_real_by_type"], code), (1, {"clearance": 1}, lib.EXIT_ROUTER_ITEMS))
        _, code = lib.summarize(unrouted, unrouted, {"below_minimum": {"track_width": 3}}, {})
        self.assertEqual(code, lib.EXIT_ROUTER_ITEMS)
        self.assertEqual(lib.not_graded("no kicad-cli")["exit_code"], lib.EXIT_NOT_GRADED)

    def test_tidiness_is_added_to_the_summary_without_changing_the_verdict(self):
        tidy = {
            "format": "freerouting-tidy-1",
            "connections": 3,
            "skipped": 1,
            "nets": 2,
            "measures": {"detour": {"median": 1.1, "p90": 2.0}},
            "connection_list": [{"net": "x"}],
        }
        summary, code = lib.summarize({}, {}, {}, {})
        added = lib.add_tidiness(summary, tidy)
        self.assertEqual(added["tidiness"]["measures"]["detour"]["p90"], 2.0)
        self.assertEqual(added["tidiness"]["connections"], 3)
        self.assertNotIn("connection_list", added["tidiness"])
        self.assertNotIn("tidiness", summary)
        self.assertEqual((added["drc_real"], added["exit_code"]), (summary["drc_real"], code))

    def test_add_tidiness_command_edits_the_file_and_ignores_a_missing_score(self):
        import tempfile

        with tempfile.TemporaryDirectory() as d:
            summary_path = os.path.join(d, "summary.json")
            tidy_path = os.path.join(d, "tidy.json")
            with open(summary_path, "w") as f:
                json.dump({"graded": True}, f)
            self.assertEqual(lib.main(["add-tidiness", summary_path, tidy_path]), 0)
            self.assertEqual(json.load(open(summary_path)), {"graded": True})
            with open(tidy_path, "w") as f:
                json.dump({"connections": 2, "measures": {}}, f)
            self.assertEqual(lib.main(["add-tidiness", summary_path, tidy_path]), 0)
            self.assertEqual(json.load(open(summary_path))["tidiness"]["connections"], 2)

    def test_capped_counts_are_flagged(self):
        self.assertEqual(lib.capped_types({"track_width": 199, "clearance": 424}), ["track_width"])


if __name__ == "__main__":
    unittest.main()

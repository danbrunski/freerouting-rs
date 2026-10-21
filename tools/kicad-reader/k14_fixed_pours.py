"""K14.4: record the fixture's export with the plugin's `fixed_pour_classes`
fix-up (`wires` mode), without pcbnew.

    python3 -I tools/kicad-reader/k14_fixed_pours.py [DATA_DIR]

Run from the repository root with any Python 3. DATA_DIR defaults to
`crates/freerouting-kicad/tests/data`. Reads the committed
`k14-fixups.kicad10.dsn` (KiCad's export) and `k14-fixups.board.json` (the
plugin's board facts, fills included), both written by `k14_fixture.py`, so
the board is not rebuilt (a rebuild gives new UUIDs). Writes, there:

- `k14-fixups.fixed-pours.dsn`: `dsn_fixups.apply` with `fixed_pour_classes`
  on top of the three fix-ups of `k14-fixups.fixed.dsn` (`SETTINGS`);
- `k14-fixups.fixed-pours-only.dsn`: `dsn_fixups.apply` with
  `fixed_pour_classes` alone;
- `k14-fixups.fixed-pours-default.dsn`: `fixed_pour_classes` alone with
  `kicad_default` listed too, so GND's pours on F.Cu and B.Cu become wires
  with VBUS's (three pours, two layers, in the plugin's zone order).

`fixed_pour_classes` is `auto_fixed_pour_classes` of the board facts, which
must be `["Power"]` (the VBUS pour, priority 1, over GND's on F.Cu); the
script stops otherwise. It also checks that `auto` gives the same list from
the board's classes (`board["classes"]`, `export_flow.board_classes`) as from
the DSN's, which is what the board snapshot resolves it from (K14.4).

Never imports the freerouting_rs plugin package (its `register()` raises a
wxWidgets dialog outside the editor): `dsn_fixups.py` and the module it needs
are loaded by path.
"""

import importlib.util
import json
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PLUGIN = os.path.join(REPO, "kicad-plugin", "freerouting_rs")
STEM = "k14-fixups"
FIXED_POUR_CLASSES = ["Power"]
SETTINGS = {
    "class_widths": {"Power": 0.6, "Switch": 0.8},
    "drop_pour_nets": ["GND"],
    "route_on_plane_layers": ["In2.Cu"],
}


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(PLUGIN, name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def main(data):
    load("rule_areas")  # `dsn_fixups` imports it as a top-level module outside the package
    dsn_fixups = load("dsn_fixups")
    base = os.path.join(data, STEM)
    with open(base + ".kicad10.dsn", encoding="utf-8", newline="") as f:
        export = f.read()
    with open(base + ".board.json", encoding="utf-8") as f:
        facts = json.load(f)
    dsn = dsn_fixups.Dsn(export)
    auto = dsn_fixups.auto_fixed_pour_classes(facts, dsn)
    if auto != FIXED_POUR_CLASSES:
        raise SystemExit(f"auto_fixed_pour_classes gives {auto}, not {FIXED_POUR_CLASSES}")
    on_board = dsn_fixups.resolve_on_board({"fixed_pour_classes": "auto"}, facts)["fixed_pour_classes"]
    if on_board != auto:
        raise SystemExit(f"auto from the board's classes gives {on_board}, from the DSN's {auto}")
    runs = [
        (".fixed-pours.dsn", {**SETTINGS, "fixed_pour_classes": FIXED_POUR_CLASSES}),
        (".fixed-pours-only.dsn", {"fixed_pour_classes": FIXED_POUR_CLASSES}),
        (".fixed-pours-default.dsn", {"fixed_pour_classes": ["kicad_default"] + FIXED_POUR_CLASSES}),
    ]
    for suffix, settings in runs:
        fixed, log = dsn_fixups.apply(export, facts, settings)
        with open(base + suffix, "w", encoding="utf-8", newline="") as f:
            f.write(fixed)
        print(STEM + suffix)
        for line in log:
            print("  " + line)
    return 0


if __name__ == "__main__":
    default = os.path.join(REPO, "crates", "freerouting-kicad", "tests", "data")
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else default))

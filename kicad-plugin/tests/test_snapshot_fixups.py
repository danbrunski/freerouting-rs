"""K14.1-K14.4: `class_widths`, `drop_pour_nets`, `route_on_plane_layers` and
`fixed_pour_classes` (`wires` mode) on the board snapshot, as
`router.input_fixups.*` flags the router's reader applies; `fixed_pour_mode =
engine` with `auto` resolved from the board.

The fixture is `crates/freerouting-kicad/tests/data/k14-fixups.*`, made with
KiCad 10.0.7's pcbnew by `tools/kicad-reader/k14_fixture.py`: the board, its
DSN export, the plugin's board facts (`kicad_board.collect` plus
`export_flow.board_classes`, read through pcbnew) and the export after
`dsn_fixups.apply`; `tools/kicad-reader/k14_fixed_pours.py` adds the export
with `fixed_pour_classes` on (K14.4, no pcbnew). `crates/freerouting-kicad/tests/k14_fixups.rs`
checks that the reader's tree with these flags equals those fixed-up exports.
"""

import json
import shutil
import subprocess
import types
from dataclasses import replace

import dsn_fixups
import export_flow
import options
import pytest
import snapshot
from conftest import REPO
from options import RouteOptions
from test_cli_integration import find_router
from test_snapshot_headless import NOISE, pcbnew_python

DATA = REPO / "crates" / "freerouting-kicad" / "tests" / "data"
STEM = "k14-fixups"
# the values the generator applies, which are also what `auto` gives on the fixture
SETTINGS = {"class_widths": {"Power": 0.6, "Switch": 0.8}, "drop_pour_nets": ["GND"], "route_on_plane_layers": ["In2.Cu"]}
FLAGS = [
    "--router.input_fixups.class_widths=Power=600,Switch=800",
    "--router.input_fixups.route_on_plane_layers=In2.Cu",
    "--router.input_fixups.drop_pour_nets=GND",
]


def read(suffix):
    with open(DATA / (STEM + suffix), encoding="utf-8", newline="") as f:
        return f.read()


def facts():
    return json.loads(read(".board.json"))


def test_the_recorded_fixed_up_export_is_the_dsn_fixups_of_the_recorded_export():
    fixed, log = dsn_fixups.apply(read(".kicad10.dsn"), facts(), SETTINGS)
    assert fixed == read(".fixed.dsn")
    assert "layer In2.Cu: power -> signal" in log


def test_the_recorded_fixed_pour_exports_are_the_dsn_fixups_of_the_recorded_export():
    """K14.4: `k14_fixed_pours.py`'s three files, as it makes them."""
    export = read(".kicad10.dsn")
    for suffix, settings in (
        (".fixed-pours.dsn", {**SETTINGS, "fixed_pour_classes": ["Power"]}),
        (".fixed-pours-only.dsn", {"fixed_pour_classes": ["Power"]}),
        (".fixed-pours-default.dsn", {"fixed_pour_classes": ["kicad_default", "Power"]}),
    ):
        fixed, _ = dsn_fixups.apply(export, facts(), settings)
        assert fixed == read(suffix), suffix
    _, log = dsn_fixups.apply(export, facts(), {"fixed_pour_classes": ["Power"]})
    assert log == ["pours: 44 fixed wires for 1 pour(s) of Power; 1 outer plane(s) removed (dropped nets: none)"]


def test_auto_on_the_snapshot_gives_the_dsn_paths_lists():
    """K14.1, K14.3 and K14.4: the board facts alone give what the DSN path
    resolves from the board and the export."""
    auto = {"class_widths": "auto", "drop_pour_nets": "auto", "fixed_pour_classes": "auto"}
    on_dsn = dsn_fixups.resolve(auto, facts(), dsn_fixups.Dsn(read(".kicad10.dsn")))
    on_board = dsn_fixups.resolve_on_board(auto, facts())
    assert on_board == on_dsn
    assert on_board["class_widths"] == SETTINGS["class_widths"]
    assert on_board["drop_pour_nets"] == SETTINGS["drop_pour_nets"]
    assert on_board["fixed_pour_classes"] == ["Power"]  # VBUS, priority 1 over GND on F.Cu
    assert dsn_fixups.auto_fixed_pour_classes_of_board(facts()) == ["Power"]
    # the classes as the export writes them
    dsn = dsn_fixups.Dsn(read(".kicad10.dsn"))
    classes = facts()["classes"]
    assert classes["net_class"] == dsn.net_class
    assert classes["default_width_mm"] == dsn.default_width_mm()
    assert classes["widths_mm"] == {n: float(dsn.class_width_node(n).text) / 1000.0 for n in dsn.classes}


def test_the_flags_carry_the_settings_as_the_dsn_fixup_writes_them():
    args, lines = dsn_fixups.input_fixup_args(SETTINGS)
    assert args == FLAGS
    assert lines == ["board snapshot fix-ups for the router: " + " ".join(FLAGS)]
    # K14.4: the fixed pour classes, sorted
    args, _ = dsn_fixups.input_fixup_args({"fixed_pour_classes": ["Switch", "Power"]})
    assert args == ["--router.input_fixups.fixed_pour_classes=Power,Switch"]
    # a width is rounded to the micrometre as `apply` writes it
    args, _ = dsn_fixups.input_fixup_args({"class_widths": {"Power": 0.6004}})
    assert args == ["--router.input_fixups.class_widths=Power=600"]
    assert dsn_fixups.input_fixup_args({}) == ([], [])


def test_a_name_a_comma_list_cannot_carry_is_left_out_with_a_warning():
    args, lines = dsn_fixups.input_fixup_args(
        {"class_widths": {"Power,Default": 0.5, "Power": 0.6}, "drop_pour_nets": ["GND", " A"], "route_on_plane_layers": ["In1.Cu"]}
    )
    assert args == [
        "--router.input_fixups.class_widths=Power=600",
        "--router.input_fixups.route_on_plane_layers=In1.Cu",
        "--router.input_fixups.drop_pour_nets=GND",
    ]
    assert lines[-1].startswith("WARNING:") and "'Power,Default'" in lines[-1] and "' A'" in lines[-1]


class _FakeClass:
    def __init__(self, name, width):
        self.name, self.width = name, width

    def GetName(self):
        return self.name

    def GetTrackWidth(self):
        return self.width


def test_board_classes_names_and_rounds_like_the_export():
    classes = {"": _FakeClass("Default", 250000), "VBUS": _FakeClass("Power", 1000000), "X": _FakeClass("Power,Default", 203200)}
    net_settings = types.SimpleNamespace(
        GetEffectiveNetClass=lambda name: classes[name], GetDefaultNetclass=lambda: classes[""]
    )
    board = types.SimpleNamespace(
        GetDesignSettings=lambda: types.SimpleNamespace(m_NetSettings=net_settings),
        GetNetsByName=lambda: {"": None, "VBUS": None, "X": None},
    )
    assert export_flow.board_classes(board) == {
        "default_width_mm": 0.25,
        "widths_mm": {"Power": 1.0, "Power,Default": float("203.2") / 1000.0},
        "net_class": {"VBUS": "Power", "X": "Power,Default"},
    }


def test_the_snapshot_passes_the_flags_explicit_or_auto(monkeypatch):
    explicit = RouteOptions(board_input="kicad_pcb", **SETTINGS)
    assert export_flow.snapshot_fixups(None, None, explicit) == dsn_fixups.input_fixup_args(SETTINGS)
    # `auto` reads the board: the recorded pcbnew facts stand in for it
    recorded = facts()
    classes = recorded.pop("classes")
    monkeypatch.setattr(export_flow.kicad_board, "collect", lambda board, pcbnew: dict(recorded))
    monkeypatch.setattr(export_flow, "board_classes", lambda board: classes)
    auto = RouteOptions(board_input="kicad_pcb", class_widths="auto", drop_pour_nets="auto", route_on_plane_layers=["In2.Cu"])
    args, lines = export_flow.snapshot_fixups(object(), object(), auto)
    assert args == FLAGS
    # nothing on: no flags, and the board is not read
    monkeypatch.setattr(export_flow.kicad_board, "collect", lambda board, pcbnew: pytest.fail("read the board"))
    assert export_flow.snapshot_fixups(object(), object(), RouteOptions(board_input="kicad_pcb")) == ([], [])


def _recorded_board(monkeypatch):
    recorded = facts()
    classes = recorded.pop("classes")
    monkeypatch.setattr(export_flow.kicad_board, "collect", lambda board, pcbnew: dict(recorded))
    monkeypatch.setattr(export_flow, "board_classes", lambda board: classes)


FIXED_FLAG = "--router.input_fixups.fixed_pour_classes=Power"


def test_the_snapshot_passes_the_fixed_pour_classes_explicit_or_auto(monkeypatch):
    """K14.4: `wires` mode, named or `auto` (from the board's classes)."""
    explicit = RouteOptions(board_input="kicad_pcb", fixed_pour_classes=["Power"])
    assert export_flow.snapshot_fixups(None, None, explicit)[0] == [FIXED_FLAG]
    _recorded_board(monkeypatch)
    auto = RouteOptions(board_input="kicad_pcb", fixed_pour_classes="auto")
    assert export_flow.snapshot_fixups(object(), object(), auto)[0] == [FIXED_FLAG]
    everything = RouteOptions(board_input="kicad_pcb", fixed_pour_classes="auto", **SETTINGS)
    assert export_flow.snapshot_fixups(object(), object(), everything)[0] == [*FLAGS, FIXED_FLAG]
    # `engine` mode lays no wires: the classes go to the engine instead
    engine = RouteOptions(board_input="kicad_pcb", fixed_pour_classes="auto", fixed_pour_mode="engine")
    assert export_flow.snapshot_fixups(object(), object(), engine) == ([], [])


def test_engine_pours_on_the_snapshot_resolve_auto_from_the_board(monkeypatch):
    """K14.4: `fixed_pour_mode = engine` with `auto` passes the classes the DSN
    path resolves (`fix_dsn`), from the board's classes."""
    _recorded_board(monkeypatch)
    engine = RouteOptions(board_input="kicad_pcb", fixed_pour_classes="auto", fixed_pour_mode="engine")
    args, lines = export_flow.snapshot_engine_args(object(), object(), engine)
    on_dsn = dsn_fixups.auto_fixed_pour_classes(facts(), dsn_fixups.Dsn(read(".kicad10.dsn")))
    assert args == dsn_fixups.engine_plane_obstacle_args(on_dsn) == ["--router.plane_obstacle_net_classes=Power"]
    assert lines == ["pours of Power kept clear by the router (--router.plane_obstacle_net_classes=Power)"]
    named = RouteOptions(board_input="kicad_pcb", fixed_pour_classes=["Switch"], fixed_pour_mode="engine")
    assert export_flow.snapshot_engine_args(None, None, named)[0] == ["--router.plane_obstacle_net_classes=Switch"]
    assert export_flow.snapshot_engine_args(None, None, RouteOptions(fixed_pour_classes=["Power"])) == ([], [])
    # through prepare_snapshot, with no reader flag for the pours
    monkeypatch.setattr(export_flow, "refill", lambda board, pcbnew: [])
    monkeypatch.setattr(export_flow.snapshot, "take", lambda *a: (True, ["snapshot taken"]))
    ok, args, lines, _ = export_flow.prepare_snapshot(object(), object(), "w", STEM, "d", replace(engine, edge_and_pad_keepouts=False))
    assert ok and args == ["--router.plane_obstacle_net_classes=Power"]


def test_a_failing_board_read_is_a_warning_and_no_flags(monkeypatch):
    def boom(board, pcbnew):
        raise RuntimeError("no board")

    monkeypatch.setattr(export_flow.kicad_board, "collect", boom)
    args, lines = export_flow.snapshot_fixups(object(), object(), RouteOptions(board_input="kicad_pcb", drop_pour_nets="auto"))
    assert args == [] and lines[0].startswith("WARNING: board snapshot fix-ups skipped")


def test_prepare_snapshot_adds_the_flags(monkeypatch):
    monkeypatch.setattr(export_flow, "refill", lambda board, pcbnew: [])
    monkeypatch.setattr(export_flow.snapshot, "take", lambda *a: (True, ["snapshot taken"]))
    chosen = RouteOptions(board_input="kicad_pcb", edge_and_pad_keepouts=False, **SETTINGS)
    ok, args, lines, locked = export_flow.prepare_snapshot(object(), object(), "w", STEM, "d", chosen)
    assert ok and args == FLAGS and locked == []
    assert "board snapshot fix-ups for the router: " + " ".join(FLAGS) in lines


@pytest.fixture(scope="module")
def router():
    exe = find_router()
    if exe is None:
        pytest.skip("no freerouting-rs executable (set FREEROUTING_RS_BIN)")
    probe = subprocess.run([exe, "--help"], capture_output=True, text=True, timeout=60)
    if "input_fixups" not in probe.stdout:
        pytest.skip(f"{exe} predates K14")
    return exe


def _copy(tmp_path):
    for suffix in (".kicad_pcb", ".kicad_pro"):
        shutil.copy(DATA / (STEM + suffix), tmp_path / (STEM + suffix))
    return tmp_path / (STEM + ".kicad_pcb")


def test_the_router_applies_the_flags_to_the_snapshot(router, tmp_path):
    design = _copy(tmp_path)
    ses = tmp_path / "out.ses"
    command = [router, "-de", str(design), "-do", str(ses), "--max-passes", "1", "--router.optimizer.enabled=false", *FLAGS]
    run = subprocess.run(command, capture_output=True, text=True, timeout=600)
    out = run.stdout + run.stderr
    assert run.returncode == 0, out
    for line in (
        "input fix-up: class Power: width 1000 -> 600 um",
        "input fix-up: class Switch: width 1500 -> 800 um",
        "input fix-up: pours: 2 outer plane(s) removed (dropped nets: GND)",
        "input fix-up: layer In2.Cu: power -> signal",
    ):
        assert line in out, out
    assert ses.is_file()
    # the run log keeps them: `plugin.py` writes `snapshot.reader_lines` of the router's lines
    kept = snapshot.reader_lines(out.splitlines())
    assert sum("input fix-up: " in line for line in kept) == 4, kept


def test_the_router_lays_the_fixed_pour_wires_and_writes_none(router, tmp_path):
    """K14.4: the router's reader covers VBUS's F.Cu pour with the 44 wires of
    the fixed-up export, and the session KiCad imports holds none of them."""
    design = _copy(tmp_path)
    ses = tmp_path / "out.ses"
    command = [router, "-de", str(design), "-do", str(ses), "--router.autorouter.enabled=false",
               "--router.fanout.enabled=false", "--router.optimizer.enabled=false", FIXED_FLAG]  # fmt: skip
    run = subprocess.run(command, capture_output=True, text=True, timeout=600)
    out = run.stdout + run.stderr
    assert run.returncode == 0, out
    assert "input fix-up: pours: 44 fixed wires for 1 pour(s) of Power; 1 outer plane(s) removed" in out
    assert "(wire" not in ses.read_text(encoding="utf-8")
    bad = subprocess.run([router, "-de", str(design), "-do", str(tmp_path / "x.ses"), "--router.input_fixups.fixed_pour_classes=Nope"],
                         capture_output=True, text=True, timeout=600)  # fmt: skip
    assert bad.returncode != 0 and "the board has no net class Nope" in bad.stdout + bad.stderr


def test_the_reader_lines_keep_the_fixup_lines():
    lines = ["[info] input fix-up: layer In2.Cu: power -> signal", "[warn] input fix-up: WARNING: x", "[info] other"]
    assert snapshot.reader_lines(lines) == lines[:2]


def test_the_router_refuses_a_class_or_layer_the_board_does_not_have(router, tmp_path):
    """As the DSN fix-ups raise them (`dsn_fixups.apply`), so both inputs fail alike."""
    design = _copy(tmp_path)
    for flag, name in (
        ("--router.input_fixups.class_widths=Nope=300", "Nope"),
        ("--router.input_fixups.route_on_plane_layers=F.Cu", "F.Cu"),
    ):
        run = subprocess.run([router, "-de", str(design), "-do", str(tmp_path / "x.ses"), flag], capture_output=True, text=True, timeout=600)
        assert run.returncode != 0
        assert "router.input_fixups." in run.stdout + run.stderr and name in run.stdout + run.stderr


def test_the_router_warns_about_an_unknown_net_and_routes(router, tmp_path):
    """The DSN fix-up passes an unknown net silently; the snapshot warns and goes on."""
    design = _copy(tmp_path)
    command = [router, "-de", str(design), "-do", str(tmp_path / "x.ses"), "--max-passes", "1",
               "--router.optimizer.enabled=false", "--router.input_fixups.drop_pour_nets=NOPE,GND"]  # fmt: skip
    run = subprocess.run(command, capture_output=True, text=True, timeout=600)
    out = run.stdout + run.stderr
    assert run.returncode == 0, out
    assert "router.input_fixups.drop_pour_nets: the board has no net NOPE; left out" in out
    assert "input fix-up: pours: 2 outer plane(s) removed (dropped nets: GND)" in out


def test_board_classes_on_the_fixture_through_pcbnew_gives_the_recorded_facts(tmp_path):
    """The pcbnew half of `auto`: `export_flow.board_classes` on the fixture,
    in KiCad's own Python (skipped without one), gives the recorded classes."""
    python = pcbnew_python()
    if python is None:
        pytest.skip("no python3 that imports pcbnew")
    design = _copy(tmp_path)
    script = tmp_path / "classes.py"
    script.write_text(
        "import json, sys\n"
        f"sys.path.insert(0, {str(REPO / 'kicad-plugin' / 'freerouting_rs')!r})\n"
        "import pcbnew, export_flow\n"
        "print('CLASSES ' + json.dumps(export_flow.board_classes(pcbnew.LoadBoard(sys.argv[1]))))\n",
        encoding="utf-8",
    )
    run = subprocess.run([python, "-I", str(script), str(design)], capture_output=True, text=True, timeout=300)
    line = next((l for l in run.stdout.splitlines() if l.startswith("CLASSES ")), None)
    assert line is not None, [l for l in (run.stdout + run.stderr).splitlines() if not any(n in l for n in NOISE)]
    assert json.loads(line[len("CLASSES "):]) == facts()["classes"]

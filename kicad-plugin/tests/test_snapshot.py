"""The direct read of the board (K10): the paths, the argument lists, the
files staged beside the snapshot, and the options check. Pure Python: pcbnew
is a small fake."""

import json
import sys
import types
from pathlib import Path

import options
import pytest
import snapshot
import tools
from options import RouteOptions

KICAD = {"board_input": "kicad_pcb", "edge_and_pad_keepouts": False}


def opts(**kwargs):
    return RouteOptions(**{**KICAD, **kwargs})


# --- K10.1: paths and argument lists -------------------------------------------


def test_the_design_file_is_the_dsn_or_the_snapshot_beside_it(tmp_path):
    work = tmp_path / "freerouting-rs"
    assert snapshot.design_path(work, "b", "dsn") == work / "b.dsn"
    assert snapshot.design_path(work, "b", "kicad_pcb") == work / "b.kicad_pcb"
    assert snapshot.snapshot_files(work, "b") == {
        "pcb": work / "b.kicad_pcb",
        "pro": work / "b.kicad_pro",
        "dru": work / "b.kicad_dru",
        "prl": work / "b.kicad_prl",
    }


@pytest.mark.parametrize("stem", ["v3.0", "ça va.ok", "board.rev2.1"])
def test_a_dot_in_the_board_name_stays_in_the_snapshots_files(tmp_path, stem):
    # K15.20: `v3.0.kicad_pcb`'s snapshot was written as `v3.kicad_pcb`, and the route
    # found no session (K15.14's pocketsdr-v3-0 row).
    work = tmp_path / "freerouting-rs"
    files = snapshot.snapshot_files(work, stem)
    assert files == {
        "pcb": work / (stem + ".kicad_pcb"),
        "pro": work / (stem + ".kicad_pro"),
        "dru": work / (stem + ".kicad_dru"),
        "prl": work / (stem + ".kicad_prl"),
    }
    assert files["pcb"] == snapshot.design_path(work, stem, "kicad_pcb")


@pytest.mark.parametrize("stem", ["board", "Platine für Größe 2", "板子 v2", "ça va.ok"])
def test_the_argument_lists_for_both_inputs_with_non_ascii_names(tmp_path, stem):
    work = tmp_path / "Ordner äöü" / "freerouting-rs"
    ses = work / (stem + ".ses")
    for board_input, suffix in (("dsn", ".dsn"), ("kicad_pcb", ".kicad_pcb")):
        design = snapshot.design_path(work, stem, board_input)
        assert design == work / (stem + suffix)
        args = tools.route_args(design, ses, RouteOptions(board_input=board_input).to_args(150.0))
        assert args == ["-de", design, "-do", ses, "--router.min_trace_width_um=150"]
        assert str(args[1]).endswith(stem + suffix)  # the name is kept character for character


def test_the_dsn_input_adds_nothing_and_the_enhanced_profile_wants_the_snapshot():
    assert RouteOptions().board_input == "dsn"  # the dataclass default is legacy's (K13.7)
    assert not snapshot.wanted(RouteOptions())
    assert snapshot.wanted(opts())
    assert snapshot.wanted(options.profile_defaults("enhanced"))
    assert not snapshot.wanted(options.profile_defaults("legacy"))
    assert RouteOptions().to_args() == []
    assert snapshot.router_args(RouteOptions()) == []


# --- K10.3: the .kicad_dru switch -----------------------------------------------


def test_the_rules_switch_is_passed_for_every_snapshot_route_and_never_for_a_dsn():
    # K13.1: the board's rules reach the router on both inputs, so the
    # snapshot always gets the switch (the option `apply_kicad_rules` is gone,
    # K13.7); `export_flow.kicad_dru_args` adds it (test_board_rules_both_inputs.py).
    assert opts().to_args() == []
    assert snapshot.router_args(opts()) == ["--apply-design-rules"]
    # not on a DSN input
    assert snapshot.router_args(RouteOptions()) == []


def test_the_run_log_notes_say_what_a_snapshot_does_not_do():
    assert any("rule-area fix-up not applied" in n for n in snapshot.notes(opts()))
    assert all("rule-area" not in n for n in snapshot.notes(opts(rule_area_fixup=False)))
    assert any("--apply-design-rules" in n for n in snapshot.notes(opts()))


def test_the_reader_lines_are_picked_out_of_the_router_log():
    lines = [
        "[info] KiCad project file: b.kicad_pro",
        "[info] rule areas: 2",
        "[warn] KiCad reader: line 12: wire_keepout area",
        "[info] design rule 'fast tracks' (line 8), constraint track_width: applied (1 target(s))",
        "[info] Effective start_ripup_costs: 100 (source: defaults)",
        "a plain line",
        "[info] KiCad design rules file: b.kicad_dru",
    ]
    assert snapshot.reader_lines(lines) == [lines[0], lines[1], lines[2], lines[3], lines[6]]


# --- K10.1/K10.2: taking the snapshot -------------------------------------------


class FakePcbnew:
    """SaveBoard and SettingsManager.SaveProjectCopy, recording their calls."""

    def __init__(self, save_ok=True, project="live", save_type_error=False):
        self.calls = []
        self.save_ok, self.project, self.save_type_error = save_ok, project, save_type_error
        outer = self

        class Manager:
            def SaveProjectCopy(self, path, project):
                outer.calls.append(("SaveProjectCopy", Path(path).name))
                if outer.project == "fail":
                    raise RuntimeError("boom")
                if outer.project == "none":
                    return
                Path(path).write_text("live project", encoding="utf-8")
                Path(path).with_suffix(".kicad_prl").write_text("{}", encoding="utf-8")

        self.GetSettingsManager = Manager

    def SaveBoard(self, path, board, *rest):
        if self.save_type_error and rest:
            raise TypeError("SaveBoard() takes 2 positional arguments")
        self.calls.append(("SaveBoard", Path(path).name, rest))
        if self.save_ok:
            Path(path).write_text("(kicad_pcb)", encoding="utf-8")
        return self.save_ok


class Board:
    def GetProject(self):
        return object()


@pytest.fixture
def folders(tmp_path):
    src = tmp_path / "src"
    src.mkdir()
    (src / "b.kicad_pro").write_text("disk project", encoding="utf-8")
    (src / "b.kicad_dru").write_text("(version 1)", encoding="utf-8")
    return src, tmp_path / "work"


def text(path):
    return Path(path).read_text(encoding="utf-8")


def test_take_saves_the_board_without_settings_and_writes_the_live_project(folders):
    src, work = folders
    pcbnew = FakePcbnew()
    ok, lines = snapshot.take(Board(), pcbnew, work, "b", src)
    assert ok
    assert pcbnew.calls == [("SaveBoard", "b.kicad_pcb", (True,)), ("SaveProjectCopy", "b.kicad_pro")]
    assert text(work / "b.kicad_pcb") == "(kicad_pcb)"
    assert text(work / "b.kicad_pro") == "live project"  # not the file on disk
    assert text(work / "b.kicad_dru") == "(version 1)"  # not in memory: copied
    assert not (work / "b.kicad_prl").exists()
    assert not any(line.startswith("WARNING") for line in lines)
    assert text(src / "b.kicad_pro") == "disk project"  # the board's own folder is untouched


@pytest.mark.parametrize("project", ["fail", "none"])
def test_a_missing_live_project_copy_falls_back_to_the_file_on_disk_and_says_what_is_missed(folders, project):
    src, work = folders
    ok, lines = snapshot.take(Board(), FakePcbnew(project=project), work, "b", src)
    assert ok and text(work / "b.kicad_pro") == "disk project"
    warning = [line for line in lines if line.startswith("WARNING: project")]
    assert warning and "not saved is not in the snapshot" in warning[0]


def test_a_board_without_project_files_stages_none(tmp_path):
    ok, lines = snapshot.take(Board(), FakePcbnew(project="none"), tmp_path / "w", "b", tmp_path)
    assert ok and sorted(p.name for p in (tmp_path / "w").iterdir()) == ["b.kicad_pcb"]
    assert any("no .kicad_pro" in line for line in lines)


def test_a_stale_snapshot_never_survives_a_failed_one(folders):
    src, work = folders
    work.mkdir()
    for name in ("b.kicad_pcb", "b.kicad_pro", "b.kicad_dru", "b.kicad_prl"):
        (work / name).write_text("stale", encoding="utf-8")
    ok, lines = snapshot.take(Board(), FakePcbnew(save_ok=False), work, "b", src)
    assert not ok and "SaveBoard wrote no board file" in lines[0]
    assert list(work.iterdir()) == []


def test_a_saveboard_without_askipsettings_is_refused_not_called_the_old_way(folders):
    src, work = folders
    pcbnew = FakePcbnew(save_type_error=True)
    ok, lines = snapshot.take(Board(), pcbnew, work, "b", src)
    assert not ok and "aSkipSettings" in lines[0]
    assert pcbnew.calls == []  # the two-argument form would save the project file


# --- K10.4: the options check ---------------------------------------------------

DSN_REWRITES = {
    "class_widths": ({"GND": 0.4}, "Class widths"),
    "fixed_pour_classes": (["Power"], "Fixed pour classes"),
    "drop_pour_nets": (["GND"], "Drop pours of nets"),
    "edge_and_pad_keepouts": (True, "Keep the board-edge and pad clearances"),
    "route_on_plane_layers": (["Power.Cu"], "Route on plane layers"),
}


def test_the_defaults_are_accepted_on_the_snapshot_and_the_keepouts_are_ignored():
    """K13.7: `edge_and_pad_keepouts` is on by default (K09.9) and the reader
    writes the same keepouts itself (edge shapes and `min_copper_edge_clearance`,
    the pads' own clearances), so the snapshot takes the defaults and says the
    option is ignored there."""
    assert RouteOptions().edge_and_pad_keepouts
    default_snapshot = RouteOptions(board_input="kicad_pcb")
    assert options.input_ignored(default_snapshot) == [DSN_REWRITES["edge_and_pad_keepouts"][1]]
    note = options.input_ignored_note(default_snapshot)
    assert "'Keep the board-edge and pad clearances'" in note and "ignored" in note
    # nothing is ignored when the option is off, or on the DSN input, where it applies
    assert options.input_ignored(opts()) == [] and options.input_ignored_note(opts()) == ""
    assert options.input_ignored(RouteOptions()) == [] and options.input_ignored_note(RouteOptions()) == ""


def test_the_snapshot_run_log_carries_the_ignored_note_once():
    lines = snapshot.notes(RouteOptions(board_input="kicad_pcb"))
    assert sum("ignored" in line and "Keep the board-edge and pad clearances" in line for line in lines) == 1
    assert not any("Keep the board-edge" in line for line in snapshot.notes(opts()))


def load_headless_run(monkeypatch):
    """`headless_run.py` by path, with an empty `pcbnew` (it imports one at the
    top); nothing here touches a board."""
    import importlib.util

    from conftest import REPO

    monkeypatch.setitem(sys.modules, "pcbnew", types.ModuleType("pcbnew"))
    spec = importlib.util.spec_from_file_location("headless_run_under_test", REPO / "kicad-plugin" / "headless_run.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run_headless_route(monkeypatch, tmp_path, saved):
    """`headless_run.route` on options `saved`, with the preparation process
    replaced by a recorder that fails (code 7) so nothing else runs. Returns
    `(exit code, the preparation command)`."""
    headless = load_headless_run(monkeypatch)
    board = tmp_path / "proj" / "b.kicad_pcb"
    board.parent.mkdir(parents=True)
    path = tmp_path / "options.json"
    path.write_text(json.dumps({"schema": 4, **saved}), encoding="utf-8")
    calls = []

    def fake_run(command, *args, **kwargs):
        calls.append(command)
        return types.SimpleNamespace(returncode=7)

    monkeypatch.setattr(headless.subprocess, "run", fake_run)
    code = headless.route(str(board), str(tmp_path / "work"), str(path), "router")
    return code, (calls[0] if calls else None)


def test_headless_route_takes_the_default_options_on_a_snapshot_and_prints_the_note(monkeypatch, tmp_path, capsys):
    code, command = run_headless_route(monkeypatch, tmp_path, {"board_input": "kicad_pcb"})
    assert code == 7 and "snapshot" in command
    out = capsys.readouterr()
    assert "Incompatible" not in out.err
    assert "Keep the board-edge and pad clearances" in out.out and "ignored" in out.out


def test_headless_route_takes_the_fixed_pour_classes_on_a_snapshot(monkeypatch, tmp_path, capsys):
    """K14.4: the fixed pour wires go to the router's reader too."""
    code, command = run_headless_route(monkeypatch, tmp_path, {"board_input": "kicad_pcb", "fixed_pour_classes": ["Power"]})
    assert code == 7 and "snapshot" in command
    assert "Incompatible" not in capsys.readouterr().err


def test_headless_route_takes_the_reader_fixups_on_a_snapshot(monkeypatch, tmp_path, capsys):
    """K14.1-K14.3: the three go to the router's reader, so the snapshot runs."""
    saved = {"board_input": "kicad_pcb", "class_widths": {"GND": 0.4}, "drop_pour_nets": ["GND"], "route_on_plane_layers": ["In1.Cu"]}
    code, command = run_headless_route(monkeypatch, tmp_path, saved)
    assert code == 7 and "snapshot" in command
    assert "Incompatible" not in capsys.readouterr().err


def test_headless_route_on_the_dsn_prints_no_note(monkeypatch, tmp_path, capsys):
    code, command = run_headless_route(monkeypatch, tmp_path, {"board_input": "dsn"})
    assert code == 7 and "export" in command
    assert "ignored" not in capsys.readouterr().out


def test_headless_route_follows_the_profile_when_the_options_name_no_input(monkeypatch, tmp_path):
    """K13.7: a file without `board_input` takes the profile's input, as the dialog does."""
    _, command = run_headless_route(monkeypatch, tmp_path / "a", {"profile": "enhanced"})
    assert "snapshot" in command
    _, command = run_headless_route(monkeypatch, tmp_path / "b", {"profile": "legacy"})
    assert "export" in command
    # K14.5: a fix-up and no choice takes the profile's input too; a file that says `dsn` keeps it
    _, command = run_headless_route(monkeypatch, tmp_path / "c", {"class_widths": {"GND": 0.4}})
    assert "snapshot" in command
    _, command = run_headless_route(monkeypatch, tmp_path / "d", {"class_widths": {"GND": 0.4}, "board_input": "dsn"})
    assert "export" in command


def test_headless_export_and_snapshot_modes_set_their_input_whatever_the_options_say(monkeypatch, tmp_path):
    headless = load_headless_run(monkeypatch)
    seen = []
    headless.pcbnew.LoadBoard = lambda path: object()
    monkeypatch.setattr(headless.export_flow, "prepare", lambda board, pcbnew, dsn, opts, export: seen.append(opts) or (False, [], [], []))
    monkeypatch.setattr(headless.export_flow, "prepare_snapshot", lambda *a: seen.append(a[-1]) or (False, [], [], []))
    path = tmp_path / "options.json"
    path.write_text(json.dumps({"schema": 4, "profile": "enhanced"}), encoding="utf-8")
    headless.export(str(tmp_path / "b.kicad_pcb"), str(tmp_path / "w"), str(path))
    path.write_text(json.dumps({"schema": 6, "profile": "enhanced", "board_input": "dsn"}), encoding="utf-8")
    headless.snapshot_mode(str(tmp_path / "b.kicad_pcb"), str(tmp_path / "w"), str(path), [])
    assert [o.board_input for o in seen] == ["dsn", "kicad_pcb"]


def test_every_k09_option_is_either_checked_or_allowed():
    """The K09 fields: five rewrite the DSN text (the reader applies four on a snapshot since K14, and ignores the fifth); the rest are
    router flags or board locks and work on a snapshot. A new K09 option has to
    be put in one list."""
    checked = set(DSN_REWRITES)
    allowed = {
        "fixed_pour_mode",  # `engine` with named classes is an engine flag
        "fix_existing",
        "start_ripup_costs",
        "sweep",
        "sweep_passes",
        "use_pass_hint",
        "pass_hint",
        "pin_escape",  # Q21: a router flag
    }
    from dataclasses import fields

    k09 = {f.name for f in fields(RouteOptions)} - {
        "max_passes", "threads", "optimizer", "improvement_threshold", "ignore_net_classes", "seed",
        "min_trace_width_um", "timeout", "rule_area_fixup", "profile", "stages", "board_input",
    }  # fmt: skip
    assert k09 == checked | allowed


def test_the_form_accepts_every_dsn_option_on_a_snapshot():
    # K14.4: the fixed pour wires, named or auto, are accepted too
    for value in ("Power", "auto"):
        chosen, errors = options.from_form({"board_input": "kicad_pcb", "fixed_pour_classes": value})
        assert errors == [] and chosen.board_input == "kicad_pcb", value
    # K14.1-K14.3: the reader's fix-ups are accepted on the snapshot
    form = {"board_input": "kicad_pcb", "class_widths": "GND=0.4", "drop_pour_nets": "auto", "route_on_plane_layers": "In1.Cu"}
    chosen, errors = options.from_form(form)
    assert errors == [] and chosen.class_widths == {"GND": 0.4} and chosen.drop_pour_nets == "auto"
    assert chosen.route_on_plane_layers == ["In1.Cu"]
    # the keepouts are on by default and do not refuse the snapshot (K13.7)
    chosen, errors = options.from_form({"board_input": "kicad_pcb"})
    assert errors == [] and chosen.board_input == "kicad_pcb" and chosen.edge_and_pad_keepouts
    chosen, errors = options.from_form({"board_input": "kicad_pcb", "edge_and_pad_keepouts": True})
    assert errors == [] and chosen.edge_and_pad_keepouts
    chosen, errors = options.from_form({"board_input": "kicad_pcb", "edge_and_pad_keepouts": False})
    assert errors == [] and chosen.board_input == "kicad_pcb"
    _, errors = options.from_form({"board_input": "ses"})
    assert errors and "board input" in errors[0]


def test_the_new_fields_round_trip_and_older_files_load(tmp_path):
    path = tmp_path / "route-options.json"
    chosen = opts()
    options.save(path, chosen)
    assert options.load(path) == chosen
    import json

    # K13.7: the snapshot is enhanced's default, so it is not written; a choice that differs is
    saved = json.loads(path.read_text(encoding="utf-8"))
    assert saved["schema"] == 6 and "board_input" not in saved and "apply_kicad_rules" not in saved
    options.save(path, RouteOptions())  # the DSN input under enhanced
    assert json.loads(path.read_text(encoding="utf-8"))["board_input"] == "dsn"
    options.save(path, opts(profile="legacy"))
    assert json.loads(path.read_text(encoding="utf-8"))["board_input"] == "kicad_pcb"
    # a schema 3 file has no K10 fields: the profile's input
    assert RouteOptions.from_dict({"schema": 3, "profile": "enhanced"}).board_input == "kicad_pcb"
    assert RouteOptions.from_dict({"schema": 3, "profile": "legacy"}).board_input == "dsn"
    # bad values keep the default
    bad = {"schema": 4, "board_input": "svg", "apply_kicad_rules": "yes"}
    assert RouteOptions.from_dict(bad) == options.profile_defaults("enhanced")

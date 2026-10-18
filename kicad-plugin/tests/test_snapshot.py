"""The direct read of the board (K10): the paths, the argument lists, the
files staged beside the snapshot, and the options check. Pure Python: pcbnew
is a small fake."""

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


def test_the_dsn_input_is_the_default_and_adds_nothing():
    assert RouteOptions().board_input == "dsn"
    assert not snapshot.wanted(RouteOptions())
    assert snapshot.wanted(opts())
    assert RouteOptions().to_args() == []
    assert snapshot.router_args(RouteOptions(apply_kicad_rules=True)) == []


# --- K10.3: the .kicad_dru switch -----------------------------------------------


def test_the_rules_switch_is_passed_only_when_the_option_is_on_and_the_input_is_the_snapshot():
    assert opts().to_args() == []
    assert opts(apply_kicad_rules=True).to_args() == ["--apply-design-rules"]
    assert snapshot.router_args(opts(apply_kicad_rules=True)) == ["--apply-design-rules"]
    assert snapshot.router_args(opts()) == []
    # not on a DSN input, whatever the option says
    assert RouteOptions(apply_kicad_rules=True).to_args() == []
    # not for the GUI, which has no such switch
    assert opts(apply_kicad_rules=True).to_args(for_gui=True) == []
    # in order: after the K09 router flags, before extra args and stages
    args = opts(apply_kicad_rules=True, sweep="free", stages={"a.b": "x"}).to_args(150.0, extra_args=["--e"])
    assert args == [
        "--router.min_trace_width_um=150",
        "--router.autorouter.sweep=free",
        "--apply-design-rules",
        "--e",
        "--router.a.b=x",
    ]


def test_the_run_log_notes_say_what_a_snapshot_does_not_do():
    assert any("rule-area fix-up not applied" in n for n in snapshot.notes(opts()))
    assert snapshot.notes(opts(rule_area_fixup=False)) == []
    assert any("--apply-design-rules" in n for n in snapshot.notes(opts(apply_kicad_rules=True)))


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


@pytest.mark.parametrize("name", sorted(DSN_REWRITES))
def test_each_dsn_rewriting_option_refuses_the_snapshot_by_name(name):
    value, label = DSN_REWRITES[name]
    chosen = RouteOptions(**{**KICAD, name: value})
    assert options.input_conflicts(chosen) == [label]
    # the DSN input has no such problem
    assert options.input_conflicts(RouteOptions(**{name: value})) == []


def test_the_defaults_refuse_the_snapshot_until_the_keepouts_are_off():
    assert RouteOptions().edge_and_pad_keepouts  # the board's clearances, on by default (K09.9)
    assert options.input_conflicts(RouteOptions(board_input="kicad_pcb")) == [DSN_REWRITES["edge_and_pad_keepouts"][1]]
    assert options.input_conflicts(opts()) == []


def test_every_k09_option_is_either_checked_or_allowed():
    """The K09 fields: five rewrite the DSN text and are refused; the rest are
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
    }
    from dataclasses import fields

    k09 = {f.name for f in fields(RouteOptions)} - {
        "max_passes", "threads", "optimizer", "improvement_threshold", "ignore_net_classes", "seed",
        "min_trace_width_um", "timeout", "rule_area_fixup", "profile", "stages", "board_input", "apply_kicad_rules",
    }  # fmt: skip
    assert k09 == checked | allowed
    assert options.input_conflicts(
        opts(fix_existing=True, start_ripup_costs=1500, sweep="free", sweep_passes=2, use_pass_hint=True, pass_hint=3)
    ) == []


def test_engine_pours_with_named_classes_work_on_a_snapshot_but_auto_does_not():
    named = opts(fixed_pour_classes=["Power_High"], fixed_pour_mode="engine")
    assert options.input_conflicts(named) == []
    auto = opts(fixed_pour_classes="auto", fixed_pour_mode="engine")
    assert options.input_conflicts(auto) == ["Fixed pour classes"]
    assert options.input_conflicts(opts(fixed_pour_classes=["Power_High"], fixed_pour_mode="wires")) == ["Fixed pour classes"]
    assert options.input_conflicts(opts(class_widths="auto")) == ["Class widths"]


def test_the_form_refuses_a_snapshot_with_a_dsn_option_on_and_names_it():
    chosen, errors = options.from_form({"board_input": "kicad_pcb", "edge_and_pad_keepouts": True, "class_widths": "GND=0.4"})
    assert chosen.board_input == "kicad_pcb"
    assert len(errors) == 2
    assert "'Class widths'" in errors[0] or "'Class widths'" in errors[1]
    assert any("'Keep the board-edge and pad clearances'" in e for e in errors)
    chosen, errors = options.from_form({"board_input": "kicad_pcb", "edge_and_pad_keepouts": False})
    assert errors == [] and chosen.board_input == "kicad_pcb"
    _, errors = options.from_form({"board_input": "ses"})
    assert errors and "board input" in errors[0]


def test_the_new_fields_round_trip_and_older_files_load(tmp_path):
    path = tmp_path / "route-options.json"
    chosen = opts(apply_kicad_rules=True)
    options.save(path, chosen)
    assert options.load(path) == chosen
    import json

    saved = json.loads(path.read_text(encoding="utf-8"))
    assert saved["schema"] == 4 and saved["board_input"] == "kicad_pcb" and saved["apply_kicad_rules"] is True
    # the defaults are not written
    options.save(path, RouteOptions())
    assert "board_input" not in json.loads(path.read_text(encoding="utf-8"))
    # a schema 3 file has no K10 fields: the DSN input
    assert RouteOptions.from_dict({"schema": 3, "profile": "enhanced"}).board_input == "dsn"
    # bad values keep the default
    bad = {"schema": 4, "board_input": "svg", "apply_kicad_rules": "yes"}
    assert RouteOptions.from_dict(bad) == options.profile_defaults("enhanced")

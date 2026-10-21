"""K13.3: `board_input = kicad_pcb` falls back to the DSN export when the reader
refuses the snapshot, and says which input ran and why.

The pre-check (`input_choice.snapshot_refusal`) reads the `(version N)` of the
snapshot; the router's own refusal is recognised from its error log line, which
the real CLI prints as `There was a parse error while reading board file at
'<name>': <reason>` with exit code 1. The fake router's `route-refuse-kicad`
mode refuses a `.kicad_pcb` the same way. The stub pcbnew's SaveBoard writes the
text in `snapshot_text`."""

import json
from pathlib import Path

import input_choice
import options
import pytest
import stub_ui
import summary

KICAD_5 = '(kicad_pcb (version 20171130) (host pcbnew 5.1.6)\n)\n'
KICAD_9 = '(kicad_pcb (version 20241229) (generator "pcbnew")\n)\n'
REFUSAL_LINE = (
    "[error] There was a parse error while reading board file at 'b.kicad_pcb': "
    "line 7, column 3: unsupported construct (test)"
)


# --- pure functions ---------------------------------------------------------------


@pytest.mark.parametrize(
    "text, refused",
    [
        ('(kicad_pcb (version 20171130) (host pcbnew 5.1.6))', True),  # KiCad 5
        ("(kicad_pcb (version 4) (host pcbnew 4.0.7))", True),  # the first s-expression format
        ("(kicad_pcb (version 20201114) (generator pcbnew))", True),  # one day short of KiCad 6's format
        ("(kicad_pcb (version 20201115) (generator pcbnew))", False),  # FIRST_SUPPORTED_VERSION of the reader
        ('(kicad_pcb (version 20260206) (generator "pcbnew"))', False),
        ("(kicad_pcb (generator pcbnew))", False),  # no version: the reader reads it as 20201115
        ("not a board at all", False),  # a syntax problem is the router's to report
    ],
)
def test_snapshot_refusal_reads_the_version_the_reader_gates_on(tmp_path, text, refused):
    pcb = tmp_path / "b.kicad_pcb"
    pcb.write_text(text, encoding="utf-8")
    reason = input_choice.snapshot_refusal(pcb)
    assert (reason is not None) == refused
    if refused:
        assert "older format than KiCad 6" in reason and "version" in reason


def test_snapshot_refusal_without_a_file_is_not_a_refusal(tmp_path):
    assert input_choice.snapshot_refusal(tmp_path / "missing.kicad_pcb") is None


def test_load_refusal_takes_the_reason_from_the_routers_error_line():
    lines = ["[info] Effective start_ripup_costs: 100", REFUSAL_LINE, "[info] done"]
    assert input_choice.load_refusal(lines) == "line 7, column 3: unsupported construct (test)"
    # a file name with quotes and colons in it
    odd = "[error] There was a parse error while reading board file at 'it's: a.kicad_pcb': line 1, column 12: bad"
    assert input_choice.load_refusal([odd]) == "line 1, column 12: bad"


@pytest.mark.parametrize(
    "line",
    [
        "[error] Couldn't load the input file 'b.kicad_pcb': No such file or directory",  # I/O, not a refusal
        "[error] Routing failed: out of memory",
        "[warn] KiCad reader: line 12: wire_keepout area",
    ],
)
def test_load_refusal_ignores_other_failures(line):
    assert input_choice.load_refusal([line]) is None


def test_the_log_line_names_the_input_and_the_reason():
    assert input_choice.input_line("kicad_pcb", None) == "input: kicad_pcb (the board snapshot, read directly)"
    assert input_choice.input_line("dsn", None) == "input: dsn (KiCad's DSN export, as chosen)"
    assert input_choice.input_line("dsn", "version 4") == (
        "input: dsn (KiCad's DSN export; fallback from kicad_pcb because version 4)"
    )


def test_the_summary_records_and_shows_the_input(tmp_path):
    path = tmp_path / "b.result.json"
    path.write_text(json.dumps({"final_state": "COMPLETED"}), encoding="utf-8")
    assert summary.merge_input(path, "dsn", "version 4") is True
    manifest = summary.load_manifest(path)
    assert manifest["final_state"] == "COMPLETED"  # the router's keys stay
    assert manifest["input"] == "dsn" and manifest["input_fallback_reason"] == "version 4"
    assert "Input: dsn (fallback from kicad_pcb: version 4)" in summary.format_summary(manifest)
    summary.merge_input(path, "kicad_pcb", None)
    manifest = summary.load_manifest(path)
    assert manifest["input"] == "kicad_pcb" and manifest["input_fallback_reason"] is None
    assert "Input: kicad_pcb\n" in summary.format_summary(manifest) + "\n"
    assert not any(l.startswith("Input:") for l in summary.format_summary({"final_state": "COMPLETED"}).splitlines())


def test_merge_input_without_a_manifest_does_nothing(tmp_path):
    assert summary.merge_input(tmp_path / "none.json", "dsn", None) is False
    assert not (tmp_path / "none.json").exists()


# --- the Route flow ------------------------------------------------------------------


@pytest.fixture
def env(tmp_path, monkeypatch, make_fake_router, isolated_env):
    plugin, ui = stub_ui.install(monkeypatch)
    pcb = tmp_path / "board dir" / "b.kicad_pcb"
    pcb.parent.mkdir()
    pcb.write_text("", encoding="utf-8")
    pcbnew = stub_ui.sys.modules["pcbnew"]
    real_save = pcbnew.SaveBoard
    text = {"value": KICAD_9}

    def save_board(path, board, skip_settings=False):
        ok = real_save(path, board, skip_settings)
        Path(path).write_text(text["value"], encoding="utf-8")
        return ok

    pcbnew.SaveBoard = save_board

    def make_dialog(mode, snapshot_text=KICAD_9, saved=None):
        text["value"] = snapshot_text
        if saved is not None:  # a route-options.json the board already has
            folder = pcb.parent / "freerouting-rs"
            folder.mkdir(exist_ok=True)
            (folder / "route-options.json").write_text(json.dumps(saved), encoding="utf-8")
        router = make_fake_router(mode)
        monkeypatch.setattr(plugin.tools, "find_tool", lambda name: plugin.tools.Tool(name, router))
        monkeypatch.setattr(plugin.tools, "query_version", lambda tool, timeout=10: (0, 1, 0, "abc"))
        return plugin.Dialog(None, stub_ui.FakeBoard(pcb))  # K13.7: the snapshot is the default input

    return make_dialog, ui, pcb.parent / "freerouting-rs"


def result(work):
    return json.loads((work / "b.result.json").read_text(encoding="utf-8"))


def log_of(work):
    return (work / "plugin.log").read_text(encoding="utf-8")


def test_the_normal_case_routes_the_snapshot_and_says_so(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route")
    dialog.on_route(None)
    assert len(ui.snapshots) == 1 and ui.exports == []
    assert ui.imports == [work / "b.ses"]
    manifest = result(work)
    assert manifest["input"] == "kicad_pcb" and manifest["input_fallback_reason"] is None
    text = ui.messages[-1][0]
    assert text.startswith("Routed and imported the result into the board.")
    assert "Input: kicad_pcb" in text.splitlines() and "fallback" not in text
    assert "input: kicad_pcb (the board snapshot, read directly)" in log_of(work)
    assert "board_input" not in json.loads((work / "route-options.json").read_text(encoding="utf-8"))  # the default


IGNORED = "Keep the board-edge and pad clearances"


def test_a_board_with_no_saved_file_opens_on_the_snapshot_with_only_the_keepouts_note(env):
    """K13.7: the default input, and no input note beyond the keepouts one."""
    make_dialog, _ui, work = env
    dialog = make_dialog("route")
    assert not (work / "route-options.json").exists()
    assert options.BOARD_INPUTS[dialog.board_input.GetSelection()] == "kicad_pcb"
    assert dialog.input_note.label == options.input_ignored_note(options.profile_defaults())
    assert IGNORED in dialog.input_note.label
    assert dialog.input_note.label.count("ignored") == 1 and "DSN" not in dialog.input_note.label.split(":")[0]
    assert "xperimental" not in " ".join(dialog.board_input.choices)


def test_a_route_with_the_defaults_runs_the_snapshot(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route")
    dialog.on_route(None)
    assert len(ui.snapshots) == 1 and ui.exports == []
    assert result(work)["input"] == "kicad_pcb" and result(work)["input_fallback_reason"] is None
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["schema"] == 6 and "board_input" not in saved  # the default is not written


def test_a_saved_dsn_choice_keeps_the_dsn(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route", saved={"schema": 6, "profile": "enhanced", "board_input": "dsn"})
    assert dialog.board_input.GetSelection() == 0 and dialog.input_note.label == ""
    dialog.on_route(None)
    assert ui.snapshots == [] and len(ui.exports) == 1
    assert json.loads((work / "route-options.json").read_text(encoding="utf-8"))["board_input"] == "dsn"


def test_the_legacy_profile_opens_on_the_dsn(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route", saved={"schema": 6, "profile": "legacy"})
    assert dialog.board_input.GetSelection() == 0
    dialog.on_route(None)
    assert ui.snapshots == [] and len(ui.exports) == 1


def test_the_profile_choice_does_not_move_the_input(env):
    make_dialog, _ui, _work = env
    dialog = make_dialog("route")
    dialog.profile.SetSelection(options.PROFILES.index("legacy"))
    assert options.from_form(dialog.form_values())[0].board_input == "kicad_pcb"
    assert dialog.board_input.GetSelection() == 1


def test_a_schema_5_board_with_class_widths_and_no_choice_still_routes_on_the_dsn(env):
    """The board's saved setup must not refuse its own Route (K13: a run never
    fails just because of the input choice)."""
    make_dialog, ui, work = env
    dialog = make_dialog("route", saved={"schema": 5, "profile": "enhanced", "class_widths": {"GND": 0.4}})
    assert dialog.board_input.GetSelection() == 0
    dialog.on_route(None)
    assert ui.snapshots == [] and len(ui.exports) == 1
    assert ui.messages[-1][0].startswith("Routed and imported the result into the board.")


def test_a_schema_5_file_with_apply_kicad_rules_routes_and_is_saved_as_schema_6(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route", saved={"schema": 5, "profile": "enhanced", "apply_kicad_rules": True})
    assert "saved_kicad_rules" not in vars(dialog)
    dialog.on_route(None)
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["schema"] == 6 and "apply_kicad_rules" not in saved
    assert len(ui.snapshots) == 1


def test_the_defaults_route_the_snapshot_and_the_dialog_says_the_keepouts_are_ignored(env):
    """K13.7: the keepouts box is on by default; on the snapshot it is ignored,
    the dialog says so (a label and a log line) and the run goes ahead."""
    make_dialog, ui, work = env
    dialog = make_dialog("route")
    assert dialog.keepouts.GetValue()
    dialog.on_input_changed(None)  # what the board input choice calls
    assert IGNORED in dialog.input_note.label and "ignored" in dialog.input_note.label
    dialog.on_route(None)
    assert not any(style & 0x400 for _, style in ui.messages)  # no error box
    assert ui.messages[-1][0].startswith("Routed and imported the result into the board.")
    assert len(ui.snapshots) == 1 and ui.exports == []
    log = log_of(work)
    assert [line for line in log.splitlines() if IGNORED in line and "ignored" in line]
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert "board_input" not in saved and saved.get("edge_and_pad_keepouts", True) is True


def test_the_note_follows_the_input_and_the_keepouts_box(env):
    make_dialog, _ui, _work = env
    dialog = make_dialog("route")
    dialog.on_input_changed(None)
    assert IGNORED in dialog.input_note.label
    dialog.keepouts.SetValue(False)
    dialog.on_input_changed(None)
    assert dialog.input_note.label == ""
    dialog.keepouts.SetValue(True)
    dialog.board_input.SetSelection(0)  # KiCad's DSN export applies the box
    dialog.on_input_changed(None)
    assert dialog.input_note.label == ""
    dialog.board_input.SetSelection(1)
    dialog.show_options(options.RouteOptions(board_input="kicad_pcb"))  # a preset
    assert IGNORED in dialog.input_note.label


CANARY_SAVED = {  # canary-pwr's route-options.json from a run before K13.7: its K09 fix-ups, no input
    "schema": 4,
    "profile": "enhanced",
    "class_widths": {"GND": 0.4, "Power": 0.3},
    "fixed_pour_classes": ["Power_High", "Switch_Node"],
    "drop_pour_nets": ["GND"],
}
THREE = "'Class widths', 'Fixed pour classes' and 'Drop pours of nets'"


def test_a_board_kept_on_the_dsn_by_its_fixups_says_why_and_the_snapshot_says_what_blocks_it(env):
    """The owner's run (2026-10-21): canary-pwr opened on the DSN with no word
    why, and the snapshot was refused only at Route."""
    make_dialog, ui, work = env
    dialog = make_dialog("route", saved=CANARY_SAVED)
    assert dialog.board_input.GetSelection() == 0
    assert dialog.input_note.label == f"{THREE} need KiCad's DSN export: turn them off to route the board snapshot."
    dialog.board_input.SetSelection(1)
    dialog.on_input_changed(None)
    assert dialog.input_note.label.split("\n")[0] == (
        f"The board snapshot cannot apply {THREE}: turn them off, or choose KiCad's DSN export."
    )
    for box in (dialog.widths, dialog.fixed_pours, dialog.drop_pours):  # each text box calls on_input_changed
        box.SetValue("")
        dialog.on_input_changed(None)
    assert dialog.input_note.label == options.input_ignored_note(options.profile_defaults())
    dialog.on_route(None)
    assert len(ui.snapshots) == 1 and ui.exports == []
    assert ui.messages[-1][0].startswith("Routed and imported the result into the board.")


def test_the_note_wraps_to_the_board_inputs_column(env, monkeypatch):
    """The owner's run (2026-10-21): the note ran off the right side. It wraps to
    the width the board input choice was laid out to, and again when that
    changes."""
    make_dialog, _ui, _work = env
    monkeypatch.setattr(stub_ui.Choice, "size", (333, 23))
    dialog = make_dialog("route", saved=CANARY_SAVED)
    assert dialog.input_note.wrapped == 333
    dialog.board_input.SetSelection(1)
    dialog.on_input_changed(None)
    assert dialog.input_note.wrapped == 333  # a new text keeps the column's width
    dialog.on_input_choice_size(stub_ui.SizeEvent(250, 23))
    assert dialog.input_note.wrapped == 250
    dialog.widths.SetValue("")
    dialog.on_input_changed(None)
    assert dialog.input_note.wrapped == 250
    monkeypatch.setattr(type(dialog), "__bool__", lambda self: False, raising=False)  # closed before the call ran
    dialog.wrap_input_note_later(200)
    assert dialog.input_note.wrapped == 250


def test_a_kicad_5_board_is_exported_as_a_dsn_without_routing_the_snapshot(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo", snapshot_text=KICAD_5)  # echo: records the one router call
    assert "apply_rules" not in vars(dialog)  # the check box is gone (K13.7); the DSN run must not get the snapshot switch
    dialog.on_route(None)
    assert len(ui.snapshots) == 1 and [Path(p) for p in ui.exports] == [work / "b.dsn"]
    args = ui.messages[-1][0].splitlines()
    assert args[args.index("-de") + 1] == str(work / "b.dsn")  # the router never saw the snapshot
    assert "--apply-design-rules" not in args
    recipe = json.loads((work / "b.args.json").read_text(encoding="utf-8"))
    assert "--apply-design-rules" not in recipe["args"]
    log = log_of(work)
    assert "input: dsn (KiCad's DSN export; fallback from kicad_pcb because" in log and "version 20171130" in log
    assert "ignored" not in log  # the DSN run applies the keepouts box
    # the saved choice is the user's, not the fallback: the default, so nothing (a `dsn` would show the fallback leaked)
    assert "board_input" not in json.loads((work / "route-options.json").read_text(encoding="utf-8"))


def test_a_kicad_5_board_result_names_the_dsn_and_why(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route-refuse-kicad", snapshot_text=KICAD_5)
    dialog.on_route(None)
    assert ui.imports == [work / "b.ses"]
    manifest = result(work)
    assert manifest["input"] == "dsn" and "version 20171130" in manifest["input_fallback_reason"]
    text = ui.messages[-1][0]
    assert text.startswith("Routed and imported the result into the board.")
    assert any(l.startswith("Input: dsn (fallback from kicad_pcb: ") for l in text.splitlines())


def test_a_construct_the_router_refuses_exports_the_dsn_and_routes_once_more(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route-refuse-kicad")  # the snapshot looks fine to the pre-check
    dialog.on_route(None)
    assert len(ui.snapshots) == 1 and [Path(p) for p in ui.exports] == [work / "b.dsn"]
    assert ui.imports == [work / "b.ses"]  # the DSN run's session
    manifest = result(work)
    assert manifest["input"] == "dsn"
    assert manifest["input_fallback_reason"] == "line 7, column 3: unsupported construct (test)"
    text = ui.messages[-1][0]
    assert text.startswith("Routed and imported the result into the board.")
    assert "Input: dsn (fallback from kicad_pcb: line 7, column 3: unsupported construct (test))" in text
    assert not any("Routing failed" in m[0] for m in ui.messages)  # no error box for the refused first run
    recipe = json.loads((work / "b.args.json").read_text(encoding="utf-8"))
    assert "--apply-design-rules" not in recipe["args"]
    log = log_of(work)
    assert "input: dsn (KiCad's DSN export; fallback from kicad_pcb because line 7, column 3" in log
    assert "the router refused the snapshot" in log


def test_a_failure_that_is_not_a_load_refusal_is_not_retried(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route-fail")  # "Couldn't load the input file": an I/O problem, not a refusal
    dialog.on_route(None)
    assert ui.exports == [] and ui.imports == []
    assert "Routing failed" in ui.messages[-1][0]


def test_the_dsn_input_chosen_is_recorded_without_a_reason(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route-refuse-kicad")
    dialog.board_input.SetSelection(0)
    dialog.on_route(None)
    assert ui.snapshots == [] and len(ui.exports) == 1
    manifest = result(work)
    assert manifest["input"] == "dsn" and manifest["input_fallback_reason"] is None
    assert "Input: dsn" in ui.messages[-1][0].splitlines()
    assert "input: dsn (KiCad's DSN export, as chosen)" in log_of(work)


def test_fix_existing_locks_are_restored_around_the_fallback(env):
    make_dialog, ui, work = env
    for snapshot_text, mode in ((KICAD_5, "route"), (KICAD_9, "route-refuse-kicad")):
        dialog = make_dialog(mode, snapshot_text=snapshot_text)
        tracks = [stub_ui.FakeTrack("a"), stub_ui.FakeTrack("b", locked=True)]
        dialog.board.tracks = list(tracks)
        dialog.fix_existing.SetValue(True)
        dialog.on_route(None)
        assert [t.IsLocked() for t in tracks] == [False, True]
        assert "fix existing:" in log_of(work)
        assert result(work)["input"] == "dsn"


def test_view_with_a_kicad_5_snapshot_opens_the_dsn(env, monkeypatch):
    make_dialog, ui, work = env
    dialog = make_dialog("echo", snapshot_text=KICAD_5)
    started = []
    monkeypatch.setattr(stub_ui.sys.modules["freerouting_rs.plugin"].tools, "open_viewer", lambda tool, design, ses=None: started.append(design))
    dialog.on_view(None)
    assert started == [work / "b.dsn"]

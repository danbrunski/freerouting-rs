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

    def make_dialog(mode, snapshot_text=KICAD_9):
        text["value"] = snapshot_text
        router = make_fake_router(mode)
        monkeypatch.setattr(plugin.tools, "find_tool", lambda name: plugin.tools.Tool(name, router))
        monkeypatch.setattr(plugin.tools, "query_version", lambda tool, timeout=10: (0, 1, 0, "abc"))
        dialog = plugin.Dialog(None, stub_ui.FakeBoard(pcb))
        dialog.board_input.SetSelection(1)  # Board snapshot (kicad_pcb)
        dialog.keepouts.SetValue(False)  # a DSN rewrite: refused on a snapshot
        return dialog

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
    assert json.loads((work / "route-options.json").read_text(encoding="utf-8"))["board_input"] == "kicad_pcb"


def test_a_kicad_5_board_is_exported_as_a_dsn_without_routing_the_snapshot(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo", snapshot_text=KICAD_5)  # echo: records the one router call
    dialog.apply_rules.SetValue(True)  # a snapshot-only switch; the DSN run must not get it
    dialog.on_route(None)
    assert len(ui.snapshots) == 1 and [Path(p) for p in ui.exports] == [work / "b.dsn"]
    args = ui.messages[-1][0].splitlines()
    assert args[args.index("-de") + 1] == str(work / "b.dsn")  # the router never saw the snapshot
    assert "--apply-design-rules" not in args
    recipe = json.loads((work / "b.args.json").read_text(encoding="utf-8"))
    assert "--apply-design-rules" not in recipe["args"]
    log = log_of(work)
    assert "input: dsn (KiCad's DSN export; fallback from kicad_pcb because" in log and "version 20171130" in log
    # the saved choice is the user's, not the fallback
    assert json.loads((work / "route-options.json").read_text(encoding="utf-8"))["board_input"] == "kicad_pcb"


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
    dialog.apply_rules.SetValue(True)
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

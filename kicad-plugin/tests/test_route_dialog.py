"""The Route flow of plugin.py (K02.1 to K02.4) against the fake router and the
stub wx/pcbnew: options saved and passed, progress shown, Stop keeps, Abort
discards, the summary after the import."""

import json

import pytest
import stub_ui


@pytest.fixture
def env(tmp_path, monkeypatch, make_fake_router, isolated_env):
    plugin, ui = stub_ui.install(monkeypatch)
    pcb = tmp_path / "board dir" / "b.kicad_pcb"
    pcb.parent.mkdir()
    pcb.write_text("", encoding="utf-8")

    def make_dialog(mode):
        router = make_fake_router(mode)
        pkg_tools = plugin.tools
        monkeypatch.setattr(pkg_tools, "find_tool", lambda name: pkg_tools.Tool(name, router))
        monkeypatch.setattr(pkg_tools, "query_version", lambda tool, timeout=10: (0, 1, 0, "abc"))
        return plugin.Dialog(None, stub_ui.FakeBoard(pcb))

    return make_dialog, ui, pcb.parent / "freerouting-rs"


def test_route_imports_and_summarizes(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route")
    dialog.on_route(None)
    assert ui.imports == [work / "b.ses"]
    text = ui.messages[-1][0]
    assert text.startswith("Routed and imported the result into the board.")
    assert "Connections: 10 of 10 routed, 0 incomplete" in text and "WARNING" not in text
    assert (work / "b.result.json").is_file()
    assert "route:" in (work / "plugin.log").read_text(encoding="utf-8")


def test_the_options_are_saved_and_reach_the_router(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo")  # prints its arguments and exits 0 without a session
    dialog.passes.SetValue(12)
    dialog.threads.SetValue(3)
    dialog.classes.SetValue("GND, Power")
    dialog.timeout.SetValue("10m")
    dialog.on_route(None)
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["schema"] == 1 and saved["max_passes"] == 12 and saved["ignore_net_classes"] == ["GND", "Power"]
    # The router failed to write a session ("echo"): the dialog shows its output.
    failure = ui.messages[-1][0]
    assert "Routing failed (exit code 0)" in failure
    for flag in ("--max-passes", "12", "--threads", "3", "--router.autorouter.ignore_net_classes=GND,Power", "--timeout", "10m", "--router.min_trace_width_um=150"):
        assert flag in failure.splitlines(), flag
    assert ui.imports == []
    # A new dialog for the same board restores the saved options.
    again = make_dialog("echo")
    assert (again.passes.GetValue(), again.threads.GetValue(), again.classes.GetValue(), again.timeout.GetValue()) == (
        12, 3, "GND,Power", "10m",
    )  # fmt: skip


def test_an_invalid_field_shows_the_error_and_starts_nothing(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route")
    dialog.threshold.SetValue("lots")
    dialog.on_route(None)
    assert ui.exports == [] and ui.imports == []
    assert "improvement threshold" in ui.messages[-1][0]


def test_stop_keeps_the_best_board(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route-slow")

    def press_stop(dlg, n):
        if dlg.run.state.pass_number is not None and not dlg.run.stopped:
            dlg.on_stop(None)

    ui.on_modal = press_stop
    dialog.on_route(None)
    assert ui.imports == [work / "b.ses"]
    text = ui.messages[-1][0]
    assert text.startswith("Stopped early; imported the best board so far.")
    assert "5 incomplete" in text and "WARNING: 5 connections are not routed" in text


def test_abort_discards_everything(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route-slow")

    def press_abort(dlg, n):
        if dlg.run.state.pass_number is not None:
            dlg.on_abort(None)

    ui.on_modal = press_abort
    dialog.on_route(None)
    assert ui.imports == []
    assert ui.messages[-1][0] == "Routing aborted. The board was not changed."
    assert not (work / "b.ses").exists() and not list(work.glob("*.tmp"))


def test_a_failing_router_shows_its_error_lines(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route-fail")
    dialog.on_route(None)
    assert ui.imports == []
    assert "Routing failed (exit code 1)" in ui.messages[-1][0]
    assert "[error] Couldn't load the input file" in ui.messages[-1][0]


def test_the_progress_dialog_shows_figures_and_the_log(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route-noise")
    dialog.on_route(None)
    shown = ui.dialogs[0]
    assert shown.status.label.startswith("Finished (completed)")
    assert "a plain line from the router" in shown.log.GetValue()


def test_a_stale_session_is_removed_before_routing(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route-fail")
    work.mkdir(parents=True, exist_ok=True)
    (work / "b.ses").write_text("(session stale)", encoding="utf-8")
    dialog.on_route(None)
    assert not (work / "b.ses").exists() and ui.imports == []

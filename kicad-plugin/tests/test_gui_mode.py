"""Route in GUI (K02.5), with the fake GUI executable and the stub wx/pcbnew
(stub_ui.py): the command line, the wait dialog, and whether the session is
imported."""

import json

import pytest
import stub_ui


@pytest.fixture
def env(tmp_path, monkeypatch, make_fake_gui, isolated_env):
    plugin, ui = stub_ui.install(monkeypatch)
    pcb = tmp_path / "board dir" / "b.kicad_pcb"
    pcb.parent.mkdir()
    pcb.write_text("", encoding="utf-8")

    def make_dialog(mode):
        gui = make_fake_gui(mode)
        pkg_tools = plugin.tools  # freerouting_rs.tools, not the top-level test import
        monkeypatch.setattr(pkg_tools, "find_tool", lambda name: pkg_tools.Tool(name, gui))
        monkeypatch.setattr(pkg_tools, "query_version", lambda tool, timeout=10: (0, 1, 0, "abc"))
        dialog = plugin.Dialog(None, stub_ui.FakeBoard(pcb))
        dialog.board_input.SetSelection(0)  # these tests mean the DSN; the default is the snapshot (K13.7)
        return dialog

    return make_dialog, ui, pcb.parent / "freerouting-rs"


def test_a_gui_that_writes_the_session_is_imported(env):
    make_dialog, ui, work = env
    dialog = make_dialog("write")
    dialog.on_route_gui(None)
    # The command line is `freerouting-gui -de x.dsn -do x.ses` plus the route options.
    argv = json.loads((work / "b.ses.argv").read_text(encoding="utf-8"))
    assert argv == ["-de", str(work / "b.dsn"), "-do", str(work / "b.ses"), "--router.min_trace_width_um=150",
                    "--router.board_minimums.track_width_um=150"]  # fmt: skip
    # The import step was reached, on the session the GUI wrote.
    assert ui.imports == [work / "b.ses"]
    assert "imported the result" in ui.messages[-1][0]
    assert (work / "route-options.json").is_file()  # the options were saved with the run


def test_the_gui_gets_the_options_but_not_the_seed(env):
    make_dialog, ui, work = env
    dialog = make_dialog("write")
    dialog.passes.SetValue(7)
    dialog.threads.SetValue(2)
    dialog.seed.SetValue(0)
    dialog.optimizer.SetValue(False)
    dialog.on_route_gui(None)
    argv = json.loads((work / "b.ses.argv").read_text(encoding="utf-8"))
    assert argv[4:] == [
        "--max-passes", "7", "--threads", "2", "--router.optimizer.enabled=false", "--router.min_trace_width_um=150",
        "--router.board_minimums.track_width_um=150",
    ]  # fmt: skip


def test_a_gui_that_exits_without_a_session_imports_nothing(env):
    make_dialog, ui, work = env
    dialog = make_dialog("nowrite")
    dialog.on_route_gui(None)
    assert ui.imports == []
    assert "without writing b.ses" in ui.messages[-1][0]


def test_terminate_kills_the_gui_and_imports_nothing(env):
    make_dialog, ui, work = env
    dialog = make_dialog("hang")
    ui.on_modal = lambda dlg, n: dlg.on_terminate(None) if n == 3 else None
    dialog.on_route_gui(None)
    assert ui.imports == []
    assert ui.messages[-1][0] == "Terminated; the board was not changed."
    assert not (work / "b.ses").exists()


def test_the_wait_dialog_carries_the_java_plugins_text(env):
    make_dialog, ui, work = env
    dialog = make_dialog("write")
    dialog.on_route_gui(None)
    from freerouting_rs import routing  # the module the dialog used

    assert routing.GuiRun.MESSAGE == (
        "Freerouting GUI is running...\nComplete routing and close the window,\nor press Terminate to cancel."
    )


def test_a_failed_import_is_reported(env):
    make_dialog, ui, work = env
    dialog = make_dialog("write")
    ui.import_ok = False
    dialog.on_route_gui(None)
    assert ui.imports and "could not import" in ui.messages[-1][0]


def test_an_invalid_option_stops_before_anything_starts(env):
    make_dialog, ui, work = env
    dialog = make_dialog("write")
    dialog.timeout.SetValue("whenever")
    dialog.on_route_gui(None)
    assert ui.exports == [] and ui.imports == []
    assert "timeout" in ui.messages[-1][0]

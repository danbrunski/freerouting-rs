"""The Route flow of plugin.py (K02.1 to K02.4) against the fake router and the
stub wx/pcbnew: options saved and passed, progress shown, Stop keeps, Abort
discards, the summary after the import."""

import json
import sys

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
    assert saved["schema"] == 3 and saved["profile"] == "enhanced" and saved["max_passes"] == 12 and saved["ignore_net_classes"] == ["GND", "Power"]
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


def test_the_profile_and_the_advanced_stages_reach_the_router(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo")
    # The defaults: Enhanced, each stage at the profile's choice.
    assert dialog.profile.GetStringSelection() == "Enhanced"
    assert sorted(dialog.stage_choices) == ["autorouter.connection_router", "special.length_matching"]
    assert dialog.chosen_stages() == {"autorouter.connection_router": "freerouting-connection", "special.length_matching": ""}
    dialog.profile.SetStringSelection("Legacy")
    choice, ids = dialog.stage_choices["autorouter.connection_router"]
    choice.SetSelection(ids.index("x-grid"))
    dialog.on_route(None)
    lines = ui.messages[-1][0].splitlines()
    for flag in ("--profile", "legacy", "--router.autorouter.connection_router=x-grid"):
        assert flag in lines, flag
    assert not any(l.startswith("--router.special.length_matching") for l in lines)
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["profile"] == "legacy"
    assert saved["stages"] == {"autorouter.connection_router": "x-grid"}
    # Restored by a new dialog.
    again = make_dialog("echo")
    assert again.profile.GetStringSelection() == "Legacy"
    choice, ids = again.stage_choices["autorouter.connection_router"]
    assert ids[choice.GetSelection()] == "x-grid"
    # Back at the profile's choice, nothing is kept.
    choice.SetSelection(ids.index("freerouting-connection"))
    again.on_route(None)
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert "stages" not in saved


@pytest.mark.parametrize("label", ["Legacy", "Enhanced"])
def test_the_summary_names_the_profile(env, label):
    make_dialog, ui, work = env
    dialog = make_dialog("route")
    dialog.profile.SetStringSelection(label)
    dialog.on_route(None)
    assert f"Profile: {label}" in ui.messages[-1][0].splitlines()


def test_a_router_without_a_stage_listing_has_no_choices(env):
    make_dialog, _, _ = env
    dialog = make_dialog("fail")  # `--list-stages=json` exits 3
    assert dialog.stage_choices == {}
    assert dialog.chosen_stages() == {}


def test_every_drop_down_gets_the_dark_list_fix(env, monkeypatch):
    make_dialog, _, _ = env
    passed = []
    dropdowns = sys.modules["freerouting_rs.dropdowns"]
    monkeypatch.setattr(dropdowns, "fix_dark_lists", lambda choices: passed.append(list(choices)))
    dialog = make_dialog("echo")
    stages = [choice for choice, _ in dialog.stage_choices.values()]
    assert len(stages) == 2
    assert passed == [[dialog.profile, dialog.pour_mode, dialog.sweep] + stages]


def test_the_import_deletes_no_track_kicad_still_holds(env, tmp_path):
    """KiCad's action-plugin runner snapshots every board item before Run and,
    after it, commits the missing ones as removed. ImportSpecctraSES deletes the
    tracks it replaces, so those pointers dangled: wx's "Unhandled exception"
    box on closing the dialog. The plugin detaches the unlocked tracks first, so
    the importer deletes none and the runner records them as removed (Undo
    restores them)."""
    _make_dialog, ui, _work = env
    plugin = sys.modules["freerouting_rs.plugin"]
    old, kept = stub_ui.FakeTrack("old"), stub_ui.FakeTrack("locked", locked=True)
    board = stub_ui.FakeBoard(tmp_path / "x.kicad_pcb", tracks=[old, kept])
    assert plugin.import_ses(board, tmp_path / "x.ses")
    assert not old.freed and not kept.freed
    assert old not in board.tracks and kept in board.tracks
    assert [t.name for t in board.tracks if t not in (old, kept)] == ["routed"]


def test_a_failed_import_puts_the_tracks_back(env, tmp_path):
    _make_dialog, ui, _work = env
    plugin = sys.modules["freerouting_rs.plugin"]
    ui.import_ok = False
    old = stub_ui.FakeTrack("old")
    board = stub_ui.FakeBoard(tmp_path / "x.kicad_pcb", tracks=[old])
    assert not plugin.import_ses(board, tmp_path / "x.ses")
    assert board.tracks == [old] and not old.freed


def test_the_canary_preset_fills_the_fields_and_reaches_the_router(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo")
    dialog.on_preset(None)
    dialog.on_route(None)
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["class_widths"] == {"GND": 0.4, "Power": 0.3, "Power_High": 0.3, "Switch_Node": 0.3}
    assert saved["fixed_pour_classes"] == ["Power_High", "Switch_Node"] and saved["drop_pour_nets"] == ["GND"]
    lines = ui.messages[-1][0].splitlines()
    for flag in (
        "--max-passes",
        "1",
        "--router.min_trace_width_um=150",
        "--router.scoring.start_ripup_costs=1500",
        "--router.autorouter.sweep=free",
        "--router.autorouter.sweep_passes=2",
    ):
        assert flag in lines, flag
    # The stub board's DSN has none of canary's classes: the fix-up says so and routing goes on.
    log = (work / "plugin.log").read_text(encoding="utf-8")
    assert "WARNING: DSN fix-ups skipped: ValueError('no class GND with a width in the DSN')" in log


def plugin_pcbnew():
    return sys.modules["pcbnew"]


def test_fix_existing_keeps_the_tracks_locked_through_the_import_only(env, monkeypatch):
    make_dialog, ui, work = env
    dialog = make_dialog("route")
    tracks = [stub_ui.FakeTrack("a"), stub_ui.FakeTrack("b", locked=True), stub_ui.FakeTrack("c")]
    dialog.board.tracks = list(tracks)
    seen = []
    real_import = plugin_pcbnew().ImportSpecctraSES

    def import_ses(board, path):
        seen.append([t.IsLocked() for t in tracks])
        return real_import(board, path)

    monkeypatch.setattr(plugin_pcbnew(), "ImportSpecctraSES", import_ses)
    dialog.fix_existing.SetValue(True)
    dialog.on_route(None)
    assert seen == [[True, True, True]]  # KiCad's import keeps locked tracks
    assert [t.IsLocked() for t in tracks] == [False, True, False]  # and the lock states come back
    assert all(t in dialog.board.tracks and not t.freed for t in tracks)  # the import deleted none
    assert "fix existing: 2 track(s) and via(s) exported as fixed" in (work / "plugin.log").read_text(encoding="utf-8")


def test_the_winning_pass_is_remembered_and_offered(env):
    make_dialog, ui, work = env
    dialog = make_dialog("route")
    dialog.on_route(None)
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["pass_hint"] == dialog.saved_pass_hint and saved["pass_hint"] >= 1
    again = make_dialog("echo")
    again.pass_hint.SetValue(True)
    again.on_route(None)
    lines = ui.messages[-1][0].splitlines()
    assert lines[lines.index("--max-passes") + 1] == str(saved["pass_hint"])

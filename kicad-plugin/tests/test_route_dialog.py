"""The Route flow of plugin.py (K02.1 to K02.4) against the fake router and the
stub wx/pcbnew: options saved and passed, progress shown, Stop keeps, Abort
discards, the summary after the import."""

import hashlib
import json
import sys
from pathlib import Path

import pytest
import stub_ui


@pytest.fixture
def env(tmp_path, monkeypatch, make_fake_router, isolated_env):
    plugin, ui = stub_ui.install(monkeypatch)
    pcb = tmp_path / "board dir" / "b.kicad_pcb"
    pcb.parent.mkdir()
    pcb.write_text("", encoding="utf-8")

    def make_dialog(mode, snapshot=False):
        router = make_fake_router(mode)
        pkg_tools = plugin.tools
        monkeypatch.setattr(pkg_tools, "find_tool", lambda name: pkg_tools.Tool(name, router))
        monkeypatch.setattr(pkg_tools, "query_version", lambda tool, timeout=10: (0, 1, 0, "abc"))
        dialog = plugin.Dialog(None, stub_ui.FakeBoard(pcb))
        if not snapshot:  # the DSN tests: the default input is the snapshot (K13.7)
            dialog.board_input.SetSelection(0)
        return dialog

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


def test_route_uses_and_records_the_current_board_rules(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo")
    dialog.board.min_track_nm = 225000
    dialog.on_route(None)
    recipe = json.loads((work / "b.args.json").read_text(encoding="utf-8"))
    # K15.44: Board Setup's minimum as Board Setup's, which a `.kicad_dru` rule replaces
    assert "--board-setup.router.board_minimums.track_width_um=225" in recipe["args"]
    assert "--board-setup.router.board_minimums.track_width_um=225" in ui.messages[-1][0].splitlines()
    assert recipe["input_sha256"] == hashlib.sha256((work / "b.dsn").read_bytes()).hexdigest()


def test_the_options_are_saved_and_reach_the_router(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo")  # prints its arguments and exits 0 without a session
    dialog.passes.SetValue(12)
    dialog.threads.SetValue(3)
    dialog.classes.SetValue("GND, Power")
    dialog.timeout.SetValue("10m")
    dialog.on_route(None)
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["schema"] == 6 and saved["profile"] == "enhanced" and saved["max_passes"] == 12 and saved["ignore_net_classes"] == ["GND", "Power"]
    # The router failed to write a session ("echo"): the dialog shows its output.
    failure = ui.messages[-1][0]
    assert "Routing failed (exit code 0)" in failure
    for flag in ("--max-passes", "12", "--threads", "3", "--router.autorouter.ignore_net_classes=GND,Power", "--timeout", "10m", "--board-setup.router.board_minimums.track_width_um=150"):
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
    assert passed == [[dialog.profile, dialog.board_input, dialog.preset_choice, dialog.pour_mode, dialog.sweep] + stages]


def test_the_options_sit_on_three_tabs_and_the_buttons_stay_outside(env):
    """The dialog had grown taller than the screen, hiding Route in GUI and
    the other buttons; the options now sit on tabs and the buttons below them."""
    make_dialog, ui, _ = env
    dialog = make_dialog("echo")
    route, board, advanced = dialog.pages
    notebook = route.parent
    assert [(page, title) for page, title in notebook.pages] == [(route, "Route"), (board, "Board and DSN"), (advanced, "Advanced")]
    assert notebook.parent is dialog
    on_route = [dialog.profile, dialog.passes, dialog.threads, dialog.optimizer, dialog.threshold, dialog.classes, dialog.timeout, dialog.seed, dialog.min_width]
    on_board = [dialog.board_input, dialog.fix_areas, dialog.widths, dialog.fixed_pours, dialog.pour_mode, dialog.drop_pours, dialog.keepouts, dialog.open_layers, dialog.fix_existing]
    on_advanced = [dialog.ripup, dialog.sweep, dialog.sweep_passes, dialog.pass_hint] + [c for c, _ in dialog.stage_choices.values()]
    assert [page_of(c, dialog) for c in on_route] == [route] * len(on_route)
    assert [page_of(c, dialog) for c in on_board] == [board] * len(on_board)
    assert [page_of(c, dialog) for c in on_advanced] == [advanced] * len(on_advanced)
    # The fix-up, flow and stage controls are children of their group's box.
    assert {dialog.widths.parent.label, dialog.ripup.parent.label, next(iter(dialog.stage_choices.values()))[0].parent.label} == {"Fix-ups", "Flow", "Stages"}
    assert [b.label for b in ui.buttons if b.parent is dialog] == ["Route", "Route in GUI", "Open in viewer", "Import SES...", "Close"]
    assert [(b.label, b.parent.label) for b in ui.buttons if b.parent is not dialog] == [("Apply", "Presets"), ("Save as preset...", "Presets")]
    assert all(page_of(b, dialog) is None for b in ui.buttons)


def page_of(control, dialog):
    """The tab page `control` sits on, or None when it is outside the tabs."""
    window = control.parent
    while window is not None and window is not dialog:
        if window in dialog.pages:
            return window
        window = window.parent
    return None


def test_each_page_asks_for_its_contents_when_the_dialog_fits(env):
    make_dialog, ui, _ = env
    ui.page_size, ui.dialog_size, ui.work_area_height = (400, 500), (700, 800), 1040
    dialog = make_dialog("echo")
    assert [page.min_size for page in dialog.pages] == [stub_ui.Size(400, 500)] * 3


def test_every_tab_asks_for_its_whole_contents_where_the_screen_has_room(env):
    """The notebook takes the longest tab's height (the Advanced stage list),
    so no tab scrolls on a screen that has the room (owner)."""
    make_dialog, ui, _ = env
    ui.page_sizes = {"Route": (400, 340), "Board and DSN": (420, 380), "Advanced": (450, 640)}
    dialog = make_dialog("echo")
    assert [page.min_size for page in dialog.pages] == [stub_ui.Size(400, 340), stub_ui.Size(420, 380), stub_ui.Size(450, 640)]


def test_a_dialog_taller_than_the_screen_cuts_its_pages_so_they_scroll(env):
    """Fitted at 1300 on a 1000-pixel work area (900 allowed), every page loses
    the 400 excess and gets room for its scroll bar; its virtual size stays its
    contents, so it scrolls."""
    make_dialog, ui, _ = env
    ui.page_size, ui.dialog_size, ui.work_area_height = (400, 700), (700, 1300), 1000
    dialog = make_dialog("echo")
    assert [page.min_size for page in dialog.pages] == [stub_ui.Size(417, 300)] * 3
    assert [page.virtual_size for page in dialog.pages] == [stub_ui.Size(400, 700)] * 3


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


def test_a_board_preset_fills_the_fields_and_reaches_the_router(env):
    """The canary-pwr preset file, copied into the board's presets folder, appears in the
    Preset drop-down; Apply fills the fields (K09.8 values) and Route passes them."""
    import shutil

    from conftest import CANARY_PRESET_FILE

    make_dialog, ui, work = env
    (work / "presets").mkdir(parents=True)
    shutil.copy(CANARY_PRESET_FILE, work / "presets")
    dialog = make_dialog("echo")
    assert dialog.preset_choice.choices == ["canary graded flow (board)"]
    dialog.on_apply_preset(None)
    assert dialog.widths.GetValue() == "GND=0.4, Power=0.3, Power_High=0.3, Switch_Node=0.3"
    assert dialog.passes.GetValue() == 1 and dialog.sweep.GetStringSelection() == "free"
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


def test_without_presets_the_drop_down_says_so_and_apply_does_nothing(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo")
    assert dialog.preset_choice.choices == ["(no presets)"] and dialog.presets == []
    dialog.passes.SetValue(7)
    dialog.on_apply_preset(None)
    assert dialog.passes.GetValue() == 7


def test_applying_a_preset_keeps_the_fields_it_does_not_hold(env):
    make_dialog, ui, work = env
    write_preset(work, "p.json", {"schema": 4, "name": "p", "max_passes": 3})
    dialog = make_dialog("echo")
    dialog.threads.SetValue(6)
    dialog.classes.SetValue("GND")
    dialog.on_apply_preset(None)
    assert (dialog.passes.GetValue(), dialog.threads.GetValue(), dialog.classes.GetValue()) == (3, 6, "GND")


def test_a_malformed_preset_file_is_logged_and_the_dialog_still_opens(env):
    make_dialog, ui, work = env
    write_preset(work, "bad.json", "{oops")
    write_preset(work, "ok.json", {"schema": 4, "name": "ok", "max_passes": 2})
    dialog = make_dialog("echo")
    assert dialog.preset_choice.choices == ["ok (board)"]
    assert "WARNING: preset bad.json skipped: not JSON" in (work / "plugin.log").read_text(encoding="utf-8")


def test_save_as_preset_writes_the_fields_and_lists_it(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo")
    dialog.passes.SetValue(4)
    dialog.sweep.SetSelection(2)  # free
    dialog.ask_preset_name = lambda: "My flow"
    dialog.on_save_preset(None)
    path = work / "presets" / "My-flow.json"
    stored = json.loads(path.read_text(encoding="utf-8"))
    assert stored["name"] == "My flow" and stored["max_passes"] == 4 and stored["sweep"] == "free"
    assert dialog.preset_choice.choices == ["My flow (board)"] and dialog.preset_choice.GetSelection() == 0
    # a fresh dialog lists it; applying it on changed fields restores them
    again = make_dialog("echo")
    again.passes.SetValue(1)
    again.on_apply_preset(None)
    assert again.passes.GetValue() == 4 and again.sweep.GetStringSelection() == "free"
    # cancelling the name dialog writes nothing more
    dialog.ask_preset_name = lambda: None
    dialog.on_save_preset(None)
    assert len(list((work / "presets").glob("*.json"))) == 1


def write_preset(work, name, data):
    (work / "presets").mkdir(parents=True, exist_ok=True)
    text = data if isinstance(data, str) else json.dumps(data)
    (work / "presets" / name).write_text(text, encoding="utf-8")


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


# --- K10: the board snapshot as the router's input -----------------------------


def snapshot_dialog(make_dialog, mode):
    """The dialog on the board snapshot, the default input (K13.7), with the
    keepouts box on: the snapshot ignores it."""
    dialog = make_dialog(mode, snapshot=True)
    assert dialog.board_input.GetSelection() == 1
    return dialog


def test_route_with_the_snapshot_gives_the_router_the_kicad_pcb(env):
    make_dialog, ui, work = env
    dialog = snapshot_dialog(make_dialog, "echo")
    dialog.on_route(None)
    assert ui.snapshots == [(work / "b.kicad_pcb", True)]  # SaveBoard(path, board, aSkipSettings=True)
    assert ui.exports == []  # no DSN export
    assert ui.project_copies == [work / "b.kicad_pro"]
    assert not (work / "b.kicad_prl").exists()
    lines = ui.messages[-1][0].splitlines()
    assert lines[lines.index("-de") + 1] == str(work / "b.kicad_pcb")
    assert lines.count("--apply-design-rules") == 1
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    # K13.7: the snapshot is the default input, so it is not written; the `apply_kicad_rules` option and its check box are gone
    assert "board_input" not in saved and "apply_kicad_rules" not in saved
    assert "apply_rules" not in vars(dialog)
    log = (work / "plugin.log").read_text(encoding="utf-8")
    assert "rule-area fix-up not applied" in log and "snapshot: b.kicad_pcb" in log


def test_the_rules_switch_is_passed_on_a_snapshot_route(env):
    # K13.1: the board's rules reach the router on both inputs.
    make_dialog, ui, work = env
    dialog = snapshot_dialog(make_dialog, "echo")
    dialog.on_route(None)
    assert "--apply-design-rules" in ui.messages[-1][0].splitlines()


def test_the_dsn_choice_exports_the_dsn(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo")
    dialog.on_route(None)
    assert [Path(p) for p in ui.exports] == [work / "b.dsn"] and ui.snapshots == []
    lines = ui.messages[-1][0].splitlines()
    assert lines[lines.index("-de") + 1] == str(work / "b.dsn")


def test_the_snapshot_result_comes_back_through_the_detaching_import(env):
    make_dialog, ui, work = env
    dialog = snapshot_dialog(make_dialog, "route")
    old, kept = stub_ui.FakeTrack("old"), stub_ui.FakeTrack("locked", locked=True)
    dialog.board.tracks = [old, kept]
    dialog.on_route(None)
    assert ui.imports == [work / "b.ses"]  # the session is named as on the DSN path
    assert not old.freed and not kept.freed  # 9bc7ee6: the import deleted nothing KiCad holds
    assert old not in dialog.board.tracks and kept in dialog.board.tracks
    assert ui.messages[-1][0].startswith("Routed and imported the result into the board.")


def test_a_snapshot_route_passes_the_fixed_pour_classes_to_the_router(env):
    """K14.4: the fixed pour wires route on the snapshot too, as a
    `router.input_fixups.*` flag; the keepouts option is on by default and is
    only ignored (K13.7)."""
    make_dialog, ui, work = env
    dialog = snapshot_dialog(make_dialog, "echo")
    dialog.fixed_pours.SetValue("Power")
    dialog.on_route(None)
    assert ui.snapshots == [(work / "b.kicad_pcb", True)] and ui.exports == []
    assert "--router.input_fixups.fixed_pour_classes=Power" in ui.messages[-1][0].splitlines()


def test_a_snapshot_route_passes_the_reader_fixups_to_the_router(env):
    """K14.1-K14.3: class widths, dropped pours and opened plane layers route on
    the snapshot, as `router.input_fixups.*` flags."""
    make_dialog, ui, work = env
    dialog = snapshot_dialog(make_dialog, "echo")
    dialog.widths.SetValue("GND=0.4")
    dialog.drop_pours.SetValue("GND")
    dialog.on_route(None)
    assert ui.snapshots == [(work / "b.kicad_pcb", True)] and ui.exports == []
    lines = ui.messages[-1][0].splitlines()
    assert "--router.input_fixups.class_widths=GND=400" in lines
    assert "--router.input_fixups.drop_pour_nets=GND" in lines


def test_a_failed_snapshot_says_so_and_routes_nothing(env, monkeypatch):
    make_dialog, ui, work = env
    dialog = snapshot_dialog(make_dialog, "route")
    monkeypatch.setattr(plugin_pcbnew(), "SaveBoard", lambda *a: False)
    dialog.on_route(None)
    assert ui.imports == []
    assert "could not save a snapshot" in ui.messages[-1][0]


def test_route_in_gui_and_the_viewer_take_the_snapshot_too(env, monkeypatch):
    make_dialog, ui, work = env
    dialog = snapshot_dialog(make_dialog, "echo")
    started = []
    plugin = sys.modules["freerouting_rs.plugin"]
    monkeypatch.setattr(plugin.tools, "open_viewer", lambda tool, design, ses=None: started.append((design, ses)))
    dialog.on_view(None)
    assert started == [(work / "b.kicad_pcb", None)] and ui.snapshots == [(work / "b.kicad_pcb", True)]
    # Route in GUI: the snapshot is the design; the GUI has no rules switch, so none is passed.
    seen = []

    class Run:
        MESSAGE = ""
        finished = True

        def __init__(self, tool, design, ses, args):
            seen.append((design, list(args)))

        def start(self):
            pass

        def should_import(self):
            return False

        def outcome_text(self):
            return "x"

    monkeypatch.setattr(plugin.routing, "GuiRun", Run)
    dialog.on_route_gui(None)
    assert seen[0][0] == work / "b.kicad_pcb" and "--apply-design-rules" not in seen[0][1]
    assert "Route in GUI reads the .kicad_dru but does not apply it" in (work / "plugin.log").read_text(encoding="utf-8")


def test_fix_existing_locks_the_tracks_into_the_snapshot(env):
    make_dialog, ui, work = env
    dialog = snapshot_dialog(make_dialog, "route")
    tracks = [stub_ui.FakeTrack("a"), stub_ui.FakeTrack("b", locked=True)]
    dialog.board.tracks = list(tracks)
    dialog.fix_existing.SetValue(True)
    states = []
    real = plugin_pcbnew().SaveBoard

    def save(path, board, skip=False):
        states.append([t.IsLocked() for t in tracks])
        return real(path, board, skip)

    plugin_pcbnew().SaveBoard = save
    dialog.on_route(None)
    assert states == [[True, True]]  # written as fixed copper (the reader: type fix)
    assert [t.IsLocked() for t in tracks] == [False, True]  # and restored afterwards


def test_the_pin_escape_checkbox_is_saved_and_reaches_the_router(env):
    make_dialog, ui, work = env
    dialog = make_dialog("echo")
    assert dialog.pin_escape.GetValue() is False
    assert page_of(dialog.pin_escape, dialog) is dialog.pages[2]  # Advanced
    assert dialog.pin_escape.parent.label == "Flow"
    dialog.on_route(None)
    assert "--router.fanout.pin_escape=straight" not in ui.messages[-1][0].splitlines()
    dialog.pin_escape.SetValue(True)
    dialog.on_route(None)
    assert "--router.fanout.pin_escape=straight" in ui.messages[-1][0].splitlines()
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["schema"] == 6 and saved["pin_escape"] is True
    again = make_dialog("echo")
    assert again.pin_escape.GetValue() is True


def test_the_all_pours_checkbox_is_saved_and_reaches_the_router(env):
    make_dialog, ui, work = env
    flag = "--router.plane_obstacle_all_pours=true"
    dialog = make_dialog("echo")
    assert dialog.plane_obstacle_all_pours.GetValue() is False
    assert page_of(dialog.plane_obstacle_all_pours, dialog) is dialog.pages[2]  # Advanced
    assert dialog.plane_obstacle_all_pours.parent.label == "Flow"
    tip = dialog.plane_obstacle_all_pours.tooltip
    assert "pour" in tip and "47 -> 20" in tip and "5 -> 83" in tip
    dialog.on_route(None)
    assert flag not in ui.messages[-1][0].splitlines()
    dialog.plane_obstacle_all_pours.SetValue(True)
    dialog.on_route(None)
    assert flag in ui.messages[-1][0].splitlines()
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["schema"] == 6 and saved["plane_obstacle_all_pours"] is True
    again = make_dialog("echo")
    assert again.plane_obstacle_all_pours.GetValue() is True


def test_the_fill_target_checkbox_is_saved_and_reaches_the_router(env):
    make_dialog, ui, work = env
    flag = "--router.pour_fill_target=true"
    dialog = make_dialog("echo")
    assert dialog.pour_fill_target.GetValue() is False
    assert page_of(dialog.pour_fill_target, dialog) is dialog.pages[2]  # Advanced
    assert dialog.pour_fill_target.parent.label == "Flow"
    assert dialog.pour_fill_target.parent is dialog.plane_obstacle_all_pours.parent
    tip = dialog.pour_fill_target.tooltip
    assert "pour" in tip and "40 -> 31" in tip and "47 -> 49" in tip and "board snapshot" in tip
    dialog.on_route(None)
    assert flag not in ui.messages[-1][0].splitlines()
    dialog.pour_fill_target.SetValue(True)
    dialog.on_route(None)
    assert flag in ui.messages[-1][0].splitlines()
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["schema"] == 6 and saved["pour_fill_target"] is True
    again = make_dialog("echo")
    assert again.pour_fill_target.GetValue() is True


def test_the_fill_contacts_checkbox_is_saved_and_reaches_the_router(env):
    make_dialog, ui, work = env
    flag = "--router.pour_fill_contacts=true"
    dialog = make_dialog("echo")
    assert dialog.pour_fill_contacts.GetValue() is False
    assert page_of(dialog.pour_fill_contacts, dialog) is dialog.pages[2]  # Advanced
    assert dialog.pour_fill_contacts.parent.label == "Flow"
    assert dialog.pour_fill_contacts.parent is dialog.pour_fill_target.parent
    assert dialog.pour_fill_contacts.label == "Count each pour island as its own connection"
    tip = dialog.pour_fill_contacts.tooltip
    assert "End traces where KiCad fills a pour" in tip
    assert "338 -> 272" in tip and "47 -> 20" in tip and "40 -> 26" in tip and "12 -> 27" in tip
    assert "board snapshot" in tip
    dialog.on_route(None)
    assert flag not in ui.messages[-1][0].splitlines()
    dialog.pour_fill_contacts.SetValue(True)
    dialog.on_route(None)
    assert flag in ui.messages[-1][0].splitlines()
    saved = json.loads((work / "route-options.json").read_text(encoding="utf-8"))
    assert saved["schema"] == 6 and saved["pour_fill_contacts"] is True
    again = make_dialog("echo")
    assert again.pour_fill_contacts.GetValue() is True

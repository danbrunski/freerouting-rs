"""KiCad PCB editor action plugin for freerouting-rs.

Exports the open board to Specctra DSN, routes it with the freerouting-rs
command line (live progress, Stop and keep, Abort, a result summary) or in
the GUI window ("Route in GUI"), imports the resulting SES session, and opens
boards in the freerouting-rs viewer. Files go in a `freerouting-rs` folder
next to the .kicad_pcb file. The route options are saved per board in
`freerouting-rs/route-options.json` (options.py).

Every pcbnew and wx call happens here, on KiCad's UI thread. The router's
output is read by one thread (progress.pump_lines, started by
routing.RouteRun) that only fills a queue; the dialogs' timers drain it.
"""

import time
from pathlib import Path

import pcbnew
import wx

from . import kicad_areas, options, progress, routing, rule_areas, tools, version

TITLE = "freerouting-rs"
TICK_MS = 100


def export_dsn(board, dsn):
    dsn.parent.mkdir(parents=True, exist_ok=True)
    try:
        ok = pcbnew.ExportSpecctraDSN(board, str(dsn))
    except TypeError:  # older KiCad: only the current board
        ok = pcbnew.ExportSpecctraDSN(str(dsn))
    return bool(ok) and dsn.is_file()


def import_ses(board, ses):
    try:
        ok = pcbnew.ImportSpecctraSES(board, str(ses))
    except TypeError:
        ok = pcbnew.ImportSpecctraSES(str(ses))
    if ok:
        if hasattr(pcbnew, "UpdateUserInterface"):
            pcbnew.UpdateUserInterface()
        pcbnew.Refresh()
    return bool(ok)


def board_min_track_width_um(board):
    """The Board Setup minimum track width in micrometres, or None. The DSN
    does not carry it (D8), so it is passed to the router explicitly."""
    try:
        nm = board.GetDesignSettings().m_TrackMinWidth
        return nm / 1000.0 if nm and nm > 0 else None
    except Exception:  # an older KiCad without that attribute
        return None


def fix_rule_areas(board, dsn, work_dir):
    """Rewrites the rule-area keepouts in the exported DSN (see rule_areas).
    pcbnew is read here, on the UI thread. Returns the log lines; a failure is
    logged and leaves the DSN as KiCad wrote it."""
    try:
        areas = kicad_areas.collect(board, pcbnew)
        # open(), not Path.read_text/write_text(newline=...): read_text has no `newline`
        # before Python 3.13, and KiCad 10 ships 3.12 (found in K03.6 on KiCad 10.0.7).
        with open(dsn, encoding="utf-8", newline="") as f:
            text = f.read()
        new_text, lines = rule_areas.rewrite(text, areas)
        if new_text != text:
            with open(dsn, "w", encoding="utf-8", newline="") as f:
                f.write(new_text)
    except Exception as e:  # never block routing on the fix-up
        lines = [f"WARNING: rule-area fix-up skipped: {e!r}"]
    stamp = time.strftime("%Y-%m-%d %H:%M:%S")
    for line in lines or ["no rule areas on the board"]:
        tools.log_line(work_dir, f"{stamp} {dsn.name}: {line}")
    return lines


class RouteProgressDialog(wx.Dialog):
    """Shows a running `routing.RouteRun`: the figures, the router's log, Stop
    and Abort. A timer on the UI thread drains the reader's queue. The dialog
    ends itself when the run has finished."""

    def __init__(self, parent, run):
        super().__init__(parent, title=TITLE + ": routing", style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        self.run = run
        self.status = wx.StaticText(self, label="Starting...")
        self.gauge = wx.Gauge(self, range=100, style=wx.GA_HORIZONTAL)
        self.log = wx.TextCtrl(self, style=wx.TE_MULTILINE | wx.TE_READONLY | wx.HSCROLL, size=(560, 220))
        self.stop = wx.Button(self, label="Stop and keep result")
        self.abort = wx.Button(self, label="Abort")
        self.stop.SetToolTip("Finish the current step, write the best board so far and import it")
        self.abort.SetToolTip("Kill the router and discard the run")
        self.stop.Bind(wx.EVT_BUTTON, self.on_stop)
        self.abort.Bind(wx.EVT_BUTTON, self.on_abort)
        self.Bind(wx.EVT_CLOSE, self.on_abort)
        buttons = wx.BoxSizer(wx.HORIZONTAL)
        buttons.Add(self.stop, 0, wx.RIGHT, 6)
        buttons.Add(self.abort)
        sizer = wx.BoxSizer(wx.VERTICAL)
        sizer.Add(self.status, 0, wx.ALL | wx.EXPAND, 12)
        sizer.Add(self.gauge, 0, wx.LEFT | wx.RIGHT | wx.EXPAND, 12)
        sizer.Add(self.log, 1, wx.ALL | wx.EXPAND, 12)
        sizer.Add(buttons, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.ALIGN_RIGHT, 12)
        self.SetSizerAndFit(sizer)
        self.CentreOnParent()
        self.timer = wx.Timer(self)
        self.Bind(wx.EVT_TIMER, self.on_tick, self.timer)
        self.timer.Start(TICK_MS)

    def append_log(self, text):
        self.log.AppendText(text + "\n")

    def on_tick(self, _evt):
        for kind, payload in self.run.poll():
            if kind == "log":
                self.append_log(payload)
            elif payload.get("event") == "log" and payload.get("level") != "debug":
                self.append_log(progress.format_log_event(payload))
        self.status.SetLabel(self.run.state.text())
        self.gauge.Pulse()
        if self.run.finished:
            self.timer.Stop()
            self.EndModal(wx.ID_OK)

    def on_stop(self, _evt):
        self.run.stop()
        self.stop.Disable()
        self.status.SetLabel("Stopping: finishing the current step...")

    def on_abort(self, _evt):
        self.timer.Stop()
        self.run.abort()
        self.EndModal(wx.ID_CANCEL)


class GuiWaitDialog(wx.Dialog):
    """Waits for the GUI process of "Route in GUI". It ends when the process
    exits; Terminate kills it."""

    def __init__(self, parent, run):
        super().__init__(parent, title=TITLE + ": routing in the GUI", style=wx.DEFAULT_DIALOG_STYLE)
        self.run = run
        text = wx.StaticText(self, label=run.MESSAGE)
        terminate = wx.Button(self, label="Terminate")
        terminate.Bind(wx.EVT_BUTTON, self.on_terminate)
        self.Bind(wx.EVT_CLOSE, self.on_terminate)
        sizer = wx.BoxSizer(wx.VERTICAL)
        sizer.Add(text, 1, wx.ALL | wx.EXPAND, 16)
        sizer.Add(terminate, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.ALIGN_RIGHT, 16)
        self.SetSizerAndFit(sizer)
        self.CentreOnParent()
        self.timer = wx.Timer(self)
        self.Bind(wx.EVT_TIMER, self.on_tick, self.timer)
        self.timer.Start(250)

    def on_tick(self, _evt):
        if self.run.finished:
            self.timer.Stop()
            self.EndModal(wx.ID_OK)

    def on_terminate(self, _evt):
        self.timer.Stop()
        self.run.terminate()
        self.EndModal(wx.ID_CANCEL)


class Dialog(wx.Dialog):
    def __init__(self, parent, board):
        super().__init__(parent, title=TITLE, style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        self.board = board
        pcb = Path(board.GetFileName())
        self.work_dir = pcb.parent / "freerouting-rs"
        self.dsn = self.work_dir / (pcb.stem + ".dsn")
        self.ses = self.work_dir / (pcb.stem + ".ses")
        self.result_json = routing.manifest_path(self.work_dir, pcb.stem)
        self.router = tools.find_tool(tools.ROUTER)
        self.viewer = tools.find_tool(tools.VIEWER)
        self.gui = tools.find_tool(tools.VIEWER)  # the same executable: front-end mode
        self.board_min_um = board_min_track_width_um(board)
        saved = options.load_for_board(self.work_dir, tools.load_board_settings(self.work_dir))
        # The router's stage slots and each profile's choices (N01.4); empty
        # for a router without `--list-stages`.
        self.stage_listing = {
            p: options.parse_stage_listing(tools.query_stages(self.router, p) if self.router else None)
            for p in options.PROFILES
        }

        def found(tool):
            return tool.describe() if tool else "not found"

        # Runs `freerouting --version` (up to 10 s) before the dialog shows.
        self.router_version = tools.query_version(self.router) if self.router else None
        if self.router:
            self.version_status, self.version_message = version.check(self.router_version)
        else:
            self.version_status, self.version_message = version.UNKNOWN, ""
        router_ver = version.format_router_version(self.router_version)
        notice = f"\n{self.version_message}" if self.version_status != version.OK and self.router else ""
        if not self.router:
            notice += f"\n{tools.missing_router_hint()}"

        info = wx.StaticText(
            self,
            label=(
                f"Board files: {self.work_dir}\n"
                f"Router: {found(self.router)}\n"
                f"Router version: {router_ver}    Plugin version: {version.__version__}{notice}\n"
                f"Viewer: {found(self.viewer)}"
            ),
        )
        route = wx.Button(self, label="Route")
        route_gui = wx.Button(self, label="Route in GUI")
        view = wx.Button(self, label="Open in viewer")
        imp = wx.Button(self, label="Import SES...")
        close = wx.Button(self, wx.ID_CANCEL, "Close")
        route.Enable(self.router is not None and self.version_status != version.MISMATCH)
        route_gui.Enable(self.gui is not None)
        view.Enable(self.viewer is not None)
        route.Bind(wx.EVT_BUTTON, self.on_route)
        route_gui.Bind(wx.EVT_BUTTON, self.on_route_gui)
        view.Bind(wx.EVT_BUTTON, self.on_view)
        imp.Bind(wx.EVT_BUTTON, self.on_import)

        form = wx.FlexGridSizer(cols=2, vgap=4, hgap=8)
        form.AddGrowableCol(1)
        self.passes = wx.SpinCtrl(self, min=options.MAX_PASSES_RANGE[0], max=options.MAX_PASSES_RANGE[1])
        self.passes.SetValue(saved.max_passes)
        self.passes.SetToolTip("Maximum routing passes (-mp); 0 means no limit")
        self.threads = wx.SpinCtrl(self, min=options.THREADS_RANGE[0], max=options.THREADS_RANGE[1])
        self.threads.SetValue(saved.threads)
        self.threads.SetToolTip("Worker threads (--threads); 0 lets the router choose")
        self.optimizer = wx.CheckBox(self, label="Run the optimizer after routing")
        self.optimizer.SetValue(saved.optimizer)
        self.threshold = wx.TextCtrl(self, value="" if saved.improvement_threshold is None else str(saved.improvement_threshold))
        self.threshold.SetHint("router default (2.5)")
        self.threshold.SetToolTip("Stop the optimizer when a pass improves the score by less than this percentage (1 to 100)")
        self.classes = wx.TextCtrl(self, value=options.join_net_classes(saved.ignore_net_classes))
        self.classes.SetHint("none")
        self.classes.SetToolTip("Net classes the router skips (-inc), comma separated; \\, keeps a comma in a name")
        self.timeout = wx.TextCtrl(self, value=saved.timeout)
        self.timeout.SetHint("none, e.g. 10m")
        self.timeout.SetToolTip("Time limit (--timeout): 90s, 10m, 2h or HH:MM:SS")
        self.seed = wx.SpinCtrl(self, min=0, max=2147483647)
        self.seed.SetValue(saved.seed)
        self.seed.SetToolTip("Seed (--seed). This router version accepts only 0, and the GUI ignores it")
        self.min_width = wx.TextCtrl(
            self, value="" if saved.min_trace_width_um is None else str(saved.min_trace_width_um)
        )
        self.min_width.SetHint("from Board Setup: " + (f"{self.board_min_um:g}" if self.board_min_um else "not set"))
        self.min_width.SetToolTip("Minimum trace width in micrometres (--router.min_trace_width_um); empty uses the Board Setup value")
        self.fix_areas = wx.CheckBox(self, label="Fix rule areas in the exported DSN")
        self.fix_areas.SetValue(saved.rule_area_fixup)
        self.profile = wx.Choice(self, choices=[options.PROFILE_LABELS[p] for p in options.PROFILES])
        self.profile.SetSelection(options.PROFILES.index(saved.profile))
        self.profile.SetToolTip(
            "Routing profile (--profile): Legacy gives results identical to Java Freerouting; "
            "Enhanced (the default) adds the accepted improvements. The settings below start from it."
        )
        for label, ctrl in (
            ("Profile", self.profile),
            ("Maximum passes", self.passes),
            ("Threads", self.threads),
            ("", self.optimizer),
            ("Optimizer threshold (%)", self.threshold),
            ("Ignore net classes", self.classes),
            ("Time limit", self.timeout),
            ("Seed", self.seed),
            ("Minimum trace width (um)", self.min_width),
            ("", self.fix_areas),
        ):
            form.Add(wx.StaticText(self, label=label), 0, wx.ALIGN_CENTER_VERTICAL)
            form.Add(ctrl, 1, wx.EXPAND)

        buttons = wx.BoxSizer(wx.HORIZONTAL)
        for b in (route, route_gui, view, imp):
            buttons.Add(b, 0, wx.RIGHT, 6)
        buttons.AddStretchSpacer()
        buttons.Add(close)
        sizer = wx.BoxSizer(wx.VERTICAL)
        sizer.Add(info, 0, wx.ALL | wx.EXPAND, 12)
        sizer.Add(form, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 12)
        sizer.Add(self.advanced_group(saved), 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 12)
        sizer.Add(buttons, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 12)
        self.SetSizerAndFit(sizer)
        self.CentreOnParent()

    def advanced_group(self, saved):
        """The "Advanced" group (N01.4): one choice per stage slot of the
        router, preset to the saved choice or the profile's."""
        group = wx.StaticBoxSizer(wx.VERTICAL, self, "Advanced")
        slots = self.stage_listing.get(saved.profile) or []
        self.stage_choices = {}
        if not slots:
            group.Add(wx.StaticText(self, label="This router does not list its stages (no --list-stages)."))
            return group
        group.Add(wx.StaticText(self, label="Stages start at the profile's choice; pick another to change one."))
        grid = wx.FlexGridSizer(cols=2, vgap=4, hgap=8)
        grid.AddGrowableCol(1)
        for key, label, ids, default in slots:
            choice = wx.Choice(self, choices=[i or "(none)" for i in ids])
            picked = saved.stages.get(key, default)
            if picked not in ids:
                picked = default if default in ids else ids[0]
            choice.SetSelection(ids.index(picked))
            choice.SetToolTip(f"router.{key}; the profile's choice is {default or '(none)'}")
            self.stage_choices[key] = (choice, ids)
            grid.Add(wx.StaticText(self, label=label), 0, wx.ALIGN_CENTER_VERTICAL)
            grid.Add(choice, 1, wx.EXPAND)
        group.Add(grid, 0, wx.EXPAND)
        return group

    def chosen_stages(self):
        """`{key: id}` of the Advanced choices."""
        out = {}
        for key, (choice, ids) in self.stage_choices.items():
            index = choice.GetSelection()
            if 0 <= index < len(ids):
                out[key] = ids[index]
        return out

    def error(self, msg):
        wx.MessageBox(msg, TITLE, wx.OK | wx.ICON_ERROR, self)

    def read_options(self):
        """The options in the controls, saved for this board. None (after a
        message) when a field is invalid."""
        chosen, errors = options.from_form(
            {
                "max_passes": self.passes.GetValue(),
                "threads": self.threads.GetValue(),
                "optimizer": self.optimizer.GetValue(),
                "improvement_threshold": self.threshold.GetValue(),
                "ignore_net_classes": self.classes.GetValue(),
                "seed": self.seed.GetValue(),
                "min_trace_width_um": self.min_width.GetValue(),
                "timeout": self.timeout.GetValue(),
                "rule_area_fixup": self.fix_areas.GetValue(),
                "profile": options.PROFILES[max(self.profile.GetSelection(), 0)],
                "stages": self.chosen_stages(),
            }
        )
        if errors:
            self.error("Fix these route options first:\n\n" + "\n".join(errors))
            return None
        # A stage at the profile's choice follows the profile: not passed, not kept.
        defaults = options.stage_defaults(self.stage_listing.get(chosen.profile) or [])
        chosen.stages = options.differing_stages(chosen.stages, defaults)
        try:
            options.save(options.options_path(self.work_dir), chosen, defaults)
        except OSError as e:
            tools.log_line(self.work_dir, f"could not save the route options: {e}")
        return chosen

    def export(self, fix_areas):
        if export_dsn(self.board, self.dsn):
            if fix_areas:
                fix_rule_areas(self.board, self.dsn, self.work_dir)
            return True
        self.error(f"KiCad could not export the board to\n{self.dsn}")
        return False

    def on_route(self, _evt):
        chosen = self.read_options()
        if chosen is None or not self.export(chosen.rule_area_fixup):
            return
        self.ses.unlink(missing_ok=True)
        self.result_json.unlink(missing_ok=True)
        run = routing.RouteRun(
            self.router, self.dsn, self.ses, chosen.to_args(self.board_min_um), self.result_json, chosen.profile
        )
        try:
            run.start()
        except OSError as e:
            self.error(f"Could not start {self.router.describe()}:\n{e}")
            return
        with RouteProgressDialog(self, run) as progress_dialog:
            progress_dialog.ShowModal()
        tools.log_line(self.work_dir, f"{time.strftime('%Y-%m-%d %H:%M:%S')} route: " + run.state.text())
        if run.aborted:
            wx.MessageBox("Routing aborted. The board was not changed.", TITLE, wx.OK, self)
            return
        if run.returncode != 0 or not run.ses_written():
            self.error(run.failure_text())
            return
        self.finish_import(self.ses, run.summary_text(), stopped=run.stopped)

    def on_route_gui(self, _evt):
        chosen = self.read_options()
        if chosen is None or not self.export(chosen.rule_area_fixup):
            return
        self.ses.unlink(missing_ok=True)
        run = routing.GuiRun(self.gui, self.dsn, self.ses, chosen.to_args(self.board_min_um, for_gui=True))
        try:
            run.start()
        except OSError as e:
            self.error(f"Could not start {self.gui.describe()}:\n{e}")
            return
        with GuiWaitDialog(self, run) as wait_dialog:
            wait_dialog.ShowModal()
        if run.should_import():
            self.finish_import(self.ses, "", stopped=False)
        else:
            wx.MessageBox(run.outcome_text(), TITLE, wx.OK | wx.ICON_INFORMATION, self)

    def finish_import(self, ses, summary_text, stopped):
        if not import_ses(self.board, ses):
            self.error(f"KiCad could not import\n{ses}")
            return
        head = "Stopped early; imported the best board so far." if stopped else "Routed and imported the result into the board."
        text = head + ("\n\n" + summary_text if summary_text else "")
        icon = wx.ICON_EXCLAMATION if "WARNING" in summary_text else wx.ICON_INFORMATION
        wx.MessageBox(text, TITLE, wx.OK | icon, self)

    def on_view(self, _evt):
        if not self.export(self.fix_areas.GetValue()):
            return
        ses = self.ses if self.ses.is_file() else None
        try:
            tools.open_viewer(self.viewer, self.dsn, ses)
        except OSError as e:
            self.error(f"Could not start {self.viewer.describe()}:\n{e}")

    def on_import(self, _evt):
        with wx.FileDialog(
            self,
            "Import Specctra session",
            defaultDir=str(self.work_dir if self.work_dir.is_dir() else self.work_dir.parent),
            defaultFile=self.ses.name if self.ses.is_file() else "",
            wildcard="Specctra session (*.ses)|*.ses|All files (*.*)|*.*",
            style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST,
        ) as fd:
            if fd.ShowModal() != wx.ID_OK:
                return
            path = Path(fd.GetPath())
        if import_ses(self.board, path):
            wx.MessageBox(f"Imported {path.name}.", TITLE, wx.OK, self)
        else:
            self.error(f"KiCad could not import\n{path}")


class FreeroutingRsPlugin(pcbnew.ActionPlugin):
    def defaults(self):
        self.name = "freerouting-rs"
        self.category = "Routing"
        self.description = "Route the board with freerouting-rs, or open it in the freerouting-rs viewer"
        self.show_toolbar_button = True
        self.icon_file_name = str(Path(__file__).with_name("icon.png"))
        self.dark_icon_file_name = self.icon_file_name

    def Run(self):
        board = pcbnew.GetBoard()
        parent = wx.FindWindowByName("PcbFrame") or wx.GetActiveWindow()
        if not board.GetFileName():
            wx.MessageBox("Save the board first.", TITLE, wx.OK | wx.ICON_INFORMATION, parent)
            return
        dlg = Dialog(parent, board)
        dlg.ShowModal()
        dlg.Destroy()

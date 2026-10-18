"""KiCad PCB editor action plugin for freerouting-rs.

Exports the open board to Specctra DSN (or, with the experimental board input
`kicad_pcb`, takes a snapshot of it with SaveBoard, K10), routes it with the freerouting-rs
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

from . import dropdowns, export_flow, kicad_board, layout, options, presets, progress, routing, snapshot, tools, version

TITLE = "freerouting-rs"
TICK_MS = 100
# The route dialog's tabs, in order.
TABS = ("Route", "Board and DSN", "Advanced")


def export_dsn(board, dsn):
    dsn.parent.mkdir(parents=True, exist_ok=True)
    try:
        ok = pcbnew.ExportSpecctraDSN(board, str(dsn))
    except TypeError:  # older KiCad: only the current board
        ok = pcbnew.ExportSpecctraDSN(str(dsn))
    return bool(ok) and dsn.is_file()


def detach_unlocked_tracks(board):
    """Takes the unlocked tracks and vias off the board without deleting them.

    ImportSpecctraSES(board, ...) deletes the tracks it replaces, but KiCad's
    action-plugin runner holds every board item from before Run and, after Run,
    commits the missing ones as removed. Deleted, those pointers dangle: wx's
    "Unhandled exception" box when the dialog closes, or a crash on Undo.
    Detached first, they stay alive; the importer finds none to delete and the
    runner records them as removed, so Undo restores them.

    pcbnew's `Remove` leaves the C++ side owning the item only while an action
    plugin is running; otherwise Python takes it and deletes it on garbage
    collection, which breaks the board's later reads. `thisown = 0` keeps C++
    the owner in both cases (the undo list frees them)."""
    detached = [track for track in board.GetTracks() if not track.IsLocked()]
    for track in detached:
        board.Remove(track)
        track.thisown = 0
    return detached


def reattach(board, tracks):
    for track in tracks:
        board.Add(track)


def import_ses(board, ses):
    detached = detach_unlocked_tracks(board)
    try:
        ok = pcbnew.ImportSpecctraSES(board, str(ses))
    except TypeError:  # older KiCad: the frame's import, which clears the undo list itself
        reattach(board, detached)
        detached = []
        ok = pcbnew.ImportSpecctraSES(str(ses))
    if not ok:
        reattach(board, detached)
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


def labelled_grid(parent, rows):
    """A two-column grid of `(label, control)` rows; an empty label leaves the
    left cell blank (check boxes)."""
    grid = wx.FlexGridSizer(cols=2, vgap=4, hgap=8)
    grid.AddGrowableCol(1)
    for label, ctrl in rows:
        grid.Add(wx.StaticText(parent, label=label), 0, wx.ALIGN_CENTER_VERTICAL)
        grid.Add(ctrl, 1, wx.EXPAND)
    return grid


def add_page(notebook, title):
    """A tab page that scrolls vertically when the dialog is cut down to the
    screen (Dialog.fit_to_display)."""
    page = wx.ScrolledWindow(notebook, style=wx.VSCROLL)
    page.SetScrollRate(0, 12)
    notebook.AddPage(page, title)
    return page


def fill_page(page, *parts):
    """Lays `parts` (sizers) out top to bottom on `page`."""
    sizer = wx.BoxSizer(wx.VERTICAL)
    for part in parts:
        sizer.Add(part, 0, wx.ALL | wx.EXPAND, 8)
    page.SetSizer(sizer)


def log_lines(work_dir, dsn, lines):
    stamp = time.strftime("%Y-%m-%d %H:%M:%S")
    for line in lines:
        tools.log_line(work_dir, f"{stamp} {dsn.name}: {line}")


def fix_rule_areas(board, dsn, work_dir):
    """Rewrites the rule-area keepouts in the exported DSN (see rule_areas).
    pcbnew is read here, on the UI thread. Returns the log lines; a failure is
    logged and leaves the DSN as KiCad wrote it."""
    lines = export_flow.fix_rule_areas(board, pcbnew, dsn)
    log_lines(work_dir, dsn, lines)
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
        self.stem = pcb.stem
        self.board_dir = pcb.parent
        self.dsn = self.work_dir / (pcb.stem + ".dsn")
        self.design = self.dsn  # what the router reads: the DSN, or the snapshot (K10)
        self.ses = self.work_dir / (pcb.stem + ".ses")
        self.result_json = routing.manifest_path(self.work_dir, pcb.stem)
        self.router = tools.find_tool(tools.ROUTER)
        self.viewer = tools.find_tool(tools.VIEWER)
        self.gui = tools.find_tool(tools.VIEWER)  # the same executable: front-end mode
        self.board_min_um = board_min_track_width_um(board)
        saved = options.load_for_board(self.work_dir, tools.load_board_settings(self.work_dir))
        # The router's stage slots and each profile's choices (N01.4); empty
        # for a router without `--list-stages`.
        listings = {p: tools.query_stages(self.router, p) if self.router else None for p in options.PROFILES}
        self.stage_listing = {p: options.parse_stage_listing(listings[p]) for p in options.PROFILES}
        # Stage ids the router marks experimental (N02.4), per profile.
        self.stage_experimental = {p: options.experimental_stage_ids(listings[p]) for p in options.PROFILES}

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

        notebook = wx.Notebook(self)
        self.pages = [add_page(notebook, title) for title in TABS]
        route_page, board_page, advanced_page = self.pages
        # A group box's controls are its children (wxWidgets 3.3 logs a
        # warning for a control in the box's sizer but not in the box).
        dsn_group = wx.StaticBoxSizer(wx.VERTICAL, board_page, "DSN fix-ups")
        flow_group = wx.StaticBoxSizer(wx.VERTICAL, advanced_page, "Flow")
        stages_group = wx.StaticBoxSizer(wx.VERTICAL, advanced_page, "Stages")
        preset_group = wx.StaticBoxSizer(wx.VERTICAL, self, "Presets")
        self.passes = wx.SpinCtrl(route_page, min=options.MAX_PASSES_RANGE[0], max=options.MAX_PASSES_RANGE[1])
        self.passes.SetValue(saved.max_passes)
        self.passes.SetToolTip("Maximum routing passes (-mp); 0 means no limit")
        self.threads = wx.SpinCtrl(route_page, min=options.THREADS_RANGE[0], max=options.THREADS_RANGE[1])
        self.threads.SetValue(saved.threads)
        self.threads.SetToolTip("Worker threads (--threads); 0 lets the router choose")
        self.optimizer = wx.CheckBox(route_page, label="Run the optimizer after routing")
        self.optimizer.SetValue(saved.optimizer)
        self.threshold = wx.TextCtrl(route_page, value="" if saved.improvement_threshold is None else str(saved.improvement_threshold))
        self.threshold.SetHint("router default (2.5)")
        self.threshold.SetToolTip("Stop the optimizer when a pass improves the score by less than this percentage (1 to 100)")
        self.classes = wx.TextCtrl(route_page, value=options.join_net_classes(saved.ignore_net_classes))
        self.classes.SetHint("none")
        self.classes.SetToolTip("Net classes the router skips (-inc), comma separated; \\, keeps a comma in a name")
        self.timeout = wx.TextCtrl(route_page, value=saved.timeout)
        self.timeout.SetHint("none, e.g. 10m")
        self.timeout.SetToolTip("Time limit (--timeout): 90s, 10m, 2h or HH:MM:SS")
        self.seed = wx.SpinCtrl(route_page, min=0, max=2147483647)
        self.seed.SetValue(saved.seed)
        self.seed.SetToolTip("Seed (--seed). This router version accepts only 0, and the GUI ignores it")
        self.min_width = wx.TextCtrl(
            route_page, value="" if saved.min_trace_width_um is None else str(saved.min_trace_width_um)
        )
        self.min_width.SetHint("from Board Setup: " + (f"{self.board_min_um:g}" if self.board_min_um else "not set"))
        self.min_width.SetToolTip("Minimum trace width in micrometres (--router.min_trace_width_um); empty uses the Board Setup value")
        self.fix_areas = wx.CheckBox(board_page, label="Fix rule areas in the exported DSN")
        self.fix_areas.SetValue(saved.rule_area_fixup)
        self.add_k09_controls(saved, dsn_group.GetStaticBox(), flow_group.GetStaticBox())
        self.add_k10_controls(saved, board_page)
        self.add_preset_controls(preset_group.GetStaticBox())
        self.profile = wx.Choice(route_page, choices=[options.PROFILE_LABELS[p] for p in options.PROFILES])
        self.profile.SetSelection(options.PROFILES.index(saved.profile))
        self.profile.SetToolTip(
            "Routing profile (--profile): Legacy gives results identical to Java Freerouting; "
            "Enhanced (the default) adds the accepted improvements. The other settings start from it."
        )
        fill_page(
            route_page,
            labelled_grid(
                route_page,
                (
                    ("Profile", self.profile),
                    ("Maximum passes", self.passes),
                    ("Threads", self.threads),
                    ("", self.optimizer),
                    ("Optimizer threshold (%)", self.threshold),
                    ("Ignore net classes", self.classes),
                    ("Time limit", self.timeout),
                    ("Seed", self.seed),
                    ("Minimum trace width (um)", self.min_width),
                ),
            ),
        )
        fill_page(
            board_page,
            labelled_grid(board_page, (("Board input", self.board_input), ("", self.apply_rules), ("", self.fix_areas))),
            self.fill_dsn_group(dsn_group),
        )
        fill_page(advanced_page, self.fill_flow_group(flow_group), self.fill_stages_group(stages_group, saved))
        # The tabs open tall enough for the longest (owner: no scrolling where
        # the screen has the room); fit_to_display cuts them on a small one.
        self.page_contents = [page.GetSizer().GetMinSize() for page in self.pages]
        self.set_page_heights([size.height for size in self.page_contents])

        buttons = wx.BoxSizer(wx.HORIZONTAL)
        for b in (route, route_gui, view, imp):
            buttons.Add(b, 0, wx.RIGHT, 6)
        buttons.AddStretchSpacer()
        buttons.Add(close)
        sizer = wx.BoxSizer(wx.VERTICAL)
        # The board's paths, the presets and the buttons stay outside the
        # tabs: always in view, whichever tab is open.
        sizer.Add(info, 0, wx.ALL | wx.EXPAND, 12)
        sizer.Add(self.fill_preset_group(preset_group), 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 12)
        sizer.Add(notebook, 1, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 12)
        sizer.Add(buttons, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 12)
        self.SetSizerAndFit(sizer)
        self.fit_to_display()
        self.CentreOnParent()
        # KiCad 10's wxWidgets draws the selected row of a dark drop-down
        # white on white on recent Windows 11 builds (dropdowns.py).
        dropdowns.fix_dark_lists([self.profile, self.board_input, self.preset_choice, self.pour_mode, self.sweep] + [c for c, _ in self.stage_choices.values()])

    def add_k09_controls(self, saved, dsn_box, flow_box):
        """The K09 fields: DSN fix-ups (in `dsn_box`) and the flow's router
        settings (in `flow_box`)."""
        self.widths = wx.TextCtrl(dsn_box, value=options.format_class_widths(saved.class_widths))
        self.widths.SetHint("as exported, or CLASS=mm, CLASS=mm, or auto")
        self.widths.SetToolTip("Routed width per net class in mm, written into the DSN; auto caps wide classes at their narrowest SMD pad")
        self.fixed_pours = wx.TextCtrl(dsn_box, value=options.format_names(saved.fixed_pour_classes))
        self.fixed_pours.SetHint("none, e.g. CLASS, CLASS, or auto")
        self.fixed_pours.SetToolTip("Net classes whose outer-layer pours other nets must keep out of; auto: pours above their layer's lowest zone priority")
        self.pour_mode = wx.Choice(dsn_box, choices=list(options.POUR_MODES))
        self.pour_mode.SetSelection(options.POUR_MODES.index(saved.fixed_pour_mode))
        self.pour_mode.SetToolTip("wires: the fill becomes fixed copper in the DSN; engine: the router keeps foreign nets out (Q01)")
        self.drop_pours = wx.TextCtrl(dsn_box, value=options.format_names(saved.drop_pour_nets))
        self.drop_pours.SetHint("none, e.g. NET, NET, or auto")
        self.drop_pours.SetToolTip("Nets whose outer-layer pours are left out, so their pads fan out to the inner plane; auto: a net alone on an inner layer")
        self.keepouts = wx.CheckBox(dsn_box, label="Keep the board-edge and pad clearances (keepouts)")
        self.keepouts.SetValue(saved.edge_and_pad_keepouts)
        self.open_layers = wx.TextCtrl(dsn_box, value=options.format_names(saved.route_on_plane_layers))
        self.open_layers.SetHint("none, e.g. an inner layer name")
        self.open_layers.SetToolTip("Inner plane layers the router may also route on")
        self.fix_existing = wx.CheckBox(dsn_box, label="Keep the existing tracks (add-only run)")
        self.fix_existing.SetValue(saved.fix_existing)
        self.ripup = wx.TextCtrl(flow_box, value="" if saved.start_ripup_costs is None else str(saved.start_ripup_costs))
        self.ripup.SetHint("router default (100)")
        self.ripup.SetToolTip("Start rip-up cost (--router.scoring.start_ripup_costs)")
        self.sweep = wx.Choice(flow_box, choices=["off", "fixed", "free"])
        self.sweep.SetSelection(options.SWEEPS.index(saved.sweep))
        self.sweep.SetToolTip("Add-only completion sweep after the passes (Q10)")
        self.sweep_passes = wx.TextCtrl(flow_box, value="" if saved.sweep_passes is None else str(saved.sweep_passes))
        self.sweep_passes.SetHint("router default (2)")
        self.pass_hint = wx.CheckBox(flow_box, label="Stop at the pass that won last time" + (f" ({saved.pass_hint})" if saved.pass_hint else ""))
        self.pass_hint.SetValue(saved.use_pass_hint)
        self.saved_pass_hint = saved.pass_hint

    def add_k10_controls(self, saved, page):
        """The K10 fields: what the router reads, and the .kicad_dru switch."""
        self.board_input = wx.Choice(page, choices=[options.BOARD_INPUT_LABELS[i] for i in options.BOARD_INPUTS])
        self.board_input.SetSelection(options.BOARD_INPUTS.index(saved.board_input))
        self.board_input.SetToolTip(
            "What the router reads. KiCad's DSN export is the default. The board snapshot is "
            "the open board saved with SaveBoard and read by the router's own reader (rule areas by "
            "their flags, track arcs, unsaved Board Setup changes); it cannot be combined with the "
            "DSN fix-ups below. Experimental."
        )
        self.apply_rules = wx.CheckBox(page, label="Apply the project's .kicad_dru rules (snapshot, Route only)")
        self.apply_rules.SetValue(saved.apply_kicad_rules)
        self.apply_rules.SetToolTip("Class-pair clearances and class track widths of the .kicad_dru beside the board (--apply-design-rules)")

    def k10_values(self):
        return {
            "board_input": options.BOARD_INPUTS[max(self.board_input.GetSelection(), 0)],
            "apply_kicad_rules": self.apply_rules.GetValue(),
        }

    def fill_dsn_group(self, group):
        rows = (
            ("Class widths (mm)", self.widths),
            ("Fixed pour classes", self.fixed_pours),
            ("Fixed pours as", self.pour_mode),
            ("Drop pours of nets", self.drop_pours),
            ("", self.keepouts),
            ("Route on plane layers", self.open_layers),
            ("", self.fix_existing),
        )
        group.Add(labelled_grid(group.GetStaticBox(), rows), 0, wx.EXPAND)
        return group

    def fill_flow_group(self, group):
        rows = (
            ("Start rip-up cost", self.ripup),
            ("Completion sweep", self.sweep),
            ("Sweep passes", self.sweep_passes),
            ("", self.pass_hint),
        )
        group.Add(labelled_grid(group.GetStaticBox(), rows), 0, wx.EXPAND)
        return group

    def add_preset_controls(self, box):
        """Presets (presets.py): option files the board or the user brings."""
        self.preset_choice = wx.Choice(box, choices=[])
        self.preset_apply = wx.Button(box, label="Apply")
        self.preset_save = wx.Button(box, label="Save as preset...")
        self.preset_choice.SetToolTip(
            "Option presets: JSON files in freerouting-rs/presets next to the board, or in the user's presets folder"
        )
        self.preset_apply.SetToolTip("Fill the fields with the preset's options; each can still be changed")
        self.preset_save.SetToolTip("Save the fields as a preset in this board's freerouting-rs/presets folder")
        self.preset_apply.Bind(wx.EVT_BUTTON, self.on_apply_preset)
        self.preset_save.Bind(wx.EVT_BUTTON, self.on_save_preset)
        self.load_presets()

    def load_presets(self, select=None):
        """Lists the presets; a skipped file is logged as a warning."""
        self.presets, warnings = presets.discover(self.work_dir, presets.user_dir())
        for warning in warnings:
            tools.log_line(self.work_dir, f"{time.strftime('%Y-%m-%d %H:%M:%S')} WARNING: {warning}")
        names = [p.label() for p in self.presets]
        self.preset_choice.Clear()
        for name in names or ["(no presets)"]:
            self.preset_choice.Append(name)
        index = next((i for i, p in enumerate(self.presets) if p.name == select), 0)
        self.preset_choice.SetSelection(index)
        self.preset_apply.Enable(bool(self.presets))

    def fill_preset_group(self, group):
        row = wx.BoxSizer(wx.HORIZONTAL)
        row.Add(self.preset_choice, 1, wx.RIGHT | wx.EXPAND, 6)
        row.Add(self.preset_apply, 0, wx.RIGHT, 6)
        row.Add(self.preset_save)
        group.Add(row, 0, wx.EXPAND)
        return group

    def show_options(self, opts):
        """Puts `opts` into every control (applying a preset)."""
        self.passes.SetValue(opts.max_passes)
        self.threads.SetValue(opts.threads)
        self.optimizer.SetValue(opts.optimizer)
        self.threshold.SetValue("" if opts.improvement_threshold is None else options._fmt(opts.improvement_threshold))
        self.classes.SetValue(options.join_net_classes(opts.ignore_net_classes))
        self.timeout.SetValue(opts.timeout)
        self.seed.SetValue(opts.seed)
        self.min_width.SetValue("" if opts.min_trace_width_um is None else options._fmt(opts.min_trace_width_um))
        self.fix_areas.SetValue(opts.rule_area_fixup)
        self.profile.SetSelection(options.PROFILES.index(opts.profile))
        self.board_input.SetSelection(options.BOARD_INPUTS.index(opts.board_input))
        self.apply_rules.SetValue(opts.apply_kicad_rules)
        self.widths.SetValue(options.format_class_widths(opts.class_widths))
        self.fixed_pours.SetValue(options.format_names(opts.fixed_pour_classes))
        self.pour_mode.SetSelection(options.POUR_MODES.index(opts.fixed_pour_mode))
        self.drop_pours.SetValue(options.format_names(opts.drop_pour_nets))
        self.keepouts.SetValue(opts.edge_and_pad_keepouts)
        self.open_layers.SetValue(options.format_names(opts.route_on_plane_layers))
        self.fix_existing.SetValue(opts.fix_existing)
        self.ripup.SetValue("" if opts.start_ripup_costs is None else str(opts.start_ripup_costs))
        self.sweep.SetSelection(options.SWEEPS.index(opts.sweep))
        self.sweep_passes.SetValue("" if opts.sweep_passes is None else str(opts.sweep_passes))
        self.pass_hint.SetValue(opts.use_pass_hint)
        for key, (choice, ids) in self.stage_choices.items():
            if opts.stages.get(key) in ids:
                choice.SetSelection(ids.index(opts.stages[key]))

    def on_apply_preset(self, _evt):
        index = self.preset_choice.GetSelection()
        if not 0 <= index < len(self.presets):
            return
        preset = self.presets[index]
        current, _errors = options.from_form(self.form_values())  # a bad field keeps its default here
        merged, warnings = presets.apply(preset, current)
        for warning in warnings:
            tools.log_line(self.work_dir, f"{time.strftime('%Y-%m-%d %H:%M:%S')} WARNING: {warning}")
        self.show_options(merged)

    def ask_preset_name(self):
        """The name the user types for "Save as preset...", or None."""
        with wx.TextEntryDialog(self, "Name of the preset:", TITLE + ": save as preset") as entry:
            if entry.ShowModal() != wx.ID_OK:
                return None
            return entry.GetValue().strip() or None

    def on_save_preset(self, _evt):
        name = self.ask_preset_name()
        if not name:
            return
        chosen, errors = options.from_form(self.form_values())
        if errors:
            self.error("Fix these route options first:\n\n" + "\n".join(errors))
            return
        chosen.pass_hint = self.saved_pass_hint
        defaults = options.stage_defaults(self.stage_listing.get(chosen.profile) or [])
        chosen.stages = options.differing_stages(chosen.stages, defaults)
        try:
            path = presets.save(self.work_dir, name, chosen, stage_defaults=defaults)
        except (OSError, ValueError) as e:
            self.error(f"Could not save the preset:\n{e}")
            return
        self.load_presets(select=name)
        wx.MessageBox(f"Saved {path.name} in\n{path.parent}", TITLE, wx.OK | wx.ICON_INFORMATION, self)

    def k09_values(self):
        return {
            "class_widths": self.widths.GetValue(),
            "fixed_pour_classes": self.fixed_pours.GetValue(),
            "fixed_pour_mode": options.POUR_MODES[max(self.pour_mode.GetSelection(), 0)],
            "drop_pour_nets": self.drop_pours.GetValue(),
            "edge_and_pad_keepouts": self.keepouts.GetValue(),
            "route_on_plane_layers": self.open_layers.GetValue(),
            "fix_existing": self.fix_existing.GetValue(),
            "start_ripup_costs": self.ripup.GetValue(),
            "sweep": options.SWEEPS[max(self.sweep.GetSelection(), 0)],
            "sweep_passes": self.sweep_passes.GetValue(),
            "use_pass_hint": self.pass_hint.GetValue(),
        }

    def fill_stages_group(self, group, saved):
        """The "Stages" group of the Advanced tab (N01.4): one choice per stage
        slot of the router, preset to the saved choice or the profile's."""
        box = group.GetStaticBox()
        slots = self.stage_listing.get(saved.profile) or []
        self.stage_choices = {}
        if not slots:
            group.Add(wx.StaticText(box, label="This router does not list its stages (no --list-stages)."))
            return group
        group.Add(wx.StaticText(box, label="Stages start at the profile's choice; pick another to change one."))
        rows = []
        for key, label, ids, default in slots:
            experimental = self.stage_experimental.get(saved.profile, {}).get(key, set())
            choice = wx.Choice(box, choices=[options.stage_label(i, experimental) for i in ids])
            picked = saved.stages.get(key, default)
            if picked not in ids:
                picked = default if default in ids else ids[0]
            choice.SetSelection(ids.index(picked))
            choice.SetToolTip(f"router.{key}; the profile's choice is {default or '(none)'}")
            self.stage_choices[key] = (choice, ids)
            rows.append((label, choice))
        group.Add(labelled_grid(box, rows), 0, wx.TOP | wx.EXPAND, 6)
        return group

    def set_page_heights(self, heights):
        """A scrolled window with a vertical scroll rate asks for no height of
        its own, so each page's minimum size is set: its contents', at
        `heights`, with room for the scroll bar where that cuts it."""
        scrollbar = wx.SystemSettings.GetMetric(wx.SYS_VSCROLL_X)
        for page, contents, height in zip(self.pages, self.page_contents, heights):
            page.SetMinSize(wx.Size(contents.width + (scrollbar if height < contents.height else 0), height))
            page.SetVirtualSize(contents)

    def fit_to_display(self):
        """Cuts the dialog down to its screen's work area when it would come
        out taller (layout.py): the tab pages then scroll and the buttons stay
        in view."""
        index = wx.Display.GetFromWindow(self)
        area = wx.Display(index if index >= 0 else 0).GetClientArea()
        heights = layout.cut_to_screen(
            [page.GetMinSize().height for page in self.pages], self.GetSize().height, layout.max_dialog_height(area.height)
        )
        if heights is None:
            return
        self.set_page_heights(heights)
        self.GetSizer().SetSizeHints(self)

    def chosen_stages(self):
        """`{key: id}` of the Advanced tab's stage choices."""
        out = {}
        for key, (choice, ids) in self.stage_choices.items():
            index = choice.GetSelection()
            if 0 <= index < len(ids):
                out[key] = ids[index]
        return out

    def error(self, msg):
        wx.MessageBox(msg, TITLE, wx.OK | wx.ICON_ERROR, self)

    def form_values(self):
        """What the controls hold, as `options.from_form` takes it."""
        return {
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
            **self.k09_values(),
            **self.k10_values(),
        }

    def read_options(self):
        """The options in the controls, saved for this board. None (after a
        message) when a field is invalid."""
        chosen, errors = options.from_form(self.form_values())
        chosen.pass_hint = self.saved_pass_hint
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

    def export(self, chosen):
        """Exports and fixes up the DSN, or takes the board snapshot (K10),
        and sets `self.design` to the file the router reads. Returns `(ok,
        router_args, locked)`; `locked` are the tracks `fix_existing` locked,
        which stay locked until `kicad_board.restore_locks` after the import."""
        self.design = snapshot.design_path(self.work_dir, self.stem, chosen.board_input)
        if snapshot.wanted(chosen):
            ok, extra, lines, locked = export_flow.prepare_snapshot(
                self.board, pcbnew, self.work_dir, self.stem, self.board_dir, chosen
            )
            if not ok:
                log_lines(self.work_dir, self.design, lines)
                self.error(f"KiCad could not save a snapshot of the board to\n{self.design}\n\n" + "\n".join(lines))
                return False, [], []
        else:
            ok, extra, lines, locked = export_flow.prepare(self.board, pcbnew, self.dsn, chosen, export_dsn)
            if not ok:
                self.error(f"KiCad could not export the board to\n{self.dsn}")
                return False, [], []
        log_lines(self.work_dir, self.design, lines)
        return True, extra, locked

    def on_route(self, _evt):
        chosen = self.read_options()
        if chosen is None:
            return
        ok, extra, locked = self.export(chosen)
        if not ok:
            return
        try:
            self.route(chosen, extra)
        finally:
            kicad_board.restore_locks(locked)

    def route(self, chosen, extra):
        self.ses.unlink(missing_ok=True)
        self.result_json.unlink(missing_ok=True)
        run = routing.RouteRun(
            self.router,
            self.design,
            self.ses,
            chosen.to_args(self.board_min_um, extra_args=extra),
            self.result_json,
            chosen.profile,
        )
        try:
            run.start()
        except OSError as e:
            self.error(f"Could not start {self.router.describe()}:\n{e}")
            return
        with RouteProgressDialog(self, run) as progress_dialog:
            progress_dialog.ShowModal()
        tools.log_line(self.work_dir, f"{time.strftime('%Y-%m-%d %H:%M:%S')} route: " + run.state.text())
        if snapshot.wanted(chosen):  # the reader's report and warnings (K10.3)
            log_lines(self.work_dir, self.design, snapshot.reader_lines(run.lines))
        if run.aborted:
            wx.MessageBox("Routing aborted. The board was not changed.", TITLE, wx.OK, self)
            return
        if run.returncode != 0 or not run.ses_written():
            self.error(run.failure_text())
            return
        self.remember_pass(chosen, run.state.best_pass())
        self.finish_import(self.ses, run.summary_text(), stopped=run.stopped)

    def remember_pass(self, chosen, best):
        """K09.7: the pass that won goes into the options file as the hint."""
        if not best or best == chosen.pass_hint or chosen.use_pass_hint:
            return
        chosen.pass_hint = best
        self.saved_pass_hint = best
        defaults = options.stage_defaults(self.stage_listing.get(chosen.profile) or [])
        try:
            options.save(options.options_path(self.work_dir), chosen, defaults)
        except OSError as e:
            tools.log_line(self.work_dir, f"could not save the pass hint: {e}")

    def on_route_gui(self, _evt):
        chosen = self.read_options()
        if chosen is None:
            return
        ok, extra, locked = self.export(chosen)
        if not ok:
            return
        try:
            self.route_gui(chosen, extra)
        finally:
            kicad_board.restore_locks(locked)

    def route_gui(self, chosen, extra):
        self.ses.unlink(missing_ok=True)
        if snapshot.wanted(chosen) and chosen.apply_kicad_rules:
            log_lines(self.work_dir, self.design, ["Route in GUI reads the .kicad_dru but does not apply it (use Route)"])
        run = routing.GuiRun(self.gui, self.design, self.ses, chosen.to_args(self.board_min_um, for_gui=True, extra_args=extra))
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
        log_lines(self.work_dir, ses, export_flow.after_import(self.board, ses))
        head = "Stopped early; imported the best board so far." if stopped else "Routed and imported the result into the board."
        text = head + ("\n\n" + summary_text if summary_text else "")
        icon = wx.ICON_EXCLAMATION if "WARNING" in summary_text else wx.ICON_INFORMATION
        wx.MessageBox(text, TITLE, wx.OK | icon, self)

    def on_view(self, _evt):
        chosen = self.read_options()
        if chosen is None:
            return
        ok, _extra, locked = self.export(chosen)
        kicad_board.restore_locks(locked)
        if not ok:
            return
        ses = self.ses if self.ses.is_file() else None
        try:
            tools.open_viewer(self.viewer, self.design, ses)
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

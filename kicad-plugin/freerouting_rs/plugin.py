"""KiCad PCB editor action plugin for freerouting-rs.

Exports the open board to Specctra DSN, routes it with the freerouting-rs
command line, imports the resulting SES session, and opens boards in the
freerouting-rs viewer. Files go in a `freerouting-rs` folder next to the
.kicad_pcb file.
"""

import threading
import time
from pathlib import Path

import pcbnew
import wx

from . import tools

TITLE = "freerouting-rs"


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


class Dialog(wx.Dialog):
    def __init__(self, parent, board):
        super().__init__(parent, title=TITLE, style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        self.board = board
        pcb = Path(board.GetFileName())
        self.work_dir = pcb.parent / "freerouting-rs"
        self.dsn = self.work_dir / (pcb.stem + ".dsn")
        self.ses = self.work_dir / (pcb.stem + ".ses")
        self.router = tools.find_tool(tools.ROUTER)
        self.viewer = tools.find_tool(tools.VIEWER)

        def found(tool):
            return tool.describe() if tool else "not found"

        info = wx.StaticText(
            self,
            label=(
                f"Board files: {self.work_dir}\n"
                f"Router: {found(self.router)}\n"
                f"Viewer: {found(self.viewer)}\n\n"
                "freerouting-rs does not route yet: Route will report that until\n"
                "the autorouter is ported. Viewing and SES import work today."
            ),
        )
        route = wx.Button(self, label="Route")
        view = wx.Button(self, label="Open in viewer")
        imp = wx.Button(self, label="Import SES...")
        close = wx.Button(self, wx.ID_CANCEL, "Close")
        route.Enable(self.router is not None)
        view.Enable(self.viewer is not None)
        route.Bind(wx.EVT_BUTTON, self.on_route)
        view.Bind(wx.EVT_BUTTON, self.on_view)
        imp.Bind(wx.EVT_BUTTON, self.on_import)

        buttons = wx.BoxSizer(wx.HORIZONTAL)
        for b in (route, view, imp):
            buttons.Add(b, 0, wx.RIGHT, 6)
        buttons.AddStretchSpacer()
        buttons.Add(close)
        sizer = wx.BoxSizer(wx.VERTICAL)
        sizer.Add(info, 1, wx.ALL | wx.EXPAND, 12)
        sizer.Add(buttons, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 12)
        self.SetSizerAndFit(sizer)
        self.CentreOnParent()

    def error(self, msg):
        wx.MessageBox(msg, TITLE, wx.OK | wx.ICON_ERROR, self)

    def export(self):
        if export_dsn(self.board, self.dsn):
            return True
        self.error(f"KiCad could not export the board to\n{self.dsn}")
        return False

    def on_route(self, _evt):
        if not self.export():
            return
        self.ses.unlink(missing_ok=True)
        try:
            proc = tools.start_route(self.router, self.dsn, self.ses)
        except OSError as e:
            self.error(f"Could not start {self.router.describe()}:\n{e}")
            return
        result = {}
        worker = threading.Thread(target=lambda: result.update(r=tools.finish_route(self.router, proc)))
        worker.start()
        progress = wx.ProgressDialog(
            TITLE, "Routing...", parent=self, style=wx.PD_APP_MODAL | wx.PD_CAN_ABORT | wx.PD_ELAPSED_TIME
        )
        cancelled = False
        while worker.is_alive():
            keep_going, _ = progress.Pulse()
            if not keep_going:
                cancelled = True
                proc.kill()
                break
            wx.Yield()
            time.sleep(0.1)
        worker.join()
        progress.Destroy()
        if cancelled:
            return
        code, output = result["r"]
        if code != 0 or not self.ses.is_file():
            self.error(f"Routing failed (exit code {code}).\n\n{output or 'No output.'}")
            return
        if import_ses(self.board, self.ses):
            wx.MessageBox("Routed and imported the result into the board.", TITLE, wx.OK, self)
        else:
            self.error(f"KiCad could not import\n{self.ses}")

    def on_view(self, _evt):
        if not self.export():
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

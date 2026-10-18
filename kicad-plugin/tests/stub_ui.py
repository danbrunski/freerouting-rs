"""Stand-ins for `wx` and `pcbnew` so that plugin.py's glue (the dialogs, the
Route / Route in GUI flows) runs without KiCad. They record what the plugin
asks for: message boxes, SES imports, DSN exports.

`install(monkeypatch, work)` puts the stubs in `sys.modules`, loads
`freerouting_rs.plugin` as part of a package whose `__init__` never runs (it
would register the plugin), and returns `(plugin, ui)`. A modal dialog's
`ShowModal` runs its timer handlers in a loop until `EndModal`; `ui.on_modal`
is called on every iteration with `(dialog, iteration)` so a test can press
Stop, Abort or Terminate.
"""

import importlib
import shutil
import sys
import time
import types
from pathlib import Path

from conftest import PLUGIN_SRC, TESTS


class UI:
    def __init__(self):
        self.messages = []  # (text, style)
        self.imports = []  # paths given to ImportSpecctraSES
        self.exports = []
        self.on_modal = None
        self.dialogs = []  # every modal dialog that was shown, in order
        self.import_ok = True
        self.snapshots = []  # (path, aSkipSettings) given to SaveBoard
        self.project_copies = []  # paths given to SaveProjectCopy
        self.project_copy_fails = False
        self.buttons = []  # every wx.Button, in creation order
        self.dialog_size = (700, 800)  # what a fitted dialog's GetSize returns
        self.page_size = (400, 500)  # a tab page's contents (its sizer's GetMinSize)
        self.page_sizes = {}  # tab title -> its contents, overriding page_size
        self.work_area_height = 1040  # the display's GetClientArea().height
        self.scrollbar_width = 17


class Size:
    def __init__(self, width, height):
        self.width, self.height = width, height

    def __eq__(self, other):
        return (self.width, self.height) == (other.width, other.height)

    def __repr__(self):
        return f"Size({self.width}, {self.height})"


class Control:
    """A control that holds a value (SpinCtrl, TextCtrl, CheckBox)."""

    def __init__(self, *args, value=None, min=0, max=100, label="", **kwargs):
        self._v = value if value is not None else min
        self.label = label
        self.parent = args[0] if args else None

    def GetValue(self):
        return self._v

    def SetValue(self, v):
        self._v = v

    def AppendText(self, text):
        self._v = (self._v or "") + text

    def SetLabel(self, text):
        self.label = text

    def __getattr__(self, name):  # SetHint, SetToolTip, Enable, Bind, Pulse, ...
        if name.startswith("__"):
            raise AttributeError(name)
        return lambda *a, **k: None


class Sizer:
    def __init__(self, *args, **kwargs):
        pass

    def __getattr__(self, name):
        if name.startswith("__"):
            raise AttributeError(name)
        return lambda *a, **k: None


class TextControl(Control):
    def __init__(self, *args, value="", **kwargs):
        super().__init__(*args, value=value, **kwargs)


class CheckBox(Control):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, value=False, **kwargs)


class Choice(Control):
    def __init__(self, *args, choices=(), **kwargs):
        super().__init__(*args, **kwargs)
        self.choices = list(choices)
        self.value = 0

    def Clear(self):
        self.choices = []
        self.value = 0

    def Append(self, text):
        self.choices.append(text)

    def GetSelection(self):
        return self.value

    def SetSelection(self, index):
        self.value = index

    def GetStringSelection(self):
        return self.choices[self.value] if 0 <= self.value < len(self.choices) else ""

    def SetStringSelection(self, text):
        if text in self.choices:
            self.value = self.choices.index(text)


class Timer:
    def __init__(self, owner):
        self.running = False

    def Start(self, ms):
        self.running = True

    def Stop(self):
        self.running = False


def build_wx(ui):
    wx = types.ModuleType("wx")

    class Dialog:
        def __init__(self, parent=None, *args, **kwargs):
            self._handlers = {}
            self._ended = None
            self.parent = parent
            self.min_size = None
            self.virtual_size = None
            self._sizer = Sizer()

        def SetMinSize(self, size):
            self.min_size = size

        def GetMinSize(self):
            return self.min_size

        def SetVirtualSize(self, size):
            self.virtual_size = size

        def GetSize(self):
            return Size(*ui.dialog_size)

        def SetSizer(self, sizer):
            self._sizer = sizer
            sizer.owner = self

        def SetSizerAndFit(self, sizer):
            self._sizer = sizer

        def GetSizer(self):
            return self._sizer

        def Bind(self, event, handler, source=None):
            self._handlers.setdefault(event, []).append(handler)

        def ShowModal(self):
            ui.dialogs.append(self)
            for n in range(5000):
                if self._ended is not None:
                    break
                if ui.on_modal:
                    ui.on_modal(self, n)
                if self._ended is not None:
                    break
                for handler in self._handlers.get("EVT_TIMER", []):
                    handler(None)
                time.sleep(0.01)
            assert self._ended is not None, "the modal dialog never ended"
            return self._ended

        def EndModal(self, code):
            self._ended = code

        def Destroy(self):
            pass

        def __enter__(self):
            return self

        def __exit__(self, *exc):
            return False

        def __getattr__(self, name):
            if name.startswith("__"):
                raise AttributeError(name)
            return lambda *a, **k: None

    class Notebook(Dialog):
        def __init__(self, parent=None, *args, **kwargs):
            super().__init__(parent)
            self.pages = []  # (page, title)

        def AddPage(self, page, title):
            self.pages.append((page, title))
            page.title = title

    class Button(Control):
        def __init__(self, parent=None, id=None, label="", **kwargs):  # wx.Button(parent, id, label)
            super().__init__(parent, label=label, **kwargs)
            ui.buttons.append(self)

    class Display:
        def __init__(self, index=0):
            pass

        @staticmethod
        def GetFromWindow(window):
            return 0

        def GetClientArea(self):
            return Size(1920, ui.work_area_height)

    class StaticBoxSizer:  # a ContentSizer with its box, a child of `parent`
        def __new__(cls, orient=0, parent=None, label=""):
            sizer = ContentSizer()
            box = Dialog(parent)
            box.label = label
            sizer.GetStaticBox = lambda: box
            return sizer

    class ContentSizer(Sizer):
        def GetMinSize(self):  # a page's contents: ui.page_sizes[title] or ui.page_size
            title = getattr(getattr(self, "owner", None), "title", None)
            return Size(*ui.page_sizes.get(title, ui.page_size))

    wx.Dialog = Dialog
    wx.Panel = Dialog
    wx.ScrolledWindow = Dialog
    wx.Notebook = Notebook
    wx.Button = Button
    wx.Display = Display
    wx.Size = Size
    wx.SystemSettings = types.SimpleNamespace(GetMetric=lambda metric: ui.scrollbar_width)
    wx.SpinCtrl = Control
    wx.TextCtrl = TextControl
    wx.CheckBox = CheckBox
    wx.Choice = Choice
    for name in ("StaticText", "Gauge"):
        setattr(wx, name, Control)
    for name in ("BoxSizer", "FlexGridSizer"):
        setattr(wx, name, ContentSizer)
    wx.StaticBoxSizer = StaticBoxSizer
    wx.Timer = Timer
    wx.EVT_BUTTON, wx.EVT_TIMER, wx.EVT_CLOSE = "EVT_BUTTON", "EVT_TIMER", "EVT_CLOSE"
    wx.ID_OK, wx.ID_CANCEL = 5100, 5101
    wx.ICON_EXCLAMATION, wx.ICON_INFORMATION, wx.ICON_ERROR = 0x200, 0x800, 0x200 | 0x400
    wx.OK = 4

    def message_box(text, caption="", style=0, parent=None):
        ui.messages.append((text, style))

    wx.MessageBox = message_box
    wx.__getattr__ = lambda name: 0  # remaining constants (sizer flags, styles)
    return wx


def build_pcbnew(ui, dsn_text):
    pcbnew = types.ModuleType("pcbnew")

    class ActionPlugin:
        def register(self):
            pass

    def export(board, path):
        ui.exports.append(path)
        Path(path).write_text(dsn_text, encoding="utf-8")
        return True

    def import_ses(board, path):
        """Like KiCad 10's DSN::ImportSpecctraSession: on success, every unlocked
        track still on the board is deleted (marked `freed`) and the session's
        tracks are added."""
        ui.imports.append(Path(path))
        if ui.import_ok and isinstance(board, FakeBoard):
            for track in [t for t in board.tracks if not t.locked]:
                board.tracks.remove(track)
                track.freed = True
            board.tracks.append(FakeTrack("routed"))
        return ui.import_ok

    def save_board(path, board, skip_settings=False):
        ui.snapshots.append((Path(path), skip_settings))
        Path(path).write_text("(kicad_pcb (version 20241229))\n", encoding="utf-8")
        return True

    class SettingsManager:
        def SaveProjectCopy(self, path, project):
            if ui.project_copy_fails:
                raise RuntimeError("no project")
            ui.project_copies.append(Path(path))
            Path(path).write_text('{"net_settings": {"live": true}}', encoding="utf-8")
            Path(path).with_suffix(".kicad_prl").write_text("{}", encoding="utf-8")

    pcbnew.SaveBoard = save_board
    pcbnew.GetSettingsManager = SettingsManager
    pcbnew.ActionPlugin = ActionPlugin
    pcbnew.F_Cu, pcbnew.B_Cu = 0, 2
    pcbnew.ToMM = lambda nm: nm / 1e6
    pcbnew.GetBuildVersion = lambda: "test KiCad"
    pcbnew.ExportSpecctraDSN = export
    pcbnew.ImportSpecctraSES = import_ses
    pcbnew.Refresh = lambda: None
    return pcbnew


class FakeTrack:
    def __init__(self, name, locked=False):
        self.name = name
        self.locked = locked
        self.freed = False  # deleted by the importer while KiCad still points at it

    def IsLocked(self):
        return self.locked

    def SetLocked(self, value):
        self.locked = value


class FakeBoard:
    def __init__(self, pcb_path, min_track_nm=150000, tracks=()):
        self.path = str(pcb_path)
        self.min_track_nm = min_track_nm
        self.tracks = list(tracks)

    def GetTracks(self):
        return list(self.tracks)

    def Remove(self, item):
        self.tracks.remove(item)

    def Add(self, item):
        self.tracks.append(item)

    def GetFileName(self):
        return self.path

    def GetProject(self):
        return object()

    def GetDesignSettings(self):
        return types.SimpleNamespace(m_TrackMinWidth=self.min_track_nm, m_CopperEdgeClearance=0)

    def Zones(self):
        return []

    def GetFootprints(self):
        return []

    def GetBoardPolygonOutlines(self, outline, *args):
        return False


def install(monkeypatch):
    ui = UI()
    monkeypatch.setitem(sys.modules, "wx", build_wx(ui))
    dsn_text = (TESTS / "data" / "five_areas.dsn").read_text(encoding="utf-8")
    monkeypatch.setitem(sys.modules, "pcbnew", build_pcbnew(ui, dsn_text))
    package = types.ModuleType("freerouting_rs")
    package.__path__ = [str(PLUGIN_SRC)]
    monkeypatch.setitem(sys.modules, "freerouting_rs", package)
    for name in list(sys.modules):
        if name.startswith("freerouting_rs."):
            monkeypatch.delitem(sys.modules, name)
    plugin = importlib.import_module("freerouting_rs.plugin")
    return plugin, ui

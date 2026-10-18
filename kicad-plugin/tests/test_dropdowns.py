"""dropdowns.py: when the plugin re-themes its drop-down lists, and the Win32
calls it makes. The calls go to fakes; the real ones were checked on Dan's
Windows 11 26200 with KiCad 10.0's wxWidgets 3.3.2 (kicad-plugin.md)."""

import importlib
import sys
import types
from types import SimpleNamespace

import pytest

WINDOWS_ARGS = dict(platform="win32", wx_version=(3, 3, 2), has_dark_theme=True, dark=True)


@pytest.fixture
def dropdowns(monkeypatch):
    monkeypatch.setitem(sys.modules, "wx", types.ModuleType("wx"))
    monkeypatch.delitem(sys.modules, "dropdowns", raising=False)
    return importlib.import_module("dropdowns")


def fake_wx(wx_version=(3, 3, 2), has_dark_theme=True, dark=True):
    info = SimpleNamespace(
        GetMajor=lambda: wx_version[0], GetMinor=lambda: wx_version[1], GetMicro=lambda: wx_version[2]
    )
    appearance = SimpleNamespace(IsUsingDarkBackground=lambda: dark)
    return SimpleNamespace(
        GetLibraryVersionInfo=lambda: info,
        CheckOsVersion=lambda *v: has_dark_theme and v == (10, 0, 26200),
        SystemSettings=SimpleNamespace(GetAppearance=lambda: appearance),
    )


class FakeWin32:
    """user32 and uxtheme. `lists` maps a combo handle to its list window;
    a handle not in it fails GetComboBoxInfo."""

    def __init__(self, lists, theme_result=0):
        self.lists = lists
        self.theme_result = theme_result
        self.themed = []
        self.sent = []
        self.calls = 0

    def __call__(self):
        self.calls += 1
        user32 = SimpleNamespace(GetComboBoxInfo=self.combo_info, SendMessageW=self.send)
        return user32, SimpleNamespace(SetWindowTheme=self.set_theme)

    def combo_info(self, handle, ref):
        if handle not in self.lists:
            return 0
        ref._obj.hwndList = self.lists[handle]
        return 1

    def set_theme(self, hwnd, name, sub_id):
        self.themed.append((hwnd, name, sub_id))
        return self.theme_result

    def send(self, hwnd, message, wparam, lparam):
        self.sent.append((hwnd, message))


def choice(handle):
    return SimpleNamespace(GetHandle=lambda: handle)


def on_windows(monkeypatch, dropdowns, **wx_args):
    monkeypatch.setattr(sys, "platform", "win32")
    monkeypatch.setattr(dropdowns, "wx", fake_wx(**wx_args))


def test_needs_fix_only_for_dark_windows_26200_with_old_wx(dropdowns):
    assert dropdowns.needs_fix(**WINDOWS_ARGS)
    assert dropdowns.needs_fix(**{**WINDOWS_ARGS, "wx_version": (3, 2, 9)})
    for change in (
        {"platform": "linux"},
        {"platform": "darwin"},
        {"dark": False},
        {"has_dark_theme": False},
        {"wx_version": (3, 3, 3)},  # wxWidgets themes the list itself
        {"wx_version": (3, 4, 0)},
    ):
        assert not dropdowns.needs_fix(**{**WINDOWS_ARGS, **change}), change


def test_does_nothing_off_windows(dropdowns):
    win32 = FakeWin32({1: 101})
    assert dropdowns.fix_dark_lists([choice(1)], win32=win32) == 0
    assert win32.calls == 0


@pytest.mark.parametrize(
    "wx_args",
    [{"dark": False}, {"has_dark_theme": False}, {"wx_version": (3, 3, 3)}],
)
def test_does_nothing_when_not_needed(monkeypatch, dropdowns, wx_args):
    on_windows(monkeypatch, dropdowns, **wx_args)
    win32 = FakeWin32({1: 101})
    assert dropdowns.fix_dark_lists([choice(1)], win32=win32) == 0
    assert win32.calls == 0


def test_gives_each_list_the_dark_theme(monkeypatch, dropdowns):
    on_windows(monkeypatch, dropdowns)
    win32 = FakeWin32({1: 101, 2: 102})
    assert dropdowns.fix_dark_lists([choice(1), choice(2)], win32=win32) == 2
    assert win32.themed == [(101, "DarkMode_DarkTheme", None), (102, "DarkMode_DarkTheme", None)]
    assert win32.sent == [(101, dropdowns.WM_THEMECHANGED), (102, dropdowns.WM_THEMECHANGED)]


def test_skips_a_choice_without_a_list_or_a_theme(monkeypatch, dropdowns):
    on_windows(monkeypatch, dropdowns)
    win32 = FakeWin32({1: 101, 3: 0})  # 2: GetComboBoxInfo fails; 3: no list window
    assert dropdowns.fix_dark_lists([choice(1), choice(2), choice(3)], win32=win32) == 1
    assert [hwnd for hwnd, _, _ in win32.themed] == [101]

    failing = FakeWin32({1: 101}, theme_result=-2147467259)  # E_FAIL
    assert dropdowns.fix_dark_lists([choice(1)], win32=failing) == 0
    assert failing.sent == []


def test_missing_win32_leaves_the_lists_alone(monkeypatch, dropdowns):
    on_windows(monkeypatch, dropdowns)

    def no_win32():
        raise OSError("uxtheme.dll not found")

    assert dropdowns.fix_dark_lists([choice(1)], win32=no_win32) == 0

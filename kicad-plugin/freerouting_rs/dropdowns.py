"""Readable drop-down lists in KiCad's Windows dark mode.

wxWidgets 3.3.2, which KiCad 10.0 bundles, gives the drop-down list of a
`wx.Choice` the "Explorer" theme in dark mode. Recent Windows 11 builds
(26200.9711 on Dan's machine; 26300.8553 on Insider, where it was first
seen) draw that theme's selected row white under the list's white text, so
the row under the mouse can't be read (wxWidgets #26535, KiCad #23570).
wxWidgets 3.3.3 gives the list "DarkMode_DarkTheme" instead, on the builds
that have it (26200 and later). `fix_dark_lists` does the same for the
plugin's own choices while KiCad bundles an older wxWidgets.

Elsewhere (Linux, macOS, light mode, older Windows, wxWidgets 3.3.3 or
later) it does nothing. A Win32 call that fails leaves the list as
wxWidgets drew it: the dialog still works, only the highlight is unreadable.
"""

import ctypes
import sys

import wx

FIXED_IN = (3, 3, 3)  # wxWidgets themes the list itself from this version on
DARK_THEME_BUILD = 26200  # the first Windows build with DarkMode_DarkTheme
DARK_THEME = "DarkMode_DarkTheme"
WM_THEMECHANGED = 0x031A


def needs_fix(platform, wx_version, has_dark_theme, dark):
    """True when the drop-down lists need the dark theme (see the module
    docs). `wx_version` is wxWidgets' (major, minor, micro), not wxPython's;
    `has_dark_theme` is whether Windows is build 26200 or later."""
    return platform == "win32" and dark and has_dark_theme and tuple(wx_version) < FIXED_IN


class _Rect(ctypes.Structure):
    _fields_ = [(n, ctypes.c_long) for n in ("left", "top", "right", "bottom")]


class _ComboBoxInfo(ctypes.Structure):
    _fields_ = [
        ("cbSize", ctypes.c_ulong),
        ("rcItem", _Rect),
        ("rcButton", _Rect),
        ("stateButton", ctypes.c_ulong),
        ("hwndCombo", ctypes.c_void_p),
        ("hwndItem", ctypes.c_void_p),
        ("hwndList", ctypes.c_void_p),
    ]


def _win32():
    """user32 and uxtheme with the signatures used here."""
    user32 = ctypes.windll.user32
    uxtheme = ctypes.windll.uxtheme
    user32.GetComboBoxInfo.argtypes = [ctypes.c_void_p, ctypes.POINTER(_ComboBoxInfo)]
    user32.SendMessageW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p]
    uxtheme.SetWindowTheme.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_wchar_p]
    return user32, uxtheme


def _running_needs_fix():
    """`needs_fix` for this process; only called on Windows. The build
    check is wxWidgets' own (`HasDarkTheme`): `sys.getwindowsversion()`'s
    `platform_version` reads 26100 on 25H2, which is build 26200."""
    info = wx.GetLibraryVersionInfo()
    version = (info.GetMajor(), info.GetMinor(), info.GetMicro())
    has_dark_theme = wx.CheckOsVersion(10, 0, DARK_THEME_BUILD)
    dark = wx.SystemSettings.GetAppearance().IsUsingDarkBackground()
    return needs_fix(sys.platform, version, has_dark_theme, dark)


def fix_dark_lists(choices, win32=_win32):
    """Gives the drop-down list of each `wx.Choice` the dark theme when
    `needs_fix`. Returns how many lists were changed."""
    if sys.platform != "win32" or not _running_needs_fix():
        return 0
    try:
        user32, uxtheme = win32()
    except (OSError, AttributeError):
        return 0
    changed = 0
    for choice in choices:
        info = _ComboBoxInfo()
        info.cbSize = ctypes.sizeof(info)
        if not user32.GetComboBoxInfo(choice.GetHandle(), ctypes.byref(info)) or not info.hwndList:
            continue
        if uxtheme.SetWindowTheme(info.hwndList, DARK_THEME, None) != 0:  # an HRESULT
            continue
        user32.SendMessageW(info.hwndList, WM_THEMECHANGED, None, None)
        changed += 1
    return changed

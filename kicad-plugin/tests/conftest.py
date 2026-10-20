"""Puts freerouting_rs/ on sys.path so tools, rule_areas, version, kicad_areas
import as top-level modules. The package __init__ (which needs pcbnew and wx
and registers the plugin) never runs."""

import os
import stat
import sys
from pathlib import Path

import pytest

TESTS = Path(__file__).resolve().parent
PLUGIN_SRC = TESTS.parent / "freerouting_rs"
REPO = TESTS.parent.parent
FAKE_BIN = TESTS / "fake_bin"
sys.path.insert(0, str(PLUGIN_SRC))
sys.path.insert(0, str(TESTS))  # install.py's parent is added by test_install


@pytest.fixture
def make_fake_router(tmp_path):
    """Returns make(mode, name="freerouting-rs") -> path of an executable that
    behaves like fake_bin/fake_freerouting.py in `mode`."""

    def make(mode, name="freerouting-rs"):
        script = FAKE_BIN / "fake_freerouting.py"
        d = tmp_path / f"fake-{mode}"
        d.mkdir(exist_ok=True)
        if os.name == "nt":
            exe = d / (name + ".cmd")
            exe.write_text(f'@"{sys.executable}" "{script}" {mode} %*\r\n')
        else:
            exe = d / name
            exe.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{script}" {mode} "$@"\n')
            exe.chmod(exe.stat().st_mode | stat.S_IXUSR)
        return exe

    return make


@pytest.fixture
def make_fake_gui(tmp_path):
    """Returns make(mode) -> path of an executable that behaves like
    fake_bin/fake_gui.py in `mode`."""

    def make(mode):
        script = FAKE_BIN / "fake_gui.py"
        d = tmp_path / f"fakegui-{mode}"
        d.mkdir(exist_ok=True)
        if os.name == "nt":
            exe = d / "freerouting-gui.cmd"
            exe.write_text(f'@"{sys.executable}" "{script}" {mode} %*\r\n')
        else:
            exe = d / "freerouting-gui"
            exe.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{script}" {mode} "$@"\n')
            exe.chmod(exe.stat().st_mode | stat.S_IXUSR)
        return exe

    return make


@pytest.fixture
def isolated_env(tmp_path, monkeypatch):
    """No settings.json, no FREEROUTING_RS_BIN, an empty PATH and home."""
    import tools

    monkeypatch.setattr(tools, "SETTINGS_FILE", tmp_path / "no-settings.json")
    monkeypatch.delenv("FREEROUTING_RS_BIN", raising=False)
    monkeypatch.setenv("PATH", str(tmp_path / "empty-path"))
    home = tmp_path / "home"
    home.mkdir()
    monkeypatch.setattr(Path, "home", classmethod(lambda cls: home))
    return tmp_path


CANARY_PRESET_FILE = REPO / "fixtures" / "canary-pwr" / "presets" / "canary-graded-flow.json"


def canary_preset_options(current=None):
    """The options of the canary-pwr preset file (a benchmark input, not
    plugin code), applied on `current` or the defaults."""
    import options
    import presets

    preset, warning = presets.parse(CANARY_PRESET_FILE.read_text(encoding="utf-8"), CANARY_PRESET_FILE, "board")
    assert warning is None, warning
    merged, warnings = presets.apply(preset, current or options.RouteOptions())
    assert warnings == [], warnings
    return merged

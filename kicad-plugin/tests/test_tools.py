import subprocess
import sys
from pathlib import Path

import pytest
import tools


def touch(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("")
    return path


def exe(d, name="freerouting"):
    return Path(d) / (name + tools.EXE_SUFFIX)


# --- lookup order -----------------------------------------------------------


def test_nothing_found(isolated_env):
    assert tools.find_tool("freerouting") is None


def test_lookup_order_each_source_wins_over_later_ones(isolated_env, monkeypatch):
    tmp = isolated_env
    settings_dir, env_dir, path_dir = tmp / "settings", tmp / "env", tmp / "path"
    cargo = touch(exe(tmp / "home" / ".cargo" / "bin"))
    monkeypatch.setattr(tools, "load_settings", lambda: {"bin_dir": str(settings_dir)})
    monkeypatch.setenv("FREEROUTING_RS_BIN", str(env_dir))
    monkeypatch.setenv("PATH", str(path_dir))
    on_path = exe(path_dir)
    on_path.parent.mkdir()
    on_path.write_text("#!/bin/sh\n")
    on_path.chmod(0o755)

    # ~/.cargo/bin is the last native source, so PATH must beat it:
    assert Path(tools.find_tool("freerouting").exe).resolve() == on_path.resolve()
    # FREEROUTING_RS_BIN beats PATH
    in_env = touch(exe(env_dir))
    assert tools.find_tool("freerouting").exe == in_env
    # settings.json bin_dir beats FREEROUTING_RS_BIN
    in_settings = touch(exe(settings_dir))
    assert tools.find_tool("freerouting").exe == in_settings
    # remove the winners one by one: each falls back to the next source
    in_settings.unlink()
    assert tools.find_tool("freerouting").exe == in_env
    in_env.unlink()
    assert Path(tools.find_tool("freerouting").exe).resolve() == on_path.resolve()
    on_path.unlink()
    assert tools.find_tool("freerouting").exe == cargo


def test_other_tool_name_is_not_confused(isolated_env, monkeypatch):
    d = isolated_env / "bin"
    touch(exe(d, "freerouting"))
    monkeypatch.setenv("FREEROUTING_RS_BIN", str(d))
    assert tools.find_tool("freerouting-gui") is None
    assert tools.find_tool("freerouting") is not None


def test_wsl_fallback_only_on_windows(isolated_env, monkeypatch):
    monkeypatch.setattr(tools, "load_settings", lambda: {"wsl_bin_dir": "/opt/fr/", "wsl_distro": "Ubuntu"})
    monkeypatch.setattr(tools, "IS_WINDOWS", False)
    assert tools.find_tool("freerouting") is None
    monkeypatch.setattr(tools, "IS_WINDOWS", True)
    tool = tools.find_tool("freerouting")
    assert tool.via_wsl and tool.exe == "/opt/fr/freerouting" and tool.wsl_distro == "Ubuntu"


def test_native_beats_wsl_on_windows(isolated_env, monkeypatch):
    d = isolated_env / "bin"
    native = touch(exe(d))
    monkeypatch.setattr(tools, "load_settings", lambda: {"bin_dir": str(d), "wsl_bin_dir": "/opt/fr"})
    monkeypatch.setattr(tools, "IS_WINDOWS", True)
    tool = tools.find_tool("freerouting")
    assert not tool.via_wsl and tool.exe == native


def test_load_settings_reads_json_and_survives_garbage(tmp_path, monkeypatch):
    f = tmp_path / "settings.json"
    monkeypatch.setattr(tools, "SETTINGS_FILE", f)
    assert tools.load_settings() == {}
    f.write_text("{not json")
    assert tools.load_settings() == {}
    f.write_text('{"bin_dir": "x"}')
    assert tools.load_settings() == {"bin_dir": "x"}


# --- WSL path translation ---------------------------------------------------


def test_windows_to_wsl_path_drive_letters(monkeypatch):
    # Path.resolve() on POSIX would treat "C:\..." as a relative name; the
    # translation itself is what is under test, so feed it resolved strings.
    class P:
        def __init__(self, s):
            self.s = s

        def resolve(self):
            return self.s

        def __str__(self):
            return self.s

    monkeypatch.setattr(tools, "Path", P)
    assert tools.windows_to_wsl_path(r"C:\Users\me\b.dsn") == "/mnt/c/Users/me/b.dsn"
    assert tools.windows_to_wsl_path("D:/x y/b.dsn") == "/mnt/d/x y/b.dsn"


def test_unc_path_raises(monkeypatch):
    class P:
        def __init__(self, s):
            self.s = s

        def resolve(self):
            return self.s

        def __str__(self):
            return self.s

    monkeypatch.setattr(tools, "Path", P)
    with pytest.raises(ValueError):
        tools.windows_to_wsl_path(r"\\server\share\b.dsn")


# --- Tool.command -----------------------------------------------------------


def test_native_command_stringifies(tmp_path):
    tool = tools.Tool("freerouting", tmp_path / "freerouting")
    assert tool.command(["-de", tmp_path / "a.dsn", 5]) == [str(tmp_path / "freerouting"), "-de", str(tmp_path / "a.dsn"), "5"]
    assert not tool.via_wsl


def test_wsl_command_translates_paths_and_picks_distro(monkeypatch):
    monkeypatch.setattr(tools, "windows_to_wsl_path", lambda p: "/mnt/c/" + Path(p).name)
    tool = tools.Tool("freerouting", "/opt/fr/freerouting", wsl_distro="Ubuntu")
    cmd = tool.command(["-de", Path("b.dsn"), "-mp", 3])
    assert cmd == ["wsl.exe", "-d", "Ubuntu", "--", "/opt/fr/freerouting", "-de", "/mnt/c/b.dsn", "-mp", "3"]
    default = tools.Tool("freerouting", "/opt/fr/freerouting", wsl_distro="")
    assert default.via_wsl and default.command(["x"])[:3] == ["wsl.exe", "--", "/opt/fr/freerouting"]
    assert "default distro" in default.describe()


# --- finish_route -----------------------------------------------------------


class FakeProc:
    def __init__(self, raw, returncode=0):
        self.raw, self.returncode = raw, returncode

    def communicate(self):
        return self.raw, None


def test_finish_route_decodes_utf8():
    tool = tools.Tool("freerouting", Path("fr"))
    assert tools.finish_route(tool, FakeProc("routed \u00e4\u00f6\n".encode("utf-8"), 0)) == (0, "routed \u00e4\u00f6")


def test_finish_route_decodes_wsl_utf16():
    tool = tools.Tool("freerouting", "/opt/fr", wsl_distro="")
    raw = "Wsl/Service/WSL_E_DISTRO_NOT_FOUND\r\n".encode("utf-16-le")
    code, out = tools.finish_route(tool, FakeProc(raw, 1))
    assert (code, out) == (1, "Wsl/Service/WSL_E_DISTRO_NOT_FOUND")


def test_finish_route_native_tool_never_tries_utf16():
    tool = tools.Tool("freerouting", Path("fr"))
    raw = "ab".encode("utf-16-le")
    assert "\x00" in tools.finish_route(tool, FakeProc(raw))[1]


def test_start_and_finish_route_with_fake_binary(make_fake_router, tmp_path):
    fake = make_fake_router("echo")
    tool = tools.Tool("freerouting", fake)
    proc = tools.start_route(tool, tmp_path / "a.dsn", tmp_path / "a.ses")
    code, out = tools.finish_route(tool, proc)
    assert code == 0
    assert out.splitlines() == ["-de", str(tmp_path / "a.dsn"), "-do", str(tmp_path / "a.ses")]


# --- per-board settings and log -----------------------------------------------


def test_board_settings_round_trip(tmp_path):
    work = tmp_path / "freerouting-rs"
    assert tools.load_board_settings(work) == {}
    tools.save_board_settings(work, {"fix_rule_areas": False})
    assert tools.load_board_settings(work) == {"fix_rule_areas": False}
    (work / tools.BOARD_SETTINGS_FILE).write_text("[1]")
    assert tools.load_board_settings(work) == {}


def test_log_line_appends(tmp_path):
    work = tmp_path / "freerouting-rs"
    tools.log_line(work, "one")
    tools.log_line(work, "two\n")
    assert (work / tools.PLUGIN_LOG).read_text().splitlines() == ["one", "two"]

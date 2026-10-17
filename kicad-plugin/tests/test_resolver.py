"""K03.3: the binaries bundled in the PCM package (`bin/<os>-<arch>/`)."""

import importlib.util
import os
import stat
import sys
import zipfile
from pathlib import Path

import pytest
import tools
from conftest import PLUGIN_SRC

sys.path.insert(0, str(PLUGIN_SRC.parent))
import package  # noqa: E402

CASES = [
    # (sys.platform, platform.machine(), expected bin/ sub-folder)
    ("win32", "AMD64", "windows-x86_64"),
    ("win32", "x86_64", "windows-x86_64"),
    ("darwin", "arm64", "macos-universal"),
    ("darwin", "x86_64", "macos-universal"),
    ("linux", "x86_64", "linux-x86_64"),
    ("linux", "aarch64", None),  # unsupported
    ("win32", "ARM64", None),
    ("freebsd14", "amd64", None),
]


@pytest.fixture
def bundle(tmp_path, monkeypatch):
    """A package-style `bin/` with all three platform folders, each holding
    non-executable `freerouting` and `freerouting-gui` files."""
    root = tmp_path / "pkg" / "bin"
    for platform in tools.BUNDLE_PLATFORMS:
        for name in (tools.ROUTER, tools.VIEWER):
            f = root / platform / (name + (".exe" if platform.startswith("windows") else ""))
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_text("#!/bin/sh\n")
            f.chmod(0o644)
    monkeypatch.setattr(tools, "BUNDLE_DIR", root)
    return root


def fake_system(monkeypatch, sys_platform, machine):
    monkeypatch.setattr(tools.sys, "platform", sys_platform)
    monkeypatch.setattr(tools.platform, "machine", lambda: machine)


def test_bundle_platform_names_match_the_packager():
    assert tools.BUNDLE_PLATFORMS == package.PLATFORMS
    assert tools.ROUTER in package.TOOLS and tools.VIEWER in package.TOOLS


@pytest.mark.parametrize("sys_platform,machine,expected", CASES)
def test_bundled_bin_dir_per_system(bundle, monkeypatch, sys_platform, machine, expected):
    fake_system(monkeypatch, sys_platform, machine)
    assert tools.bundle_platform() == expected
    got = tools.bundled_bin_dir()
    assert got == (bundle / expected if expected else None)


def test_no_bundle_folder_means_none(tmp_path, monkeypatch):
    """An install.py copy has no bin/ at all."""
    monkeypatch.setattr(tools, "BUNDLE_DIR", tmp_path / "nothing")
    fake_system(monkeypatch, "linux", "x86_64")
    assert tools.bundled_bin_dir() is None


def test_package_without_this_platform_is_none(bundle, monkeypatch):
    """A partial package (--allow-missing): the folder for this system is absent."""
    import shutil

    shutil.rmtree(bundle / "linux-x86_64")
    fake_system(monkeypatch, "linux", "x86_64")
    assert tools.bundled_bin_dir() is None


@pytest.mark.skipif(os.name == "nt", reason="no executable bit on Windows")
def test_missing_exec_bit_is_set(bundle, monkeypatch):
    fake_system(monkeypatch, "linux", "x86_64")
    router = bundle / "linux-x86_64" / "freerouting"
    assert not os.access(router, os.X_OK)
    tools.bundled_bin_dir()
    for name in ("freerouting", "freerouting-gui"):
        mode = (bundle / "linux-x86_64" / name).stat().st_mode
        assert mode & stat.S_IXUSR and os.access(bundle / "linux-x86_64" / name, os.X_OK)
    # other platforms' folders are not touched
    assert not os.access(bundle / "macos-universal" / "freerouting", os.X_OK)


@pytest.mark.skipif(os.name == "nt", reason="no executable bit on Windows")
def test_macos_gets_the_exec_bit_too(bundle, monkeypatch):
    fake_system(monkeypatch, "darwin", "arm64")
    tools.bundled_bin_dir()
    assert os.access(bundle / "macos-universal" / "freerouting-gui", os.X_OK)


def test_windows_is_left_alone(bundle, monkeypatch):
    fake_system(monkeypatch, "win32", "AMD64")
    assert tools.bundled_bin_dir() == bundle / "windows-x86_64"


def test_lookup_order_with_the_bundle(isolated_env, bundle, monkeypatch):
    """bin_dir > bundled > FREEROUTING_RS_BIN > PATH > ~/.cargo/bin."""
    tmp = isolated_env
    fake_system(monkeypatch, "linux", "x86_64")
    settings_dir, env_dir, path_dir = tmp / "settings", tmp / "env", tmp / "path"
    cargo = tmp / "home" / ".cargo" / "bin" / "freerouting"
    cargo.parent.mkdir(parents=True)
    cargo.write_text("")
    monkeypatch.setattr(tools, "load_settings", lambda: {"bin_dir": str(settings_dir)})
    monkeypatch.setenv("FREEROUTING_RS_BIN", str(env_dir))
    monkeypatch.setenv("PATH", str(path_dir))
    on_path = path_dir / "freerouting"
    on_path.parent.mkdir()
    on_path.write_text("#!/bin/sh\n")
    on_path.chmod(0o755)
    in_env = env_dir / "freerouting"
    in_env.parent.mkdir()
    in_env.write_text("")
    in_settings = settings_dir / "freerouting"
    in_settings.parent.mkdir()
    in_settings.write_text("")
    bundled = bundle / "linux-x86_64" / "freerouting"

    assert tools.find_tool("freerouting").exe == in_settings  # a developer's explicit bin_dir wins
    in_settings.unlink()
    assert tools.find_tool("freerouting").exe == bundled  # then the package's binaries
    assert tools.find_tool("freerouting-gui").exe == bundle / "linux-x86_64" / "freerouting-gui"
    bundled.unlink()
    assert tools.find_tool("freerouting").exe == in_env
    in_env.unlink()
    assert Path(tools.find_tool("freerouting").exe).resolve() == on_path.resolve()


def test_unsupported_platform_message(bundle, monkeypatch):
    fake_system(monkeypatch, "linux", "aarch64")
    msg = tools.missing_router_hint()
    for platform in tools.BUNDLE_PLATFORMS:
        assert platform in msg
    assert "aarch64" in msg
    assert "bin_dir" in msg and "cargo build" in msg


def test_hint_without_a_bundle_only_says_how_to_point_at_a_build(tmp_path, monkeypatch):
    monkeypatch.setattr(tools, "BUNDLE_DIR", tmp_path / "nothing")
    fake_system(monkeypatch, "linux", "aarch64")
    msg = tools.missing_router_hint()
    assert "bin_dir" in msg and "carries binaries" not in msg


@pytest.mark.skipif(os.name == "nt", reason="Linux layout")
def test_resolver_works_on_an_unpacked_package(tmp_path, monkeypatch):
    """Build the package, unpack it the way a plugin manager does (no mode bits kept),
    load the unpacked tools.py, and find the bundled router."""
    bins = tmp_path / "bins"
    for platform in package.PLATFORMS:
        for tool in package.TOOLS:
            f = bins / platform / package.exe_name(tool, platform)
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_text("#!/bin/sh\necho freerouting 0.1.0\n")
    zip_path, _ = package.build([str(bins)], tmp_path / "out")
    unpacked = tmp_path / "unpacked"
    with zipfile.ZipFile(zip_path) as zf:
        for info in zf.infolist():  # extract() drops the Unix mode bits, like wxZip does
            zf.extract(info, unpacked)
    plugin_dir = unpacked / "plugins"
    assert not os.access(plugin_dir / "bin" / "linux-x86_64" / "freerouting", os.X_OK)

    spec = importlib.util.spec_from_file_location("tools_unpacked", plugin_dir / "tools.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    monkeypatch.setattr(mod.sys, "platform", "linux")
    monkeypatch.setattr(mod.platform, "machine", lambda: "x86_64")
    monkeypatch.setattr(mod, "SETTINGS_FILE", tmp_path / "no-settings.json")
    monkeypatch.delenv("FREEROUTING_RS_BIN", raising=False)

    tool = mod.find_tool("freerouting")
    assert tool.exe == plugin_dir / "bin" / "linux-x86_64" / "freerouting"
    assert os.access(tool.exe, os.X_OK)

"""Finding and launching the freerouting-rs executables.

Kept free of pcbnew and wx imports so it can be exercised outside KiCad.

Each tool (`freerouting` for headless routing, `freerouting-gui` for the
viewer) is looked up in this order:

1. `bin_dir` from settings.json next to this file (written by install.py),
2. the `FREEROUTING_RS_BIN` environment variable,
3. the PATH,
4. ~/.cargo/bin,
5. on Windows only, `wsl_bin_dir` from settings.json: a Linux build inside
   WSL, run through wsl.exe (the viewer then shows up through WSLg).
"""

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SETTINGS_FILE = HERE / "settings.json"

ROUTER = "freerouting"
VIEWER = "freerouting-gui"

IS_WINDOWS = sys.platform == "win32"
EXE_SUFFIX = ".exe" if IS_WINDOWS else ""
# Keep a console window from flashing up for every headless run on Windows.
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def load_settings():
    try:
        with open(SETTINGS_FILE, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def windows_to_wsl_path(path):
    """C:\\Users\\me\\board.dsn -> /mnt/c/Users/me/board.dsn"""
    p = str(Path(path).resolve())
    m = re.match(r"^([A-Za-z]):[\\/](.*)$", p)
    if not m:
        raise ValueError(f"{p} is not on a drive WSL can see")
    return f"/mnt/{m.group(1).lower()}/" + m.group(2).replace("\\", "/")


class Tool:
    """A resolved executable. `command(args)` builds the argv; file arguments
    must be passed as Path objects so they can be translated for WSL."""

    def __init__(self, name, exe, wsl_distro=None):
        self.name = name
        self.exe = exe
        self.wsl_distro = wsl_distro

    @property
    def via_wsl(self):
        return self.wsl_distro is not None

    def describe(self):
        if self.via_wsl:
            return f"{self.exe} (WSL {self.wsl_distro or 'default distro'})"
        return str(self.exe)

    def command(self, args):
        if not self.via_wsl:
            return [str(self.exe)] + [str(a) for a in args]
        conv = [windows_to_wsl_path(a) if isinstance(a, Path) else str(a) for a in args]
        cmd = ["wsl.exe"]
        if self.wsl_distro:
            cmd += ["-d", self.wsl_distro]
        return cmd + ["--", self.exe] + conv


def _native_candidates(settings):
    dirs = []
    if settings.get("bin_dir"):
        dirs.append(Path(settings["bin_dir"]))
    if os.environ.get("FREEROUTING_RS_BIN"):
        dirs.append(Path(os.environ["FREEROUTING_RS_BIN"]))
    return dirs


def find_tool(name):
    """Returns a Tool, or None when the executable can't be found anywhere."""
    settings = load_settings()
    for d in _native_candidates(settings):
        exe = d / (name + EXE_SUFFIX)
        if exe.is_file():
            return Tool(name, exe)
    on_path = shutil.which(name)
    if on_path:
        return Tool(name, Path(on_path))
    cargo = Path.home() / ".cargo" / "bin" / (name + EXE_SUFFIX)
    if cargo.is_file():
        return Tool(name, cargo)
    if IS_WINDOWS and settings.get("wsl_bin_dir"):
        # Not checked for existence: probing WSL here would mean starting
        # the VM every time the plugin dialog opens.
        exe = settings["wsl_bin_dir"].rstrip("/") + "/" + name
        return Tool(name, exe, wsl_distro=settings.get("wsl_distro", ""))
    return None


def start_route(tool, dsn, ses):
    """Starts headless routing of `dsn` into `ses`. Collect the result with
    `finish_route` once the process has exited (or been killed)."""
    return subprocess.Popen(
        tool.command(["-de", Path(dsn), "-do", Path(ses)]),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        stdin=subprocess.DEVNULL,
        creationflags=NO_WINDOW,
    )


def finish_route(tool, proc):
    """Returns (exit code, combined output) of a process from `start_route`."""
    raw, _ = proc.communicate()
    out = raw.decode("utf-8", errors="replace")
    # wsl.exe prints its own errors in UTF-16.
    if tool.via_wsl and "\x00" in out:
        out = raw.decode("utf-16-le", errors="replace")
    return proc.returncode, out.strip()


def open_viewer(tool, dsn, ses=None):
    """Starts the viewer without waiting for it to close."""
    args = [Path(dsn)] + ([Path(ses)] if ses else [])
    kwargs = {"stdin": subprocess.DEVNULL, "stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL}
    if IS_WINDOWS:
        kwargs["creationflags"] = NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        kwargs["start_new_session"] = True
    return subprocess.Popen(tool.command(args), **kwargs)

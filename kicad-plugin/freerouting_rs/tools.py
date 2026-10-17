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

try:  # as part of the plugin package
    from . import version
except ImportError:  # top-level import, as the tests do
    import version

HERE = Path(__file__).resolve().parent
SETTINGS_FILE = HERE / "settings.json"
BOARD_SETTINGS_FILE = "plugin.json"
PLUGIN_LOG = "plugin.log"

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


def load_board_settings(work_dir):
    """Per-board plugin settings from `<work_dir>/plugin.json` ({} if absent)."""
    try:
        with open(Path(work_dir) / BOARD_SETTINGS_FILE, encoding="utf-8") as f:
            data = json.load(f)
        return data if isinstance(data, dict) else {}
    except (OSError, ValueError):
        return {}


def save_board_settings(work_dir, settings):
    work_dir = Path(work_dir)
    work_dir.mkdir(parents=True, exist_ok=True)
    with open(work_dir / BOARD_SETTINGS_FILE, "w", encoding="utf-8") as f:
        json.dump(settings, f, indent=2)


def log_line(work_dir, text):
    """Appends one line to `<work_dir>/plugin.log`."""
    work_dir = Path(work_dir)
    work_dir.mkdir(parents=True, exist_ok=True)
    with open(work_dir / PLUGIN_LOG, "a", encoding="utf-8") as f:
        f.write(text.rstrip("\n") + "\n")


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


def route_args(dsn, ses, extra_args=(), result_json=None, progress=False):
    """The router's argument list: files, then `--progress json` and
    `--result-json` when asked for, then the option flags (K02)."""
    args = ["-de", Path(dsn), "-do", Path(ses)]
    if progress:
        args += ["--progress", "json"]
    if result_json:
        args += ["--result-json", Path(result_json)]
    return args + list(extra_args)


def start_route(tool, dsn, ses, extra_args=(), result_json=None, progress=False):
    """Starts headless routing of `dsn` into `ses`. Collect the result with
    `finish_route` once the process has exited (or been killed), or read
    `proc.stdout` line by line while it runs (`routing.RouteRun`).

    Standard input is a pipe: `stop_route` writes the router's `stop` line to
    it. Standard output carries stderr too (`--progress json` events and the
    human log)."""
    return subprocess.Popen(
        tool.command(route_args(dsn, ses, extra_args, result_json, progress)),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        stdin=subprocess.PIPE,
        creationflags=NO_WINDOW,
    )


def stop_route(proc):
    """Asks the router to finish the step it is in, write its best board and
    exit 0 (`stop` on standard input, C08.5). Returns False when the pipe is
    already closed (the process has ended)."""
    try:
        proc.stdin.write(b"stop\n")
        proc.stdin.flush()
        return True
    except (OSError, ValueError, AttributeError):
        return False


def start_gui(tool, dsn, ses, extra_args=()):
    """Starts the GUI in front-end mode, `freerouting-gui -de x.dsn -do x.ses`
    (K02.5), without waiting for it. Its window routes after the countdown
    and writes `ses` when routing ends or the window closes."""
    kwargs = {"stdin": subprocess.DEVNULL, "stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL}
    if IS_WINDOWS:
        kwargs["creationflags"] = NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        kwargs["start_new_session"] = True
    return subprocess.Popen(tool.command(route_args(dsn, ses, extra_args)), **kwargs)


def finish_route(tool, proc):
    """Returns (exit code, combined output) of a process from `start_route`."""
    raw, _ = proc.communicate()
    out = raw.decode("utf-8", errors="replace")
    # wsl.exe prints its own errors in UTF-16.
    if tool.via_wsl and "\x00" in out:
        out = raw.decode("utf-16-le", errors="replace")
    return proc.returncode, out.strip()


def query_version(tool, timeout=10):
    """Runs `<tool> --version` and returns the parsed version (see
    `version.parse_router_version`), or None when the executable fails, times
    out, cannot be started, or prints something else."""
    try:
        proc = subprocess.Popen(
            tool.command(["--version"]),
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            stdin=subprocess.DEVNULL,
            creationflags=NO_WINDOW,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    try:
        raw, _ = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        # Don't communicate() again after the kill: a grandchild (a wrapper
        # script's child) can keep the pipe open.
        proc.kill()
        proc.wait()
        if proc.stdout:
            proc.stdout.close()
        return None
    if proc.returncode != 0:
        return None
    return version.parse_router_version(raw.decode("utf-8", errors="replace"))


def open_viewer(tool, dsn, ses=None):
    """Starts the viewer without waiting for it to close."""
    args = [Path(dsn)] + ([Path(ses)] if ses else [])
    kwargs = {"stdin": subprocess.DEVNULL, "stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL}
    if IS_WINDOWS:
        kwargs["creationflags"] = NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        kwargs["start_new_session"] = True
    return subprocess.Popen(tool.command(args), **kwargs)

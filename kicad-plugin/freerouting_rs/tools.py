"""Finding and launching the freerouting-rs executables.

Kept free of pcbnew and wx imports so it can be exercised outside KiCad.

Each tool (`freerouting` for headless routing, `freerouting-gui` for the
viewer) is looked up in this order:

1. `bin_dir` from settings.json next to this file (written by install.py),
2. the binaries bundled in the PCM package, `bin/<os>-<arch>/` next to this
   file (K03.3; absent in an install.py copy),
3. the `FREEROUTING_RS_BIN` environment variable,
4. the PATH,
5. ~/.cargo/bin,
6. on Windows only, `wsl_bin_dir` from settings.json: a Linux build inside
   WSL, run through wsl.exe (the viewer then shows up through WSLg).
"""

import json
import os
import platform
import re
import shutil
import subprocess
import stat
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

# The PCM package (K03) carries both executables per platform under
# `<this folder>/bin/<BUNDLE_PLATFORMS entry>/`. `kicad-plugin/package.py`
# writes the same names.
BUNDLE_DIR = HERE / "bin"
BUNDLE_PLATFORMS = ("windows-x86_64", "macos-universal", "linux-x86_64")

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


def bundle_platform(sys_platform=None, machine=None):
    """The `bin/` subfolder name for a system, or None when the package
    carries no binaries for it. Defaults to this system. macOS maps to the
    universal build whatever the architecture; Windows and Linux need x86_64
    (`AMD64` on Windows, `x86_64` on Linux)."""
    sys_platform = sys.platform if sys_platform is None else sys_platform
    machine = (platform.machine() if machine is None else machine).lower()
    x86_64 = machine in ("amd64", "x86_64", "x64")
    if sys_platform == "win32":
        return "windows-x86_64" if x86_64 else None
    if sys_platform == "darwin":
        return "macos-universal"
    if sys_platform.startswith("linux"):
        return "linux-x86_64" if x86_64 else None
    return None


def _make_executable(folder):
    """Sets the executable bit on the tools in `folder` where it is missing:
    an unzip by the plugin manager may drop it (unverified). Errors are
    ignored; the run then fails with the system's own message."""
    for name in (ROUTER, VIEWER):
        exe = folder / name
        try:
            mode = exe.stat().st_mode
            if exe.is_file() and not mode & stat.S_IXUSR:
                exe.chmod(mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
        except OSError:
            pass


def bundled_bin_dir():
    """`bin/<os>-<arch>/` of the PCM package for this system, or None when
    this is not a package install or the package has no binaries for the
    system. On Linux and macOS it makes sure the tools are executable."""
    key = bundle_platform()
    if key is None:
        return None
    folder = BUNDLE_DIR / key
    if not folder.is_dir():
        return None
    if sys.platform != "win32":
        _make_executable(folder)
    return folder


def missing_router_hint():
    """One sentence for the dialog when no router was found: on a package
    install without binaries for this system, which systems the package
    carries; otherwise how to point the plugin at a local build."""
    build = (
        "Build it (cargo build --release -p freerouting-cli -p freerouting-gui) and set "
        f'"bin_dir" in {SETTINGS_FILE}, or set FREEROUTING_RS_BIN.'
    )
    if BUNDLE_DIR.is_dir() and bundle_platform() is None:
        return (
            f"This package carries binaries for {', '.join(BUNDLE_PLATFORMS)} only, "
            f"not for this system ({sys.platform}, {platform.machine() or 'unknown'}). " + build
        )
    return build


def _native_candidates(settings):
    dirs = []
    if settings.get("bin_dir"):
        dirs.append(Path(settings["bin_dir"]))
    bundled = bundled_bin_dir()
    if bundled:
        dirs.append(bundled)
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

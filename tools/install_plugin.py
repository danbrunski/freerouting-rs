"""Builds freerouting-rs and installs its KiCad plugin, setting up what the build needs.

    python tools/install_plugin.py              check the machine, offer fixes, build, install
    python tools/install_plugin.py --check      only report what is there and what is missing
    python tools/install_plugin.py --yes        apply every fix without asking

Other options: --skip-build (install the executables already built), --no-setup
(report problems but install nothing), --windows-kicad (from WSL: install into
the Windows KiCad, which then runs this Linux build through WSL). Options it
does not know (--kicad-version, --plugins-dir, --keep-pcm, --wsl-bin-dir
for a WSL fallback, ...) go to kicad-plugin/install.py. Any Python 3.8+ works, KiCad's own included.

What it checks, and offers to fix:
- Rust: cargo and a rustc at least the workspace's rust-version (rustup through
  winget on Windows, the rustup.rs script elsewhere; `rustup update` when old).
- A linker: on Windows the MSVC build tools, or else the windows-gnu toolchain
  with WinLibs' mingw-w64 (the dlltool that ships with Rust calls an `as` it
  does not bundle); a C compiler on Linux; the Xcode command line tools on macOS.
- Linux: the libraries the viewer loads when it starts (xkbcommon, Vulkan or
  EGL), from the distribution's packages (apt, dnf, pacman, zypper).
- KiCad's user folder (the native install, the Flatpak one, or from WSL the
  Windows one), Python, free disk space, and on Windows a viewer or router
  still running (Windows locks a running exe, so the build cannot replace it).

Afterwards, in KiCad's PCB editor: Tools > External Plugins > Refresh Plugins.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import List, NamedTuple, Optional

ROOT = Path(__file__).resolve().parent.parent
INSTALL = ROOT / "kicad-plugin" / "install.py"
PACKAGES = ("freerouting-cli", "freerouting-gui")
EXES = ("freerouting", "freerouting-gui")
MIN_PYTHON = (3, 8)
# A release build of both packages with its dependencies.
MIN_FREE_GB = 6
GNU_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"
WINLIBS_ID = "BrechtSanders.WinLibs.POSIX.MSVCRT"
RUSTUP_SH = "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal"
# Fix, check again; installing Rust can show a missing toolchain, then a linker.
FIX_ROUNDS = 3
WINGET_FLAGS = ["-e", "--accept-source-agreements", "--accept-package-agreements"]

# Linux package managers: the command that installs, and how /etc/os-release names the family.
PACKAGE_MANAGERS = {
    "apt": (["sudo", "apt-get", "install", "-y"], {"debian", "ubuntu"}),
    "dnf": (["sudo", "dnf", "install", "-y"], {"fedora", "rhel", "centos"}),
    "pacman": (["sudo", "pacman", "-S", "--needed", "--noconfirm"], {"arch"}),
    "zypper": (["sudo", "zypper", "install", "-y"], {"suse", "opensuse"}),
}
# A C compiler for the linker, and curl for the rustup script.
BUILD_PACKAGES = {
    "apt": ["build-essential", "curl"],
    "dnf": ["gcc", "curl"],
    "pacman": ["base-devel", "curl"],
    "zypper": ["gcc", "curl"],
}
# What the viewer loads at start (winit's keyboard handling): soname -> package.
RUNTIME_LIBS = {
    "libxkbcommon.so.0": {"apt": "libxkbcommon0", "dnf": "libxkbcommon", "pacman": "libxkbcommon", "zypper": "libxkbcommon0"},
    "libxkbcommon-x11.so.0": {
        "apt": "libxkbcommon-x11-0",
        "dnf": "libxkbcommon-x11",
        "pacman": "libxkbcommon-x11",
        "zypper": "libxkbcommon-x11-0",
    },
}
# wgpu draws with Vulkan, or with EGL (OpenGL ES) when there is no Vulkan; one is enough.
GRAPHICS_LIBS = ("libvulkan.so.1", "libEGL.so.1")
GRAPHICS_PACKAGES = {
    "apt": ["libvulkan1", "mesa-vulkan-drivers", "libegl1"],
    "dnf": ["vulkan-loader", "mesa-vulkan-drivers", "mesa-libEGL"],
    "pacman": ["vulkan-icd-loader", "mesa", "libglvnd"],
    "zypper": ["libvulkan1", "Mesa-libEGL1"],
}


class Host(NamedTuple):
    """The machine the script runs on."""

    system: str  # "windows", "linux" or "macos"
    wsl: bool
    package_manager: Optional[str]  # Linux only
    interactive: bool


class Problem(NamedTuple):
    """Something missing. `fix` is a command that installs it (run only after
    asking, or with --yes); `hint` says what to do by hand. A blocking problem
    stops the build or the install."""

    what: str
    hint: str
    fix: Optional[List[str]] = None
    blocking: bool = True


# --- small helpers ----------------------------------------------------------


def exe_name(name, windows):
    return f"{name}.exe" if windows else name


def run_quiet(cmd, env=None, cwd=None):
    """(return code, stdout) of `cmd`, or (None, "") when it cannot start."""
    try:
        done = subprocess.run(cmd, env=env, cwd=cwd, capture_output=True, text=True)
    except OSError:
        return None, ""
    return done.returncode, done.stdout


def version_tuple(text):
    return tuple(int(x) for x in re.findall(r"\d+", text)[:3])


def read_text(path):
    try:
        return Path(path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


# --- what the machine is ----------------------------------------------------


def is_wsl(proc_version):
    return "microsoft" in proc_version.lower()


def package_manager(os_release):
    """The package manager of the distribution `/etc/os-release` describes."""
    fields = dict(re.findall(r'^(\w+)="?([^"\n]*)"?$', os_release, re.MULTILINE))
    family = {fields.get("ID", "").lower()} | set(fields.get("ID_LIKE", "").lower().split())
    for name, (_, ids) in PACKAGE_MANAGERS.items():
        if family & ids or any(i.startswith("opensuse") and "suse" in ids for i in family):
            return name
    return None


def detect_host():
    if sys.platform == "win32":
        system = "windows"
    elif sys.platform == "darwin":
        system = "macos"
    else:
        system = "linux"
    wsl = system == "linux" and is_wsl(read_text("/proc/version"))
    manager = package_manager(read_text("/etc/os-release")) if system == "linux" else None
    return Host(system, wsl, manager, sys.stdin.isatty())


def install_command(host, packages):
    if not host.package_manager or not packages:
        return None
    return PACKAGE_MANAGERS[host.package_manager][0] + list(packages)


# --- Rust -------------------------------------------------------------------


def find_cargo(path, home, windows):
    """cargo on `path`, else in `home`/.cargo/bin, else None."""
    found = shutil.which("cargo", path=path)
    if found:
        return found
    candidate = Path(home) / ".cargo" / "bin" / exe_name("cargo", windows)
    return str(candidate) if candidate.is_file() else None


def host_triple(rustc_vv):
    """The `host:` line of `rustc -vV`, or None."""
    for line in rustc_vv.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    return None


def workspace_rust_version(cargo_toml):
    """The `rust-version` of the workspace's Cargo.toml, as a tuple, or None."""
    found = re.search(r'^rust-version\s*=\s*"([\d.]+)"', cargo_toml, re.MULTILINE)
    return version_tuple(found.group(1)) if found else None


def rust_problems(host, cargo, rustc_vv, minimum):
    """Rust itself: installed, a toolchain, new enough."""
    windows = host.system == "windows"
    if not cargo:
        if windows:
            fix = ["winget", "install", "--id", "Rustlang.Rustup", *WINGET_FLAGS] if shutil.which("winget") else None
        else:
            fix = ["sh", "-c", RUSTUP_SH] if shutil.which("curl") else None
        return [Problem("Rust (cargo) is not installed", "Install it from https://rustup.rs, then open a new terminal.", fix)]
    rustup = Path(cargo).with_name(exe_name("rustup", windows))
    if not rustc_vv:
        fix = [str(rustup), "default", "stable"] if rustup.is_file() else None
        return [Problem("cargo is there but no Rust toolchain is installed", "Run `rustup default stable`.", fix)]
    have = version_tuple(rustc_vv.split("\n", 1)[0])
    if minimum and have < minimum:
        want = ".".join(map(str, minimum))
        fix = [str(rustup), "update", "stable"] if rustup.is_file() else None
        return [Problem(f"rustc {'.'.join(map(str, have))} is older than the {want} this workspace needs", "Run `rustup update stable`.", fix)]
    return []


# --- the linker ---------------------------------------------------------------


def winlibs_bins(local_app_data):
    """WinLibs' mingw64\\bin folders of winget installs, newest first."""
    packages = Path(local_app_data) / "Microsoft" / "WinGet" / "Packages"
    if not packages.is_dir():
        return []
    found = [p / "mingw64" / "bin" for p in packages.glob("BrechtSanders.WinLibs.*")]
    found = [p for p in found if (p / "as.exe").is_file()]
    return sorted(found, key=lambda p: p.stat().st_mtime, reverse=True)


def mingw_path(path, local_app_data):
    """The folder to put first on the PATH for a windows-gnu build: None when
    an assembler is on the PATH already, "" when none is installed."""
    if shutil.which("as", path=path):
        return None
    found = winlibs_bins(local_app_data) if local_app_data else []
    return str(found[0]) if found else ""


def has_msvc_tools(program_files_x86):
    """Whether Visual Studio (or its Build Tools) has the C++ x64 tools."""
    vswhere = Path(program_files_x86 or "") / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    if not vswhere.is_file():
        return False
    code, out = run_quiet(
        [str(vswhere), "-latest", "-products", "*", "-requires",
         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"]
    )
    return code == 0 and bool(out.strip())


def choose_windows_toolchain(host_tc, msvc_tools, toolchains, rustup="rustup"):
    """The toolchain to build with ("" for rustup's default) and the problems
    in the way. MSVC works when its build tools are there; otherwise the
    windows-gnu toolchain does, with WinLibs (checked by the caller)."""
    if host_tc and host_tc.endswith("windows-gnu"):
        return "", []
    if msvc_tools:
        return "", []
    if any(t.startswith(GNU_TOOLCHAIN) for t in toolchains):
        return GNU_TOOLCHAIN, []
    return GNU_TOOLCHAIN, [
        Problem(
            "The MSVC toolchain has no Visual Studio C++ build tools to link with",
            "Install the windows-gnu toolchain (`rustup toolchain install stable-x86_64-pc-windows-gnu`) "
            "or Visual Studio Build Tools with the C++ workload.",
            [rustup, "toolchain", "install", GNU_TOOLCHAIN, "--profile", "minimal"],
        )
    ]


def linker_problems(host, env):
    if host.system == "linux":
        if shutil.which("cc", path=env.get("PATH")):
            return []
        packages = BUILD_PACKAGES.get(host.package_manager or "", [])
        return [Problem("No C compiler (cc) for Rust to link with", "Install gcc (Debian/Ubuntu: build-essential).", install_command(host, packages))]
    if host.system == "macos":
        code, _ = run_quiet(["xcode-select", "-p"])
        if code == 0:
            return []
        return [Problem("The Xcode command line tools are not installed", "Run `xcode-select --install`, then run this again.", ["xcode-select", "--install"])]
    return []


def gnu_problems(env):
    """The windows-gnu toolchain's mingw-w64; puts it on env's PATH when found."""
    mingw = mingw_path(env.get("PATH", ""), env.get("LOCALAPPDATA"))
    if mingw:
        env["PATH"] = mingw + os.pathsep + env.get("PATH", "")
    if mingw == "":
        fix = ["winget", "install", "--id", WINLIBS_ID, *WINGET_FLAGS] if shutil.which("winget") else None
        return [Problem("The windows-gnu toolchain needs a full mingw-w64 to link", f"Install WinLibs: `winget install {WINLIBS_ID}`.", fix)]
    return []


# --- running the viewer (Linux) ------------------------------------------------


def ldconfig_sonames(ldconfig_p):
    """The library names `ldconfig -p` lists."""
    return {line.split()[0] for line in ldconfig_p.splitlines()[1:] if line.strip()}


def runtime_problems(host, sonames):
    """The libraries the viewer loads when it starts. Not needed to build, so
    not blocking: the router runs without them."""
    if host.system != "linux" or not sonames:
        return []
    found = []
    missing = [lib for lib in RUNTIME_LIBS if lib not in sonames]
    if missing:
        packages = [RUNTIME_LIBS[lib].get(host.package_manager or "") for lib in missing]
        found.append(
            Problem(
                f"The viewer needs {', '.join(missing)}",
                "Install libxkbcommon and libxkbcommon-x11 from your distribution.",
                install_command(host, [p for p in packages if p]),
                blocking=False,
            )
        )
    if not any(lib in sonames for lib in GRAPHICS_LIBS):
        found.append(
            Problem(
                "No Vulkan or EGL library for the viewer to draw with",
                "Install Mesa's Vulkan drivers or EGL (Debian/Ubuntu: mesa-vulkan-drivers libegl1).",
                install_command(host, GRAPHICS_PACKAGES.get(host.package_manager or "", [])),
                blocking=False,
            )
        )
    return found


# --- KiCad ---------------------------------------------------------------------


def windows_documents():
    """The Documents folder KiCad uses on Windows: the shell's (OneDrive can
    move it), else the usual ones."""
    found = []
    try:
        import winreg

        key = r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders"
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key) as k:
            found.append(Path(os.path.expandvars(winreg.QueryValueEx(k, "Personal")[0])))
    except (ImportError, OSError):
        pass
    return found


def kicad_roots(host, home):
    """KiCad's user folders that may exist on this system, preferred first."""
    home = Path(home)
    if host.system == "windows":
        docs = windows_documents() + [home / "Documents", home / "OneDrive" / "Documents"]
        return [d / "KiCad" for d in dict.fromkeys(docs)]
    if host.system == "macos":
        return [home / "Documents" / "KiCad"]
    return [home / ".local" / "share" / "kicad", home / ".var" / "app" / "org.kicad.KiCad" / "data" / "kicad"]


def newest_version_dir(root):
    """The newest `<root>/<major.minor>` folder, or None."""
    root = Path(root)
    if not root.is_dir():
        return None
    versions = [p for p in root.iterdir() if p.is_dir() and re.fullmatch(r"\d+(\.\d+)*", p.name)]
    return max(versions, key=lambda p: version_tuple(p.name), default=None)


def windows_profile_from_wsl():
    """The Windows user's profile folder as a WSL path, or None."""
    code, out = run_quiet(["cmd.exe", "/c", "echo %USERPROFILE%"], cwd="/mnt/c")
    if code != 0 or not out.strip() or "%" in out:
        return None
    code, path = run_quiet(["wslpath", "-u", out.strip()])
    return Path(path.strip()) if code == 0 and path.strip() else None


def windows_kicad_from_wsl():
    """The newest version folder of the Windows KiCad, seen from WSL, or None."""
    profile = windows_profile_from_wsl()
    if not profile:
        return None
    for docs in (profile / "Documents", profile / "OneDrive" / "Documents"):
        version = newest_version_dir(docs / "KiCad")
        if version:
            return version
    return None


def kicad_target(host, home, args, rest):
    """Where install.py puts the plugin: (plugins folder or None for its own
    choice, problems). With --plugins-dir or --kicad-version it decides."""
    if any(a.split("=", 1)[0] in ("--plugins-dir", "--kicad-version") for a in rest):
        return None, []
    if args.windows_kicad:
        version = windows_kicad_from_wsl() if host.wsl else None
        if not version:
            return None, [Problem("No Windows KiCad user folder found from WSL", "Start the Windows KiCad once, or pass --plugins-dir.")]
        return version / "scripting" / "plugins", []
    for root in kicad_roots(host, home):
        version = newest_version_dir(root)
        if version:
            return version / "scripting" / "plugins", []
    hint = "Start KiCad once (it creates the folder), or pass --plugins-dir."
    if host.wsl and windows_kicad_from_wsl():
        hint = (
            "This is WSL and only the Windows KiCad is set up: run again with --windows-kicad "
            "(the plugin then runs this Linux build through WSL), or run this script from Windows "
            "for native executables."
        )
    return None, [Problem("No KiCad user folder found", hint)]


# --- Windows ---------------------------------------------------------------------


def running(tasklist_csv):
    """Our executables among the image names of `tasklist /FO CSV /NH`."""
    ours = {exe_name(n, True).lower() for n in EXES}
    names = {line.split(",", 1)[0].strip('"').lower() for line in tasklist_csv.splitlines() if line}
    return sorted(ours & names)


def windows_problems():
    found = []
    _, out = run_quiet(["tasklist", "/FO", "CSV", "/NH"])
    busy = running(out)
    if busy:
        found.append(Problem(f"{' and '.join(busy)} is running", "Close it: Windows locks a running exe, so the build cannot replace it."))
    if str(ROOT).startswith("\\\\"):
        found.append(
            Problem(
                "The checkout is on a network or WSL path",
                "Builds from \\\\wsl$ paths are slow and can fail; clone the repository onto a Windows drive.",
                blocking=False,
            )
        )
    return found


# --- the checks together ----------------------------------------------------------


class Plan(NamedTuple):
    """What the build and install will use, what is fine and what is in the way."""

    env: dict
    cargo: Optional[str]
    toolchain: str  # "" for rustup's default
    plugins: Optional[Path]
    oks: List[str]
    problems: List[Problem]


def rust_tools(cargo, env, windows):
    """(`rustc -vV` of the rustc beside `cargo`, `rustup toolchain list`)."""
    rustc = Path(cargo).with_name(exe_name("rustc", windows))
    rustc = str(rustc) if rustc.is_file() else shutil.which("rustc", path=env.get("PATH"))
    rustup = Path(cargo).with_name(exe_name("rustup", windows))
    vv = run_quiet([rustc, "-vV"], env=env) if rustc else (None, "")
    toolchains = run_quiet([str(rustup), "toolchain", "list"], env=env) if rustup.is_file() else (None, "")
    return (vv[1] if vv[0] == 0 else ""), toolchains[1], str(rustup)


def build_checks(host, env, cargo, oks):
    """Rust, the linker, Windows' locks and the disk; returns (toolchain, problems)."""
    windows = host.system == "windows"
    problems = linker_problems(host, env)
    rustc_vv, toolchains, rustup = rust_tools(cargo, env, windows) if cargo else ("", "", "")
    problems += rust_problems(host, cargo, rustc_vv, workspace_rust_version(read_text(ROOT / "Cargo.toml")))
    toolchain = ""
    if rustc_vv:
        oks.append(f"Rust: {rustc_vv.splitlines()[0]} ({host_triple(rustc_vv)})")
    if windows and rustc_vv:
        triple = host_triple(rustc_vv) or ""
        msvc = has_msvc_tools(env.get("ProgramFiles(x86)"))
        toolchain, found = choose_windows_toolchain(triple, msvc, toolchains.split(), rustup)
        problems += found
        if toolchain or triple.endswith("windows-gnu"):
            mingw_before = env.get("PATH")
            problems += gnu_problems(env)
            if env.get("PATH") != mingw_before:
                oks.append(f"mingw-w64: {env['PATH'].split(os.pathsep, 1)[0]}")
        elif msvc:
            oks.append("linker: Visual Studio C++ build tools")
    elif host.system != "windows" and not problems:
        oks.append(f"linker: {shutil.which('cc', path=env.get('PATH')) or 'Xcode command line tools'}")
    if windows:
        problems += windows_problems()
    free = shutil.disk_usage(ROOT).free / 1e9
    if free < MIN_FREE_GB:
        problems.append(Problem(f"Only {free:.1f} GB free", f"The release build needs about {MIN_FREE_GB} GB.", blocking=False))
    return toolchain, problems


def check(host, args, rest):
    env = dict(os.environ)
    oks = [f"Python {sys.version.split()[0]}"]
    problems = []
    if sys.version_info < MIN_PYTHON:
        problems.append(Problem(f"Python {sys.version.split()[0]} is too old", "Run this with Python 3.8 or newer."))
    cargo = find_cargo(env.get("PATH", ""), Path.home(), host.system == "windows")
    if cargo:
        env["PATH"] = str(Path(cargo).parent) + os.pathsep + env.get("PATH", "")
    toolchain = ""
    if not args.skip_build:
        toolchain, found = build_checks(host, env, cargo, oks)
        problems += found
    if host.system == "linux":
        ldconfig = run_quiet(["ldconfig", "-p"])[1] or run_quiet(["/sbin/ldconfig", "-p"])[1]
        found = runtime_problems(host, ldconfig_sonames(ldconfig))
        problems += found
        if ldconfig and not found:
            oks.append("viewer libraries: xkbcommon, Vulkan or EGL")
    plugins, found = kicad_target(host, Path.home(), args, rest)
    if plugins:
        oks.append(f"KiCad plugins folder: {plugins}")
    return Plan(env, cargo, toolchain, plugins, oks, problems + found)


def report(plan):
    for ok in plan.oks:
        print(f"[ok] {ok}")
    for p in plan.problems:
        print(f"[{'!!' if p.blocking else '--'}] {p.what}\n     {p.hint}")
        if p.fix:
            print(f"     fix offered: {' '.join(p.fix)}")


def run_fix(cmd):
    """Runs a fix in the foreground (sudo and installers may ask questions)."""
    try:
        return subprocess.run(cmd).returncode
    except OSError as e:
        print(f"Could not run {cmd[0]}: {e}")
        return 1


def confirm(question, host, assume_yes):
    if assume_yes:
        return True
    if not host.interactive:
        return False
    return input(f"{question} [y/N] ").strip().lower() in ("y", "yes")


def set_up(host, args, rest):
    """Checks the machine and offers the fixes, a few rounds (installing Rust
    can reveal a missing toolchain). Returns the final plan; exits when
    something blocking is left."""
    tried = set()
    plan = check(host, args, rest)
    report(plan)
    if args.check:
        sys.exit(1 if any(p.blocking for p in plan.problems) else 0)
    for _ in range(FIX_ROUNDS):
        fixable = [p for p in plan.problems if p.fix and tuple(p.fix) not in tried]
        if not fixable or args.no_setup:
            break
        ran = False
        for p in fixable:
            tried.add(tuple(p.fix))
            if confirm(f"Fix '{p.what}' by running `{' '.join(p.fix)}`?", host, args.yes):
                print("$ " + " ".join(p.fix), flush=True)
                ran = True
                if run_fix(p.fix) != 0:
                    print(f"That did not work. {p.hint}")
        if not ran:
            break
        plan = check(host, args, rest)
        print("Checked again:")
        report(plan)
    blocking = [p for p in plan.problems if p.blocking]
    if blocking:
        more = "" if host.interactive or args.yes or args.no_setup else " Run with --yes to apply the fixes offered."
        sys.exit(f"Stopped: {len(blocking)} problem(s) marked [!!] must be fixed first.{more}")
    return plan


# --- build and install ------------------------------------------------------------


def target_dir(cargo, env):
    """Cargo's target directory (honours CARGO_TARGET_DIR and config)."""
    code, out = run_quiet([cargo, "metadata", "--format-version", "1", "--no-deps"], env=env, cwd=ROOT)
    if code != 0:
        sys.exit("`cargo metadata` failed: is this a freerouting-rs checkout?")
    return Path(json.loads(out)["target_directory"])


def build(plan):
    cmd = [plan.cargo] + ([f"+{plan.toolchain}"] if plan.toolchain else []) + ["build", "--release"]
    cmd += [a for p in PACKAGES for a in ("-p", p)]
    print("$ " + " ".join(["cargo"] + cmd[1:]), flush=True)
    if subprocess.run(cmd, cwd=ROOT, env=plan.env).returncode != 0:
        sys.exit("The build failed; nothing was installed.")


def keep_windows_bin_dir(plugins, previous):
    """--windows-kicad writes the WSL build's folder; a Windows build's
    `bin_dir` that was set before stays (the plugin prefers it)."""
    if not previous.get("bin_dir"):
        return
    settings_path = plugins / "freerouting_rs" / "settings.json"
    settings = json.loads(read_text(settings_path) or "{}")
    settings["bin_dir"] = previous["bin_dir"]
    settings_path.write_text(json.dumps(settings, indent=2), encoding="utf-8")
    print(f"Kept the Windows executables of the earlier install: {previous['bin_dir']}")


def install(plan, host, args, rest, bin_dir):
    windows = host.system == "windows"
    missing = [n for n in EXES if not (bin_dir / exe_name(n, windows)).is_file()]
    if missing:
        sys.exit(f"Not built yet in {bin_dir}: {', '.join(missing)}. Run without --skip-build.")
    cmd = [sys.executable, str(INSTALL)]
    previous = {}
    if args.windows_kicad:
        previous = json.loads(read_text(plan.plugins / "freerouting_rs" / "settings.json") or "{}")
        cmd += ["--plugins-dir", str(plan.plugins), "--wsl-bin-dir", str(bin_dir)]
        cmd += ["--wsl-distro", os.environ.get("WSL_DISTRO_NAME", "")]
    else:
        cmd += ["--bin-dir", str(bin_dir)]
        if plan.plugins:
            cmd += ["--plugins-dir", str(plan.plugins)]
    if subprocess.run(cmd + rest).returncode != 0:
        sys.exit("install.py failed.")
    if args.windows_kicad:
        keep_windows_bin_dir(plan.plugins, previous)
    print("In KiCad's PCB editor: Tools > External Plugins > Refresh Plugins (no restart needed).")


def parse(argv):
    ap = argparse.ArgumentParser(
        description=__doc__.splitlines()[0],
        epilog="Other options go to kicad-plugin/install.py (see its --help).",
    )
    ap.add_argument("--check", action="store_true", help="only report what is there and what is missing")
    ap.add_argument("--yes", "-y", action="store_true", help="apply every fix offered without asking")
    ap.add_argument("--no-setup", action="store_true", help="never install anything; stop on a blocking problem")
    ap.add_argument("--skip-build", action="store_true", help="install the executables already built")
    ap.add_argument(
        "--windows-kicad",
        action="store_true",
        help="from WSL: install into the Windows KiCad, which runs this Linux build through WSL",
    )
    args, rest = ap.parse_known_args(argv)
    flags = {a.split("=", 1)[0] for a in rest}
    if "--bin-dir" in flags:
        ap.error("the executables' folder is the build's own; use kicad-plugin/install.py for another")
    if args.windows_kicad and "--wsl-bin-dir" in flags:
        ap.error("--windows-kicad sets --wsl-bin-dir to this build")
    return ap, args, rest


def main(argv=None, host=None):
    ap, args, rest = parse(argv)
    host = host or detect_host()
    if args.windows_kicad and not host.wsl:
        ap.error("--windows-kicad is for running in WSL")
    plan = set_up(host, args, rest)
    if not plan.cargo:
        sys.exit("cargo is still not found: open a new terminal (so the PATH is updated) and run this again.")
    bin_dir = target_dir(plan.cargo, plan.env) / "release"
    if not args.skip_build:
        build(plan)
    install(plan, host, args, rest, bin_dir)


if __name__ == "__main__":
    main()

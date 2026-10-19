"""tools/install_plugin.py: the environment checks, the fixes it offers, and the
build and install it runs. Nothing here runs cargo, a package manager or KiCad."""

import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "tools" / "install_plugin.py"
spec = importlib.util.spec_from_file_location("install_plugin", SCRIPT)
ip = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ip)

LINUX = ip.Host("linux", False, "apt", False)
WSL = ip.Host("linux", True, "apt", False)


def touch(path, mtime=None):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("")
    if mtime is not None:
        os.utime(path, (mtime, mtime))
    return path


# --- what the machine is ----------------------------------------------------


@pytest.mark.parametrize(
    "os_release, manager",
    [
        ('NAME="Ubuntu"\nID=ubuntu\nID_LIKE=debian\nVERSION_ID="24.04"\n', "apt"),
        ("ID=debian\n", "apt"),
        ('ID=linuxmint\nID_LIKE="ubuntu debian"\n', "apt"),
        ("ID=fedora\n", "dnf"),
        ('ID="rocky"\nID_LIKE="rhel centos fedora"\n', "dnf"),
        ("ID=manjaro\nID_LIKE=arch\n", "pacman"),
        ('ID="opensuse-tumbleweed"\nID_LIKE="opensuse suse"\n', "zypper"),
        ("ID=nixos\n", None),
        ("", None),
    ],
)
def test_the_package_manager_follows_os_release(os_release, manager):
    assert ip.package_manager(os_release) == manager


def test_wsl_is_told_by_the_kernel_version():
    assert ip.is_wsl("Linux version 6.6.87.2-microsoft-standard-WSL2 (gcc 11.2.0)")
    assert not ip.is_wsl("Linux version 6.8.0-45-generic (buildd@lcy02)")


def test_install_commands_use_the_distributions_tool():
    assert ip.install_command(LINUX, ["gcc"]) == ["sudo", "apt-get", "install", "-y", "gcc"]
    arch = ip.Host("linux", False, "pacman", False)
    assert ip.install_command(arch, ["gcc"])[:4] == ["sudo", "pacman", "-S", "--needed"]
    assert ip.install_command(ip.Host("linux", False, None, False), ["gcc"]) is None
    assert ip.install_command(LINUX, []) is None


# --- Rust -------------------------------------------------------------------


def test_host_triple_reads_the_host_line():
    vv = "rustc 1.90.0 (abc 2026-09-01)\nbinary: rustc\nhost: x86_64-pc-windows-gnu\nrelease: 1.90.0\n"
    assert ip.host_triple(vv) == "x86_64-pc-windows-gnu"
    assert ip.host_triple("no host here") is None


def test_cargo_is_found_in_the_home_folder_when_not_on_the_path(tmp_path):
    cargo = touch(tmp_path / ".cargo" / "bin" / "cargo.exe")
    assert ip.find_cargo(str(tmp_path / "empty"), tmp_path, windows=True) == str(cargo)
    assert ip.find_cargo(str(tmp_path / "empty"), tmp_path, windows=False) is None


def test_the_workspace_rust_version_is_read_from_cargo_toml():
    assert ip.workspace_rust_version('[workspace.package]\nrust-version = "1.85"\n') == (1, 85)
    assert ip.workspace_rust_version("[package]\n") is None
    # The real one parses.
    assert ip.workspace_rust_version(ip.read_text(ip.ROOT / "Cargo.toml")) >= (1, 70)


def test_missing_rust_offers_rustup_when_curl_is_there(monkeypatch):
    monkeypatch.setattr(ip.shutil, "which", lambda name, path=None: "/usr/bin/curl" if name == "curl" else None)
    (problem,) = ip.rust_problems(LINUX, None, "", (1, 85))
    assert problem.blocking and problem.fix[:2] == ["sh", "-c"] and "sh.rustup.rs" in problem.fix[2]
    monkeypatch.setattr(ip.shutil, "which", lambda name, path=None: None)
    (problem,) = ip.rust_problems(LINUX, None, "", (1, 85))
    assert problem.fix is None and "rustup.rs" in problem.hint


def test_an_old_rustc_is_updated_and_a_new_one_passes(tmp_path):
    cargo = touch(tmp_path / "cargo")
    touch(tmp_path / "rustup")
    (problem,) = ip.rust_problems(LINUX, str(cargo), "rustc 1.80.1 (x 2024-08-01)\n", (1, 85))
    assert "older than the 1.85" in problem.what
    assert problem.fix == [str(tmp_path / "rustup"), "update", "stable"]
    assert ip.rust_problems(LINUX, str(cargo), "rustc 1.92.0 (x 2025-12-08)\n", (1, 85)) == []
    (problem,) = ip.rust_problems(LINUX, str(cargo), "", (1, 85))
    assert "no Rust toolchain" in problem.what


# --- the linker ---------------------------------------------------------------


def test_the_newest_winlibs_with_an_assembler_is_picked(tmp_path):
    packages = tmp_path / "Microsoft" / "WinGet" / "Packages"
    old = touch(packages / "BrechtSanders.WinLibs.POSIX.MSVCRT_a" / "mingw64" / "bin" / "as.exe")
    new = touch(packages / "BrechtSanders.WinLibs.POSIX.UCRT_b" / "mingw64" / "bin" / "as.exe")
    os.utime(old.parent, (1_000, 1_000))
    os.utime(new.parent, (2_000, 2_000))
    touch(packages / "BrechtSanders.WinLibs.Broken" / "mingw64" / "bin" / "gcc.exe")
    assert ip.winlibs_bins(tmp_path) == [new.parent, old.parent]
    assert ip.winlibs_bins(tmp_path / "nowhere") == []


def test_mingw_is_added_only_without_an_assembler(tmp_path):
    winlibs = touch(
        tmp_path / "Microsoft" / "WinGet" / "Packages" / "BrechtSanders.WinLibs.X" / "mingw64" / "bin" / "as.exe"
    ).parent
    empty = str(tmp_path / "empty")
    assert ip.mingw_path(empty, tmp_path) == str(winlibs)
    # Not installed: the caller offers winget.
    assert ip.mingw_path(empty, tmp_path / "nowhere") == ""
    on_path = touch(tmp_path / "tools" / "as")
    on_path.chmod(0o755)
    assert ip.mingw_path(str(on_path.parent), tmp_path) is None


@pytest.mark.parametrize(
    "host, msvc, toolchains, chosen, problem",
    [
        ("x86_64-pc-windows-gnu", False, [], "", False),
        ("x86_64-pc-windows-msvc", True, [], "", False),
        # MSVC without Visual Studio: the gnu toolchain, installed or offered.
        ("x86_64-pc-windows-msvc", False, ["stable-x86_64-pc-windows-gnu", "stable-x86_64-pc-windows-msvc"], ip.GNU_TOOLCHAIN, False),
        ("x86_64-pc-windows-msvc", False, ["stable-x86_64-pc-windows-msvc", "(default)"], ip.GNU_TOOLCHAIN, True),
    ],
)
def test_the_windows_toolchain_works_without_visual_studio(host, msvc, toolchains, chosen, problem):
    toolchain, problems = ip.choose_windows_toolchain(host, msvc, toolchains, "C:/r/rustup.exe")
    assert toolchain == chosen
    assert bool(problems) == problem
    if problem:
        assert problems[0].fix == ["C:/r/rustup.exe", "toolchain", "install", ip.GNU_TOOLCHAIN, "--profile", "minimal"]


def test_running_finds_our_exes_in_tasklist_output():
    csv = (
        '"System Idle Process","0","Services","0","8 K"\n'
        '"freerouting-gui.exe","4242","Console","1","120,000 K"\n'
        '"kicad.exe","77","Console","1","900,000 K"\n'
    )
    assert ip.running(csv) == ["freerouting-gui.exe"]
    assert ip.running('"FREEROUTING.EXE","1","Console","1","1 K"\n') == ["freerouting.exe"]
    assert ip.running("") == []


# --- running the viewer ---------------------------------------------------------

LDCONFIG = """1234 libs found in cache `/etc/ld.so.cache'
\tlibxkbcommon.so.0 (libc6,x86-64) => /lib/x86_64-linux-gnu/libxkbcommon.so.0
\tlibvulkan.so.1 (libc6,x86-64) => /lib/x86_64-linux-gnu/libvulkan.so.1
\tlibc.so.6 (libc6,x86-64, OS ABI: Linux 3.2.0) => /lib/x86_64-linux-gnu/libc.so.6
"""


def test_ldconfig_lists_sonames():
    assert ip.ldconfig_sonames(LDCONFIG) == {"libxkbcommon.so.0", "libvulkan.so.1", "libc.so.6"}


def test_missing_viewer_libraries_are_offered_from_the_distribution():
    (xkb,) = ip.runtime_problems(LINUX, ip.ldconfig_sonames(LDCONFIG))
    assert not xkb.blocking
    assert xkb.fix == ["sudo", "apt-get", "install", "-y", "libxkbcommon-x11-0"]
    no_graphics = {"libxkbcommon.so.0", "libxkbcommon-x11.so.0"}
    (graphics,) = ip.runtime_problems(ip.Host("linux", False, "dnf", False), no_graphics)
    assert graphics.fix == ["sudo", "dnf", "install", "-y", "vulkan-loader", "mesa-vulkan-drivers", "mesa-libEGL"]
    assert ip.runtime_problems(LINUX, no_graphics | {"libEGL.so.1"}) == []
    # No ldconfig to ask, or not Linux: nothing is claimed missing.
    assert ip.runtime_problems(LINUX, set()) == []
    assert ip.runtime_problems(ip.Host("macos", False, None, False), {"x"}) == []


# --- KiCad ---------------------------------------------------------------------


def test_the_newest_kicad_version_folder_wins(tmp_path):
    for name in ("8.0", "9.0", "10.0", "template", "10.0.bak"):
        (tmp_path / name).mkdir()
    assert ip.newest_version_dir(tmp_path) == tmp_path / "10.0"
    assert ip.newest_version_dir(tmp_path / "missing") is None


def args(**kw):
    defaults = dict(check=False, yes=False, no_setup=False, skip_build=False, windows_kicad=False, plugins_dir=None, kicad_version=None)
    return ip.argparse.Namespace(**{**defaults, **kw})


def test_the_flatpak_kicad_is_found_when_it_is_the_only_one(tmp_path):
    flatpak = tmp_path / ".var" / "app" / "org.kicad.KiCad" / "data" / "kicad" / "9.0"
    flatpak.mkdir(parents=True)
    plugins, problems = ip.kicad_target(LINUX, tmp_path, {}, args())
    assert plugins == flatpak / "scripting" / "plugins" and problems == []
    native = tmp_path / ".local" / "share" / "kicad" / "10.0"
    native.mkdir(parents=True)
    assert ip.kicad_target(LINUX, tmp_path, {}, args())[0] == native / "scripting" / "plugins"


def test_kicads_own_folder_variables_are_followed(tmp_path):
    home = tmp_path / "home"
    xdg = tmp_path / "xdg"
    assert ip.kicad_roots(LINUX, home, {"XDG_DATA_HOME": str(xdg)})[0] == xdg / "kicad"
    # KICAD_DOCUMENTS_HOME replaces the system's folder, as in KiCad's PATHS::getUserDocumentPath.
    docs = tmp_path / "docs"
    assert ip.kicad_roots(LINUX, home, {"KICAD_DOCUMENTS_HOME": str(docs), "XDG_DATA_HOME": str(xdg)}) == [docs / "kicad"]
    assert ip.kicad_roots(ip.Host("macos", False, None, False), home, {"KICAD_DOCUMENTS_HOME": str(docs)}) == [docs / "KiCad"]
    assert ip.kicad_roots(ip.Host("macos", False, None, False), home, {}) == [home / "Documents" / "KiCad"]


def fake_windows(monkeypatch, powershell_out, code=0, wsl_paths=None):
    """Windows as WSL asks it: powershell.exe prints `powershell_out`, and
    wslpath converts by `wsl_paths` (a path missing there is unreachable)."""
    monkeypatch.setattr(ip, "windows_powershell", lambda: "powershell.exe")

    def run(cmd, **kw):
        if cmd[0] == "powershell.exe":
            assert ip.WINDOWS_FOLDERS_PS in cmd
            return code, powershell_out
        assert cmd[:2] == ["wslpath", "-u"]
        found = (wsl_paths or {}).get(cmd[2])
        return (0, found + "\n") if found else (1, "")

    monkeypatch.setattr(ip, "run_quiet", run)


def test_windows_documents_moved_to_another_drive_is_found_from_wsl(tmp_path, monkeypatch):
    version = tmp_path / "d" / "Documents" / "KiCad" / "10.0"
    version.mkdir(parents=True)
    # OneDrive or a redirection moved Documents off C:\Users\<name>; Windows says where.
    fake_windows(monkeypatch, "kicad=\r\ndocuments=D:\\Documents\r\n", wsl_paths={"D:\\Documents": str(tmp_path / "d" / "Documents")})
    assert ip.windows_kicad_root() == (tmp_path / "d" / "Documents" / "KiCad", "D:\\Documents\\KiCad")
    plugins, problems = ip.kicad_target(WSL, tmp_path, {}, args(windows_kicad=True))
    assert plugins == version / "scripting" / "plugins" and problems == []


def test_windows_kicad_documents_home_wins_over_the_documents_folder(monkeypatch):
    fake_windows(monkeypatch, "kicad=E:\\kicad-docs\r\ndocuments=D:\\Documents\r\n", wsl_paths={"E:\\kicad-docs": "/mnt/e/kicad-docs"})
    assert ip.windows_kicad_root() == (Path("/mnt/e/kicad-docs/KiCad"), "E:\\kicad-docs\\KiCad")


def test_windows_kicad_problems_say_what_was_tried(tmp_path, monkeypatch):
    fake_windows(monkeypatch, "", code=1)
    _, (problem,) = ip.kicad_target(WSL, tmp_path, {}, args(windows_kicad=True))
    assert problem.blocking and "interop" in problem.hint and "--plugins-dir" in problem.hint
    fake_windows(monkeypatch, "kicad=\r\ndocuments=\\\\server\\home\\Documents\r\n")
    _, (problem,) = ip.kicad_target(WSL, tmp_path, {}, args(windows_kicad=True))
    assert "\\\\server\\home\\Documents\\KiCad, which WSL cannot reach" in problem.hint
    fake_windows(monkeypatch, "kicad=\r\ndocuments=D:\\Documents\r\n", wsl_paths={"D:\\Documents": str(tmp_path)})
    _, (problem,) = ip.kicad_target(WSL, tmp_path, {}, args(windows_kicad=True))
    assert problem.hint.startswith("Looked in D:\\Documents\\KiCad. Start the Windows KiCad once")


def test_without_kicad_the_hint_points_at_wsls_windows_kicad(tmp_path, monkeypatch):
    (tmp_path / "win" / "KiCad" / "10.0").mkdir(parents=True)
    monkeypatch.setattr(ip, "windows_kicad_root", lambda: (tmp_path / "win" / "KiCad", "C:\\Users\\me\\Documents\\KiCad"))
    plugins, (problem,) = ip.kicad_target(WSL, tmp_path, {}, args())
    assert plugins is None and problem.blocking and "--windows-kicad" in problem.hint
    plugins, problems = ip.kicad_target(WSL, tmp_path, {}, args(windows_kicad=True))
    assert plugins == tmp_path / "win" / "KiCad" / "10.0" / "scripting" / "plugins" and problems == []
    # Plain Linux names the folders it looked in.
    _, (problem,) = ip.kicad_target(LINUX, tmp_path, {}, args())
    assert problem.hint.startswith(f"Looked in {tmp_path / '.local' / 'share' / 'kicad'}, ")


def test_kicad_version_picks_the_folder_and_plugins_dir_decides(tmp_path, monkeypatch):
    for v in ("9.0", "10.0"):
        (tmp_path / ".local" / "share" / "kicad" / v).mkdir(parents=True)
    plugins, _ = ip.kicad_target(LINUX, tmp_path, {}, args(kicad_version="9.0"))
    assert plugins == tmp_path / ".local" / "share" / "kicad" / "9.0" / "scripting" / "plugins"
    _, (problem,) = ip.kicad_target(LINUX, tmp_path, {}, args(kicad_version="8.0"))
    assert problem.what == "No KiCad 8.0 user folder found"
    assert ip.kicad_target(LINUX, tmp_path, {}, args(plugins_dir="/x", kicad_version="8.0")) == (Path("/x"), [])
    # From WSL, a folder copied from Explorer is converted; one WSL cannot reach is refused.
    fake_windows(monkeypatch, "", wsl_paths={"D:\\KiCad\\plugins": "/mnt/d/KiCad/plugins"})
    assert ip.kicad_target(WSL, tmp_path, {}, args(windows_kicad=True, plugins_dir="D:\\KiCad\\plugins")) == (Path("/mnt/d/KiCad/plugins"), [])
    assert ip.kicad_target(WSL, tmp_path, {}, args(windows_kicad=True, plugins_dir="/mnt/c/k")) == (Path("/mnt/c/k"), [])
    _, (problem,) = ip.kicad_target(WSL, tmp_path, {}, args(windows_kicad=True, plugins_dir="\\\\nas\\k"))
    assert problem.what == "WSL cannot reach \\\\nas\\k"


# --- set_up: asking, fixing, stopping ---------------------------------------------


def plan(problems=(), plugins=None, cargo="/c/cargo", toolchain=""):
    return ip.Plan({"PATH": ""}, cargo, toolchain, plugins, ["Python 3"], list(problems))


MISSING_CC = ip.Problem("No C compiler", "Install gcc.", ["sudo", "apt-get", "install", "-y", "gcc"])


def test_yes_runs_each_fix_once_and_checks_again(monkeypatch):
    checks = iter([plan([MISSING_CC]), plan()])
    monkeypatch.setattr(ip, "check", lambda *a: next(checks))
    ran = []
    monkeypatch.setattr(ip, "run_fix", lambda cmd: ran.append(cmd) or 0)
    assert ip.set_up(LINUX, args(yes=True)).problems == []
    assert ran == [MISSING_CC.fix]


def test_a_fix_that_does_not_help_is_not_retried(monkeypatch):
    monkeypatch.setattr(ip, "check", lambda *a: plan([MISSING_CC]))
    ran = []
    monkeypatch.setattr(ip, "run_fix", lambda cmd: ran.append(cmd) or 1)
    with pytest.raises(SystemExit, match="1 problem"):
        ip.set_up(LINUX, args(yes=True))
    assert ran == [MISSING_CC.fix]


def test_without_a_terminal_or_yes_nothing_is_installed(monkeypatch):
    monkeypatch.setattr(ip, "check", lambda *a: plan([MISSING_CC]))
    monkeypatch.setattr(ip, "run_fix", lambda cmd: pytest.fail("ran a fix"))
    with pytest.raises(SystemExit, match="--yes"):
        ip.set_up(LINUX, args())
    with pytest.raises(SystemExit):
        ip.set_up(LINUX, args(no_setup=True, yes=True))


def test_an_interactive_no_skips_the_fix(monkeypatch):
    monkeypatch.setattr(ip, "check", lambda *a: plan([MISSING_CC]))
    monkeypatch.setattr(ip, "run_fix", lambda cmd: pytest.fail("ran a fix"))
    monkeypatch.setattr("builtins.input", lambda prompt: "n")
    with pytest.raises(SystemExit):
        ip.set_up(ip.Host("linux", False, "apt", True), args())


def test_check_only_reports_and_exits_by_whether_anything_blocks(monkeypatch, capsys):
    warning = ip.Problem("Only 2.0 GB free", "Free some space.", blocking=False)
    monkeypatch.setattr(ip, "check", lambda *a: plan([warning]))
    with pytest.raises(SystemExit) as done:
        ip.set_up(LINUX, args(check=True))
    assert done.value.code == 0
    out = capsys.readouterr().out
    assert "[ok] Python 3" in out and "[--] Only 2.0 GB free" in out
    monkeypatch.setattr(ip, "check", lambda *a: plan([MISSING_CC]))
    with pytest.raises(SystemExit) as done:
        ip.set_up(LINUX, args(check=True))
    assert done.value.code == 1
    assert "fix offered: sudo apt-get install -y gcc" in capsys.readouterr().out


# --- build and install -----------------------------------------------------------


@pytest.fixture
def fake_tools(tmp_path, monkeypatch):
    """A clean check, a target dir with both executables, and the commands
    main runs recorded instead of run."""
    target = tmp_path / "target"
    windows = sys.platform == "win32"
    for name in ip.EXES:
        touch(target / "release" / ip.exe_name(name, windows))
    plugins = tmp_path / "KiCad" / "10.0" / "scripting" / "plugins"
    monkeypatch.setattr(ip, "check", lambda *a: plan(plugins=plugins))
    monkeypatch.setattr(ip, "run_quiet", lambda cmd, **kw: (0, json.dumps({"target_directory": str(target)})))
    ran = []

    def fake_run(cmd, **kwargs):
        ran.append(cmd)
        return subprocess.CompletedProcess(cmd, 0)

    monkeypatch.setattr(ip.subprocess, "run", fake_run)
    return target, plugins, ran


def test_main_builds_then_installs_into_the_folder_it_found(fake_tools):
    target, plugins, ran = fake_tools
    ip.main(["--keep-pcm"], host=LINUX)
    build, install = ran
    assert build == ["/c/cargo", "build", "--release", "-p", "freerouting-cli", "-p", "freerouting-gui"]
    assert install[:2] == [sys.executable, str(ip.INSTALL)]
    assert install[2:] == ["--bin-dir", str(target / "release"), "--plugins-dir", str(plugins), "--keep-pcm"]


def test_the_chosen_toolchain_goes_on_the_cargo_line(fake_tools, monkeypatch):
    _, plugins, ran = fake_tools
    monkeypatch.setattr(ip, "check", lambda *a: plan(plugins=plugins, toolchain=ip.GNU_TOOLCHAIN))
    ip.main([], host=LINUX)
    assert ran[0][:3] == ["/c/cargo", "+" + ip.GNU_TOOLCHAIN, "build"]


def test_skip_build_installs_what_is_there(fake_tools):
    _, _, ran = fake_tools
    ip.main(["--skip-build"], host=LINUX)
    assert len(ran) == 1 and ran[0][1] == str(ip.INSTALL)


def test_missing_executables_stop_before_install(fake_tools):
    target, _, ran = fake_tools
    for exe in (target / "release").iterdir():
        exe.unlink()
    with pytest.raises(SystemExit, match="Not built yet"):
        ip.main(["--skip-build"], host=LINUX)
    assert ran == []


def test_windows_kicad_installs_the_wsl_build_and_keeps_a_windows_bin_dir(fake_tools, monkeypatch):
    target, plugins, ran = fake_tools
    settings = plugins / "freerouting_rs" / "settings.json"
    settings.parent.mkdir(parents=True)
    settings.write_text(json.dumps({"bin_dir": "C:\\code\\freerouting-rs\\target\\release"}))
    monkeypatch.setenv("WSL_DISTRO_NAME", "Ubuntu")

    def fake_install(cmd, **kwargs):
        ran.append(cmd)
        if cmd[1] == str(ip.INSTALL):  # install.py rewrites settings.json
            settings.write_text(json.dumps({"wsl_bin_dir": str(target / "release"), "wsl_distro": "Ubuntu"}))
        return subprocess.CompletedProcess(cmd, 0)

    monkeypatch.setattr(ip.subprocess, "run", fake_install)
    ip.main(["--windows-kicad", "--skip-build"], host=WSL)
    (install,) = ran
    assert install[2:] == ["--plugins-dir", str(plugins), "--wsl-bin-dir", str(target / "release"), "--wsl-distro", "Ubuntu"]
    assert json.loads(settings.read_text()) == {
        "wsl_bin_dir": str(target / "release"),
        "wsl_distro": "Ubuntu",
        "bin_dir": "C:\\code\\freerouting-rs\\target\\release",
    }


def test_windows_kicad_is_for_wsl_only(fake_tools):
    with pytest.raises(SystemExit):
        ip.main(["--windows-kicad"], host=LINUX)


def test_a_bin_dir_of_ones_own_is_refused_but_a_wsl_fallback_passes():
    for flags in (["--bin-dir", "elsewhere"], ["--bin-dir=elsewhere"], ["--windows-kicad", "--wsl-bin-dir=/x"]):
        with pytest.raises(SystemExit):
            ip.parse(flags)
    # The owner's Windows install keeps a WSL build as the fallback.
    _, _, rest = ip.parse(["--wsl-bin-dir", "/home/me/target/release"])
    assert rest == ["--wsl-bin-dir", "/home/me/target/release"]

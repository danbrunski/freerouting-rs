"""Installs the freerouting-rs KiCad action plugin.

    python install.py [--bin-dir DIR] [--wsl-bin-dir DIR] [--wsl-distro NAME]
                      [--kicad-version 10.0] [--plugins-dir DIR]

Copies freerouting_rs/ into KiCad's user scripting/plugins folder (newest
KiCad version found, unless one is given) and writes settings.json with
where to find the freerouting-rs executables. Re-run it after changing the
plugin source. In the PCB editor, Tools > External Plugins > Refresh Plugins
picks up the change without restarting.
"""

import argparse
import json
import shutil
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent / "freerouting_rs"


def kicad_root():
    home = Path.home()
    if sys.platform == "win32" or sys.platform == "darwin":
        return home / "Documents" / "KiCad"
    return home / ".local" / "share" / "kicad"


def newest_version(root):
    versions = [p.name for p in root.iterdir() if p.is_dir() and p.name[0].isdigit()] if root.is_dir() else []
    if not versions:
        sys.exit(f"No KiCad user folder found under {root}; pass --plugins-dir.")
    return max(versions, key=lambda v: tuple(int(x) for x in v.split(".") if x.isdigit()))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bin-dir", help="folder holding freerouting(.exe) and freerouting-gui(.exe)")
    ap.add_argument("--wsl-bin-dir", help="Windows only: Linux folder inside WSL holding the executables")
    ap.add_argument("--wsl-distro", default="", help="WSL distribution to use (default: WSL's default)")
    ap.add_argument("--kicad-version", help="KiCad user folder version, e.g. 10.0")
    ap.add_argument("--plugins-dir", help="install into this folder instead")
    args = ap.parse_args()

    if args.plugins_dir:
        plugins = Path(args.plugins_dir)
    else:
        root = kicad_root()
        plugins = root / (args.kicad_version or newest_version(root)) / "scripting" / "plugins"
    dest = plugins / "freerouting_rs"

    if dest.exists():
        shutil.rmtree(dest)
    shutil.copytree(SRC, dest, ignore=shutil.ignore_patterns("__pycache__", "settings.json"))

    settings = {}
    if args.bin_dir:
        settings["bin_dir"] = str(Path(args.bin_dir).resolve())
    if args.wsl_bin_dir:
        settings["wsl_bin_dir"] = args.wsl_bin_dir
        settings["wsl_distro"] = args.wsl_distro
    with open(dest / "settings.json", "w", encoding="utf-8") as f:
        json.dump(settings, f, indent=2)

    print(f"Installed to {dest}")
    if settings:
        print(f"Settings: {json.dumps(settings)}")


if __name__ == "__main__":
    main()

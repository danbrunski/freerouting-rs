"""Installs the freerouting-rs KiCad action plugin.

    python install.py [--bin-dir DIR] [--wsl-bin-dir DIR] [--wsl-distro NAME]
                      [--kicad-version 10.0] [--plugins-dir DIR] [--keep-pcm]

Copies freerouting_rs/ into KiCad's user scripting/plugins folder (newest
KiCad version found, unless one is given) and writes settings.json with
where to find the freerouting-rs executables. Re-run it after changing the
plugin source. In the PCB editor, Tools > External Plugins > Refresh Plugins
picks up the change without restarting.

Before copying, it looks for another copy of this plugin under the same KiCad
version (a plugin manager install in `3rdparty/plugins`, or any folder in
`scripting/plugins` whose metadata.json identifier is ours): KiCad would load
both. Such a folder is moved to `<KiCad user dir>/disabled_pcm_plugins/<version>/`
and its new path printed; `--keep-pcm` refuses to install instead. The Java
Freerouting plugin (identifier `app.freerouting.kicad-plugin`) is never touched.
"""

import argparse
import json
import shutil
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent / "freerouting_rs"
FOLDER_NAME = "freerouting_rs"

# The PCM identifier of metadata.json (K03.1; test_package checks they agree).
OUR_IDENTIFIERS = {"com.github.danbrunski.freerouting-rs"}
JAVA_IDENTIFIER = "app.freerouting.kicad-plugin"


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


def folder_identifier(folder):
    """The `identifier` in <folder>/metadata.json, or None."""
    try:
        data = json.loads((folder / "metadata.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    ident = data.get("identifier") if isinstance(data, dict) else None
    return ident if isinstance(ident, str) else None


def is_ours(folder):
    ident = folder_identifier(folder)
    if ident == JAVA_IDENTIFIER or folder.name == JAVA_IDENTIFIER.replace(".", "_"):
        return False  # the owner runs the Java plugin beside this one
    if ident in OUR_IDENTIFIERS:
        return True
    names = {FOLDER_NAME} | {i.replace(".", "_") for i in OUR_IDENTIFIERS} | OUR_IDENTIFIERS
    return folder.name in names


def find_shadow_installs(version_dir, plugins):
    """Folders other than the install destination that KiCad would also load
    as this plugin: under `scripting/plugins` and `3rdparty/plugins` of
    `version_dir`."""
    dest = plugins / FOLDER_NAME
    found = []
    for parent in (version_dir / "scripting" / "plugins", version_dir / "3rdparty" / "plugins"):
        if not parent.is_dir():
            continue
        for folder in sorted(parent.iterdir()):
            if folder.is_dir() and folder != dest and is_ours(folder):
                found.append(folder)
    return found


def disable_shadow(folder, version_dir):
    """Moves `folder` to <KiCad user dir>/disabled_pcm_plugins/<version>/ and
    returns the new path."""
    target_dir = version_dir.parent / "disabled_pcm_plugins" / version_dir.name
    target_dir.mkdir(parents=True, exist_ok=True)
    target = target_dir / folder.name
    n = 1
    while target.exists():
        target = target_dir / f"{folder.name}-{n}"
        n += 1
    shutil.move(str(folder), str(target))
    return target


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bin-dir", help="folder holding freerouting-rs(.exe) and freerouting-gui(.exe)")
    ap.add_argument("--wsl-bin-dir", help="Windows only: Linux folder inside WSL holding the executables")
    ap.add_argument("--wsl-distro", default="", help="WSL distribution to use (default: WSL's default)")
    ap.add_argument("--kicad-version", help="KiCad user folder version, e.g. 10.0")
    ap.add_argument("--plugins-dir", help="install into this folder instead")
    ap.add_argument(
        "--keep-pcm",
        action="store_true",
        help="refuse to install when another copy of this plugin exists, instead of disabling it",
    )
    args = ap.parse_args(argv)

    if args.plugins_dir:
        plugins = Path(args.plugins_dir)
    else:
        root = kicad_root()
        plugins = root / (args.kicad_version or newest_version(root)) / "scripting" / "plugins"
    dest = plugins / FOLDER_NAME

    # <KiCad user dir>/<version>/scripting/plugins; with another layout there is
    # no version folder to search.
    version_dir = plugins.parent.parent if plugins.name == "plugins" and plugins.parent.name == "scripting" else None
    shadows = find_shadow_installs(version_dir, plugins) if version_dir else []
    if shadows and args.keep_pcm:
        sys.exit(
            "Another copy of this plugin would load alongside the new one:\n  "
            + "\n  ".join(str(f) for f in shadows)
            + "\nRemove it in the plugin manager, or run without --keep-pcm to have it moved aside."
        )
    for folder in shadows:
        print(f"Disabled the plugin manager copy {folder} -> {disable_shadow(folder, version_dir)}")

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

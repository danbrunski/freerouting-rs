#!/usr/bin/env python3
"""Builds the KiCad plugin-manager (PCM) package of the plugin (K03.2, K03.4).

    package.py --bin DIR [--bin DIR ...] --out DIR [--allow-missing]

Writes `freerouting-rs-<version>-pcm.zip` and `freerouting-rs-<version>-pcm.zip.meta.json`
into `--out` and prints the zip's path. One zip holds every platform's binaries
(KiCad's validator rejects two version entries with the same version number).

`--bin` says where the binaries are. A directory is searched for one sub-folder per
platform, `<platform>/` or `bin-<platform>/` (the release workflow downloads its artifacts
as `bin-<platform>/`), with platform one of `windows-x86_64`, `macos-universal`,
`linux-x86_64`. `PLATFORM=DIR` names one platform's folder directly. Each folder needs
`freerouting` and `freerouting-gui` (`.exe` on Windows); nothing else in it is packaged.
A missing platform is an error unless `--allow-missing` (local builds with one OS).

Layout of the zip (all the places KiCad's validator allows: `metadata.json`,
`resources/icon.png` and anything under `plugins/`):

    metadata.json                 the repository's, with the version entry reduced to what the
                                  validator accepts inside a package (no download_* or install_size)
    resources/icon.png            64 x 64
    plugins/                      freerouting_rs/ (the plugin package), by denylist (see DENY)
    plugins/bin/<platform>/       freerouting, freerouting-gui
    plugins/LICENSE, THIRD_PARTY_NOTICES.md, LICENSES/...

KiCad copies `plugins/` to `3rdparty/plugins/<identifier with . -> _>/` and imports it as the
plugin package, so `plugins/__init__.py` is the plugin's `__init__.py`.

The zip is deterministic: entries sorted by path, timestamp 1980-01-01, create_system Unix,
mode 0755 for directories and executables and 0644 otherwise, deflate level 9. Two runs on
the same inputs give identical bytes (with the same zlib; another zlib may compress differently,
so build the release zip once and publish its sha256).

The sidecar `<zip>.meta.json` (`version`, `file`, `sha256`, `download_size`, `install_size`)
is what `update_metadata.py` patches into the repository's `metadata.json`. Standard library
only.
"""

import argparse
import hashlib
import json
import os
import stat
import sys
import tempfile
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
PLUGIN_SRC = HERE / "freerouting_rs"
METADATA = HERE / "metadata.json"
ICON = HERE / "resources" / "icon.png"

PLATFORMS = ("windows-x86_64", "macos-universal", "linux-x86_64")
TOOLS = ("freerouting", "freerouting-gui")
# Copied from the repository root into plugins/ (F04.1). KiCad's validator allows nothing
# but metadata.json, resources/icon.png and plugins/** in a package.
NOTICE_FILES = ("LICENSE", "THIRD_PARTY_NOTICES.md")
NOTICE_DIRS = ("LICENSES",)

# Denylist for the plugin package (KRT #49: an allowlist once dropped a new module). A name
# matches at any depth; `bin` only at the top, where the package's own bundled binaries go.
DENY_NAMES = {"__pycache__", "tests", ".pytest_cache", "settings.json", ".DS_Store", ".git"}
DENY_SUFFIXES = (".pyc", ".pyo", ".orig", ".rej", ".swp", "~")
DENY_TOP_LEVEL = {"bin"}

# Fields of the version entry that must not be in the metadata inside the package.
OUTER_ONLY = ("download_url", "download_sha256", "download_size", "install_size")

ZIP_DATE = (1980, 1, 1, 0, 0, 0)
FILE_MODE, EXEC_MODE, DIR_MODE = 0o644, 0o755, 0o755


class PackageError(Exception):
    pass


def denied(rel_parts):
    if rel_parts[0] in DENY_TOP_LEVEL:
        return True
    return any(p in DENY_NAMES or p.endswith(DENY_SUFFIXES) for p in rel_parts)


def walk_files(base):
    """Sorted (relative parts, path) of the files under `base` the denylist lets through."""
    found = []
    for dirpath, dirnames, filenames in os.walk(base):
        rel_dir = Path(dirpath).relative_to(base).parts
        dirnames[:] = sorted(d for d in dirnames if not denied(rel_dir + (d,)))
        for name in sorted(filenames):
            parts = rel_dir + (name,)
            if not denied(parts):
                found.append((parts, Path(dirpath) / name))
    return found


def exe_name(tool, platform):
    return tool + (".exe" if platform.startswith("windows") else "")


def resolve_bins(specs, allow_missing=False):
    """{platform: folder} from `--bin` arguments."""
    found = {}
    for spec in specs:
        name, sep, rest = spec.partition("=")
        if sep and name in PLATFORMS:
            found[name] = Path(rest)
            continue
        base = Path(spec)
        for platform in PLATFORMS:
            for cand in (base / platform, base / f"bin-{platform}"):
                if cand.is_dir():
                    found.setdefault(platform, cand)
                    break
    missing = [p for p in PLATFORMS if p not in found]
    if missing and not allow_missing:
        raise PackageError(
            f"no binaries for {', '.join(missing)} under {', '.join(specs) or '(no --bin)'} "
            "(each platform needs a folder <platform>/ or bin-<platform>/; --allow-missing builds a partial package)"
        )
    if not found:
        raise PackageError("no platform folder found under " + ", ".join(specs))
    for platform, folder in sorted(found.items()):
        for tool in TOOLS:
            if not (folder / exe_name(tool, platform)).is_file():
                raise PackageError(f"{platform}: {folder / exe_name(tool, platform)} is missing")
    return found


def inner_metadata(text):
    """The metadata.json text for inside the package, and the version string."""
    doc = json.loads(text)
    versions = doc.get("versions")
    if not isinstance(versions, list) or len(versions) != 1:
        raise PackageError("metadata.json must have exactly one entry in versions[]")
    entry = {k: v for k, v in versions[0].items() if k not in OUTER_ONLY}
    doc["versions"] = [entry]
    return json.dumps(doc, indent=2, ensure_ascii=False) + "\n", str(entry.get("version", ""))


class Entry:
    """One zip member: a file (`data` bytes) or a directory (`data` None)."""

    def __init__(self, name, data=None, executable=False):
        self.name = name
        self.data = data
        self.executable = executable


def collect(bins, metadata_text, icon=ICON, plugin_src=PLUGIN_SRC, repo=REPO):
    """Every member of the zip, files and directories, sorted by name."""
    entries = {}

    def add_file(name, data, executable=False):
        entries[name] = Entry(name, data, executable)

    add_file("metadata.json", metadata_text.encode("utf-8"))
    add_file("resources/icon.png", Path(icon).read_bytes())
    for parts, path in walk_files(plugin_src):
        add_file("plugins/" + "/".join(parts), path.read_bytes())
    for name in NOTICE_FILES:
        if not (repo / name).is_file():
            raise PackageError(f"{repo / name} is missing (F04.1 notices go in every package)")
        add_file(f"plugins/{name}", (repo / name).read_bytes())
    for name in NOTICE_DIRS:
        if not (repo / name).is_dir():
            raise PackageError(f"{repo / name} is missing")
        for parts, path in walk_files(repo / name):
            add_file(f"plugins/{name}/" + "/".join(parts), path.read_bytes())
    for platform, folder in bins.items():
        for tool in TOOLS:
            exe = exe_name(tool, platform)
            add_file(f"plugins/bin/{platform}/{exe}", (folder / exe).read_bytes(), executable=True)
    # Directory entries: standard in zips, and they make extraction independent of whether
    # the unzipper creates parents of a file.
    for name in list(entries):
        parts = name.split("/")[:-1]
        for i in range(1, len(parts) + 1):
            d = "/".join(parts[:i]) + "/"
            entries.setdefault(d, Entry(d))
    return [entries[k] for k in sorted(entries)]


def zip_info(entry):
    info = zipfile.ZipInfo(entry.name, date_time=ZIP_DATE)
    info.create_system = 3  # Unix, whatever the host: the attribute bytes would differ otherwise
    if entry.data is None:
        info.external_attr = ((stat.S_IFDIR | DIR_MODE) << 16) | 0x10
    else:
        info.external_attr = (stat.S_IFREG | (EXEC_MODE if entry.executable else FILE_MODE)) << 16
    info.compress_type = zipfile.ZIP_DEFLATED if entry.data else zipfile.ZIP_STORED
    return info


def write_zip(entries, path):
    """Writes the zip atomically; returns the sum of the uncompressed file sizes."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, suffix=".tmp")
    os.close(fd)
    try:
        with zipfile.ZipFile(tmp, "w") as zf:
            for e in entries:
                if e.data is None:
                    zf.writestr(zip_info(e), b"")
                else:
                    zf.writestr(zip_info(e), e.data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
        os.chmod(tmp, 0o644)  # mkstemp makes it 0600
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)
    return sum(len(e.data) for e in entries if e.data is not None)


def build(bin_specs, out_dir, allow_missing=False, metadata_path=METADATA, plugin_version=None, **kw):
    """Builds the zip and its sidecar. Returns (zip path, sidecar dict)."""
    bins = resolve_bins(list(bin_specs), allow_missing)
    text, version = inner_metadata(Path(metadata_path).read_text(encoding="utf-8"))
    if plugin_version is None:
        sys.path.insert(0, str(PLUGIN_SRC))
        try:
            import version as plugin_version_module

            plugin_version = plugin_version_module.__version__
        finally:
            sys.path.remove(str(PLUGIN_SRC))
    if version != plugin_version:
        raise PackageError(
            f"metadata.json version {version} differs from the plugin's __version__ {plugin_version}"
        )
    entries = collect(bins, text, **kw)
    zip_path = Path(out_dir) / f"freerouting-rs-{version}-pcm.zip"
    install_size = write_zip(entries, zip_path)
    data = zip_path.read_bytes()
    sidecar = {
        "file": zip_path.name,
        "version": version,
        "sha256": hashlib.sha256(data).hexdigest(),
        "download_size": len(data),
        "install_size": install_size,
    }
    side_path = zip_path.with_name(zip_path.name + ".meta.json")
    side_path.write_text(json.dumps(sidecar, indent=2) + "\n", encoding="utf-8")
    return zip_path, sidecar


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--bin", action="append", default=[], metavar="DIR|PLATFORM=DIR", help="binaries; repeatable")
    ap.add_argument("--out", required=True, help="folder for the zip and its .meta.json")
    ap.add_argument("--allow-missing", action="store_true", help="package only the platforms that were given")
    args = ap.parse_args(argv)
    try:
        zip_path, sidecar = build(args.bin, args.out, args.allow_missing)
    except PackageError as e:
        print(f"package.py: {e}", file=sys.stderr)
        return 1
    print(zip_path)
    print(
        f"sha256 {sidecar['sha256']}  download_size {sidecar['download_size']}  "
        f"install_size {sidecar['install_size']}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

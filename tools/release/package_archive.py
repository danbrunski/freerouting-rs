#!/usr/bin/env python3
"""Builds the release archive of one platform (R01.4 tar.gz, R01.5 zip, R01.6 tar.gz).

    package_archive.py --platform linux-x86_64 --bin-dir target/release --out dist [--version X.Y.Z]

Writes `freerouting-rs-<version>-<platform>.tar.gz` (`.zip` for a platform
starting with `windows`) holding one top-level directory of the same name
with: the two binaries (`freerouting`, `freerouting-gui`, `.exe` on Windows),
`kicad-plugin/` (without caches and tests), `README.md`, `LICENSE`,
`THIRD_PARTY_NOTICES.md` and `LICENSES/` (F04.1: every archive carries the
notices). The archive is reproducible: entries are sorted, mtimes are fixed
to SOURCE_DATE_EPOCH (default 0), owners are root, and the gzip header has
no name or time. Prints the archive path. Standard library only.
"""

import argparse
import gzip
import io
import os
import sys
import tarfile
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))

import check_version  # noqa: E402

ROOT_FILES = ["README.md", "LICENSE", "THIRD_PARTY_NOTICES.md"]
SKIP_DIRS = {"__pycache__", "tests", ".pytest_cache"}
SKIP_SUFFIXES = {".pyc", ".pyo"}


def archive_name(version, platform):
    ext = "zip" if platform.startswith("windows") else "tar.gz"
    return f"freerouting-rs-{version}-{platform}.{ext}"


def collect(repo, bin_dir, platform):
    """[(archive-relative path, source Path, executable)] sorted by path."""
    exe = ".exe" if platform.startswith("windows") else ""
    entries = []
    for name in ("freerouting", "freerouting-gui"):
        src = Path(bin_dir) / (name + exe)
        if not src.is_file():
            raise SystemExit(f"missing binary: {src}")
        entries.append((name + exe, src, True))
    for name in ROOT_FILES:
        src = repo / name
        if not src.is_file():
            raise SystemExit(f"missing file: {src}")
        entries.append((name, src, False))
    for top in ("LICENSES", "kicad-plugin"):
        base = repo / top
        if not base.is_dir():
            raise SystemExit(f"missing directory: {base}")
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
            for f in filenames:
                p = Path(dirpath) / f
                if p.suffix in SKIP_SUFFIXES:
                    continue
                entries.append((p.relative_to(repo).as_posix(), p, False))
    entries.sort(key=lambda e: e[0])
    return entries


def write_tar_gz(path, top, entries, mtime):
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as tar:
        for rel, src, executable in entries:
            info = tarfile.TarInfo(f"{top}/{rel}")
            data = src.read_bytes()
            info.size = len(data)
            info.mtime = mtime
            info.mode = 0o755 if executable else 0o644
            info.uname = info.gname = "root"
            info.uid = info.gid = 0
            tar.addfile(info, io.BytesIO(data))
    with open(path, "wb") as out:
        with gzip.GzipFile(filename="", mode="wb", fileobj=out, mtime=0, compresslevel=9) as gz:
            gz.write(raw.getvalue())


def write_zip(path, top, entries, mtime):
    import time

    stamp = time.gmtime(max(mtime, 315532800))[:6]  # zip cannot go before 1980
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for rel, src, executable in entries:
            info = zipfile.ZipInfo(f"{top}/{rel}", date_time=stamp)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (0o755 if executable else 0o644) << 16
            z.writestr(info, src.read_bytes())


def build(repo, bin_dir, out_dir, platform, version=None):
    repo = Path(repo)
    if version is None:
        version = check_version.workspace_version((repo / "Cargo.toml").read_text(encoding="utf-8"))
    name = archive_name(version, platform)
    top = name.removesuffix(".tar.gz").removesuffix(".zip")
    entries = collect(repo, bin_dir, platform)
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    path = out_dir / name
    mtime = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    (write_zip if name.endswith(".zip") else write_tar_gz)(path, top, entries, mtime)
    return path


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--platform", required=True, help="linux-x86_64, windows-x86_64, macos-universal")
    ap.add_argument("--bin-dir", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--version", help="default: the workspace version")
    ap.add_argument("--repo", default=str(REPO))
    args = ap.parse_args(argv)
    print(build(args.repo, args.bin_dir, args.out, args.platform, args.version))
    return 0


if __name__ == "__main__":
    sys.exit(main())

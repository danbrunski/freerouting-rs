#!/usr/bin/env python3
"""R01.1 version gate: every place that carries the release version must agree.

    check_version.py [--tag vX.Y.Z] [--root DIR]

Reads the workspace version from `Cargo.toml`, the `version` of
`Packager.toml` (the MSI, DMG and AppImage carry it), `kicad-plugin/freerouting_rs/
version.py` (K01.3), the single version entry of `kicad-plugin/metadata.json`
and the version in its `download_url` (K03.1), and the tag when given. It
prints each, and exits 1 when two differ or when a version is not
`MAJOR.MINOR.PATCH` (KiCad's PCM accepts at most three numeric parts; a
release with four is rejected only after upload). Files that do not exist
yet are reported as "not yet present" and skipped.

Standard library only; the CI release job runs it before anything is built.
"""

import argparse
import json
import re
import sys
from pathlib import Path

# KiCad PCM: up to three dot-separated numbers, optionally a pre-release suffix is
# not allowed in "version", so the gate takes the plain form.
PCM_VERSION = re.compile(r"^\d+\.\d+\.\d+$")
CARGO_REL = "Cargo.toml"
VERSION_PY_REL = "kicad-plugin/freerouting_rs/version.py"
METADATA_REL = "kicad-plugin/metadata.json"
PACKAGER_REL = "Packager.toml"


def workspace_version(text):
    """The `version = "..."` of the `[workspace.package]` table."""
    in_table = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            in_table = stripped == "[workspace.package]"
            continue
        if in_table:
            m = re.match(r'version\s*=\s*"([^"]+)"', stripped)
            if m:
                return m.group(1)
    return None


def plugin_version(text):
    m = re.search(r'^__version__\s*=\s*"([^"]+)"', text, re.M)
    return m.group(1) if m else None


def metadata_versions(text):
    """(version, download_url version) of the single version entry.

    More than one entry is an error (the PCM metadata of one release lists
    exactly one version). Returns (list of versions, list of url versions).
    """
    doc = json.loads(text)
    entries = doc.get("versions", [])
    versions = [str(e.get("version", "")) for e in entries]
    urls = []
    for e in entries:
        url = e.get("download_url", "")
        # .../download/v0.1.0/<name>-0.1.0.zip or .../v0.1.0/...: the tag segment.
        m = re.search(r"/download/v?(\d[^/]*)/", url) or re.search(r"/v?(\d+\.\d+\.\d+[^/]*)/", url)
        urls.append(m.group(1) if m else None)
    return versions, urls


def collect(root):
    """[(label, file, value or None, note)] for every source of a version.

    `value` None with a note means the file is absent or has no version;
    a missing file is skipped, a present file without a version is an error.
    """
    root = Path(root)
    found = []
    errors = []

    def read(rel):
        p = root / rel
        return p.read_text(encoding="utf-8") if p.is_file() else None

    cargo = read(CARGO_REL)
    if cargo is None:
        errors.append(f"{CARGO_REL}: missing")
    else:
        v = workspace_version(cargo)
        if v is None:
            errors.append(f"{CARGO_REL}: no version in [workspace.package]")
        else:
            found.append(("workspace", CARGO_REL, v))

    packager = read(PACKAGER_REL)
    if packager is None:
        found.append(("packager", PACKAGER_REL, None))
    else:
        m = re.search(r'^version\s*=\s*"([^"]+)"', packager, re.M)  # top-level key, before any table
        if m is None:
            errors.append(f"{PACKAGER_REL}: no version")
        else:
            found.append(("packager", PACKAGER_REL, m.group(1)))

    py = read(VERSION_PY_REL)
    if py is None:
        found.append(("plugin", VERSION_PY_REL, None))
    else:
        v = plugin_version(py)
        if v is None:
            errors.append(f"{VERSION_PY_REL}: no __version__")
        else:
            found.append(("plugin", VERSION_PY_REL, v))

    meta = read(METADATA_REL)
    if meta is None:
        found.append(("metadata", METADATA_REL, None))
        found.append(("download_url", METADATA_REL, None))
    else:
        try:
            versions, urls = metadata_versions(meta)
        except (ValueError, AttributeError) as e:
            errors.append(f"{METADATA_REL}: not valid metadata JSON ({e})")
        else:
            if len(versions) != 1:
                errors.append(
                    f"{METADATA_REL}: expected exactly one version entry, found {len(versions)}"
                )
            else:
                found.append(("metadata", METADATA_REL, versions[0] or None))
                if urls[0] is None:
                    errors.append(f"{METADATA_REL}: download_url has no version segment")
                else:
                    found.append(("download_url", METADATA_REL, urls[0]))
    return found, errors


def check(root, tag=None):
    """Returns (lines, ok): the report lines and whether the gate passes."""
    found, errors = collect(root)
    if tag is not None:
        m = re.fullmatch(r"v(.+)", tag)
        if not m:
            errors.append(f"tag {tag!r} does not start with 'v'")
        else:
            found.append(("tag", "git tag", m.group(1)))
    lines = []
    present = []
    for label, where, value in found:
        if value is None:
            lines.append(f"{label:13} not yet present ({where})")
        else:
            lines.append(f"{label:13} {value}  ({where})")
            present.append((label, where, value))
    for label, where, value in present:
        if not PCM_VERSION.match(value):
            errors.append(
                f"{where}: {label} version {value!r} is not MAJOR.MINOR.PATCH "
                f"(the KiCad PCM rejects it)"
            )
    distinct = {value for _, _, value in present}
    if len(distinct) > 1:
        first = present[0]
        for label, where, value in present[1:]:
            if value != first[2]:
                errors.append(
                    f"{where}: {label} version {value} differs from {first[0]} version "
                    f"{first[2]} ({first[1]})"
                )
    lines.extend(f"ERROR {e}" for e in errors)
    return lines, not errors


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--tag", help="release tag, vX.Y.Z")
    ap.add_argument("--root", default=str(Path(__file__).resolve().parents[2]), help="repository root")
    args = ap.parse_args(argv)
    lines, ok = check(args.root, args.tag)
    print("\n".join(lines))
    print("version gate: ok" if ok else "version gate: FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())

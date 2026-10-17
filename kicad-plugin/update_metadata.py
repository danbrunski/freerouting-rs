#!/usr/bin/env python3
"""Patches a built package's checksum and sizes into the repository's metadata.json (K03.4).

    update_metadata.py METADATA.json ZIP.meta.json [--out FILE]

`package.py` writes the sidecar `<zip>.meta.json` (`version`, `file`, `sha256`,
`download_size`, `install_size`). This sets `download_sha256`, `download_size` and
`install_size` on the `versions[]` entry whose `version` equals the sidecar's and rewrites the
file in place (or to `--out`). It fails, naming both versions, when no entry matches, and when
the entry's `download_url` does not end in the sidecar's file name (the metadata would then
point at a file whose checksum it does not carry). The release job attaches the patched file
next to the zip. Standard library only.
"""

import argparse
import json
import re
import sys
from pathlib import Path

SHA256 = re.compile(r"^[a-f0-9]{64}$")


class PatchError(Exception):
    pass


def load_sidecar(path):
    try:
        side = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise PatchError(f"{path}: cannot read the sidecar ({e})") from e
    for key in ("version", "file", "sha256", "download_size", "install_size"):
        if key not in side:
            raise PatchError(f"{path}: sidecar has no {key!r}")
    if not SHA256.match(str(side["sha256"])):
        raise PatchError(f"{path}: sha256 is not 64 lower-case hex digits")
    return side


def patch(metadata_text, sidecar):
    """Returns the patched metadata text; raises PatchError."""
    doc = json.loads(metadata_text)
    versions = doc.get("versions", [])
    matching = [v for v in versions if v.get("version") == sidecar["version"]]
    if not matching:
        have = ", ".join(str(v.get("version")) for v in versions) or "none"
        raise PatchError(
            f"the sidecar is for version {sidecar['version']} ({sidecar['file']}), "
            f"but metadata.json has version {have}"
        )
    entry = matching[0]
    url = entry.get("download_url", "")
    if url.rsplit("/", 1)[-1] != sidecar["file"]:
        raise PatchError(
            f"download_url {url!r} does not end in the packaged file name {sidecar['file']!r}"
        )
    entry["download_sha256"] = sidecar["sha256"]
    entry["download_size"] = sidecar["download_size"]
    entry["install_size"] = sidecar["install_size"]
    return json.dumps(doc, indent=2, ensure_ascii=False) + "\n"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("metadata", help="the repository's metadata.json (patched in place)")
    ap.add_argument("sidecar", help="<zip>.meta.json written by package.py")
    ap.add_argument("--out", help="write here instead of in place")
    args = ap.parse_args(argv)
    try:
        text = patch(Path(args.metadata).read_text(encoding="utf-8"), load_sidecar(args.sidecar))
    except (PatchError, OSError, ValueError) as e:
        print(f"update_metadata.py: {e}", file=sys.stderr)
        return 1
    Path(args.out or args.metadata).write_text(text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())

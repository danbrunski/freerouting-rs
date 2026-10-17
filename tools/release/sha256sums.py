#!/usr/bin/env python3
"""Writes SHA256SUMS for the release assets (R01.7).

    sha256sums.py DIR [--out DIR/SHA256SUMS]

One line per regular file of DIR (not SHA256SUMS itself), `<hex>  <name>`,
sorted by name, so `sha256sum -c SHA256SUMS` run in DIR checks them (the
two spaces mark text mode, which is what GNU sha256sum and macOS
`shasum -a 256 -c` both accept).
"""

import argparse
import hashlib
import sys
from pathlib import Path


def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def sums(directory):
    directory = Path(directory)
    files = sorted(p for p in directory.iterdir() if p.is_file() and p.name != "SHA256SUMS")
    return "".join(f"{digest(p)}  {p.name}\n" for p in files)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("dir")
    ap.add_argument("--out")
    args = ap.parse_args(argv)
    out = Path(args.out) if args.out else Path(args.dir) / "SHA256SUMS"
    text = sums(args.dir)
    if not text:
        print("no files to checksum", file=sys.stderr)
        return 1
    out.write_text(text, encoding="utf-8", newline="\n")
    print(text, end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Renders icon.svg to the PNG sizes, icon.ico and icon.icns (R01.3).

    python3 render.py            # rewrites the files next to this script

Needs ImageMagick's `convert` (the SVG is drawn once at 1024 px) and Pillow
(downscaling with Lanczos, the ICO). The ICNS container is written here: it
is a list of (type, PNG) chunks, and macOS reads PNG payloads for every type
used below. The output is deterministic, so a re-run changes no file.
"""

import io
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
SIZES = [16, 32, 48, 64, 128, 256, 512, 1024]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
# ICNS types holding PNG data: size -> OSType.
ICNS_TYPES = [(16, b"icp4"), (32, b"icp5"), (64, b"icp6"), (128, b"ic07"), (256, b"ic08"),
              (512, b"ic09"), (1024, b"ic10")]


def render_master():
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "master.png"
        subprocess.run(
            ["convert", "-background", "none", "-density", "96", str(HERE / "icon.svg"),
             "-resize", "1024x1024", f"PNG32:{out}"],
            check=True,
        )
        return Image.open(out).convert("RGBA")


def png_bytes(image):
    buf = io.BytesIO()
    image.save(buf, "PNG", optimize=True)
    return buf.getvalue()


def main():
    master = render_master()
    assert master.size == (1024, 1024), master.size
    scaled = {s: (master if s == 1024 else master.resize((s, s), Image.LANCZOS))
              for s in sorted(set(SIZES) | set(ICO_SIZES))}
    for s in SIZES:
        (HERE / f"icon-{s}.png").write_bytes(png_bytes(scaled[s]))
    scaled[256].save(HERE / "icon.ico", format="ICO",
                     sizes=[(s, s) for s in ICO_SIZES], append_images=[])
    chunks = b"".join(
        kind + struct.pack(">I", 8 + len(data)) + data
        for kind, data in ((k, png_bytes(scaled[s])) for s, k in ICNS_TYPES)
    )
    (HERE / "icon.icns").write_bytes(b"icns" + struct.pack(">I", 8 + len(chunks)) + chunks)


if __name__ == "__main__":
    sys.exit(main())
